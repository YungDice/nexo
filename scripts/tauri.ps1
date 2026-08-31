<#
.SYNOPSIS
  Runs the Tauri CLI with a prepared build environment.

.DESCRIPTION
  `pnpm tauri dev` shells out to cargo, which compiles C++ build scripts
  (vswhom-sys, webview2-com-sys) through cc-rs. cc-rs auto-detects a toolchain
  only when VCINSTALLDIR / INCLUDE / LIB are unset, and its choice is the
  *newest* Visual Studio present — which on a machine with more than one
  install is not necessarily a complete one. An incomplete install fails late
  and confusingly:

      fatal error C1083: Cannot open include file: 'excpt.h'
      LNK1104: cannot open file 'msvcrt.lib'

  Rather than requiring everyone to remember to dot-source dev-env.ps1 first,
  this wrapper does it, so `pnpm tauri dev` works in any shell.

.NOTES
  No param() block on purpose: every argument then lands in $args untouched,
  so `pnpm tauri build --debug` forwards `--debug` instead of PowerShell
  trying to bind it as a parameter of this script.
#>

. "$PSScriptRoot\dev-env.ps1"

pnpm --filter nexo-desktop tauri @args
exit $LASTEXITCODE
