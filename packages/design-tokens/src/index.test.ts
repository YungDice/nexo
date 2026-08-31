import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { describe, expect, it } from "vitest";

import { groupTokens, parseTokens } from "./index.js";

const here = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(join(here, "..", "tokens.css"), "utf8");

describe("parseTokens", () => {
  it("finds the tokens in the real file", () => {
    const tokens = parseTokens(css);
    expect(tokens.length).toBeGreaterThan(20);
    // A few that every screen depends on. If a rename ever silently drops one,
    // the app renders with a missing custom property and no error anywhere.
    const names = tokens.map((t) => t.name);
    for (const required of ["color-accent", "color-surface-0", "color-text-hi"]) {
      expect(names).toContain(required);
    }
  });

  it("ignores tokens that are only mentioned in a comment", () => {
    const parsed = parseTokens(`@theme {
      /* --color-old: #000; was the previous accent */
      --color-accent: #7b5cfa;
    }`);
    expect(parsed).toEqual([{ name: "color-accent", value: "#7b5cfa" }]);
  });

  it("takes the last value when a token is declared twice, as CSS does", () => {
    const parsed = parseTokens(`@theme { --a: 1px; --a: 2px; }`);
    expect(parsed).toEqual([{ name: "a", value: "2px" }]);
  });

  it("refuses a file that is not in the expected shape", () => {
    // Better to fail loudly than to emit an empty token set that a consumer
    // would read as "this platform has no colours".
    expect(() => parseTokens("body { color: red }")).toThrow(/no @theme/);
    expect(() => parseTokens("@theme { }")).toThrow(/no tokens/);
    expect(() => parseTokens("@theme {")).toThrow(/no opening brace|not closed/);
  });

  it("does not stop at a nested block's closing brace", () => {
    const parsed = parseTokens(`@theme {
      --a: 1px;
      @media (prefers-reduced-motion) { --b: 0ms; }
      --c: 3px;
    }`);
    expect(parsed.map((t) => t.name)).toEqual(["a", "b", "c"]);
  });
});

describe("groupTokens", () => {
  it("groups by the first segment", () => {
    expect(
      groupTokens([
        { name: "color-surface-1", value: "#131315" },
        { name: "radius-window", value: "12px" },
      ]),
    ).toEqual({
      color: { "surface-1": "#131315" },
      radius: { window: "12px" },
    });
  });
});

/**
 * The surfaces `useChrome`'s depth slider rebuilds at runtime, and the rule
 * that makes that possible.
 *
 * Kept in step with `SURFACE_TOKENS` in `apps/desktop/src/app/useChrome.ts` by
 * hand, because this package cannot import from the app.
 */
const RUNTIME_SURFACES = [
  "color-void",
  "color-surface-0",
  "color-surface-1",
  "color-surface-2",
  "color-surface-3",
] as const;

/** The body of the first top-level `:root { ... }` rule, comments stripped. */
function plainRootBlock(source: string): string {
  const match = /\n:root \{\n([\s\S]*?)\n\}/.exec(source);
  if (!match) throw new Error("no plain :root block in tokens.css");
  return match[1].replace(/\/\*[\s\S]*?\*\//g, "");
}

describe("the surfaces the depth slider rewrites", () => {
  // What went wrong, so the next person does not undo it:
  //
  // The slider sets `--color-surface-2` to a `color-mix` of
  // `--color-surface-2-base` and black. Those `-base` twins were declared in
  // `@theme` — and Tailwind prunes any `@theme` variable no used utility class
  // needs. Nobody writes `bg-surface-2-base`, so four of the five never
  // reached the built stylesheet. With one `var()` undefined the whole
  // `color-mix` is invalid at computed-value time, which takes every
  // declaration reading it down to `initial`: menus, the modal dialog and
  // every glass pane lost their background the moment the slider left zero.
  //
  // The old tests read this same file and stayed green throughout, because in
  // the *source* the tokens were there. What was broken was where they lived.
  const theme = new Set(parseTokens(css).map((t) => t.name));
  const root = plainRootBlock(css);

  for (const name of RUNTIME_SURFACES) {
    it(`declares ${name}-base outside @theme, where Tailwind cannot prune it`, () => {
      expect(theme.has(`${name}-base`)).toBe(false);
      expect(root).toContain(`--${name}-base:`);
    });

    it(`still exposes ${name} itself as a theme colour`, () => {
      // The live half has to stay in `@theme`: that is what generates
      // `bg-surface-2` and friends in the first place.
      expect(theme.has(name)).toBe(true);
    });
  }

  it("gives every light-theme override its own base, not the dark one", () => {
    // Redeclaring only some of them is the same bug wearing a different hat:
    // a light theme with the slider up would mix the rest from the dark
    // palette toward white.
    for (const selector of [/:root:not\(\[data-theme="dark"\]\)\s*\{([\s\S]*?)\n {2}\}/,
                            /:root\[data-theme="light"\]\s*\{([\s\S]*?)\n\}/]) {
      const block = selector.exec(css);
      expect(block, `no block matching ${selector}`).not.toBeNull();
      const body = block![1];
      for (const name of RUNTIME_SURFACES) {
        expect(body).toContain(`--${name}-base:`);
      }
    }
  });
});

describe("tokens.json", () => {
  it("is up to date with tokens.css", () => {
    // The check that makes the JSON safe to consume from another platform: it
    // cannot drift from the CSS without this failing.
    const checkedIn = JSON.parse(
      readFileSync(join(here, "..", "tokens.json"), "utf8"),
    ) as Record<string, unknown>;
    const { $comment, ...groups } = checkedIn;
    expect($comment).toBeTypeOf("string");
    expect(groups).toEqual(groupTokens(parseTokens(css)));
  });
});
