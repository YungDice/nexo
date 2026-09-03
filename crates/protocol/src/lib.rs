//! Wire types shared by the Nexo client and server.
//!
//! Deliberate constraints on this crate:
//! - no I/O, no crypto, no platform calls, so it compiles unchanged for Android;
//! - nothing here may carry message plaintext. The server handles these types,
//!   and the server must never be able to read message contents (brief rule 4).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Wire protocol version. Bump on any breaking change to the types below.
///
/// 2 adds the Meet&Greet types at the end of this file.
pub const PROTOCOL_VERSION: u16 = 2;

/// A conversation identifier. One MLS group per conversation; a 1:1 chat is a
/// two-member group with no special-casing (§4.2).
pub type ConversationId = Uuid;

/// A device identifier. In v0.1 an account has exactly one device, but the MLS
/// group member is the *device*, not the user, so multi-device is an added
/// member later rather than a schema change.
pub type DeviceId = Uuid;

/// The complete set of fields that travel with a message on the wire.
///
/// §4.2 fixes this shape: nothing else is permitted. No plaintext subject, no
/// preview, no attachment filename, no MIME type. Attachment metadata lives
/// *inside* `ciphertext`, never beside it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Envelope {
    /// Which conversation this belongs to.
    pub conversation_id: ConversationId,
    /// Which device sent it.
    pub sender_device_id: DeviceId,
    /// The MLS epoch the ciphertext was produced in.
    pub epoch: u64,
    /// The opaque MLS message. The server never decrypts this.
    #[serde(with = "serde_bytes_vec")]
    pub ciphertext: Vec<u8>,
    /// Server receive time, in milliseconds since the Unix epoch. Set by the
    /// server; a client-supplied value is ignored.
    pub server_timestamp_ms: i64,
}

/// What is actually inside an MLS ciphertext.
///
/// §4.2 fixes the *envelope* shape and forbids a plaintext subject, preview,
/// filename or MIME type beside a message. This is the other half of that rule:
/// everything those would have carried lives **inside** the ciphertext instead,
/// where the server cannot reach it.
///
/// Tagged and versioned, because this is the one structure both ends must agree
/// on forever. A client meeting a `kind` it does not know shows "this message
/// needs a newer version of Nexo" rather than guessing.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Payload {
    /// An ordinary message.
    Text {
        /// The message.
        body: String,
    },
    /// A file. The bytes live in object storage; the key to them is here.
    ///
    /// This is why an attachment is end-to-end encrypted rather than merely
    /// encrypted-at-rest: the object in the bucket is AES-256-GCM ciphertext,
    /// and the only copy of the key that opens it is inside an MLS message the
    /// server cannot read.
    Attachment {
        /// Where the ciphertext sits in the bucket.
        s3_key: String,
        /// The AES-256-GCM key, hex. **Never leaves an MLS message.**
        key: String,
        /// The nonce, hex. Fresh per file.
        nonce: String,
        /// SHA-256 of the *plaintext*, hex, so a recipient can tell a corrupted
        /// download from a tampered one.
        sha256: String,
        /// The original file name. Inside the ciphertext, never beside it.
        name: String,
        /// MIME type, likewise inside.
        mime: String,
        /// Plaintext size in bytes.
        size: u64,
        /// An optional message sent with the file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        body: Option<String>,
    },
    /// A change to what the conversation is called.
    ///
    /// Sent as an ordinary message so every member converges on the same name,
    /// and so the **server never learns it**. A title column on the server
    /// would have been simpler and would have handed it whatever people call
    /// their group — which is content, not the routing metadata the threat
    /// model already concedes.
    ///
    /// Renaming is last-writer-wins by envelope order. Two people renaming at
    /// once is not worth a merge: the later one is what everybody sees, and
    /// they can see each other do it.
    Rename {
        /// What the conversation is now called.
        title: String,
    },
    /// A new picture for the conversation.
    ///
    /// Encrypted exactly like an [`Payload::Attachment`], and for the same
    /// reason: the object in the bucket is AES-256-GCM ciphertext and the only
    /// copy of the key is inside an MLS message. A group's picture is
    /// something its members chose to show each other, not something the
    /// server is entitled to look at.
    GroupAvatar {
        /// Where the ciphertext sits in the bucket.
        s3_key: String,
        /// The AES-256-GCM key, hex. **Never leaves an MLS message.**
        key: String,
        /// The nonce, hex. Fresh per image.
        nonce: String,
        /// SHA-256 of the *plaintext*, hex.
        sha256: String,
        /// MIME type, sniffed from the bytes rather than the file name.
        mime: String,
        /// Plaintext size in bytes.
        size: u64,
    },
    /// A payload this build cannot read.
    ///
    /// Produced only by [`Payload::decode`] and never sent — it is what a
    /// client does *instead of* guessing. The alternative is what this used to
    /// do: render the raw JSON as though someone had typed it, which turns
    /// every future variant into a bubble full of punctuation on every
    /// installation that has not updated yet.
    ///
    /// The `kind` is carried so the UI can say which thing is missing rather
    /// than only that something is. The undecoded bytes are kept beside the
    /// message by the client, because MLS will not decrypt that envelope a
    /// second time: a build that learns the variant later can still read what
    /// arrived today.
    #[serde(skip)]
    Unsupported {
        /// The `kind` the sender used.
        kind: String,
    },
}

