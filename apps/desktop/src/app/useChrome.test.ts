import { describe, expect, it } from "vitest";

import { fieldVeil } from "./useChrome";

/**
 * The transparency slider decides how much of someone's wallpaper ends up
 * behind their messages, which makes it the one appearance setting that can
 * make the app unreadable. The floor is the point of these tests.
 */
describe("fieldVeil", () => {
  it("keeps a floor under the field, however far the slider goes", () => {
    // A veil that could reach zero would put message bubbles straight onto an
    // arbitrary photograph. The floor moved from 50% to 20% when the effect
    // was reported as too weak -- the number that has to stay non-zero is this
    // one, but the legibility it protects comes mostly from the pane above it,
    // which blurs the wallpaper before any text is drawn over it.
    for (let strength = 0; strength <= 1; strength += 0.05) {
      const percent = Number.parseInt(fieldVeil(strength), 10);
      expect(percent).toBeGreaterThanOrEqual(20);
      expect(percent).toBeLessThanOrEqual(100);
    }
  });

  it("gets more transparent as the slider goes up", () => {
    expect(Number.parseInt(fieldVeil(0), 10)).toBeGreaterThan(
      Number.parseInt(fieldVeil(1), 10),
    );
  });

  it("clamps a value from outside the slider's range", () => {
    // The preference is persisted JSON: an older build, a hand-edited store or
    // a future range all reach this function, and none of them may produce a
    // percentage CSS would reject.
    expect(fieldVeil(-5)).toBe(fieldVeil(0));
    expect(fieldVeil(99)).toBe(fieldVeil(1));
  });

  it("emits a percentage, because color-mix takes nothing else", () => {
    expect(fieldVeil(0.5)).toMatch(/^\d+%$/);
  });
});
