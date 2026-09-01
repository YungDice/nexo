//! The Home feed (§4.4, §6.2).
//!
//! **Nothing here is end-to-end encrypted, and the UI says so.** A feed is
//! content written to be read by strangers; there is no closed group to encrypt
//! it to, so encrypting it would be theatre. §4.4 requires this to be stated in
//! plain language rather than glossed over, and `FeedNotice` states it above
//! the composer where someone is about to post.
//!
//! The one thing worth being careful about is the boundary. This module and
//! `delivery` sit in the same process and share a database, and the whole
//! product rests on them never mixing: `envelopes` holds ciphertext the server
//! cannot read, `posts` holds text it can. No code path moves a row between
//! them, and no post ever references a conversation.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::delete, routing::get, routing::post};
use serde::{Deserialize, Serialize};

use crate::auth::bearer::Caller;
use crate::state::AppState;

/// How many posts one page of the feed holds.
///
/// The client asks for infinite scroll (§6.2), which means many small pages
/// rather than one large one — a page big enough to feel instant and small
/// enough that a slow connection sees something quickly.
const PAGE_SIZE: i64 = 20;
/// The largest page a caller may ask for.
const MAX_PAGE_SIZE: i64 = 50;

/// Feed routes.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/v1/feed", get(feed))
        .route("/v1/posts", post(create_post))
        .route("/v1/posts/{id}", delete(delete_post))
        .route("/v1/posts/{id}/pin", post(pin).delete(unpin))
        .route("/v1/posts/{id}/react", post(react))
        .route("/v1/posts/{id}/vote", post(vote))
        .route(
            "/v1/posts/{id}/comments",
            get(list_comments).post(create_comment),
        )
        .route("/v1/comments/{id}", delete(delete_comment))
        .route("/v1/users/{handle}/posts", get(posts_by))
}

/// Why a feed request was refused.
#[derive(Debug)]
pub enum PostError {
    /// No such post, or not yours.
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

impl IntoResponse for PostError {
    fn into_response(self) -> Response {
        let (status, error, message) = match self {
            PostError::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "That post is gone.".to_string(),
            ),
            PostError::Invalid(message) => (StatusCode::BAD_REQUEST, "invalid_request", message),
            PostError::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "Too many requests. Slow down.".to_string(),
            ),
            PostError::Internal(error) => {
                tracing::error!(%error, "feed request failed");
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

impl<E: Into<anyhow::Error>> From<E> for PostError {
    fn from(error: E) -> Self {
        PostError::Internal(error.into())
    }
}

/// One post, as the feed renders it.
#[derive(Debug, Clone, Serialize)]
pub struct PostView {
    pub id: i64,
    pub author_id: i64,
    pub author_handle: String,
    pub author_display_name: String,
    pub author_avatar_key: Option<String>,
    pub body: String,
    /// Object keys in `nexo-media`. The client presigns each to render it.
    pub media_keys: Vec<String>,
    /// Milliseconds since the epoch.
    ///
    /// Not RFC 3339: every other timestamp this API returns is epoch millis
    /// (`sent_at_ms`, `server_timestamp_ms`), and a second format would be one
    /// more thing for a client to get wrong for no gain.
    pub created_at_ms: i64,
    /// Emoji to count, ordered by count then emoji so the row is stable.
    pub reactions: Vec<ReactionCount>,
    /// Which emoji the caller has used, so the UI can show them as pressed.
    pub my_reactions: Vec<String>,
    /// True when the caller wrote it — the only one who may delete it.
    pub is_mine: bool,
    /// The headline, when there is one. Older posts have none.
    pub title: Option<String>,
    /// `text`, `link` or `image`.
    pub kind: String,
    /// Pinned to the top of its author's profile.
    ///
    /// Only ever true in a profile's own page: the global feed does not treat
    /// pinning as a signal, because a pin is a statement about a profile and
    /// not a claim on everybody else's reading.
    #[serde(default)]
    pub pinned: bool,
    /// Where a link post points. Always `None` for the other kinds.
    pub link_url: Option<String>,
    /// Upvotes minus downvotes.
    pub score: i64,
    /// The caller's own vote: `1`, `-1`, or `0` for none.
    pub my_vote: i16,
    /// How many comments the thread holds, deleted ones excluded.
    pub comment_count: i64,
}

/// One emoji and how many people used it.
#[derive(Debug, Clone, Serialize)]
pub struct ReactionCount {
    pub emoji: String,
    pub count: i64,
}

/// A page of the feed, with the cursor for the next one.
#[derive(Debug, Serialize)]
pub struct FeedPage {
    pub posts: Vec<PostView>,
    /// Pass as `before` to get the next page. `None` at the end.
    ///
    /// A post id rather than a timestamp or an offset: ids are unique and
    /// monotonic, so a page boundary cannot land mid-second, and a post created
    /// while someone is scrolling cannot shift every later page by one.
    pub next_cursor: Option<i64>,
}

#[derive(Deserialize)]
pub struct FeedQuery {
    /// Return posts older than this id.
    ///
    /// A post id under `new`, where the feed is strictly reverse-chronological.
    /// Under `top` and `hot` it is how many rows the caller has already seen:
    /// those orders are not monotonic in id, so an id cursor cannot express a
    /// page boundary in them. Either way it is whatever `next_cursor` returned.
    pub before: Option<i64>,
    pub limit: Option<i64>,
    /// `new` (default), `top`, or `hot`.
    pub sort: Option<String>,
}

/// How a feed page is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    /// Newest first. The only order with a stable id cursor.
    New,
    /// Highest score first, ties broken by recency.
    Top,
    /// Score decayed by age, so a good post from today outranks a better one
    /// from last week. The shape is Reddit's: the log of the score plus a
    /// twelve-and-a-half-hour time term, which makes each order of magnitude
    /// of votes worth about that much age.
    Hot,
}

impl Sort {
    fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some("top") => Self::Top,
            Some("hot") => Self::Hot,
            _ => Self::New,
        }
    }
}

