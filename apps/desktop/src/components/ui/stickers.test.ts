import { describe, expect, it } from "vitest";

import { PACK, STICKERS, findSticker, matchesSticker } from "./stickers";

/**
 * The pack is part of the wire format.
 *
 * A `Payload::Sticker` names a pack and an id, so these strings are not
 * internal: messages already sent refer to them. Renaming one silently changes
 * what an old message draws, which is why the ids are tested rather than
 * trusted to review.
 */
describe("the bundled sticker pack", () => {
  it("has no duplicate ids", () => {
    const ids = STICKERS.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("keeps the ids messages already refer to", () => {
    // Add to this list when a sticker is added. Never remove from it: an id
    // that disappears is a message somewhere that stops drawing.
    for (const id of ["thumbs-up", "heart", "laughing", "party", "check", "no"]) {
      expect(findSticker(PACK, id), `${id} must still exist`).toBeDefined();
    }
  });

  it("gives every sticker something to search by", () => {
    for (const sticker of STICKERS) {
      expect(sticker.label.length, sticker.id).toBeGreaterThan(0);
      expect(sticker.keywords.length, sticker.id).toBeGreaterThan(0);
    }
  });

  it("does not resolve a pack it does not have", () => {
    // A newer client can name a pack this build has never heard of, and the
    // bubble has to say so rather than draw nothing.
    expect(findSticker("someone-elses-pack", "heart")).toBeUndefined();
    expect(findSticker(PACK, "not-a-sticker")).toBeUndefined();
  });

  it("searches meaning as well as name", () => {
    const party = findSticker(PACK, "party");
    expect(party).toBeDefined();
    // People search for what they mean, not for what it is called.
    expect(matchesSticker(party!, "congrats")).toBe(true);
    expect(matchesSticker(party!, "party")).toBe(true);
    expect(matchesSticker(party!, "spreadsheet")).toBe(false);
  });

  it("shows everything for an empty search", () => {
    for (const sticker of STICKERS) {
      expect(matchesSticker(sticker, "  ")).toBe(true);
    }
  });
});
