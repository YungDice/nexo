import type { CSSProperties } from "react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { isMuted, useApp } from "../../app/store";
import { cn } from "../../lib/cn";
import { relativeTime } from "../../lib/format";
import {
  asConversationError,
  deleteConversation,
  searchMessages,
} from "../../lib/conversations";
import { confirm, notify } from "../../lib/native";

import type { Conversation, Message } from "../../lib/types";

import { ConversationAvatar } from "../../components/ui/ConversationAvatar";
import { useContextMenu, type MenuItem } from "../../components/ui/ContextMenu";
import { IconButton } from "../../components/ui/Button";
import { Field } from "../../components/ui/Controls";
import { EmptyState } from "../../components/ui/Feedback";
import { Icon } from "../../components/ui/Icon";
import { Panel } from "../../components/ui/Surface";
import { blockPeer, peerHandle } from "./peer";
import { clickSelection, pruneSelection } from "./selection";

/**
 * Everything a selection of several conversations can be asked to do.
 *
 * Passed down rather than reached for from the row, because the row does not
 * know what else is selected and must not decide on its own -- a right-click
 * inside a selection of four has to act on four, and the only place that count
 * exists is the list.
 */
interface Bulk {
  ids: string[];
  /** Whether every selected conversation is already pinned, for the label. */
  allPinned: boolean;
  pin: (pinned: boolean) => void;
  mute: (until: number | null) => void;
  remove: () => Promise<void>;
  clear: () => void;
}

function countLabel(n: number): string {
  return n === 1 ? "this conversation" : `these ${n} conversations`;
}

/**
 * The 300px conversation list (§7.3): own profile card, search, then the rows.
 *
 * Search covers conversation names and message bodies. Names are matched here;
 * bodies go through the FTS5 index inside the encrypted store, so the whole of
 * history is searched rather than the one message each row happens to be
 * showing — and the term never reaches the network (§6.1).
 */
