# Hetzner runbook

How to stand up the Nexo backend on Hetzner, in order.

**You do not need any of this until M4.** M2 (auth, local encrypted store) and M3
(MLS in isolation) run entirely on a development machine against a local
Postgres. Provisioning early costs about €8/month and buys familiarity; it does
not unblock anything before M4.

Everything here is Hetzner except two things it cannot supply: the Authenticode
code-signing certificate (M9 — that comes from a CA) and CI (GitHub Actions,
because the client is a Windows binary and needs a Windows runner).

---

## Phase 0 — Decisions to make before you click anything

### 0.1 Do you control `delidev.net`?

The whole design hardcodes `api.delidev.net`, `www.delidev.net` and
`updates.delidev.net`. Hetzner is an ICANN-accredited registrar but its TLD list is
conventional (`.com`, `.de`, `.net`, `.org`, `.eu`, `.ch`, `.at` …) and almost
certainly excludes `.fit`. That is fine: leave the registration wherever it is
and point the nameservers at Hetzner DNS. You do not need to transfer it.

### 0.2 Full-disk encryption: read this before choosing

The brief calls for LUKS on the VPS, and the threat model puts *"someone with
the offline disk"* in scope. Be precise about what that buys on a cloud VM.

**LUKS on a cloud VM protects against one thing: a decommissioned or reused disk
being read later.** It does **not** protect against the hosting provider, who
can snapshot the RAM of a running machine — and the LUKS key is in that RAM the
entire time the server is up. No VPS configuration changes that. If a provider
with physical access is genuinely in your threat model, the answer is hardware
you own, not a different disk layout.

It also has a real operational cost. An encrypted root cannot boot unattended:
every reboot stops in the initramfs waiting for a passphrase, which you supply
by SSHing to a dropbear listener on a second port and running `cryptroot-unlock`.
Unattended reboots, kernel updates and automatic recovery all stop being
automatic.

Three honest options:

| Option | Protects against disposed disks | Boots unattended | Notes |
|---|---|---|---|
| **A.** No disk encryption | No | Yes | Simplest. Say so plainly in `THREAT-MODEL.md`. |
| **B.** LUKS root + dropbear unlock | Yes | **No** | What the brief asks for. Every reboot needs you present. |
| **C.** LUKS on a separate data volume | Yes, for the data | Yes, if the key is on the encrypted-at-rest root — which mostly defeats the point | A middle road that is easy to fool yourself with. |

Recommendation: **B if you mean the threat model literally, A if you would
rather be honest than ceremonial.** Do not pick C without writing down exactly
where the key lives and what that implies. Whichever you choose, record it in
`docs/THREAT-MODEL.md` — the brief's rule 5 is about being straight, and the
server's disk is part of that.

**Decide before the server is built, not after.** Encrypting a root volume
retroactively means rebuilding the machine from the rescue system; there is no
in-place path. This is the one decision in Phase 0 with no second chance.

#### What is actually on that disk

Written down 2026-08-26 so the decision can be made from the contents rather
than from the principle. What LUKS would protect, and what it would not:

| On the disk | Value to someone holding it | Encrypted already? |
|---|---|---|
| `envelopes.ciphertext` | **None.** MLS ciphertext; the group keys exist only on the devices. Undelivered rows are purged after 30 days, delivered ones on acknowledgement. | Yes, end to end |
| `jwt-ed25519.pem` | **The highest.** Forges an access token for any account — but still cannot read a message. | No |
| `nexo.env` | Postgres password, S3 credentials | No |
| Profiles: handle, display name, `bio`, `location` | Public by design, except where per-field visibility restricts *other users* — never the server | No |
| Feed posts and their media keys | Public by design (§2.1 of the threat model) | No |
| `pw_salt`, `pw_hash` | An offline attack on passwords, over Argon2id twice — expensive, not impossible | Hashed |
| Metadata: who talks to whom, when, group membership | Real, and §2.2 already says the running server sees all of it | No |

Two things follow, and they cut in opposite directions:

- **LUKS adds nothing to the thing this app exists to protect.** Message
  content is already unreadable to anyone holding this disk.
- **The most dangerous plaintext is also the most easily neutralised.**
  Rotating the JWT key and the S3 credentials when the server is
  decommissioned kills every stolen token and every bucket write, in about
  five minutes — and that rotation is worth doing whichever option is chosen.
  Put it in the decommissioning checklist either way.

What LUKS genuinely buys, then, is the metadata and the password hashes on a
disposed disk. Weigh that against a single-operator, single-server messenger
that cannot boot unattended: a kernel update or a Hetzner host migration at
04:00 leaves Nexo down until someone is awake to type `cryptroot-unlock`. For
a messenger, being reachable is itself a security property — one that cannot
deliver pushes people back to SMS.

