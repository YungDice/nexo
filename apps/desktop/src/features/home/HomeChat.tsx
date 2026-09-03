import { useEffect, useRef, useState } from "react";

import { useApp } from "../../app/store";
import { useConversations } from "../../app/useConversations";
import { ConversationAvatar } from "../../components/ui/ConversationAvatar";
import { Button, IconButton } from "../../components/ui/Button";
import { ContextMenu, type MenuItem } from "../../components/ui/ContextMenu";
import { EmptyState } from "../../components/ui/Feedback";
import { Icon } from "../../components/ui/Icon";
import { Panel } from "../../components/ui/Surface";
import { Composer } from "../messages/Composer";
import { MessageList } from "../messages/MessageList";
import { MIN_HOME_CHAT } from "./Splitter";

/**
 * A conversation beside the feed.
 *
 * The feed column is 660px wide and the window is usually much wider, so
 * without this the right-hand margin was empty. Filling it with the
 * conversation you were last in is the one thing that is almost always what
 * you want next — reading the feed and answering someone are the two things
 * people do in the same sitting.
 *
 * # Which conversation
 *
 * By default the most recent one. `conversations` arrives sorted by last
 * activity (the server-side list is ordered by `last_message_at_ms`), so the
 * first one is the answer, and there is no separate "last opened" to track:
 * the conversation you last *wrote in* is a better guess than the one you last
 * clicked on, and it is already known.
 *
 * The header is also a picker, because the default is a guess and a guess that
 * cannot be overruled is just a limitation. Choosing one suspends the
 * following until it is taken back — the staleness the paragraph above warns
 * about is the **silent** kind, and a choice somebody made is not that.
 *
 * The choice does not survive a restart, deliberately. It answers "not that
 * one, this one, right now"; a conversation that sat here for a week while
 * everything happened elsewhere would be the silent staleness again, arrived
 * at by a different route.
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
/**
 * How many conversations the header's picker offers.
 *
 * The menu does not scroll, so this is the number that fits without running
 * off the screen at the smallest supported window. Everything else lives in
 * Messages, one click away.
 */
const PICKER_LIMIT = 8;

export function HomeChat({ now, width }: { now: Date; width: string }) {
  const go = useApp((s) => s.go);
  const openConversation = useApp((s) => s.openConversation);
  const overrides = useApp((s) => s.conversationOverrides);

  // Two steps, because `useConversations` loads the history for an id it is
  // given and the id is only known once the list has arrived. The first pass
  // fetches the list with nothing selected; the effect then names one and the
  // second pass fills in its messages.
  const [shownId, setShownId] = useState<string | undefined>(undefined);
  // `undefined` means "follow whatever is most recent". A string means someone
  // chose that one and it stays until they choose otherwise.
  const [chosenId, setChosenId] = useState<string | undefined>(undefined);
  const live = useConversations(shownId);
  const newest = live.conversations[0];
  const following = chosenId === undefined;

  useEffect(() => {
    // A chosen conversation that has been removed from this device stops being
    // a choice. Falling back to following beats an empty panel beside a feed
    // that is plainly not empty.
    if (
      chosenId &&
      live.conversations.length > 0 &&
      !live.conversations.some((c) => c.id === chosenId)
    ) {
      setChosenId(undefined);
      return;
    }
    const want = chosenId ?? newest?.id;
    if (want && want !== shownId) setShownId(want);
  }, [chosenId, newest, shownId, live.conversations]);

  // The fallback covers one frame: the list arrives before the effect below
  // has named anything, and without it the panel would flash "no conversations
  // yet" beside a feed that plainly has some.
  const conversation = live.conversations.find((c) => c.id === shownId) ?? newest;

  const openInMessages = () => {
    if (!conversation) return;
    openConversation(conversation.id);
    go("messages");
  };

  // Anchored under the header rather than at the pointer: this menu belongs to
  // a control, so it opens where that control is and can be found there again.
  const trigger = useRef<HTMLButtonElement>(null);
  const [pickerAt, setPickerAt] = useState<{ x: number; y: number } | null>(null);
  // `ContextMenu` closes on any `pointerdown` outside itself, and the trigger
  // is outside itself. Without this, clicking it while open would close and
  // immediately reopen. Capture runs before the menu's document listener, so
  // this reads the state the click actually started from; keyboard activation
  // fires no pointer event and finds it `false`, which is correct there.
  const wasOpen = useRef(false);
  const closePicker = () => {
    setPickerAt(null);
    wasOpen.current = false;
  };

  const pickerItems: MenuItem[] = [
    {
      label: "Most recent",
      ...(following ? { icon: "check" as const } : {}),
      onSelect: () => setChosenId(undefined),
    },
    { label: "", separator: true },
    // Capped, because this menu does not scroll and a person with two hundred
    // conversations would get a list taller than the screen. The ones missing
    // from it are one click away in Messages, which is what the button beside
    // this one is for.
    ...live.conversations.slice(0, PICKER_LIMIT).map((c) => ({
      label: c.title,
      ...(c.id === shownId ? { icon: "check" as const } : {}),
      onSelect: () => setChosenId(c.id),
    })),
  ];

  return (
    <Panel
      tone="list"
      className="flex shrink-0 flex-col"
      style={{ width, minWidth: MIN_HOME_CHAT, maxWidth: "58%" }}
      aria-label="Conversation beside the feed"
    >
      {conversation ? (
        <>
          <div className="flex items-center gap-2.5 border-b border-[var(--hairline)] px-3 py-2.5">
            <button
              ref={trigger}
              type="button"
              aria-haspopup="menu"
              aria-expanded={pickerAt !== null}
              onPointerDownCapture={() => {
                wasOpen.current = pickerAt !== null;
              }}
              onClick={() => {
                if (wasOpen.current) return;
                const rect = trigger.current?.getBoundingClientRect();
                if (rect) setPickerAt({ x: rect.left, y: rect.bottom + 4 });
              }}
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
                <span className="text-text-lo text-[11px]">
                  {following ? "Most recent" : "Chosen"}
                </span>
              </span>
              <Icon
                name="chevronLeft"
                size={13}
                className="text-text-lo shrink-0 -rotate-90"
              />
            </button>
            <IconButton
              name="external"
              label="Open in Messages"
              size={16}
              onClick={openInMessages}
            />
          </div>

          {pickerAt ? (
            <ContextMenu items={pickerItems} at={pickerAt} onClose={closePicker} />
          ) : null}

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
