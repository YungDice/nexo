//! Access tokens and refresh tokens.
//!
//! Access tokens are short-lived EdDSA JWTs (BRIEF 5.2 — deliberately not
//! HS256, so the signing material is a keypair and a leaked *verifying* key
//! forges nothing). Refresh tokens are opaque random bytes; the server stores
//! only their SHA-256, and rotates them on every use.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, anyhow, bail};
use ed25519_dalek::SigningKey;
use ed25519_dalek::pkcs8::{DecodePrivateKey, KeypairBytes};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// How long an access token is good for. Short, because it cannot be revoked:
/// revocation happens at the refresh step, and this window is how long a stolen
/// access token stays useful.
pub const ACCESS_TOKEN_TTL_SECS: u64 = 15 * 60;

/// How long a refresh token is good for.
pub const REFRESH_TOKEN_TTL_DAYS: i32 = 30;

/// Bytes of entropy in a refresh token.
const REFRESH_TOKEN_BYTES: usize = 32;

/// The claims Nexo puts in an access token, and nothing else.
///
/// No handle, no display name, no email. A JWT is base64, not encryption:
/// everything here is readable by anyone holding the token, so it carries
/// identifiers rather than personal data.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Claims {
    /// User id.
    pub sub: String,
    /// Device id — the MLS member is the device, not the user.
    pub did: String,
    /// Expiry, seconds since the Unix epoch.
    pub exp: u64,
    /// Issued at.
    pub iat: u64,
}

/// The keypair that signs and verifies access tokens.
pub struct TokenKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    /// Domain-separation input for the anti-enumeration salt in `salt.rs`.
    /// Derived from the key material, so it is stable per deployment without
    /// being another thing to configure.
    salt_seed: [u8; 32],
}

impl std::fmt::Debug for TokenKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never Debug-print key material.
        f.write_str("TokenKeys { .. }")
    }
}

impl TokenKeys {
    /// Loads the Ed25519 keypair from a PEM file.
    ///
    /// `docs/TUTORIAL.md` 5 has the `openssl genpkey` line that produces it.
    pub fn from_pem_file(path: &str) -> anyhow::Result<Self> {
        let pem = std::fs::read(path)
            .with_context(|| format!("reading the JWT key at {path}"))
            .context("NEXO_JWT_PRIVATE_KEY_PEM points at a file that could not be read")?;
        Self::from_pem_bytes(&pem)
    }

    /// Builds keys from a PKCS#8 PEM private key.
    ///
    /// Only the private key is configured, because that is all
    /// `openssl genpkey -algorithm ed25519` writes. jsonwebtoken's
    /// `DecodingKey` wants the *public* half separately, so it is derived here
    /// rather than made into a second file someone has to keep in step with the
    /// first.
    pub fn from_pem_bytes(pem: &[u8]) -> anyhow::Result<Self> {
        let encoding = EncodingKey::from_ed_pem(pem)
            .map_err(|e| anyhow!("the JWT key is not a usable Ed25519 PKCS#8 PEM: {e}"))?;

        let pem_str = std::str::from_utf8(pem).context("the JWT key file is not UTF-8")?;
        let keypair = KeypairBytes::from_pkcs8_pem(pem_str)
            .map_err(|e| anyhow!("the JWT key is not a valid Ed25519 PKCS#8 PEM: {e}"))?;
        let signing = SigningKey::try_from(&keypair)
            .map_err(|e| anyhow!("the JWT key is not a usable Ed25519 key: {e}"))?;
        let decoding = DecodingKey::from_ed_der(signing.verifying_key().as_bytes());

        let mut hasher = Sha256::new();
        hasher.update(b"nexo-salt-seed-v1");
        hasher.update(pem);
        let salt_seed: [u8; 32] = hasher.finalize().into();

        Ok(Self {
            encoding,
            decoding,
            salt_seed,
        })
    }

    /// Domain-separated seed for deriving decoy salts. Not a signing secret,
    /// but still key-derived, so it is never logged.
    pub fn salt_seed(&self) -> &[u8; 32] {
        &self.salt_seed
    }

