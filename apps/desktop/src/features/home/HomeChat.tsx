import { useEffect, useState } from "react";

import { useApp } from "../../app/store";
import { useConversations } from "../../app/useConversations";
import { ConversationAvatar } from "../../components/ui/ConversationAvatar";
import { Button, IconButton } from "../../components/ui/Button";
import { EmptyState } from "../../components/ui/Feedback";
import { Panel } from "../../components/ui/Surface";
import { Composer } from "../messages/Composer";
import { MessageList } from "../messages/MessageList";
import { MIN_HOME_CHAT } from "./Splitter";

/**
 * The most recent conversation, beside the feed.
 *
 * The feed column is 660px wide and the window is usually much wider, so
 * without this the right-hand margin was empty. Filling it with the
 * conversation you were last in is the one thing that is almost always what
 * you want next — reading the feed and answering someone are the two things
 * people do in the same sitting.
 *
 * # Which conversation
 *
 * `conversations` arrives sorted by last activity (the server-side list is
 * ordered by `last_message_at_ms`), so the first one is the answer. There is
 * no separate "last opened" to track: the conversation you last *wrote in* is
 * a better guess than the one you last clicked on, and it is already known.
 *
 * # Why the real components
 *
 * `MessageList` and `Composer` are the ones the Messages page uses, not
 * lookalikes. A second implementation would need its own copy of message
 * grouping, delivery states, the offline-queue "sending" mark and the
 * attachment rules — and would drift from the real one the first time either
 * changed. The panel is a narrower container around the same parts.
 *
 * # Width
 *
 * Given, not chosen, and given as a CSS length rather than a number: while the
 * splitter is being dragged the width is a custom property the splitter writes
 * directly, with no React render in the loop. Passing `var(--home-chat-w)`
 * keeps that arrangement visible at the call site instead of hiding it in a
 * variable name this file would have to know about.
 *
 * The floor and the ceiling are CSS too. What they guard against is the
 * *window* shrinking under a width that was perfectly legal when it was set,
 * and CSS re-evaluates that during a window resize without being asked.
 */
export function HomeChat({ now, width }: { now: Date; width: string }) {
  const go = useApp((s) => s.go);
  const openConversation = useApp((s) => s.openConversation);
  const overrides = useApp((s) => s.conversationOverrides);

  // Two steps, because `useConversations` loads the history for an id it is
  // given and the id is only known once the list has arrived. The first pass
  // fetches the list with nothing selected; the effect then names the most
  // recent one and the second pass fills in its messages.
  const [shownId, setShownId] = useState<string | undefined>(undefined);
  const live = useConversations(shownId);
  const newest = live.conversations[0];

  // Follows whatever is most recent, including when a message lands somewhere
  // else while this is open. That is what "the last conversation" means, and
  // the alternative -- pinning the first one seen -- goes stale silently.
  useEffect(() => {
    if (newest && newest.id !== shownId) setShownId(newest.id);
  }, [newest, shownId]);

  const conversation = newest;

  const openInMessages = () => {
    if (!conversation) return;
    openConversation(conversation.id);
    go("messages");
  };

  return (
    <Panel
      tone="list"
      className="flex shrink-0 flex-col"
      style={{ width, minWidth: MIN_HOME_CHAT, maxWidth: "58%" }}
      aria-label="Most recent conversation"
    >
      {conversation ? (
        <>
          <div className="flex items-center gap-2.5 border-b border-[var(--hairline)] px-3 py-2.5">
            <button
              type="button"
              onClick={openInMessages}
              className="rounded-control flex min-w-0 flex-1 items-center gap-2.5 px-1 py-0.5 text-left outline-none transition-colors duration-[var(--motion-fast)] ease-[var(--ease-state)] hover:bg-fill-hover focus-visible:ring-1 focus-visible:ring-accent"
            >
              <ConversationAvatar
                conversationId={conversation.id}
                kind={conversation.kind}
                title={conversation.title}
                hasAvatar={conversation.hasAvatar ?? false}
                size={30}
              />
              <span className="flex min-w-0 flex-col">
                <span className="text-text-hi truncate text-body font-medium">
                  {conversation.title}
                </span>
                <span className="text-text-lo text-[11px]">Most recent</span>
              </span>
            </button>
            <IconButton
              name="external"
              label="Open in Messages"
              size={16}
              onClick={openInMessages}
            />
          </div>

          <MessageList
            messages={live.messages}
            now={now}
            conversation={{ ...conversation, ...overrides[conversation.id] }}
          />

          <Composer
            conversationTitle={conversation.title}
            onSend={(body, attachment) => {
              if (attachment) void live.sendFile(attachment.path, body || undefined);
              else void live.send(body);
            }}
          />
        </>
      ) : (
        // No conversations yet. The panel says so rather than sitting empty,
        // and sends you where starting one actually happens.
        <EmptyState
          icon="messages"
          title="No conversations yet"
          body="Once you are writing with someone, the most recent conversation appears here beside the feed."
          action={<Button onClick={() => go("messages")}>Go to Messages</Button>}
        />
      )}
    </Panel>
  );
}
