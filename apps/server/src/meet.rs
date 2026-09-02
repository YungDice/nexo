//! Meet&Greet: a map of strangers, and one message to reach one of them.
//!
//! Everything this module stores is readable by the server and by every
//! signed-in person — a pin, a headline, a character. That is the design, and
//! the agreement screen says so in those words, because rule 5 makes a feature
//! that implies privacy it does not have worse than one that never claimed it.
//!
//! Four things live here rather than in the client, and each for the same
//! reason `blocks.rs` gives in its own header: *a block the client applies is a
//! promise the product cannot keep.*
//!
//! **Coarsening.** [`coarsen`] runs on every write and the submitted figure is
//! never stored. A client that promised to round before sending would be a
//! client anybody can replace with one that does not.
//!
//! **Blocks.** [`pins`] spends `blocks::hidden_authors`, so a blocked person is
//! absent from your map and you from theirs. Reused rather than reimplemented:
//! two mechanisms for one rule is one of them being wrong later.
//!
//! **The one-message cap.** Enforced in the delivery service's `send`, not
//! here and not in the app. A cap the client applies is the same empty promise
//! as a client-side block.
//!
//! **Consent.** Versioned, so changing the words re-asks rather than
//! inheriting agreement to something nobody read.
//!
//! # What is deliberately absent
//!
//! Nexo never reads device location. There is no endpoint that accepts an
//! accuracy, a heading or a timestamp of measurement, and the table has nowhere
//! to put one. A pin is a claim somebody typed.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get, routing::post};
use serde::{Deserialize, Serialize};

use nexo_protocol::{MeetProfile, MeetProfileUpdate, MeetRequest, MeetRequestState};

use crate::auth::Caller;
use crate::state::AppState;

/// The agreement's version. Bump it when its words change and everyone is
/// asked again.
pub const CONSENT_VERSION: i32 = 1;

/// The most pins one request will return.
const PAGE: i64 = 500;

/// The longest a headline may be, matching the table's CHECK.
const HEADLINE_MAX: usize = 80;

/// The largest a character config may serialise to, matching the table's CHECK.
const CHAR_CONFIG_MAX: usize = 2048;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/meet/pins", get(pins))
        .route("/v1/meet/me", get(me).put(set_me).delete(leave))
        .route("/v1/meet/consent", post(consent))
        .route("/v1/meet/requests", get(inbox).post(open_request))
        .route("/v1/meet/requests/{id}/accept", post(accept))
        .route("/v1/meet/requests/{id}/decline", post(decline))
}

// ------------------------------------------------------------ coarsening ---

/// The grid a pin is snapped to, in degrees. About 25 km at the equator.
const GRID: f64 = 0.25;

/// How far the jitter may move a pin from its grid point, in degrees.
const JITTER: f64 = GRID / 2.0;

