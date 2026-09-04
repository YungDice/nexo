import { useEffect, useState } from "react";

import { onTyping } from "../lib/stream";
import { useApp } from "./store";

/**
 * Who is typing in a conversation, right now.
 *
 * # Why it expires on a timer rather than on a "stopped" event
 *
 * There is no stop event, deliberately. Somebody who closes the window mid-word
 * sends nothing, and a protocol that relied on a stop notice would leave
 * "typing…" on screen for ever the first time one went missing. So each notice
 * is worth a few seconds and then lapses on its own — the sender re-sends while
 * they keep typing, and silence is what "stopped" means.
 *
 * The window is deliberately a little longer than the sender's repeat interval:
 * shorter and the indicator would flicker between notices, which reads as a
 * fault rather than as typing.
 */

/** How long one notice keeps somebody marked as typing. */
const TYPING_LASTS_MS = 5000;

/** How often lapsed notices are swept. */
const SWEEP_MS = 1000;

export function useTyping(conversationId: string | undefined): boolean {
  // The preference is honoured on both sides: not sending is not the same as
  // not showing, and somebody who turned this off means both.
  const showPresence = useApp((s) => s.preferences.presence);
  const [seenAt, setSeenAt] = useState<Record<string, number>>({});

  useEffect(() => {
    if (!showPresence) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void onTyping((event) => {
      setSeenAt((current) => ({
        ...current,
        [`${event.conversation_id}:${event.user_id}`]: Date.now(),
      }));
    })
      .then((off) => {
        // The listener may resolve after this effect was torn down; without
        // this the subscription would outlive the component that made it.
        if (cancelled) off();
        else unlisten = off;
      })
      .catch(() => {
        // No Tauri runtime — a browser preview. Nothing types there.
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [showPresence]);

  // Sweeping rather than filtering on read, so the indicator disappears on its
  // own rather than only when something else causes a render.
  useEffect(() => {
    const timer = window.setInterval(() => {
      const cutoff = Date.now() - TYPING_LASTS_MS;
      setSeenAt((current) => {
        const kept = Object.entries(current).filter(([, at]) => at > cutoff);
        // A new object every tick would re-render every listener every second
        // for no change, so the old one is returned when nothing lapsed.
        return kept.length === Object.keys(current).length
          ? current
          : Object.fromEntries(kept);
      });
    }, SWEEP_MS);
    return () => window.clearInterval(timer);
  }, []);

  if (!conversationId || !showPresence) return false;
  const cutoff = Date.now() - TYPING_LASTS_MS;
  return Object.entries(seenAt).some(
    ([key, at]) => key.startsWith(`${conversationId}:`) && at > cutoff,
  );
}
