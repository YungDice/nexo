//! The seam between session logic and the network.
//!
//! Everything in this crate is deliberately blocking. Argon2id at 64 MiB,
//! SQLCipher, and DPAPI are all CPU- or syscall-bound work with no useful
//! concurrency inside them, so making the core `async` would add a colour to
//! every function and buy nothing. The Tauri command layer owns the
//! `spawn_blocking` that keeps the UI responsive; that is the right place for
//! it, because that is where the runtime lives.
//!
//! [`Transport`] exists so that [`crate::session`] can be tested without a
//! server. The HTTP implementation arrives with M4, when the client starts
//! talking to `api.delidev.net` for real.

use serde::{Deserialize, Serialize};

/// Argon2id parameters the server tells the client to use.
///
/// Sent rather than hardcoded so they can be raised without shipping a new
/// client. A client that hardcoded them would silently keep using the old cost
/// after a server-side change, and nobody would notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Argon2Params {
    /// Memory cost in KiB.
    pub memory_kib: u32,
    /// Time cost.
    pub iterations: u32,
    /// Lanes.
    pub parallelism: u32,
}

/// What `POST /v1/auth/salt` returns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaltResponse {
    /// Hex-encoded per-account salt.
    ///
    /// For a handle with no account this is a decoy the server derives
    /// deterministically, so that asking cannot reveal whether an account
    /// exists. The client cannot tell the difference, and does not need to.
    pub salt: String,
    /// The parameters to derive the verifier with.
    pub argon2: Argon2Params,
}

/// What the auth endpoints return on success.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTokens {
    /// Short-lived bearer token.
    pub access_token: String,
    /// Long-lived, single-use, rotating.
    pub refresh_token: String,
    /// Seconds until `access_token` expires.
    pub expires_in: u64,
    /// Server-assigned account id.
    pub user_id: i64,
    /// This device's id.
    pub device_id: String,
}

/// Errors a transport can report.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    /// The server said the credentials were wrong, or the handle is unknown.
    ///
    /// One variant for both on purpose: the server refuses to distinguish them
    /// (see its `salt` endpoint), and a client that split them apart would
    /// reintroduce the enumeration oracle in its own error messages.
    #[error("that handle and password do not match an account")]
    InvalidCredentials,
    /// The handle is already registered.
    #[error("that handle is already taken")]
    HandleTaken,
    /// The current password given to change-password was wrong.
    ///
    /// Separate from [`Self::InvalidCredentials`], which also means "your
    /// session expired": the caller here is authenticated, so there is no
    /// enumeration to protect and the two need different prose.
    #[error("that is not your current password")]
    WrongPassword,
    /// A commit cited an epoch that is no longer current.
    ///
    /// Not an error to show a user: it means resync and rebuild (PLAN.md risk
    /// 4(b)), and the server tells us where to resync to.
    #[error("stale epoch: the conversation is at {current}")]
    StaleEpoch {
        /// What the server considers current.
        current: i64,
    },
    /// The server rejected the request.
    #[error("the server rejected the request: {0}")]
    Rejected(String),
    /// The server could not be reached.
    #[error("could not reach the server: {0}")]
    Unreachable(String),
}

/// A conversation as the server sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSummary {
    /// Which conversation.
    pub conversation_id: String,
    /// `dm` or `group`.
    pub kind: String,
    /// The epoch the server believes is current.
    pub epoch: i64,
    /// The newest envelope, for deciding whether a sync is worth making.
    pub latest_envelope_id: Option<i64>,
    /// Every member's handle, the caller included.
    ///
    /// The only way an invited device can name a conversation: it learns of
    /// one through this list, and MLS credentials name devices rather than
    /// accounts. Defaults to empty so an older server stays readable.
    #[serde(default)]
    pub members: Vec<String>,
}

/// One envelope, straight off the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Cursor.
    pub envelope_id: i64,
    /// Which conversation.
    pub conversation_id: String,
    /// Which device sent it.
    pub sender_device_id: String,
    /// The epoch it was built against.
    pub epoch: i64,
    /// Hex-encoded, opaque.
    pub ciphertext: String,
    /// Whether it carries a commit.
    pub is_commit: bool,
    /// Server receive time.
    pub server_timestamp_ms: i64,
}

/// What the server said about an accepted envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Accepted {
    /// The new envelope's id.
    pub envelope_id: i64,
    /// The epoch in force afterwards.
    pub epoch: i64,
}

/// A KeyPackage claimed for someone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimedKeyPackage {
    /// Hex-encoded.
    pub key_package: String,
    /// Whose device it belongs to.
    pub device_id: String,
}

/// The network calls session logic needs.
pub trait Transport {
    /// Fetch the salt and Argon2id parameters for a handle.
    fn salt(&self, handle: &str) -> Result<SaltResponse, TransportError>;

    /// Create an account.
    fn register(
        &self,
        handle: &str,
        display_name: &str,
        pw_salt_hex: &str,
        pw_verifier_hex: &str,
        identity_pubkey_hex: &str,
    ) -> Result<SessionTokens, TransportError>;

    /// Sign in to an existing account.
    fn login(
        &self,
        handle: &str,
        pw_verifier_hex: &str,
        identity_pubkey_hex: &str,
    ) -> Result<SessionTokens, TransportError>;

