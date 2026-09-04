# Retention Engines — Implementation Plan

**Goal:** Build the mechanics that make a messenger worth opening tomorrow —
engines 2–7 of the Telegram analysis and the whole build queue — without
weakening a single invariant. Nine waves, each shippable on its own.

**Scope:** Voice messages, replies, view-once media, typing indicators, chunked
attachment crypto and streaming, stickers, the control surface, and a follow
graph on the feed. One bugfix comes first because two waves stand on it.

**Honest size:** this is several sessions of work, not one. The waves are
ordered so that stopping after any of them leaves the repository in a state
somebody could ship.

---

## Global constraints

Every wave is bound by these; they are not restated per wave.

- **Invariant 1** — no cryptography is invented. Wave 5 uses AES-256-GCM
  exactly as `crates/crypto/src/attachment.rs` already does, segmented.
- **Invariant 2** — no key material in the WebView. Waves 1, 3 and 5 all move
  media; the key never crosses IPC, only decrypted bytes or a stream handle do.
- **Invariant 3** — no remote code. Rules out bots and mini-apps entirely, and
  constrains wave 6: a sticker is data this app renders, never a script.
- **Invariant 4** — the server never reads message content. Waves 1, 2, 3 and 6
  add `Payload` variants, which are inside MLS ciphertext by construction.
- **Invariant 5** — honest UI. Wave 3 is where this bites: view-once must not
  imply an enforcement it does not have.
- **Invariant 7** — fail closed. Wave 5's segment framing must fail
  authentication on a truncated or reordered stream, never play a short video.
- **Invariant 8** — every new dependency pinned exactly, both lockfiles
  committed.
- `crates/protocol`, `crates/crypto`, `crates/platform` must still compile
  unchanged for Android. Recording hardware is reached from the shell, never
  from `crates/client`.
- Each wave ends with `.\scripts\check.ps1` green and its own commit, authored
  by a human.

## The chain a new message kind travels

Established by tracing `Payload::Reaction`. Waves 1, 2, 3 and 6 each walk it:

`crates/protocol/src/lib.rs` → `crates/store/src/lib.rs` (schema + row) →
`crates/client/src/conversations.rs` → `apps/desktop/src-tauri/src/conversations.rs`
(IPC) → register in `src-tauri/src/lib.rs` → `apps/desktop/src/lib/conversations.ts`
+ `lib/types.ts` → `features/messages/*`.

Seven layers. A wave that touches five of them is normal; one that touches two
is suspicious.

---

## Wave 0 — Media can actually play *(bugfix, alone)* — **done**

`docs/STATUS.md` says video and sound play in the bubble as of v0.1.19. The CSP
in `tauri.conf.json` has no `media-src`, so it falls back to `default-src
'self'`, and `attachment_data_url` hands `<video>` a `data:` URL. `img-src`
lists `data:` and images therefore work; media almost certainly does not.

A fix and a feature never share a commit, so this is its own wave, and it comes
first because waves 1 and 5 are both unverifiable underneath it.

1. **Confirm before fixing.** `pnpm tauri dev`, send an mp4 and a wav, open the
   WebView console. A CSP violation there turns a suspicion into a bug.
2. Add `media-src 'self' data: blob:` to the CSP. `blob:` is included because
   wave 1's recorder produces one.
3. Correct the v0.1.19 entry in `STATUS.md` — it claims a thing that did not
   work.

| File | Change |
|---|---|
| `apps/desktop/src-tauri/tauri.conf.json` | `media-src` in the CSP |
| `docs/STATUS.md` | correct the claim |

**Done when:** an mp4 and a wav play in a bubble on a real run, and the console
is clean.

---

## Wave 1 — Voice messages *(queue 1, engine 4)* — **done**

Playback, the byte-sniffer and the pinned-panel label all exist. Capture does
not, and `Composer.tsx:15` admits the button is a label. `lib/media.ts` already
specifies the right design: **flag a voice message in the payload** rather than
guessing from MIME, and let the WAV/FLAC list become the fallback for old
messages.

- Recording happens in the WebView with `MediaRecorder` — it is the only audio
  capture available without a new native dependency, and it keeps
  `crates/client` free of platform calls. Output is WebM/Opus, which the
  existing player handles.
