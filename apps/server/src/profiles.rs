//! Public profiles (§4.4, §6.3) and per-field visibility (G2).
//!
//! Everything here is server-readable, and that is the design rather than a
//! compromise. A profile exists to be shown to people who are not you; there is
//! no group to encrypt it to. §4.4 requires the UI to say so plainly, which it
//! does — see `PrivacyTable` on the Security tab.
//!
//! What that makes important is the **visibility** model. Since the server can
//! read every field, the only meaningful control is which fields it will hand
//! to whom, and that decision is made here, once, in [`visible_fields`] —
//! never by the client choosing what to render. A client-side filter over a
//! full payload is not a privacy control, it is a suggestion.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get, routing::patch};
use serde::{Deserialize, Serialize};

use crate::auth::bearer::Caller;
use crate::state::AppState;

/// Profile routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/users/{handle}", get(public_profile))
        .route("/v1/me", get(my_profile).patch(update_me))
        .route("/v1/me/visibility", patch(update_visibility))
}

/// Why a profile request was refused.
#[derive(Debug)]
pub enum ProfileError {
    /// No such handle.
    NotFound,
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

impl IntoResponse for ProfileError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            ProfileError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "No account with that handle.".to_string(),
            ),
            ProfileError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            ProfileError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests. Slow down.".to_string(),
            ),
            ProfileError::Internal(error) => {
                tracing::error!(%error, "profile request failed");
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

impl<E: Into<anyhow::Error>> From<E> for ProfileError {
    fn from(error: E) -> Self {
        ProfileError::Internal(error.into())
    }
}

/// Who may see one profile field (G2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Any logged-in account.
    Public,
    /// People this user has a conversation with.
    ///
    /// v0.1 has no friendship model, so "contacts" means exactly that: you have
    /// a conversation together. It is a real relationship the server can check
    /// rather than an invented one, which is why it is the middle setting.
    Contacts,
    /// Nobody but the owner.
    Private,
}

impl Visibility {
    /// Parses a stored value.
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "public" => Some(Visibility::Public),
            "contacts" => Some(Visibility::Contacts),
            "private" => Some(Visibility::Private),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Contacts => "contacts",
            Visibility::Private => "private",
        }
    }
}

/// A profile field whose visibility can be set.
///
/// Handle, display name, avatar, banner, and numeric id are absent on purpose:
/// they are how you are addressed and found, so hiding them would break
/// discovery while creating the impression of privacy. §6.3 lists them as
/// read-only public, and offering a switch that cannot honestly be honoured is
/// worse than offering none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// The free-text bio.
    Bio,
    /// Free text, never a geolocation API (§6.3).
    Location,
    /// The link list.
    Links,
    /// When the account was created.
    JoinDate,
}

impl Field {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "bio" => Some(Field::Bio),
            "location" => Some(Field::Location),
            "links" => Some(Field::Links),
            "join_date" => Some(Field::JoinDate),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Field::Bio => "bio",
            Field::Location => "location",
            Field::Links => "links",
            Field::JoinDate => "join_date",
        }
    }

    /// What this field defaults to when the user has never chosen.
    ///
    /// Not uniformly public. A bio is written to be read, but a location is the
    /// one field here that can put someone in physical danger, so it starts
    /// closed and opens only on a deliberate choice. Defaults are the setting
    /// almost everyone keeps.
    fn default_visibility(self) -> Visibility {
        match self {
            Field::Bio | Field::Links => Visibility::Public,
            Field::Location => Visibility::Private,
            Field::JoinDate => Visibility::Contacts,
        }
    }

    /// Every settable field, for listing in Settings.
    pub const ALL: [Field; 4] = [Field::Bio, Field::Location, Field::Links, Field::JoinDate];
}

/// A link on a profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileLink {
    /// What to show.
    pub label: String,
    /// Where it goes. `http` or `https` only, enforced in three places.
    pub url: String,
}

/// A profile as it is handed to someone.
///
/// Fields the viewer may not see are `None` — not empty strings, and not
/// present-but-blank. The distinction matters: a UI can then say "not shared"
/// rather than rendering an empty line that looks like the user wrote nothing.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileView {
    /// The numeric in-app id (§3: an in-app ID, never a phone number).
    pub user_id: i64,
    pub handle: String,
    pub display_name: String,
    pub avatar_key: Option<String>,
    pub banner_key: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub links: Option<Vec<ProfileLink>>,
    /// Milliseconds since the epoch. `None` when hidden.
    pub join_date_ms: Option<i64>,
    /// True when this is the caller's own profile.
    pub is_me: bool,
}

