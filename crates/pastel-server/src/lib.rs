//! pastel-server library: HTTP + WebSocket entrypoint and room registry.
//!
//! The binary in `src/main.rs` is a thin wrapper that builds the router and
//! serves it on a TCP listener. Integration tests use the same `build_router`
//! against a port-zero listener.

pub mod rooms;
pub mod words;
pub mod ws;

use axum::extract::State;
use axum::routing::get;
use axum::Router;
use pastel_room::WordLists;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub rooms: rooms::Rooms,
}

impl AppState {
    pub fn new(words: Arc<WordLists>) -> Self {
        Self {
            rooms: rooms::Rooms::new(words),
        }
    }

    pub fn with_test_words() -> Self {
        Self::new(Arc::new(WordLists::test_fixture()))
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics))
        .route("/ws/:code", get(ws::ws_handler))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn metrics(State(state): State<AppState>) -> String {
    format!(
        "# HELP pastel_rooms_active Active rooms hosted on this node.\n\
         # TYPE pastel_rooms_active gauge\n\
         pastel_rooms_active {}\n",
        state.rooms.count(),
    )
}
