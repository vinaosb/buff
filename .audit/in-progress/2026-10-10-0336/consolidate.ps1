# consolidate.ps1 - v3.2 consolidation + emission for run 2026-10-10-0336
# Reads squad files FROM DISK, validates v3.1 schema, merges, emits JSON/MD/SARIF atomically.
$ErrorActionPreference = 'Stop'
$runId = '2026-10-10-0336'
$runDir = ".audit/in-progress/$runId"
$squadDir = "$runDir/squads"
$outDir = '.sisyphus/reports'

$required = @('id','severity','file','line','title','description','root_cause','impact','recommendation','fix_examples','fix_guide','effort_estimate','technology','category','detection_method','verification_command','references','confidence','fp_risk_self_assessment')

$allFindings = @()
$provenance = @()
$coverage = @{}
$squadStats = @()
$validationErrors = @()

foreach ($f in (Get-ChildItem $squadDir -Filter 'audit-*.json' | Sort-Object Name)) {
  $squad = $null
  try { $squad = Get-Content -LiteralPath $f.FullName -Raw | ConvertFrom-Json } catch { $validationErrors += "$($f.Name): file unparseable: $($_.Exception.Message)"; continue }
  $sname = $squad.squad
  $errs = @()
  $kept = @()
  foreach ($fd in @($squad.findings)) {
    $missing = @($required | Where-Object { -not $fd.PSObject.Properties[$_] })
    if ($missing.Count -gt 0) { $errs += "$($fd.id): missing [$($missing -join ',')]"; continue }
    $kept += $fd
  }
  $provenance += [PSCustomObject]@{ squad = $sname; file = $f.Name; findings_submitted = @($squad.findings).Count; findings_accepted = $kept.Count; validation_errors = $errs }
  foreach ($fd in $kept) { $allFindings += $fd }
  $t = $squad.summary.totals
  $squadStats += [PSCustomObject]@{ squad = $sname; critical = $t.critical; high = $t.high; medium = $t.medium; low = $t.low; files_examined = @($squad.summary.files_examined).Count; false_negatives = @($squad.summary.falseNegativesIdentified).Count }
  foreach ($fe in @($squad.summary.files_examined)) {
    $key = ($fe.path -replace '\\','/')
    if (-not $coverage.ContainsKey($key)) { $coverage[$key] = @{ squads = @(); verdict = 'clean' } }
    $coverage[$key].squads += $sname
    if ($fe.verdict -eq 'findings') { $coverage[$key].verdict = 'findings' }
  }
}
# fingerprints + delta (baseline run: all new)
$sha = [System.Security.Cryptography.SHA256]::Create()
$now = (Get-Date).ToString('yyyy-MM-ddTHH:mm:sszzz')
foreach ($fd in $allFindings) {
  $raw = ("$($fd.file)|$($fd.line)|$($fd.title)".ToLower())
  $h = $sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($raw))
  $fp = ([System.BitConverter]::ToString($h) -replace '-','').Substring(0,16)
  $fd | Add-Member -NotePropertyName fingerprint -NotePropertyValue $fp -Force
  $fd | Add-Member -NotePropertyName status -NotePropertyValue 'new' -Force
  $fd | Add-Member -NotePropertyName first_seen -NotePropertyValue $now -Force
  $fd | Add-Member -NotePropertyName last_seen -NotePropertyValue $now -Force
  $fd | Add-Member -NotePropertyName squad -NotePropertyValue ($fd.id -replace '^AUD-([A-Z]+)-\d+$','$1') -Force
}
# FP/FN + DAG injection from optional authored files
$falsePositives = @(); $falseNegatives = @(); $dag = $null; $validation = $null
if (Test-Path "$runDir/validation.json") { $validation = Get-Content "$runDir/validation.json" -Raw | ConvertFrom-Json; $falsePositives = @($validation.false_positives); $falseNegatives = @($validation.false_negatives) }
if (Test-Path "$runDir/dag.json") { $dag = Get-Content "$runDir/dag.json" -Raw | ConvertFrom-Json }

$totals = @{ critical = @($allFindings | Where-Object severity -eq 'critical').Count; high = @($allFindings | Where-Object severity -eq 'high').Count; medium = @($allFindings | Where-Object severity -eq 'medium').Count; low = @($allFindings | Where-Object severity -eq 'low').Count }
$coverageList = @()
foreach ($k in ($coverage.Keys | Sort-Object)) { $coverageList += [PSCustomObject]@{ path = $k; examined_by = $coverage[$k].squads; verdict = $coverage[$k].verdict } }

