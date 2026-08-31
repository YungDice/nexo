//! M3: two clients, in isolation, through the API M4 will actually call.
//!
//! `mls_smoke.rs` proved the pinned OpenMLS *can* do these things using raw
//! OpenMLS calls. This suite exercises `nexo_crypto::mls` instead — the wrapper
//! everything above it uses — so a mistake in the wrapper cannot hide behind a
//! passing smoke test.
//!
//! Everything crosses between the two clients as **bytes**, never as a Rust
//! value, because that is what will happen over the wire at M4. A test that
//! passes objects around proves the crypto works and the serialisation
//! untested.

use nexo_crypto::identity::{IdentityKeypair, SafetyNumber};
use nexo_crypto::mls::{Conversation, Incoming, credential_for, generate_key_packages};
use nexo_crypto::{CryptoError, KEY_PACKAGE_TARGET};
use nexo_protocol::{ConversationId, DeviceId};
use openmls::prelude::CredentialWithKey;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;

const T0: i64 = 1_760_000_000_000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// One client: its own provider (so nothing is shared by accident), its
/// identity key, and its MLS credential.
struct Client {
    provider: OpenMlsRustCrypto,
    identity: IdentityKeypair,
    credential: CredentialWithKey,
    signer: SignatureKeyPair,
    device_id: DeviceId,
}

impl Client {
    fn new() -> Self {
        let provider = OpenMlsRustCrypto::default();
        let identity = IdentityKeypair::generate();
        let device_id = DeviceId::new_v4();
        let (credential, signer) = credential_for(device_id, &identity);
        // The signer has to be in the provider's store for OpenMLS to find it.
        signer
            .store(openmls_traits::OpenMlsProvider::storage(&provider))
            .unwrap();
        Self {
            provider,
            identity,
            credential,
            signer,
            device_id,
        }
    }

    fn key_package(&self) -> Vec<u8> {
        generate_key_packages(&self.provider, &self.signer, self.credential.clone(), 1)
            .unwrap()
            .remove(0)
    }
}

/// Alice creates a conversation and invites Bob. Returns both sides, joined.
fn paired() -> (Client, Conversation, Client, Conversation) {
    let alice = Client::new();
    let bob = Client::new();

    let mut alice_conv = Conversation::create(
        &alice.provider,
        &alice.signer,
        alice.credential.clone(),
        ConversationId::new_v4(),
        T0,
    )
    .unwrap();

    let commit = alice_conv
        .add_member(&alice.provider, &alice.signer, &bob.key_package())
        .unwrap();
    alice_conv.confirm_commit(&alice.provider, T0).unwrap();

    let welcome = commit.welcome.expect("adding a member produces a welcome");
    let bob_conv = Conversation::join(&bob.provider, &welcome, T0).unwrap();

    (alice, alice_conv, bob, bob_conv)
}

#[test]
fn a_one_to_one_conversation_is_a_two_member_group() {
    // Brief 4.2: no special-casing for 1:1.
    let (_a, alice, _b, bob) = paired();
    assert_eq!(alice.member_count(), 2);
    assert_eq!(bob.member_count(), 2);
    assert_eq!(alice.epoch(), bob.epoch());
}

#[test]
fn the_group_id_is_the_conversation_id() {
    // No mapping table, so the two can never disagree.
    let alice = Client::new();
    let id = ConversationId::new_v4();
    let conv = Conversation::create(
        &alice.provider,
        &alice.signer,
        alice.credential.clone(),
        id,
        T0,
    )
    .unwrap();
    assert_eq!(conv.conversation_id(), Some(id));
}

#[test]
fn a_message_round_trips_and_names_its_sender() {
    let (a, mut alice, b, mut bob) = paired();

    let ciphertext = alice
        .encrypt(&a.provider, &a.signer, b"hello from nexo")
        .unwrap();

    match bob.decrypt(&b.provider, &ciphertext).unwrap() {
        Incoming::Message { sender, plaintext } => {
            assert_eq!(plaintext, b"hello from nexo");
            assert_eq!(
                sender,
                Some(a.device_id),
                "the MLS member is the device, so the sender is a device id"
            );
        }
        other => panic!("expected a message, got {other:?}"),
    }
    let _ = b;
}

#[test]
fn the_ciphertext_does_not_contain_the_plaintext() {
    let (a, mut alice, _b, _bob) = paired();
    let secret = b"the quick brown fox jumps";
    let ciphertext = alice.encrypt(&a.provider, &a.signer, secret).unwrap();
    assert!(
        !ciphertext
            .windows(secret.len())
            .any(|w| w == secret.as_slice()),
        "plaintext found in the ciphertext"
    );
}

#[test]
fn messages_flow_in_both_directions() {
    let (a, mut alice, b, mut bob) = paired();

    let from_alice = alice.encrypt(&a.provider, &a.signer, b"ping").unwrap();
    assert!(matches!(
        bob.decrypt(&b.provider, &from_alice).unwrap(),
        Incoming::Message { .. }
    ));

    let from_bob = bob.encrypt(&b.provider, &b.signer, b"pong").unwrap();
    match alice.decrypt(&a.provider, &from_bob).unwrap() {
        Incoming::Message { plaintext, .. } => assert_eq!(plaintext, b"pong"),
        other => panic!("expected a message, got {other:?}"),
    }
}