- `Payload::Attachment` gains `voice: Option<VoiceMeta>` carrying duration and
  a coarse waveform (a handful of amplitude buckets, computed at record time).
  `Option` because absence must stay representable for every message already
  sent — the same reasoning the `id` field documents.
- The waveform is drawn from those buckets. It is decoration with a purpose:
  it makes a 4-second note visibly different from a 40-second one before you
  commit to listening.
- Microphone permission is asked for on first use and the denial path renders
  as a normal disabled control with a reason, not a dialog.

| File | Change |
|---|---|
| `crates/protocol/src/lib.rs` | `VoiceMeta`, `voice` field on `Attachment` |
| `crates/store/src/lib.rs` | persist and read it back |
| `crates/client/src/conversations.rs` | thread it through send and load |
| `apps/desktop/src-tauri/src/conversations.rs` | accept it on the send command |
| `apps/desktop/src/features/messages/Composer.tsx` | record, hold-to-talk, cancel, send |
| `apps/desktop/src/features/messages/MessageList.tsx` | waveform + duration bubble |
| `apps/desktop/src/lib/media.ts` | prefer the flag, keep the list as fallback |
| `apps/desktop/src/lib/types.ts` | the TS mirror |

**Done when:** hold, speak, release sends a note that plays back with its
waveform on both sides, and a denied microphone explains itself.

---

## Wave 2 — Replies and quotes *(queue 2)* — **done**

The conversational primitive people miss fastest, and what decides whether a
group of five reads as a conversation or as noise.

- A new `Payload::Reply { to: Uuid, body: String, id: Option<Uuid> }`, modelled
  on `Edit` and `Retract`, which already name a target by the sender's own
  message id rather than the envelope id.
- A reply to a message this device never received, or one since retracted,
  renders as a stub that says so. That is the fail-closed reading: never drop
  the reply, never invent the quoted text.
- Swipe-to-reply is not a desktop gesture; the entry belongs in the existing
  context menu and on a hover affordance.
- Clicking the quote scrolls to the original and flashes it — the jump-to
  behaviour `MessageList` already has for the pinned panel.

| File | Change |
|---|---|
| `crates/protocol/src/lib.rs` | the `Reply` variant |
| `crates/store/src/lib.rs` | schema bump, target column, index |
| `crates/client/src/conversations.rs` | resolve the target on load |
| `apps/desktop/src-tauri/src/conversations.rs` | `send_reply` IPC |
| `apps/desktop/src-tauri/src/lib.rs` | register it |
| `apps/desktop/src/features/messages/MessageList.tsx` | quote block, jump, menu entry |
| `apps/desktop/src/features/messages/Composer.tsx` | the replying-to strip |

**Done when:** replying quotes correctly, the quote jumps to its target, and a
reply to a deleted message says what happened.

---

## Wave 3 — View-once media *(queue 3, engine 5)*

Wave 7's story pipeline is this feature with a different expiry rule. Reuse it
rather than inventing a second ephemeral path.

- `Payload::ViewOnce`, carrying the same fields as an encrypted attachment.
- **The key is the expiry mechanism.** On open, the local copy of the key is
  destroyed. Re-opening then fails cryptographically on the recipient's own
  device rather than being refused by a client that could be patched — which is
  strictly better than what Telegram offers, and worth saying plainly.
- The server refuses a URL for an already-opened object, and the object-store
  lifecycle rule drops the bytes. Three independent layers, no scheduled job —
  the shape wave 7 already established.
- Bytes are never written to the store in plaintext, and never handed to the
  WebView except while the viewer is open.
- **Invariant 5 is the hard part.** The composer says, at the point of sending,
  that the key is destroyed after one view and that a screenshot is not
  something Nexo can prevent. No screenshot notification — it is unreliable on
  Windows and implies a guarantee `THREAT-MODEL.md` §4 explicitly disclaims.
- One-to-one conversations only, as Telegram does. A view-once in a group means
  "once per member", which is a different feature wearing the same word.

| File | Change |
|---|---|
| `crates/protocol/src/lib.rs` | the `ViewOnce` variant |
| `crates/crypto/src/attachment.rs` | reuse; no new primitive |
| `crates/store/src/lib.rs` | key destruction on open, no plaintext at rest |
| `crates/client/src/conversations.rs` | open-once state machine |
| `apps/server/src/` | refuse a URL for an opened object |
| `apps/desktop/src/features/messages/` | locked bubble, viewer, the honest line |
| `docs/THREAT-MODEL.md` | a section for what view-once does and does not buy |

