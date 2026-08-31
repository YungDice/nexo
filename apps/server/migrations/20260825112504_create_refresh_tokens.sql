-- Refresh tokens, per BRIEF 5.2: 30 days, rotating on every use, revoked on
-- reuse detection.
--
-- The token itself is never stored. `token_hash` is SHA-256 of the bearer
-- value, so a dump of this table does not let anyone log in as anybody -- the
-- same reason `users.pw_hash` is a hash and not a password.
--
-- `used_at` is what makes theft detectable. Rotation stamps it. A token
-- presented *after* that timestamp exists is a token that was replayed, which
-- means two parties hold it, which means one of them stole it. The response is
-- to revoke the whole family rather than to guess which is which.
CREATE TABLE refresh_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id UUID REFERENCES devices(id) ON DELETE SET NULL,
    token_hash BYTEA NOT NULL UNIQUE,
    issued_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ
);

-- Family revocation walks every live token for one user.
CREATE INDEX refresh_tokens_user_id_idx ON refresh_tokens (user_id);

-- Expired rows are dead weight; a periodic sweep uses this.
CREATE INDEX refresh_tokens_expires_at_idx ON refresh_tokens (expires_at);
