//! The commit-ordering rule, on its own.
//!
//! Extracted from the handler so the rule can be read and tested without a
//! database. The handler holds the row lock and does the I/O; this decides.
//!
//! PLAN.md risk 4(b), stated once and enforced in one place.

/// What the server does with an incoming envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Store it. The epoch is unchanged.
    Accept {
        /// The epoch still in force.
        epoch: i64,
    },
    /// Store it and advance the epoch. This commit won.
    AcceptAndAdvance {
        /// The epoch now in force.
        epoch: i64,
    },
    /// Refuse it. The sender resyncs and rebuilds.
    Stale {
        /// What the server considers current.
        current: i64,
        /// What the commit cited.
        cited: i64,
    },
}

/// Decides what to do with an envelope.
///
/// - **Application messages are accepted at any cited epoch.** MLS tolerates
///   bounded reordering through the secret tree, so a server that refused them
///   would be breaking a guarantee the protocol makes, in the name of a
///   tidiness nobody asked for. It is also not the server's business: it cannot
///   read them.
/// - **A commit must cite the current epoch.** First writer wins.
pub fn decide(current_epoch: i64, cited_epoch: i64, is_commit: bool) -> Decision {
    if !is_commit {
        return Decision::Accept {
            epoch: current_epoch,
        };
    }
    if cited_epoch != current_epoch {
        return Decision::Stale {
            current: current_epoch,
            cited: cited_epoch,
        };
    }
    Decision::AcceptAndAdvance {
        epoch: current_epoch + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_commit_citing_the_current_epoch_wins_and_advances() {
        assert_eq!(decide(3, 3, true), Decision::AcceptAndAdvance { epoch: 4 });
    }

    #[test]
    fn the_second_commit_for_the_same_epoch_is_stale() {
        // The first advanced 3 -> 4. The second still cites 3.
        assert_eq!(
            decide(4, 3, true),
            Decision::Stale {
                current: 4,
                cited: 3
            }
        );
    }

    #[test]
    fn a_commit_from_the_future_is_also_refused() {
        // Not a race — a client that has diverged. Same remedy: resync.
        assert_eq!(
            decide(3, 9, true),
            Decision::Stale {
                current: 3,
                cited: 9
            }
        );
    }

    #[test]
    fn application_messages_are_accepted_at_any_epoch() {
        // Bounded reordering is a guarantee MLS makes; the server must not
        // second-guess it, and cannot read the messages anyway.
        for cited in [0, 3, 4, 99] {
            assert_eq!(decide(4, cited, false), Decision::Accept { epoch: 4 });
        }
    }

    #[test]
    fn an_application_message_never_advances_the_epoch() {
        // Only commits move an epoch. If this were wrong, a chatty client would
        // desynchronise a conversation just by talking.
        match decide(7, 7, false) {
            Decision::Accept { epoch } => assert_eq!(epoch, 7),
            other => panic!("expected Accept, got {other:?}"),
        }
    }

    #[test]
    fn the_first_commit_in_a_new_conversation_cites_zero() {
        assert_eq!(decide(0, 0, true), Decision::AcceptAndAdvance { epoch: 1 });
    }
}
