<#
.SYNOPSIS
    Golden-output test harness for Buff use-case examples.
.DESCRIPTION
    Iterates over examples/use-cases/*.buff, typechecks each with `buff check`,
    runs each with `buff run` (if a .expected golden file exists), and compares
    stdout to the golden output.
.PARAMETER Verbose
    Show detailed output for each test case.
.EXAMPLE
    .\scripts\test-use-cases.ps1
    .\scripts\test-use-cases.ps1 -Verbose
#>
param(
    [switch]$Verbose
)

$ErrorActionPreference = 'Continue'
$root = Resolve-Path "$PSScriptRoot/.."
$useCasesDir = Join-Path $root "examples/use-cases"

if (-not (Test-Path $useCasesDir)) {
    Write-Host "No use-cases directory found at $useCasesDir" -ForegroundColor Yellow
    exit 0
}

$buffFiles = Get-ChildItem -Path $useCasesDir -Filter "*.buff" -Recurse -File
$total = $buffFiles.Count
$passCount = 0
$failCount = 0
$skipCount = 0

Write-Host "`n=== Buff Use-Case Golden Tests ===" -ForegroundColor Cyan
Write-Host "Found $total .buff file(s) in $useCasesDir`n"

foreach ($file in $buffFiles) {
    $relPath = $file.FullName.Substring($root.Path.Length + 1)
    $expectedFile = [System.IO.Path]::ChangeExtension($file.FullName, ".expected")

    if ($Verbose) {
        Write-Host "Testing: $relPath" -ForegroundColor White
    }

    # Step 1: Typecheck
    $checkResult = & cargo run -p buff-lang-cli --release -- check $file.FullName 2>&1
    $checkExit = $LASTEXITCODE

    if ($checkExit -ne 0) {
        Write-Host "  [FAIL] $relPath - typecheck failed" -ForegroundColor Red
        if ($Verbose) {
            Write-Host "  Error: $checkResult" -ForegroundColor DarkRed
        }
        $failCount++
        continue
    }

    # Step 2: Check for golden file
    if (-not (Test-Path $expectedFile)) {
        Write-Host "  [SKIP] $relPath - no .expected golden file (typecheck OK)" -ForegroundColor Yellow
        $skipCount++
        continue
    }

    # Step 3: Run and compare
    $runResult = & cargo run -p buff-lang-cli --release -- run $file.FullName 2>&1
    $runExit = $LASTEXITCODE

    if ($runExit -ne 0) {
        Write-Host "  [FAIL] $relPath - run failed (exit $runExit)" -ForegroundColor Red
        $failCount++
        continue
    }

    $expected = (Get-Content $expectedFile -Raw).TrimEnd()
    $actual = ($runResult | Where-Object { $_ -is [string] } | Out-String).TrimEnd()

    if ($actual -eq $expected) {
        Write-Host "  [PASS] $relPath" -ForegroundColor Green
        $passCount++
    } else {
        Write-Host "  [FAIL] $relPath - output mismatch" -ForegroundColor Red
        if ($Verbose) {
            Write-Host "  Expected: $expected" -ForegroundColor DarkYellow
            Write-Host "  Actual:   $actual" -ForegroundColor DarkRed
        }
        $failCount++
    }
}

Write-Host "`n=== Summary ===" -ForegroundColor Cyan
Write-Host "Pass: $passCount / $total" -ForegroundColor Green
Write-Host "Fail: $failCount / $total" -ForegroundColor $(if ($failCount -gt 0) { 'Red' } else { 'Gray' })
Write-Host "Skip: $skipCount / $total" -ForegroundColor Yellow

if ($failCount -gt 0) {
    exit 1
}
exit 0
