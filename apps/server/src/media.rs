//! Presigned URLs for object storage.
//!
//! The bytes never pass through this process. The client encrypts a file,
//! asks for a URL, and PUTs the ciphertext straight to Hetzner; a recipient
//! asks for a URL and GETs it. The server's whole role is to hold the
//! credentials and hand out time-limited permission to use them.
//!
//! That split is deliberate and it is worth being precise about what it buys:
//!
//! - **The server never handles attachment bytes.** No bandwidth, no disk, and
//!   nothing to accidentally log.
//! - **The client never handles S3 credentials.** A desktop binary ships
//!   whatever you put in it, so a client with a bucket key is a bucket key
//!   published to every user (docs/TUTORIAL.md 6).
//!
//! Uploads get 10 minutes and downloads 60 (brief 5.3). Short, because a
//! presigned URL is a bearer credential for one object: anyone holding it can
//! use it. For `nexo-enc` that matters less than it sounds — the object is
//! AES-256-GCM ciphertext whose key only exists inside an MLS message — but a
//! short life is free, and `nexo-media` holds plaintext images.

use std::time::Duration;

use aws_sdk_s3::presigning::PresigningConfig;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::post};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::bearer::Caller;
use crate::state::AppState;

/// How long an upload URL lives (brief 5.3).
const UPLOAD_TTL: Duration = Duration::from_secs(10 * 60);
/// How long a download URL lives.
const DOWNLOAD_TTL: Duration = Duration::from_secs(60 * 60);

/// Largest attachment the server will issue an upload URL for.
///
/// M6's target is 20 MB. This is the ceiling, and it is enforced here as well
/// as in the client because a limit only the client applies is not a limit.
const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

/// Media routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/media/upload", post(upload_url))
        .route("/v1/media/download", post(download_url))
}

/// Why a media request was refused.
#[derive(Debug)]
pub enum MediaError {
    /// Object storage is not configured on this server.
    NotConfigured,
    /// The request was malformed.
    Invalid(String),
    /// Too many of these, too quickly.
    TooManyRequests,
    /// Something the caller cannot act on.
    Internal(anyhow::Error),
}

#[derive(Serialize)]
struct ErrorBody {
    error: &'static str,
    message: String,
}

impl IntoResponse for MediaError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            MediaError::NotConfigured => (
                StatusCode::SERVICE_UNAVAILABLE,
                "not_configured",
                "This server has no object storage configured, so attachments \
                 are unavailable."
                    .to_string(),
            ),
            MediaError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            MediaError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests. Slow down.".to_string(),
            ),
            MediaError::Internal(error) => {
                tracing::error!(%error, "media request failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "Something went wrong. Try again.".to_string(),
                )
            }
        };
        (status, Json(ErrorBody { error, message })).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for MediaError {
    fn from(error: E) -> Self {
        MediaError::Internal(error.into())
    }
}

/// Which bucket a request is about.
///
/// Two buckets with different meanings, so the caller says which rather than
/// the server guessing from a key prefix — guessing is how a plaintext image
/// eventually ends up in the encrypted bucket, or worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bucket {
    /// Feed and profile images. Server-readable by design (§4.4).
    Media,
    /// Encrypted attachments. Opaque ciphertext.
    Encrypted,
    /// An encrypted story. Also opaque ciphertext, and in the same bucket —
    /// but under its own `story/` prefix.
    ///
    /// The prefix is not cosmetic. A story expires after 24 hours and an
    /// attachment does not, and expiry is enforced partly by a lifecycle rule
    /// on the object store (`docs/OPS.md`). A rule can only name a prefix, so
    /// without one the only rule that could reach stories would also reach
    /// every attachment in every conversation.
    Story,
}

