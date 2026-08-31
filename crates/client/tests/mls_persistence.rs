//! MLS state must survive closing the app.
//!
//! This is the property that makes a conversation usable across restarts: the
//! group secrets and ratchet state live in `store.db`, so reopening the app
//! continues the conversation rather than starting a new one. Without it, every
//! restart would look to the other side like a key change.

use std::path::PathBuf;

use nexo_client::mls_state;
use nexo_crypto::identity::IdentityKeypair;
use nexo_crypto::mls::{Conversation, Incoming, credential_for, generate_key_packages};
use nexo_protocol::{ConversationId, DeviceId};
use nexo_store::EncryptedStore;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;

const T0: i64 = 1_760_000_000_000;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "nexo-mls-{}-{}-{tag}",
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

fn key() -> Vec<u8> {
    vec![0x5Au8; 32]
}

#[test]
fn an_empty_store_yields_an_empty_provider() {
    // First run. Not an error, just nothing to restore.
    let dir = TempDir::new("empty");
    let store = EncryptedStore::open(dir.db(), &key()).unwrap();
    let provider = mls_state::load(&store).unwrap();
    assert!(provider.storage().values.read().unwrap().is_empty());
}

#[test]
fn mls_state_round_trips_through_the_store() {
    let dir = TempDir::new("roundtrip");
    let store = EncryptedStore::open(dir.db(), &key()).unwrap();

    let provider = OpenMlsRustCrypto::default();
    let identity = IdentityKeypair::generate();
    let (credential, signer) = credential_for(DeviceId::new_v4(), &identity);
    signer.store(provider.storage()).unwrap();

    let _conversation =
        Conversation::create(&provider, &signer, credential, ConversationId::new_v4(), T0).unwrap();

    let before = provider.storage().values.read().unwrap().clone();
    assert!(
        !before.is_empty(),
        "creating a group should store something"
    );

    mls_state::save(&provider, &store).unwrap();
    let restored = mls_state::load(&store).unwrap();
    let after = restored.storage().values.read().unwrap().clone();

    assert_eq!(before, after, "every key and value must survive the trip");
}

/// The real property: a conversation still works after a restart.
#[test]
fn a_conversation_continues_after_a_restart() {
    let dir = TempDir::new("continue");

    // Alice persists; Bob stays in memory, standing in for the other machine.
    let bob_provider = OpenMlsRustCrypto::default();
    let bob_identity = IdentityKeypair::generate();
    let (bob_credential, bob_signer) = credential_for(DeviceId::new_v4(), &bob_identity);
    bob_signer.store(bob_provider.storage()).unwrap();
    let bob_package = generate_key_packages(&bob_provider, &bob_signer, bob_credential, 1)
        .unwrap()
        .remove(0);

    let conversation_id = ConversationId::new_v4();
    let alice_identity = IdentityKeypair::generate();
    let alice_device = DeviceId::new_v4();

    let (welcome, first_message) = {
        // --- first run of the app ---
        let store = EncryptedStore::open(dir.db(), &key()).unwrap();
        let provider = mls_state::load(&store).unwrap();
        let (credential, signer) = credential_for(alice_device, &alice_identity);
        signer.store(provider.storage()).unwrap();

        let mut conversation =
            Conversation::create(&provider, &signer, credential, conversation_id, T0).unwrap();
        let commit = conversation
            .add_member(&provider, &signer, &bob_package)
            .unwrap();
        conversation.confirm_commit(&provider, T0).unwrap();

        let message = conversation
            .encrypt(&provider, &signer, b"before the restart")
            .unwrap();

        mls_state::save(&provider, &store).unwrap();
        (commit.welcome.unwrap(), message)
    };

    let mut bob = Conversation::join(&bob_provider, &welcome, T0).unwrap();
    match bob.decrypt(&bob_provider, &first_message).unwrap() {
        Incoming::Message { plaintext, .. } => assert_eq!(plaintext, b"before the restart"),
        other => panic!("expected a message, got {other:?}"),
    }

    // --- the app closes and reopens ---
    let store = EncryptedStore::open(dir.db(), &key()).unwrap();
    let provider = mls_state::load(&store).unwrap();
    let (_credential, signer) = credential_for(alice_device, &alice_identity);

    // The group is reconstructed from what was persisted, not created afresh.
    let mut conversation = nexo_crypto::mls::Conversation::load(&provider, conversation_id, T0)
        .expect("the group should be restorable from the store")
        .expect("the group should be there");

    let after = conversation
        .encrypt(&provider, &signer, b"after the restart")
        .unwrap();

    match bob.decrypt(&bob_provider, &after).unwrap() {
        Incoming::Message { plaintext, .. } => assert_eq!(
            plaintext, b"after the restart",
            "the far side must be able to read a message sent after a restart"
        ),
        other => panic!("expected a message, got {other:?}"),
    }
}

#[test]
fn the_state_is_not_readable_without_the_store_key() {
    // The state is group secrets. It only ever exists inside the encrypted
    // file.
    let dir = TempDir::new("opaque");
    {
        let store = EncryptedStore::open(dir.db(), &key()).unwrap();
        let provider = OpenMlsRustCrypto::default();
        let identity = IdentityKeypair::generate();
        let (credential, signer) = credential_for(DeviceId::new_v4(), &identity);
        signer.store(provider.storage()).unwrap();
        Conversation::create(&provider, &signer, credential, ConversationId::new_v4(), T0).unwrap();
        mls_state::save(&provider, &store).unwrap();
    }

    let wrong = vec![0x11u8; 32];
    assert!(
        EncryptedStore::open(dir.db(), &wrong).is_err(),
        "MLS state must not be reachable with the wrong key"
    );
}
