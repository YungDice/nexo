/**
 * The bundled sticker pack.
 *
 * Drawn here as inline SVG for the same three reasons the icon set is, and one
 * more that matters more for stickers than for chrome:
 *
 * - Rule 3 wants everything bundled and nothing fetched. Inline SVG is the
 *   strongest form of that — there is no request to make, and therefore no
 *   third party learning which sticker you sent, or when, or that you were
 *   looking through them at two in the morning.
 * - A sticker sent as a picture would be an attachment: an upload, an object in
 *   the bucket, a key, a download. Sending a *name* instead costs a few bytes
 *   inside the ciphertext, so a sticker is as cheap as a short message.
 * - **Nothing in a pack can execute.** No `<script>`, no `<foreignObject>`, no
 *   external reference. These are components compiled into the app, not data
 *   interpreted at runtime, so there is no format for a hostile pack to abuse
 *   (rule 3 again, from the other side).
 *
 * Colour is deliberate here and nowhere else in the chrome. `README.md` says
 * content keeps its colour while the interface stays neutral — a sticker is
 * content somebody chose, so it is the one thing in this directory that is
 * allowed to be loud.
 *
 * Animation is CSS, and every animated sticker holds a readable pose when
 * `prefers-reduced-motion` is set: a sticker whose joke depends on movement
 * still has to be a picture for somebody who turned movement off.
 */

import type { JSX } from "react";

/** One sticker, as the picker and the bubble see it. */
export interface Sticker {
  /** Stable, and part of the wire format — never rename one of these. */
  id: string;
  /** What it is, for search and for the accessible name. */
  label: string;
  /** Extra search terms. The label is always searched too. */
  keywords: string[];
  art: JSX.Element;
}

/**
 * The pack's name, carried in every `Payload::Sticker`.
 *
 * A message names the pack as well as the sticker so that a second pack can
 * exist later without its ids having to avoid this one's.
 */
export const PACK = "nexo";

/**
 * Drawn on a 96px grid, unlike the icons' 24 — a sticker is a picture rather
 * than a glyph, and filling shapes at icon scale produces mud.
 */