Note that the **backups** are a separate question with an easier answer: Borg
encrypts natively (Phase 9), so the copies that live longest are covered
regardless of what the server's own disk does.

Note also that most published Hetzner LUKS guides target **dedicated (Robot)**
servers and their `installimage` tool. On Hetzner **Cloud** there is no
`installimage`; you boot the rescue system and drive `cryptsetup` by hand. Budget
an evening, and follow Hetzner's own tutorial rather than a blog post.

---

## Phase 1 — Account and project

1. Create an account at <https://console.hetzner.cloud>.
2. Enable two-factor authentication immediately. This account will shortly hold
   your server, your object storage and your backups — see the consolidation
   warning at the end.
3. Create a project called `nexo`.
4. **Security → SSH keys → Add SSH key.** Paste your public key.

If you do not have a key yet:

```powershell
ssh-keygen -t ed25519 -C "nexo-deploy"
type $env:USERPROFILE\.ssh\id_ed25519.pub
```

Never paste the private half anywhere.

---

## Phase 2 — The server

**Servers → Add Server.**

| Setting | Value |
|---|---|
| Location | Falkenstein (fsn1) |
| Image | Ubuntu 24.04 |
| Type | Shared vCPU → **Arm64 → CAX21** (4 vCPU, 8 GB RAM, 80 GB NVMe) |
| Networking | IPv4 + IPv6 |
| SSH key | the one from Phase 1 |
| Firewall | create one, see below |
| Name | `nexo-api` |

Roughly €8–11/month depending on whether you keep the IPv4 address; Hetzner
raised cloud prices twice in 2026, so check the figure in the console rather
than trusting any number written down.

Keep IPv4. A messenger client on arbitrary home and mobile networks cannot rely
on IPv6-only reachability.

**Firewall rules — inbound only:**

| Port | Source | Why |
|---|---|---|
| 22 | your IP, ideally | SSH |
| 80 | anywhere | ACME HTTP challenge, redirect to HTTPS |
| 443 | anywhere | the API and WebSocket |

Postgres (5432) is **not** in that list and must never be. It listens on
localhost only.

If you chose LUKS option B, add the dropbear port (commonly 2222) too.

---

## Phase 3 — Harden

SSH in as root, then:

```bash
# A non-root user for everything after this
adduser --disabled-password --gecos "" deploy
usermod -aG sudo deploy
rsync --archive --chown=deploy:deploy ~/.ssh /home/deploy

# Keys only, no root login
sed -i 's/^#\?PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config
sed -i 's/^#\?PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config
systemctl restart ssh

apt update && apt upgrade -y
apt install -y unattended-upgrades fail2ban
dpkg-reconfigure -plow unattended-upgrades
```

Open a **second** terminal and confirm `ssh deploy@<ip>` works before closing
the first. Locking yourself out of a fresh box is cheap; locking yourself out of
a running one is not.

---

## Phase 4 — Postgres

Ubuntu 24.04 ships Postgres 16. The project targets **17**, so add the PGDG
repository:

```bash
sudo apt install -y postgresql-common
sudo /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh   # answer yes
sudo apt install -y postgresql-17
```

Create the role and database:

```bash
sudo -u postgres psql <<'SQL'
CREATE ROLE nexo LOGIN PASSWORD 'replace-me-with-a-generated-secret';
CREATE DATABASE nexo OWNER nexo;
\c nexo
CREATE EXTENSION IF NOT EXISTS citext;
SQL
```

Confirm it is not listening publicly — the only addresses should be loopback:

```bash
sudo ss -lntp | grep 5432
```

`listen_addresses` defaults to `localhost` on a Debian/Ubuntu package install.
Leave it that way. The application connects over the loopback interface; nothing
external ever needs to reach the database.

---

## Phase 5 — DNS

