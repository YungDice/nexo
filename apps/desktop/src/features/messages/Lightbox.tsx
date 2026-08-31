import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import { cn } from "../../lib/cn";
import { fileSize, relativeTime } from "../../lib/format";
import { notify, pickSavePath } from "../../lib/native";
import { fieldFor } from "../../lib/palette";
import {
  asConversationError,
  attachmentDataUrl,
  saveAttachmentTo,
  type AttachmentEntry,
} from "../../lib/conversations";
import { IconButton } from "../../components/ui/Button";
import { Icon } from "../../components/ui/Icon";

/** How far each step of the zoom goes, and where it stops. */
const ZOOM_STEP = 0.25;
const ZOOM_MIN = 1;
const ZOOM_MAX = 4;

/**
 * One attachment, full size, over everything.
 *
 * Portalled to `document.body` for the same reason `Modal` is: `fixed` resolves
 * against the nearest transformed ancestor, and the message list sits inside
 * several. Escape and a click on the backdrop both close it; the arrow keys
 * move through the conversation's other media without going back to the thread.
 *
 * Only the envelope id is held here. The bytes are fetched one at a time, and
 * the key that decrypts them never leaves Rust.
 */
export function Lightbox({
  items,
  startAt,
  now,
  onClose,
}: {
  /** Every image and video in the conversation, oldest first. */
  items: AttachmentEntry[];
  /** Which one was clicked. */
  startAt: number;
  now: Date;
  onClose: () => void;
}) {
  const [index, setIndex] = useState(startAt);
  const [url, setUrl] = useState<string | null>(null);
  const [zoom, setZoom] = useState(1);
  const [problem, setProblem] = useState<string | null>(null);
  const stripRef = useRef<HTMLDivElement>(null);

  const current = items[index];

  const step = useCallback(
    (by: number) => {
      setIndex((i) => {
        const next = i + by;
        if (next < 0 || next >= items.length) return i;
        return next;
      });
    },
    [items.length],
  );

  // Fetching resets the zoom: staying at 4x while a different picture arrives
  // shows a corner of something nobody asked to see that closely.
  useEffect(() => {
    if (!current) return;
    let cancelled = false;
    setUrl(null);
    setZoom(1);
    setProblem(null);
    void attachmentDataUrl(current.envelope_id)
      .then((next) => {
        if (!cancelled) setUrl(next);
      })
      .catch((error) => {
        if (!cancelled) setProblem(asConversationError(error).message);
      });
    return () => {
      cancelled = true;
    };
  }, [current]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
      else if (event.key === "ArrowRight") step(1);
      else if (event.key === "ArrowLeft") step(-1);
      else if (event.key === "+" || event.key === "=") setZoom((z) => Math.min(ZOOM_MAX, z + ZOOM_STEP));
      else if (event.key === "-") setZoom((z) => Math.max(ZOOM_MIN, z - ZOOM_STEP));
      else if (event.key === "0") setZoom(1);
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose, step]);

  // Keep the current thumbnail in view when stepping with the keyboard.
  useEffect(() => {
    stripRef.current
      ?.querySelector<HTMLElement>(`[data-index="${index}"]`)
      ?.scrollIntoView({ block: "nearest", inline: "center", behavior: "smooth" });
  }, [index]);

  async function save() {
    if (!current) return;
    const path = await pickSavePath(current.name);
    if (!path) return;
    try {
      await saveAttachmentTo(current.envelope_id, path);
      await notify("Saved", `${current.name} was saved.`);
    } catch (error) {
      await notify("Couldn't save that", asConversationError(error).message);
    }
  }

  if (!current) return null;

  return createPortal(
    <div
      className="fixed inset-0 flex flex-col bg-black/85"
      style={{ zIndex: 300 }}
      role="dialog"
      aria-modal="true"
      aria-label={current.name}
    >
      <header className="no-drag flex shrink-0 items-center gap-3 px-4 py-3">
        <div className="min-w-0 flex-1">
          <p className="truncate text-body font-medium text-white">{current.name}</p>
          <p className="text-[11px] text-white/60">
            {fileSize(current.size)} · {relativeTime(new Date(current.sent_at_ms), now)}
            {items.length > 1 ? ` · ${index + 1} of ${items.length}` : ""}
          </p>
        </div>
        <IconButton
          name="minus"
          label="Zoom out"
          size={17}
          disabled={zoom <= ZOOM_MIN}
          onClick={() => setZoom((z) => Math.max(ZOOM_MIN, z - ZOOM_STEP))}
        />
        <span className="w-12 text-center font-mono text-[11px] text-white/70">
          {Math.round(zoom * 100)}%
        </span>
        <IconButton
          name="plus"
          label="Zoom in"
          size={17}
          disabled={zoom >= ZOOM_MAX}
          onClick={() => setZoom((z) => Math.min(ZOOM_MAX, z + ZOOM_STEP))}
        />
        <IconButton name="download" label="Save" size={17} onClick={() => void save()} />
        <IconButton name="close" label="Close" size={17} onClick={onClose} />
      </header>

      {/* The backdrop closes; the media itself does not, so a click meant for
          the picture is not a click meant to leave. */}
      <div
        className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden p-4"
        onClick={onClose}
      >
        {index > 0 ? (
          <button
            type="button"
            aria-label="Previous"
            onClick={(e) => {
              e.stopPropagation();
              step(-1);
            }}
            className="absolute left-4 z-10 rounded-full bg-black/50 p-2 text-white hover:bg-black/70"
          >
            <Icon name="chevronLeft" size={22} />
          </button>
        ) : null}

        <div onClick={(e) => e.stopPropagation()} className="max-h-full max-w-full">
          {problem ? (
            <p className="text-white/70">{problem}</p>
          ) : !url ? (
            <div
              className="size-[320px] animate-pulse rounded-panel"
              style={{ background: fieldFor(String(current.envelope_id)) }}
              aria-label="Loading"
              role="img"
            />
          ) : current.kind === "video" ? (
            // Controls, because a video with no way to pause it is a
            // decoration rather than something you can watch.
            <video
              src={url}
              controls
              autoPlay
              className="max-h-[calc(100vh-13rem)] max-w-full rounded-panel"
            />
          ) : (
            <img
              src={url}
              alt={current.name}
              style={{ transform: `scale(${zoom})` }}
              className="max-h-[calc(100vh-13rem)] max-w-full rounded-panel transition-transform duration-[var(--motion-fast)]"
            />
          )}
        </div>

        {index < items.length - 1 ? (
          <button
            type="button"
            aria-label="Next"
            onClick={(e) => {
              e.stopPropagation();
              step(1);
            }}
            className="absolute right-4 z-10 rounded-full bg-black/50 p-2 text-white hover:bg-black/70"
          >
            <Icon name="chevronLeft" size={22} className="rotate-180" />
          </button>
        ) : null}
      </div>

      {/* N2: everything else in the conversation, to jump straight to rather
          than scrolling the thread back to find it. */}
      {items.length > 1 ? (
        <div
          ref={stripRef}
          className="no-drag flex shrink-0 gap-2 overflow-x-auto px-4 py-3"
          aria-label="Media in this conversation"
        >
          {items.map((item, i) => (
            <Thumbnail
              key={item.envelope_id}
              item={item}
              index={i}
              active={i === index}
              onClick={() => setIndex(i)}
            />
          ))}
        </div>
      ) : null}
    </div>,
    document.body,
  );
}