    /// Signs an access token for one device of one user.
    pub fn issue_access_token(&self, user_id: i64, device_id: Uuid) -> anyhow::Result<String> {
        let now = unix_now()?;
        let claims = Claims {
            sub: user_id.to_string(),
            did: device_id.to_string(),
            iat: now,
            exp: now + ACCESS_TOKEN_TTL_SECS,
        };
        jsonwebtoken::encode(&Header::new(Algorithm::EdDSA), &claims, &self.encoding)
            .context("signing an access token")
    }

    /// Verifies an access token and returns its claims.
    ///
    /// The algorithm is pinned to EdDSA rather than read from the token's own
    /// header. Trusting the header is the classic JWT algorithm-confusion bug,
    /// where an attacker sets `alg` to something the verifier will accept on
    /// weaker terms.
    pub fn verify_access_token(&self, token: &str) -> anyhow::Result<Claims> {
        let mut validation = Validation::new(Algorithm::EdDSA);
        validation.validate_exp = true;
        validation.required_spec_claims = ["exp", "sub"].into_iter().map(String::from).collect();
        let data = jsonwebtoken::decode::<Claims>(token, &self.decoding, &validation)
            .context("the access token is not valid")?;
        Ok(data.claims)
    }
}

/// A freshly minted refresh token: the value to hand the client, and the hash
/// to store.
pub struct RefreshToken {
    /// Give this to the client. It is never written down anywhere.
    pub secret: String,
    /// Store this. It cannot be turned back into `secret`.
    pub hash: Vec<u8>,
}

/// Mints a refresh token from the OS CSPRNG.
pub fn new_refresh_token() -> RefreshToken {
    let mut bytes = [0u8; REFRESH_TOKEN_BYTES];
    OsRng.fill_bytes(&mut bytes);
    // URL-safe and unpadded, so it survives headers, query strings and JSON
    // without anyone reaching for an encoder.
    let secret = base64_url(&bytes);
    let hash = hash_refresh_token(&secret);
    RefreshToken { secret, hash }
}

/// SHA-256 of a refresh token, which is the only form the database sees.
///
/// A plain hash rather than Argon2 on purpose: this input is 256 bits of
/// uniform randomness, so there is no dictionary to slow down, and login paths
/// should not carry avoidable cost.
pub fn hash_refresh_token(secret: &str) -> Vec<u8> {
    Sha256::digest(secret.as_bytes()).to_vec()
}

/// Seconds since the Unix epoch.
fn unix_now() -> anyhow::Result<u64> {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the system clock is before the Unix epoch")?;
    Ok(since.as_secs())
}

/// Minimal URL-safe base64 without padding.
fn base64_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        let take = chunk.len() + 1;
        for i in 0..take {
            let idx = (n >> (18 - 6 * i)) & 0x3F;
            out.push(ALPHABET[idx as usize] as char);
        }
    }
    out
}

/// Rejects a refresh token that is expired, revoked, or already rotated.
///
/// Split out from the query so the rule is testable and stated once.
pub fn classify(expired: bool, revoked: bool, already_used: bool) -> Result<(), RefreshRejection> {
    if already_used {
        // Two parties hold this token. Which one is the thief is not knowable
        // here, so the whole family goes.
        return Err(RefreshRejection::Reused);
    }
    if revoked {
        return Err(RefreshRejection::Revoked);
    }
    if expired {
        return Err(RefreshRejection::Expired);
    }
    Ok(())
}

/// Why a refresh token was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshRejection {
    /// Presented after it was rotated. Treated as theft.
    Reused,
    /// Explicitly revoked, by logout or by a reuse elsewhere in the family.
    Revoked,
    /// Past its 30 days.
    Expired,
}

impl RefreshRejection {
    /// Whether this rejection should revoke every other token for the user.
    pub fn revokes_family(self) -> bool {
        matches!(self, Self::Reused)
    }
}