**Done when:** a view-once photo opens exactly once, the second attempt fails
because the key is gone, and the UI never claims more than that.

---

## Wave 4 — Typing indicators *(queue 4)*

`apps/server/src/stream/mod.rs:158` already relays a `Typing` event with a
comment explaining it is opt-out-able and carries no content. No client ever
sends or draws one. This is the cheapest wave here.

- Send on composer input, throttled, and stop on send, on blur and on a timeout.
- Respect the existing preference rather than adding a second one; `showPresence`
  in `ConversationList.tsx` is currently voided.
- Render in the conversation header and as a row in the list.

| File | Change |
|---|---|
| `crates/client/src/transport.rs` | send the event |
| `apps/desktop/src-tauri/src/conversations.rs` | IPC for typing start/stop |
| `apps/desktop/src/features/messages/Composer.tsx` | throttled emit |
| `apps/desktop/src/features/messages/ConversationList.tsx` | consume `showPresence` |

**Done when:** two accounts see each other type, and the preference genuinely
turns it off in both directions.

---

## Wave 5 — Chunked attachment crypto and streaming media *(queue 5, engine 2)*

The one infrastructure wave, and the only item here that is a week or more.
Everything in engine 2 is downstream of it.

**The problem.** `attachment.rs` encrypts a whole object under one AES-256-GCM
tag, so no byte range can be decrypted without all of it.
`attachment_data_url` therefore fetches, decrypts and verifies everything, caps
inline media at 12 MB, and base64s the result across IPC. A 12 MB video becomes
a ~16 MB string in the WebView before a frame is shown.

**The fix, in two halves.**

1. **Segment framing** in `crates/crypto`. Fixed-size plaintext segments, one
   key, a nonce derived from the segment index, and AAD binding each segment to
   its index *and* the total count — so a dropped, reordered or truncated
   stream fails authentication instead of playing a shortened video
   (invariant 7). The whole-file SHA-256 stays as it is.
2. **A streaming protocol handler** in the Tauri shell, replacing `data:` URLs
   for media. It answers HTTP range requests by decrypting only the segments a
   range touches. This removes the 12 MB cap, the base64 inflation, and the
   need for `data:` in `media-src` at once.

Then engine 2's non-crypto half, which is what a user actually feels:

- A poster frame extracted at send time and carried in the payload, so a video
  arrives as a picture rather than a black rectangle.
- Duration in the corner of that poster.
- Silent looping autoplay for clips under a threshold, off when
  `prefers-reduced-motion` is set.
- MP4 index normalisation at send, so playback can start before the file has
  fully arrived — the thing that makes streaming actually stream.

| File | Change |
|---|---|
| `crates/crypto/src/attachment.rs` | segment framing + tests for truncation and reorder |
| `crates/protocol/src/lib.rs` | poster frame, duration, segment size |
| `apps/desktop/src-tauri/src/` | the range-serving protocol handler |
| `apps/desktop/src-tauri/tauri.conf.json` | allow the new scheme in `media-src` |
| `apps/desktop/src/features/messages/MessageList.tsx` | poster, duration, autoloop |
| `apps/desktop/src/features/messages/Composer.tsx` | extract the poster at send |
| `docs/CONTEXT.md` | the new scheme is a convention worth naming |

**Done when:** a 200 MB video starts playing within a second of tapping it,
scrubs without a full download, and a truncated stream refuses to play at all.

---

## Wave 6 — Stickers and custom emoji *(queue 6, engine 6)*

Cheap, heavily used, and the one item people evangelise. No server intelligence
and no crypto risk: a sticker reference is content like any other.

- A pack is a bundle of images plus a manifest. **Invariant 3 forbids a script
  in a pack** — no Lottie evaluation, no remote fetch. Static and APNG-style
  animation only, rendered by this app.
- Packs ship with the app first. User-supplied packs are a later question and
  are explicitly out of this wave, because they are a moderation surface.
- `Payload::Sticker { pack, id }`, so a sticker costs a few bytes rather than an
  attachment round trip.
- The picker sits beside the emoji picker, which already handles 1,914 entries
  quickly and gives the interaction pattern to copy.