In **Hetzner DNS Console** (<https://dns.hetzner.com>, free):

1. Add zone `delidev.net`.
2. At your existing registrar, change the nameservers to the ones Hetzner shows.
3. Add records:

```
api       A     <server IPv4>
api       AAAA  <server IPv6>
nexo      A     <server IPv4>
nexo      AAAA  <server IPv6>
updates   A     <server IPv4>
updates   AAAA  <server IPv6>
```

Wait for propagation before Phase 6 — Caddy's certificate request will fail if
the name does not resolve to the box yet.

---

## Phase 6 — Caddy and TLS

```bash
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
  | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
  | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo apt update && sudo apt install -y caddy
```

`/etc/caddy/Caddyfile`:

```caddyfile
api.delidev.net {
	encode zstd gzip
	header {
		Strict-Transport-Security "max-age=63072000; includeSubDomains; preload"
		X-Content-Type-Options "nosniff"
		-Server
	}
	reverse_proxy 127.0.0.1:8080
}

updates.delidev.net {
	root * /srv/updates
	file_server
}
```

```bash
sudo systemctl reload caddy
```

Caddy obtains and renews Let's Encrypt certificates by itself. TLS 1.3 is on by
default.

> **Do not enable certificate pinning in the client yet.** Caddy rotates the
> certificate key on every renewal by default, and a pinned client that meets a
> rotated key fails closed with no remote fix. Pinning is M9 work, after the
> key-reuse setup and `docs/PIN-ROTATION.md` exist. See PLAN.md risk 2.

---

## Phase 7 — Run the server

Build for `aarch64-unknown-linux-gnu` in CI and copy the artifact up, or build
on the box. Then:

`/etc/systemd/system/nexo-server.service`:

```ini
[Unit]
Description=Nexo API and delivery service
After=network-online.target postgresql.service
Wants=network-online.target

[Service]
User=nexo
Group=nexo
ExecStart=/usr/local/bin/nexo-server
Restart=on-failure
RestartSec=5
EnvironmentFile=/etc/nexo/nexo.env

# The service needs the network and its own state, and nothing else.
NoNewPrivileges=yes
PrivateTmp=yes
ProtectSystem=strict
ProtectHome=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX
MemoryDenyWriteExecute=yes

[Install]
WantedBy=multi-user.target
```

The token signing key, first. The server **refuses to start without it** —
`auth::tokens::load_from_env` bails rather than inventing one, because a key
invented on each boot signs everyone out on every restart and reads as a bug
instead of the misconfiguration it is:

```bash
sudo mkdir -p /etc/nexo
sudo openssl genpkey -algorithm ed25519 -out /etc/nexo/jwt-ed25519.pem
sudo chown nexo:nexo /etc/nexo/jwt-ed25519.pem
sudo chmod 600 /etc/nexo/jwt-ed25519.pem
```

Rotating this key signs every account out. That is the correct response to a
suspected compromise, and also why you do not rotate it idly.

`/etc/nexo/nexo.env` — `chmod 600`, owned by root:

```
NEXO_BIND=127.0.0.1:8080
DATABASE_URL=postgres://nexo:<password>@localhost/nexo
RUST_LOG=nexo_server=info

# Required. Without it the service exits immediately with
# "NEXO_JWT_PRIVATE_KEY_PEM is not set; refusing to start".
NEXO_JWT_PRIVATE_KEY_PEM=/etc/nexo/jwt-ed25519.pem
```

`ProtectSystem=strict` leaves `/etc` readable, so the unit can read the key
there; `ProtectHome=yes` means a key under a home directory could not be read.

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin nexo
sudo systemctl daemon-reload
sudo systemctl enable --now nexo-server
curl -s https://api.delidev.net/v1/health
# {"status":"ok","protocol_version":1}
```

Logs: `journalctl -u nexo-server -f`. Nothing above `debug` may contain user
content, and `debug` is compiled out of release builds.

---

## Phase 8 — Object Storage (M6, not before)

**Console → Object Storage.** Two buckets in FSN1, both **private**:

| Bucket | Contents | Keys |
|---|---|---|
| `nexo-media` | feed and profile images — server-readable by design | `media/{user_id}/{uuid}` |
| `nexo-enc` | encrypted attachments — opaque ciphertext | `enc/{conversation_id}/{uuid}` |

Generate **separate credentials per bucket** — and then do the second half,
without which the first half buys nothing.

> **Correction, verified 2026-08-25.** Separate credential pairs are *not*
> sufficient on Hetzner. S3 keys are **project-wide by default**: every key can
> read and write every bucket in the same project, so two distinct pairs still
> reach both buckets. This was found by
> `cargo test -p nexo-server --test s3_smoke -- --ignored`, which addresses
> `nexo-enc` using the *media* credentials and requires a refusal — it did not
> get one.
>
> To actually get the property this page claims, either:
>
> 1. **Bucket policy** on `nexo-enc` allowlisting only the encrypted key (and
>    the same in reverse on `nexo-media`, if you want it symmetric); or
> 2. **Separate projects**, one bucket each, which makes the default scoping
>    do the work.
>
> A policy looks like the following. Apply it with any S3 tool that speaks
> `put-bucket-policy`, substituting the *access key id* of each key — the ids,
> not the secrets:
>
> ```json
> {
>   "Version": "2012-10-17",
>   "Statement": [
>     {
>       "Effect": "Deny",
>       "NotPrincipal": { "AWS": ["arn:aws:iam:::user/<ENC-ACCESS-KEY-ID>"] },
>       "Action": "s3:*",
>       "Resource": ["arn:aws:s3:::nexo-enc", "arn:aws:s3:::nexo-enc/*"]
>     }
>   ]
> }
> ```
>
> Re-run the smoke test afterwards. It passing is the only evidence that the
> separation is real; until then, treat `nexo-enc` as readable by any key in
> the project and say so in `docs/THREAT-MODEL.md`.

The point of two buckets is that the credential handling public media can never
reach encrypted blobs; one shared key — or two unrestricted keys — throws that
away.

Endpoint `https://fsn1.your-objectstorage.com`, **path-style addressing** (the
bucket is not part of the hostname), SigV4 pinned explicitly rather than left to
SDK defaults. All uploads and downloads happen from the Rust process, never from
the WebView — that sidesteps CORS entirely and keeps encryption on the Rust
side.

