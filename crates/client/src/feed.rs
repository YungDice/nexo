//! The Home feed and profiles, client side (§6.2, §6.3).
//!
//! Deliberately separate from [`crate::transport::Transport`]. That trait is
//! the end-to-end encrypted path — key packages, envelopes, commits — and it is
//! a trait so the parts that matter can be tested against a fake without a
//! server. None of that applies here: a feed post is plaintext both ends, there
//! is no MLS state to keep, and nothing to get wrong that a fake would catch.
//! Bolting ten more methods onto `Transport` would make every implementation
//! stub out ten things it does not care about, and blur the one distinction the
//! whole codebase is organised around.
//!
//! **Nothing in this module is end-to-end encrypted.** §4.4 requires that to be
//! said in the UI, in plain language, where someone is about to post — see
//! `FeedNotice`. Saying it here too, because this is the file where somebody
//! would otherwise assume the rest of the app's guarantees carry over.

use serde::{Deserialize, Serialize};

use crate::transport::TransportError;

/// One post as the feed renders it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Post {
    /// Server-assigned. Also the paging cursor.
    pub id: i64,
    /// Who wrote it.
    pub author_id: i64,
    /// Their handle, for `@name` and for opening their profile.
    pub author_handle: String,
    /// Their display name.
    pub author_display_name: String,
    /// Their avatar's object key, if they have one.
    pub author_avatar_key: Option<String>,
    /// The text. Server-readable, like everything else here.
    pub body: String,
    /// Object keys in `nexo-media`, presigned individually to render.
    pub media_keys: Vec<String>,
    /// When the server accepted it, in milliseconds since the epoch.
    pub created_at_ms: i64,
    /// Every emoji used on it, with counts.
    pub reactions: Vec<ReactionCount>,
    /// Emoji this account has used, so the UI can show them pressed.
    pub my_reactions: Vec<String>,
    /// True when this account wrote it — the only one who may delete it.
    pub is_mine: bool,
    /// The headline, when there is one. Posts written before titles have none.
    #[serde(default)]
    pub title: Option<String>,
    /// `text`, `link` or `image`.
    #[serde(default = "text_kind")]
    pub kind: String,
    /// Where a link post points. `None` for the other kinds.
    #[serde(default)]
    pub link_url: Option<String>,
    /// Upvotes minus downvotes.
    #[serde(default)]
    pub score: i64,
    /// This account's own vote: `1`, `-1`, or `0`.
    #[serde(default)]
    pub my_vote: i16,
    /// How many comments the thread holds.
    #[serde(default)]
    pub comment_count: i64,
    /// Pinned to the top of its author's profile. Only ever set on a
    /// profile's own page; the global feed does not treat pinning as a signal.
    #[serde(default)]
    pub pinned: bool,
}

/// What a post is when a server predates post kinds.
fn text_kind() -> String {
    "text".to_string()
}

/// One comment, flat. The tree is rebuilt from `parent_id` by whoever draws it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Comment {
    /// Server-assigned.
    pub id: i64,
    /// Which post it is on.
    pub post_id: i64,
    /// The comment being replied to. `None` at the top level.
    pub parent_id: Option<i64>,
    /// Who wrote it.
    pub author_id: i64,
    /// Their handle.
    pub author_handle: String,
    /// Their display name.
    pub author_display_name: String,
    /// Their avatar's object key, if any.
    pub author_avatar_key: Option<String>,
    /// Empty when deleted.
    pub body: String,
    /// When the server accepted it, in milliseconds since the epoch.
    pub created_at_ms: i64,
    /// True when this account wrote it.
    pub is_mine: bool,
    /// Deleted comments keep their place so their replies keep theirs.
    pub deleted: bool,
}

/// What a post is being written as.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NewPost {
    /// The headline. Optional.
    pub title: Option<String>,
    /// `text`, `link` or `image`.
    pub kind: String,
    /// Required for a link post.
    pub link_url: Option<String>,
    /// The text.
    pub body: String,
    /// Object keys already uploaded.
    pub media_keys: Vec<String>,
}

/// The score after a vote.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct VoteResult {
    /// Upvotes minus downvotes.
    pub score: i64,
    /// What this account's vote now is.
    pub my_vote: i16,
}

/// One emoji and how many people used it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReactionCount {
    /// The emoji itself.
    pub emoji: String,
    /// How many people used it.
    pub count: i64,
}

/// A page of the feed.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FeedPage {
    /// Newest first.
    pub posts: Vec<Post>,
    /// Pass back as `before`. `None` means the end.
    pub next_cursor: Option<i64>,
}

/// A link on a profile.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProfileLink {
    /// What to show.
    pub label: String,
    /// Always `http://` or `https://` — the server refuses anything else, and
    /// the UI opens it in the system browser rather than the WebView.
    pub url: String,
}

