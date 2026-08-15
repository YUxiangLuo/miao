use serde::Serialize;

use super::geo::GeoLocation;

#[derive(Clone, Debug, Serialize)]
pub struct ClientEntity {
    #[serde(rename = "type")]
    pub entity_type: &'static str,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo: Option<GeoLocation>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProxyEntity {
    #[serde(rename = "type")]
    pub entity_type: &'static str,
    pub name: String,
    pub server: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo: Option<GeoLocation>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DestinationEntity {
    #[serde(rename = "type")]
    pub entity_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    pub ip: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub geo: Option<GeoLocation>,
}

#[derive(Clone, Debug, Serialize)]
pub struct NetworkFlow {
    pub id: String,
    pub destination: DestinationEntity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyEntity>,
    pub network: String,
    pub upload_speed: f64,
    pub download_speed: f64,
    pub upload_total: u64,
    pub download_total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MapSnapshot {
    pub client: ClientEntity,
    pub proxies: Vec<ProxyEntity>,
    pub flows: Vec<NetworkFlow>,
}

#[cfg(test)]
mod tests {
    use super::{ClientEntity, MapSnapshot};

    #[test]
    fn snapshot_serializes_entity_type_fields() {
        let snapshot = MapSnapshot {
            client: ClientEntity {
                entity_type: "client",
                name: "This Device".into(),
                geo: None,
            },
            proxies: vec![],
            flows: vec![],
        };

        let value = serde_json::to_value(snapshot).unwrap();
        assert_eq!(value["client"]["type"], "client");
        assert_eq!(value["client"]["name"], "This Device");
        assert!(value["proxies"].as_array().unwrap().is_empty());
    }
}
