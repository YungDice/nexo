//! An unlock PIN (N11).
//!
//! # What a PIN is worth here, and what it is not
//!
//! Four digits are ten thousand possibilities. On its own that is not a secret,
//! and nothing in this module pretends otherwise. What makes it worth having is
//! what it sits on top of: the verifier below is wrapped by `SecureStore`,
//! which on Windows is DPAPI bound to the user account. Someone who steals the
//! disk gets a blob they cannot open; someone at a signed-in Windows session
//! needs the PIN. The attacker has to hold **both**, and neither alone is
//! enough.
//!
//! It is still weaker than the password, deliberately. The password reaches the
//! server and re-derives everything; the PIN reaches only this machine. That is
//! a real reduction in protection against a stolen, logged-in device, and
//! `docs/THREAT-MODEL.md` is where that trade belongs — not in a comment
//! nobody reads while deciding whether to turn it on.
//!
//! # Why there is an attempt limit
//!
//! Ten thousand guesses is minutes of typing and nothing at all to a script.
//! After [`MAX_ATTEMPTS`] failures the PIN stops being accepted at all and the
//! full password is the only way back in. The counter is stored beside the
//! verifier, so clearing it means clearing the keystore entry — which is the
//! same act as forgetting the PIN.

use argon2::{Algorithm, Argon2, Params, Version};
use nexo_platform::SecureStore;
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

/// Where the verifier lives in the OS keystore.
const PIN_NAME: &str = "nexo-unlock-pin";
/// Where the failed-attempt count lives.
const ATTEMPTS_NAME: &str = "nexo-unlock-pin-attempts";

/// How many wrong PINs before only the password will do.
pub const MAX_ATTEMPTS: u8 = 5;

/// Shortest PIN accepted. Four is the convention; anything less is theatre.
pub const MIN_PIN_LEN: usize = 4;
/// Longest. Beyond this it is a password, and there is already one of those.
pub const MAX_PIN_LEN: usize = 12;

/// 16 bytes of salt, then 32 bytes of Argon2id output.
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;

/// What can go wrong with a PIN.
#[derive(Debug, thiserror::Error)]
pub enum PinError {
    /// The keystore refused.
    #[error("the OS keystore failed: {0}")]
    Keystore(String),
    /// The PIN was too short, too long, or not digits.
    #[error("{0}")]
    Invalid(String),
    /// Deriving the verifier failed.
    #[error("could not derive the PIN verifier: {0}")]
    Derivation(String),
    /// Too many wrong guesses; the password is the only way in now.
    #[error("too many attempts; sign in with your password")]
    Locked,
}

/// Whether a PIN is set, and how many tries are left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinStatus {
    /// True when one has been set on this device.
    pub set: bool,
    /// Remaining attempts before only the password is accepted.
    pub attempts_left: u8,
}

/// Argon2id, sized for something typed at a lock screen.
///
/// Lighter than the password's parameters on purpose: this runs while somebody
/// waits to get back into an app they are already signed into, and the work
/// factor is not what is protecting four digits — the attempt limit is.
fn hasher() -> Result<Argon2<'static>, PinError> {
    let params = Params::new(19 * 1024, 2, 1, Some(HASH_LEN))
        .map_err(|e| PinError::Derivation(e.to_string()))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

fn check_shape(pin: &str) -> Result<(), PinError> {
    if pin.len() < MIN_PIN_LEN || pin.len() > MAX_PIN_LEN {
        return Err(PinError::Invalid(format!(
            "A PIN is between {MIN_PIN_LEN} and {MAX_PIN_LEN} digits."
        )));
    }
    if !pin.bytes().all(|b| b.is_ascii_digit()) {
        return Err(PinError::Invalid("A PIN is digits only.".to_string()));
    }
    Ok(())
}

fn derive(pin: &str, salt: &[u8]) -> Result<Zeroizing<Vec<u8>>, PinError> {
    let mut out = Zeroizing::new(vec![0u8; HASH_LEN]);
    hasher()?
        .hash_password_into(pin.as_bytes(), salt, &mut out)
        .map_err(|e| PinError::Derivation(e.to_string()))?;
    Ok(out)
}

/// Sets or replaces the PIN, and clears any failed attempts.
pub fn set<S: SecureStore>(keystore: &S, pin: &str) -> Result<(), PinError> {
    check_shape(pin)?;

    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let hash = derive(pin, &salt)?;

    let mut blob = Vec::with_capacity(SALT_LEN + HASH_LEN);
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&hash);

    keystore
        .store(PIN_NAME, &blob)
        .map_err(|e| PinError::Keystore(e.to_string()))?;
    keystore
        .store(ATTEMPTS_NAME, &[0])
        .map_err(|e| PinError::Keystore(e.to_string()))?;
    Ok(())
}

/// Forgets the PIN. The password is then the only way past the lock screen.
pub fn clear<S: SecureStore>(keystore: &S) -> Result<(), PinError> {
    keystore
        .erase(PIN_NAME)
        .map_err(|e| PinError::Keystore(e.to_string()))?;
    keystore
        .erase(ATTEMPTS_NAME)
        .map_err(|e| PinError::Keystore(e.to_string()))?;
    Ok(())
}

/// Whether a PIN is set, and how many attempts remain.
pub fn status<S: SecureStore>(keystore: &S) -> Result<PinStatus, PinError> {
    let set = keystore
        .load(PIN_NAME)
        .map_err(|e| PinError::Keystore(e.to_string()))?
        .is_some();
    Ok(PinStatus {
        set,
        attempts_left: MAX_ATTEMPTS.saturating_sub(attempts(keystore)?),
    })
}

