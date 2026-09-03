import type { PostKind } from "../../lib/feed";

/** What is in the composer right now. */
export interface Draft {
  title: string;
  body: string;
  linkUrl: string;
  images: number;
}

export interface Composed {
  kind: PostKind;
  /** The link to send, or null. Never an empty string. */
  linkUrl: string | null;
  /** Post would do something. */
  ready: boolean;
  /** Why it would not be accepted, when that can be said before sending. */
  problem: string | null;
}

/**
 * What a draft amounts to.
 *
 * The composer had three tabs — Text, Link, Image — chosen before anything was
 * written, and they were a mode where a derivation would do: whether a post is
 * a link post is entirely answered by whether it has a link in it. Choosing
 * first meant deciding what you were going to write before writing it, and
 * then being unable to add a picture to a "text" post that already had one.
 *
 * So the kind is read off the draft instead, and this is where that reading
 * lives, because it has to agree with `posts.rs` exactly:
 *
 * - a link wins, and a link post may carry images too — the server allows it;
 * - failing that, images make an image post, which the server refuses to give
 *   a link (so the order here is not a preference, it is the only order that
 *   does not produce a request the server rejects);
 * - failing both, it is text, which the server also refuses to give a link.
 *
 * The scheme is checked here as well as there. The server has to check it —
 * `javascript:` in a feed is stored XSS and the row is the place that must not
 * hold one — but a person who typed `example.com` deserves to be told before
 * they press Post rather than after.
 */
export function compose(draft: Draft): Composed {
  const link = draft.linkUrl.trim();
  const title = draft.title.trim();
  const body = draft.body.trim();

  const kind: PostKind = link ? "link" : draft.images > 0 ? "image" : "text";

  if (link && !/^https?:\/\//i.test(link)) {
    return {
      kind,
      linkUrl: link,
      ready: false,
      problem: "A link has to start with http:// or https://.",
    };
  }
  if (draft.images > 4) {
    return {
      kind,
      linkUrl: link || null,
      ready: false,
      problem: "Up to four images.",
    };
  }

  return {
    kind,
    linkUrl: link || null,
    // Anything at all counts, which is the server's rule for a text post
    // widened to the other two by the fact that they cannot be empty anyway:
    // a link post has a link and an image post has an image.
    ready: !!(link || title || body || draft.images > 0),
    problem: null,
  };
}
