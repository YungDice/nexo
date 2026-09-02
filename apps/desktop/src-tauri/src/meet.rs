//! Meet&Greet, across the IPC seam.
//!
//! Nothing new crosses here. A pin, a headline, a handle and a character
//! config are all plaintext already and all readable by the server; there is
//! no key material anywhere in this file, so invariant 2 is untouched.
//!
//! The one thing worth stating plainly: **the character config is opaque on
//! this side too.** It is carried as a JSON value from the server to the
//! WebView and back, and neither Rust nor the server parses it. The renderer
//! in the page is the only thing that reads it, which is what keeps a
//! character out of object storage and out of anything that would have to
//! moderate an image.

use serde::{Deserialize, Serialize};
use tauri::State;

use nexo_client::meet::{self, Context};
use nexo_protocol::MeetProfileUpdate;

use crate::client::ClientState;

/// Why a Meet&Greet call failed, as the page sees it.
#[derive(Debug, Serialize)]
pub struct MeetErrorView {
    pub kind: &'static str,
    pub message: String,
}

fn failure(kind: &'static str, message: impl Into<String>) -> MeetErrorView {
    MeetErrorView {
        kind,
        message: message.into(),
    }
}

impl From<meet::MeetError> for MeetErrorView {
    fn from(error: meet::MeetError) -> Self {
        use nexo_client::transport::TransportError;
        // The detail goes to the log; the page gets the summary.
        tracing::warn!(%error, "meet call failed");
        match &error {
            meet::MeetError::Transport(TransportError::Unreachable(_)) => failure(
                "unreachable",
                "Can't reach the server. This is the map as it was.",
            ),
            meet::MeetError::Transport(TransportError::InvalidCredentials) => {
                failure("signed_out", "Your session expired. Sign in again.")
            }
            meet::MeetError::Transport(TransportError::NotFound) => {
                failure("not_found", "That is not there.")
            }
            meet::MeetError::Transport(TransportError::Rejected(detail)) => {
                failure("rejected", detail.clone())
            }
            _ => failure("internal", "Something went wrong. Try again."),
        }
    }
}

/// Runs a blocking closure against the signed-in client.
///
/// One helper, so no command can forget the lock or the `spawn_blocking`.
async fn with_client<T, F>(state: &ClientState, work: F) -> Result<T, MeetErrorView>
where
    T: Send + 'static,
    F: FnOnce(&crate::client::LoggedIn) -> Result<T, MeetErrorView> + Send + 'static,
{
    let handle = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let guard = handle
            .lock()
            .map_err(|_| failure("internal", "The client is unavailable."))?;
        let client = guard
            .as_ref()
            .ok_or_else(|| failure("signed_out", "You are not signed in."))?;
        work(client)
    })
    .await
    .map_err(|_| failure("internal", "That did not finish."))?
}

/// One pin, as the page draws it.
///
/// Flattened out of the store's row rather than passed through, so the page
/// never sees a database shape and `char_config` arrives as JSON rather than
/// as a string the page would have to parse itself.
#[derive(Debug, Serialize)]
pub struct PinView {
    pub handle: String,
    pub display_name: String,
    pub lat: f64,
    pub lon: f64,
    pub headline: Option<String>,
    pub char_config: serde_json::Value,
    pub updated_at_ms: i64,
}

