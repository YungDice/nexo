//! Stories: encrypted once, and the key handed to every contact.
//!
//! # Why this shape rather than a story group
//!
//! A group per author, with the author's contacts as members, was the obvious
//! design and is the wrong one. Four reasons, and the first is decisive:
//!
//! **Blocking would leak.** `delivery/mod.rs` applies `blocked_between` only to
//! conversations with exactly two members, and its own comment explains why
//! widening it to groups would break the group. A story group is multi-member,
//! so somebody who blocked the author would go on receiving stories until an
//! explicit removal commit landed. Sending down the conversations that already
//! exist inherits the check that already works: a blocked person has no
//! deliverable conversation left, so there is nothing to inherit *from*.
//!
//! **A group needs a commit per audience change** — every new contact, every
//! block — and consumes KeyPackages across the whole contact graph. That is a
//! permanently maintained group lifetime for an object that lasts a day.
//!
//! **Eight places would have to learn that a conversation is not always a
//! chat**: the list, search, titles, the attachment strip, and so on. This
//! design teaches none of them: a story writes to its own table and ends the
//! receive branch with `continue`, exactly as `Rename` does.
//!
//! **"Contacts" already exists.** The server defines it as *shares at least one
//! conversation with you*, and those conversations are the delivery route. No
//! follower graph, which `docs/PLAN.md` lists as a non-goal.
//!
//! # The price, stated plainly
//!
//! The author sends N payloads for N contacts, and the server sees N envelopes
//! — the same metadata it sees for messages anyway, and no more. It still
//! cannot read any of them.

use nexo_protocol::{ConversationId, Payload};

use nexo_store::StoredStory;

use crate::conversations::{Context, ConversationError};
use crate::transport::Transport;

/// Post a story to every conversation this device has.
///
/// The order matters and mirrors attachments: encrypt, upload, record, then
/// tell people. A failure before the last step leaves an object nobody has the
/// key to, which expires on its own; the other order would hand out a key to
/// something that was never uploaded.
///
/// One encryption, one object, one key — the key is what is copied, not the
/// bytes. `crates/crypto/src/attachment.rs` already does exactly this, and a
/// story is an attachment with an expiry date.
pub fn post<T: Transport>(
    ctx: &Context<'_, T>,
    contents: &[u8],
    mime: &str,
    now_ms: i64,
) -> Result<i64, ConversationError> {
    let sealed = nexo_crypto::attachment::encrypt(contents)?;

    // A story has no conversation, so it does not go through the attachment
    // route. The first version borrowed a random conversation id for the key
    // path and could never have worked: that route checks membership, and
    // nobody is a member of a conversation that does not exist. It also put
    // stories under `enc/`, where no lifecycle rule can reach them without
    // reaching every attachment too.
    let (url, s3_key) = ctx
        .transport
        .story_upload_url(sealed.ciphertext.len() as u64)?;
    let size = sealed.ciphertext.len() as i64;
    ctx.transport.put_object(&url, sealed.ciphertext)?;

    let summary = ctx.transport.create_story(&s3_key, size)?;

    let payload = Payload::Story {
        // The reader's only route to the bytes. Without it the receiving side
        // has nothing the download route recognises -- see `Payload::Story`.
        story_id: summary.id,
        s3_key: s3_key.clone(),
        key: hex(sealed.key.as_slice()),
        nonce: hex(&sealed.nonce),
        sha256: hex(&sealed.sha256),
        mime: mime.to_string(),
        size: contents.len() as u64,
        expires_at_ms: summary.expires_at_ms,
    };

    // Our own copy first, so the author sees it whatever the fan-out does.
    ctx.store.insert_story(&StoredStory {
        id: summary.id,
        author_handle: summary.author_handle.clone(),
        // Our own: the device is this one, and the server told us the handle.
        author_device_id: String::new(),
        s3_key,
        enc_key: hex(sealed.key.as_slice()),
        nonce: hex(&sealed.nonce),
        sha256: hex(&sealed.sha256),
        mime: mime.to_string(),
        size: contents.len() as i64,
        created_at_ms: summary.created_at_ms,
        expires_at_ms: summary.expires_at_ms,
    })?;

    // The fan-out. A conversation that refuses is skipped rather than
    // abandoning the rest: one unreachable contact should not cost the story
    // to everybody else, and the story exists on the server either way.
    for id in ctx.store.conversation_ids()? {
        let Ok(conversation_id) = id.parse::<ConversationId>() else {
            continue;
        };
        if let Err(error) =
            crate::conversations::send_payload(ctx, conversation_id, &payload, now_ms)
        {
            tracing::warn!(%id, %error, "a story did not reach one conversation");
        }
    }

    Ok(summary.id)
}

