/**
 * The arithmetic behind zooming and dragging a picture.
 *
 * Pure and separate from the viewer, because this is the part that is easy to
 * get subtly wrong and impossible to check by looking: an off-by-one in the
 * clamp is a picture that drifts a few pixels every time you let go, and a
 * wrong sign in the zoom-about is a picture that runs away from the cursor.
 * Neither shows up as an error. Both show up as "the viewer feels broken".
 */

export interface Point {
  x: number;
  y: number;
}

/** No pan at all: the picture sits centred. */
export const CENTRED: Point = { x: 0, y: 0 };

/**
 * How far the picture may be dragged before it would leave the frame.
 *
 * The picture is centred and scaled about its own middle, so the amount that
 * hangs off each side is half the difference between the scaled size and the
 * frame. When the scaled picture is *smaller* than the frame there is nothing
 * hanging off and the answer is zero — dragging a picture that already fits is
 * the thing that makes a viewer feel loose.
 */
export function panBounds(scaled: Point, frame: Point): Point {
  return {
    x: Math.max(0, (scaled.x - frame.x) / 2),
    y: Math.max(0, (scaled.y - frame.y) / 2),
  };
}

/** Holds a pan inside those bounds. */
export function clampPan(pan: Point, scaled: Point, frame: Point): Point {
  const bounds = panBounds(scaled, frame);
  return {
    x: Math.min(bounds.x, Math.max(-bounds.x, pan.x)),
    y: Math.min(bounds.y, Math.max(-bounds.y, pan.y)),
  };
}

/**
 * The pan that keeps the point under the cursor under the cursor.
 *
 * Zooming about the middle is the easy version and it is the wrong one: you
 * point at a face, zoom, and the face leaves the screen. `at` is the cursor
 * relative to the centre of the frame — the same coordinates the pan is in.
 *
 * The derivation, once, so the next person does not have to redo it: the point
 * of the picture under the cursor is `(at - pan) / from`. For it to still be
 * under the cursor at `to`, the new pan must satisfy
 * `at - pan' = (at - pan) * to / from`, which rearranges to the line below.
 */
export function zoomAbout(at: Point, pan: Point, from: number, to: number): Point {
  const ratio = to / from;
  return {
    x: at.x - (at.x - pan.x) * ratio,
    y: at.y - (at.y - pan.y) * ratio,
  };
}

/** Keeps a zoom inside the range, at a sane number of decimal places. */
export function clampZoom(zoom: number, min: number, max: number): number {
  // Rounded because a wheel emits fractional deltas and an unrounded zoom
  // reaches 99.99999% instead of 100%, which the readout then displays.
  return Math.min(max, Math.max(min, Math.round(zoom * 1000) / 1000));
}
