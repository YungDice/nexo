//! Starting, sending and syncing a conversation.
//!
//! Where MLS, the encrypted store and the transport meet. Everything here obeys
//! one ordering rule, and it is the same rule every time:
//!
//! > **The server decides, then we persist.**
//!
//! Encrypt, hand it to the delivery service, and only write local state once it
//! was accepted. The other order — apply locally, then send — is how a client
//! ends up in an epoch nobody else is in, with every subsequent message
//! undecryptable to everyone (PLAN.md risk 4(b)).

use nexo_crypto::CryptoError;
use nexo_crypto::mls::{self, Conversation, Incoming, Peeked};
use nexo_protocol::{ConversationId, Payload};
use nexo_store::EncryptedStore;
use openmls::prelude::CredentialWithKey;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;

use crate::mls_state;
use crate::outbox;
use crate::transport::{Transport, TransportError};

/// What can go wrong running a conversation.
#[derive(Debug, thiserror::Error)]
pub enum ConversationError {
    /// The network layer failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// MLS refused.
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    /// The local store failed.
    #[error(transparent)]
    Store(#[from] nexo_store::StoreError),
    /// Persisting MLS state failed.
    #[error(transparent)]
    MlsState(#[from] mls_state::MlsStateError),
    /// Identity key material was rejected.
    #[error(transparent)]
    Identity(#[from] nexo_crypto::identity::IdentityError),
    /// Attachment encryption or decryption failed.
    #[error(transparent)]
    Attachment(#[from] nexo_crypto::attachment::AttachmentError),
    /// This device is not in that conversation.
    #[error("this device is not a member of that conversation")]
    NotAMember,
    /// A caller asked to open something that is not a file.
    #[error("that message has no attachment")]
    NotAnAttachment,
}

impl ConversationError {
    /// Whether this is the network being down rather than a refusal.
    ///
    /// The distinction the outbox turns on: an unreachable server means try
    /// again later, anything else means retrying will produce the same answer.
    pub fn is_offline(&self) -> bool {
        matches!(
            self,
            ConversationError::Transport(TransportError::Unreachable(_))
        )
    }
}

/// Everything a conversation call needs to reach.
///
/// Grouped into one struct because passing five arguments to every function is
/// how one of them eventually gets passed in the wrong order.
pub struct Context<'a, T: Transport> {
    /// The network.
    pub transport: &'a T,
    /// MLS storage and crypto.
    pub provider: &'a OpenMlsRustCrypto,
    /// The encrypted local store.
    pub store: &'a EncryptedStore,
    /// This device's MLS signer.
    pub signer: &'a SignatureKeyPair,
    /// This device's MLS credential.
    pub credential: CredentialWithKey,
}

/// Publishes a fresh batch of KeyPackages, so other people can invite us.
///
/// Called at registration and whenever the server says the supply is low.
/// Running out is not a crash — it is nobody being able to start a conversation
/// with you, which is worse for being invisible.
pub fn publish_key_packages<T: Transport>(
    ctx: &Context<'_, T>,
    count: usize,
) -> Result<(), ConversationError> {
    let packages =
        mls::generate_key_packages(ctx.provider, ctx.signer, ctx.credential.clone(), count)?;
    let hex: Vec<String> = packages.iter().map(|p| to_hex(p)).collect();
    ctx.transport.publish_key_packages(&hex)?;
    mls_state::save(ctx.provider, ctx.store)?;
    Ok(())
}

/// Tops up KeyPackages if the server says we are running low.
///
/// Returns how many were published, so a caller can log "nothing to do"
/// distinctly from "topped up".
pub fn refill_key_packages_if_low<T: Transport>(
    ctx: &Context<'_, T>,
) -> Result<usize, ConversationError> {
    let (remaining, refill_below) = ctx.transport.key_package_count()?;
    if remaining >= refill_below {
        return Ok(0);
    }
    let wanted = nexo_crypto::KEY_PACKAGE_TARGET.saturating_sub(remaining.max(0) as usize);
    if wanted == 0 {
        return Ok(0);
    }
    publish_key_packages(ctx, wanted)?;
    Ok(wanted)
}

/// Starts a 1:1 conversation with someone.
///
/// The order matters and is the rule at the top of this module:
///
/// 1. claim their KeyPackage — single-use, so this is the point of no return;
/// 2. build the group locally and create the add commit;
/// 3. register the conversation with the server, then **send the commit
///    before applying it**;
/// 4. only once the server accepted, confirm it locally and persist.
///
/// # Why registration moved to step 3
///
/// It used to happen before the commit was built, and the doc comment here
/// claimed that a refusal left "no half-created conversation the UI can find".
/// That was true of the local store and false of the server: `create` writes
/// the conversation and both membership rows and commits them, so a failure
/// afterwards left a row nobody could ever send in — and `discover` dutifully
/// pulled it onto both devices as a real chat. That is one of the two ways the
/// same person ended up with two conversations.
///
/// Registering last shrinks the window to a single round trip. It cannot close
/// it: there is no transaction spanning "create" and "send". What closes it is
/// [`open_with`] refusing to reuse a conversation with no envelopes and no
/// group, and the server refusing to open a second DM between the same two
/// people at all.
pub fn start_with<T: Transport>(
    ctx: &Context<'_, T>,
    handle: &str,
) -> Result<ConversationId, ConversationError> {
    let claimed = ctx.transport.claim_key_package(handle)?;
    let key_package = from_hex(&claimed.key_package)?;

    let conversation_id = ConversationId::new_v4();
    let mut conversation = Conversation::create(
        ctx.provider,
        ctx.signer,
        ctx.credential.clone(),
        conversation_id,
        now_ms(),
    )?;

    let commit = conversation.add_member(ctx.provider, ctx.signer, &key_package)?;

    // Only now, with something to send. Everything above this line is local
    // and costs nothing if it fails; everything below leaves a row on the
    // server whether or not the rest works.
    //
    // The commit is pending by this point, so a refusal here has to abandon it
    // for the same reason a refused `send` does -- a group left holding a
    // commit it will never apply cannot be used again.
    let settled = match ctx
        .transport
        .create_conversation(&conversation_id.to_string(), &[handle.to_string()])
    {
        Ok(settled) => settled,
        Err(error) => {
            conversation.abandon_commit(ctx.provider)?;
            return Err(error.into());
        }
    };

    // The server may hand back a different conversation: there is one DM per
    // pair of people, and if the other person started theirs a moment earlier
    // that is the one that exists. Adopt it. The group built above is thrown
    // away unused -- the way into theirs is the Welcome their commit already
    // sent to our KeyPackage, which arrives on the next sync.
    if settled != conversation_id.to_string() {
        tracing::info!(
            mine = %conversation_id,
            theirs = %settled,
            "the server already had a conversation with this person; adopting it"
        );
        conversation.abandon_commit(ctx.provider)?;
        ctx.store.remember_conversation(&settled)?;
        ctx.store.set_conversation_cursor(&settled, 0)?;
        ctx.store.set_conversation_title(&settled, handle)?;
        ctx.store
            .set_conversation_meta(&settled, "dm", &[handle.to_string()])?;
        mls_state::save(ctx.provider, ctx.store)?;
        return settled.parse::<ConversationId>().map_err(|error| {
            ConversationError::Transport(TransportError::Rejected(format!(
                "the server answered with a conversation id that will not parse: {error}"
            )))
        });
    }

    // Send first. If the delivery service refuses, nothing local has changed.
    // A fresh id per commit, so a retry after a lost reply is answered with
    // the envelope the first attempt created rather than refused as stale.
    let accepted = ctx.transport.send(
        &conversation_id.to_string(),
        &to_hex(&commit.message),
        conversation.epoch() as i64,
        true,
        &outbox::new_message_id(),
    );
    let accepted = match accepted {
        Ok(accepted) => accepted,
        Err(error) => {
            conversation.abandon_commit(ctx.provider)?;
            return Err(error.into());
        }
    };

    conversation.confirm_commit(ctx.provider, now_ms())?;

    // The Welcome travels as an ordinary envelope. The invitee is already a
    // member server-side, so the conversation's own stream is the delivery
    // path and no separate endpoint is needed.
    if let Some(welcome) = commit.welcome {
        ctx.transport.send(
            &conversation_id.to_string(),
            &to_hex(&welcome),
            conversation.epoch() as i64,
            false,
            &outbox::new_message_id(),
        )?;
    }

    ctx.store
        .set_conversation_cursor(&conversation_id.to_string(), accepted.envelope_id)?;
    // The only moment this device knows who it invited.
    ctx.store
        .set_conversation_title(&conversation_id.to_string(), handle)?;
    ctx.store
        .set_conversation_meta(&conversation_id.to_string(), "dm", &[handle.to_string()])?;
    mls_state::save(ctx.provider, ctx.store)?;

    Ok(conversation_id)
}

/// Opens the conversation with someone, starting one only if there is none.
///
/// The reason this exists rather than calling [`start_with`] directly: a
/// KeyPackage is single-use and a group is created per call, so "message this
/// person" wired straight to `start_with` makes a new conversation every time
/// it is pressed. Pressed from a profile, that is an endless list of identical
/// empty chats and a consumed KeyPackage for each.
///
/// The server's membership list is the authority, not the local store: the
/// other person may have started it, in which case this device has a
/// conversation it never created and must not duplicate.
///
/// # Not every match is worth reusing
///
/// A conversation can exist on the server and be **unusable**: [`start_with`]
/// registers it before it knows the add commit will be accepted, and if that
/// send fails the local group is abandoned while the server's row survives
/// with both members on it. Nothing can ever be sent in it -- nobody holds MLS
/// state for it -- and because the server lists newest first, such a leftover
/// would be found *before* a conversation that works. That is one of the two
/// ways the same person ended up with two chats. So a candidate is only reused
/// if it is alive: it has envelopes, or this device holds the group.
pub fn open_with<T: Transport>(
    ctx: &Context<'_, T>,
    handle: &str,
) -> Result<ConversationId, ConversationError> {
    let handle = handle.trim().to_lowercase();
    let me = ctx.store.account()?.map(|a| a.handle.to_lowercase());
    let forgotten = ctx.store.forgotten_conversations()?;

    let mut dead: Option<(ConversationId, String)> = None;

    for summary in ctx.transport.list_conversations()? {
        if summary.kind != "dm" {
            continue;
        }
        // Exactly the two of us. A DM that has grown members is a group the
        // server has not relabelled, and reusing it would put a private
        // message somewhere it does not belong.
        let others: Vec<String> = summary
            .members
            .iter()
            .map(|h| h.to_lowercase())
            .filter(|h| Some(h) != me.as_ref())
            .collect();
        if others.len() != 1 || others[0] != handle {
            continue;
        }
        let id = match summary.conversation_id.parse::<ConversationId>() {
            Ok(id) => id,
            Err(error) => {
                // Loud, because of what silence here would cost. This is the
                // one branch that turns "we already have this conversation"
                // into "start a new one", and a `if let Ok` that quietly moved
                // on would produce a second chat with the same person and no
                // trace of why. If this ever fires, the duplicate is the
                // symptom and this line is the cause.
                tracing::warn!(
                    id = %summary.conversation_id,
                    %error,
                    "the server listed a conversation whose id will not parse; \
                     it cannot be matched, and starting a new one is the only \
                     thing left"
                );
                continue;
            }
        };

        // Has anything ever been sent in it, or do we hold the group? Either
        // makes it real. Neither makes it a leftover from a `start_with` that
        // did not finish.
        let has_envelopes = summary.latest_envelope_id.is_some();
        let joined = Conversation::load(ctx.provider, id, now_ms())?.is_some();
        if !has_envelopes && !joined {
            tracing::warn!(
                %id,
                "the server lists a conversation with this person that has no \
                 envelopes and no group on this device; it is a half-created \
                 leftover and will not be reused"
            );
            dead.get_or_insert((id, summary.conversation_id.clone()));
            continue;
        }

        // Asking to talk to somebody again is a clearer instruction than any
        // envelope, so a deliberate open lifts a tombstone -- and resumes from
        // where the removal left off rather than replaying what was deleted.
        let resume = forgotten
            .get(&summary.conversation_id)
            .copied()
            .unwrap_or(0);
        if resume > 0 {
            ctx.store.remember_conversation(&summary.conversation_id)?;
        }

        // It may be one we have never synced -- the other side started it.
        ctx.store
            .set_conversation_cursor(&summary.conversation_id, resume)?;
        ctx.store
            .set_conversation_title(&summary.conversation_id, &handle)?;
        ctx.store.set_conversation_meta(
            &summary.conversation_id,
            &summary.kind,
            &summary.members,
        )?;
        return Ok(id);
    }

    // Nothing usable. If a leftover is what we found, clear it away first --
    // locally, so `discover` stops drawing it beside the conversation about to
    // be created, and on the server, so the one-DM-per-pair rule does not hand
    // the same leftover back forever.
    //
    // The server refuses to discard anything with an envelope in it, so this
    // cannot reach a real conversation. What it can reach, in the width of one
    // round trip, is a conversation the other person created a moment ago and
    // has not sent into yet. That resolves itself: their send then fails, they
    // abandon their commit and try again, and they find the one created here.
    // One conversation survives either way, which is the point.
    if let Some((id, raw)) = dead {
        tracing::info!(%id, "clearing a half-created conversation before starting a new one");
        ctx.store.delete_conversation(&raw)?;
        if let Err(error) = ctx.transport.discard_conversation(&raw) {
            // Best effort. Failing here means the leftover stays on the server
            // and `start_with` below will be handed it back -- worth a line in
            // the log, not worth refusing to open a conversation over.
            tracing::warn!(%id, %error, "the server would not discard the leftover");
        }
    }

    tracing::debug!(%handle, "no existing conversation on the server; starting one");
    start_with(ctx, &handle)
}

/// Starts a group conversation with several people at once.
///
/// The same order as [`start_with`], repeated per member: each KeyPackage is
/// claimed, added by its own commit, and the resulting Welcome sent, before
/// moving to the next. One commit per member rather than one for all of them,
/// because a partial failure then leaves a group that is smaller than asked
/// for rather than one whose state the server and this device disagree about.
///
/// The server decides `kind` from the member count, so naming every member at
/// creation is what makes this a group rather than a DM that grew.
pub fn start_group_with<T: Transport>(
    ctx: &Context<'_, T>,
    handles: &[String],
    title: &str,
) -> Result<ConversationId, ConversationError> {
    if handles.is_empty() {
        return Err(ConversationError::NotAMember);
    }

    // Claim every KeyPackage first. They are single-use, so a handle that
    // cannot be invited should fail before any group exists rather than after
    // half of them are in it.
    let mut packages = Vec::with_capacity(handles.len());
    for handle in handles {
        let claimed = ctx.transport.claim_key_package(handle)?;
        packages.push((handle.clone(), from_hex(&claimed.key_package)?));
    }

    let conversation_id = ConversationId::new_v4();
    let mut conversation = Conversation::create(
        ctx.provider,
        ctx.signer,
        ctx.credential.clone(),
        conversation_id,
        now_ms(),
    )?;

    let id = conversation_id.to_string();
    ctx.transport.create_conversation(&id, handles)?;

    for (_, key_package) in &packages {
        let commit = conversation.add_member(ctx.provider, ctx.signer, key_package)?;

        let sent = ctx.transport.send(
            &id,
            &to_hex(&commit.message),
            conversation.epoch() as i64,
            true,
            &outbox::new_message_id(),
        );
        if let Err(error) = sent {
            conversation.abandon_commit(ctx.provider)?;
            return Err(error.into());
        }
        conversation.confirm_commit(ctx.provider, now_ms())?;

        if let Some(welcome) = commit.welcome {
            ctx.transport.send(
                &id,
                &to_hex(&welcome),
                conversation.epoch() as i64,
                false,
                &outbox::new_message_id(),
            )?;
        }
    }

    ctx.store.set_conversation_title(&id, title)?;
    ctx.store.set_conversation_meta(&id, "group", handles)?;
    mls_state::save(ctx.provider, ctx.store)?;
    Ok(conversation_id)
}

/// Adds someone to an existing conversation.
///
/// The order is the whole of M5's correctness, and it is the reverse of what
/// looks natural:
///
/// 1. **Membership row first.** The invitee has to be able to *fetch* the
///    Welcome, and they cannot sync a conversation they are not a member of.
///    The row grants no ability to read — the server holds no group secrets.
/// 2. **Then the commit**, sent before it is applied, like every other commit.
/// 3. **Then the Welcome**, which is the thing that actually admits them.
///
/// Doing it the other way — commit first — produces a Welcome addressed to
/// someone who cannot reach it, and a group that has already rekeyed around a
/// member who will never arrive.
///
/// What the new member can read is decided by MLS, not by any of this: they
/// join at the current epoch and the ratchet gives them no way back. That is
/// M5's check, and `a_member_added_later_cannot_read_earlier_messages` is where
/// it is proven.
pub fn add_to<T: Transport>(
    ctx: &Context<'_, T>,
    conversation_id: ConversationId,
    handle: &str,
) -> Result<(), ConversationError> {
    let claimed = ctx.transport.claim_key_package(handle)?;
    let key_package = from_hex(&claimed.key_package)?;

    let id = conversation_id.to_string();
    ctx.transport.add_member(&id, handle)?;

    let mut conversation = Conversation::load(ctx.provider, conversation_id, now_ms())?
        .ok_or(ConversationError::NotAMember)?;

    let commit = conversation.add_member(ctx.provider, ctx.signer, &key_package)?;

    let sent = ctx.transport.send(
        &id,
        &to_hex(&commit.message),
        conversation.epoch() as i64,
        true,
        &outbox::new_message_id(),
    );
    if let Err(error) = sent {
        // The membership row is left in place deliberately: it grants nothing
        // on its own, and removing it here would need another call that can
        // also fail. The next successful add reuses it.
        conversation.abandon_commit(ctx.provider)?;
        return Err(error.into());
    }

    conversation.confirm_commit(ctx.provider, now_ms())?;

    if let Some(welcome) = commit.welcome {
        ctx.transport.send(
            &id,
            &to_hex(&welcome),
            conversation.epoch() as i64,
            false,
            &outbox::new_message_id(),
        )?;
    }

    mls_state::save(ctx.provider, ctx.store)?;
    Ok(())
}

/// Removes someone from a conversation.
///
/// The reverse ordering, for the same reason: the **commit** goes first,
/// because that is what rekeys the group and actually stops them reading.
/// Dropping the membership row first would stop them fetching while leaving
/// them able to decrypt anything they had already collected — and would remove
/// the only route the removal commit has to reach them.
pub fn remove_from<T: Transport>(
    ctx: &Context<'_, T>,
    conversation_id: ConversationId,
    handle: &str,
    leaf_index: u32,
) -> Result<(), ConversationError> {
    let id = conversation_id.to_string();
    let mut conversation = Conversation::load(ctx.provider, conversation_id, now_ms())?
        .ok_or(ConversationError::NotAMember)?;

    let commit = conversation.remove_member(ctx.provider, ctx.signer, leaf_index)?;

    let sent = ctx.transport.send(
        &id,
        &to_hex(&commit.message),
        conversation.epoch() as i64,
        true,
        &outbox::new_message_id(),
    );
    if let Err(error) = sent {
        conversation.abandon_commit(ctx.provider)?;
        return Err(error.into());
    }
    conversation.confirm_commit(ctx.provider, now_ms())?;

    // Only now does routing catch up with the crypto.
    ctx.transport.remove_member(&id, handle)?;
    mls_state::save(ctx.provider, ctx.store)?;
    Ok(())
}

/// Sends a message.
///
/// Encrypt, send, then persist — never the other way round. A message written
/// to the local history that the server refused is a message the user believes
/// they sent.
pub fn send_message<T: Transport>(
    ctx: &Context<'_, T>,
    conversation_id: ConversationId,
    body: &str,
) -> Result<Sent, ConversationError> {
    let mut conversation = Conversation::load(ctx.provider, conversation_id, now_ms())?
        .ok_or(ConversationError::NotAMember)?;

    // Encrypted before anything else, and exactly once. MLS ratchets forward
    // on every encryption, so these bytes are the message -- a retry sends
    // them again rather than producing new ones.
    let ciphertext =
        conversation.encrypt(ctx.provider, ctx.signer, &Payload::text(body).encode())?;

    // Queued before it is sent, not after a failure. If this process dies
    // between encrypting and sending, the message is on disk and the ratchet
    // has already moved -- queueing first is what makes those two facts agree.
    let client_msg_id = outbox::new_message_id();
    let item = nexo_store::OutboxItem {
        client_msg_id: client_msg_id.clone(),
        conversation_id: conversation_id.to_string(),
        ciphertext: to_hex(&ciphertext),
        epoch: conversation.epoch() as i64,
        is_commit: false,
        body: body.to_string(),
        payload: None,
        queued_at_ms: now_ms(),
        attempts: 0,
        last_error: None,
    };
    // One transaction, not two statements. The ratchet advanced when the
    // message was encrypted, so the queued ciphertext belongs to a generation
    // the stored state does not yet know about; a crash between the two writes
    // would hand that generation out twice. RFC 9420 6.3.1 is explicit about
    // the consequence, and both writes already go to the same connection.
    ctx.store
        .enqueue_with_mls_state(&item, &mls_state::encode(ctx.provider)?)?;

    match outbox::send_now(ctx, &item) {
        Ok(accepted) => {
            ctx.store.dequeue(&client_msg_id)?;
            // Our own message is not echoed back by sync in a form we can
            // decrypt -- MLS does not let a sender decrypt its own ciphertext
            // -- so the local copy is written here, keyed by the server's
            // envelope id so a later sync cannot duplicate it.
            ctx.store.insert_message(
                accepted.envelope_id,
                &conversation_id.to_string(),
                None,
                body,
                now_ms(),
            )?;
            Ok(Sent::Delivered {
                envelope_id: accepted.envelope_id,
            })
        }
        Err(error) if error.is_offline() => {
            ctx.store
                .record_attempt(&client_msg_id, &error.to_string())?;
            // Not an error to the caller. The message is written down, it will
            // go when the network returns, and telling someone their message
            // failed when it is safely queued is worse than useless.
            Ok(Sent::Queued { client_msg_id })
        }
        Err(error) => {
            ctx.store
                .record_attempt(&client_msg_id, &error.to_string())?;
            Err(error)
        }
    }
}

/// What happened to a message the user just wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// The server has it.
    Delivered {
        /// The envelope id, which is also the local message's key.
        envelope_id: i64,
    },
    /// Written to the outbox, waiting for the network.
    ///
    /// Not a failure. The UI shows it as pending, and the next flush sends it.
    Queued {
        /// Its id in the outbox.
        client_msg_id: String,
    },
}

impl Sent {
    /// The envelope id, for a message that actually reached the server.
    pub fn envelope_id(&self) -> Option<i64> {
        match self {
            Sent::Delivered { envelope_id } => Some(*envelope_id),
            Sent::Queued { .. } => None,
        }
    }
}

/// Learns about conversations this device has been invited to.
///
/// Being added to a conversation happens entirely on the server: the inviter
/// calls `add_member`, and a membership row appears. Nothing about that reaches
/// this device on its own — the Welcome that admits us to the group is inside
/// *that conversation's* envelope stream, and a stream we do not know exists is
/// a stream we never sync. Without this, an invitation is invisible: the server
/// knows, the inviter knows, and the invitee's app shows an empty list forever.
///
/// The local row is metadata only and grants no ability to read. The cursor
/// starts at 0 deliberately, because the Welcome is one of the earliest
/// envelopes in the conversation and starting anywhere later would skip past
/// the one message that makes the rest readable.
pub fn discover<T: Transport>(ctx: &Context<'_, T>) -> Result<usize, ConversationError> {
    // Titles as well as ids: a conversation this device joined before the
    // server reported its membership is already known but still nameless, and
    // skipping it outright would leave it "Unnamed" for good.
    let known: std::collections::HashMap<String, Option<String>> = ctx
        .store
        .conversations()?
        .into_iter()
        .map(|c| (c.id, c.title))
        .collect();

    // Our own handle, so a DM is named after the other person rather than
    // after whoever the server happened to list first.
    let me = ctx.store.account()?.map(|a| a.handle);

    // Conversations this device was told to forget, and how far. Without this
    // "Remove from this device" lasted until the next sync: the row was gone,
    // the server still listed us as a member, and the loop below could not
    // tell "never seen" from "deliberately removed". The chat came back within
    // seconds, empty, which is the opposite of what the button said.
    let forgotten = ctx.store.forgotten_conversations()?;

    let mut added = 0;
    for summary in ctx.transport.list_conversations()? {
        let existing = known.get(&summary.conversation_id);
        let is_new = existing.is_none();

        // A tombstone holds until something newer than the removal exists.
        // When it lifts, the conversation resumes from where it was removed
        // rather than from zero -- the confirmation promises it comes back
        // "with the new message in it", not with the history that was deleted.
        let resume = match forgotten.get(&summary.conversation_id) {
            Some(&through) => {
                if summary.latest_envelope_id.unwrap_or(0) <= through {
                    continue;
                }
                ctx.store.remember_conversation(&summary.conversation_id)?;
                through
            }
            None => 0,
        };

        if is_new {
            ctx.store
                .set_conversation_cursor(&summary.conversation_id, resume)?;
            added += 1;
        }

        // What the server knows about the shape of it. Kept so the UI can tell
        // a DM from a group without asking again -- it used to assume every
        // conversation was a DM, and looked up its title as if it were a handle.
        ctx.store.set_conversation_meta(
            &summary.conversation_id,
            &summary.kind,
            &summary.members,
        )?;

        // Name it only when it has no name. A title the user or `start_with`
        // already set is better than one derived from a member list, and this
        // runs on every sync — overwriting each time would undo a rename on
        // the next pass.
        let unnamed = existing.map(|t| t.is_none()).unwrap_or(true);
        if unnamed && let Some(title) = title_from(&summary.members, me.as_deref()) {
            ctx.store
                .set_conversation_title(&summary.conversation_id, &title)?;
        }
    }
    Ok(added)
}

/// What to call a conversation known only by its membership.
///
/// A DM is the other person. A group is everyone else, which is a placeholder
/// for a real name rather than a good one — the server holds no title, so this
/// is the most a discovering device can honestly say. `None` when there is
/// nobody but us to name it after, which leaves any existing title alone
/// rather than replacing it with something worse.
fn title_from(members: &[String], me: Option<&str>) -> Option<String> {
    let others: Vec<&str> = members
        .iter()
        .map(String::as_str)
        .filter(|h| Some(*h) != me)
        .collect();

    match others.len() {
        0 => None,
        1 => Some(others[0].to_string()),
        _ => Some(others.join(", ")),
    }
}

/// What a sync did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncOutcome {
    /// New messages written to the local history.
    pub messages: usize,
    /// Commits applied, each of which moved the epoch.
    pub commits: usize,
    /// Envelopes that could not be processed. Rule 7: counted and reported,
    /// never silently skipped.
    pub failed: usize,
    /// Envelopes that predate this device joining, and so are not its to
    /// apply.
    ///
    /// Distinct from `failed` on purpose. The commit that *adds* you arrives
    /// before the Welcome that lets you join, and you are already past it by
    /// the time you can read anything — counting that as a failure would report
    /// "a message could not be read" on every new conversation, and any alert
    /// built on `failed` would cry wolf immediately.
    pub skipped: usize,
}

/// Pulls everything new for a conversation and applies it.
///
/// Idempotent on purpose: reconnecting replays from the stored cursor, and
/// messages are keyed by envelope id, so running this twice changes nothing the
/// second time.
pub fn sync<T: Transport>(
    ctx: &Context<'_, T>,
    conversation_id: ConversationId,
) -> Result<SyncOutcome, ConversationError> {
    let id = conversation_id.to_string();
    let since = ctx.store.conversation_cursor(&id)?;
    let envelopes = ctx.transport.sync(&id, since)?;

    let mut outcome = SyncOutcome::default();
    let mut cursor = since;

    // Our own device id. The delivery service returns every envelope in the
    // conversation, ours included, and MLS cannot decrypt a message this device
    // sent -- the ratchet moves on as it encrypts. Handing our own ciphertext
    // back to `decrypt` therefore fails every time, and counted that failure as
    // "a message could not be read" for a message the sender is looking at.
    let mine = ctx.store.account()?.map(|a| a.device_id);

    // Decode once. An envelope whose hex is malformed never reached us intact,
    // and that is a genuine failure.
    let mut decoded = Vec::with_capacity(envelopes.len());
    for envelope in envelopes {
        match from_hex(&envelope.ciphertext) {
            Ok(bytes) => decoded.push((envelope, bytes)),
            Err(_) => {
                outcome.failed += 1;
            }
        }
    }

    // First pass: joins.
    //
    // A Welcome later in the batch is what makes the batch processable, so it
    // has to be handled before anything that depends on membership. Joining
    // also fixes the epoch: everything at or before the Welcome is history this
    // device was never part of, which MLS is explicit about — a member added at
    // epoch N cannot read anything before N.
    let mut joined_at: Option<i64> = None;
    for (envelope, bytes) in &decoded {
        if matches!(mls::peek(bytes), Ok(Peeked::Welcome))
            && Conversation::join(ctx.provider, bytes, now_ms()).is_ok()
        {
            joined_at = Some(envelope.envelope_id);
        }
    }

    // Second pass: everything else.
    for (envelope, bytes) in &decoded {
        cursor = cursor.max(envelope.envelope_id);

        // Ours already: `send` writes to local history the moment the server
        // accepts, so there is nothing here to learn and nothing to report.
        if mine.as_deref() == Some(envelope.sender_device_id.as_str()) {
            continue;
        }

        // Anything up to and including the Welcome predates membership.
        if joined_at.is_some_and(|at| envelope.envelope_id <= at) {
            outcome.skipped += 1;
            continue;
        }

        match mls::peek(bytes) {
            // A Welcome we did not join from: either already a member, or an
            // invitation to a group this device is not the target of.
            Ok(Peeked::Welcome) => outcome.skipped += 1,

            Ok(Peeked::GroupMessage) => {
                match Conversation::load(ctx.provider, conversation_id, now_ms())? {
                    Some(mut conversation) => match conversation.decrypt(ctx.provider, bytes) {
                        Ok(Incoming::Message { sender, plaintext }) => {
                            // What is stored is the preview. The full payload —
                            // including an attachment's key — stays inside the
                            // ciphertext until someone asks to open the file.
                            let payload = Payload::decode(&plaintext);

                            // Neither of these is a message: they change what
                            // the conversation is called or looks like, and
                            // leave no bubble behind.
                            if let Payload::Rename { title } = &payload {
                                ctx.store.set_conversation_title(&id, title)?;
                                continue;
                            }
                            if matches!(payload, Payload::GroupAvatar { .. }) {
                                // The payload is kept, not the picture: it
                                // holds the key, and the bytes are fetched when
                                // something actually needs to draw them.
                                ctx.store
                                    .set_conversation_avatar(&id, &payload.encode_string())?;
                                continue;
                            }

                            let body = payload.preview().to_string();
                            // Written down now or never: MLS will not decrypt
                            // this envelope a second time.
                            let stored = match &payload {
                                Payload::Attachment { .. } => Some(payload.encode_string()),
                                // Text needs nothing beyond the body already in
                                // `body`; a future variant that does will fail
                                // to open its file until it is added here, which
                                // is the safe direction for this to go wrong.
                                _ => None,
                            };
                            ctx.store.insert_message_with_payload(
                                envelope.envelope_id,
                                &id,
                                sender.map(|s| s.to_string()).as_deref(),
                                &body,
                                stored.as_deref(),
                                envelope.server_timestamp_ms,
                            )?;
                            outcome.messages += 1;
                        }
                        Ok(Incoming::CommitApplied { .. }) => outcome.commits += 1,
                        Ok(Incoming::ProposalQueued) => {}
                        // `Incoming` is non-exhaustive; an unrecognised variant
                        // is counted rather than assumed harmless.
                        Ok(_) => outcome.failed += 1,
                        // Rule 7: a message that cannot be decrypted is counted,
                        // never skipped silently and never shown as plaintext.
                        Err(_) => outcome.failed += 1,
                    },
                    // A message for a group this device is not in. Not a
                    // failure to read — there is nothing here to read it with.
                    None => outcome.skipped += 1,
                }
            }

            // `Peeked` is non-exhaustive, so a future variant lands here rather
            // than failing to compile in a build nobody expected to touch this.
            Ok(_) | Err(_) => outcome.failed += 1,
        }
    }

    ctx.store.set_conversation_cursor(&id, cursor)?;
    mls_state::save(ctx.provider, ctx.store)?;

    Ok(outcome)
}

/// Renames a conversation, for everyone in it.
///
/// Sent as an ordinary encrypted message rather than written to the server:
/// what people call their group is content, and the server holds no title
/// column to leak. Every member applies it when they sync, so the name
/// converges without anyone having to agree on who owns it.
///
/// Applied locally only after the delivery service accepted it — the ordering
/// rule at the top of this module, for the same reason.
pub fn rename<T: Transport>(
    ctx: &Context<'_, T>,
    conversation_id: ConversationId,
    title: &str,
) -> Result<(), ConversationError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(ConversationError::NotAMember);
    }

    let id = conversation_id.to_string();
    let payload = Payload::Rename {
        title: title.to_string(),
    };

    let mut conversation = Conversation::load(ctx.provider, conversation_id, now_ms())?
        .ok_or(ConversationError::NotAMember)?;
    let ciphertext = conversation.encrypt(ctx.provider, ctx.signer, &payload.encode())?;

    ctx.transport.send(
        &id,
        &to_hex(&ciphertext),
        conversation.epoch() as i64,
        false,
        &outbox::new_message_id(),
    )?;

    ctx.store.set_conversation_title(&id, title)?;
    mls_state::save(ctx.provider, ctx.store)?;
    Ok(())
}

