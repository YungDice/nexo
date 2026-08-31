//! The per-account salt, and why an unknown handle still gets one.
//!
//! `POST /v1/auth/salt` has to answer before the client can derive a verifier,
//! which means it answers *before* anyone has proved who they are. The obvious
//! implementation — look the handle up, 404 if it is missing — turns the
//! endpoint into a user-enumeration oracle: anyone can walk a wordlist and
//! learn exactly which handles exist.
//!
//! So an unknown handle gets a salt too. It is derived deterministically from
//! the server's key material and the handle, which gives it the two properties
//! that matter:
//!
//! - **Stable.** Asking twice returns the same bytes, so a decoy cannot be
//!   spotted by asking again.
//! - **Unpredictable without the seed.** An attacker cannot compute it offline
//!   and compare, which is what would give the game away.
//!
//! The login attempt that follows fails either way. The point is that it fails
//! the *same* way, so failure carries no information about whether the account
//! exists.

use hmac_sha256::derive;

use super::password::CLIENT_SALT_LEN;

/// A stable decoy salt for a handle that has no account.
pub fn decoy_salt(seed: &[u8; 32], handle: &str) -> [u8; CLIENT_SALT_LEN] {
    // Lower-cased so that `Alice` and `alice` cannot be distinguished by their
    // decoys — `users.handle` is CITEXT, so they would be the same account.
    let normalized = handle.to_lowercase();
    let full = derive(seed, b"nexo-decoy-salt-v1", normalized.as_bytes());
    let mut out = [0u8; CLIENT_SALT_LEN];
    out.copy_from_slice(&full[..CLIENT_SALT_LEN]);
    out
}

/// A tiny domain-separated KDF over SHA-256.
///
/// Not a general-purpose HMAC: it exists so the decoy salt depends on the
/// server seed without adding a dependency, and it is used for nothing that
/// needs message authentication.
mod hmac_sha256 {
    use sha2::{Digest, Sha256};

    pub fn derive(seed: &[u8; 32], domain: &[u8], input: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(seed);
        hasher.update(domain);
        // Length-prefixed so that ("ab", "c") and ("a", "bc") cannot collide.
        hasher.update((input.len() as u64).to_be_bytes());
        hasher.update(input);
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: [u8; 32] = [7u8; 32];

    #[test]
    fn a_decoy_is_stable_for_the_same_handle() {
        // If it were not, asking twice would reveal which handles are decoys.
        assert_eq!(decoy_salt(&SEED, "ghost"), decoy_salt(&SEED, "ghost"));
    }

    #[test]
    fn different_handles_get_different_decoys() {
        assert_ne!(decoy_salt(&SEED, "ghost"), decoy_salt(&SEED, "phantom"));
    }

    #[test]
    fn case_does_not_produce_a_second_decoy() {
        // users.handle is CITEXT: these are the same account, so they must not
        // look like two.
        assert_eq!(decoy_salt(&SEED, "Ghost"), decoy_salt(&SEED, "ghost"));
        assert_eq!(decoy_salt(&SEED, "GHOST"), decoy_salt(&SEED, "ghost"));
    }

    #[test]
    fn a_different_server_seed_gives_a_different_decoy() {
        // Otherwise the decoy is computable offline and enumeration is back.
        assert_ne!(decoy_salt(&SEED, "ghost"), decoy_salt(&[9u8; 32], "ghost"));
    }

    #[test]
    fn a_decoy_is_the_same_length_as_a_real_salt() {
        // A length difference would be as good a tell as a 404.
        assert_eq!(decoy_salt(&SEED, "ghost").len(), CLIENT_SALT_LEN);
    }

    #[test]
    fn the_length_prefix_prevents_collisions() {
        // Without it, concatenation would make these two identical.
        assert_ne!(decoy_salt(&SEED, "ab"), decoy_salt(&SEED, "a\u{0}b"));
    }
}
