# BUILD PROMPT — Nexo, Secure Messenger, Windows Desktop (MVP v0.1)

> Paste this whole document into your coding agent as the project brief.

**Product:** Nexo
**Domain:** `delidev.net`
**Service hosts:** API + WebSocket at `api.delidev.net`, marketing/download page at `www.delidev.net`, updater manifests at `updates.delidev.net`. Use these exact hosts everywhere — no `localhost` hardcoded outside dev config, no placeholder domains.

---

## 0. Role and mission

You are the lead engineer building **Nexo**, an end-to-end encrypted messenger. This milestone ships a **Windows 10/11 desktop client + server**. Android comes later and must not require a rewrite.

The product is a small, fast, dark, glassy chat app with three top-level destinations: **Home** (a feed of posts users create), **Messages** (E2EE 1:1 and group chat), **Profile** (avatar, banner, display name, handle, ID number, bio, location, links).

Build for correctness and security first, features second. This is v0.1 of something that grows — every decision below is chosen so v0.5 doesn't require a rewrite.

---

## 1. Non-negotiable rules

Read these before writing a single line. If a task in this document conflicts with a rule here, the rule wins and you flag the conflict.

1. **Never invent cryptography.** No custom key exchange, no custom ratchet, no "XOR with a key", no rolling your own AEAD framing. Only the audited libraries named in §3.
2. **No key material in the WebView.** All crypto, all private keys, all plaintext-at-rest lives in the Rust process. The frontend receives already-decrypted strings over IPC and holds them in memory only.
3. **No remote code in the client.** Strict CSP, no CDN scripts, no `eval`, no runtime-fetched JS. Everything is bundled.
4. **The server must never be able to read message contents.** If you find yourself writing a server endpoint that touches message plaintext, stop and flag it.
5. **Be honest in the UI about what is and isn't encrypted.** See §4.4. Do not label anything "military grade" or "unhackable" anywhere in the product or the README.
6. **Zeroize secrets.** Use the `zeroize` crate for key material, MLS state buffers, and password bytes.
7. **Fail closed.** Decryption failure shows a "can't decrypt this message" state — never a fallback to plaintext, never a silent skip.
8. **Every dependency is pinned** and checked by `cargo-deny` + `cargo-audit` + `npm audit` in CI. No unmaintained crypto crates.

---

## 2. What you should ask me before assuming

Do not guess on these. Ask, then proceed:

- **"Number" in the profile** — is that a phone number (identity/discovery, needs SMS verification, leaks the social graph) or an in-app numeric ID like Telegram's? Default assumption if I don't answer: **in-app ID**, no phone number collected.
- **Multi-device from day one?** Multi-device E2EE roughly doubles the crypto work. Default assumption: **one device per account in v0.1**, with the key hierarchy designed so multi-device drops in later.
- **Self-hosted server on a Hetzner VPS, or managed?** Default assumption: **self-hosted Rust server on Hetzner CAX21 (ARM), Falkenstein.**
- **Is the Home feed public, followers-only, or friends-only?** Default assumption: **public posts, visible to any logged-in user.**

---

## 3. Locked technology decisions