export function ConversationList({
  now,
  conversations,
  lastMessages,
  onStart,
  onRemoved,
}: {
  now: Date;
  conversations: Conversation[];
  /** The most recent message per conversation, for the row preview. */
  lastMessages: Record<string, Message | undefined>;
  onStart: () => void;
  /** A conversation was removed from this device; the list needs a re-read. */
  onRemoved: (id: string) => void;
}) {
  const active = useApp((s) => s.activeConversationId);
  const open = useApp((s) => s.openConversation);
  const showPresence = useApp((s) => s.preferences.presence);
  const overrides = useApp((s) => s.conversationOverrides);
  const toggleFlag = useApp((s) => s.toggleConversationFlag);
  const mute = useApp((s) => s.muteConversation);
  const forget = useApp((s) => s.forgetConversation);
  const [query, setQuery] = useState("");

  // Multi-selection, the two ways every file list has done it for thirty
  // years: Ctrl adds and removes one, Shift takes everything between the last
  // one touched and this one. The rules themselves are in `./selection`, where
  // they can be read and tested without a DOM.
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set());
  const [anchor, setAnchor] = useState<string | null>(null);

  // Conversations whose history contains the term.
  //
  // The old filter looked at `lastMessages`, which holds one message per
  // conversation -- and only for the open one. So "search" found a word if it
  // happened to be in the newest message and nowhere else, which is close
  // enough to nothing that the comment above it was a promise rather than a
  // description.
  const [matching, setMatching] = useState<ReadonlySet<string> | null>(null);
  const term = query.trim();

  useEffect(() => {
    if (!term) {
      setMatching(null);
      return;
    }
    let cancelled = false;
    // Debounced: a query per keystroke would run an FTS scan per keystroke.
    const timer = window.setTimeout(() => {
      void searchMessages(term, 200)
        .then((hits) => {
          if (!cancelled) setMatching(new Set(hits.map((h) => h.conversation_id)));
        })
        .catch(() => {
          // Falling back to titles alone is better than an error banner over a
          // search box; the name filter below still applies.
          if (!cancelled) setMatching(new Set());
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [term]);

  const rows = useMemo(() => {
    const lowered = term.toLowerCase();
    return conversations
      .map((base) => ({
        conversation: { ...base, ...overrides[base.id] },
        last: lastMessages[base.id],
      }))
      .filter(({ conversation }) => {
        if (!lowered) return true;
        // A name match needs no index; a body match comes from FTS5.
        return (
          conversation.title.toLowerCase().includes(lowered) ||
          (matching?.has(conversation.id) ?? false)
        );
      })
      // Pinned first, then by when each conversation was last *written in*,
      // taken from the conversation itself. Sorting by the loaded history
      // instead meant only the open conversation had a timestamp at all --
      // every other one fell back to zero, so merely clicking a conversation
      // sent it to the top. Now a conversation moves when a message arrives or
      // is sent, and at no other time.
      //
      // Pinning is a second key rather than a separate list: a pinned
      // conversation that goes quiet should sink within the pinned ones, and
      // splitting the list in two would freeze it at the top instead.
      .sort((a, b) => {
        const pin =
          Number(overrides[b.conversation.id]?.pinned ?? false) -
          Number(overrides[a.conversation.id]?.pinned ?? false);
        if (pin !== 0) return pin;
        return (
          (b.conversation.lastMessageAt?.getTime() ?? 0) -
          (a.conversation.lastMessageAt?.getTime() ?? 0)
        );
      });
  }, [term, matching, overrides, conversations, lastMessages]);

  const order = useMemo(() => rows.map((row) => row.conversation.id), [rows]);

  // A selection may only ever name rows that are on screen. Typing in the
  // search box or having a conversation removed underneath would otherwise
  // leave a bulk action pointed at chats nobody can see -- "Remove 4" with two
  // rows visible is how somebody deletes something they did not mean to.
  useEffect(() => {
    setSelected((current) => pruneSelection(order, current));
  }, [order]);

  const onRowClick = useCallback(
    (id: string, event: React.MouseEvent) => {
      const next = clickSelection(
        order,
        { selected, anchor, open: false },
        id,
        { toggle: event.ctrlKey || event.metaKey, range: event.shiftKey },
      );
      setSelected(next.selected);
      setAnchor(next.anchor);
      if (next.open) open(id);
    },
    [anchor, order, open, selected],
  );

  const clearSelection = useCallback(() => setSelected(new Set()), []);

  // Escape clears it, the same key that closes everything else here.
  useEffect(() => {
    if (selected.size === 0) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") clearSelection();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [selected.size, clearSelection]);

  const bulk: Bulk = useMemo(
    () => ({
      ids: [...selected],
      allPinned: [...selected].every((id) => overrides[id]?.pinned ?? false),
      pin: (pinned: boolean) => {
        for (const id of selected) {
          if ((overrides[id]?.pinned ?? false) !== pinned) toggleFlag(id, "pinned");
        }
        clearSelection();
      },
      mute: (until: number | null) => {
        for (const id of selected) mute(id, until);
        clearSelection();
      },
      remove: async () => {
        const ids = [...selected];
        const ok = await confirm(
          ids.length === 1 ? "Remove from this device" : `Remove ${ids.length} conversations`,
          `The messages in ${countLabel(ids.length)} are deleted here and cannot be ` +
            "recovered. Everyone else keeps their copy, and if they write again the " +
            "conversation comes back with the new message in it.",
        );
        if (!ok) return;
        // One at a time and keep going: a failure on the third should not
        // silently abandon the fourth, and the ones that did work are already
        // gone from disk either way.
        const failed: string[] = [];
        for (const id of ids) {
          try {
            await deleteConversation(id);
            forget(id);
            onRemoved(id);
          } catch {
            failed.push(id);
          }
        }
        clearSelection();
        if (failed.length > 0) {
          await notify(
            "Some conversations could not be removed",
            `${failed.length} of ${ids.length} are still on this device.`,
          );
        }
      },
      clear: clearSelection,
    }),
    [selected, overrides, toggleFlag, mute, forget, onRemoved, clearSelection],
  );

  return (
    <Panel
      tone="list"
      edge={false}
      className="flex w-[300px] shrink-0 flex-col border-r border-[var(--hairline)]"
    >
      <div className="flex items-end gap-2 px-3 py-3">
        <div className="min-w-0 flex-1">
          <Field
            label="Search"
            hideLabel
            icon="search"
            placeholder="People and messages"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            className="[&]:h-9"
          />
        </div>
        <IconButton
          name="plus"
          label="New conversation"
          variant="secondary"
          onClick={onStart}
        />
      </div>

      {selected.size > 0 ? (
        <div className="flex items-center gap-2 border-y border-[var(--hairline)] bg-fill px-3 py-2">
          <span className="text-text-hi flex-1 text-meta font-medium">
            {selected.size} selected
          </span>
          <IconButton
            name="pin"
            label={bulk.allPinned ? "Unpin all" : "Pin all to the top"}
            variant="ghost"
            onClick={() => bulk.pin(!bulk.allPinned)}
          />
          <IconButton
            name="bell-off"
            label="Mute all"
            variant="ghost"
            onClick={() => bulk.mute(Number.POSITIVE_INFINITY)}
          />
          <IconButton
            name="trash"
            label="Remove all from this device"
            variant="danger"
            onClick={() => void bulk.remove()}
          />
          <IconButton
            name="close"
            label="Clear the selection"
            variant="ghost"
            onClick={bulk.clear}
          />
        </div>
      ) : null}

      <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
        {rows.length === 0 ? (
          query.trim() ? (
            <EmptyState
              icon="search"
              title="No matches"
              body={`Nothing here matches “${query.trim()}”. Search covers conversation names and the messages already on this machine.`}
            />
          ) : (
            <EmptyState
              icon="messages"
              title="No conversations yet"
              body="Start one with someone's handle. They need to have used Nexo at least once, so there is a key package to invite them with."
            />
          )
        ) : (
          <ul
            role="listbox"
            aria-multiselectable="true"
            aria-label="Conversations"
            className="flex flex-col gap-0.5"
          >
            {rows.map(({ conversation, last }, index) => (
              <li key={conversation.id}>
                <ConversationRow
                  conversation={conversation}
                  last={last}
                  onRemoved={() => onRemoved(conversation.id)}
                  now={now}
                  active={conversation.id === active}
                  selected={selected.has(conversation.id)}
                  showPresence={showPresence}
                  onClick={(event) => onRowClick(conversation.id, event)}
                  // Only when this row is part of a selection of more than
                  // one: a right-click on an unselected row is about that row,
                  // whatever else happens to be highlighted elsewhere.
                  bulk={selected.has(conversation.id) && selected.size > 1 ? bulk : null}
                  index={index}
                />
              </li>
            ))}
          </ul>
        )}
      </div>
    </Panel>
  );
}

/**
 * How long "mute" can mean.
 *
 * Five, not eight: every extra row is one more thing to read before making a
 * small decision, and the difference between six hours and eight is not worth
 * a line in a menu.
 *
 * "Until I turn it off" sits first, and it is exactly what the plain Mute
 * entry above these has always done on click. It is spelled out because the
 * behaviour was otherwise unfindable: a bare "Mute" beside four durations
 * reads as "mute for how long?", and nobody discovers an option by not seeing
 * it. Naming it costs one line and stops the most-wanted answer from being
 * the hidden one.
 *
 * `Infinity` needs no special case anywhere downstream. It compares correctly
 * against `Date.now()`, and `JSON.stringify` turns it into `null`, which the
 * reader in `store.ts` already treats as "muted, no end".
 */
const MUTE_DURATIONS: readonly { label: string; ms: number }[] = [
  { label: "Until I turn it off", ms: Number.POSITIVE_INFINITY },
  { label: "For 1 hour", ms: 60 * 60 * 1000 },
  { label: "For 8 hours", ms: 8 * 60 * 60 * 1000 },
  { label: "Until tomorrow", ms: 24 * 60 * 60 * 1000 },
  { label: "For a week", ms: 7 * 24 * 60 * 60 * 1000 },
];

/**
 * The durations, as submenu entries under whichever Mute opened them.
 *
 * `Date.now()` is read when the entry is chosen rather than when the menu is
 * built, so a menu left open does not mute from the moment it appeared. For
 * the first entry the addition is `now + Infinity`, which is `Infinity` — the
 * arithmetic carries it without being told to.
 */
function timedMutes(apply: (until: number) => void): MenuItem[] {
  return MUTE_DURATIONS.map((option) => ({
    label: option.label,
    onSelect: () => apply(Date.now() + option.ms),
  }));
}

function ConversationRow({
  conversation,
  last,
  now,
  active,
  selected,
  showPresence,
  onClick,
  onRemoved,
  bulk,
  index,
}: {
  conversation: Conversation;
  last: Message | undefined;
  now: Date;
  active: boolean;
  selected: boolean;
  showPresence: boolean;
  onClick: (event: React.MouseEvent) => void;
  onRemoved: () => void;
  /** Set when this row is one of several selected; the menu then acts on all. */
  bulk: Bulk | null;
  index: number;
}) {
  // No profile directory yet (M7), so there is nobody to look up: the avatar
  // is seeded from the conversation and presence is simply not shown rather
  // than guessed at.
  void showPresence;
  const fromMe = last?.authorId === "me";
  const toggleFlag = useApp((s) => s.toggleConversationFlag);
  const mute = useApp((s) => s.muteConversation);
  const forget = useApp((s) => s.forgetConversation);
  const account = useApp((s) => s.account);
  const peer = peerHandle(conversation, account?.handle);
  const override = useApp((s) => s.conversationOverrides[conversation.id]);
  const muted = isMuted(override, now.getTime());
  const pinned = override?.pinned ?? false;

  // A row's own actions. Leaving the group is still absent on purpose --
  // leaving an MLS group is a self-removal the core does not have, and an
  // entry that asked for confirmation and then did nothing is what D7 was
  // about. Removing it from *this device* is a different promise and one the
  // app can keep, so that is what the entry says.
  const { onContextMenu, menu } = useContextMenu((): MenuItem[] => {
    if (bulk) {
      const n = bulk.ids.length;
      return [
        {
          label: bulk.allPinned ? `Unpin ${n}` : `Pin ${n} to the top`,
          icon: "pin",
          onSelect: () => bulk.pin(!bulk.allPinned),
        },
        {
          label: `Mute ${n}`,
          icon: "bell-off",
          onSelect: () => bulk.mute(Number.POSITIVE_INFINITY),
          submenu: timedMutes((until) => bulk.mute(until)),
        },
        { label: `Unmute ${n}`, icon: "bell", onSelect: () => bulk.mute(null) },
        { label: "", separator: true },
        {
          label: `Remove ${n} from this device`,
          icon: "trash",
          danger: true,
          onSelect: () => void bulk.remove(),
        },
      ];
    }
    return [
      {
        label: pinned ? "Unpin" : "Pin to the top",
        icon: "pin",
        onSelect: () => toggleFlag(conversation.id, "pinned"),
      },
      muted
        ? {
            label: "Unmute",
            icon: "bell",
            onSelect: () => mute(conversation.id, null),
          }
        : {
            // Click means "not now, indefinitely", which is what most muting
            // is. The choices are behind the arrow rather than laid out
            // beneath: five of them standing open would turn a two-line
            // decision into a seven-line one and put the answer above the
            // question. The first of them says what this click does, so the
            // shortcut is a shortcut rather than a secret.
            label: "Mute",
            icon: "bell-off",
            onSelect: () => mute(conversation.id, Number.POSITIVE_INFINITY),
            submenu: timedMutes((until) => mute(conversation.id, until)),
          },
      { label: "", separator: true },
      // Only where there is somebody to name. A group has several people and
      // no single answer, and a DM whose member list has not arrived yet has
      // no answer at all -- in both cases the entry is absent rather than
      // present and broken.
      ...(peer
        ? [
            {
              label: `Block ${conversation.title}`,
              icon: "shield" as const,
              danger: true,
              onSelect: () => void blockPeer(peer, conversation.title),
            },
          ]
        : []),
      {
        label: "Remove from this device",
        icon: "trash",
        danger: true,
        onSelect: () => void removeFromDevice(),
      },
    ];
  });

  async function removeFromDevice() {
    const ok = await confirm(
      "Remove from this device",
      `The messages in ${conversation.title} are deleted here and cannot be recovered. ` +
        "Everyone else keeps their copy, and if they write again the conversation comes back " +
        "with the new message in it.",
    );
    if (!ok) return;
    try {
      await deleteConversation(conversation.id);
      forget(conversation.id);
      onRemoved();
    } catch (error) {
      await notify("Couldn't remove that conversation", asConversationError(error).message);
    }
  }

  // `last` is only ever set for the conversation that is open -- the other
  // rows have no history loaded at all. So the row prefers a live message when
  // it has one and otherwise falls back to the preview the core sent with the
  // conversation itself. Only when there is neither is a conversation really
  // empty, and saying "No messages yet" about a full one reads as data loss.
  const preview = last?.undecryptable
    ? "Can't decrypt this message"
    : last?.body
      ? last.body
      : last?.attachments?.length
        ? `${last.attachments.length} attachment${last.attachments.length > 1 ? "s" : ""}`
        : conversation.lastMessage
          ? conversation.lastMessage
          : "No messages yet";

  return (
    <>
    {menu}
    <button
      type="button"
      role="option"
      onClick={onClick}
      onContextMenu={onContextMenu}
      aria-current={active ? "true" : undefined}
      aria-selected={selected}
      style={{ "--stagger": `${Math.min(index, 8) * 30}ms` } as CSSProperties}
      className={cn(
        "rise-in flex w-full items-center gap-3 rounded-panel px-2.5 py-2.5 text-left transition-colors duration-[var(--motion-fast)] ease-[var(--ease-state)]",
        // Selected outranks active: while a selection is live it is the thing
        // the next action will happen to, and which conversation is open is
        // the less urgent fact.
        selected
          ? "bg-accent/18 ring-accent/45 ring-1"
          : active
            ? "bg-fill-hover"
            : "hover:bg-fill",
      )}
    >
      <ConversationAvatar
        conversationId={conversation.id}
        kind={conversation.kind}
        title={conversation.title}
        hasAvatar={conversation.hasAvatar ?? false}
        size={40}
      />

      <span className="min-w-0 flex-1">
        <span className="flex items-baseline gap-2">
          <span className="text-text-hi truncate text-body font-medium">
            {conversation.title}
          </span>
          {/* Pinned before muted: one says where the row is, the other says
              how it behaves, and the first is what the eye is scanning for. */}
          {pinned ? <Icon name="pin" size={12} className="text-accent-soft shrink-0" /> : null}
          {muted ? (
            <Icon name="bell-off" size={12} className="text-text-lo shrink-0" />
          ) : null}
        </span>
        <span className="flex items-center gap-1.5">
          {fromMe && !last?.undecryptable ? (
            <span className="text-text-lo text-meta shrink-0">You:</span>
          ) : null}
          <span
            className={cn(
              "truncate text-meta",
              last?.undecryptable ? "text-danger" : "text-text-mid",
            )}
          >
            {preview}
          </span>
        </span>
      </span>

      <span className="flex shrink-0 flex-col items-end gap-1">
        <span className="text-text-lo text-[11px]">
          {last ? relativeTime(last.at, now) : ""}
        </span>
        {conversation.unread > 0 ? (
          <span className="bg-accent min-w-[18px] rounded-full px-1.5 text-center text-[11px] leading-[18px] font-semibold text-white">
            {conversation.unread}
          </span>
        ) : fromMe && last ? (
          <DeliveryTick state={last.state} />
        ) : null}
      </span>
    </button>
    </>
  );
}

/** §6.1: one tick sent, two delivered, two in success green once read. */
export function DeliveryTick({ state }: { state: Message["state"] }) {
  if (state === "sending") {
    return <Icon name="clock" size={13} className="text-text-lo" aria-label="Sending" />;
  }
  if (state === "failed") {
    return <Icon name="alert" size={13} className="text-danger" aria-label="Not sent" />;
  }
  if (state === "sent") {
    return <Icon name="check" size={13} className="text-text-lo" aria-label="Sent" />;
  }
  return (
    <Icon
      name="checks"
      size={14}
      className={state === "read" ? "text-success" : "text-text-lo"}
      aria-label={state === "read" ? "Read" : "Delivered"}
    />
  );
}
