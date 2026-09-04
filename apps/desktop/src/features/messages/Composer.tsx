import { useEffect, useRef, useState } from "react";
import { IconButton } from "../../components/ui/Button";
import { EmojiPicker } from "../../components/ui/EmojiPicker";
import { StickerPicker } from "../../components/ui/StickerPicker";
import { Icon } from "../../components/ui/Icon";
import { pickFile, type PickedFile } from "../../lib/native";
import { formatDuration, useRecorder, type Recording } from "./useRecorder";


/**
 * The composer (§6.1).
 *
 * Enter sends, Shift+Enter makes a newline, and the box grows to a ceiling of
 * roughly six lines before it scrolls. It sits on the pane behind a single
 * hairline rather than inside a bordered card: the composer is the floor of
 * the conversation, not a widget parked on top of it. The attachment button
 * opens the real Explorer file picker, and the microphone records — while it is
 * running the row becomes the recorder, because a timer and a waveform beside a
 * text box you cannot type in anyway is two controls pretending to be
 * available.
 */
export function Composer({
  onSend,
  onSendVoice,
  replyingTo,
  onCancelReply,
  onSendViewOnce,
  onSendSticker,
  conversationTitle,
}: {
  onSend: (body: string, attachment?: PickedFile) => void;
  onSendVoice: (recording: Recording) => void;
  /** The message being answered, when one is. */
  replyingTo?: { excerpt: string; outgoing: boolean } | undefined;
  onCancelReply?: (() => void) | undefined;
  /**
   * Sends a picture or clip the other person can open once.
   *
   * Absent in a group, and the control is then absent too. "Once" in a group
   * would have to mean "once each", which is a different promise wearing the
   * same word -- and the one people would assume is the stricter one.
   */
  onSendViewOnce?: (() => void) | undefined;
  /** Sends a sticker by name. Nothing is uploaded. */
  onSendSticker?: ((pack: string, stickerId: string) => void) | undefined;
  conversationTitle: string;
}) {
  const [value, setValue] = useState("");
  const [attachment, setAttachment] = useState<PickedFile | null>(null);
  const [emojiOpen, setEmojiOpen] = useState(false);
  const [stickersOpen, setStickersOpen] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const emojiWrap = useRef<HTMLDivElement>(null);
  const stickerWrap = useRef<HTMLDivElement>(null);
  const recorder = useRecorder();

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(el.scrollHeight, 148)}px`;
  }, [value]);

  useEffect(() => {
    if (!emojiOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!emojiWrap.current?.contains(event.target as Node)) setEmojiOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [emojiOpen]);

  useEffect(() => {
    if (!stickersOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!stickerWrap.current?.contains(event.target as Node))
        setStickersOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    return () => document.removeEventListener("pointerdown", onPointerDown);
  }, [stickersOpen]);

  const send = () => {
    const body = value.trim();
    if (!body && !attachment) return;
    onSend(body, attachment ?? undefined);
    setValue("");
    setAttachment(null);
  };

  const attach = async () => {
    const picked = await pickFile({ title: "Attach a file" });
    if (picked) setAttachment(picked);
  };

  const finishRecording = async () => {
    const recording = await recorder.stop();
    // `null` means the button was tapped rather than held: nothing was said,
    // so nothing is sent.
    if (recording) onSendVoice(recording);
  };

  return (
    <div className="shrink-0 border-t border-[var(--hairline)] px-3 py-2">
      {replyingTo ? (
        <div className="rounded-control bg-fill mb-1.5 flex items-stretch gap-2 px-2.5 py-1.5">
          <span
            aria-hidden
            className="bg-accent-soft w-[2px] shrink-0 rounded-full"
          />
          <span className="min-w-0 flex-1">
            <span className="text-text-mid block text-[11px] font-medium">
              Replying to {replyingTo.outgoing ? "yourself" : "them"}
            </span>
            <span className="text-text-lo block truncate text-meta">
              {replyingTo.excerpt || "a message"}
            </span>
          </span>
          <button
            type="button"
            aria-label="Stop replying"
            onClick={onCancelReply}
            className="text-text-lo hover:text-text-hi shrink-0 self-center"
          >
            <Icon name="close" size={13} />
          </button>
        </div>
      ) : null}
      {attachment ? (
        <div className="rounded-control bg-fill mb-1.5 flex items-center gap-2 px-2.5 py-1.5 text-meta">
          <Icon name="paperclip" size={14} className="text-text-lo shrink-0" />
          <span className="text-text-hi min-w-0 flex-1 truncate">{attachment.name}</span>
          <button
            type="button"
            aria-label="Remove attachment"
            onClick={() => setAttachment(null)}
            className="text-text-lo hover:text-text-hi shrink-0"
          >
            <Icon name="close" size={13} />
          </button>
        </div>
      ) : null}
      {recorder.state === "denied" ? (
        <div className="rounded-control bg-fill text-text-lo mb-1.5 flex items-center gap-2 px-2.5 py-1.5 text-meta">
          <Icon name="mic" size={14} className="shrink-0" />
          <span className="min-w-0 flex-1">
            Nexo cannot reach a microphone. Check Windows privacy settings for
            this app, then try again.
          </span>
        </div>
      ) : null}
      {recorder.state === "recording" ? (
        // The whole row, not a badge beside the text box: while this runs the
        // text box does nothing, and leaving it there invites typing into it.
        <div className="flex items-center gap-1.5">
          <IconButton
            name="trash"
            label="Discard this recording"
            onClick={recorder.cancel}
          />
          <div className="rounded-control bg-fill flex min-w-0 flex-1 items-center gap-2.5 px-2.5 py-2">
            <span
              aria-hidden
              className="size-2 shrink-0 rounded-full bg-[var(--danger)] motion-safe:animate-pulse"
            />
            <span className="text-text-hi shrink-0 font-mono text-meta tabular-nums">
              {formatDuration(recorder.elapsedMs)}
            </span>
            <Waveform peaks={recorder.peaks} className="min-w-0 flex-1" />
          </div>
          <IconButton
            name="send"
            label="Send this recording"
            variant="primary"
            onClick={() => void finishRecording()}
          />
        </div>
      ) : (
      <div className="flex items-end gap-1.5">
        <IconButton
          name="paperclip"
          label="Attach a file"
          active={attachment !== null}
          onClick={() => void attach()}
        />
        {onSendViewOnce ? (
          <IconButton
            name="eye"
            label="Send a photo or video they can open once"
            onClick={onSendViewOnce}
          />
        ) : null}
        <textarea
          ref={box}
          rows={1}
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              send();
            }
          }}
          aria-label={`Message ${conversationTitle}`}
          placeholder="Write a message"
          className="text-text-hi placeholder:text-text-lo max-h-[148px] min-h-9 flex-1 resize-none bg-transparent px-1 py-1.5 text-message leading-6 outline-none"
        />
        {onSendSticker ? (
          <div className="relative" ref={stickerWrap}>
            <IconButton
              name="image"
              label="Send a sticker"
              active={stickersOpen}
              onClick={() => setStickersOpen((open) => !open)}
            />
            {stickersOpen ? (
              <div className="rounded-panel bg-surface-2 ring-line-strong absolute right-0 bottom-full z-[300] mb-2 overflow-hidden shadow-[0_12px_32px_-8px_rgba(0,0,0,0.5)] ring-1">
                <StickerPicker
                  onPick={(pack, stickerId) => {
                    onSendSticker(pack, stickerId);
                    // Closed after one, unlike the emoji picker: an emoji joins
                    // a sentence being written, a sticker *is* the message and
                    // has already been sent by the time this runs.
                    setStickersOpen(false);
                  }}
                />
              </div>
            ) : null}
          </div>
        ) : null}
        <div className="relative" ref={emojiWrap}>
          <IconButton
            name="emoji"
            label="Insert an emoji"
            active={emojiOpen}
            onClick={() => setEmojiOpen((open) => !open)}
          />
          {emojiOpen ? (
            <div className="rounded-panel bg-surface-2 ring-line-strong absolute right-0 bottom-full z-[300] mb-2 overflow-hidden shadow-[0_12px_32px_-8px_rgba(0,0,0,0.5)] ring-1">
              <EmojiPicker
                onPick={(emoji) => {
                  setValue((current) => `${current}${emoji}`);
                  // Left open: picking one emoji is usually picking two, and
                  // reopening it each time is the thing that makes a picker
                  // tedious.
                  box.current?.focus();
                }}
              />
            </div>
          ) : null}
        </div>
        {/*
          The microphone gives way to Send as soon as there is anything to
          send. Two primary-looking actions side by side is a choice nobody
          asked for, and the one you want is never ambiguous: an empty box
          means a recording, a full one means a message.
        */}
        {value.trim().length === 0 && !attachment ? (
          <IconButton
            name="mic"
            label="Record a voice message"
            disabled={recorder.state === "asking"}
            onClick={() => void recorder.start()}
          />
        ) : (
          <IconButton
            name="send"
            label="Send message"
            variant="primary"
            onClick={send}
          />
        )}
      </div>
      )}
    </div>
  );
}

/**
 * The bars a recording is drawn as.
 *
 * One flex row of rounded slivers rather than a canvas or an SVG path: there
 * are at most sixty-four of them, they need no anti-aliasing, and this way they
 * take their colour from the same tokens as everything around them instead of
 * carrying their own.
 *
 * A bar is never drawn at zero height — a silent moment is a dot on the line,
 * not a gap in it, and a gap reads as the recording having stopped.
 */
export function Waveform({
  peaks,
  className = "",
}: {
  peaks: number[];
  className?: string;
}) {
  return (
    <span
      aria-hidden
      className={`flex h-6 items-center gap-[2px] overflow-hidden ${className}`}
    >
      {peaks.map((peak, index) => (
        <span
          key={index}
          className="bg-text-lo w-[2px] shrink-0 rounded-full"
          style={{ height: `${Math.max(10, (peak / 255) * 100)}%` }}
        />
      ))}
    </span>
  );
}
