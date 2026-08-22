use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VersionInfo {
    pub current: String,
    pub latest: Option<String>,
    pub has_update: bool,
    pub download_url: Option<String>,
    pub upgrade_supported: bool,
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
    #[cfg_attr(windows, allow(dead_code))]
    pub size: u64,
}