#[test]
fn a_rekey_advances_the_epoch_on_both_sides() {
    let (a, mut alice, b, mut bob) = paired();
    let before = alice.epoch();

    let commit = alice.rekey(&a.provider, &a.signer).unwrap();
    alice.confirm_commit(&a.provider, T0).unwrap();
    match bob.decrypt(&b.provider, &commit.message).unwrap() {
        Incoming::CommitApplied { epoch } => assert!(epoch > before),
        other => panic!("expected a commit, got {other:?}"),
    }

    assert!(alice.epoch() > before);
    assert_eq!(alice.epoch(), bob.epoch());
}

#[test]
fn messages_still_flow_after_a_rekey() {
    // A rekey that silently broke the conversation would be worse than none.
    let (a, mut alice, b, mut bob) = paired();

    let commit = alice.rekey(&a.provider, &a.signer).unwrap();
    alice.confirm_commit(&a.provider, T0).unwrap();
    bob.decrypt(&b.provider, &commit.message).unwrap();

    let ciphertext = alice.encrypt(&a.provider, &a.signer, b"after").unwrap();
    match bob.decrypt(&b.provider, &ciphertext).unwrap() {
        Incoming::Message { plaintext, .. } => assert_eq!(plaintext, b"after"),
        other => panic!("expected a message, got {other:?}"),
    }
}

/// PLAN.md risk 4(a) and 4(b), in one test.
///
/// Commits are strictly epoch-ordered. A stale one must be **rejected**, and it
/// must be distinguishable from corruption, because the remedy is different:
/// resync and rebuild, not "show the user a broken message".
#[test]
fn a_commit_that_lost_the_race_is_rejected_as_a_stale_epoch() {
    let (a, mut alice, b, mut bob) = paired();
    let epoch = alice.epoch();

    // Both sides decide to rekey at the same moment, against the same epoch.
    let alices = alice.rekey(&a.provider, &a.signer).unwrap();
    let bobs = bob.rekey(&b.provider, &b.signer).unwrap();

    // The delivery service orders them. Alice's arrives first and wins.
    alice.confirm_commit(&a.provider, T0).unwrap();

    // Bob loses: he abandons his own commit and applies the winner. This is
    // the "resync and rebuild" of risk 4(b).
    bob.abandon_commit(&b.provider).unwrap();
    match bob.decrypt(&b.provider, &alices.message).unwrap() {
        Incoming::CommitApplied { .. } => {}
        other => panic!("expected a commit, got {other:?}"),
    }
    assert_eq!(alice.epoch(), bob.epoch());
    assert!(alice.epoch() > epoch);

    // Bob's commit now reaches Alice. It cites an epoch that is gone.
    let error = alice
        .decrypt(&a.provider, &bobs.message)
        .expect_err("a commit that lost the race must never be applied");

    assert!(
        matches!(error, CryptoError::StaleEpoch { .. }),
        "a losing commit must be reported as StaleEpoch, not as corruption: {error:?}"
    );
}

/// Merging your own commit before the server accepts it is the bug this API
/// shape exists to prevent.
#[test]
fn an_abandoned_commit_leaves_the_epoch_untouched() {
    let (a, mut alice, _b, _bob) = paired();
    let before = alice.epoch();

    alice.rekey(&a.provider, &a.signer).unwrap();
    assert_eq!(
        alice.epoch(),
        before,
        "creating a commit must not move the epoch on its own"
    );

    alice.abandon_commit(&a.provider).unwrap();
    assert_eq!(alice.epoch(), before);

    // And the conversation still works afterwards.
    let commit = alice.rekey(&a.provider, &a.signer).unwrap();
    alice.confirm_commit(&a.provider, T0).unwrap();
    assert!(alice.epoch() > before);
    let _ = commit;
}

#[test]
fn garbage_is_undecryptable_rather_than_a_stale_epoch() {
    // The two failures must not be confused: one means resync, the other means
    // show the user that a message could not be read.
    let (_a, _alice, b, mut bob) = paired();
    let error = bob
        .decrypt(&b.provider, b"not an mls message at all")
        .expect_err("garbage must not decrypt");
    assert!(matches!(error, CryptoError::Undecryptable), "got {error:?}");
}

#[test]
fn application_messages_tolerate_bounded_reordering() {
    // The other half of risk 4(a): *application* messages may arrive out of
    // order, and the secret tree handles it. Only commits are strict.
    let (a, mut alice, b, mut bob) = paired();

    let one = alice.encrypt(&a.provider, &a.signer, b"one").unwrap();
    let two = alice.encrypt(&a.provider, &a.signer, b"two").unwrap();

    match bob.decrypt(&b.provider, &two).unwrap() {
        Incoming::Message { plaintext, .. } => assert_eq!(plaintext, b"two"),
        other => panic!("expected a message, got {other:?}"),
    }
    match bob.decrypt(&b.provider, &one).unwrap() {
        Incoming::Message { plaintext, .. } => assert_eq!(plaintext, b"one"),
        other => panic!("expected a message, got {other:?}"),
    }
}