$report = [PSCustomObject]@{
  report_version = '3.2'; run_id = $runId; project = 'buff'; generated_at = $now
  totals = $totals; findings = $allFindings
  squads = $squadStats
  coverage_map = @{ files_examined_count = $coverageList.Count; files_with_findings = @($coverageList | Where-Object verdict -eq 'findings').Count; files = $coverageList }
  delta = @{ previous = $null; baseline_run = $true; new = $totals; resolved = 0; unchanged = 0; note = 'first consolidated run - all findings status=new' }
  fix_dependency_dag = $dag
  false_positives = $falsePositives
  false_negatives = $falseNegatives
  provenance = [PSCustomObject]@{ squads = $provenance; validation_errors = $validationErrors; notes = 'schema per audit-template 7.1/7.2; squad outputs read from disk only' }
  ci_gate = @{ fail_on = 'none'; evaluated = $false }
}

# emit consolidated JSON (atomic)
$jTmp = "$runDir/consolidated.partial.json.tmp"
$report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath $jTmp -Encoding UTF8
$null = Get-Content -LiteralPath $jTmp -Raw | ConvertFrom-Json
Move-Item -LiteralPath $jTmp -Destination "$runDir/consolidated.partial.json" -Force
"CONSOLIDATED: findings=$($allFindings.Count) sev(c/h/m/l)=$($totals.critical)/$($totals.high)/$($totals.medium)/$($totals.low) coverage_files=$($coverageList.Count) schema_rejects=$(@($validationErrors).Count)"
$report | ConvertTo-Json -Depth 40 | Set-Content -LiteralPath "$outDir/buff-audit-$runId-v3.json.tmp" -Encoding UTF8
Move-Item -LiteralPath "$outDir/buff-audit-$runId-v3.json.tmp" -Destination "$outDir/buff-audit-$runId-v3.json" -Force
"EMIT json -> $outDir/buff-audit-$runId-v3.json"

