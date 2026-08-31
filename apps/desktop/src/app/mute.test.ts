import { describe, expect, it } from "vitest";

import { isMuted } from "./store";
import { arrivalDecision } from "./syncAgent";

/**
 * Muting decides whether a person gets interrupted, so both directions cost
 * something real: too eager and a conversation someone silenced still shouts;
 * too shy and a message arrives with nothing to announce it.
 *
 * The list and the sync agent both read this. A disagreement between them
 * would be a conversation that draws the bell and toasts anyway.
 */
describe("isMuted", () => {
  const now = 1_000_000;

  it("is not muted when nothing was ever chosen", () => {
    expect(isMuted(undefined, now)).toBe(false);
    expect(isMuted({}, now)).toBe(false);
    // Pinned but never muted is the case that would break a naive
    // "does the override exist" check.
    expect(isMuted({ pinned: true }, now)).toBe(false);
  });

  it("stays muted while the deadline is ahead", () => {
    expect(isMuted({ mutedUntil: now + 1 }, now)).toBe(true);
  });

  it("comes back on by itself once the deadline passes", () => {
    // The whole point of a timestamp over a flag: nothing has to run at the
    // right moment for a timed mute to end.
    expect(isMuted({ mutedUntil: now - 1 }, now)).toBe(false);
    expect(isMuted({ mutedUntil: now }, now)).toBe(false);
  });

  it("treats a mute with no end as endless, through JSON and back", () => {
    expect(isMuted({ mutedUntil: Number.POSITIVE_INFINITY }, now)).toBe(true);
    // `JSON.stringify(Infinity)` is `null`, and the overrides are persisted as
    // JSON. If `null` read as "not muted", every permanent mute would undo
    // itself on the next restart -- silently, and only for people who had
    // restarted.
    const round = JSON.parse(
      JSON.stringify({ mutedUntil: Number.POSITIVE_INFINITY }),
    ) as { mutedUntil: number | null };
    expect(round.mutedUntil).toBeNull();
    expect(isMuted(round, now)).toBe(true);
  });
});

describe("arrivalDecision with mute", () => {
  const base = {
    conversationId: "a",
    activeConversationId: "b",
    onMessagesRoute: false,
    windowFocused: false,
  };

  it("silences the toast but keeps counting", () => {
    // Mute means "stop interrupting me", not "lose my messages". Dropping the
    // count as well is the version of this feature that loses mail.
    expect(arrivalDecision({ ...base, muted: true })).toEqual({
      countUnread: true,
      toast: false,
    });
  });

  it("says nothing at all about a conversation being read right now", () => {
    expect(
      arrivalDecision({
        conversationId: "a",
        activeConversationId: "a",
        onMessagesRoute: true,
        windowFocused: true,
        muted: false,
      }),
    ).toEqual({ countUnread: false, toast: false });
  });
});
