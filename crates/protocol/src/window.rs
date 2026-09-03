//! How long a message can be taken back or changed.
//!
//! Extracted from anything that does I/O so the rule can be read and tested on
//! its own — the same reason `apps/server/src/delivery/epoch.rs` exists.
//!
//! # The asymmetry is the point
//!
//! The sender is held to ten minutes exactly. The receiver allows eleven.
//!
//! Without that minute, a sender whose clock runs slightly fast sends an edit
//! it believes is at 9:59. The server stamps 10:01. The sender applies it
//! locally, every receiver refuses it, and the group is now permanently in
//! disagreement about what the message says — with no way for anyone to notice,
//! because each side is behaving correctly by its own lights.
//!
//! The grace does not weaken anything. The window is a courtesy in the first
//! place: it cannot be enforced against a modified client, which sends whatever
//! it likes whenever it likes. What it *does* is keep honest clients agreeing
//! with each other, and agreement is what a minute of slack buys.
//!
//! # What the clocks are
//!
//! Both sides compare like with like, and this is checked in the code rather
//! than assumed. An incoming message carries the server's timestamp from its
//! envelope; our own carries the local clock at the moment we sent it. Nobody
//! else may edit our own messages, so a comparison never mixes the two.

/// How long the sender is allowed. Ten minutes, in milliseconds.
pub const EDIT_WINDOW_MS: i64 = 10 * 60 * 1000;

/// What the receiver allows on top, for clock skew between two machines.
pub const RECEIVER_GRACE_MS: i64 = 60 * 1000;

/// Whether this device may still take back or change its own message.
///
/// The courtesy check. A modified client ignores it, which is why the receiver
/// checks too — and why the UI removes the menu entry rather than greying it
/// out: an action that is gone was never offered, and one that is greyed out
/// invites the question of how to get it back.
pub fn sender_may_change(sent_at_ms: i64, now_ms: i64) -> bool {
    let age = now_ms.saturating_sub(sent_at_ms);
    (0..=EDIT_WINDOW_MS).contains(&age)
}

/// Whether an arriving edit or retraction is still in time.
///
/// Both timestamps are the server's: the one on the message being changed and
/// the one on the envelope carrying the change. That is what makes this
/// judgeable at all — neither is a clock the sender controls.
///
/// A change that arrives *before* the message it changes is refused rather than
/// treated as instant. It cannot happen honestly, and reading a negative age as
/// "very fresh" would turn a reordering into a permanent edit right.
pub fn receiver_may_apply(target_sent_at_ms: i64, change_sent_at_ms: i64) -> bool {
    let age = change_sent_at_ms.saturating_sub(target_sent_at_ms);
    (0..=EDIT_WINDOW_MS + RECEIVER_GRACE_MS).contains(&age)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_760_000_000_000;

    #[test]
    fn the_sender_has_exactly_ten_minutes() {
        assert!(sender_may_change(T0, T0), "immediately");
        assert!(
            sender_may_change(T0, T0 + EDIT_WINDOW_MS - 1),
            "just inside"
        );
        assert!(
            sender_may_change(T0, T0 + EDIT_WINDOW_MS),
            "on the boundary"
        );
        assert!(
            !sender_may_change(T0, T0 + EDIT_WINDOW_MS + 1),
            "one millisecond past is past"
        );
    }

    /// A clock that jumps backwards must not hand out a fresh window.
    #[test]
    fn a_message_from_the_future_is_not_editable() {
        assert!(!sender_may_change(T0 + 1, T0));
    }

    #[test]
    fn the_receiver_allows_one_extra_minute() {
        let limit = EDIT_WINDOW_MS + RECEIVER_GRACE_MS;
        assert!(
            receiver_may_apply(T0, T0 + EDIT_WINDOW_MS),
            "the honest case"
        );
        assert!(
            receiver_may_apply(T0, T0 + EDIT_WINDOW_MS + 30_000),
            "half a minute of skew is still applied"
        );
        assert!(receiver_may_apply(T0, T0 + limit), "on the boundary");
        assert!(
            !receiver_may_apply(T0, T0 + limit + 1),
            "past the grace is past"
        );
    }

    /// The case the grace exists for, stated as a test.
    ///
    /// A sender whose clock runs fast sends at what it thinks is 9:59; the
    /// server stamps 10:01. Without the grace the sender would apply the change
    /// and every receiver would refuse it, leaving the group permanently
    /// disagreeing about what the message says.
    #[test]
    fn a_slightly_fast_clock_does_not_split_the_group() {
        let stamped_late = T0 + EDIT_WINDOW_MS + 2_000;
        assert!(
            receiver_may_apply(T0, stamped_late),
            "the receiver must still take it, or the two sides diverge"
        );
    }

    #[test]
    fn a_change_that_predates_its_target_is_refused() {
        assert!(!receiver_may_apply(T0, T0 - 1));
    }
}
