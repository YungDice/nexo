# Threat model

What Nexo protects, what it does not, and who it does not protect you from.

This document is the one place allowed to be blunt about limitations. Rule 5 of
the brief says be honest about what is and is not encrypted; a threat model that
oversells is worse than none, because it makes people act on a protection they
do not have.

Status: written at M2, revised at M9. Everything described here is built and
tested unless a line says otherwise; the "(planned)" markers from the M2 draft
are gone because the mechanisms now exist.

---

## 1. What is protected

| Data | Protection | Server can read it? |
|---|---|---|
| Direct and group message bodies | End-to-end encrypted with MLS (RFC 9420) via OpenMLS | **No** |
| Message attachments | AES-256-GCM, key inside the MLS-encrypted message (M6; the round-trip test refetches the raw object and proves it is ciphertext) | **No** |
| Private identity keys | Generated on device, never transmitted (M2) | **No** |
| Local message history | SQLCipher, key wrapped by Windows DPAPI (M2; proven unreadable by an unkeyed connection) | n/a — never leaves the machine |
| Passwords | Argon2id on the client; server stores a hash of a verifier | **No** — the server never sees the password |
| App updates | Manifests minisign-signed; verified against the public key pinned in the app before anything installs (M9, `docs/RELEASING.md`) | n/a — the update *server* is untrusted by design |

## 2. What is NOT protected — read this part

### 2.1 Feed posts and profiles are public and server-readable

This is a design decision, not a gap. End-to-end encryption and a feed readable
by strangers are structurally incompatible: content encrypted to a closed group
cannot also be readable by people who are not in it.

| Data | Reality |
|---|---|
| Home feed posts and their media | Server-readable. Visible to any logged-in user. |
| Profile picture, banner, display name, handle, bio, location, links | Server-readable and public. |

Consequences worth stating plainly:

- **Feed and profile images are stored as plaintext** in the `nexo-media`
  bucket. The server, and anyone who compromises it, can view them.
- **`location` is a free-text profile field.** No geolocation API is used, but
  whatever a user types there is stored server-readable. Per-field visibility
  (M7, G2): bio, location, links and join date each carry an audience —
  everyone, contacts, or only me — **location defaults to private**, and the
  filtering is done by the server in `visible_fields`, never by the client
  choosing what to render. Visibility limits *other users*; it does not limit
  the server, which stores the field either way.

The UI must say this where a user can see it, not only here: a Privacy panel in
Settings carrying this table, and a lock icon shown **only** in E2EE
conversations.

> This differs from the original build prompt, which scoped the feed to the
> user's contacts and asked object storage to hold "only encrypted blobs, never
> plaintext media at rest". The public-feed decision (PLAN.md, "Decisions
> taken") is what changed it. If that decision is revisited, this section and
> the `nexo-media` bucket both change with it.

### 2.2 Metadata

The server sees, and therefore anyone controlling the server sees:

- who talks to whom, and when
- message sizes and timing
- when each account is online
- group membership and when it changes

Message *content* stays unreadable throughout. But metadata is often enough to
infer what matters, and Nexo does not claim otherwise. Sealed-sender-style
techniques are not implemented and are not planned for v0.1.

### 2.3 Link previews tell the link's owner that you read the message

Off by default, and this is why. With previews **on**, opening a conversation
makes this machine fetch the first `https` link in a message — so whoever
controls that URL learns your IP address and roughly when you read it. A
sender can plant a link precisely to find that out.

The alternative is worse, which is why the setting exists at all rather than
the feature being dropped: a *server-side* fetcher would tell the server which
links its users read — metadata the rest of the app refuses to hold — and
would turn the server into a request forwarder for anyone who can send a
message.

What the client will not do, whatever a message asks for: no `http`, no
redirects, no URL that resolves to a private, loopback or link-local address
(checked after DNS resolution, so a public name pointing at `127.0.0.1` is
caught), no response over 256 KB, nothing but HTML, and no image fetch. The
rules live in `apps/desktop/src-tauri/src/preview.rs` next to their tests.

### 2.4 Encrypted attachments are opaque, but their existence is not

The server knows a conversation transferred an object of a given size at a given
time. It cannot learn the filename, the type, or the contents.

---

### 2.5 The unlock PIN is a convenience, and the app now requires it

A PIN is mandatory from this release: signing in on a machine that has none
stops at a screen that asks for one, and Settings offers Change rather than
Remove.

That is a usability decision doing security work, and it is worth being exact
about which. The PIN protects nothing on its own. It is a salted Argon2id
verifier in the DPAPI-wrapped keystore, so it is bound to this Windows account
as well as to the digits — someone with the disk and not the account has
nothing to try it against, and someone with both already has the store. Five
wrong guesses and only the password will do.

What it actually buys is that **auto-lock stays switched on**. Locking drops
the SQLCipher connection and the MLS state (see §3), and it only protects an
unattended machine if people leave it enabled. Without a cheap way back in,
every lock costs a full password, so timers get lengthened and the feature gets
turned off — and a protection that everyone disables is worth less than one
that is slightly weaker and stays on.

