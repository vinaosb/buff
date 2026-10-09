#requires -Version 5.1
# ITER-41: mechanized enforcement of the Cargo.toml conventions that
# previously lived only as prose in root AGENTS.md / CONTRIBUTING.md.
# Checks EVERY workspace crate (crates/*):
#   1) every [dependencies] / [dev-dependencies] (incl. build- and
#      target-specific tables) entry uses `workspace = true` - the root
#      [workspace.dependencies] is the single source for version pins;
#   2) NO [features], [lints], [lints.*], [profile], [profile.*] sections;
#   3) version-tier sanity: core compiler crates 1.2.0, framework/tooling
#      crates 1.0.0, buff-dataframe 2.0.0. buff-lang-* names default to the
#      core tier; the documented tooling-tier exceptions live in the
#      allow-list below (mirrors root AGENTS.md CONVENTIONS + the root
#      Cargo.toml tier comment);
#   4) an AGENTS.md exists in every crate directory.
# Exits nonzero listing all violations. ASCII-only and PS 5.1-compatible so
# it behaves identically under Windows PowerShell 5.1 (local) and pwsh (CI).

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$cratesRoot = Join-Path $repoRoot 'crates'

# --- Version tiers (single source: root Cargo.toml header comment + root AGENTS.md CONVENTIONS) ---
# Core compiler tier: must be exactly 1.2.0.
$coreTierCrates = @(
    'buff-lang-error', 'buff-lang-ast', 'buff-lang-lexer', 'buff-lang-parser',
    'buff-lang-types', 'buff-lang-codegen-rust', 'buff-lang-codegen-wgsl',
    'buff-lang-runtime'
)
# Documented historical exceptions: buff-lang-* named crates that are
# tooling-tier 1.0.0 (root AGENTS.md conventions lists ast-rsx /
# buffhtml-parser / codegen-buffhtml / buff-cli as tooling; debug-info /
# pipeline / check / fmt / ffi-guide ship in the same tier). A NEW
# buff-lang-* crate NOT listed here defaults to the core tier (1.2.0);
# deviating requires adding it to this allow-list.
$langToolingAllowList = @(
    'buff-lang-ast-rsx', 'buff-lang-buffhtml-parser', 'buff-lang-check',
    'buff-lang-cli', 'buff-lang-codegen-buffhtml', 'buff-lang-debug-info',
    'buff-lang-ffi-guide', 'buff-lang-fmt', 'buff-lang-pipeline'
)

$violations = New-Object System.Collections.Generic.List[string]

function Add-Violation {
    param([string]$Message)
    $script:violations.Add($Message) | Out-Null
}

function Test-TextUnbalanced {
    # True while a TOML inline table/array is still open across lines.
    param([string]$Text)
    $openCurly = ([regex]::Matches($Text, '\{')).Count
    $closeCurly = ([regex]::Matches($Text, '\}')).Count
    if ($openCurly -ne $closeCurly) { return $true }
    $openSquare = ([regex]::Matches($Text, '\[')).Count
    $closeSquare = ([regex]::Matches($Text, '\]')).Count
    return ($openSquare -ne $closeSquare)
}

function Test-IsDependencySection {
    param([string]$SectionName)
    if ($SectionName -eq 'dependencies' -or
        $SectionName -eq 'dev-dependencies' -or
        $SectionName -eq 'build-dependencies') { return $true }
    # target-specific tables: [target.'cfg(...)'.dependencies]
    return ($SectionName -match '\.dependencies$')
}

function Test-IsBannedSection {
    param([string]$SectionName)
    if ($SectionName -eq 'features') { return $true }
    if ($SectionName -match '^lints(\.|$)') { return $true }
    if ($SectionName -match '^profile(\.|$)') { return $true }
    return $false
}

if (-not (Test-Path -LiteralPath $cratesRoot)) {
    Write-Output ("::error::crates directory not found at " + $cratesRoot)
    exit 1
}

$crateDirs = Get-ChildItem -LiteralPath $cratesRoot -Directory | Sort-Object Name
$checked = 0

