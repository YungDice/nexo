/**
 * In-app dialogs and toasts.
 *
 * The app is a frameless window with its own chrome, and an OS message box
 * dropped on top of it belongs to a different application as far as the eye is
 * concerned: different corners, different type, its own taskbar entry, and it
 * steals focus from a window that was already in front. Everything the app has
 * to say, it says inside itself.
 *
 * Deliberately free of React so `lib/native.ts` — which the sync agent imports
 * outside any component — can call into it. The host component subscribes.
 *
 * This is *not* where a Windows toast for an incoming message goes. That one is
 * an OS notification on purpose: it has to be visible when the app is not.
 */

/** `info` is a transient toast. `confirm` is a modal with two answers. */
export type DialogKind = "info" | "confirm";

export interface DialogRequest {
  id: number;
  kind: DialogKind;
  title: string;
  body: string;
  /** Resolves the promise the caller is awaiting. `true` means confirmed. */
  resolve: (ok: boolean) => void;
}

let queue: DialogRequest[] = [];
let toasts: DialogRequest[] = [];
const listeners = new Set<() => void>();
let nextId = 1;

function emit(): void {
  for (const listener of listeners) listener();
}

/** How long an informational toast stays up. */
const TOAST_MS = 5000;

/**
 * Shows something and resolves when it has been dealt with.
 *
 * An `info` resolves immediately: nothing downstream should wait on a message
 * whose only purpose is to be read. A `confirm` resolves with the answer.
 */
export function requestDialog(kind: DialogKind, title: string, body: string): Promise<boolean> {
  return new Promise((resolve) => {
    if (kind === "info") {
      const entry: DialogRequest = { id: nextId++, kind, title, body, resolve: () => {} };
      toasts = [...toasts, entry];
      emit();
      setTimeout(() => dismissToast(entry.id), TOAST_MS);
      resolve(true);
      return;
    }
    // Modals queue rather than stack: two questions at once is one the user
    // cannot see and one they answer without reading.
    queue = [...queue, { id: nextId++, kind, title, body, resolve }];
    emit();
  });
}

/** The modal awaiting an answer, if any. */
export function currentModal(): DialogRequest | undefined {
  return queue[0];
}

/** Every toast currently on screen, oldest first. */
export function currentToasts(): readonly DialogRequest[] {
  return toasts;
}

/** Answers the front modal and moves to the next. */
export function answerModal(ok: boolean): void {
  const [front, ...rest] = queue;
  if (!front) return;
  queue = rest;
  front.resolve(ok);
  emit();
}

export function dismissToast(id: number): void {
  const before = toasts.length;
  toasts = toasts.filter((t) => t.id !== id);
  if (toasts.length !== before) emit();
}

export function subscribeDialogs(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
