-- The rest of the schema in docs/BRIEF.md 5.1: the Delivery Service tables.
--
-- The rule these are shaped by is rule 4: the server stores and forwards
-- ciphertext it cannot read. Every column here is routing or bookkeeping.
-- There is no subject, no preview, no attachment filename and no MIME type --
-- attachment metadata lives *inside* the ciphertext (4.2).

-- Published KeyPackages. Single-use: an invite consumes one.
CREATE TABLE key_packages (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    device_id   UUID NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    data        BYTEA NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Handing out an unconsumed package is the hot path: one per fetch, oldest
-- first, and it must not collide with a concurrent fetch.
CREATE INDEX key_packages_available_idx
    ON key_packages (device_id, created_at)
    WHERE consumed_at IS NULL;

CREATE TABLE conversations (
    id         UUID PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('dm', 'group')),
    -- The MLS epoch the server believes is current.
    --
    -- This is what makes commit ordering possible (PLAN.md risk 4(b)): a commit
    -- must cite the epoch it was built against, and the first one to arrive for
    -- a given epoch wins. The server cannot read the commit, but it can count.
    epoch      BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE conversation_members (
    conversation_id UUID   NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    user_id         BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT   NOT NULL DEFAULT 'member',
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (conversation_id, user_id)
);

CREATE INDEX conversation_members_user_idx ON conversation_members (user_id);

CREATE TABLE envelopes (
    id                BIGSERIAL PRIMARY KEY,
    conversation_id   UUID   NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    sender_device_id  UUID   NOT NULL REFERENCES devices(id) ON DELETE CASCADE,
    epoch             BIGINT NOT NULL,
    -- Opaque. The server has no key that opens this and never will.
    ciphertext        BYTEA  NOT NULL,
    -- Whether this envelope carries a commit. The server cannot see inside it,
    -- so the sender declares it -- and the server only trusts the flag enough
    -- to order commits, never enough to learn anything about the contents.
    is_commit         BOOLEAN NOT NULL DEFAULT FALSE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    delivered_at      TIMESTAMPTZ
);

-- Sync reads a conversation forward from a cursor.
CREATE INDEX envelopes_conversation_idx ON envelopes (conversation_id, id);

-- The 30-day purge of undelivered ciphertext (4.3) sweeps on this.
CREATE INDEX envelopes_undelivered_idx
    ON envelopes (created_at)
    WHERE delivered_at IS NULL;
