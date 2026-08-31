/**
 * The Nexo mark: an N and a full stop, drawn as paths rather than typeset.
 *
 * It used to be a `<span>` reading `N.` in `--font-display` at 19px bold. That
 * looked right on a machine with Inter installed and wrong everywhere else,
 * because this repo bundles no fonts (rule 3: nothing is fetched at runtime,
 * and no webfont was ever added to make up for it). `--font-display` therefore
 * falls through Inter to Segoe UI on a stock Windows box, and the mark — the
 * one place the product's identity shows in the chrome — changed shape with
 * it. A path does not.
 *
 * Drawn on a cap-height grid of 24 units, matching how the icon set is built,
 * but filled rather than stroked: this is a letterform, not a pictogram, so it
 * does not belong in `Icon`'s 1.75px stroke idiom.
 *
 * Geometry, so a later change stays consistent rather than eyeballed:
 *
 *   cap height    24        the grid
 *   stem          3.7       both verticals
 *   diagonal      4.6       measured horizontally, so the two edges are
 *                           exactly parallel: (0,0)->(13.4,24) and
 *                           (4.6,0)->(18,24)
 *   joins         y=6.63 on the left stem, y=17.37 on the right; both fall
 *                 out of the geometry above and are symmetric about the
 *                 centre, which is what keeps the counters even
 *   full stop     r 1.9, so its diameter reads as one stem, sitting on the
 *                 baseline 1.6 to the right of the N
 *
 * The N takes `currentColor` and the stop takes the accent, which is exactly
 * the split the old two-span markup had.
 */

import type { SVGProps } from "react";

/** 23.5 wide over 24 of cap height — the mark's own box, no padding. */
const ASPECT = 23.5 / 24;

export interface BrandMarkProps extends SVGProps<SVGSVGElement> {
  /** Cap height in px. 14 is what the 19px display text it replaced measured. */
  size?: number;
}

export function BrandMark({ size = 14, ...rest }: BrandMarkProps) {
  return (
    <svg
      width={Math.round(size * ASPECT * 100) / 100}
      height={size}
      viewBox="0 0 23.5 24"
      fill="none"
      role="img"
      aria-label="Nexo"
      focusable="false"
      {...rest}
    >
      <path fill="currentColor" d="M0 0H4.6L14.3 17.37V0H18V24H13.4L3.7 6.63V24H0Z" />
      <circle cx="21.6" cy="22.1" r="1.9" fill="var(--color-accent)" />
    </svg>
  );
}
