use crate::models::{NodeSelect, Region};

/// 按节点 tag 判断是否属于指定地区。匹配中文名、国旗 emoji、机场码和独立英文代号。
pub fn node_matches_region(tag: &str, region: Region) -> bool {
    if tag.contains(region_flag(region)) {
        return true;
    }
    if region_cjk(region)
        .iter()
        .any(|keyword| tag.contains(keyword))
    {
        return true;
    }

    let tokens = tokenize(tag);
    if tokens
        .iter()
        .any(|token| region_codes(region).contains(&token.as_str()))
    {
        return true;
    }
    if match_compound_tokens(&tokens, region) {
        return true;
    }

    let compact = compact_ascii(tag);
    region_compact_names(region)
        .iter()
        .any(|name| compact.contains(name))
}

#[cfg(test)]
pub fn resolve_node_select(select: NodeSelect, names: &[String]) -> NodeSelect {
    match select.region() {
        Some(region) if names.iter().any(|name| node_matches_region(name, region)) => select,
        Some(_) => NodeSelect::Manual,
        None => NodeSelect::Manual,
    }
}

#[cfg(test)]
pub fn group_member_names(select: NodeSelect, names: &[String]) -> Vec<String> {
    match select.region() {
        Some(region) => names
            .iter()
            .filter(|name| node_matches_region(name, region))
            .cloned()
            .collect(),
        None => names.to_vec(),
    }
}

/// 秒开缓存能否直接对应 yaml 里的 node_select：类型一致，最快模式还要求组成员都属于该地区。
pub fn runtime_config_matches_node_select(
    config_json: &serde_json::Value,
    select: NodeSelect,
) -> bool {
    let Some(proxy) = config_json
        .get("outbounds")
        .and_then(|outbounds| outbounds.as_array())
        .and_then(|outbounds| {
            outbounds
                .iter()
                .find(|outbound| outbound.get("tag").and_then(|tag| tag.as_str()) == Some("proxy"))
        })
    else {
        return false;
    };
    let ty = proxy
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    match select.region() {
        None => ty == "selector",
        Some(region) => {
            if ty != "urltest" {
                return false;
            }
            let Some(members) = proxy.get("outbounds").and_then(|value| value.as_array()) else {
                return false;
            };
            let names: Vec<&str> = members.iter().filter_map(|item| item.as_str()).collect();
            !names.is_empty() && names.iter().all(|name| node_matches_region(name, region))
        }
    }
}

fn region_flag(region: Region) -> &'static str {
    match region {
        Region::Hk => "🇭🇰",
        Region::Jp => "🇯🇵",
        Region::Tw => "🇹🇼",
        Region::Sg => "🇸🇬",
        Region::Us => "🇺🇸",
    }
}

fn region_cjk(region: Region) -> &'static [&'static str] {
    match region {
        Region::Hk => &["香港"],
        Region::Jp => &["日本", "东京", "東京", "大阪"],
        Region::Tw => &["台湾", "台灣", "台北"],
        Region::Sg => &["新加坡"],
        Region::Us => &[
            "美国",
            "美國",
            "洛杉矶",
            "洛杉磯",
            "旧金山",
            "舊金山",
            "纽约",
            "紐約",
            "西雅图",
            "西雅圖",
            "芝加哥",
            "达拉斯",
            "達拉斯",
        ],
    }
}

fn region_codes(region: Region) -> &'static [&'static str] {
    match region {
        Region::Hk => &["hk", "hkg"],
        Region::Jp => &["jp", "jpn", "tyo", "nrt", "hnd", "kix"],
        Region::Tw => &["tw", "twn", "tpe"],
        Region::Sg => &["sg", "sgp", "sin"],
        Region::Us => &["us", "usa", "lax", "sfo", "nyc", "iad"],
    }
}

fn region_compact_names(region: Region) -> &'static [&'static str] {
    match region {
        Region::Hk => &["hongkong"],
        Region::Jp => &["japan", "tokyo", "osaka"],
        Region::Tw => &["taiwan", "taipei"],
        Region::Sg => &["singapore"],
        Region::Us => &[
            "america",
            "unitedstates",
            "losangeles",
            "sanfrancisco",
            "newyork",
        ],
    }
}