# emit markdown (atomic)
$md = New-Object System.Text.StringBuilder
[void]$md.AppendLine("# Buff Audit Report v3.2 - Run $runId")
[void]$md.AppendLine("")
[void]$md.AppendLine("Generated: $now | Project: buff (73-crate Rust workspace) | Mode: all squads")
[void]$md.AppendLine("")
[void]$md.AppendLine("## Totals")
[void]$md.AppendLine("")
[void]$md.AppendLine("| Severity | Count |")
[void]$md.AppendLine("|---|---|")
[void]$md.AppendLine("| critical | $($totals.critical) |")
[void]$md.AppendLine("| high | $($totals.high) |")
[void]$md.AppendLine("| medium | $($totals.medium) |")
[void]$md.AppendLine("| low | $($totals.low) |")
[void]$md.AppendLine("| **total** | **$($allFindings.Count)** |")
[void]$md.AppendLine("")
[void]$md.AppendLine("## Squad Summary")
[void]$md.AppendLine("")
[void]$md.AppendLine("| Squad | c | h | m | l | files_examined | FNs |")
[void]$md.AppendLine("|---|---|---|---|---|---|---|")
foreach ($s in $squadStats) { [void]$md.AppendLine("| $($s.squad) | $($s.critical) | $($s.high) | $($s.medium) | $($s.low) | $($s.files_examined) | $($s.false_negatives) |") }
[void]$md.AppendLine("")
[void]$md.AppendLine("## Findings")
[void]$md.AppendLine("")
$sevRank = @{ critical = 0; high = 1; medium = 2; low = 3 }
foreach ($fd in ($allFindings | Sort-Object { $sevRank[$_.severity] }, id)) {
  [void]$md.AppendLine("### $($fd.id) [$($fd.severity)] $($fd.title)")
  [void]$md.AppendLine("")
  [void]$md.AppendLine("- **Squad:** $($fd.squad) | **File:** ``$($fd.file)``:$($fd.line)$(if ($fd.endLine -and $fd.endLine -ne $fd.line) { "-$($fd.endLine)" }) | **Category:** $($fd.category)")
  [void]$md.AppendLine("- **Impact:** $($fd.impact)")
  [void]$md.AppendLine("- **Root cause:** $($fd.root_cause)")
  [void]$md.AppendLine("- **Recommendation:** $($fd.recommendation)")
  [void]$md.AppendLine("- **Effort:** $($fd.effort_estimate.human_readable) (~$($fd.effort_estimate.minutes) min, confidence $($fd.effort_estimate.confidence)) | **Confidence:** $($fd.confidence.score)")
  [void]$md.AppendLine("- **FP risk (self):** $($fd.fp_risk_self_assessment.risk_level) | **Verify:** ``$($fd.verification_command)``")
  if ($fd.fix_examples) { [void]$md.AppendLine(""); [void]$md.AppendLine('```rust'); [void]$md.AppendLine($fd.fix_examples.before); [void]$md.AppendLine('```'); [void]$md.AppendLine('```rust'); [void]$md.AppendLine($fd.fix_examples.after); [void]$md.AppendLine('```') }
  [void]$md.AppendLine("")
}
if ($dag) {
  [void]$md.AppendLine("## Fix Dependency DAG")
  [void]$md.AppendLine("")
  [void]$md.AppendLine('```mermaid')
  [void]$md.AppendLine('graph TD')
  foreach ($e in @($dag.edges)) { [void]$md.AppendLine("  $($e.from -replace '-','_')[$($e.from)] -->|$(if ($e.reason) { $e.reason } else { '' })| $($e.to -replace '-','_')[$($e.to)]") }
  [void]$md.AppendLine('```')
  [void]$md.AppendLine("")
}
if ($falsePositives.Count -gt 0 -or $falseNegatives.Count -gt 0) {
  [void]$md.AppendLine("## Critical-Thinking Validation (FP/FN)")
  [void]$md.AppendLine("")
  [void]$md.AppendLine("Validated: $(@($validation.validated_finding_ids).Count) findings (sample $(if ($validation) { $validation.sample_percent } else { 20 })%). Confirmed FPs: $($falsePositives.Count). Confirmed FNs: $($falseNegatives.Count).")
  [void]$md.AppendLine("")
}
[void]$md.AppendLine("## Coverage")
[void]$md.AppendLine("")
[void]$md.AppendLine("Files examined (union across squads): $($coverageList.Count); with findings: $(@($coverageList | Where-Object verdict -eq 'findings').Count).")
[void]$md.AppendLine("")
$mTmp = "$runDir/report.md.tmp"
$txt = $md.ToString()
[System.IO.File]::WriteAllText("$PWD\$mTmp", $txt, (New-Object System.Text.UTF8Encoding($false)))
Move-Item -LiteralPath $mTmp -Destination "$outDir/buff-audit-$runId-v3.md" -Force
"EMIT md -> $outDir/buff-audit-$runId-v3.md"

# emit SARIF 2.1.0 (atomic)
$rules = @(); $results = @()
$ruleIndex = @{}
$i = 0
foreach ($fd in ($allFindings | Sort-Object id)) {
  if (-not $ruleIndex.ContainsKey($fd.category)) { $ruleIndex[$fd.category] = $i; $rules += @{ id = "BUFF-$($fd.category)"; shortDescription = @{ text = "$($fd.category) finding" }; helpUri = "https://github.com/example/buff/AGENTS.md" }; $i++ }
  $lvl = if ($fd.severity -eq 'critical' -or $fd.severity -eq 'high') { 'error' } elseif ($fd.severity -eq 'medium') { 'warning' } else { 'note' }
  $results += @{ ruleId = "BUFF-$($fd.category)"; level = $lvl; message = @{ text = "[$($fd.id)] $($fd.title) -- $($fd.description)" }; locations = @(@{ physicalLocation = @{ artifactLocation = @{ uri = ($fd.file -replace '\\','/') }; region = @{ startLine = [int]$fd.line; endLine = $(if ($fd.endLine) { [int]$fd.endLine } else { [int]$fd.line }) } } }) }
}
$sarif = @{ version = '2.1.0'; '$schema' = 'https://json.schemastore.org/sarif-2.1.0.json'; runs = @(@{ tool = @{ driver = @{ name = 'buff-audit'; informationUri = 'https://github.com/example/buff'; version = '3.2' } }; rules = $rules; results = $results }) }
$sTmp = "$runDir/report.sarif.tmp"
$sarif | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $sTmp -Encoding UTF8
$null = Get-Content -LiteralPath $sTmp -Raw | ConvertFrom-Json
Move-Item -LiteralPath $sTmp -Destination "$outDir/buff-audit-$runId-v3.sarif" -Force
"EMIT sarif -> $outDir/buff-audit-$runId-v3.sarif"
"DONE"