# Progress — M2 / Local Postgres Infrastructure

**Plan:** [`docs/superpowers/plans/2026-08-25-m2-local-postgres-infra.md`](../../../docs/superpowers/plans/2026-08-25-m2-local-postgres-infra.md)
**Branch:** `feature/m2-local-postgres-infra`
**Status:** all four plan tasks complete, plus object storage. Not pushed.

---

## State at session start

The working tree was clean at `cb15c39` ("postgres infra", which adds only the
plan file). A previous session had built a database layer and an object storage
module; none of it was committed, and it was gone — the tree looked like a
`git reset --hard` followed by `git clean -fd`. Untracked files never enter the
object database, so git could not recover them and there was no stash. `.env`
survived because it is ignored; `docs/TUTORIAL.md` did not, because it was
untracked.

**This ledger did not exist.** The invoking message referred to a preflight scan
and a recorded blocker in it. Neither was readable, so every check below was
re-run from scratch. If a future session finds this file, it is the record that
was missing.

---

## Preflight (re-verified, not inherited)

| Claim in the plan | Verified |
|---|---|
| `.gitignore:14-16` = `.env` / `.env.*` / `!.env.example` | ✅ exact |
| `server` CI job at ci.yml lines 72–91 | ✅ matches |
| `docker-compose.yml`, `.env.example`, `.cargo/config.toml` absent | ✅ |
| Postgres 17 target (OPS.md Phase 4 supersedes BRIEF §3's "16") | ✅ |

---

## Tasks

- [x] **Task 1** — Local Postgres via Docker Compose · `06d039b`
- [x] **Task 2** — sqlx, offline-by-default, users/devices migration · `ccc9069`
- [x] **Task 3** — pool, AppState, first compile-checked query · `20fcd85`
- [x] **Task 4** — CI Postgres service · `4789677`
- [x] **Extra** — Hetzner object storage module · `f527990`

Task 4 Step 2 (`git push`) is **not done**: it creates a remote branch and
spends Actions minutes, so it was left for the human.

---

## Deviations from the plan, and why

### 1. Host port 5433, not 5432 — *this was the blocker*

A native `postgresql-x64-17` Windows service is running and holds
`127.0.0.1:5432` and `::1:5432`. Docker publishing the same port binds only the
IPv6 wildcard `:::5432`, so `localhost` still reaches the **native** server. The
symptom is an authentication failure against a database you did not mean to
contact, and because that server runs a German locale, sqlx cannot even decode
the error text — it reports *"Postgres returned a non-UTF-8 string for its error
message"*, which points nowhere near the real cause.

`docker-compose.yml` publishes `5433:5432`. Container-internal and CI stay 5432.

### 2. `Copy-Item .env.example .env` was NOT run

Task 1 Step 5 says to copy the template over `.env`. The live `.env` contained
four filled-in Hetzner S3 access/secret keys. Copying would have destroyed them.
`DATABASE_URL` was merged in instead; every `NEXO_S3_*` line was left untouched.
Backup taken before editing.

**A future session must not blind-copy `.env.example` over `.env`.**

### 3. The plan fails its own CI gate

`AppState { db: PgPool }` with nothing reading `db` is `dead_code`, and both the
`client` and `server` jobs run `clippy -D warnings`. Fixed with
`#[expect(dead_code)]` rather than `#[allow]`, so the attribute would start
warning — and force its own deletion — as soon as a handler read the field.

It did: adding the lib target for object storage made the field public API, the
lint stopped firing, and the attribute was removed. Working as intended.

### 4. `cargo sqlx prepare --workspace` writes an empty cache

It reports **"no queries found"**. The only `sqlx::query!` lives in a
`#[cfg(test)]` module, and `prepare` does not compile test targets by default.
Correct invocation:

```powershell
$env:SQLX_OFFLINE = "false"
cargo sqlx prepare --workspace -- --all-targets
```

Without `-- --all-targets` the Task 3 Step 6 offline proof passes vacuously —
a plain `cargo build` never compiles the macro either — and CI fails on the
first real build.

### 5. Supply chain, two failures the plan did not anticipate

- `tls-rustls` pulls `webpki-roots` under `CDLA-Permissive-2.0`, absent from
  `deny.toml`'s allow list → `licenses FAILED`. Added: it is the licence
  Mozilla publishes its root CA bundle under, permissive, no copyleft, and the
  crate carries certificate data rather than code.
- `cargo audit` flags `RUSTSEC-2023-0071` in `rsa`, reached only through
  sqlx's optional MySQL driver. `cargo tree -i rsa --target all` finds nothing.
  Recorded in `.cargo/audit.toml`.

### 6. `sqlx-cli` pinned to 0.8.6

Bare `cargo install sqlx-cli` gives 0.9.0 against a 0.8.6 library. Pinned both
locally and in CI (`tool: sqlx-cli@0.8.6`).

---

## Open finding — object storage isolation does not hold

`cargo test -p nexo-server --test s3_smoke -- --ignored` against the real
buckets:

| Test | Result |
|---|---|
| `both_buckets_are_reachable` | ✅ |
| `a_media_object_round_trips` | ✅ |
| `a_twenty_megabyte_encrypted_object_round_trips` | ✅ — M6's acceptance criterion |
| `media_credentials_cannot_reach_the_encrypted_bucket` | ❌ **fails** |

The media credentials **can** reach `nexo-enc`. This is not the "same pair
pasted twice" mistake: the two access keys and two secrets were compared and are
genuinely different.

The cause is that **Hetzner S3 keys are project-wide by default** — every key
reads and writes every bucket in the project
([docs](https://docs.hetzner.com/storage/object-storage/faq/s3-credentials/)).
`docs/OPS.md` Phase 8 said to "generate separate credentials per bucket", which
on its own achieves nothing. Restricting a key needs a **bucket policy**, or the
two buckets in separate projects. Phase 8 now carries the correction and a
policy template.

**The failing test is left failing.** It is reporting a true property of the
deployment, and it is `#[ignore]`d so CI is unaffected. It must not be weakened
to green. Until a bucket policy is applied and it passes, `nexo-enc` is readable
by any key in the project and `docs/THREAT-MODEL.md` should say so.

---

## Verified

`fmt` · `clippy --workspace --all-targets -D warnings` · `test --workspace` ·
both `cargo deny` targets · `cargo audit` — all green.

Beyond the gate:

- compiled with Postgres **stopped** and `DATABASE_URL` **unset** — a genuine
  50s rebuild of the test targets, not a cache hit
- tests pass with no database at all (the Windows CI condition), skip path clean
- server starts, `/v1/health` 200, and logs
  `object storage configured media="nexo-media" encrypted="nexo-enc"` —
  bucket names only, never a key

---

## Session 2 — M2 proper (2026-08-25, later)

The local-postgres plan was already merged as PR #3. This session continued
into M2 itself, after fixing the build break that started it: `node_modules`
was behind the lockfile after the pull that added four Tauri plugins, so
`@tauri-apps/plugin-dialog` did not resolve. `pnpm install` fixed it; the
plugins were also declared as `"2"` where every other dependency here is an
exact pin, so both sides were pinned.

Landed, each with tests and a green gate:

| Commit | What |
|---|---|
| `8527aa2` | `docs/THREAT-MODEL.md`, restored `docs/TUTORIAL.md`, scheduled gaps G1–G6 |
| `f40d716` | Server auth: register, login, rotating refresh, logout |
| `6e59b0a` | Windows DPAPI implementation of `SecureStore` |
| `0a69105` | `crates/store`: SQLCipher database keyed through the keystore |
| `67cee8c` | `crates/crypto`: identity keypair and safety numbers |
| `f6e8105` | `crates/client`: session logic (the prompt's `api-client`) |
| `b768234` | Tauri auth commands, the login/register screen, and the HTTP transport |

### Decisions a future session should not silently reverse

- **`crates/platform` is `deny(unsafe_code)`, not `forbid`.** It is the only
  crate in the workspace that is, and the exception is two FFI calls in
  `dpapi::ffi`. DPAPI is a C API; `forbid` cannot be locally overridden. The
  alternative was an unmaintained crates.io wrapper, which rule 8 excludes.
- **`jsonwebtoken` uses the `aws_lc_rs` backend.** The `rust_crypto` backend
  bundles `ed25519-dalek` and `rsa` into one inseparable feature, and `rsa`
  carries RUSTSEC-2023-0071, which `.cargo/audit.toml` records as never
  compiled. Taking that feature would make the entry false.
- **`rusqlite` is pinned to 0.32.1**, five releases back and not by choice:
  it and `sqlx-sqlite` both `links = "sqlite3"` and cargo allows only one.
  sqlx-sqlite is never compiled, but optional dependencies still take part in
  resolution. sqlx 0.9 widens the range enough to reach rusqlite 0.37 —
  take it at the next sqlx bump.
- **The client transport is a trait, not an HTTP client.** The concrete
  implementation belongs to M4. This keeps `reqwest` out of the desktop until
  something needs it and makes the session logic testable now.

### The bug only a live test could find

`crates/client/tests/live_auth.rs` runs the real client against a real server.
On its first run it failed at login, and the cause was a genuine design fault
that no amount of fake-transport testing would have shown:

**Registration and login were deriving the verifier against different salts.**
At registration the handle does not exist yet, so `/v1/auth/salt` returns a
*decoy* — which is the entire point of that endpoint. The server then minted
its own salt when creating the account. At login the endpoint returned that
stored salt, the client derived a different verifier, and the password never
matched. **Every account created that way would have been impossible to log
into.**

Fixed by making the salt client-chosen at registration: the client generates
16 random bytes, derives against them, and sends them with the request. A salt
needs uniqueness, not secrecy, and a client that picks a badly one only weakens
itself. Both halves now agree by construction rather than by coincidence.

This is the argument for keeping the live test: a fake transport agrees with
whatever the client sends, so it cannot catch two correct-looking sides
disagreeing about a protocol.

### Two more bugs found by writing the tests

- A safety-number test asserted `[0xFF; 32]` is an invalid Ed25519 key. It is
  valid — roughly half of all 32-byte strings decompress to real points — so
  the assertion proved nothing. It now uses an encoding verified invalid in
  the test itself.
- `session::login` passed the handle as the display name, quietly renaming the
  account on every sign-in. It now keeps what is on record.

---

## Session 3 — M3 and most of M4

| Commit | What |
|---|---|
| `9fe3783` | **M3 complete** — the conversation API, and the commit-race rule |
| `3c1fbf3` | M4: delivery service — key packages, conversations, commit ordering |
| `b66fb4b` | M4: WebSocket transport and the fan-out seam |
| `2271cae` | M4: MLS state persisted, so a conversation survives a restart |

### Two design bugs the tests caught, both mine

- **`Conversation::rekey` merged its own commit immediately**, which silently
  assumes your commit always wins. With two clients rekeying against the same
  epoch one must lose, and a client that merged optimistically would believe it
  had moved to an epoch nobody else was in — every message it sent afterwards
  undecryptable to everyone. Commits now stage, and the caller calls
  `confirm_commit` or `abandon_commit`. This is the client half of risk 4(b);
  the server half followed in `3c1fbf3`.
- The M3 test suite initially asserted `[0xFF; 32]` is an invalid Ed25519 key.
  It is valid — roughly half of all 32-byte strings decompress to real points —
  so the assertion proved nothing.

### Decisions worth not reversing

- **MLS state is one blob, not a `StorageProvider`.** One device, one process,
  kilobytes of state, and it either round-trips or it does not.
  `MemoryStorage::serialize` is gated behind `test-utils`, so the public
  `values` map is used instead — shipping a test-only feature is not a trade
  worth making. Versioned, so a later `StorageProvider` can migrate off it.
- **G5: the fan-out seam exists, Redis does not.** `LocalHub` is correct while
  `OPS.md` Phase 7 runs one systemd unit. Redis slots in behind `Fanout` when
  there is a second instance — same trigger as G6.
- **The MLS signing key is the identity key.** Otherwise a safety number
  verifies a key that signs nothing.
- **Rekey counters are not persisted.** They are not MLS state, and a counter
  that drifts out of step with the epoch it describes is worse than a restart
  occasionally delaying a rekey.

### M4 remaining

Server side is done and proven: `two_clients_exchange_real_end_to_end_encrypted_messages`
runs real MLS through the real server and asserts no plaintext in what was
stored. What is left is client-side:

1. Conversation commands in `crates/client` — create, send, sync — over the
   HTTP transport, persisting through `mls_state` and the message tables.
2. The WebSocket client and its reconnect-then-sync loop.
3. Wiring `apps/desktop/src/features/messages` to real data instead of
   `src/mock`, and the safety-number display in the conversation header.

M4's check is "two machines exchange real E2EE messages; safety numbers
match" — (1) and (3) are what remain before that can be demonstrated.

---

## Next session

1. `docker compose up -d` (5433). `.env` is already correct — **do not**
   overwrite it from the template.
2. Push the branch and confirm CI: the signal is `server`'s
   `users_table_is_reachable_after_migration ... ok` rather than the skip
   message.
3. Apply the `nexo-enc` bucket policy, then re-run the s3_smoke test until
   `media_credentials_cannot_reach_the_encrypted_bucket` passes.
4. ~~`docs/TUTORIAL.md` was lost~~ — restored in `8527aa2`, rewritten against
   the current code. A link checker over all docs reports zero broken links.
5. ~~`docs/PLAN.md` says "Postgres 16"~~ — corrected to 17.
6. **G6 is still an open decision**: Docker Compose deployment (what the build
   prompt asks for) versus the systemd runbook in `OPS.md` (what exists and is
   tested). Pick one deliberately; do not let the repo drift into half of each.
7. ~~M2's remaining work is the Tauri command layer and the UI~~ — both landed
   in `b768234`, along with the HTTP transport pulled forward from M4. M2 is
   complete and verified against a running server. **M3 (OpenMLS in isolation)
   is the next milestone.**
8. G6 decided: **Compose is a development tool, production is systemd.**
   Reasoning is in PLAN.md. `OPS.md` still needs a deploy script to close the
   one real gap versus Compose (rollback).
