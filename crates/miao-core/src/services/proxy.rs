use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::error::{AppError, AppResult};
use crate::models::LastProxy;
use crate::services::singbox::get_sing_box_home;
use crate::state::AppState;

const LAST_PROXY_FILENAME: &str = ".last_proxy";

/// Where `.last_proxy` is stored.
///
/// Default is the runtime dir under `/tmp` so OpenWrt overlay/flash is never
/// used, even when `/etc/openwrt_release` is missing. The cwd-relative file is
/// only used when PID 1 is systemd and the system does not look like OpenWrt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LastProxyStore {
    RuntimeDir,
    WorkingDir,
}

fn last_proxy_store(openwrt_like: bool, pid1_comm: &str) -> LastProxyStore {
    if cfg!(windows) {
        return LastProxyStore::RuntimeDir;
    }
    if openwrt_like || pid1_comm.trim() != "systemd" {
        LastProxyStore::RuntimeDir
    } else {
        LastProxyStore::WorkingDir
    }
}

fn last_proxy_path_for(store: LastProxyStore) -> PathBuf {
    match store {
        LastProxyStore::RuntimeDir => get_sing_box_home().join(LAST_PROXY_FILENAME),
        LastProxyStore::WorkingDir => PathBuf::from(LAST_PROXY_FILENAME),
    }
}

fn os_release_looks_like_openwrt(content: &str) -> bool {
    content.lines().any(|line| {
        let line = line.trim();
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        key.starts_with("OPENWRT_")
            || (key == "ID"
                && matches!(value, "openwrt" | "immortalwrt" | "libremesh" | "istoreos"))
    })
}

fn openwrt_like_from_paths(exists: impl Fn(&str) -> bool, os_release: Option<&str>) -> bool {
    exists("/etc/openwrt_release")
        || exists("/etc/openwrt_version")
        || exists("/sbin/procd")
        || os_release.is_some_and(os_release_looks_like_openwrt)
}

fn is_openwrt_like() -> bool {
    let os_release = std::fs::read_to_string("/etc/os-release").ok();
    openwrt_like_from_paths(|path| Path::new(path).exists(), os_release.as_deref())
}

fn pid1_comm() -> String {
    std::fs::read_to_string("/proc/1/comm")
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

fn get_last_proxy_path() -> PathBuf {
    last_proxy_path_for(last_proxy_store(is_openwrt_like(), &pid1_comm()))
}

async fn write_last_proxy_file(path: &Path, proxy: &LastProxy) -> AppResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| AppError::context("Failed to create last-proxy directory", e))?;
    }

    let json = serde_json::to_string(proxy)?;
    tokio::fs::write(path, json)
        .await
        .map_err(|e| AppError::context("Failed to write last-proxy file", e))?;
    Ok(())
}

pub async fn save_last_proxy(proxy: &LastProxy) -> AppResult<()> {
    write_last_proxy_file(&get_last_proxy_path(), proxy).await
}

async fn load_last_proxy() -> Option<LastProxy> {
    let path = get_last_proxy_path();
    if let Ok(content) = tokio::fs::read_to_string(&path).await {
        serde_json::from_str(&content).ok()
    } else {
        None
    }
}

