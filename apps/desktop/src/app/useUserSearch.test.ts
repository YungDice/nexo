import { describe, expect, it } from "vitest";

import { MIN_TERM, searchable } from "./useUserSearch";

/**
 * When a search is worth sending.
 *
 * The number matches the server's, which refuses anything shorter and says
 * why: one character stops being a search and becomes a download of the user
 * table. Knowing it here saves a round trip for the first letter of every name
 * anybody types, so the two must not drift apart.
 */
describe("searchable", () => {
  it("needs two characters", () => {
    expect(MIN_TERM).toBe(2);
    expect(searchable("")).toBe(false);
    expect(searchable("a")).toBe(false);
    expect(searchable("al")).toBe(true);
  });

  it("does not count whitespace towards them", () => {
    // " a " is one character typed and two spaces, and the server trims before
    // it counts. Sending it would be a request guaranteed to return nothing.
    expect(searchable("  ")).toBe(false);
    expect(searchable(" a ")).toBe(false);
    expect(searchable(" al ")).toBe(true);
  });
});
