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
