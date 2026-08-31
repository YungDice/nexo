//! Conversations: the MLS group, wrapped in the shape Nexo actually uses.
//!
//! One MLS group per conversation, and a 1:1 chat is a two-member group with no
//! special-casing (brief 4.2). This module exists so that nothing above it ever
//! touches an OpenMLS type directly — the API here is `encrypt`, `decrypt`,
//! `rekey`, `add_member`, `remove_member`, and the honest errors that go with
//! them.
//!
//! # The signing key is the identity key
//!
//! [`credential_for`] builds the MLS credential from the same
//! [`IdentityKeypair`] that safety numbers are computed over. That is not a
//! convenience: if MLS signed with a *different* key, a safety number two
//! people compared would be verifying something that authenticates nothing, and
//! the whole ceremony would be theatre. The key that signs commits is the key
//! in the fingerprint.
//!
//! # No I/O, no clock
//!
//! Storage arrives as a provider and the current time arrives as an argument
//! (brief 12). Nothing here reads a clock or a disk, so the crate compiles
//! unchanged for Android and every rekey decision is reproducible in a test.

use nexo_protocol::{ConversationId, DeviceId};
use openmls::prelude::{tls_codec::*, *};
use openmls_basic_credential::SignatureKeyPair;

use crate::identity::IdentityKeypair;
use crate::{CIPHERSUITE, CryptoError, group_create_config};

/// Builds the MLS credential and signer for a device.
///
/// The credential names the **device**, not the user: the MLS group member is
/// the device (brief 4.2), so a second device later is an added member rather
/// than a schema change. Mapping a device back to an account is the store's
/// job, not MLS's.
pub fn credential_for(
    device_id: DeviceId,
    identity: &IdentityKeypair,
) -> (CredentialWithKey, SignatureKeyPair) {
    let public = identity.public_bytes();
    let signer = SignatureKeyPair::from_raw(
        CIPHERSUITE.signature_algorithm(),
        identity.secret_bytes().to_vec(),
        public.to_vec(),
    );
    let credential = CredentialWithKey {
        credential: BasicCredential::new(device_id.as_bytes().to_vec()).into(),
        signature_key: public.to_vec().into(),
    };
    (credential, signer)
}

/// Generates a batch of KeyPackages to publish.
///
/// Brief 4.2: 50 on registration, refilled when the server reports fewer than
/// 15 left. Each one is single-use — an invite consumes it — so running out
/// means nobody can start a conversation with you until you top up.
pub fn generate_key_packages<P: OpenMlsProvider>(
    provider: &P,
    signer: &SignatureKeyPair,
    credential: CredentialWithKey,
    count: usize,
) -> Result<Vec<Vec<u8>>, CryptoError> {
    (0..count)
        .map(|_| {
            let bundle = KeyPackage::builder()
                .build(CIPHERSUITE, provider, signer, credential.clone())
                .map_err(|e| CryptoError::Mls(format!("building a key package: {e}")))?;
            bundle
                .key_package()
                .tls_serialize_detached()
                .map_err(|e| CryptoError::Mls(format!("serialising a key package: {e}")))
        })
        .collect()
}

/// A commit that has been created and staged, but not yet applied.
///
/// Staged rather than applied because **a commit can lose**. The delivery
/// service orders commits and the first writer wins (PLAN.md risk 4(b)); a
/// client that merged its own commit optimistically would believe it had moved
/// to an epoch nobody else is in, and every message it sent afterwards would be
/// undecryptable to everyone.
///
/// So: send [`Commit::message`], and then call
/// [`Conversation::confirm_commit`] if the server accepted it or
/// [`Conversation::abandon_commit`] if it did not.
#[derive(Debug, Clone)]
pub struct Commit {
    /// The commit to hand to the delivery service.
    pub message: Vec<u8>,
    /// The Welcome for a newly added member, if this commit added one.
    pub welcome: Option<Vec<u8>>,
}

/// What came out of [`Conversation::decrypt`].
#[derive(Debug)]
#[non_exhaustive]
pub enum Incoming {
    /// A message, already decrypted.
    Message {
        /// The device that sent it.
        sender: Option<DeviceId>,
        /// The plaintext.
        plaintext: Vec<u8>,
    },
    /// A commit that was applied. The epoch has moved.
    CommitApplied {
        /// The epoch now in force.
        epoch: u64,
    },
    /// A proposal was queued, awaiting the commit that carries it.
    ProposalQueued,
}

