import { invoke } from "@tauri-apps/api/core";

/**
 * Blocking (§6.1).
 *
 * Every one of these is a round trip to the server, and that is the point.
 * A block applied in here would change nothing: the other person would go on
 * sending, the server would go on accepting, and only this one app would look
 * away. The server drops their posts from the feed and refuses to open a
 * conversation between you — which is a thing the word can honestly mean.
 *
 * What it cannot do is stop somebody making a second account. The UI says so
 * where blocking is offered, because a security promise that overstates itself
 * is worse than none (rule 5).
 */
export interface Block {
  handle: string;
  display_name: string;
  blocked_at_ms: number;
}

/** Everyone you are blocking, newest first. */
export function listBlocks(): Promise<Block[]> {
  return invoke<Block[]>("blocks");
}

/** Blocks somebody. Doing it twice is not an error. */
export function block(handle: string): Promise<void> {
  return invoke<void>("block", { handle });
}

/** Unblocks somebody. Doing it twice is not an error. */
export function unblock(handle: string): Promise<void> {
  return invoke<void>("unblock", { handle });
}
