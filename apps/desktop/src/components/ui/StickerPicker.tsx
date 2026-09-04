import { useMemo, useRef, useState } from "react";

import { cn } from "../../lib/cn";
import { Icon } from "./Icon";
import { PACK, STICKERS, StickerArt, matchesSticker } from "./stickers";

/**
 * The sticker picker.
 *
 * Modelled on `EmojiPicker` — same shell, same search-at-the-top, same
 * recents-first ordering — because they are the same interaction and having
 * them behave differently is the sort of small friction nobody can name but
 * everybody feels.
 *
 * What is different is the grid: stickers are pictures, so three across at 72px
 * rather than a dense wall of glyphs. There is one pack, so there is no pack
 * rail; when there is a second, the rail goes down the left and this comment
 * stops being true.
 */

const RECENT_KEY = "nexo.stickers.recent";
const RECENT_MAX = 8;

function loadRecent(): string[] {
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((x) => typeof x === "string") : [];
  } catch {
    // A private window, cleared site data, or storage turned off. A picker
    // with no memory is a working picker, so this is not worth reporting.
    return [];
  }
}

function remember(id: string, current: string[]): string[] {
  const next = [id, ...current.filter((x) => x !== id)].slice(0, RECENT_MAX);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    // Same as above: the picker still works, it just forgets.
  }
  return next;
}

export function StickerPicker({
  onPick,
}: {
  onPick: (pack: string, id: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [recent, setRecent] = useState<string[]>(loadRecent);
  const search = useRef<HTMLInputElement>(null);

  const shown = useMemo(() => {
    const matching = STICKERS.filter((s) => matchesSticker(s, query));
    if (query.trim()) return matching;
    // Recents first, and only when nothing is being searched for: reordering
    // results while somebody types would move the thing they were aiming at.
    const order = new Map(recent.map((id, index) => [id, index]));
    return [...matching].sort((a, b) => {
      const ai = order.get(a.id) ?? Number.MAX_SAFE_INTEGER;
      const bi = order.get(b.id) ?? Number.MAX_SAFE_INTEGER;
      return ai - bi;
    });
  }, [query, recent]);

  return (
    <div className="flex w-[260px] flex-col gap-2 p-2">
      <label className="rounded-control bg-surface-3 flex items-center gap-2 px-2 py-1.5">
        <Icon name="search" size={14} className="text-text-lo shrink-0" />
        <input
          ref={search}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search stickers"
          aria-label="Search stickers"
          className="text-text-hi placeholder:text-text-lo min-w-0 flex-1 bg-transparent text-meta outline-none"
        />
      </label>

      {shown.length === 0 ? (
        <p className="text-text-lo px-1 py-6 text-center text-meta">
          Nothing matches “{query.trim()}”.
        </p>
      ) : (
        <div className="grid max-h-[280px] grid-cols-3 gap-1 overflow-y-auto">
          {shown.map((sticker) => (
            <button
              key={sticker.id}
              type="button"
              title={sticker.label}
              aria-label={sticker.label}
              onClick={() => {
                setRecent((current) => remember(sticker.id, current));
                onPick(PACK, sticker.id);
              }}
              className={cn(
                "rounded-control flex items-center justify-center p-1",
                "hover:bg-surface-3 focus-visible:ring-accent focus-visible:ring-1 focus-visible:outline-none",
              )}
            >
              <StickerArt sticker={sticker} size={72} />
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
