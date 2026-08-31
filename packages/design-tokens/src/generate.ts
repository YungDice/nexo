/**
 * Regenerates `tokens.json` from `tokens.css`.
 *
 * Run with `pnpm --filter @nexo/design-tokens build:tokens`. The output is
 * checked in, and `index.test.ts` fails if it is stale — so an Android build
 * consuming the JSON can never be reading values the CSS abandoned.
 */
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

import { groupTokens, parseTokens } from "./index.js";

const here = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(join(here, "..", "tokens.css"), "utf8");
const groups = groupTokens(parseTokens(css));

const out = {
  $comment:
    "Generated from tokens.css by src/generate.ts. Do not edit by hand -- edit the CSS, which carries the reasoning, and regenerate.",
  ...groups,
};

writeFileSync(join(here, "..", "tokens.json"), `${JSON.stringify(out, null, 2)}\n`, "utf8");
console.log(`wrote ${Object.values(groups).reduce((n, g) => n + Object.keys(g).length, 0)} tokens`);
