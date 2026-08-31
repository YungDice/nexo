-- Retiring the device a login replaces (RESEARCH-COMPARISON S1).
--
-- `login` has always said, in a comment, that "the device row is replaced
-- rather than added to". It was not. The upsert conflicts on
-- `identity_pubkey`, so signing in from a machine with no local store -- which
-- generates a *fresh* identity keypair -- inserted a second row and left the
-- first one live.
--
-- The consequence was not cosmetic. `claim_key_package` picks the oldest
-- unconsumed KeyPackage across every device belonging to a handle, so after a
-- reinstall a peer's Welcome went to a device that no longer exists. The
-- conversation could not be joined and nothing said why: the sender saw a
-- successful claim, the recipient saw nothing at all.
--
-- This is the other half of key-change detection. Detection tells you the key
-- changed; without retirement the account is simply unreachable afterwards,
-- which is a worse answer than the warning.
ALTER TABLE devices ADD COLUMN retired_at TIMESTAMPTZ;

-- Every claim already filters on `consumed_at IS NULL`; this index is for the
-- join that now also filters retired devices out.
CREATE INDEX devices_live_idx ON devices (user_id) WHERE retired_at IS NULL;

-- Existing rows are all live. A backfill that retired anything here would
-- strand accounts that are working today.
