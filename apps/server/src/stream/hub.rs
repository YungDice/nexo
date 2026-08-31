//! Fan-out: getting an envelope from the socket that accepted it to the
//! sockets that are waiting for it.
//!
//! # Why this is a seam
//!
//! One process can do this with a broadcast channel. More than one cannot: two
//! server instances behind a load balancer each hold half the connections, and
//! a message accepted by one has to reach the other. That is what BRIEF §3
//! specifies Redis pub/sub for.
//!
//! [`Fanout`] is the seam. [`LocalHub`] is the single-process implementation,
//! and it is the *correct* implementation right now, because there is one
//! process (docs/OPS.md Phase 7 runs one systemd unit). A Redis-backed one
//! slots in behind the same trait when there is a second — see the note in
//! `docs/PLAN.md` G5.
//!
//! Building the seam now rather than the Redis client is deliberate: the seam
//! is what makes the change cheap later, and an unused Redis dependency is a
//! dependency to audit for no benefit today.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use nexo_protocol::{ConversationId, ServerEvent};
use tokio::sync::broadcast;

/// How many events a slow subscriber may fall behind before it is dropped.
///
/// A subscriber that lags past this misses events. That is survivable *only*
/// because the client resyncs from its cursor on reconnect — the socket is a
/// latency optimisation, not the source of truth. If it were the only delivery
/// path, this number would have to be a queue with no ceiling, and a slow
/// client would take the server down with it.
const CHANNEL_CAPACITY: usize = 256;

/// Somewhere to publish events and somewhere to subscribe to them.
pub trait Fanout: Send + Sync + 'static {
    /// Publishes to everyone watching `conversation_id`.
    ///
    /// Delivery is best-effort by design: nobody listening is the normal case
    /// (everyone offline), and it must not be an error.
    fn publish(&self, conversation_id: ConversationId, event: ServerEvent);

    /// Subscribes to a conversation.
    fn subscribe(&self, conversation_id: ConversationId) -> broadcast::Receiver<ServerEvent>;
}

/// Single-process fan-out over `tokio::sync::broadcast`.
#[derive(Default)]
pub struct LocalHub {
    // RwLock rather than a Mutex: subscribing and publishing both read far more
    // often than they create, and the write is only the first subscriber to a
    // conversation.
    channels: RwLock<HashMap<ConversationId, broadcast::Sender<ServerEvent>>>,
}

impl std::fmt::Debug for LocalHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.channels.read().map(|c| c.len()).unwrap_or(0);
        f.debug_struct("LocalHub")
            .field("conversations", &count)
            .finish()
    }
}

impl LocalHub {
    /// A hub with no subscribers.
    pub fn new() -> Self {
        Self::default()
    }

    fn sender(&self, conversation_id: ConversationId) -> broadcast::Sender<ServerEvent> {
        if let Some(sender) = self
            .channels
            .read()
            .expect("hub lock")
            .get(&conversation_id)
        {
            return sender.clone();
        }
        self.channels
            .write()
            .expect("hub lock")
            .entry(conversation_id)
            .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0)
            .clone()
    }

    /// How many conversations currently have a channel. Tests and metrics.
    pub fn tracked_conversations(&self) -> usize {
        self.channels.read().expect("hub lock").len()
    }
}

impl Fanout for LocalHub {
    fn publish(&self, conversation_id: ConversationId, event: ServerEvent) {
        // `send` fails only when there are no receivers, which is the ordinary
        // state of a conversation whose members are all offline.
        let _ = self.sender(conversation_id).send(event);
    }

    fn subscribe(&self, conversation_id: ConversationId) -> broadcast::Receiver<ServerEvent> {
        self.sender(conversation_id).subscribe()
    }
}

/// The hub as it is carried in application state.
pub type SharedFanout = Arc<dyn Fanout>;

#[cfg(test)]
mod tests {
    use super::*;
    use nexo_protocol::DeviceId;

    fn envelope(id: i64) -> ServerEvent {
        ServerEvent::Envelope {
            envelope_id: id,
            conversation_id: ConversationId::nil(),
            sender_device_id: DeviceId::nil(),
            epoch: 1,
            ciphertext: "aabb".into(),
            is_commit: false,
            server_timestamp_ms: 0,
        }
    }

    #[tokio::test]
    async fn a_subscriber_receives_what_is_published() {
        let hub = LocalHub::new();
        let id = ConversationId::new_v4();
        let mut rx = hub.subscribe(id);

        hub.publish(id, envelope(1));

        assert_eq!(rx.recv().await.unwrap(), envelope(1));
    }

    #[tokio::test]
    async fn every_subscriber_to_a_conversation_receives_it() {
        // Fan-out, not hand-off: two devices in the same conversation both get
        // the message.
        let hub = LocalHub::new();
        let id = ConversationId::new_v4();
        let mut one = hub.subscribe(id);
        let mut two = hub.subscribe(id);

        hub.publish(id, envelope(7));

        assert_eq!(one.recv().await.unwrap(), envelope(7));
        assert_eq!(two.recv().await.unwrap(), envelope(7));
    }

    #[tokio::test]
    async fn conversations_are_isolated() {
        // Subscribing to one conversation must not leak another's traffic --
        // the ciphertext is opaque, but *that a message happened* is metadata.
        let hub = LocalHub::new();
        let mine = ConversationId::new_v4();
        let theirs = ConversationId::new_v4();
        let mut rx = hub.subscribe(mine);

        hub.publish(theirs, envelope(1));

        assert!(
            rx.try_recv().is_err(),
            "an event leaked between conversations"
        );
    }

    #[tokio::test]
    async fn publishing_with_nobody_listening_is_not_an_error() {
        // The ordinary case: everyone is offline.
        let hub = LocalHub::new();
        hub.publish(ConversationId::new_v4(), envelope(1));
    }

    #[tokio::test]
    async fn a_dropped_subscriber_does_not_block_the_next_publish() {
        let hub = LocalHub::new();
        let id = ConversationId::new_v4();
        {
            let _rx = hub.subscribe(id);
        }
        hub.publish(id, envelope(2));

        let mut rx = hub.subscribe(id);
        hub.publish(id, envelope(3));
        assert_eq!(rx.recv().await.unwrap(), envelope(3));
    }

    #[tokio::test]
    async fn a_subscriber_that_falls_far_behind_is_lagged_rather_than_unbounded() {
        // The socket is a latency optimisation; a client that misses events
        // resyncs from its cursor. Buffering without limit instead would let one
        // slow client exhaust the server's memory.
        let hub = LocalHub::new();
        let id = ConversationId::new_v4();
        let mut rx = hub.subscribe(id);

        for i in 0..(CHANNEL_CAPACITY as i64 + 10) {
            hub.publish(id, envelope(i));
        }

        assert!(
            matches!(
                rx.try_recv(),
                Err(broadcast::error::TryRecvError::Lagged(_))
            ),
            "expected the channel to report lag rather than grow without bound"
        );
    }
}
