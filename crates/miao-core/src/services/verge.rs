//! 从 clash-verge-rev 的 profiles.yaml 只读导入订阅。
//!
//! 数据面：profiles.yaml 的 items 里 `type: remote` 且带 url 的条目即订阅，
//! 与 miao 的 subs（Clash YAML 订阅 URL）同构。只读不写，解析宽松——
//! schema 演进、缺字段都不得致命，最坏结果是「未检测到」。

use std::path::PathBuf;

/// clash-verge-rev 的应用数据目录名（Tauri bundle identifier）。
const VERGE_DATA_DIR: &str = "io.github.clash-verge-rev.clash-verge-rev";
const PROFILES_FILE: &str = "profiles.yaml";

/// 一条可导入的订阅。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VergeSubscription {
    pub name: Option<String>,
    pub url: String,
}

#[derive(serde::Deserialize)]
struct VergeProfiles {
    items: Option<Vec<VergeProfileItem>>,
}

#[derive(serde::Deserialize)]
struct VergeProfileItem {
    #[serde(rename = "type")]
    item_type: Option<String>,
    name: Option<String>,
    url: Option<String>,
}

/// 宽松解析 profiles.yaml：只认 items/type/name/url 四个字段。
/// 文件整体非法、条目缺 url、非 remote 条目都安静跳过。
pub fn parse_profiles(yaml: &str) -> Vec<VergeSubscription> {
    let Ok(profiles) = yaml_serde::from_str::<VergeProfiles>(yaml) else {
        return vec![];
    };
    profiles
        .items
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            if item.item_type.as_deref() != Some("remote") {
                return None;
            }
            let url = item.url?.trim().to_string();
            if url.is_empty() {
                return None;
            }
            let name = item
                .name
                .map(|n| n.trim().to_string())
                .filter(|n| !n.is_empty());
            Some(VergeSubscription { name, url })
        })
        .collect()
}

/// 扫描本机的 clash-verge-rev 订阅；文件不存在/不可读返回 None。
pub async fn scan() -> Option<Vec<VergeSubscription>> {
    let path = profiles_path()?;
    let content = tokio::fs::read_to_string(path).await.ok()?;
    Some(parse_profiles(&content))
}

/// 解析 profiles.yaml 的位置；不存在返回 None。
#[cfg(windows)]
fn profiles_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let path = PathBuf::from(appdata)
        .join(VERGE_DATA_DIR)
        .join(PROFILES_FILE);
    path.is_file().then_some(path)
}

/// 解析 profiles.yaml 的位置；不存在返回 None。
#[cfg(unix)]
fn profiles_path() -> Option<PathBuf> {
    unix_candidates()
        .into_iter()
        .map(|home| {
            home.join(".local")
                .join("share")
                .join(VERGE_DATA_DIR)
                .join(PROFILES_FILE)
        })
        .find(|path| path.is_file())
}

/// Unix 候选 home 目录，按优先级：sudo 前的真实用户 > 进程 HOME > /home 扫描。
///
/// miao 以 root 运行（sudo 或 systemd），而 clash-verge-rev 的数据在普通用户
/// home 下；sudo 时 HOME 可能指向 /root，systemd 下则没有 SUDO_USER 可用。
#[cfg(unix)]
fn unix_candidates() -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = Vec::new();
    if let Some(home) = sudo_user_home() {
        homes.push(home);
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        homes.push(home);
    }
    homes.extend(home_dir_scan());
    // 去重保持优先级（sudo 用户与 HOME 通常相同）
    let mut seen = std::collections::HashSet::new();
    homes.retain(|h| seen.insert(h.clone()));
    homes
}

#[cfg(unix)]
fn sudo_user_home() -> Option<PathBuf> {
    let name = std::env::var("SUDO_USER").ok()?;
    let user = nix::unistd::User::from_name(&name).ok()??;
    Some(user.dir)
}

#[cfg(unix)]
fn home_dir_scan() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir("/home") else {
        return vec![];
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

/// 从给定 home 目录集合中解析 profiles.yaml（unix 测试注入点；
/// 唯一调用方是 cfg(unix) 测试，Windows 测试构建下会报死代码，故同门）。
#[cfg(all(test, unix))]
fn resolve_in_homes(homes: &[PathBuf]) -> Option<PathBuf> {
    homes
        .iter()
        .map(|home| {
            home.join(".local")
                .join("share")
                .join(VERGE_DATA_DIR)
                .join(PROFILES_FILE)
        })
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_items_with_urls() {
        let yaml = r#"
current: aaa
items:
  - uid: aaa
    type: remote
    name: 香港机场
    url: "https://example.com/sub?token=abc"
    file: aaa.yaml
    extra: {upload: 0, download: 123}
  - uid: bbb
    type: remote
    name: 备用
    url: "https://backup.example/feed"
  - uid: ccc
    type: local
    name: 本地配置
    file: ccc.yaml
  - uid: ddd
    type: merge
    file: Merge.yaml
"#;
        let subs = parse_profiles(yaml);
        assert_eq!(
            subs,
            vec![
                VergeSubscription {
                    name: Some("香港机场".to_string()),
                    url: "https://example.com/sub?token=abc".to_string(),
                },
                VergeSubscription {
                    name: Some("备用".to_string()),
                    url: "https://backup.example/feed".to_string(),
                },
            ]
        );
    }

    #[test]
    fn skips_items_without_url_and_trims_blanks() {
        let yaml = r#"
items:
  - uid: a
    type: remote
    name: "  "
    url: "  https://example.com/sub  "
  - uid: b
    type: remote
    name: 没链接
  - uid: c
    type: remote
    url: ""
"#;
        assert_eq!(
            parse_profiles(yaml),
            vec![VergeSubscription {
                name: None,
                url: "https://example.com/sub".to_string(),
            }]
        );
    }

    #[test]
    fn tolerates_garbage_and_missing_items() {
        assert!(parse_profiles("not: [valid: yaml: at: all").is_empty());
        assert!(parse_profiles("").is_empty());
        assert!(parse_profiles("current: only\n").is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn resolves_profiles_file_under_xdg_data_home() {
        let root = std::env::temp_dir().join(format!("miao-verge-test-{}", std::process::id()));
        let home = root.join("alice");
        let profiles = home
            .join(".local")
            .join("share")
            .join(VERGE_DATA_DIR)
            .join(PROFILES_FILE);
        std::fs::create_dir_all(profiles.parent().unwrap()).unwrap();
        std::fs::write(&profiles, "items: []\n").unwrap();

        assert_eq!(
            resolve_in_homes(std::slice::from_ref(&home)),
            Some(profiles)
        );
        assert_eq!(resolve_in_homes(&[root.join("bob")]), None);

        let _ = std::fs::remove_dir_all(&root);
    }
}
