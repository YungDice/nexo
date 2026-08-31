-- The feed and the public profile (BRIEF.md §4.4, §5.2, §6.2, §6.3).
--
-- Everything in this file is **server-readable by design**. That is not a
-- weakness to be apologised for, it is what a feed is: content meant to be read
-- by strangers cannot be encrypted to a closed group. §4.4 requires the UI to
-- say so in plain language, and it does -- see the FeedNotice component.
--
-- The line between this file and the M4 migration is the line the whole product
-- is organised around. `envelopes` holds ciphertext the server cannot read;
-- `posts` holds text the server can. Nothing crosses.

-- Links on a profile. A separate table rather than a JSON column because they
-- are ordered, individually visible-or-not (G2), and individually validated.
CREATE TABLE profile_links (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label TEXT NOT NULL CHECK (length(label) BETWEEN 1 AND 40),
    -- Scheme is checked here as well as in the handler. A `javascript:` URL
    -- reaching a profile page is a stored XSS with a very long tail, and a
    -- constraint outlives whichever handler forgets.
    url TEXT NOT NULL CHECK (url ~ '^https?://' AND length(url) <= 200),
    position INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX profile_links_user_idx ON profile_links (user_id, position);

-- G2: per-field profile visibility.
--
-- One row per (user, field) rather than a column per field, so adding a field
-- later is an INSERT and not a migration -- and so the *absence* of a row can
-- mean "the default for this field", which differs by field: a display name is
-- necessarily public because it is how you are addressed, a location is not.
--
-- The values are deliberately few. "Friends" does not exist in v0.1 because
-- there is no friendship model to hang it on, and offering a control that
-- silently means something else is worse than not offering it.
CREATE TABLE profile_visibility (
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK (field IN ('bio', 'location', 'links', 'join_date')),
    visibility TEXT NOT NULL CHECK (visibility IN ('public', 'contacts', 'private')),
    PRIMARY KEY (user_id, field)
);

-- Feed posts. Public to any logged-in user (PLAN.md's recorded default for the
-- brief's open question §3).
CREATE TABLE posts (
    id BIGSERIAL PRIMARY KEY,
    author_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 2000 chars (§6.2). Enforced here so a client that skips the check cannot
    -- write a row no client can render.
    body TEXT NOT NULL CHECK (length(body) <= 2000),
    -- Up to 4 images (§6.2), as object keys in `nexo-media`. An array rather
    -- than a child table: they are ordered, always read together, never queried
    -- individually, and capped at four.
    media_keys TEXT[] NOT NULL DEFAULT '{}' CHECK (cardinality(media_keys) <= 4),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Soft delete. A hard DELETE would cascade reactions away and make the
    -- cursor jump; this keeps the feed's paging stable while making the row
    -- invisible. The body is blanked on delete, so "deleted" means the text is
    -- actually gone rather than merely hidden.
    deleted_at TIMESTAMPTZ
);

-- The feed is reverse-chronological and cursor-paginated (§6.2), so the index
-- has to match that order exactly or every page is a sort.
CREATE INDEX posts_feed_idx ON posts (id DESC) WHERE deleted_at IS NULL;
CREATE INDEX posts_author_idx ON posts (author_id, id DESC) WHERE deleted_at IS NULL;

-- One reaction per emoji per person per post. The primary key is the rule:
-- reacting twice with the same emoji is idempotent rather than a second count.
CREATE TABLE post_reactions (
    post_id BIGINT NOT NULL REFERENCES posts(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Short enough to exclude a pasted essay, long enough for a ZWJ sequence
    -- like a multi-person family emoji.
    emoji TEXT NOT NULL CHECK (length(emoji) BETWEEN 1 AND 16),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, user_id, emoji)
);

CREATE INDEX post_reactions_post_idx ON post_reactions (post_id);