/// What an envelope turns out to hold, without processing it.
///
/// A client syncing a conversation it has just been invited to receives the
/// Welcome as an ordinary envelope — there is no separate endpoint for it, and
/// there does not need to be: the invitee is already a member server-side, so
/// the conversation's own stream is the delivery path. This is how the client
/// tells "join this" from "decrypt this" before committing to either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Peeked {
    /// An invitation. Hand it to [`Conversation::join`].
    Welcome,
    /// A message or commit for a group this device is already in.
    GroupMessage,
    /// Something this build does not handle.
    Other,
}

/// Looks at an envelope's shape without decrypting or applying anything.
pub fn peek(bytes: &[u8]) -> Result<Peeked, CryptoError> {
    let message =
        MlsMessageIn::tls_deserialize_exact(bytes).map_err(|_| CryptoError::Undecryptable)?;
    Ok(match message.extract() {
        MlsMessageBodyIn::Welcome(_) => Peeked::Welcome,
        MlsMessageBodyIn::PrivateMessage(_) | MlsMessageBodyIn::PublicMessage(_) => {
            Peeked::GroupMessage
        }
        _ => Peeked::Other,
    })
}

/// One conversation's MLS group, plus the state the rekey policy needs.
pub struct Conversation {
    group: MlsGroup,
    sent_since_rekey: u64,
    last_rekey_ms: i64,
}

impl std::fmt::Debug for Conversation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print group state: it is key material in a trench coat.
        f.debug_struct("Conversation")
            .field("epoch", &self.group.epoch().as_u64())
            .field("members", &self.group.members().count())
            .finish_non_exhaustive()
    }
}

/// One member of a group: where they sit, who they are, and what signs for them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberInfo {
    /// Position in the ratchet tree. What `remove_member` takes.
    pub leaf_index: u32,
    /// The device this member is. MLS names devices, not accounts.
    pub device_id: DeviceId,
    /// The key that signs this member's messages.
    pub signature_key: Vec<u8>,
}

impl Conversation {
    /// Creates a new conversation with this device as its only member.
    ///
    /// The MLS group id *is* the conversation id, so there is never a mapping
    /// table between the two and never a chance of them disagreeing.
    pub fn create<P: OpenMlsProvider>(
        provider: &P,
        signer: &SignatureKeyPair,
        credential: CredentialWithKey,
        conversation_id: ConversationId,
        now_ms: i64,
    ) -> Result<Self, CryptoError> {
        let group = MlsGroup::new_with_group_id(
            provider,
            signer,
            &group_create_config(),
            GroupId::from_slice(conversation_id.as_bytes()),
            credential,
        )
        .map_err(|e| CryptoError::Mls(format!("creating a group: {e}")))?;
        Ok(Self {
            group,
            sent_since_rekey: 0,
            last_rekey_ms: now_ms,
        })
    }

    /// Joins a conversation from a Welcome that arrived over the wire.
    pub fn join<P: OpenMlsProvider>(
        provider: &P,
        welcome_bytes: &[u8],
        now_ms: i64,
    ) -> Result<Self, CryptoError> {
        let message = MlsMessageIn::tls_deserialize_exact(welcome_bytes)
            .map_err(|e| CryptoError::Mls(format!("reading a welcome: {e}")))?;
        let welcome = match message.extract() {
            MlsMessageBodyIn::Welcome(w) => w,
            _ => return Err(CryptoError::UnexpectedMessage),
        };
        let config = group_create_config();
        let group = StagedWelcome::new_from_welcome(provider, config.join_config(), welcome, None)
            .map_err(|e| CryptoError::Mls(format!("staging a welcome: {e}")))?
            .into_group(provider)
            .map_err(|e| CryptoError::Mls(format!("joining from a welcome: {e}")))?;
        Ok(Self {
            group,
            sent_since_rekey: 0,
            last_rekey_ms: now_ms,
        })
    }