async fn feed(
    State(state): State<AppState>,
    caller: Caller,
    Query(query): Query<FeedQuery>,
) -> Result<Json<FeedPage>, PostError> {
    let limit = query.limit.unwrap_or(PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let sort = Sort::parse(query.sort.as_deref());
    // i64::MAX rather than a branch: under `new`, "no cursor" is "older than
    // the largest possible id", which is the same query.
    let before = query.before.unwrap_or(i64::MAX);
    // Under `top` and `hot` the same field is an offset, and starts at zero.
    let offset = query.before.unwrap_or(0).max(0);

    // Who this caller must not see: people they blocked, and people who
    // blocked them. Fetched once and passed into whichever query runs, rather
    // than joined inside each of the three -- the same list also filters a
    // profile's posts, and a join would have to be written four times and kept
    // in step four times.
    let hidden = crate::blocks::hidden_authors(&state.db, caller.user_id).await?;

    // One query per order rather than SQL assembled at runtime: `query_as!`
    // checks the statement against the database at compile time, and it can
    // only do that for a statement that exists in the source.
    let rows = match sort {
        Sort::New => {
            sqlx::query_as!(
                FeedRow,
                "SELECT p.id, p.author_id, p.body, p.media_keys, p.title, p.kind, p.link_url,
                    (EXTRACT(EPOCH FROM p.created_at) * 1000)::BIGINT AS \"created_at_ms!\",
                    u.handle::TEXT AS \"handle!\", u.display_name, u.avatar_key,
                    COALESCE(v.score, 0) AS \"score!\",
                    COALESCE(mv.value, 0::SMALLINT) AS \"my_vote!\",
                    COALESCE(c.total, 0) AS \"comment_count!\"
             FROM posts p
             JOIN users u ON u.id = p.author_id
             LEFT JOIN (SELECT post_id, SUM(value)::BIGINT AS score
                        FROM post_votes GROUP BY post_id) v ON v.post_id = p.id
             LEFT JOIN post_votes mv ON mv.post_id = p.id AND mv.user_id = $3
             LEFT JOIN (SELECT post_id, COUNT(*)::BIGINT AS total
                        FROM post_comments WHERE deleted_at IS NULL
                        GROUP BY post_id) c ON c.post_id = p.id
             WHERE p.deleted_at IS NULL AND p.id < $1
               AND NOT (p.author_id = ANY($4))
             ORDER BY p.id DESC
             LIMIT $2",
                before,
                limit,
                caller.user_id,
                &hidden
            )
            .fetch_all(&state.db)
            .await?
        }

        Sort::Top => {
            sqlx::query_as!(
                FeedRow,
                "SELECT p.id, p.author_id, p.body, p.media_keys, p.title, p.kind, p.link_url,
                    (EXTRACT(EPOCH FROM p.created_at) * 1000)::BIGINT AS \"created_at_ms!\",
                    u.handle::TEXT AS \"handle!\", u.display_name, u.avatar_key,
                    COALESCE(v.score, 0) AS \"score!\",
                    COALESCE(mv.value, 0::SMALLINT) AS \"my_vote!\",
                    COALESCE(c.total, 0) AS \"comment_count!\"
             FROM posts p
             JOIN users u ON u.id = p.author_id
             LEFT JOIN (SELECT post_id, SUM(value)::BIGINT AS score
                        FROM post_votes GROUP BY post_id) v ON v.post_id = p.id
             LEFT JOIN post_votes mv ON mv.post_id = p.id AND mv.user_id = $3
             LEFT JOIN (SELECT post_id, COUNT(*)::BIGINT AS total
                        FROM post_comments WHERE deleted_at IS NULL
                        GROUP BY post_id) c ON c.post_id = p.id
             WHERE p.deleted_at IS NULL
               AND NOT (p.author_id = ANY($4))
             ORDER BY COALESCE(v.score, 0) DESC, p.id DESC
             LIMIT $2 OFFSET $1",
                offset,
                limit,
                caller.user_id,
                &hidden
            )
            .fetch_all(&state.db)
            .await?
        }

        Sort::Hot => {
            sqlx::query_as!(
                FeedRow,
                "SELECT p.id, p.author_id, p.body, p.media_keys, p.title, p.kind, p.link_url,
                    (EXTRACT(EPOCH FROM p.created_at) * 1000)::BIGINT AS \"created_at_ms!\",
                    u.handle::TEXT AS \"handle!\", u.display_name, u.avatar_key,
                    COALESCE(v.score, 0) AS \"score!\",
                    COALESCE(mv.value, 0::SMALLINT) AS \"my_vote!\",
                    COALESCE(c.total, 0) AS \"comment_count!\"
             FROM posts p
             JOIN users u ON u.id = p.author_id
             LEFT JOIN (SELECT post_id, SUM(value)::BIGINT AS score
                        FROM post_votes GROUP BY post_id) v ON v.post_id = p.id
             LEFT JOIN post_votes mv ON mv.post_id = p.id AND mv.user_id = $3
             LEFT JOIN (SELECT post_id, COUNT(*)::BIGINT AS total
                        FROM post_comments WHERE deleted_at IS NULL
                        GROUP BY post_id) c ON c.post_id = p.id
             WHERE p.deleted_at IS NULL
               AND NOT (p.author_id = ANY($4))
             ORDER BY
                 sign(COALESCE(v.score, 0))
                   * log(greatest(abs(COALESCE(v.score, 0)), 1))
                   + EXTRACT(EPOCH FROM p.created_at) / 45000.0 DESC,
                 p.id DESC
             LIMIT $2 OFFSET $1",
                offset,
                limit,
                caller.user_id,
                &hidden
            )
            .fetch_all(&state.db)
            .await?
        }
    };

    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let reactions = reactions_for(&state, &ids, caller.user_id).await?;
    let posts = to_views(rows, &reactions, caller.user_id);

    // Only a cursor when the page was full. A short page is the end of the
    // feed, and handing back a cursor there makes the client fetch an empty
    // page to discover that.
    let next_cursor = (posts.len() as i64 == limit).then(|| match sort {
        Sort::New => posts.last().map(|p| p.id).unwrap_or(before),
        // An offset, matching what `before` means under these orders.
        Sort::Top | Sort::Hot => offset + limit,
    });

    Ok(Json(FeedPage { posts, next_cursor }))
}

/// One feed row, as all three orders select it.
///
/// Named so `query_as!` can return the same shape from each without three
/// near-identical mapping closures drifting apart.
struct FeedRow {
    id: i64,
    author_id: i64,
    body: String,
    media_keys: Vec<String>,
    title: Option<String>,
    kind: String,
    link_url: Option<String>,
    created_at_ms: i64,
    handle: String,
    display_name: String,
    avatar_key: Option<String>,
    score: i64,
    my_vote: i16,
    comment_count: i64,
}

fn to_views(
    rows: Vec<FeedRow>,
    reactions: &std::collections::HashMap<i64, (Vec<ReactionCount>, Vec<String>)>,
    caller_id: i64,
) -> Vec<PostView> {
    rows.into_iter()
        .map(|row| {
            let (counts, mine) = reactions
                .get(&row.id)
                .cloned()
                .unwrap_or_else(|| (Vec::new(), Vec::new()));
            PostView {
                // Set by `posts_by` for the profile's own page; false is right
                // everywhere else, including the global feed.
                pinned: false,
                id: row.id,
                is_mine: row.author_id == caller_id,
                author_id: row.author_id,
                author_handle: row.handle,
                author_display_name: row.display_name,
                author_avatar_key: row.avatar_key,
                body: row.body,
                media_keys: row.media_keys,
                created_at_ms: row.created_at_ms,
                reactions: counts,
                my_reactions: mine,
                title: row.title,
                kind: row.kind,
                link_url: row.link_url,
                score: row.score,
                my_vote: row.my_vote,
                comment_count: row.comment_count,
            }
        })
        .collect()
}

/// One person's posts, for the Posts tab on a profile.
async fn posts_by(
    State(state): State<AppState>,
    caller: Caller,
    Path(handle): Path<String>,
    Query(query): Query<FeedQuery>,
) -> Result<Json<FeedPage>, PostError> {
    let limit = query.limit.unwrap_or(PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let before = query.before.unwrap_or(i64::MAX);
    // A blocked person's profile answers with an empty list rather than a
    // refusal. "No posts" is what someone who has nothing to show looks like,
    // and it is the same asymmetry the module doc explains: being blocked
    // should be indistinguishable from being uninteresting.
    let hidden = crate::blocks::hidden_authors(&state.db, caller.user_id).await?;

    let rows = sqlx::query_as!(
        FeedRow,
        "SELECT p.id, p.author_id, p.body, p.media_keys, p.title, p.kind, p.link_url,
                (EXTRACT(EPOCH FROM p.created_at) * 1000)::BIGINT AS \"created_at_ms!\",
                u.handle::TEXT AS \"handle!\", u.display_name, u.avatar_key,
                COALESCE(v.score, 0) AS \"score!\",
                COALESCE(mv.value, 0::SMALLINT) AS \"my_vote!\",
                COALESCE(c.total, 0) AS \"comment_count!\"
         FROM posts p
         JOIN users u ON u.id = p.author_id
         LEFT JOIN (SELECT post_id, SUM(value)::BIGINT AS score
                    FROM post_votes GROUP BY post_id) v ON v.post_id = p.id
         LEFT JOIN post_votes mv ON mv.post_id = p.id AND mv.user_id = $4
         LEFT JOIN (SELECT post_id, COUNT(*)::BIGINT AS total
                    FROM post_comments WHERE deleted_at IS NULL
                    GROUP BY post_id) c ON c.post_id = p.id
         WHERE p.deleted_at IS NULL AND p.id < $1 AND u.handle = $2::CITEXT
           AND NOT (p.author_id = ANY($5))
           AND p.pinned_at IS NULL
         ORDER BY p.id DESC
         LIMIT $3",
        before,
        handle,
        limit,
        caller.user_id,
        &hidden
    )
    .fetch_all(&state.db)
    .await?;

    // Pinned posts are excluded from the query above and prepended here, on
    // the first page only. Sorting them into it instead would break the
    // cursor: paging is `id < before`, and a pinned post with a low id would
    // appear on page one and then again wherever its id fell.
    //
    // At most three (`MAX_PINNED`), so this is a small query rather than a
    // second page of its own.
    let pinned = if query.before.is_none() {
        sqlx::query_as!(
            FeedRow,
            "SELECT p.id, p.author_id, p.body, p.media_keys, p.title, p.kind, p.link_url,
                    (EXTRACT(EPOCH FROM p.created_at) * 1000)::BIGINT AS \"created_at_ms!\",
                    u.handle::TEXT AS \"handle!\", u.display_name, u.avatar_key,
                    COALESCE(v.score, 0) AS \"score!\",
                    COALESCE(mv.value, 0::SMALLINT) AS \"my_vote!\",
                    COALESCE(c.total, 0) AS \"comment_count!\"
             FROM posts p
             JOIN users u ON u.id = p.author_id
             LEFT JOIN (SELECT post_id, SUM(value)::BIGINT AS score
                        FROM post_votes GROUP BY post_id) v ON v.post_id = p.id
             LEFT JOIN post_votes mv ON mv.post_id = p.id AND mv.user_id = $2
             LEFT JOIN (SELECT post_id, COUNT(*)::BIGINT AS total
                        FROM post_comments WHERE deleted_at IS NULL
                        GROUP BY post_id) c ON c.post_id = p.id
             WHERE p.deleted_at IS NULL AND u.handle = $1::CITEXT
               AND NOT (p.author_id = ANY($3))
               AND p.pinned_at IS NOT NULL
             ORDER BY p.pinned_at DESC",
            handle,
            caller.user_id,
            &hidden
        )
        .fetch_all(&state.db)
        .await?
    } else {
        Vec::new()
    };
    // The cursor is decided by the *paginated* rows alone, before the pinned
    // ones are mixed in. Counting them would end paging early on a profile with
    // three pinned posts, and taking `last()` from the combined list would hand
    // back a pinned post's id as the cursor whenever the page below was empty
    // -- so the next request would page from the wrong place, or from a post
    // that is deliberately never in the paged set at all.
    let next_cursor = (rows.len() as i64 == limit)
        .then(|| rows.last().map(|r| r.id))
        .flatten();

    let pinned_ids: Vec<i64> = pinned.iter().map(|r| r.id).collect();
    let rows: Vec<FeedRow> = pinned.into_iter().chain(rows).collect();

    let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
    let reactions = reactions_for(&state, &ids, caller.user_id).await?;
    let mut posts = to_views(rows, &reactions, caller.user_id);
    for post in &mut posts {
        post.pinned = pinned_ids.contains(&post.id);
    }

    Ok(Json(FeedPage { posts, next_cursor }))
}

#[derive(Deserialize)]
pub struct CreatePostRequest {
    pub body: String,
    /// Keys already uploaded to `nexo-media` via `/v1/media/upload`.
    #[serde(default)]
    pub media_keys: Vec<String>,
    /// The headline. Optional, so a client that predates titles still posts.
    #[serde(default)]
    pub title: Option<String>,
    /// `text`, `link` or `image`. Defaults to `text`.
    #[serde(default)]
    pub kind: Option<String>,
    /// Required for a link post, refused for the others.
    #[serde(default)]
    pub link_url: Option<String>,
}

/// The kinds a post may be.
///
/// Validated here rather than left to the CHECK constraint alone, so the client
/// gets a sentence it can show instead of a database error.
const POST_KINDS: [&str; 3] = ["text", "link", "image"];

async fn create_post(
    State(state): State<AppState>,
    caller: Caller,
    Json(request): Json<CreatePostRequest>,
) -> Result<Json<PostView>, PostError> {
    // A person writes at human speed; a loop does not. Bounded here rather
    // than by the feed query, because a post that exists has already cost
    // a row and a fan-out to everyone reading.
    if !state.limits.posts.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "posts rate limit reached");
        return Err(PostError::TooManyRequests);
    }

    let body = request.body.trim();
    let kind = request.kind.as_deref().unwrap_or("text");
    if !POST_KINDS.contains(&kind) {
        return Err(PostError::Invalid(
            "A post is text, a link, or an image.".into(),
        ));
    }

    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    if let Some(title) = &title
        && title.chars().count() > 300
    {
        return Err(PostError::Invalid(
            "A title is up to 300 characters.".into(),
        ));
    }

    let link_url = request
        .link_url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(str::to_string);

    match kind {
        // A link post is the link. Requiring it here as well as in the
        // constraint means the client is told which field is missing.
        "link" => {
            let Some(url) = &link_url else {
                return Err(PostError::Invalid("A link post needs a link.".into()));
            };
            // Scheme, not shape: `javascript:` in a feed is stored XSS, and
            // this is the check that keeps it out of the row.
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(PostError::Invalid(
                    "A link must start with http:// or https://.".into(),
                ));
            }
            if url.chars().count() > 2000 {
                return Err(PostError::Invalid("That link is too long.".into()));
            }
        }
        "image" => {
            if request.media_keys.is_empty() {
                return Err(PostError::Invalid("An image post needs an image.".into()));
            }
            if link_url.is_some() {
                return Err(PostError::Invalid("An image post has no link.".into()));
            }
        }
        _ => {
            if link_url.is_some() {
                return Err(PostError::Invalid("A text post has no link.".into()));
            }
        }
    }

    // A link or image post carries its own content, so an empty body is fine
    // there. A text post with nothing in it is not a post.
    if kind == "text" && body.is_empty() && request.media_keys.is_empty() && title.is_none() {
        return Err(PostError::Invalid("Write something first.".into()));
    }
    // Characters, not bytes: §6.2 says 2000 chars, and to the person typing
    // an emoji is one character.
    if body.chars().count() > 2000 {
        return Err(PostError::Invalid(
            "A post is up to 2000 characters.".into(),
        ));
    }
    if request.media_keys.len() > 4 {
        return Err(PostError::Invalid("Up to 4 images.".into()));
    }
    for key in &request.media_keys {
        // Attaching someone else's object would let a post point at media its
        // author never uploaded — including into the *encrypted* bucket's
        // namespace, which must never be referenced from a public row.
        if !key.starts_with(&format!("media/{}/", caller.user_id)) {
            return Err(PostError::Invalid("That image is not yours.".into()));
        }
    }

    let row = sqlx::query!(
        "INSERT INTO posts (author_id, body, media_keys, title, kind, link_url)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS \"created_at_ms!\"",
        caller.user_id,
        body,
        &request.media_keys,
        title.as_deref(),
        kind,
        link_url.as_deref()
    )
    .fetch_one(&state.db)
    .await?;

    let author = sqlx::query!(
        "SELECT handle::TEXT AS \"handle!\", display_name, avatar_key
         FROM users WHERE id = $1",
        caller.user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(PostView {
        pinned: false,
        id: row.id,
        author_id: caller.user_id,
        author_handle: author.handle,
        author_display_name: author.display_name,
        author_avatar_key: author.avatar_key,
        body: body.to_string(),
        media_keys: request.media_keys,
        created_at_ms: row.created_at_ms,
        reactions: Vec::new(),
        my_reactions: Vec::new(),
        is_mine: true,
        title,
        kind: kind.to_string(),
        link_url,
        // Brand new: nobody has voted or replied yet.
        score: 0,
        my_vote: 0,
        comment_count: 0,
    }))
}