/// Sets the conversation's picture, for everyone in it.
///
/// The bytes are encrypted before they are uploaded and the key travels inside
/// an MLS message, exactly as an attachment does — so the bucket holds
/// ciphertext and the server never sees the picture. It is sent before it is
/// applied locally, which is the ordering rule at the top of this module.
pub fn set_group_avatar<T: Transport>(
    ctx: &Context<'_, T>,
    conversation_id: ConversationId,
    contents: &[u8],
    mime: &str,
) -> Result<(), ConversationError> {
    let sealed = nexo_crypto::attachment::encrypt(contents)?;

    let id = conversation_id.to_string();
    let (url, s3_key) = ctx
        .transport
        .upload_url(&id, sealed.ciphertext.len() as u64)?;
    ctx.transport.put_object(&url, sealed.ciphertext)?;

    let payload = Payload::GroupAvatar {
        s3_key,
        key: to_hex(sealed.key.as_slice()),
        nonce: to_hex(&sealed.nonce),
        sha256: to_hex(&sealed.sha256),
        mime: mime.to_string(),
        size: sealed.size,
    };

    let mut conversation = Conversation::load(ctx.provider, conversation_id, now_ms())?
        .ok_or(ConversationError::NotAMember)?;
    let ciphertext = conversation.encrypt(ctx.provider, ctx.signer, &payload.encode())?;

    ctx.transport.send(
        &id,
        &to_hex(&ciphertext),
        conversation.epoch() as i64,
        false,
        &outbox::new_message_id(),
    )?;

    ctx.store
        .set_conversation_avatar(&id, &payload.encode_string())?;
    mls_state::save(ctx.provider, ctx.store)?;
    Ok(())
}

