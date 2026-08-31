-- Blocking, and it has to be here rather than in the client.
--
-- A client-side block is a promise the product cannot keep: the blocked person
-- goes on sending, the server goes on accepting and storing, and all that
-- changes is whether one app draws it. Rule 5 -- say plainly what is and is not
-- protected -- makes that kind of block worse than none at all, because the
-- word means something to the person using it.
--
-- So the server enforces it, in the two places it can:
--
--   * the feed and profile queries drop posts in both directions;
--   * the delivery service refuses to open a conversation or accept an
--     envelope across a block.
--
-- What it still cannot do is stop somebody making a second account. Nothing
-- short of identity verification can, the app says so where blocking is
-- offered, and pretending otherwise is the failure this table exists to avoid.
CREATE TABLE blocks (
    blocker_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- One row per direction. Blocking is not symmetric as a *record* -- who
    -- blocked whom is worth knowing, and only the blocker may undo it -- even
    -- though its effects are applied in both directions.
    PRIMARY KEY (blocker_id, blocked_id),
    -- Blocking yourself is not a state with a meaning; it would only ever be a
    -- client bug arriving as data.
    CHECK (blocker_id <> blocked_id)
);

-- The feed asks "is either of us blocking the other" for every candidate
-- author, so the reverse direction needs an index of its own: the primary key
-- only covers lookups that start from the blocker.
CREATE INDEX blocks_blocked_idx ON blocks (blocked_id, blocker_id);
