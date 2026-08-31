import { invoke } from "@tauri-apps/api/core";

/**
 * The Home feed and profiles, as the WebView sees them.
 *
 * **None of this is end-to-end encrypted**, and that is not a caveat buried in
 * a type file — §4.4 requires the product to say it in plain language where
 * someone is about to post, which `HomePage` does under the composer. It is
 * repeated here because this is the module a future change would touch while
 * assuming the rest of the app's guarantees carry over. They do not: the server
 * can read every post, every profile field, and every feed image.
 *
 * What still holds is rule 2. Image bytes and presigned URLs stay in Rust: the
 * WebView passes a **path** to `uploadImage` and gets back an object key.
 */

export interface Post {
  id: number;
  author_id: number;
  author_handle: string;
  author_display_name: string;
  author_avatar_key: string | null;
  body: string;
  /** Object keys, not URLs. Each is presigned on demand by `imageUrl`. */
  media_keys: string[];
  created_at_ms: number;
  reactions: ReactionCount[];
  /** Emoji this account has used, so the UI can draw them pressed. */
  my_reactions: string[];
  is_mine: boolean;
  /** The headline, when there is one. Posts written before titles have none. */
  title: string | null;
  kind: PostKind;
  /** Where a link post points. `null` for the other kinds. */
  link_url: string | null;
  /** Upvotes minus downvotes. */
  score: number;
  /** This account's own vote: 1, -1, or 0. */
  my_vote: number;
  comment_count: number;
  /** Pinned to the top of its author's profile. Only set on a profile page. */
  pinned?: boolean;
}

export type PostKind = "text" | "link" | "image";

/** How a feed page is ordered. */
export type FeedSort = "new" | "top" | "hot";

/**
 * One comment, flat.
 *
 * The tree is rebuilt from `parent_id` where it is drawn, rather than arriving
 * nested: a flat list keeps the response shape independent of depth, and the
 * whole thread comes in one round trip either way.
 */
export interface Comment {
  id: number;
  post_id: number;
  /** `null` at the top level. */
  parent_id: number | null;
  author_id: number;
  author_handle: string;
  author_display_name: string;
  author_avatar_key: string | null;
  /** Empty when deleted. */
  body: string;
  created_at_ms: number;
  is_mine: boolean;
  /** Deleted comments keep their place so their replies keep theirs. */
  deleted: boolean;
}

export interface VoteResult {
  score: number;
  my_vote: number;
}

export interface ReactionCount {
  emoji: string;
  count: number;
}

export interface FeedPage {
  posts: Post[];
  /** Pass back as `before`. `null` means the end of the feed. */
  next_cursor: number | null;
}

export interface ProfileLink {
  label: string;
  /** Always http(s) — refused at three layers, and opened in the system
   * browser rather than the WebView. */
  url: string;
}

/**
 * Someone's profile, as far as this viewer may see it.
 *
 * A field that is `null` is **hidden**, not empty. The UI must say "not
 * shared" rather than drawing a blank line that reads as "they wrote nothing" —
 * the two are different facts and conflating them misrepresents a privacy
 * setting as an empty one.
 */
export interface Profile {
  user_id: number;
  handle: string;
  display_name: string;
  avatar_key: string | null;
  banner_key: string | null;
  bio: string | null;
  location: string | null;
  links: ProfileLink[] | null;
  join_date_ms: number | null;
  is_me: boolean;
}

/** Who may see one field (G2). */
export type Visibility = "public" | "contacts" | "private";

/** A settable field. Handle and display name are absent: they are how you are
 * addressed, so a control for them would be one that cannot be honoured. */
export type VisibilityField = "bio" | "location" | "links" | "join_date";

export interface MyProfile extends Profile {
  /** Every settable field, with the value actually in force. */
  visibility: Record<VisibilityField, Visibility>;
}

export interface ProfileEdit {
  display_name?: string;
  bio?: string;
  location?: string;
  links?: ProfileLink[];
  avatar_key?: string;
  banner_key?: string;
}

export interface FeedError {
  kind:
    | "unreachable"
    | "signed_out"
    | "rejected"
    | "invalid_request"
    | "unreadable_file"
    | "too_large"
    | "internal";
  message: string;
}

