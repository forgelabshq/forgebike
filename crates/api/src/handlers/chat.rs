//! AI chat WebSocket handler.
//!
//! Opens a persistent WebSocket connection where the client can send
//! conversational messages and receive AI replies from a restaurant-aware
//! assistant.
//!
//! ## Authentication
//! JWT authentication is handled by the [`crate::middleware::auth::require_ws_auth`]
//! middleware that runs **before** this handler.  The middleware validates
//! `?token=<access_jwt>` and injects [`AuthIdentity`] into the request
//! extensions.  Unauthenticated or invalid requests receive `401` from the
//! middleware before `WebSocketUpgrade` extraction is ever attempted.
//!
//! ## Why middleware, not a handler extractor?
//! `WebSocketUpgrade` is an axum extractor that runs as part of the handler's
//! parameter extraction.  If a plain HTTP GET arrives (no Upgrade headers),
//! `WebSocketUpgrade` returns `400 Bad Request` before the handler body
//! executes.  By putting auth in a middleware layer the JWT check always runs
//! first, regardless of whether the request is a real WebSocket upgrade.
//!
//! ## Protocol
//! Client → server (JSON text frame):
//! ```json
//! {
//!   "message": "What are your opening hours?",
//!   "history": [
//!     { "role": "user",      "content": "Do you have vegan options?" },
//!     { "role": "assistant", "content": "Yes! We have several vegan dishes…" }
//!   ]
//! }
//! ```
//!
//! Server → client (JSON text frame — success):
//! ```json
//! { "reply": "We're open Monday to Sunday, 11am–10pm.", "tokens_used": 87 }
//! ```
//!
//! Server → client (JSON text frame — error):
//! ```json
//! { "error": "OpenAI API key is not configured" }
//! ```

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    response::IntoResponse,
    Extension,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use forgebike_domain::{
    entities::auth_identity::AuthIdentity,
    identifiers::RestaurantId,
    ports::ai_port::{ChatMessage, ChatRole},
};

use crate::state::AppState;

// ---------------------------------------------------------------------------
// Message types (client → server)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ChatRequest {
    /// The user's current message.
    message: String,
    /// Previous turns to include as context (client-managed history).
    #[serde(default)]
    history: Vec<HistoryEntry>,
}

#[derive(Debug, Deserialize)]
struct HistoryEntry {
    role: String,
    content: String,
}

// ---------------------------------------------------------------------------
// Response types (server → client)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ChatSuccess<'a> {
    reply: &'a str,
    tokens_used: u64,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of previous turns to include in the context window.
const MAX_HISTORY: usize = 20;

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/v1/ws/chat/:restaurant_id?token=<jwt>`
///
/// Auth is enforced by [`crate::middleware::auth::require_ws_auth`] which
/// runs before this handler and injects [`AuthIdentity`] into extensions.
/// The `Extension` extractor here simply reads what the middleware stored.
#[tracing::instrument(skip(state, ws), name = "handlers::chat::ws")]
pub async fn chat_ws(
    State(state): State<AppState>,
    Extension(identity): Extension<AuthIdentity>,
    Path(restaurant_id): Path<Uuid>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let rid = RestaurantId::from_uuid(restaurant_id);
    ws.on_upgrade(move |socket| handle_socket(socket, state, identity, rid))
}

// ---------------------------------------------------------------------------
// Socket loop
// ---------------------------------------------------------------------------

/// Handles a single WebSocket connection: reads JSON messages, calls AI,
/// sends JSON replies.  Runs until the client disconnects.
async fn handle_socket(
    mut socket: WebSocket,
    state: AppState,
    identity: AuthIdentity,
    restaurant_id: RestaurantId,
) {
    while let Some(Ok(msg)) = socket.recv().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            // Ignore ping/pong and binary frames.
            _ => continue,
        };

        let reply_text = process_turn(&state, &identity, restaurant_id, &text).await;

        if socket.send(Message::Text(reply_text)).await.is_err() {
            break; // client disconnected
        }
    }
}

/// Parse one client message, call AI, return a JSON reply string.
async fn process_turn(
    state: &AppState,
    identity: &AuthIdentity,
    restaurant_id: RestaurantId,
    raw: &str,
) -> String {
    // Parse the client's request.
    let request: ChatRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => {
            return json!({ "error": format!("invalid JSON: {e}") }).to_string();
        }
    };

    // Build the message list: history + current message.
    let mut messages: Vec<ChatMessage> = request
        .history
        .into_iter()
        .map(|h| {
            let role = match h.role.as_str() {
                "assistant" => ChatRole::Assistant,
                _ => ChatRole::User,
            };
            ChatMessage {
                role,
                content: h.content,
            }
        })
        .collect();

    messages.push(ChatMessage {
        role: ChatRole::User,
        content: request.message,
    });

    // Limit context window to avoid token overflow.
    if messages.len() > MAX_HISTORY {
        let drop = messages.len() - MAX_HISTORY;
        messages.drain(0..drop);
    }

    // Call the AI service.
    match state
        .ai_service
        .chat(identity, restaurant_id, messages)
        .await
    {
        Ok(reply) => serde_json::to_string(&ChatSuccess {
            reply: &reply.text,
            tokens_used: reply.tokens_used,
        })
        .unwrap_or_else(|_| json!({"error":"serialisation failed"}).to_string()),
        Err(e) => json!({ "error": e.to_string() }).to_string(),
    }
}
