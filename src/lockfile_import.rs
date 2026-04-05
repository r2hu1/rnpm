use crate::lockfile::{LockedPackage, Lockfile};
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Import dependencies from npm's package-lock.json
pub fn import_npm_lockfile(path: &str) -> Result<Option<Lockfile>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let npm_lock: NpmLockfile = serde_json::from_str(&content)?;

    let mut packages = HashMap::new();

    // npm v2+ format (hoisted)
    if let Some(packages_map) = npm_lock.packages {
        for (name, pkg) in packages_map {
            // Skip empty root entry
            if name.is_empty() {
                continue;
            }

            // Extract package name from path (node_modules/@scope/name or node_modules/name)
            let pkg_name = extract_package_name(&name);

            if let Some(version) = pkg.version {
                let tarball = pkg.resolved.clone().unwrap_or_default();
                let integrity = pkg.integrity.clone().unwrap_or_default();

                packages.insert(
                    pkg_name,
                    LockedPackage {
                        version,
                        tarball,
                        shasum: integrity,
                        dependencies: pkg.dependencies.clone(),
                    },
                );
            }
        }
    }

    Ok(Some(Lockfile { packages }))
}

/// Import dependencies from Yarn's yarn.lock
pub fn import_yarn_lockfile(path: &str) -> Result<Option<Lockfile>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;
    let mut packages = HashMap::new();

    // Simple yarn.lock parser (handles basic cases)
    let mut current_packages: Vec<String> = vec![];
    let mut current_version: Option<String> = None;
    let mut current_resolution: Option<String> = None;
    let mut current_integrity: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            // End of a package block
            if let (Some(_), Some(resolution), Some(integrity)) =
                (&current_version, &current_resolution, &current_integrity)
            {
                for pkg_name in &current_packages {
                    let name = extract_package_name_from_yarn(pkg_name);
                    packages.insert(
                        name,
                        LockedPackage {
                            version: current_version.clone().unwrap(),
                            tarball: resolution.clone(),
                            shasum: integrity.clone(),
                            dependencies: None,
                        },
                    );
                }
            }

            current_packages.clear();
            current_version = None;
            current_resolution = None;
            current_integrity = None;
            continue;
        }

        if !line.starts_with(' ') && !line.starts_with('\t') {
            // Package identifier line (e.g., "package@^1.0.0":)
            if trimmed.ends_with(':') {
                let pkg_id = &trimmed[..trimmed.len() - 1];
                current_packages.push(pkg_id.to_string());
            }
        } else {
            // Property lines
            if trimmed.starts_with("version") {
                if let Some(ver) = trimmed.split('"').nth(1) {
                    current_version = Some(ver.to_string());
                }
            } else if trimmed.starts_with("resolved") {
                if let Some(url) = trimmed.split('"').nth(1) {
                    current_resolution = Some(url.to_string());
                }
            } else if trimmed.starts_with("integrity") {
                if let Some(hash) = trimmed.split_whitespace().nth(1) {
                    current_integrity = Some(hash.to_string());
                }
            }
        }
    }

    Ok(Some(Lockfile { packages }))
}

