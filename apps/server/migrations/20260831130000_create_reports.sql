-- Reporting (BRIEF 13, RESEARCH-COMPARISON W6).
--
-- The feed is a single global stream visible to every signed-in account, with
-- no follow graph to filter it. That is a settled decision, and its consequence
-- is that whatever the first stranger posts is in front of everybody. Blocking
-- answers "I do not want to see this person"; it does not answer "this should
-- not be here", and the second question needs somewhere to go before real
-- people are invited in.
--
-- Deliberately minimal: a table an operator can read. There is no moderation
-- queue, no automated action, and no reputation score, because a report that
-- silently triggers something is worse than one that visibly waits.
CREATE TABLE reports (
    id BIGSERIAL PRIMARY KEY,
    reporter_user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- What is being reported. Not a foreign key: a post can be deleted after
    -- it is reported, and losing the report with it would destroy the only
    -- record of why the deletion happened.
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('post', 'comment', 'user')),
    subject_id BIGINT NOT NULL,
    -- A short reason from a fixed list, so reports can be counted, plus free
    -- text for the part a list cannot anticipate.
    reason TEXT NOT NULL CHECK (reason IN (
        'spam', 'harassment', 'illegal', 'impersonation', 'other'
    )),
    note TEXT CHECK (note IS NULL OR length(note) <= 1000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- Set when an operator has looked. Nothing reads it yet; it exists so that
    -- "looked at" is recorded from the first report rather than reconstructed
    -- later from memory.
    reviewed_at TIMESTAMPTZ
);

-- One report per person per thing. Reporting twice is not two reports, and the
-- primary use of this table is counting distinct reporters.
CREATE UNIQUE INDEX reports_unique_idx
    ON reports (reporter_user_id, subject_kind, subject_id);

-- What an operator actually queries: everything unreviewed, newest first.
CREATE INDEX reports_pending_idx ON reports (created_at DESC) WHERE reviewed_at IS NULL;