/// The relationship between a viewer and the profile's owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewer {
    /// Looking at their own profile.
    Owner,
    /// Shares at least one conversation with the owner.
    Contact,
    /// Any other logged-in account.
    Stranger,
}

impl Viewer {
    /// Whether this viewer may see a field set to `visibility`.
    fn may_see(self, visibility: Visibility) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Contacts => matches!(self, Viewer::Owner | Viewer::Contact),
            Visibility::Private => matches!(self, Viewer::Owner),
        }
    }
}

/// Decides which fields a viewer may see.
///
/// The single place that answers this question. Extracted from the handler so
/// it can be tested exhaustively without a database — a privacy rule that is
/// only exercised through HTTP is a privacy rule that is barely exercised.
pub fn visible_fields(viewer: Viewer, settings: &[(Field, Visibility)]) -> Vec<Field> {
    Field::ALL
        .into_iter()
        .filter(|field| {
            let visibility = settings
                .iter()
                .find(|(f, _)| f == field)
                .map(|(_, v)| *v)
                .unwrap_or_else(|| field.default_visibility());
            viewer.may_see(visibility)
        })
        .collect()
}

/// A public profile by handle.
async fn public_profile(
    State(state): State<AppState>,
    caller: Caller,
    Path(handle): Path<String>,
) -> Result<Json<ProfileView>, ProfileError> {
    let row = sqlx::query!(
        "SELECT id, handle::TEXT AS \"handle!\", display_name, bio, location,
                avatar_key, banner_key,
                (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS \"created_at_ms!\"
         FROM users WHERE handle = $1::CITEXT",
        handle
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ProfileError::NotFound)?;

    let viewer = if row.id == caller.user_id {
        Viewer::Owner
    } else if shares_a_conversation(&state, caller.user_id, row.id).await? {
        Viewer::Contact
    } else {
        Viewer::Stranger
    };

    let settings = visibility_settings(&state, row.id).await?;
    let allowed = visible_fields(viewer, &settings);

    let links = if allowed.contains(&Field::Links) {
        Some(links_for(&state, row.id).await?)
    } else {
        None
    };

    Ok(Json(ProfileView {
        user_id: row.id,
        handle: row.handle,
        display_name: row.display_name,
        avatar_key: row.avatar_key,
        banner_key: row.banner_key,
        bio: allowed.contains(&Field::Bio).then_some(row.bio).flatten(),
        location: allowed
            .contains(&Field::Location)
            .then_some(row.location)
            .flatten(),
        links,
        join_date_ms: allowed
            .contains(&Field::JoinDate)
            .then_some(row.created_at_ms),
        is_me: viewer == Viewer::Owner,
    }))
}

/// The caller's own profile, with nothing hidden.
async fn my_profile(
    State(state): State<AppState>,
    caller: Caller,
) -> Result<Json<MyProfileView>, ProfileError> {
    let row = sqlx::query!(
        "SELECT id, handle::TEXT AS \"handle!\", display_name, bio, location,
                avatar_key, banner_key,
                (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS \"created_at_ms!\"
         FROM users WHERE id = $1",
        caller.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or(ProfileError::NotFound)?;

    let stored = visibility_settings(&state, caller.user_id).await?;
    // Every field, with the effective value — so the Settings screen shows what
    // is actually in force rather than blanks for the ones never touched.
    let visibility = Field::ALL
        .into_iter()
        .map(|field| {
            let value = stored
                .iter()
                .find(|(f, _)| *f == field)
                .map(|(_, v)| *v)
                .unwrap_or_else(|| field.default_visibility());
            (field.as_str().to_string(), value)
        })
        .collect();

    Ok(Json(MyProfileView {
        profile: ProfileView {
            user_id: row.id,
            handle: row.handle,
            display_name: row.display_name,
            avatar_key: row.avatar_key,
            banner_key: row.banner_key,
            bio: row.bio,
            location: row.location,
            links: Some(links_for(&state, caller.user_id).await?),
            join_date_ms: Some(row.created_at_ms),
            is_me: true,
        },
        visibility,
    }))
}

/// Your own profile plus the settings only you can see.
#[derive(Debug, Serialize)]
pub struct MyProfileView {
    #[serde(flatten)]
    pub profile: ProfileView,
    /// Field name to visibility, every field present.
    pub visibility: std::collections::BTreeMap<String, Visibility>,
}

#[derive(Deserialize)]
pub struct UpdateMeRequest {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    /// Replaces the whole list when present. A list edit is a reorder as often
    /// as an add, and PATCHing individual links would need ids the UI has no
    /// use for.
    pub links: Option<Vec<ProfileLink>>,
    /// A key already uploaded to `nexo-media`.
    ///
    /// Committing the key is separate from presigning it (§5.3: objects are
    /// write-once) so a failed upload leaves the old picture in place rather
    /// than a profile pointing at an object that was never written.
    pub avatar_key: Option<String>,
    /// Same, for the 3:1 banner.
    pub banner_key: Option<String>,
}

async fn update_me(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<UpdateMeRequest>,
) -> Result<Json<MyProfileView>, ProfileError> {
    if !state.limits.profile.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "profile rate limit reached");
        return Err(ProfileError::TooManyRequests);
    }

    // Validated before anything is written, so a bad link cannot leave a
    // half-applied profile behind.
    let display_name = match &request.display_name {
        Some(name) => {
            let trimmed = name.trim();
            if trimmed.is_empty() || trimmed.chars().count() > 40 {
                return Err(ProfileError::Invalid(
                    "A display name is 1 to 40 characters.".into(),
                ));
            }
            Some(trimmed.to_string())
        }
        None => None,
    };

    let bio = match &request.bio {
        // 280 chars (§6.3), counted in characters rather than bytes — an emoji
        // is one character to the person typing it.
        Some(bio) if bio.chars().count() > 280 => {
            return Err(ProfileError::Invalid(
                "A bio is up to 280 characters.".into(),
            ));
        }
        Some(bio) => Some(bio.trim().to_string()),
        None => None,
    };

    let location = match &request.location {
        Some(location) if location.chars().count() > 60 => {
            return Err(ProfileError::Invalid(
                "A location is up to 60 characters.".into(),
            ));
        }
        Some(location) => Some(location.trim().to_string()),
        None => None,
    };

    if let Some(links) = &request.links {
        if links.len() > 5 {
            return Err(ProfileError::Invalid("Up to 5 links.".into()));
        }
        for link in links {
            check_link(link)?;
        }
    }

    // An image key has to be one this caller uploaded. Without the check, a
    // profile could point at anyone's object -- or at a key in the *encrypted*
    // bucket's namespace, which must never be referenced from a public row.
    for key in [&request.avatar_key, &request.banner_key]
        .into_iter()
        .flatten()
    {
        if !crate::media::is_media_key(key)
            || !key.starts_with(&format!("media/{}/", caller.user_id))
        {
            return Err(ProfileError::Invalid("That image is not yours.".into()));
        }
    }

    let mut transaction = state.db.begin().await?;

    // The keys being replaced, so their objects can be deleted afterwards.
    // §5.3: a picture change writes a new key and deletes the old one, or the
    // bucket accumulates every avatar anyone has ever had.
    let previous = sqlx::query!(
        "SELECT avatar_key, banner_key FROM users WHERE id = $1",
        caller.user_id
    )
    .fetch_one(&mut *transaction)
    .await?;

    sqlx::query!(
        "UPDATE users SET
             display_name = COALESCE($2, display_name),
             bio          = COALESCE($3, bio),
             location     = COALESCE($4, location),
             avatar_key   = COALESCE($5, avatar_key),
             banner_key   = COALESCE($6, banner_key)
         WHERE id = $1",
        caller.user_id,
        display_name,
        bio,
        location,
        request.avatar_key,
        request.banner_key
    )
    .execute(&mut *transaction)
    .await?;

    if let Some(links) = &request.links {
        sqlx::query!(
            "DELETE FROM profile_links WHERE user_id = $1",
            caller.user_id
        )
        .execute(&mut *transaction)
        .await?;
        for (position, link) in links.iter().enumerate() {
            sqlx::query!(
                "INSERT INTO profile_links (user_id, label, url, position)
                 VALUES ($1, $2, $3, $4)",
                caller.user_id,
                link.label.trim(),
                link.url.trim(),
                position as i32
            )
            .execute(&mut *transaction)
            .await?;
        }
    }

    transaction.commit().await?;

    // After the commit, and best-effort. A stale object left in the bucket is
    // untidy; a profile pointing at a deleted object is broken. If the delete
    // fails the worst case is a few kilobytes nobody references.
    if let Some(storage) = state.storage.as_ref() {
        for (new, old) in [
            (&request.avatar_key, previous.avatar_key),
            (&request.banner_key, previous.banner_key),
        ] {
            let (Some(_), Some(old)) = (new, old) else {
                continue;
            };
            if let Err(error) = storage
                .client_for(false)
                .delete_object()
                .bucket(storage.media().name())
                .key(&old)
                .send()
                .await
            {
                tracing::warn!(%error, key = %old, "could not delete a replaced image");
            }
        }
    }

    my_profile(State(state), caller).await
}

#[derive(Deserialize)]
pub struct UpdateVisibilityRequest {
    /// Field name to visibility. Only the named fields change.
    pub visibility: std::collections::BTreeMap<String, Visibility>,
}

async fn update_visibility(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<UpdateVisibilityRequest>,
) -> Result<Json<MyProfileView>, ProfileError> {
    if !state.limits.profile.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "profile rate limit reached");
        return Err(ProfileError::TooManyRequests);
    }

    let mut transaction = state.db.begin().await?;
    for (name, visibility) in &request.visibility {
        let field = Field::parse(name)
            .ok_or_else(|| ProfileError::Invalid(format!("There is no `{name}` field.")))?;
        sqlx::query!(
            "INSERT INTO profile_visibility (user_id, field, visibility)
             VALUES ($1, $2, $3)
             ON CONFLICT (user_id, field) DO UPDATE SET visibility = EXCLUDED.visibility",
            caller.user_id,
            field.as_str(),
            visibility.as_str()
        )
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    my_profile(State(state), caller).await
}

