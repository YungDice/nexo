<#
.SYNOPSIS
  Runs cargo with a prepared build environment.

.DESCRIPTION
  Same reasoning as scripts\tauri.ps1: cc-rs picks a Visual Studio install by
  itself when VCINSTALLDIR / INCLUDE / LIB are unset, and on a machine with
  more than one install its pick may be an incomplete one. This wrapper sets
  the environment first so cargo works from any shell.

  Used by `pnpm server`. For day-to-day cargo work, dot-source dev-env.ps1
  once per terminal instead and then use cargo directly.

.NOTES
  No param() block on purpose, so flags like -p reach cargo untouched.
#>

. "$PSScriptRoot\dev-env.ps1"

cargo @args
exit $LASTEXITCODE
