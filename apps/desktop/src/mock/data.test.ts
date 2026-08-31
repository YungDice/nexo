import { describe, expect, it } from "vitest";
import { buildMockData, lastMessage, messagesFor, personById } from "./data";

const now = new Date(2026, 7, 24, 14, 30);
const data = buildMockData(now);

describe("mock data", () => {
  it("references only people it defines", () => {
    const ids = new Set(data.people.map((person) => person.id));
    for (const message of data.messages) expect(ids.has(message.authorId)).toBe(true);
    for (const post of data.posts) expect(ids.has(post.authorId)).toBe(true);
    for (const conversation of data.conversations) {
      for (const member of conversation.memberIds) expect(ids.has(member)).toBe(true);
    }
  });

  it("attaches every message to a conversation that exists", () => {
    const ids = new Set(data.conversations.map((conversation) => conversation.id));
    for (const message of data.messages) expect(ids.has(message.conversationId)).toBe(true);
  });

  /**
   * §4.1 fixes the shape of a safety number, and the UI renders whatever it is
   * given. A short one would silently draw a short grid, so the invariant is
   * checked here rather than discovered on screen.
   */
  it("carries 60-digit safety numbers and fingerprints", () => {
    for (const conversation of data.conversations) {
      expect(conversation.safetyDigits).toMatch(/^\d{60}$/);
    }
    expect(data.deviceFingerprint).toMatch(/^\d{60}$/);
  });

  it("has handles that match the 3–20 char rule in §4.1", () => {
    for (const person of data.people) expect(person.handle).toMatch(/^[a-z0-9_]{3,20}$/);
  });

  /** Rule 7: an undecryptable message never carries a body to fall back to. */
  it("keeps undecryptable messages empty", () => {
    for (const message of data.messages) {
      if (message.undecryptable) expect(message.body).toBe("");
    }
  });

  it("sorts a conversation oldest first, so the newest is the preview", () => {
    const messages = messagesFor(data, "c-design");
    const times = messages.map((message) => message.at.getTime());
    expect([...times].sort((a, b) => a - b)).toEqual(times);
    expect(lastMessage(data, "c-design")?.id).toBe(messages[messages.length - 1]?.id);
  });

  it("throws on an unknown person rather than rendering a blank", () => {
    expect(() => personById(data, "u-nobody")).toThrow();
  });
});
