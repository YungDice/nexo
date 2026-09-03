//! Stories: an encrypted object that stops being available after 24 hours.
//!
//! This module handles bookkeeping and access. It never sees a story: the
//! object is AES-256-GCM ciphertext in the **encrypted** bucket, and the key
//! travels inside MLS messages the server cannot read. `storage.rs` keeps the
//! two buckets apart by type, so "a story is not media" is a property of the
//! code rather than a rule somebody has to remember.
//!
//! # Who gets the bytes
//!
//! Three conditions on the download route, all of them made of code that
//! already existed:
//!
//!   1. **Not expired.** [`expiry::still_available`] — a pure function, so the
//!      rule can be read and tested without a database, the same reason
//!      `delivery/epoch.rs` is shaped that way.
//!   2. **A contact**, meaning `profiles::shares_a_conversation`. That function
//!      was private to profiles and is now shared, because two definitions of
//!      "contact" drift apart eventually and the one that drifts is always the
//!      one guarding something.
//!   3. **Not blocked**, in either direction, via `blocks::blocked_between`.
//!
//! # What the server sees anyway
//!
//! Worth naming rather than leaving implied, and `docs/THREAT-MODEL.md` says it
//! too: that you posted, when, how large the ciphertext is, who asked for a
//! URL, and the burst of envelopes going to every one of your conversations at
//! the same moment. That last is the shape of the fan-out, and it is the price
//! of not building a story group. The content stays shut.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get, routing::post};
use serde::{Deserialize, Serialize};

use crate::auth::Caller;
use crate::state::AppState;

pub mod expiry;

/// How long a story's download URL lasts.
///
/// Short, because a presigned URL is a bearer credential for one object:
/// anybody holding it can fetch the ciphertext, expiry check or not. Ten
/// minutes is what `media.rs` uses and there is no reason to differ.
const DOWNLOAD_TTL: std::time::Duration = std::time::Duration::from_secs(600);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/stories", get(list).post(create))
        .route("/v1/stories/{id}/url", post(download_url))
}

/// Why a story request was refused.
#[derive(Debug)]
pub enum StoryError {
    NotFound,
    Invalid(String),
    /// Not a contact, blocked, or expired. One answer for all three on
    /// purpose: which of them it was is a fact about somebody else.
    Refused,
    NotConfigured,
    TooManyRequests,
    Internal(anyhow::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl IntoResponse for StoryError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            StoryError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "That story is gone.".to_string(),
            ),
            StoryError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            StoryError::Refused => (
                StatusCode::FORBIDDEN,
                "refused",
                "That story is not available.".to_string(),
            ),
            StoryError::NotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "not_configured",
                "Stories are unavailable on this server.".to_string(),
            ),
            StoryError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests. Slow down.".to_string(),
            ),
            StoryError::Internal(error) => {
                tracing::error!(%error, "story request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "Something went wrong. Try again.".to_string(),
                )
            }
        };
        (status, Json(ErrorBody { error, message })).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for StoryError {
    fn from(error: E) -> Self {
        StoryError::Internal(error.into())
    }
}

