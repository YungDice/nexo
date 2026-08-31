//! The platform seam.
//!
//! §12 of the brief requires that every OS call sit behind a trait so the
//! Android port swaps implementations rather than rewriting callers. That trait
//! is defined here in M0; the Windows DPAPI implementation lands in M2.
//!
//! Nothing above this crate may call a Windows API directly.

// `deny` rather than the `forbid` every other crate in this workspace uses.
// DPAPI is a C API and reaching it needs FFI, which needs `unsafe`; `forbid`
// cannot be locally overridden, which is the point of it. The exception is
// confined to `dpapi::ffi` -- two calls, documented there -- and the
// alternative was a third-party wrapper containing the same `unsafe` somewhere
// we do not read. This is the ONLY crate where the relaxation applies.
#![deny(unsafe_code)]
#![warn(missing_docs)]

use zeroize::Zeroizing;

/// Wraps and unwraps a secret using an OS-held key that never leaves the OS.
///
/// The only thing Nexo puts through this in v0.1 is the 32-byte SQLCipher key
/// for `store.db` (§4.3). Windows backs it with DPAPI (`CryptProtectData`, user
/// scope, `CRYPTPROTECT_UI_FORBIDDEN`); Android will back it with the Keystore.
///
/// Implementations must guarantee that unwrapping is impossible for any other
/// OS user, and that [`SecureStore::erase`] makes the wrapped blob permanently
/// unrecoverable — that is what makes "delete the keyring, lose the database"
/// true rather than aspirational (§10).
pub trait SecureStore {
    /// The error type this implementation reports.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Wrap `secret` with the OS-held key and persist it under `name`.
    fn store(&self, name: &str, secret: &[u8]) -> Result<(), Self::Error>;

    /// Unwrap the secret previously stored under `name`.
    ///
    /// Returns `Ok(None)` when nothing is stored under that name — a missing
    /// secret is a normal first-run state, not an error.
    ///
    /// The result is [`Zeroizing`] so the plaintext key is wiped when dropped,
    /// per rule 6.
    fn load(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, Self::Error>;

    /// Permanently destroy the wrapped secret stored under `name`.
    fn erase(&self, name: &str) -> Result<(), Self::Error>;
}

#[cfg(any(target_os = "windows", doc))]
pub mod dpapi;

/// The name the SQLCipher key for `store.db` is stored under.
pub const STORE_KEY_NAME: &str = "store-db-key";
