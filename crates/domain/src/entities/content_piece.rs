//! AI-generated marketing content entity.

use chrono::{DateTime, Utc};

use crate::identifiers::{ContentPieceId, RestaurantId, TenantId};

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

/// A piece of AI-generated marketing content owned by a restaurant.
///
/// Lifecycle: `draft` → (human review) → `approved` → `published`.
#[derive(Debug, Clone)]
pub struct ContentPiece {
    pub id: ContentPieceId,
    pub restaurant_id: RestaurantId,
    pub tenant_id: TenantId,
    pub content_type: ContentType,
    /// Optional AI-generated title (email subject line, blog headline, etc.).
    pub title: Option<String>,
    pub body: String,
    pub status: ContentStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Content type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentType {
    /// Short post for Twitter / Instagram / Facebook (≤ 280 chars + hashtags).
    SocialPost,
    /// Marketing email with a subject line and body.
    Email,
    /// Short appetising description for a menu item.
    MenuDescription,
    /// Opening paragraphs for a blog article.
    BlogIntro,
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SocialPost => write!(f, "social_post"),
            Self::Email => write!(f, "email"),
            Self::MenuDescription => write!(f, "menu_description"),
            Self::BlogIntro => write!(f, "blog_intro"),
        }
    }
}

impl std::str::FromStr for ContentType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "social_post" => Ok(Self::SocialPost),
            "email" => Ok(Self::Email),
            "menu_description" => Ok(Self::MenuDescription),
            "blog_intro" => Ok(Self::BlogIntro),
            _ => Err(format!("unknown content type: {s:?}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Content status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ContentStatus {
    /// AI-generated, not yet reviewed by a human.
    #[default]
    Draft,
    /// Reviewed and approved — ready to publish.
    Approved,
    /// Published externally by the restaurant.
    Published,
}

impl std::fmt::Display for ContentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Approved => write!(f, "approved"),
            Self::Published => write!(f, "published"),
        }
    }
}

impl std::str::FromStr for ContentStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "approved" => Ok(Self::Approved),
            "published" => Ok(Self::Published),
            _ => Err(format!("unknown content status: {s:?}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn content_type_round_trips() {
        for ct in [
            ContentType::SocialPost,
            ContentType::Email,
            ContentType::MenuDescription,
            ContentType::BlogIntro,
        ] {
            let s = ct.to_string();
            assert_eq!(ContentType::from_str(&s).unwrap(), ct);
        }
    }

    #[test]
    fn content_status_round_trips() {
        for cs in [
            ContentStatus::Draft,
            ContentStatus::Approved,
            ContentStatus::Published,
        ] {
            let s = cs.to_string();
            assert_eq!(ContentStatus::from_str(&s).unwrap(), cs);
        }
    }

    #[test]
    fn default_status_is_draft() {
        assert_eq!(ContentStatus::default(), ContentStatus::Draft);
    }
}
