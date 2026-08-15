//! 从节点名称推导地理位置。
//!
//! 机场订阅节点大多是中转（IEPL 等）入口域名，按服务器地址做 GeoIP 只能得到
//! 入口位置甚至解析失败；而节点名（「🇭🇰 香港W01」「免费-日本1」）本身标注了
//! 出口地区，是更可靠的信息源。中文关键字优先于国旗 emoji（如「🇨🇳 台湾W01」）。

use crate::models::GeoLocation;

struct CountryGeo {
    code: &'static str,
    label: &'static str,
    latitude: f64,
    longitude: f64,
    keywords: &'static [&'static str],
}

// 常见机场节点地区；坐标取代表城市（多为首都/枢纽），只用于地图展示。
const COUNTRIES: &[CountryGeo] = &[
    CountryGeo {
        code: "HK",
        label: "Hong Kong",
        latitude: 22.32,
        longitude: 114.17,
        keywords: &["香港", "Hong Kong", "HongKong"],
    },
    CountryGeo {
        code: "MO",
        label: "Macau",
        latitude: 22.20,
        longitude: 113.55,
        keywords: &["澳门", "Macau", "Macao"],
    },
    CountryGeo {
        code: "TW",
        label: "Taiwan",
        latitude: 25.03,
        longitude: 121.56,
        keywords: &["台湾", "台北", "Taiwan", "Taipei"],
    },
    CountryGeo {
        code: "JP",
        label: "Japan",
        latitude: 35.68,
        longitude: 139.69,
        keywords: &["日本", "东京", "大阪", "Japan", "Tokyo", "Osaka"],
    },
    CountryGeo {
        code: "KR",
        label: "South Korea",
        latitude: 37.57,
        longitude: 126.98,
        keywords: &["韩国", "首尔", "Korea", "Seoul"],
    },
    CountryGeo {
        code: "SG",
        label: "Singapore",
        latitude: 1.35,
        longitude: 103.82,
        keywords: &["新加坡", "狮城", "Singapore"],
    },
    CountryGeo {
        code: "MY",
        label: "Malaysia",
        latitude: 3.139,
        longitude: 101.69,
        keywords: &["马来西亚", "Malaysia"],
    },
    CountryGeo {
        code: "TH",
        label: "Thailand",
        latitude: 13.76,
        longitude: 100.50,
        keywords: &["泰国", "曼谷", "Thailand", "Bangkok"],
    },
    CountryGeo {
        code: "VN",
        label: "Vietnam",
        latitude: 21.03,
        longitude: 105.85,
        keywords: &["越南", "Vietnam"],
    },
    CountryGeo {
        code: "PH",
        label: "Philippines",
        latitude: 14.60,
        longitude: 120.98,
        keywords: &["菲律宾", "Philippines"],
    },
    CountryGeo {
        code: "ID",
        label: "Indonesia",
        latitude: -6.21,
        longitude: 106.85,
        keywords: &["印尼", "印度尼西亚", "Indonesia"],
    },
    CountryGeo {
        code: "IN",
        label: "India",
        latitude: 28.61,
        longitude: 77.21,
        keywords: &["印度", "India"],
    },
    CountryGeo {
        code: "TR",
        label: "Turkey",
        latitude: 39.93,
        longitude: 32.86,
        keywords: &["土耳其", "Turkey"],
    },
    CountryGeo {
        code: "AE",
        label: "UAE",
        latitude: 25.20,
        longitude: 55.27,
        keywords: &["阿联酋", "迪拜", "UAE", "Dubai"],
    },
    CountryGeo {
        code: "RU",
        label: "Russia",
        latitude: 55.76,
        longitude: 37.62,
        keywords: &["俄罗斯", "莫斯科", "Russia", "Moscow"],
    },
    CountryGeo {
        code: "UA",
        label: "Ukraine",
        latitude: 50.45,
        longitude: 30.52,
        keywords: &["乌克兰", "Ukraine"],
    },
    CountryGeo {
        code: "GB",
        label: "United Kingdom",
        latitude: 51.51,
        longitude: -0.13,
        keywords: &["英国", "伦敦", "UK", "Britain", "London"],
    },
    CountryGeo {
        code: "DE",
        label: "Germany",
        latitude: 52.52,
        longitude: 13.40,
        keywords: &["德国", "法兰克福", "Germany", "Frankfurt"],
    },
    CountryGeo {
        code: "FR",
        label: "France",
        latitude: 48.86,
        longitude: 2.35,
        keywords: &["法国", "巴黎", "France", "Paris"],
    },
    CountryGeo {
        code: "NL",
        label: "Netherlands",
        latitude: 52.37,
        longitude: 4.90,
        keywords: &["荷兰", "Netherlands", "Amsterdam"],
    },
    CountryGeo {
        code: "IT",
        label: "Italy",
        latitude: 41.90,
        longitude: 12.50,
        keywords: &["意大利", "Italy"],
    },
    CountryGeo {
        code: "ES",
        label: "Spain",
        latitude: 40.42,
        longitude: -3.70,
        keywords: &["西班牙", "Spain"],
    },
    CountryGeo {
        code: "CH",
        label: "Switzerland",
        latitude: 47.38,
        longitude: 8.54,
        keywords: &["瑞士", "Switzerland", "Zurich"],
    },
    CountryGeo {
        code: "SE",
        label: "Sweden",
        latitude: 59.33,
        longitude: 18.07,
        keywords: &["瑞典", "Sweden"],
    },
    CountryGeo {
        code: "NO",
        label: "Norway",
        latitude: 59.91,
        longitude: 10.75,
        keywords: &["挪威", "Norway"],
    },
    CountryGeo {
        code: "FI",
        label: "Finland",
        latitude: 60.17,
        longitude: 24.94,
        keywords: &["芬兰", "Finland"],
    },
    CountryGeo {
        code: "PL",
        label: "Poland",
        latitude: 52.23,
        longitude: 21.01,
        keywords: &["波兰", "Poland"],
    },
    CountryGeo {
        code: "US",
        label: "United States",
        latitude: 38.90,
        longitude: -77.04,
        keywords: &[
            "美国",
            "纽约",
            "洛杉矶",
            "旧金山",
            "圣何塞",
            "西雅图",
            "USA",
            "United States",
            "Los Angeles",
            "San Jose",
            "Seattle",
            "New York",
            "Silicon",
        ],
    },
    CountryGeo {
        code: "CA",
        label: "Canada",
        latitude: 45.42,
        longitude: -75.70,
        keywords: &[
            "加拿大",
            "多伦多",
            "温哥华",
            "Canada",
            "Toronto",
            "Vancouver",
        ],
    },
    CountryGeo {
        code: "MX",
        label: "Mexico",
        latitude: 19.43,
        longitude: -99.13,
        keywords: &["墨西哥", "Mexico"],
    },
    CountryGeo {
        code: "BR",
        label: "Brazil",
        latitude: -23.55,
        longitude: -46.63,
        keywords: &["巴西", "圣保罗", "Brazil"],
    },
    CountryGeo {
        code: "AR",
        label: "Argentina",
        latitude: -34.60,
        longitude: -58.38,
        keywords: &["阿根廷", "Argentina"],
    },
    CountryGeo {
        code: "AU",
        label: "Australia",
        latitude: -33.87,
        longitude: 151.21,
        keywords: &["澳大利亚", "澳洲", "悉尼", "Australia", "Sydney"],
    },
    CountryGeo {
        code: "NZ",
        label: "New Zealand",
        latitude: -36.85,
        longitude: 174.76,
        keywords: &["新西兰", "New Zealand"],
    },
    CountryGeo {
        code: "ZA",
        label: "South Africa",
        latitude: -26.20,
        longitude: 28.05,
        keywords: &["南非", "South Africa"],
    },
    CountryGeo {
        code: "CN",
        label: "China",
        latitude: 39.90,
        longitude: 116.40,
        keywords: &[
            "中国", "北京", "上海", "广州", "深圳", "China", "Beijing", "Shanghai",
        ],
    },
];