| File | Change |
|---|---|
| `crates/protocol/src/lib.rs` | the `Sticker` variant |
| `packages/design-tokens/` or a new asset package | the bundled packs |
| `apps/desktop/src/components/ui/StickerPicker.tsx` | new, modelled on `EmojiPicker` |
| `apps/desktop/src/features/messages/MessageList.tsx` | render a sticker bubble |
| `docs/CONTEXT.md` | new package in the map |

**Done when:** a sticker sends, arrives and renders on both sides, and nothing
in a pack can execute.

---

## Wave 7 — The control surface *(engine 7)*

None of this acquires a user; all of it prevents the moment a heavy user
decides the app is unmanageable. Its value scales with how much traffic the
other waves generate, which is why it sits here rather than earlier.

- **Folders** — user-defined, local, with rules simple enough to explain in one
  line. Not Telegram's full filter language.
- **Archive** — a conversation out of the way without leaving it or muting it.
- **Drafts** — persisted per conversation in the encrypted store. Losing a typed
  paragraph by switching conversations is a small betrayal that people remember.
- All three are local state. Nothing new reaches the server, which is the point:
  this wave costs the threat model nothing.

| File | Change |
|---|---|
| `crates/store/src/lib.rs` | schema bump: folders, archived flag, drafts |
| `crates/client/src/conversations.rs` | read and write them |
| `apps/desktop/src-tauri/src/conversations.rs` | IPC |
| `apps/desktop/src/features/messages/ConversationList.tsx` | folder rail, archive |
| `apps/desktop/src/features/messages/Composer.tsx` | restore a draft |

**Done when:** a draft survives switching away and back, an archived
conversation stays out of the list until it matters, and folders filter.

---

## Wave 8 — A follow graph on the feed *(engine 3)*

The largest wave, and the one that changes what the product is: a reason to open
the app when nobody has messaged you.

Nexo already has the right shape — the feed, profiles and stories are already a
server-readable public layer, so this is not a new architectural category. It is
the feed with an author and a follow relation.

- A `follows` table, follow and unfollow routes, and a feed that can be filtered
  to people you follow instead of one global reverse-chronological list.
- Blocks apply in both directions, reusing `blocked_between` rather than adding
  a second rule — the reasoning wave 7's stories already established.
- A private account's posts are not followable by a stranger; this must reuse
  wave 6's privacy checks in `search_users` and not re-implement them.
- **Deliberately not in scope: unbounded public groups.** 200,000-member groups
  are a moderation obligation, not a feature, and
  `RESEARCH-COMPARISON.md` already flags reporting as higher-priority than its
  feature weight suggests. The public layer grows no faster than the tools to
  police it.

| File | Change |
|---|---|
| `apps/server/migrations/` | the `follows` table |
| `apps/server/src/feed.rs`, `profiles.rs` | routes, filtered feed, block rules |
| `crates/client/src/feed.rs` | the client half |
| `apps/desktop/src/features/home/HomePage.tsx` | Following vs Everyone |
| `apps/desktop/src/features/profile/PublicProfile.tsx` | the follow control |
| `docs/CONTEXT.md` | the route table and its count |

**Done when:** following someone changes what the feed shows, blocks still win,
and a private account cannot be followed by a stranger.

---

## What is deliberately not here

Named so that leaving them out is visibly a decision, not an omission:

- **Cloud history and multi-device.** The largest engine Telegram has, and
  unavailable without a server that can read messages. Refused already.
- **Bots and mini-apps.** Third-party code in the client, which invariant 3
  forbids outright. There is no version that keeps the guarantee.
- **Screenshot notification.** Unreliable on Windows and dishonest anywhere:
  the local device is out of scope in `THREAT-MODEL.md` §4.
- **Round video messages.** The other half of engine 4. Worth doing, and it
  belongs after wave 5, since it wants the same capture and streaming paths.

## Documentation kept in step

Per `CLAUDE.md`, in the same commit as the work:

- `docs/CONTEXT.md` — the IPC table and its count (waves 1–4, 7), the route
  table (wave 8), the map (waves 5, 6), and conventions (wave 5's scheme).
- `docs/STATUS.md` — after every wave.
- `docs/THREAT-MODEL.md` — wave 3, and wave 5 if segment framing changes what
  an attacker can do to a stream.
- The `RESEARCH-COMPARISON.md` comparison table is **stale today**: it lists
  editing, delete-for-everyone, history search and reporting as absent, and all
  four have shipped. Correcting it is a separate commit, not folded into a wave.
