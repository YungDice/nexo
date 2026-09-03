//! Meet&Greet, from the client's side.
//!
//! The same shape as [`crate::feed`]: everything goes through the
//! [`Transport`](crate::transport::Transport) trait, there is no HTTP client
//! here and no platform call, so this crate still compiles for Android
//! unchanged.
//!
//! Two things are worth knowing before reading further.
//!
//! **This is not a feed and must not become one.** The map is fetched when the
//! tab is opened and when somebody asks for it again. Never on a timer, and
//! never through `syncAgent` — that loop belongs to messages, and a map that
//! polls would turn "where somebody said they are" into "where somebody is
//! right now", which is the one thing this feature is built not to be.
//!
//! **The pin that comes back is not the pin that was sent.** The server
//! coarsens on write. Anything drawing the person's own pin reads it back
//! rather than echoing what it submitted, or it would draw a precision that
//! does not exist.

use nexo_protocol::{MeetProfile, MeetProfileUpdate, MeetRequest};
use nexo_store::{EncryptedStore, MeetPin};

use crate::transport::{Transport, TransportError};

/// What went wrong.
#[derive(Debug, thiserror::Error)]
pub enum MeetError {
    /// The network, or the server.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// The local store.
    #[error(transparent)]
    Store(#[from] nexo_store::StoreError),
}

impl From<crate::conversations::ConversationError> for MeetError {
    /// Stories go out through the conversation layer, so its failures surface
    /// here. Only the two that a caller can act on are distinguished; the rest
    /// become a transport rejection carrying the detail, which is what the
    /// shell already knows how to show.
    fn from(error: crate::conversations::ConversationError) -> Self {
        use crate::conversations::ConversationError as E;
        match error {
            E::Transport(inner) => MeetError::Transport(inner),
            E::Store(inner) => MeetError::Store(inner),
            other => MeetError::Transport(TransportError::Rejected(other.to_string())),
        }
    }
}

/// The map, and how old it is.
#[derive(Debug, Clone)]
pub struct Map {
    /// Everyone on it.
    pub pins: Vec<MeetPin>,
    /// When these were fetched, in milliseconds since the Unix epoch. `0` when
    /// they have never been fetched on this device.
    pub fetched_at_ms: i64,
    /// Whether this came from the cache because the server could not be
    /// reached. The UI says so rather than presenting stale pins as current.
    pub stale: bool,
}

/// Everything needed to reach the map.
pub struct Context<'a, T: Transport> {
    /// The network.
    pub transport: &'a T,
    /// The encrypted local store, which holds the cache.
    pub store: &'a EncryptedStore,
}

/// Fetch the map, falling back to the cached copy when the server is away.
///
/// The fallback is the point of the cache. A map that renders empty because a
/// train went into a tunnel looks like a map with nobody on it, and "nobody is
/// here" is a different and much worse message than "this is how it looked an
/// hour ago".
pub fn map<T: Transport>(ctx: &Context<'_, T>, now_ms: i64) -> Result<Map, MeetError> {
    match fetch_all(ctx.transport) {
        Ok(pins) => {
            let cached: Vec<MeetPin> = pins.iter().map(|p| to_cache(p, now_ms)).collect();
            ctx.store.cache_meet_pins(&cached, now_ms)?;
            Ok(Map {
                pins: cached,
                fetched_at_ms: now_ms,
                stale: false,
            })
        }
        Err(TransportError::Unreachable(_)) => {
            let pins = ctx.store.cached_meet_pins()?;
            let fetched_at_ms = pins.first().map(|p| p.fetched_at_ms).unwrap_or(0);
            Ok(Map {
                pins,
                fetched_at_ms,
                stale: true,
            })
        }
        Err(error) => Err(error.into()),
    }
}

/// Every page of pins.
///
/// Paged rather than capped: a map that silently stopped at the first five
/// hundred would be missing people with no way to tell.
fn fetch_all<T: Transport>(transport: &T) -> Result<Vec<MeetProfile>, TransportError> {
    let mut all: Vec<MeetProfile> = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let page = transport.meet_pins(after.as_deref())?;
        let last = page.last().map(|p| p.handle.clone());
        let count = page.len();
        all.extend(page);
        match last {
            // A short page is the end. A full one might be, and asking once
            // more is cheaper than being wrong.
            Some(handle) if count > 0 => after = Some(handle),
            _ => break,
        }
        if count < 2 {
            break;
        }
        // A page that does not advance the cursor would loop for ever. The
        // server orders by handle and the cursor is the last handle, so this
        // can only happen if it stops honouring `after`.
        if all.len() > 100_000 {
            break;
        }
    }
    Ok(all)
}

