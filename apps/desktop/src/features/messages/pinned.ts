import type { IconName } from "../../components/ui/Icon";
import type { Message } from "../../lib/types";

/** How a pinned message is listed when it is not a paragraph of text. */
export interface PinnedLine {
  /** What to draw beside it, or nothing for plain text. */
  icon?: IconName;
  /** The line itself. Never empty. */
  text: string;
  /**
   * True when the text is the app describing the message rather than the
   * message itself — a file name, or "Photo". Drawn quieter, because a
   * caption in the same weight as somebody's words reads as their words.
   */
  described: boolean;
}

/**
 * What the pinned list shows for one message.
 *
 * Every kind of message can be pinned, so every kind has to be listable, and
 * the list used to render `body || "(no text)"` — which meant pinning a
 * photograph put the words "(no text)" in the panel and lost the photograph.
 * A pin that shows nothing of what was pinned is a pin that did not work, as
 * far as the person who made it is concerned.
 *
 * The order matters: a caption wins over the file that carries it, because if
 * somebody wrote something they wrote it for a reason, and a name like
 * `IMG_4021.jpg` is not information.
 */
export function pinnedLine(message: Message): PinnedLine {
  if (message.retracted) {
    return { icon: "close", text: "Taken back", described: true };
  }
  if (message.undecryptable) {
    return { icon: "alert", text: "Could not be opened", described: true };
  }
  if (message.unsupported) {
    return { icon: "alert", text: "Needs a newer version", described: true };
  }

  const attachment = message.attachments?.[0];
  if (message.body) {
    // A caption keeps the icon of what it captions, so a pinned line still
    // says at a glance that there is a picture attached to those words.
    return attachment
      ? { icon: iconFor(attachment.kind), text: message.body, described: false }
      : { text: message.body, described: false };
  }
  if (attachment) {
    return {
      icon: iconFor(attachment.kind),
      // A voice message has no name worth showing -- whatever the recorder
      // called it is not what it is.
      text: attachment.kind === "voice" ? "Voice message" : attachment.name,
      described: true,
    };
  }
  // Nothing at all. Reachable, and better said plainly than left blank.
  return { text: "Empty message", described: true };
}

function iconFor(kind: NonNullable<Message["attachments"]>[number]["kind"]): IconName {
  switch (kind) {
    case "image":
      return "image";
    case "video":
      return "camera";
    case "voice":
      return "mic";
    case "audio":
      return "music";
    default:
      return "file";
  }
}
