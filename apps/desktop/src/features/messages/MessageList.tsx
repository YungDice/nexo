import type { CSSProperties } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../../app/store";
import { firstLink, useLinkPreview } from "../../app/useLinkPreview";
import { cn } from "../../lib/cn";
import { clockTime, dayDivider, fileSize, isSameDay } from "../../lib/format";
import {
  confirm,
  copyText,
  notify,
  openUrl,
  pickSavePath,
} from "../../lib/native";
import {
  deleteMessageForMe,
  reactToMessage,
  reviseMessage,
  setMessagePinned,
} from "../../lib/conversations";
import { EmojiPicker } from "../../components/ui/EmojiPicker";

/**
 * How long a message stays editable, matching `nexo_protocol::window`.
 *
 * The UI's copy of a rule the sender is held to anyway. The receiver allows a
 * minute more, which is not this number's business — see the Rust module for
 * why the two differ.
 */
const EDIT_WINDOW_MS = 10 * 60 * 1000;
import {
  asConversationError,
  attachmentDataUrl,
  conversationAttachments,
  saveAttachmentTo,
  type AttachmentEntry,
} from "../../lib/conversations";
import { fieldFor, fileTone } from "../../lib/palette";
import type { Attachment, Conversation, Message } from "../../lib/types";
import { Avatar } from "../../components/ui/Avatar";
import { HandleAvatar } from "../../components/ui/HandleAvatar";
import { IconButton } from "../../components/ui/Button";
import { Icon } from "../../components/ui/Icon";
import { DeliveryTick } from "./ConversationList";
import { Lightbox } from "./Lightbox";
import { useContextMenu } from "../../components/ui/ContextMenu";
import { messageMenuItems } from "./menu";

/** Messages from the same person inside five minutes are one run (§6.1). */
const RUN_WINDOW_MS = 5 * 60_000;

interface Row {
  message: Message;
  /** First of a run: gets the avatar and the name. */
  startsRun: boolean;
  /** Last of a run: gets the timestamp and the delivery tick. */
  endsRun: boolean;
  divider?: string;
}

export function buildRows(messages: Message[], now: Date): Row[] {
  return messages.map((message, index) => {
    const previous = messages[index - 1];
    const next = messages[index + 1];
    const newDay = !previous || !isSameDay(previous.at, message.at);
    const startsRun =
      newDay ||
      !previous ||
      previous.authorId !== message.authorId ||
      message.at.getTime() - previous.at.getTime() > RUN_WINDOW_MS;
    const endsRun =
      !next ||
      next.authorId !== message.authorId ||
      !isSameDay(next.at, message.at) ||
      next.at.getTime() - message.at.getTime() > RUN_WINDOW_MS;
    return {
      message,
      startsRun,
      endsRun,
      ...(newDay ? { divider: dayDivider(message.at, now) } : {}),
    };
  });
}

