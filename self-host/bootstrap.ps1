# self-host/bootstrap.ps1 — T19 bootstrap determinism gate (Wave 4), Windows.
#
# PowerShell mirror of bootstrap.sh. Runs the three-stage self-hosting
# verification on a Windows host where the Rust-written Buff compiler can
# fully link (i.e. MSVC environment is properly configured — see
# self-host/msvc-env.ps1).
#
# Stages (per task spec):
#   1.  Rust-written compiler compiles Buff-written compiler → buff-self-hosted.exe
#   2.  .\buff-self-hosted.exe build self-host/ → stage2 output (hashed)
#   3.  .\buff-self-hosted.exe build self-host/ → stage3 output (hashed)
#   ✓  if (Get-FileHash stage2).Hash == (Get-FileHash stage3).Hash → DETERMINISM HOLDS
#
# Run from the repo root so relative paths resolve. The script will dot-source
# msvc-env.ps1 on Windows to load the MSVC environment that rustc needs for
# the link step.
#
# Usage:
#   .\self-host\bootstrap.ps1                       # full pipeline
#   .\self-host\bootstrap.ps1 -SkipStage1           # re-use existing buff-self-hosted.exe
#
# Exit codes (set via $LASTEXITCODE on exit):
#   0  Stage 2 == Stage 3 (determinism gate holds)
#   1  Stage 1 failed (Rust compiler cannot compile the .buff sources)
#   2  Stage 2 failed (buff-self-hosted cannot recompile itself)
#   3  Stage 3 failed
#   4  Stage 2 != Stage 3 (NON-DETERMINISM — investigate)
#   5  Missing prerequisites

[CmdletBinding()]
param(
    [switch]$SkipStage1
)

$ErrorActionPreference = 'Stop'

$RepoRoot    = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$SelfHostDir = Join-Path $RepoRoot 'self-host'
$StageDir    = Join-Path $RepoRoot 'target/bootstrap'
$Stage1Bin   = Join-Path $StageDir 'buff-self-hosted.exe'
$Stage2Out   = Join-Path $StageDir 'stage2.rs'
$Stage3Out   = Join-Path $StageDir 'stage3.rs'

# --- helpers ---------------------------------------------------------------

function Log  { param([string]$Msg) Write-Host "[bootstrap] $Msg" }
function Ok   { param([string]$Msg) Write-Host "[ok]      $Msg" -ForegroundColor Green }
function Fail { param([string]$Msg) Write-Host "[fail]    $Msg" -ForegroundColor Red }
function Note-Warn { param([string]$Msg) Write-Host "[note]    $Msg" -ForegroundColor Yellow }

function Invoke-Stage {
    param(
        [string]$Label,
        [scriptblock]$Action
    )
    Log $Label
    & $Action
    if ($LASTEXITCODE -ne 0) {
        return $false
    }
    return $true
}

# --- prerequisites ---------------------------------------------------------

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail "cargo not on PATH"
    exit 5
}

if (-not (Test-Path -LiteralPath $StageDir)) {
    New-Item -ItemType Directory -Path $StageDir -Force | Out-Null
}

# Load MSVC environment on Windows (rustc needs cl.exe/link.exe in PATH and
# LIB/INCLUDE set). No-op if rustc can already link.
$msvcEnv = Join-Path $PSScriptRoot 'msvc-env.ps1'
if (Test-Path -LiteralPath $msvcEnv) {
    if (-not $env:LIB -or -not $env:INCLUDE) {
        Log "Loading MSVC environment from $msvcEnv"
        . $msvcEnv | Out-Null
    }
}

# --- Stage 1 ---------------------------------------------------------------

if (-not $SkipStage1) {
    Log "Stage 1a: building Rust-written buff compiler (cargo build --release -p buff-lang-cli)"
    & cargo build --release -p buff-lang-cli
    if ($LASTEXITCODE -eq 0) {
        Ok "Stage 1a: buff binary built at target/release/buff.exe"
    } else {
        Fail "Stage 1a: cargo build failed"
        exit 1
    }

    Log "Stage 1b: Rust-written compiler transpiling + linking Buff-written compiler"
    $buffBin = Join-Path $RepoRoot 'target/release/buff.exe'
    & $buffBin build $SelfHostDir --output $Stage1Bin
    if ($LASTEXITCODE -eq 0) {
        Ok "Stage 1b: buff-self-hosted built at $Stage1Bin"
    } else {
        Fail "Stage 1b: buff build failed"
        Note-Warn "EXPECTED on first bootstrap attempt — see bootstrap-report.md."
        Note-Warn "Falling back to the determinism driver example..."
        & cargo run -p buff-lang-codegen-rust --release --example bootstrap_t19 -- $SelfHostDir (Join-Path $StageDir 'bootstrap-report.json')
        if ($LASTEXITCODE -eq 0) {
            Ok "Stage 1b fallback: determinism driver ran successfully"
            exit 1  # still non-zero: Stage 1 did not complete as spec'd
        } else {
            Fail "Stage 1b fallback also failed"
            exit 1
        }
    }
}

if (-not (Test-Path -LiteralPath $Stage1Bin)) {
    Fail "buff-self-hosted binary missing at $Stage1Bin"
    Note-Warn "Run without -SkipStage1, or copy a pre-built binary into place."
    exit 2
}

# --- Stage 2 ---------------------------------------------------------------

Log "Stage 2: $Stage1Bin build $SelfHostDir -> $Stage2Out"
& $Stage1Bin build $SelfHostDir --emit-rust $Stage2Out
if ($LASTEXITCODE -ne 0) {
    Fail "Stage 2: buff-self-hosted could not recompile self-host/"
    exit 2
}
$hash2 = (Get-FileHash -LiteralPath $Stage2Out -Algorithm SHA256).Hash
Ok "Stage 2: complete  sha256=$hash2"

# --- Stage 3 ---------------------------------------------------------------

Log "Stage 3: $Stage1Bin build $SelfHostDir -> $Stage3Out"
& $Stage1Bin build $SelfHostDir --emit-rust $Stage3Out
if ($LASTEXITCODE -ne 0) {
    Fail "Stage 3: buff-self-hosted could not recompile self-host/"
    exit 3
}
$hash3 = (Get-FileHash -LiteralPath $Stage3Out -Algorithm SHA256).Hash
Ok "Stage 3: complete  sha256=$hash3"

# --- Determinism assertion -------------------------------------------------

Log "Comparing Stage 2 vs Stage 3 ..."
if ($hash2 -eq $hash3) {
    $bytes2 = [System.IO.File]::ReadAllBytes($Stage2Out)
    $bytes3 = [System.IO.File]::ReadAllBytes($Stage3Out)
    if ([Linq.Enumerable]::SequenceEqual([byte[]]$bytes2, [byte[]]$bytes3)) {
        Ok "DETERMINISM HOLDS: Stage 2 == Stage 3 byte-identical"
        Write-Host "         sha256(stage2.rs) = $hash2"
        Write-Host "         sha256(stage3.rs) = $hash3"
        exit 0
    }
}
Fail "NON-DETERMINISM: Stage 2 != Stage 3"
Write-Host "         sha256(stage2.rs) = $hash2"
Write-Host "         sha256(stage3.rs) = $hash3"
Note-Warn "Probable causes:"
Note-Warn "  1. HashMap/HashSet iteration order leaking into codegen (use BTreeMap/BTreeSet)."
Note-Warn "  2. Timestamp / process-id / random embedded in output."
Note-Warn "  3. Race in parallel codegen (rayon) producing non-deterministic splice order."
exit 4
