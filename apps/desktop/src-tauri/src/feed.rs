//! Feed and profile commands (§6.2, §6.3, G2).
//!
//! **Nothing here is end-to-end encrypted**, and unlike every other command
//! module that is not a caveat but the subject. §4.4 requires the product to
//! say so in plain language rather than let the app's overall reputation for
//! encryption quietly cover a feed it cannot cover; `FeedNotice` says it in the
//! composer, and `PrivacyTable` says it on the Security tab.
//!
//! What still applies from rule 2 is that images are moved by Rust, not the
//! WebView. `pick_and_upload_image` takes a **path** and returns a key: the
//! bytes are read and PUT here, so a 4 MB banner never crosses the IPC bridge
//! and the WebView never holds a presigned URL it could send somewhere else.

use nexo_client::Transport;
use nexo_client::feed::{Comment, FeedApi, NewPost, ProfileEdit, ProfileLink, VoteResult};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::client::ClientState;

/// Feed commands.
pub use nexo_client::feed::{FeedPage, MyProfile, Post, Profile, ReactionCount};

/// An error the UI can act on.
#[derive(Debug, Serialize)]
pub struct FeedErrorView {
    pub kind: &'static str,
    pub message: String,
}

fn failure(kind: &'static str, message: impl Into<String>) -> FeedErrorView {
    FeedErrorView {
        kind,
        message: message.into(),
    }
}

impl From<nexo_client::transport::TransportError> for FeedErrorView {
    fn from(error: nexo_client::transport::TransportError) -> Self {
        use nexo_client::transport::TransportError;
        // The detail goes to the log, the summary to the user. A transport
        // error can carry a URL with a query string in it.
        tracing::warn!(%error, "feed call failed");
        match error {
            TransportError::Unreachable(_) => {
                failure("unreachable", "Can't reach the server. Try again shortly.")
            }
            TransportError::InvalidCredentials => {
                failure("signed_out", "Your session expired. Sign in again.")
            }
            // The server's refusals here are written for people -- "A post is
            // up to 2000 characters." -- so passing them through is more useful
            // than replacing them with something generic.
            TransportError::Rejected(detail) => failure("rejected", detail),
            _ => failure("internal", "Something went wrong. Try again."),
        }
    }
}

/// Runs work against the signed-in client on a blocking thread.
///
/// Same shape as `conversations::with_client`, and separate for the same reason
/// the modules are separate: this one never touches MLS state, and sharing a
/// helper would invite sharing more than the helper.
async fn with_client<T, F>(state: &ClientState, work: F) -> Result<T, FeedErrorView>
where
    T: Send + 'static,
    F: FnOnce(&crate::client::LoggedIn) -> Result<T, FeedErrorView> + Send + 'static,
{
    let handle = state.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let guard = handle.lock().map_err(|_| {
            failure(
                "internal",
                "The client state was poisoned by an earlier failure. Restart the app.",
            )
        })?;
        let client = guard
            .as_ref()
            .ok_or_else(|| failure("signed_out", "You are not signed in."))?;
        let outcome = work(client);

        // An access token ages on the clock, so the transport may have traded
        // the refresh token for a new pair mid-call. Writing the new one down
        // is not optional: the next launch replays whatever is stored, and a
        // spent refresh token is what the server reads as theft -- it revokes
        // every session for the account.
        if let Some(rotated) = client.transport.take_rotated_refresh_token()
            && let Err(error) = client.store.set_refresh_token(&rotated)
        {
            tracing::error!(%error, "could not persist a rotated refresh token");
        }

        outcome
    })
    .await
    .map_err(|e| {
        tracing::error!(%e, "a feed task panicked");
        failure("internal", "Something went wrong. Try again.")
    })?
}

/// A page of the global feed, newest first.
#[tauri::command]
pub async fn feed(
    state: State<'_, ClientState>,
    before: Option<i64>,
    sort: Option<String>,
) -> Result<FeedPage, FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.feed(before, None, sort.as_deref())?)
    })
    .await
}

