mod downloader;
mod lockfile;
mod registry;
mod resolver;

use crate::lockfile::{LockedPackage, Lockfile};
use crate::registry::VersionMetadata;
use anyhow::Result;
use clap::{Parser, Subcommand};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::{fs, process};

#[derive(Debug, Deserialize, Serialize)]
pub struct PackageJson {
    pub name: String,
    pub version: String,
    pub dependencies: Option<HashMap<String, String>>,
    #[serde(rename = "devDependencies")]
    pub dev_dependencies: Option<HashMap<String, String>>,
    pub scripts: Option<HashMap<String, String>>,
}

#[derive(Parser)]
#[command(name = "rnpm")]
#[command(about = "A fast Rust-based Node Package Manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new package to the dependencies
    Add {
        /// Name of the package to add
        package_name: String,
        /// Add to devDependencies
        #[arg(short = 'D', long)]
        dev: bool,
    },
    /// Install all dependencies from package.json
    Install,
    /// Update all dependencies from package.json
    Update,
    /// Remove a package from the dependencies
    Remove {
        /// Name of the package to remove
        package_name: String,
    },
    /// Run a script defined in package.json
    Run {
        /// Name of the script to run
        script_name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let registry = Arc::new(registry::RegistryClient::new());
    let downloader = Arc::new(downloader::Downloader::new());

    match &cli.command {
        Commands::Add { package_name, dev } => {
            let pb = ProgressBar::new_spinner();
            pb.set_message(format!("Resolving {}...", package_name));
            pb.enable_steady_tick(std::time::Duration::from_millis(120));

            let resolver = resolver::Resolver::new(Arc::clone(&registry));
            resolver
                .resolve_recursive(package_name.clone(), "latest".to_string())
                .await?;
            pb.finish_with_message("Resolved dependencies.");

            let resolved_lock = resolver.resolved.lock().unwrap().clone();
            download_resolved_from_meta(&resolved_lock, &downloader).await?;

            let mut lockfile = Lockfile::load().unwrap_or(Lockfile::new());
            let new_lock = Lockfile::from_resolved(&resolved_lock);
            lockfile.packages.extend(new_lock.packages);
            lockfile.save()?;

            update_package_json_add(package_name, "latest", *dev)?;
        }
        Commands::Install => {
            if Path::new("rnpm.lock").exists() {
                println!("Using rnpm.lock...");
                let lockfile = Lockfile::load()?;
                download_resolved_from_lock(&lockfile.packages, &downloader).await?;
            } else {
                let pb = ProgressBar::new_spinner();
                pb.set_message("Resolving dependencies from package.json...");
                pb.enable_steady_tick(std::time::Duration::from_millis(120));

                let content = fs::read_to_string("package.json")
                    .map_err(|_| anyhow::anyhow!("package.json not found"))?;
                let package_json: PackageJson = serde_json::from_str(&content)?;

                let resolver =
                    resolver::Resolver::new(Arc::clone(&registry)).with_progress(pb.clone());

                // Collect all dependencies into a single list for unified BFS resolution
                let mut packages_to_resolve = vec![];

                if let Some(deps) = package_json.dependencies {
                    for (name, range) in deps {
                        packages_to_resolve.push((name, range));
                    }
                }

                if let Some(dev_deps) = package_json.dev_dependencies {
                    for (name, range) in dev_deps {
                        packages_to_resolve.push((name, range));
                    }
                }

                if !packages_to_resolve.is_empty() {
                    let resolved_count = resolver.resolve_multiple(packages_to_resolve).await?;
                    let resolved = resolver.resolved.lock().unwrap().clone();
                    pb.finish_with_message(format!("Resolved {} dependencies.", resolved.len()));
                    download_resolved_from_meta(&resolved, &downloader).await?;
                    Lockfile::from_resolved(&resolved).save()?;
                } else {
                    pb.finish_with_message("No dependencies to install.");
                }
            }
        }
        Commands::Update => {
            let pb = ProgressBar::new_spinner();
            pb.set_message("Updating dependencies...");
            pb.enable_steady_tick(std::time::Duration::from_millis(120));

            let content = fs::read_to_string("package.json")
                .map_err(|_| anyhow::anyhow!("package.json not found"))?;
            let package_json: PackageJson = serde_json::from_str(&content)?;

            let resolver = resolver::Resolver::new(Arc::clone(&registry)).with_progress(pb.clone());
            let mut futures = vec![];

            if let Some(deps) = package_json.dependencies {
                for (name, range) in deps {
                    futures.push(resolver.resolve_recursive(name, range));
                }
            }

            if let Some(dev_deps) = package_json.dev_dependencies {
                for (name, range) in dev_deps {
                    futures.push(resolver.resolve_recursive(name, range));
                }
            }

            if !futures.is_empty() {
                futures::future::join_all(futures)
                    .await
                    .into_iter()
                    .collect::<Result<Vec<()>>>()?;
                let resolved = resolver.resolved.lock().unwrap().clone();
                pb.finish_with_message(format!("Updated {} dependencies.", resolved.len()));
                download_resolved_from_meta(&resolved, &downloader).await?;
                Lockfile::from_resolved(&resolved).save()?;
            } else {
                pb.finish_with_message("No dependencies to update.");
            }
        }
        Commands::Remove { package_name } => {
            let pb = ProgressBar::new_spinner();
            pb.set_message(format!("Removing {}...", package_name));

            let dest = format!("node_modules/{}", package_name);
            if Path::new(&dest).exists() {
                fs::remove_dir_all(dest)?;
            }

            update_package_json_remove(package_name)?;

            let mut lockfile = Lockfile::load().unwrap_or(Lockfile::new());
            lockfile.packages.remove(package_name);
            lockfile.save()?;

            pb.finish_with_message(format!("Removed {}.", package_name));
        }
        Commands::Run { script_name } => {
            let content = fs::read_to_string("package.json")
                .map_err(|_| anyhow::anyhow!("package.json not found"))?;
            let package_json: PackageJson = serde_json::from_str(&content)?;
            if let Some(scripts) = package_json.scripts {
                if let Some(cmd) = scripts.get(script_name) {
                    println!("> rnpm run {}", script_name);
                    println!("> {}", cmd);
                    run_command(cmd)?;
                } else {
                    println!("Script not found: {}", script_name);
                }
            } else {
                println!("No scripts found in package.json");
            }
        }
    }

    Ok(())
}