/// How many posts one profile may pin.
///
/// Three, because the pinned block sits above everything else a visitor came
/// to read. A cap that let someone pin twenty would turn the top of a profile
/// into a second feed, and the point of pinning is that it says "these",
/// which stops being true at some number well below twenty.
const MAX_PINNED: i64 = 3;

/// Pins one of your own posts to the top of your profile.
async fn pin(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
) -> Result<StatusCode, PostError> {
    if !state.limits.reactions.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "reactions rate limit reached");
        return Err(PostError::TooManyRequests);
    }

    // Counted before the write, in the same transaction, so two pins racing
    // cannot both see two and both become the fourth.
    let mut tx = state.db.begin().await?;

    let already = sqlx::query!(
        "SELECT COUNT(*) AS \"count!\" FROM posts
         WHERE author_id = $1 AND pinned_at IS NOT NULL AND deleted_at IS NULL",
        caller.user_id
    )
    .fetch_one(&mut *tx)
    .await?;

    // Re-pinning something already pinned only moves it to the front, so it
    // must not count against the limit.
    let mine = sqlx::query!(
        "SELECT pinned_at IS NOT NULL AS \"pinned!\" FROM posts
         WHERE id = $1 AND author_id = $2 AND deleted_at IS NULL",
        id,
        caller.user_id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(PostError::NotFound)?;

    if !mine.pinned && already.count >= MAX_PINNED {
        return Err(PostError::Invalid(format!(
            "You can pin {MAX_PINNED} posts. Unpin one first."
        )));
    }

    sqlx::query!(
        "UPDATE posts SET pinned_at = now() WHERE id = $1 AND author_id = $2",
        id,
        caller.user_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Unpins one of your own posts. Idempotent.
async fn unpin(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
) -> Result<StatusCode, PostError> {
    if !state.limits.reactions.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "reactions rate limit reached");
        return Err(PostError::TooManyRequests);
    }

    sqlx::query!(
        "UPDATE posts SET pinned_at = NULL WHERE id = $1 AND author_id = $2",
        id,
        caller.user_id
    )
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_post(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
) -> Result<StatusCode, PostError> {
    if !state.limits.posts.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "posts rate limit reached");
        return Err(PostError::TooManyRequests);
    }

    // The author check is in the WHERE clause, so there is no window between
    // deciding and doing, and no way to reach the UPDATE without it.
    //
    // The body is blanked as well as the row hidden: "deleted" has to mean the
    // text is gone, not merely filtered out of one query.
    let result = sqlx::query!(
        "UPDATE posts SET deleted_at = now(), body = '', media_keys = '{}'
         WHERE id = $1 AND author_id = $2 AND deleted_at IS NULL",
        id,
        caller.user_id
    )
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        // Deliberately the same answer for "no such post" and "not yours": the
        // difference would confirm a post exists to someone who cannot see it.
        return Err(PostError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ReactRequest {
    pub emoji: String,
    /// `false` removes the reaction. One endpoint because the UI control is one
    /// toggle, and two endpoints would be two ways to get the same state wrong.
    #[serde(default = "yes")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

async fn react(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
    Json(request): Json<ReactRequest>,
) -> Result<Json<Vec<ReactionCount>>, PostError> {
    if !state.limits.reactions.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "reactions rate limit reached");
        return Err(PostError::TooManyRequests);
    }

    let emoji = request.emoji.trim();
    // A length in characters, and a refusal of anything with whitespace or
    // control characters in it: this string is rendered as-is in a reaction
    // pill, and the column's CHECK counts bytes rather than graphemes.
    if emoji.is_empty()
        || emoji.chars().count() > 4
        || emoji.len() > 16
        || emoji.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(PostError::Invalid("That is not an emoji.".into()));
    }

    let exists = sqlx::query!(
        "SELECT 1 AS \"ok!\" FROM posts WHERE id = $1 AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(PostError::NotFound);
    }

    if request.on {
        sqlx::query!(
            "INSERT INTO post_reactions (post_id, user_id, emoji)
             VALUES ($1, $2, $3)
             ON CONFLICT DO NOTHING",
            id,
            caller.user_id,
            emoji
        )
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query!(
            "DELETE FROM post_reactions
             WHERE post_id = $1 AND user_id = $2 AND emoji = $3",
            id,
            caller.user_id,
            emoji
        )
        .execute(&state.db)
        .await?;
    }

    let counts = reactions_for(&state, &[id], caller.user_id).await?;
    Ok(Json(
        counts.get(&id).map(|(c, _)| c.clone()).unwrap_or_default(),
    ))
}