/// One person's posts, for the Posts tab.
#[tauri::command]
pub async fn posts_by(
    state: State<'_, ClientState>,
    handle: String,
    before: Option<i64>,
) -> Result<FeedPage, FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.posts_by(&handle, before, None)?)
    })
    .await
}

/// Writes a post.
#[tauri::command]
pub async fn create_post(
    state: State<'_, ClientState>,
    body: String,
    media_keys: Vec<String>,
    title: Option<String>,
    kind: Option<String>,
    link_url: Option<String>,
) -> Result<Post, FeedErrorView> {
    with_client(&state, move |client| {
        let trimmed = body.trim();
        let kind = kind.unwrap_or_else(|| "text".to_string());
        if !matches!(kind.as_str(), "text" | "link" | "image") {
            return Err(failure(
                "invalid_request",
                "A post is text, a link, or an image.",
            ));
        }

        let title = title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        if title.as_deref().is_some_and(|t| t.chars().count() > 300) {
            return Err(failure(
                "invalid_request",
                "A title is up to 300 characters.",
            ));
        }

        let link_url = link_url
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())
            .map(str::to_string);

        // The same checks the server makes, so the sentence arrives before the
        // round trip rather than after it.
        match kind.as_str() {
            "link" => {
                let Some(url) = link_url.as_deref() else {
                    return Err(failure("invalid_request", "A link post needs a link."));
                };
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return Err(failure(
                        "invalid_request",
                        "A link must start with http:// or https://.",
                    ));
                }
            }
            "image" if media_keys.is_empty() => {
                return Err(failure("invalid_request", "An image post needs an image."));
            }
            _ => {}
        }

        if kind == "text" && trimmed.is_empty() && media_keys.is_empty() && title.is_none() {
            return Err(failure("invalid_request", "Write something first."));
        }
        // Characters, not bytes, and the same limit the server applies -- so
        // the message arrives before the round trip rather than after it.
        if trimmed.chars().count() > 2000 {
            return Err(failure(
                "invalid_request",
                "A post is up to 2000 characters.",
            ));
        }
        if media_keys.len() > 4 {
            return Err(failure("invalid_request", "Up to 4 images."));
        }
        Ok(client.transport.create_post(&NewPost {
            title,
            kind,
            link_url,
            body: trimmed.to_string(),
            media_keys,
        })?)
    })
    .await
}

/// Deletes one of your own posts.
#[tauri::command]
pub async fn delete_post(state: State<'_, ClientState>, id: i64) -> Result<(), FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.delete_post(id)?)
    })
    .await
}

/// Votes on a post. `1`, `-1`, or `0` to take a vote back.
#[tauri::command]
pub async fn vote(
    state: State<'_, ClientState>,
    id: i64,
    value: i16,
) -> Result<VoteResult, FeedErrorView> {
    with_client(&state, move |client| {
        if !matches!(value, -1..=1) {
            return Err(failure("invalid_request", "A vote is up, down, or none."));
        }
        Ok(client.transport.vote(id, value)?)
    })
    .await
}

/// The whole comment thread for a post, oldest first.
///
/// Flat on the wire; the tree is rebuilt from `parent_id` where it is drawn.
#[tauri::command]
pub async fn comments(
    state: State<'_, ClientState>,
    post_id: i64,
) -> Result<Vec<Comment>, FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.comments(post_id)?)
    })
    .await
}

/// Adds a comment, or a reply when `parent_id` is set.
#[tauri::command]
pub async fn add_comment(
    state: State<'_, ClientState>,
    post_id: i64,
    body: String,
    parent_id: Option<i64>,
) -> Result<Comment, FeedErrorView> {
    with_client(&state, move |client| {
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return Err(failure("invalid_request", "Write something first."));
        }
        if trimmed.chars().count() > 2000 {
            return Err(failure(
                "invalid_request",
                "A comment is up to 2000 characters.",
            ));
        }
        Ok(client.transport.add_comment(post_id, trimmed, parent_id)?)
    })
    .await
}

