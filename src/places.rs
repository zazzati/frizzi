use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::{sync::LazyLock, time::Duration};

use crate::observability::upstream_class;
use crate::{bad, db, locale, rate_limit, ApiError, ApiResult};

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LocationGeo {
    latitude: f64,
    longitude: f64,
    kind: LocationKind,
    attribution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    place_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum LocationKind {
    Address,
    Venue,
    Approximate,
}

impl LocationGeo {
    pub fn directions_destination(&self) -> Option<String> {
        (self.kind != LocationKind::Approximate)
            .then(|| format!("{},{}", self.latitude, self.longitude))
    }

    pub async fn venue_website(&self) -> Option<reqwest::Url> {
        if self.kind != LocationKind::Venue {
            return None;
        }
        let id = self.place_id.as_deref()?;
        let key = std::env::var("GEOAPIFY_API_KEY").ok()?;
        if key.trim().is_empty() {
            return None;
        }
        let data = CLIENT
            .get("https://api.geoapify.com/v2/place-details")
            .query(&[("id", id), ("apiKey", key.trim())])
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .inspect_err(|error| log_failure("venue_details", error))
            .ok()?
            .json::<serde_json::Value>()
            .await
            .inspect_err(|error| log_failure("venue_details", error))
            .ok()?;
        let details = data["features"]
            .as_array()?
            .iter()
            .map(|feature| &feature["properties"])
            .find(|properties| properties["feature_type"] == "details")?;
        let url = reqwest::Url::parse(details["website"].as_str()?).ok()?;
        (matches!(url.scheme(), "https" | "http")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.as_str().len() <= 1024)
            .then_some(url)
    }

    pub fn with_coordinates(&self, label: &str) -> String {
        format!("{label} ({:.5}, {:.5})", self.latitude, self.longitude)
    }

    pub fn validate(&self) -> Result<(), ApiError> {
        if !(-90.0..=90.0).contains(&self.latitude)
            || !(-180.0..=180.0).contains(&self.longitude)
            || self
                .attribution
                .as_ref()
                .is_some_and(|text| text.chars().count() > 255)
            || self.place_id.as_deref().is_some_and(|id| {
                id.is_empty() || id.len() > 1024 || !id.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Err(bad());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct Search {
    text: String,
    #[serde(default = "crate::default_locale")]
    locale: String,
}

#[derive(Deserialize)]
struct ProviderResponse {
    results: Vec<ProviderPlace>,
}

#[derive(Deserialize)]
struct ProviderPlace {
    formatted: String,
    name: Option<String>,
    address_line1: Option<String>,
    result_type: Option<String>,
    street: Option<String>,
    housenumber: Option<String>,
    lat: f64,
    lon: f64,
    datasource: Option<DataSource>,
    place_id: Option<String>,
}

#[derive(Deserialize)]
struct DataSource {
    attribution: Option<String>,
}

#[derive(Serialize)]
struct Suggestion {
    label: String,
    geo: LocationGeo,
}

impl ProviderPlace {
    fn suggestion(self) -> Option<Suggestion> {
        let address = self.formatted.trim();
        let venue = self.result_type.as_deref() == Some("amenity");
        let name = self
            .name
            .as_deref()
            .or(self.address_line1.as_deref())
            .unwrap_or("")
            .trim();
        let label = if venue
            && !name.is_empty()
            && !address.to_lowercase().starts_with(&name.to_lowercase())
        {
            format!("{name}, {address}")
        } else {
            address.to_string()
        };
        let kind = if venue {
            LocationKind::Venue
        } else if self.result_type.as_deref() == Some("building")
            && self.street.is_some()
            && self.housenumber.is_some()
        {
            LocationKind::Address
        } else {
            LocationKind::Approximate
        };
        let geo = LocationGeo {
            latitude: self.lat,
            longitude: self.lon,
            kind,
            place_id: self.place_id,
            attribution: self
                .datasource
                .and_then(|source| source.attribution)
                .filter(|text| !text.trim().is_empty() && text.chars().count() <= 255),
        };
        (crate::valid(&label, 255) && geo.validate().is_ok()).then_some(Suggestion { label, geo })
    }
}

pub async fn suggest(State(pool): State<PgPool>, Json(search): Json<Search>) -> ApiResult {
    let text = search.text.trim();
    if !(3..=255).contains(&text.chars().count()) || text.contains("://") {
        return Err(bad());
    }
    let Some(key) = std::env::var("GEOAPIFY_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
    else {
        return Ok(Json(json!({"enabled": false, "suggestions": []})));
    };

    // 120 lookups per hour keeps the public endpoint within the free daily allowance.
    let mut tx = pool.begin().await.map_err(db)?;
    rate_limit(&mut tx, "geoapify:autocomplete", 120).await?;
    tx.commit().await.map_err(db)?;

    let response = CLIENT
        .get("https://api.geoapify.com/v1/geocode/autocomplete")
        .query(&[
            ("text", text),
            ("lang", locale(&search.locale)),
            ("limit", "5"),
            ("format", "json"),
            ("apiKey", key.trim()),
        ])
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .inspect_err(|error| log_failure("autocomplete", error))
        .map_err(|_| unavailable())?
        .json::<ProviderResponse>()
        .await
        .inspect_err(|error| log_failure("autocomplete", error))
        .map_err(|_| unavailable())?;
    let suggestions: Vec<_> = response
        .results
        .into_iter()
        .filter_map(ProviderPlace::suggestion)
        .take(5)
        .collect();
    Ok(Json(json!({"enabled": true, "suggestions": suggestions})))
}

fn unavailable() -> ApiError {
    crate::error(axum::http::StatusCode::BAD_GATEWAY, "location_unavailable")
}

fn log_failure(operation: &'static str, error: &reqwest::Error) {
    tracing::warn!(
        event = "geoapify_unavailable",
        operation,
        reason = upstream_class(error),
        upstream_status = error.status().map(|status| status.as_u16())
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn street_only_is_not_presented_as_a_precise_address() {
        let street = serde_json::from_value::<ProviderPlace>(json!({
            "formatted": "Via Roma, Torino, Italia",
            "result_type": "street",
            "street": "Via Roma",
            "lat": 45.06,
            "lon": 7.68
        }))
        .unwrap();
        assert_eq!(
            street.suggestion().unwrap().geo.kind,
            LocationKind::Approximate
        );
    }

    #[test]
    fn named_venue_includes_address_and_data_source() {
        let venue = serde_json::from_value::<ProviderPlace>(json!({
            "formatted": "Piazza San Marco 57, Venezia, Italia",
            "name": "Caffè Florian",
            "result_type": "amenity",
            "lat": 45.43,
            "lon": 12.33,
            "datasource": { "attribution": "© OpenStreetMap contributors" }
        }))
        .unwrap()
        .suggestion()
        .unwrap();
        assert_eq!(
            venue.label,
            "Caffè Florian, Piazza San Marco 57, Venezia, Italia"
        );
        assert_eq!(venue.geo.kind, LocationKind::Venue);
        assert_eq!(
            venue.geo.attribution.as_deref(),
            Some("© OpenStreetMap contributors")
        );
    }

    #[test]
    fn geo_validation_bounds_coordinates_attribution_and_place_id() {
        let check = |patch: serde_json::Value| {
            let mut geo = json!({"latitude": 45.0, "longitude": 7.0, "kind": "venue"});
            geo.as_object_mut()
                .unwrap()
                .extend(patch.as_object().unwrap().clone());
            serde_json::from_value::<LocationGeo>(geo)
                .unwrap()
                .validate()
                .is_ok()
        };
        assert!(check(json!({})));
        assert!(check(json!({"latitude": 90.0, "longitude": -180.0})));
        assert!(check(json!({"latitude": -90.0, "longitude": 180.0})));
        assert!(!check(json!({"latitude": 90.01})));
        assert!(!check(json!({"longitude": -180.01})));
        // Attribution is limited by characters, not bytes.
        assert!(check(json!({"attribution": "é".repeat(255)})));
        assert!(!check(json!({"attribution": "a".repeat(256)})));
        assert!(check(json!({"place_id": "51a3f0c2DE"})));
        assert!(check(json!({"place_id": "a".repeat(1024)})));
        assert!(!check(json!({"place_id": ""})));
        assert!(!check(json!({"place_id": "a".repeat(1025)})));
        assert!(!check(json!({"place_id": "abc/../x"})));
    }
}
