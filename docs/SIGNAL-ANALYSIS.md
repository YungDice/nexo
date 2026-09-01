# What Signal builds, and which parts Nexo should

A read of [Signal-Server](https://github.com/signalapp/Signal-Server) and
[Signal-Desktop](https://github.com/signalapp/Signal-Desktop) at the engineering
level: what mechanisms exist, and which of them transfer to Nexo given the three
architectural choices Nexo has already made — **MLS instead of Double Ratchet**,
**handles instead of phone numbers**, and **one device per account**.

Companion to [`RESEARCH-COMPARISON.md`](RESEARCH-COMPARISON.md), which compares
the two products feature by feature. This one is about code and mechanism, and
it corrects that document where Nexo has moved on since it was written.

---

## 1. Where Nexo is already level with Signal, or ahead

Worth stating first, because three of these look like gaps and are not.

**Local database key protection — Nexo got there first.** Signal Desktop stored
its SQLCipher key *in plaintext* in a JSON config file until July 2024, when it
adopted Electron's `safeStorage` API after
[public pressure](https://www.bleepingcomputer.com/news/security/signal-downplays-encryption-key-flaw-fixes-it-after-x-drama/).
On Windows `safeStorage` is DPAPI — which is exactly what Nexo has wrapped its
store key with since the beginning. Nexo also has no Linux fallback problem;
Signal's key
[breaks when a user moves between GNOME and KDE](https://yingtongli.me/blog/2025/08/13/signal-secrets.html)
because the secret lives in whichever keyring was present at setup.

**No phone number means an entire subsystem does not exist.** Signal's server
carries `telephony`, `registration`, `captcha` and a private contact discovery
service running in SGX enclaves — all of it downstream of the decision to
identify people by phone number. Nexo identifies by handle and needs none of it.
This is the single largest complexity saving in the comparison.

**Groups.** Signal builds groups on pairwise Double Ratchet sessions with
sender-key fanout. Nexo uses MLS (RFC 9420), where a group is one cryptographic
object and membership changes are O(log n). For groups specifically, Nexo's
choice is the more modern one.

**Key-change detection now exists.** `RESEARCH-COMPARISON.md` §2 says
"absent for change detection". That is out of date — `mark_verified`,
`acknowledge_key_change` and `SyncOutcome::key_changes` were added in `b530cf5`.
What is still missing is verification *without* an out-of-band channel, which is
§3.5 below.

---

## 2. Worth adopting, in order

### 2.1 Leaky bucket, and limits that survive a restart

**Signal:** `limits/LeakyBucketRateLimiter.java`, backed by Redis, with
`RateLimitByIpFilter`, `CardinalityEstimator` for distinct-source abuse, and
per-endpoint `RateLimiterConfig`.

**Nexo today:** a hand-written fixed-window counter in `apps/server/src/limits.rs`,
held in process memory.

Two concrete weaknesses, both inherent to the shape rather than the numbers:

- **Fixed windows allow a 2× burst across a boundary.** Twenty posts at 11:59:59
  and twenty more at 12:00:00 is forty in one second against a 20/minute limit.
  A leaky bucket has no boundary to straddle.
- **In-process counters reset on restart and do not span servers.** Every deploy
  currently clears every limit, and the moment there is a second server the
  limits halve in effectiveness.

This is the cheapest item on the list and the one that touches code just
written. A leaky bucket is roughly the same amount of code as the current window.

### 2.2 Challenges instead of refusals

**Signal:** `PushChallengeManager`, `RateLimitChallengeManager`,
`RateLimitChallengeOption` — a client that hits a limit is offered a way to
prove it is a person (a push round trip, or a CAPTCHA) rather than simply being
told no.

**Why it matters for Nexo:** a hard 429 punishes the enthusiastic user exactly
as hard as the script. Since Nexo has no phone number to fall back on, and no
account recovery, a wrongly rate-limited user has no route back. A challenge
gives them one.

### 2.3 Reproducible builds — the highest-value item for Nexo's positioning

**Signal:** a top-level `reproducible-builds/` directory in Signal-Desktop.

**Nexo:** none, and installers are unsigned (`docs/RELEASING.md`).

This is the recommendation to take most seriously, because it is the one that
props up the public claim. Nexo's argument is *"the source is open, so you can
check that we cannot read your messages."* That argument has a hole in it: what
users install is a binary, and nothing today connects that binary to the
published source. Reproducible builds close it. They are also considerably
cheaper than an EV certificate and independent of it — Authenticode proves who
built it, reproducibility proves *what* was built. Different questions.

### 2.4 Sealed sender — the metadata gap already admitted

**Signal:** the sender's identity is encrypted inside the envelope; the server
learns who to deliver to and not who sent it, using delivery tokens.

**Nexo:** `conversation_members` maps conversations to user ids, so the server
knows who talks to whom and when. `THREAT-MODEL.md` §2.2 says so honestly.

This is the gap that matters most for the "Chat Control" framing, because the
social graph is precisely what such a request asks for first. Adapting sealed
sender to MLS is real work and is not a weekend, but nothing about MLS prevents
it — and the honest interim position is the one the threat model already takes.

### 2.5 Key transparency

Signal launched
[Automatic Key Verification](https://signal.org/blog/automatic-key-verification/)
on **11 August 2026** — a month ago. Registrations and key changes go into a log
tree searchable via prefix trees, audited independently by Cloudflare and Trail
of Bits, with identifiers hidden behind a VRF so auditors never see plaintext.
WhatsApp shipped key transparency in 2023, iMessage in 2024, Messenger in
November 2025, and it is now being standardised at the IETF.

**Relevance to Nexo:** safety numbers only help people who compare them, which
in practice is almost nobody. Key transparency is how the field is closing that.
Nexo is too small to run auditors, but two things are worth knowing: the design
is being standardised, so it can be adopted rather than invented; and **handles
are easier to put in a transparency log than phone numbers**, because there is
no privacy-preserving lookup problem to solve first.

Not now. But it belongs on the roadmap, and the current safety-number UI should
not be built in a way that has to be thrown away.

### 2.6 Message features Nexo's protocol cannot express

`Payload` in `crates/protocol/src/lib.rs` has exactly four variants: `Text`,
`Attachment`, `Rename`, `GroupAvatar`. Signal has replies, edits (10 within 24 h),
delete-for-everyone, disappearing messages with a default timer, reactions,
typing indicators and read receipts.

Each is a new `Payload` variant and a UI affordance — individually small. The
one worth doing first is **replies**, because its absence is most obvious in a
group, followed by **delete-for-everyone**, which people expect as a safety
valve after a mistake.

### 2.7 Backups

Signal has encrypted backups. Nexo has no export path at all — the local store
is the only copy, so a lost machine is lost history. For a product with no
account recovery *by design*, having no backup either means one disk failure
ends someone's use of the app permanently. These two decisions compound, and
they should not both stand.

---

## 3. What not to copy

- **Secure Value Recovery.** Signal's PIN-based recovery runs in SGX enclaves
  with guess limits. It is a large operational burden, and Nexo has deliberately
  chosen no account recovery. Encrypted local backups (§2.7) address the same
  user pain at a fraction of the cost.
- **Phone numbers and contact discovery.** The whole enclave-based CDS apparatus
  exists to make phone-number lookup private. Nexo does not have the problem.
- **Signal's group construction.** Pairwise sessions with sender-key fanout is
  what you build when you started from Double Ratchet in 2013. MLS is better here.
- **`subscriptions`, `badges`, `currency`, `sticker-creator`.** Product surface
  irrelevant to Nexo.

---

## 4. Suggested order

| | Item | Size | Why here |
|---|---|---|---|
| 1 | Leaky bucket + Redis-backed limits | S | Touches code just written; fixes a real 2× burst |
| 2 | Encrypted backup / export | M | No recovery *and* no backup is the riskiest pairing in the product |
| 3 | Reproducible builds | M | Makes the public claim verifiable; independent of code signing |
| 4 | Replies, then delete-for-everyone | S each | Most-noticed absences |
| 5 | Rate-limit challenges | M | Needed once real users hit limits |
| 6 | Sealed sender | L | Closes the admitted metadata gap |
| 7 | Key transparency | XL | Where the field is going; adopt the IETF standard, do not invent |

---

## Sources

- [Signal-Server](https://github.com/signalapp/Signal-Server) — package layout under `service/src/main/java/org/whispersystems/textsecuregcm`, and `limits/`
- [Signal-Desktop](https://github.com/signalapp/Signal-Desktop) — top-level layout, `reproducible-builds/`
- [Introducing Automatic Key Verification](https://signal.org/blog/automatic-key-verification/) — Signal, 11 August 2026
- [Automatic Key Verification](https://support.signal.org/hc/en-us/articles/10223569377562-Automatic-Key-Verification) — Signal Support
- [Signal downplays encryption key flaw, fixes it after X drama](https://www.bleepingcomputer.com/news/security/signal-downplays-encryption-key-flaw-fixes-it-after-x-drama/) — BleepingComputer
- [Migrating Signal Desktop keyring backend](https://yingtongli.me/blog/2025/08/13/signal-secrets.html) — on the Linux keyring split
- [Protecting Signal Keys on Desktop](https://cryptographycaffe.sandboxaq.com/posts/protecting-signal-desktop-keys/) — Cryptography Caffè
