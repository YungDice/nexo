import { invoke } from "@tauri-apps/api/core";

/**
 * Meet&Greet, as the WebView sees it.
 *
 * **None of this is end-to-end encrypted.** A pin, a headline and a character
 * are readable by the server and by every signed-in person, exactly like a
 * profile or a post. That is the design, not an oversight, and `MeetAgreement`
 * says so in plain words before anyone appears on the map — rule 5 makes a
 * feature that implies privacy it does not have worse than one that never
 * claimed any.
 *
 * Two things a future change should not quietly undo:
 *
 * **Nexo never reads device location.** There is no `navigator.geolocation`
 * call anywhere in this feature and no bridge here that could carry one. A pin
 * is a place somebody dragged onto a map.
 *
 * **The pin that comes back is not the pin that was sent.** The server snaps
 * it to a grid and offsets it. Anything drawing your own pin uses the answer
 * from {@link setMyPin}, never the value it submitted, or it would show a
 * precision that no longer exists.
 */

/** The agreement's version. Must match the server's `CONSENT_VERSION`. */
export const AGREEMENT_VERSION = 1;

/** Somebody on the map. */
export interface Pin {
  handle: string;
  display_name: string;
  /** Already coarsened by the server. Never a measurement. */
  lat: number;
  lon: number;
  headline: string | null;
  /**
   * The NexoChar, as its generator config. Opaque to Rust and to the server;
   * `NexoChar` is the only thing that reads it.
   */
  char_config: unknown;
  updated_at_ms: number;
}

/** The map, and how old it is. */
export interface MeetMapData {
  pins: Pin[];
  /** When these were fetched. `0` before the first successful fetch. */
  fetched_at_ms: number;
  /**
   * True when the server could not be reached and this is the cached copy.
   * The UI says so rather than presenting old pins as current.
   */
  stale: boolean;
}

/** One intro waiting for an answer. */
export interface MeetRequest {
  id: number;
  from_handle: string;
  conversation_id: string;
  created_at_ms: number;
}

/** What the studio saves. Every field is optional: this is a patch. */
export interface PinUpdate {
  lat?: number;
  lon?: number;
  headline?: string;
  char_config?: unknown;
  active?: boolean;
}

/**
 * Everyone on the map.
 *
 * Fetched when the tab opens and when somebody asks again — never on a timer,
 * and never through `syncAgent`, which belongs to messages. A map that polled
 * would turn "where somebody said they are" into "where somebody is now".
 */
export function meetPins(): Promise<MeetMapData> {
  return invoke<MeetMapData>("meet_pins");
}

/** My own pin, or `null` when I am not on the map. */
export function myPin(): Promise<Pin | null> {
  return invoke<Pin | null>("meet_me");
}

/** Place or move my pin. Returns what the server stored, not what was sent. */
export function setMyPin(update: PinUpdate): Promise<Pin | null> {
  return invoke<Pin | null>("meet_set_me", { request: update });
}

/** Come off the map. The character survives. */
export function leaveMap(): Promise<void> {
  return invoke<void>("meet_leave");
}

/** Accept the agreement. */
export function acceptAgreement(): Promise<void> {
  return invoke<void>("meet_consent", { version: AGREEMENT_VERSION });
}

/** Intros waiting for me. */
export function meetRequests(): Promise<MeetRequest[]> {
  return invoke<MeetRequest[]>("meet_requests");
}

/**
 * Mark an already-opened conversation as an intro.
 *
 * Call order matters and belongs to the caller: open the conversation with
 * `startConversation`, send the one message through the ordinary path, then
 * call this. The other order leaves a request pointing at nothing.
 */
export function sendIntro(
  handle: string,
  conversationId: string,
): Promise<MeetRequest> {
  return invoke<MeetRequest>("meet_send_request", {
    handle,
    conversationId,
  });
}

/** Accept an intro, which lifts the one-message cap. */
export function acceptIntro(id: number): Promise<void> {
  return invoke<void>("meet_accept_request", { id });
}

/** Decline an intro. */
export function declineIntro(id: number): Promise<void> {
  return invoke<void>("meet_decline_request", { id });
}

/** Why something was reported. The server accepts exactly these. */
export type ReportReason =
  "spam" | "harassment" | "illegal" | "impersonation" | "other";

