use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct VersionInfo {
    pub current: String,
    pub latest: Option<String>,
    pub has_update: bool,
    pub download_url: Option<String>,
}

#[derive(Clone, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub assets: Vec<GitHubAsset>,
}

#[derive(Clone, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
}