    /// Restores a conversation from storage.
    ///
    /// `Ok(None)` means this device is not in that conversation — a normal
    /// answer, not a failure.
    ///
    /// The rekey counters are **not** restored, because they are not MLS state:
    /// they start again from zero and from `now_ms`. The effect is that a
    /// restart can delay a rekey by up to the policy interval, which is
    /// acceptable — and far better than the alternative of persisting a
    /// counter that could drift out of step with the epoch it describes.
    pub fn load<P: OpenMlsProvider>(
        provider: &P,
        conversation_id: ConversationId,
        now_ms: i64,
    ) -> Result<Option<Self>, CryptoError> {
        let group = MlsGroup::load(
            provider.storage(),
            &GroupId::from_slice(conversation_id.as_bytes()),
        )
        .map_err(|e| CryptoError::Mls(format!("loading a group: {e}")))?;

        Ok(group.map(|group| Self {
            group,
            sent_since_rekey: 0,
            last_rekey_ms: now_ms,
        }))
    }

    /// The epoch this conversation is in.
    pub fn epoch(&self) -> u64 {
        self.group.epoch().as_u64()
    }

    /// The conversation id, which is the MLS group id.
    pub fn conversation_id(&self) -> Option<ConversationId> {
        let bytes = self.group.group_id().as_slice();
        let array: [u8; 16] = bytes.try_into().ok()?;
        Some(ConversationId::from_bytes(array))
    }

    /// How many members are in the group.
    pub fn member_count(&self) -> usize {
        self.group.members().count()
    }

    /// The identity public keys of every member, for safety numbers.
    ///
    /// These are the keys that sign this group's messages, which is what makes
    /// a fingerprint over them worth comparing.
    pub fn member_identity_keys(&self) -> Vec<Vec<u8>> {
        self.group
            .members()
            .map(|m| m.signature_key.to_vec())
            .collect()
    }

    /// Every member, with the leaf they sit at and the device they are.
    ///
    /// [`member_identity_keys`](Self::member_identity_keys) answers "what keys
    /// sign here", which is all a safety number needs. This answers "who is
    /// here", which is what two other things need and cannot get anywhere else:
    ///
    /// - **Noticing a key change.** Comparing keys alone cannot tell a member
    ///   whose key changed from a member who left and another who joined. The
    ///   device id is what makes the comparison per-person.
    /// - **Removing someone.** `remove_member` takes a leaf index, and every
    ///   caller above this has a handle. The credential carries the device id
    ///   (see [`credential_for`]), and the server maps handles to devices, so
    ///   this is the half that closes the gap.
    ///
    /// A credential that is not a `BasicCredential`, or whose identity is not a
    /// device id, is skipped rather than guessed at: it is not a member this
    /// build put there, and inventing an identity for it would be worse than
    /// omitting it.
    pub fn members(&self) -> Vec<MemberInfo> {
        self.group
            .members()
            .filter_map(|m| {
                let credential = BasicCredential::try_from(m.credential).ok()?;
                let device_id = DeviceId::from_slice(credential.identity()).ok()?;
                Some(MemberInfo {
                    leaf_index: m.index.u32(),
                    device_id,
                    signature_key: m.signature_key.to_vec(),
                })
            })
            .collect()
    }

    /// Encrypts a message. The bytes returned go straight into an
    /// [`Envelope`](nexo_protocol::Envelope) as `ciphertext`.
    pub fn encrypt<P: OpenMlsProvider>(
        &mut self,
        provider: &P,
        signer: &SignatureKeyPair,
        plaintext: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let out = self
            .group
            .create_message(provider, signer, plaintext)
            .map_err(|e| CryptoError::Mls(format!("encrypting: {e}")))?;
        self.sent_since_rekey += 1;
        out.to_bytes()
            .map_err(|e| CryptoError::Mls(format!("serialising a message: {e}")))
    }

