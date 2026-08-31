/**
 * Deterministic colour from a string.
 *
 * The CSP allows images from 'self', asset:, data: and blob: only — no remote
 * host, by design (§4.5). So an account with no uploaded avatar is drawn, not
 * fetched: same input, same colours, every run.
 *
 * The line this file sits on: **content may carry colour, the interface may
 * not.** An avatar, a photo and a file mark are things a person put there, and
 * stripping them to grey makes the app harder to scan, not calmer. Surfaces,
 * lines, rails and headers stay neutral, and the only colour the interface
 * itself spends is the accent — or a status reporting something.
 *
 * The hues are still a fixed set rather than a slice of the whole wheel.
 * Hashing into 360° gives every third avatar a different primary and a sidebar
 * of eight ends up looking like a colour picker.
 */

/** FNV-1a, 32-bit. Not a hash for security — a hash for picking a hue. */
export function hashString(value: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < value.length; i += 1) {
    hash ^= value.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return hash;
}

/** Violet, indigo, teal, rose, amber. One family, five members. */
const HUES = [258, 222, 178, 340, 30];

function hueFor(seed: string): number {
  return HUES[hashString(seed) % HUES.length] ?? HUES[0]!;
}

/**
 * An avatar: two stops, 26° apart, at a saturation under the 80% cap and a
 * lightness that keeps white initials above 4.5:1 — in both themes, since an
 * avatar is the same object on a white page as on a black one.
 */
export function gradientFor(seed: string): string {
  const hue = hueFor(seed);
  return `linear-gradient(140deg, hsl(${hue} 52% 48%), hsl(${(hue + 26) % 360} 56% 34%))`;
}

/**
 * Banners and media placeholders. Lighter and softer than an avatar, because
 * these stand in for photographs rather than for a person.
 */
export function fieldFor(seed: string): string {
  const hash = hashString(seed);
  const hue = hueFor(seed);
  const second = HUES[(HUES.indexOf(hue) + 1 + (hash % 3)) % HUES.length] ?? hue;
  return [
    `radial-gradient(85% 105% at 20% 4%, hsl(${hue} 66% 62%), transparent 70%)`,
    `radial-gradient(95% 95% at 86% 34%, hsl(${second} 62% 50%), transparent 68%)`,
    `hsl(${hue} 38% 34%)`,
  ].join(", ");
}

/**
 * The mark on a file tile.
 *
 * The colour is the point: it is how you find the PDF in a column of six
 * without reading a single filename. The tint carries the type and the letters
 * stay in the theme's own text colour, so the label is legible on a white page
 * and a black one alike rather than needing a second set of values.
 */
export function fileTone(name: string): { tint: string; label: string } {
  const extension = name.slice(name.lastIndexOf(".") + 1).toLowerCase();
  const tints: Record<string, string> = {
    pdf: "rgba(240, 66, 107, 0.22)",
    sketch: "rgba(245, 165, 36, 0.22)",
    fig: "rgba(162, 139, 255, 0.24)",
    png: "rgba(61, 214, 140, 0.22)",
    jpg: "rgba(61, 214, 140, 0.22)",
    zip: "rgba(123, 92, 250, 0.24)",
  };
  return {
    tint: tints[extension] ?? "rgba(140, 140, 160, 0.18)",
    label: extension.slice(0, 4).toUpperCase(),
  };
}
