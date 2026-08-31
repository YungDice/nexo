<#
.SYNOPSIS
  Cuts a release: bumps the version, builds, signs, and publishes to GitHub.

.DESCRIPTION
  One command, because the order is the part that is easy to get wrong.

  The version is compiled **into** the binary, so it has to be bumped before the
  build, not after. A manifest that advertises 0.1.1 pointing at an installer
  that reports 0.1.0 is worse than no manifest at all: every client downloads
  it, installs it, still sees the old version, and updates again forever.

  Steps, in the only order that works:

    1. Bump the patch version in Cargo.toml and tauri.conf.json together.
       They must agree — the updater compares them numerically.
    2. Build and sign.
    3. Write latest.json describing what was just built.
    4. Create the GitHub release and upload all three files.

.PARAMETER Notes
  Release notes, shown to the user by the updater.

.PARAMETER Version
  An explicit version instead of bumping the patch. For a minor or major bump.

.PARAMETER NoPublish
  Do everything but the GitHub release. For checking the artifacts first.

.PARAMETER SkipBuild
  Reuse the artifacts already in target\release\bundle\nsis. Only correct when
  they were built from the current version.

.EXAMPLE
  .\scripts\release.ps1 -Notes "Group pictures and nested comments."

.EXAMPLE
  .\scripts\release.ps1 -Version 0.2.0 -Notes "Feed rewrite."
#>
[CmdletBinding()]
param(
    [string]$Notes = "Bug fixes and improvements.",
    [string]$Version,
    [switch]$NoPublish,
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $root "Cargo.toml"
$tauriConf = Join-Path $root "apps\desktop\src-tauri\tauri.conf.json"

# ------------------------------------------------------------- preflight ---
#
# Everything that can refuse this release is checked before a single file is
# written. A missing password used to be found *after* the version had been
# bumped, which left the working tree one version ahead with nothing built --
# and every retry moved it further, so the number climbed without any release
# ever being cut.

if (-not $SkipBuild) {
    if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
        $keyFile = Join-Path $env:USERPROFILE ".tauri\nexo-updater.key"
        if (-not (Test-Path $keyFile)) {
            throw "No signing key. Set TAURI_SIGNING_PRIVATE_KEY, or put the key at $keyFile."
        }
        $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $keyFile -Raw
    }
    if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
        throw @"
TAURI_SIGNING_PRIVATE_KEY_PASSWORD is not set.

  `$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "your-key-password"

Use "" if the key has none. Nothing has been changed.
"@
    }
}

if (-not $NoPublish -and -not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw "gh is not installed. Re-run with -NoPublish and upload by hand. Nothing has been changed."
}

# The pinned public key must belong to the key doing the signing.
#
# Nothing downstream notices if they disagree: the build succeeds, the manifest
# is written, the release publishes, and the failure only appears on somebody
# else's machine as "The signature was created with a different key than the one
# provided" -- after they have already downloaded it. Regenerating the keypair
# and forgetting to re-pin the public half is the way this happens, and it is
# silent every step until then.
$pubFile = Join-Path $env:USERPROFILE ".tauri\nexo-updater.key.pub"
if (-not $SkipBuild -and (Test-Path $pubFile)) {
    $keyPub = (Get-Content $pubFile | Select-Object -First 1).Trim()
    $pinned = (Get-Content $tauriConf -Raw | ConvertFrom-Json).plugins.updater.pubkey
    if ($keyPub -and $pinned -and $keyPub -ne $pinned) {
        throw @"
The signing key does not match the public key pinned in tauri.conf.json.

  key file : ...$($keyPub.Substring([Math]::Max(0, $keyPub.Length - 24)))
  pinned   : ...$($pinned.Substring([Math]::Max(0, $pinned.Length - 24)))

Anything built now would be rejected by every client. Put the contents of
$pubFile into plugins.updater.pubkey and try again.

Nothing has been changed.
"@
    }
}

function Read-Version {
    $line = Select-String -Path $cargoToml -Pattern '^version = "([^"]+)"' | Select-Object -First 1
    if (-not $line) { throw "no version in $cargoToml" }
    return $line.Matches[0].Groups[1].Value
}

# ---------------------------------------------------------------- version ---

$current = Read-Version

if ($Version) {
    $next = $Version
} else {
    if ($current -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
        throw "version '$current' is not major.minor.patch; pass -Version explicitly"
    }
    $next = "$($Matches[1]).$($Matches[2]).$([int]$Matches[3] + 1)"
}

if ($next -notmatch '^\d+\.\d+\.\d+$') { throw "'$next' is not a valid version" }

# A tag that already exists usually means this version was released, and
# publishing over it would leave installs pointing at an artifact that changed
# underneath them.
#
# Usually, but not always. A release can fail after the tag is pushed -- the
# build breaks, the signing key is missing, CI refuses -- and then the tag sits
# on the very commit this run is about to build. That is a resume, not a clash,
# and telling someone to pick a higher number would burn a version number for
# nothing. What decides is where the tag points and whether a release was
# actually published from it.
$resuming = $false
$existing = (git -C $root tag --list "v$next")
if ($existing) {
    # rev-list rather than rev-parse: it resolves to a commit for an annotated
    # tag as well, without needing a `^{commit}` suffix that PowerShell would
    # have to be trusted to hand to git unmangled.
    $tagged = (git -C $root rev-list -n 1 "v$next").Trim()
    $head = (git -C $root rev-parse HEAD).Trim()
    if ($tagged -ne $head) {
        throw @"
tag v$next already exists and points at $($tagged.Substring(0, 7)), not at HEAD
($($head.Substring(0, 7))). Either check out that commit, or pass -Version with a
higher number.

Nothing has been changed.
"@
    }
    # `gh release view` answers non-zero when there is no release for the tag.
    $published = $null
    if (Get-Command gh -ErrorAction SilentlyContinue) {
        gh release view "v$next" 2>$null | Out-Null
        $published = ($LASTEXITCODE -eq 0)
    }
    if ($published) {
        throw @"
v$next is already released. Its installer is out there and people may have it;
replacing the files under that tag would change what they downloaded.

Pass -Version with a higher number.

Nothing has been changed.
"@
    }
    Write-Host "Tag v$next is already on this commit with no release -- resuming it." -ForegroundColor Yellow
    $resuming = $true
}

Write-Host ""
if ($resuming) {
    Write-Host "Resuming v$next" -ForegroundColor Cyan
} else {
    Write-Host "$current  ->  $next" -ForegroundColor Cyan
}
Write-Host ""

# Both files, together. The updater compares the version in the manifest with
# the one the running binary reports, and those come from different files.
(Get-Content $cargoToml -Raw) -replace '(?m)^version = "[^"]+"', "version = `"$next`"" |
    Out-File $cargoToml -Encoding utf8 -NoNewline

$confRaw = Get-Content $tauriConf -Raw
$confRaw = $confRaw -replace '"version": "[^"]+"', "`"version`": `"$next`""
# -NoNewline and UTF8 without a BOM: a BOM here makes the file unparseable as
# JSON, which is a confusing way to discover a version bump went wrong.
[System.IO.File]::WriteAllText($tauriConf, $confRaw, (New-Object System.Text.UTF8Encoding $false))

if ((Read-Version) -ne $next) { throw "the version bump did not take" }
if (-not $resuming) {
    Write-Host "Bumped Cargo.toml and tauri.conf.json." -ForegroundColor Green
}

# ------------------------------------------------------------------ build ---

$bundle = Join-Path $root "target\release\bundle\nsis"
$installer = Join-Path $bundle "Nexo_${next}_x64-setup.exe"
$sigFile = "$installer.sig"

if (-not $SkipBuild) {
    Write-Host ""
    Write-Host "Building. This takes a few minutes." -ForegroundColor Cyan
    Push-Location $root
    try {
        pnpm tauri build
        if ($LASTEXITCODE -ne 0) { throw "the build failed" }
    } catch {
        # Put the version back. A failed build that leaves the tree bumped is
        # how the number climbs without a release ever being cut.
        Write-Host "Build failed -- restoring version $current." -ForegroundColor Yellow
        git -C $root checkout -- Cargo.toml apps/desktop/src-tauri/tauri.conf.json
        throw
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path $installer)) { throw "installer not found: $installer" }
if (-not (Test-Path $sigFile)) {
    throw "signature not found: $sigFile`nThe build needs TAURI_SIGNING_PRIVATE_KEY set."
}

# --------------------------------------------------------------- manifest ---

$remote = (git -C $root remote get-url origin) 2>$null
if (-not $remote) { throw "no git remote 'origin'" }
$slug = $remote -replace '^.*github\.com[:/]', '' -replace '\.git$', ''
if ($slug -notmatch '^[^/]+/[^/]+$') { throw "origin is not a GitHub remote: $remote" }

$manifest = [ordered]@{
    version   = $next
    notes     = $Notes
    pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = (Get-Content $sigFile -Raw).Trim()
            url       = "https://github.com/$slug/releases/download/v$next/Nexo_${next}_x64-setup.exe"
        }
    }
}

