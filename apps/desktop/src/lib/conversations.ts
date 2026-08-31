import { invoke } from "@tauri-apps/api/core";

/**
 * Conversations, as the WebView sees them.
 *
 * Rule 2 again: everything here is already decrypted. There is no ciphertext in
 * any of these types, no epoch, and no key — the WebView is never told that MLS
 * exists, only that messages arrive.
 */
export interface Conversation {
  conversation_id: string;
  kind: string;
  /**
   * What to call it. `null` for a conversation joined from a Welcome, which
   * this device has no name for until M7's profile fetch.
   */
  title: string | null;
  members: string[];
  last_message: string | null;
  last_message_at_ms: number | null;
  /**
   * Whether this device sent the most recent message. What decides that a
   * conversation whose newest message is our own never toasts and never
   * counts as unread.
   */
  last_message_outgoing: boolean | null;
  /** Whether a picture has been set. */
  has_avatar: boolean;
}

export interface Message {
  envelope_id: number;
  /** Absent for our own messages — that absence is what `outgoing` reads. */
  sender_device_id: string | null;
  body: string;
  sent_at_ms: number;
  outgoing: boolean;
  /**
   * True while the message sits in the offline queue (M8). The server has not
   * seen it yet; telling someone it was sent would be the one lie a messenger
   * cannot afford (rule 7).
   */
  pending: boolean;
  /** Present when the message carries a file. */
  attachment: Attachment | null;
}

/**
 * What a bubble knows about an attached file.
 *
 * Deliberately just three fields. The S3 key, the AES key, and the nonce stay
 * in Rust — the WebView asks to save an attachment by envelope id, and never
 * holds anything that could open one (rule 2).
 *
 * `name` has already been sanitised on the Rust side, but it was still chosen
 * by the sender, so render it as text and never as a path.
 */
export interface Attachment {
  name: string;
  mime: string;
  size: number;
}

/** Bytes, as a short human string. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export interface SyncResult {
  messages: number;
  commits: number;
  /** Envelopes that could not be read. Rule 7: shown, never hidden. */
  failed: number;
  /**
   * Which conversations received messages, and how many each (§8). Only
   * conversations with at least one new incoming message appear; our own
   * sends are written locally at send time and are never an arrival.
   */
  arrivals: { conversation_id: string; messages: number }[];
}

/** What a flush of the offline queue did (M8). */
export interface FlushResult {
  sent: number;
  /**
   * Messages the server already had, matched by their client id — the
   * duplicates idempotency prevented. Reported apart from `sent` because
   * calling them "sent" would overstate what just happened.
   */
  already_sent: number;
  still_queued: number;
  failed: number;
}

export interface ConversationError {
  kind:
    | "unreachable"
    | "signed_out"
    | "stale_epoch"
    | "rejected"
    | "not_a_member"
    | "unreadable_file"
    | "unwritable_file"
    | "too_large"
    | "not_an_attachment"
    | "invalid_request"
    | "internal";
  message: string;
}

/** Narrows an unknown rejection to something renderable. */
export function asConversationError(error: unknown): ConversationError {
  if (
    typeof error === "object" &&
    error !== null &&
    "kind" in error &&
    "message" in error
  ) {
    return error as ConversationError;
  }
  return { kind: "internal", message: "Something went wrong. Try again." };
}

export function listConversations(): Promise<Conversation[]> {
  return invoke<Conversation[]>("list_conversations");
}

/**
 * Removes a conversation from this device.
 *
 * Local, and the wording everywhere says so. The other members keep theirs,
 * and a message sent afterwards brings the conversation back with that message
 * in it — which is the honest behaviour and what the confirmation warns about.
 */
export function deleteConversation(conversationId: string): Promise<void> {
  return invoke<void>("delete_conversation", { conversationId });
}

export function startConversation(handle: string): Promise<string> {
  return invoke<string>("start_conversation", { handle });
}

export function sendMessage(
  conversationId: string,
  body: string,
): Promise<Message> {
  return invoke<Message>("send_message", { conversationId, body });
}

export function syncConversation(conversationId: string): Promise<SyncResult> {
  return invoke<SyncResult>("sync_conversation", { conversationId });
}

/** Pulls everything new, for the startup and periodic passes. */
export function syncAll(): Promise<SyncResult> {
  return invoke<SyncResult>("sync_all");
}

/**
 * Sends everything waiting in the offline queue (M8).
 *
 * Called before every sync pass. Being offline is not an error — it is the
 * state the queue exists for — so an unreachable server resolves with
 * everything still queued rather than rejecting.
 */
