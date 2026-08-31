# Releasing Nexo (M9)

How a commit becomes a signed installer that existing installs pick up.

Two signatures are involved, and they answer different questions:

| Signature | Key | Question it answers |
|---|---|---|
| **Authenticode** on the `.exe` installer | Code-signing certificate (plan risk 1) | Windows/SmartScreen: "who published this file?" |
| **minisign** on the updater artifact | Tauri updater keypair | The running app: "did the same project sign this update?" |

Neither substitutes for the other. Authenticode gets the first install past
SmartScreen; minisign is what stops a compromised update server from feeding
new builds to every existing install. The updater checks minisign only —
`plugins.updater.pubkey` in `tauri.conf.json` is the trust anchor, so the
update *server* is untrusted by design.

## One-time setup

### 1. The updater keypair

```powershell
pnpm tauri signer generate -w $env:USERPROFILE\.tauri\nexo-updater.key
```

- The **public** key goes into `apps/desktop/src-tauri/tauri.conf.json` under
  `plugins.updater.pubkey`. **This is already done** — the key pinned there is
  the one v0.1.1 onwards were signed with, and regenerating the keypair
  without re-pinning it is the mistake `scripts\release.ps1` refuses to let you
  make. A build with an empty pubkey reports "Updates are not configured in
  this build" on check, which is deliberate: a dev build says so rather than
  pretending to have checked.
- The **private** key and its password go into the GitHub Actions secrets
  `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and
  into the password manager. **Never into the repo, never echoed in CI logs**
  (brief §8).
- Losing the private key means no existing install ever accepts an update
  again — they would have to be reinstalled by hand. Treat it like the TLS
  key, not like a build setting.

### 2. The code-signing certificate

Plan risk 1: EV on FIPS hardware or a cloud signing service, weeks of lead
time, and the definition of done is honest either way — "signed, with
reputation accruing" if EV is not viable. Since the key lives on a token or in
a cloud HSM, signing is configured as an external command rather than a file
in the repo: set `bundle.windows.signCommand` in `tauri.conf.json` (or the
`TAURI_WINDOWS_SIGNTOOL_ARGS` route your provider documents) once the
certificate exists. Nothing about the certificate belongs in the repository.

## The two paths, and which one is real

Both end at the same place — three files attached to a GitHub release — but
they are not interchangeable, and it is worth knowing which one you are using.

| | `scripts\release.ps1` | `.github/workflows/release.yml` |
|---|---|---|
| Runs on | your Windows machine | a clean `windows-latest` runner |
| Signing key | `%USERPROFILE%\.tauri\nexo-updater.key` | the `TAURI_SIGNING_PRIVATE_KEY` secret |
| Bumps the version | yes | no — it checks that the tag already agrees |
| Starts | when you run it | when a `v*` tag is pushed |

The script pushes the tag, so **running the script starts the workflow too**.
That is deliberate rather than a collision: whichever finishes second ensures
the release exists, then replaces all three files together with `--clobber`,
and takes the release notes from the release body when the script already
wrote them. Either way one self-consistent set ends up attached.

Every release through v0.1.3 came from the script, because the workflow had
never run a single step — it declared `if: ${{ secrets.X == '' }}` on a step,
and the `secrets` context is not available there. GitHub rejects the file at
validation time and reports that as a failed *run* on every push, tag filter or
not, with zero jobs inside it. Thirty-seven red crosses and nothing behind any
of them. Both the condition and the manifest it would have written (wrong
filename, wrong host — see below) are fixed.

## Every release

One command:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "<the updater key's password>"
.\scripts\release.ps1 -Notes "What changed, one paragraph."
```

It bumps the patch version, builds, signs, writes the manifest, commits, tags,
and creates the GitHub release with all three artifacts attached.

`-Version 0.2.0` for a minor or major bump instead of the patch. `-NoPublish`
stops before touching GitHub. `-SkipBuild` reuses what is already built, which
is only correct when it was built from the current version.

### Why the bump comes first

The version is compiled **into** the binary. Bumping after the build produces a
manifest advertising a version the installer does not report — every client
downloads it, installs it, still sees the old version, and updates again
forever. The script does them in the only order that works, which is the whole
reason it exists rather than a list of steps in this file.

`Cargo.toml` and `tauri.conf.json` are bumped together and must agree: the
updater compares the manifest's version with the one the running binary
reports, and those come from different files.

### The manifest

`latest.json`, attached to the release and fetched from
`https://github.com/<owner>/<repo>/releases/latest/download/latest.json`:

```json
{
  "version": "0.1.1",
  "notes": "What changed, one paragraph.",
  "pub_date": "2026-09-01T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<contents of the .sig file>",
      "url": "https://github.com/<owner>/<repo>/releases/download/v0.1.1/Nexo_0.1.1_x64-setup.exe"
    }
  }
}
```

Tauri's other form — a per-version endpoint answering `204 No Content` when the
caller is current — needs a host that can serve one response per installed
version. GitHub Releases serves static assets, so this is the static form: the
client fetches one file and compares versions itself. The signature is what
makes the host untrusted either way.

The tag must be exactly `v<version>`; the download URL is built from it. The
filename matters as much as the contents: `plugins.updater.endpoints` points at
`.../releases/latest/download/latest.json`, so a manifest attached under any
other name is a 404 the app reports as "could not check for updates".

The workflow re-checks the tag against `Cargo.toml` and `tauri.conf.json`
before it builds anything, and fails the release if the three disagree. That is
the one failure mode the whole ordering above exists to prevent, and it is
cheap to assert.

## Third-party notices

MIT and every other permissive licence in the tree put the same duty on whoever
distributes a copy: the copyright and permission notices travel with it. The
source on GitHub satisfies that. The installer is also a copy, and on its own it
does not — so the notices ship with the build, in the About panel (brief §236).

Regenerate them when dependencies change, not every release:

```powershell
cargo about generate --format json > third-party-notices.json
```

Then add by hand the three things `cargo-about` and `cargo deny` both miss,
because they read crate metadata and these are vendored C source:

- **SQLCipher**, BSD-3-clause, © Zetetic LLC — via `libsqlite3-sys`, which
  declares only `MIT` for its own wrapper.
- **OpenSSL 3.6.3**, Apache-2.0 — vendored by `openssl-src` under
  `bundled-sqlcipher-vendored-openssl`.
- The **OpenSSL advertising acknowledgement** required by `aws-lc-sys`'s
  licence, which by its wording reaches the marketing page as well as the app:

  > This product includes software developed by the OpenSSL Project for use in
  > the OpenSSL Toolkit.

[`docs/LICENSING.md`](LICENSING.md) has the reasoning and the per-licence
obligations.

### Before announcing

Install the previous version in a clean Windows 11 VM and open the app. It
should update on its own and come back on the new version. This is M9's
definition of done and it is not delegable to CI.

## What the client does with all this

- `check_update` (About panel) fetches the manifest, verifies the minisign
  signature against the pinned pubkey, and reports the version or "you're
  current".
- `install_update` downloads, re-verifies, runs the NSIS installer, and
  restarts. Tauri downloads full installers rather than diffs; at this bundle
  size that is fine, but measure it (brief §8).
- A manifest the key did not sign is an error, never an install. There is no
  "skip verification" path to configure, which is the point.
