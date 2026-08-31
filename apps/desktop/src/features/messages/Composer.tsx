import { useEffect, useRef, useState } from "react";
import { IconButton } from "../../components/ui/Button";
import { EmojiPicker } from "../../components/ui/EmojiPicker";
import { Icon } from "../../components/ui/Icon";
import { pickFile, type PickedFile } from "../../lib/native";


/**
 * The composer (§6.1).
 *
 * Enter sends, Shift+Enter makes a newline, and the box grows to a ceiling of
 * roughly six lines before it scrolls. It sits on the pane behind a single
 * hairline rather than inside a bordered card: the composer is the floor of
 * the conversation, not a widget parked on top of it. The attachment button
 * opens the real Explorer file picker; emoji and voice are still labelled but
 * inert until the milestones that give them something to do — a control that
 * lies about being ready is worse than one that is visibly not.
 */
export function Composer({
  onSend,
  conversationTitle,
}: {
  onSend: (body: string, attachment?: PickedFile) => void;
  conversationTitle: string;
}) {
  const [value, setValue] = useState("");
  const [attachment, setAttachment] = useState<PickedFile | null>(null);
  const [emojiOpen, setEmojiOpen] = useState(false);
  const box = useRef<HTMLTextAreaElement>(null);
  const emojiWrap = useRef<HTMLDivElement>(null);

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

  return (
    <div className="shrink-0 border-t border-[var(--hairline)] px-3 py-2">
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
      <div className="flex items-end gap-1.5">
        <IconButton
          name="paperclip"
          label="Attach a file"
          active={attachment !== null}
          onClick={() => void attach()}
        />
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
        <IconButton
          name="send"
          label="Send message"
          variant="primary"
          disabled={value.trim().length === 0 && !attachment}
          onClick={send}
        />
      </div>
    </div>
  );
}
