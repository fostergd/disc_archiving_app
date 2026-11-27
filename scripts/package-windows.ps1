param([Parameter(Mandatory=$true)][string]$Destination,[string]$NoticeBundle)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
$dest = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $dest) { throw 'Package destination already exists' }
$names = @('disc-archive.exe','disc-archive-studio.exe')
foreach ($name in $names) {
    $source = Join-Path $root "target/release/$name"
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Missing release executable: $name" }
}
if ($NoticeBundle) {
    python (Join-Path $PSScriptRoot 'build-license-bundle.py') --verify $NoticeBundle
    if ($LASTEXITCODE -ne 0) { throw 'Invalid review notice bundle' }
}
[void](New-Item -ItemType Directory -Path $dest -ErrorAction Stop)
foreach ($name in $names) {
    Copy-Item -LiteralPath (Join-Path $root "target/release/$name") -Destination (Join-Path $dest $name)
}
Copy-Item -LiteralPath (Join-Path $root 'README.md') -Destination (Join-Path $dest 'README.md')
Copy-Item -LiteralPath (Join-Path $root 'WINDOWS_ACCEPTANCE.md') -Destination (Join-Path $dest 'WINDOWS_ACCEPTANCE.md')
Copy-Item -LiteralPath (Join-Path $root 'LICENSE.md') -Destination (Join-Path $dest 'LICENSE.md')
Copy-Item -LiteralPath (Join-Path $root 'THIRD_PARTY_NOTICES.md') -Destination (Join-Path $dest 'THIRD_PARTY_NOTICES.md')
$payload = $names + @('README.md','WINDOWS_ACCEPTANCE.md','LICENSE.md','THIRD_PARTY_NOTICES.md')
if ($NoticeBundle) {
    Copy-Item -LiteralPath $NoticeBundle -Destination (Join-Path $dest 'THIRD_PARTY_LICENSES.zip')
    $payload += 'THIRD_PARTY_LICENSES.zip'
}
$lines = foreach ($name in $payload) {
    $hash = (Get-FileHash -LiteralPath (Join-Path $dest $name) -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $name"
}
$lines | Set-Content -LiteralPath (Join-Path $dest 'SHA256SUMS.txt') -Encoding ascii