export function MessageList({
  messages,
  now,
  conversation,
  onChanged,
}: {
  messages: Message[];
  now: Date;
  conversation: Conversation;
  /** Pinning or deleting changed the local store; reload from it. */
  onChanged?: () => void;
}) {
  const rows = buildRows(messages, now);
  const scroller = useRef<HTMLDivElement>(null);
  const count = messages.length;

  // Whether the newest message is on screen. Not a scroll position but a
  // question about intent: someone reading back through a conversation has not
  // asked to be yanked to the bottom because somebody typed.
  // Loaded once when something is opened, not on every render: it is a read of
  // the local store, but the whole conversation's worth.
  const [media, setMedia] = useState<AttachmentEntry[] | null>(null);
  const [viewing, setViewing] = useState<number | null>(null);

  const openMedia = useCallback(
    async (envelopeId: number) => {
      const all = media ?? (await conversationAttachments(conversation.id));
      if (!media) setMedia(all);
      // Only what can actually be shown. A PDF in the strip would be a
      // thumbnail of nothing with no way to view it.
      const shown = all.filter((a) => a.kind === "image" || a.kind === "video");
      const at = shown.findIndex((a) => a.envelope_id === envelopeId);
      if (at >= 0) setViewing(at);
    },
    [conversation.id, media],
  );

  const shownMedia = (media ?? []).filter(
    (a) => a.kind === "image" || a.kind === "video",
  );

  const [atBottom, setAtBottom] = useState(true);
  // How many arrived while they were reading back. Reset the moment the bottom
  // is reached again, however they got there.
  const [missed, setMissed] = useState(0);
  const seen = useRef(count);

  const scrollToBottom = useCallback(() => {
    const el = scroller.current;
    if (!el) return;
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }, []);

  const onScroll = useCallback(() => {
    const el = scroller.current;
    if (!el) return;
    // A few pixels of slack: a list can sit a fraction of a pixel short of the
    // bottom after a resize, and treating that as "scrolled up" would leave the
    // button on screen permanently.
    const bottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
    setAtBottom(bottom);
    if (bottom) {
      setMissed(0);
      seen.current = count;
    }
  }, [count]);

  // A chat opens at the newest message, and stays there when one arrives --
  // unless the reader has scrolled away, in which case the new ones are
  // counted instead of chased.
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    if (atBottom) {
      el.scrollTop = el.scrollHeight;
      seen.current = count;
      setMissed(0);
    } else {
      setMissed(Math.max(0, count - seen.current));
    }
    // `atBottom` is deliberately not a dependency: this reacts to messages
    // arriving, and including it would re-run on every scroll.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [count]);

  return (
    <div className="relative min-h-0 flex-1">
      <div
        ref={scroller}
        onScroll={onScroll}
        className="h-full overflow-y-auto"
      >
        <ol className="mt-auto flex flex-col gap-0.5 px-4 py-4">
          {rows.map((row, index) => (
            <li key={row.message.id} className="contents">
              {row.divider ? <DayDivider label={row.divider} /> : null}
              <Bubble
                row={row}
                index={index}
                conversation={conversation}
                onOpenMedia={(id) => void openMedia(id)}
                onRevise={async (target, body) => {
                  await reviseMessage(conversation.id, target, body);
                  onChanged?.();
                }}
                onReact={async (target, emoji, on) => {
                  await reactToMessage(conversation.id, target, emoji, on);
                  onChanged?.();
                }}
                onPinnedChange={async (envelopeId, pinned) => {
                  await setMessagePinned(conversation.id, envelopeId, pinned);
                  onChanged?.();
                }}
                onDeleteForMe={async (envelopeId) => {
                  // Named in the confirmation, not just in the menu: this is
                  // the difference the person is actually choosing.
                  const ok = await confirm(
                    "Delete for me",
                    "This removes the message from this device. Everyone else keeps their copy.",
                  );
                  if (!ok) return;
                  await deleteMessageForMe(conversation.id, envelopeId);
                  onChanged?.();
                }}
              />
            </li>
          ))}
        </ol>
      </div>

      {viewing !== null && shownMedia.length > 0 ? (
        <Lightbox
          items={shownMedia}
          startAt={viewing}
          now={now}
          onClose={() => setViewing(null)}
        />
      ) : null}

      {!atBottom ? (
        <button
          type="button"
          onClick={scrollToBottom}
          aria-label={
            missed > 0
              ? `${missed} new messages, jump to the latest`
              : "Jump to the latest"
          }
          className="rounded-full bg-surface-2 text-text-hi ring-line-strong absolute bottom-4 left-1/2 flex -translate-x-1/2 items-center gap-1.5 px-3 py-2 shadow-lg ring-1 transition-colors duration-[var(--motion-fast)] hover:bg-fill-hover"
        >
          <Icon name="chevronLeft" size={14} className="-rotate-90" />
          {missed > 0 ? (
            <span className="bg-accent rounded-full px-1.5 py-0.5 font-mono text-[11px] text-white">
              {missed}
            </span>
          ) : null}
        </button>
      ) : null}
    </div>
  );
}

function DayDivider({ label }: { label: string }) {
  return (
    <div
      className="my-3 flex items-center gap-3"
      role="separator"
      aria-label={label}
    >
      <span className="h-px flex-1 bg-[var(--hairline)]" />
      <span className="text-text-lo text-[11px] font-medium tracking-[0.06em] uppercase">
        {label}
      </span>
      <span className="h-px flex-1 bg-[var(--hairline)]" />
    </div>
  );
}