/// The conversation's picture, decrypted.
///
/// `None` when it has none. The payload holding the key never leaves the
/// encrypted store; only the bytes come back.
pub fn group_avatar<T: Transport>(
    ctx: &Context<'_, T>,
    conversation_id: ConversationId,
) -> Result<Option<(Vec<u8>, String)>, ConversationError> {
    let Some(encoded) = ctx
        .store
        .conversation_avatar(&conversation_id.to_string())?
    else {
        return Ok(None);
    };
    let payload = Payload::decode(encoded.as_bytes());
    let Payload::GroupAvatar { mime, .. } = &payload else {
        return Ok(None);
    };
    let mime = mime.clone();
    let contents = fetch_attachment(ctx, &payload)?;
    Ok(Some((contents, mime)))
}

/// Sends a file.
///
/// Brief 5.3, in order: encrypt with a fresh key, upload the **ciphertext**,
/// then put the key inside the MLS message. The bucket receives bytes the
/// server cannot read, and the only copy of the key that opens them travels
/// end-to-end.
///
/// The upload happens before the message for the same reason every commit is
/// sent before it is applied: a message announcing a file that failed to upload
/// is a message pointing at nothing, and the recipient has no way to tell that
/// from a file they simply cannot fetch yet.
pub fn send_attachment<T: Transport>(
    ctx: &Context<'_, T>,
    conversation_id: ConversationId,
    name: &str,
    mime: &str,
    contents: &[u8],
    body: Option<&str>,
) -> Result<i64, ConversationError> {
    let sealed = nexo_crypto::attachment::encrypt(contents)?;

    let id = conversation_id.to_string();
    let (url, s3_key) = ctx
        .transport
        .upload_url(&id, sealed.ciphertext.len() as u64)?;
    ctx.transport.put_object(&url, sealed.ciphertext)?;

    let payload = Payload::Attachment {
        s3_key,
        key: to_hex(sealed.key.as_slice()),
        nonce: to_hex(&sealed.nonce),
        sha256: to_hex(&sealed.sha256),
        name: name.to_string(),
        mime: mime.to_string(),
        size: sealed.size,
        body: body.map(str::to_string),
    };

    let mut conversation = Conversation::load(ctx.provider, conversation_id, now_ms())?
        .ok_or(ConversationError::NotAMember)?;
    let ciphertext = conversation.encrypt(ctx.provider, ctx.signer, &payload.encode())?;

    let accepted = ctx.transport.send(
        &id,
        &to_hex(&ciphertext),
        conversation.epoch() as i64,
        false,
        &outbox::new_message_id(),
    )?;

    // With the payload, exactly as `sync` stores an arriving one. Without it
    // the sender's own copy is a line of text naming a file: the message list
    // builds its attachment view from the payload, so a message stored without
    // one has no picture to draw and no key to open the file with. The
    // recipient saw the image; the person who sent it did not.
    ctx.store.insert_message_with_payload(
        accepted.envelope_id,
        &id,
        None,
        payload.preview(),
        Some(&payload.encode_string()),
        now_ms(),
    )?;
    mls_state::save(ctx.provider, ctx.store)?;

    Ok(accepted.envelope_id)
}