/// Reaction counts and the caller's own reactions, for a set of posts.
///
/// One query for the whole page rather than one per post: a 20-post feed would
/// otherwise be 21 round trips, and that is the shape of slowness that only
/// shows up once real people are using it.
type Reactions = std::collections::HashMap<i64, (Vec<ReactionCount>, Vec<String>)>;

async fn reactions_for(
    state: &AppState,
    post_ids: &[i64],
    caller_id: i64,
) -> Result<Reactions, PostError> {
    if post_ids.is_empty() {
        return Ok(Reactions::new());
    }

    let rows = sqlx::query!(
        "SELECT post_id, emoji,
                COUNT(*) AS \"count!\",
                BOOL_OR(user_id = $2) AS \"mine!\"
         FROM post_reactions
         WHERE post_id = ANY($1)
         GROUP BY post_id, emoji
         ORDER BY \"count!\" DESC, emoji",
        post_ids,
        caller_id
    )
    .fetch_all(&state.db)
    .await?;

    let mut out = Reactions::new();
    for row in rows {
        let entry = out
            .entry(row.post_id)
            .or_insert_with(|| (Vec::new(), Vec::new()));
        entry.0.push(ReactionCount {
            emoji: row.emoji.clone(),
            count: row.count,
        });
        if row.mine {
            entry.1.push(row.emoji);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ votes ---

#[derive(Deserialize)]
pub struct VoteRequest {
    /// `1`, `-1`, or `0` to take a vote back.
    pub value: i16,
}

/// The score after a vote, so the client does not have to guess or refetch.
#[derive(Serialize)]
pub struct VoteView {
    pub score: i64,
    pub my_vote: i16,
}

/// Casts, changes, or withdraws a vote.
///
/// Idempotent by construction: the primary key is (post, user), so voting twice
/// the same way is an upsert that changes nothing and a score cannot be
/// inflated by clicking. `0` deletes the row, because "no vote" is the absence
/// of one rather than a third value a SUM would have to special-case.
async fn vote(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
    Json(request): Json<VoteRequest>,
) -> Result<Json<VoteView>, PostError> {
    if !state.limits.reactions.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "reactions rate limit reached");
        return Err(PostError::TooManyRequests);
    }

    if !matches!(request.value, -1..=1) {
        return Err(PostError::Invalid("A vote is up, down, or none.".into()));
    }

    let exists = sqlx::query!(
        "SELECT 1 AS \"ok!\" FROM posts WHERE id = $1 AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(PostError::NotFound);
    }

    if request.value == 0 {
        sqlx::query!(
            "DELETE FROM post_votes WHERE post_id = $1 AND user_id = $2",
            id,
            caller.user_id
        )
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query!(
            "INSERT INTO post_votes (post_id, user_id, value) VALUES ($1, $2, $3)
             ON CONFLICT (post_id, user_id) DO UPDATE SET value = excluded.value",
            id,
            caller.user_id,
            request.value
        )
        .execute(&state.db)
        .await?;
    }

    let row = sqlx::query!(
        "SELECT COALESCE(SUM(value), 0)::BIGINT AS \"score!\" FROM post_votes WHERE post_id = $1",
        id
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(VoteView {
        score: row.score,
        my_vote: request.value,
    }))
}

// --------------------------------------------------------------- comments ---

/// One comment. Flat on the wire; the client builds the tree from `parent_id`.
///
/// Sending a flat list rather than a nested structure keeps the response shape
/// independent of depth, and a thread arrives in one round trip either way.
#[derive(Debug, Serialize)]
pub struct CommentView {
    pub id: i64,
    pub post_id: i64,
    /// `None` for a top-level comment.
    pub parent_id: Option<i64>,
    pub author_id: i64,
    pub author_handle: String,
    pub author_display_name: String,
    pub author_avatar_key: Option<String>,
    /// Empty when the comment was deleted; `deleted` says which.
    pub body: String,
    pub created_at_ms: i64,
    pub is_mine: bool,
    /// Deleted comments keep their place so their replies keep theirs.
    pub deleted: bool,
}

#[derive(Deserialize)]
pub struct CreateCommentRequest {
    pub body: String,
    /// The comment being replied to. `None` posts at the top level.
    #[serde(default)]
    pub parent_id: Option<i64>,
}

/// The whole thread for a post, oldest first.
///
/// Deleted comments are included, blanked. Dropping them would orphan every
/// reply underneath and silently collapse the thread; keeping the row is what
/// makes "[deleted]" with its replies intact possible.
async fn list_comments(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
) -> Result<Json<Vec<CommentView>>, PostError> {
    let rows = sqlx::query!(
        "SELECT c.id, c.post_id, c.parent_id, c.author_id, c.body,
                c.deleted_at IS NOT NULL AS \"deleted!\",
                (EXTRACT(EPOCH FROM c.created_at) * 1000)::BIGINT AS \"created_at_ms!\",
                u.handle::TEXT AS \"handle!\", u.display_name, u.avatar_key
         FROM post_comments c
         JOIN users u ON u.id = c.author_id
         WHERE c.post_id = $1
         ORDER BY c.id",
        id
    )
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        rows.into_iter()
            .map(|r| CommentView {
                id: r.id,
                post_id: r.post_id,
                parent_id: r.parent_id,
                author_id: r.author_id,
                author_handle: r.handle,
                author_display_name: r.display_name,
                author_avatar_key: r.avatar_key,
                body: if r.deleted { String::new() } else { r.body },
                created_at_ms: r.created_at_ms,
                is_mine: r.author_id == caller.user_id,
                deleted: r.deleted,
            })
            .collect(),
    ))
}

