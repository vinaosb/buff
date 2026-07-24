# msvc-env.ps1 — load the MSVC x64 environment for cargo/rustc on this host.
#
# T19 helper. The build host has TWO MSVC installs:
#   1. VS 18 "Insiders" (broken — vcvarsall.bat missing, msvcrt.lib unreachable
#      via the default env that rustc picks up).
#   2. VS 2022 Enterprise at `C:\Program Files\Microsoft Visual Studio\2022\Enterprise`
#      — fully populated MSVC 14.44 + Win10 SDK 10.0.26100.0.
#
# The default rustc on this host probes for the highest-numbered MSVC it can
# find, which is the broken VS 18 Insiders. Loading this script redirects
# LIB/INCLUDE/PATH at the working VS 2022 Enterprise install so `cargo build`
# and `buff build` (which shells out to rustc→link.exe) succeed.
#
# Usage:
#   . .\self-host\msvc-env.ps1
#   cargo build --release -p buff-lang-cli
#   .\target\release\buff.exe check self-host\lexer\token.buff
#
# Idempotent — safe to dot-source multiple times.
param(
    [string]$VSDir = 'C:\Program Files\Microsoft Visual Studio\2022\Enterprise',
    [string]$WinKits = 'C:\Program Files (x86)\Windows Kits\10',
    [string]$SdkVer = '10.0.26100.0'
)

$ErrorActionPreference = 'Stop'

# Locate the highest-numbered MSVC toolchain under the VS 2022 Enterprise install.
$msvcRoot = Join-Path $VSDir 'VC\Tools\MSVC'
if (-not (Test-Path -LiteralPath $msvcRoot)) {
    throw "MSVC tools dir not found: $msvcRoot (is VS 2022 Enterprise installed?)"
}
$msvc = Get-ChildItem -LiteralPath $msvcRoot -Directory |
    Sort-Object Name -Descending |
    Select-Object -First 1
if (-not $msvc) {
    throw "No MSVC toolchain under $msvcRoot"
}
$msvcPath = $msvc.FullName
Write-Host "msvc-env: using MSVC $($msvc.Name) at $msvcPath"

# Build LIB search path (where link.exe finds .lib files).
$env:LIB = (
    "$msvcPath\lib\x64",
    "$msvcPath\lib\onecore\x64",
    "$WinKits\Lib\$SdkVer\ucrt\x64",
    "$WinKits\Lib\$SdkVer\um\x64"
) -join ';'

# Build INCLUDE search path (where cl.exe finds .h files).
# The host's VS 2022 Enterprise install is missing `vcruntime.h` (the C++
# runtime core header) — scavenged from the VS 18 Insiders ScopeCppSDK vc15
# tree, which carries a compatible copy. See bootstrap-report.md §"Build host
# MSVC voodoo" for the full forensic story.
$orphanVcruntime = 'C:\Program Files\Microsoft Visual Studio\18\Insiders\SDK\ScopeCppSDK\vc15\VC\include'
$env:INCLUDE = (
    $orphanVcruntime,
    "$msvcPath\include",
    "$WinKits\Include\$SdkVer\ucrt",
    "$WinKits\Include\$SdkVer\um",
    "$WinKits\Include\$SdkVer\shared",
    "$WinKits\Include\$SdkVer\winrt"
) -join ';'

# Prepend MSVC bin so rustc spawns the matching link.exe.
$env:PATH = "$msvcPath\bin\HostX64\x64;$env:PATH"

Write-Host "msvc-env: LIB/INCLUDE/PATH set. Cargo build/run now works."
