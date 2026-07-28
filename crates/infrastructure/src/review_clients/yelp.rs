//! Yelp Fusion API client.
//!
//! ## Endpoint
//! ```text
//! GET https://api.yelp.com/v3/businesses/{id}/reviews?limit=50
//! Authorization: Bearer {api_key}
//! ```
//!
//! The free tier returns the 3 most recent reviews.
//!
//! ## API key
//! Obtain one from the Yelp Fusion portal → Manage App.
//! Set via `APP__EXTERNAL_APIS__YELP_API_KEY`.

use async_trait::async_trait;
use chrono::NaiveDateTime;
use serde::Deserialize;

use forgebike_domain::{
    ports::review_fetch_port::{FetchedReview, ReviewFetchPort},
    DomainError,
};

// ---------------------------------------------------------------------------
// Response shapes
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct YelpReviewsResponse {
    reviews: Vec<YelpReview>,
}

#[derive(Deserialize)]
struct YelpReview {
    id: String,
    rating: u8,
    text: String,
    /// `"YYYY-MM-DD HH:MM:SS"` format.
    time_created: String,
    user: YelpUser,
}

#[derive(Deserialize)]
struct YelpUser {
    name: String,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct YelpFusionClient {
    client: reqwest::Client,
    api_key: String,
}

impl YelpFusionClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
        }
    }
}

#[async_trait]
impl ReviewFetchPort for YelpFusionClient {
    async fn fetch_reviews(&self, business_id: &str) -> Result<Vec<FetchedReview>, DomainError> {
        if self.api_key.is_empty() {
            tracing::debug!("Yelp Fusion API key not configured — skipping");
            return Ok(vec![]);
        }

        let url = format!("https://api.yelp.com/v3/businesses/{business_id}/reviews?limit=50");

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| DomainError::ExternalService(format!("Yelp request failed: {e}")))?;

        if !response.status().is_success() {
            return Err(DomainError::ExternalService(format!(
                "Yelp returned HTTP {}",
                response.status()
            )));
        }

        let body: YelpReviewsResponse = response
            .json()
            .await
            .map_err(|e| DomainError::ExternalService(format!("Yelp parse error: {e}")))?;

        let reviews = body
            .reviews
            .into_iter()
            .filter_map(|r| {
                // Parse Yelp's non-standard datetime format.
                let dt = NaiveDateTime::parse_from_str(&r.time_created, "%Y-%m-%d %H:%M:%S")
                    .ok()?
                    .and_utc();

                Some(FetchedReview {
                    external_id: r.id,
                    author_name: r.user.name,
                    rating: i16::from(r.rating).clamp(1, 5),
                    body: if r.text.is_empty() {
                        None
                    } else {
                        Some(r.text)
                    },
                    published_at: dt,
                })
            })
            .collect();

        Ok(reviews)
    }
}
