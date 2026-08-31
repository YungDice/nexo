//! The WebSocket at `/v1/stream`.
//!
//! What crosses it is [`ServerEvent`] and [`ClientEvent`] as JSON. Ciphertext
//! is hex inside those, and the server has no key for it — the socket is a
//! delivery path, not a decryption point (rule 4).
//!
//! # The socket is not the source of truth
//!
//! Every envelope also lands in the database, and every client keeps a cursor.
//! A dropped connection, a missed event, a subscriber that lagged: all of them
//! are repaired by `GET /v1/conversations/{id}/sync` on reconnect. That is what
//! lets the fan-out channel have a bounded buffer, and what makes it safe to
//! drop a slow client rather than buffer for it forever.

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::Response;
use axum::{Router, routing::get};
use futures_util::{SinkExt, StreamExt};
use nexo_protocol::{ClientEvent, ConversationId, ServerEvent};
use tokio::sync::mpsc;

use crate::auth::bearer::Caller;
use crate::state::AppState;

pub mod hub;

/// How many events may queue for one socket before it is disconnected.
///
/// Smaller than the hub's buffer on purpose: this is one client's slowness, and
/// the remedy — reconnect and sync — is cheap.
const SOCKET_QUEUE: usize = 64;

/// The stream route.
pub fn router() -> Router<AppState> {
    Router::new().route("/v1/stream", get(upgrade))
}

/// Authenticates, then upgrades.
///
/// The [`Caller`] extractor runs **before** the upgrade, so an unauthenticated
/// connection is refused with an ordinary 401 rather than being accepted and
/// then closed. A client that got a socket and then lost it cannot tell a bad
/// token from a flaky network.
async fn upgrade(
    State(state): State<AppState>,
    caller: Caller,
    upgrade: WebSocketUpgrade,
) -> Response {
    upgrade.on_upgrade(move |socket| run(socket, state, caller))
}

/// One connection, for as long as it lasts.
async fn run(socket: WebSocket, state: AppState, caller: Caller) {
    let conversations = match member_conversations(&state, caller.user_id).await {
        Ok(list) => list,
        Err(error) => {
            tracing::error!(%error, user_id = caller.user_id, "could not list conversations");
            return;
        }
    };

    tracing::debug!(
        user_id = caller.user_id,
        conversations = conversations.len(),
        "stream opened"
    );

    let (mut sink, mut incoming) = socket.split();
    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<ServerEvent>(SOCKET_QUEUE);

    // One task per conversation, forwarding hub events onto this socket's
    // single queue. They all end when `outgoing_tx` is dropped.
    let mut forwarders = Vec::new();
    for conversation_id in conversations {
        let mut subscription = state.fanout.subscribe(conversation_id);
        let tx = outgoing_tx.clone();
        forwarders.push(tokio::spawn(async move {
            loop {
                match subscription.recv().await {
                    Ok(event) => {
                        // A full queue means this client is not keeping up.
                        // Drop the connection rather than the server's memory;
                        // it will reconnect and sync.
                        if tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::debug!(missed, "subscriber lagged; the client will resync");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }));
    }
    drop(outgoing_tx);

    loop {
        tokio::select! {
            // Server -> client.
            event = outgoing_rx.recv() => {
                let Some(event) = event else { break };
                let Ok(json) = serde_json::to_string(&event) else { continue };
                if sink.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }

            // Client -> server.
            message = incoming.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientEvent>(&text) {
                            Ok(event) => {
                                if let Err(error) = handle(&state, &caller, event).await {
                                    tracing::warn!(%error, "handling a client event failed");
                                }
                            }
                            // A message we cannot parse is not worth closing
                            // over: an older client sending a variant this
                            // build has never heard of is a normal state.
                            Err(error) => tracing::debug!(%error, "unparseable client event"),
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        tracing::debug!(%error, "stream error");
                        break;
                    }
                }
            }
        }
    }

    for forwarder in forwarders {
        forwarder.abort();
    }
    tracing::debug!(user_id = caller.user_id, "stream closed");
}

/// Acts on something the client sent.
async fn handle(
    state: &AppState,
    caller: &Caller,
    event: ClientEvent,
) -> Result<(), anyhow::Error> {
    match event {
        ClientEvent::Ack {
            conversation_id,
            envelope_id,
        } => {
            acknowledge(&state.db, caller.user_id, conversation_id, envelope_id).await?;
            Ok(())
        }

        ClientEvent::Typing { conversation_id } => {
            // Typing is opt-out-able (§6.1) and carries no content, so it is
            // published without touching the database at all.
            state.fanout.publish(
                conversation_id,
                ServerEvent::Typing {
                    conversation_id,
                    user_id: caller.user_id,
                },
            );
            Ok(())
        }

        ClientEvent::Ping => Ok(()),
    }
}

/// Marks every envelope up to `envelope_id` delivered for this conversation.
///
/// §4.3: delivered ciphertext is deleted on acknowledgement, and the sweep that
/// does the deleting reads `delivered_at`. This is what sets it.
///
/// Membership is re-checked in the statement rather than trusted from
/// connection time, because a socket can outlive a removal. Returns how many
/// rows it marked, so a caller — or a test — can tell "nothing to do" from
/// "not allowed".
pub async fn acknowledge(
    db: &sqlx::PgPool,
    user_id: i64,
    conversation_id: ConversationId,
    envelope_id: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE envelopes SET delivered_at = now()
         WHERE conversation_id = $1
           AND id <= $2
           AND delivered_at IS NULL
           AND EXISTS (
               SELECT 1 FROM conversation_members
               WHERE conversation_id = $1 AND user_id = $3
           )",
        conversation_id,
        envelope_id,
        user_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

/// Every conversation this user is a member of.
async fn member_conversations(
    state: &AppState,
    user_id: i64,
) -> Result<Vec<ConversationId>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT conversation_id FROM conversation_members WHERE user_id = $1",
        user_id
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows.into_iter().map(|r| r.conversation_id).collect())
}