impl Payload {
    /// A plain text message.
    pub fn text(body: impl Into<String>) -> Self {
        Self::Text { body: body.into() }
    }

    /// What to show in a conversation list, and in the bubble.
    ///
    /// An attachment with no message shows its file name, because "" is not a
    /// useful thing to render and the name is already inside the ciphertext.
    pub fn preview(&self) -> &str {
        match self {
            Payload::Text { body } => body,
            Payload::Attachment { body, name, .. } => match body {
                Some(body) if !body.is_empty() => body,
                _ => name,
            },
            // Neither is something anyone said, so neither is a preview of the
            // conversation. The row keeps whatever came before it.
            Payload::Rename { .. } | Payload::GroupAvatar { .. } => "",
            // Nor is this. Whatever it says, this build cannot read it, and
            // guessing at a preview would be the same mistake in a smaller
            // place.
            Payload::Unsupported { .. } => "",
        }
    }

    /// Encodes for putting inside an MLS message.
    pub fn encode(&self) -> Vec<u8> {
        // JSON rather than a compact codec: this is inside a padded, encrypted
        // message, so a few bytes buy nothing, and being able to read it in a
        // debugger without a decoder is worth real money when something is
        // wrong.
        serde_json::to_vec(self).unwrap_or_else(|_| b"{}".to_vec())
    }

    /// Encodes as a string, for storing beside the message.
    pub fn encode_string(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Decodes what came out of an MLS message.
    ///
    /// Three outcomes, and the distinction between the last two is the point:
    ///
    /// - a payload this build knows;
    /// - a JSON object naming a `kind` it cannot read — [`Payload::Unsupported`],
    ///   which draws no bubble and says so;
    /// - anything else — text, because the very first messages this project
    ///   ever sent were bare UTF-8 with no envelope, and refusing to read them
    ///   would be self-inflicted data loss.
    ///
    /// The middle case used to fall through to the last one. That made every
    /// new variant a display bug on older installations, and it read the
    /// sender's structure as if it were their prose — the opposite of failing
    /// closed (rule 7).
    pub fn decode(bytes: &[u8]) -> Self {
        match serde_json::from_slice::<Payload>(bytes) {
            Ok(payload) => payload,
            Err(_) => match tagged_kind(bytes) {
                Some(kind) => Payload::Unsupported { kind },
                None => Payload::Text {
                    body: String::from_utf8_lossy(bytes).into_owned(),
                },
            },
        }
    }
}

/// The `kind` of a JSON object, when the bytes are one.
///
/// Deliberately not a second attempt at the whole payload: all this decides is
/// whether somebody sent a *tagged* thing, and pulling one string out of an
/// untyped value cannot fail on fields this build has never heard of — which
/// is exactly the case it exists to catch.
fn tagged_kind(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    Some(value.get("kind")?.as_str()?.to_string())
}

/// Reduces a sender-supplied file name to something safe to write to disk.
///
/// The name inside an attachment payload is chosen by whoever sent it. It is
/// end-to-end authenticated -- so it is genuinely *their* name -- but that is a
/// statement about who wrote it, not about whether it is safe. A contact whose
/// device has been taken over can send `..\..\Startup\evil.exe`, and
/// authentication does not make that harmless.
///
/// So: the last path segment only, with separators, drive colons, wildcards,
/// control characters, and leading dots removed, truncated, and never empty.
/// The result is a suggestion for a save dialog, which is the only place it is
/// ever used -- nothing writes to it without the user choosing a location.
pub fn safe_file_name(name: &str) -> String {
    // Both separators, because a Windows client can receive a name composed on
    // any platform and `/` is a separator here too.
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);

