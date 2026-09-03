import { describe, expect, it, vi } from "vitest";

import {
  messageMenuItems,
  type MessageMenuActions,
  type MessageMenuState,
} from "./menu";

const actions: MessageMenuActions = {
  copy: vi.fn(),
  edit: vi.fn(),
  react: vi.fn(),
  togglePin: vi.fn(),
  deleteForMe: vi.fn(),
  deleteForEveryone: vi.fn(),
};

const base: MessageMenuState = {
  hasBody: true,
  mine: true,
  clientId: "c1",
  retracted: false,
  withinWindow: true,
  queued: false,
  pinned: false,
};

const labels = (state: Partial<MessageMenuState>) =>
  messageMenuItems({ ...base, ...state }, actions).map((item) => item.label);

/**
 * The order of the message menu.
 *
 * Worth a test rather than a careful read, because the entries come and go
 * with the message's state: "the last two" means something different in each
 * of the cases below, and every one of them is reachable by right-clicking a
 * real message. This is also where `MenuItem`'s "destructive entries always sit
 * last" is actually kept.
 */
describe("messageMenuItems", () => {
  it("ends with the two deletions, least reaching first", () => {
    expect(labels({})).toEqual([
      "Copy text",
      "Edit",
      "React",
      "Pin on this device",
      "Delete for me",
      "Delete for everyone",
    ]);
  });

  it("keeps them last when there is nothing to copy", () => {
    // An image with no caption. The entry above them goes, they do not move.
    expect(labels({ hasBody: false }).slice(-2)).toEqual([
      "Delete for me",
      "Delete for everyone",
    ]);
  });

  it("leaves only the local delete once the window has closed", () => {
    // Absent, not greyed out: an action that is gone was never offered.
    const items = labels({ withinWindow: false });
    expect(items).not.toContain("Edit");
    expect(items).not.toContain("Delete for everyone");
    expect(items.at(-1)).toBe("Delete for me");
  });

  it("leaves only the local delete on somebody else's message", () => {
    const items = labels({ mine: false });
    expect(items).not.toContain("Delete for everyone");
    expect(items.at(-1)).toBe("Delete for me");
  });

  it("offers no deletion at all while the message is queued", () => {
    // No envelope id yet, and that is what both a pin and a local delete are
    // keyed by. "Delete for everyone" is still ours to offer -- taking one out
    // of the outbox is the cleanest version of it, since nothing was sent.
    const items = labels({ queued: true });
    expect(items).not.toContain("Delete for me");
    expect(items).not.toContain("Pin on this device");
    expect(items.at(-1)).toBe("Delete for everyone");
  });

  it("offers nothing to revise on a message with no name", () => {
    // Sent before message ids existed. Nothing can refer to it.
    const items = labels({ clientId: undefined });
    expect(items).toEqual(["Copy text", "Pin on this device", "Delete for me"]);
  });

  it("marks both deletions destructive", () => {
    const items = messageMenuItems(base, actions);
    expect(items.slice(-2).every((item) => item.danger)).toBe(true);
    // And nothing above them is, or "last" would not be saying anything.
    expect(items.slice(0, -2).some((item) => item.danger)).toBe(false);
  });
});