/// Fails a startup that has no signing key in a release build.
pub fn load_from_env() -> anyhow::Result<TokenKeys> {
    match std::env::var("NEXO_JWT_PRIVATE_KEY_PEM") {
        Ok(path) if !path.trim().is_empty() => TokenKeys::from_pem_file(&path),
        _ => {
            if cfg!(debug_assertions) {
                bail!(
                    "NEXO_JWT_PRIVATE_KEY_PEM is not set. Generate one with:\n  \
                     openssl genpkey -algorithm ed25519 -out jwt-ed25519.pem\n\
                     then point NEXO_JWT_PRIVATE_KEY_PEM at it in .env. See \
                     docs/TUTORIAL.md 5."
                )
            } else {
                bail!("NEXO_JWT_PRIVATE_KEY_PEM is not set; refusing to start")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway Ed25519 PKCS#8 PEM, generated for these tests only. It signs
    /// nothing outside this file.
    const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
        MC4CAQAwBQYDK2VwBCIEIBD8O+mO1pxsOJPSKpso2043G54kPXsxDyl6dTJ6H5Io\n\
        -----END PRIVATE KEY-----\n";

    fn keys() -> TokenKeys {
        TokenKeys::from_pem_bytes(TEST_KEY_PEM.as_bytes()).expect("test key should load")
    }

    #[test]
    fn an_issued_token_verifies_and_carries_the_right_subject() {
        let keys = keys();
        let device = Uuid::new_v4();
        let token = keys.issue_access_token(42, device).unwrap();
        let claims = keys.verify_access_token(&token).unwrap();
        assert_eq!(claims.sub, "42");
        assert_eq!(claims.did, device.to_string());
        assert_eq!(claims.exp - claims.iat, ACCESS_TOKEN_TTL_SECS);
    }

    #[test]
    fn a_tampered_token_is_rejected() {
        let keys = keys();
        let token = keys.issue_access_token(1, Uuid::nil()).unwrap();
        // Flip the last character of the signature.
        let mut bad = token.clone();
        let last = bad.pop().unwrap();
        bad.push(if last == 'A' { 'B' } else { 'A' });
        assert!(keys.verify_access_token(&bad).is_err());
    }

    #[test]
    fn a_token_from_another_key_is_rejected() {
        let other = "-----BEGIN PRIVATE KEY-----\n\
            MC4CAQAwBQYDK2VwBCIEIA0EKetjp5uf2icFLisK5GAlpF/MS9/ndeJsjK/LU322\n\
            -----END PRIVATE KEY-----\n";
        let token = TokenKeys::from_pem_bytes(other.as_bytes())
            .unwrap()
            .issue_access_token(1, Uuid::nil())
            .unwrap();
        assert!(keys().verify_access_token(&token).is_err());
    }

    #[test]
    fn refresh_tokens_are_unique_and_never_stored_in_the_clear() {
        let a = new_refresh_token();
        let b = new_refresh_token();
        assert_ne!(a.secret, b.secret);
        assert_ne!(a.hash, b.hash);
        // The stored form must not contain the secret.
        assert!(!String::from_utf8_lossy(&a.hash).contains(&a.secret));
        assert_eq!(a.hash, hash_refresh_token(&a.secret));
        assert_eq!(a.hash.len(), 32);
    }

    #[test]
    fn reuse_outranks_every_other_rejection() {
        // A replayed token that is *also* expired is still theft, and theft is
        // the finding that matters.
        assert_eq!(
            classify(true, true, true).unwrap_err(),
            RefreshRejection::Reused
        );
        assert!(RefreshRejection::Reused.revokes_family());
        assert!(!RefreshRejection::Expired.revokes_family());
        assert!(!RefreshRejection::Revoked.revokes_family());
    }

    #[test]
    fn a_healthy_token_passes() {
        assert!(classify(false, false, false).is_ok());
    }

    #[test]
    fn base64_url_matches_known_vectors() {
        assert_eq!(base64_url(b""), "");
        assert_eq!(base64_url(b"f"), "Zg");
        assert_eq!(base64_url(b"fo"), "Zm8");
        assert_eq!(base64_url(b"foo"), "Zm9v");
        assert_eq!(base64_url(b"foob"), "Zm9vYg");
        assert_eq!(base64_url(b"fooba"), "Zm9vYmE");
        assert_eq!(base64_url(b"foobar"), "Zm9vYmFy");
        // URL-safe alphabet: no '+' or '/' anywhere.
        let all: Vec<u8> = (0u8..=255).collect();
        let encoded = base64_url(&all);
        assert!(!encoded.contains('+') && !encoded.contains('/') && !encoded.contains('='));
    }
}
