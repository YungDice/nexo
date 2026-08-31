import { describe, expect, it } from "vitest";

import { clickSelection, pruneSelection, type SelectionState } from "./selection";

const ORDER = ["a", "b", "c", "d", "e"];
const NOTHING: SelectionState = { selected: new Set(), anchor: null, open: false };

const plain = { toggle: false, range: false };
const ctrl = { toggle: true, range: false };
const shift = { toggle: false, range: true };

function ids(state: SelectionState): string[] {
  return [...state.selected].sort();
}

describe("clickSelection", () => {
  it("opens the conversation on a plain click and selects nothing", () => {
    const next = clickSelection(ORDER, NOTHING, "c", plain);
    expect(next.open).toBe(true);
    expect(ids(next)).toEqual([]);
    expect(next.anchor).toBe("c");
  });

  it("adds and removes one at a time with ctrl, without opening anything", () => {
    let state = clickSelection(ORDER, NOTHING, "b", ctrl);
    state = clickSelection(ORDER, state, "d", ctrl);
    expect(ids(state)).toEqual(["b", "d"]);
    expect(state.open).toBe(false);

    state = clickSelection(ORDER, state, "b", ctrl);
    expect(ids(state)).toEqual(["d"]);
  });

  it("takes everything between the anchor and the click with shift", () => {
    const anchored = clickSelection(ORDER, NOTHING, "b", plain);
    const range = clickSelection(ORDER, anchored, "d", shift);
    expect(ids(range)).toEqual(["b", "c", "d"]);
  });

  it("runs a range backwards just as well", () => {
    const anchored = clickSelection(ORDER, NOTHING, "d", plain);
    const range = clickSelection(ORDER, anchored, "b", shift);
    expect(ids(range)).toEqual(["b", "c", "d"]);
  });

  it("keeps the anchor still across repeated shift clicks", () => {
    // The case that goes wrong when the anchor moves: the second shift click
    // has to redraw the range from the same end, not from wherever the first
    // one finished. Otherwise shrinking a selection is impossible.
    const anchored = clickSelection(ORDER, NOTHING, "b", plain);
    const wide = clickSelection(ORDER, anchored, "e", shift);
    expect(ids(wide)).toEqual(["b", "c", "d", "e"]);

    const narrow = clickSelection(ORDER, wide, "c", shift);
    expect(ids(narrow)).toEqual(["b", "c"]);
    expect(narrow.anchor).toBe("b");
  });

  it("clears a selection when an ordinary click lands", () => {
    const some = clickSelection(ORDER, clickSelection(ORDER, NOTHING, "a", ctrl), "b", ctrl);
    expect(ids(some)).toEqual(["a", "b"]);

    const after = clickSelection(ORDER, some, "e", plain);
    expect(ids(after)).toEqual([]);
    expect(after.open).toBe(true);
  });

  it("falls back to a plain click when the anchor is no longer on screen", () => {
    // Typing in the search box can take the anchor out of the list. A range
    // with one end missing is not a range, and guessing at it would select
    // rows nobody pointed at.
    const orphaned: SelectionState = { selected: new Set(["b"]), anchor: "zz", open: false };
    const next = clickSelection(ORDER, orphaned, "d", shift);
    expect(ids(next)).toEqual([]);
    expect(next.open).toBe(true);
    expect(next.anchor).toBe("d");
  });

  it("treats a shift click with no anchor at all as a plain one", () => {
    const next = clickSelection(ORDER, NOTHING, "c", shift);
    expect(next.open).toBe(true);
    expect(ids(next)).toEqual([]);
  });
});

describe("pruneSelection", () => {
  it("drops ids that are no longer on screen", () => {
    const pruned = pruneSelection(["a", "b"], new Set(["a", "b", "z"]));
    expect([...pruned].sort()).toEqual(["a", "b"]);
  });

  it("returns the same set when nothing changed, so no render is forced", () => {
    const selected = new Set(["a", "b"]);
    expect(pruneSelection(ORDER, selected)).toBe(selected);
  });

  it("leaves an empty selection alone", () => {
    const empty = new Set<string>();
    expect(pruneSelection([], empty)).toBe(empty);
  });
});
