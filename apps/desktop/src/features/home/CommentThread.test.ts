import { describe, expect, it } from "vitest";

import { buildThread } from "./CommentThread";
import type { Comment } from "../../lib/feed";

function comment(id: number, parent_id: number | null = null): Comment {
  return {
    id,
    post_id: 1,
    parent_id,
    author_id: 1,
    author_handle: "alice",
    author_display_name: "Alice",
    author_avatar_key: null,
    body: `comment ${id}`,
    created_at_ms: id,
    is_mine: false,
    deleted: false,
  };
}

describe("buildThread", () => {
  it("keeps top-level comments in order", () => {
    const roots = buildThread([comment(1), comment(2), comment(3)]);
    expect(roots.map((n) => n.comment.id)).toEqual([1, 2, 3]);
    expect(roots.every((n) => n.replies.length === 0)).toBe(true);
  });

  it("hangs a reply off its parent", () => {
    const roots = buildThread([comment(1), comment(2, 1)]);
    expect(roots).toHaveLength(1);
    expect(roots[0]!.replies.map((n) => n.comment.id)).toEqual([2]);
  });

  it("nests to arbitrary depth", () => {
    const roots = buildThread([comment(1), comment(2, 1), comment(3, 2), comment(4, 3)]);
    expect(roots).toHaveLength(1);
    const depth2 = roots[0]!.replies[0]!;
    expect(depth2.comment.id).toBe(2);
    const depth3 = depth2.replies[0]!;
    expect(depth3.comment.id).toBe(3);
    expect(depth3.replies[0]!.comment.id).toBe(4);
  });

  it("keeps sibling replies in the order they arrived", () => {
    const roots = buildThread([comment(1), comment(3, 1), comment(2, 1)]);
    expect(roots[0]!.replies.map((n) => n.comment.id)).toEqual([3, 2]);
  });

  /// Should not happen — the server refuses a parent from another post — but
  /// losing somebody's comment to a broken link is the worse failure.
  it("promotes an orphan rather than dropping it", () => {
    const roots = buildThread([comment(1), comment(2, 99)]);
    expect(roots.map((n) => n.comment.id)).toEqual([1, 2]);
  });

  it("has nothing to build from an empty thread", () => {
    expect(buildThread([])).toEqual([]);
  });
});
