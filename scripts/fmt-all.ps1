<#
.SYNOPSIS
Per-crate `cargo fmt --check` across the whole workspace (Windows os-error-206 workaround).

.DESCRIPTION
`cargo fmt --all` / `cargo fmt --check --all` fails on Windows hosts with
"os error 206: The filename or extension is too long": cargo hands rustfmt one
command line covering every workspace crate, and 73 crates on long paths exceed
the CreateProcess command-line limit. GitHub CI runs Linux, so the gate passes
there; local dev needs this script instead. It loops `cargo fmt --check -p
<crate>` - one short command line per crate - and exits nonzero listing the
crates that are not clean.

.USAGE
    powershell -NoProfile -File scripts/fmt-all.ps1          # check only
    powershell -NoProfile -File scripts/fmt-all.ps1 -Fix     # apply fixes

.NOTES
Run from the repo root. CARGO_TARGET_DIR is respected if already set.
This file is deliberately ASCII-only: under PS 5.1 a file saved as UTF-8 can be
read back as ANSI, and em dashes / smart quotes then mojibake into syntax
errors. Same lesson as scripts/local-ci.ps1.
#>
[CmdletBinding()]
param(
    # Apply formatting fixes instead of only checking (runs `cargo fmt -p <crate>`).
    [switch]$Fix
)

# NOTE: deliberately NOT setting $ErrorActionPreference='Stop' - under PS 5.1
# that turns native stderr chatter (e.g. cargo's "Blocking waiting for file
# lock") into terminating exceptions. Gate decisions use $LASTEXITCODE only.

if (-not (Test-Path 'Cargo.toml') -or -not (Test-Path 'crates')) {
    Write-Host '[fmt-all] ERROR: run from the repo root (Cargo.toml + crates/ not found).' -ForegroundColor Red
    exit 2
}

# --- locate cargo ------------------------------------------------------------
$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if ($cargoCmd) {
    $cargo = $cargoCmd.Source
} else {
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
}

$swTotal = [System.Diagnostics.Stopwatch]::StartNew()

# --- discover workspace crates (crates/<name>/Cargo.toml; same as local-ci) --
$crates = @(Get-ChildItem -Path crates -Directory |
    Where-Object { Test-Path (Join-Path $_.FullName 'Cargo.toml') } |
    ForEach-Object { $_.Name } |
    Sort-Object)

if ($crates.Count -eq 0) {
    Write-Host '[fmt-all] ERROR: no workspace crates found under crates/.' -ForegroundColor Red
    exit 2
}

$targetDir = '<default>'
if ($env:CARGO_TARGET_DIR) { $targetDir = $env:CARGO_TARGET_DIR }
$mode = 'check'
if ($Fix) { $mode = 'fix' }
Write-Host ("[fmt-all] mode={0} crates={1} target-dir={2}" -f $mode, $crates.Count, $targetDir)

# --- step: one cargo fmt invocation per crate --------------------------------
# Per-crate invocation dodges the os-error-206 arg-length limit of --all.
Write-Host ("[fmt-all] fmt {0} ..." -f $mode) -NoNewline
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$fmtBad = @()
foreach ($c in $crates) {
    if ($Fix) {
        & $cargo fmt -p $c 2>&1 | Out-Null
    } else {
        & $cargo fmt --check -p $c 2>&1 | Out-Null
    }
    if ($LASTEXITCODE -ne 0) { $fmtBad += $c }
}

if ($fmtBad.Count -gt 0) {
    Write-Host ' FAIL' -ForegroundColor Red
    Write-Host ("[fmt-all] fmt not clean in: {0}" -f ($fmtBad -join ', '))
    Write-Host '[fmt-all] fix with: scripts/fmt-all.ps1 -Fix (or cargo fmt -p <crate>)'
    Write-Host ("[fmt-all] FAILED ({0} of {1} crates not clean) in {2:N1}s - fix before pushing." -f $fmtBad.Count, $crates.Count, $swTotal.Elapsed.TotalSeconds) -ForegroundColor Red
    exit 1
}
Write-Host (" ok ({0:N1}s)" -f $sw.Elapsed.TotalSeconds) -ForegroundColor Green
Write-Host ("[fmt-all] green ({0} crates) in {1:N1}s" -f $crates.Count, $swTotal.Elapsed.TotalSeconds) -ForegroundColor Green
exit 0
