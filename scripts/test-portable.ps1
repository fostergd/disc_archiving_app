param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$dest = [IO.Path]::GetFullPath($Destination)
if (-not (Test-Path -LiteralPath $dest -PathType Container)) { throw 'Missing package directory' }
$expected = @('disc-archive.exe','disc-archive-studio.exe','README.md','WINDOWS_ACCEPTANCE.md','LICENSE.md','THIRD_PARTY_NOTICES.md','SHA256SUMS.txt')
foreach ($name in $expected) {
  if (-not (Test-Path -LiteralPath (Join-Path $dest $name) -PathType Leaf)) { throw "Missing package file: $name" }
}
$lines = @(Get-Content -LiteralPath (Join-Path $dest 'SHA256SUMS.txt'))
if ($lines.Count -ne 6) { throw 'Unexpected checksum count' }
foreach ($name in $expected[0..5]) {
  $hash = (Get-FileHash -LiteralPath (Join-Path $dest $name) -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($lines -cnotcontains "$hash  $name") { throw "Checksum mismatch: $name" }
}

$actual = @(Get-ChildItem -LiteralPath $dest -Force)
if ($actual.Count -ne $expected.Count) { throw 'Unexpected package entries' }
foreach ($entry in $actual) {
    if ($entry.PSIsContainer -or $expected -cnotcontains $entry.Name) { throw "Unexpected package entry: $($entry.Name)" }
}
$helpText = & (Join-Path $dest 'disc-archive.exe') --help
if ($LASTEXITCODE -ne 0) { throw 'Packaged CLI failed' }
if (($helpText -join "`n") -notmatch 'experimental write-once optical recording') { throw 'Packaged CLI help is outdated' }
$helpText