function Bubble({
  row,
  index,
  conversation,
  onOpenMedia,
  onPinnedChange,
  onDeleteForMe,
  onReact,
  onRevise,
}: {
  row: Row;
  index: number;
  conversation: Conversation;
  onOpenMedia: (envelopeId: number) => void;
  onPinnedChange: (envelopeId: number, pinned: boolean) => void | Promise<void>;
  onReact: (target: string, emoji: string, on: boolean) => void | Promise<void>;
  onRevise: (target: string, body?: string) => void | Promise<void>;
  onDeleteForMe: (envelopeId: number) => void | Promise<void>;
}) {
  const { message, startsRun, endsRun } = row;
  const account = useApp((s) => s.account);
  const [picking, setPicking] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);

  // Read at the moment the menu is built, from the message's own send time --
  // never frozen when the conversation opened. A menu opened at 9:58 must stop
  // offering these two minutes later, not stay valid for ever.
  const withinWindow = Date.now() - message.at.getTime() <= EDIT_WINDOW_MS;

  async function askToRetract() {
    // The wording is the feature. "Deleted for everyone" is a claim this
    // cannot make; what actually happens is a request that well-behaved
    // clients honour.
    const ok = await confirm(
      "Delete for everyone",
      "This asks every Nexo app that has this message to remove it. Copies on a modified app can remain.",
    );
    if (ok && message.clientId) await onRevise(message.clientId, undefined);
  }
  // `useConversations` marks our own messages "me"; everything else is a
  // sender device id. There is no profile directory yet (M7), so an incoming
  // message is named from the conversation when that is unambiguous -- a DM
  // has exactly one other party -- and left unnamed in a group rather than
  // attributed to a guess.
  const mine = message.authorId === "me";
  const authorName = mine
    ? (account?.display_name ?? "You")
    : conversation.kind === "dm"
      ? conversation.title
      : "Unknown sender";
  const authorSeed = mine ? (account?.handle ?? "me") : message.authorId;

  const { onContextMenu, menu } = useContextMenu(() =>
    messageMenuItems(
      {
        hasBody: !!message.body,
        mine,
        clientId: message.clientId,
        retracted: !!message.retracted,
        withinWindow,
        queued: message.state === "sending",
        pinned: !!message.pinned,
      },
      {
        copy: () => void copyText(message.body),
        edit: () => setEditing(message.body),
        react: () => setPicking(true),
        togglePin: () => void onPinnedChange(Number(message.id), !message.pinned),
        deleteForMe: () => void onDeleteForMe(Number(message.id)),
        deleteForEveryone: () => void askToRetract(),
      },
    ),
  );
  const authorHandle = mine
    ? (account?.handle ?? "")
    : conversation.kind === "dm"
      ? conversation.title
      : "";
  const showReceipts = useApp((s) => s.preferences.readReceipts);

  return (
    <div
      className={cn(
        "message-in flex w-full items-end gap-2",
        mine ? "flex-row-reverse" : "flex-row",
        startsRun ? "mt-3 first:mt-0" : "mt-0.5",
      )}
      style={{ "--stagger": `${Math.min(index, 10) * 30}ms` } as CSSProperties}
      onContextMenu={onContextMenu}
    >
      {menu}
      <span className="w-8 shrink-0">
        {endsRun ? (
          authorHandle ? (
            <HandleAvatar handle={authorHandle} name={authorName} size={32} />
          ) : (
            <Avatar seed={authorSeed} name={authorName} size={32} />
          )
        ) : null}
      </span>

      {/* The column shrinks to its content: a two-word reply is a two-word
          bubble, not a bubble stretched to the width of the column. */}
      <div
        className={cn(
          "flex max-w-[min(520px,64%)] flex-col gap-1",
          mine ? "items-end" : "items-start",
        )}
      >
        {startsRun && !mine ? (
          <span className="text-text-mid px-1 text-[11px] font-medium">
            {authorName}
          </span>
        ) : null}

        {message.undecryptable ? (
          <UndecryptableBubble />
        ) : message.unsupported ? (
          <UnsupportedBubble />
        ) : (
          <>
            {message.attachments?.some((a) => a.kind === "image") ? (
              <ImageGrid
                attachments={message.attachments.filter(
                  (a) => a.kind === "image",
                )}
                onOpen={onOpenMedia}
              />
            ) : null}

            <LinkPreviewCard message={message} />

            {message.retracted ? (
              // The row is still here on purpose. Removing it would close the
              // gap where something used to be, which is not what being taken
              // back looks like to the people who saw it.
              <div className="rounded-bubble border-line text-text-lo border border-dashed px-3.5 py-2 text-message italic">
                {mine ? "You took this back" : "This message was taken back"}
              </div>
            ) : editing !== null ? (
              <div className="rounded-bubble bg-surface-3 border-line flex flex-col gap-2 border p-2">
                <textarea
                  rows={3}
                  value={editing}
                  autoFocus
                  onChange={(event) => setEditing(event.target.value)}
                  aria-label="Edit this message"
                  className="text-text-hi rounded-control bg-surface-2 resize-none px-2 py-1.5 text-message outline-none focus:ring-1 focus:ring-accent"
                />
                <div className="flex gap-2">
                  <button
                    type="button"
                    className="text-accent-soft text-meta hover:underline"
                    onClick={() => {
                      const next = editing.trim();
                      // An empty edit is a retraction wearing a disguise, and
                      // it should be the deliberate one instead.
                      if (next && message.clientId) {
                        void onRevise(message.clientId, next);
                      }
                      setEditing(null);
                    }}
                  >
                    Save
                  </button>
                  <button
                    type="button"
                    className="text-text-lo text-meta hover:underline"
                    onClick={() => setEditing(null)}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            ) : message.body ? (
              <div
                className={cn(
                  "rounded-bubble px-3.5 py-2 text-message leading-6 whitespace-pre-wrap",
                  mine
                    ? "bg-accent text-white"
                    : "bg-surface-3 text-text-hi border border-line",
                  // The corner nearest the sender squares off inside a run, so
                  // a run reads as one block rather than a stack of pills.
                  !startsRun && (mine ? "rounded-tr-md" : "rounded-tl-md"),
                  !endsRun && (mine ? "rounded-br-md" : "rounded-bl-md"),
                )}
              >
                {message.body}
              </div>
            ) : null}

            {message.attachments
              ?.filter((a) => a.kind === "file")
              .map((attachment) => (
                <FileRow key={attachment.id} attachment={attachment} />
              ))}
          </>
        )}

        {/* The feed's pills, in a conversation. Same shape, same aria — the
            component was written once and the reaction data was given the same
            shape on purpose. */}
        {message.reactions && message.reactions.length > 0 ? (
          <div
            className={cn(
              "flex flex-wrap gap-1",
              mine ? "justify-end" : "justify-start",
            )}
          >
            {message.reactions.map((reaction) => (
              <button
                key={reaction.emoji}
                type="button"
                aria-label={`${reaction.count} reacted ${reaction.emoji}`}
                aria-pressed={reaction.mine}
                onClick={() =>
                  message.clientId &&
                  void onReact(message.clientId, reaction.emoji, !reaction.mine)
                }
                className={cn(
                  "rounded-full px-2 py-0.5 text-meta transition-colors duration-[var(--motion-fast)]",
                  reaction.mine
                    ? "bg-accent/18 text-accent-soft"
                    : "text-text-mid bg-fill-hover hover:bg-fill-active",
                )}
              >
                {/* User content, not chrome: somebody chose this. */}
                <span aria-hidden="true">{reaction.emoji}</span>{" "}
                <span className="font-mono text-[11px]">{reaction.count}</span>
              </button>
            ))}
          </div>
        ) : null}

        {picking && message.clientId ? (
          <div className="rounded-panel bg-surface-2 ring-line-strong z-[300] overflow-hidden ring-1">
            {/* Closed after one: a reaction is a single choice, unlike the
                composer where two in a row is normal. */}
            <EmojiPicker
              onPick={(emoji) => {
                if (message.clientId)
                  void onReact(message.clientId, emoji, true);
                setPicking(false);
              }}
            />
          </div>
        ) : null}

        {endsRun ? (
          <span
            className={cn(
              "text-text-lo flex items-center gap-1.5 px-1 text-[11px]",
              mine && "flex-row-reverse",
            )}
          >
            {clockTime(message.at)}
            {/* A quiet mark, and nothing that claims the original is gone --
                it is, from this device's point of view, and saying more than
                that would be a promise about other people's copies. */}
            {message.edited && !message.retracted ? (
              <span title="Edited by the sender">edited</span>
            ) : null}
            {mine && showReceipts ? (
              <DeliveryTick state={message.state} />
            ) : null}
            {message.state === "sending" && !showReceipts ? (
              <Icon name="clock" size={12} aria-label="Sending" />
            ) : null}
          </span>
        ) : null}
      </div>
    </div>
  );
}

