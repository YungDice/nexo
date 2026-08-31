//! The SQLCipher key, and the only two places it ever exists.
//!
//! In memory, briefly, inside a [`Zeroizing`] buffer. On disk, only ever
//! wrapped by the OS keystore ([`SecureStore`]). There is no third place, and
//! in particular no master password: brief 4.3 chooses the OS user secret so
//! that there is nothing for a user to lose or to be phished for.
//!
//! The consequence, stated plainly because it is a support burden rather than a
//! bug: **erasing the wrapped key makes `store.db` permanently unreadable.**
//! That is brief section 10's claim, and it is only true because this module
//! keeps the key nowhere else.

use nexo_platform::{STORE_KEY_NAME, SecureStore};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

/// SQLCipher takes a 256-bit key.
pub const KEY_LEN: usize = 32;

/// Errors from getting hold of the key.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// The OS keystore refused.
    #[error("the OS keystore failed: {0}")]
    Keystore(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// A stored key was the wrong length, which means it is not our key.
    #[error("the stored key is {found} bytes, expected {KEY_LEN}")]
    WrongLength {
        /// What was actually found.
        found: usize,
    },
}

/// Loads the store key, creating one on first run.
///
/// The distinction matters to the caller: a freshly created key means there is
/// no usable database yet, so a caller that finds an existing `store.db`
/// alongside a *new* key is looking at an unrecoverable file and should say so
/// rather than reporting corruption.
pub fn load_or_create<S: SecureStore>(store: &S) -> Result<(Zeroizing<Vec<u8>>, bool), KeyError>
where
    S::Error: 'static,
{
    if let Some(existing) = store
        .load(STORE_KEY_NAME)
        .map_err(|e| KeyError::Keystore(Box::new(e)))?
    {
        if existing.len() != KEY_LEN {
            return Err(KeyError::WrongLength {
                found: existing.len(),
            });
        }
        return Ok((existing, false));
    }

    let key = generate();
    store
        .store(STORE_KEY_NAME, &key)
        .map_err(|e| KeyError::Keystore(Box::new(e)))?;
    Ok((key, true))
}

/// 32 bytes from the OS CSPRNG.
///
/// `OsRng` rather than a seeded userspace PRNG: this key is the only thing
/// standing between a stolen laptop and the whole message history, so it comes
/// from the same source the OS uses for its own keys. It is also the same
/// generator the server's password module uses, so there is one answer in this
/// repo to "where does key material come from".
fn generate() -> Zeroizing<Vec<u8>> {
    let mut key = Zeroizing::new(vec![0u8; KEY_LEN]);
    OsRng.fill_bytes(&mut key);
    key
}

/// Renders the key the way SQLCipher's `PRAGMA key` wants a raw key: an
/// `x'...'` blob literal, so it is used verbatim rather than run through
/// SQLCipher's own key-derivation.
///
/// Deriving again would be pointless work on 32 bytes that are already uniform
/// random, and it would make the key depend on SQLCipher's KDF parameters
/// staying put across versions.
pub fn as_pragma_literal(key: &[u8]) -> Zeroizing<String> {
    let mut out = String::with_capacity(key.len() * 2 + 3);
    out.push_str("x'");
    for b in key {
        out.push(char::from_digit(u32::from(b >> 4), 16).unwrap());
        out.push(char::from_digit(u32::from(b & 0x0F), 16).unwrap());
    }
    out.push('\'');
    Zeroizing::new(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// An in-memory SecureStore, so these tests say nothing about DPAPI and
    /// everything about this module's own logic.
    #[derive(Default)]
    struct FakeStore {
        items: RefCell<HashMap<String, Vec<u8>>>,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("fake store failure")]
    struct FakeError;

    impl SecureStore for FakeStore {
        type Error = FakeError;

        fn store(&self, name: &str, secret: &[u8]) -> Result<(), Self::Error> {
            self.items
                .borrow_mut()
                .insert(name.to_string(), secret.to_vec());
            Ok(())
        }

        fn load(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, Self::Error> {
            Ok(self.items.borrow().get(name).cloned().map(Zeroizing::new))
        }

        fn erase(&self, name: &str) -> Result<(), Self::Error> {
            self.items.borrow_mut().remove(name);
            Ok(())
        }
    }

    #[test]
    fn the_first_run_creates_a_key_and_says_so() {
        let store = FakeStore::default();
        let (key, created) = load_or_create(&store).unwrap();
        assert!(created, "first run must report that it created the key");
        assert_eq!(key.len(), KEY_LEN);
    }

    #[test]
    fn a_second_run_returns_the_same_key() {
        // If it did not, every restart would orphan the previous database.
        let store = FakeStore::default();
        let (first, created) = load_or_create(&store).unwrap();
        assert!(created);
        let (second, created_again) = load_or_create(&store).unwrap();
        assert!(!created_again);
        assert_eq!(first.as_slice(), second.as_slice());
    }

    #[test]
    fn erasing_the_key_orphans_the_database() {
        // Brief section 10: deleting the keyring blob makes store.db
        // permanently unreadable. A *new* key is generated, and it is not the
        // old one.
        let store = FakeStore::default();
        let (original, _) = load_or_create(&store).unwrap();
        store.erase(STORE_KEY_NAME).unwrap();
        let (replacement, created) = load_or_create(&store).unwrap();
        assert!(created);
        assert_ne!(original.as_slice(), replacement.as_slice());
    }

    #[test]
    fn keys_are_not_predictable() {
        let a = generate();
        let b = generate();
        assert_ne!(a.as_slice(), b.as_slice());
        assert!(
            a.iter().any(|&b| b != 0),
            "an all-zero key means no entropy"
        );
    }

    #[test]
    fn a_stored_key_of_the_wrong_length_is_refused() {
        // Silently padding or truncating would produce a key that opens
        // nothing, reported as database corruption a long way from the cause.
        let store = FakeStore::default();
        store.store(STORE_KEY_NAME, &[1u8; 16]).unwrap();
        assert!(matches!(
            load_or_create(&store),
            Err(KeyError::WrongLength { found: 16 })
        ));
    }

    #[test]
    fn the_pragma_literal_is_a_raw_blob() {
        let key = [0x00u8, 0x0f, 0xa5, 0xff];
        assert_eq!(&*as_pragma_literal(&key), "x'000fa5ff'");
    }

    #[test]
    fn the_pragma_literal_covers_the_whole_key() {
        let key = [0xABu8; KEY_LEN];
        let literal = as_pragma_literal(&key);
        // 64 hex characters plus x'' — a short literal would mean SQLCipher
        // silently keying off less material than we think.
        assert_eq!(literal.len(), KEY_LEN * 2 + 3);
    }
}
