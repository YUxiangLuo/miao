use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreferenceStore {
    Runtime,
    Working,
    Persistent,
}

fn store_for(windows: bool, openwrt_like: bool, pid1: &str) -> PreferenceStore {
    if windows {
        PreferenceStore::Persistent
    } else if openwrt_like || pid1.trim() != "systemd" {
        PreferenceStore::Runtime
    } else {
        PreferenceStore::Working
    }
}

pub(super) fn preference_store() -> PreferenceStore {
    let os_release = std::fs::read_to_string("/etc/os-release").ok();
    let openwrt = openwrt_like_from_paths(|path| Path::new(path).exists(), os_release.as_deref());
    let pid1 = std::fs::read_to_string("/proc/1/comm").unwrap_or_default();
    store_for(cfg!(windows), openwrt, &pid1)
}

fn os_release_looks_like_openwrt(content: &str) -> bool {
    content.lines().any(|line| {
        let Some((key, value)) = line.trim().split_once('=') else {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistence_policy_preserves_windows_systemd_and_flash_safe_defaults() {
        for openwrt in [true, false] {
            for pid1 in ["systemd", "procd", "", "init"] {
                assert_eq!(store_for(true, openwrt, pid1), PreferenceStore::Persistent);
                assert_eq!(
                    store_for(false, openwrt, pid1),
                    if !openwrt && pid1 == "systemd" {
                        PreferenceStore::Working
                    } else {
                        PreferenceStore::Runtime
                    }
                );
            }
        }
    }
    #[test]
    fn detects_openwrt_even_without_the_release_marker() {
        for marker in [
            "/etc/openwrt_release",
            "/etc/openwrt_version",
            "/sbin/procd",
        ] {
            assert!(openwrt_like_from_paths(|path| path == marker, None));
        }
        for text in [
            "ID=openwrt",
            "ID=\"immortalwrt\"",
            "ID='istoreos'",
            "OPENWRT_BOARD=x",
        ] {
            assert!(openwrt_like_from_paths(|_| false, Some(text)));
        }
        assert!(!openwrt_like_from_paths(|_| false, Some("ID=arch")));
        assert!(!openwrt_like_from_paths(|_| false, None));
    }
}