/**
 * Rule 7: fail closed. A message that will not decrypt says so, in place,
 * permanently. There is no plaintext fallback, and skipping it silently would
 * hide exactly the event a user needs to know about.
 */
/**
 * A message that decrypted but whose shape this build does not know.
 *
 * Deliberately not the danger treatment of `UndecryptableBubble`: nothing
 * failed and nothing is lost. Somebody is running a newer Nexo, the bytes are
 * in the store, and the remedy is an update rather than asking them to resend.
 *
 * The kind is not shown. It is an internal name -- "reaction", "story" -- and
 * putting it in front of someone would be leaking the wire format into prose
 * to no purpose.
 */
function UnsupportedBubble() {
  return (
    <div className="rounded-bubble border-line bg-surface-3 flex items-start gap-2.5 border px-3.5 py-2.5">
      <Icon name="refresh" size={16} className="text-text-lo mt-0.5 shrink-0" />
      <span className="text-meta leading-relaxed">
        <span className="text-text-hi block font-medium">
          This message needs a newer version of Nexo
        </span>
        <span className="text-text-mid">
          It arrived safely and is kept on this device. Updating will show it.
        </span>
      </span>
    </div>
  );
}

function UndecryptableBubble() {
  return (
    <div className="rounded-bubble border-danger/35 bg-danger/8 flex items-start gap-2.5 border px-3.5 py-2.5">
      <Icon name="lock" size={16} className="text-danger mt-0.5 shrink-0" />
      <span className="text-meta leading-relaxed">
        <span className="text-danger block font-medium">
          Can't decrypt this message
        </span>
        <span className="text-text-mid">
          It was sent in an epoch this device has no key for. Ask the sender to
          send it again.
        </span>
      </span>
    </div>
  );
}

