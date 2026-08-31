-- Titles, post kinds, voting and comments.
--
-- Same line as the migration that created `posts`: everything here is
-- **server-readable by design**, because a feed is content meant to be read by
-- strangers. Nothing in this file goes near `envelopes`.

-- A title, and what kind of post this is.
--
-- Nullable rather than NOT NULL DEFAULT '': existing rows genuinely have no
-- title, and inventing an empty one would make "untitled" and "titled with
-- nothing" the same state. The feed renders the body alone when it is null.
ALTER TABLE posts ADD COLUMN title TEXT
    CHECK (title IS NULL OR length(title) BETWEEN 1 AND 300);

-- 'text', 'link' or 'image'. Existing rows are text posts, which is what they
-- are: a body and optional media.
ALTER TABLE posts ADD COLUMN kind TEXT NOT NULL DEFAULT 'text'
    CHECK (kind IN ('text', 'link', 'image'));

-- The destination of a link post. Same scheme constraint as profile_links, for
-- the same reason: a `javascript:` URL that reaches a feed is stored XSS, and a
-- constraint outlives whichever handler forgets to check.
ALTER TABLE posts ADD COLUMN link_url TEXT
    CHECK (link_url IS NULL OR (link_url ~ '^https?://' AND length(link_url) <= 2000));

-- A link post points somewhere; the others do not.
ALTER TABLE posts ADD CONSTRAINT posts_link_has_url
    CHECK ((kind = 'link') = (link_url IS NOT NULL));

-- One vote per person per post, and it is +1 or -1.
--
-- The primary key is the rule: voting twice replaces rather than accumulates,
-- so a score cannot be inflated by clicking. Removing a vote is a DELETE, not
-- a third value -- "no vote" is the absence of a row, which keeps the score a
-- plain SUM instead of a CASE.
CREATE TABLE post_votes (
    post_id BIGINT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    value SMALLINT NOT NULL CHECK (value IN (-1, 1)),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, user_id)
);

CREATE INDEX post_votes_post_idx ON post_votes (post_id);

-- Comments, nested to any depth.
--
-- `parent_id` references this same table, so a reply is a comment like any
-- other. ON DELETE CASCADE on the parent would take a whole subtree with it,
-- which is why deletion is soft here as it is for posts: a deleted comment
-- keeps its row so its replies keep their place in the thread.
CREATE TABLE post_comments (
    id BIGSERIAL PRIMARY KEY,
    post_id BIGINT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    parent_id BIGINT REFERENCES post_comments(id) ON DELETE CASCADE,
    author_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body TEXT NOT NULL CHECK (length(body) BETWEEN 1 AND 2000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Soft, so replies survive their parent. The body is blanked on delete, so
    -- "deleted" means the text is gone rather than merely hidden.
    deleted_at TIMESTAMPTZ
);

-- One thread is read whole and ordered by id, so the index matches that.
CREATE INDEX post_comments_post_idx ON post_comments (post_id, id);
CREATE INDEX post_comments_parent_idx ON post_comments (parent_id);