/// Snap a submitted pin to a grid, then move it by a fixed amount of that
/// person's own.
///
/// The jitter is derived from `user_id` and from nothing else, and that is the
/// part worth being careful about. A jitter re-rolled on every write would let
/// anybody who watches a pin across several saves average the offsets away and
/// recover the true grid point; one derived from the account is the same offset
/// every time, so repeated writes disclose nothing that the first one did not.
///
/// It is not a hash for secrecy — it does not need to be, since the output is
/// public — it needs only to be stable, spread evenly, and free of any input a
/// caller controls.
fn coarsen(user_id: i64, lat: f64, lon: f64) -> (f64, f64) {
    let snap = |v: f64| (v / GRID).round() * GRID;

    // Two decorrelated offsets in [-JITTER, JITTER), from one id. The
    // multipliers are arbitrary odd constants; what matters is that latitude
    // and longitude do not move together, which would make the offset visible
    // as a diagonal.
    let spread = |salt: u64| {
        let mut h = (user_id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt;
        h ^= h >> 33;
        h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        h ^= h >> 33;
        // [0, 1) -> [-JITTER, JITTER)
        ((h >> 11) as f64 / (1u64 << 53) as f64) * 2.0 * JITTER - JITTER
    };

    let lat = (snap(lat) + spread(0x1)).clamp(-85.0, 85.0);
    let lon = (snap(lon) + spread(0x2)).clamp(-180.0, 180.0);
    (lat, lon)
}

// ---------------------------------------------------------------- errors ---

/// Why a Meet&Greet request was refused.
#[derive(Debug)]
pub enum MeetError {
    /// No such pin, request, or person.
    NotFound,
    /// The request was malformed.
    Invalid(String),
    /// Blocked, or not the caller's to answer.
    Refused,
    /// The agreement has not been accepted, or its version has moved on.
    ConsentRequired,
    /// Over the account's rate limit.
    TooManyRequests,
    /// Something the caller cannot act on.
    Internal(anyhow::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl IntoResponse for MeetError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            MeetError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "That is not there.".to_string(),
            ),
            MeetError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            MeetError::Refused => (
                StatusCode::FORBIDDEN,
                "refused",
                "That is not available.".to_string(),
            ),
            MeetError::ConsentRequired => (
                StatusCode::FORBIDDEN,
                "consent_required",
                "Read and accept the Meet&Greet agreement first.".to_string(),
            ),
            MeetError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests. Slow down.".to_string(),
            ),
            MeetError::Internal(error) => {
                tracing::error!(%error, "meet request failed");
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

impl<E: Into<anyhow::Error>> From<E> for MeetError {
    fn from(error: E) -> Self {
        MeetError::Internal(error.into())
    }
}

// ------------------------------------------------------------------ pins ---

#[derive(Deserialize)]
struct PinsQuery {
    /// Continue after this handle. Cursor rather than offset: pins move.
    after: Option<String>,
}

/// Every active pin, minus the people blocked in either direction.
async fn pins(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<PinsQuery>,
) -> Result<Json<Vec<MeetProfile>>, MeetError> {
    if !state.limits.meet.check(&caller.user_id.to_string()) {
        return Err(MeetError::TooManyRequests);
    }

    let hidden = crate::blocks::hidden_authors(&state.db, caller.user_id).await?;
    let after = query.after.unwrap_or_default();

    let rows = sqlx::query!(
        "SELECT u.handle, u.display_name, m.lat, m.lon, m.headline, m.char_config,
                (EXTRACT(EPOCH FROM m.updated_at) * 1000)::BIGINT AS updated_at_ms
         FROM meet_profiles m
         JOIN users u ON u.id = m.user_id
         WHERE m.active
           AND NOT (m.user_id = ANY($1))
           AND u.handle > $2
         ORDER BY u.handle
         LIMIT $3",
        &hidden,
        after,
        PAGE
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| MeetProfile {
                handle: r.handle,
                display_name: r.display_name,
                lat: r.lat,
                lon: r.lon,
                headline: r.headline,
                char_config: r.char_config,
                updated_at_ms: r.updated_at_ms.unwrap_or(0),
            })
            .collect(),
    ))
}

