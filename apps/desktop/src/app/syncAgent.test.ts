import { describe, expect, it } from "vitest";

import { arrivalDecision } from "./syncAgent";

/**
 * The rule that decides what a new message becomes: nothing, a badge, or a
 * badge and a toast. Wrong in one direction it spams toasts for a conversation
 * the person is reading; wrong in the other it swallows messages silently.
 */
describe("arrivalDecision", () => {
  const reading = {
    conversationId: "c-1",
    activeConversationId: "c-1",
    onMessagesRoute: true,
    windowFocused: true,
    muted: false,
  };

  it("a conversation being read right now needs neither badge nor toast", () => {
    expect(arrivalDecision(reading)).toEqual({ countUnread: false, toast: false });
  });

  it("another conversation counts and toasts, even while the app is focused", () => {
    const decision = arrivalDecision({ ...reading, conversationId: "c-2" });
    expect(decision).toEqual({ countUnread: true, toast: true });
  });

  it("an unfocused window means even the open conversation is unread", () => {
    // The window may still be rendering it, but nobody is looking at it.
    const decision = arrivalDecision({ ...reading, windowFocused: false });
    expect(decision).toEqual({ countUnread: true, toast: true });
  });

  it("a different page means the open conversation is not being read", () => {
    const decision = arrivalDecision({ ...reading, onMessagesRoute: false });
    expect(decision).toEqual({ countUnread: true, toast: true });
  });

  it("muted silences the toast but never the count", () => {
    // Mute means "stop interrupting me". Hiding the badge too would turn it
    // into "lose my messages".
    const decision = arrivalDecision({
      ...reading,
      conversationId: "c-2",
      muted: true,
    });
    expect(decision).toEqual({ countUnread: true, toast: false });
  });
});
