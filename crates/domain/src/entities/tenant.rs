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