/**
 * One entry in the strip.
 *
 * Each fetches its own bytes, which is the cost of a strip that shows what
 * things actually are. The generated field stands in until then, so the strip
 * has its full width from the first frame and nothing jumps as they arrive.
 */
function Thumbnail({
  item,
  index,
  active,
  onClick,
}: {
  item: AttachmentEntry;
  index: number;
  active: boolean;
  onClick: () => void;
}) {
  const [url, setUrl] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void attachmentDataUrl(item.envelope_id)
      .then((next) => {
        if (!cancelled) setUrl(next);
      })
      .catch(() => {
        // The field stands. A thumbnail that will not decrypt is not worth an
        // error message under a picture somebody is looking at.
      });
    return () => {
      cancelled = true;
    };
  }, [item.envelope_id]);

  return (
    <button
      type="button"
      data-index={index}
      onClick={onClick}
      aria-label={item.name}
      aria-current={active ? "true" : undefined}
      className={cn(
        "size-14 shrink-0 rounded-control bg-cover bg-center ring-2 transition-[box-shadow,opacity] duration-[var(--motion-fast)]",
        active ? "ring-accent opacity-100" : "opacity-60 ring-transparent hover:opacity-100",
      )}
      style={
        url
          ? { backgroundImage: `url(${url})` }
          : { background: fieldFor(String(item.envelope_id)) }
      }
    />
  );
}
