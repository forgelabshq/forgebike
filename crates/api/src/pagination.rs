//! HTTP-layer pagination helpers.
//!
//! Converts between the typed [`Cursor`] the domain uses and the opaque
//! base64url string that callers see in query parameters and response bodies.
//!
//! ## Wire format
//!
//! `{epoch_milliseconds}:{uuid_hyphenated}` encoded as URL-safe base64
//! (no padding).  Example: `MTc0NjM4MDgwMDAwMDphMWIyYzNkNC0…`
//!
//! The format is intentionally simple and human-readable when decoded so
//! debugging is straightforward.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use forgebike_domain::pagination::Cursor;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Encode / decode
// ---------------------------------------------------------------------------

/// Encode a [`Cursor`] into the opaque string the API sends to callers.
#[must_use]
pub fn encode_cursor(cursor: &Cursor) -> String {
    let raw = format!("{}:{}", cursor.created_at.timestamp_millis(), cursor.id);
    URL_SAFE_NO_PAD.encode(raw.as_bytes())
}

/// Decode a cursor string received from a caller.
///
/// Returns `None` on any decode/parse error so callers receive a clean
/// `None` rather than an error — an invalid cursor simply resets to page 1.
#[must_use]
pub fn decode_cursor(s: &str) -> Option<Cursor> {
    let bytes = URL_SAFE_NO_PAD.decode(s).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let (ts, id_str) = text.split_once(':')?;
    let ts_ms: i64 = ts.parse().ok()?;
    let created_at = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ts_ms)?;
    let id = uuid::Uuid::parse_str(id_str).ok()?;
    Some(Cursor { created_at, id })
}

// ---------------------------------------------------------------------------
// Query parameter struct
// ---------------------------------------------------------------------------

/// Common query parameters accepted by every list endpoint.
///
/// Extracted with `Query<PageQuery>` in handlers.
#[derive(Debug, Deserialize)]
pub struct PageQuery {
    /// Maximum items per page (1–100, default 20).
    #[serde(default = "default_limit")]
    pub limit: i64,
    /// Opaque cursor from the previous response's `next_cursor`.
    pub cursor: Option<String>,
}

fn default_limit() -> i64 {
    20
}

// ---------------------------------------------------------------------------
// Response wrapper
// ---------------------------------------------------------------------------

/// The JSON shape returned by every list endpoint.
#[derive(Debug, Serialize)]
pub struct PageResponse<T: Serialize> {
    pub items: Vec<T>,
    /// `null` when there are no further pages.
    pub next_cursor: Option<String>,
}

impl<T: Serialize> PageResponse<T> {
    #[must_use]
    pub fn new(items: Vec<T>, next_cursor: Option<String>) -> Self {
        Self { items, next_cursor }
    }
}
