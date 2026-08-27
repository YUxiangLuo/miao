use super::{
    current_arch_asset_name, get_version_info, parse_semver_tag, parse_sha256sum_line,
    release_is_newer_than_current, stdout_version_matches_release, version_info_from_release,
};
use crate::models::{Config, GitHubAsset, GitHubRelease};
use crate::platform::upgrade_supported;
use crate::test_support::app_state;

#[cfg(not(windows))]
use super::{
    current_version, mark_upgrade_healthy_at, upgrade_pending_path, upgrade_requires_rollback,
    write_upgrade_pending, UpgradePending,
};

#[cfg(not(windows))]
#[test]
fn pending_upgrade_rolls_back_only_after_an_unhealthy_second_boot() {
    let dir = std::env::temp_dir().join(format!(
        "miao-upgrade-marker-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let executable = dir.join("miao");
    let backup = std::path::PathBuf::from(format!("{}.bak", executable.display()));
    std::fs::write(&executable, b"new").unwrap();
    std::fs::write(&backup, b"old").unwrap();
    write_upgrade_pending(
        &upgrade_pending_path(&executable),
        &UpgradePending {
            expected_version: current_version(),
            state: "installed".to_string(),
            boot_pid: None,
        },
    )
    .unwrap();

    assert!(!upgrade_requires_rollback(&executable).unwrap());
    let marker: UpgradePending =
        serde_json::from_slice(&std::fs::read(upgrade_pending_path(&executable)).unwrap()).unwrap();
    assert_eq!(marker.state, "booting");
    assert_eq!(marker.boot_pid, Some(std::process::id()));
    assert!(!upgrade_requires_rollback(&executable).unwrap());

    write_upgrade_pending(
        &upgrade_pending_path(&executable),
        &UpgradePending {
            boot_pid: Some(u32::MAX),
            ..marker
        },
    )
    .unwrap();
    assert!(upgrade_requires_rollback(&executable).unwrap());

    mark_upgrade_healthy_at(&executable);
    assert!(!backup.exists());
    assert!(!upgrade_pending_path(&executable).exists());
    assert!(executable.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn parse_semver_tag_accepts_prefixed_and_unprefixed() {
    assert!(parse_semver_tag("v1.2.3").is_some());
    assert!(parse_semver_tag("1.2.3").is_some());
}

#[test]
fn parse_semver_tag_rejects_invalid() {
    assert!(parse_semver_tag("v1.2").is_none());
    assert!(parse_semver_tag("not-a-version").is_none());
}

#[test]
fn release_is_newer_than_current_semver() {
    assert!(release_is_newer_than_current("v0.9.9", "v0.10.0"));
    assert!(release_is_newer_than_current("v1.2.9", "v1.3.0"));
    assert!(!release_is_newer_than_current("v1.0.0", "v1.0.0"));
    assert!(!release_is_newer_than_current("v2.0.0", "v1.9.9"));
}

#[test]
fn release_is_newer_than_current_pre_release() {
    assert!(release_is_newer_than_current("v1.0.0-beta", "v1.0.0"));
    assert!(!release_is_newer_than_current("v1.0.0", "v1.0.0-beta"));
}

#[test]
fn parse_sha256sum_line_accepts_gnu_format() {
    let line =
        "abcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcd  miao-rust-linux-amd64";
    let h = parse_sha256sum_line(line).unwrap();
    assert_eq!(h.len(), 64);
    assert!(h.starts_with("abcdabcd"));
}

#[test]
fn parse_sha256sum_line_accepts_star_filename() {
    let line =
        "abcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcdabcd *miao-rust-linux-amd64";
    assert!(parse_sha256sum_line(line).unwrap().starts_with("abcd"));
}

#[test]
fn stdout_version_matches_release_requires_miao_and_tag_or_version() {
    assert!(stdout_version_matches_release("miao v0.12.2\n", "v0.12.2"));
    assert!(stdout_version_matches_release(
        "miao-rust v1.0.0\n",
        "v1.0.0"
    ));
    assert!(!stdout_version_matches_release("other v1.0.0\n", "v1.0.0"));
}

#[test]
fn current_arch_asset_name_matches_supported_targets() {
    if cfg!(windows) {
        assert_eq!(current_arch_asset_name(), None);
    } else if cfg!(target_arch = "x86_64") {
        assert_eq!(current_arch_asset_name(), Some("miao-rust-linux-amd64"));
    } else if cfg!(target_arch = "aarch64") {
        assert_eq!(current_arch_asset_name(), Some("miao-rust-linux-arm64"));
    } else {
        assert_eq!(current_arch_asset_name(), None);
    }
}

#[tokio::test]
async fn version_info_skips_github_when_kernel_is_down() {
    let info = get_version_info(&app_state(Config::default())).await;
    assert_eq!(info.upgrade_supported, upgrade_supported());
    assert!(!info.has_update);
    assert!(info.latest.is_none());
    assert!(info.download_url.is_none());
}

#[test]
fn version_info_reports_update_without_download_when_upgrade_unsupported() {
    let release = GitHubRelease {
        tag_name: "v99.0.0".to_string(),
        assets: vec![GitHubAsset {
            name: "miao-windows-amd64-setup.exe".to_string(),
            browser_download_url: "https://example.com/setup.exe".to_string(),
            size: 1,
        }],
    };
    let info = version_info_from_release("v0.31.0".to_string(), &release, false);
    assert!(!info.upgrade_supported);
    assert!(info.has_update);
    assert_eq!(info.latest.as_deref(), Some("v99.0.0"));
    assert!(info.download_url.is_none());
}

#[test]
fn version_info_includes_linux_asset_url_when_upgrade_supported() {
    let release = GitHubRelease {
        tag_name: "v99.0.0".to_string(),
        assets: vec![GitHubAsset {
            name: "miao-rust-linux-amd64".to_string(),
            browser_download_url: "https://example.com/miao".to_string(),
            size: 1,
        }],
    };
    let info = version_info_from_release("v0.31.0".to_string(), &release, true);
    assert!(info.upgrade_supported);
    assert!(info.has_update);
    if cfg!(all(not(windows), target_arch = "x86_64")) {
        assert_eq!(
            info.download_url.as_deref(),
            Some("https://example.com/miao")
        );
    }
}
