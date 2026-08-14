//! Newtype wrappers around [`uuid::Uuid`] for every aggregate root ID.
//!
//! Using distinct types for each ID prevents accidentally passing a
//! `UserId` where a `RestaurantId` is expected — the compiler catches it.
//!
//! Infrastructure crates are responsible for converting between `Uuid` and
//! these types; the domain layer remains free of any database/HTTP coupling.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Macro — generates a newtype UUID wrapper with all the common impls.
// ---------------------------------------------------------------------------

macro_rules! uuid_id {
    (
        $(#[$meta:meta])*
        $name:ident
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Create a new, random ID.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wrap an existing [`Uuid`] (e.g. one read from the database).
            #[must_use]
            pub fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            /// Unwrap to the inner [`Uuid`].
            #[must_use]
            pub fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse::<Uuid>().map(Self)
            }
        }
    };
}

// ---------------------------------------------------------------------------
// All aggregate root IDs used across the platform
// ---------------------------------------------------------------------------

uuid_id!(
    /// Identifies a tenant (a restaurant business / account).
    TenantId
);

uuid_id!(
    /// Identifies a user (an individual login within a tenant).
    UserId
);

uuid_id!(
    /// Identifies a restaurant location owned by a tenant.
    RestaurantId
);

uuid_id!(
    /// Identifies a single item on a restaurant's menu.
    MenuItemId
);

uuid_id!(
    /// Identifies a review aggregated from an external platform.
    ReviewId
);

uuid_id!(
    /// Identifies a piece of AI-generated marketing content.
    ContentPieceId
);

uuid_id!(
    /// Identifies a customer engagement campaign.
    CampaignId
);

uuid_id!(
    /// Identifies a customer contact record.
    CustomerContactId
);

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // -- Construction & round-trips -------------------------------------------

    #[test]
    fn new_produces_unique_ids() {
        let a = TenantId::new();
        let b = TenantId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn from_uuid_preserves_the_uuid() {
        let raw = Uuid::new_v4();
        let id = RestaurantId::from_uuid(raw);
        assert_eq!(id.as_uuid(), raw);
    }

    #[test]
    fn display_matches_hyphenated_uuid() {
        let raw = Uuid::new_v4();
        let id = UserId::from_uuid(raw);
        assert_eq!(id.to_string(), raw.to_string());
    }

    #[test]
    fn from_str_round_trips_through_display() {
        let id: MenuItemId = MenuItemId::new();
        let s = id.to_string();
        let parsed = MenuItemId::from_str(&s).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn from_str_rejects_non_uuid_strings() {
        assert!(TenantId::from_str("not-a-uuid").is_err());
        assert!(TenantId::from_str("").is_err());
        assert!(TenantId::from_str("12345").is_err());
    }

    // -- Type-safety: different ID types must not compare equal ---------------

    #[test]
    fn same_uuid_in_different_types_is_not_comparable() {
        // This is a compile-time guarantee rather than a runtime one, but we
        // can verify the IDs wrap the same UUID without being equal to each
        // other (they have different types so == isn't defined between them).
        let raw = Uuid::new_v4();
        let tenant_id = TenantId::from_uuid(raw);
        let restaurant_id = RestaurantId::from_uuid(raw);
        // Both wrap the same UUID...
        assert_eq!(tenant_id.as_uuid(), restaurant_id.as_uuid());
        // ...but they are distinct Rust types — the line below won't compile:
        // assert_eq!(tenant_id, restaurant_id);  // <-- type error
    }

    // -- Default --------------------------------------------------------------

    #[test]
    fn default_produces_a_non_nil_id() {
        let id = TenantId::default();
        assert_ne!(id.as_uuid(), Uuid::nil());
    }
}