/// Whether two accounts share a conversation, which is what "contact" means.
async fn shares_a_conversation(state: &AppState, a: i64, b: i64) -> Result<bool, ProfileError> {
    let row = sqlx::query!(
        "SELECT 1 AS \"ok!\" FROM conversation_members m1
         JOIN conversation_members m2 ON m1.conversation_id = m2.conversation_id
         WHERE m1.user_id = $1 AND m2.user_id = $2
         LIMIT 1",
        a,
        b
    )
    .fetch_optional(&state.db)
    .await?;
    Ok(row.is_some())
}

/// A user's stored visibility choices. Fields never chosen are absent.
async fn visibility_settings(
    state: &AppState,
    user_id: i64,
) -> Result<Vec<(Field, Visibility)>, ProfileError> {
    let rows = sqlx::query!(
        "SELECT field, visibility FROM profile_visibility WHERE user_id = $1",
        user_id
    )
    .fetch_all(&state.db)
    .await?;

    Ok(rows
        .into_iter()
        // A row the CHECK constraint would have rejected cannot exist, but
        // filtering rather than unwrapping means a future value added to the
        // constraint degrades to "use the default" instead of a 500.
        .filter_map(|row| {
            Some((
                Field::parse(&row.field)?,
                Visibility::parse(&row.visibility)?,
            ))
        })
        .collect())
}