    let cleaned: String = last
        .chars()
        .map(|c| match c {
            // Reserved on Windows, plus control characters.
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();

    // A leading dot hides the file; a trailing dot or space is silently
    // stripped by Windows, which turns "evil.exe " into "evil.exe".
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());

    // Device names are still reserved even with an extension: `CON.txt` is
    // `CON`. Prefixing is enough to make them ordinary.
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = trimmed.split('.').next().unwrap_or("");
    let reserved = RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r));

    let mut out = if trimmed.is_empty() {
        "attachment".to_string()
    } else if reserved {
        format!("_{trimmed}")
    } else {
        trimmed.to_string()
    };

    // Well under any filesystem limit, and on a character boundary.
    const MAX: usize = 120;
    if out.len() > MAX {
        let cut = (0..=MAX)
            .rev()
            .find(|i| out.is_char_boundary(*i))
            .unwrap_or(0);
        out.truncate(cut);
    }
    if out.is_empty() {
        out = "attachment".to_string();
    }
    out
}

/// What the server pushes down the WebSocket (§5.2).
///
/// Tagged by `type` so a client can match on one field, and so adding a variant
/// later does not change how the existing ones deserialise.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// A message, or a commit, for a conversation this device is in.
    Envelope {
        /// Cursor: also what the client acknowledges.
        envelope_id: i64,
        /// Which conversation.
        conversation_id: ConversationId,
        /// Which device sent it.
        sender_device_id: DeviceId,
        /// The epoch it was built against.
        epoch: u64,
        /// The opaque MLS message, hex-encoded.
        ciphertext: String,
        /// Whether it carries a commit.
        is_commit: bool,
        /// Server receive time.
        server_timestamp_ms: i64,
    },
    /// Someone is typing. Opt-out-able in Settings (§6.1).
    Typing {
        /// Where.
        conversation_id: ConversationId,
        /// Who.
        user_id: i64,
    },
    /// Someone came online or went away.
    Presence {
        /// Who.
        user_id: i64,
        /// Whether they are connected now.
        online: bool,
    },
    /// A delivery or read receipt.
    Receipt {
        /// Which conversation.
        conversation_id: ConversationId,
        /// How far the sender has read.
        envelope_id: i64,
        /// Who read it.
        user_id: i64,
    },
    /// The server is closing this connection, and why.
    ///
    /// Sent before the close frame so the client can tell "your token expired"
    /// from "the network dropped" — the first needs a refresh, the second a
    /// retry.
    Closing {
        /// A machine-readable reason.
        reason: String,
    },
}

/// What a client sends up the WebSocket (§5.2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    /// Confirms an envelope was received, so the server can stop holding it.
    ///
    /// Delivered ciphertext is deleted on acknowledgement (§4.3); this is what
    /// triggers that.
    Ack {
        /// Which conversation.
        conversation_id: ConversationId,
        /// Everything up to and including this id has arrived.
        envelope_id: i64,
    },
    /// This device is typing.
    Typing {
        /// Where.
        conversation_id: ConversationId,
    },
    /// Keeps the connection alive through an idle NAT.
    Ping,
}