// 国旗 emoji 由两个区域指示符（U+1F1E6..=U+1F1FF）组成，对应 ISO 3166-1 alpha-2
fn flag_country_code(name: &str) -> Option<String> {
    let mut indicators = name.chars().filter_map(|ch| {
        let code = ch as u32;
        (0x1F1E6..=0x1F1FF)
            .contains(&code)
            .then(|| (b'A' + (code - 0x1F1E6) as u8) as char)
    });
    let first = indicators.next()?;
    let second = indicators.next()?;
    Some(format!("{first}{second}"))
}

fn geo_for_code(code: &str) -> Option<&'static CountryGeo> {
    COUNTRIES.iter().find(|country| country.code == code)
}

/// 从节点名推导地理位置；无法识别时返回 None（调用方再回退到 GeoIP 查询）。
pub fn geo_from_node_name(name: &str) -> Option<GeoLocation> {
    // 中文/英文关键字优先：覆盖「🇨🇳 台湾W01」这类旗与地区不一致的情况
    for country in COUNTRIES {
        if country
            .keywords
            .iter()
            .any(|keyword| name.contains(keyword))
        {
            return Some(location_from(country));
        }
    }
    let code = flag_country_code(name)?;
    let country = geo_for_code(&code)?;
    Some(location_from(country))
}

fn location_from(country: &CountryGeo) -> GeoLocation {
    GeoLocation {
        country: Some(country.label.to_string()),
        country_code: Some(country.code.to_string()),
        city: None,
        latitude: Some(country.latitude),
        longitude: Some(country.longitude),
    }
}

#[cfg(test)]
mod tests {
    use super::geo_from_node_name;

    #[test]
    fn parses_flag_emoji() {
        let geo = geo_from_node_name("🇭🇰 W01 中继").unwrap();
        assert_eq!(geo.country_code.as_deref(), Some("HK"));
        assert_eq!(geo.latitude, Some(22.32));
    }

    #[test]
    fn chinese_keyword_wins_over_flag() {
        let geo = geo_from_node_name("🇨🇳 台湾W01 | IEPL").unwrap();
        assert_eq!(geo.country_code.as_deref(), Some("TW"));
    }

    #[test]
    fn keyword_without_flag_works() {
        let geo = geo_from_node_name("免费-日本1-Ver.7").unwrap();
        assert_eq!(geo.country_code.as_deref(), Some("JP"));
    }

    #[test]
    fn unknown_names_return_none() {
        assert!(geo_from_node_name("Deguo").is_none());
        assert!(geo_from_node_name("vps-5-78-217-242").is_none());
        assert!(geo_from_node_name("🚀 极速专线").is_none());
    }

    #[test]
    fn unsupported_flag_returns_none() {
        // 🇮🇸 冰岛不在表里
        assert!(geo_from_node_name("🇮🇸 节点").is_none());
    }
}
