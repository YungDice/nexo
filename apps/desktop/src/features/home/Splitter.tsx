import { useCallback, useRef, type KeyboardEvent, type PointerEvent } from "react";

/**
 * How narrow the conversation panel on Home may get.
 *
 * A composer, an avatar and a name have to fit on one row above a message
 * bubble that is still a bubble rather than a column of single words. Below
 * this the panel stops being a conversation and becomes a reminder that one
 * exists.
 */
export const MIN_HOME_CHAT = 280;

/**
 * How narrow the feed may get before the splitter stops giving ground.
 *
 * The feed is what Home is for: a post card with an avatar, a title and a body
 * needs this much before the words start breaking every three of them.
 */
export const MIN_HOME_FEED = 420;

/**
 * The draggable line between two panes.
 *
 * Home had three vertical rules across it: the reading column drew one on each
 * side of itself, and the conversation panel drew a third. Three lines to
 * express one division, and none of them meant anything you could act on.
 * This is the one line, and it is the control: drag it and the feed and the
 * conversation trade width.
 *
 * # What it owns, and what it does not
 *
 * Nothing. The width lives with the caller, because the caller is the one that
 * has to persist it and render both sides. This measures the gesture and
 * reports a number.
 *
 * Two numbers, in fact, and the difference matters. `onResize` fires on every
 * pointer move and is for the width *now* — Home answers it by writing a
 * custom property onto the row, so nothing re-renders while the pointer is
 * down. `onCommit` fires once, when the gesture ends, and is where the
 * preference is written. Doing that on every frame instead would mean a React
 * render of the whole feed plus a `JSON.stringify` and a `localStorage` write
 * per frame, which is exactly the kind of synchronous work that makes a drag
 * feel like it is catching on something.
 *
 * # The bounds
 *
 * Measured at the start of each gesture rather than passed in, because the
 * ceiling is "whatever is left after the other pane keeps `minOther`" and that
 * changes every time the window is resized. Reading the parent's width once
 * per gesture is cheap and always current; a prop would be stale the moment
 * someone dragged the window edge.
 */
export function Splitter({
  width,
  min,
  minOther,
  onResize,
  onCommit,
  label,
}: {
  /** The current width of the pane on the *right* of this line, in px. */
  width: number;
  /** How narrow that pane may get. */
  min: number;
  /** How narrow the pane on the left may get. */
  minOther: number;
  /** Called continuously during a drag. */
  onResize: (next: number) => void;
  /** Called once, when the gesture ends. */
  onCommit: (next: number) => void;
  label: string;
}) {
  const drag = useRef<{ x: number; width: number; max: number } | null>(null);

  const boundsFrom = useCallback(
    (el: HTMLElement) => {
      const row = el.parentElement?.getBoundingClientRect().width ?? 0;
      // Never below `min`: on a window too narrow for both, the clamp has to
      // resolve to something rather than invert.
      return Math.max(min, row - minOther);
    },
    [min, minOther],
  );

  const clamp = (next: number, max: number) => Math.min(max, Math.max(min, Math.round(next)));

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    // Left button only. A right-click here belongs to whatever context menu
    // the platform wants to show, not to a resize.
    if (event.button !== 0) return;
    const el = event.currentTarget;
    drag.current = { x: event.clientX, width, max: boundsFrom(el) };
    el.setPointerCapture(event.pointerId);
    // The cursor has to survive leaving the handle, and the selection has to
    // be suppressed for the whole gesture — a drag to the left across the feed
    // would otherwise select every post it passed over.
    document.documentElement.dataset["resizing"] = "col";
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (!start) return;
    // Leftward drag widens the right-hand pane, so the delta is subtracted.
    onResize(clamp(start.width - (event.clientX - start.x), start.max));
  };

  const endDrag = (event: PointerEvent<HTMLDivElement>) => {
    const start = drag.current;
    if (!start) return;
    drag.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
    delete document.documentElement.dataset["resizing"];
    onCommit(clamp(start.width - (event.clientX - start.x), start.max));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const max = boundsFrom(event.currentTarget);
    // A separator that can only be dragged is a separator half the people who
    // use this app cannot move (§7.4).
    const step = event.shiftKey ? 64 : 16;
    let next: number | null = null;
    if (event.key === "ArrowLeft") next = width + step;
    else if (event.key === "ArrowRight") next = width - step;
    else if (event.key === "Home") next = max;
    else if (event.key === "End") next = min;
    if (next === null) return;
    event.preventDefault();
    onCommit(clamp(next, max));
  };

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={Math.round(width)}
      aria-valuemin={min}
      tabIndex={0}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onKeyDown={onKeyDown}
      // 9px of hit area pulled back to 1px of layout by the negative margin:
      // a 1px grab target is a fine line and a bad control, but a 9px column
      // between two panes is a visible gap.
      className="group relative z-10 -mx-1 w-[9px] shrink-0 cursor-col-resize touch-none"
    >
      <span
        aria-hidden="true"
        className="pointer-events-none absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-[var(--hairline)] transition-colors duration-[var(--motion-fast)] ease-[var(--ease-state)] group-hover:bg-accent/70 group-focus-visible:bg-accent"
      />
    </div>
  );
}
