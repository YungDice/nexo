//! Attachment encryption.
//!
//! Brief 5.3: the client generates a fresh AES-256-GCM key and nonce, encrypts
//! the file, uploads the **ciphertext**, and puts the key inside the
//! MLS-encrypted message. The server sees an opaque blob and never learns the
//! key, the filename, or the type.
//!
//! That is what makes an attachment end-to-end encrypted rather than merely
//! encrypted-at-rest. Encryption at rest protects a disposed disk; this
//! protects against the server itself, which is named in
//! `docs/THREAT-MODEL.md` as an adversary in scope.
//!
//! # One key per file, never reused
//!
//! GCM fails catastrophically on nonce reuse — two files under the same
//! key/nonce pair leak the XOR of their plaintexts and let an attacker forge
//! tags. A fresh 256-bit key *and* a fresh nonce per file makes reuse
//! impossible rather than unlikely, and there is no key schedule to get wrong
//! because no key is ever used twice.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// AES-256 key length.
pub const KEY_LEN: usize = 32;
/// GCM nonce length.
pub const NONCE_LEN: usize = 12;

/// Errors from attachment handling.
#[derive(Debug, thiserror::Error)]
pub enum AttachmentError {
    /// The key or nonce was the wrong size.
    #[error("expected {expected} bytes, found {found}")]
    WrongLength {
        /// How many were expected.
        expected: usize,
        /// How many were supplied.
        found: usize,
    },
    /// Decryption failed.
    ///
    /// GCM authenticates, so this means the ciphertext was altered, truncated,
    /// or encrypted under a different key — not that it decoded to nonsense.
    /// Rule 7: this is shown as a failure, never as a partial file.
    #[error("the attachment could not be decrypted")]
    Undecryptable,
    /// The plaintext did not match the hash the sender published.
    ///
    /// Separate from [`AttachmentError::Undecryptable`] because it means
    /// something different: the ciphertext *was* authentic, so this is a
    /// mismatch between what was uploaded and what was described, which is a
    /// bug rather than an attack.
    #[error("the attachment does not match its published hash")]
    HashMismatch,
}

/// A freshly encrypted attachment, ready to upload.
pub struct Encrypted {
    /// What to upload. Opaque.
    pub ciphertext: Vec<u8>,
    /// The key, which goes inside the MLS message and nowhere else.
    pub key: Zeroizing<[u8; KEY_LEN]>,
    /// The nonce.
    pub nonce: [u8; NONCE_LEN],
    /// SHA-256 of the plaintext, so the recipient can verify what they got.
    pub sha256: [u8; 32],
    /// Plaintext size.
    pub size: u64,
}

impl std::fmt::Debug for Encrypted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The key is the whole secret. Never print it.
        f.debug_struct("Encrypted")
            .field("size", &self.size)
            .field("ciphertext_len", &self.ciphertext.len())
            .finish_non_exhaustive()
    }
}

/// Encrypts a file with a fresh key and nonce.
pub fn encrypt(plaintext: &[u8]) -> Result<Encrypted, AttachmentError> {
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    OsRng.fill_bytes(key.as_mut_slice());
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_slice()));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        // Encryption fails only on absurd input sizes; there is nothing a
        // caller can do differently, and the detail is not useful.
        .map_err(|_| AttachmentError::Undecryptable)?;

    let sha256: [u8; 32] = Sha256::digest(plaintext).into();

    Ok(Encrypted {
        ciphertext,
        key,
        nonce,
        sha256,
        size: plaintext.len() as u64,
    })
}