async fn create_comment(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
    Json(request): Json<CreateCommentRequest>,
) -> Result<(StatusCode, Json<CommentView>), PostError> {
    if !state.limits.comments.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "comments rate limit reached");
        return Err(PostError::TooManyRequests);
    }

    let body = request.body.trim();
    if body.is_empty() {
        return Err(PostError::Invalid("Write something first.".into()));
    }
    if body.chars().count() > 2000 {
        return Err(PostError::Invalid(
            "A comment is up to 2000 characters.".into(),
        ));
    }

    let exists = sqlx::query!(
        "SELECT 1 AS \"ok!\" FROM posts WHERE id = $1 AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&state.db)
    .await?;
    if exists.is_none() {
        return Err(PostError::NotFound);
    }

    // A parent from another post would put a reply in a thread it does not
    // belong to, which the client would then render nowhere.
    if let Some(parent) = request.parent_id {
        let ok = sqlx::query!(
            "SELECT 1 AS \"ok!\" FROM post_comments WHERE id = $1 AND post_id = $2",
            parent,
            id
        )
        .fetch_optional(&state.db)
        .await?;
        if ok.is_none() {
            return Err(PostError::Invalid(
                "That comment is not on this post.".into(),
            ));
        }
    }

    let row = sqlx::query!(
        "INSERT INTO post_comments (post_id, parent_id, author_id, body)
         VALUES ($1, $2, $3, $4)
         RETURNING id, (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS \"created_at_ms!\"",
        id,
        request.parent_id,
        caller.user_id,
        body
    )
    .fetch_one(&state.db)
    .await?;

    let author = sqlx::query!(
        "SELECT handle::TEXT AS \"handle!\", display_name, avatar_key
         FROM users WHERE id = $1",
        caller.user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(CommentView {
            id: row.id,
            post_id: id,
            parent_id: request.parent_id,
            author_id: caller.user_id,
            author_handle: author.handle,
            author_display_name: author.display_name,
            author_avatar_key: author.avatar_key,
            body: body.to_string(),
            created_at_ms: row.created_at_ms,
            is_mine: true,
            deleted: false,
        }),
    ))
}