    /// Exchanges a refresh token for a new pair.
    ///
    /// Single-use: the token handed in is dead afterwards, and presenting it
    /// again is treated as theft (the server revokes the whole family).
    fn refresh(&self, refresh_token: &str) -> Result<SessionTokens, TransportError>;

    /// Invalidates a refresh token, ending the session server-side.
    fn logout(&self, refresh_token: &str) -> Result<(), TransportError>;

    /// Replaces the password verifier (§6.4).
    ///
    /// All three arguments are hex: the current password's verifier as proof
    /// of knowledge, then the fresh salt and the verifier derived against it.
    /// Requires an access token — the server checks possession of the session
    /// *and* knowledge of the old password, because either alone is not the
    /// account's owner.
    fn change_password(
        &self,
        old_verifier: &str,
        new_salt: &str,
        new_verifier: &str,
    ) -> Result<(), TransportError>;

    /// Remembers the access token that authenticates everything below.
    ///
    /// Held by the transport rather than passed to each call: a token that has
    /// to be threaded through every signature is a token that eventually gets
    /// logged by one of them.
    fn set_access_token(&self, token: &str);

    /// Hands over the refresh token, so an aged access token can be replaced
    /// without asking for a password.
    ///
    /// Defaulted rather than required: a transport that does not refresh is a
    /// legitimate one, and every fake in the tests would otherwise have to
    /// implement it to say "no".
    fn set_refresh_token(&self, _token: &str) {}

    /// Publishes KeyPackages so other people can invite this device.
    fn publish_key_packages(&self, key_packages: &[String]) -> Result<(), TransportError>;

    /// How many unconsumed KeyPackages are left, and the refill threshold.
    fn key_package_count(&self) -> Result<(i64, i64), TransportError>;

    /// Claims one KeyPackage for a handle. Single-use: this consumes it.
    fn claim_key_package(&self, handle: &str) -> Result<ClaimedKeyPackage, TransportError>;

    /// Registers a conversation and its members.
    /// Registers a conversation, and says which one the server settled on.
    ///
    /// The returned id is **not** always the one passed in. There is exactly
    /// one DM per pair of people, and the server enforces that: if one already
    /// exists it hands that one back rather than creating a second. Two people
    /// pressing "message" at the same moment is otherwise a race no client can
    /// win, because the check and the create are two round trips with a gap
    /// between them and only the server sees both.
    fn create_conversation(
        &self,
        conversation_id: &str,
        members: &[String],
    ) -> Result<String, TransportError>;

    /// Throws away a conversation that has never carried anything.
    ///
    /// For exactly one situation: [`create_conversation`] succeeded and the
    /// commit that follows it did not, leaving a conversation on the server
    /// that nobody holds the group for and nobody can ever send in. The server
    /// refuses the moment one envelope exists, so this cannot reach a real
    /// conversation — see `delivery::discard_conversation`.
    ///
    /// [`create_conversation`]: Transport::create_conversation
    fn discard_conversation(&self, conversation_id: &str) -> Result<(), TransportError>;

    /// Every conversation this account is in.
    fn list_conversations(&self) -> Result<Vec<ConversationSummary>, TransportError>;

    /// Hands an envelope to the delivery service.
    /// `client_msg_id` names the message so a retry is idempotent.
    ///
    /// Not optional. A client with an offline queue cannot tell "the request
    /// never arrived" from "the reply was lost", so it must retry -- and a
    /// retry without this id is a duplicate in everyone's conversation.
    fn send(
        &self,
        conversation_id: &str,
        ciphertext_hex: &str,
        epoch: i64,
        is_commit: bool,
        client_msg_id: &str,
    ) -> Result<Accepted, TransportError>;

    /// Adds someone to a conversation's membership (routing only).
    fn add_member(&self, conversation_id: &str, handle: &str) -> Result<(), TransportError>;

    /// Removes someone from a conversation's membership (routing only).
    fn remove_member(&self, conversation_id: &str, handle: &str) -> Result<(), TransportError>;

    /// Asks for a time-limited URL to PUT an encrypted attachment to.
    ///
    /// The bytes never pass through the server: it holds the bucket
    /// credentials, the client holds the file, and this is the permission that
    /// joins them for ten minutes.
    fn upload_url(
        &self,
        conversation_id: &str,
        size: u64,
    ) -> Result<(String, String), TransportError>;

    /// Asks for a time-limited URL to GET an encrypted attachment from.
    fn download_url(&self, key: &str) -> Result<String, TransportError>;

    /// PUTs bytes to a presigned URL.
    ///
    /// Part of the transport because it is the one call that goes somewhere
    /// other than the API, and a caller should not have to own an HTTP client
    /// to send a file.
    fn put_object(&self, url: &str, bytes: Vec<u8>) -> Result<(), TransportError>;

    /// GETs bytes from a presigned URL.
    fn get_object(&self, url: &str) -> Result<Vec<u8>, TransportError>;

    /// Everything after `since_id`.
    fn sync(&self, conversation_id: &str, since_id: i64) -> Result<Vec<Envelope>, TransportError>;
}