async fn links_for(state: &AppState, user_id: i64) -> Result<Vec<ProfileLink>, ProfileError> {
    let rows = sqlx::query!(
        "SELECT label, url FROM profile_links WHERE user_id = $1 ORDER BY position, id",
        user_id
    )
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| ProfileLink {
            label: row.label,
            url: row.url,
        })
        .collect())
}

/// Rejects a link that should never reach a profile page.
///
/// The scheme check is the point. A `javascript:` or `data:` URL rendered as an
/// anchor is stored XSS, and this is the outermost of three places that refuse
/// it — the column has a CHECK, and the UI renders links with `rel="noopener
/// noreferrer"` and opens them in the system browser, never in the WebView.
fn check_link(link: &ProfileLink) -> Result<(), ProfileError> {
    let label = link.label.trim();
    if label.is_empty() || label.chars().count() > 40 {
        return Err(ProfileError::Invalid(
            "A link label is 1 to 40 characters.".into(),
        ));
    }
    let url = link.url.trim();
    if url.len() > 200 {
        return Err(ProfileError::Invalid("That link is too long.".into()));
    }
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err(ProfileError::Invalid(
            "Links must start with http:// or https://.".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stranger_sees_only_public_fields() {
        let settings = [
            (Field::Bio, Visibility::Public),
            (Field::Location, Visibility::Contacts),
            (Field::Links, Visibility::Private),
        ];
        let seen = visible_fields(Viewer::Stranger, &settings);
        assert!(seen.contains(&Field::Bio));
        assert!(!seen.contains(&Field::Location));
        assert!(!seen.contains(&Field::Links));
    }

    #[test]
    fn a_contact_sees_public_and_contacts_fields() {
        let settings = [
            (Field::Bio, Visibility::Public),
            (Field::Location, Visibility::Contacts),
            (Field::Links, Visibility::Private),
        ];
        let seen = visible_fields(Viewer::Contact, &settings);
        assert!(seen.contains(&Field::Bio));
        assert!(seen.contains(&Field::Location));
        assert!(!seen.contains(&Field::Links));
    }

    #[test]
    fn the_owner_sees_everything_including_private() {
        let settings: Vec<(Field, Visibility)> = Field::ALL
            .into_iter()
            .map(|f| (f, Visibility::Private))
            .collect();
        let seen = visible_fields(Viewer::Owner, &settings);
        assert_eq!(seen.len(), Field::ALL.len());
    }

    #[test]
    fn location_is_private_until_someone_chooses_otherwise() {
        // The one field here that can put a person in physical danger. A
        // default of public would be a decision made on their behalf.
        assert_eq!(Field::Location.default_visibility(), Visibility::Private);
        let seen = visible_fields(Viewer::Stranger, &[]);
        assert!(!seen.contains(&Field::Location));
        assert!(!visible_fields(Viewer::Contact, &[]).contains(&Field::Location));
        assert!(visible_fields(Viewer::Owner, &[]).contains(&Field::Location));
    }

    #[test]
    fn an_unset_field_uses_its_own_default_not_a_blanket_one() {
        // The reason defaults are per-field: treating them uniformly would
        // either publish locations or hide bios, and both are wrong.
        let seen = visible_fields(Viewer::Stranger, &[]);
        assert!(seen.contains(&Field::Bio));
        assert!(seen.contains(&Field::Links));
        assert!(!seen.contains(&Field::JoinDate));
        assert!(!seen.contains(&Field::Location));
    }

    #[test]
    fn a_stored_setting_overrides_the_default_in_both_directions() {
        assert!(
            visible_fields(Viewer::Stranger, &[(Field::Location, Visibility::Public)])
                .contains(&Field::Location)
        );
        assert!(
            !visible_fields(Viewer::Stranger, &[(Field::Bio, Visibility::Private)])
                .contains(&Field::Bio)
        );
    }

    #[test]
    fn field_and_visibility_names_round_trip() {
        // These strings are in a CHECK constraint and in stored rows. A typo
        // that made one unparseable would silently fall back to the default,
        // which for a location means quietly publishing something private.
        for field in Field::ALL {
            assert_eq!(Field::parse(field.as_str()), Some(field));
        }
        for visibility in [
            Visibility::Public,
            Visibility::Contacts,
            Visibility::Private,
        ] {
            assert_eq!(Visibility::parse(visibility.as_str()), Some(visibility));
        }
    }

    #[test]
    fn a_javascript_url_is_not_a_link() {
        // Stored XSS with a very long tail: it would run in the WebView, which
        // is where the IPC bridge lives.
        for hostile in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///C:/Windows/System32",
            "vbscript:msgbox(1)",
            "//evil.example.com",
            "evil.example.com",
        ] {
            assert!(
                check_link(&ProfileLink {
                    label: "Site".into(),
                    url: hostile.into(),
                })
                .is_err(),
                "`{hostile}` must be refused"
            );
        }
    }

    #[test]
    fn an_ordinary_link_is_accepted() {
        assert!(
            check_link(&ProfileLink {
                label: "Homepage".into(),
                url: "https://example.com/about".into(),
            })
            .is_ok()
        );
        // http as well as https: refusing it would silently drop links to
        // things on a LAN or a hidden service, and TLS is not this function's
        // decision to make.
        assert!(
            check_link(&ProfileLink {
                label: "Local".into(),
                url: "http://192.168.1.10:8080".into(),
            })
            .is_ok()
        );
    }

    #[test]
    fn a_link_needs_a_label() {
        assert!(
            check_link(&ProfileLink {
                label: "   ".into(),
                url: "https://example.com".into(),
            })
            .is_err()
        );
    }
}
