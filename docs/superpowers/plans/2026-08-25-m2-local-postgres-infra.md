# M2 / Local Postgres Infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `nexo-server` a real Postgres database to talk to — local Docker Compose Postgres for dev, `sqlx` wired into the server with compile-time-checked queries, the `users`/`devices` migration applied, and CI exercising it — so every later M2 task (register/login endpoints, identity keypair registration) has a database to write against.

**Architecture:** `docker-compose.yml` runs Postgres 17 locally, matching the version OPS.md targets in production. `sqlx` compiles queries against a checked-in `.sqlx` offline cache by default (via `SQLX_OFFLINE=true` in `.cargo/config.toml`), so `cargo build`/`cargo test --workspace` never require a live database just to compile — including on the Windows `client` CI job, which builds the whole workspace. Only tests that actually run a query need a reachable database; those check `DATABASE_URL` at runtime and skip cleanly if it's absent, so they run for real only where a database is available (local dev with Docker running, and the Linux `server` CI job once this plan adds a Postgres service container to it).

**Tech Stack:** Postgres 17, Docker Compose, `sqlx` 0.8.x (`postgres`, `runtime-tokio`, `tls-rustls`, `macros`, `migrate` features), `sqlx-cli`, `dotenvy`.

**Spec:** [docs/BRIEF.md](../../BRIEF.md) §5.1 (schema), §3 (`PostgreSQL 16 + sqlx (compile-time checked queries)` — target is actually 17 per [docs/OPS.md](../../OPS.md) Phase 4, which supersedes the brief's number); [docs/PLAN.md](../../PLAN.md) M2 row; [docs/OPS.md](../../OPS.md) Phase 4 (production bootstrap, mirrored here for local dev) and the note that M2 "runs entirely on a development machine against a local Postgres."

## Global Constraints

- Every direct dependency is pinned to an exact version (`Cargo.toml` line 18 comment) — this applies to `sqlx` and `dotenvy` too.
- No plaintext secrets committed — `.env` stays gitignored (already true; verify, don't assume).
- `cargo test --workspace` (the Windows `client` CI job, [.github/workflows/ci.yml:64](../../../.github/workflows/ci.yml#L64)) must keep passing **without** a database present.
- Rule 4 (server must never read message content) doesn't apply to this plan — `users`/`devices` are metadata by design (BRIEF.md §4.4).

---

## File structure

| File | Change | Responsibility |
|---|---|---|
| `docker-compose.yml` | create | Local Postgres 17 for dev |
| `.env.example` | create | `DATABASE_URL` template, already referenced by `.gitignore` |
| `.cargo/config.toml` | create | Default `SQLX_OFFLINE=true` for all cargo invocations |
| `Cargo.toml` (root) | modify | Add `sqlx`, `dotenvy` to `[workspace.dependencies]` |
| `apps/server/Cargo.toml` | modify | Reference `sqlx`, `dotenvy` |
| `apps/server/migrations/*.sql` | create | First migration: `users`, `devices` tables |
| `apps/server/src/db.rs` | create | `create_pool()` + the first compile-checked query and its test |
| `apps/server/src/state.rs` | create | `AppState { db: PgPool }` + a test-only lazy-pool helper |
| `apps/server/src/main.rs` | modify | Load `.env`, build the pool, thread `AppState` through `router()` |
| `.sqlx/` | create (generated) | Checked-in offline query cache |
| `.github/workflows/ci.yml` | modify | Postgres service + `sqlx-cli` + `sqlx migrate run` in the `server` job only |
| `README.md` | modify | Note `docker compose up -d` before `pnpm dev:server` |

---

### Task 1: Local Postgres via Docker Compose

**Files:**
- Create: `docker-compose.yml`
- Create: `.env.example`
- Modify: `README.md` (the "Run" section, around the existing `pnpm dev:server` block)

**Interfaces:**
- Produces: a reachable Postgres at `postgres://nexo:nexo_dev@localhost:5432/nexo`, which every later task in this plan depends on.

- [ ] **Step 1: Create the Compose file**

`docker-compose.yml` (repo root):
```yaml
services:
  postgres:
    image: postgres:17
    restart: unless-stopped
    environment:
      POSTGRES_USER: nexo
      POSTGRES_PASSWORD: nexo_dev
      POSTGRES_DB: nexo
    ports:
      - "5432:5432"
    volumes:
      - nexo-postgres-data:/var/lib/postgresql/data
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U nexo -d nexo"]
      interval: 5s
      timeout: 5s
      retries: 10

volumes:
  nexo-postgres-data:
```

- [ ] **Step 2: Create the env template**

`.env.example` (repo root — `.gitignore` already has `.env` / `.env.*` ignored and `!.env.example` un-ignored, confirmed at `.gitignore:14-16`, so no `.gitignore` change is needed):
```
DATABASE_URL=postgres://nexo:nexo_dev@localhost:5432/nexo
NEXO_BIND=127.0.0.1:8080
RUST_LOG=nexo_server=debug,tower_http=debug
```

- [ ] **Step 3: Bring it up and verify**

```powershell
docker compose up -d
docker compose ps
```
`postgres` should show `healthy` within ~10 seconds. Then:
```powershell
docker compose exec postgres psql -U nexo -d nexo -c "select 1"
```
Should print `1`.

- [ ] **Step 4: Copy the env template for local use**

```powershell
Copy-Item .env.example .env
```

- [ ] **Step 5: Document the new prerequisite in README**

In `README.md`, immediately before the existing block:
```powershell
pnpm dev:server
# -> http://127.0.0.1:8080/v1/health  {"status":"ok","protocol_version":1}
```
add:
```markdown
The server now needs a local Postgres. Start it once with Docker, then copy
the env template:

```powershell
docker compose up -d
Copy-Item .env.example .env    # only needed once
```
```

- [ ] **Step 6: Commit**

```bash
git add docker-compose.yml .env.example README.md
git commit -m "infra: add local Postgres via Docker Compose"
```

---

### Task 2: `sqlx` dependency, offline-by-default compilation, first migration

**Files:**
- Create: `.cargo/config.toml`
- Modify: `Cargo.toml` (root, `[workspace.dependencies]`)
- Modify: `apps/server/Cargo.toml`
- Create: `apps/server/migrations/<generated-timestamp>_create_users_devices.sql`

**Interfaces:**
- Consumes: the running Postgres from Task 1 (`DATABASE_URL` in `.env`).
- Produces: `sqlx.workspace = true` and `dotenvy.workspace = true` available to any crate; a `users`/`devices` schema applied to the dev database, for Task 3 to query.

- [ ] **Step 1: Default all cargo builds to sqlx offline mode**

Create `.cargo/config.toml`:
```toml
[env]
SQLX_OFFLINE = "true"
```
This is why the Windows `client` job's `cargo test --workspace` ([.github/workflows/ci.yml:64](../../../.github/workflows/ci.yml#L64)) will keep compiling `nexo-server` without a database once it uses `sqlx::query!` — the macro reads the checked-in `.sqlx/` cache (created in Step 6) instead of connecting live. Cargo's `[env]` table does not override a variable already set in the invoking shell (no `force = true` here), so explicitly setting `SQLX_OFFLINE=false` in a shell before running `cargo sqlx prepare` (Step 6) still works.

- [ ] **Step 2: Add sqlx and dotenvy to the workspace, pinned exactly**

Run, from the repo root:
```powershell
cargo add --package nexo-server sqlx --no-default-features --features runtime-tokio,tls-rustls,postgres,macros,migrate
cargo add --package nexo-server dotenvy
```
This adds both crates directly under `apps/server/Cargo.toml`'s `[dependencies]` with whatever the current resolved versions are. Now move them into the shared pin location, matching every other entry in the workspace:

1. Open `apps/server/Cargo.toml`, note the exact version `cargo add` wrote for `sqlx` (e.g. `sqlx = { version = "0.8.3", features = [...] }`) and `dotenvy` (e.g. `dotenvy = "0.15.7"`).
2. In root `Cargo.toml`, add to `[workspace.dependencies]` (after the `tower-http` line, before the MLS block, matching the existing `# --- async / server ---` grouping):
   ```toml
   sqlx = { version = "=<the version cargo add resolved>", default-features = false, features = ["runtime-tokio", "tls-rustls", "postgres", "macros", "migrate"] }
   dotenvy = "=<the version cargo add resolved>"
   ```
   (use `=` to pin exactly, matching every other line in that block — e.g. `axum = "=0.8.9"`)
3. In `apps/server/Cargo.toml`, replace the two lines `cargo add` wrote with:
   ```toml
   sqlx.workspace = true
   dotenvy.workspace = true
   ```

- [ ] **Step 3: Verify it builds**

```powershell
cargo check -p nexo-server
```
Should succeed (no `sqlx::query!` calls exist yet, so nothing touches the database at this point).

- [ ] **Step 4: Install sqlx-cli**

```powershell
cargo install sqlx-cli --no-default-features --features rustls,postgres
```

- [ ] **Step 5: Generate the first migration**

```powershell
cd apps/server
sqlx migrate add create_users_devices
```
This creates `apps/server/migrations/<timestamp>_create_users_devices.sql` with a real generated timestamp — replace its contents with:
```sql
CREATE EXTENSION IF NOT EXISTS citext;

CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    handle CITEXT UNIQUE NOT NULL CHECK (handle ~ '^[a-z0-9_]{3,20}$'),
    display_name TEXT NOT NULL,
    bio TEXT,
    location TEXT,
    avatar_key TEXT,
    banner_key TEXT,
    pw_salt BYTEA NOT NULL,
    pw_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE devices (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id BIGINT NOT NULL REFERENCES users(id),
    identity_pubkey BYTEA UNIQUE NOT NULL,
    name TEXT,
    last_seen TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
```
This is exactly the `users`/`devices` portion of the schema in [docs/BRIEF.md §5.1](../../BRIEF.md#L120-L149) (`posts`, `conversations`, etc. belong to later milestones — M2 only needs auth + identity). The `handle` `CHECK` constraint enforces BRIEF.md §4.1's format rule (`3–20 chars, [a-z0-9_]`) server-side, not just client-side. `gen_random_uuid()` is built into Postgres 13+ core, no extra extension needed.

- [ ] **Step 6: Apply the migration and confirm**

Still in `apps/server`:
```powershell
sqlx migrate run
```
Then verify:
```powershell
docker compose exec postgres psql -U nexo -d nexo -c "\d users"
docker compose exec postgres psql -U nexo -d nexo -c "\d devices"
```
Both should show the columns above.

- [ ] **Step 7: Commit**

```bash
cd ../..
git add .cargo/config.toml Cargo.toml apps/server/Cargo.toml apps/server/migrations
git commit -m "infra: add sqlx, offline-by-default builds, users/devices migration"
```

---

### Task 3: Connection pool, `AppState`, and the first compile-checked query

**Files:**
- Create: `apps/server/src/db.rs`
- Create: `apps/server/src/state.rs`
- Modify: `apps/server/src/main.rs`
- Create (generated): `.sqlx/`

**Interfaces:**
- Consumes: `sqlx.workspace`/`dotenvy.workspace` (Task 2), the migrated `users` table (Task 2).
- Produces: `pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error>` (`db.rs`); `pub struct AppState { pub db: PgPool }` and `#[cfg(test)] pub(crate) fn test_state() -> AppState` (`state.rs`); `fn router(state: AppState) -> Router` (`main.rs`, replacing the current no-argument `router()`) — later M2 tasks (auth endpoints) build on `AppState` and call `router(state)` the same way.

- [ ] **Step 1: Write `state.rs`**

`apps/server/src/state.rs`:
```rust
//! Shared application state handed to every axum handler.

use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

#[cfg(test)]
pub(crate) fn test_state() -> AppState {
    // `connect_lazy` never touches the network, so tests that build a
    // router and state but don't run a query (e.g. the health check)
    // don't need a real database.
    let db = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused/unused")
        .expect("connect_lazy only fails on a malformed URL");
    AppState { db }
}
```

- [ ] **Step 2: Write `db.rs` with its own test**

`apps/server/src/db.rs`:
```rust
//! Postgres connection pool.

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

/// Builds a connection pool from `DATABASE_URL`, connecting eagerly so a bad
/// connection string or an unreachable database fails fast at startup rather
/// than on the first request.
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn users_table_is_reachable_after_migration() {
        let Ok(database_url) = std::env::var("DATABASE_URL") else {
            eprintln!("skipping: DATABASE_URL not set (needs a running local Postgres)");
            return;
        };
        let pool = create_pool(&database_url)
            .await
            .expect("connect to test database — is `docker compose up -d` running?");
        let row = sqlx::query!("SELECT count(*) AS count FROM users")
            .fetch_one(&pool)
            .await
            .expect("users table exists and is queryable — did you run `sqlx migrate run`?");
        assert!(row.count.unwrap_or(0) >= 0);
    }
}
```

- [ ] **Step 3: Wire both into `main.rs`**

Replace the full contents of `apps/server/src/main.rs` with:
```rust
//! Nexo server: HTTP API, MLS Delivery Service, and WebSocket fanout.
//!
//! TLS is terminated by Caddy in front of this process (§5.2), so the server
//! itself listens plaintext on loopback and holds no certificate.
//!
//! The rule that shapes this whole binary: it stores and forwards ciphertext it
//! cannot read. If a handler is ever written that touches message plaintext,
//! that is a bug in the design, not a feature (rule 4).

#![forbid(unsafe_code)]

use std::net::SocketAddr;

use axum::{Router, routing::get};
use tower_http::trace::TraceLayer;

mod db;
mod health;
mod state;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "nexo_server=info,tower_http=info".into()),
        )
        .init();

    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set — see .env.example");
    let pool = db::create_pool(&database_url).await?;
    let state = AppState { db: pool };

    let app = router(state);
    let addr: SocketAddr = std::env::var("NEXO_BIND")
        .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
        .parse()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "nexo-server listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/health", get(health::health))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_responds_through_the_router() {
        let res = router(state::test_state())
            .oneshot(
                Request::builder()
                    .uri("/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unknown_routes_are_not_found() {
        let res = router(state::test_state())
            .oneshot(
                Request::builder()
                    .uri("/v1/nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}
```
`health::health` takes no `State<AppState>` extractor and still works inside a router built with `.with_state()` — axum allows state-less handlers alongside stateful ones in the same router, so `health.rs` itself needs no changes.

- [ ] **Step 4: Generate the offline query cache**

The `sqlx::query!` macro in `db.rs` needs to check itself against a live database once, with `SQLX_OFFLINE` overridden for this one command (`.cargo/config.toml` set it to `"true"` in Task 2, but that only applies when the variable isn't already set in the invoking shell):
```powershell
$env:SQLX_OFFLINE = "false"
cargo sqlx prepare --workspace
Remove-Item Env:\SQLX_OFFLINE
```
This writes a `.sqlx/` directory to the repo root containing one JSON file per checked query. **Commit this directory** — it's what lets `SQLX_OFFLINE=true` builds (every default build, including Windows CI) compile without a database.

- [ ] **Step 5: Run the full test suite**

```powershell
cargo test -p nexo-server
```
All four tests (`health_responds_through_the_router`, `unknown_routes_are_not_found`, `users_table_is_reachable_after_migration`, plus whatever `health.rs` already had) should pass — the first two need no database (lazy pool), the third needs the Task 1 Postgres running and will print a skip message instead of failing if it isn't.

- [ ] **Step 6: Prove offline mode actually works**

```powershell
docker compose stop postgres
$env:SQLX_OFFLINE = "true"
cargo build -p nexo-server
Remove-Item Env:\SQLX_OFFLINE
docker compose start postgres
```
The build must succeed with Postgres stopped — that's the whole point of this task. If it fails asking to run `cargo sqlx prepare`, Step 4 didn't commit the cache correctly; regenerate it.

- [ ] **Step 7: Commit**

```bash
git add apps/server/src/db.rs apps/server/src/state.rs apps/server/src/main.rs .sqlx
git commit -m "feat(server): wire a Postgres pool into AppState, offline-checked queries"
```

---

### Task 4: CI — Postgres service in the `server` job

**Files:**
- Modify: `.github/workflows/ci.yml` (the `server` job only, lines 72–91 as currently written)

**Interfaces:**
- Consumes: nothing new from earlier tasks beyond what's already committed (the `.sqlx` cache means the `client` job needs no changes at all).
- Produces: a CI job where `users_table_is_reachable_after_migration` actually runs (not skipped) and proves the migration applies cleanly on a fresh database every time.

- [ ] **Step 1: Add a Postgres service and `DATABASE_URL` to the `server` job**

Replace the `server` job in `.github/workflows/ci.yml` (currently lines 72–91) with:
```yaml
  server:
    name: Server (Linux aarch64)
    runs-on: ubuntu-24.04-arm
    env:
      DATABASE_URL: postgres://nexo:nexo_dev@localhost:5432/nexo
    services:
      postgres:
        image: postgres:17
        env:
          POSTGRES_USER: nexo
          POSTGRES_PASSWORD: nexo_dev
          POSTGRES_DB: nexo
        ports:
          - 5432:5432
        options: >-
          --health-cmd "pg_isready -U nexo -d nexo"
          --health-interval 5s
          --health-timeout 5s
          --health-retries 10
    steps:
      - uses: actions/checkout@v5
      - uses: dtolnay/rust-toolchain@stable
        with:
          toolchain: "1.97.1"
          components: clippy
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: sqlx-cli
      - run: sqlx migrate run --source apps/server/migrations
      # Only the server and the shared crates: the desktop client does not
      # build for Linux and is explicitly out of scope (brief section 11).
      - run: cargo clippy -p nexo-server -p nexo-protocol -p nexo-crypto -p nexo-platform --all-targets -- -D warnings
      - run: cargo test -p nexo-server -p nexo-protocol -p nexo-crypto -p nexo-platform
      - run: cargo build --release -p nexo-server
      - uses: actions/upload-artifact@v4
        with:
          name: nexo-server-linux-arm64
          path: target/release/nexo-server
          retention-days: 7
```
Nothing else in the file changes — the `frontend`, `client`, and `supply-chain` jobs are untouched, and the `client` job's `cargo test --workspace` keeps working with no database because of the `.sqlx` cache from Task 3.

- [ ] **Step 2: Push to a branch and confirm the job goes green**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: run server tests against a real Postgres service"
git push -u origin <your-branch>
```
Open the Actions run for this push and confirm all four jobs pass, in particular that `server`'s `cargo test` output shows `users_table_is_reachable_after_migration ... ok` rather than the skip message — that's the signal the service container and migration step both worked.

---

## Self-review

**Spec coverage:** BRIEF.md §5.1's `users`/`devices` columns are all present in the migration (Task 2, Step 5). §3's `PostgreSQL + sqlx (compile-time checked queries)` is honored via `sqlx::query!` + offline cache (Task 3), not the runtime-checked `sqlx::query()` variant. OPS.md's Postgres 17 target is matched in `docker-compose.yml` and the CI service image. The remaining BRIEF.md §5.1 tables (`conversations`, `key_packages`, `envelopes`, `posts`, `post_reactions`) are out of scope for this plan on purpose — M2 only needs auth + identity, per the milestone table in PLAN.md.

**Placeholder scan:** no TBD/TODO markers; every code block is complete, runnable content. The two version numbers left for the executor to fill in (`sqlx`, `dotenvy` exact pins) are not placeholders in the forbidden sense — they're produced by running `cargo add`, a concrete, correct action, not a guess.

**Type consistency:** `AppState { db: PgPool }` (Task 3, Step 1) matches every later use — `main.rs`'s `AppState { db: pool }`, `db.rs`'s `create_pool() -> Result<PgPool, sqlx::Error>`, and `state::test_state()`'s return type. `router(state: AppState) -> Router` is the one signature change from the current codebase; every caller in this plan (production `main()` and both test functions) uses it consistently.

---

**Plan complete and saved to `docs/superpowers/plans/2026-08-25-m2-local-postgres-infra.md`.**