/// Import dependencies from pnpm's pnpm-lock.yaml
pub fn import_pnpm_lockfile(path: &str) -> Result<Option<Lockfile>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;

    // Parse YAML
    let pnpm_lock: serde_yaml::Value = serde_yaml::from_str(&content)?;

    let mut packages = HashMap::new();

    // pnpm-lock.yaml v6+ format
    if let Some(packages_map) = pnpm_lock.get("packages").and_then(|v| v.as_mapping()) {
        for (pkg_key, pkg_value) in packages_map {
            if let (Some(pkg_name), Some(pkg_info)) = (pkg_key.as_str(), pkg_value.as_mapping()) {
                // Extract version
                let version = pkg_info
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if version.is_empty() {
                    continue;
                }

                // Extract resolution (tarball or integrity)
                let tarball = pkg_info
                    .get("resolution")
                    .and_then(|r| r.get("tarball"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();

                let integrity = pkg_info
                    .get("resolution")
                    .and_then(|r| r.get("integrity"))
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();

                // Skip if we don't have enough info
                if tarball.is_empty() && integrity.is_empty() {
                    continue;
                }

                packages.insert(
                    pkg_name.to_string(),
                    LockedPackage {
                        version: version.to_string(),
                        tarball: if !tarball.is_empty() {
                            tarball
                        } else {
                            integrity.clone()
                        },
                        shasum: integrity,
                        dependencies: None,
                    },
                );
            }
        }
    }

    Ok(Some(Lockfile { packages }))
}

/// Import dependencies from Bun's bun.lock (JSON format)
pub fn import_bun_lockfile(path: &str) -> Result<Option<Lockfile>> {
    if !Path::new(path).exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(path)?;

    // Remove trailing commas (Bun allows them, but standard JSON doesn't)
    let cleaned_content = content
        .replace(",\n      }", "\n      }")
        .replace(",\n    }", "\n    }")
        .replace(",\n  }", "\n  }")
        .replace(",\n}", "\n}");

    // Try to parse as JSON first (bun.lock can be JSON in newer versions)
    match serde_json::from_str::<BunLockfile>(&cleaned_content) {
        Ok(bun_lock) => {
            let mut packages = HashMap::new();

            // Extract packages from the "packages" field
            if let Some(packages_map) = bun_lock.packages {
                for (_name, package_info) in packages_map {
                    // Package info is an array: [full_name, peer_deps, deps_object, integrity]
                    if let Some(info_array) = package_info.as_array() {
                        if info_array.len() >= 4 {
                            // Extract version from full name (e.g., "react@19.2.4")
                            let full_name = info_array[0].as_str().unwrap_or("");
                            let integrity = info_array[3].as_str().unwrap_or("");

                            // Parse name and version
                            if let Some(at_pos) = full_name.rfind('@') {
                                if at_pos > 0 {
                                    let pkg_name = &full_name[..at_pos];
                                    let version = &full_name[at_pos + 1..];

                                    // Skip workspace roots (empty version)
                                    if version.is_empty() {
                                        continue;
                                    }

                                    packages.insert(
                                        pkg_name.to_string(),
                                        LockedPackage {
                                            version: version.to_string(),
                                            tarball: format!(
                                                "https://registry.npmjs.org/{}/-/{}-{}.tgz",
                                                pkg_name.replace("/", "%2F"),
                                                pkg_name.split('/').last().unwrap_or(pkg_name),
                                                version
                                            ),
                                            shasum: integrity.to_string(),
                                            dependencies: None,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }

            return Ok(Some(Lockfile { packages }));
        }
        Err(e) => {
            eprintln!("Failed to parse bun.lock as JSON: {}", e);
            eprintln!("Binary bun.lockb format not supported, please use text-based bun.lock");
            return Ok(None);
        }
    }
}

// Helper functions

fn extract_package_name(npm_path: &str) -> String {
    // Convert "node_modules/@scope/package" or "node_modules/package" to "@scope/package" or "package"
    npm_path.trim_start_matches("node_modules/").to_string()
}

fn extract_package_name_from_yarn(yarn_id: &str) -> String {
    // Convert "package@version" to "package"
    if let Some(at_pos) = yarn_id.find('@') {
        if at_pos > 0 {
            return yarn_id[..at_pos].to_string();
        }
    }
    yarn_id.to_string()
}

// NPM lockfile structures
#[derive(Debug, Deserialize)]
struct NpmLockfile {
    #[serde(rename = "lockfileVersion")]
    lockfile_version: u32,
    packages: Option<HashMap<String, NpmPackage>>,
}

#[derive(Debug, Deserialize, Clone)]
struct NpmPackage {
    version: Option<String>,
    resolved: Option<String>,
    integrity: Option<String>,
    dependencies: Option<HashMap<String, String>>,
}

// Bun lockfile structures (JSON format)
#[derive(Debug, Deserialize)]
struct BunLockfile {
    #[serde(rename = "lockfileVersion")]
    lockfile_version: Option<u32>,
    packages: Option<HashMap<String, serde_json::Value>>,
}