/// Downloads and decrypts the attachment on a stored message.
///
/// Takes an envelope id because that is what a message list has. The payload
/// -- and with it the only copy of the file's key -- comes from the encrypted
/// store, where `sync` wrote it when the message arrived.
pub fn fetch_attachment_by_id<T: Transport>(
    ctx: &Context<'_, T>,
    envelope_id: i64,
) -> Result<Attachment, ConversationError> {
    let encoded = ctx
        .store
        .message_payload(envelope_id)?
        .ok_or(ConversationError::NotAnAttachment)?;
    let payload = Payload::decode(encoded.as_bytes());
    let Payload::Attachment { name, mime, .. } = &payload else {
        return Err(ConversationError::NotAnAttachment);
    };
    let (name, mime) = (name.clone(), mime.clone());
    let contents = fetch_attachment(ctx, &payload)?;
    Ok(Attachment {
        name,
        mime,
        contents,
    })
}

/// A decrypted attachment, ready to be written to disk or shown.
#[derive(Debug)]
pub struct Attachment {
    /// The sender's filename. Untrusted -- see `nexo_protocol::safe_file_name`.
    pub name: String,
    /// The sender's declared type. Also untrusted.
    pub mime: String,
    /// The verified plaintext.
    pub contents: Vec<u8>,
}

