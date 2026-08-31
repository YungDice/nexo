//! Persisting MLS state across restarts.
//!
//! Brief 4.3 puts MLS group state in `store.db`, alongside messages, and that
//! is where it goes: SQLCipher-encrypted, keyed by the OS keystore. Nothing
//! here writes plaintext key material anywhere.
//!
//! # Why one blob rather than a `StorageProvider`
//!
//! OpenMLS wants a `StorageProvider`, and the thorough answer is to implement
//! that trait over SQLite: fine-grained, incremental, and several hundred lines
//! of tedious impl sitting directly on top of key material — which is exactly
//! the kind of code that is subtly wrong for a year.
//!
//! Instead the whole of `MemoryStorage` is serialised into one row. For v0.1
//! that is not a compromise, it is the better fit:
//!
//! - **One device, one process** (PLAN.md, "Decisions taken"), so there is no
//!   concurrent writer to coordinate with and no partial-write hazard.
//! - **The state is small.** It holds group secrets, ratchet state and pending
//!   proposals — not the message history, which lives in its own tables. A
//!   handful of conversations is kilobytes.
//! - **It cannot be subtly wrong.** Either the blob round-trips or it does not,
//!   and a test can say which.
//!
//! `MemoryStorage::serialize` exists but is gated behind the crate's
//! `test-utils` feature, so this uses the public `values` map instead rather
//! than shipping a feature labelled for testing. The encoding below is ours and
//! versioned, so a future `StorageProvider` can migrate off it.
//!
//! **Revisit** when there are many groups, or when a second device makes two
//! writers possible. The format tag is what makes that migration cheap.

use nexo_store::{EncryptedStore, StoreError};
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;

/// One key/value pair as stored by OpenMLS.
type Entry = (Vec<u8>, Vec<u8>);

/// Version tag on the blob, so a later format can be told apart from this one
/// rather than misparsed.
const FORMAT_V1: u8 = 1;

/// Errors from saving or loading MLS state.
#[derive(Debug, thiserror::Error)]
pub enum MlsStateError {
    /// The store failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The stored blob is not something this build can read.
    #[error("the stored MLS state is not readable by this version")]
    Unreadable,
    /// The lock was poisoned by a panic elsewhere.
    #[error("the MLS state lock was poisoned")]
    Poisoned,
}

/// Serialises the provider's storage into the encrypted store.
pub fn save(provider: &OpenMlsRustCrypto, store: &EncryptedStore) -> Result<(), MlsStateError> {
    let values = provider
        .storage()
        .values
        .read()
        .map_err(|_| MlsStateError::Poisoned)?;

    let mut blob = Vec::with_capacity(1 + 8 + values.len() * 32);
    blob.push(FORMAT_V1);
    blob.extend_from_slice(&(values.len() as u64).to_be_bytes());
    for (key, value) in values.iter() {
        blob.extend_from_slice(&(key.len() as u64).to_be_bytes());
        blob.extend_from_slice(&(value.len() as u64).to_be_bytes());
        blob.extend_from_slice(key);
        blob.extend_from_slice(value);
    }

    store.set_mls_state(&blob)?;
    Ok(())
}

/// Builds a provider from whatever the store holds.
///
/// An empty store yields an empty provider, which is exactly right on a first
/// run: there are no groups yet.
pub fn load(store: &EncryptedStore) -> Result<OpenMlsRustCrypto, MlsStateError> {
    let provider = OpenMlsRustCrypto::default();
    let Some(blob) = store.mls_state()? else {
        return Ok(provider);
    };

    let map = decode(&blob)?;
    provider
        .storage()
        .values
        .write()
        .map_err(|_| MlsStateError::Poisoned)?
        .extend(map);

    Ok(provider)
}

/// Parses the blob written by [`save`].
///
/// Every length is checked against what is actually left, so a truncated or
/// corrupt blob is an error rather than a panic or a silently short read.
fn decode(blob: &[u8]) -> Result<Vec<Entry>, MlsStateError> {
    let mut cursor = 0usize;

    let take = |cursor: &mut usize, n: usize| -> Result<&[u8], MlsStateError> {
        let end = cursor.checked_add(n).ok_or(MlsStateError::Unreadable)?;
        let slice = blob.get(*cursor..end).ok_or(MlsStateError::Unreadable)?;
        *cursor = end;
        Ok(slice)
    };

    let version = take(&mut cursor, 1)?[0];
    if version != FORMAT_V1 {
        return Err(MlsStateError::Unreadable);
    }

    let count = u64::from_be_bytes(
        take(&mut cursor, 8)?
            .try_into()
            .map_err(|_| MlsStateError::Unreadable)?,
    );

    let mut out = Vec::new();
    for _ in 0..count {
        let key_len = u64::from_be_bytes(
            take(&mut cursor, 8)?
                .try_into()
                .map_err(|_| MlsStateError::Unreadable)?,
        ) as usize;
        let value_len = u64::from_be_bytes(
            take(&mut cursor, 8)?
                .try_into()
                .map_err(|_| MlsStateError::Unreadable)?,
        ) as usize;
        let key = take(&mut cursor, key_len)?.to_vec();
        let value = take(&mut cursor, value_len)?.to_vec();
        out.push((key, value));
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_truncated_blob_is_an_error_rather_than_a_panic() {
        // A short read here would mean silently losing group state, which
        // presents as "all your conversations broke" much later.
        let mut blob = vec![FORMAT_V1];
        blob.extend_from_slice(&2u64.to_be_bytes()); // claims two entries
        assert!(matches!(decode(&blob), Err(MlsStateError::Unreadable)));
    }

    #[test]
    fn a_wrong_version_is_refused() {
        let blob = vec![99u8, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(matches!(decode(&blob), Err(MlsStateError::Unreadable)));
    }

    #[test]
    fn an_empty_blob_is_refused() {
        assert!(matches!(decode(&[]), Err(MlsStateError::Unreadable)));
    }

    #[test]
    fn a_length_that_overruns_the_blob_is_refused() {
        let mut blob = vec![FORMAT_V1];
        blob.extend_from_slice(&1u64.to_be_bytes());
        blob.extend_from_slice(&u64::MAX.to_be_bytes()); // absurd key length
        blob.extend_from_slice(&0u64.to_be_bytes());
        assert!(matches!(decode(&blob), Err(MlsStateError::Unreadable)));
    }

    #[test]
    fn zero_entries_decode_to_nothing() {
        let mut blob = vec![FORMAT_V1];
        blob.extend_from_slice(&0u64.to_be_bytes());
        assert_eq!(decode(&blob).unwrap(), Vec::new());
    }
}