impl Bucket {
    /// Whether this kind lives in the encrypted bucket.
    ///
    /// A method rather than `== Bucket::Encrypted` at each call site: adding
    /// `Story` made both of those quietly wrong, sending a story's presigned
    /// URL to the media credentials and therefore to the wrong bucket. A
    /// `match` here fails to compile when a fourth kind arrives instead.
    fn is_encrypted(self) -> bool {
        match self {
            Bucket::Media => false,
            Bucket::Encrypted | Bucket::Story => true,
        }
    }
}

#[derive(Deserialize)]
pub struct UploadRequest {
    /// Which bucket.
    pub bucket: Bucket,
    /// The conversation, for an encrypted attachment. Ignored for media.
    #[serde(default)]
    pub conversation_id: Option<Uuid>,
    /// Size of what will be uploaded, in bytes.
    pub size: u64,
}

#[derive(Serialize)]
pub struct UploadResponse {
    /// PUT the bytes here.
    pub url: String,
    /// The key to record, which is what a recipient asks for later.
    pub key: String,
    /// Seconds until the URL stops working.
    pub expires_in: u64,
}

async fn upload_url(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<UploadRequest>,
) -> Result<Json<UploadResponse>, MediaError> {
    // The only limit here with a bill behind it: every grant becomes an
    // object somebody pays to store, and unlike a post there is no row to
    // delete afterwards that takes the cost back.
    if !state.limits.media.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "media rate limit reached");
        return Err(MediaError::TooManyRequests);
    }

    // Validation first, capability second. A malformed request is malformed
    // whether or not this deployment has object storage, and answering
    // "unavailable" to a request that was never valid tells the caller the
    // wrong thing — and makes every test of the validation inert on a server
    // without S3, which is exactly how the story upload path shipped broken.
    if request.size == 0 {
        return Err(MediaError::Invalid("Nothing to upload.".into()));
    }
    if request.size > MAX_ATTACHMENT_BYTES {
        return Err(MediaError::Invalid(format!(
            "Attachments are limited to {} MB.",
            MAX_ATTACHMENT_BYTES / (1024 * 1024)
        )));
    }

    // The key layout is brief 5.3: media by user, encrypted by conversation,
    // stories under their own prefix. Each is namespaced by something the
    // caller demonstrably belongs to, or by nothing the caller controls, so a
    // key cannot be aimed at someone else's space.
    //
    // This decides the key and nothing else — no storage is touched — so the
    // rules stay checkable on a deployment that has no object store.
    let key = match request.bucket {
        Bucket::Media => format!("media/{}/{}", caller.user_id, Uuid::new_v4()),
        Bucket::Encrypted => {
            let conversation_id = request
                .conversation_id
                .ok_or_else(|| MediaError::Invalid("An attachment needs a conversation.".into()))?;

            // Membership is checked here, not assumed: an upload URL for a
            // conversation you are not in would let anyone write objects into
            // someone else's namespace.
            let member = sqlx::query!(
                "SELECT 1 AS \"ok!\" FROM conversation_members
                 WHERE conversation_id = $1 AND user_id = $2",
                conversation_id,
                caller.user_id
            )
            .fetch_optional(&state.db)
            .await?;
            if member.is_none() {
                return Err(MediaError::Invalid("No such conversation.".into()));
            }

            format!("enc/{conversation_id}/{}", Uuid::new_v4())
        }
        Bucket::Story => {
            // No membership check, and that is not an omission. A story
            // belongs to no conversation: its audience is everyone the author
            // shares one with, decided when the bytes are *read*
            // (`stories.rs`), not when they are written. Requiring a
            // conversation here is what the first version did, by borrowing a
            // random id — and it could never have worked, because nobody is a
            // member of a conversation that does not exist.
            //
            // The `story/` prefix is load-bearing rather than tidy: expiry is
            // partly an object-store lifecycle rule, a rule can only name a
            // prefix, and without one the only rule that reached stories would
            // reach every attachment in every conversation too.
            format!("story/{}", Uuid::new_v4())
        }
    };

    // Only now, when there is something worth storing.
    let storage = state.storage.as_ref().ok_or(MediaError::NotConfigured)?;
    let bucket_name = if request.bucket.is_encrypted() {
        storage.encrypted().name().to_string()
    } else {
        storage.media().name().to_string()
    };

    let config = PresigningConfig::expires_in(UPLOAD_TTL)
        .map_err(|e| MediaError::Internal(anyhow::anyhow!("presign config: {e}")))?;

    let presigned = storage
        .client_for(request.bucket.is_encrypted())
        .put_object()
        .bucket(&bucket_name)
        .key(&key)
        .presigned(config)
        .await
        .map_err(|e| MediaError::Internal(anyhow::anyhow!("presigning an upload: {e}")))?;

    Ok(Json(UploadResponse {
        url: presigned.uri().to_string(),
        key,
        expires_in: UPLOAD_TTL.as_secs(),
    }))
}