const stickers: Sticker[] = [
  {
    id: "thumbs-up",
    label: "Thumbs up",
    keywords: ["yes", "ok", "agree", "good", "like"],
    art: (
      <g>
        <path
          d="M30 44h10v40H30a6 6 0 0 1-6-6V50a6 6 0 0 1 6-6z"
          fill="#e8a33d"
        />
        <path
          d="M40 44 56 16a8 8 0 0 1 12 8l-4 16h16a8 8 0 0 1 8 9l-5 27a8 8 0 0 1-8 7H40z"
          fill="#f6c064"
        />
      </g>
    ),
  },
  {
    id: "heart",
    label: "Heart",
    keywords: ["love", "like", "romance", "adore"],
    art: (
      <g className="sticker-beat">
        <path
          d="M48 82C24 66 12 54 12 40a18 18 0 0 1 36-6 18 18 0 0 1 36 6c0 14-12 26-36 42z"
          fill="#e35d6a"
        />
        <path
          d="M30 28a10 10 0 0 0-8 10c0 4 2 8 6 12"
          fill="none"
          stroke="#ffffff"
          strokeOpacity="0.45"
          strokeWidth="5"
          strokeLinecap="round"
        />
      </g>
    ),
  },
  {
    id: "laughing",
    label: "Laughing",
    keywords: ["haha", "lol", "funny", "joke", "face"],
    art: (
      <g>
        <circle cx="48" cy="48" r="38" fill="#f6c064" />
        <path d="M26 38q8-8 16 0" fill="none" stroke="#2c2118" strokeWidth="5" strokeLinecap="round" />
        <path d="M54 38q8-8 16 0" fill="none" stroke="#2c2118" strokeWidth="5" strokeLinecap="round" />
        <path d="M28 58a20 20 0 0 0 40 0z" fill="#2c2118" />
        <path d="M34 70a14 14 0 0 0 28 0z" fill="#e35d6a" />
      </g>
    ),
  },
  {
    id: "thinking",
    label: "Thinking",
    keywords: ["hmm", "unsure", "maybe", "face", "wondering"],
    art: (
      <g>
        <circle cx="48" cy="48" r="38" fill="#f6c064" />
        <circle cx="36" cy="42" r="4" fill="#2c2118" />
        <circle cx="60" cy="42" r="4" fill="#2c2118" />
        <path d="M36 66q12-6 22 2" fill="none" stroke="#2c2118" strokeWidth="5" strokeLinecap="round" />
        <circle cx="74" cy="70" r="7" fill="#8aa0b8" className="sticker-drift" />
      </g>
    ),
  },
  {
    id: "party",
    label: "Party",
    keywords: ["celebrate", "congrats", "yay", "confetti", "well done"],
    art: (
      <g>
        <path d="M18 82 44 40l14 14z" fill="#7c5cff" />
        <path d="M18 82 30 61l9 9z" fill="#a68cff" />
        <circle cx="66" cy="26" r="5" fill="#e35d6a" className="sticker-pop" />
        <circle cx="80" cy="44" r="4" fill="#f6c064" className="sticker-pop" />
        <circle cx="52" cy="18" r="4" fill="#4cbb8c" className="sticker-pop" />
        <rect x="70" y="60" width="8" height="8" rx="2" fill="#4fb6e8" className="sticker-pop" />
      </g>
    ),
  },
  {
    id: "sleeping",
    label: "Sleeping",
    keywords: ["tired", "night", "zzz", "bed", "face"],
    art: (
      <g>
        <circle cx="44" cy="52" r="34" fill="#f6c064" />
        <path d="M30 48q6 5 12 0" fill="none" stroke="#2c2118" strokeWidth="4" strokeLinecap="round" />
        <path d="M50 48q6 5 12 0" fill="none" stroke="#2c2118" strokeWidth="4" strokeLinecap="round" />
        <ellipse cx="46" cy="68" rx="7" ry="5" fill="#2c2118" />
        <text
          x="70"
          y="30"
          fontSize="22"
          fontWeight="700"
          fill="#8aa0b8"
          className="sticker-drift"
        >
          z
        </text>
      </g>
    ),
  },
  {
    id: "thanks",
    label: "Thank you",
    keywords: ["thanks", "grateful", "please", "hands"],
    art: (
      <g>
        <path d="M30 84V52a8 8 0 0 1 16 0v32z" fill="#f6c064" />
        <path d="M50 84V52a8 8 0 0 1 16 0v32z" fill="#e8a33d" />
        <path
          d="M48 40c-6-10-2-20 6-22 6-2 10 4 8 10"
          fill="none"
          stroke="#e35d6a"
          strokeWidth="5"
          strokeLinecap="round"
        />
      </g>
    ),
  },
  {
    id: "eyes",
    label: "Watching",
    keywords: ["looking", "eyes", "see", "watch", "attention"],
    art: (
      <g>
        <ellipse cx="32" cy="48" rx="20" ry="24" fill="#ffffff" stroke="#2c2118" strokeWidth="4" />
        <ellipse cx="64" cy="48" rx="20" ry="24" fill="#ffffff" stroke="#2c2118" strokeWidth="4" />
        <circle cx="36" cy="50" r="8" fill="#2c2118" className="sticker-glance" />
        <circle cx="68" cy="50" r="8" fill="#2c2118" className="sticker-glance" />
      </g>
    ),
  },
  {
    id: "fire",
    label: "Fire",
    keywords: ["hot", "great", "lit", "amazing", "flame"],
    art: (
      <g className="sticker-flicker">
        <path d="M48 88C30 88 20 76 20 62c0-16 14-22 18-38 10 8 12 16 12 22 4-4 6-8 6-14 12 8 20 20 20 32 0 14-10 24-28 24z" fill="#e35d6a" />
        <path d="M48 82c-8 0-14-6-14-14 0-8 8-12 10-22 6 6 12 12 12 22 0 8-4 14-8 14z" fill="#f6c064" />
      </g>
    ),
  },
  {
    id: "check",
    label: "Done",
    keywords: ["ok", "yes", "finished", "agreed", "tick"],
    art: (
      <g>
        <circle cx="48" cy="48" r="36" fill="#4cbb8c" />
        <path
          d="M30 50 43 63 68 36"
          fill="none"
          stroke="#ffffff"
          strokeWidth="9"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </g>
    ),
  },
  {
    id: "no",
    label: "No",
    keywords: ["stop", "nope", "refuse", "cross", "disagree"],
    art: (
      <g>
        <circle cx="48" cy="48" r="36" fill="#e35d6a" />
        <path
          d="M34 34 62 62M62 34 34 62"
          fill="none"
          stroke="#ffffff"
          strokeWidth="9"
          strokeLinecap="round"
        />
      </g>
    ),
  },
  {
    id: "waving",
    label: "Waving",
    keywords: ["hi", "hello", "bye", "hey", "greeting"],
    art: (
      <g className="sticker-wave">
        <path d="M34 88V56c0-6 4-10 9-10s9 4 9 10" fill="#f6c064" />
        <path
          d="M34 60V26a7 7 0 0 1 14 0v30M48 58V20a7 7 0 0 1 14 0v38M62 58V30a7 7 0 0 1 14 0v34c0 14-10 24-24 24H46c-8 0-14-6-14-14"
          fill="#f6c064"
          stroke="#e8a33d"
          strokeWidth="3"
        />
      </g>
    ),
  },
];

/** Every sticker in the bundled pack, in the order the picker shows them. */
export const STICKERS: readonly Sticker[] = stickers;

/** One sticker by id, or `undefined` for a pack or id this build does not know. */
export function findSticker(pack: string, id: string): Sticker | undefined {
  if (pack !== PACK) return undefined;
  return stickers.find((sticker) => sticker.id === id);
}

/**
 * Draws one sticker.
 *
 * `role="img"` with a label rather than `aria-hidden`: unlike an icon, this
 * *is* the message. Somebody using a screen reader has to be told what was
 * sent, not that a decoration is present.
 */
export function StickerArt({
  sticker,
  size = 128,
}: {
  sticker: Sticker;
  size?: number;
}) {
  return (
    <svg
      viewBox="0 0 96 96"
      width={size}
      height={size}
      role="img"
      aria-label={`${sticker.label} sticker`}
      className="shrink-0"
    >
      {sticker.art}
    </svg>
  );
}

/**
 * Matches a query against a sticker's label and its keywords.
 *
 * Substring rather than prefix, because people search stickers by what they
 * mean ("congrats") more often than by what they are called ("party").
 */
export function matchesSticker(sticker: Sticker, query: string): boolean {
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  if (sticker.label.toLowerCase().includes(needle)) return true;
  return sticker.keywords.some((word) => word.includes(needle));
}
