-- Idempotent sends, for the offline queue (M8).
--
-- The failure this exists for: a client sends, the server writes the envelope,
-- and the response is lost on the way back. The client cannot tell that from a
-- request that never arrived, so it retries — and without this column the retry
-- is a second copy of the message in everyone's conversation.
--
-- Retrying is not optional. A message queued while offline has to be sent when
-- the network returns, and "send it again" is the only thing a client can do.
-- So the fix has to be on this side: the client names the message, and a second
-- request with the same name returns the first one's envelope instead of
-- writing a new row.
--
-- Why the client picks the id rather than the server: the server's id only
-- exists after the write, which is exactly the moment that can be lost.

ALTER TABLE envelopes ADD COLUMN client_msg_id UUID;

-- Unique per conversation rather than globally. Two clients can generate the
-- same UUID only by accident or malice; scoping it to the conversation means
-- such a collision cannot reach across into a conversation the sender is not
-- in, and the natural lookup is by (conversation, id) anyway.
--
-- Partial, because every envelope written before this migration has NULL here
-- and NULLs are not equal to each other in a UNIQUE index -- but being explicit
-- is better than relying on that, and it keeps the index small.
CREATE UNIQUE INDEX envelopes_client_msg_id_idx
    ON envelopes (conversation_id, client_msg_id)
    WHERE client_msg_id IS NOT NULL;