/// One story, as a reader sees it listed.
#[derive(Debug, Serialize)]
pub struct StoryView {
    pub id: i64,
    pub author_handle: String,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

#[derive(Deserialize)]
pub struct NewStory {
    /// The object already uploaded to the encrypted bucket.
    pub s3_key: String,
    /// Ciphertext length.
    pub size: i64,
}

/// Record a story that has already been uploaded.
///
/// Upload first, record second — the same order attachments use, and for the
/// same reason `§5.3` gives: objects are write-once, so a failed upload should
/// leave no row pointing at something that was never written.
async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<NewStory>,
) -> Result<Json<StoryView>, StoryError> {
    if !state.limits.media.check(&caller.user_id.to_string()) {
        return Err(StoryError::TooManyRequests);
    }
    if request.size <= 0 {
        return Err(StoryError::Invalid("That story is empty.".into()));
    }

    let row = sqlx::query!(
        "INSERT INTO stories (author_id, s3_key, size, expires_at)
         VALUES ($1, $2, $3, now() + INTERVAL '24 hours')
         RETURNING id,
                   (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS created_at_ms,
                   (EXTRACT(EPOCH FROM expires_at) * 1000)::BIGINT AS expires_at_ms",
        caller.user_id,
        request.s3_key,
        request.size
    )
    .fetch_one(&state.db)
    .await?;

    let me = sqlx::query!(
        "SELECT handle::TEXT AS \"handle!\" FROM users WHERE id = $1",
        caller.user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(StoryView {
        id: row.id,
        author_handle: me.handle,
        created_at_ms: row.created_at_ms.unwrap_or(0),
        expires_at_ms: row.expires_at_ms.unwrap_or(0),
    }))
}

/// Live stories from people the caller shares a conversation with.
///
/// Expiry is in the query, so a story stops being listed the moment it expires
/// rather than when some job gets round to it.
async fn list(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<Vec<StoryView>>, StoryError> {
    let hidden = crate::blocks::hidden_authors(&state.db, caller.user_id).await?;
    let rows = sqlx::query!(
        "SELECT s.id, u.handle::TEXT AS \"handle!\",
                (EXTRACT(EPOCH FROM s.created_at) * 1000)::BIGINT AS created_at_ms,
                (EXTRACT(EPOCH FROM s.expires_at) * 1000)::BIGINT AS expires_at_ms
         FROM stories s
         JOIN users u ON u.id = s.author_id
         WHERE s.expires_at > now()
           AND NOT (s.author_id = ANY($2))
           AND (
             s.author_id = $1
             OR EXISTS (
               SELECT 1 FROM conversation_members m1
               JOIN conversation_members m2 ON m1.conversation_id = m2.conversation_id
               WHERE m1.user_id = $1 AND m2.user_id = s.author_id
             )
           )
         ORDER BY s.created_at DESC
         LIMIT 200",
        caller.user_id,
        &hidden
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| StoryView {
                id: r.id,
                author_handle: r.handle,
                created_at_ms: r.created_at_ms.unwrap_or(0),
                expires_at_ms: r.expires_at_ms.unwrap_or(0),
            })
            .collect(),
    ))
}

#[derive(Serialize)]
pub struct StoryUrl {
    pub url: String,
}

/// A time-limited URL for a story's ciphertext.
///
/// The three conditions in the module header are applied here, and all three
/// give the same refusal: which one failed is a fact about somebody else's
/// account or somebody else's blocks.
async fn download_url(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
) -> Result<Json<StoryUrl>, StoryError> {
    if !state.limits.media.check(&caller.user_id.to_string()) {
        return Err(StoryError::TooManyRequests);
    }
    let storage = state.storage.as_ref().ok_or(StoryError::NotConfigured)?;

    let row = sqlx::query!(
        "SELECT s3_key, author_id,
                (EXTRACT(EPOCH FROM expires_at) * 1000)::BIGINT AS expires_at_ms
         FROM stories WHERE id = $1",
        id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(StoryError::NotFound)?;

    // 1. Not expired.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    if !expiry::still_available(row.expires_at_ms.unwrap_or(0), now_ms) {
        return Err(StoryError::Refused);
    }

    if row.author_id != caller.user_id {
        // 2. A contact, by the one definition of the word this server has.
        if !crate::profiles::shares_a_conversation(&state, caller.user_id, row.author_id)
            .await
            .map_err(|_| StoryError::Refused)?
        {
            return Err(StoryError::Refused);
        }
        // 3. Not blocked, either way.
        if crate::blocks::blocked_between(&state.db, caller.user_id, row.author_id).await? {
            return Err(StoryError::Refused);
        }
    }

    // The encrypted bucket, not the media one, and `client_for` is told so
    // explicitly. A story is opaque ciphertext; putting it where profile
    // pictures live would be a category error with a privacy consequence.
    let config = aws_sdk_s3::presigning::PresigningConfig::expires_in(DOWNLOAD_TTL)
        .map_err(|e| StoryError::Internal(anyhow::anyhow!("presign config: {e}")))?;
    let presigned = storage
        .client_for(true)
        .get_object()
        .bucket(storage.encrypted().name())
        .key(&row.s3_key)
        .presigned(config)
        .await
        .map_err(|e| StoryError::Internal(anyhow::anyhow!("presigning a story: {e}")))?;

    Ok(Json(StoryUrl {
        url: presigned.uri().to_string(),
    }))
}
