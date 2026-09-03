import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { currentToasts, dismissToast, requestDialog } from "./dialogs";

/**
 * Toasts that stop stacking.
 *
 * Refresh and "check for updates" are buttons people press again when nothing
 * visibly happens, and every press used to add another identical row until the
 * corner of the window was a column of the same sentence. What these check is
 * the two rules that stop it: the same notice counts instead of repeating, and
 * a run of different ones is capped.
 */
describe("toasts", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    // The store is module-level, so a test that leaves a toast up leaks it into
    // the next one. Running the clock past the dismissal timer empties it.
    vi.runOnlyPendingTimers();
    for (const toast of [...currentToasts()]) dismissToast(toast.id);
    vi.useRealTimers();
  });

  it("counts a repeat instead of adding a row", async () => {
    await requestDialog("info", "Feed", "Nothing new.");
    await requestDialog("info", "Feed", "Nothing new.");
    await requestDialog("info", "Feed", "Nothing new.");

    expect(currentToasts()).toHaveLength(1);
    expect(currentToasts()[0]?.repeats).toBe(3);
  });

  it("replaces the entry rather than mutating it", async () => {
    // `useSyncExternalStore` compares snapshots by identity. A count bumped on
    // an object still sitting in the same array is a count nothing redraws, so
    // this is the assertion that the badge appears at all.
    await requestDialog("info", "Feed", "Nothing new.");
    const before = currentToasts();
    await requestDialog("info", "Feed", "Nothing new.");

    expect(currentToasts()).not.toBe(before);
    expect(currentToasts()[0]).not.toBe(before[0]);
  });

  it("gives a repeat the full time again", async () => {
    await requestDialog("info", "Feed", "Nothing new.");
    vi.advanceTimersByTime(4000);
    await requestDialog("info", "Feed", "Nothing new.");

    // The first toast's own deadline would have passed here. It has not, because
    // the repeat pushed it back -- a notice should outlive the last press.
    vi.advanceTimersByTime(2000);
    expect(currentToasts()).toHaveLength(1);

    vi.advanceTimersByTime(4000);
    expect(currentToasts()).toHaveLength(0);
  });

  it("keeps the newest three when the messages differ", async () => {
    await requestDialog("info", "One", "a");
    await requestDialog("info", "Two", "b");
    await requestDialog("info", "Three", "c");
    await requestDialog("info", "Four", "d");

    // Oldest out. Dropping the newest would hide the thing that just happened
    // in favour of the thing that already had its turn.
    expect(currentToasts().map((t) => t.title)).toEqual(["Two", "Three", "Four"]);
  });

  it("treats a different body as a different notice", async () => {
    await requestDialog("info", "Update", "Version 0.1.19 is ready.");
    await requestDialog("info", "Update", "You are up to date.");

    expect(currentToasts()).toHaveLength(2);
  });
});
