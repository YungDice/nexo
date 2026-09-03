# Status at v0.1.16

What the app actually does today.

**The two sections below stop at v0.1.3.** Everything after that heading was
written against that release and has not been re-walked; several things it
calls absent have since been built. What landed after it is listed under
*Since v0.1.3* at the foot of this file. Re-walking the older sections is worth
doing and has not been done — saying so is better than leaving a reader to
discover it.

Written by walking the code, not the commit messages: every line below was
checked against the file it names. `docs/PLAN.md` tracks the milestones M0–M9;
this document covers what landed after them, in the round of fixes and
features that followed the first real test on Windows.

---

## Fixed

Twelve problems were found by using the app rather than by reading it. All
twelve are fixed.

| # | What was wrong | Where it was fixed |
|---|---|---|
| D1 | Attachment images were never drawn. `ImageGrid` rendered an empty `div` with a generated gradient, and the filename survived only as a tooltip — so a photo arrived as a coloured rectangle. | `attachment_data_url` in `apps/desktop/src-tauri/src/conversations.rs`, capped by `MAX_INLINE_IMAGE_BYTES`. Rust still decrypts and verifies before anything is shown. |
| D2 | Links sat under the message as an attachment instead of being part of it. | `MessageList.tsx` renders the URL inside the text and hands it to `openUrl`, so it opens in the system browser and never inside the WebView. The context menu offers *Open link* and *Copy address*. |
| D3 | Link previews appeared not to work. | Nothing was broken: the machinery was already there and the setting is **off by default** (§4.5). Left as it was, deliberately — a preview is a request to a stranger's server, and that stays opt-in. |
| D4 | Every avatar in a conversation was generated from an id, so nobody's real picture ever appeared. | `ConversationAvatar.tsx` and `HandleAvatar.tsx` resolve the member's profile and pass a real `imageUrl` through to `Avatar`. |
| D5 | Images inside a profile did not load. | `ProfilePage.tsx` now routes them through `RemoteImage`, the same presigned-GET path the banner and avatar already used. |
| D6 | The profile's tabs mixed two ideas: selection was marked with an underline, but hovering filled a rounded box — a preview of a shape that never appeared. | `Tabs` in `components/ui/Controls.tsx` now uses one underline in three states, so hovering previews where the indicator will land. Keyboard focus gained an explicit ring, which the box had been standing in for. |
| D7 | The button beside the chats carried the sign-out icon but read *Leave this conversation* — and did nothing but open a dialog. Sign-out itself was buried in the profile. | The real sign-out moved to `IconRail.tsx`, visible from every page, red on hover. |
| D8 | Home had a button that toggled a search box which the page already showed. | Removed; the search field is simply always there. |
| D9 | The Appearance section always showed the moon, whichever theme was on. | Its icon is now derived from the active theme — the list entry carries a placeholder that the render replaces. |
| D10 | Auto-lock did not lock. Content disappeared, but the session stayed open — a security promise the app did not keep. | Locking now ends the session for real. Getting back in needs the PIN (see N11) or the password. |
| D11 | Confirmations and notices were operating-system dialogs that looked nothing like the app. | `DialogHost.tsx` and `Modal.tsx` replace them; `notify` and `confirm` route through the app's own surfaces. |
| D12 | Right-clicking anywhere produced the browser's menu. | Suppressed globally in `main.tsx`, with the app's own menu in its place (N7). |

Nothing from that round is outstanding.

---

## Built

Twelve features, in the order they matter to someone using the app.

### Messages

- **Full-size media viewer** — clicking an image or video opens it over the
  conversation. `features/messages/Lightbox.tsx`.
- **Media strip** — under the viewer, every image and video in the
  conversation, so you jump straight to one instead of scrolling back.
- **Jump to latest** — scrolling up reveals a button back to the bottom, and
  when messages arrive meanwhile it carries the count of what you missed.
  `MessageList.tsx`.
- **Composer in an empty conversation** sits with the invitation in the middle
  of the page and only moves to the bottom once there is history above it.
- **Full emoji set** with categories and search, replacing the eight that were
  hardcoded. `components/ui/EmojiPicker.tsx`.

### Throughout

- **The app's own context menu** on right-click, with entries that suit what
  was clicked — a message, an image, a link, a conversation.
  `components/ui/ContextMenu.tsx`.

### Profiles

- **Other people's profiles** can be opened, with the server deciding
  per field what a viewer may see (G2 — the client never picks).
  `features/profile/PublicProfile.tsx`.
- **Message someone from their profile**, which starts the conversation and
  goes straight to it.

### Security

- **PIN unlock.** After auto-lock, a PIN gets you back in instead of the full
  password. `crates/client/src/pin.rs`.

  What it does and does not buy, plainly: the PIN unwraps a stored copy of the
  store key, and that copy is itself bound to the Windows account through
  DPAPI. So an attacker needs the PIN *and* the signed-in Windows session — a
  four-digit code alone is roughly ten thousand guesses and would be no
  protection on its own. Attempts are limited, and the password remains the
  way back in. This deliberately trades a little of the stolen-laptop
  protection for a lock people will actually leave switched on;
  `docs/THREAT-MODEL.md` §3 is where that trade belongs.

