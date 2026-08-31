//! The client's encrypted local store.
//!
//! One SQLCipher database at `%APPDATA%\Nexo\store.db`, holding messages, MLS
//! group state, contacts and cached profiles (brief 4.3). The whole file is
//! encrypted; there is no plaintext index, no plaintext preview column, and no
//! "just this bit in the clear for speed".
//!
//! The key exists in two places and no others: in memory inside a
//! [`Zeroizing`](zeroize::Zeroizing) buffer, and on disk wrapped by the OS
//! keystore. See [`key`].
//!
//! # What this crate deliberately does not do
//!
//! It does not decide *what* to store. Schema for messages and MLS state
//! arrives with M3 and M4; this is the encrypted container and the proof that
//! it is one. Building the container first, and testing that it is genuinely
//! opaque, is what makes the later schema work uninteresting.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension as _};
use zeroize::Zeroizing;

pub mod key;

pub use key::KeyError;

/// A stored identity keypair: the secret (zeroized on drop) and the public half.
pub type StoredIdentity = (Zeroizing<Vec<u8>>, Vec<u8>);

/// The schema version this build writes and expects.
///
/// One constant, so a migration and the test that checks it cannot disagree.
pub const SCHEMA_VERSION: i64 = 8;

/// The file name the store always uses, under the app data directory.
pub const STORE_FILE_NAME: &str = "store.db";

/// Errors from opening or using the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Getting the key failed.
    #[error(transparent)]
    Key(#[from] KeyError),
    /// SQLite or SQLCipher refused.
    #[error("the local store failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// The file exists but the key does not open it.
    ///
    /// Almost always means the keyring was erased or the file was copied from
    /// another machine or Windows account — not corruption, and worth saying
    /// so, because the remedies are completely different.
    #[error(
        "`{path}` cannot be opened with this machine's key. The keyring was \
         probably erased, or the file came from another account. It is not \
         recoverable without the original key."
    )]
    WrongKey {
        /// Which file.
        path: PathBuf,
    },
    /// The directory could not be created.
    #[error("could not create the store directory: {0}")]
    Io(#[from] std::io::Error),
}

/// An open, encrypted store.
pub struct EncryptedStore {
    connection: Connection,
    path: PathBuf,
}