/** Narrows an unknown rejection to something renderable. */
export function asFeedError(error: unknown): FeedError {
  if (
    typeof error === "object" &&
    error !== null &&
    "kind" in error &&
    "message" in error
  ) {
    return error as FeedError;
  }
  return { kind: "internal", message: "Something went wrong. Try again." };
}

/**
 * A page of the feed.
 *
 * `before` is a post id under `new` and a row offset under `top` and `hot` —
 * those orders are not monotonic in id, so an id cannot express a page
 * boundary in them. Pass back whatever `next_cursor` gave you.
 */
export function feed(before?: number, sort: FeedSort = "new"): Promise<FeedPage> {
  return invoke<FeedPage>("feed", { before: before ?? null, sort });
}

export function postsBy(handle: string, before?: number): Promise<FeedPage> {
  return invoke<FeedPage>("posts_by", { handle, before: before ?? null });
}

export function createPost(input: {
  body: string;
  mediaKeys?: string[];
  title?: string | null;
  kind?: PostKind;
  linkUrl?: string | null;
}): Promise<Post> {
  return invoke<Post>("create_post", {
    body: input.body,
    mediaKeys: input.mediaKeys ?? [],
    title: input.title ?? null,
    kind: input.kind ?? "text",
    linkUrl: input.linkUrl ?? null,
  });
}

/** Pins one of your own posts. At most three; the server enforces it. */
export function pinPost(id: number): Promise<void> {
  return invoke<void>("pin_post", { id });
}

/** Unpins one of your own posts. */
export function unpinPost(id: number): Promise<void> {
  return invoke<void>("unpin_post", { id });
}

export function deletePost(id: number): Promise<void> {
  return invoke<void>("delete_post", { id });
}

export function react(
  id: number,
  emoji: string,
  on: boolean,
): Promise<ReactionCount[]> {
  return invoke<ReactionCount[]>("react", { id, emoji, on });
}

/** Votes on a post. `1`, `-1`, or `0` to take a vote back. */
export function vote(id: number, value: number): Promise<VoteResult> {
  return invoke<VoteResult>("vote", { id, value });
}

/** The whole thread for a post, oldest first, deleted ones included. */
export function comments(postId: number): Promise<Comment[]> {
  return invoke<Comment[]>("comments", { postId });
}

/** Adds a comment, or a reply when `parentId` is given. */
export function addComment(
  postId: number,
  body: string,
  parentId?: number | null,
): Promise<Comment> {
  return invoke<Comment>("add_comment", { postId, body, parentId: parentId ?? null });
}

export function deleteComment(id: number): Promise<void> {
  return invoke<void>("delete_comment", { id });
}

export function profile(handle: string): Promise<Profile> {
  return invoke<Profile>("profile", { handle });
}

export function myProfile(): Promise<MyProfile> {
  return invoke<MyProfile>("my_profile");
}

export function updateProfile(edit: ProfileEdit): Promise<MyProfile> {
  return invoke<MyProfile>("update_profile", { edit });
}

export function updateVisibility(
  visibility: Partial<Record<VisibilityField, Visibility>>,
): Promise<MyProfile> {
  return invoke<MyProfile>("update_visibility", { visibility });
}

/**
 * Uploads an image the user already picked, and returns its object key.
 *
 * The path goes to Rust; the bytes and the presigned URL stay there. A
 * presigned PUT is a bearer credential for one object, and there is no reason
 * for a WebView to be holding one.
 */
export function uploadImage(path: string): Promise<string> {
  return invoke<string>("upload_image", { path });
}

/**
 * A stored image, inlined as a `data:` URL.
 *
 * Not the presigned object-storage URL it looks like it should be: the CSP
 * allows `img-src 'self' asset: data: blob:` and no remote host, so a bucket
 * URL is blocked before a byte is fetched. Rust downloads it and hands over
 * the bytes, which keeps the bucket unreachable from anything running in the
 * page.
 */
export function imageUrl(key: string): Promise<string> {
  return invoke<string>("image_data_url", { key });
}