/// Stories this device holds, and the end of the expired ones.
///
/// The purge is inside `live_stories`, and it is the layer that matters: the
/// server refusing to serve an expired story is worth little if the key is
/// still on the reader's disk. This is also why it works offline.
///
/// A received story carries a device id, not a handle -- MLS names devices --
/// so `StoredStory::author_handle` sits empty until something resolves it. It
/// stays exactly that empty string forever unless this function fills it in,
/// which is what the rest of the body does: `GET /v1/stories` is the server
/// telling us, by story id, who a contact's story actually belongs to. The
/// same call also answers, as a side effect, who currently has one live --
/// which is the question a "this person has a story" ring needs.
///
/// The reconciliation is best-effort. A device offline reading its own cached
/// stories is the point of storing them locally at all, so a failed request
/// here falls back to the unresolved list rather than failing the whole read
/// -- the same story, with a blank name instead of no story at all.
pub fn live<T: Transport>(
    ctx: &Context<'_, T>,
    now_ms: i64,
) -> Result<Vec<StoredStory>, ConversationError> {
    let mut stories = ctx.store.live_stories(now_ms)?;

    let Ok(listed) = ctx.transport.list_stories() else {
        return Ok(stories);
    };
    // By id, not by device: the server's listing does not know about devices
    // at all, and matching this way sidesteps needing a device-to-handle
    // table that does not exist anywhere else in this codebase either.
    let by_id: std::collections::HashMap<i64, String> = listed
        .into_iter()
        .map(|s| (s.id, s.author_handle))
        .collect();
    for story in &mut stories {
        // Only ever fills a blank in. A row that already has a handle is our
        // own copy, written with the handle the server gave at post time
        // (`post`, above) -- there is nothing here to correct it with, since
        // this listing does not distinguish "you" from any other contact.
        if story.author_handle.is_empty()
            && let Some(handle) = by_id.get(&story.id)
        {
            story.author_handle = handle.clone();
        }
    }
    Ok(stories)
}

/// Fetch a story and open it.
///
/// The key never leaves this crate: the caller gets plaintext bytes, the same
/// bargain an attachment makes (rule 2). The expiry is checked here as well as
/// by the server, because a story whose key we still hold is one we could
/// otherwise open after it was supposed to be gone.
pub fn open<T: Transport>(
    ctx: &Context<'_, T>,
    id: i64,
    now_ms: i64,
) -> Result<(Vec<u8>, String), ConversationError> {
    // `live_stories` purges as it reads, so a story that has expired is not in
    // this list and its key is no longer on the disk.
    let story = ctx
        .store
        .live_stories(now_ms)?
        .into_iter()
        .find(|s| s.id == id)
        .ok_or(ConversationError::NotAMember)?;

    let url = ctx.transport.story_url(id)?;
    let ciphertext = ctx.transport.get_object(&url)?;

    let plaintext = nexo_crypto::attachment::decrypt(
        &ciphertext,
        &unhex(&story.enc_key)?,
        &unhex(&story.nonce)?,
        &unhex(&story.sha256)?,
    )?;
    Ok((plaintext.to_vec(), story.mime))
}

fn unhex(s: &str) -> Result<Vec<u8>, ConversationError> {
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(s.get(i..i + 2).unwrap_or("zz"), 16).map_err(|_| {
                ConversationError::Transport(crate::transport::TransportError::Rejected(
                    "that story is unreadable".into(),
                ))
            })
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
