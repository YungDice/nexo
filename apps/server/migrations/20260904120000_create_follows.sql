-- Who follows whom.
--
-- A directed edge: A following B says nothing about B following A. That is the
-- difference between a follow and a friendship, and it is why this is a pair of
-- columns rather than an unordered set.
--
-- This is a new disclosure and it is worth naming as one. Until now the server
-- knew who talks to whom (THREAT-MODEL.md 2.2, conceded). This adds a durable,
-- explicit record of who is *interested* in whom, including people the follower
-- has never messaged -- an interest graph rather than a contact graph. 2.13
-- says so in the same words.
CREATE TABLE IF NOT EXISTS follows (
    follower_id BIGINT      NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    followed_id BIGINT      NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),

    PRIMARY KEY (follower_id, followed_id),

    -- Following yourself is not a thing anybody means, and allowing it would
    -- put your own posts in a feed that exists to show other people's.
    CONSTRAINT follows_not_self CHECK (follower_id <> followed_id)
);

-- The feed asks "whom does this person follow" on every request under the
-- Following view; the primary key already covers that direction. This one
-- covers the other, which is what a follower count needs.
CREATE INDEX IF NOT EXISTS follows_followed_idx ON follows (followed_id);
