param([Parameter(Mandatory=$true)][string]$Destination,[switch]$RequireNoticeBundle)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$dest = [IO.Path]::GetFullPath($Destination)
if (-not (Test-Path -LiteralPath $dest -PathType Container)) { throw 'Missing package directory' }
$payload = @('disc-archive.exe','disc-archive-studio.exe','README.md','WINDOWS_ACCEPTANCE.md','LICENSE.md','THIRD_PARTY_NOTICES.md')
$bundle = Join-Path $dest 'THIRD_PARTY_LICENSES.zip'
if (Test-Path -LiteralPath $bundle -PathType Leaf) {
    $payload += 'THIRD_PARTY_LICENSES.zip'
    python (Join-Path $PSScriptRoot 'build-license-bundle.py') --verify $bundle
    if ($LASTEXITCODE -ne 0) { throw 'Invalid review notice bundle' }
} elseif ($RequireNoticeBundle) { throw 'Required notice bundle missing' }
$expected = $payload + 'SHA256SUMS.txt'
foreach ($name in $expected) {
  if (-not (Test-Path -LiteralPath (Join-Path $dest $name) -PathType Leaf)) { throw "Missing package file: $name" }
}
$lines = @(Get-Content -LiteralPath (Join-Path $dest 'SHA256SUMS.txt'))
if ($lines.Count -ne $payload.Count) { throw 'Unexpected checksum count' }
foreach ($name in $payload) {
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
