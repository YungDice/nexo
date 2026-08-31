/**
 * The design tokens, in a form that is not CSS (G3).
 *
 * `tokens.css` is the authored source — it carries the reasoning for every
 * value, and the Windows client consumes it directly through Tailwind's
 * `@theme`. This module reads that same file and produces a plain map, so a
 * second platform can have the values without having to parse CSS or, worse,
 * copy them by hand into a second source of truth that then drifts.
 *
 * The direction matters. Generating the CSS *from* JSON was the other option
 * and it loses the comments, which are the part that says why `--color-danger`
 * is that red and why the surfaces stack the way they do. So the CSS stays
 * authored, the JSON is derived, and a test asserts the checked-in JSON still
 * matches — drift becomes a failing test rather than a discrepancy nobody
 * notices until an Android screen looks subtly wrong.
 */

/** One token: the name as written, minus the leading `--`. */
export interface Token {
  /** e.g. `color-surface-1`. */
  name: string;
  /** The literal value, exactly as authored. */
  value: string;
}

/** Tokens grouped by their prefix — `color`, `radius`, `font`, and so on. */
export type TokenGroups = Record<string, Record<string, string>>;

/**
 * Pulls the custom properties out of a Tailwind `@theme` block.
 *
 * Deliberately a small hand-written scan rather than a CSS parser. The input is
 * one file in this repository whose shape is known, a parser would be a
 * dependency for one job, and — most of all — a scan that does not understand
 * CSS cannot silently succeed on something that is not the `@theme` block.
 */
export function parseTokens(css: string): Token[] {
  const start = css.indexOf("@theme");
  if (start === -1) {
    throw new Error("no @theme block: tokens.css is not in the expected shape");
  }

  const open = css.indexOf("{", start);
  if (open === -1) throw new Error("@theme has no opening brace");

  // Walk to the matching close brace, counting depth, so a nested block does
  // not end the scan early.
  let depth = 0;
  let end = -1;
  for (let i = open; i < css.length; i++) {
    if (css[i] === "{") depth++;
    else if (css[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  if (end === -1) throw new Error("@theme is not closed");

  const body = css.slice(open + 1, end);
  // Comments are stripped first: a commented-out token is not a token, and a
  // value mentioned inside prose is not one either.
  const withoutComments = body.replace(/\/\*[\s\S]*?\*\//g, "");

  const tokens: Token[] = [];
  const seen = new Set<string>();
  const declaration = /--([a-z0-9-]+)\s*:\s*([^;]+);/gi;
  let match: RegExpExecArray | null;
  while ((match = declaration.exec(withoutComments)) !== null) {
    const name = match[1];
    const value = match[2].trim().replace(/\s+/g, " ");
    if (seen.has(name)) {
      // Later wins in CSS, so mirror that rather than emitting a duplicate
      // whose order a consumer would have to guess at.
      tokens[tokens.findIndex((t) => t.name === name)] = { name, value };
      continue;
    }
    seen.add(name);
    tokens.push({ name, value });
  }

  if (tokens.length === 0) {
    throw new Error("@theme block contained no tokens");
  }
  return tokens;
}

/**
 * Groups tokens by their first segment.
 *
 * `--color-surface-1` becomes `color["surface-1"]`. The grouping is what makes
 * the JSON usable on another platform: a colour becomes a colour resource, a
 * radius becomes a dimension, and nothing has to guess from the name.
 */
export function groupTokens(tokens: Token[]): TokenGroups {
  const groups: TokenGroups = {};
  for (const { name, value } of tokens) {
    const dash = name.indexOf("-");
    const [group, rest] =
      dash === -1 ? [name, name] : [name.slice(0, dash), name.slice(dash + 1)];
    (groups[group] ??= {})[rest] = value;
  }
  return groups;
}
