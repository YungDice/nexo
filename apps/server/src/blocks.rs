//! Blocking (§6.1).
//!
//! The whole reason this is a server module and not a client one is in the
//! migration that creates its table: a block the client applies is a promise
//! the product cannot keep. The blocked person goes on sending, the server
//! goes on accepting, and the only thing that changes is whether one app draws
//! it. Rule 5 makes that worse than offering nothing, because the word means
//! something to the person who used it.
//!
//! So the effects live where they can be enforced, and there are exactly two:
//!
//! - **The feed and profile posts** drop anything by a blocked author, and
//!   anything by someone who has blocked *you* — see [`hidden_authors`], which
//!   `posts.rs` spends.
//! - **Delivery** refuses to open a conversation or accept an envelope across
//!   a block — see [`blocked_between`], which `delivery` spends.
//!
//! # What it does not do, stated here because the UI says it too
//!
//! It does not stop somebody making a second account. Nothing short of
//! identity verification does, and Nexo has none by design. It also does not
//! reach backwards: messages already delivered stay delivered, because they
//! are on the other person's disk and the server never had the keys.
//!
//! # Why blocking is not told to the blocked
//!
//! There is no notification and no distinct error. Someone who has been
//! blocked sees what someone whose message did not go through sees. Telling
//! them turns a quiet exit into a confrontation, and that asymmetry is the
//! only protection the person doing the blocking gets.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get, routing::post};
use serde::Serialize;

use crate::auth::bearer::Caller;
use crate::state::AppState;

/// Block routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/blocks", get(list))
        .route("/v1/blocks/{handle}", post(block).delete(unblock))
}

/// Why a block request was refused.
#[derive(Debug)]
pub enum BlockError {
    /// No such handle.
    NotFound,
    /// The request does not describe a state that can exist.
    Invalid(String),
    /// Too many of these, too quickly.
    TooManyRequests,
    /// Something the caller cannot act on.
    Internal(anyhow::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl IntoResponse for BlockError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            BlockError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "No account with that handle.".to_string(),
            ),
            BlockError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            BlockError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests. Slow down.".to_string(),
            ),
            BlockError::Internal(error) => {
                tracing::error!(%error, "block request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "Something went wrong.".to_string(),
                )
            }
        };
        (status, Json(ErrorBody { error, message })).into_response()
    }
}

impl From<sqlx::Error> for BlockError {
    fn from(error: sqlx::Error) -> Self {
        BlockError::Internal(error.into())
    }
}

/// One blocked account, as the settings list shows it.
#[derive(Debug, Serialize)]
pub struct BlockView {
    pub handle: String,
    pub display_name: String,
    pub blocked_at_ms: i64,
}

/// Everyone the caller is blocking.
///
/// Only the caller's own list. There is no way to ask who is blocking you --
/// that would hand the blocked person the very confirmation the design
/// withholds.
async fn list(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<Vec<BlockView>>, BlockError> {
    // The epoch conversion happens in SQL, as everywhere else in this crate:
    // sqlx would otherwise need a date-time feature compiled in to hand back a
    // `TIMESTAMPTZ`, and a millisecond integer is what the client wants anyway.
    let rows = sqlx::query!(
        "SELECT u.handle::TEXT AS \"handle!\", u.display_name,
                (EXTRACT(EPOCH FROM b.created_at) * 1000)::BIGINT AS \"blocked_at_ms!\"
         FROM blocks b
         JOIN users u ON u.id = b.blocked_id
         WHERE b.blocker_id = $1
         ORDER BY b.created_at DESC",
        caller.user_id
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| BlockView {
                handle: r.handle,
                display_name: r.display_name,
                blocked_at_ms: r.blocked_at_ms,
            })
            .collect(),
    ))
}

/// Blocks somebody.
///
/// Idempotent: blocking someone already blocked is a no-op rather than a
/// conflict. The caller cannot know the current state without asking, and
/// making them ask first would only add a round trip and a race.
async fn block(
    State(state): State<AppState>,
    caller: Caller,
    Path(handle): Path<String>,
) -> Result<StatusCode, BlockError> {
    if !state.limits.profile.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "profile rate limit reached");
        return Err(BlockError::TooManyRequests);
    }

    let target = user_id(&state, &handle).await?;
    if target == caller.user_id {
        return Err(BlockError::Invalid(
            "You cannot block yourself.".to_string(),
        ));
    }

    sqlx::query!(
        "INSERT INTO blocks (blocker_id, blocked_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
        caller.user_id,
        target
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Unblocks somebody. Idempotent for the same reason as [`block`].
async fn unblock(
    State(state): State<AppState>,
    caller: Caller,
    Path(handle): Path<String>,
) -> Result<StatusCode, BlockError> {
    if !state.limits.profile.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "profile rate limit reached");
        return Err(BlockError::TooManyRequests);
    }

    let target = user_id(&state, &handle).await?;
    sqlx::query!(
        "DELETE FROM blocks WHERE blocker_id = $1 AND blocked_id = $2",
        caller.user_id,
        target
    )
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn user_id(state: &AppState, handle: &str) -> Result<i64, BlockError> {
    sqlx::query!("SELECT id FROM users WHERE handle = $1", handle)
        .fetch_optional(&state.db)
        .await?
        .map(|row| row.id)
        .ok_or(BlockError::NotFound)
}

/// Whether a block stands between two people, in **either** direction.
///
/// Either, because a block that only stopped the blocker from hearing the
/// blocked person would still let the blocked person read everything and open
/// conversations. The record is directional; the effect is not.
pub async fn blocked_between(db: &sqlx::PgPool, a: i64, b: i64) -> Result<bool, sqlx::Error> {
    let row = sqlx::query!(
        "SELECT EXISTS (
             SELECT 1 FROM blocks
             WHERE (blocker_id = $1 AND blocked_id = $2)
                OR (blocker_id = $2 AND blocked_id = $1)
         ) AS \"blocked!\"",
        a,
        b
    )
    .fetch_one(db)
    .await?;
    Ok(row.blocked)
}

/// Every user whose posts the caller must not see.
///
/// Both directions again: people the caller blocked, and people who blocked
/// the caller. Returned as a list for the feed to pass to one query, rather
/// than as a join inside it, because the same list also filters a profile's
/// posts and the two queries are otherwise unrelated.
pub async fn hidden_authors(db: &sqlx::PgPool, caller: i64) -> Result<Vec<i64>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT blocked_id AS id FROM blocks WHERE blocker_id = $1
         UNION
         SELECT blocker_id AS id FROM blocks WHERE blocked_id = $1",
        caller
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().filter_map(|r| r.id).collect())
}
