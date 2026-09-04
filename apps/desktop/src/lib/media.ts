import type { Attachment } from "./types";

/**
 * What shape a bubble should draw an attachment in.
 *
 * From the MIME type the sender declared, which is a hint and nothing more --
 * it is guessed from the file's extension on the way out and never verified.
 * That is fine for *this* decision, which only picks a player. What the page is
 * actually handed is decided in Rust from the bytes themselves
 * (`feed::sniff_mime`), so a `.png` full of HTML is refused there regardless of
 * what this said. Keeping the two apart is deliberate: one chooses a layout,
 * the other decides what may be rendered, and only the second is a security
 * question.
 *
 * A type this does not recognise is a file. There is no "probably audio"
 * branch, because a file row is a working answer for anything -- it saves and
 * opens elsewhere -- while a player that cannot play is a dead control.
 *
 * `recorded` is the sender saying they made this, which ends the guessing. It
 * is checked before the MIME type because the recorder writes WebM/Opus, and
 * that is indistinguishable by type from a video clip somebody attached.
 */
export function attachmentKind(
  mime: string,
  recorded = false,
): Attachment["kind"] {
  if (recorded) return "voice";
  const type = mime.toLowerCase();
  if (type.startsWith("image/")) return "image";
  if (type.startsWith("video/")) return "video";
  if (VOICE_TYPES.has(type)) return "voice";
  if (type.startsWith("audio/")) return "audio";
  return "file";
}

/**
 * The sound formats shown as a voice message rather than as a player.
 *
 * WAV and FLAC, because they are what a recorder writes when nothing has
 * compressed it yet: a file in one of them is far more often somebody talking
 * than it is music somebody chose to send. Everything else with sound in it --
 * mp3, m4a, ogg -- is a track, and gets the ordinary player with a scrubber.
 *
 * A reading of what arrives, not a fact about it -- and now only the fallback.
 * Since v0.1.21 a recording carries `voice` in its payload and says what it is,
 * so this list answers for messages sent before that and for files somebody
 * picked by hand.
 */
const VOICE_TYPES = new Set([
  "audio/wav",
  "audio/x-wav",
  "audio/wave",
  "audio/vnd.wave",
  "audio/flac",
  "audio/x-flac",
]);

/** Whether this is something the bubble plays rather than lists. */
export function isPlayable(kind: Attachment["kind"]): boolean {
  return kind !== "file";
}
