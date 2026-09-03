-- Stories: an encrypted object that stops being available after 24 hours.
--
-- What is here is bookkeeping, not content. The object itself lives in the
-- **encrypted** bucket, not the media one, and that distinction is kept by the
-- type system in `storage.rs` rather than by anyone remembering it: a story is
-- opaque ciphertext, and the key to it travels inside MLS messages that this
-- server cannot read.
--
-- # The 24 hours come from three places, and none of them is a scheduled job
--
-- There is no background work anywhere in this server, and adding the first
-- one would drag in the leader-election question `docs/PLAN.md` deliberately
-- parked behind Redis. So expiry is enforced where somebody is already asking:
--
--   1. **At the reader.** Every query filters on `expires_at`, and the client
--      deletes what has expired as it reads — including, and mainly, its copy
--      of the key. The bytes are worthless without it, which makes this the
--      layer that actually makes a story disappear. The rate limiter in
--      `limits.rs` tidies up the same way, incidentally rather than on a timer.
--   2. **At this server.** A download URL is refused for an expired story. That
--      turns 24 hours from a courtesy into a property: past it, nobody gets the
--      bytes, with whatever client they like.
--   3. **At the object store**, as a lifecycle rule on the story prefix, for
--      the case where this server does nothing at all for a week. See
--      `docs/OPS.md`.
--
-- The row is kept until layer 1 or 3 removes the object; a story that has
-- expired simply stops being served and stops being listed.
CREATE TABLE stories (
    id         BIGSERIAL PRIMARY KEY,
    author_id  BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- In the encrypted bucket. Opaque here.
    s3_key     TEXT   NOT NULL UNIQUE,
    -- Ciphertext length, for the quota conversation this will eventually need.
    size       BIGINT NOT NULL CHECK (size > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- At most 24 hours out, enforced here as well as in the handler so no
    -- future writer can quietly mint one that outlives the promise.
    expires_at TIMESTAMPTZ NOT NULL,
    CHECK (expires_at > created_at),
    CHECK (expires_at <= created_at + INTERVAL '24 hours')
);

-- Listing is always "whose, and not expired".
CREATE INDEX stories_author_live_idx ON stories (author_id, expires_at DESC);

-- The download route looks a story up by its key.
CREATE INDEX stories_key_idx ON stories (s3_key);
