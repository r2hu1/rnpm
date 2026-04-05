use crate::registry::{RegistryClient, VersionMetadata};
use anyhow::Result;
use futures::FutureExt;
use futures::future::BoxFuture;
use indicatif::ProgressBar;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

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
            semaphore: Arc::new(Semaphore::new(20)), // Limit concurrent requests
            progress_bar: None,
        }
    }

    pub fn with_progress(mut self, pb: ProgressBar) -> Self {
        self.progress_bar = Some(Arc::new(pb));
        self
    }

    /// Resolve a single package and all its dependencies using BFS with progress tracking
    pub fn resolve_recursive(&self, name: String, range: String) -> BoxFuture<'static, Result<()>> {
        let registry = Arc::clone(&self.registry);
        let resolved = Arc::clone(&self.resolved);
        let semaphore = Arc::clone(&self.semaphore);
        let progress_bar = self.progress_bar.clone();

        async move {
            // Use a queue for BFS: (package_name, version_range)
            let mut queue: VecDeque<(String, String)> = VecDeque::new();
            queue.push_back((name.clone(), range.clone()));

            // Track which packages we've already queued to avoid duplicates
            let mut queued: HashMap<String, String> = HashMap::new();
            queued.insert(name.clone(), range.clone());
            let mut resolved_count = 0;

            while let Some((pkg_name, pkg_range)) = queue.pop_front() {
                // Check if already resolved
                {
                    let lock = resolved.lock().unwrap();
                    if lock.contains_key(&pkg_name) {
                        continue;
                    }
                }

                // Update progress
                if let Some(ref pb) = progress_bar {
                    pb.set_message(format!("Resolving {}...", pkg_name));
                }

                // Fetch metadata with semaphore limit
                let _permit = semaphore.acquire().await.unwrap();
                let metadata_res = registry.fetch_package_metadata(&pkg_name).await;

                let version_meta = match metadata_res {
                    Ok(metadata) => {
                        match registry.resolve_version(&metadata, &pkg_range) {
                            Ok(meta) => {
                                // Mark as resolved
                                {
                                    let mut lock = resolved.lock().unwrap();
                                    lock.insert(pkg_name.clone(), meta.clone());
                                    resolved_count += 1;
                                }

                                // Update progress bar with count
                                if let Some(ref pb) = progress_bar {
                                    pb.set_message(format!(
                                        "Resolved {} packages... (processing {})",
                                        resolved_count, pkg_name
                                    ));
                                }

                                Ok(meta)
                            }
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                }?;

                // Queue dependencies
                if let Some(deps) = version_meta.dependencies {
                    for (dep_name, dep_range) in deps {
                        // Skip if already resolved or already queued
                        {
                            let lock = resolved.lock().unwrap();
                            if lock.contains_key(&dep_name) {
                                continue;
                            }
                        }

                        if !queued.contains_key(&dep_name) {
                            queued.insert(dep_name.clone(), dep_range.clone());
                            queue.push_back((dep_name, dep_range));
                        }
                    }
                }
            }

            Ok(())
        }
        .boxed()
    }

    /// Resolve multiple packages and all their dependencies using unified BFS with progress tracking
    pub fn resolve_multiple(
        &self,
        packages: Vec<(String, String)>,
    ) -> BoxFuture<'static, Result<usize>> {
        let registry = Arc::clone(&self.registry);
        let resolved = Arc::clone(&self.resolved);
        let semaphore = Arc::clone(&self.semaphore);
        let progress_bar = self.progress_bar.clone();

        async move {
            // Use a queue for BFS: (package_name, version_range)
            let mut queue: VecDeque<(String, String)> = VecDeque::new();

            // Add all root packages to the queue
            for (name, range) in packages {
                queue.push_back((name, range));
            }

            // Track which packages we've already queued to avoid duplicates
            let mut queued: HashMap<String, String> = HashMap::new();
            for (name, range) in queue.iter() {
                queued.insert(name.clone(), range.clone());
            }

            let mut resolved_count = 0;

            while let Some((pkg_name, pkg_range)) = queue.pop_front() {
                // Check if already resolved
                {
                    let lock = resolved.lock().unwrap();
                    if lock.contains_key(&pkg_name) {
                        continue;
                    }
                }

                // Update progress
                if let Some(ref pb) = progress_bar {
                    pb.set_message(format!(
                        "Resolving {}... ({} resolved)",
                        pkg_name, resolved_count
                    ));
                    pb.enable_steady_tick(std::time::Duration::from_millis(120));
                }

                // Fetch metadata with semaphore limit
                let _permit = semaphore.acquire().await.unwrap();
                let metadata_res = registry.fetch_package_metadata(&pkg_name).await;

                let version_meta = match metadata_res {
                    Ok(metadata) => {
                        match registry.resolve_version(&metadata, &pkg_range) {
                            Ok(meta) => {
                                // Mark as resolved
                                {
                                    let mut lock = resolved.lock().unwrap();
                                    lock.insert(pkg_name.clone(), meta.clone());
                                    resolved_count += 1;
                                }

                                // Update progress bar with count
                                if let Some(ref pb) = progress_bar {
                                    pb.set_message(format!(
                                        "Resolved {} packages... (processing {})",
                                        resolved_count, pkg_name
                                    ));
                                }

                                Ok(meta)
                            }
                            Err(e) => Err(e),
                        }
                    }
                    Err(e) => Err(e),
                }?;

                // Queue dependencies
                if let Some(deps) = version_meta.dependencies {
                    for (dep_name, dep_range) in deps {
                        // Skip if already resolved or already queued
                        {
                            let lock = resolved.lock().unwrap();
                            if lock.contains_key(&dep_name) {
                                continue;
                            }
                        }

                        if !queued.contains_key(&dep_name) {
                            queued.insert(dep_name.clone(), dep_range.clone());
                            queue.push_back((dep_name, dep_range));
                        }
                    }
                }
            }

            Ok(resolved_count)
        }
        .boxed()
    }
}
