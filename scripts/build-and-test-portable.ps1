param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$dest = [IO.Path]::GetFullPath($Destination)
$package = Join-Path $PSScriptRoot 'package-windows.ps1'
& $package -Destination $dest
& (Join-Path $PSScriptRoot 'test-portable.ps1') -Destination $dest
$rejected = $false
try { & $package -Destination $dest } catch { $rejected = $true }
if (-not $rejected) { throw 'Existing package destination was accepted' }
# Verify the failed collision attempt did not damage the package.
& (Join-Path $PSScriptRoot 'test-portable.ps1') -Destination $dest
