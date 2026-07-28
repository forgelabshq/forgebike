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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use forgebike_domain::pagination::Cursor;
    use uuid::Uuid;

    fn sample_cursor() -> Cursor {
        Cursor {
            created_at: Utc::now(),
            id: Uuid::new_v4(),
        }
    }

    // -- Encode / decode round-trip -------------------------------------------

    #[test]
    fn encode_then_decode_returns_same_cursor() {
        let original = sample_cursor();
        let encoded = encode_cursor(&original);
        let decoded = decode_cursor(&encoded).expect("should decode successfully");

        // Timestamps are stored as milliseconds, so microseconds are truncated.
        assert_eq!(
            original.created_at.timestamp_millis(),
            decoded.created_at.timestamp_millis(),
        );
        assert_eq!(original.id, decoded.id);
    }

    #[test]
    fn encoded_cursor_is_url_safe_no_padding() {
        let encoded = encode_cursor(&sample_cursor());
        assert!(!encoded.contains('+'), "must not contain '+'");
        assert!(!encoded.contains('/'), "must not contain '/'");
        assert!(!encoded.contains('='), "must not contain padding '='");
    }

    // -- Invalid cursor inputs -----------------------------------------------

    #[test]
    fn empty_string_returns_none() {
        assert!(decode_cursor("").is_none());
    }

    #[test]
    fn non_base64_string_returns_none() {
        assert!(decode_cursor("not base64!!!").is_none());
    }

    #[test]
    fn valid_base64_but_wrong_format_returns_none() {
        // "hello" in base64 — decodes fine but has no ':' separator.
        assert!(decode_cursor("aGVsbG8").is_none());
    }

    #[test]
    fn cursor_with_invalid_uuid_returns_none() {
        // timestamp OK, UUID is garbage
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
        let raw = "1700000000000:not-a-real-uuid";
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());
        assert!(decode_cursor(&encoded).is_none());
    }

    // -- PageResponse --------------------------------------------------------

    #[test]
    fn page_response_new_stores_items_and_cursor() {
        let items: Vec<u32> = vec![1, 2, 3];
        let cursor = Some("abc".to_string());
        let resp = PageResponse::new(items.clone(), cursor.clone());
        assert_eq!(resp.items, items);
        assert_eq!(resp.next_cursor, cursor);
    }

    #[test]
    fn page_response_with_no_cursor() {
        let resp = PageResponse::new(vec![42u32], None);
        assert!(resp.next_cursor.is_none());
    }
}
