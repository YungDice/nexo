import { useCallback, useEffect, useState } from "react";

import { Button } from "../../components/ui/Button";
import { Callout } from "../../components/ui/Feedback";
import { useApp } from "../../app/store";
import {
  acceptIntro,
  asMeetError,
  declineIntro,
  meetRequests,
  type MeetRequest,
} from "../../lib/meet";

/**
 * Intros waiting for an answer.
 *
 * Accepting opens the conversation like any other and lifts the one-message
 * cap. Declining lifts it too, and that is deliberate rather than an
 * oversight: a declined intro that stayed capped would leave a dead thread
 * neither person could act on, and somebody who wants the sender gone rather
 * than merely refused has Block, which does more than a frozen conversation
 * ever would.
 *
 * Nobody is told they were declined. There is no message back, and the sender
 * sees only that nothing happened — which is the same thing they see when
 * somebody simply has not looked yet, and that ambiguity is the point.
 */
export function Requests() {
  const [requests, setRequests] = useState<MeetRequest[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState<number | null>(null);
  const go = useApp((s) => s.go);
  const openConversation = useApp((s) => s.openConversation);

  const load = useCallback(async () => {
    try {
      setRequests(await meetRequests());
      setProblem(null);
    } catch (error) {
      const e = asMeetError(error);
      if (e.kind !== "signed_out") setProblem(e.message);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function answer(request: MeetRequest, accept: boolean) {
    setBusy(request.id);
    try {
      if (accept) {
        await acceptIntro(request.id);
        openConversation(request.conversation_id);
        go("messages");
      } else {
        await declineIntro(request.id);
      }
      await load();
    } catch (error) {
      setProblem(asMeetError(error).message);
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

  if (!requests) return null;

  if (requests.length === 0) {
    return (
      <p className="text-text-lo text-meta">
        Nobody has written to you from the map yet.
      </p>
    );
  }

  return (
    <ul className="flex flex-col gap-2">
      {requests.map((request) => (
        <li
          key={request.id}
          className="rounded-panel bg-surface-2 border-line flex items-center gap-3 border p-3"
        >
          <div className="min-w-0 flex-1">
            <p className="text-text-hi truncate text-[13px]">
              @{request.from_handle}
            </p>
            <p className="text-text-lo text-meta">wants to say hello</p>
          </div>
          <Button
            variant="primary"
            onClick={() => void answer(request, true)}
            disabled={busy === request.id}
          >
            Read it
          </Button>
          <Button
            onClick={() => void answer(request, false)}
            disabled={busy === request.id}
          >
            Decline
          </Button>
        </li>
      ))}
    </ul>
  );
}