/// A profile, as far as this viewer is allowed to see it.
///
/// A field the viewer may not see is `None`, not an empty string. The
/// difference is what lets the UI say "not shared" instead of drawing a blank
/// line that reads as "they wrote nothing".
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Profile {
    /// The in-app numeric id (§3: never a phone number).
    pub user_id: i64,
    /// The unique handle.
    pub handle: String,
    /// What to call them.
    pub display_name: String,
    /// Object key for the avatar.
    pub avatar_key: Option<String>,
    /// Object key for the 3:1 banner.
    pub banner_key: Option<String>,
    /// `None` when hidden from this viewer, not when empty.
    pub bio: Option<String>,
    /// Free text. `None` when hidden — which is the default.
    pub location: Option<String>,
    /// `None` when hidden.
    pub links: Option<Vec<ProfileLink>>,
    /// Milliseconds since the epoch. `None` when hidden.
    pub join_date_ms: Option<i64>,
    /// True when this is the signed-in account's own profile.
    pub is_me: bool,
}

/// Your own profile, plus the visibility settings only you can see.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MyProfile {
    /// The profile itself, with nothing hidden.
    #[serde(flatten)]
    pub profile: Profile,
    /// Every settable field, with the value actually in force.
    pub visibility: std::collections::BTreeMap<String, String>,
}

/// What to change about your own profile. Absent fields are left alone.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProfileEdit {
    /// 1 to 40 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Up to 280 characters (§6.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,
    /// Free text. Never a geolocation API (§6.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
    /// Replaces the whole list when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<ProfileLink>>,
    /// A key already uploaded to `nexo-media`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_key: Option<String>,
    /// Same, for the banner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner_key: Option<String>,
}

/// One account you are blocking, as the settings list shows it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Block {
    /// Their handle, which is how you unblock them again.
    pub handle: String,
    /// What they call themselves, so the list is readable.
    pub display_name: String,
    /// When the block was made. The list is newest first.
    pub blocked_at_ms: i64,
}

/// The feed and profile API.
///
/// A trait for the same reason `Transport` is one — so a caller depends on the
/// shape rather than on `ureq` — but with none of the crypto, and no fake in
/// the tree because there is nothing here a fake would prove.
pub trait FeedApi {
    /// A page of the global feed.
    ///
    /// `sort` is `new` (default), `top` or `hot`. `before` is a post id under
    /// `new` and a row offset under the others — those orders are not monotonic
    /// in id, so an id cannot express a page boundary in them. Pass back
    /// whatever `next_cursor` returned and it is right either way.
    fn feed(
        &self,
        before: Option<i64>,
        limit: Option<i64>,
        sort: Option<&str>,
    ) -> Result<FeedPage, TransportError>;

    /// One person's posts.
    fn posts_by(
        &self,
        handle: &str,
        before: Option<i64>,
        limit: Option<i64>,
    ) -> Result<FeedPage, TransportError>;

    /// Writes a post.
    fn create_post(&self, new: &NewPost) -> Result<Post, TransportError>;

    /// Deletes one of your own.
    fn delete_post(&self, id: i64) -> Result<(), TransportError>;

    /// Adds or removes one reaction. Returns the new counts.
    fn react(&self, id: i64, emoji: &str, on: bool) -> Result<Vec<ReactionCount>, TransportError>;

    /// Votes on a post. `1`, `-1`, or `0` to take a vote back.
    fn vote(&self, id: i64, value: i16) -> Result<VoteResult, TransportError>;

    /// The whole comment thread for a post, oldest first.
    fn comments(&self, post_id: i64) -> Result<Vec<Comment>, TransportError>;

    /// Adds a comment, or a reply when `parent_id` is set.
    fn add_comment(
        &self,
        post_id: i64,
        body: &str,
        parent_id: Option<i64>,
    ) -> Result<Comment, TransportError>;

    /// Deletes one of your own comments.
    fn delete_comment(&self, id: i64) -> Result<(), TransportError>;

    /// Somebody's public profile.
    fn profile(&self, handle: &str) -> Result<Profile, TransportError>;

    /// Your own, with nothing hidden.
    fn my_profile(&self) -> Result<MyProfile, TransportError>;

    /// Edits your own.
    fn update_profile(&self, edit: &ProfileEdit) -> Result<MyProfile, TransportError>;

    /// Sets who may see which fields (G2).
    fn update_visibility(
        &self,
        visibility: &std::collections::BTreeMap<String, String>,
    ) -> Result<MyProfile, TransportError>;

    /// Pins one of your own posts to the top of your profile.
    fn pin_post(&self, id: i64) -> Result<(), TransportError>;

    /// Unpins one of your own. Idempotent.
    fn unpin_post(&self, id: i64) -> Result<(), TransportError>;

    /// Everyone you are blocking.
    ///
    /// Only your own list. There is deliberately no way to ask who is blocking
    /// *you* -- see `blocks.rs` on the server for why.
    fn blocks(&self) -> Result<Vec<Block>, TransportError>;

    /// Blocks somebody. Idempotent.
    fn block(&self, handle: &str) -> Result<(), TransportError>;

    /// Unblocks somebody. Idempotent.
    fn unblock(&self, handle: &str) -> Result<(), TransportError>;

    /// A time-limited URL to PUT a feed or profile image to, and its key.
    fn media_upload_url(&self, size: u64) -> Result<(String, String), TransportError>;

    /// A time-limited URL to GET one from.
    fn media_download_url(&self, key: &str) -> Result<String, TransportError>;
}
