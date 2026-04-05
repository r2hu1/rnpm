use anyhow::Result;
use flate2::read::GzDecoder;
use reqwest::Client;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tar::Archive;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct Downloader {
    client: Client,
    semaphore: Arc<Semaphore>,
}

impl Downloader {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap(),
            semaphore: Arc::new(Semaphore::new(15)), // Limit to 15 concurrent downloads
        }
    }

    pub async fn download_and_extract(&self, url: &str, destination: &Path) -> Result<()> {
        if destination.exists() {
            return Ok(());
        }

        // Acquire permit to limit concurrent downloads
        let _permit = self.semaphore.acquire().await.unwrap();

        let response = self.client.get(url).send().await?;
        let bytes = response.bytes().await?;

        let tar_gz = GzDecoder::new(&bytes[..]);
        let mut archive = Archive::new(tar_gz);

        let temp_dir = tempfile::tempdir()?;

        // Unpack to temp directory
        archive.unpack(temp_dir.path())?;

        let package_path = temp_dir.path().join("package");
        if package_path.exists() {
            // npm packages are usually in a "package" subdirectory
            fs::create_dir_all(destination.parent().unwrap_or(Path::new(".")))?;
            fs::rename(package_path, destination)?;
        } else {
            // Fallback: find the actual package directory
            let entries: Vec<_> = fs::read_dir(temp_dir.path())?
                .filter_map(|e| e.ok())
                .collect();

            if entries.len() == 1 && entries[0].path().is_dir() {
                // Single directory, rename it
                fs::create_dir_all(destination.parent().unwrap_or(Path::new(".")))?;
                fs::rename(entries[0].path(), destination)?;
            } else {
                // Multiple files, create destination and copy contents
                fs::create_dir_all(destination)?;
                for entry in entries {
                    let src = entry.path();
                    let dst = destination.join(entry.file_name());
                    if src.is_dir() {
                        fs::create_dir_all(&dst)?;
                        copy_dir_all(&src, &dst)?;
                    } else {
                        fs::copy(src, dst)?;
                    }
                }
            }
        }

        Ok(())
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            let dst_path = dst.join(entry.file_name());
            fs::create_dir_all(&dst_path)?;
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}