/// Decrypts a downloaded attachment and checks it against its published hash.
///
/// Both checks matter and they catch different things. GCM's tag catches a
/// ciphertext that was altered in the bucket or in transit. The hash catches a
/// sender whose upload and whose description disagree — which authentication
/// alone would happily accept, because both would be authentic.
pub fn decrypt(
    ciphertext: &[u8],
    key: &[u8],
    nonce: &[u8],
    expected_sha256: &[u8],
) -> Result<Zeroizing<Vec<u8>>, AttachmentError> {
    let key: [u8; KEY_LEN] = key.try_into().map_err(|_| AttachmentError::WrongLength {
        expected: KEY_LEN,
        found: key.len(),
    })?;
    let nonce: [u8; NONCE_LEN] = nonce.try_into().map_err(|_| AttachmentError::WrongLength {
        expected: NONCE_LEN,
        found: nonce.len(),
    })?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext)
        .map_err(|_| AttachmentError::Undecryptable)?;

    let actual: [u8; 32] = Sha256::digest(&plaintext).into();
    if actual.as_slice() != expected_sha256 {
        return Err(AttachmentError::HashMismatch);
    }

    Ok(Zeroizing::new(plaintext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_round_trips() {
        let plaintext = b"the contents of a file".to_vec();
        let sealed = encrypt(&plaintext).unwrap();
        let opened = decrypt(
            &sealed.ciphertext,
            sealed.key.as_slice(),
            &sealed.nonce,
            &sealed.sha256,
        )
        .unwrap();
        assert_eq!(&opened[..], &plaintext[..]);
    }

    #[test]
    fn the_ciphertext_does_not_contain_the_plaintext() {
        // The whole point: what lands in the bucket is opaque.
        let plaintext = b"the quick brown fox jumps over the lazy dog".to_vec();
        let sealed = encrypt(&plaintext).unwrap();
        assert!(
            !sealed
                .ciphertext
                .windows(plaintext.len())
                .any(|w| w == plaintext.as_slice()),
            "plaintext found in the ciphertext"
        );
    }

    #[test]
    fn every_file_gets_its_own_key_and_nonce() {
        // GCM fails catastrophically on nonce reuse, so this is not a nicety.
        let a = encrypt(b"same contents").unwrap();
        let b = encrypt(b"same contents").unwrap();
        assert_ne!(a.key.as_slice(), b.key.as_slice());
        assert_ne!(a.nonce, b.nonce);
        // And identical plaintexts therefore produce different ciphertexts.
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    #[test]
    fn a_tampered_ciphertext_is_refused() {
        // GCM authenticates; a flipped bit must not decrypt to a "mostly right"
        // file.
        let sealed = encrypt(b"important").unwrap();
        let mut tampered = sealed.ciphertext.clone();
        tampered[0] ^= 0x01;
        assert!(matches!(
            decrypt(
                &tampered,
                sealed.key.as_slice(),
                &sealed.nonce,
                &sealed.sha256
            ),
            Err(AttachmentError::Undecryptable)
        ));
    }

    #[test]
    fn a_truncated_ciphertext_is_refused() {
        let sealed = encrypt(b"important").unwrap();
        let truncated = &sealed.ciphertext[..sealed.ciphertext.len() - 1];
        assert!(
            decrypt(
                truncated,
                sealed.key.as_slice(),
                &sealed.nonce,
                &sealed.sha256
            )
            .is_err()
        );
    }

    #[test]
    fn the_wrong_key_is_refused() {
        let sealed = encrypt(b"important").unwrap();
        let wrong = [0u8; KEY_LEN];
        assert!(matches!(
            decrypt(&sealed.ciphertext, &wrong, &sealed.nonce, &sealed.sha256),
            Err(AttachmentError::Undecryptable)
        ));
    }

    #[test]
    fn a_hash_mismatch_is_distinct_from_a_decryption_failure() {
        // Authentication says the bytes are what the sender uploaded. The hash
        // says they are what the sender *described*. Those are different
        // claims, and conflating them would hide a real bug.
        let sealed = encrypt(b"important").unwrap();
        let wrong_hash = [0u8; 32];
        assert!(matches!(
            decrypt(
                &sealed.ciphertext,
                sealed.key.as_slice(),
                &sealed.nonce,
                &wrong_hash
            ),
            Err(AttachmentError::HashMismatch)
        ));
    }

    #[test]
    fn key_and_nonce_lengths_are_checked() {
        let sealed = encrypt(b"x").unwrap();
        assert!(matches!(
            decrypt(
                &sealed.ciphertext,
                &[0u8; 16],
                &sealed.nonce,
                &sealed.sha256
            ),
            Err(AttachmentError::WrongLength { expected: 32, .. })
        ));
        assert!(matches!(
            decrypt(
                &sealed.ciphertext,
                sealed.key.as_slice(),
                &[0u8; 8],
                &sealed.sha256
            ),
            Err(AttachmentError::WrongLength { expected: 12, .. })
        ));
    }

    #[test]
    fn an_empty_file_round_trips() {
        let sealed = encrypt(b"").unwrap();
        assert_eq!(sealed.size, 0);
        let opened = decrypt(
            &sealed.ciphertext,
            sealed.key.as_slice(),
            &sealed.nonce,
            &sealed.sha256,
        )
        .unwrap();
        assert!(opened.is_empty());
    }
}
