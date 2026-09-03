import { describe, expect, it } from "vitest";

import type { Conversation } from "../../lib/types";
import { peerHandle } from "./peer";

/**
 * Who a menu entry would block.
 *
 * Getting this wrong is not a cosmetic bug: an entry labelled with one
 * person's name that blocks a different one is the worst outcome an action
 * like this has. Every case below is a way the member list actually arrives,
 * not a hypothetical — it may contain us, it may be a group, and right after
 * joining from a Welcome it is empty until the profile fetch fills it in.
 */
describe("peerHandle", () => {
  const dm = (memberIds: string[]): Conversation => ({
    id: "c1",
    kind: "dm",
    title: "Alice",
    memberIds,
    unread: 0,
    verified: false,
    safetyDigits: "",
    muted: false,
  });

  it("is the other person in a two-person conversation", () => {
    expect(peerHandle(dm(["alice"]), "me")).toBe("alice");
  });

  it("filters us out when the list includes us", () => {
    // `title_from` in the core filters the same way, because the list is not
    // guaranteed to be "everyone else" despite what its name suggests.
    expect(peerHandle(dm(["me", "alice"]), "me")).toBe("alice");
  });

  it("has no answer before the member list has arrived", () => {
    // Joining from a Welcome leaves this empty until M7's profile fetch. The
    // menu offers nothing rather than an entry that cannot name anybody.
    expect(peerHandle(dm([]), "me")).toBeUndefined();
  });

  it("has no answer for a group", () => {
    const group: Conversation = { ...dm(["alice", "bob"]), kind: "group" };
    expect(peerHandle(group, "me")).toBeUndefined();
  });

  it("has no answer when a DM somehow holds more than two people", () => {
    // Should not happen, and if it does, guessing which of them the entry
    // means is worse than saying nothing.
    expect(peerHandle(dm(["alice", "bob"]), "me")).toBeUndefined();
  });

  it("does not filter when we do not know our own handle", () => {
    // Signed in but the account has not loaded yet. One name is still one
    // name, so the answer stands.
    expect(peerHandle(dm(["alice"]), undefined)).toBe("alice");
  });
});
