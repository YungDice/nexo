//! Liveness endpoint.
//!
//! Deliberately says nothing about database state, user counts, or version —
//! an unauthenticated endpoint should not be a reconnaissance surface.

use axum::Json;
use nexo_protocol::PROTOCOL_VERSION;
use serde::Serialize;

#[derive(Serialize)]
pub struct Health {
    status: &'static str,
    protocol_version: u16,
}

pub async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        protocol_version: PROTOCOL_VERSION,
    })
}