fn match_compound_tokens(tokens: &[String], region: Region) -> bool {
    let pairs: &[&[&str]] = match region {
        Region::Hk => &[&["hong", "kong"]],
        Region::Us => &[&["united", "states"], &["los", "angeles"], &["new", "york"]],
        _ => return false,
    };
    tokens.windows(2).any(|window| {
        pairs
            .iter()
            .any(|pair| window[0] == pair[0] && window[1] == pair[1])
    })
}

fn tokenize(tag: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut last_kind = None;
    for ch in tag.chars() {
        let kind = if ch.is_ascii_alphabetic() {
            Some(1u8)
        } else if ch.is_ascii_digit() {
            Some(2u8)
        } else {
            None
        };
        match kind {
            Some(kind) if last_kind == Some(kind) => current.push(ch.to_ascii_lowercase()),
            Some(kind) => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                current.push(ch.to_ascii_lowercase());
                last_kind = Some(kind);
            }
            None => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
                last_kind = None;
            }
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn compact_ascii(tag: &str) -> String {
    tag.chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        group_member_names, node_matches_region, resolve_node_select,
        runtime_config_matches_node_select,
    };
    use crate::models::{NodeSelect, Region};
    use serde_json::json;

    #[test]
    fn matches_cjk_flag_and_code_tokens() {
        assert!(node_matches_region("🇭🇰 香港 01", Region::Hk));
        assert!(node_matches_region("HK-01", Region::Hk));
        assert!(node_matches_region("Hong Kong BGP", Region::Hk));
        assert!(node_matches_region("日本-东京", Region::Jp));
        assert!(node_matches_region("JP-NRT", Region::Jp));
        assert!(node_matches_region("台灣 台北", Region::Tw));
        assert!(node_matches_region("新加坡 SG", Region::Sg));
        assert!(node_matches_region("美国洛杉矶", Region::Us));
        assert!(node_matches_region("US-LAX-01", Region::Us));
    }

    #[test]
    fn rejects_false_friends() {
        assert!(!node_matches_region("AUS-01", Region::Us));
        assert!(!node_matches_region("Australia", Region::Us));
        assert!(!node_matches_region("NETWORK-01", Region::Tw));
        assert!(!node_matches_region("PLUS", Region::Us));
        assert!(!node_matches_region("JPEG-relay", Region::Jp));
        assert!(!node_matches_region("日本节点", Region::Hk));
    }

    #[test]
    fn resolve_falls_back_when_region_empty() {
        let names = vec!["日本-01".to_string(), "新加坡-02".to_string()];
        assert_eq!(
            resolve_node_select(NodeSelect::Fastest(Region::Hk), &names),
            NodeSelect::Manual
        );
        assert_eq!(
            resolve_node_select(NodeSelect::Fastest(Region::Jp), &names),
            NodeSelect::Fastest(Region::Jp)
        );
        assert_eq!(
            group_member_names(NodeSelect::Fastest(Region::Jp), &names),
            vec!["日本-01".to_string()]
        );
    }

    #[test]
    fn cache_match_requires_type_and_region_members() {
        let selector = json!({
            "outbounds": [{"type": "selector", "tag": "proxy", "outbounds": ["香港-01", "日本-01"]}]
        });
        let urltest_hk = json!({
            "outbounds": [{"type": "urltest", "tag": "proxy", "outbounds": ["香港-01", "HK-02"]}]
        });
        let urltest_mixed = json!({
            "outbounds": [{"type": "urltest", "tag": "proxy", "outbounds": ["香港-01", "日本-01"]}]
        });

        assert!(runtime_config_matches_node_select(
            &selector,
            NodeSelect::Manual
        ));
        assert!(!runtime_config_matches_node_select(
            &selector,
            NodeSelect::Fastest(Region::Hk)
        ));
        assert!(runtime_config_matches_node_select(
            &urltest_hk,
            NodeSelect::Fastest(Region::Hk)
        ));
        assert!(!runtime_config_matches_node_select(
            &urltest_mixed,
            NodeSelect::Fastest(Region::Hk)
        ));
        assert!(!runtime_config_matches_node_select(
            &urltest_hk,
            NodeSelect::Manual
        ));
    }
}