/// Downloads and decrypts an attachment.
///
/// Both checks in `attachment::decrypt` apply: GCM's tag catches a ciphertext
/// altered in the bucket or in transit, and the SHA-256 catches an upload that
/// disagrees with the message describing it.
pub fn fetch_attachment<T: Transport>(
    ctx: &Context<'_, T>,
    payload: &Payload,
) -> Result<Vec<u8>, ConversationError> {
    // A group picture is encrypted the same way and read the same way; only
    // what it is attached to differs.
    let (s3_key, key, nonce, sha256) = match payload {
        Payload::Attachment {
            s3_key,
            key,
            nonce,
            sha256,
            ..
        }
        | Payload::GroupAvatar {
            s3_key,
            key,
            nonce,
            sha256,
            ..
        } => (s3_key, key, nonce, sha256),
        _ => return Err(ConversationError::NotAnAttachment),
    };

    let url = ctx.transport.download_url(s3_key)?;
    let ciphertext = ctx.transport.get_object(&url)?;

    let plaintext = nexo_crypto::attachment::decrypt(
        &ciphertext,
        &from_hex(key)?,
        &from_hex(nonce)?,
        &from_hex(sha256)?,
    )?;
    Ok(plaintext.to_vec())
}

/// The safety number for a 1:1 conversation, for the Verify screen.
///
/// `None` when the group does not have exactly two members: a safety number is
/// a fingerprint over *both* parties, and there is no meaningful one to show
/// for a group of five.
pub fn safety_number(
    provider: &OpenMlsRustCrypto,
    conversation_id: ConversationId,
) -> Result<Option<String>, ConversationError> {
    let Some(conversation) = Conversation::load(provider, conversation_id, now_ms())? else {
        return Ok(None);
    };
    let keys = conversation.member_identity_keys();
    if keys.len() != 2 {
        return Ok(None);
    }
    let number = nexo_crypto::identity::SafetyNumber::new(&keys[0], &keys[1])?;
    Ok(Some(number.to_display_string()))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit(u32::from(b >> 4), 16).unwrap());
        out.push(char::from_digit(u32::from(b & 0x0F), 16).unwrap());
    }
    out
}

fn from_hex(s: &str) -> Result<Vec<u8>, ConversationError> {
    if !s.len().is_multiple_of(2) {
        return Err(TransportError::Rejected("not hex".into()).into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| TransportError::Rejected("not hex".into()).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
        assert_eq!(from_hex("000fa5ff").unwrap(), vec![0x00, 0x0f, 0xa5, 0xff]);
    }

    #[test]
    fn odd_length_hex_is_refused_rather_than_truncated() {
        assert!(from_hex("abc").is_err());
        assert!(from_hex("zz").is_err());
    }

    #[test]
    fn a_sync_outcome_starts_empty() {
        let outcome = SyncOutcome::default();
        assert_eq!(outcome.messages, 0);
        assert_eq!(outcome.commits, 0);
        assert_eq!(outcome.failed, 0);
    }
}