/// Errors that can be reported across the wire.
#[derive(Debug, thiserror::Error, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProtocolError {
    /// The commit referenced an epoch that is no longer current. The client
    /// must resync and rebuild it. See docs/PLAN.md risk 4(b).
    #[error("stale epoch: server is at {current}, commit cited {cited}")]
    StaleEpoch {
        /// The epoch the server considers current.
        current: u64,
        /// The epoch the rejected commit cited.
        cited: u64,
    },
    /// The wire protocol versions do not match.
    #[error("unsupported protocol version {0}")]
    UnsupportedVersion(u16),
}

mod serde_bytes_vec {
    //! Serialize `Vec<u8>` compactly under CBOR while staying readable in JSON.
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(v)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        Vec::<u8>::deserialize(d)
    }
}

// ------------------------------------------------------------- Meet&Greet ---
//
// These are the exception to this file's usual rule, and it is worth being
// exact about why. Everything above carries ciphertext the server cannot read.
// A Meet&Greet presence is the opposite: a pin, a headline and a character are
// *meant* to be readable by the server and by every signed-in person, exactly
// as a profile is. Rule 4 is not weakened by that, because none of this is
// message content -- and rule 5 is what makes it honest, so the agreement
// screen says all three are public in those words.
//
// What is deliberately absent: any field a device could fill in by itself. No
// accuracy, no heading, no speed, no "seen at". A pin is a claim somebody
// typed, and a schema that cannot express a measurement cannot later be made
// to carry one by a well-meaning change.

/// Somebody's presence on the Meet&Greet map.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MeetProfile {
    /// Who this is.
    pub handle: String,
    /// What to call them.
    pub display_name: String,
    /// Latitude, **as the server stored it** — snapped to a grid and jittered.
    ///
    /// Never the value a client submitted. The coarsening happens on write, so
    /// the precise figure is not kept anywhere and cannot leak from here later.
    pub lat: f64,
    /// Longitude, under the same rule as `lat`.
    pub lon: f64,
    /// One line about themselves. The only free text in P0.
    pub headline: Option<String>,
    /// The NexoChar, as its generator config.
    ///
    /// `Value` on purpose: the server does not know what a hairstyle is and
    /// must not learn. It enforces a size ceiling and nothing else, and the
    /// character is rendered on whichever client draws it. Storing the config
    /// rather than an image is also what keeps this out of object storage —
    /// there is no picture to host, and none to moderate.
    pub char_config: serde_json::Value,
    /// When the pin last moved, in milliseconds since the Unix epoch.
    pub updated_at_ms: i64,
}

/// A change to one's own presence. Every field is optional: this is a patch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MeetProfileUpdate {
    /// Where the pin was dropped. Coarsened by the server before it is stored.
    pub lat: Option<f64>,
    /// The other half of the pin.
    pub lon: Option<f64>,
    /// One line, at most 80 characters.
    pub headline: Option<String>,
    /// The NexoChar config.
    pub char_config: Option<serde_json::Value>,
    /// Whether to appear on the map at all.
    ///
    /// Leaving is a flag rather than a delete: a character somebody spent ten
    /// minutes on survives being off the map, and coming back is one tap.
    pub active: Option<bool>,
}

/// How far an intro has got.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeetRequestState {
    /// Sent, not yet answered. The sender may not send again while this holds.
    Pending,
    /// The conversation is now an ordinary one.
    Accepted,
    /// Refused. Nothing is sent back beyond the state itself.
    Declined,
}