| Layer | Choice | Why this and not the obvious alternative |
|---|---|---|
| Desktop shell | **Tauri 2.x** | Rust core + OS WebView2. Deny-by-default permission model, tiny installer, and — decisively — Tauri 2 also targets Android, so the Windows work carries over. Electron ships Chromium (~85–100 MB installers vs single-digit MB) and its secure setup is opt-in hardening rather than the default. |
| UI | **React 19 + TypeScript (strict) + Vite + Tailwind CSS v4** | Standard, fast, and the design in §7 is a normal component tree, not a canvas app. |
| State | **Zustand** + **TanStack Query** | Zustand for chat/session state, TanStack Query for feed and profile HTTP. Redux is overkill here. |
| Client crypto | **OpenMLS** (Rust, RFC 9420) | Standardised group E2EE with forward secrecy and post-compromise security; group ops scale logarithmically instead of the O(n²) pairwise fan-out Signal-style protocols need for groups. **Licence note: `libsignal` is AGPL-3.0-only** — linking it into a distributed client has copyleft consequences for the whole app. OpenMLS is Apache-2.0/MIT. This is the main reason for the choice; make sure I understand it. |
| Ciphersuite | `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519` | The mandatory-to-implement suite; maximum interop. Leave a config seam for the X-Wing hybrid post-quantum suite later — do not enable it in v0.1. |
| Local DB | **SQLite via SQLCipher** (`rusqlite`, `bundled-sqlcipher-vendored-openssl`) | Whole-file encryption of messages, MLS state, and cached profiles. |
| Key storage | **Windows DPAPI** (`CryptProtectData`, user scope, `CRYPTPROTECT_UI_FORBIDDEN`) wrapping the SQLCipher key, stored in `%APPDATA%\Nexo\keyring.bin`. Optional Windows Hello gate via `windows` crate. | Uses the OS user secret; no master password to lose in v0.1. |
| Server | **Rust + axum + tokio**, `rustls` (TLS 1.3 only) | Same language as the crypto core, so shared types live in one crate. |
| Server DB | **PostgreSQL 16** + `sqlx` (compile-time checked queries) | |
| Presence / fanout | **Redis** pub/sub | |
| Realtime transport | **WebSocket (WSS)**, binary frames, protobuf or CBOR payloads | |
| Object storage | **Hetzner Object Storage** (S3-compatible, FSN1) via `aws-sdk-s3` **from the Rust side** | See §5.3 for the Hetzner-specific gotchas. |
| Packaging | Tauri MSI/NSIS bundle, **Authenticode-signed**, Tauri updater with signed manifests | Unsigned installers trigger SmartScreen warnings that kill trust on first run. Budget for a code-signing certificate. |
| CI | GitHub Actions: `cargo clippy -D warnings`, `cargo test`, `cargo deny`, `cargo audit`, `tsc --noEmit`, `vitest`, Playwright smoke, Windows build artifact | |

**Explicitly rejected:** Electron (size + opt-in security), Supabase as the primary backend (its row-level-security model doesn't help when the payload is opaque ciphertext, and you'd still need a custom delivery service), storing anything derived from a password on the server in reversible form.

---

## 4. Security architecture

### 4.1 Identity

- Account = unique `handle` (3–20 chars, `[a-z0-9_]`) + display name + numeric `user_id`.
- Password: client derives with **Argon2id** (m=64 MiB, t=3, p=1, unique 16-byte salt from server), sends the derived verifier over TLS; server stores a second Argon2id hash of the verifier. Server never sees the password.
- On registration the client generates an **Ed25519 identity keypair**. Private half never leaves the machine. Public half is registered and becomes the account's cryptographic identity.
- **Safety numbers:** SHA-256 over the sorted identity public keys of both parties, rendered as 12 groups of 5 digits, plus a QR code. Surface this in a "Verify" screen in the conversation header. Changed key → conversation shows a persistent, non-dismissable warning banner until re-verified.

### 4.2 Message encryption

- One **MLS group per conversation**. A 1:1 chat is a two-member group — no special-casing.
- The server is an MLS **Delivery Service + Authentication Service**: it stores KeyPackages, fans out ciphertext, orders commits. It holds no group secrets and cannot derive any.
- Each client publishes **50 KeyPackages** on registration and refills whenever the server reports fewer than 15 remaining. KeyPackages are single-use — each invite consumes one.
- **Rekey policy:** issue an Update commit every 100 messages sent or every 7 days, whichever comes first, and always on member add/remove.
- Message envelope on the wire: `{conversation_id, sender_device_id, epoch, ciphertext, server_timestamp}`. Nothing else. No plaintext subject, no plaintext preview, no plaintext attachment filenames.

### 4.3 Storage

**Client:** SQLite/SQLCipher at `%APPDATA%\Nexo\store.db`. Holds messages, MLS group state, contacts, cached profiles and media. Key is 32 random bytes from the OS CSPRNG, DPAPI-wrapped. On logout: wipe the DB file and the keyring blob.

**Server:** Postgres holds ciphertext blobs, routing metadata, KeyPackages, and the public profile/feed data from §4.4. Full-disk encryption (LUKS) on the VPS. Undelivered ciphertext is purged after **30 days**; delivered ciphertext is deleted on ack.

### 4.4 What is NOT end-to-end encrypted — and how the UI must say so

E2EE and a public feed are structurally incompatible: content meant to be readable by strangers cannot be encrypted to a closed group. Be straight about it.

