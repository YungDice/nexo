# Nexo

An end-to-end encrypted messenger for Windows 10/11, with a public feed and
public profiles alongside private conversations.

- **Messages** are end-to-end encrypted with MLS ([RFC 9420](https://www.rfc-editor.org/rfc/rfc9420.html)),
  via [OpenMLS](https://github.com/openmls/openmls). The server stores and
  forwards ciphertext it cannot read.
- **Feed posts and profiles are not end-to-end encrypted.** They are readable by
  the server, and public to any logged-in user. Content meant to be readable by
  strangers cannot also be encrypted to a closed group, and the app says so
  rather than implying otherwise.
- **Conversation metadata** — who talks to whom, when, and message sizes — is
  visible to the server. That is the honest limit of this design.

[`docs/CONTEXT.md`](docs/CONTEXT.md) is the map of the repository — where every
concern lives, which document answers which question, and which two or three
files a given task actually needs. Start there. Then
[`docs/STATUS.md`](docs/STATUS.md) for what the app does today and what
is known to be broken, [`docs/PLAN.md`](docs/PLAN.md) for the build plan and the open risks,
[`docs/OPS.md`](docs/OPS.md) for the Hetzner runbook, and
[`docs/BRIEF.md`](docs/BRIEF.md) for the original specification.

## Layout

```
apps/desktop        Tauri 2 + React 19 client (Windows)
apps/server         axum API and MLS Delivery Service (Linux, aarch64)
crates/protocol     Wire types shared by both. No I/O, no crypto.
crates/crypto       MLS, the identity keypair, and safety numbers.
crates/platform     The OS seam: SecureStore, and the Windows DPAPI backing.
crates/store        The client's SQLCipher database.
crates/client       Session logic. No platform calls, no HTTP client.
```

`crates/client` is what the build prompt called `packages/api-client`: the
client-side logic that is identical on Windows and Android, so the port reuses
it instead of reimplementing it. It reaches the OS through `SecureStore` and
the network through a `Transport` trait, both supplied by the shell around it.

Inside the client:

```
src/components/ui       Buttons, avatars, panes, controls, the icon set
src/components/chrome   Titlebar and the icon rail
src/features/{home,messages,profile,settings}
src/mock                The data every surface reads until the network exists
```

Colour, type, radius, motion and the glass utilities are not in the client at
all: they are authored in `packages/design-tokens/tokens.css` and imported by
`src/main.tsx`, so a second platform can consume the same values as
`tokens.json` rather than a copy.

`crates/protocol`, `crates/crypto` and `crates/platform` must compile unchanged
for Android; keep every platform call behind `nexo-platform`.

Inside the server:

```
src/db.rs           The Postgres pool.
src/state.rs        AppState: the pool and, from M6, object storage.
src/storage.rs      Hetzner object storage. Two buckets the types keep apart.
migrations/         Applied with sqlx-cli; checked into .sqlx for offline builds.
tests/s3_smoke.rs   Ignored by default. Needs real credentials.
```

## Getting started

Windows 10 1809+ or Windows 11, on x86_64.

### 1. Prerequisites

| | Version | Notes |
|---|---|---|
| [Rust](https://rustup.rs/) | 1.97.1 | Pinned by `rust-toolchain.toml`; rustup installs it for you on the first build. |
| [Node.js](https://nodejs.org/) | 24.x | |
| pnpm | 10.20.0 | `npm install -g pnpm@10.20.0` |
| Visual Studio Build Tools | 2022 or newer | Workload **Desktop development with C++**, which must include *MSVC v14.x — VS 2022+ C++ x64/x86 build tools* and the *Windows 11 SDK*. Without the x64 CRT nothing links. |
| WebView2 Runtime | Evergreen | Already present on Windows 11 and on updated Windows 10. The installer ships a bootstrapper for machines that lack it. |
| [Strawberry Perl](https://strawberryperl.com/) | any | **Not needed yet.** Required from M2 onward, when SQLCipher starts building a vendored OpenSSL. `winget install StrawberryPerl.StrawberryPerl` |
| CMake | any | Needed by `aws-lc-sys`, which the AWS S3 SDK builds for its TLS. Strawberry Perl ships one, so installing that usually covers it. On the aarch64 server: `apt install cmake`. |

### 2. Install

```powershell
git clone https://github.com/YungDice/nexo.git
cd nexo
pnpm install
```

Re-run `pnpm install` after any pull that changes `pnpm-lock.yaml`. A stale
`node_modules` shows up as `Can't resolve '@fontsource/...'` during a build.

### 3. Run

The desktop app, with hot reload on the React side:

```powershell
pnpm tauri dev
```

The first run compiles the Rust core and takes a couple of minutes; later runs
start in seconds. Editing anything under `apps/desktop/src` reloads instantly;
editing Rust rebuilds and relaunches the window.

The API server, separately, in its own terminal. It now needs a local Postgres.
Start it once with Docker, then copy the env template:

```powershell
docker compose up -d
Copy-Item .env.example .env    # only needed once
```

Compose publishes Postgres on **5433**, not 5432, so a native Windows
PostgreSQL install cannot be contacted by mistake. Then:

```powershell
pnpm dev:server
# -> http://127.0.0.1:8080/v1/health  {"status":"ok","protocol_version":1}
```

Nothing in the client talks to it yet — that is M4. Set `NEXO_BIND` to listen
somewhere other than `127.0.0.1:8080`.

Both of these prepare their own build environment, so they work in any shell.

### 4. Using cargo directly

Raw `cargo` commands — `cargo test`, `cargo clippy`, `cargo build` — need the
environment set up first, once per terminal:

```powershell
. .\scripts\dev-env.ps1
cargo test --workspace
```

Dot-source it (the leading `. `). It edits the current session rather than a
child process, so running it without the dot does nothing useful. It finds a
usable MSVC toolchain, puts Strawberry Perl on `PATH` when it is installed, and
says what is missing. [Why it is needed](#why-dev-envps1-exists).

You can also serve the UI alone in a browser at `http://localhost:1420`:

```powershell
pnpm dev
```

That is useful for pure layout work, but anything that calls into Rust — the
titlebar buttons, Settings — will not work, because there is no Tauri process
behind it. Prefer `pnpm tauri dev`.

### 5. Before you push

```powershell
. .\scripts\dev-env.ps1     # if this shell has not had it yet
.\scripts\check.ps1
```

Runs exactly what CI runs: `cargo fmt`, `cargo clippy -D warnings`, the Rust
tests, both `cargo deny` passes, `cargo audit`, `pnpm typecheck` and
`pnpm build`.

### Building a release binary

```powershell
. .\scripts\dev-env.ps1
pnpm build          # must come first: the Rust client embeds apps/desktop/dist
cargo build --release
```

The binary lands at `target\release\nexo-desktop.exe`. `pnpm tauri build`
produces the NSIS installer, though signing and the updater are M9 work.

### Why `dev-env.ps1` exists

Two things on a stock Windows box stop this repo from building, and neither is
a code problem:

1. **Multiple Visual Studio installs.** Rust picks the newest MSVC it finds,
   which is not always the complete one. An install carrying only `lib\onecore`
   fails to link with `LNK1104: cannot open file 'msvcrt.lib'`. The script picks
   the newest install that actually has the desktop x64 CRT.
2. **Perl.** From M2 onward, SQLCipher's vendored OpenSSL needs a full Perl to
   run `Configure`. The Perl inside Git for Windows is not complete enough — it
   fails on a missing `Locale::Maketext::Simple`. Install Strawberry Perl:
   `winget install StrawberryPerl.StrawberryPerl`.

### Troubleshooting

| Symptom | Cause |
|---|---|
| `C1083: Cannot open include file: 'excpt.h'`, or `LNK1104: cannot open file 'msvcrt.lib'` | A raw `cargo` command in a shell that has not had `.\scripts\dev-env.ps1` dot-sourced, so cc-rs auto-detected an incomplete Visual Studio. Dot-source it, or use `pnpm tauri dev` / `pnpm dev:server`, which do it themselves. If the script reports no usable install, add the **Desktop development with C++** workload in the Visual Studio Installer. |
| `Can't resolve '@fontsource/...'` | `node_modules` is behind the lockfile. Run `pnpm install`. |
| `Command 'perl' not found` or `Locale::Maketext::Simple` | Install Strawberry Perl and open a fresh shell (M2 onward only). |
| `pnpm: command not found` after `corepack enable` | `corepack enable` needs administrator rights. Use `npm install -g pnpm@10.20.0` instead. |
| `pnpm server` prints nothing and exits | `server` is one of pnpm's own commands (its store daemon), so the shorthand never reaches the script. The script is named `dev:server` for this reason. |
| `Port 1420 is already in use` | A previous `pnpm tauri dev` is still running; the port is fixed on purpose so Tauri and Vite cannot disagree about it. Free it with `Get-NetTCPConnection -LocalPort 1420 -State Listen \| ForEach-Object { Stop-Process -Id $_.OwningProcess -Force }`. |

## Design

**Colour means something.** The interface itself is one grey scale plus a single
violet accent — the accent marks the active destination, your own messages, the
primary action and the focus ring; the semantic colours only ever report a
status. Everything else is neutral. **Content keeps its colour**: avatars, images
and file marks are things people put there, and greying them out makes the app
harder to scan, not calmer.

**Light and dark.** §6.4 planned dark only for v0.1 with "a theme seam in
place" — the seam is now used, because every surface, line and fill is a named
token and a theme is the same names with different values. Settings →
Appearance offers System, Light and Dark; System is the absence of an explicit
choice, so `prefers-color-scheme` answers and Windows switching at sunset
switches the app with it. No component knows which theme it is in.

Panels are genuinely translucent over a blurred, neutral field, and the window
is a card floating on that field with a margin — going edge to edge when
maximised. Blur is not free: **Settings → Appearance → Frosted panels** turns it
off and makes every pane solid, and a WebView that cannot blur at all gets the
same treatment through an `@supports` branch. Components ask for `glass-0`
through `glass-3` and never write `backdrop-filter` themselves, which is what
keeps both fallbacks honest.

Icons are inline SVG drawn in this repo, and avatars, banners and media
placeholders are gradients derived from a handle or an id. Nothing is fetched at
runtime, and the CSP has no remote image host to allow.

## Security

- No cryptography is invented here. Only OpenMLS and its audited primitives.
- No key material, no private keys and no plaintext-at-rest ever enter the
  WebView. It receives already-decrypted strings over IPC and nothing else.
- Nothing is loaded at runtime: strict CSP, no CDN, no `eval`, everything
  bundled.
- Decryption failure fails closed and is shown as such. There is no plaintext
  fallback and no silent skip.

[`docs/THREAT-MODEL.md`](docs/THREAT-MODEL.md) names the adversaries in and out
of scope, and is blunt about what is not protected: feed posts, profiles and
their media are server-readable by design, and so is conversation metadata.

## Licence

MIT — the text in [`LICENSE`](LICENSE), unedited on purpose so that licence
scanners recognise it. Copyright is held under `delidev`, jointly by the two
people who write here; see [`docs/LICENSING.md`](docs/LICENSING.md).

That document is the precise version: what MIT obliges us to ship alongside the
installer, which parts of its warranty disclaimer Swiss law does not enforce
(Art. 100 OR, Art. 8 PrHG), what each dependency licence asks of us, the two
vendored native libraries `cargo deny` cannot see, and the export-control
position. It also names the three exposures a licence file cannot fix: the
trademark on the name, BÜPF/VÜPF duties for a Swiss communications service, and
revDSG/GDPR obligations once real users exist.

[`docs/THIRD-PARTY-NOTICES.md`](docs/THIRD-PARTY-NOTICES.md) is the other half:
what has to reach someone who installs the binary and never opens GitHub. Seven
of those notices — SQLCipher, two OpenSSL-derived components, the two bundled
fonts — are invisible to `cargo deny` and to `pnpm audit`, so they are carried
by hand.
