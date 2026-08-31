//! Round-trips a real object through real Hetzner buckets.
//!
//! Ignored by default, because it needs credentials and costs a few requests.
//! CI never runs it. Run it by hand after Phase 8 of docs/OPS.md:
//!
//! ```text
//! cargo test -p nexo-server --test s3_smoke -- --ignored --nocapture
//! ```
//!
//! It reads `NEXO_S3_*` from the environment or from `.env`, and prints bucket
//! names and byte counts only. Access keys and secrets are never printed, never
//! included in an assertion message, and never written anywhere: the point of
//! this file is that you can prove the setup works without the credentials
//! leaving the machine (docs/TUTORIAL.md 9).

use nexo_server::Storage;
use nexo_server::storage::{Encrypted, Media, ObjectKey};
use uuid::Uuid;

/// Loads configuration, or explains what is missing and stops.
fn storage() -> Storage {
    let _ = dotenvy::dotenv();
    match Storage::from_env() {
        Ok(Some(storage)) => storage,
        Ok(None) => panic!(
            "object storage is not configured. Fill the NEXO_S3_* values in \
             .env (see .env.example) and run this again."
        ),
        Err(error) => panic!("{error}"),
    }
}

#[tokio::test]
#[ignore = "needs real Hetzner credentials"]
async fn both_buckets_are_reachable() {
    let storage = storage();

    storage
        .media()
        .check()
        .await
        .expect("media bucket should be reachable with the media credentials");
    println!("ok: reached {}", storage.media().name());

    storage
        .encrypted()
        .check()
        .await
        .expect("encrypted bucket should be reachable with the encrypted credentials");
    println!("ok: reached {}", storage.encrypted().name());
}

/// The property the whole two-bucket design rests on. If this fails, both
/// buckets are sharing one credential pair and the split is decorative.
#[tokio::test]
#[ignore = "needs real Hetzner credentials"]
async fn media_credentials_cannot_reach_the_encrypted_bucket() {
    let storage = storage();

    storage
        .verify_isolation()
        .await
        .expect("media credentials must be refused by the encrypted bucket");

    println!(
        "ok: the {} credentials are refused by {}",
        storage.media().name(),
        storage.encrypted().name()
    );
}

#[tokio::test]
#[ignore = "needs real Hetzner credentials"]
async fn a_media_object_round_trips() {
    let storage = storage();
    let key = ObjectKey::<Media>::new(Uuid::new_v4());
    let body = b"nexo media smoke test".to_vec();

    storage
        .media()
        .put(&key, body.clone(), Some("application/octet-stream"))
        .await
        .expect("put");
    println!("ok: wrote {} bytes to {}", body.len(), key.as_str());

    let read_back = storage.media().get(&key).await.expect("get");
    assert_eq!(read_back, body, "what came back is not what went in");
    println!("ok: read {} bytes back identical", read_back.len());

    storage.media().delete(&key).await.expect("delete");
    assert!(
        storage.media().get(&key).await.is_err(),
        "the object should be gone after delete"
    );
    println!("ok: deleted, and it is gone");
}

/// The size M6 actually has to carry. Objects under 64 kB bill as 64 kB on
/// Hetzner, so this is also the first test whose cost is worth a thought.
#[tokio::test]
#[ignore = "needs real Hetzner credentials; uploads 20 MB"]
async fn a_twenty_megabyte_encrypted_object_round_trips() {
    let storage = storage();
    let key = ObjectKey::<Encrypted>::new(Uuid::new_v4());

    // Not compressible, so nothing along the path can flatter the result.
    let body: Vec<u8> = (0..20 * 1024 * 1024)
        .map(|i: usize| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();

    // No content type: the server is not supposed to know what this is.
    storage
        .encrypted()
        .put(&key, body.clone(), None)
        .await
        .expect("put");
    println!("ok: wrote {} bytes to {}", body.len(), key.as_str());

    let read_back = storage.encrypted().get(&key).await.expect("get");
    assert_eq!(read_back.len(), body.len(), "length changed in transit");
    assert_eq!(read_back, body, "what came back is not what went in");
    println!("ok: read {} bytes back identical", read_back.len());

    storage.encrypted().delete(&key).await.expect("delete");
    println!("ok: deleted");
}