/**
 * Report a person.
 *
 * Blocking answers "I do not want to see this person"; reporting answers "this
 * should not be here", and only the second asks somebody else to look. The
 * reporter is told it was received and nothing more — not whether others
 * reported the same account, which would make reporting a way of learning
 * about other people.
 */
export function reportUser(
  userId: number,
  reason: ReportReason,
  note?: string,
): Promise<void> {
  return invoke<void>("meet_report", {
    subjectKind: "user",
    subjectId: userId,
    reason,
    note: note ?? null,
  });
}

/** Why a Meet&Greet call failed. Match on `kind`, never on `message`. */
export interface MeetError {
  kind:
    | "unreachable"
    | "signed_out"
    | "not_found"
    | "rejected"
    | "consent_required"
    | "internal";
  message: string;
}

/** Narrows an unknown thrown value to something the UI can show. */
export function asMeetError(error: unknown): MeetError {
  if (
    error &&
    typeof error === "object" &&
    "kind" in error &&
    "message" in error
  ) {
    return error as MeetError;
  }
  return { kind: "internal", message: "Something went wrong. Try again." };
}

// ------------------------------------------------- public and private ---

/** Somebody a search turned up. */
export interface SearchResult {
  handle: string;
  display_name: string;
  avatar_key: string | null;
}

/**
 * A freshly minted invitation.
 *
 * `secret` is readable **once**. The server keeps only a hash, so a lost
 * secret cannot be looked up — it is revoked and replaced, the same answer a
 * password reset gives.
 */
export interface MintedInvite {
  id: number;
  secret: string;
  expires_at_ms: number;
}

/** One invitation afterwards. */
export interface Invite {
  id: number;
  label: string | null;
  created_at_ms: number;
  expires_at_ms: number;
  revoked: boolean;
  /** Usable right now. Expiry is by the clock, not by a cleanup job. */
  live: boolean;
  /** How many people reached you through it. */
  used: number;
}

/**
 * Find people by handle or display name.
 *
 * **Private accounts are absent**, and the server decides that — a directory
 * the client trims is one anybody can untrim.
 */
export function searchUsers(term: string): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("meet_search", { term });
}

/** Mint an invitation. At most seven days. */
export function createInvite(
  label: string | undefined,
  days: number,
): Promise<MintedInvite> {
  return invoke<MintedInvite>("meet_create_invite", {
    label: label ?? null,
    days,
  });
}

/** My invitations, live and spent. */
export function listInvites(): Promise<Invite[]> {
  return invoke<Invite[]>("meet_invites");
}

/** Withdraw one. The record stays, so requests can still say where they came from. */
export function revokeInvite(id: number): Promise<void> {
  return invoke<void>("meet_revoke_invite", { id });
}

// ---------------------------------------------------------------- stories ---

/**
 * One story this device holds.
 *
 * The key that opens it is **not** here and never crosses the IPC seam — the
 * page asks for a story by id and Rust hands back bytes, exactly as with an
 * attachment (rule 2).
 */
export interface Story {
  id: number;
  /**
   * Who posted it, when known. **Empty for a story that arrived over the
   * wire**: an envelope names a device, not an account, and inventing a handle
   * from a device id would put a UUID under somebody's story.
   */
  author_handle: string;
  /** The device that sent it, resolved to a person the same way a message is. */
  author_device_id: string;
  mime: string;
  created_at_ms: number;
  /** When it stops being available. At most 24 hours after it was posted. */
  expires_at_ms: number;
}

/**
 * Post a story.
 *
 * Encrypted once and uploaded once; the key is then sent down every
 * conversation you already have. Your contacts are whoever you share a
 * conversation with, which is the same definition the server uses, and
 * blocking therefore takes effect without any story-specific code.
 */
export function postStory(path: string): Promise<number> {
  return invoke<number>("story_post", { path });
}

/**
 * Stories this device holds.
 *
 * Reading is also what ends the expired ones: the call deletes them and their
 * keys as it goes. That is the layer that actually makes a story disappear —
 * ciphertext without its key is nothing — and it works offline.
 */
export function listStories(): Promise<Story[]> {
  return invoke<Story[]>("story_list");
}

/**
 * A story's bytes, as a `data:` URL.
 *
 * The key that opens it stays in Rust — the page asks by id and gets pixels,
 * never what decrypted them.
 */
export function openStory(id: number): Promise<string> {
  return invoke<string>("story_open", { id });
}
