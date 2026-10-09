# check-unwrap-ban.ps1 - mechanize the no-unwrap/expect/panic rule beyond
# clippy's reach (ITER-54). The raw grep figure (~1.9k hits) dissolves via
# honest classification: inline #[cfg(test)] modules, doc-comment examples,
# the parser's own TokenStream::expect method, assertion-macro products,
# quote!-GENERATED code, and scaffold templates are NOT violations. What
# remains must be zero - every deliberate exception lives in the allowlist
# below with a reason.
#
# Usage: powershell -File scripts/check-unwrap-ban.ps1 [-Root <repo>]
# Exit 0 = clean; Exit 1 = undocumented violations (printed).

param([string]$Root = (Join-Path $PSScriptRoot ".."))

$ErrorActionPreference = "Stop"

# Every deliberate exception: "<file substring>|<line substring>|<reason>".
# Line numbers drift, so both matches are substring-based.
$Allowlist = @(
    "crates\buff-assertions\src|panic!|assertion macros' panics ARE the product (test-assertion API)",
    "crates\buff-web3\src\lib.rs|fundamentally broken TLS backend|documented unreachable invariant: a preceding try_from loop pre-validates every well-formed URL spelling",
    "crates\buff-registry\src\storage_sqlite.rs|use SqliteStorage::open in production|Default impl is test/dev convenience; in-memory open+init cannot fail outside OOM/SQLite corruption",
    "crates\buff-lang-codegen-rust\src\rust_codegen\expr_lowering.rs|failed to create tokio runtime|inside quote!{} - GENERATED code that runs in the output program, not the compiler",
    "crates\buff-lang-codegen-rust\src\rust_codegen\prelude_types.rs|#fallback.unwrap()|inside quote!{} - GENERATED fallback chain in output programs",
    "crates\buff-lang-codegen-rust\src\rust_codegen\prelude_types.rs|about:blank|inside quote!{} - GENERATED Url fallback in output programs",
    "crates\buff-lang-runtime\src\tiling.rs|BUFF_FAIL_LOUD_GPU|T70 env-gated development diagnostic (opt-in via env var; zero overhead when absent)",
    "crates\buff-lang-runtime\src\hints.rs|BUFF_FAIL_LOUD_GPU|T70 env-gated development diagnostic (opt-in via env var; zero overhead when absent)"
)

function IsAllowed([string]$file, [string]$line) {
    foreach ($e in $Allowlist) {
        $parts = $e -split '\|', 3
        if ($file.Contains($parts[0]) -and $line.Contains($parts[1])) {
            return $true
        }
    }
    return $false
}

# Context window per hit: the matched line joined with the next 6 lines, so
# multi-line macro invocations (panic!(\n  "BUFF_FAIL_LOUD_GPU: ..."\n))
# allowlist-match on their payload, not just the bare `panic!(` opener.
function ContextOf([array]$lines, [int]$i) {
    $end = [Math]::Min($i + 6, $lines.Count - 1)
    return ($lines[$i..$end] -join ' ')
}

$pattern = '\.unwrap\(\)|panic!\(|todo!\(|unimplemented!\(|\.expect\('
$violations = @( )

$srcFiles = Get-ChildItem -Path (Join-Path $Root "crates") -Recurse -Filter *.rs |
    Where-Object {
        $_.FullName -notmatch '\\tests\\|\\examples\\|\\snapshots\\|\\templates\\'
    }

foreach ($f in $srcFiles) {
    $lines = Get-Content $f.FullName
    $cfgTestLine = $null
    for ($i = 0; $i -lt $lines.Count; $i++) {
        if ($lines[$i] -match '#\[cfg\(test\)\]') { $cfgTestLine = $i + 1; break }
    }
    for ($i = 0; $i -lt $lines.Count; $i++) {
        $ln = $i + 1
        if ($cfgTestLine -and $ln -ge $cfgTestLine) { continue }
        $text = $lines[$i]
        if ($text -notmatch $pattern) { continue }
        if ($text.TrimStart() -match '^(//|///|//!)') { continue }
        # The parser's TokenStream::expect returns Result - not a panic.
        if ($text -match '\.expect\(TokenKind|stream\.expect\(') { continue }
        $rel = $f.FullName.Substring((Resolve-Path $Root).Path.Length + 1)
        $ctx = ContextOf $lines $i
        if (IsAllowed $rel $ctx) { continue }
        $violations += ("{0}:{1}: {2}" -f $rel, $ln, $text.Trim())
    }
}

if ($violations.Count -gt 0) {
    Write-Output "check-unwrap-ban: $($violations.Count) undocumented violation(s):"
    $violations | ForEach-Object { Write-Output "  $_" }
    Write-Output "Fix them, or add a reviewed entry with reason to the allowlist in scripts/check-unwrap-ban.ps1."
    exit 1
}
Write-Output "check-unwrap-ban: clean (0 undocumented; $($Allowlist.Count) reviewed exceptions)"
exit 0