/// The map, and how old it is.
#[derive(Debug, Serialize)]
pub struct MapView {
    pub pins: Vec<PinView>,
    pub fetched_at_ms: i64,
    /// True when the server could not be reached and this is the cached copy.
    /// The page says so rather than presenting old pins as current.
    pub stale: bool,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Everyone on the map, from the server when it can be reached and from the
/// cache when it cannot.
#[tauri::command]
pub async fn meet_pins(state: State<'_, ClientState>) -> Result<MapView, MeetErrorView> {
    with_client(&state, |client| {
        let map = meet::map(
            &Context {
                transport: &client.transport,
                store: &client.store,
            },
            now_ms(),
        )?;
        Ok(MapView {
            pins: map
                .pins
                .into_iter()
                .map(|p| PinView {
                    handle: p.handle,
                    display_name: p.display_name,
                    lat: p.lat,
                    lon: p.lon,
                    headline: p.headline,
                    // Stored as text; handed over as JSON. A parse failure
                    // means a config this build cannot read, and `null` is a
                    // fallback the renderer already has to handle.
                    char_config: serde_json::from_str(&p.char_config)
                        .unwrap_or(serde_json::Value::Null),
                    updated_at_ms: p.updated_at_ms,
                })
                .collect(),
            fetched_at_ms: map.fetched_at_ms,
            stale: map.stale,
        })
    })
    .await
}

/// My own pin, or `null` when I am not on the map.
#[tauri::command]
pub async fn meet_me(state: State<'_, ClientState>) -> Result<Option<PinView>, MeetErrorView> {
    with_client(&state, |client| {
        let mine = meet::me(&Context {
            transport: &client.transport,
            store: &client.store,
        })?;
        Ok(mine.map(|p| PinView {
            handle: p.handle,
            display_name: p.display_name,
            lat: p.lat,
            lon: p.lon,
            headline: p.headline,
            char_config: p.char_config,
            updated_at_ms: p.updated_at_ms,
        }))
    })
    .await
}

/// What the studio sends when a pin is placed or a character saved.
#[derive(Debug, Deserialize)]
pub struct SetMeRequest {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub headline: Option<String>,
    pub char_config: Option<serde_json::Value>,
    pub active: Option<bool>,
}

/// Place or move my pin.
///
/// Answers with what the server stored, which is deliberately not what was
/// sent: the pin is coarsened on write, and the page draws the answer rather
/// than the request or it would show a precision that does not exist.
#[tauri::command]
pub async fn meet_set_me(
    state: State<'_, ClientState>,
    request: SetMeRequest,
) -> Result<Option<PinView>, MeetErrorView> {
    with_client(&state, move |client| {
        let update = MeetProfileUpdate {
            lat: request.lat,
            lon: request.lon,
            headline: request.headline,
            char_config: request.char_config,
            active: request.active,
        };
        let stored = meet::set_me(
            &Context {
                transport: &client.transport,
                store: &client.store,
            },
            &update,
        )?;
        Ok(stored.map(|p| PinView {
            handle: p.handle,
            display_name: p.display_name,
            lat: p.lat,
            lon: p.lon,
            headline: p.headline,
            char_config: p.char_config,
            updated_at_ms: p.updated_at_ms,
        }))
    })
    .await
}

/// Come off the map. The character survives.
#[tauri::command]
pub async fn meet_leave(state: State<'_, ClientState>) -> Result<(), MeetErrorView> {
    with_client(&state, |client| {
        meet::leave(&Context {
            transport: &client.transport,
            store: &client.store,
        })?;
        Ok(())
    })
    .await
}

/// Accept the agreement at a version.
#[tauri::command]
pub async fn meet_consent(
    state: State<'_, ClientState>,
    version: i32,
) -> Result<(), MeetErrorView> {
    with_client(&state, move |client| {
        meet::accept_agreement(
            &Context {
                transport: &client.transport,
                store: &client.store,
            },
            version,
        )?;
        Ok(())
    })
    .await
}

/// One intro waiting for an answer.
#[derive(Debug, Serialize)]
pub struct RequestView {
    pub id: i64,
    pub from_handle: String,
    pub conversation_id: String,
    pub created_at_ms: i64,
}

/// The intro inbox.
#[tauri::command]
pub async fn meet_requests(
    state: State<'_, ClientState>,
) -> Result<Vec<RequestView>, MeetErrorView> {
    with_client(&state, |client| {
        let requests = meet::requests(&Context {
            transport: &client.transport,
            store: &client.store,
        })?;
        Ok(requests
            .into_iter()
            .map(|r| RequestView {
                id: r.id,
                from_handle: r.from_handle,
                conversation_id: r.conversation_id.to_string(),
                created_at_ms: r.created_at_ms,
            })
            .collect())
    })
    .await
}

/// Mark a conversation as an intro, after its one message has been sent.
///
/// The ordering is the caller's and it matters: the page opens the
/// conversation with the existing `start_conversation`, sends one message
/// through the existing path, and only then calls this. Doing it the other way
/// would leave a request pointing at a conversation that does not exist.
///
/// Reusing the ordinary conversation path is the whole point — MLS group
/// creation, KeyPackage consumption and Welcome delivery already work, and a
/// second copy of them for this feature would be a second thing to get wrong.
#[tauri::command]
pub async fn meet_send_request(
    state: State<'_, ClientState>,
    handle: String,
    conversation_id: String,
) -> Result<RequestView, MeetErrorView> {
    with_client(&state, move |client| {
        let request = meet::open_request(
            &Context {
                transport: &client.transport,
                store: &client.store,
            },
            &handle,
            &conversation_id,
        )?;
        Ok(RequestView {
            id: request.id,
            from_handle: request.from_handle,
            conversation_id: request.conversation_id.to_string(),
            created_at_ms: request.created_at_ms,
        })
    })
    .await
}

/// Accept an intro, which lifts the one-message cap.
#[tauri::command]
pub async fn meet_accept_request(
    state: State<'_, ClientState>,
    id: i64,
) -> Result<(), MeetErrorView> {
    with_client(&state, move |client| {
        meet::answer(
            &Context {
                transport: &client.transport,
                store: &client.store,
            },
            id,
            true,
        )?;
        Ok(())
    })
    .await
}

/// Report somebody.
///
/// `subject_kind` and `subject_id` rather than a handle, because the server's
/// reports table covers posts and comments too and this is the first caller of
/// an endpoint that predates the map. The card resolves a handle to an id
/// through the profile it is already showing.
#[tauri::command]
pub async fn meet_report(
    state: State<'_, ClientState>,
    subject_kind: String,
    subject_id: i64,
    reason: String,
    note: Option<String>,
) -> Result<(), MeetErrorView> {
    with_client(&state, move |client| {
        meet::report(
            &Context {
                transport: &client.transport,
                store: &client.store,
            },
            &subject_kind,
            subject_id,
            &reason,
            note.as_deref(),
        )?;
        Ok(())
    })
    .await
}

/// Decline an intro.
#[tauri::command]
pub async fn meet_decline_request(
    state: State<'_, ClientState>,
    id: i64,
) -> Result<(), MeetErrorView> {
    with_client(&state, move |client| {
        meet::answer(
            &Context {
                transport: &client.transport,
                store: &client.store,
            },
            id,
            false,
        )?;
        Ok(())
    })
    .await
}
