use anyhow::{Result, anyhow};
use reqwest::Client;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PackageMetadata {
    pub name: String,
    pub versions: HashMap<String, VersionMetadata>,
    #[serde(rename = "dist-tags")]
    pub dist_tags: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VersionMetadata {
    pub name: String,
    pub version: String,
    pub dist: Dist,
    pub dependencies: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Dist {
    pub tarball: String,
    pub shasum: String,
}

pub struct RegistryClient {
    client: Client,
    base_url: String,
    // Cache to avoid duplicate network requests for the same package
    metadata_cache: RwLock<HashMap<String, PackageMetadata>>,
}

impl RegistryClient {
    pub fn new() -> Self {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static("rnpm/0.1.0"),
        );
        // Request abbreviated metadata format for much better performance
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static(
                "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8",
            ),
        );

        Self {
            client: Client::builder()
                .default_headers(headers)
                .timeout(std::time::Duration::from_secs(60))
                .tcp_keepalive(Some(std::time::Duration::from_secs(30)))
                .pool_max_idle_per_host(20)
                .connect_timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            base_url: "https://registry.npmjs.org".to_string(),
            metadata_cache: RwLock::new(HashMap::new()),
        }
    }

    pub async fn fetch_package_metadata(&self, name: &str) -> Result<PackageMetadata> {
        // Check cache first to avoid duplicate network requests
        {
            let cache = self.metadata_cache.read().await;
            if let Some(metadata) = cache.get(name) {
                return Ok(metadata.clone());
            }
        }

        let encoded_name = name.replace("/", "%2f");
        let url = format!("{}/{}", self.base_url, encoded_name);

        // Retry logic for failed requests
        let mut retries = 3;
        let metadata = loop {
            match self.client.get(&url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<PackageMetadata>().await {
                            Ok(meta) => break meta,
                            Err(e) => {
                                if retries == 0 {
                                    return Err(anyhow!(
                                        "Failed to parse metadata for {}: {}",
                                        name,
                                        e
                                    ));
                                }
                                retries -= 1;
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                continue;
                            }
                        }
                    } else {
                        return Err(anyhow!(
                            "Package not found: {} (Status: {})",
                            name,
                            response.status()
                        ));
                    }
                }
                Err(e) => {
                    if retries == 0 {
                        return Err(anyhow!(
                            "Failed to fetch metadata for {} after 3 retries: {}",
                            name,
                            e
                        ));
                    }
                    retries -= 1;
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        };

        // Cache the result
        {
            let mut cache = self.metadata_cache.write().await;
            cache.insert(name.to_string(), metadata.clone());
        }

        Ok(metadata)
    }

    pub fn resolve_version(
        &self,
        metadata: &PackageMetadata,
        range: &str,
    ) -> Result<VersionMetadata> {
        if let Some(version) = metadata.dist_tags.get(range) {
            return metadata
                .versions
                .get(version)
                .cloned()
                .ok_or(anyhow!("Invalid version in dist-tags"));
        }

        if let Some(version_meta) = metadata.versions.get(range) {
            return Ok(version_meta.clone());
        }

        // Handle semver ranges
        let req = VersionReq::parse(range).unwrap_or_else(|_| VersionReq::parse("*").unwrap());

        let mut available_versions: Vec<Version> = metadata
            .versions
            .keys()
            .filter_map(|v| Version::parse(v).ok())
            .collect();

        available_versions.sort_by(|a, b| b.cmp(a));

        for version in available_versions {
            if req.matches(&version) {
                return Ok(metadata.versions.get(&version.to_string()).unwrap().clone());
            }
        }

        if let Some(latest) = metadata.dist_tags.get("latest") {
            if let Some(meta) = metadata.versions.get(latest) {
                return Ok(meta.clone());
            }
        }

        Err(anyhow!(
            "Could not resolve version for {} with range {}",
            metadata.name,
            range
        ))
    }
}
