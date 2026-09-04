//! Following, and the feed that follows from it.
//!
//! A follow is a **directed** edge: following somebody says nothing about
//! whether they follow you. That is what separates this from the two relations
//! the server already understands, and keeping the three apart is the whole
//! difficulty here:
//!
//! - **A block** is mutual by effect — `blocked_between` hides two people from
//!   each other whichever of them pressed it.
//! - **A contact** means *shares a conversation*, and it is what gates stories.
//!   `shares_a_conversation` was deliberately moved out of `profiles.rs` so
//!   there would be one definition of it rather than two that drift.
//! - **A follow** is neither. It is an interest somebody declared, it can be
//!   entirely one-sided, and it grants nothing that was not already public.
//!
//! That last clause is the one to hold on to. Following changes **what you are
//! shown**, never **what you are allowed to see**: every post the Following
//! feed can return is a post the Everyone feed would have returned to the same
//! caller. If that ever stops being true, following has quietly become an
//! access-control mechanism, and access control that started life as a
//! convenience is how permission bugs happen.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use crate::auth::Caller;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/users/{handle}/follow", post(follow).delete(unfollow))
        .route("/v1/users/{handle}/follow-state", get(follow_state))
}

/// Why a follow was refused.
#[derive(Debug)]
pub enum FollowError {
    /// No such handle — or one the caller may not act on, which is answered
    /// the same way on purpose. See `follow`.
    NotFound,
    /// Following yourself.
    Self_,
    Database(sqlx::Error),
}

impl From<sqlx::Error> for FollowError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl IntoResponse for FollowError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            FollowError::NotFound => (StatusCode::NOT_FOUND, "No such account."),
            FollowError::Self_ => (
                StatusCode::BAD_REQUEST,
                "You cannot follow your own account.",
            ),
            FollowError::Database(error) => {
                tracing::error!(%error, "follow query failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong.")
            }
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// Whether the caller follows this account, and how many follow it.
#[derive(Debug, Serialize)]
pub struct FollowState {
    pub following: bool,
    /// How many accounts follow this one.
    ///
    /// Public because the posts already are: anybody who can see somebody's
    /// posts can count the people reacting to them, so the number discloses
    /// nothing the feed does not. **Who** they are is not offered by any route
    /// here — a follower *list* is a social graph handed out on request, and
    /// that is a different decision from a count.
    pub followers: i64,
}

/// Follows an account.
///
/// Refuses in three cases, and two of them answer identically to a handle that
/// does not exist:
///
/// - **Blocked, either way.** Reusing `blocked_between` rather than writing a
///   second rule, because the one that gets written twice is the one that ends
///   up enforced once.
/// - **Private.** A private account is unreachable by somebody new (wave 6), and
///   a follow is somebody new arriving. Answering `404` rather than `403` keeps
///   the existing property that a private account is indistinguishable from an
///   account that is not there — a `403` would confirm the handle exists, which
///   is exactly what being absent from search is meant to prevent.
async fn follow(
    State(state): State<AppState>,
    caller: Caller,
    Path(handle): Path<String>,
) -> Result<StatusCode, FollowError> {
    let target = resolve(&state, caller.user_id, &handle).await?;
    if target == caller.user_id {
        return Err(FollowError::Self_);
    }

    sqlx::query!(
        "INSERT INTO follows (follower_id, followed_id)
         VALUES ($1, $2)
         ON CONFLICT (follower_id, followed_id) DO NOTHING",
        caller.user_id,
        target
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Unfollows an account.
///
/// Deliberately *not* routed through `resolve`: somebody who has been blocked
/// since following must still be able to stop following, and a check that
/// refuses them would leave a row nobody can remove. Unfollowing removes an
/// edge and can never disclose anything, so it needs no visibility rule at all.
async fn unfollow(
    State(state): State<AppState>,
    caller: Caller,
    Path(handle): Path<String>,
) -> Result<StatusCode, FollowError> {
    sqlx::query!(
        "DELETE FROM follows
          WHERE follower_id = $1
            AND followed_id = (SELECT id FROM users WHERE handle = $2)",
        caller.user_id,
        handle
    )
    .execute(&state.db)
    .await?;

    // No content whether or not a row went. The caller wanted not to be
    // following, and they are not.
    Ok(StatusCode::NO_CONTENT)
}

/// Whether the caller follows this account, and its follower count.
async fn follow_state(
    State(state): State<AppState>,
    caller: Caller,
    Path(handle): Path<String>,
) -> Result<Json<FollowState>, FollowError> {
    let target = resolve(&state, caller.user_id, &handle).await?;

    let row = sqlx::query!(
        "SELECT
             EXISTS (SELECT 1 FROM follows WHERE follower_id = $1 AND followed_id = $2)
                 AS \"following!\",
             (SELECT COUNT(*) FROM follows WHERE followed_id = $2)::BIGINT
                 AS \"followers!\"",
        caller.user_id,
        target
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(FollowState {
        following: row.following,
        followers: row.followers,
    }))
}

/// The account this handle names, if this caller may act on it at all.
///
/// One place for the visibility rules, so `follow` and `follow_state` cannot
/// drift apart about who exists. Everything it refuses becomes `NotFound`,
/// which is the point: "blocked", "private" and "no such handle" are one answer
/// on the wire.
async fn resolve(state: &AppState, caller: i64, handle: &str) -> Result<i64, FollowError> {
    let row = sqlx::query!("SELECT id, is_private FROM users WHERE handle = $1", handle)
        .fetch_optional(&state.db)
        .await?
        .ok_or(FollowError::NotFound)?;

    // Your own account resolves whatever its settings say; `follow` rejects it
    // afterwards with a message that explains itself, and `follow_state` needs
    // it so a profile can show its own follower count.
    if row.id == caller {
        return Ok(row.id);
    }

    if crate::blocks::blocked_between(&state.db, caller, row.id).await? {
        return Err(FollowError::NotFound);
    }
    if row.is_private {
        return Err(FollowError::NotFound);
    }
    Ok(row.id)
}
