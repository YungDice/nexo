# Licensing

What Nexo is licensed under, what that obliges us to do when we ship a binary,
and where the licence stops covering us. Written for a Swiss project: the code
is MIT, but a warranty disclaimer written for US law does not survive contact
with Art. 100 OR unchanged, and the statutory limits are named below rather
than assumed away.

This file is a description, not legal advice. The three items under
[Not a licensing question](#not-a-licensing-question) are the ones worth a
lawyer's hour; nothing in this repository resolves them.

## What is licensed, and by whom

Everything in this repository — Rust crates, the React client, the design
tokens, the scripts and the documentation — is under the **MIT licence**, the
text in [`LICENSE`](../LICENSE). `license = "MIT"` in `Cargo.toml` says the same
thing for the published crate metadata, and the two must not drift apart.

The copyright line names `filiusfetish`. Two people write code here,
`bananaaboy` and `YungDice`, which under Swiss law makes this **joint authorship
(Miturheberschaft, Art. 7 URG)**: the work belongs to both, and unless they
agree otherwise, both must consent to relicensing it — a later move to
Apache-2.0, a dual licence, or a commercial licence for one customer cannot be
done by one of them alone. A pseudonym is a valid way to hold copyright in
Switzerland (the right arises in the human being, not in the name), but
enforcing it means first proving that the handle is you.

Two things follow, and both are cheap now and expensive later:

- Put the authorship split in writing between the two of you — who holds which
  share, who may license, what happens if one leaves. An informal split is the
  usual reason an otherwise clean open-source project cannot be relicensed
  years on.
- If either of you wrote any of this in the course of employment, note that
  **Art. 17 URG** transfers software copyright to the employer by default. That
  would make the employer a rights holder here, regardless of what `LICENSE`
  says.

The upstream repository is <https://github.com/YungDice/nexo>.

## Why `LICENSE` is not edited

The MIT text in `LICENSE` is byte-for-byte the canonical wording, and it stays
that way. This is deliberate and it is the safest available choice:

- Licence scanners, `cargo deny`, GitHub's licence detection and every
  corporate open-source review match MIT by comparing the text. One added
  sentence turns it into an unrecognised custom licence, which many companies
  are required to reject outright.
- Adding a governing-law clause, a liability cap or a Swiss venue to the MIT
  text would not make MIT stronger — it would make it something that is no
  longer MIT while still being labelled as such, which is worse than either
  option on its own.

Everything that needs saying about Swiss law is therefore said here, in prose,
outside the licence.

## What MIT obliges *us* to do

MIT is short but it is not free of duties, and the duty lands on the
distributor — us:

> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.

Shipping the source on GitHub satisfies this. **Shipping `Nexo_x.y.z_x64-setup.exe`
does not**, on its own. A binary installer is a copy of the Software, so the
copyright notice and the permission notice have to travel with it — ours and
every dependency's whose licence says the same thing (MIT, BSD-2, BSD-3, ISC,
Apache-2.0, Zlib all do).

