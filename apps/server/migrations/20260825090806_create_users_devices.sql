-- The users/devices portion of the schema in docs/BRIEF.md 5.1. The remaining
-- tables (conversations, key_packages, envelopes, posts, post_reactions) belong
-- to later milestones; M2 needs auth and identity only.

-- Case-insensitive text, for handles. Discovery is by handle only -- no phone
-- number is collected anywhere (docs/PLAN.md) -- so two handles differing only
-- in case must be the same handle rather than two accounts, and that rule
-- belongs in the column type rather than in every query that touches it.
--
-- citext is a trusted extension from Postgres 13 onward, so the database owner
-- can create it without superuser. That is why docs/OPS.md Phase 4 has the
-- `nexo` role own the `nexo` database.
CREATE EXTENSION IF NOT EXISTS citext;

CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    -- The CHECK enforces BRIEF.md 4.1's handle format (3-20 chars, [a-z0-9_])
    -- server-side, not only in the client.
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

-- The MLS group member is the *device*, not the user (docs/PLAN.md), so a
-- second device later is an added member rather than a schema migration --
-- even though v0.1 allows exactly one per account.
CREATE TABLE devices (
    -- gen_random_uuid() is core in Postgres 13+; no extension needed.
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id BIGINT NOT NULL REFERENCES users(id),
    identity_pubkey BYTEA UNIQUE NOT NULL,
    name TEXT,
    last_seen TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