async fn download_resolved_from_meta(
    resolved: &HashMap<String, VersionMetadata>,
    downloader: &downloader::Downloader,
) -> Result<()> {
    let to_download: Vec<(&String, &VersionMetadata)> = resolved
        .iter()
        .filter(|(name, _)| !Path::new(&format!("node_modules/{}", name)).exists())
        .collect();

    if to_download.is_empty() {
        println!("All dependencies already installed.");
        return Ok(());
    }

    let multi = MultiProgress::new();
    let main_pb = multi.add(ProgressBar::new(to_download.len() as u64));
    main_pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")?
        .progress_chars("#>-"));
    main_pb.set_message("Downloading packages");

    let mut futures = vec![];

    for (name, version_meta) in to_download {
        let dest = format!("node_modules/{}", name);
        let url = version_meta.dist.tarball.clone();
        let d = downloader.clone();
        let pb = main_pb.clone();
        let pkg_name = name.clone();

        futures.push(async move {
            d.download_and_extract(&url, Path::new(&dest)).await?;
            pb.inc(1);
            pb.set_message(format!("Installed {}", pkg_name));
            Ok::<(), anyhow::Error>(())
        });
    }

    futures::future::join_all(futures)
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>()?;
    main_pb.finish_with_message("All packages installed successfully.");
    Ok(())
}

async fn download_resolved_from_lock(
    packages: &HashMap<String, LockedPackage>,
    downloader: &downloader::Downloader,
) -> Result<()> {
    let to_download: Vec<(&String, &LockedPackage)> = packages
        .iter()
        .filter(|(name, _)| !Path::new(&format!("node_modules/{}", name)).exists())
        .collect();

    if to_download.is_empty() {
        println!("All dependencies already installed.");
        return Ok(());
    }

    let multi = MultiProgress::new();
    let main_pb = multi.add(ProgressBar::new(to_download.len() as u64));
    main_pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")?
        .progress_chars("#>-"));
    main_pb.set_message("Downloading packages from lockfile");

    let mut futures = vec![];

    for (name, locked) in to_download {
        let dest = format!("node_modules/{}", name);
        let url = locked.tarball.clone();
        let d = downloader.clone();
        let pb = main_pb.clone();
        let pkg_name = name.clone();

        futures.push(async move {
            d.download_and_extract(&url, Path::new(&dest)).await?;
            pb.inc(1);
            pb.set_message(format!("Installed {}", pkg_name));
            Ok::<(), anyhow::Error>(())
        });
    }

    futures::future::join_all(futures)
        .await
        .into_iter()
        .collect::<Result<Vec<()>>>()?;
    main_pb.finish_with_message("All packages installed successfully.");
    Ok(())
}

fn run_command(cmd: &str) -> Result<()> {
    let mut parts = cmd.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty command"))?;
    let args: Vec<&str> = parts.collect();

    let status = process::Command::new(program).args(args).status()?;

    if !status.success() {
        process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn update_package_json_add(name: &str, range: &str, dev: bool) -> Result<()> {
    let content = fs::read_to_string("package.json")?;
    let mut package_json: serde_json::Value = serde_json::from_str(&content)?;

    let key = if dev {
        "devDependencies"
    } else {
        "dependencies"
    };

    if let Some(deps) = package_json.get_mut(key) {
        deps.as_object_mut().unwrap().insert(
            name.to_string(),
            serde_json::Value::String(range.to_string()),
        );
    } else {
        let mut deps = serde_json::Map::new();
        deps.insert(
            name.to_string(),
            serde_json::Value::String(range.to_string()),
        );
        package_json
            .as_object_mut()
            .unwrap()
            .insert(key.to_string(), serde_json::Value::Object(deps));
    }

    let content = serde_json::to_string_pretty(&package_json)?;
    fs::write("package.json", content)?;
    Ok(())
}

fn update_package_json_remove(name: &str) -> Result<()> {
    let content = fs::read_to_string("package.json")?;
    let mut package_json: serde_json::Value = serde_json::from_str(&content)?;

    if let Some(deps) = package_json.get_mut("dependencies") {
        deps.as_object_mut().unwrap().remove(name);
    }

    if let Some(dev_deps) = package_json.get_mut("devDependencies") {
        dev_deps.as_object_mut().unwrap().remove(name);
    }

    let content = serde_json::to_string_pretty(&package_json)?;
    fs::write("package.json", content)?;
    Ok(())
}
