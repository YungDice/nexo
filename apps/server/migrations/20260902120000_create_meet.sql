-- Meet&Greet: a pin, a character, and one message to a stranger.
--
-- Everything in the three tables below is readable by the server and by every
-- signed-in person, and that is the design rather than a compromise. The
-- agreement screen says so in those words, because rule 5 makes a feature that
-- implies privacy it does not have worse than one that never claimed it.
--
-- What this schema deliberately cannot express:
--
--   * a measured location. There is no accuracy column, no heading, no speed,
--     no "seen at". Nexo never reads device location, and a schema with
--     nowhere to put a measurement cannot quietly grow one later because a
--     future handler found it convenient.
--   * a precise one. `lat` and `lon` are written already snapped to a grid and
--     jittered, so the figure the client submitted is never stored anywhere.
--     A column that never holds the true value cannot leak it.
--   * a second message before the first is answered. That is the UNIQUE
--     constraint below, not a check in a handler -- see `meet_requests`.
--
-- Blocking and reporting add nothing here. `blocks` and `reports` already
-- exist and already work in both directions; a second mechanism beside them
-- would be a second thing to keep correct.

CREATE TABLE meet_profiles (
    user_id     BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    -- Already coarsened when it is written. See `meet::coarsen`, which owns
    -- the grid and the jitter and explains why the jitter is derived from the
    -- user id rather than rolled fresh.
    lat         DOUBLE PRECISION NOT NULL CHECK (lat BETWEEN -85 AND 85),
    lon         DOUBLE PRECISION NOT NULL CHECK (lon BETWEEN -180 AND 180),
    headline    TEXT CHECK (length(headline) <= 80),
    -- Opaque to the server on purpose: it does not know what a hairstyle is and
    -- must not learn. The ceiling is the only rule it enforces, and it is here
    -- rather than in a handler so that no future writer can forget it.
    char_config JSONB NOT NULL CHECK (pg_column_size(char_config) <= 2048),
    -- Leaving the map is one flag, not a delete: the character somebody spent
    -- ten minutes on survives being off the map, and coming back is one tap.
    active      BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- The map query reads active pins newest-first and nothing else.
CREATE INDEX meet_profiles_active_idx ON meet_profiles (updated_at DESC)
    WHERE active;

-- One intro per direction, ever.
--
-- The UNIQUE constraint is the rule, not a convenience. A handler check would
-- be a read followed by a write with a gap in between, and a client that
-- retries -- or one that is trying it on -- gets two conversations through that
-- gap. Here the second attempt cannot be written at all, whoever sends it and
-- however fast.
--
-- `conversation_id` points at a real conversation opened through the ordinary
-- delivery path. There is no second, lesser kind of message in Nexo: an intro
-- is an MLS group like any other, and what makes it an intro is this row.
CREATE TABLE meet_requests (
    id              BIGSERIAL PRIMARY KEY,
    from_id         BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    to_id           BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    conversation_id UUID   NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    state           TEXT   NOT NULL CHECK (state IN ('pending', 'accepted', 'declined')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at     TIMESTAMPTZ,
    CHECK (from_id <> to_id),
    UNIQUE (from_id, to_id)
);

-- The inbox: what is waiting for me, newest first.
CREATE INDEX meet_requests_inbox_idx ON meet_requests (to_id, created_at DESC)
    WHERE state = 'pending';

-- The delivery service asks this on every send into a two-member conversation,
-- to decide whether the one-message cap is still in force.
CREATE INDEX meet_requests_pending_conversation_idx
    ON meet_requests (conversation_id, from_id)
    WHERE state = 'pending';

-- Consent, versioned.
--
-- The version is the point. Consent is to particular words, and changing what
-- the agreement says without asking again would be inheriting agreement to
-- something nobody read. A bumped version re-asks; an unchanged one does not.
CREATE TABLE meet_consent (
    user_id     BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    version     INT NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