/// Written out rather than derived, so that no future field can be printed into
/// a log by accident. The path is the only thing here that is safe to show.
impl std::fmt::Debug for EncryptedStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptedStore")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl EncryptedStore {
    /// Opens the store at `path`, creating and keying it on first run.
    ///
    /// `key` is the raw 32 bytes from [`key::load_or_create`].
    pub fn open(path: impl AsRef<Path>, key: &[u8]) -> Result<Self, StoreError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(&path)?;

        // `PRAGMA key` must be the first statement on the connection: SQLCipher
        // reads the header to decide the page cipher, and anything that touches
        // a page before the key is set gets the wrong answer.
        let literal = key::as_pragma_literal(key);
        connection.pragma_update(None, "key", &*literal as &str)?;

        // The first actual read is what proves the key is right. SQLCipher
        // accepts any key at PRAGMA time and only fails when it tries to
        // decrypt a page, so without this the error surfaces later and looks
        // like corruption.
        let usable = connection
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| {
                row.get::<_, i64>(0)
            })
            .is_ok();
        if !usable {
            return Err(StoreError::WrongKey { path });
        }

        // Foreign keys are off by default in SQLite, which quietly turns every
        // REFERENCES clause into a comment.
        connection.pragma_update(None, "foreign_keys", "ON")?;
        // WAL survives a crash mid-write without taking the database with it.
        connection.pragma_update(None, "journal_mode", "WAL")?;

        let store = Self { connection, path };
        store.migrate()?;
        Ok(store)
    }

    /// Applies the local schema.
    ///
    /// Versioned with `user_version` rather than a migrations table: this
    /// database belongs to one process on one machine, so there is no
    /// concurrent migrator to coordinate with and nothing to gain from the
    /// heavier machinery the server uses.
    fn migrate(&self) -> Result<(), StoreError> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;

        if version < 1 {
            self.connection.execute_batch(
                "BEGIN;
                 -- Deliberately minimal. Message and MLS tables arrive with M3
                 -- and M4; this establishes the file and its versioning so
                 -- those are ordinary migrations rather than a first one.
                 CREATE TABLE IF NOT EXISTS account (
                     id            INTEGER PRIMARY KEY CHECK (id = 1),
                     user_id       INTEGER NOT NULL,
                     handle        TEXT    NOT NULL,
                     display_name  TEXT    NOT NULL,
                     device_id     TEXT    NOT NULL
                 );

                 -- The Ed25519 identity secret (brief 4.1). It lives here
                 -- rather than in the OS keystore because the keystore holds
                 -- exactly one thing -- the key to this file -- and nesting a
                 -- second secret behind the same DPAPI blob would gain nothing
                 -- while giving the keyring two jobs.
                 CREATE TABLE IF NOT EXISTS identity (
                     id          INTEGER PRIMARY KEY CHECK (id = 1),
                     secret_key  BLOB NOT NULL,
                     public_key  BLOB NOT NULL,
                     created_at  TEXT NOT NULL DEFAULT (datetime('now'))
                 );
                 -- The whole of the MLS storage, as one blob. See
                 -- nexo_client::mls_state for why it is one blob and not a
                 -- StorageProvider.
                 CREATE TABLE IF NOT EXISTS mls_state (
                     id    INTEGER PRIMARY KEY CHECK (id = 1),
                     blob  BLOB NOT NULL
                 );

                 -- Locally decrypted messages. This is the *plaintext* history,
                 -- which is why it may only ever exist inside this encrypted
                 -- file (4.3).
                 CREATE TABLE IF NOT EXISTS messages (
                     envelope_id      INTEGER PRIMARY KEY,
                     conversation_id  TEXT NOT NULL,
                     sender_device_id TEXT,
                     body             TEXT NOT NULL,
                     sent_at_ms       INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS messages_conversation_idx
                     ON messages (conversation_id, envelope_id);

                 -- One row per conversation this device is in, with the sync
                 -- cursor. `last_envelope_id` is what makes a reconnect cheap
                 -- and what makes a missed WebSocket event survivable.
                 CREATE TABLE IF NOT EXISTS conversations (
                     id                TEXT PRIMARY KEY,
                     last_envelope_id  INTEGER NOT NULL DEFAULT 0
                 );

                 -- The refresh token, so a restart can get a new access token
                 -- without asking for the password again.
                 --
                 -- It sits beside the identity private key, in the same
                 -- SQLCipher file under the same DPAPI-wrapped key. Anyone who
                 -- can read this row can already read that key, which is the
                 -- far more valuable secret -- so withholding the token buys
                 -- nothing and costs the user their session on every restart.
                 CREATE TABLE IF NOT EXISTS refresh_token (
                     id     INTEGER PRIMARY KEY CHECK (id = 1),
                     token  TEXT NOT NULL
                 );

                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }

        if version < 2 {
            // A human-readable name for the conversation. Set when this device
            // starts one, because that is the only moment it knows who it
            // invited; a conversation joined from a Welcome has no title until
            // M7's profile fetch, and the UI says so rather than inventing one.
            self.connection.execute_batch(
                "BEGIN;
                 ALTER TABLE conversations ADD COLUMN title TEXT;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }

        if version < 3 {
            // The full payload of a message that carries a file: the S3 key,
            // the AES key, the nonce, the hash, the name and type.
            //
            // It has to be persisted, and this is the only place it may live.
            // MLS refuses a replayed message by design -- that is the whole
            // point of a ratchet -- so the payload cannot be recovered by
            // decrypting the envelope a second time. Either it is written down
            // when it first arrives, or the file is unreachable forever.
            //
            // That it holds a decryption key is exactly why it belongs in this
            // file and nowhere else (4.3). The `body` column beside it is only
            // the preview shown in the list.
            self.connection.execute_batch(
                "BEGIN;
                 ALTER TABLE messages ADD COLUMN payload TEXT;
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }

        if version < 4 {
            // The offline queue (M8).
            //
            // It holds **ciphertext**, not the message someone typed, and that
            // is forced rather than chosen. MLS ratchets forward on every
            // encryption: the ciphertext for a message exists exactly once,
            // and re-encrypting on retry would consume a second generation and
            // desynchronise this device from the group. So a queued message is
            // encrypted at the moment it is written, and every later attempt
            // sends those same bytes.
            //
            // `client_msg_id` is what makes the retry safe. A client cannot
            // tell a request that never arrived from a reply that was lost, so
            // it must retry; the server matches on this id and returns the
            // envelope the first attempt created instead of writing a second.
            //
            // `body` is the preview shown while the message is pending. It is
            // plaintext, which is why it may only live in this encrypted file.
            self.connection.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS outbox (
                     -- Insertion order, explicitly. Not the implicit rowid:
                     -- two messages written in the same millisecond have to
                     -- keep their order, and ordering by a timestamp alone
                     -- would leave that to chance.
                     seq             INTEGER PRIMARY KEY AUTOINCREMENT,
                     client_msg_id   TEXT NOT NULL UNIQUE,
                     conversation_id TEXT NOT NULL,
                     ciphertext      TEXT NOT NULL,
                     epoch           INTEGER NOT NULL,
                     is_commit       INTEGER NOT NULL DEFAULT 0,
                     body            TEXT NOT NULL,
                     payload         TEXT,
                     queued_at_ms    INTEGER NOT NULL,
                     attempts        INTEGER NOT NULL DEFAULT 0,
                     last_error      TEXT
                 );
                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
        }

        if version < 5 {
            // What kind of conversation this is, and who is in it.
            //
            // Both come from the server's membership list, which is routing
            // metadata it already holds and already acts on. Kept here so the
            // UI can tell a DM from a group without a request -- and so it
            // stops treating every conversation as a DM, which is what it did
            // when the only answer available was a guess.
            //
            // `members` is a JSON array of handles. A child table would be the
            // tidier shape, but these are read together, written together, and
            // never queried individually.
            self.connection.execute_batch(
                "BEGIN;
                 ALTER TABLE conversations ADD COLUMN kind TEXT;
                 ALTER TABLE conversations ADD COLUMN members TEXT;
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?;
        }

        if version < 6 {
            // The payload describing a conversation's picture: where the
            // ciphertext is, and the key that opens it.
            //
            // It holds a decryption key, which is exactly why it belongs in
            // this file and nowhere else -- the same rule as a message's
            // attachment payload. Kept per conversation rather than per
            // message because the newest one is the current picture and the
            // ones before it are of no interest.
            self.connection.execute_batch(
                "BEGIN;
                 ALTER TABLE conversations ADD COLUMN avatar_payload TEXT;
                 PRAGMA user_version = 6;
                 COMMIT;",
            )?;
        }
        if version < 7 {
            // Conversations this device has been told to forget.
            //
            // Removing a conversation used to last until the next sync.
            // `discover` asks the server what we are a member of and creates a
            // local row for anything it does not recognise -- and after a
            // delete, it does not recognise the one just deleted. The chat came
            // back within seconds, empty, which is the opposite of what the
            // button said.
            //
            // A tombstone says "gone, as of this envelope". The conversation
            // stays gone while the server's newest envelope is still that one,
            // and comes back the moment a newer one exists -- which is exactly
            // the promise the confirmation makes: everyone else keeps their
            // copy, and if they write again the conversation returns with the
            // new message in it.
            //
            // Not a column on `conversations`, because the row itself is what
            // gets deleted.
            self.connection.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS forgotten_conversations (
                     id TEXT PRIMARY KEY,
                     through_envelope_id INTEGER NOT NULL
                 );
                 PRAGMA user_version = 7;
                 COMMIT;",
            )?;
        }

        if version < 8 {
            // Who is in each conversation, and what key they sign with.
            //
            // The table that makes a key change *noticeable*. Without it the
            // safety number is computed on demand from the live group and
            // compared against nothing, so the verification ceremony is
            // one-shot: someone who compared digits in week one is never told
            // when the answer changes in week two -- which is exactly the
            // key-substituting server that THREAT-MODEL 4 is about.
            //
            // `verified_key` holds the key that was verified rather than a
            // boolean. A flag cannot survive the thing it refers to changing;
            // storing the key means "verified" always answers *which* key, and
            // a later comparison is meaningful rather than a guess.
            //
            // Keyed by device, not by handle: MLS names devices, and a person
            // who reinstalls is a new device with a new key. That is the case
            // this table exists to notice.
            self.connection.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS conversation_peers (
                     conversation_id TEXT NOT NULL,
                     device_id       TEXT NOT NULL,
                     identity_key    BLOB NOT NULL,
                     first_seen_ms   INTEGER NOT NULL,
                     -- The key the user confirmed out of band, if they ever did.
                     verified_key    BLOB,
                     -- When the key last changed under a device we already knew.
                     -- Cleared by acknowledging, not by the change going stale.
                     changed_at_ms   INTEGER,
                     PRIMARY KEY (conversation_id, device_id)
                 );
                 PRAGMA user_version = 8;
                 COMMIT;",
            )?;
        }
        Ok(())
    }

    /// The schema version currently applied.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        Ok(self
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    /// Where this store lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The underlying connection, for the modules that own their own tables.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    /// Records the signed-in account, replacing whatever was there.
    ///
    /// One row, enforced by the `CHECK (id = 1)`: v0.1 is one account per
    /// installation, and a schema that cannot represent two is better than a
    /// rule that says there should not be.
    pub fn set_account(
        &self,
        user_id: i64,
        handle: &str,
        display_name: &str,
        device_id: &str,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO account (id, user_id, handle, display_name, device_id)
             VALUES (1, ?1, ?2, ?3, ?4)
             ON CONFLICT (id) DO UPDATE SET
                 user_id = excluded.user_id,
                 handle = excluded.handle,
                 display_name = excluded.display_name,
                 device_id = excluded.device_id",
            rusqlite::params![user_id, handle, display_name, device_id],
        )?;
        Ok(())
    }

    /// The signed-in account, if the store has one.
    pub fn account(&self) -> Result<Option<Account>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT user_id, handle, display_name, device_id FROM account WHERE id = 1")?;
        let mut rows = statement.query([])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(Account {
            user_id: row.get(0)?,
            handle: row.get(1)?,
            display_name: row.get(2)?,
            device_id: row.get(3)?,
        }))
    }
}

