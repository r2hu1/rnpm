use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct RnpmConfig {
    /// Use external lock file for dependency resolution
    /// Options: "npm" (package-lock.json), "yarn" (yarn.lock), "pnpm" (pnpm-lock.yaml), or null (use rnpm.lock)
    #[serde(rename = "useLockfile")]
    pub use_lockfile: Option<String>,
}

impl RnpmConfig {
    pub fn load() -> Result<Self> {
        let config_path = Path::new("rnpm.config.json");
        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(config_path)?;
        let config: RnpmConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        fs::write("rnpm.config.json", content)?;
        Ok(())
    }

    pub fn get_lockfile_path(&self) -> Option<String> {
        match &self.use_lockfile {
            Some(manager) => match manager.as_str() {
                "npm" => Some("package-lock.json".to_string()),
                "yarn" => Some("yarn.lock".to_string()),
                "pnpm" => Some("pnpm-lock.yaml".to_string()),
                "bun" => {
                    // Check for both bun.lock (newer) and bun.lockb (older binary format)
                    if std::path::Path::new("bun.lock").exists() {
                        Some("bun.lock".to_string())
                    } else {
                        Some("bun.lockb".to_string())
                    }
                }
                _ => None,
            },
            None => None,
        }
    }

    pub fn detect_and_prompt() -> Result<Option<String>> {
        // Check if any external lock files exist (including both Bun formats)
        let candidates = vec![
            ("npm", "package-lock.json"),
            ("yarn", "yarn.lock"),
            ("pnpm", "pnpm-lock.yaml"),
            ("bun", "bun.lock"),
        ];

        // Also check for bun.lockb if bun.lock doesn't exist
        let bun_lockb_exists = Path::new("bun.lockb").exists() && !Path::new("bun.lock").exists();
        let candidates_with_lockb = if bun_lockb_exists {
            vec![("bun", "bun.lockb")]
        } else {
            vec![]
        };

        let all_candidates: Vec<_> = candidates
            .iter()
            .chain(candidates_with_lockb.iter())
            .collect();

        for (manager, filename) in &all_candidates {
            if Path::new(filename).exists() {
                println!("\n📦 Detected {} lock file: {}", manager, filename);
                print!("Would you like to use it for dependency resolution? (y/N): ");

                use std::io::{self, Write};
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;

                if input.trim().to_lowercase() == "y" || input.trim().to_lowercase() == "yes" {
                    return Ok(Some(manager.to_string()));
                }
            }
        }

        Ok(None)
    }
}