/// Deletes one of your own comments.
#[tauri::command]
pub async fn delete_comment(state: State<'_, ClientState>, id: i64) -> Result<(), FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.delete_comment(id)?)
    })
    .await
}

/// Adds or removes one reaction.
#[tauri::command]
pub async fn react(
    state: State<'_, ClientState>,
    id: i64,
    emoji: String,
    on: bool,
) -> Result<Vec<ReactionCount>, FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.react(id, &emoji, on)?)
    })
    .await
}

/// Pins one of your own posts to the top of your profile.
///
/// Three at a time, checked on the server: a cap enforced only in the UI is a
/// cap a second client does not have.
#[tauri::command]
pub async fn pin_post(state: State<'_, ClientState>, id: i64) -> Result<(), FeedErrorView> {
    with_client(&state, move |client| Ok(client.transport.pin_post(id)?)).await
}

/// Unpins one of your own posts.
#[tauri::command]
pub async fn unpin_post(state: State<'_, ClientState>, id: i64) -> Result<(), FeedErrorView> {
    with_client(&state, move |client| Ok(client.transport.unpin_post(id)?)).await
}

/// Everyone this account is blocking.
#[tauri::command]
pub async fn blocks(
    state: State<'_, ClientState>,
) -> Result<Vec<nexo_client::feed::Block>, FeedErrorView> {
    with_client(&state, |client| Ok(client.transport.blocks()?)).await
}

/// Blocks somebody.
///
/// The effects are the server's -- their posts leave the feed, and neither of
/// you can open a conversation with the other. Nothing here hides anything
/// locally, because a block the client applied would be a promise the product
/// cannot keep.
#[tauri::command]
pub async fn block(state: State<'_, ClientState>, handle: String) -> Result<(), FeedErrorView> {
    with_client(&state, move |client| Ok(client.transport.block(&handle)?)).await
}

/// Unblocks somebody.
#[tauri::command]
pub async fn unblock(state: State<'_, ClientState>, handle: String) -> Result<(), FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.unblock(&handle)?)
    })
    .await
}

/// Somebody's public profile.
#[tauri::command]
pub async fn profile(
    state: State<'_, ClientState>,
    handle: String,
) -> Result<Profile, FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.profile(&handle)?)
    })
    .await
}

/// Your own profile, with nothing hidden.
#[tauri::command]
pub async fn my_profile(state: State<'_, ClientState>) -> Result<MyProfile, FeedErrorView> {
    with_client(&state, |client| Ok(client.transport.my_profile()?)).await
}

/// What the Edit Profile form sends.
#[derive(Debug, Deserialize)]
pub struct ProfileEditRequest {
    pub display_name: Option<String>,
    pub bio: Option<String>,
    pub location: Option<String>,
    pub links: Option<Vec<LinkInput>>,
    pub avatar_key: Option<String>,
    pub banner_key: Option<String>,
}

/// One link from the form.
#[derive(Debug, Deserialize)]
pub struct LinkInput {
    pub label: String,
    pub url: String,
}

/// Edits your own profile.
#[tauri::command]
pub async fn update_profile(
    state: State<'_, ClientState>,
    edit: ProfileEditRequest,
) -> Result<MyProfile, FeedErrorView> {
    with_client(&state, move |client| {
        if let Some(links) = &edit.links {
            for link in links {
                // Checked here as well as at the server and in the column. A
                // `javascript:` URL that reached a profile page would run in
                // the WebView, which is where the IPC bridge lives -- so the
                // check belongs at every layer that could be the last one.
                let lower = link.url.trim().to_ascii_lowercase();
                if !(lower.starts_with("http://") || lower.starts_with("https://")) {
                    return Err(failure(
                        "invalid_request",
                        "Links must start with http:// or https://.",
                    ));
                }
            }
        }

        let edit = ProfileEdit {
            display_name: edit.display_name,
            bio: edit.bio,
            location: edit.location,
            links: edit.links.map(|links| {
                links
                    .into_iter()
                    .map(|l| ProfileLink {
                        label: l.label,
                        url: l.url,
                    })
                    .collect()
            }),
            avatar_key: edit.avatar_key,
            banner_key: edit.banner_key,
        };
        Ok(client.transport.update_profile(&edit)?)
    })
    .await
}