It is not a second factor. The server has never heard of it and it cannot sign
anyone in anywhere: it only ever re-opens a store that is already on this
machine.

### 2.6 Blocking is enforced by the server, and does not reach backwards

Blocking removes someone's posts from your feed in both directions and stops
either of you opening a conversation with the other. It is enforced on the
server, not in the client, because a client-side block changes only what one
app draws while the other person goes on sending.

Three limits, stated because the word promises more than it can deliver:

- **It does not stop a second account.** Nexo has no identity verification by
  design, so nothing stops someone registering again.
- **It does not reach messages already delivered.** Those are on the other
  person's machine and the server never had the keys.
- **It does not apply inside a group.** A group is one MLS state shared by
  everyone in it; dropping one member's envelope would leave the others at an
  epoch that member never reaches. Leaving the group is the answer there, and
  the client cannot do that yet.

The blocked person is not told. A refused delivery returns what any other
failure returns, so being blocked is indistinguishable from a message that did
not go through — the one asymmetry the person doing the blocking gets.

### 2.7 The WebView can read the clipboard

The capability set (`apps/desktop/src-tauri/capabilities/default.json`) starts
from empty and grows only deliberately. It grew here. The clipboard used to be
write-only — enough to offer "copy the device key" and nothing more — and it is
now readable as well.

**Why.** The window is transparent, and a menu drawn by the operating system
takes no background from the document: Chromium's own right-click menu inside a
text field came out as unreadable text floating over the desktop. The app draws
that menu itself now, and a text field's menu without Paste is not one.

**What it costs.** Clipboard text already reached the WebView on every Ctrl+V —
the browser puts it in the DOM, where the page can read it. What changes is who
starts it: code running in the WebView can now ask without being asked, so
whatever is on the clipboard at that moment (a password from a manager, say) is
reachable by a compromised renderer rather than only by a paste. In an app whose
WebView already holds decrypted messages this is a small step, but it is a step.

**What it is not.** It is not clipboard *history*, which Windows keeps and Nexo
never asks for, and it does not survive the app: nothing is stored, logged, or
sent anywhere.

## 3. Adversaries in scope

**A network attacker.** Defeated by TLS 1.3 for transport plus MLS for content.
Someone who fully controls the network still cannot read messages, and cannot
forge them without a private identity key.

**A curious server operator.** Can read feed posts, profiles, media in
`nexo-media`, and all metadata in §2.2. Cannot read message bodies or
attachments, and cannot derive the group secrets to try.

**Anyone who can send the server requests.** **Open — no rate limiting exists.**
`apps/server/src/lib.rs` composes the router with a trace layer and nothing
else, so none of BRIEF §4.5's three limits is in force. Two consequences worth
naming rather than leaving implied:

- `/v1/auth/login` runs Argon2id at 19 MiB per attempt on the server. Unlimited,
  it is both a password-guessing oracle and a memory-exhaustion lever against a
  single small machine. `/v1/auth/salt` is unauthenticated by construction.
- `/v1/keypackages/{handle}` **consumes** a KeyPackage per call. A loop can
  exhaust an account's supply, after which nobody can start a conversation with
  that person — a denial of service the victim is never shown an error for.

Tracked as B2 in `docs/RESEARCH-COMPARISON.md`.

**Someone holding the disk from a decommissioned server.** **Open — no disk
encryption is configured, and the decision is deliberately deferred**
(`OPS.md` Phase 0.2, which now lists what is actually on that disk). Until it
is made, assume the unencrypted case, which is this: message content is safe
regardless — it is MLS ciphertext and the group keys never leave the devices —
while the JWT signing key, the S3 credentials, the profile and feed data, the
password hashes and all of §2.2's metadata would be readable. Rotating the JWT
key and the S3 credentials at decommissioning is what neutralises the first
two, and belongs in the shutdown checklist whichever way the decision goes.
The decision itself has to be made **before the production server is built**:
encrypting a root volume afterwards means rebuilding the machine.

**Someone who steals the user's laptop, powered off.** The local store is
SQLCipher-encrypted with a key wrapped by DPAPI under the user's Windows
account. Without that account's credentials the database is unreadable — built
at M2 and proven by a test that opens `store.db` with an unkeyed connection
and fails.

**Someone who sits down at an unattended, unlocked machine.** Auto-lock (M8):
after a configurable idle period the SQLCipher connection is closed and the
in-memory MLS state is dropped; unlocking is a full sign-in. What lock does
*not* claim: it cannot guarantee freed memory is zeroed — the allocator and
the OS decide that — so it defends against the person at the desk, not
against someone who can already read this process's memory. `lock.rs` states
the same limitation next to the code.

