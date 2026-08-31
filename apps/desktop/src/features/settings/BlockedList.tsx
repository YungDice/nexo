import { useCallback, useEffect, useState } from "react";

import { listBlocks, unblock, type Block } from "../../lib/blocks";
import { asFeedError } from "../../lib/feed";
import { relativeTime } from "../../lib/format";
import { Button } from "../../components/ui/Button";
import { HandleAvatar } from "../../components/ui/HandleAvatar";
import { Callout, EmptyState } from "../../components/ui/Feedback";

/**
 * Everyone this account is blocking, and the way back.
 *
 * # Why it lives in Settings rather than only on each profile
 *
 * Because unblocking has to be possible without finding the person again.
 * Blocking someone usually means you do not want to go looking for them, and a
 * feature whose only undo is on the blocked person's own profile is one people
 * cannot reverse without doing the thing they were avoiding.
 *
 * # Only your own list
 *
 * There is no "who has blocked me", by design and not by omission. Handing
 * that over would give the blocked person exactly the confirmation the server
 * withholds — see `blocks.rs`, where a refused delivery is deliberately
 * indistinguishable from any other failure.
 */
export function BlockedList({ now }: { now: Date }) {
  const [blocks, setBlocks] = useState<Block[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setBlocks(await listBlocks());
      setProblem(null);
    } catch (error) {
      setProblem(asFeedError(error).message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function lift(handle: string) {
    setBusy(handle);
    try {
      await unblock(handle);
      // Re-read rather than splice: the server owns this list, and a local
      // edit would quietly diverge from it the first time two devices are
      // signed in.
      await load();
    } catch (error) {
      setProblem(asFeedError(error).message);
    } finally {
      setBusy(null);
    }
  }

  if (problem) {
    return (
      <Callout tone="warning" icon="alert">
        {problem}
      </Callout>
    );
  }

  if (blocks === null) return null;

  if (blocks.length === 0) {
    return (
      <EmptyState
        icon="user"
        title="Nobody is blocked"
        body="Blocking someone takes their posts out of your feed and stops either of you starting a conversation. You can do it from their profile."
      />
    );
  }

  return (
    <ul className="flex flex-col">
      {blocks.map((entry) => (
        <li
          key={entry.handle}
          className="flex items-center gap-3 border-b border-[var(--hairline)] py-3 last:border-b-0"
        >
          <HandleAvatar handle={entry.handle} name={entry.display_name} size={34} />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-text-hi truncate text-body font-medium">
              {entry.display_name}
            </span>
            <span className="text-text-lo truncate text-meta">
              @{entry.handle} · blocked {relativeTime(new Date(entry.blocked_at_ms), now)}
            </span>
          </span>
          <Button
            variant="secondary"
            disabled={busy === entry.handle}
            onClick={() => void lift(entry.handle)}
          >
            {busy === entry.handle ? "Unblocking…" : "Unblock"}
          </Button>
        </li>
      ))}
    </ul>
  );
}
