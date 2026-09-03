import { useSyncExternalStore } from "react";
import { createPortal } from "react-dom";

import {
  answerModal,
  currentModal,
  currentToasts,
  dismissToast,
  subscribeDialogs,
} from "../../lib/dialogs";
import { Button, IconButton } from "./Button";
import { Icon } from "./Icon";

/**
 * Where everything the app has to say is drawn.
 *
 * Mounted once, above the shell. Toasts stack in the corner and leave on their
 * own; a confirm is modal because it is a question, and a question that can be
 * ignored is one the code has to guess the answer to.
 */
export function DialogHost() {
  const modal = useSyncExternalStore(subscribeDialogs, currentModal, () => undefined);
  const toasts = useSyncExternalStore(subscribeDialogs, currentToasts, () => EMPTY);

  // Portalled for the same reason `Modal` is: `fixed` resolves against the
  // nearest transformed ancestor, and a toast rendered inside one lands inside
  // it rather than in the corner of the window.
  return createPortal(
    <>
      {toasts.length > 0 ? (
        // z-index contract, §7.3: overlays sit at 200.
        <div
          className="no-drag pointer-events-none fixed right-4 bottom-4 flex w-[320px] flex-col gap-2"
          style={{ zIndex: 200 }}
          role="status"
          aria-live="polite"
        >
          {toasts.map((toast) => (
            <div
              key={toast.id}
              className="rounded-panel bg-surface-2 pointer-events-auto flex items-start gap-2.5 border border-line p-3 shadow-lg"
            >
              <Icon name="info" size={15} className="text-accent-soft mt-0.5 shrink-0" />
              <div className="min-w-0 flex-1">
                <p className="text-text-hi text-meta font-medium">
                  {toast.title}
                  {toast.repeats > 1 ? (
                    // Says the thing happened again rather than saying it
                    // again. Somebody pressing Refresh four times gets one
                    // notice that counts, and can still see it counted.
                    <span className="text-text-lo ml-1.5 font-mono text-[11px] font-normal">
                      ×{toast.repeats}
                    </span>
                  ) : null}
                </p>
                <p className="text-text-mid mt-0.5 text-[11px] leading-relaxed">{toast.body}</p>
              </div>
              <IconButton
                name="close"
                label="Dismiss"
                size={14}
                onClick={() => dismissToast(toast.id)}
              />
            </div>
          ))}
        </div>
      ) : null}

      {modal ? (
        <>
          <div className="fixed inset-0 bg-scrim" style={{ zIndex: 200 }} aria-hidden="true" />
          <div
            className="no-drag fixed inset-0 flex items-center justify-center p-8"
            style={{ zIndex: 200 }}
            role="dialog"
            aria-modal="true"
            aria-label={modal.title}
          >
            <div className="rounded-panel bg-surface-2 w-full max-w-[380px] border border-line p-5">
              <h2 className="text-text-hi font-display text-[17px] font-medium">{modal.title}</h2>
              <p className="text-text-mid mt-2 text-meta leading-relaxed">{modal.body}</p>
              <div className="mt-4 flex justify-end gap-2">
                <Button onClick={() => answerModal(false)}>Cancel</Button>
                <Button variant="primary" onClick={() => answerModal(true)} autoFocus>
                  Continue
                </Button>
              </div>
            </div>
          </div>
        </>
      ) : null}
    </>,
    document.body,
  );
}

/** A stable empty array, so `useSyncExternalStore` does not loop. */
const EMPTY: readonly never[] = [];
