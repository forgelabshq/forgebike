//! User entity and role enum.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::identifiers::{TenantId, UserId};

// ---------------------------------------------------------------------------
// Entity
// ---------------------------------------------------------------------------

/// A human login within a single tenant.
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub tenant_id: TenantId,
    pub email: String,
    /// Argon2id hash — never returned over the API.
    pub password_hash: String,
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Role
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserRole {
    Owner,
    Manager,
    Viewer,
}

impl std::fmt::Display for UserRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Owner => write!(f, "owner"),
            Self::Manager => write!(f, "manager"),
            Self::Viewer => write!(f, "viewer"),
        }
    }
}

impl std::str::FromStr for UserRole {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "owner" => Ok(Self::Owner),
            "manager" => Ok(Self::Manager),
            "viewer" => Ok(Self::Viewer),
            _ => Err(format!("unknown user role: {s:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn every_variant_round_trips_through_display_and_from_str() {
        let variants = [UserRole::Owner, UserRole::Manager, UserRole::Viewer];
        for role in variants {
            let s = role.to_string();
            let parsed = UserRole::from_str(&s).unwrap();
            assert_eq!(role, parsed, "round-trip failed for {s:?}");
        }
    }

    #[test]
    fn display_produces_lowercase_strings() {
        assert_eq!(UserRole::Owner.to_string(), "owner");
        assert_eq!(UserRole::Manager.to_string(), "manager");
        assert_eq!(UserRole::Viewer.to_string(), "viewer");
    }

    #[test]
    fn from_str_rejects_unknown_and_empty_strings() {
        assert!(UserRole::from_str("admin").is_err());
        assert!(UserRole::from_str("OWNER").is_err()); // case-sensitive
        assert!(UserRole::from_str("").is_err());
    }
}