/// Sets who may see which profile fields (G2).
#[tauri::command]
pub async fn update_visibility(
    state: State<'_, ClientState>,
    visibility: std::collections::BTreeMap<String, String>,
) -> Result<MyProfile, FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.update_visibility(&visibility)?)
    })
    .await
}

/// The largest image this app will upload.
///
/// §6.3 puts a banner at 4 MB. The same ceiling for every image: an avatar
/// larger than a banner is a mistake rather than a use case.
const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;

/// Uploads an image the user picked, and returns its object key.
///
/// The **path** crosses the bridge, never the bytes and never the presigned
/// URL. A presigned PUT is a bearer credential for one object; handing it to
/// the WebView would put a writable URL somewhere a script could reach it, for
/// no benefit — the bytes are on disk, where Rust can read them.
#[tauri::command]
pub async fn upload_image(
    state: State<'_, ClientState>,
    path: String,
) -> Result<String, FeedErrorView> {
    with_client(&state, move |client| {
        let path = std::path::PathBuf::from(&path);
        let bytes = std::fs::read(&path).map_err(|e| {
            failure(
                "unreadable_file",
                format!("That image could not be read: {e}"),
            )
        })?;
        if bytes.is_empty() {
            return Err(failure("invalid_request", "That file is empty."));
        }
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(failure(
                "too_large",
                format!(
                    "Images are limited to {} MB.",
                    MAX_IMAGE_BYTES / (1024 * 1024)
                ),
            ));
        }
        // The bytes are checked against their own magic number rather than
        // trusted from the extension: this object is served to other people's
        // browsers, and an HTML file named `.png` is a stored XSS on whatever
        // origin serves it.
        if !looks_like_an_image(&bytes) {
            return Err(failure(
                "invalid_request",
                "That doesn't look like a PNG, JPEG, GIF, or WebP.",
            ));
        }

        let (url, key) = client.transport.media_upload_url(bytes.len() as u64)?;
        use nexo_client::transport::Transport as _;
        client.transport.put_object(&url, bytes)?;
        Ok(key)
    })
    .await
}

/// A time-limited URL for rendering a feed or profile image.
///
/// Kept for callers outside the WebView. The page itself cannot use this: the
/// CSP allows no remote image host, so use [`image_data_url`].
#[tauri::command]
pub async fn image_url(
    state: State<'_, ClientState>,
    key: String,
) -> Result<String, FeedErrorView> {
    with_client(&state, move |client| {
        Ok(client.transport.media_download_url(&key)?)
    })
    .await
}

/// The largest image this app will inline into the page.
///
/// A `data:` URL is base64, so it costs a third more than the bytes and lives
/// in the WebView's memory as a string. Feed and profile images are small; a
/// ceiling keeps a pathological one from wedging the renderer.
pub(crate) const MAX_INLINE_IMAGE_BYTES: usize = 12 * 1024 * 1024;

/// A feed or profile image, as a `data:` URL the page can actually render.
///
/// The CSP is `img-src 'self' asset: data: blob:` — deliberately no remote
/// host, so a presigned object-storage URL is blocked before a byte is
/// fetched. Rust downloads it instead and hands over the bytes inline, which
/// is the only route the policy leaves open and keeps the bucket unreachable
/// from anything that ever runs script in the page.
#[tauri::command]
pub async fn image_data_url(
    state: State<'_, ClientState>,
    key: String,
) -> Result<String, FeedErrorView> {
    with_client(&state, move |client| {
        let url = client.transport.media_download_url(&key)?;
        let bytes = client.transport.get_object(&url)?;

        if bytes.len() > MAX_INLINE_IMAGE_BYTES {
            return Err(FeedErrorView {
                kind: "too_large",
                message: "That image is too large to display.".into(),
            });
        }
        // The same check the upload path makes. A bucket object is not
        // trusted just because we asked for it: an object that is really HTML
        // or SVG must never reach the page as an image.
        if !looks_like_an_image(&bytes) {
            return Err(FeedErrorView {
                kind: "not_an_image",
                message: "That file is not an image.".into(),
            });
        }

        Ok(data_url(sniff_mime(&bytes), &bytes))
    })
    .await
}

