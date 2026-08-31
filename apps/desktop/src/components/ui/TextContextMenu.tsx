import { useCallback, useEffect, useState } from "react";

import { ContextMenu, type MenuItem } from "./ContextMenu";
import { copyText, pasteText } from "../../lib/native";

/** The kinds of field this menu knows how to act on. */
type Editable = HTMLInputElement | HTMLTextAreaElement;

function asEditable(target: EventTarget | null): Editable | null {
  const el = target as HTMLElement | null;
  const field = el?.closest?.("input, textarea") as Editable | null | undefined;
  if (!field) return null;
  // Buttons, checkboxes and the like are `<input>` too, and none of them has
  // text to cut.
  if (field instanceof HTMLInputElement) {
    const type = field.type.toLowerCase();
    const textual = ["text", "search", "url", "tel", "email", "password", "number"];
    if (!textual.includes(type)) return null;
  }
  return field;
}

function selectionOf(field: Editable): { start: number; end: number; text: string } {
  const start = field.selectionStart ?? 0;
  const end = field.selectionEnd ?? 0;
  return { start, end, text: start === end ? "" : field.value.slice(start, end) };
}

/**
 * Puts the caret back where it was, then acts.
 *
 * Clicking a menu entry focuses the menu's button, so by the time an action
 * runs the field is no longer the active element -- and `execCommand` acts on
 * whatever is. Worse, `copyText` is async, so the rest of a Cut lands a
 * microtask later, after the menu has unmounted and focus has fallen to the
 * body. Restoring both the focus and the range first is what makes the
 * commands land on the field the person right-clicked.
 */
function onField(field: Editable, start: number, end: number, run: () => void): void {
  field.focus();
  try {
    field.setSelectionRange(start, end);
  } catch {
    // A number input refuses `setSelectionRange`. The focus above is still
    // worth having, and paste into one works without a range.
  }
  run();
}

/**
 * The app's own right-click menu inside text fields.
 *
 * # Why the browser's menu had to go
 *
 * `main.tsx` suppresses Chromium's context menu everywhere, and text fields
 * used to be the one exception: cut, copy and paste are real functionality,
 * and taking them away to look tidy costs more than it gains. That reasoning
 * was sound until the window became transparent. A menu the *operating system*
 * draws is not part of the document, gets no background from it, and over a
 * transparent window it comes out as floating text with the desktop showing
 * through — unreadable, and unmistakably not part of the app.
 *
 * So the exception is gone and this takes its place: the same four actions, on
 * the app's own opaque surface, in the app's own type.
 *
 * # Why one listener at the root rather than a prop on every field
 *
 * The same reason `main.tsx` suppresses globally: a field that forgot to opt
 * in would fall back to nothing at all, and there is no way to notice that in
 * review. Mounted once, it covers every `<input>` and `<textarea>` in the app,
 * including ones nobody has written yet.
 *
 * # Undo still works
 *
 * Cut and paste go through `execCommand`, which is deprecated and still the
 * only thing that edits a field the way the browser does: it fires a real
 * `input` event (so React sees it) and it lands on the native undo stack (so
 * Ctrl+Z takes it back). Setting `.value` directly does neither.
 */
export function TextContextMenu() {
  const [at, setAt] = useState<{ x: number; y: number } | null>(null);
  const [items, setItems] = useState<MenuItem[]>([]);

  const close = useCallback(() => setAt(null), []);

  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      const field = asEditable(event.target);
      if (!field) return;
      event.preventDefault();

      // Right-clicking a field does not focus it on its own, and every action
      // below acts on the focused element.
      field.focus();

      const { start, end, text: selected } = selectionOf(field);
      const editable = !field.readOnly && !field.disabled;
      const isPassword =
        field instanceof HTMLInputElement && field.type.toLowerCase() === "password";

      const next: MenuItem[] = [];
      // A password field is exactly where the clipboard should not be offered
      // a shortcut. The browser refuses to copy from one; so does this.
      if (selected && !isPassword) {
        if (editable) {
          next.push({
            label: "Cut",
            icon: "scissors",
            onSelect: () => {
              void copyText(selected).then(() =>
                onField(field, start, end, () => document.execCommand("delete")),
              );
            },
          });
        }
        next.push({
          label: "Copy",
          icon: "copy",
          onSelect: () => void copyText(selected),
        });
      }
      if (editable) {
        next.push({
          label: "Paste",
          icon: "clipboard",
          onSelect: () => {
            void pasteText().then((text) => {
              if (!text) return;
              // Over the selection when there is one, which is what paste
              // means everywhere else.
              onField(field, start, end, () =>
                document.execCommand("insertText", false, text),
              );
            });
          },
        });
      }
      if (field.value.length > 0) {
        next.push({
          // No icon: a tick here would read as "already selected".
          label: "Select all",
          onSelect: () => onField(field, 0, field.value.length, () => {}),
        });
      }

      // Nothing to offer -- an empty read-only field -- means no menu, rather
      // than a box of greyed-out entries.
      if (next.length === 0) return;
      setItems(next);
      setAt({ x: event.clientX, y: event.clientY });
    };

    document.addEventListener("contextmenu", onContextMenu);
    return () => document.removeEventListener("contextmenu", onContextMenu);
  }, []);

  if (at === null) return null;
  return <ContextMenu items={items} at={at} onClose={close} />;
}
