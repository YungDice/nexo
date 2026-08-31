//! Reporting (BRIEF 13).
//!
//! Blocking answers "I do not want to see this person". It does not answer
//! "this should not be here", and those are different questions with different
//! remedies — the first is a preference, the second is a claim about the
//! content that somebody other than the reporter has to act on.
//!
//! The feed is one global stream with no follow graph to filter it, so whatever
//! the first stranger posts is in front of everyone. That makes somewhere for
//! "this is illegal" to go a precondition for inviting real people, not a
//! feature to add once there are enough of them.
//!
//! # What this deliberately is not
//!
//! There is no moderation queue, no automatic hiding, and no threshold at which
//! something disappears. A report lands in a table an operator reads. Automatic
//! action on an unreviewed report is a tool for whoever files the most reports,
//! and at this size a human reading them is both possible and better.
//!
//! # What the reporter is told
//!
//! That it was received, and nothing else — not whether the subject was already
//! reported, not what happened next. Reporting is not a channel for learning
//! about other people's accounts, and a report that confirms "yes, others
//! reported this too" is one.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::post};
use serde::{Deserialize, Serialize};

use crate::auth::Caller;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/v1/reports", post(create))
}

/// Why a report was refused.
#[derive(Debug)]
pub enum ReportError {
    Invalid(String),
    /// Over the account's rate limit.
    TooManyRequests,
    Internal(anyhow::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl IntoResponse for ReportError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            ReportError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            ReportError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests. Slow down.".to_string(),
            ),
            ReportError::Internal(error) => {
                tracing::error!(%error, "report failed");
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

impl<E: Into<anyhow::Error>> From<E> for ReportError {
    fn from(error: E) -> Self {
        ReportError::Internal(error.into())
    }
}

/// What may be reported, and why.
///
/// Both lists are closed and match the CHECK constraints on the table. A fixed
/// set is what makes reports countable; the free-text note carries the part a
/// list cannot anticipate.
const SUBJECT_KINDS: [&str; 3] = ["post", "comment", "user"];
const REASONS: [&str; 5] = ["spam", "harassment", "illegal", "impersonation", "other"];

#[derive(Deserialize)]
pub struct CreateReportRequest {
    pub subject_kind: String,
    pub subject_id: i64,
    pub reason: String,
    #[serde(default)]
    pub note: Option<String>,
}

async fn create(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<CreateReportRequest>,
) -> Result<StatusCode, ReportError> {
    // Reports are cheap to file and cheap to automate. The same account limit
    // the delivery endpoints use keeps one account from filling the table.
    if !state
        .limits
        .send
        .check(&format!("report:{}", caller.user_id))
    {
        return Err(ReportError::TooManyRequests);
    }

    if !SUBJECT_KINDS.contains(&request.subject_kind.as_str()) {
        return Err(ReportError::Invalid(
            "That is not something reportable.".into(),
        ));
    }
    if !REASONS.contains(&request.reason.as_str()) {
        return Err(ReportError::Invalid(
            "Pick one of the listed reasons.".into(),
        ));
    }

    let note = request
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    if note.as_deref().is_some_and(|n| n.chars().count() > 1000) {
        return Err(ReportError::Invalid(
            "Keep the note under 1000 characters.".into(),
        ));
    }

    // Reporting the same thing twice is not two reports. `DO NOTHING` rather
    // than an error, because the caller does not need to be told they already
    // did this -- and telling them would confirm the subject exists.
    sqlx::query!(
        "INSERT INTO reports (reporter_user_id, subject_kind, subject_id, reason, note)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (reporter_user_id, subject_kind, subject_id) DO NOTHING",
        caller.user_id,
        request.subject_kind,
        request.subject_id,
        request.reason,
        note
    )
    .execute(&state.db)
    .await?;

    // Logged so an operator sees the volume without querying, and can tell a
    // burst from a trickle. No note in the log: it is somebody's free text.
    tracing::info!(
        reporter = caller.user_id,
        kind = %request.subject_kind,
        subject = request.subject_id,
        reason = %request.reason,
        "report filed"
    );

    // No body. There is nothing to say that is not either obvious or a leak.
    Ok(StatusCode::NO_CONTENT)
}
