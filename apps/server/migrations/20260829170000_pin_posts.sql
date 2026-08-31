-- Pinned posts on a profile.
--
-- A timestamp rather than a boolean, so the three a person may pin have an
-- order of their own and the newest pin sits first. A boolean would leave the
-- order to whatever the id happened to be, which is the order the profile
-- already shows underneath.
ALTER TABLE posts ADD COLUMN pinned_at TIMESTAMPTZ;

-- The profile asks for "this author's pinned posts" on every first page, and
-- the limit of three is checked against the same set. Partial, because the
-- overwhelming majority of rows are not pinned and have no business in the
-- index.
CREATE INDEX posts_pinned_idx ON posts (author_id, pinned_at DESC)
    WHERE pinned_at IS NOT NULL AND deleted_at IS NULL;