### Appearance

- **Accent colour** as a hue rather than a free colour: saturation and
  lightness stay fixed so every accent still clears the 4.5:1 the design
  system asks for (§7.4). `preferences.accentHue`.
- **Background depth** — a slider from the designed palette to pure black in
  dark mode, pure white in light. Every surface moves proportionally, so
  panels stay distinguishable at the far end. `preferences.contrast`.
- **Blur strength** as a slider rather than a switch, where zero drops
  `backdrop-filter` entirely instead of setting it to zero — the property
  costs the GPU whatever its radius, which is why the switch existed
  (PLAN.md risk 9). `preferences.glassStrength`.
- **A translucent window.** The same switch asks Windows for an acrylic
  backdrop, so the desktop behind the app is genuinely visible through it,
  blurred by the window manager. CSS cannot do this on its own:
  `backdrop-filter` blurs what is behind an element *in the document*, and the
  wallpaper is not in the document. The window is created transparent and Rust
  asks DWM (`windows::set_backdrop`); the WebView only makes its own field
  translucent once that call reports the effect actually applied, because a
  transparent window without it is a hole through to the desktop rather than
  glass over it.
- **A draggable divider on Home**, between the feed and the most recent
  conversation. Replaces three fixed hairlines with one line that means
  something. `preferences.homeChatWidth`.

---

## Since v0.1.3

Written the same way as the rest: walked against the code, not the commit log.

### Messaging

- **Search across history**, not just the newest message per row. FTS5 inside
  the encrypted store, so the term never leaves the machine
  (`crates/store`, schema 9).
- **Key-change detection.** `conversation_peers` records the key a peer was
  last seen with, so a change is noticed and reported rather than assumed
  benign. `mark_verified` and `acknowledge_key_change` are deliberately
  separate: having seen a change is not the same as having re-compared digits.
- **Conversations that could never be entered are now escaped.** A `start_with`
  whose commit reached the server but whose Welcome did not used to leave a
  chat that was listed, opened, and answered every message with "You are not in
  that conversation." for ever. `open_with` syncs, and if no Welcome exists it
  leaves the conversation so a usable one can be made.

### Server

- **Rate limits across every mutating route** — posts, comments, reactions,
  media, profile and membership, beside the auth and send limits that already
  existed. `NEXO_RATE_LIMITS=off` exists for the integration suite and is
  loud about itself at startup.
- **Device retirement** on login, and **reporting** (`/v1/reports`).

### Meet&Greet (M10)

A fifth destination: a world map on which a person may place one pin saying
roughly where they are, wearing a character they built.

- **Nexo never reads device location.** No `navigator.geolocation` call exists
  in the feature, and `meet_profiles` has no column that could hold a
  measurement. A pin is a claim somebody typed.
- **The pin is coarsened before it is stored** — snapped to a 0.25° grid and
  offset by a fixed amount derived from the account, so repeated saves disclose
  no more than the first. The submitted figure is never written anywhere.
- **The map is bundled**, not tiled: MapLibre over 105 KB of public-domain
  Natural Earth data. No tile server, no API key, no attribution obligation,
  and nothing fetched at runtime.
- **A NexoChar is stored as its config**, never as an image — a couple of
  hundred bytes of JSON rendered by whichever client draws it. Nothing in
  object storage, and no picture to moderate.
- **An intro buys exactly one message** until it is answered, enforced by the
  delivery service rather than the app.
- **Blocking removes both pins**, in both directions, reusing `blocks`.
- The whole feature is behind a lazy import: the map chunk is 288 KB gzipped
  and the startup bundle is unchanged.

**Known gap:** on this machine the map shows horizontal banding while being
dragged, under a transparent window with the DWM acrylic backdrop. WebGL,
the worker and zoom all work; the banding is cosmetic and unresolved. Three
candidate fixes were identified and none has been confirmed.

### Since v0.1.18

- **A payload this build cannot read is shown as one**, rather than rendered
  as raw text. `Payload::decode` used to fall back to text for *any* parse
  failure, including a `kind` from a newer client — so the first new variant
  ever sent would have put JSON in a chat bubble on every installation that
  had not updated. It now separates "tagged JSON I do not know" from "not JSON
  at all"; the latter is still read as text, because the project's first
  messages were bare UTF-8 and refusing them would be self-inflicted data
  loss. The undecoded bytes are kept in the store — MLS will not decrypt that
  envelope twice, so a later build reads what arrived today or never does.
- **The conversation beside the feed can be chosen.** It still follows the most
  recent by default; the header is now also a picker, and picking one suspends
  the following until it is taken back. The choice does not survive a restart,
  which is deliberate: a conversation sitting there for a week while everything
  happens elsewhere is the silent staleness the default exists to avoid.
- **The profile picture is changed from the picture.** Hovering or focusing the
  avatar offers it, the way the banner already did. The button that used to do
  this sat in the row underneath, away from the thing it acted on and giving no
  hint the picture was changeable at all.
- **"Until I turn it off" is a mute entry you can see.** It always was the
  behaviour of a plain click on Mute; beside four durations, a bare "Mute" read
  as "for how long?" instead. Naming it costs one line.