**A compromised or impersonated update server.** It can refuse updates
(freeze attack — visible, since checks are user-initiated from the About
panel) but cannot inject one: manifests are minisign-signed and verified
against the public key pinned in the app, and there is no unsigned install
path. Losing control of the *signing key* is out of this defence's scope —
see `docs/RELEASING.md` for how it is held.

**A stranger reading the feed.** Not an adversary — the feed is public by
design. Listed here so it is never mistaken for a breach.

---

## 4. Adversaries explicitly OUT of scope

**Malware running as the user on their own machine.** It can read decrypted
messages out of the app's memory, key the DPAPI store, or screenshot the window.
No messenger defends against this; claiming otherwise would be dishonest.

**A compromised server that swaps public keys.** The server distributes identity
keys, so a malicious one can hand you an attacker's key for a contact. The only
defence is users actually comparing **safety numbers** out of band.

> **Not yet true, and this is the gap that matters most.** This section used to
> claim Nexo "warns loudly and non-dismissably when a key changes". It does not.
> Nothing stores a peer's identity key, so nothing can notice that it changed:
> the safety number is computed on demand from the live group, and the
> "verified" mark is a boolean in WebView `localStorage` that is not bound to
> any key. The ceremony is therefore **one-shot** — someone who compared digits
> in week one is never told when the answer changes in week two, which is
> exactly the attack this section is about.
>
> Worse in combination: signing in on a machine with no local store generates a
> **fresh identity keypair** (`crates/client/src/session.rs:199`), and the
> server accepts it as an additional device. A reinstall therefore rotates the
> account's cryptographic identity silently, and looks — correctly — like the
> attack above to anyone who was checking.
>
> Tracked as B1 and S1 in `docs/RESEARCH-COMPARISON.md`. Until they land, treat
> a key-substituting server as **in scope and undefended**, not as mitigated by
> a ceremony the app does not support.

A user who never checks is vulnerable to this regardless.

**Traffic analysis.** See §2.2.

**Someone who knows the password.** Changing it (§6.4) requires the current
password as well as a signed-in session, and retires every other session — but
someone who knows the current password can simply change it first. There is no
second factor and no recovery flow to appeal to, by design (§4 above).

**The hosting provider, against a running machine.** Hetzner can snapshot the
RAM of a live VM, and a LUKS key sits in that RAM the whole time the server is
up. Disk encryption protects a disposed disk, not a running one. If a provider
with physical access is genuinely in your threat model, the answer is hardware
you own.

**A user who loses their only device.** One device per account, and the identity
key is local-only. Losing it loses the account and all history, because
server-side ciphertext is deleted on acknowledgement. This is the correct
security posture and a permanent support burden; the registration UI must say so
before it happens. Encrypted key backup behind a recovery code is a v0.2 answer.

---

## 5. Cryptographic choices, and what they are not

- **No cryptography is invented here.** OpenMLS and its audited primitives only.
- The ciphersuite is `MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519` — the
  mandatory-to-implement suite.
- **Not post-quantum.** A harvest-now-decrypt-later adversary recording traffic
  today may be able to read it if a cryptographically relevant quantum computer
  arrives. A hybrid suite is a config seam, deliberately off in v0.1.
- **The password scheme is not a PAKE.** The client derives a verifier with
  Argon2id and sends it over TLS; the server stores a hash of that verifier. A
  server that is compromised *at the moment you log in* sees the verifier. This
  is better than sending a password and worse than a real PAKE (OPAQUE), and it
  is chosen for implementation simplicity in v0.1.

## 6. Object storage

Two buckets, deliberately separated:

| Bucket | Contents | Encrypted? |
|---|---|---|
| `nexo-media` | feed and profile images | **No** — plaintext, server-readable by design (§2.1) |
| `nexo-enc` | message attachments | Yes — AES-256-GCM, key never leaves the clients |

Hetzner S3 credentials are **project-wide by default**: every key reaches every
bucket in the project. Two separate credential pairs are not sufficient on their
own. A bucket policy on `nexo-enc` denying the media key is what makes the
separation real, and
`cargo test -p nexo-server --test s3_smoke -- --ignored` is the only evidence it
holds. Re-run it after any credential rotation — a newly generated key is
unrestricted, and the policy names the old one.

---

## 7. Reporting a vulnerability

Open a private security advisory on the repository rather than a public issue.
There is no bug bounty.

## 8. What must be updated here, and when

- the disk-encryption decision from `OPS.md` Phase 0.2, once made — **still
  open, and deferred deliberately**; §3 assumes the unencrypted case and says
  what that exposes. Due before the production server is built, because there
  is no in-place path afterwards.
- ~~M2, when the local store and keyring exist~~ — done, markers removed
- ~~M7, when per-field profile visibility ships~~ — done, §2.1 revised
- ~~M9, the update channel~~ — done: §1, §3, and `docs/RELEASING.md`
- TLS pinning, if it ever ships — `docs/PIN-ROTATION.md` records the decision
  not to pin in v0.1 and what shipping it would require
- any change to the public-feed decision, which rewrites §2.1 and §6
