//! Tenant entity and subscription plan enum.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::identifiers::TenantId;

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

/// Top-level isolation boundary — one per restaurant business account.
#[derive(Debug, Clone)]
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub plan_tier: PlanTier,
    pub stripe_customer_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Plan tier
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanTier {
    #[default]
    Starter,
    Growth,
    Scale,
}

impl std::fmt::Display for PlanTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Starter => write!(f, "starter"),
            Self::Growth => write!(f, "growth"),
            Self::Scale => write!(f, "scale"),
        }
    }
}

impl std::str::FromStr for PlanTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "starter" => Ok(Self::Starter),
            "growth" => Ok(Self::Growth),
            "scale" => Ok(Self::Scale),
            _ => Err(format!("unknown plan tier: {s:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn every_variant_round_trips_through_display_and_from_str() {
        let variants = [PlanTier::Starter, PlanTier::Growth, PlanTier::Scale];
        for tier in variants {
            let s = tier.to_string();
            let parsed = PlanTier::from_str(&s).unwrap();
            assert_eq!(tier, parsed, "round-trip failed for {s:?}");
        }
    }

    #[test]
    fn display_produces_lowercase_strings() {
        assert_eq!(PlanTier::Starter.to_string(), "starter");
        assert_eq!(PlanTier::Growth.to_string(), "growth");
        assert_eq!(PlanTier::Scale.to_string(), "scale");
    }

    #[test]
    fn default_is_starter() {
        assert_eq!(PlanTier::default(), PlanTier::Starter);
    }

    #[test]
    fn from_str_rejects_unknown_strings() {
        assert!(PlanTier::from_str("enterprise").is_err());
        assert!(PlanTier::from_str("Starter").is_err()); // case-sensitive
        assert!(PlanTier::from_str("").is_err());
    }
}
