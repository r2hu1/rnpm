use anyhow::Result;
use flate2::read::GzDecoder;
use reqwest::Client;
use std::fs;
use std::path::Path;
use tar::Archive;

#[derive(Clone)]
pub struct Downloader {
    client: Client,
}

impl Downloader {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub async fn download_and_extract(&self, url: &str, destination: &Path) -> Result<()> {
        if destination.exists() {
            return Ok(());
        }

        let response = self.client.get(url).send().await?;
        let bytes = response.bytes().await?;

        let tar_gz = GzDecoder::new(&bytes[..]);
        let mut archive = Archive::new(tar_gz);

        let temp_dir = tempfile::tempdir()?;
        archive.unpack(temp_dir.path())?;

        let package_path = temp_dir.path().join("package");
        if package_path.exists() {
            fs::create_dir_all(destination.parent().unwrap())?;
            fs::rename(package_path, destination)?;
        } else {
             fs::create_dir_all(destination)?;
             archive.unpack(destination)?;
        }

        Ok(())
    }
}
