import { describe, expect, it } from "vitest";

import { applyReaction } from "./useFeed";
import type { Post } from "../lib/feed";

function post(overrides: Partial<Post> = {}): Post {
  return {
    id: 1,
    author_id: 1,
    author_handle: "alice",
    author_display_name: "Alice",
    author_avatar_key: null,
    body: "hello",
    media_keys: [],
    created_at_ms: 0,
    reactions: [],
    my_reactions: [],
    is_mine: false,
    title: null,
    kind: "text",
    link_url: null,
    score: 0,
    my_vote: 0,
    comment_count: 0,
    ...overrides,
  };
}

describe("applyReaction", () => {
  it("adds a new emoji with a count of one", () => {
    const next = applyReaction(post(), "👍", true);
    expect(next.reactions).toEqual([{ emoji: "👍", count: 1 }]);
    expect(next.my_reactions).toEqual(["👍"]);
  });

  it("increments an emoji someone else already used", () => {
    const next = applyReaction(
      post({ reactions: [{ emoji: "👍", count: 3 }] }),
      "👍",
      true,
    );
    expect(next.reactions).toEqual([{ emoji: "👍", count: 4 }]);
    expect(next.my_reactions).toEqual(["👍"]);
  });

  it("removes the pill entirely when the last reaction goes", () => {
    // The case that looks wrong on screen: a pill reading "👍 0" left behind
    // by an off-by-one.
    const next = applyReaction(
      post({ reactions: [{ emoji: "👍", count: 1 }], my_reactions: ["👍"] }),
      "👍",
      false,
    );
    expect(next.reactions).toEqual([]);
    expect(next.my_reactions).toEqual([]);
  });

  it("decrements without removing when others still hold it", () => {
    const next = applyReaction(
      post({ reactions: [{ emoji: "👍", count: 3 }], my_reactions: ["👍"] }),
      "👍",
      false,
    );
    expect(next.reactions).toEqual([{ emoji: "👍", count: 2 }]);
    expect(next.my_reactions).toEqual([]);
  });

  it("leaves other emoji alone", () => {
    const next = applyReaction(
      post({
        reactions: [
          { emoji: "👍", count: 2 },
          { emoji: "🔥", count: 5 },
        ],
        my_reactions: ["🔥"],
      }),
      "👍",
      true,
    );
    expect(next.reactions).toEqual([
      { emoji: "👍", count: 3 },
      { emoji: "🔥", count: 5 },
    ]);
    expect(next.my_reactions).toEqual(["🔥", "👍"]);
  });

  it("does not mutate the post it was given", () => {
    // The list is React state. A mutation here would update the array in place
    // and the re-render would not happen.
    const original = post({ reactions: [{ emoji: "👍", count: 1 }] });
    const snapshot = JSON.stringify(original);
    applyReaction(original, "👍", true);
    expect(JSON.stringify(original)).toBe(snapshot);
  });

  it("turning off an emoji that was never on is a no-op on the counts", () => {
    // Reachable if a reply from the server arrives between the click and the
    // optimistic update. It must not produce a negative count.
    const next = applyReaction(post(), "👍", false);
    expect(next.reactions).toEqual([]);
    expect(next.my_reactions).toEqual([]);
  });
});
