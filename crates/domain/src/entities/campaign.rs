//! Campaign entity.
//!
//! A campaign is a bulk message (email or SMS) sent to a filtered set of
//! customer contacts.  Lifecycle: `draft` → `sending` → `sent` (or `failed`).

use chrono::{DateTime, Utc};

use crate::identifiers::{CampaignId, RestaurantId, TenantId};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Delivery channel for a campaign.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CampaignChannel {
    /// Email via SMTP (`lettre`).
    Email,
    /// SMS — requires Twilio configuration (Phase 9+).
    Sms,
}

impl std::fmt::Display for CampaignChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Email => write!(f, "email"),
            Self::Sms => write!(f, "sms"),
        }
    }
}

impl std::str::FromStr for CampaignChannel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "email" => Ok(Self::Email),
            "sms" => Ok(Self::Sms),
            _ => Err(format!("unknown campaign channel: {s:?}")),
        }
    }
}

/// Lifecycle status of a campaign.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum CampaignStatus {
    /// Created but not yet sent.
    #[default]
    Draft,
    /// Queued for a future `scheduled_at` time (UI only — auto-send not yet
    /// implemented; the send endpoint ignores `scheduled_at`).
    Scheduled,
    /// Currently being dispatched (set synchronously before spawning the
    /// background send task).
    Sending,
    /// All recipients received the message.
    Sent,
    /// Sending failed — see server logs for details.
    Failed,
}

impl std::fmt::Display for CampaignStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Draft => write!(f, "draft"),
            Self::Scheduled => write!(f, "scheduled"),
            Self::Sending => write!(f, "sending"),
            Self::Sent => write!(f, "sent"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for CampaignStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "draft" => Ok(Self::Draft),
            "scheduled" => Ok(Self::Scheduled),
            "sending" => Ok(Self::Sending),
            "sent" => Ok(Self::Sent),
            "failed" => Ok(Self::Failed),
            _ => Err(format!("unknown campaign status: {s:?}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

/// A bulk-message campaign sent to a filtered set of contacts.
#[derive(Debug, Clone)]
pub struct Campaign {
    pub id: CampaignId,
    pub tenant_id: TenantId,
    pub restaurant_id: RestaurantId,
    pub name: String,
    pub channel: CampaignChannel,
    pub status: CampaignStatus,
    /// Email subject line (required for `email` channel).
    pub subject: Option<String>,
    /// Message body (plain text).
    pub body: String,
    /// When set, only contacts with this tag receive the campaign.
    /// `None` = send to all contacts for this restaurant.
    pub tag_filter: Option<String>,
    /// Informational future send time (auto-send not yet implemented).
    pub scheduled_at: Option<DateTime<Utc>>,
    /// Timestamp set when the send completes.
    pub sent_at: Option<DateTime<Utc>>,
    /// Number of recipients the campaign was dispatched to.
    pub recipients_count: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn channel_round_trips() {
        for ch in [CampaignChannel::Email, CampaignChannel::Sms] {
            let s = ch.to_string();
            assert_eq!(CampaignChannel::from_str(&s).unwrap(), ch);
        }
    }

    #[test]
    fn status_round_trips() {
        for st in [
            CampaignStatus::Draft,
            CampaignStatus::Scheduled,
            CampaignStatus::Sending,
            CampaignStatus::Sent,
            CampaignStatus::Failed,
        ] {
            let s = st.to_string();
            assert_eq!(CampaignStatus::from_str(&s).unwrap(), st);
        }
    }

    #[test]
    fn default_status_is_draft() {
        assert_eq!(CampaignStatus::default(), CampaignStatus::Draft);
    }
}
