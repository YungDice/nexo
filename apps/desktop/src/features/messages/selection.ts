/**
 * What a click does to a multi-selection.
 *
 * Pure and on its own because the rules are the part that goes wrong. Ctrl and
 * Shift selection is thirty years old and everybody has an opinion about the
 * edge cases -- what Shift measures from, whether Ctrl moves that point,
 * whether a plain click keeps anything -- and none of that is visible in a
 * component that also renders avatars. Here it can be read in one screen and
 * tested without a DOM.
 */

/** Which modifier keys were held, as much as the rules care about. */
export interface Modifiers {
  /** Ctrl on Windows, and Meta so a Mac keyboard behaves the same way. */
  toggle: boolean;
  /** Shift: take everything between the anchor and this one. */
  range: boolean;
}

export interface SelectionState {
  /** The selected ids. Order is not meaningful. */
  selected: ReadonlySet<string>;
  /**
   * What a range selection measures from.
   *
   * Moved by a plain or a toggling click, never by a range click: a second
   * Shift click has to grow or shrink the same range, and moving the anchor
   * would make it extend from the wrong end.
   */
  anchor: string | null;
  /**
   * Whether this click should also open the conversation.
   *
   * A plain click is still navigation first. Only the modified ones are purely
   * about selecting, which is why a selection cannot happen by accident.
   */
  open: boolean;
}

/**
 * The state after clicking `id`, given the rows currently on screen in the
 * order they are drawn.
 *
 * `order` is the *visible* order, which is what makes Shift mean what people
 * expect: the range is what lies between the two rows on screen, not what lies
 * between them in some underlying list they cannot see.
 */
export function clickSelection(
  order: readonly string[],
  current: SelectionState,
  id: string,
  modifiers: Modifiers,
): SelectionState {
  if (modifiers.range && current.anchor !== null) {
    const from = order.indexOf(current.anchor);
    const to = order.indexOf(id);
    if (from !== -1 && to !== -1) {
      const [lo, hi] = from < to ? [from, to] : [to, from];
      return {
        selected: new Set(order.slice(lo, hi + 1)),
        anchor: current.anchor,
        open: false,
      };
    }
    // The anchor has scrolled out of the filter, or was removed. Falling
    // through to a plain click is better than selecting nothing and better
    // than guessing at a range that no longer has two ends.
  }

  if (modifiers.toggle) {
    const selected = new Set(current.selected);
    if (selected.has(id)) selected.delete(id);
    else selected.add(id);
    return { selected, anchor: id, open: false };
  }

  // A plain click drops the selection. Keeping one alive across an ordinary
  // click is how a bulk action ends up applying to something the person had
  // forgotten was selected.
  return { selected: new Set(), anchor: id, open: true };
}

/**
 * The selection with everything that is no longer on screen removed.
 *
 * Returns the same object when nothing changed, so it can be used directly in
 * a state setter without causing a render every time the list re-sorts.
 */
export function pruneSelection(
  order: readonly string[],
  selected: ReadonlySet<string>,
): ReadonlySet<string> {
  if (selected.size === 0) return selected;
  const visible = new Set(order);
  const next = new Set([...selected].filter((id) => visible.has(id)));
  return next.size === selected.size ? selected : next;
}