    /// Decrypts whatever arrived, and applies it if it is a commit.
    ///
    /// Rule 7 is the whole shape of this function: a message that cannot be
    /// decrypted returns an error the caller must show. There is no plaintext
    /// fallback and no silent skip.
    pub fn decrypt<P: OpenMlsProvider>(
        &mut self,
        provider: &P,
        ciphertext: &[u8],
    ) -> Result<Incoming, CryptoError> {
        let message = MlsMessageIn::tls_deserialize_exact(ciphertext)
            .map_err(|_| CryptoError::Undecryptable)?;
        let protocol = message
            .try_into_protocol_message()
            .map_err(|_| CryptoError::UnexpectedMessage)?;

        let processed = self
            .group
            .process_message(provider, protocol)
            .map_err(|e| classify(e, self.epoch()))?;

        let sender = processed.credential().serialized_content();
        let sender = <[u8; 16]>::try_from(sender).ok().map(DeviceId::from_bytes);

        match processed.into_content() {
            ProcessedMessageContent::ApplicationMessage(m) => Ok(Incoming::Message {
                sender,
                plaintext: m.into_bytes(),
            }),
            ProcessedMessageContent::StagedCommitMessage(commit) => {
                self.group
                    .merge_staged_commit(provider, *commit)
                    .map_err(|e| CryptoError::Mls(format!("merging a commit: {e}")))?;
                // A commit resets the rekey clock on this side too: the epoch
                // moved, which is exactly what a rekey is for.
                self.sent_since_rekey = 0;
                Ok(Incoming::CommitApplied {
                    epoch: self.epoch(),
                })
            }
            ProcessedMessageContent::ProposalMessage(proposal) => {
                self.group
                    .store_pending_proposal(provider.storage(), *proposal)
                    .map_err(|e| CryptoError::Mls(format!("storing a proposal: {e}")))?;
                Ok(Incoming::ProposalQueued)
            }
            ProcessedMessageContent::ExternalJoinProposalMessage(_) => {
                // v0.1 has no external joins: every member arrives by invitation.
                Err(CryptoError::UnexpectedMessage)
            }
        }
    }

    /// Proposes an Update commit — the rekey of brief 4.2.
    ///
    /// The commit is **staged, not applied**. See [`Commit`].
    pub fn rekey<P: OpenMlsProvider>(
        &mut self,
        provider: &P,
        signer: &SignatureKeyPair,
    ) -> Result<Commit, CryptoError> {
        let (commit, _welcome, _info) = self
            .group
            .self_update(provider, signer, LeafNodeParameters::default())
            .map_err(|e| CryptoError::Mls(format!("rekeying: {e}")))?
            .into_contents();
        Ok(Commit {
            message: commit
                .to_bytes()
                .map_err(|e| CryptoError::Mls(format!("serialising a commit: {e}")))?,
            welcome: None,
        })
    }

    /// Applies the commit this client created, after the delivery service
    /// accepted it.
    pub fn confirm_commit<P: OpenMlsProvider>(
        &mut self,
        provider: &P,
        now_ms: i64,
    ) -> Result<u64, CryptoError> {
        self.group
            .merge_pending_commit(provider)
            .map_err(|e| CryptoError::Mls(format!("applying a commit: {e}")))?;
        self.sent_since_rekey = 0;
        self.last_rekey_ms = now_ms;
        Ok(self.epoch())
    }

    /// Discards the commit this client created, because it lost the race.
    ///
    /// The caller then processes the winning commit through [`Self::decrypt`]
    /// and, if it still wants whatever its own commit was for, builds a new one
    /// against the new epoch. That is the "resync and rebuild" of risk 4(b).
    pub fn abandon_commit<P: OpenMlsProvider>(&mut self, provider: &P) -> Result<(), CryptoError> {
        self.group
            .clear_pending_commit(provider.storage())
            .map_err(|e| CryptoError::Mls(format!("clearing a commit: {e}")))
    }

    /// Adds a member from their published KeyPackage.
    ///
    /// Returns the commit for existing members and the Welcome for the new one.
    /// The new member sees messages from this epoch forward and **never** the
    /// history before it — that is MLS doing its job, and M5's check.
    pub fn add_member<P: OpenMlsProvider>(
        &mut self,
        provider: &P,
        signer: &SignatureKeyPair,
        key_package_bytes: &[u8],
    ) -> Result<Commit, CryptoError> {
        let key_package = KeyPackageIn::tls_deserialize_exact(key_package_bytes)
            .map_err(|e| CryptoError::Mls(format!("reading a key package: {e}")))?
            .validate(provider.crypto(), ProtocolVersion::Mls10)
            .map_err(|e| CryptoError::Mls(format!("validating a key package: {e}")))?;

        let (commit, welcome, _info) = self
            .group
            .add_members(provider, signer, &[key_package])
            .map_err(|e| CryptoError::Mls(format!("adding a member: {e}")))?;

        // An add is a commit, so confirming it also rekeys (brief 4.2: "always
        // on member add or remove"). Staged, like every other commit.
        Ok(Commit {
            message: commit
                .to_bytes()
                .map_err(|e| CryptoError::Mls(format!("serialising a commit: {e}")))?,
            welcome: Some(
                welcome
                    .to_bytes()
                    .map_err(|e| CryptoError::Mls(format!("serialising a welcome: {e}")))?,
            ),
        })
    }