export function flushOutbox(): Promise<FlushResult> {
  return invoke<FlushResult>("flush_outbox");
}

/** How many messages are waiting to be sent. */
export function outboxCount(): Promise<number> {
  return invoke<number>("outbox_count");
}

export function conversationMessages(
  conversationId: string,
): Promise<Message[]> {
  return invoke<Message[]>("conversation_messages", { conversationId });
}

/**
 * The safety number for a 1:1 conversation (§4.1).
 *
 * `null` for a group: a safety number is a fingerprint over *both* parties, and
 * there is no meaningful one to show for five.
 */
/**
 * An attached image, decrypted, as a `data:` URL.
 *
 * The envelope id is all the page sends; the S3 key and the AES key stay in
 * Rust, which downloads, decrypts and verifies before any byte comes back.
 * Rejects anything that is not really an image, whatever the sender named it.
 */
export function attachmentDataUrl(envelopeId: number): Promise<string> {
  return invoke<string>("attachment_data_url", { envelopeId });
}

/** Starts a group conversation with several handles at once. */
export function startGroup(handles: string[], title: string): Promise<string> {
  return invoke<string>("start_group", { handles, title });
}

/**
 * Renames a conversation for everyone in it.
 *
 * Sent as an encrypted message, not written to the server: what people call
 * their group is content, and the server has no title to leak.
 */
export function renameConversation(conversationId: string, title: string): Promise<void> {
  return invoke<void>("rename_conversation", { conversationId, title });
}

/**
 * Sets the conversation's picture from a file on disk.
 *
 * The path goes in; the bytes never come through here. Rust reads, encrypts and
 * uploads them, and the key that opens the object travels inside an MLS
 * message — so every member sees it and the server does not.
 */
export function setConversationAvatar(conversationId: string, path: string): Promise<void> {
  return invoke<void>("set_conversation_avatar", { conversationId, path });
}

/** The conversation's picture as a `data:` URL, or `null` if it has none. */
export function conversationAvatar(conversationId: string): Promise<string | null> {
  return invoke<string | null>("conversation_avatar", { conversationId });
}

/** One image, video or file in a conversation. */
export interface AttachmentEntry {
  /** All the page sends to ask for the bytes. */
  envelope_id: number;
  kind: "image" | "video" | "file";
  name: string;
  mime: string;
  size: number;
  sent_at_ms: number;
  outgoing: boolean;
}

/**
 * Every attachment in a conversation, oldest first.
 *
 * Read from the local store, so it works offline and costs no round trip. No
 * payloads come back — they hold decryption keys; the bytes are fetched one at
 * a time by envelope id.
 */
export function conversationAttachments(conversationId: string): Promise<AttachmentEntry[]> {
  return invoke<AttachmentEntry[]>("conversation_attachments", { conversationId });
}

/** Adds someone to a conversation that already exists. */
export function addToConversation(conversationId: string, handle: string): Promise<void> {
  return invoke<void>("add_to_conversation", { conversationId, handle });
}

export function safetyNumber(conversationId: string): Promise<string | null> {
  return invoke<string | null>("safety_number", { conversationId });
}

/**
 * How often the app pulls, in milliseconds.
 *
 * Polling rather than the WebSocket for now. The socket exists on the server
 * and removes this latency, but `sync` is the source of truth either way — a
 * client that misses an event repairs itself from its cursor — so polling is
 * correct, just slower to notice. Wiring the socket changes how fast this
 * happens, not whether it works.
 */
export const SYNC_INTERVAL_MS = 4000;

/**
 * Sends a file that the user already picked.
 *
 * Only the **path** is passed to Rust. The bytes are read, encrypted, and
 * uploaded there — a 20 MB file does not cross the IPC bridge, and its
 * plaintext never enters the WebView's heap (rule 2).
 */
export function sendAttachment(
  conversationId: string,
  path: string,
  body?: string,
): Promise<Message> {
  return invoke<Message>("send_attachment", {
    conversationId,
    path,
    body: body?.trim() ? body.trim() : null,
  });
}

/**
 * Downloads, decrypts, and writes one attachment to a path the user chose.
 *
 * Resolves to the number of bytes written. Rust only writes after both the
 * GCM tag and the SHA-256 have matched, so a partial or tampered file never
 * reaches disk (rule 7).
 */
export function saveAttachmentTo(
  envelopeId: number,
  path: string,
): Promise<number> {
  return invoke<number>("save_attachment", { envelopeId, path });
}