/// A `data:` URL from bytes already known to be an image.
pub(crate) fn data_url(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine as _;
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

/// The content type from the bytes themselves, never from a supplied name.
pub(crate) fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        "image/png"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
        // ISO base media: MP4 and everything wearing its box structure. The
        // brand at 8..12 distinguishes them, and the WebView plays what it
        // plays either way, so the container is as specific as this needs.
        "video/mp4"
    } else if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        // EBML, which in practice means WebM or Matroska.
        "video/webm"
    } else {
        "application/octet-stream"
    }
}

/// Whether these bytes are something the page may be handed to render.
///
/// The sender's declared type is not evidence -- this is decided from the bytes
/// -- and anything that is not a picture or a video is refused rather than
/// inlined. An HTML file arriving as an "image" is the case this exists for.
pub(crate) fn is_renderable(mime: &str) -> bool {
    mime.starts_with("image/") || mime.starts_with("video/")
}

/// Whether these bytes begin like an image format the app accepts.
///
/// Not a full parse — it cannot prove the file is well-formed, and it is not
/// trying to. It refuses the specific thing that matters: a file that is
/// actually HTML, SVG, or a script, uploaded under an image's name, and later
/// served to somebody's browser.
fn looks_like_an_image(bytes: &[u8]) -> bool {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    const GIF87: &[u8] = b"GIF87a";
    const GIF89: &[u8] = b"GIF89a";

    if bytes.starts_with(PNG) || bytes.starts_with(GIF87) || bytes.starts_with(GIF89) {
        return true;
    }
    // JPEG: SOI marker.
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return true;
    }
    // WebP is RIFF with a WEBP fourcc four bytes later.
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_image_headers_are_accepted() {
        assert!(looks_like_an_image(&[
            0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0
        ]));
        assert!(looks_like_an_image(b"GIF89a...."));
        assert!(looks_like_an_image(b"GIF87a...."));
        assert!(looks_like_an_image(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]));
        assert!(looks_like_an_image(b"RIFF\0\0\0\0WEBPVP8 "));
    }

    #[test]
    fn markup_wearing_an_image_name_is_refused() {
        // The reason this check exists. These are served to other people's
        // browsers, and an HTML or SVG file named `.png` is stored XSS.
        for hostile in [
            &b"<html><script>alert(1)</script>"[..],
            &b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script/></svg>"[..],
            &b"<!DOCTYPE html>"[..],
            &b"MZ\x90\x00"[..], // a Windows executable
            &b"#!/bin/sh"[..],
            &b""[..],
            &b"RIFF\0\0\0\0AVI "[..], // RIFF, but not WebP
        ] {
            assert!(
                !looks_like_an_image(hostile),
                "should be refused: {:?}",
                String::from_utf8_lossy(&hostile[..hostile.len().min(20)])
            );
        }
    }

    #[test]
    fn a_truncated_header_does_not_panic() {
        // Every prefix of every accepted header, to prove no slice indexes
        // past the end.
        for header in [
            &b"\x89PNG\r\n\x1a\n"[..],
            &b"RIFF\0\0\0\0WEBP"[..],
            &b"GIF89a"[..],
            &b"\xFF\xD8\xFF"[..],
        ] {
            for n in 0..header.len() {
                let _ = looks_like_an_image(&header[..n]);
            }
        }
    }

    #[test]
    fn the_image_ceiling_matches_the_brief() {
        // §6.3: a banner is at most 4 MB.
        const { assert!(MAX_IMAGE_BYTES == 4 * 1024 * 1024) };
    }
}
