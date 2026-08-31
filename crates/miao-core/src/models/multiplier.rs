use std::fmt::{Display, Formatter};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 节点计费倍率，内部用千分之一保存，避免用浮点数参与配置比较和持久化。
/// API / YAML 使用去掉尾零的十进制字符串（例如 `1`、`2.5`、`6.25`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeMultiplier(u32);

impl NodeMultiplier {
    const SCALE: u32 = 1_000;
    const MAX_WHOLE: u32 = 10_000;
    pub const ONE: Self = Self(Self::SCALE);

    pub fn parse(raw: &str) -> Option<Self> {
        let raw = raw.trim();
        let raw = raw
            .strip_suffix(['x', 'X', '×', '倍'])
            .unwrap_or(raw)
            .trim();
        if raw.is_empty() || raw.starts_with('-') || raw.starts_with('+') {
            return None;
        }

        let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
        if whole.is_empty()
            || !whole.bytes().all(|byte| byte.is_ascii_digit())
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
            || fraction.len() > 3
        {
            return None;
        }
        let whole: u32 = whole.parse().ok()?;
        if whole > Self::MAX_WHOLE {
            return None;
        }
        let fraction = match fraction.len() {
            0 => 0,
            1 => fraction.parse::<u32>().ok()? * 100,
            2 => fraction.parse::<u32>().ok()? * 10,
            3 => fraction.parse::<u32>().ok()?,
            _ => return None,
        };
        let scaled = whole.checked_mul(Self::SCALE)?.checked_add(fraction)?;
        (scaled > 0 && scaled <= Self::MAX_WHOLE * Self::SCALE).then_some(Self(scaled))
    }

    pub fn as_config_value(self) -> String {
        let whole = self.0 / Self::SCALE;
        let fraction = self.0 % Self::SCALE;
        if fraction == 0 {
            return whole.to_string();
        }
        let mut fraction = format!("{fraction:03}");
        while fraction.ends_with('0') {
            fraction.pop();
        }
        format!("{whole}.{fraction}")
    }
}

impl Display for NodeMultiplier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_config_value())
    }
}

impl Serialize for NodeMultiplier {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_config_value())
    }
}

impl<'de> Deserialize<'de> for NodeMultiplier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            String(String),
            Integer(u64),
            Float(f64),
        }

        let raw = match Repr::deserialize(deserializer)? {
            Repr::String(value) => value,
            Repr::Integer(value) => value.to_string(),
            Repr::Float(value) => value.to_string(),
        };
        Self::parse(&raw)
            .ok_or_else(|| serde::de::Error::custom("倍率必须是大于 0 且不超过 10000 的十进制数"))
    }
}

static SUFFIX_MULTIPLIER: LazyLock<Regex> = LazyLock::new(|| {
    // 数字部分故意比 NodeMultiplier::parse 宽松：只要名字明确写了倍率标记，
    // 超范围或精度过高都必须识别为“无效”，不能伪装成未标注的 1x。
    Regex::new(r"(?i)(?:^|[^0-9.])([0-9]+(?:\.[0-9]+)?)\s*(?:x|×|倍)")
        .expect("valid multiplier regex")
});
static PREFIX_MULTIPLIER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"倍率\s*[:：]?\s*([0-9]+(?:\.[0-9]+)?)(?:$|[^0-9.])")
        .expect("valid multiplier regex")
});

/// 从常见节点名提取倍率。未标注的节点按 1x 处理；明确写了但无法解析的
/// 倍率返回 None，调用方应将其排除出受倍率限制的自动候选。
pub fn node_multiplier(name: &str) -> Option<NodeMultiplier> {
    match SUFFIX_MULTIPLIER
        .captures(name)
        .or_else(|| PREFIX_MULTIPLIER.captures(name))
    {
        Some(captures) => captures
            .get(1)
            .and_then(|value| NodeMultiplier::parse(value.as_str())),
        None => Some(NodeMultiplier::ONE),
    }
}

#[cfg(test)]
mod tests {
    use super::{node_multiplier, NodeMultiplier};

    #[test]
    fn parses_and_formats_decimal_multipliers() {
        for (raw, expected) in [
            ("1", "1"),
            ("1.0", "1"),
            ("1.3x", "1.3"),
            ("2.40", "2.4"),
            ("6.25×", "6.25"),
        ] {
            assert_eq!(
                NodeMultiplier::parse(raw)
                    .expect("valid multiplier")
                    .as_config_value(),
                expected
            );
        }
        assert!(NodeMultiplier::parse("0").is_none());
        assert!(NodeMultiplier::parse("1.2345").is_none());
        assert!(NodeMultiplier::parse("not-a-number").is_none());
    }

    #[test]
    fn extracts_common_node_name_formats_and_defaults_to_one() {
        assert_eq!(
            node_multiplier("日本[18x]-联移专线").unwrap().to_string(),
            "18"
        );
        assert_eq!(
            node_multiplier("日本 [6.5X] 豪华").unwrap().to_string(),
            "6.5"
        );
        assert_eq!(
            node_multiplier("香港 2.4倍 高端").unwrap().to_string(),
            "2.4"
        );
        assert_eq!(
            node_multiplier("新加坡-倍率：1.3").unwrap().to_string(),
            "1.3"
        );
        assert_eq!(node_multiplier("美国普通节点").unwrap().to_string(), "1");
        assert_eq!(node_multiplier("超范围 10001x 标记"), None);
        assert_eq!(node_multiplier("错误的 1.2345x 标记"), None);
        assert_eq!(node_multiplier("错误的 倍率：1.2345 标记"), None);
    }

    #[test]
    fn serde_accepts_yaml_numbers_and_writes_canonical_strings() {
        let parsed: NodeMultiplier = yaml_serde::from_str("2.5").unwrap();
        assert_eq!(parsed.to_string(), "2.5");
        let yaml = yaml_serde::to_string(&parsed).unwrap();
        assert!(yaml.contains("2.5"));
    }
}