function ImageGrid({
  attachments,
  onOpen,
}: {
  attachments: Attachment[];
  onOpen: (envelopeId: number) => void;
}) {
  return (
    <div
      className={cn(
        "grid gap-1 overflow-hidden rounded-panel",
        attachments.length === 1 ? "grid-cols-1" : "grid-cols-2",
      )}
    >
      {attachments.map((attachment) => (
        <AttachedImage
          key={attachment.id}
          attachment={attachment}
          onOpen={onOpen}
        />
      ))}
    </div>
  );
}

/**
 * One attached image, decrypted and drawn.
 *
 * The envelope id is all the WebView gets; Rust downloads the ciphertext,
 * decrypts it, verifies the tag and the digest, and only then hands back bytes
 * — as a `data:` URL, because the CSP allows no remote image host. Until that
 * returns, and if it never does, the generated field stands in: a message that
 * carried a picture should still occupy the space it occupies.
 */
function AttachedImage({
  attachment,
  onOpen,
}: {
  attachment: Attachment;
  onOpen: (envelopeId: number) => void;
}) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void attachmentDataUrl(Number(attachment.id))
      .then((next) => {
        if (!cancelled) setUrl(next);
      })
      .catch(() => {
        // Left as the field. Rule 7: the failure shows as "no picture here",
        // never as a broken-image glyph or a dialog nobody asked for.
      });
    return () => {
      cancelled = true;
    };
  }, [attachment.id]);

  const { onContextMenu, menu } = useContextMenu(() => [
    {
      label: "View",
      icon: "eye",
      onSelect: () => onOpen(Number(attachment.id)),
    },
    {
      label: "Save picture as…",
      icon: "download",
      onSelect: () => void saveTo(attachment),
    },
  ]);

  return (
    <>
      {menu}
      <button
        type="button"
        onClick={() => onOpen(Number(attachment.id))}
        onContextMenu={onContextMenu}
        aria-label={`Open ${attachment.name}`}
        title={attachment.name}
        // Fitted rather than cropped: a picture somebody sent is not the
        // app's to trim. The lightbox shows it full size either way.
        className="aspect-[4/3] w-full min-w-36 cursor-zoom-in bg-contain bg-center bg-no-repeat bg-surface-3"
        style={
          url
            ? { backgroundImage: `url(${url})` }
            : { background: fieldFor(attachment.id) }
        }
      />
    </>
  );
}

/** Downloads, decrypts and writes one attachment where the user chooses. */
async function saveTo(attachment: Attachment): Promise<void> {
  const path = await pickSavePath(attachment.name);
  if (!path) return;
  try {
    await saveAttachmentTo(Number(attachment.id), path);
    await notify("Saved", `${attachment.name} was saved.`);
  } catch (error) {
    await notify("Couldn't save that", asConversationError(error).message);
  }
}

/**
 * One attached file.
 *
 * `attachment.id` is the envelope id: that is all the WebView needs to ask for
 * the file, and all it is given. The S3 key and the AES key stay in Rust
 * (rule 2), which downloads, decrypts, verifies, and only then writes to the
 * path the user chose.
 */