$manifestPath = Join-Path $bundle "latest.json"
# WriteAllText with a BOM-less encoder, not Out-File: in Windows PowerShell
# `-Encoding utf8` means UTF-8 *with* a byte-order mark, and a BOM makes this
# unparseable as JSON. The updater downloads it fine and then fails with
# "error decoding response body", which points at the network rather than here.
[System.IO.File]::WriteAllText(
    $manifestPath,
    ($manifest | ConvertTo-Json -Depth 5),
    (New-Object System.Text.UTF8Encoding $false)
)
Write-Host "Wrote $manifestPath" -ForegroundColor Green

# ---------------------------------------------------------------- publish ---

if ($NoPublish) {
    Write-Host ""
    Write-Host "Not published (-NoPublish). Artifacts are in $bundle." -ForegroundColor Yellow
    Write-Host "The version bump is still in your working tree -- commit it before releasing."
    exit 0
}

# Committed and tagged before the release exists, so the tag names the source
# the artifacts were actually built from.
#
# Each of the four steps below asks first whether it still has anything to do.
# On a resumed release the bump is already committed and the tag already
# pushed, and a `git commit` with nothing staged or a `git tag` over an
# existing name is a non-zero exit that would abort the run *after* the build
# -- throwing away the ten minutes that mattered, one step short of the finish.
Push-Location $root
try {
    git add Cargo.toml Cargo.lock apps/desktop/src-tauri/tauri.conf.json
    if (git diff --cached --name-only) {
        git commit -m "release v$next" | Out-Null
    } else {
        Write-Host "Version already committed." -ForegroundColor DarkGray
    }

    if (-not (git tag --list "v$next")) { git tag "v$next" }
    git push
    git push origin "v$next"

    Write-Host ""
    Write-Host "Creating release v$next" -ForegroundColor Cyan
    gh release create "v$next" $installer $sigFile $manifestPath --title "v$next" --notes $Notes
    if ($LASTEXITCODE -ne 0) { throw "gh release create failed" }
} finally {
    Pop-Location
}

Write-Host ""
Write-Host "Released v$next." -ForegroundColor Green
Write-Host "Installs on $current and earlier will pick it up the next time they open."
