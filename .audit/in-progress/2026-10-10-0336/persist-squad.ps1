param(
  [Parameter(Mandatory=$true)][string]$Source,
  [Parameter(Mandatory=$true)][string]$SquadFile
)
$ErrorActionPreference = 'Stop'
$runDir = '.audit/in-progress/2026-10-10-0336'
$raw = Get-Content -LiteralPath $Source -Raw
$m = [regex]::Matches($raw, '"squad"\s*:\s*"audit-')
if ($m.Count -eq 0) { throw "no squad JSON marker in $Source" }
$j = $raw.LastIndexOf('{', $m[$m.Count-1].Index)
if ($j -lt 0) { throw "no opening brace before squad marker in $Source" }
$cand = $raw.Substring($j).Trim()
# tolerant single-pass escape fixer: lone backslash before invalid escape char -> doubled
$valid = '"','\','/','b','f','n','r','t','u'
$sb = New-Object System.Text.StringBuilder
$i = 0
while ($i -lt $cand.Length) {
  $ch = $cand[$i]
  if ($ch -eq '\' -and ($i + 1) -lt $cand.Length) {
    $nx = $cand[$i+1]
    if ($valid -contains $nx) { [void]$sb.Append($ch); [void]$sb.Append($nx); $i += 2; continue }
    [void]$sb.Append('\\'); $i += 1; continue
  }
  [void]$sb.Append($ch); $i += 1
}
$fixed = $sb.ToString()
# pass 2 refined: inside-string quote is CLOSING iff next-non-ws in ':]}' OR (',' followed by value-start char " { [ digit t f n -)
$sb2 = New-Object System.Text.StringBuilder
$k = 0; $inStr = $false
while ($k -lt $fixed.Length) {
  $c = $fixed[$k]
  if ($c -eq '\') { [void]$sb2.Append($fixed.Substring($k,2)); $k += 2; continue }
  if ($c -eq '"') {
    if (-not $inStr) { $inStr = $true; [void]$sb2.Append($c); $k++; continue }
    $n = $k + 1
    while ($n -lt $fixed.Length -and ($fixed[$n] -match '\s')) { $n++ }
    $nc = if ($n -lt $fixed.Length) { $fixed[$n] } else { ']' }
    $closing = $false
    if (':]}'.IndexOf($nc) -ge 0) { $closing = $true }
    elseif ($nc -eq ',') {
      $n2 = $n + 1
      while ($n2 -lt $fixed.Length -and ($fixed[$n2] -match '\s')) { $n2++ }
      $nc2 = if ($n2 -lt $fixed.Length) { $fixed[$n2] } else { ']' }
      if ('"{[0123456789tfn-'.IndexOf($nc2) -ge 0) { $closing = $true }
    }
    if ($closing) { $inStr = $false; [void]$sb2.Append($c) } else { [void]$sb2.Append('\"') }
    $k++; continue
  }
  [void]$sb2.Append($c); $k++
}
$fixed = $sb2.ToString()
$obj = $null
try { $obj = $fixed | ConvertFrom-Json } catch {
  $tries = 0
  while (-not $obj -and $tries -lt 20) {
    $tries++
    $end = $fixed.LastIndexOf('}')
    if ($end -lt 2) { throw "unparseable squad JSON in ${Source}: $($_.Exception.Message)" }
    $fixed = $fixed.Substring(0, $end + 1)
    try { $obj = $fixed | ConvertFrom-Json } catch { }
  }
  if (-not $obj) { throw "unparseable squad JSON in $Source" }
}
$obj | Add-Member -NotePropertyName report_version -NotePropertyValue '3.2' -Force
$obj | Add-Member -NotePropertyName run_id -NotePropertyValue '2026-10-10-0336' -Force
if (-not $obj.PSObject.Properties['findings']) { throw "no findings array in $Source" }
$tmp = Join-Path "$runDir/squads" "$SquadFile.tmp"
$obj | ConvertTo-Json -Depth 30 | Set-Content -LiteralPath $tmp -Encoding UTF8
$null = Get-Content -LiteralPath $tmp -Raw | ConvertFrom-Json
Move-Item -LiteralPath $tmp -Destination (Join-Path "$runDir/squads" $SquadFile) -Force
$fin = Get-Content -LiteralPath (Join-Path "$runDir/squads" $SquadFile) -Raw | ConvertFrom-Json
$t = $fin.summary.totals
"WROTE $SquadFile findings=$($fin.findings.Count) fe=$($fin.summary.files_examined.Count) fns=$($fin.summary.falseNegativesIdentified.Count) sev(c/h/m/l)=$($t.critical)/$($t.high)/$($t.medium)/$($t.low)"