| Data | Protection |
|---|---|
| DM and group message bodies, attachments, reactions | **E2EE** (MLS) |
| Home feed posts + their media | TLS in transit, encrypted at rest on the server. **Server-readable.** |
| Profile picture, banner, display name, handle, bio, location, links | TLS + at rest. **Server-readable and public.** |
| Conversation metadata (who talks to whom, when, message sizes) | TLS + at rest. **Server-readable.** This is the honest limit of the design — don't claim metadata privacy. |

Ship a **Privacy** panel in Settings that states exactly the table above in plain language, and put a small lock icon in E2EE conversations only.

### 4.5 Hardening checklist

- Tauri capabilities: start from empty. Allow only the specific commands the UI needs. No `shell`, no broad `fs`, no `http` plugin from the frontend.
- CSP: `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' asset: data: blob:; connect-src 'self' ipc: https://ipc.localhost; object-src 'none'; frame-ancestors 'none'`
- **Certificate pinning** for `api.delidev.net` in the Rust HTTP/WS client: pin the SPKI of the leaf plus one backup key held offline. Ship a documented rotation path — a pinned client that outlives its pin is a bricked client.
- Rate limit auth endpoints (10/min/IP), KeyPackage fetches (60/min/account), and message send (30/s/account).
- Link previews are **generated client-side and off by default** — a server-side preview fetcher is a request-forgery and metadata leak.
- No crash reporting that can capture plaintext. If you add crash reporting later, it must be opt-in and scrub message content.
- Panic hook that never writes message bodies to logs. Log levels: nothing above `debug` may contain user content, and `debug` is compiled out of release builds.

---

## 5. Backend design

### 5.1 Postgres schema (starting point)

```sql
users(id BIGSERIAL PK, handle CITEXT UNIQUE, display_name TEXT, bio TEXT,
      location TEXT, avatar_key TEXT, banner_key TEXT,
      pw_salt BYTEA, pw_hash TEXT, created_at TIMESTAMPTZ)

devices(id UUID PK, user_id BIGINT FK, identity_pubkey BYTEA UNIQUE,
        name TEXT, last_seen TIMESTAMPTZ, created_at TIMESTAMPTZ)

key_packages(id UUID PK, device_id UUID FK, data BYTEA,
             consumed_at TIMESTAMPTZ NULL, created_at TIMESTAMPTZ)

conversations(id UUID PK, kind TEXT CHECK (kind IN ('dm','group')),
              created_at TIMESTAMPTZ)

conversation_members(conversation_id UUID FK, user_id BIGINT FK,
                     role TEXT, joined_at TIMESTAMPTZ,
                     PRIMARY KEY (conversation_id, user_id))

envelopes(id BIGSERIAL PK, conversation_id UUID FK, sender_device_id UUID FK,
          epoch BIGINT, ciphertext BYTEA, created_at TIMESTAMPTZ,
          delivered_at TIMESTAMPTZ NULL)

posts(id UUID PK, author_id BIGINT FK, body TEXT, media_keys TEXT[],
      created_at TIMESTAMPTZ, edited_at TIMESTAMPTZ NULL)

post_reactions(post_id UUID FK, user_id BIGINT FK, emoji TEXT,
               PRIMARY KEY (post_id, user_id, emoji))
```

Index `envelopes(conversation_id, id)` and `envelopes(delivered_at) WHERE delivered_at IS NULL`.

### 5.2 API surface

Base URL `https://api.delidev.net`, WebSocket `wss://api.delidev.net/v1/stream`. Both behind Caddy or nginx terminating TLS 1.3 on the Hetzner VPS, with HSTS (`max-age=63072000; includeSubDomains; preload`).

```
POST   /v1/auth/register          handle, display_name, pw_verifier, identity_pubkey, key_packages[]
POST   /v1/auth/salt              handle -> salt
POST   /v1/auth/login             handle, pw_verifier -> access_token (15 min), refresh_token (30 d, rotating)
POST   /v1/auth/logout

GET    /v1/users/:handle          public profile
PATCH  /v1/me                     display_name, bio, location
POST   /v1/me/avatar              -> presigned PUT
POST   /v1/me/banner              -> presigned PUT

GET    /v1/keypackages/:handle    -> one KeyPackage, marked consumed
POST   /v1/keypackages            top up own supply
GET    /v1/keypackages/count      -> remaining

POST   /v1/conversations          member handles -> conversation
GET    /v1/conversations          list with last-envelope pointer
POST   /v1/conversations/:id/send ciphertext
GET    /v1/conversations/:id/sync since_id -> envelopes[]

GET    /v1/feed                   cursor-paginated posts
POST   /v1/posts                  body, media_keys
DELETE /v1/posts/:id
POST   /v1/posts/:id/react        emoji

POST   /v1/media/presign          content_type, size -> {url, key, expires_at}

WSS    /v1/stream                 server->client: envelope, typing, presence, receipt
                                  client->server: ack, typing, presence
```

