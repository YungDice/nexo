//! Password verifiers.
//!
//! The scheme is BRIEF 4.1, and it is worth being precise about what the server
//! does and does not see:
//!
//! 1. The client asks for the account's salt.
//! 2. The client computes `verifier = Argon2id(password, salt)` locally.
//! 3. The client sends the **verifier** over TLS. The password never leaves the
//!    machine.
//! 4. The server stores `Argon2id(verifier, server_salt)` -- a second,
//!    independent hash, so a database dump does not yield anything replayable.
//!
//! This is not a PAKE. A server compromised *at the moment someone logs in*
//! sees the verifier, which is enough to impersonate that user until the
//! password changes. That is better than receiving a password and worse than
//! OPAQUE, and `docs/THREAT-MODEL.md` 5 says so out loud rather than implying
//! otherwise.

use anyhow::anyhow;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::rngs::OsRng;

/// Length of the per-account salt handed to the client.
///
/// BRIEF 4.1 says 16 bytes. A salt needs uniqueness rather than secrecy, and
/// 128 bits of it will not collide.
pub const CLIENT_SALT_LEN: usize = 16;

/// Argon2id parameters for the server-side hash of the verifier.
///
/// Deliberately lighter than the client's m=64 MiB: the client hashes once, on
/// a machine doing nothing else, and that cost is what protects the password.
/// The server hashes on **every login attempt**, so the same parameters would
/// hand anyone a memory-exhaustion lever. The value being hashed here is
/// already a 32-byte Argon2 output rather than a human password, so it is not
/// guessable by dictionary in the first place.
fn hasher() -> anyhow::Result<Argon2<'static>> {
    let params = Params::new(
        19 * 1024, // 19 MiB
        2,         // iterations
        1,         // parallelism
        None,      // default output length
    )
    .map_err(|e| anyhow!("argon2 parameters rejected: {e}"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Hashes a client-supplied verifier for storage.
///
/// Returns a PHC string, which carries the algorithm, parameters and salt
/// alongside the digest -- so raising the parameters later does not invalidate
/// existing rows.
pub fn hash_verifier(verifier: &[u8]) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(hasher()?
        .hash_password(verifier, &salt)
        .map_err(|e| anyhow!("hashing the verifier failed: {e}"))?
        .to_string())
}

/// Checks a verifier against a stored PHC string.
///
/// `Ok(false)` is a wrong verifier. `Err` means the stored hash could not be
/// parsed, which is corruption rather than a failed login and must not be
/// reported to the caller as "wrong password".
pub fn verify(verifier: &[u8], stored_phc: &str) -> anyhow::Result<bool> {
    let parsed = PasswordHash::new(stored_phc)
        .map_err(|e| anyhow!("stored password hash is unparseable: {e}"))?;
    match hasher()?.verify_password(verifier, &parsed) {
        Ok(()) => Ok(true),
        Err(argon2::password_hash::Error::Password) => Ok(false),
        Err(e) => Err(anyhow!("verifying the password failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_correct_verifier_is_accepted() {
        let stored = hash_verifier(b"a-client-derived-verifier").unwrap();
        assert!(verify(b"a-client-derived-verifier", &stored).unwrap());
    }

    #[test]
    fn a_wrong_verifier_is_rejected_without_erroring() {
        let stored = hash_verifier(b"the-real-one").unwrap();
        // Distinguishing "wrong" from "broken" matters: only the first is a
        // failed login, and only the second should page anyone.
        assert!(!verify(b"not-the-real-one", &stored).unwrap());
    }

    #[test]
    fn a_corrupt_stored_hash_is_an_error_not_a_rejection() {
        assert!(verify(b"anything", "not-a-phc-string").is_err());
    }

    #[test]
    fn the_same_verifier_hashes_differently_every_time() {
        // Distinct salts, so identical passwords do not produce identical rows
        // and a dump cannot be scanned for users who share one.
        let a = hash_verifier(b"same").unwrap();
        let b = hash_verifier(b"same").unwrap();
        assert_ne!(a, b);
        assert!(verify(b"same", &a).unwrap());
        assert!(verify(b"same", &b).unwrap());
    }

    #[test]
    fn the_documented_salt_length_is_the_brief_s() {
        // The client generates the salt now (see RegisterRequest::pw_salt);
        // the server only checks it is this long.
        assert_eq!(CLIENT_SALT_LEN, 16);
    }
}