/// Blanks a comment, keeping the row so its replies keep their place.
async fn delete_comment(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<i64>,
) -> Result<StatusCode, PostError> {
    if !state.limits.comments.check(&caller.user_id.to_string()) {
        tracing::warn!(user_id = caller.user_id, "comments rate limit reached");
        return Err(PostError::TooManyRequests);
    }

    let done = sqlx::query!(
        "UPDATE post_comments SET deleted_at = now(), body = ''
         WHERE id = $1 AND author_id = $2 AND deleted_at IS NULL",
        id,
        caller.user_id
    )
    .execute(&state.db)
    .await?;

    // Not yours and no such comment are the same answer on purpose: telling
    // them apart tells a stranger which ids exist.
    if done.rows_affected() == 0 {
        return Err(PostError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_size_is_clamped_to_something_sane() {
        // A caller asking for a million posts is either broken or hostile, and
        // either way the answer is the same.
        for (asked, expected) in [
            (Some(10), 10),
            (Some(0), 1),
            (Some(-5), 1),
            (Some(10_000), MAX_PAGE_SIZE),
            (None, PAGE_SIZE),
        ] {
            let limit = asked.unwrap_or(PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
            assert_eq!(limit, expected, "limit={asked:?}");
        }
    }

    #[test]
    fn media_keys_are_namespaced_to_their_owner() {
        // The check that stops a post pointing at someone else's object -- or
        // at the encrypted bucket, which must never be referenced from a
        // public row.
        let mine = "media/42/abc";
        let theirs = "media/43/abc";
        let encrypted = "enc/11111111-1111-1111-1111-111111111111/abc";
        let prefix = "media/42/";
        assert!(mine.starts_with(prefix));
        assert!(!theirs.starts_with(prefix));
        assert!(!encrypted.starts_with(prefix));
        // And a prefix that merely starts the same is not a match.
        assert!(!"media/420/abc".starts_with(prefix));
    }
}
