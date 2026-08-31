import type { ReactNode } from "react";
import { useEffect } from "react";
import { createPortal } from "react-dom";

/**
 * A dialog, drawn over the whole window.
 *
 * Portalled to `document.body` rather than rendered where it is written, and
 * that is the whole point of the component. `position: fixed` resolves against
 * the nearest ancestor with a transform, a filter, or a backdrop-filter — not
 * against the viewport — so a dialog opened from inside the titlebar's glass
 * layer lays itself out inside the titlebar, a strip forty pixels tall. It ends
 * up clipped and half off-screen, which is exactly what it did.
 *
 * A portal escapes that: the markup leaves the transformed subtree entirely, so
 * `inset-0` means the window again no matter which component opened it.
 */
export function Modal({
  label,
  onClose,
  children,
}: {
  /** Names the dialog for screen readers. */
  label: string;
  onClose: () => void;
  children: ReactNode;
}) {
  // Escape closes it. A modal with no keyboard way out is a trap for anyone
  // not using a mouse, and the scrim is the only other exit.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return createPortal(
    <>
      {/* z-index contract, §7.3: overlays sit at 200. */}
      <div
        className="fixed inset-0 bg-scrim"
        style={{ zIndex: 200 }}
        onClick={onClose}
        aria-hidden="true"
      />
      <div
        className="no-drag fixed inset-0 flex items-center justify-center p-8"
        style={{ zIndex: 200 }}
        role="dialog"
        aria-modal="true"
        aria-label={label}
      >
        {children}
      </div>
    </>,
    document.body,
  );
}