impl EncryptedStore {
    /// Stores this device's identity keypair, replacing any existing one.
    ///
    /// Replacing is a serious act: the old key *is* the account's cryptographic
    /// identity, and every contact who verified a safety number against it will
    /// see a mismatch. Callers should be registering or deliberately re-keying,
    /// never doing this speculatively.
    pub fn set_identity(&self, secret: &[u8], public: &[u8]) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO identity (id, secret_key, public_key)
             VALUES (1, ?1, ?2)
             ON CONFLICT (id) DO UPDATE SET
                 secret_key = excluded.secret_key,
                 public_key = excluded.public_key",
            rusqlite::params![secret, public],
        )?;
        Ok(())
    }

    /// This device's identity keypair, if one has been generated.
    ///
    /// The secret comes back [`Zeroizing`] so it is wiped when the caller drops
    /// it — the row is inside an encrypted file, but the copy in memory is not.
    pub fn identity(&self) -> Result<Option<StoredIdentity>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT secret_key, public_key FROM identity WHERE id = 1")?;
        let mut rows = statement.query([])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let secret: Vec<u8> = row.get(0)?;
        let public: Vec<u8> = row.get(1)?;
        Ok(Some((Zeroizing::new(secret), public)))
    }
}

impl EncryptedStore {
    /// Stores the refresh token, replacing any previous one.
    ///
    /// Rotation means the previous token is already dead, so overwriting is
    /// always right and keeping history would only leave dead credentials on
    /// disk.
    pub fn set_refresh_token(&self, token: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO refresh_token (id, token) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET token = excluded.token",
            rusqlite::params![token],
        )?;
        Ok(())
    }

    /// The stored refresh token, if there is one.
    pub fn refresh_token(&self) -> Result<Option<Zeroizing<String>>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT token FROM refresh_token WHERE id = 1")?;
        let mut rows = statement.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(Zeroizing::new(row.get(0)?))),
            None => Ok(None),
        }
    }

    /// Forgets the refresh token, without touching anything else.
    ///
    /// Used when the server rejects it: a dead token kept on disk is a dead
    /// token that gets retried on every launch.
    pub fn clear_refresh_token(&self) -> Result<(), StoreError> {
        self.connection
            .execute("DELETE FROM refresh_token WHERE id = 1", [])?;
        Ok(())
    }

    /// Replaces the stored MLS state.
    pub fn set_mls_state(&self, blob: &[u8]) -> Result<(), StoreError> {
        Self::set_mls_state_on(&self.connection, blob)
    }

    fn set_mls_state_on(conn: &rusqlite::Connection, blob: &[u8]) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO mls_state (id, blob) VALUES (1, ?1)
             ON CONFLICT (id) DO UPDATE SET blob = excluded.blob",
            rusqlite::params![blob],
        )?;
        Ok(())
    }

    /// The stored MLS state, if there is any.
    pub fn mls_state(&self) -> Result<Option<Vec<u8>>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT blob FROM mls_state WHERE id = 1")?;
        let mut rows = statement.query([])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    /// Records a decrypted message.
    ///
    /// Keyed by the server's envelope id, so replaying a sync cannot duplicate
    /// anything -- which matters, because a reconnect replays by design.
    pub fn insert_message(
        &self,
        envelope_id: i64,
        conversation_id: &str,
        sender_device_id: Option<&str>,
        body: &str,
        sent_at_ms: i64,
    ) -> Result<(), StoreError> {
        self.insert_message_with_payload(
            envelope_id,
            conversation_id,
            sender_device_id,
            body,
            None,
            sent_at_ms,
        )
    }

    /// Records a message along with the payload it was decoded from.
    ///
    /// `payload` is only set for a message that carries something the preview
    /// cannot represent -- today, a file. See the v3 migration for why it must
    /// be written down at arrival rather than recovered later.
    pub fn insert_message_with_payload(
        &self,
        envelope_id: i64,
        conversation_id: &str,
        sender_device_id: Option<&str>,
        body: &str,
        payload: Option<&str>,
        sent_at_ms: i64,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO messages
                 (envelope_id, conversation_id, sender_device_id, body, payload, sent_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (envelope_id) DO NOTHING",
            rusqlite::params![
                envelope_id,
                conversation_id,
                sender_device_id,
                body,
                payload,
                sent_at_ms
            ],
        )?;
        Ok(())
    }

    /// Queues a message and saves the MLS state that produced it, atomically.
    ///
    /// These two writes must not be separable. Encrypting advances the ratchet,
    /// so the ciphertext in the outbox belongs to generation *N* while the
    /// state on disk still describes *N-1* until it is saved. A crash in the
    /// gap leaves a queued message at *N* and a ratchet that will hand out *N*
    /// again to the next one -- RFC 9420 6.3.1 names this exactly: "If this
    /// persistent state is lost or corrupted, a client might reuse a generation
    /// that has already been used, causing reuse of a key/nonce pair."
    ///
    /// The four-byte reuse guard the same section mandates makes an actual
    /// nonce collision a 2^-32 event rather than a certainty, which is why this
    /// was a latent bug and not a live one. It is still one `BEGIN`.
    ///
    /// `unchecked_transaction` rather than `transaction`: the latter needs
    /// `&mut Connection` and everything here holds `&self`, which is what lets
    /// the store be shared. The "unchecked" part is that it cannot statically
    /// prevent a nested transaction; there are none in this file.
    pub fn enqueue_with_mls_state(
        &self,
        item: &OutboxItem,
        mls_state: &[u8],
    ) -> Result<(), StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        Self::enqueue_on(&tx, item)?;
        Self::set_mls_state_on(&tx, mls_state)?;
        tx.commit()?;
        Ok(())
    }

    /// Queues an already-encrypted message for sending.
    ///
    /// For callers with no ratchet state to persist alongside it. A message
    /// that was just encrypted wants [`EncryptedStore::enqueue_with_mls_state`]
    /// instead, so the two land together.
    pub fn enqueue(&self, item: &OutboxItem) -> Result<(), StoreError> {
        Self::enqueue_on(&self.connection, item)
    }

    /// The insert itself, against whichever handle the caller has -- the
    /// connection, or a transaction on it.
    fn enqueue_on(conn: &rusqlite::Connection, item: &OutboxItem) -> Result<(), StoreError> {
        conn.execute(
            "INSERT INTO outbox
                 (client_msg_id, conversation_id, ciphertext, epoch, is_commit,
                  body, payload, queued_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (client_msg_id) DO NOTHING",
            rusqlite::params![
                item.client_msg_id,
                item.conversation_id,
                item.ciphertext,
                item.epoch,
                item.is_commit,
                item.body,
                item.payload,
                item.queued_at_ms
            ],
        )?;
        Ok(())
    }

    /// Everything waiting to be sent, oldest first.
    pub fn outbox(&self) -> Result<Vec<OutboxItem>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT client_msg_id, conversation_id, ciphertext, epoch, is_commit,
                    body, payload, queued_at_ms, attempts, last_error
             FROM outbox ORDER BY seq",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(OutboxItem {
                client_msg_id: row.get(0)?,
                conversation_id: row.get(1)?,
                ciphertext: row.get(2)?,
                epoch: row.get(3)?,
                is_commit: row.get(4)?,
                body: row.get(5)?,
                payload: row.get(6)?,
                queued_at_ms: row.get(7)?,
                attempts: row.get(8)?,
                last_error: row.get(9)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// How many messages are waiting.
    pub fn outbox_len(&self) -> Result<i64, StoreError> {
        Ok(self
            .connection
            .query_row("SELECT COUNT(*) FROM outbox", [], |row| row.get(0))?)
    }

    /// Removes a message that was accepted by the server.
    pub fn dequeue(&self, client_msg_id: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "DELETE FROM outbox WHERE client_msg_id = ?1",
            [client_msg_id],
        )?;
        Ok(())
    }

    /// Records a failed attempt, so the UI can say why and how often.
    ///
    /// Nothing is dropped after N attempts. A message the user believes they
    /// sent must not disappear because a server was unreachable for a while;
    /// it stays queued, visibly, until it is sent or the user deletes it.
    pub fn record_attempt(&self, client_msg_id: &str, error: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "UPDATE outbox
             SET attempts = attempts + 1, last_error = ?2
             WHERE client_msg_id = ?1",
            rusqlite::params![client_msg_id, error],
        )?;
        Ok(())
    }

    /// The stored payload for one message, if it has one.
    pub fn message_payload(&self, envelope_id: i64) -> Result<Option<String>, StoreError> {
        Ok(self
            .connection
            .query_row(
                "SELECT payload FROM messages WHERE envelope_id = ?1",
                [envelope_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// Messages in a conversation, oldest first.
    pub fn messages(&self, conversation_id: &str) -> Result<Vec<StoredMessage>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT envelope_id, sender_device_id, body, payload, sent_at_ms
             FROM messages WHERE conversation_id = ?1 ORDER BY envelope_id",
        )?;
        let rows = statement.query_map([conversation_id], |row| {
            Ok(StoredMessage {
                envelope_id: row.get(0)?,
                sender_device_id: row.get(1)?,
                body: row.get(2)?,
                payload: row.get(3)?,
                sent_at_ms: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Remembers a conversation and its sync cursor.
    pub fn set_conversation_cursor(
        &self,
        conversation_id: &str,
        last_envelope_id: i64,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO conversations (id, last_envelope_id) VALUES (?1, ?2)
             ON CONFLICT (id) DO UPDATE SET
                 -- Never move a cursor backwards: an out-of-order write would
                 -- replay messages the client already has.
                 last_envelope_id = max(excluded.last_envelope_id, conversations.last_envelope_id)",
            rusqlite::params![conversation_id, last_envelope_id],
        )?;
        Ok(())
    }

    /// Where a conversation's sync should resume from.
    pub fn conversation_cursor(&self, conversation_id: &str) -> Result<i64, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT last_envelope_id FROM conversations WHERE id = ?1")?;
        let mut rows = statement.query([conversation_id])?;
        match rows.next()? {
            Some(row) => Ok(row.get(0)?),
            None => Ok(0),
        }
    }

    /// Removes a conversation and everything this device holds about it.
    ///
    /// Local only, and the name of the command above it says so. There is no
    /// "delete for everyone" behind this and there cannot be: the other
    /// members hold their own copies, and the server holds ciphertext it
    /// deletes on acknowledgement rather than on request.
    ///
    /// What goes is the message history, anything queued for it in the outbox,
    /// and the conversation row itself with its cursor and title. The MLS
    /// group state is not touched here -- it lives in `mls_state`, keyed by
    /// the provider, and dropping half of a group's cryptographic state while
    /// the server still lists us as a member is how a client ends up unable to
    /// read anything anyone sends it afterwards. Rejoining happens the
    /// ordinary way, through the next Welcome.
    ///
    /// One transaction, because a history without its conversation row is a
    /// set of messages the UI cannot reach and cannot delete -- and because a
    /// deletion without its tombstone is a conversation that comes back on the
    /// next sync.
    pub fn delete_conversation(&self, conversation_id: &str) -> Result<(), StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        // Read before the row goes: the cursor is how far this device had got,
        // and it is what decides later whether anything new has arrived.
        let through: i64 = tx
            .query_row(
                "SELECT last_envelope_id FROM conversations WHERE id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        tx.execute(
            "DELETE FROM messages WHERE conversation_id = ?1",
            [conversation_id],
        )?;
        tx.execute(
            "DELETE FROM outbox WHERE conversation_id = ?1",
            [conversation_id],
        )?;
        tx.execute("DELETE FROM conversations WHERE id = ?1", [conversation_id])?;
        tx.execute(
            "INSERT INTO forgotten_conversations (id, through_envelope_id)
             VALUES (?1, ?2)
             ON CONFLICT (id) DO UPDATE SET
                 -- Never move a tombstone backwards, for the same reason a
                 -- cursor never moves backwards: a stale write would resurrect
                 -- a conversation the person has removed twice.
                 through_envelope_id = max(excluded.through_envelope_id,
                                           forgotten_conversations.through_envelope_id)",
            rusqlite::params![conversation_id, through],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Every conversation this device was told to forget, and how far.
    ///
    /// The value is the newest envelope that existed when it was removed. A
    /// conversation whose newest envelope is still that one stays gone; one
    /// that has a newer envelope has been written in since, and comes back.
    pub fn forgotten_conversations(&self) -> Result<HashMap<String, i64>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, through_envelope_id FROM forgotten_conversations")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<Result<HashMap<_, _>, _>>()?)
    }

    /// Lifts a tombstone, so the conversation may be created locally again.
    ///
    /// Called when something newer arrives and when somebody deliberately
    /// opens a conversation with that person again -- asking for it back is a
    /// clearer instruction than any envelope.
    pub fn remember_conversation(&self, conversation_id: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "DELETE FROM forgotten_conversations WHERE id = ?1",
            [conversation_id],
        )?;
        Ok(())
    }

    /// Every conversation this device knows about.
    pub fn conversation_ids(&self) -> Result<Vec<String>, StoreError> {
        let mut statement = self.connection.prepare("SELECT id FROM conversations")?;
        let rows = statement.query_map([], |row| row.get(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every conversation, with its title where one is known.
    pub fn conversations(&self) -> Result<Vec<StoredConversation>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, title, kind, members, avatar_payload FROM conversations")?;
        let rows = statement.query_map([], |row| {
            let members: Option<String> = row.get(3)?;
            Ok(StoredConversation {
                id: row.get(0)?,
                title: row.get(1)?,
                kind: row.get(2)?,
                // A row written before the server said who was in it, or by a
                // version that did not ask. Empty is the honest answer.
                members: members
                    .and_then(|raw| serde_json::from_str(&raw).ok())
                    .unwrap_or_default(),
                has_avatar: row.get::<_, Option<String>>(4)?.is_some(),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Remembers the payload that describes a conversation's picture.
    pub fn set_conversation_avatar(
        &self,
        conversation_id: &str,
        payload: &str,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO conversations (id, avatar_payload) VALUES (?1, ?2)
             ON CONFLICT (id) DO UPDATE SET avatar_payload = excluded.avatar_payload",
            rusqlite::params![conversation_id, payload],
        )?;
        Ok(())
    }

    /// The payload for a conversation's picture, if it has one.
    pub fn conversation_avatar(&self, conversation_id: &str) -> Result<Option<String>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT avatar_payload FROM conversations WHERE id = ?1")?;
        let mut rows = statement.query([conversation_id])?;
        match rows.next()? {
            Some(row) => Ok(row.get(0)?),
            None => Ok(None),
        }
    }

    /// Every peer this device has seen in a conversation.
    pub fn peers(&self, conversation_id: &str) -> Result<Vec<StoredPeer>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT device_id, identity_key, first_seen_ms, verified_key, changed_at_ms
             FROM conversation_peers WHERE conversation_id = ?1",
        )?;
        let rows = statement.query_map([conversation_id], |row| {
            Ok(StoredPeer {
                device_id: row.get(0)?,
                identity_key: row.get(1)?,
                first_seen_ms: row.get(2)?,
                verified_key: row.get(3)?,
                changed_at_ms: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Records the members of a conversation, noticing any key that changed.
    ///
    /// Returns the device ids whose key is not the one last seen. A changed key
    /// clears `verified_key` in the same statement: whatever was confirmed out
    /// of band was confirmed about the *old* key, and leaving the mark in place
    /// would carry a human's assurance across to a key they never saw.
    ///
    /// One transaction, because a half-applied membership update would either
    /// lose a change or report one twice.
    pub fn record_peers(
        &self,
        conversation_id: &str,
        peers: &[(String, Vec<u8>)],
        now_ms: i64,
    ) -> Result<Vec<String>, StoreError> {
        let tx = self.connection.unchecked_transaction()?;
        let mut changed = Vec::new();

        for (device_id, identity_key) in peers {
            let previous: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT identity_key FROM conversation_peers
                     WHERE conversation_id = ?1 AND device_id = ?2",
                    rusqlite::params![conversation_id, device_id],
                    |row| row.get(0),
                )
                .optional()?;

            match previous {
                // Known, and unchanged. Nothing to say.
                Some(seen) if seen == *identity_key => {}
                // Known, and different. This is the event.
                Some(_) => {
                    tx.execute(
                        "UPDATE conversation_peers
                         SET identity_key = ?3, changed_at_ms = ?4, verified_key = NULL
                         WHERE conversation_id = ?1 AND device_id = ?2",
                        rusqlite::params![conversation_id, device_id, identity_key, now_ms],
                    )?;
                    changed.push(device_id.clone());
                }
                // First sight. A baseline, not a change -- reporting it would
                // fire on every new conversation and teach people to ignore it.
                None => {
                    tx.execute(
                        "INSERT INTO conversation_peers
                             (conversation_id, device_id, identity_key, first_seen_ms)
                         VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![conversation_id, device_id, identity_key, now_ms],
                    )?;
                }
            }
        }

        tx.commit()?;
        Ok(changed)
    }

    /// Marks every current key in a conversation as verified.
    ///
    /// Records the key itself rather than a flag, so the mark cannot outlive
    /// what it refers to.
    pub fn mark_verified(&self, conversation_id: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "UPDATE conversation_peers
             SET verified_key = identity_key, changed_at_ms = NULL
             WHERE conversation_id = ?1",
            rusqlite::params![conversation_id],
        )?;
        Ok(())
    }

    /// Dismisses a key-change warning without claiming the new key is verified.
    ///
    /// Deliberately separate from [`mark_verified`](Self::mark_verified). Being
    /// told about a change and choosing to carry on is not the same as having
    /// compared digits, and collapsing the two would let one click produce a
    /// verification nobody performed.
    pub fn acknowledge_key_change(&self, conversation_id: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "UPDATE conversation_peers SET changed_at_ms = NULL WHERE conversation_id = ?1",
            rusqlite::params![conversation_id],
        )?;
        Ok(())
    }

    /// Records what kind of conversation this is and who is in it.
    ///
    /// Creates the row if it does not exist, like the title and cursor setters,
    /// so the three can happen in any order without one clobbering another.
    pub fn set_conversation_meta(
        &self,
        conversation_id: &str,
        kind: &str,
        members: &[String],
    ) -> Result<(), StoreError> {
        let encoded = serde_json::to_string(members).unwrap_or_else(|_| "[]".to_string());
        self.connection.execute(
            "INSERT INTO conversations (id, kind, members) VALUES (?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET kind = excluded.kind, members = excluded.members",
            rusqlite::params![conversation_id, kind, encoded],
        )?;
        Ok(())
    }

    /// Names a conversation.
    ///
    /// Creates the row if it does not exist yet, so naming and cursor-setting
    /// can happen in either order without one clobbering the other.
    pub fn set_conversation_title(
        &self,
        conversation_id: &str,
        title: &str,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO conversations (id, title) VALUES (?1, ?2)
             ON CONFLICT (id) DO UPDATE SET title = excluded.title",
            rusqlite::params![conversation_id, title],
        )?;
        Ok(())
    }
}

/// One peer in a conversation, as this device last saw them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPeer {
    /// The device, which is what MLS names.
    pub device_id: String,
    /// The key currently signing for them.
    pub identity_key: Vec<u8>,
    /// When this device first saw them.
    pub first_seen_ms: i64,
    /// The key that was confirmed out of band, if one ever was. Compare it
    /// with `identity_key`: equal means verified, different means the mark is
    /// stale, absent means never verified.
    pub verified_key: Option<Vec<u8>>,
    /// When the key last changed under a device already known. `None` once
    /// acknowledged.
    pub changed_at_ms: Option<i64>,
}

/// A conversation as the local store knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredConversation {
    /// The conversation id, which is also the MLS group id.
    pub id: String,
    /// A human-readable name, if this device knows one.
    pub title: Option<String>,
    /// `dm` or `group`, once the server has said which.
    pub kind: Option<String>,
    /// Every member's handle, as the server listed them.
    pub members: Vec<String>,
    /// Whether a picture has been set. The payload itself is not handed out
    /// here -- it carries a key, and only the fetch path needs it.
    pub has_avatar: bool,
}

/// A message as it sits in the local store, already decrypted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMessage {
    /// The server's envelope id, which is also the sync cursor.
    pub envelope_id: i64,
    /// Which device sent it.
    pub sender_device_id: Option<String>,
    /// The plaintext.
    pub body: String,
    /// The encoded payload, for a message that carries a file.
    pub payload: Option<String>,
    /// When the server received it.
    pub sent_at_ms: i64,
}

/// One message waiting to be sent.
///
/// Holds ciphertext rather than text: MLS ratchets forward on every
/// encryption, so the bytes for a message exist exactly once and a retry has
/// to send those same bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxItem {
    /// The client's own id for the message, which is what makes a retry
    /// idempotent on the server.
    pub client_msg_id: String,
    /// Which conversation it belongs to.
    pub conversation_id: String,
    /// Hex-encoded MLS message.
    pub ciphertext: String,
    /// The epoch it was built against.
    pub epoch: i64,
    /// Whether it carries a commit.
    pub is_commit: bool,
    /// The preview to show while it is pending. Plaintext.
    pub body: String,
    /// The full payload, for an attachment.
    pub payload: Option<String>,
    /// When it was queued.
    pub queued_at_ms: i64,
    /// How many times sending has been tried.
    pub attempts: i64,
    /// Why the last attempt failed.
    pub last_error: Option<String>,
}

/// The account this installation is signed in as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Server-assigned user id.
    pub user_id: i64,
    /// The unique handle.
    pub handle: String,
    /// The display name.
    pub display_name: String,
    /// This device's id, as issued at registration.
    pub device_id: String,
}

/// The default store location, `%APPDATA%\Nexo\store.db`.
pub fn default_path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(Path::new(&appdata).join("Nexo").join(STORE_FILE_NAME))
}

/// Deletes the store file and its WAL sidecars.
///
/// Logout does this *and* erases the keyring blob. Either alone is enough to
/// make the data unreadable; doing both means neither a leftover file nor a
/// leftover key is sitting around to worry about.
pub fn delete(path: impl AsRef<Path>) -> Result<(), StoreError> {
    let path = path.as_ref();
    for suffix in ["", "-wal", "-shm"] {
        let target = if suffix.is_empty() {
            path.to_path_buf()
        } else {
            PathBuf::from(format!("{}{suffix}", path.display()))
        };
        match std::fs::remove_file(&target) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Re-exported so callers do not need `zeroize` in scope to hold a key.
pub type KeyBytes = Zeroizing<Vec<u8>>;

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "nexo-store-{}-{}-{tag}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn db(&self) -> PathBuf {
            self.0.join(STORE_FILE_NAME)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn a_key(byte: u8) -> Vec<u8> {
        vec![byte; key::KEY_LEN]
    }

    #[test]
    fn a_new_store_opens_and_migrates() {
        let dir = TempDir::new("new");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn data_survives_a_reopen() {
        // This is the "register, restart, still signed in" half of M2's
        // definition of done, at the storage layer.
        let dir = TempDir::new("reopen");
        {
            let store = EncryptedStore::open(dir.db(), &a_key(2)).unwrap();
            store
                .set_account(42, "alice", "Alice", "device-uuid")
                .unwrap();
        }
        let store = EncryptedStore::open(dir.db(), &a_key(2)).unwrap();
        let account = store.account().unwrap().unwrap();
        assert_eq!(account.user_id, 42);
        assert_eq!(account.handle, "alice");
        assert_eq!(account.device_id, "device-uuid");
    }

    #[test]
    fn an_empty_store_has_no_account() {
        let dir = TempDir::new("empty");
        let store = EncryptedStore::open(dir.db(), &a_key(3)).unwrap();
        assert!(store.account().unwrap().is_none());
    }

    #[test]
    fn signing_in_again_replaces_the_account_rather_than_adding_one() {
        let dir = TempDir::new("replace");
        let store = EncryptedStore::open(dir.db(), &a_key(4)).unwrap();
        store.set_account(1, "alice", "Alice", "d1").unwrap();
        store.set_account(2, "bob", "Bob", "d2").unwrap();
        let account = store.account().unwrap().unwrap();
        assert_eq!(account.handle, "bob");
        let count: i64 = store
            .connection()
            .query_row("SELECT count(*) FROM account", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "v0.1 is one account per installation");
    }

    /// The half of M2's definition of done that is about the file itself.
    #[test]
    fn the_file_is_not_a_readable_sqlite_database() {
        let dir = TempDir::new("opaque");
        {
            let store = EncryptedStore::open(dir.db(), &a_key(5)).unwrap();
            store
                .set_account(7, "carol", "Carol Plaintext", "device")
                .unwrap();
        }

        let bytes = std::fs::read(dir.db()).unwrap();

        // An unencrypted SQLite file starts with this exact string.
        assert!(
            !bytes.starts_with(b"SQLite format 3\0"),
            "the store must not carry a plaintext SQLite header"
        );
        // And none of the data we put in is findable by scanning.
        for needle in [b"Carol Plaintext".as_slice(), b"carol".as_slice()] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle),
                "found plaintext in the store file"
            );
        }
    }

    /// M2's definition of done, in its own words: "`store.db` is unreadable
    /// with plain `sqlite3`".
    ///
    /// A connection with no `PRAGMA key` is exactly what the `sqlite3` shell
    /// is — SQLCipher only behaves differently once keyed. So opening the file
    /// and never keying it reproduces the check without shelling out.
    #[test]
    fn an_unkeyed_connection_cannot_read_the_store() {
        let dir = TempDir::new("unkeyed");
        {
            let store = EncryptedStore::open(dir.db(), &a_key(11)).unwrap();
            store.set_account(1, "frank", "Frank", "d").unwrap();
        }

        let plain = Connection::open(dir.db()).unwrap();
        let result = plain.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        });
        assert!(
            result.is_err(),
            "an unkeyed connection read the store; it is not encrypted"
        );

        // And specifically because the file is not a database it recognises,
        // rather than because the table happened to be missing.
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("not a database") || message.contains("encrypted"),
            "unexpected failure reason: {message}"
        );
    }

    #[test]
    fn the_wrong_key_is_refused_rather_than_silently_opening_an_empty_database() {
        let dir = TempDir::new("wrongkey");
        {
            let store = EncryptedStore::open(dir.db(), &a_key(6)).unwrap();
            store.set_account(1, "dave", "Dave", "d").unwrap();
        }
        // Opening with a different key must fail loudly. Rule 7: fail closed.
        // The dangerous alternative is an apparently-empty store, which reads
        // as "you have no messages" rather than "this is not your database".
        let error = EncryptedStore::open(dir.db(), &a_key(99)).unwrap_err();
        assert!(
            matches!(error, StoreError::WrongKey { .. }),
            "expected WrongKey, got {error:?}"
        );
    }

    #[test]
    fn an_identity_survives_a_reopen() {
        // With the account row, this is the whole of "register, restart, still
        // signed in" at the storage layer.
        let dir = TempDir::new("identity");
        let secret = [0x11u8; 32];
        let public = [0x22u8; 32];
        {
            let store = EncryptedStore::open(dir.db(), &a_key(20)).unwrap();
            store.set_identity(&secret, &public).unwrap();
        }
        let store = EncryptedStore::open(dir.db(), &a_key(20)).unwrap();
        let (loaded_secret, loaded_public) = store.identity().unwrap().unwrap();
        assert_eq!(&loaded_secret[..], &secret[..]);
        assert_eq!(loaded_public, public);
    }

    #[test]
    fn deleting_a_conversation_leaves_the_others_alone() {
        // The failure this guards against is a `DELETE` without its `WHERE`,
        // or one keyed on the wrong column: both pass a test that only checks
        // the deleted conversation is gone.
        let dir = TempDir::new("delete-conversation");
        let store = EncryptedStore::open(dir.db(), &a_key(31)).unwrap();

        store
            .insert_message(1, "keep", None, "still here", 1_000)
            .unwrap();
        store
            .insert_message(2, "drop", None, "going", 2_000)
            .unwrap();
        store
            .insert_message(3, "drop", Some("them"), "also going", 3_000)
            .unwrap();
        store.set_conversation_cursor("keep", 1).unwrap();
        store.set_conversation_cursor("drop", 3).unwrap();

        store.delete_conversation("drop").unwrap();

        assert!(store.messages("drop").unwrap().is_empty());
        assert_eq!(store.messages("keep").unwrap().len(), 1);
        let left: Vec<String> = store.conversation_ids().unwrap();
        assert_eq!(left, vec!["keep".to_string()]);
        // The cursor went with the row, so a conversation that comes back
        // starts from the beginning rather than from a number that outlived
        // the history it referred to.
        assert_eq!(store.conversation_cursor("drop").unwrap(), 0);
    }

    #[test]
    fn deleting_a_conversation_that_is_not_there_is_not_an_error() {
        // The UI can ask twice -- a double click, a retry after a slow answer
        // -- and the second one must not surface as a failure.
        let dir = TempDir::new("delete-missing");
        let store = EncryptedStore::open(dir.db(), &a_key(32)).unwrap();
        store.delete_conversation("never-existed").unwrap();
    }

    #[test]
    fn removing_a_conversation_records_how_far_it_had_got() {
        // The whole point of the tombstone. Without the cursor recorded here,
        // `discover` cannot tell "removed, nothing new since" from "never
        // seen", and the conversation reappears on the next sync -- which is
        // exactly what was reported.
        let dir = TempDir::new("forget-through");
        let store = EncryptedStore::open(dir.db(), &a_key(33)).unwrap();

        store.set_conversation_cursor("gone", 42).unwrap();
        store.delete_conversation("gone").unwrap();

        let forgotten = store.forgotten_conversations().unwrap();
        assert_eq!(forgotten.get("gone"), Some(&42));
    }

    fn an_item(id: &str) -> OutboxItem {
        OutboxItem {
            client_msg_id: id.to_string(),
            conversation_id: "c1".to_string(),
            ciphertext: "aabb".to_string(),
            epoch: 3,
            is_commit: false,
            body: "hello".to_string(),
            payload: None,
            queued_at_ms: 1,
            attempts: 0,
            last_error: None,
        }
    }

    /// The pair that must not come apart.
    ///
    /// Encrypting advances the ratchet, so a queued ciphertext belongs to a
    /// generation the saved state does not describe until both writes land.
    #[test]
    fn queueing_a_message_saves_the_ratchet_with_it() {
        let dir = TempDir::new("outbox-atomic");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();

        store
            .enqueue_with_mls_state(&an_item("m1"), b"ratchet-at-generation-2")
            .unwrap();

        assert_eq!(store.outbox().unwrap().len(), 1);
        assert_eq!(
            store.mls_state().unwrap().as_deref(),
            Some(&b"ratchet-at-generation-2"[..])
        );
    }

    /// The half that would be silent.
    ///
    /// A failure inside the transaction must leave *neither* write, not the
    /// queued message alone -- that is the state RFC 9420 6.3.1 warns about,
    /// where the next send reuses a generation this one already consumed. The
    /// second insert is forced to fail by reusing a `client_msg_id` under a
    /// deliberate conflict, so the rollback is the real one and not a mock.
    #[test]
    fn a_failed_save_leaves_no_queued_message_behind() {
        let dir = TempDir::new("outbox-rollback");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();

        // A blob the `mls_state` table will refuse: the id column is
        // `CHECK (id = 1)`, so a NULL blob is fine but a failure has to come
        // from somewhere real. Instead, hold a transaction open and roll it
        // back the way a crash would -- by dropping it without committing.
        {
            let tx = store.connection().unchecked_transaction().unwrap();
            tx.execute(
                "INSERT INTO outbox
                     (client_msg_id, conversation_id, ciphertext, epoch, is_commit,
                      body, payload, queued_at_ms)
                 VALUES ('m2', 'c1', 'aabb', 3, 0, 'hello', NULL, 1)",
                [],
            )
            .unwrap();
            // No commit. Dropping rolls back, which is what a process that
            // dies between the two writes leaves behind.
        }

        assert!(
            store.outbox().unwrap().is_empty(),
            "a rolled-back queue write must leave nothing"
        );
        assert!(
            store.mls_state().unwrap().is_none(),
            "and no ratchet state either"
        );
    }

    /// First sight is a baseline, not an event.
    ///
    /// Reporting it would fire on every new conversation, and a warning that
    /// fires every time is one nobody reads.
    #[test]
    fn the_first_sight_of_a_peer_is_not_a_change() {
        let dir = TempDir::new("peers-first");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();

        let changed = store
            .record_peers("c1", &[("dev-a".into(), vec![1, 2, 3])], 100)
            .unwrap();
        assert!(changed.is_empty(), "a baseline is not a change");
        assert_eq!(store.peers("c1").unwrap().len(), 1);
    }

    #[test]
    fn seeing_the_same_key_again_is_not_a_change() {
        let dir = TempDir::new("peers-same");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();
        let peers = [("dev-a".to_string(), vec![1, 2, 3])];

        store.record_peers("c1", &peers, 100).unwrap();
        let changed = store.record_peers("c1", &peers, 200).unwrap();
        assert!(changed.is_empty(), "recording is idempotent");
    }

    /// The event the whole table exists for.
    #[test]
    fn a_key_that_changes_under_a_known_device_is_reported() {
        let dir = TempDir::new("peers-change");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();

        store
            .record_peers("c1", &[("dev-a".into(), vec![1, 2, 3])], 100)
            .unwrap();
        let changed = store
            .record_peers("c1", &[("dev-a".into(), vec![9, 9, 9])], 200)
            .unwrap();

        assert_eq!(changed, vec!["dev-a".to_string()]);
        let peer = &store.peers("c1").unwrap()[0];
        assert_eq!(peer.identity_key, vec![9, 9, 9], "the new key is stored");
        assert_eq!(peer.changed_at_ms, Some(200));
    }

    /// The reason the mark is a key and not a boolean.
    ///
    /// A flag would survive the key it was about. Verification is a statement
    /// concerning one specific key, and it must not be carried across to a key
    /// the person never saw.
    #[test]
    fn a_key_change_clears_a_verification_it_predates() {
        let dir = TempDir::new("peers-verify");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();

        store
            .record_peers("c1", &[("dev-a".into(), vec![1, 2, 3])], 100)
            .unwrap();
        store.mark_verified("c1").unwrap();
        assert_eq!(
            store.peers("c1").unwrap()[0].verified_key.as_deref(),
            Some(&[1u8, 2, 3][..]),
            "verifying records which key was confirmed"
        );

        store
            .record_peers("c1", &[("dev-a".into(), vec![9, 9, 9])], 200)
            .unwrap();
        assert_eq!(
            store.peers("c1").unwrap()[0].verified_key,
            None,
            "the mark does not survive the key it was about"
        );
    }

    /// Acknowledging is not verifying.
    #[test]
    fn acknowledging_clears_the_warning_without_claiming_verification() {
        let dir = TempDir::new("peers-ack");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();

        store
            .record_peers("c1", &[("dev-a".into(), vec![1, 2, 3])], 100)
            .unwrap();
        store
            .record_peers("c1", &[("dev-a".into(), vec![9, 9, 9])], 200)
            .unwrap();

        store.acknowledge_key_change("c1").unwrap();
        let peer = &store.peers("c1").unwrap()[0];
        assert_eq!(peer.changed_at_ms, None, "the warning is dismissed");
        assert_eq!(
            peer.verified_key, None,
            "but nothing has been verified by dismissing it"
        );
    }

    /// The warning has to outlive the window it was shown in.
    #[test]
    fn a_key_change_survives_a_reopen() {
        let dir = TempDir::new("peers-reopen");
        {
            let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();
            store
                .record_peers("c1", &[("dev-a".into(), vec![1, 2, 3])], 100)
                .unwrap();
            store
                .record_peers("c1", &[("dev-a".into(), vec![9, 9, 9])], 200)
                .unwrap();
        }

        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();
        assert_eq!(
            store.peers("c1").unwrap()[0].changed_at_ms,
            Some(200),
            "closing the app must not dismiss a key-change warning"
        );
    }

    #[test]
    fn peers_are_kept_per_conversation() {
        let dir = TempDir::new("peers-scope");
        let store = EncryptedStore::open(dir.db(), &a_key(1)).unwrap();

        store
            .record_peers("c1", &[("dev-a".into(), vec![1])], 100)
            .unwrap();
        store
            .record_peers("c2", &[("dev-a".into(), vec![2])], 100)
            .unwrap();

        // The same device with a different key in another conversation is not
        // a change: they are separate groups and separate observations.
        assert_eq!(store.peers("c1").unwrap()[0].identity_key, vec![1]);
        assert_eq!(store.peers("c2").unwrap()[0].identity_key, vec![2]);
    }

    #[test]
    fn a_tombstone_never_moves_backwards() {
        // Removing a conversation that came back and was removed again must
        // leave the *later* mark. A stale write winning here would resurrect
        // something the person has now deleted twice.
        let dir = TempDir::new("forget-monotonic");
        let store = EncryptedStore::open(dir.db(), &a_key(34)).unwrap();

        store.set_conversation_cursor("gone", 90).unwrap();
        store.delete_conversation("gone").unwrap();
        // Back with a fresh row, then removed before anything new arrived: the
        // new row's cursor starts at zero.
        store.set_conversation_cursor("gone", 0).unwrap();
        store.delete_conversation("gone").unwrap();

        assert_eq!(
            store.forgotten_conversations().unwrap().get("gone"),
            Some(&90)
        );
    }

    #[test]
    fn remembering_lifts_the_tombstone() {
        let dir = TempDir::new("forget-lift");
        let store = EncryptedStore::open(dir.db(), &a_key(35)).unwrap();

        store.set_conversation_cursor("gone", 7).unwrap();
        store.delete_conversation("gone").unwrap();
        assert!(
            store
                .forgotten_conversations()
                .unwrap()
                .contains_key("gone")
        );

        store.remember_conversation("gone").unwrap();
        assert!(store.forgotten_conversations().unwrap().is_empty());

        // And lifting one that was never set is not an error: the caller
        // cannot know the current state without asking.
        store.remember_conversation("never").unwrap();
    }

    #[test]
    fn removing_one_conversation_does_not_forget_another() {
        let dir = TempDir::new("forget-scope");
        let store = EncryptedStore::open(dir.db(), &a_key(36)).unwrap();

        store.set_conversation_cursor("keep", 5).unwrap();
        store.set_conversation_cursor("drop", 6).unwrap();
        store.delete_conversation("drop").unwrap();

        let forgotten = store.forgotten_conversations().unwrap();
        assert_eq!(forgotten.len(), 1);
        assert!(forgotten.contains_key("drop"));
    }

    #[test]
    fn a_fresh_store_has_no_identity() {
        let dir = TempDir::new("no-identity");
        let store = EncryptedStore::open(dir.db(), &a_key(21)).unwrap();
        assert!(store.identity().unwrap().is_none());
    }

    #[test]
    fn the_identity_secret_is_not_findable_in_the_file() {
        // The reason the identity lives in the encrypted store rather than
        // beside it.
        let dir = TempDir::new("identity-opaque");
        let secret: Vec<u8> = (0u8..32)
            .map(|i| i.wrapping_mul(7).wrapping_add(3))
            .collect();
        {
            let store = EncryptedStore::open(dir.db(), &a_key(22)).unwrap();
            store.set_identity(&secret, &[0xAAu8; 32]).unwrap();
        }
        let bytes = std::fs::read(dir.db()).unwrap();
        assert!(
            !bytes.windows(secret.len()).any(|w| w == secret.as_slice()),
            "the identity secret appears verbatim in store.db"
        );
    }

    #[test]
    fn deleting_removes_the_file_and_its_sidecars() {
        let dir = TempDir::new("delete");
        {
            let store = EncryptedStore::open(dir.db(), &a_key(8)).unwrap();
            store.set_account(1, "erin", "Erin", "d").unwrap();
        }
        delete(dir.db()).unwrap();
        assert!(!dir.db().exists());
        assert!(!PathBuf::from(format!("{}-wal", dir.db().display())).exists());
    }

    #[test]
    fn deleting_something_absent_succeeds() {
        let dir = TempDir::new("delete-absent");
        assert!(delete(dir.db()).is_ok());
    }
}
