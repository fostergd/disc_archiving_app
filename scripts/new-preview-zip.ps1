param(
    [Parameter(Mandatory=$true)][string]$PackageDirectory,
    [Parameter(Mandatory=$true)][string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$package = [IO.Path]::GetFullPath($PackageDirectory)
$out = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $out) { throw 'ZIP output directory already exists' }
& (Join-Path $PSScriptRoot 'test-portable.ps1') -Destination $package
[void](New-Item -ItemType Directory -Path $out -ErrorAction Stop)
$zip = Join-Path $out 'disc-archive-windows-x64-preview.zip'
$checksum = Join-Path $out 'disc-archive-windows-x64-preview.zip.sha256'
# Compress exactly the validated package entries, not the parent directory.
$entries = @(Get-ChildItem -LiteralPath $package -File | ForEach-Object FullName)
Compress-Archive -LiteralPath $entries -DestinationPath $zip -CompressionLevel Optimal
if ((Get-Item -LiteralPath $zip).Length -ge 2GB) { throw 'Release asset exceeds size limit' }
$extract = Join-Path $out 'validation'
Expand-Archive -LiteralPath $zip -DestinationPath $extract
& (Join-Path $PSScriptRoot 'test-portable.ps1') -Destination $extract
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  disc-archive-windows-x64-preview.zip" | Set-Content -LiteralPath $checksum -Encoding ascii
# Check that the extracted executable and all six internal checksums passed
# before writing the external checksum. No upload is performed by this script.