/// The reason the MLS signing key is the identity key.
#[test]
fn the_safety_number_covers_the_keys_that_sign_the_messages() {
    let (a, alice, b, _bob) = paired();

    let members = alice.member_identity_keys();
    assert_eq!(members.len(), 2);

    // Both members' signature keys are exactly the two identity public keys,
    // so a fingerprint over them is a fingerprint over what actually
    // authenticates this conversation.
    let mut from_group = members;
    from_group.sort();
    let mut expected = vec![
        a.identity.public_bytes().to_vec(),
        b.identity.public_bytes().to_vec(),
    ];
    expected.sort();
    assert_eq!(from_group, expected);

    // And the safety number the UI would show is computable from them.
    let number = SafetyNumber::new(&expected[0], &expected[1]).unwrap();
    assert_eq!(number.to_display_string().split(' ').count(), 12);
}

#[test]
fn the_rekey_policy_counts_messages_sent() {
    let (a, mut alice, _b, _bob) = paired();
    assert_eq!(alice.sent_since_rekey(), 0);

    for _ in 0..5 {
        alice.encrypt(&a.provider, &a.signer, b"x").unwrap();
    }
    assert_eq!(alice.sent_since_rekey(), 5);
    assert!(!alice.needs_rekey(T0));

    for _ in 0..95 {
        alice.encrypt(&a.provider, &a.signer, b"x").unwrap();
    }
    assert_eq!(alice.sent_since_rekey(), 100);
    assert!(
        alice.needs_rekey(T0),
        "brief 4.2: rekey every 100 messages sent"
    );
}

#[test]
fn a_rekey_resets_the_counter_and_the_clock() {
    let (a, mut alice, _b, _bob) = paired();
    for _ in 0..100 {
        alice.encrypt(&a.provider, &a.signer, b"x").unwrap();
    }
    assert!(alice.needs_rekey(T0));

    alice.rekey(&a.provider, &a.signer).unwrap();
    alice.confirm_commit(&a.provider, T0).unwrap();
    assert_eq!(alice.sent_since_rekey(), 0);
    assert!(!alice.needs_rekey(T0));
    assert!(
        alice.needs_rekey(T0 + 7 * DAY_MS),
        "brief 4.2: or every 7 days, whichever comes first"
    );
}

#[test]
fn applying_a_commit_resets_the_receivers_counter_too() {
    // The epoch moved for both sides, so both sides' policies reset.
    let (a, mut alice, b, mut bob) = paired();
    for _ in 0..10 {
        bob.encrypt(&b.provider, &b.signer, b"x").unwrap();
    }
    assert_eq!(bob.sent_since_rekey(), 10);

    let commit = alice.rekey(&a.provider, &a.signer).unwrap();
    alice.confirm_commit(&a.provider, T0).unwrap();
    bob.decrypt(&b.provider, &commit.message).unwrap();
    assert_eq!(bob.sent_since_rekey(), 0);
}

#[test]
fn adding_a_member_rekeys_by_itself() {
    // Brief 4.2: "always on member add or remove".
    let (a, mut alice, _b, _bob) = paired();
    for _ in 0..10 {
        alice.encrypt(&a.provider, &a.signer, b"x").unwrap();
    }

    let carol = Client::new();
    let before = alice.epoch();
    alice
        .add_member(&a.provider, &a.signer, &carol.key_package())
        .unwrap();
    alice.confirm_commit(&a.provider, T0).unwrap();

    assert!(alice.epoch() > before);
    assert_eq!(alice.sent_since_rekey(), 0);
    assert_eq!(alice.member_count(), 3);
}

#[test]
fn a_full_batch_of_key_packages_is_generated() {
    let alice = Client::new();
    let packages = generate_key_packages(
        &alice.provider,
        &alice.signer,
        alice.credential.clone(),
        KEY_PACKAGE_TARGET,
    )
    .unwrap();

    assert_eq!(packages.len(), KEY_PACKAGE_TARGET);
    // Single-use means each one must be distinct; a batch of identical packages
    // would let one invite consume all of them.
    let mut sorted = packages.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        KEY_PACKAGE_TARGET,
        "key packages must be unique"
    );
}

#[test]
fn a_key_package_is_consumed_by_the_invite_that_uses_it() {
    // Using the same package twice must not silently produce two members.
    let alice = Client::new();
    let bob = Client::new();
    let package = bob.key_package();

    let mut conv = Conversation::create(
        &alice.provider,
        &alice.signer,
        alice.credential.clone(),
        ConversationId::new_v4(),
        T0,
    )
    .unwrap();

    conv.add_member(&alice.provider, &alice.signer, &package)
        .unwrap();
    conv.confirm_commit(&alice.provider, T0).unwrap();
    let second = conv.add_member(&alice.provider, &alice.signer, &package);
    assert!(
        second.is_err(),
        "re-using a key package must be refused, not silently accepted"
    );
}
