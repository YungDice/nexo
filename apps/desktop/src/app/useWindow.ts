import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

/**
 * The frameless window's own state and controls (§7.3).
 *
 * Every call is wrapped: `vite dev` has no Tauri runtime, and a titlebar that
 * throws on click would make the shell undevelopable in a plain browser.
 */
export async function windowAction(
  action: "minimize" | "toggleMaximize" | "close",
): Promise<void> {
  try {
    await getCurrentWindow()[action]();
  } catch {
    // No Tauri runtime. Nothing to do.
  }
}

/**
 * Whether the window is maximised. The shell uses it to decide whether to
 * float its card on the gradient field or run edge to edge.
 */
export function useMaximized(): boolean {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    let cancelled = false;
    const sync = async () => {
      try {
        const value = await getCurrentWindow().isMaximized();
        if (!cancelled) setMaximized(value);
      } catch {
        // Browser preview: treat it as a floating window.
      }
    };
    void sync();
    window.addEventListener("resize", sync);
    return () => {
      cancelled = true;
      window.removeEventListener("resize", sync);
    };
  }, []);

  return maximized;
}
