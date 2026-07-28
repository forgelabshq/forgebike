//! Google Places API client (legacy Places Details endpoint).
//!
//! ## Endpoint
//! ```text
//! GET https://maps.googleapis.com/maps/api/place/details/json
//!     ?place_id={place_id}&fields=reviews&key={api_key}
//! ```
//!
//! Returns up to 5 reviews per place in the free tier.
//!
//! ## API key
//! Obtain one from Google Cloud Console → Places API.
//! Set via `APP__EXTERNAL_APIS__GOOGLE_PLACES_API_KEY`.

use async_trait::async_trait;
use chrono::DateTime;
use serde::Deserialize;

use forgebike_domain::{
    ports::review_fetch_port::{FetchedReview, ReviewFetchPort},
    DomainError,
};

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct PlaceDetailsResponse {
    result: Option<PlaceResult>,
    status: String,
}

#[derive(Deserialize)]
struct PlaceResult {
    #[serde(default)]
    reviews: Vec<GoogleReview>,
}

#[derive(Deserialize)]
struct GoogleReview {
    /// Author display name.
    author_name: String,
    /// 1–5 integer rating.
    rating: u8,
    /// Review text.
    #[serde(default)]
    text: String,
    /// Unix timestamp of when the review was written.
    time: i64,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct GooglePlacesClient {
    client: reqwest::Client,
    api_key: String,
}

impl GooglePlacesClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }
}

#[async_trait]
impl ReviewFetchPort for GooglePlacesClient {
    async fn fetch_reviews(&self, place_id: &str) -> Result<Vec<FetchedReview>, DomainError> {
        if self.api_key.is_empty() {
            tracing::debug!("Google Places API key not configured — skipping");
            return Ok(vec![]);
        }

        let response = self
            .client
            .get("https://maps.googleapis.com/maps/api/place/details/json")
            .query(&[
                ("place_id", place_id),
                ("fields", "reviews"),
                ("key", &self.api_key),
            ])
            .send()
            .await
            .map_err(|e| {
                DomainError::ExternalService(format!("Google Places request failed: {e}"))
            })?;

        if !response.status().is_success() {
            return Err(DomainError::ExternalService(format!(
                "Google Places returned HTTP {}",
                response.status()
            )));
        }

        let body: PlaceDetailsResponse = response
            .json()
            .await
            .map_err(|e| DomainError::ExternalService(format!("Google Places parse error: {e}")))?;

        if body.status != "OK" {
            return Err(DomainError::ExternalService(format!(
                "Google Places API status: {}",
                body.status
            )));
        }

        let reviews = body
            .result
            .unwrap_or(PlaceResult { reviews: vec![] })
            .reviews
            .into_iter()
            .filter_map(|r| {
                let published_at = DateTime::from_timestamp(r.time, 0)?;
                Some(FetchedReview {
                    external_id: format!(
                        "{}-{}",
                        r.time,
                        r.author_name.to_lowercase().replace(' ', "_")
                    ),
                    author_name: r.author_name,
                    rating: i16::from(r.rating).clamp(1, 5),
                    body: if r.text.is_empty() {
                        None
                    } else {
                        Some(r.text)
                    },
                    published_at: published_at.with_timezone(&chrono::Utc),
                })
            })
            .collect();

        Ok(reviews)
    }
}