fn to_cache(profile: &MeetProfile, fetched_at_ms: i64) -> MeetPin {
    MeetPin {
        handle: profile.handle.clone(),
        display_name: profile.display_name.clone(),
        lat: profile.lat,
        lon: profile.lon,
        headline: profile.headline.clone(),
        // Kept as text. Neither this crate nor the store reads it.
        char_config: profile.char_config.to_string(),
        updated_at_ms: profile.updated_at_ms,
        fetched_at_ms,
    }
}

/// My own pin, or `None` when I am not on the map.
pub fn me<T: Transport>(ctx: &Context<'_, T>) -> Result<Option<MeetProfile>, MeetError> {
    Ok(ctx.transport.meet_me()?)
}

/// Place or move my pin.
///
/// Returns what the server stored, which is deliberately not what was sent.
pub fn set_me<T: Transport>(
    ctx: &Context<'_, T>,
    update: &MeetProfileUpdate,
) -> Result<Option<MeetProfile>, MeetError> {
    ctx.transport.meet_set_me(update)?;
    // Read back rather than assume: the pin has been coarsened, and drawing the
    // submitted one would show a precision the server refused to keep.
    Ok(ctx.transport.meet_me()?)
}

/// Come off the map. The character survives.
pub fn leave<T: Transport>(ctx: &Context<'_, T>) -> Result<(), MeetError> {
    ctx.transport.meet_leave()?;
    Ok(())
}

/// Accept the agreement.
pub fn accept_agreement<T: Transport>(ctx: &Context<'_, T>, version: i32) -> Result<(), MeetError> {
    ctx.transport.meet_consent(version)?;
    Ok(())
}

/// Intros waiting for me.
pub fn requests<T: Transport>(ctx: &Context<'_, T>) -> Result<Vec<MeetRequest>, MeetError> {
    Ok(ctx.transport.meet_requests()?)
}

/// Record that a conversation is an intro.
///
/// The conversation must already exist and already carry its one message. The
/// ordering matters and belongs to the caller: opening the conversation
/// through the ordinary path first means a failure here leaves an ordinary
/// conversation rather than a request pointing at nothing.
pub fn open_request<T: Transport>(
    ctx: &Context<'_, T>,
    handle: &str,
    conversation_id: &str,
) -> Result<MeetRequest, MeetError> {
    Ok(ctx.transport.meet_open_request(handle, conversation_id)?)
}

/// Find people. Public accounts only, and never yourself.
///
/// A private account is absent from this, and that absence is enforced by the
/// server rather than filtered here — a directory the client trims is one
/// anybody can untrim.
pub fn search<T: Transport>(
    ctx: &Context<'_, T>,
    term: &str,
) -> Result<Vec<crate::transport::SearchResult>, MeetError> {
    Ok(ctx.transport.search_users(term)?)
}

/// Mint an invitation.
///
/// The secret comes back once. It is stored as a hash, so a lost one cannot be
/// recovered — it is revoked and replaced, the same answer a password reset
/// gives and for the same reason.
pub fn create_invite<T: Transport>(
    ctx: &Context<'_, T>,
    label: Option<&str>,
    days: i64,
) -> Result<crate::transport::MintedInvite, MeetError> {
    Ok(ctx.transport.create_invite(label, days)?)
}

/// My invitations, live and spent.
pub fn invites<T: Transport>(
    ctx: &Context<'_, T>,
) -> Result<Vec<crate::transport::InviteSummary>, MeetError> {
    Ok(ctx.transport.list_invites()?)
}

/// Withdraw an invitation. The row stays, so requests can still say where they
/// came from.
pub fn revoke_invite<T: Transport>(ctx: &Context<'_, T>, id: i64) -> Result<(), MeetError> {
    ctx.transport.revoke_invite(id)?;
    Ok(())
}

/// File a report about somebody.
///
/// Reporting is not a Meet&Greet idea — blocking answers "I do not want to see
/// this person", reporting answers "this should not be here", and the second
/// needs somebody other than the reporter to act. The server has had the
/// endpoint since BRIEF 13; this is the first thing in the app to reach it.
pub fn report<T: Transport>(
    ctx: &Context<'_, T>,
    subject_kind: &str,
    subject_id: i64,
    reason: &str,
    note: Option<&str>,
) -> Result<(), MeetError> {
    ctx.transport
        .report(subject_kind, subject_id, reason, note)?;
    Ok(())
}

/// Answer an intro.
pub fn answer<T: Transport>(ctx: &Context<'_, T>, id: i64, accept: bool) -> Result<(), MeetError> {
    if accept {
        ctx.transport.meet_accept(id)?;
    } else {
        ctx.transport.meet_decline(id)?;
    }
    Ok(())
}