Access tokens are short-lived JWTs (EdDSA, not HS256). Refresh tokens rotate on every use and are revoked on reuse detection.

### 5.3 Hetzner Object Storage — read this before writing the S3 code

- Endpoint is `https://fsn1.your-objectstorage.com` (or `hel1`/`nbg1`). **The bucket name is not part of the hostname** — all buckets in a region share a domain, so you must use **path-style addressing**, not virtual-hosted style.
- Buckets are **private**. Every read and write goes through a presigned URL with a short TTL (uploads 10 min, downloads 60 min).
- Some S3 SDKs default to a signature version Hetzner doesn't accept — verify SigV4 works and pin the config explicitly rather than relying on SDK defaults.
- **CORS on presigned uploads is a known pain point.** Sidestep it entirely: **do all uploads and downloads from the Rust process with `aws-sdk-s3`, never from the WebView with `fetch`.** The frontend asks Rust to upload a file; Rust streams it. No browser, no CORS, and it keeps the encryption in Rust where it belongs.
- Objects are immutable (write-once). Profile picture changes write a new key and delete the old one.
- Limits to design within: 100 buckets, 100 TB and 50 M objects per bucket, objects under 64 kB billed as 64 kB.
- Two buckets, both private: **`nexo-media`** with keys `media/{user_id}/{uuid}` for feed and profile images, and **`nexo-enc`** with keys `enc/{conversation_id}/{uuid}` for encrypted attachments. Separate buckets so the credentials for public media can never touch encrypted blobs.

**Attachment flow (E2EE):** Rust generates a fresh AES-256-GCM key + nonce → encrypts the file → uploads ciphertext via presigned PUT → puts `{s3_key, key, nonce, sha256, mime, size}` **inside the MLS-encrypted message**. The server sees an opaque blob and never learns the key, the filename, or the type.

---

## 6. Feature specification

### 6.1 Messages (the core — build this first)

- Conversation list: avatar, name, last message preview (decrypted locally), timestamp, unread pill, delivery ticks.
- Search across conversation names and locally decrypted message bodies (SQLite FTS5 inside the encrypted DB).
- Chat pane: grouped bubbles by sender and time, own messages right-aligned in accent violet, incoming left-aligned on a dark surface, day dividers, timestamps under bubbles, double-tick read receipts.
- Composer: multiline autogrow, Enter sends / Shift+Enter newline, emoji picker, attachment button, drag-and-drop file, paste image from clipboard.
- Typing indicators, online/last-seen presence (both opt-out-able in Settings).
- Right context panel, collapsible: **Shared Media** grid, **Shared Files** list with size and date, **Shared Links** list. All derived from the local decrypted store — no server-side index.
- Group chats: create, add/remove member (triggers MLS commit), rename, leave.
- Offline: outbound queue with retry and exponential backoff; messages show a pending state.

### 6.2 Home

- Reverse-chronological feed of posts, cursor-paginated, infinite scroll.
- Composer: text up to 2000 chars, up to 4 images, optional link.
- Post card: author avatar + display name + handle + relative timestamp, body, media grid, reaction row, comment count.
- Own posts: delete. Edit is v0.2.
- Empty state that invites the first post rather than apologising for emptiness.
- **A visible, plain-language note that feed posts are public and not end-to-end encrypted.**

### 6.3 Profile

- Banner (3:1, max 4 MB) with avatar overlapping the lower-left, matching the layout language of the sidebar profile card.
- Editable: display name, bio (280 chars), location (free text — do **not** use geolocation APIs), links.
- Read-only: handle, numeric ID, join date.
- Tabs: Posts, Media.
- Own profile shows an extra **Security** tab: this device's safety number and QR, active session list, "Log out everywhere", and the §4.4 privacy table.

### 6.4 Settings

Appearance (dark only in v0.1, theme seam in place), Notifications, Privacy (read receipts, typing indicators, presence, link previews), Security (change password, lock timeout, optional Windows Hello), Storage (cache size, clear cache), About (version, licences, update check).