#[derive(Deserialize)]
pub struct DownloadRequest {
    /// Which bucket.
    pub bucket: Bucket,
    /// The key that was recorded at upload.
    pub key: String,
}

#[derive(Serialize)]
pub struct DownloadResponse {
    /// GET the bytes here.
    pub url: String,
    /// Seconds until the URL stops working.
    pub expires_in: u64,
}

async fn download_url(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<DownloadRequest>,
) -> Result<Json<DownloadResponse>, MediaError> {
    if !state.limits.media.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "media rate limit reached");
        return Err(MediaError::TooManyRequests);
    }

    let storage = state.storage.as_ref().ok_or(MediaError::NotConfigured)?;

    let bucket_name = match request.bucket {
        Bucket::Media => {
            // Feed and profile images are readable by any logged-in account by
            // design (4.4), so there is no per-object permission to check --
            // but the key still has to be one this server issues. Presigning
            // an arbitrary caller-supplied string is how a bucket ends up
            // handing out objects nobody meant to expose.
            if !is_media_key(&request.key) {
                return Err(MediaError::Invalid("That is not a media key.".into()));
            }
            storage.media().name().to_string()
        }
        Bucket::Encrypted => {
            // An `enc/` key names its conversation, so membership is checkable
            // — and has to be, or any account could fetch any conversation's
            // ciphertext. It would still be undecryptable, but "you cannot read
            // it" is a weaker guarantee than "you cannot have it".
            let conversation_id = conversation_of(&request.key)
                .ok_or_else(|| MediaError::Invalid("That is not an attachment key.".into()))?;
            let member = sqlx::query!(
                "SELECT 1 AS \"ok!\" FROM conversation_members
                 WHERE conversation_id = $1 AND user_id = $2",
                conversation_id,
                caller.user_id
            )
            .fetch_optional(&state.db)
            .await?;
            if member.is_none() {
                return Err(MediaError::Invalid("No such attachment.".into()));
            }
            storage.encrypted().name().to_string()
        }
        Bucket::Story => {
            // Refused here, deliberately, and this is not a gap.
            //
            // A story's audience is not "members of a conversation" — it is
            // everyone who shares one with the author, minus blocks, and only
            // while it is unexpired. Those three conditions live in
            // `stories.rs`, which has the row to check them against. This
            // route has only a key, so answering here would mean answering
            // without the checks. `POST /v1/stories/{id}/url` is the way in.
            return Err(MediaError::Invalid(
                "Ask for a story through the stories route.".into(),
            ));
        }
    };

    let config = PresigningConfig::expires_in(DOWNLOAD_TTL)
        .map_err(|e| MediaError::Internal(anyhow::anyhow!("presign config: {e}")))?;

    let presigned = storage
        .client_for(request.bucket.is_encrypted())
        .get_object()
        .bucket(&bucket_name)
        .key(&request.key)
        .presigned(config)
        .await
        .map_err(|e| MediaError::Internal(anyhow::anyhow!("presigning a download: {e}")))?;

    Ok(Json(DownloadResponse {
        url: presigned.uri().to_string(),
        expires_in: DOWNLOAD_TTL.as_secs(),
    }))
}

