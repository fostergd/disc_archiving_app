param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
$out = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $out) { throw 'License inventory directory already exists' }
[void](New-Item -ItemType Directory -Path $out)
Push-Location $root
try {
    if (-not (Test-Path -LiteralPath 'Cargo.lock' -PathType Leaf)) {
        throw 'Reviewed Cargo.lock is required; dependency updates need a new review'
    }
    $raw = & cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc --features gui
    if ($LASTEXITCODE -ne 0) { throw 'Windows dependency metadata failed' }
    $metadata = ($raw -join [Environment]::NewLine) | ConvertFrom-Json
    if (-not $metadata.resolve.root) { throw 'Dependency metadata has no root' }
    $resolved = @{}
    foreach ($node in $metadata.resolve.nodes) { $resolved[[string]$node.id] = $true }
    $rows = @()
    foreach ($pkg in ($metadata.packages | Sort-Object name,version)) {
        if ($pkg.id -eq $metadata.resolve.root -or -not $resolved.ContainsKey([string]$pkg.id)) { continue }
        $base = Split-Path -Parent $pkg.manifest_path
        $prefix = $base.TrimEnd([char[]]@('\','/')) + [IO.Path]::DirectorySeparatorChar
        $legal = @(Get-ChildItem -LiteralPath $base -Recurse -File | Where-Object {
            $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE|OFL|UNLICENSE|AUTHORS)([._-].*|\d+.*)?$' -or
            $_.Name -match '^(LICENSE|LICENCE|COPYING|COPYRIGHT|NOTICE|OFL|UNLICENSE|AUTHORS)[A-Z]'
        } | Sort-Object FullName)
        if ($pkg.license_file) {
            $explicit = Join-Path $base $pkg.license_file
            if (-not (Test-Path -LiteralPath $explicit -PathType Leaf)) { throw "Missing declared license file for $($pkg.name)" }
            $legal = @($legal + (Get-Item -LiteralPath $explicit) | Sort-Object FullName -Unique)
        }
        $legalEntries = @()
        foreach ($file in $legal) {
            if (-not $file.FullName.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'License path outside package' }
            $relative = $file.FullName.Substring($prefix.Length)
            $legalEntries += [PSCustomObject]@{path=$relative;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}
        }
        $assets = @(Get-ChildItem -LiteralPath $base -Recurse -File | Where-Object {
            $_.Extension -match '^\.(ttf|otf|woff|woff2|ttc|dll|lib|a|so|dylib)$'
        } | ForEach-Object { $_.FullName.Substring($prefix.Length) })
        $rows += [PSCustomObject]@{
            name=$pkg.name;version=$pkg.version;license=$pkg.license;license_file=$pkg.license_file;
            legal_files=@($legalEntries);embedded_asset_candidates=@($assets)
        }
    }
    if ($rows.Count -eq 0) { throw 'Empty dependency inventory' }
    $lockHash=(Get-FileHash -LiteralPath 'Cargo.lock' -Algorithm SHA256).Hash.ToLowerInvariant()
    Copy-Item -LiteralPath 'Cargo.lock' -Destination (Join-Path $out 'Cargo.lock')
    ConvertTo-Json -InputObject @($rows) -Depth 8 | Set-Content -LiteralPath (Join-Path $out 'windows-license-inventory.json') -Encoding UTF8
    Write-Output "LICENSE_INVENTORY_BEGIN lock_sha256=$lockHash packages=$($rows.Count)"
    foreach ($row in $rows) { Write-Output ($row | ConvertTo-Json -Depth 8 -Compress) }
    Write-Output 'LICENSE_INVENTORY_END'
    # Emit only a crates.io lockfile, not environment, credentials, or Cargo config.
    $lockText = Get-Content -LiteralPath 'Cargo.lock' -Raw
    if ($lockText -match '(?m)^source\s*=\s*"(?!registry\+https://github\.com/rust-lang/crates\.io-index")') {
        throw 'Unexpected non-crates.io dependency source; review before logging lockfile'
    }
    Write-Output 'RESOLVED_LOCKFILE_BEGIN'
    Get-Content -LiteralPath 'Cargo.lock' | ForEach-Object { Write-Output ("LOCK|"+$_) }
    Write-Output 'RESOLVED_LOCKFILE_END'
    Write-Output 'Inventory only: no license clearance or public release authorization is implied.'
} finally {
    Pop-Location
}