foreach ($crateDir in $crateDirs) {
    $crateName = $crateDir.Name
    $manifestPath = Join-Path $crateDir.FullName 'Cargo.toml'
    $agentsPath = Join-Path $crateDir.FullName 'AGENTS.md'
    $relManifest = 'crates/' + $crateName + '/Cargo.toml'

    if (-not (Test-Path -LiteralPath $manifestPath)) {
        Add-Violation ("crates/" + $crateName + ": missing Cargo.toml (every crates/* directory must be a workspace crate)")
        continue
    }
    if (-not (Test-Path -LiteralPath $agentsPath)) {
        Add-Violation ("crates/" + $crateName + ": missing AGENTS.md (per-crate AGENTS.md required by root AGENTS.md)")
    }

    $checked++
    $lines = [System.IO.File]::ReadAllLines($manifestPath)
    $section = ''
    $i = 0
    while ($i -lt $lines.Count) {
        $line = $lines[$i]
        $lineNo = $i + 1
        $i++

        if ($line -match '^\s*#') { continue }  # comment
        if ($line -match '^\s*$') { continue }  # blank

        $sectionMatch = [regex]::Match($line, '^\s*\[\[?([^\]]+)\]\]?\s*(#.*)?$')
        if ($sectionMatch.Success) {
            $section = $sectionMatch.Groups[1].Value.Trim()
            if (Test-IsBannedSection -SectionName $section) {
                Add-Violation ($relManifest + ":" + $lineNo + ": banned section [" + $section + "] (no [features]/[lints]/[profile.*] in crate manifests - see root AGENTS.md)")
            }
            continue
        }

        if (-not (Test-IsDependencySection -SectionName $section)) {
            if ($section -eq 'package' -and $line -match '^\s*version\s*=\s*"([^"]+)"') {
                $version = $Matches[1]
                $expected = '1.0.0'
                if ($coreTierCrates -contains $crateName) {
                    $expected = '1.2.0'
                } elseif ($langToolingAllowList -contains $crateName) {
                    $expected = '1.0.0'
                } elseif ($crateName -eq 'buff-dataframe') {
                    $expected = '2.0.0'
                } elseif ($crateName.StartsWith('buff-lang-')) {
                    $expected = '1.2.0'
                }
                if ($version -ne $expected) {
                    Add-Violation ($relManifest + ":" + $lineNo + ": version '" + $version + "' violates the tier contract (expected '" + $expected + "') - see the tier comment in root Cargo.toml; documented exceptions live in scripts/check-conventions.ps1")
                }
            }
            continue
        }

        # Dependency entry: accumulate continuation lines while inline
        # tables/arrays are still open (handles multi-line dep entries).
        $entryMatch = [regex]::Match($line, '^\s*([A-Za-z0-9_.-]+)\s*=')
        if (-not $entryMatch.Success) { continue }
        $depName = $entryMatch.Groups[1].Value
        $entryText = $line
        while ((Test-TextUnbalanced -Text $entryText) -and ($i -lt $lines.Count)) {
            $entryText = $entryText + ' ' + $lines[$i]
            $i++
        }
        if ($entryText -notmatch '(^|[.\s])workspace\s*=\s*true') {
            Add-Violation ($relManifest + ":" + $lineNo + ": dependency '" + $depName + "' must use workspace inheritance (dep.workspace = true); pin the version in root [workspace.dependencies] instead")
        }
    }
}

if ($violations.Count -gt 0) {
    Write-Output ("Conventions check FAILED: " + $violations.Count + " violation(s) across " + $checked + " crates")
    foreach ($v in $violations) {
        Write-Output ("::error::" + $v)
    }
    exit 1
}

Write-Output ("Conventions check OK: " + $checked + " crates, 0 violations")
Write-Output "  - all dependency entries use workspace = true"
Write-Output "  - no [features]/[lints]/[profile.*] sections"
Write-Output "  - version tiers: core 1.2.0 / tooling+framework 1.0.0 / buff-dataframe 2.0.0"
Write-Output "  - per-crate AGENTS.md present"
exit 0
