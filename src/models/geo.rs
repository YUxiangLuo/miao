use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoLocation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
}

impl GeoLocation {
    pub fn has_coordinates(&self) -> bool {
        self.latitude.is_some() && self.longitude.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::GeoLocation;

    #[test]
    fn has_coordinates_requires_both_values() {
        let geo = GeoLocation {
            country: Some("Japan".into()),
            country_code: Some("JP".into()),
            city: Some("Tokyo".into()),
            latitude: Some(35.6),
            longitude: None,
        };

        assert!(!geo.has_coordinates());
    }
}
