//! Nexo's MLS layer (RFC 9420), built on OpenMLS.
//!
//! Rule 1 of the brief: never invent cryptography. Every primitive used here
//! comes from OpenMLS and its RustCrypto provider. This crate exists to choose
//! the ciphersuite, hold the group state, and expose a small honest API — not
//! to add protocol of its own.
//!
//! This crate must compile unchanged for `aarch64-linux-android` (§12), so it
//! contains no platform calls and no I/O. Storage arrives as a provider.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod attachment;
pub mod identity;
pub mod mls;

use openmls::prelude::*;

/// The ciphersuites Nexo can be built with.
///
/// §3 locks v0.1 to the mandatory-to-implement suite. The extra variant is the
/// *seam* for the X-Wing hybrid post-quantum suite and is deliberately not
/// constructible yet — leaving the seam is in scope, building it is not (§11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SuiteChoice {
    /// `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519`.
    #[default]
    Mti,
}

impl SuiteChoice {
    /// The OpenMLS ciphersuite this choice maps to.
    pub const fn ciphersuite(self) -> Ciphersuite {
        match self {
            Self::Mti => Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519,
        }
    }
}

/// The ciphersuite v0.1 ships with.
pub const CIPHERSUITE: Ciphersuite = SuiteChoice::Mti.ciphersuite();

/// Out-of-order tolerance for *application* messages.
///
/// This does not, and cannot, apply to commits: MLS commits are strictly
/// epoch-ordered and a stale one must be rejected. See docs/PLAN.md risk 4(a).
pub const OUT_OF_ORDER_TOLERANCE: u32 = 10;

/// How far ahead in the ratchet a client will skip to find a message.
pub const MAX_FORWARD_DISTANCE: u32 = 2000;

/// KeyPackages published on registration (§4.2).
pub const KEY_PACKAGE_TARGET: usize = 50;

/// Refill KeyPackages when the server reports fewer than this many left (§4.2).
pub const KEY_PACKAGE_REFILL_THRESHOLD: usize = 15;

/// Rekey after this many messages sent, or [`REKEY_INTERVAL_DAYS`], whichever
/// comes first — and always on member add or remove (§4.2).
pub const REKEY_EVERY_N_MESSAGES: u64 = 100;

/// Rekey after this many days (§4.2).
pub const REKEY_INTERVAL_DAYS: u64 = 7;

/// Errors this crate can produce.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CryptoError {
    /// A message could not be decrypted. Rule 7 (fail closed): the caller must
    /// surface this to the user, never fall back to plaintext and never skip
    /// the message silently.
    #[error("message could not be decrypted")]
    Undecryptable,

    /// A commit arrived for an epoch that is no longer current.
    ///
    /// Split out from [`CryptoError::Undecryptable`] because it is the one
    /// failure with a *different* remedy: it means two commits raced, and the
    /// answer is to resync and rebuild rather than to show the user a broken
    /// message (PLAN.md risk 4(b)).
    #[error("stale epoch: this conversation is at {current}")]
    StaleEpoch {
        /// The epoch this client considers current.
        current: u64,
    },

    /// A message arrived that is well-formed but not the kind expected here.
    #[error("unexpected message type")]
    UnexpectedMessage,

    /// OpenMLS refused, for a reason the caller cannot act on differently.
    ///
    /// The detail is for a log, never for a user: it can name internal state.
    #[error("MLS operation failed: {0}")]
    Mls(String),
}

/// The group configuration every Nexo conversation is created with.
pub fn group_create_config() -> MlsGroupCreateConfig {
    MlsGroupCreateConfig::builder()
        .ciphersuite(CIPHERSUITE)
        // Pad to a fixed block so ciphertext length leaks less about the
        // plaintext. §4.4 is honest that message *size* is still server-visible.
        .padding_size(100)
        .sender_ratchet_configuration(SenderRatchetConfiguration::new(
            OUT_OF_ORDER_TOLERANCE,
            MAX_FORWARD_DISTANCE,
        ))
        .use_ratchet_tree_extension(true)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ships_the_mandatory_to_implement_suite() {
        assert_eq!(
            CIPHERSUITE,
            Ciphersuite::MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519
        );
    }

    #[test]
    fn group_config_uses_the_pinned_suite() {
        assert_eq!(group_create_config().ciphersuite(), CIPHERSUITE);
    }
}
