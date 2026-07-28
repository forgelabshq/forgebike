//! Review entity and platform enum.

use chrono::{DateTime, Utc};

use crate::identifiers::{RestaurantId, ReviewId, TenantId};

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

/// A customer review aggregated from an external platform.
///
/// `sentiment_score` and `ai_reply_draft` are populated by Phase 4
/// (AI analysis) and are `None` immediately after syncing.
#[derive(Debug, Clone)]
pub struct Review {
    pub id: ReviewId,
    pub restaurant_id: RestaurantId,
    pub tenant_id: TenantId,
    pub platform: ReviewPlatform,
    /// Platform-assigned unique identifier — used for deduplication on upsert.
    pub external_id: String,
    pub author_name: String,
    /// Star rating (1–5).
    pub rating: i16,
    pub body: Option<String>,
    pub published_at: DateTime<Utc>,
    /// Populated by Phase 4 AI sentiment analysis.  Range: −1.0 (negative)
    /// … +1.0 (positive).
    pub sentiment_score: Option<f32>,
    /// AI-generated reply draft, populated by Phase 4.
    pub ai_reply_draft: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewPlatform {
    Google,
    Yelp,
    TripAdvisor,
}

impl std::fmt::Display for ReviewPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Google => write!(f, "google"),
            Self::Yelp => write!(f, "yelp"),
            Self::TripAdvisor => write!(f, "tripadvisor"),
        }
    }
}

impl std::str::FromStr for ReviewPlatform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "google" => Ok(Self::Google),
            "yelp" => Ok(Self::Yelp),
            "tripadvisor" => Ok(Self::TripAdvisor),
            _ => Err(format!("unknown review platform: {s:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn every_platform_round_trips_through_display_and_from_str() {
        for platform in [
            ReviewPlatform::Google,
            ReviewPlatform::Yelp,
            ReviewPlatform::TripAdvisor,
        ] {
            let s = platform.to_string();
            let parsed = ReviewPlatform::from_str(&s).unwrap();
            assert_eq!(platform, parsed, "round-trip failed for {s:?}");
        }
    }

    #[test]
    fn from_str_rejects_unknown_platforms() {
        assert!(ReviewPlatform::from_str("facebook").is_err());
        assert!(ReviewPlatform::from_str("Google").is_err()); // case-sensitive
        assert!(ReviewPlatform::from_str("").is_err());
    }
}
