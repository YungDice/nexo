//! Whether a story is still available.
//!
//! One line of logic, pulled out for the same reason `delivery/epoch.rs` is:
//! the rule can then be read and tested without a database, and there is
//! exactly one copy of it.
//!
//! This is the layer that turns "24 hours" from a courtesy into a property.
//! The reader deletes its own copy and its key, which is what actually makes a
//! story go; this makes sure that a client which does not delete gets nothing
//! more from the server anyway.

/// How long a story lasts.
pub const STORY_LIFETIME_MS: i64 = 24 * 60 * 60 * 1000;

/// Whether a story may still be handed out.
///
/// Exclusive at the boundary: at the instant it expires, it has expired. The
/// alternative reads better in a sentence and worse in a promise.
pub fn still_available(expires_at_ms: i64, now_ms: i64) -> bool {
    now_ms < expires_at_ms
}

/// When a story created now should expire.
///
/// Here rather than only in SQL so the client can be told the same number
/// without asking, and so the two cannot drift.
pub fn expires_at(created_at_ms: i64) -> i64 {
    created_at_ms.saturating_add(STORY_LIFETIME_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_760_000_000_000;

    #[test]
    fn a_fresh_story_is_available() {
        assert!(still_available(expires_at(T0), T0));
        assert!(still_available(expires_at(T0), T0 + STORY_LIFETIME_MS - 1));
    }

    #[test]
    fn a_story_is_gone_the_instant_it_expires() {
        assert!(
            !still_available(expires_at(T0), T0 + STORY_LIFETIME_MS),
            "at the boundary it is over, not nearly over"
        );
        assert!(!still_available(expires_at(T0), T0 + STORY_LIFETIME_MS + 1));
    }

    #[test]
    fn the_lifetime_is_twenty_four_hours() {
        assert_eq!(expires_at(T0) - T0, 24 * 60 * 60 * 1000);
    }

    /// A clock far in the future must not resurrect anything.
    #[test]
    fn nothing_is_available_long_after() {
        assert!(!still_available(
            expires_at(T0),
            T0 + 10 * STORY_LIFETIME_MS
        ));
    }
}
