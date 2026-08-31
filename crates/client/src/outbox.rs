//! The offline queue (M8).
//!
//! Its definition of done is one sentence: *network killed mid-send; the
//! message delivers on reconnect, once.* Both halves are load-bearing, and they
//! pull in different directions.
//!
//! # Why the queue holds ciphertext
//!
//! MLS ratchets forward on every encryption. The bytes for a given message
//! exist exactly once; encrypting the same text again produces a *different*
//! message at the next generation, and doing that on every retry would burn
//! generations and eventually desynchronise this device from the group. So a
//! message is encrypted the moment it is queued, and every attempt afterwards
//! sends those same bytes.
//!
//! That has a consequence worth stating: **the plaintext is gone by the time
//! the queue exists.** A queued message cannot be edited. The `body` column is
//! a preview for the UI, not something the retry re-encrypts.
//!
//! # Why "once" needs the server's help
//!
//! A client cannot distinguish a request that never arrived from a reply that
//! was lost. Both look like a timeout, and both must be retried, so the client
//! alone cannot avoid duplicates. Each queued message therefore carries a
//! `client_msg_id`, and the server returns the first attempt's envelope when it
//! sees that id again.
//!
//! # Why order is preserved
//!
//! Flushing is strictly oldest-first, and it stops at the first failure rather
//! than skipping ahead. Two reasons. A commit sent out of order is refused as
//! stale by the server and would leave the group's epoch inconsistent with what
//! this device believes. And plain messages sent out of order arrive out of
//! order, which is a visible bug in a chat app.

use nexo_store::OutboxItem;
use uuid::Uuid;

use crate::conversations::{Context, ConversationError};
use crate::transport::{Transport, TransportError};

/// What a flush did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlushOutcome {
    /// Messages the server accepted.
    pub sent: usize,
    /// Messages the server had already accepted, matched by `client_msg_id`.
    ///
    /// Counted separately from `sent` because they mean something different:
    /// these are the duplicates that idempotency prevented, and a UI that
    /// reported them as newly sent would be reporting a lie.
    pub already_sent: usize,
    /// Still queued. Not an error on its own — being offline is normal.
    pub still_queued: usize,
    /// Messages the server refused for a reason retrying will not fix.
    pub failed: usize,
}

/// Generates an id for a new outgoing message.
pub fn new_message_id() -> String {
    Uuid::new_v4().to_string()
}

/// Sends everything queued, oldest first.
///
/// Stops at the first message that cannot be sent, and reports the rest as
/// still queued. Continuing past a failure would reorder the conversation, and
/// for a commit it would also mean sending an epoch the group has moved past.
///
/// Being unreachable is not an error here. It is the ordinary state this whole
/// module exists for, and it leaves the queue exactly as it was.
pub fn flush<T: Transport>(ctx: &Context<'_, T>) -> Result<FlushOutcome, ConversationError> {
    let queued = ctx.store.outbox()?;
    let mut outcome = FlushOutcome::default();

    for (index, item) in queued.iter().enumerate() {
        match send_one(ctx, item) {
            Ok(Sent::Accepted) => outcome.sent += 1,
            Ok(Sent::AlreadyThere) => outcome.already_sent += 1,
            Err(Stop::Offline(error)) => {
                // The queue is untouched. Record why, so the UI can say
                // something more useful than "pending".
                ctx.store.record_attempt(&item.client_msg_id, &error)?;
                outcome.still_queued = queued.len() - index;
                return Ok(outcome);
            }
            Err(Stop::Refused(error)) => {
                // A refusal retrying will not fix -- a conversation that no
                // longer exists, an expired session. Left in the queue with
                // the reason attached rather than deleted: a message someone
                // believes they sent must not vanish silently (rule 7).
                ctx.store.record_attempt(&item.client_msg_id, &error)?;
                outcome.failed += 1;
                outcome.still_queued = queued.len() - index;
                return Ok(outcome);
            }
        }

        // Only after the server has it. A crash between the send and this
        // delete leaves the message queued, and the next flush's retry is
        // answered by the idempotency check -- which is the safe direction.
        ctx.store.dequeue(&item.client_msg_id)?;
    }

    Ok(outcome)
}

/// Sends one queued message immediately.
///
/// The path `send_message` takes when the network is up. Split out so the
/// first attempt and every retry go through the same call with the same
/// `client_msg_id` -- if they diverged, the first attempt would be the one
/// case the idempotency guarantee did not cover.
pub fn send_now<T: Transport>(
    ctx: &Context<'_, T>,
    item: &OutboxItem,
) -> Result<crate::transport::Accepted, ConversationError> {
    Ok(ctx.transport.send(
        &item.conversation_id,
        &item.ciphertext,
        item.epoch,
        item.is_commit,
        &item.client_msg_id,
    )?)
}

/// What happened to one message.
enum Sent {
    /// Newly written by the server.
    Accepted,
    /// The server already had it, from an earlier attempt whose reply was lost.
    AlreadyThere,
}

/// Why a flush stopped.
enum Stop {
    /// The network. Ordinary; try again later.
    Offline(String),
    /// The server said no for a reason a retry will not change.
    Refused(String),
}

fn send_one<T: Transport>(ctx: &Context<'_, T>, item: &OutboxItem) -> Result<Sent, Stop> {
    // `envelope_id` before and after cannot distinguish the two cases -- an
    // idempotent hit returns the same id the first attempt got, which this
    // device never saw. So "already there" is inferred from the attempt count:
    // a message on its first attempt that succeeds was accepted now; one that
    // has failed before and now succeeds may have been accepted then.
    //
    // That is an honest approximation and it is only used for reporting. The
    // guarantee that matters -- exactly one copy on the server -- comes from
    // the unique index, not from this.
    let retry = item.attempts > 0;

    match ctx.transport.send(
        &item.conversation_id,
        &item.ciphertext,
        item.epoch,
        item.is_commit,
        &item.client_msg_id,
    ) {
        Ok(_) if retry => Ok(Sent::AlreadyThere),
        Ok(_) => Ok(Sent::Accepted),
        Err(TransportError::Unreachable(detail)) => Err(Stop::Offline(detail)),
        Err(error) => Err(Stop::Refused(error.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_id_is_a_uuid_and_never_repeats() {
        let a = new_message_id();
        let b = new_message_id();
        assert_ne!(a, b);
        assert!(a.parse::<Uuid>().is_ok());
    }

    #[test]
    fn an_empty_flush_reports_nothing() {
        let outcome = FlushOutcome::default();
        assert_eq!(outcome.sent, 0);
        assert_eq!(outcome.still_queued, 0);
        assert_eq!(outcome.failed, 0);
    }
}