That is a release step, not a repository state. The brief already reserves the
place for it: §236's **About panel** ("version, licences, update check") is
where the notices belong, and
[`docs/RELEASING.md`](RELEASING.md#third-party-notices) records it as part of
what has to be true before a build is announced.

## What MIT does not do, under Swiss law

The all-caps paragraph in `LICENSE` disclaims warranty and liability in terms
written for US law. In Switzerland it is partially enforceable, and it is worth
knowing exactly which part:

- **Liability for intent and gross negligence cannot be excluded in advance.**
  Art. 100 Abs. 1 OR voids such a clause. The disclaimer does cover ordinary
  negligence (leichtes Verschulden), which is most of what it is there for.
- **Product liability for personal injury or damage to property cannot be
  excluded at all** — Art. 8 PrHG. For a messenger this is close to
  theoretical, but no wording removes it.
- Because the software is handed over free of charge, the standard of care is
  measured against a gratuitous transfer (Schenkung, Art. 248 OR) rather than a
  sale, which works in our favour.
- **MIT names no governing law and no venue.** For a Swiss rights holder and a
  Swiss user, the IPRG decides, and the answer depends on the dispute. This is
  a gap in MIT, not a defect in this repository, and it is not fixable by
  editing the licence (see above).

The distinction that actually matters: **MIT licenses the software; it says
nothing about operating the service.** The moment `api.dice.fit` accepts a
registration, that is a separate legal relationship with a user, governed by
terms of service and a privacy notice that do not exist in this repository yet.
No licence text will stand in for them.

## Dependency licences, and the gate that enforces them

[`deny.toml`](../deny.toml) allows permissive licences only, and CI fails on
anything else. The reason is recorded there and it is a real one: `libsignal` is
AGPL-3.0-only, and linking it into a distributed desktop client would push
copyleft onto the whole application. OpenMLS is MIT, which is why it is the
crypto core.

What each allowed family asks of us as a distributor:

| Licence | What it obliges |
|---|---|
| MIT, BSD-2-Clause, BSD-3-Clause, ISC, Zlib | Ship the copyright and permission notice with the binary. |
| Apache-2.0 (incl. WITH LLVM-exception) | §4: ship the licence, pass on any `NOTICE` file, mark modified files. Also grants a **patent licence** — a reason to prefer it, not merely tolerate it. |
| MPL-2.0 | **File-level copyleft.** Unmodified use as a library needs only the notice and a pointer to upstream source. If we ever *modify* an MPL file and ship it, that file's source must be made available. In this tree the crate is `option-ext 0.2.0`, reached through `dirs-sys`; we do not modify it. |
| Unicode-3.0, CDLA-Permissive-2.0 | Data licences, no copyleft. `webpki-roots` carries Mozilla's root CA bundle under CDLA-Permissive-2.0 — certificate data, not code. |
| BSL-1.0, CC0-1.0 | Notice-only and public-domain-equivalent respectively. |
| `OpenSSL` | The **old** OpenSSL/SSLeay licence, and the only entry on the list with a clause that reaches beyond the binary — see below. |

### The advertising clause, precisely

`aws-lc-sys 0.44.0` (pulled in by `jsonwebtoken`'s `aws_lc_rs` backend and by
the S3 SDK's TLS) declares `OpenSSL` in its SPDX expression, because it carries
BoringSSL/OpenSSL-derived C code. Clause 3 of that licence requires an
acknowledgement in **advertising materials** mentioning features or use of the
software:

> This product includes software developed by the OpenSSL Project for use in the
> OpenSSL Toolkit.

Read literally that reaches the marketing page at `nexo.dice.fit`, not just the
About panel. The cheap and complete answer is to carry the acknowledgement in
the third-party notices and in the download page footer, and then stop thinking
about it.

Note the contrast, because it is easy to get backwards: `openssl-src 300.6.1`
vendors **OpenSSL 3.6.3**, and OpenSSL relicensed to **Apache-2.0 as of 3.0** —
that copy carries *no* advertising clause. The clause comes from `aws-lc-sys`,
not from OpenSSL itself. The `OpenSSL` entry in `deny.toml` is therefore earning
its place for a different crate than the comment history suggests, and is worth
re-reading next time that list is touched.

### The blind spot in the gate

**`cargo deny` reads crate metadata, not vendored C source.** Two of the native
libraries we ship are invisible to it:

- `libsqlite3-sys 0.30.1` declares `MIT` — its own wrapper code. What it
  actually compiles, under the `bundled-sqlcipher-vendored-openssl` feature, is
  **SQLCipher** (BSD-3-clause, © Zetetic LLC) and **OpenSSL 3.6.3**
  (Apache-2.0). Neither notice is in any crate's `license` field.
- The same applies to `aws-lc-sys`'s bundled C, which is where the clause above
  comes from.

So a green `cargo deny check licenses` does **not** mean the notices we ship are
complete. The Zetetic copyright and the two OpenSSL-related notices have to be
carried by hand. This is the single most likely way for this project to be in
technical breach of a licence, and it is entirely avoidable.

Generating the list of the rest is mechanical:

```powershell
cargo install cargo-about
cargo about generate --format json > third-party-notices.json
```

`cargo-about` has the same blind spot, so the three vendored notices above are
appended to whatever it produces, not replaced by it.

## Export control

Nexo is cryptographic software, so Swiss export control is a fair question. The
short version:

- Cryptography for data confidentiality falls under **dual-use category 5 part
  2**, implemented in Switzerland through the **GKG** (Güterkontrollgesetz) and
  **GKV** (Güterkontrollverordnung), following the Wassenaar lists.
- Software **in the public domain** — publicly available without restriction on
  further dissemination — is outside those lists. The source is public on
  GitHub, so the source distribution is not controlled.
- For the compiled installer, the **General Software Note** exempts software
  that is generally available to the public, downloadable by anyone without
  restriction and installable by the user without further support from us. A
  free public GitHub release matches that.

Our reading is therefore that no export licence is required for what this
project does today. Two things would change the analysis and should trigger a
question to **SECO** before they happen: selling the client commercially or
under a restrictive agreement, and adding cryptographic functionality that is
not publicly available open source.

## Not a licensing question

These are outside what any licence file can settle, and they are where the real
exposure is:

- **The name.** "Nexo" is in active trademark use by an unrelated company in
  crypto-finance, and "nexo" is also the name of a card-payment standard.
  Classes 9, 38 and 42 — software, telecommunications, hosted services — are
  exactly this product's classes. Before a public launch, a store listing or any
  paid promotion, run a search on **Swissreg** and **EUIPO** for the classes we
  would occupy. A rename is cheap now and expensive after the first install
  base.
- **BÜPF / VÜPF.** Operating a communications service from Switzerland makes us
  at minimum a *provider of derived communication services* (Anbieterin
  abgeleiteter Kommunikationsdienste, Art. 2 BÜPF), with duties to cooperate
  with lawful surveillance requests. End-to-end encryption is not a way out of
  those duties: what is owed is the data we hold, and
  [`README.md`](../README.md) is already blunt that we hold conversation
  metadata — who talked to whom, when, and message sizes. That is precisely the
  category that gets requested. The Threema litigation is the relevant
  precedent on how lightly a small E2EE provider can be classified.
- **revDSG and GDPR.** A privacy notice, a record of processing, and a data
  processing agreement with Hetzner (an EU/EEA processor, so no transfer
  problem) are required before real users exist — not because of the licence,
  but because of the service.