/// Whether a key is exactly `media/{user_id}/{uuid}`.
///
/// The same shape discipline as [`conversation_of`], for the same reason:
/// anything with extra segments, a missing segment, or a non-numeric owner is
/// not a key this server ever wrote.
pub fn is_media_key(key: &str) -> bool {
    let mut parts = key.split('/');
    if parts.next() != Some("media") {
        return false;
    }
    let Some(owner) = parts.next() else {
        return false;
    };
    if owner.is_empty() || !owner.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let Some(object) = parts.next() else {
        return false;
    };
    if object.parse::<Uuid>().is_err() {
        return false;
    }
    parts.next().is_none()
}

/// Pulls the conversation id out of an `enc/{conversation}/{uuid}` key.
///
/// Returns `None` for anything that is not exactly that shape — including a key
/// with extra segments, which is how a traversal attempt would look.
fn conversation_of(key: &str) -> Option<Uuid> {
    let mut parts = key.split('/');
    if parts.next()? != "enc" {
        return None;
    }
    let conversation = parts.next()?;
    parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    conversation.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_well_formed_attachment_key_names_its_conversation() {
        let id = Uuid::new_v4();
        let key = format!("enc/{id}/{}", Uuid::new_v4());
        assert_eq!(conversation_of(&key), Some(id));
    }

    #[test]
    fn a_well_formed_media_key_is_accepted() {
        assert!(is_media_key(&format!("media/42/{}", Uuid::new_v4())));
    }

    #[test]
    fn a_malformed_media_key_is_refused() {
        // Extra segments, missing segments, a non-numeric owner, and a
        // non-uuid object are all keys this server never wrote.
        let id = Uuid::new_v4();
        assert!(!is_media_key(&format!("media/42/{id}/extra")));
        assert!(!is_media_key("media/42"));
        assert!(!is_media_key(&format!("media/{id}")));
        assert!(!is_media_key(&format!("media/alice/{id}")));
        assert!(!is_media_key(&format!("media//{id}")));
        assert!(!is_media_key("media/42/not-a-uuid"));
        assert!(!is_media_key(""));
        // And an attachment key is not a media key, so the two paths cannot
        // be crossed by pointing one bucket's request at the other's layout.
        assert!(!is_media_key(&format!("enc/{id}/{id}")));
        assert!(!is_media_key(&format!("../media/42/{id}")));
    }

    #[test]
    fn a_media_key_is_not_an_attachment_key() {
        // Otherwise a membership check could be skipped by pointing a download
        // at the other bucket.
        assert_eq!(conversation_of("media/7/abc"), None);
    }

    #[test]
    fn a_key_with_extra_segments_is_refused() {
        let id = Uuid::new_v4();
        assert_eq!(conversation_of(&format!("enc/{id}/a/b")), None);
    }

    #[test]
    fn traversal_shapes_are_refused() {
        assert_eq!(conversation_of("enc/../media/7/abc"), None);
        assert_eq!(conversation_of("enc"), None);
        assert_eq!(conversation_of(""), None);
    }

    #[test]
    fn a_non_uuid_conversation_is_refused() {
        assert_eq!(conversation_of("enc/not-a-uuid/abc"), None);
    }

    #[test]
    fn the_ceiling_is_above_the_milestone_target() {
        // M6 has to carry 20 MB. A const block so lowering this stops the
        // build rather than one test -- a ceiling below the size the app
        // promises to send is not something to discover from a red test.
        const { assert!(MAX_ATTACHMENT_BYTES >= 20 * 1024 * 1024) };
    }

    #[test]
    fn upload_urls_are_shorter_lived_than_download_urls() {
        // Brief 5.3: 10 minutes and 60. A presigned URL is a bearer credential
        // for one object, and an upload URL is the more dangerous of the two —
        // it lets someone write.
        assert!(UPLOAD_TTL < DOWNLOAD_TTL);
        assert_eq!(UPLOAD_TTL.as_secs(), 600);
        assert_eq!(DOWNLOAD_TTL.as_secs(), 3600);
    }
}
