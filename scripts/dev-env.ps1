<#
.SYNOPSIS
  Puts the current PowerShell session into a working Nexo build environment.

.DESCRIPTION
  Two things on a stock Windows box stop this repo from building, and both are
  environment problems rather than code problems:

  1. If more than one Visual Studio is installed, Rust picks the newest MSVC it
     can find. That is not always the complete one -- an install carrying only
     `lib\onecore` will fail to link with "cannot open file 'msvcrt.lib'".
     This script picks the newest install that actually has the desktop x64 CRT.

  2. Building SQLCipher's vendored OpenSSL (from M2 onward) needs a full Perl.
     The one shipped inside Git for Windows is not complete enough -- OpenSSL's
     Configure fails on a missing Locale::Maketext::Simple. Strawberry Perl is
     the supported option, and this script puts it on PATH if it is installed.

  Run it in each new shell:  . .\scripts\dev-env.ps1
#>

$ErrorActionPreference = 'Stop'

# --- 1. Select a Visual Studio install with the desktop x64 CRT --------------
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) { throw "vswhere.exe not found. Install Visual Studio Build Tools." }

$candidates = & $vswhere -all -prerelease -products * -format value -property installationPath
$chosen = $null
foreach ($path in $candidates) {
    $msvc = Join-Path $path 'VC\Tools\MSVC'
    if (-not (Test-Path $msvc)) { continue }
    foreach ($ver in (Get-ChildItem $msvc | Sort-Object Name -Descending)) {
        if (Test-Path (Join-Path $ver.FullName 'lib\x64\msvcrt.lib')) {
            $chosen = $path
            break
        }
    }
    if ($chosen) { break }
}

if (-not $chosen) {
    throw @"
No Visual Studio install has the desktop x64 CRT (lib\x64\msvcrt.lib).
Open the Visual Studio Installer and add, under 'Desktop development with C++':
  - MSVC v14.x - VS 2022+ C++ x64/x86 build tools
  - Windows 11 SDK
"@
}

$vcvars = Join-Path $chosen 'VC\Auxiliary\Build\vcvars64.bat'
Write-Host "MSVC:   $chosen" -ForegroundColor DarkGray

# Import the environment vcvars64 sets, into this session.
cmd /c "`"$vcvars`" >nul 2>nul && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') {
        Set-Item -Path "env:$($matches[1])" -Value $matches[2] -ErrorAction SilentlyContinue
    }
}

# --- 2. Strawberry Perl, for SQLCipher's vendored OpenSSL (M2+) --------------
$perl = @(
    'C:\Strawberry\perl\bin',
    "$env:LOCALAPPDATA\Programs\Strawberry\perl\bin"
) | Where-Object { Test-Path (Join-Path $_ 'perl.exe') } | Select-Object -First 1

if ($perl) {
    $env:PATH = "$perl;$env:PATH"
    Write-Host "Perl:   $perl" -ForegroundColor DarkGray
} else {
    Write-Host "Perl:   Strawberry Perl not found - needed from M2 onward (SQLCipher/OpenSSL)." -ForegroundColor Yellow
    Write-Host "        winget install StrawberryPerl.StrawberryPerl" -ForegroundColor Yellow
}

# --- 3. pnpm ----------------------------------------------------------------
$npmGlobal = "$env:APPDATA\npm"
if ((Test-Path (Join-Path $npmGlobal 'pnpm.cmd')) -and ($env:PATH -notlike "*$npmGlobal*")) {
    $env:PATH = "$npmGlobal;$env:PATH"
}

Write-Host "Ready." -ForegroundColor Green
