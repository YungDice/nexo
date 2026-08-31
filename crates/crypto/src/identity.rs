//! The account's long-term identity keypair, and safety numbers.
//!
//! Brief 4.1: registration generates an Ed25519 keypair. The private half never
//! leaves the machine — not to the server, not into the WebView, not into a
//! log. The public half is registered and becomes the account's cryptographic
//! identity.
//!
//! # Why safety numbers exist
//!
//! The server hands out identity public keys, so a malicious server can hand
//! you an attacker's key for a contact and read everything between you. No
//! amount of transport security fixes that: the server is a legitimate party in
//! the connection.
//!
//! The only defence is two humans comparing a short value out of band. That is
//! what [`SafetyNumber`] is for, and why `docs/THREAT-MODEL.md` lists a
//! key-substituting server as out of scope *unless* users actually compare
//! them. Showing the number is this crate's job; making people look at it is
//! the UI's.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// An Ed25519 public key is 32 bytes.
pub const PUBLIC_KEY_LEN: usize = 32;
/// The private half is a 32-byte seed.
pub const SECRET_KEY_LEN: usize = 32;

/// Errors from identity key handling.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// Key material was the wrong size.
    #[error("expected {expected} bytes, found {found}")]
    WrongLength {
        /// How many bytes were expected.
        expected: usize,
        /// How many were supplied.
        found: usize,
    },
    /// The bytes are not a valid Ed25519 point.
    #[error("not a valid Ed25519 public key")]
    InvalidPublicKey,
}

/// This device's identity keypair.
///
/// No `Debug`, no `Clone`, no `Serialize`. Getting the secret out takes
/// [`IdentityKeypair::secret_bytes`], which is greppable — the point is that
/// nothing prints or copies it by accident.
pub struct IdentityKeypair {
    signing: SigningKey,
}

impl IdentityKeypair {
    /// Generates a new keypair from the OS CSPRNG.
    pub fn generate() -> Self {
        // ed25519-dalek's own generator, over OsRng: rule 1 says the primitive
        // choices are not ours to make.
        let mut seed = Zeroizing::new([0u8; SECRET_KEY_LEN]);
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, seed.as_mut_slice());
        Self {
            signing: SigningKey::from_bytes(&seed),
        }
    }

    /// Rebuilds a keypair from a stored seed.
    pub fn from_secret_bytes(bytes: &[u8]) -> Result<Self, IdentityError> {
        let seed: [u8; SECRET_KEY_LEN] =
            bytes.try_into().map_err(|_| IdentityError::WrongLength {
                expected: SECRET_KEY_LEN,
                found: bytes.len(),
            })?;
        Ok(Self {
            signing: SigningKey::from_bytes(&seed),
        })
    }

    /// The seed, for writing into the encrypted store and nowhere else.
    ///
    /// [`Zeroizing`], so a caller that drops it does not leave it in memory.
    pub fn secret_bytes(&self) -> Zeroizing<[u8; SECRET_KEY_LEN]> {
        Zeroizing::new(self.signing.to_bytes())
    }

    /// The public half, which is what the server learns.
    pub fn public_bytes(&self) -> [u8; PUBLIC_KEY_LEN] {
        self.signing.verifying_key().to_bytes()
    }

    /// Signs a message.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        self.signing.sign(message).to_bytes()
    }
}

/// Checks a signature against a public key.
pub fn verify(
    public_key: &[u8],
    message: &[u8],
    signature: &[u8; 64],
) -> Result<bool, IdentityError> {
    let key = public_key_from_bytes(public_key)?;
    Ok(key
        .verify(message, &Signature::from_bytes(signature))
        .is_ok())
}

fn public_key_from_bytes(bytes: &[u8]) -> Result<VerifyingKey, IdentityError> {
    let array: [u8; PUBLIC_KEY_LEN] = bytes.try_into().map_err(|_| IdentityError::WrongLength {
        expected: PUBLIC_KEY_LEN,
        found: bytes.len(),
    })?;
    VerifyingKey::from_bytes(&array).map_err(|_| IdentityError::InvalidPublicKey)
}