fn attempts<S: SecureStore>(keystore: &S) -> Result<u8, PinError> {
    Ok(keystore
        .load(ATTEMPTS_NAME)
        .map_err(|e| PinError::Keystore(e.to_string()))?
        .and_then(|v| v.first().copied())
        .unwrap_or(0))
}

/// Checks a PIN, counting the failure if it is wrong.
///
/// The count is written **before** the answer is returned, so killing the
/// process on a wrong guess does not hand back a free attempt.
pub fn verify<S: SecureStore>(keystore: &S, pin: &str) -> Result<bool, PinError> {
    let failed = attempts(keystore)?;
    if failed >= MAX_ATTEMPTS {
        return Err(PinError::Locked);
    }

    let Some(blob) = keystore
        .load(PIN_NAME)
        .map_err(|e| PinError::Keystore(e.to_string()))?
    else {
        return Ok(false);
    };
    if blob.len() != SALT_LEN + HASH_LEN {
        return Ok(false);
    }

    let expected = &blob[SALT_LEN..];
    let actual = derive(pin, &blob[..SALT_LEN])?;

    // Constant time: a comparison that returns early leaks how much of the
    // verifier was right, and four digits do not have much to leak.
    let ok = actual
        .iter()
        .zip(expected.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0;

    if ok {
        keystore
            .store(ATTEMPTS_NAME, &[0])
            .map_err(|e| PinError::Keystore(e.to_string()))?;
    } else {
        keystore
            .store(ATTEMPTS_NAME, &[failed + 1])
            .map_err(|e| PinError::Keystore(e.to_string()))?;
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeKeystore {
        items: RefCell<HashMap<String, Vec<u8>>>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("fake keystore failure")]
    struct FakeError;

    impl SecureStore for FakeKeystore {
        type Error = FakeError;

        fn store(&self, name: &str, secret: &[u8]) -> Result<(), Self::Error> {
            self.items
                .borrow_mut()
                .insert(name.to_string(), secret.to_vec());
            Ok(())
        }

        fn load(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, Self::Error> {
            Ok(self
                .items
                .borrow()
                .get(name)
                .map(|v| Zeroizing::new(v.clone())))
        }

        fn erase(&self, name: &str) -> Result<(), Self::Error> {
            self.items.borrow_mut().remove(name);
            Ok(())
        }
    }

    #[test]
    fn a_pin_verifies_against_itself() {
        let ks = FakeKeystore::default();
        set(&ks, "1234").unwrap();
        assert!(verify(&ks, "1234").unwrap());
    }

    #[test]
    fn a_wrong_pin_is_refused() {
        let ks = FakeKeystore::default();
        set(&ks, "1234").unwrap();
        assert!(!verify(&ks, "4321").unwrap());
    }

    #[test]
    fn nothing_verifies_when_no_pin_is_set() {
        let ks = FakeKeystore::default();
        assert!(!verify(&ks, "1234").unwrap());
    }

    #[test]
    fn the_verifier_is_not_the_pin() {
        // Whatever is written down, it must not be the digits themselves.
        let ks = FakeKeystore::default();
        set(&ks, "123456").unwrap();
        let blob = ks.load(PIN_NAME).unwrap().unwrap();
        assert!(!blob.windows(6).any(|w| w == b"123456"));
    }

    #[test]
    fn two_devices_with_the_same_pin_store_different_verifiers() {
        // The salt is what stops one cracked verifier answering for every
        // device that happened to pick the same four digits.
        let (a, b) = (FakeKeystore::default(), FakeKeystore::default());
        set(&a, "1234").unwrap();
        set(&b, "1234").unwrap();
        assert_ne!(
            a.load(PIN_NAME).unwrap().unwrap().to_vec(),
            b.load(PIN_NAME).unwrap().unwrap().to_vec()
        );
    }

    #[test]
    fn guessing_runs_out() {
        let ks = FakeKeystore::default();
        set(&ks, "1234").unwrap();
        for _ in 0..MAX_ATTEMPTS {
            assert!(!verify(&ks, "0000").unwrap());
        }
        // Even the right one, now: the limit is the protection, so it cannot
        // be escaped by eventually guessing correctly.
        assert!(matches!(verify(&ks, "1234"), Err(PinError::Locked)));
    }

    #[test]
    fn a_correct_pin_forgives_earlier_mistakes() {
        let ks = FakeKeystore::default();
        set(&ks, "1234").unwrap();
        assert!(!verify(&ks, "0000").unwrap());
        assert!(verify(&ks, "1234").unwrap());
        assert_eq!(status(&ks).unwrap().attempts_left, MAX_ATTEMPTS);
    }

    #[test]
    fn shape_is_enforced() {
        let ks = FakeKeystore::default();
        assert!(matches!(set(&ks, "123"), Err(PinError::Invalid(_))));
        assert!(matches!(set(&ks, "12a4"), Err(PinError::Invalid(_))));
        assert!(matches!(
            set(&ks, "1234567890123"),
            Err(PinError::Invalid(_))
        ));
    }

    #[test]
    fn clearing_removes_it() {
        let ks = FakeKeystore::default();
        set(&ks, "1234").unwrap();
        assert!(status(&ks).unwrap().set);
        clear(&ks).unwrap();
        assert!(!status(&ks).unwrap().set);
    }
}
