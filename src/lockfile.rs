use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use anyhow::Result;
use std::fs;
use crate::registry::VersionMetadata;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Lockfile {
    pub packages: HashMap<String, LockedPackage>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LockedPackage {
    pub version: String,
    pub tarball: String,
    pub shasum: String,
    pub dependencies: Option<HashMap<String, String>>,
}

impl Lockfile {
    pub fn new() -> Self {
        Self {
            packages: HashMap::new(),
        }
    }

    pub fn load() -> Result<Self> {
        if !std::path::Path::new("rnpm.lock").exists() {
            return Ok(Self::new());
        }
        let content = fs::read_to_string("rnpm.lock")?;
        let lockfile: Lockfile = serde_json::from_str(&content)?;
        Ok(lockfile)
    }

    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write("rnpm.lock", content)?;
        Ok(())
    }

    pub fn from_resolved(resolved: &HashMap<String, VersionMetadata>) -> Self {
        let mut packages = HashMap::new();
        for (name, meta) in resolved {
            packages.insert(name.clone(), LockedPackage {
                version: meta.version.clone(),
                tarball: meta.dist.tarball.clone(),
                shasum: meta.dist.shasum.clone(),
                dependencies: meta.dependencies.clone(),
            });
        }
        Self { packages }
    }
}
