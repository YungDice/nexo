/**
 * The shapes the UI renders.
 *
 * These are *view* types, not wire types. `crates/protocol` owns what crosses
 * the network, and an `Envelope` there carries a ciphertext and nothing else
 * (§4.2). What arrives here has already been decrypted in Rust and handed over
 * IPC as plain values (rule 2), which is why a `Message` has a `body` and an
 * `Envelope` never could.
 *
 * M1 filled these from `src/mock`; M4 onwards fills them from the core, and
 * nothing about the components changed when it did — which was the point of
 * writing them as view types in the first place. The fixtures are still here
 * for the component tests and for a browser preview with no Tauri runtime.
 */

export type PresenceState = "online" | "away" | "offline";

export interface Person {
  id: string;
  /** 3–20 chars, [a-z0-9_], unique. Discovery is by handle only (§4.1). */
  handle: string;
  displayName: string;
  /** The in-app numeric ID from §4.1. Never a phone number. */
  userId: number;
  bio: string;
  location: string;
  links: string[];
  joinedAt: Date;
  presence: PresenceState;
  lastSeen: Date;
}

/** Where a message got to. `failed` is a real state, not an edge case (§6.1). */
export type DeliveryState =
  "sending" | "sent" | "delivered" | "read" | "failed";

export interface Attachment {
  id: string;
  name: string;
  /** Bytes. Rendered in the mono face so a column of them lines up. */
  size: number;
  mime: string;
  kind: "image" | "file";
}

export interface LinkPreview {
  url: string;
  title: string;
  description: string;
  /** §4.5: previews are generated client-side and are off by default. */
  source: string;
}

export interface Message {
  id: string;
  conversationId: string;
  authorId: string;
  body: string;
  at: Date;
  state: DeliveryState;
  attachments?: Attachment[];
  preview?: LinkPreview;
  /**
   * Rule 7: decryption failure is shown as a failure. There is no plaintext
   * fallback and no silent skip, so the UI needs a way to say "this one did
   * not open" — and that means carrying it in the model from the start.
   */
  undecryptable?: boolean;
  /**
   * Set when the payload decrypted but this build does not know its shape.
   *
   * Distinct from `undecryptable`: the message opened, and the remedy is an
   * update rather than asking the sender to try again. The bytes are kept in
   * the store, so a later build reads what arrived today.
   */
  unsupported?: string;
}

export interface Conversation {
  id: string;
  kind: "dm" | "group";
  /** A DM's title is the other person's display name; a group has its own. */
  title: string;
  memberIds: string[];
  unread: number;
  /** §4.1: safety numbers verified by hand. Unverified is not "insecure". */
  verified: boolean;
  /**
   * Somebody's key changed under a device this conversation already knew.
   *
   * Either they reinstalled or the server substituted a key. Nothing here can
   * tell those apart, which is why it is shown rather than resolved.
   */
  keyChanged?: boolean;
  keyChangedAtMs?: number | null;
  /** 60 digits, rendered as 12 groups of 5 (§4.1). */
  safetyDigits: string;
  /** Whether the conversation has a picture of its own. */
  hasAvatar?: boolean;
  /**
   * The newest message, as the core already computed it.
   *
   * The list needs this because only the *open* conversation's history is
   * loaded — every other row has no messages in memory to look at. Without it
   * the sidebar said "No messages yet" about conversations full of messages,
   * which reads as data loss rather than as a missing preview.
   */
  lastMessage?: string | null;
  /** Whether that message was ours, so the row can say who spoke last. */
  lastMessageOutgoing?: boolean | null;
  /**
   * @deprecated Muting lives in `conversationOverrides`, where it is a
   * deadline rather than a flag and survives a restart. Kept only because the
   * mock fixtures still set it; read `isMuted` from the store instead.
   */
  muted: boolean;
  /**
   * When the newest message in this conversation was sent.
   *
   * Comes from the store, not from whatever history happens to be loaded, so
   * ordering the list does not depend on which conversation is open. That
   * distinction is the whole point: opening a conversation must not reorder
   * it, only writing in it or being written to.
   */
  lastMessageAt?: Date;
}

export interface SharedLink {
  id: string;
  url: string;
  title: string;
  source: string;
  at: Date;
}

export interface Post {
  id: string;
  authorId: string;
  body: string;
  /** Media ids, not URLs: the tiles are generated, nothing is fetched. */
  media: string[];
  at: Date;
  reactions: { emoji: string; count: number; mine: boolean }[];
  comments: number;
}
