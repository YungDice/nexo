<#
.SYNOPSIS
  Runs the same gate CI runs. Run this before pushing.

.DESCRIPTION
  Assumes the session has already been prepared by scripts\dev-env.ps1.
#>

$ErrorActionPreference = 'Continue'
$failed = @()

function Step($name, [scriptblock]$body) {
    Write-Host "`n=== $name ===" -ForegroundColor Cyan
    & $body
    if ($LASTEXITCODE -ne 0) {
        $script:failed += $name
        Write-Host "FAILED: $name" -ForegroundColor Red
    }
}

Step 'cargo fmt'        { cargo fmt --all --check }
Step 'cargo clippy'     { cargo clippy --workspace --all-targets -- -D warnings }
Step 'cargo test'       { cargo test --workspace }

# Two passes: the client ships only for Windows, the server runs only on Linux.
# See the comment at the top of deny.toml.
Step 'cargo deny (windows client)' {
    cargo deny --target x86_64-pc-windows-msvc --exclude nexo-server check
}
Step 'cargo deny (linux server)' {
    cargo deny --target aarch64-unknown-linux-gnu --exclude nexo-desktop check
}
Step 'cargo audit'      { cargo audit }

Step 'pnpm typecheck'   { pnpm typecheck }
Step 'pnpm build'       { pnpm build }

if ($failed.Count -gt 0) {
    Write-Host "`n$($failed.Count) step(s) failed:" -ForegroundColor Red
    $failed | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}

Write-Host "`nAll checks passed." -ForegroundColor Green
