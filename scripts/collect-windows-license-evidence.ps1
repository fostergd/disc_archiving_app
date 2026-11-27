param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$root = Split-Path -Parent $PSScriptRoot
$out = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $out) { throw 'License evidence destination already exists' }
$expected = Get-Content -LiteralPath (Join-Path $root 'docs/compliance/windows-license-inventory.json') -Raw | ConvertFrom-Json
$lock = (Get-FileHash -LiteralPath (Join-Path $root 'Cargo.lock') -Algorithm SHA256).Hash.ToLowerInvariant()
if ($lock -cne $expected.lock_sha256) { throw 'Reviewed lockfile changed; review and update inventory first' }
Push-Location $root
try {
    $raw = & cargo metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc --features gui
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed' }
    $metadata = ($raw -join [Environment]::NewLine) | ConvertFrom-Json
    $packages = @{}
    $resolved = @{}
    foreach ($node in $metadata.resolve.nodes) { $resolved[[string]$node.id] = $true }
    foreach ($pkg in $metadata.packages) {
        if ($pkg.id -eq $metadata.resolve.root -or -not $resolved.ContainsKey([string]$pkg.id)) { continue }
        $key = "$($pkg.name)@$($pkg.version)"
        if ($packages.ContainsKey($key)) { throw "Ambiguous package $key" }
        if ($pkg.source -cne 'registry+https://github.com/rust-lang/crates.io-index') { throw "Unexpected package source $key" }
        $packages[$key] = $pkg
    }
    if ($packages.Count -ne @($expected.packages).Count) { throw 'Resolved dependency count changed' }
    [void](New-Item -ItemType Directory -Path $out)
    $collected = @()
    $missing = @()
    $seen = @{}
    foreach ($row in $expected.packages) {
        $key = "$($row.name)@$($row.version)"
        if ($seen.ContainsKey($key) -or -not $packages.ContainsKey($key)) { throw "Unexpected inventory identity $key" }
        $seen[$key] = $true
        if ($key -notmatch '^[A-Za-z0-9_-]+@[A-Za-z0-9.+-]+$') { throw 'Unsafe package name/version' }
        $pkg = $packages[$key]
        if ($pkg.license -cne $row.license -or $pkg.license_file -cne $row.license_file) { throw "License metadata changed: $key" }
        $base = Split-Path -Parent $pkg.manifest_path
        $prefix = $base.TrimEnd([char[]]@('\','/')) + [IO.Path]::DirectorySeparatorChar
        if (@($row.legal_files).Count -eq 0) {
            $vcs = $null
            $vcsFile = Join-Path $base '.cargo_vcs_info.json'
            if (Test-Path -LiteralPath $vcsFile -PathType Leaf) {
                $vcs = Get-Content -LiteralPath $vcsFile -Raw | ConvertFrom-Json
            }
            $missing += [PSCustomObject]@{package=$key;license=$pkg.license;repository=$pkg.repository;vcs=$vcs}
        }
        foreach ($item in $row.legal_files) {
            $source = [IO.Path]::GetFullPath((Join-Path $base $item.path))
            if (-not $source.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'License path outside crate' }
            $digest = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
            if ($digest -cne $item.sha256) { throw "Notice hash mismatch: $key/$($item.path)" }
            $relative = Join-Path (Join-Path 'registry' $key) $item.path
            $target = Join-Path $out $relative
            [void](New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force)
            if (Test-Path -LiteralPath $target) { throw "Duplicate evidence file $relative" }
            Copy-Item -LiteralPath $source -Destination $target
            if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant() -cne $digest) { throw 'Copied notice hash mismatch' }
            $collected += [PSCustomObject]@{package=$key;path=$relative;sha256=$digest}
        }
    }
    $upstreamPath = Join-Path $root 'docs/compliance/upstream/egui-0.31.1/provenance.json'
    $upstream = Get-Content -LiteralPath $upstreamPath -Raw | ConvertFrom-Json
    foreach ($item in $upstream.notices) {
        $source = Join-Path $root $item.path
        $blob = & git hash-object --no-filters $source
        if ($LASTEXITCODE -ne 0 -or $blob -cne $item.git_blob_sha1) { throw "Upstream Git blob mismatch: $($item.path)" }
        $relative = Join-Path 'upstream/egui-0.31.1' (Split-Path -Leaf $source)
        $target = Join-Path $out $relative
        [void](New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force)
        if (Test-Path -LiteralPath $target) { throw 'Duplicate upstream evidence' }
        Copy-Item -LiteralPath $source -Destination $target
        $digest = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
        if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant() -cne $digest) { throw 'Copied upstream notice mismatch' }
        $collected += [PSCustomObject]@{package='egui upstream 0.31.1';path=$relative;sha256=$digest}
    }
    # Resolve every previously missing registry notice by exact crate VCS provenance.
    $supplement = Get-Content -LiteralPath (Join-Path $root 'docs/compliance/upstream/missing-registry-provenance.json') -Raw | ConvertFrom-Json
    $upstreamRoot = [IO.Path]::GetFullPath((Join-Path $root 'docs/compliance/upstream'))
    $upstreamPrefix = $upstreamRoot.TrimEnd([char[]]@('\','/')) + [IO.Path]::DirectorySeparatorChar
    foreach ($item in $supplement.notices) {
        $source = [IO.Path]::GetFullPath((Join-Path $root $item.path))
        if (-not $source.StartsWith($upstreamPrefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Upstream notice outside evidence root' }
        $blob = & git hash-object --no-filters $source
        if ($LASTEXITCODE -ne 0 -or $blob -cne $item.git_blob_sha1) { throw "Upstream Git blob mismatch: $($item.path)" }
        $relative = Join-Path 'upstream' $source.Substring($upstreamPrefix.Length)
        $target = Join-Path $out $relative
        [void](New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force)
        if (Test-Path -LiteralPath $target) { throw 'Duplicate upstream evidence' }
        Copy-Item -LiteralPath $source -Destination $target
        $digest = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
        if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash.ToLowerInvariant() -cne $digest) { throw 'Copied upstream notice mismatch' }
        $collected += [PSCustomObject]@{package=$item.repository;path=$relative;sha256=$digest}
    }
    if (@($supplement.mappings).Count -ne $missing.Count) { throw 'Missing-license mapping count differs from observed packages' }
    $mapped = @{}
    foreach ($item in $supplement.mappings) {
        $match = @($missing | Where-Object { $_.package -ceq $item.package })
        if ($match.Count -ne 1 -or $mapped.ContainsKey($item.package)) { throw "Missing or duplicate mapping: $($item.package)" }
        $vcs = $match[0].vcs
        if (-not $vcs -or $vcs.git.sha1 -cne $item.commit) { throw "Crate VCS commit differs: $($item.package)" }
        $observedPath = $null
        if ($vcs.PSObject.Properties.Name -contains 'path_in_vcs') { $observedPath = $vcs.path_in_vcs }
        if ($observedPath -cne $item.path_in_vcs) { throw "Crate VCS path differs: $($item.package)" }
        if (@($item.notices).Count -eq 0) { throw 'Empty upstream notice mapping' }
        foreach ($notice in $item.notices) {
            $target = [IO.Path]::GetFullPath((Join-Path (Join-Path $out 'upstream') $notice))
            $evidencePrefix = [IO.Path]::GetFullPath((Join-Path $out 'upstream')) + [IO.Path]::DirectorySeparatorChar
            if (-not $target.StartsWith($evidencePrefix,[StringComparison]::OrdinalIgnoreCase) -or -not (Test-Path -LiteralPath $target -PathType Leaf)) { throw "Missing mapped notice $notice" }
        }
        $mapped[$item.package] = $true
    }
    Write-Output "MISSING_REGISTRY_MAPPING_PASS mapped=$($mapped.Count)"
    $report = [PSCustomObject]@{
        lock_sha256=$lock;registry_candidate_count=$packages.Count;copied_notice_count=$collected.Count;
        missing_registry_notices=$missing;upstream_mapped_missing_count=$mapped.Count;files=$collected;public_distribution_approved=$false
    }
    $report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $out 'review-evidence.json') -Encoding UTF8
    Write-Output "NOTICE_COLLECTION_PASS copied=$($collected.Count) candidates=$($packages.Count) missing_registry=$($missing.Count)"
    foreach ($item in $missing) { Write-Output ('MISSING_NOTICE|' + ($item | ConvertTo-Json -Depth 8 -Compress)) }
    Write-Output 'Review evidence only; no complete-license or public-distribution approval.'
} finally {
    Pop-Location
}
