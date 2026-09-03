import { block } from "../../lib/blocks";
import { asFeedError } from "../../lib/feed";
import { confirm, notify } from "../../lib/native";
import type { Conversation } from "../../lib/types";

/**
 * Blocking somebody you are already talking to.
 *
 * Blocking existed before this and was reachable from three places — their
 * profile, their pin on the map, and the undo list in Settings — none of which
 * is where you are when somebody starts being a problem. You were in the
 * conversation. This is the entry that belongs there.
 *
 * # Why not in the conversation header too
 *
 * The header is three buttons for the things people do often. `MessagesHeader`
 * writes the rule down itself where it explains why mute *durations* are not
 * up there: the toolbar offers the plain, frequent action and the elaborate or
 * rare one lives in the row's own menu. Blocking is rare, destructive, and
 * wants a sentence of explanation before it happens — a permanent button for
 * it beside "show details" would be the loudest control in the room for the
 * thing you do least.
 */

/**
 * The other person in a two-person conversation, by handle.
 *
 * `memberIds` carries handles, despite the name, and may still include our
 * own — `title_from` in the core filters it the same way for the same reason.
 *
 * `undefined` for a group, and for a DM whose member list has not been filled
 * in yet: an action that needs a name it does not have is one the menu should
 * simply not offer. An empty array is the honest state right after joining
 * from a Welcome, not an error.
 */
export function peerHandle(
  conversation: Conversation,
  me: string | undefined,
): string | undefined {
  if (conversation.kind !== "dm") return undefined;
  const others = conversation.memberIds.filter((handle) => handle !== me);
  return others.length === 1 ? others[0] : undefined;
}

/**
 * Blocks them, after saying what that does and does not do.
 *
 * The wording is the one on their profile plus the part that only matters
 * here: the conversation does not vanish and neither does anything in it.
 * Blocking governs what happens next. What has already been delivered sits on
 * two machines and the server never held the keys to it, so nothing in this
 * app or on that server can take it back — and saying otherwise would be
 * exactly the overstatement rule 5 exists to prevent.
 *
 * Only blocking, not unblocking. Knowing whether somebody is already blocked
 * costs a round trip, and doing that for every row of a list to decide a menu
 * label would be a request per conversation for a word. Undo lives where it
 * already did: Settings → Privacy, and their profile.
 */
export async function blockPeer(handle: string, name: string): Promise<void> {
  const ok = await confirm(
    `Block ${name}?`,
    "Neither of you will be able to write to the other, and your posts leave each other's " +
      "feeds. This conversation stays where it is, on both machines — the server never had " +
      "the keys and cannot take it back. Blocking also cannot stop somebody making a " +
      "second account.",
  );
  if (!ok) return;
  try {
    await block(handle);
    notify("Blocked", `${name} can no longer reach you.`);
  } catch (error) {
    notify("Couldn't block", asFeedError(error).message);
  }
}
