//! The whole client auth path against a real `nexo-server`.
//!
//! Ignored by default, because it needs a server and a database. Run it with
//! both up:
//!
//! ```text
//! docker compose up -d
//! pnpm dev:server                       # in another terminal
//! $env:NEXO_API_BASE = "http://127.0.0.1:8080"
//! cargo test -p nexo-client --features http --test live_auth -- --ignored --nocapture
//! ```
//!
//! The unit tests in `lib.rs` cover the same orchestration against a fake
//! transport, which is what makes them fast and what makes them run in CI.
//! This one exists to catch the things a fake cannot: a field name that does
//! not match the server's, a status code mapped wrongly, a salt encoding that
//! disagrees at the hex boundary.

#![cfg(feature = "http")]

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use nexo_client::transport::Transport;
use nexo_client::{HttpTransport, session};
use nexo_platform::SecureStore;
use zeroize::Zeroizing;

#[derive(Default)]
struct FakeKeystore {
    items: RefCell<HashMap<String, Vec<u8>>>,
}

#[derive(Debug, thiserror::Error)]
#[error("fake keystore failure")]
struct FakeKeystoreError;

impl SecureStore for FakeKeystore {
    type Error = FakeKeystoreError;
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

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "nexo-live-{}-{}-{tag}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> PathBuf {
        self.0.join("store.db")
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn transport() -> HttpTransport {
    let base =
        std::env::var("NEXO_API_BASE").unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    HttpTransport::with_base_url(base)
}

/// Handles are `[a-z0-9_]{3,20}`.
fn unique_handle() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("l{:015}", nanos % 1_000_000_000_000_000u128)
}

#[test]
#[ignore = "needs a running nexo-server and Postgres"]
fn the_whole_flow_works_against_a_real_server() {
    let t = transport();
    let keystore = FakeKeystore::default();
    let dir = TempDir::new("flow");
    let handle = unique_handle();
    let password = "a long enough development password";

    println!("server: {}", t.base_url());

    // Register.
    let created = session::register(&t, &keystore, &dir.db(), &handle, "Live Test", password)
        .expect("register should succeed against a running server");
    println!(
        "ok: registered {} as user {}",
        handle, created.account.user_id
    );
    assert_eq!(created.account.handle, handle);
    assert!(!created.access_token.is_empty());

    // Restart: nothing carried over but the keystore and the file.
    let restored = session::restore(&keystore, &dir.db())
        .expect("restore should succeed")
        .expect("an account should be on record after registering");
    assert_eq!(restored.user_id, created.account.user_id);
    assert_eq!(restored.display_name, "Live Test");
    println!("ok: still signed in after a restart");

    // Log in again, and keep the same cryptographic identity.
    let signed_in =
        session::login(&t, &keystore, &dir.db(), &handle, password).expect("login should succeed");
    assert_eq!(signed_in.account.user_id, created.account.user_id);
    assert_eq!(
        signed_in.account.display_name, "Live Test",
        "login must not rename the account"
    );
    println!("ok: signed in again, display name intact");

    // Wrong password is refused, and refused as such.
    let wrong = session::login(&t, &keystore, &dir.db(), &handle, "not the password");
    assert!(wrong.is_err(), "a wrong password must not sign in");
    println!("ok: a wrong password is refused");

    // Log out: server-side revocation plus a local wipe.
    session::logout(&t, &keystore, &dir.db(), &signed_in.refresh_token)
        .expect("logout should succeed");
    assert!(!dir.db().exists(), "the local store should be gone");
    assert!(
        session::restore(&keystore, &dir.db()).unwrap().is_none(),
        "nothing should remain to restore"
    );
    println!("ok: signed out, local store destroyed");
}

/// An unknown handle must be indistinguishable from a known one at the only
/// step a stranger can reach without credentials.
#[test]
#[ignore = "needs a running nexo-server and Postgres"]
fn an_unknown_handle_still_gets_a_salt() {
    let t = transport();
    let ghost = unique_handle();

    let first = t.salt(&ghost).expect("salt should answer for any handle");
    let again = t.salt(&ghost).expect("salt should answer again");

    assert_eq!(
        first.salt, again.salt,
        "the decoy must be stable, or asking twice reveals it"
    );
    assert_eq!(first.salt.len(), 32, "16 bytes as hex");
    assert_eq!(first.argon2.memory_kib, 64 * 1024);
    println!("ok: an unknown handle gets a stable, correctly sized salt");
}
