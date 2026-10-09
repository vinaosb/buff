<#
.SYNOPSIS
Fast local CI gate for the buff workspace (maintainability ITER-16).

.DESCRIPTION
Fail-fast ladder for this repo:

  1. pre-commit hook  -> local-ci -Scope Commit  (fmt check on staged crates; seconds)
  2. pre-push hook    -> local-ci -Scope Push    (fmt + clippy + tests on changed crates; ~1-3 min warm)
  3. GitHub CI        -> full matrix (cross-OS core tests, docker, equivalence, audit, ...)
  4. branch protection-> mechanical merge gate on main (11 required hard-gate checks)

NOT replicated locally (by design): docker build, windows/macos core tests,
buff-validation release run, cargo-audit (network). GitHub remains the source
of truth; this gate exists to fail FAST on the common breakages (fmt, clippy,
unit tests) before wasting a CI round-trip.

The gate is diff-aware: only crates whose files changed (staged and/or
committed since merge-base with origin/main) are checked, which keeps the
common case well under a couple of minutes on a warm target dir.

.NOTES
Run from the repo root (hooks do this automatically). CARGO_TARGET_DIR is
respected if already set (workers use C:\ct).
#>
[CmdletBinding()]
param(
    [ValidateSet('Auto', 'Commit', 'Push', 'All')]
    [string]$Scope = 'Auto'
)

# NOTE: deliberately NOT setting $ErrorActionPreference='Stop' — under PS 5.1
# that turns native stderr chatter (e.g. cargo's "Blocking waiting for file
# lock") into terminating exceptions. Gate decisions use $LASTEXITCODE only.

# --- locate cargo ------------------------------------------------------------
$cargoCmd = Get-Command cargo -ErrorAction SilentlyContinue
if ($cargoCmd) {
    $cargo = $cargoCmd.Source
} else {
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin\cargo.exe'
}

# --- helpers -----------------------------------------------------------------
function Get-ChangedFiles([string]$Scope) {
    $files = @()
    if ($Scope -ne 'Commit') {
        # Everything committed on this branch vs the merge-base with origin/main.
        $base = git merge-base HEAD origin/main 2>$null
        if (-not $base -or $LASTEXITCODE -ne 0) { $base = 'origin/main' }
        $files += @(git diff --name-only "$base..HEAD" 2>$null)
    }
    # Plus anything staged right now (matters for the pre-commit hook).
    $files += @(git diff --cached --name-only 2>$null)
    return @($files | Where-Object { $_ } | Sort-Object -Unique)
}

function Get-CratesFromFileList([string[]]$Files) {
    $set = New-Object 'System.Collections.Generic.HashSet[string]'
    foreach ($f in $Files) {
        if ($f -match '^crates/([^/]+)/') { [void]$set.Add($Matches[1]) }
    }
    return @($set)
}

function Get-AllCrates() {
    Get-ChildItem -Path crates -Directory |
        Where-Object { Test-Path (Join-Path $_.FullName 'Cargo.toml') } |
        ForEach-Object { $_.Name }
}

function Get-CiClippyCrates() {
    # Parse the CI_CRATES allow-list from the clippy step (single source of truth).
    $line = Get-Content .github\workflows\ci.yml |
        Where-Object { $_ -match 'run:\s*cargo clippy' } | Select-Object -First 1
    if (-not $line) { return @() }
    return @([regex]::Matches($line, '-p ([A-Za-z0-9_-]+)') | ForEach-Object { $_.Groups[1].Value })
}

# --- resolve scope + crate set -----------------------------------------------
if ($Scope -eq 'Auto') { $Scope = 'Push' }

$swTotal = [System.Diagnostics.Stopwatch]::StartNew()
$failures = @()

if ($Scope -eq 'All') {
    $crates = Get-AllCrates
} else {
    $crates = Get-CratesFromFileList (Get-ChangedFiles $Scope)
}

Write-Host ("[local-ci] scope={0} target-dir={1}" -f $Scope, $(if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { '<default>' }))
if ($crates.Count -eq 0) {
    Write-Host '[local-ci] no changed crates - nothing to check.' -ForegroundColor Green
    exit 0
}
Write-Host ("[local-ci] crates ({0}): {1}" -f $crates.Count, ($crates -join ', '))

# --- step 1: fmt (per crate; also dodges the os-error-206 of --all on Windows)
Write-Host '[local-ci] fmt ...' -NoNewline
$sw = [System.Diagnostics.Stopwatch]::StartNew()
$fmtBad = @()
foreach ($c in $crates) {
    & $cargo fmt --check -p $c 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { $fmtBad += $c }
}
if ($fmtBad.Count -gt 0) {
    Write-Host (' FAIL' -f '') -ForegroundColor Red
    Write-Host ("[local-ci] fmt not clean in: {0}. Run: cargo fmt -p <crate>" -f ($fmtBad -join ', '))
    $failures += 'fmt'
} else {
    Write-Host (" ok ({0:N1}s)" -f $sw.Elapsed.TotalSeconds) -ForegroundColor Green
}

# --- step 2: clippy (-D warnings) on every changed crate ----------------------
# (GitHub only clippies the CI_CRATES allow-list; checking every changed crate
# locally is a bonus net, not an extra requirement.)
if ($Scope -ne 'Commit') {
    Write-Host '[local-ci] clippy ...' -NoNewline
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $clippyBad = @()
    foreach ($c in $crates) {
        $cout = & $cargo clippy -p $c --all-targets -- -D warnings 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) {
            $clippyBad += $c
            Write-Host ''
            Write-Host ("[local-ci] ---- {0} clippy output (tail) ----" -f $c)
            ($cout -split "`r?`n") | Select-Object -Last 25 | Write-Host
        }
    }
    if ($clippyBad.Count -gt 0) {
        Write-Host ' FAIL' -ForegroundColor Red
        Write-Host ("[local-ci] clippy failed in: {0}" -f ($clippyBad -join ', '))
        $failures += 'clippy'
    } else {
        Write-Host (" ok ({0:N1}s)" -f $sw.Elapsed.TotalSeconds) -ForegroundColor Green
    }
}

# --- step 3: tests per changed crate ------------------------------------------
if ($Scope -ne 'Commit') {
    Write-Host '[local-ci] tests ...' -NoNewline
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $testBad = @()
    foreach ($c in $crates) {
        $out = & $cargo test -p $c 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) {
            $testBad += $c
            Write-Host ''
            Write-Host ("[local-ci] ---- {0} test output ----" -f $c) -ForegroundColor Red
            Write-Host $out
        }
    }
    if ($testBad.Count -gt 0) {
        Write-Host ("[local-ci] tests failed in: {0}" -f ($testBad -join ', '))
        $failures += 'tests'
    } else {
        Write-Host (" ok ({0:N1}s)" -f $sw.Elapsed.TotalSeconds) -ForegroundColor Green
    }
}

# --- summary -------------------------------------------------------------------
if ($failures.Count -gt 0) {
    Write-Host ("[local-ci] FAILED ({0}) in {1:N1}s - fix before pushing." -f ($failures -join ', '), $swTotal.Elapsed.TotalSeconds) -ForegroundColor Red
    exit 1
}
Write-Host ("[local-ci] green in {0:N1}s" -f $swTotal.Elapsed.TotalSeconds) -ForegroundColor Green
exit 0
