mod config;
mod downloader;
mod lockfile;
mod lockfile_import;
mod registry;
mod resolver;

use crate::config::RnpmConfig;
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
    /// Import lock file from npm/yarn/pnpm
    Import {
        /// Path to lock file (package-lock.json, yarn.lock, pnpm-lock.yaml)
        lockfile_path: Option<String>,
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
            // Load config to check for external lock file preference
            let config = RnpmConfig::load()?;
            let mut handled = false;

            // Check if configured to use external lock file
            if let Some(lockfile_path) = config.get_lockfile_path() {
                if Path::new(&lockfile_path).exists() {
                    println!(
                        "Using {} (configured in rnpm.config.json)...",
                        lockfile_path
                    );

                    let imported = if lockfile_path == "package-lock.json" {
                        lockfile_import::import_npm_lockfile(&lockfile_path)?
                    } else if lockfile_path == "yarn.lock" {
                        lockfile_import::import_yarn_lockfile(&lockfile_path)?
                    } else if lockfile_path == "pnpm-lock.yaml" {
                        lockfile_import::import_pnpm_lockfile(&lockfile_path)?
                    } else if lockfile_path == "bun.lock" || lockfile_path == "bun.lockb" {
                        lockfile_import::import_bun_lockfile(&lockfile_path)?
                    } else {
                        None
                    };

                    if let Some(imported_lockfile) = imported {
                        download_resolved_from_lock(&imported_lockfile.packages, &downloader)
                            .await?;
                        handled = true;
                    } else {
                        println!(
                            "Failed to import {}, resolving from package.json...",
                            lockfile_path
                        );
                    }
                }
            }

            // Auto-detect external lock files if not already handled and no rnpm.lock exists
            if !handled && !Path::new("rnpm.lock").exists() {
                if let Some(manager) = RnpmConfig::detect_and_prompt()? {
                    // Save preference to config
                    let mut new_config = RnpmConfig::load()?;
                    new_config.use_lockfile = Some(manager.clone());
                    new_config.save()?;
                    println!("✓ Saved preference to rnpm.config.json");

                    // Import and use the detected lock file
                    let lockfile_path = if manager == "bun" {
                        // Check both bun.lock and bun.lockb
                        if Path::new("bun.lock").exists() {
                            "bun.lock"
                        } else {
                            "bun.lockb"
                        }
                    } else {
                        match manager.as_str() {
                            "npm" => "package-lock.json",
                            "yarn" => "yarn.lock",
                            "pnpm" => "pnpm-lock.yaml",
                            _ => unreachable!(),
                        }
                    };

                    if Path::new(lockfile_path).exists() {
                        println!("Importing {}...", lockfile_path);
                        let imported = if lockfile_path == "package-lock.json" {
                            lockfile_import::import_npm_lockfile(lockfile_path)?
                        } else if lockfile_path == "yarn.lock" {
                            lockfile_import::import_yarn_lockfile(lockfile_path)?
                        } else if lockfile_path == "pnpm-lock.yaml" {
                            lockfile_import::import_pnpm_lockfile(lockfile_path)?
                        } else if lockfile_path == "bun.lock" || lockfile_path == "bun.lockb" {
                            lockfile_import::import_bun_lockfile(lockfile_path)?
                        } else {
                            None
                        };

                        if let Some(imported_lockfile) = imported {
                            download_resolved_from_lock(&imported_lockfile.packages, &downloader)
                                .await?;
                            handled = true;
                        }
                    }
                }
            }

            // Fall back to rnpm.lock or package.json resolution if not handled
            if !handled {
                if Path::new("rnpm.lock").exists() {
                    println!("Using rnpm.lock...");
                    let lockfile = Lockfile::load()?;
                    download_resolved_from_lock(&lockfile.packages, &downloader).await?;
                } else {
                    let pb = ProgressBar::new_spinner();
                    install_from_package_json(&pb, &registry, &downloader).await?;
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
                resolver.resolve_multiple(packages_to_resolve).await?;
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
        Commands::Import { lockfile_path } => {
            let pb = ProgressBar::new_spinner();
            pb.set_message("Importing lock file...");
            pb.enable_steady_tick(std::time::Duration::from_millis(120));

            // Auto-detect lock file if not specified
            let path = if let Some(p) = &lockfile_path {
                p.clone()
            } else {
                let detected = if std::path::Path::new("package-lock.json").exists() {
                    "package-lock.json"
                } else if std::path::Path::new("yarn.lock").exists() {
                    "yarn.lock"
                } else if std::path::Path::new("pnpm-lock.yaml").exists() {
                    "pnpm-lock.yaml"
                } else {
                    "package-lock.json"
                };
                detected.to_string()
            };

            let imported_lockfile = if path.ends_with("package-lock.json") {
                lockfile_import::import_npm_lockfile(&path)?
            } else if path.ends_with("yarn.lock") {
                lockfile_import::import_yarn_lockfile(&path)?
            } else if path.ends_with("pnpm-lock.yaml") {
                lockfile_import::import_pnpm_lockfile(&path)?
            } else if path.ends_with("bun.lock") || path.ends_with("bun.lockb") {
                lockfile_import::import_bun_lockfile(&path)?
            } else {
                eprintln!("Unsupported lock file format: {}", path);
                std::process::exit(1);
            };

            if let Some(lockfile) = imported_lockfile {
                let pkg_count = lockfile.packages.len();
                lockfile.save()?;
                pb.finish_with_message(format!(
                    "Imported {} packages from {} to rnpm.lock",
                    pkg_count, path
                ));
                println!("\nYou can now run 'rnpm install' to install dependencies.");
            } else {
                pb.finish_with_message("No lock file found or import failed.");
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
    // Check for missing packages AND missing bins
    let mut to_download = Vec::new();
    let mut missing_bins = Vec::new();

    for (name, locked) in packages {
        let pkg_path_str = format!("node_modules/{}", name);
        let pkg_path = Path::new(&pkg_path_str);
        if !pkg_path.exists() {
            to_download.push((name, locked));
        } else {
            // Package exists but check if it has bin entries that aren't linked
            let pkg_json = pkg_path.join("package.json");
            if pkg_json.exists() {
                if let Ok(content) = fs::read_to_string(&pkg_json) {
                    if let Ok(pkg_data) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(bin) = pkg_data.get("bin") {
                            match bin {
                                serde_json::Value::String(_) => {
                                    let bin_name = pkg_data["name"].as_str().unwrap_or(name);
                                    let bin_path = Path::new("node_modules/.bin").join(bin_name);
                                    if !bin_path.exists() {
                                        missing_bins.push(name.clone());
                                    }
                                }
                                serde_json::Value::Object(bin_map) => {
                                    for bin_name in bin_map.keys() {
                                        let bin_path =
                                            Path::new("node_modules/.bin").join(bin_name);
                                        if !bin_path.exists() {
                                            missing_bins.push(name.clone());
                                            break;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    if to_download.is_empty() && missing_bins.is_empty() {
        println!("All dependencies already installed.");
        return Ok(());
    }

    if !missing_bins.is_empty() {
        println!(
            "Found {} package(s) with missing binary links. Rebuilding .bin directory...",
            missing_bins.len()
        );
    }

    let multi = MultiProgress::new();
    let main_pb = multi.add(ProgressBar::new(to_download.len() as u64));
    main_pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}")?
        .progress_chars("#>-"));
    main_pb.set_message(if to_download.is_empty() {
        "Rebuilding binaries..."
    } else {
        "Downloading packages from lockfile"
    });

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

    if !futures.is_empty() {
        futures::future::join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<()>>>()?;
        main_pb.finish_with_message("All packages installed successfully.");
    } else {
        main_pb.finish_with_message("All packages already installed.");
    }

    // Create/rebuild .bin symlinks
    create_bin_links()?;

    Ok(())
}

fn create_bin_links() -> Result<()> {
    let bin_dir = Path::new("node_modules/.bin");
    if !bin_dir.exists() {
        fs::create_dir_all(bin_dir)?;
    }

    // Read all packages in node_modules
    if let Ok(entries) = fs::read_dir("node_modules") {
        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();

                // Skip hidden directories and .bin
                if name.starts_with('.') || name == ".bin" {
                    continue;
                }

                // Check for package.json bin field
                let pkg_json_path = path.join("package.json");
                if pkg_json_path.exists() {
                    if let Ok(content) = fs::read_to_string(&pkg_json_path) {
                        if let Ok(pkg_json) = serde_json::from_str::<serde_json::Value>(&content) {
                            // Handle "bin" field
                            if let Some(bin) = pkg_json.get("bin") {
                                match bin {
                                    serde_json::Value::String(bin_path) => {
                                        // Single binary: use package name
                                        let pkg_name = pkg_json["name"].as_str().unwrap_or(&name);
                                        let bin_src = path.join(bin_path);
                                        let bin_dst = bin_dir.join(pkg_name);
                                        let _ = create_bin_link(&bin_src, &bin_dst);
                                    }
                                    serde_json::Value::Object(bin_map) => {
                                        // Multiple binaries
                                        for (bin_name, bin_path) in bin_map {
                                            let bin_src =
                                                path.join(bin_path.as_str().unwrap_or(""));
                                            let bin_dst = bin_dir.join(bin_name);
                                            let _ = create_bin_link(&bin_src, &bin_dst);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn create_bin_link(src: &Path, dst: &Path) -> Result<()> {
    // Remove existing link/file
    if dst.exists() {
        let _ = fs::remove_file(dst);
    }

    // Make the target executable and create symlink
    if src.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            // Make executable
            if let Ok(metadata) = fs::metadata(src) {
                let mut perms = metadata.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(src, perms);
            }
            // Create symlink with ABSOLUTE path to avoid relative path issues
            let abs_src = std::env::current_dir()?.join(src);
            if let Err(e) = symlink(&abs_src, dst) {
                eprintln!(
                    "Warning: Failed to create symlink {:?} -> {:?}: {}",
                    abs_src, dst, e
                );
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::fs::symlink_file;
            if let Err(e) = symlink_file(src, dst) {
                eprintln!(
                    "Warning: Failed to create symlink {:?} -> {:?}: {}",
                    src, dst, e
                );
            }
        }
    }

    Ok(())
}

fn run_command(cmd: &str) -> Result<()> {
    let mut parts = cmd.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Empty command"))?;
    let args: Vec<&str> = parts.collect();

    // Add node_modules/.bin to PATH
    let current_path = std::env::var("PATH").unwrap_or_default();
    let bin_dir = std::env::current_dir()?.join("node_modules/.bin");
    let new_path = if bin_dir.exists() {
        format!("{}:{}", bin_dir.display(), current_path)
    } else {
        current_path
    };

    let status = process::Command::new(program)
        .args(args)
        .env("PATH", new_path)
        .status()?;

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

async fn install_from_package_json(
    pb: &ProgressBar,
    registry: &Arc<registry::RegistryClient>,
    downloader: &Arc<downloader::Downloader>,
) -> Result<()> {
    pb.set_message("Resolving dependencies from package.json...");
    pb.enable_steady_tick(std::time::Duration::from_millis(120));

    let content = fs::read_to_string("package.json")
        .map_err(|_| anyhow::anyhow!("package.json not found"))?;
    let package_json: PackageJson = serde_json::from_str(&content)?;

    let resolver = resolver::Resolver::new(Arc::clone(registry)).with_progress(pb.clone());

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
        resolver.resolve_multiple(packages_to_resolve).await?;
        let resolved = resolver.resolved.lock().unwrap().clone();
        pb.finish_with_message(format!("Resolved {} dependencies.", resolved.len()));
        download_resolved_from_meta(&resolved, downloader).await?;
        Lockfile::from_resolved(&resolved).save()?;
    } else {
        pb.finish_with_message("No dependencies to install.");
    }

    Ok(())
}