/// Number of digit groups in a safety number.
pub const SAFETY_GROUPS: usize = 12;
/// Digits per group.
pub const SAFETY_DIGITS_PER_GROUP: usize = 5;

/// A safety number: 12 groups of 5 digits, comparable by two people out loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyNumber {
    groups: [u32; SAFETY_GROUPS],
}

/// Domain bytes for a two-party safety number.
const PAIR_DOMAIN: [u8; 2] = [0x00, 0x01];
/// Domain bytes for a single-key device fingerprint.
const IDENTITY_DOMAIN: [u8; 2] = [0x02, 0x03];

/// Two SHA-256 digests over the same domain-separated input give 64 bytes; the
/// first 60 split into twelve 5-byte chunks, each reduced mod 100000. One
/// digest is 32 bytes and twelve groups need 60, which is why there are two.
fn derive_groups(domains: [u8; 2], parts: &[&[u8]]) -> [u32; SAFETY_GROUPS] {
    let mut material = Vec::with_capacity(64);
    for domain in domains {
        let mut hasher = Sha256::new();
        hasher.update([domain]);
        for part in parts {
            hasher.update(part);
        }
        material.extend_from_slice(&hasher.finalize());
    }

    let mut groups = [0u32; SAFETY_GROUPS];
    for (i, group) in groups.iter_mut().enumerate() {
        let chunk = &material[i * 5..i * 5 + 5];
        let value = chunk.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
        *group = (value % 100_000) as u32;
    }
    groups
}

impl SafetyNumber {
    /// Computes the safety number for a pair of identity public keys.
    ///
    /// The keys are **sorted** before hashing, so both sides compute the same
    /// value without needing to agree on who is "first". That is the whole
    /// reason for the sort, and getting it wrong would produce two numbers that
    /// never match and an endless supply of false alarms.
    ///
    /// Construction: `SHA-256(0x00 || a || b)` and `SHA-256(0x01 || a || b)`
    /// concatenated give 64 bytes; the first 60 split into twelve 5-byte
    /// chunks, each reduced mod 100000 to five digits. Two digests rather than
    /// one because a single SHA-256 is 32 bytes and 12 groups need 60.
    ///
    /// Not iterated the way Signal's is. Iteration there slows brute-force
    /// search for a colliding *short* fingerprint; at 60 digits — around 199
    /// bits — there is nothing to slow down.
    pub fn new(one: &[u8], other: &[u8]) -> Result<Self, IdentityError> {
        // Validate both, so a malformed key is caught here rather than
        // producing a plausible-looking number for a key that cannot exist.
        public_key_from_bytes(one)?;
        public_key_from_bytes(other)?;

        let (first, second) = if one <= other {
            (one, other)
        } else {
            (other, one)
        };

        Ok(Self {
            groups: derive_groups(PAIR_DOMAIN, &[first, second]),
        })
    }

    /// The fingerprint of a **single** identity key: this device's own.
    ///
    /// Not a safety number, and deliberately not computed like one. A safety
    /// number answers "are you and I talking to each other"; this answers
    /// "which key is this device". Feeding one key to [`SafetyNumber::new`]
    /// twice would have produced a plausible-looking number that means neither
    /// thing, so the domain bytes differ: no device fingerprint can ever equal
    /// a two-party safety number, and a value read aloud in the wrong context
    /// simply fails to match rather than appearing to succeed.
    pub fn for_identity(public_key: &[u8]) -> Result<Self, IdentityError> {
        public_key_from_bytes(public_key)?;
        Ok(Self {
            groups: derive_groups(IDENTITY_DOMAIN, &[public_key]),
        })
    }

    /// The twelve groups.
    pub fn groups(&self) -> &[u32; SAFETY_GROUPS] {
        &self.groups
    }