/// One intro, from a stranger on the map.
///
/// The conversation is a real MLS group opened through the ordinary delivery
/// path — there is no second, lesser kind of message here. What the request
/// adds is the one-message rule while it is `Pending`, enforced by the server
/// because a cap the client applies is not a cap.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeetRequest {
    /// The server's id for this request.
    pub id: i64,
    /// Who sent it.
    pub from_handle: String,
    /// The conversation their one message is in.
    pub conversation_id: ConversationId,
    /// Where it has got to.
    pub state: MeetRequestState,
    /// When it was sent, in milliseconds since the Unix epoch.
    pub created_at_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_payload_round_trips() {
        let payload = Payload::text("hello");
        assert_eq!(Payload::decode(&payload.encode()), payload);
    }

    #[test]
    fn an_attachment_payload_round_trips() {
        let payload = Payload::Attachment {
            s3_key: "enc/abc/def".into(),
            key: "aa".repeat(32),
            nonce: "bb".repeat(12),
            sha256: "cc".repeat(32),
            name: "report.pdf".into(),
            mime: "application/pdf".into(),
            size: 1234,
            body: Some("as promised".into()),
        };
        assert_eq!(Payload::decode(&payload.encode()), payload);
    }

    #[test]
    fn bare_bytes_are_read_as_text() {
        // The first messages this project sent had no envelope. Refusing to
        // read them would be self-inflicted data loss.
        assert_eq!(
            Payload::decode(b"just a message"),
            Payload::text("just a message")
        );
    }

    #[test]
    fn an_unknown_kind_is_not_rendered_as_text() {
        // What a newer build's payload looks like to this one. Reading it as
        // prose would put raw JSON in a chat bubble on every installation that
        // has not updated, which is what this used to do.
        let from_the_future = br#"{"kind":"reaction","target":42,"emoji":"x"}"#;
        assert_eq!(
            Payload::decode(from_the_future),
            Payload::Unsupported {
                kind: "reaction".into()
            }
        );
    }

    #[test]
    fn a_malformed_known_kind_also_fails_closed() {
        // A `text` payload with no body is not a message someone typed; it is
        // a payload that did not survive the trip. Same answer, because the
        // honest thing to say about both is "this did not open".
        assert_eq!(
            Payload::decode(br#"{"kind":"text"}"#),
            Payload::Unsupported {
                kind: "text".into()
            }
        );
    }

    #[test]
    fn json_that_is_not_a_tagged_payload_is_still_text() {
        // Somebody typing JSON into the composer is sending prose that happens
        // to have braces in it. Only a `kind` makes it a payload.
        let typed = br#"{"hello": "world"}"#;
        assert_eq!(
            Payload::decode(typed),
            Payload::text(r#"{"hello": "world"}"#)
        );
    }

    #[test]
    fn an_unsupported_payload_previews_as_nothing() {
        // It must not reach the conversation list. A row that shows the kind
        // of a thing it cannot read is leaking structure into prose again.
        let payload = Payload::Unsupported {
            kind: "reaction".into(),
        };
        assert_eq!(payload.preview(), "");
    }

    #[test]
    fn an_attachment_with_no_message_previews_its_name() {
        let payload = Payload::Attachment {
            s3_key: "k".into(),
            key: String::new(),
            nonce: String::new(),
            sha256: String::new(),
            name: "holiday.jpg".into(),
            mime: "image/jpeg".into(),
            size: 1,
            body: None,
        };
        assert_eq!(payload.preview(), "holiday.jpg");
    }

    #[test]
    fn a_message_sent_with_an_attachment_wins_the_preview() {
        let payload = Payload::Attachment {
            s3_key: "k".into(),
            key: String::new(),
            nonce: String::new(),
            sha256: String::new(),
            name: "holiday.jpg".into(),
            mime: "image/jpeg".into(),
            size: 1,
            body: Some("from the trip".into()),
        };
        assert_eq!(payload.preview(), "from the trip");
    }

    #[test]
    fn the_filename_is_inside_the_payload_not_beside_it() {
        // §4.2: no plaintext filename may sit next to a message. The only place
        // it exists is here, inside what gets encrypted.
        let payload = Payload::Attachment {
            s3_key: "enc/x/y".into(),
            key: "0".repeat(64),
            nonce: "0".repeat(24),
            sha256: "0".repeat(64),
            name: "payslip.pdf".into(),
            mime: "application/pdf".into(),
            size: 10,
            body: None,
        };
        let encoded = payload.encode();
        assert!(
            encoded.windows(11).any(|w| w == b"payslip.pdf"),
            "the name must be in the payload, which is what gets encrypted"
        );
        // And an Envelope has nowhere to put it.
        let envelope = Envelope {
            conversation_id: Uuid::nil(),
            sender_device_id: Uuid::nil(),
            epoch: 1,
            ciphertext: encoded,
            server_timestamp_ms: 0,
        };
        let json = serde_json::to_value(&envelope).unwrap();
        let keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        assert!(!keys.contains(&"name"));
        assert!(!keys.contains(&"mime"));
    }

    #[test]
    fn envelope_roundtrips_and_carries_nothing_extra() {
        let e = Envelope {
            conversation_id: Uuid::nil(),
            sender_device_id: Uuid::nil(),
            epoch: 7,
            ciphertext: vec![1, 2, 3],
            server_timestamp_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_value(&e).unwrap();
        let mut keys: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(|s| s.as_str())
            .collect();
        keys.sort_unstable();
        // Field *order* is not meaningful here (serde_json sorts); the point is
        // that the set is exactly these five and nothing has crept in.
        assert_eq!(
            keys,
            [
                "ciphertext",
                "conversation_id",
                "epoch",
                "sender_device_id",
                "server_timestamp_ms"
            ],
            "§4.2 fixes the envelope shape; adding a field is a protocol change"
        );

        let back: Envelope = serde_json::from_value(json).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn a_traversing_file_name_loses_its_path() {
        // The attack this function exists for: an authenticated contact whose
        // device was taken over choosing where the file lands.
        assert_eq!(safe_file_name(r"..\..\Startup\evil.exe"), "evil.exe");
        assert_eq!(safe_file_name("../../etc/passwd"), "passwd");
        assert_eq!(
            safe_file_name(r"C:\Windows\System32\drivers\etc\hosts"),
            "hosts"
        );
    }

    #[test]
    fn separators_and_wildcards_do_not_survive() {
        assert!(!safe_file_name("a/b").contains('/'));
        assert!(!safe_file_name(r"a\b").contains('\\'));
        assert_eq!(safe_file_name("re*port?.txt"), "re_port_.txt");
        assert_eq!(safe_file_name("a:b.txt"), "a_b.txt");
    }

    #[test]
    fn control_characters_are_removed() {
        // A newline in a name is either a mistake or an attempt to make a log
        // line or a dialog say something it does not.
        assert_eq!(safe_file_name("re\nport\t.txt"), "re_port_.txt");
        assert_eq!(safe_file_name("null\0byte"), "null_byte");
    }

    #[test]
    fn trailing_dots_and_spaces_are_stripped() {
        // Windows strips them silently, so "evil.exe " and "evil.exe" name the
        // same file -- better to make that visible here than to be surprised.
        assert_eq!(safe_file_name("evil.exe "), "evil.exe");
        assert_eq!(safe_file_name("evil.exe."), "evil.exe");
        assert_eq!(safe_file_name(".hidden"), "hidden");
    }

    #[test]
    fn reserved_device_names_are_defused() {
        // CON, CON.txt, and con.TXT all name the console device.
        assert_eq!(safe_file_name("CON"), "_CON");
        assert_eq!(safe_file_name("con.txt"), "_con.txt");
        assert_eq!(safe_file_name("LPT1.pdf"), "_LPT1.pdf");
        // But a name that merely starts with those letters is fine.
        assert_eq!(safe_file_name("contract.pdf"), "contract.pdf");
    }

    #[test]
    fn a_name_that_reduces_to_nothing_gets_one() {
        // Every branch has to end with something writable; "" is not a file
        // name, and neither is "...".
        assert_eq!(safe_file_name(""), "attachment");
        assert_eq!(safe_file_name("..."), "attachment");
        assert_eq!(safe_file_name("   "), "attachment");
        assert_eq!(safe_file_name("/"), "attachment");
    }

    #[test]
    fn a_very_long_name_is_truncated_on_a_character_boundary() {
        let long = format!("{}.txt", "\u{00e4}".repeat(400));
        let safe = safe_file_name(&long);
        assert!(safe.len() <= 120);
        // The point of the boundary search: this would panic on a bad cut.
        assert!(std::str::from_utf8(safe.as_bytes()).is_ok());
        assert!(!safe.is_empty());
    }

    #[test]
    fn an_ordinary_name_is_left_alone() {
        // A sanitiser that mangles normal input is a bug users will actually
        // notice.
        assert_eq!(
            safe_file_name("Quarterly Report (final) v2.pdf"),
            "Quarterly Report (final) v2.pdf"
        );
        assert_eq!(
            safe_file_name("\u{4f1a}\u{8b70}\u{8a18}\u{9332}.docx"),
            "\u{4f1a}\u{8b70}\u{8a18}\u{9332}.docx"
        );
    }

    #[test]
    fn a_meet_profile_round_trips() {
        let profile = MeetProfile {
            handle: "dice".into(),
            display_name: "Dice".into(),
            lat: 47.25,
            lon: 8.5,
            headline: Some("here for the mountains".into()),
            char_config: serde_json::json!({ "topVariant": "hoodie", "eyesVariant": "happy" }),
            updated_at_ms: 1_760_000_000_000,
        };
        let wire = serde_json::to_string(&profile).unwrap();
        assert_eq!(serde_json::from_str::<MeetProfile>(&wire).unwrap(), profile);
    }

    /// A headline is optional, and absent is not the same as empty.
    #[test]
    fn a_meet_profile_without_a_headline_round_trips() {
        let profile = MeetProfile {
            handle: "bananaaboy".into(),
            display_name: "bananaaboy".into(),
            lat: -33.75,
            lon: 151.0,
            headline: None,
            char_config: serde_json::json!({}),
            updated_at_ms: 1,
        };
        let wire = serde_json::to_string(&profile).unwrap();
        let back: MeetProfile = serde_json::from_str(&wire).unwrap();
        assert_eq!(back, profile);
        assert!(back.headline.is_none());
    }

    /// The update is a patch: an omitted field means "leave it alone", which is
    /// a different instruction from "set it to nothing".
    #[test]
    fn a_meet_update_round_trips_and_defaults_to_changing_nothing() {
        let empty = MeetProfileUpdate::default();
        assert!(empty.lat.is_none() && empty.char_config.is_none() && empty.active.is_none());

        let leaving = MeetProfileUpdate {
            active: Some(false),
            ..Default::default()
        };
        let wire = serde_json::to_string(&leaving).unwrap();
        assert_eq!(
            serde_json::from_str::<MeetProfileUpdate>(&wire).unwrap(),
            leaving
        );
    }

    #[test]
    fn a_meet_request_round_trips_in_every_state() {
        for state in [
            MeetRequestState::Pending,
            MeetRequestState::Accepted,
            MeetRequestState::Declined,
        ] {
            let request = MeetRequest {
                id: 7,
                from_handle: "dice".into(),
                conversation_id: ConversationId::nil(),
                state,
                created_at_ms: 1_760_000_000_000,
            };
            let wire = serde_json::to_string(&request).unwrap();
            assert_eq!(serde_json::from_str::<MeetRequest>(&wire).unwrap(), request);
        }
    }

    /// The states are written into the database's CHECK constraint, so their
    /// wire spelling is not free to drift.
    #[test]
    fn meet_request_states_are_snake_case_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&MeetRequestState::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&MeetRequestState::Declined).unwrap(),
            "\"declined\""
        );
    }
}