---

## 7. Design system

Reference: the two dark chat mockups this brief was built from. Match their *feel* — deep near-black glass panels floating over a saturated gradient, violet accents, generous rounding — not their pixels.

### 7.1 Tokens

```css
--void:        #08080F;  /* behind the window, where the gradient lives */
--surface-0:   #101018;  /* icon rail */
--surface-1:   #14141F;  /* conversation list, context panel */
--surface-2:   #191926;  /* chat pane */
--surface-3:   #22222F;  /* incoming bubble, inputs, hover */
--accent:      #7B5CFA;  /* outgoing bubble, active nav, focus ring */
--accent-soft: #A28BFF;  /* links, secondary accent text */
--success:     #3DD68C;  /* read receipts, online dot */
--warning:     #F5A524;  /* unverified safety number */
--danger:      #F0426B;  /* destructive actions, decryption failure */
--text-hi:     #EDEDF5;
--text-mid:    #A0A0B8;
--text-lo:     #6E6E85;
--hairline:    rgba(255,255,255,0.06);

--r-window: 12px;  --r-panel: 14px;  --r-bubble: 18px;  --r-control: 10px;
--space: 4px base, 8/12/16/24/32 scale
```

Panels use `background: color-mix(in srgb, var(--surface-1) 88%, transparent)` with `backdrop-filter: blur(24px)` and a 1px `--hairline` top border to get the glass edge from the references. Test this on WebView2 specifically and provide an opaque fallback — blur is expensive on integrated GPUs and some users disable transparency in Windows.

### 7.2 Type

- **Body/UI: Inter Variable**, self-hosted, `font-optical-sizing: auto`. Sizes: 13px meta, 14px body, 15px message text, 16px titles.
- **Display (wordmark, empty states, section headers): General Sans** — geometric with more character than Inter, used sparingly so it stays a signal.
- **Utility: JetBrains Mono** for safety numbers, device fingerprints, and file sizes. This is functional, not decorative: fingerprint digits must be unambiguously comparable at a glance, which is exactly what a mono face with slashed zeros is for.

### 7.3 Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│ ▓ custom titlebar — drag region, wordmark left, min/max/close right  │
├────┬───────────────┬─────────────────────────────┬───────────────────┤
│ 64 │ 300px         │ flex                        │ 280px collapsible │
│ px │               │                             │                   │
│ ▪  │ own profile   │ conversation header         │ Shared Media      │
│ ▪  │ search + new  │ ─────────────────────────── │ ─────────────     │
│ ▪  │ ───────────── │                             │ Shared Files      │
│ ▪  │ conversation  │ message scroll              │ ─────────────     │
│    │ list          │                             │ Shared Links      │
│ ▪  │               │ ─────────────────────────── │                   │
│ ⚙  │               │ composer                    │                   │
└────┴───────────────┴─────────────────────────────┴───────────────────┘
```

- Frameless Tauri window, `--r-window` corners, custom titlebar with Windows-convention controls on the **right** (the references show macOS traffic lights — do not copy that on Windows).
- Icon rail holds Home / Messages / Profile / Settings. Active item: accent-tinted background pill + full-opacity icon; inactive: `--text-lo`.
- Below 1100px the context panel auto-collapses; below 860px the conversation list becomes an overlay drawer. The layout must survive 125% and 150% Windows display scaling.

### 7.4 Motion and the quality floor

One orchestrated moment, not scattered effects: **messages arrive with a 180 ms spring — 8px rise plus opacity — staggered 30 ms when several land at once.** Everything else is a 120 ms ease-out on hover, focus, and panel collapse. No page transitions, no parallax.

Non-negotiable floor: visible keyboard focus rings on every interactive element, full keyboard navigation of the conversation list and composer, `prefers-reduced-motion` respected, 4.5:1 contrast on body text, screen-reader labels on icon-only buttons.

### 7.5 Copy

Sentence case. Active voice. A button that says "Send invite" produces a toast that says "Invite sent." Errors state what happened and what to do: "Can't reach the server. Your message will send when you're back online." Never "Oops!" and never an apology.

---

## 8. Windows-specific requirements

- Windows 10 1809+ and Windows 11. WebView2 Evergreen runtime, with the bootstrapper in the installer for machines that lack it.
- Native toast notifications via the Tauri notification plugin. Notification body respects a Privacy setting: show sender + message / sender only / neither.
- System tray icon: unread badge, "Open", "Quit". Close-to-tray configurable.
- Single-instance guard — second launch focuses the running window.
- Start-with-Windows toggle via registry `Run` key (user scope, never machine scope).
- Auto-lock after N minutes idle → locks the encrypted store, requires Windows Hello or password to unlock.
- Installer: NSIS, per-user by default (no admin prompt), Authenticode signed.
- Updater: Tauri's updater pointed at `https://updates.delidev.net/nexo/{{target}}/{{current_version}}`, manifests signed with the Tauri minisign key (private half never in the repo, never in CI logs — use a GitHub Actions secret). Tauri downloads full binaries rather than diffs, which is fine at this bundle size but measure it.