function FileRow({ attachment }: { attachment: Attachment }) {
  const [busy, setBusy] = useState(false);

  const save = async () => {
    // The name is a suggestion for the dialog. The user picks where it goes,
    // so a sender cannot choose a destination.
    const path = await pickSavePath(attachment.name);
    if (!path) return;
    setBusy(true);
    try {
      await saveAttachmentTo(Number(attachment.id), path);
    } catch (error) {
      // Rule 7: a file that failed to decrypt or failed to download says so.
      // Nothing partial is written -- Rust verifies before it writes -- and a
      // silent no-op here would look exactly like success.
      await notify(
        "Couldn't save that file",
        asConversationError(error).message,
      );
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-panel bg-surface-3 flex items-center gap-3 border border-line px-3 py-2.5">
      <span
        className="text-text-hi flex size-9 shrink-0 items-center justify-center rounded-control font-mono text-[9px] font-semibold"
        style={{ background: fileTone(attachment.name).tint }}
        aria-hidden="true"
      >
        {fileTone(attachment.name).label}
      </span>
      <span className="min-w-0 flex-1">
        <span className="text-text-hi block truncate text-meta font-medium">
          {attachment.name}
        </span>
        <span className="text-text-lo block font-mono text-[11px]">
          {busy ? "Decrypting\u2026" : fileSize(attachment.size)}
        </span>
      </span>
      <IconButton
        name="download"
        label={`Save ${attachment.name}`}
        size={16}
        disabled={busy}
        onClick={() => void save()}
      />
    </div>
  );
}

/**
 * §4.5: previews are fetched by this machine and are off by default, because
 * a server-side fetcher is a request-forgery and a metadata leak. With the
 * setting off, a link stays a link — and nothing is fetched at all, which is
 * the part of the promise that matters.
 *
 * The card is drawn only once a preview has actually come back. No skeleton:
 * a placeholder for something that may never arrive (an unreachable host, a
 * page with no title, a link Rust refuses) would leave a hole in the
 * conversation, and the link underneath it is already the honest fallback.
 *
 * No image is fetched. The band is generated from the URL, so the preview
 * costs exactly one request to whoever owns the link, not two.
 */
/**
 * Hands a link to the OS browser instead of following it here.
 *
 * A bare `href` navigates the WebView itself: the app is replaced by the page,
 * and there is no back button in a frameless window to come back with — the
 * only way out is quitting. The `href` stays for the hover target and the
 * accessibility tree; the click never uses it.
 */
function openExternally(url: string) {
  return (event: React.MouseEvent<HTMLAnchorElement>) => {
    event.preventDefault();
    void openUrl(url);
  };
}

function LinkPreviewCard({ message }: { message: Message }) {
  const fetched = useLinkPreview(message.body);
  // Mock fixtures carry their own preview; live messages get theirs from the
  // hook. Both render the same way.
  const preview = message.preview ?? fetched;
  const url = preview?.url ?? firstLink(message.body);

  // Built before the early return: hooks cannot be called conditionally, and
  // an empty address simply yields an empty menu, which opens nothing.
  const { onContextMenu, menu } = useContextMenu(() =>
    url
      ? [
          {
            label: "Open link",
            icon: "external",
            onSelect: () => void openUrl(url),
          },
          {
            label: "Copy address",
            icon: "link",
            onSelect: () => void copyText(url),
          },
        ]
      : [],
  );

  if (!url) return null;

  if (!preview) {
    return (
      <>
        {menu}
        <a
          href={url}
          onClick={openExternally(url)}
          onContextMenu={onContextMenu}
          className="text-accent-soft inline-flex items-center gap-1.5 text-meta underline decoration-line-strong underline-offset-2"
        >
          <Icon name="link" size={14} />
          {url}
        </a>
      </>
    );
  }

  return (
    <>
      {menu}
      <a
        href={preview.url}
        onClick={openExternally(preview.url)}
        onContextMenu={onContextMenu}
        className="rounded-panel bg-surface-3/70 block w-full max-w-[360px] border border-line p-2.5"
      >
        <span className="text-accent-soft block truncate text-[11px] underline decoration-line-strong underline-offset-2">
          {preview.url}
        </span>
        <span className="text-accent-soft mt-2 block text-[11px] font-medium">
          {preview.source}
        </span>
        <span className="text-text-hi block text-meta font-medium">
          {preview.title}
        </span>
        {preview.description ? (
          <span className="text-text-mid mt-0.5 block text-[11px] leading-relaxed">
            {preview.description}
          </span>
        ) : null}
        <span
          className="rounded-control mt-2.5 block h-32 w-full"
          style={{ background: fieldFor(preview.url) }}
          aria-hidden="true"
        />
      </a>
    </>
  );
}
