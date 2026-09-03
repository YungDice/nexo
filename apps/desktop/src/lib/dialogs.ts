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
  /**
   * How many times this exact notice has been asked for.
   *
   * Always at least 1. Above that, the same thing happened again while it was
   * still on screen — pressing Refresh five times is one notice that counts,
   * not five notices in a column.
   */
  repeats: number;
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
 * How many toasts may be on screen at once.
 *
 * Three, because the fourth is already off the bottom of anything worth
 * reading and the point of a toast is that it is read in passing.
 */
const TOAST_MAX = 3;

/**
 * Shows something and resolves when it has been dealt with.
 *
 * An `info` resolves immediately: nothing downstream should wait on a message
 * whose only purpose is to be read. A `confirm` resolves with the answer.
 */
export function requestDialog(kind: DialogKind, title: string, body: string): Promise<boolean> {
  return new Promise((resolve) => {
    if (kind === "info") {
      // The same message twice is one message that happened twice.
      //
      // Refresh and "check for updates" are buttons people press again when
      // nothing appears to happen, and each press used to add a row: five
      // taps, five identical toasts, a column of them covering the thing
      // being refreshed. Saying it once and counting is what a person would
      // do, and it keeps the screen readable while they keep pressing.
      const same = toasts.find((t) => t.title === title && t.body === body);
      if (same) {
        // Replaced rather than incremented in place: `currentToasts` hands
        // `useSyncExternalStore` this array, and that compares snapshots by
        // identity. A count bumped on the object nobody swapped out is a count
        // nothing redraws.
        const counted = { ...same, repeats: same.repeats + 1 };
        toasts = toasts.map((t) => (t.id === same.id ? counted : t));
        // The timer starts again, so the notice outlives the last press
        // rather than the first.
        restartToastTimer(counted);
        emit();
        resolve(true);
        return;
      }

      const entry: DialogRequest = {
        id: nextId++,
        kind,
        title,
        body,
        repeats: 1,
        resolve: () => {},
      };
      // Oldest out first. A cap that dropped the newest would hide the thing
      // that just happened in favour of the thing that already had its turn.
      toasts = [...toasts, entry].slice(-TOAST_MAX);
      restartToastTimer(entry);
      emit();
      resolve(true);
      return;
    }
    // Modals queue rather than stack: two questions at once is one the user
    // cannot see and one they answer without reading.
    queue = [...queue, { id: nextId++, kind, title, body, repeats: 1, resolve }];
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

/**
 * Starts, or restarts, the countdown that takes a toast away.
 *
 * Held per toast so a repeat can push its own deadline back without the
 * earlier timer firing underneath it and dismissing a notice that is still
 * being repeated at.
 */
const timers = new Map<number, ReturnType<typeof setTimeout>>();

function restartToastTimer(entry: DialogRequest): void {
  const existing = timers.get(entry.id);
  if (existing) clearTimeout(existing);
  timers.set(
    entry.id,
    setTimeout(() => dismissToast(entry.id), TOAST_MS),
  );
}

export function dismissToast(id: number): void {
  const timer = timers.get(id);
  if (timer) clearTimeout(timer);
  timers.delete(id);
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
