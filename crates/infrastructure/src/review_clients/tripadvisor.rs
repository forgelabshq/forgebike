//! `TripAdvisor` Content API client.
//!
//! ## Endpoint
//! ```text
//! GET https://api.content.tripadvisor.com/api/v1/location/{id}/reviews
//!     ?key={api_key}&language=en
//! ```
//!
//! ## Status
//! `TripAdvisor` Content API access requires an approved partnership
//! application.  This client is fully implemented but will return
//! `Ok(vec![])` until the restaurant entity gains a
//! `tripadvisor_location_id` column (planned for a future migration) and
//! a valid API key is configured.
//!
//! Set the key via `APP__EXTERNAL_APIS__TRIPADVISOR_API_KEY`.

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
struct TripAdvisorResponse {
    data: Vec<TripAdvisorReview>,
}

#[derive(Deserialize)]
struct TripAdvisorReview {
    id: u64,
    rating: u8,
    text: Option<String>,
    published_date: String,
    user: TripAdvisorUser,
}

#[derive(Deserialize)]
struct TripAdvisorUser {
    username: String,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct TripAdvisorClient {
    client: reqwest::Client,
    api_key: String,
}

impl TripAdvisorClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }
}

#[async_trait]
impl ReviewFetchPort for TripAdvisorClient {
    async fn fetch_reviews(&self, location_id: &str) -> Result<Vec<FetchedReview>, DomainError> {
        if self.api_key.is_empty() {
            tracing::debug!("TripAdvisor API key not configured — skipping");
            return Ok(vec![]);
        }

        let url =
            format!("https://api.content.tripadvisor.com/api/v1/location/{location_id}/reviews");

        let response = self
            .client
            .get(&url)
            .query(&[("key", &self.api_key), ("language", &"en".to_string())])
            .send()
            .await
            .map_err(|e| {
                DomainError::ExternalService(format!("TripAdvisor request failed: {e}"))
            })?;

        if !response.status().is_success() {
            return Err(DomainError::ExternalService(format!(
                "TripAdvisor returned HTTP {}",
                response.status()
            )));
        }

        let body: TripAdvisorResponse = response
            .json()
            .await
            .map_err(|e| DomainError::ExternalService(format!("TripAdvisor parse error: {e}")))?;

        let reviews = body
            .data
            .into_iter()
            .filter_map(|r| {
                let published_at = DateTime::parse_from_rfc3339(&r.published_date)
                    .ok()?
                    .with_timezone(&chrono::Utc);

                Some(FetchedReview {
                    external_id: r.id.to_string(),
                    author_name: r.user.username,
                    rating: i16::from(r.rating).clamp(1, 5),
                    body: r.text.filter(|t| !t.is_empty()),
                    published_at,
                })
            })
            .collect();

        Ok(reviews)
    }
}