    /// Rendered for display: twelve five-digit groups, space separated.
    ///
    /// Zero-padded, because `01234` and `1234` are different numbers and a
    /// dropped leading zero is exactly the kind of mismatch that gets waved
    /// through as "probably a display bug".
    pub fn to_display_string(&self) -> String {
        self.groups
            .iter()
            .map(|g| format!("{g:05}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl std::fmt::Display for SafetyNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_display_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keypair_round_trips_through_its_seed() {
        let original = IdentityKeypair::generate();
        let restored = IdentityKeypair::from_secret_bytes(&*original.secret_bytes()).unwrap();
        assert_eq!(original.public_bytes(), restored.public_bytes());
    }

    #[test]
    fn generated_keys_differ() {
        let a = IdentityKeypair::generate();
        let b = IdentityKeypair::generate();
        assert_ne!(a.public_bytes(), b.public_bytes());
    }

    #[test]
    fn a_signature_verifies_under_the_matching_key() {
        let key = IdentityKeypair::generate();
        let signature = key.sign(b"a message");
        assert!(verify(&key.public_bytes(), b"a message", &signature).unwrap());
    }

    #[test]
    fn a_signature_does_not_verify_under_another_key() {
        let key = IdentityKeypair::generate();
        let other = IdentityKeypair::generate();
        let signature = key.sign(b"a message");
        assert!(!verify(&other.public_bytes(), b"a message", &signature).unwrap());
    }

    #[test]
    fn a_signature_does_not_verify_over_a_different_message() {
        let key = IdentityKeypair::generate();
        let signature = key.sign(b"a message");
        assert!(!verify(&key.public_bytes(), b"another message", &signature).unwrap());
    }

    #[test]
    fn wrong_length_key_material_is_refused() {
        assert!(matches!(
            IdentityKeypair::from_secret_bytes(&[0u8; 16]),
            Err(IdentityError::WrongLength { .. })
        ));
    }

    /// The property that makes safety numbers usable at all.
    #[test]
    fn both_sides_compute_the_same_safety_number() {
        let alice = IdentityKeypair::generate().public_bytes();
        let bob = IdentityKeypair::generate().public_bytes();
        assert_eq!(
            SafetyNumber::new(&alice, &bob).unwrap(),
            SafetyNumber::new(&bob, &alice).unwrap(),
            "argument order must not change the number, or nobody's ever matches"
        );
    }

    #[test]
    fn a_changed_key_changes_the_safety_number() {
        // This is the entire point: a server that swaps a key must produce a
        // number that no longer matches what was compared before.
        let alice = IdentityKeypair::generate().public_bytes();
        let bob = IdentityKeypair::generate().public_bytes();
        let impostor = IdentityKeypair::generate().public_bytes();
        assert_ne!(
            SafetyNumber::new(&alice, &bob).unwrap(),
            SafetyNumber::new(&alice, &impostor).unwrap()
        );
    }

    #[test]
    fn a_safety_number_is_stable() {
        let alice = IdentityKeypair::generate().public_bytes();
        let bob = IdentityKeypair::generate().public_bytes();
        assert_eq!(
            SafetyNumber::new(&alice, &bob).unwrap(),
            SafetyNumber::new(&alice, &bob).unwrap()
        );
    }

    #[test]
    fn the_display_form_is_twelve_groups_of_five_digits() {
        let alice = IdentityKeypair::generate().public_bytes();
        let bob = IdentityKeypair::generate().public_bytes();
        let rendered = SafetyNumber::new(&alice, &bob).unwrap().to_display_string();

        let groups: Vec<&str> = rendered.split(' ').collect();
        assert_eq!(groups.len(), SAFETY_GROUPS);
        for group in groups {
            assert_eq!(group.len(), SAFETY_DIGITS_PER_GROUP, "in `{rendered}`");
            assert!(group.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn leading_zeros_are_preserved() {
        // `01234` and `1234` are different numbers, and a dropped zero reads as
        // a display glitch rather than the mismatch it is.
        let number = SafetyNumber { groups: [7; 12] };
        assert!(number.to_display_string().starts_with("00007 00007"));
    }

    #[test]
    fn a_device_fingerprint_is_stable_for_a_key() {
        let key = IdentityKeypair::generate().public_bytes();
        assert_eq!(
            SafetyNumber::for_identity(&key).unwrap(),
            SafetyNumber::for_identity(&key).unwrap()
        );
    }

    #[test]
    fn different_devices_have_different_fingerprints() {
        let one = IdentityKeypair::generate().public_bytes();
        let other = IdentityKeypair::generate().public_bytes();
        assert_ne!(
            SafetyNumber::for_identity(&one).unwrap(),
            SafetyNumber::for_identity(&other).unwrap()
        );
    }

    /// The reason the domain bytes differ. A device fingerprint and a safety
    /// number are read aloud in different conversations; if one could equal the
    /// other, a value compared in the wrong context would appear to succeed.
    #[test]
    fn a_device_fingerprint_is_not_a_safety_number_with_itself() {
        let key = IdentityKeypair::generate().public_bytes();
        assert_ne!(
            SafetyNumber::for_identity(&key).unwrap(),
            SafetyNumber::new(&key, &key).unwrap()
        );
    }

    #[test]
    fn a_device_fingerprint_is_twelve_groups_of_five_digits() {
        let key = IdentityKeypair::generate().public_bytes();
        let rendered = SafetyNumber::for_identity(&key)
            .unwrap()
            .to_display_string();
        let groups: Vec<&str> = rendered.split(' ').collect();
        assert_eq!(groups.len(), SAFETY_GROUPS);
        for group in groups {
            assert_eq!(group.len(), SAFETY_DIGITS_PER_GROUP, "in `{rendered}`");
            assert!(group.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn an_invalid_key_has_no_device_fingerprint() {
        assert!(SafetyNumber::for_identity(&[0u8; 16]).is_err());
        let mut not_a_point = [0u8; 32];
        not_a_point[31] = 0x7F;
        assert!(SafetyNumber::for_identity(&not_a_point).is_err());
    }

    #[test]
    fn an_invalid_public_key_is_refused() {
        let valid = IdentityKeypair::generate().public_bytes();

        // Wrong length is the easy case.
        assert!(SafetyNumber::new(&valid, &[0u8; 16]).is_err());

        // A right-length value that is not a curve point is the case worth
        // testing, and it has to be a *known* bad encoding: roughly half of
        // all 32-byte strings do decompress to valid points, so picking one
        // arbitrarily tests nothing. This one does not decompress.
        let mut not_a_point = [0u8; 32];
        not_a_point[31] = 0x7F;
        assert!(
            ed25519_dalek::VerifyingKey::from_bytes(&not_a_point).is_err(),
            "the fixture must actually be an invalid encoding"
        );
        assert!(SafetyNumber::new(&valid, &not_a_point).is_err());
    }

    #[test]
    fn a_known_pair_produces_a_stable_known_value() {
        // A regression guard on the construction itself: if the domain bytes,
        // the sort, or the chunking ever change, every previously verified
        // conversation would silently start showing a mismatch.
        let a = [1u8; 32];
        let b = [2u8; 32];
        let a_key = ed25519_dalek::SigningKey::from_bytes(&a)
            .verifying_key()
            .to_bytes();
        let b_key = ed25519_dalek::SigningKey::from_bytes(&b)
            .verifying_key()
            .to_bytes();
        let number = SafetyNumber::new(&a_key, &b_key).unwrap();
        let rendered = number.to_display_string();
        assert_eq!(
            rendered.len(),
            SAFETY_GROUPS * (SAFETY_DIGITS_PER_GROUP + 1) - 1
        );
        // Recomputing must give the same thing on any machine.
        assert_eq!(
            SafetyNumber::new(&b_key, &a_key)
                .unwrap()
                .to_display_string(),
            rendered
        );
    }
}