/// My own pin, or 404 if I am not on the map.
async fn me(State(state): State<AppState>, caller: Caller) -> Result<Json<MeetProfile>, MeetError> {
    let row = sqlx::query!(
        "SELECT u.handle, u.display_name, m.lat, m.lon, m.headline, m.char_config,
                (EXTRACT(EPOCH FROM m.updated_at) * 1000)::BIGINT AS updated_at_ms
         FROM meet_profiles m
         JOIN users u ON u.id = m.user_id
         WHERE m.user_id = $1 AND m.active",
        caller.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(MeetError::NotFound)?;

    Ok(Json(MeetProfile {
        handle: row.handle,
        display_name: row.display_name,
        lat: row.lat,
        lon: row.lon,
        headline: row.headline,
        char_config: row.char_config,
        updated_at_ms: row.updated_at_ms.unwrap_or(0),
    }))
}

/// Place or move my pin, and set what goes with it.
async fn set_me(
    State(state): State<AppState>,
    caller: Caller,
    Json(update): Json<MeetProfileUpdate>,
) -> Result<StatusCode, MeetError> {
    if !state.limits.meet.check(&caller.user_id.to_string()) {
        return Err(MeetError::TooManyRequests);
    }
    require_consent(&state, caller.user_id).await?;

    if let Some(headline) = &update.headline
        && headline.chars().count() > HEADLINE_MAX
    {
        return Err(MeetError::Invalid(format!(
            "A headline is at most {HEADLINE_MAX} characters."
        )));
    }
    if let Some(config) = &update.char_config
        && serde_json::to_vec(config)
            .map(|v| v.len())
            .unwrap_or(usize::MAX)
            > CHAR_CONFIG_MAX
    {
        return Err(MeetError::Invalid("That character is too complex.".into()));
    }

    // A pin needs both halves or neither: half a coordinate is not a place.
    let pin = match (update.lat, update.lon) {
        (Some(lat), Some(lon)) => {
            if !(-85.0..=85.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                return Err(MeetError::Invalid("That is not on the map.".into()));
            }
            // Coarsened here, and only the result goes anywhere. The submitted
            // pair is not logged, not returned, and not stored.
            Some(coarsen(caller.user_id, lat, lon))
        }
        (None, None) => None,
        _ => {
            return Err(MeetError::Invalid(
                "A pin needs both a latitude and a longitude.".into(),
            ));
        }
    };

    let existing = sqlx::query!(
        "SELECT 1 AS \"present!\" FROM meet_profiles WHERE user_id = $1",
        caller.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    if !existing {
        let (lat, lon) = pin
            .ok_or_else(|| MeetError::Invalid("Place your pin on the map before saving.".into()))?;
        let config = update
            .char_config
            .clone()
            .ok_or_else(|| MeetError::Invalid("Build your character first.".into()))?;
        sqlx::query!(
            "INSERT INTO meet_profiles (user_id, lat, lon, headline, char_config, active)
             VALUES ($1, $2, $3, $4, $5, COALESCE($6, TRUE))",
            caller.user_id,
            lat,
            lon,
            update.headline,
            config,
            update.active
        )
        .execute(&state.db)
        .await?;
        return Ok(StatusCode::CREATED);
    }

    sqlx::query!(
        "UPDATE meet_profiles
         SET lat        = COALESCE($2, lat),
             lon        = COALESCE($3, lon),
             headline   = COALESCE($4, headline),
             char_config = COALESCE($5, char_config),
             active     = COALESCE($6, active),
             updated_at = now()
         WHERE user_id = $1",
        caller.user_id,
        pin.map(|p| p.0),
        pin.map(|p| p.1),
        update.headline,
        update.char_config,
        update.active
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Come off the map, keeping the character.
async fn leave(State(state): State<AppState>, caller: Caller) -> Result<StatusCode, MeetError> {
    sqlx::query!(
        "UPDATE meet_profiles SET active = FALSE, updated_at = now() WHERE user_id = $1",
        caller.user_id
    )
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// --------------------------------------------------------------- consent ---

#[derive(Deserialize)]
struct ConsentRequest {
    version: i32,
}

async fn consent(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<ConsentRequest>,
) -> Result<StatusCode, MeetError> {
    if request.version != CONSENT_VERSION {
        return Err(MeetError::Invalid(
            "That is not the current agreement.".into(),
        ));
    }
    sqlx::query!(
        "INSERT INTO meet_consent (user_id, version) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET version = $2, accepted_at = now()",
        caller.user_id,
        request.version
    )
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Refuse anything that puts a person on the map before they have agreed to
/// what that means.
async fn require_consent(state: &AppState, user_id: i64) -> Result<(), MeetError> {
    let ok = sqlx::query!(
        "SELECT 1 AS \"present!\" FROM meet_consent WHERE user_id = $1 AND version >= $2",
        user_id,
        CONSENT_VERSION
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();
    if ok {
        Ok(())
    } else {
        Err(MeetError::ConsentRequired)
    }
}

// -------------------------------------------------------------- requests ---

#[derive(Deserialize)]
struct OpenRequest {
    handle: String,
    conversation_id: uuid::Uuid,
}

/// Record that a conversation is an intro, so the cap applies to it.
///
/// The conversation already exists by the time this is called — the client
/// opens it through the ordinary path, sends its one message, and then says so
/// here. Doing it in that order means a failure leaves an ordinary
/// conversation rather than a request pointing at nothing.
async fn open_request(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<OpenRequest>,
) -> Result<Json<MeetRequest>, MeetError> {
    if !state
        .limits
        .meet_requests
        .check(&caller.user_id.to_string())
    {
        return Err(MeetError::TooManyRequests);
    }
    require_consent(&state, caller.user_id).await?;

    let other = sqlx::query!("SELECT id FROM users WHERE handle = $1", request.handle)
        .fetch_optional(&state.db)
        .await?
        .ok_or(MeetError::NotFound)?;

    if other.id == caller.user_id {
        return Err(MeetError::Invalid("That is you.".into()));
    }
    if crate::blocks::blocked_between(&state.db, caller.user_id, other.id).await? {
        return Err(MeetError::Refused);
    }

    // The UNIQUE constraint decides this, not a check above it: a retry and a
    // second attempt look identical from here, and only the database sees both.
    let row = sqlx::query!(
        "INSERT INTO meet_requests (from_id, to_id, conversation_id, state)
         VALUES ($1, $2, $3, 'pending')
         ON CONFLICT (from_id, to_id) DO NOTHING
         RETURNING id, (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS created_at_ms",
        caller.user_id,
        other.id,
        request.conversation_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| MeetError::Invalid("You have already written to them.".into()))?;

    Ok(Json(MeetRequest {
        id: row.id,
        from_handle: String::new(),
        conversation_id: request.conversation_id,
        state: MeetRequestState::Pending,
        created_at_ms: row.created_at_ms.unwrap_or(0),
    }))
}

/// What is waiting for me.
async fn inbox(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<Vec<MeetRequest>>, MeetError> {
    let hidden = crate::blocks::hidden_authors(&state.db, caller.user_id).await?;
    let rows = sqlx::query!(
        "SELECT r.id, u.handle, r.conversation_id,
                (EXTRACT(EPOCH FROM r.created_at) * 1000)::BIGINT AS created_at_ms
         FROM meet_requests r
         JOIN users u ON u.id = r.from_id
         WHERE r.to_id = $1 AND r.state = 'pending'
           AND NOT (r.from_id = ANY($2))
         ORDER BY r.created_at DESC
         LIMIT $3",
        caller.user_id,
        &hidden,
        PAGE
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| MeetRequest {
                id: r.id,
                from_handle: r.handle,
                conversation_id: r.conversation_id,
                state: MeetRequestState::Pending,
                created_at_ms: r.created_at_ms.unwrap_or(0),
            })
            .collect(),
    ))
}

async fn accept(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
) -> Result<StatusCode, MeetError> {
    resolve(&state, caller.user_id, id, "accepted").await
}

async fn decline(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
) -> Result<StatusCode, MeetError> {
    resolve(&state, caller.user_id, id, "declined").await
}

/// Answer an intro. Only its recipient may, and only while it is pending.
///
/// Accepting lifts the one-message cap by moving the row off `pending`;
/// declining lifts it too, and that is deliberate. The alternative — a
/// declined request that keeps the conversation frozen — leaves a dead thread
/// nobody can act on, and the person who declined has `blocks` if they want
/// the sender gone rather than merely refused.
async fn resolve(
    state: &AppState,
    caller: i64,
    id: i64,
    to: &str,
) -> Result<StatusCode, MeetError> {
    let updated = sqlx::query!(
        "UPDATE meet_requests
         SET state = $3, resolved_at = now()
         WHERE id = $1 AND to_id = $2 AND state = 'pending'
         RETURNING id",
        id,
        caller,
        to
    )
    .fetch_optional(&state.db)
    .await?;

    // The same answer for "no such request" and "not yours": which of the two
    // it is would be a fact about somebody else's inbox.
    updated.ok_or(MeetError::NotFound)?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stored pin is never the submitted one.
    #[test]
    fn a_pin_is_moved_off_what_was_sent() {
        let (lat, lon) = coarsen(42, 47.3769, 8.5417);
        assert_ne!(lat, 47.3769);
        assert_ne!(lon, 8.5417);
    }

    /// The property the whole scheme rests on.
    ///
    /// A jitter rolled fresh on each write could be averaged away by anyone
    /// who watched a pin being saved a few times. The same account must
    /// therefore land on the same offset every time, so that saving twice
    /// discloses exactly what saving once did.
    #[test]
    fn the_same_account_is_jittered_identically_every_time() {
        let first = coarsen(1234, 47.3769, 8.5417);
        let second = coarsen(1234, 47.3769, 8.5417);
        assert_eq!(first, second);

        // And a nearby point in the same cell lands in exactly the same place,
        // which is what makes the grid do any work at all.
        let near = coarsen(1234, 47.3800, 8.5400);
        assert_eq!(first, near);
    }

    #[test]
    fn two_accounts_in_one_cell_are_not_stacked_on_each_other() {
        assert_ne!(coarsen(1, 47.3769, 8.5417), coarsen(2, 47.3769, 8.5417));
    }

    /// The offset must stay inside the cell it was drawn for, or a pin could
    /// drift into a neighbouring one and the grid would be doing nothing.
    #[test]
    fn the_jitter_stays_within_half_a_cell() {
        for id in 0..500i64 {
            let (lat, lon) = coarsen(id, 0.0, 0.0);
            assert!(lat.abs() <= JITTER + f64::EPSILON, "lat {lat} for {id}");
            assert!(lon.abs() <= JITTER + f64::EPSILON, "lon {lon} for {id}");
        }
    }

    #[test]
    fn a_coarsened_pin_is_always_on_the_map() {
        for (lat, lon) in [(85.0, 180.0), (-85.0, -180.0), (0.0, 0.0)] {
            let (a, b) = coarsen(7, lat, lon);
            assert!((-85.0..=85.0).contains(&a), "lat {a}");
            assert!((-180.0..=180.0).contains(&b), "lon {b}");
        }
    }
}