pub async fn restore_last_proxy(state: &Arc<AppState>) {
    let proxy = match load_last_proxy().await {
        Some(p) => p,
        None => return,
    };

    info!(
        "Attempting to restore last proxy: {} -> {}",
        proxy.group, proxy.name
    );

    sleep(Duration::from_secs(1)).await;

    let url = format!(
        "http://127.0.0.1:6262/proxies/{}",
        urlencoding::encode(&proxy.group)
    );
    let group_info = match state
        .http_client
        .get(&url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        Ok(res) => match res.json::<serde_json::Value>().await {
            Ok(v) => v,
            Err(_) => return,
        },
        Err(_) => return,
    };

    let all_nodes = group_info.get("all").and_then(|v| v.as_array());
    if let Some(nodes) = all_nodes {
        let node_exists = nodes.iter().any(|n| n.as_str() == Some(&proxy.name));
        if !node_exists {
            warn!(
                "Last proxy '{}' not found in current node list, skipping restore",
                proxy.name
            );
            return;
        }
    } else {
        return;
    }

    match state
        .http_client
        .put(&url)
        .timeout(Duration::from_secs(5))
        .json(&serde_json::json!({ "name": proxy.name }))
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            info!("Successfully restored last proxy: {}", proxy.name);
        }
        Ok(res) => {
            warn!("Failed to restore last proxy: status {}", res.status());
        }
        Err(e) => {
            error!("Failed to restore last proxy: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        get_sing_box_home, last_proxy_path_for, last_proxy_store, openwrt_like_from_paths,
        os_release_looks_like_openwrt, write_last_proxy_file, LastProxyStore, LAST_PROXY_FILENAME,
    };
    use crate::models::LastProxy;

    #[test]
    fn last_proxy_path_uses_tmp_on_openwrt() {
        assert_eq!(
            last_proxy_path_for(LastProxyStore::RuntimeDir),
            get_sing_box_home().join(LAST_PROXY_FILENAME)
        );
    }

    #[test]
    fn last_proxy_path_uses_working_directory_on_regular_linux() {
        assert_eq!(
            last_proxy_path_for(LastProxyStore::WorkingDir),
            std::path::PathBuf::from(LAST_PROXY_FILENAME)
        );
    }

    #[test]
    fn last_proxy_store_uses_tmp_when_openwrt_markers_exist() {
        assert_eq!(
            last_proxy_store(true, "systemd"),
            LastProxyStore::RuntimeDir
        );
        assert_eq!(last_proxy_store(true, "procd"), LastProxyStore::RuntimeDir);
    }

    #[test]
    fn last_proxy_store_uses_tmp_when_pid1_is_not_systemd() {
        assert_eq!(last_proxy_store(false, "procd"), LastProxyStore::RuntimeDir);
        assert_eq!(last_proxy_store(false, ""), LastProxyStore::RuntimeDir);
    }

    #[cfg(not(windows))]
    #[test]
    fn last_proxy_store_uses_cwd_only_for_systemd_linux() {
        assert_eq!(
            last_proxy_store(false, "systemd"),
            LastProxyStore::WorkingDir
        );
    }

    #[cfg(windows)]
    #[test]
    fn last_proxy_store_is_runtime_dir_on_windows() {
        assert_eq!(
            last_proxy_store(false, "systemd"),
            LastProxyStore::RuntimeDir
        );
    }

    #[test]
    fn openwrt_like_detects_release_file() {
        assert!(openwrt_like_from_paths(
            |path| path == "/etc/openwrt_release",
            None
        ));
    }

    #[test]
    fn openwrt_like_detects_version_file() {
        assert!(openwrt_like_from_paths(
            |path| path == "/etc/openwrt_version",
            None
        ));
    }

    #[test]
    fn openwrt_like_detects_procd_without_release_file() {
        assert!(openwrt_like_from_paths(|path| path == "/sbin/procd", None));
    }

    #[test]
    fn openwrt_like_is_false_without_markers() {
        assert!(!openwrt_like_from_paths(|_| false, None));
        assert!(!openwrt_like_from_paths(
            |_| false,
            Some("ID=arch\nPRETTY_NAME=\"Arch Linux\"\n")
        ));
    }

    #[test]
    fn os_release_detects_openwrt_and_forks() {
        assert!(os_release_looks_like_openwrt(
            "ID=openwrt\nOPENWRT_RELEASE=\"OpenWrt 23.05\"\n"
        ));
        assert!(os_release_looks_like_openwrt(
            "ID=\"immortalwrt\"\nNAME=\"ImmortalWrt\"\n"
        ));
        assert!(os_release_looks_like_openwrt(
            "ID=debian\nOPENWRT_BOARD=\"ramips/mt7621\"\n"
        ));
        assert!(!os_release_looks_like_openwrt(
            "ID=ubuntu\nVERSION_ID=24.04\n"
        ));
    }

    #[tokio::test]
    async fn write_last_proxy_creates_missing_parent_directory() {
        let dir = std::env::temp_dir().join(format!(
            "miao-last-proxy-{}-{}",
            std::process::id(),
            "mkdir"
        ));
        let _ = tokio::fs::remove_dir_all(&dir).await;
        let path = dir.join("nested").join(LAST_PROXY_FILENAME);
        let proxy = LastProxy {
            group: "proxy".to_string(),
            name: "node-a".to_string(),
        };

        write_last_proxy_file(&path, &proxy).await.unwrap();

        let content = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("node-a"));
        assert!(content.contains("proxy"));
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