Base price includes 1 TB storage and 1 TB egress. Objects under 64 kB bill as
64 kB, which matters for avatars and thumbnails.

---

## Phase 9 — Backups

A Storage Box (from about €3.20/month for 1 TB) speaks SFTP, rsync and
BorgBackup, with unlimited traffic. Nightly `pg_dump` into Borg is enough at this
scale.

Back up: the Postgres dump, `/etc/nexo/`, `/etc/caddy/`.
Do not back up: undelivered envelopes are purged after 30 days by design, so a
restore is allowed to lose them.

**Test a restore before you need one.** An untested backup is a belief, not a
backup.

---

## Phase 10 — Updates host (M9, not before)

`updates.delidev.net` serves the updater: static files behind Caddy, nothing
else. No database, no code — the security of the channel comes from the
minisign signature the app verifies (`docs/RELEASING.md`), not from this
host, so it stays boring on purpose.

1. DNS: `updates` → the server's IP, same as Phase 5.
2. Caddy: a second site block serving a directory.

   ```
   updates.delidev.net {
   	root * /var/www/nexo-updates
   	file_server
   	header {
   		Strict-Transport-Security "max-age=63072000; includeSubDomains; preload"
   		X-Content-Type-Options nosniff
   	}
   }
   ```

3. Layout under `/var/www/nexo-updates`:

   ```
   nexo/releases/Nexo_<version>_x64-setup.exe     the installers
   nexo/windows-x86_64/<installed-version>        the manifest each installed
                                                  version fetches (JSON; see
                                                  RELEASING.md for the body)
   ```

   The client asks `/nexo/{{target}}/{{current_version}}`. Every *installed*
   version must answer: on each release, write the new manifest under every
   older version's path (a loop in the publish script), and give the path for
   the release itself a `204` (an empty `respond` matcher in Caddy, or simply
   no file — but a 404 shows up in the About panel as a failed check, so
   prefer the explicit 204).

4. Uploads are `rsync` from the machine that built and signed the release.
   The web root is owned by a deploy user; Caddy only reads.

Back up with Phase 9? No — every artifact here is reproducible from a tag
plus the signing keys, and the signing keys are exactly what must **not** sit
on this host.

---

## What this setup does not protect against

For `docs/THREAT-MODEL.md`, stated plainly:

- **Hetzner.** They can snapshot the RAM of a running VM. LUKS does not help
  against a live machine, only a disposed disk. Message *content* is still safe,
  because it is end-to-end encrypted and the server never holds the keys.
- **Metadata.** Who talks to whom, when, and how large the messages are, is
  visible to the server and therefore to anyone who controls it.
- **Feed posts and profiles.** Server-readable by design. Not a leak — a
  documented property.
- **A compromised server swapping public keys.** Defeated only by users actually
  comparing safety numbers.

---

## Consolidation warning

Server, object storage and backups in one Hetzner account means one compromised
login, one billing failure or one suspension takes out all three at once. Keep
two-factor authentication on, and keep one periodic backup copy **off Hetzner**.
That is the single exception worth making to "everything in one place".

---

## Sources

- [Hetzner Cloud console](https://console.hetzner.cloud) · [DNS Console](https://dns.hetzner.com)
- [Hetzner Object Storage](https://www.hetzner.com/storage/object-storage/)
- [Installing Ubuntu 24.04 with full disk encryption](https://community.hetzner.com/tutorials/install-ubuntu-2404-with-full-disk-encryption/)
- [PostgreSQL APT repository](https://www.postgresql.org/download/linux/ubuntu/)
- [Caddy installation](https://caddyserver.com/docs/install)
