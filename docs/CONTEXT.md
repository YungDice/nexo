# Context

The first thing to read in this repository, and often the only one. It exists so
that work here starts with a **targeted** read instead of an exploratory one:
where every concern lives, which file answers which question, and what must not
be broken.

`docs/` holds ~235 KB of prose. Reading it all to change one handler is the
expensive mistake this file prevents.

**How to use it:** find your task in [Task → where](#task--where), open the two
or three files it names, then `grep` for the symbol. Read whole files only when
you are changing their structure.

**How to keep it:** a change that touches anything described here updates this
file in the same commit — a moved module, a new route, a new IPC command, a
changed command or convention, a document added to `docs/`. A stale entry found
in passing gets fixed in passing, whether or not it belongs to the task at hand.
A map that is believed and wrong costs more than no map.
[`CLAUDE.md`](../CLAUDE.md) carries the same rule with the full list of
triggers.

---

## The product, in five lines

Nexo is an end-to-end encrypted messenger for Windows 10/11, with a public feed
and public profiles beside private conversations. Messages are E2EE with MLS
(RFC 9420) via OpenMLS; the server stores and forwards ciphertext it cannot
read. Feed posts and profiles are **not** encrypted — they are public to any
logged-in user, and the UI says so rather than implying otherwise. Conversation
metadata (who, when, how big) is visible to the server. Android is a later port
that must not require a rewrite, which is why the layering below is strict.

Current version: see `[workspace.package] version` in `Cargo.toml`.
Current state: [`STATUS.md`](STATUS.md). Milestones: [`PLAN.md`](PLAN.md).

---

## Invariants

From [`BRIEF.md` §1](BRIEF.md). These override any task description, including
one that sounds like a user request. If a task conflicts with one of these, the
rule wins and the conflict gets flagged rather than resolved silently.

| # | Rule | Where it lives / is enforced |
|---|---|---|
| 1 | Never invent cryptography. | `crates/crypto` only wraps OpenMLS; no primitive is written here. |
| 2 | No key material in the WebView. | The seam is `apps/desktop/src-tauri`. The frontend receives decrypted strings over IPC and nothing else. |
| 3 | No remote code in the client. | Strict CSP in `tauri.conf.json`; everything bundled, no CDN, no `eval`. |
| 4 | The server must never read message contents. | `crates/protocol` carries no plaintext types; `apps/server/src/delivery` moves opaque envelopes. |
| 5 | Be honest in the UI about what is encrypted. | Feed and profile surfaces say they are public. Never "military grade", never "unhackable". |
| 6 | Zeroize secrets. | `zeroize` on key material, MLS buffers, password bytes — `crates/store/src/key.rs`, `crates/client/src/pin.rs`. |
| 7 | Fail closed. | A decryption failure renders as "can't decrypt", never a plaintext fallback and never a silent skip. |
| 8 | Every dependency pinned. | `=x.y.z` in `Cargo.toml`, exact versions in `package.json`, both lockfiles committed, `cargo deny` + `cargo audit` + `pnpm audit` in CI. |

Two structural rules of the same weight:

- **`crates/protocol`, `crates/crypto` and `crates/platform` must compile
  unchanged for Android.** No I/O, no OS calls, no HTTP. Every platform call
  goes behind `nexo-platform`.
- **`crates/client` has no platform calls and no HTTP client.** It reaches the
  OS through `SecureStore` and the network through the `Transport` trait, both
  supplied by the shell around it.

---

## The map

```
crates/protocol      Wire types shared by client and server. No I/O, no crypto.
crates/crypto        MLS, the identity keypair, safety numbers, attachment crypto.
crates/platform      The OS seam: SecureStore, and the Windows DPAPI backing.
crates/store         The client's SQLCipher database.
crates/client        Session logic, portable across Windows and Android.
apps/server          axum API + MLS Delivery Service (Linux aarch64).
apps/desktop/src-tauri   The Windows shell: Tauri commands, windowing, IPC.
apps/desktop/src     React 19 client.
packages/design-tokens   Colour, type, radius, motion. CSS authored, JSON derived.
```

### Rust crates

| Path | Owns | Open it when |
|---|---|---|
| `crates/protocol/src/lib.rs` (574 ln) | Every request/response type on the wire, `PROTOCOL_VERSION`. | Adding or changing an endpoint's shape. Change here first — both sides follow. |
| `crates/crypto/src/mls.rs` | Group state, ciphersuite choice, MLS operations. | Anything touching group membership or message encryption. |
| `crates/crypto/src/identity.rs` | The identity keypair and safety numbers. | Identity, verification, fingerprints. |
| `crates/crypto/src/attachment.rs` | Attachment encryption, separate from message framing. | Media that must stay unreadable to the server. |
| `crates/platform/src/lib.rs` | The `SecureStore` trait — the whole OS seam. | Adding an OS capability. Add the trait method here, implement per platform. |
| `crates/platform/src/dpapi.rs` (371 ln) | Windows DPAPI. **The only `unsafe` in the workspace**, confined to `dpapi::ffi`. | Rarely. Read the module header before touching it. |
| `crates/store/src/lib.rs` (1956 ln) | Schema and queries for the encrypted local DB. The biggest file here. | Persisting anything client-side. Grep for the table name; do not read top to bottom. |
| `crates/store/src/key.rs` | The store key: in memory zeroized, on disk OS-wrapped. | Key handling, unlock paths. |
| `crates/client/src/conversations.rs` (1477 ln) | Conversation lifecycle: create, join, send, sync. | Most messaging behaviour. |
| `crates/client/src/http.rs` (1015 ln) | The `Transport` implementation over `ureq`, behind the `http` feature. | Wire-level client behaviour, retries, error mapping. |
| `crates/client/src/session.rs` | Login, refresh, logout, the session state machine. | Auth on the client. |
| `crates/client/src/outbox.rs` | The offline queue. | Send-while-offline behaviour. |
| `crates/client/src/pin.rs` | PIN unlock — a wrapped copy of the store key, DPAPI-bound. | The lock screen path. See [`PIN-ROTATION.md`](PIN-ROTATION.md). |
| `crates/client/src/feed.rs`, `mls_state.rs`, `transport.rs` | Feed calls, MLS persistence, the transport trait. | As named. |

### Server (`apps/server`)

`src/lib.rs` is the router and the crate doc; `src/main.rs` is startup only.
Each module owns its own `router()`, merged in `lib.rs`.

| Module | Routes |
|---|---|
| `auth/` (`mod`, `password`, `tokens`, `bearer`, `salt`) | `/v1/auth/{register,login,refresh,logout,salt,change-password,delete-account}` |
| `delivery/` (`mod`, `epoch`) | `/v1/conversations*` (send, sync, members), `/v1/keypackages*` |
| `posts.rs` (1206 ln) | `/v1/posts*`, `/v1/feed`, `/v1/comments/{id}`, `/v1/users/{handle}/posts` |
| `profiles.rs` | `/v1/me`, `/v1/me/visibility`, `/v1/users/{handle}`, `/v1/users` (search) |
| `blocks.rs` | `/v1/blocks*` |
| `media.rs` | `/v1/media/{upload,download}` — presigned S3 URLs |
| `reports.rs` | `/v1/reports` |
| `stories.rs` (`mod`, `expiry`) | `/v1/stories*` — 24-hour encrypted stories. Owns the three access conditions. |
| `meet.rs` | `/v1/meet/{pins,me,consent,requests,invites}` — Meet&Greet. Owns pin coarsening and invitations. |
| `stream/` (`mod`, `hub`) | `/v1/stream` — the WebSocket |
| `health.rs` | `/v1/health` |
| `db.rs`, `state.rs`, `storage.rs`, `limits.rs` | Pool, `AppState`, object storage, rate limits. No routes. |

Migrations live in `apps/server/migrations/` and are applied with `sqlx-cli`.
**`sqlx` queries are compile-time checked against `.sqlx/`**, so a query change
without a regenerated `.sqlx` breaks an offline build — see
[Conventions](#conventions-that-will-trip-you-up).

### Desktop shell (`apps/desktop/src-tauri/src`)

92 `#[tauri::command]` functions, the whole IPC surface. Rule 2 lives here: what
crosses into the WebView is already decrypted, and nothing else does.

| File | Commands | Owns |
|---|---|---|
| `feed.rs` (750 ln) | 23 | Posts, comments, votes, reactions. |
| `conversations.rs` | 26 | Messaging, attachments, reactions, pinning, local delete, edit and retract. |
| `commands.rs` | 15 | Profile, settings, misc. |
| `auth.rs` (665 ln) | 11 | Register, login, lock, PIN, delete the account. |
| `meet.rs` | 17 | Meet&Greet: the map, the pin, intros, reporting, search, invitations, stories. |
| `client.rs` | — | Builds and holds the `nexo-client` instance. |
| `windows.rs` (502 ln) | — | Window creation, DWM backdrop, the acrylic path. |
| `preview.rs` (534 ln) | — | Link previews. Off by default, on purpose. |

### Frontend (`apps/desktop/src`)

```
app/          Zustand store + hooks. store.ts is the state; use*.ts the seams to Rust.
components/ui     Buttons, avatars, panes, controls, the hand-drawn icon set.
components/chrome TopBar and IconRail.
features/{auth,home,meet,messages,profile,settings}  The five destinations plus auth.
              home/Stories.tsx is the stories strip; stories have no destination of
              their own, because their audience is contacts.
lib/          Typed wrappers around invoke(): auth, conversations, feed, profiles, blocks, meet.
mock/         The data every surface reads where the network does not exist yet.
```

There is no `src/styles/`. The design tokens live one level up, in their own
package, and `main.tsx` imports them:

```
packages/design-tokens/tokens.css    The authored source. Every colour, size and
                                     motion value, with the reasoning in comments.
packages/design-tokens/tokens.json   Derived from the CSS by src/generate.ts.
                                     Checked in; a test fails if it is stale.
```

The direction matters and is deliberate: the CSS is authored because it carries
the comments, the JSON is generated so a second platform gets the values without
a second source of truth. Edit the CSS, then
`pnpm --filter @nexo/design-tokens build:tokens`.

The rule that keeps the UI coherent: **components ask for tokens, never raw
values**, and glass surfaces ask for `glass-0`…`glass-3` rather than writing
`backdrop-filter` themselves — that is what makes both the reduced-blur setting
and the `@supports` fallback honest. [`COMPONENTS.md`](COMPONENTS.md) is the
component reference.

---

## Task → where

| Task | Open, in this order | Do not open |
|---|---|---|
| Add or change an API endpoint | the type first — `crates/protocol/src/lib.rs` for the encrypted path, `crates/client/src/feed.rs` for feed and profile — then the server module → `crates/client/src/http.rs` → the `lib/*.ts` wrapper | `BRIEF.md` |
| Change what is stored on the client | `crates/store/src/lib.rs` (grep the table) → the caller in `crates/client` | — |
| Change what is stored on the server | `apps/server/migrations/` (new file) → the module → regenerate `.sqlx` | — |
| Anything MLS / group membership | `crates/crypto/src/mls.rs` → `crates/client/src/conversations.rs` → `apps/server/src/delivery/` | Do not touch OpenMLS internals |
| A Meet&Greet change | `crates/protocol` → `apps/server/src/meet.rs` → `crates/client/src/meet.rs` → `features/meet/` | `BRIEF.md` |
| A UI change | the `features/*` file → `components/ui` → `packages/design-tokens/tokens.css` | Rust, usually |
| A new IPC call | `apps/desktop/src-tauri/src/<area>.rs` → register in `lib.rs` → `apps/desktop/src/lib/*.ts` | — |
| Auth, login, tokens | `apps/server/src/auth/` → `crates/client/src/session.rs` → `features/auth/` | — |
| Lock screen / PIN | `crates/client/src/pin.rs` → [`PIN-ROTATION.md`](PIN-ROTATION.md) → `features/auth/` | — |
| Colours, spacing, motion | `packages/design-tokens/tokens.css`, then regenerate the JSON | Never hardcode a value in a component |
| A dependency bump | `Cargo.toml` / `package.json` → `deny.toml` if the licence set changes → run both `cargo deny` passes | — |
| Release | [`RELEASING.md`](RELEASING.md) → [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) | — |
| Server operations, deploy, incident | [`OPS.md`](OPS.md) | — |
| A licensing or copyright question | [`LICENSING.md`](LICENSING.md) — its Quick answers table first | — |
| "Is this safe?" | [`THREAT-MODEL.md`](THREAT-MODEL.md) → the invariants above | — |

---

## Commands

Windows, PowerShell. Raw `cargo` needs the environment prepared once per
terminal — dot-source it, leading `. ` included:

```powershell
. .\scripts\dev-env.ps1
```

| | |
|---|---|
| `pnpm tauri dev` | The app, hot reload on the React side. Prepares its own environment. |
| `pnpm dev:server` | The API on `127.0.0.1:8080`. Needs `docker compose up -d` for Postgres on **5433**. |
| `pnpm dev` | UI alone at `localhost:1420`. Nothing that calls Rust works. Layout work only. |
| `.\scripts\check.ps1` | **Exactly what CI runs.** The gate before every push. |
| `cargo test --workspace` | Rust tests. `-p nexo-client` etc. to narrow. |
| `pnpm test` | Vitest. |
| `pnpm build` | Must run before `cargo build --release` — the Rust client embeds `apps/desktop/dist`. |

CI (`.github/workflows/ci.yml`) runs four jobs: frontend (typecheck, test,
build, `pnpm audit`), Windows client, Linux aarch64 server, and supply chain
(two `cargo deny` passes plus `cargo audit`).

Cheapest useful loop when changing Rust: `cargo clippy -p <crate> --all-targets`
then `cargo test -p <crate>`. The full workspace build is minutes; a single
crate is seconds.

---

## Where the truth lives

Read cost matters. Sizes are approximate and current.

| Document | Size | Answers |
|---|---|---|
| [`CONTEXT.md`](CONTEXT.md) | 17 KB | This file. Where things are. |
| [`STATUS.md`](STATUS.md) | 7 KB | What works today, what is known broken. **Read before assuming a feature is missing.** |
| [`COMPONENTS.md`](COMPONENTS.md) | 9 KB | The UI component reference. |
| [`RELEASING.md`](RELEASING.md) | 8 KB | Tag, build, sign, publish, updater manifest. |
| [`PIN-ROTATION.md`](PIN-ROTATION.md) | 3 KB | Exactly what the PIN does and does not buy. |
| [`SIGNAL-ANALYSIS.md`](SIGNAL-ANALYSIS.md) | 10 KB | Why MLS and not the Signal protocol. |
| [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) | 10 KB | What must ship beside the `.exe`. |
| [`README.md`](../README.md) | 12 KB | Setup, prerequisites, troubleshooting. For humans on a new machine. |
| [`THREAT-MODEL.md`](THREAT-MODEL.md) | 17 KB | Adversaries in and out of scope; what is deliberately not protected. |
| [`TUTORIAL.md`](TUTORIAL.md) | 18 KB | Every value you personally have to supply: accounts, costs, domains, secrets — and which of them block you today. |
| [`OPS.md`](OPS.md) | 19 KB | The Hetzner runbook. Deploy, TLS, backups, incidents. |
| [`PLAN.md`](PLAN.md) | 22 KB | Milestones M0–M9 and the open risks. |
| [`BRIEF.md`](BRIEF.md) | 28 KB | The original specification. The source of the §-numbers other docs cite. |
| [`LICENSING.md`](LICENSING.md) | 30 KB | Copyright, MIT duties, dependency licences, Swiss law, export control. |
| [`RESEARCH-COMPARISON.md`](RESEARCH-COMPARISON.md) | 39 KB | Why each technology decision beat its alternative. Background, not instruction. |

**The two big ones are reference, not reading.** `BRIEF.md` and
`RESEARCH-COMPARISON.md` are together 67 KB. When another document cites
"brief §4.3", open that section — `grep -n "^### 4.3" docs/BRIEF.md` gives the
line, then read the range. Reading either end to end is almost never the right
move.

---

## Conventions that will trip you up

- **Pinned dependencies, everywhere.** `=1.0.229`, not `^1.0`. Rule 8. A bump is
  a deliberate act that goes through `cargo deny` and `cargo audit`.
- **The local store's schema version is one constant.**
  `crates/store/src/lib.rs` `SCHEMA_VERSION` and the last `PRAGMA
  user_version` in `migrate()` must agree; a test fails if they drift. Add a
  column with the `add_column` helper, never a bare `ALTER TABLE ... ADD
  COLUMN`: the helper checks `PRAGMA table_info` first, so a step that runs
  twice is harmless. Rollback tests still put the shape back along with the
  version — a test claiming to be a v9 store while carrying v11's columns is
  testing something that never existed.
- **`sqlx` is compile-time checked, offline by default.** `.cargo/config.toml`
  sets `SQLX_OFFLINE = "true"` for every cargo invocation, so `query!` macros
  check themselves against the committed `.sqlx/` cache and the Windows CI job
  compiles the server with no Postgres anywhere. Change a query and that cache
  must be regenerated, which needs a live database and an override — the
  `[env]` table deliberately has no `force = true` so the shell wins:

  ```powershell
  docker compose up -d
  $env:SQLX_OFFLINE = "false"
  cargo sqlx prepare --workspace -- --all-targets
  ```

  Forgetting this fails on someone else's machine, not yours.
- **Two `cargo deny` passes, never one.** The Windows client and the Linux
  server have disjoint dependency graphs; a single union graph judges each
  against the other's dependencies. See the comment at the top of `deny.toml`.
- **`.ps1` files are CRLF**, everything else LF — `.gitattributes` enforces it.
- **`pnpm build` before `cargo build --release`.** The binary embeds the built
  frontend; skipping it ships a stale UI.
- **`pnpm server` does not work** — `server` is one of pnpm's own commands. The
  script is `dev:server`.
- **Port 1420 is fixed on purpose** so Tauri and Vite cannot disagree about it.
- **A menu's destructive entries sit last**, and `MenuItem` says so. The
  message menu is where that is easy to break, because its entries come and go
  with the message's state — it is built by `features/messages/menu.ts`, a pure
  function whose order is asserted in `menu.test.ts` rather than read.
- **Design values live in tokens**, not in components. A hex code in a `.tsx` is
  a bug. Tokens are authored in `packages/design-tokens/tokens.css`;
  `tokens.json` is generated from it and a test fails when the two drift.
- **The commit rules in [`CLAUDE.md`](../CLAUDE.md) are not decoration.** No
  attribution trailers, no tool names, in commits or anywhere else. Run
  `git config core.hooksPath .githooks` after a fresh clone so the hook backs
  the rule up.

---

## Working economically

Habits that keep a session's context small enough to stay useful:

1. **Route, then read.** Use the table above. Opening `crates/store/src/lib.rs`
   whole costs ~25 000 tokens; `grep -n "TABLE IF NOT EXISTS messages" -A 20` costs
   almost nothing and usually answers the question.
2. **`grep` for the symbol, then read the range** — `sed -n '400,460p'`. Read
   whole files only when changing their structure.
3. **Trust the module headers.** Every crate's `lib.rs` opens with a doc comment
   stating what it may and may not do. Fourteen lines that save reading the
   crate.
4. **Narrow the build.** `cargo test -p nexo-client` over
   `cargo test --workspace`; `cargo clippy -p <crate>` over the workspace.
5. **Do not re-derive what a doc already settled.** If a decision looks odd,
   the reason is written down — usually in `RESEARCH-COMPARISON.md` or a comment
   at the point of the decision. Search before re-litigating.
6. **`STATUS.md` before "this feature is missing".** It was written by walking
   the code, and it is current.
7. **`.\scripts\check.ps1` before pushing**, not a guess about what CI wants.
   One validated push beats three speculative ones.
