//! M0 acceptance test for the MLS layer.
//!
//! This is not the M3 test suite — it is the proof that the pinned OpenMLS
//! version supports the four behaviours M3 depends on, so that M3 cannot
//! discover a dead end late:
//!
//!   1. a two-member group forms from a Welcome that crossed the wire as bytes;
//!   2. an application message round-trips;
//!   3. an Update commit advances the epoch on both sides (the rekey policy);
//!   4. a *stale* commit is rejected rather than applied out of order,
//!      while application messages tolerate bounded reordering.
//!
//! (4) is the correction recorded in docs/PLAN.md risk 4(a).
use openmls::prelude::{tls_codec::*, *};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;

use nexo_crypto::{CIPHERSUITE as CS, group_create_config};

fn credential(id: &str, p: &OpenMlsRustCrypto) -> (CredentialWithKey, SignatureKeyPair) {
    let keys = SignatureKeyPair::new(CS.signature_algorithm()).unwrap();
    keys.store(p.storage()).unwrap();
    (
        CredentialWithKey {
            credential: BasicCredential::new(id.into()).into(),
            signature_key: keys.to_public_vec().into(),
        },
        keys,
    )
}

/// Everything crosses the wire as bytes, exactly as it will in M4.
fn wire(out: &MlsMessageOut) -> MlsMessageIn {
    MlsMessageIn::tls_deserialize_exact(out.to_bytes().unwrap()).unwrap()
}

fn roundtrip(out: &MlsMessageOut) -> ProtocolMessage {
    wire(out).try_into_protocol_message().unwrap()
}

fn welcome_of(out: &MlsMessageOut) -> Welcome {
    match wire(out).extract() {
        MlsMessageBodyIn::Welcome(w) => w,
        _ => panic!("expected a welcome"),
    }
}

#[test]
fn two_clients_exchange_rekey_and_reject_stale_commit() {
    let (ap, bp) = (OpenMlsRustCrypto::default(), OpenMlsRustCrypto::default());
    let (alice_cred, alice_sk) = credential("alice", &ap);
    let (bob_cred, bob_sk) = credential("bob", &bp);

    let bob_kp = KeyPackage::builder()
        .build(CS, &bp, &bob_sk, bob_cred)
        .unwrap();

    let cfg = group_create_config();

    // --- 1:1 group (a two-member group, no special-casing) ---
    let mut alice = MlsGroup::new(&ap, &alice_sk, &cfg, alice_cred).unwrap();
    let (_commit, welcome, _gi) = alice
        .add_members(&ap, &alice_sk, core::slice::from_ref(bob_kp.key_package()))
        .unwrap();
    alice.merge_pending_commit(&ap).unwrap();

    let welcome = welcome_of(&welcome);
    let mut bob = StagedWelcome::new_from_welcome(&bp, cfg.join_config(), welcome, None)
        .unwrap()
        .into_group(&bp)
        .unwrap();

    assert_eq!(alice.members().count(), 2);
    assert_eq!(
        alice.epoch_authenticator().as_slice(),
        bob.epoch_authenticator().as_slice(),
        "both sides must agree on the epoch authenticator"
    );

    // --- application message ---
    let out = alice
        .create_message(&ap, &alice_sk, b"hello from nexo")
        .unwrap();
    match bob
        .process_message(&bp, roundtrip(&out))
        .unwrap()
        .into_content()
    {
        ProcessedMessageContent::ApplicationMessage(m) => {
            assert_eq!(m.into_bytes(), b"hello from nexo")
        }
        _ => panic!("expected an application message"),
    }

    // --- rekey: Update commit (the §4.2 100-message / 7-day policy) ---
    let epoch_before = alice.epoch();
    let (commit, _w, _gi) = alice
        .self_update(&ap, &alice_sk, LeafNodeParameters::default())
        .unwrap()
        .into_contents();
    let staged = match bob
        .process_message(&bp, roundtrip(&commit))
        .unwrap()
        .into_content()
    {
        ProcessedMessageContent::StagedCommitMessage(c) => c,
        _ => panic!("expected a commit"),
    };
    bob.merge_staged_commit(&bp, *staged).unwrap();
    alice.merge_pending_commit(&ap).unwrap();
    assert!(alice.epoch() > epoch_before, "rekey must advance the epoch");
    assert_eq!(alice.epoch(), bob.epoch());

    // --- Risk 4(a): a STALE commit is rejected, not applied out of order ---
    // Alice builds a commit, then the epoch moves on beneath it.
    let (stale, _w, _gi) = alice
        .self_update(&ap, &alice_sk, LeafNodeParameters::default())
        .unwrap()
        .into_contents();
    alice.clear_pending_commit(ap.storage()).unwrap();
    let (fresh, _w, _gi) = alice
        .self_update(&ap, &alice_sk, LeafNodeParameters::default())
        .unwrap()
        .into_contents();
    let s = match bob
        .process_message(&bp, roundtrip(&fresh))
        .unwrap()
        .into_content()
    {
        ProcessedMessageContent::StagedCommitMessage(c) => c,
        _ => panic!("expected a commit"),
    };
    bob.merge_staged_commit(&bp, *s).unwrap();
    alice.merge_pending_commit(&ap).unwrap();

    let err = bob.process_message(&bp, roundtrip(&stale));
    assert!(
        err.is_err(),
        "a stale commit must be rejected, never applied"
    );

    // --- bounded out-of-order tolerance applies to APPLICATION messages ---
    let m1 = alice.create_message(&ap, &alice_sk, b"one").unwrap();
    let m2 = alice.create_message(&ap, &alice_sk, b"two").unwrap();
    assert!(
        bob.process_message(&bp, roundtrip(&m2)).is_ok(),
        "m2 before m1"
    );
    assert!(
        bob.process_message(&bp, roundtrip(&m1)).is_ok(),
        "m1 after m2"
    );
}
