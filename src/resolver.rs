use crate::registry::{RegistryClient, VersionMetadata};
use anyhow::Result;
use futures::FutureExt;
use futures::future::BoxFuture;
use indicatif::ProgressBar;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

pub struct Resolver {
    registry: Arc<RegistryClient>,
    pub resolved: Arc<Mutex<HashMap<String, VersionMetadata>>>,
    semaphore: Arc<Semaphore>,
    pub progress_bar: Option<Arc<ProgressBar>>,
}

impl Resolver {
    pub fn new(registry: Arc<RegistryClient>) -> Self {
        Self {
            registry,
            resolved: Arc::new(Mutex::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(30)), // 30 concurrent requests
            progress_bar: None,
        }
    }

    pub fn with_progress(mut self, pb: ProgressBar) -> Self {
        self.progress_bar = Some(Arc::new(pb));
        self
    }

    /// Resolve a single package recursively (kept for backward compatibility)
    pub fn resolve_recursive(&self, name: String, range: String) -> BoxFuture<'static, Result<()>> {
        let packages = vec![(name, range)];
        let resolver = self.clone_resolver();

        async move {
            resolver.resolve_multiple_internal(packages).await?;
            Ok(())
        }
        .boxed()
    }

    /// Resolve multiple packages using highly concurrent parallel workers
    pub fn resolve_multiple(
        &self,
        packages: Vec<(String, String)>,
    ) -> BoxFuture<'static, Result<usize>> {
        let resolver = self.clone_resolver();

        async move { resolver.resolve_multiple_internal(packages).await }.boxed()
    }

    fn clone_resolver(&self) -> Self {
        Self {
            registry: Arc::clone(&self.registry),
            resolved: Arc::clone(&self.resolved),
            semaphore: Arc::clone(&self.semaphore),
            progress_bar: self.progress_bar.clone(),
        }
    }

    /// Internal implementation with proper task management
    async fn resolve_multiple_internal(&self, packages: Vec<(String, String)>) -> Result<usize> {
        // Shared state
        let pending: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let processing: Arc<Mutex<HashMap<String, Arc<tokio::sync::Notify>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Initialize with root packages
        {
            let mut p = pending.lock().unwrap();
            *p = packages;
        }

        let mut handles: Vec<JoinHandle<Result<()>>> = vec![];

        loop {
            // Get next pending package
            let next_package = {
                let mut p = pending.lock().unwrap();
                if p.is_empty() {
                    None
                } else {
                    Some(p.remove(0))
                }
            };

            match next_package {
                Some((pkg_name, pkg_range)) => {
                    // Check if already resolved
                    {
                        let r = self.resolved.lock().unwrap();
                        if r.contains_key(&pkg_name) {
                            continue;
                        }
                    }

                    // Update progress
                    if let Some(ref pb) = self.progress_bar {
                        let count = self.resolved.lock().unwrap().len();
                        let pend_count = pending.lock().unwrap().len();
                        pb.set_message(format!(
                            "Resolving {}... ({} resolved, {} pending)",
                            pkg_name, count, pend_count
                        ));
                    }

                    // Clone for task
                    let registry = Arc::clone(&self.registry);
                    let resolved = Arc::clone(&self.resolved);
                    let pending_clone = Arc::clone(&pending);
                    let semaphore = Arc::clone(&self.semaphore);
                    let pb_clone = self.progress_bar.clone();
                    let pkg_name_clone = pkg_name.clone();

                    // Spawn concurrent task
                    let handle = tokio::spawn(async move {
                        // Fetch with semaphore control
                        let _permit = semaphore.acquire().await.unwrap();

                        match registry.fetch_package_metadata(&pkg_name_clone).await {
                            Ok(metadata) => {
                                match registry.resolve_version(&metadata, &pkg_range) {
                                    Ok(meta) => {
                                        // Store resolved package
                                        {
                                            let mut r = resolved.lock().unwrap();
                                            r.insert(pkg_name_clone.clone(), meta.clone());
                                        }

                                        // Queue dependencies
                                        if let Some(deps) = meta.dependencies {
                                            let mut p = pending_clone.lock().unwrap();
                                            let r = resolved.lock().unwrap();
                                            for (dep_name, dep_range) in deps {
                                                // Only add if not resolved and not already pending
                                                if !r.contains_key(&dep_name)
                                                    && !p.iter().any(|(n, _)| n == &dep_name)
                                                {
                                                    p.push((dep_name, dep_range));
                                                }
                                            }
                                            drop(p);
                                            drop(r);
                                        }

                                        if let Some(ref pb) = pb_clone {
                                            let count = resolved.lock().unwrap().len();
                                            let pend_count = pending_clone.lock().unwrap().len();
                                            pb.set_message(format!(
                                                "✓ {} ({} total, {} pending)",
                                                pkg_name_clone, count, pend_count
                                            ));
                                        }

                                        Ok(())
                                    }
                                    Err(e) => Err(anyhow::anyhow!(
                                        "Failed to resolve version for {}: {}",
                                        pkg_name_clone,
                                        e
                                    )),
                                }
                            }
                            Err(e) => Err(anyhow::anyhow!(
                                "Failed to fetch metadata for {}: {}",
                                pkg_name_clone,
                                e
                            )),
                        }
                    });

                    handles.push(handle);
                }
                None => {
                    // No more pending packages
                    // Wait for all running tasks to complete
                    break;
                }
            }
        }

        // Wait for all spawned tasks to finish
        for handle in handles {
            let _ = handle.await;
        }

        // Final wait to ensure all dependencies are queued
        // Keep checking until no new work appears
        loop {
            let pending_count = pending.lock().unwrap().len();
            if pending_count == 0 {
                break;
            }

            // Process remaining pending
            let mut new_handles = vec![];

            loop {
                let next_package = {
                    let mut p = pending.lock().unwrap();
                    if p.is_empty() {
                        None
                    } else {
                        Some(p.remove(0))
                    }
                };

                match next_package {
                    Some((pkg_name, pkg_range)) => {
                        // Check if already resolved
                        {
                            let r = self.resolved.lock().unwrap();
                            if r.contains_key(&pkg_name) {
                                continue;
                            }
                        }

                        let registry = Arc::clone(&self.registry);
                        let resolved = Arc::clone(&self.resolved);
                        let pending_clone = Arc::clone(&pending);
                        let semaphore = Arc::clone(&self.semaphore);
                        let pb_clone = self.progress_bar.clone();

                        let handle = tokio::spawn(async move {
                            let _permit = semaphore.acquire().await.unwrap();

                            match registry.fetch_package_metadata(&pkg_name).await {
                                Ok(metadata) => {
                                    match registry.resolve_version(&metadata, &pkg_range) {
                                        Ok(meta) => {
                                            {
                                                let mut r = resolved.lock().unwrap();
                                                r.insert(pkg_name.clone(), meta.clone());
                                            }

                                            if let Some(deps) = meta.dependencies {
                                                let mut p = pending_clone.lock().unwrap();
                                                let r = resolved.lock().unwrap();
                                                for (dep_name, dep_range) in deps {
                                                    if !r.contains_key(&dep_name)
                                                        && !p.iter().any(|(n, _)| n == &dep_name)
                                                    {
                                                        p.push((dep_name, dep_range));
                                                    }
                                                }
                                                drop(p);
                                                drop(r);
                                            }

                                            if let Some(ref pb) = pb_clone {
                                                let count = resolved.lock().unwrap().len();
                                                pb.set_message(format!(
                                                    "✓ {} ({} total)",
                                                    pkg_name, count
                                                ));
                                            }

                                            Ok(())
                                        }
                                        Err(e) => Err(anyhow::anyhow!("{}", e)),
                                    }
                                }
                                Err(e) => Err(anyhow::anyhow!("{}", e)),
                            }
                        });

                        new_handles.push(handle);
                    }
                    None => break,
                }
            }

            for handle in new_handles {
                let _ = handle.await;
            }
        }

        let final_count = self.resolved.lock().unwrap().len();

        if let Some(ref pb) = self.progress_bar {
            pb.set_message(format!("Resolved {} packages", final_count));
        }

        Ok(final_count)
    }
}