---

## 9. Build order

Ship each milestone as a working, reviewable increment. Do not start the next until the previous one's tests pass.

| # | Milestone | Done when |
|---|---|---|
| M0 | Monorepo scaffold: `apps/desktop` (Tauri+React), `apps/server` (axum), `crates/protocol` (shared types), `crates/crypto` (OpenMLS wrapper). CI green. | `cargo build --release` and `pnpm build` both pass on a clean Windows runner |
| M1 | Design system + all three pages shelled out with mock data. No network. | The app looks like §7 at 100/125/150% scaling |
| M2 | Auth, device identity keypair, encrypted local store, DPAPI keyring | Register → restart app → still logged in; DB file is unreadable with plain `sqlite3` |
| M3 | **`crates/crypto`: OpenMLS 1:1 with full unit tests, in isolation** | Two in-process clients exchange messages, rekey, and recover from an out-of-order commit |
| M4 | WebSocket transport + wire M3 into the UI | Two machines on the same server exchange real E2EE messages; safety numbers match |
| M5 | Group conversations, add/remove members | Adding a member gives them forward messages only, never history |
| M6 | Encrypted attachments via Hetzner (Rust-side S3) | Send a 20 MB file; the object in the bucket is verifiably ciphertext |
| M7 | Home feed + Profile + media upload | Post, react, edit profile, upload avatar and banner |
| M8 | Settings, tray, notifications, auto-lock, offline queue | Kill the network mid-send; message delivers on reconnect |
| M9 | Signed MSI + updater + threat-model doc | Clean Windows 11 VM: install, register, message, update — no SmartScreen warning |

---

## 10. Definition of done for v0.1

- [ ] Clean Windows 11 VM installs the signed MSI without a SmartScreen block
- [ ] Two accounts on two machines exchange E2EE messages, files, and images
- [ ] Safety numbers are displayable, comparable, and warn on key change
- [ ] `store.db` is unreadable without the DPAPI-wrapped key; deleting the keyring blob makes it permanently unreadable
- [ ] Packet capture during a send shows TLS only, and the server DB contains no plaintext
- [ ] Feed and profile work; the privacy table in Settings accurately describes what's encrypted
- [ ] Cold start under 1.5 s; idle RAM under 200 MB; installer under 20 MB
- [ ] `cargo audit`, `cargo deny`, `npm audit` clean
- [ ] `docs/THREAT-MODEL.md` names the adversaries in scope (network attacker, curious server operator, someone with the offline disk) and out of scope (malware on the user's machine, a compromised server that swaps your public keys without safety-number verification, metadata analysis)

---

## 11. Out of scope for v0.1

Voice/video calls, disappearing messages, multi-device sync, message editing, stories, reactions on DMs, federation, post-quantum ciphersuites, macOS/Linux builds, translation. Leave seams, don't build them.

---

## 12. Android (context only — do not build yet)

Tauri 2 targets Android from the same repo. The plan: `crates/crypto` and `crates/protocol` compile unchanged; the React UI gets responsive breakpoints and a bottom tab bar replacing the icon rail; DPAPI is swapped for the Android Keystore behind a `SecureStore` trait that you should **define now in M2** with a Windows implementation. Keep every platform call behind that trait.

---

## 13. One thing outside the code

Operating a messenger with real user accounts means real obligations — a privacy policy, a lawful basis for the data you hold, a breach process, and a decision about what you'd do with a data request. Under Swiss revDSG and, if you have EU users, GDPR, the metadata in §4.4 is personal data even though the messages aren't readable. Worth an hour with someone who does this professionally before launch, not after. (I'm not a lawyer and this isn't legal advice.)