    /// Removes a member by their leaf index.
    pub fn remove_member<P: OpenMlsProvider>(
        &mut self,
        provider: &P,
        signer: &SignatureKeyPair,
        index: u32,
    ) -> Result<Commit, CryptoError> {
        let (commit, _welcome, _info) = self
            .group
            .remove_members(provider, signer, &[LeafNodeIndex::new(index)])
            .map_err(|e| CryptoError::Mls(format!("removing a member: {e}")))?;
        Ok(Commit {
            message: commit
                .to_bytes()
                .map_err(|e| CryptoError::Mls(format!("serialising a commit: {e}")))?,
            welcome: None,
        })
    }

    /// Whether the rekey policy says it is time.
    pub fn needs_rekey(&self, now_ms: i64) -> bool {
        rekey_due(self.sent_since_rekey, self.last_rekey_ms, now_ms)
    }

    /// Messages sent since the last epoch change. Exposed for tests and for a
    /// caller that wants to show the policy rather than just obey it.
    pub fn sent_since_rekey(&self) -> u64 {
        self.sent_since_rekey
    }
}

/// Turns an OpenMLS processing failure into something a caller can act on.
///
/// The one distinction that matters to the rest of the system is a **stale
/// epoch**: it is not corruption and not an attack, it is two commits racing,
/// and the answer is to resync and rebuild (PLAN.md risk 4(b)). Everything else
/// collapses to "could not decrypt", because a caller cannot do anything
/// different with the detail and rule 7 says fail closed either way.
fn classify<S>(error: ProcessMessageError<S>, current_epoch: u64) -> CryptoError {
    let is_wrong_epoch = matches!(
        &error,
        ProcessMessageError::ValidationError(ValidationError::WrongEpoch)
    );
    if is_wrong_epoch {
        return CryptoError::StaleEpoch {
            current: current_epoch,
        };
    }
    CryptoError::Undecryptable
}

/// The rekey policy from brief 4.2: every 100 messages sent or every 7 days,
/// whichever comes first.
///
/// A free function over explicit inputs rather than a method reading a clock,
/// so the boundary conditions are testable without waiting a week.
pub fn rekey_due(sent_since_rekey: u64, last_rekey_ms: i64, now_ms: i64) -> bool {
    if sent_since_rekey >= crate::REKEY_EVERY_N_MESSAGES {
        return true;
    }
    let interval_ms = (crate::REKEY_INTERVAL_DAYS as i64) * 24 * 60 * 60 * 1000;
    // A clock that went backwards must not be read as "no rekey needed
    // forever"; saturating keeps the answer false rather than wrapping into a
    // huge positive.
    now_ms.saturating_sub(last_rekey_ms) >= interval_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_MS: i64 = 24 * 60 * 60 * 1000;

    #[test]
    fn a_hundred_messages_is_due_and_ninety_nine_is_not() {
        assert!(!rekey_due(99, 0, 0));
        assert!(rekey_due(100, 0, 0));
        assert!(rekey_due(101, 0, 0));
    }

    #[test]
    fn seven_days_is_due_and_six_is_not() {
        assert!(!rekey_due(0, 0, 6 * DAY_MS));
        assert!(rekey_due(0, 0, 7 * DAY_MS));
    }

    #[test]
    fn either_trigger_alone_is_enough() {
        // "whichever comes first".
        assert!(rekey_due(100, 0, 0));
        assert!(rekey_due(0, 0, 7 * DAY_MS));
    }

    #[test]
    fn a_backwards_clock_does_not_disable_rekeying() {
        // If the subtraction wrapped, this would come out enormous and report
        // "due" forever; if it saturated the wrong way it would report "never".
        // Neither is acceptable, and the message counter still works.
        assert!(!rekey_due(0, 10 * DAY_MS, 0));
        assert!(rekey_due(100, 10 * DAY_MS, 0));
    }
}
