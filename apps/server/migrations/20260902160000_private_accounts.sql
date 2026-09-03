-- Private accounts, and the invite IDs that get past one.
--
-- This reverses a decision `docs/PLAN.md` recorded as taken: *"Discovery is by
-- handle only"*. The reversal and its reason live in that file's
-- *Changes to the brief*; this migration is only the shape of it.
--
-- The word "private" has to be worth something, so the enforcement is
-- server-side in both places it can be evaded:
--
--   * a private account does not appear in search;
--   * a private account cannot be written to without a valid invite, checked
--     where the block rule and the one-message rule already sit
--     (`delivery/mod.rs`).
--
-- `profiles.rs` refused a per-field visibility switch for handle and display
-- name on the grounds that "offering a switch that cannot honestly be honoured
-- is worse than offering none". This is the same standard, met rather than
-- avoided: the switch exists because it can be kept.
--
-- Existing accounts start public. That is the behaviour they already have, and
-- silently making everyone private would break every conversation anybody was
-- about to start.
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_private BOOLEAN NOT NULL DEFAULT FALSE;

-- Search only ever looks at public accounts, so the index says so too.
CREATE INDEX IF NOT EXISTS users_public_handle_idx ON users (handle)
    WHERE NOT is_private;

-- Invite IDs.
--
-- The secret is **not** stored. What is stored is its SHA-256, for the same
-- reason a password is not kept: a leaked table should not hand out working
-- invitations. Lookup is by hash, which is exact, so nothing is lost.
--
-- Expiry is checked in the query (`expires_at > now()`), never by a cleanup
-- job. There is no scheduled work anywhere in this server, and an invitation
-- that only stops working once a sweeper runs is one that still works.
--
-- A revoked or spent invite is kept rather than deleted: `meet_requests` points
-- at the one it came through, and the answer to "how did this person reach me"
-- should survive the invite being withdrawn.
CREATE TABLE meet_invites (
    id          BIGSERIAL PRIMARY KEY,
    owner_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- SHA-256 of the secret, hex. Never the secret itself.
    secret_hash TEXT   NOT NULL UNIQUE,
    -- A name the owner gave it, so a list of them is readable.
    label       TEXT,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- At most seven days out. Enforced here as well as in the handler, so no
    -- future writer can quietly mint an invitation that never expires.
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ,
    CHECK (expires_at > created_at),
    CHECK (expires_at <= created_at + INTERVAL '7 days')
);

CREATE INDEX meet_invites_owner_idx ON meet_invites (owner_id, created_at DESC);

-- Which invitation a request came through.
--
-- Nullable: requests made before invitations existed came through no
-- particular one, and so do requests to a public account.
ALTER TABLE meet_requests ADD COLUMN IF NOT EXISTS invite_id BIGINT
    REFERENCES meet_invites(id) ON DELETE SET NULL;
