$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Only mount the exact supplied ISO; never use an existing physical optical volume.
$image = $env:DAS_VERIFY_IMAGE
# Storage cmdlets expect conventional local Windows paths rather than extended syntax.
if ($image.StartsWith('\\?\UNC\')) { $image = '\\' + $image.Substring(8) }
elseif ($image.StartsWith('\\?\')) { $image = $image.Substring(4) }
$manifest = Get-Content -LiteralPath $env:DAS_VERIFY_MANIFEST -Raw -Encoding UTF8 | ConvertFrom-Json
$existing = Get-DiskImage -ImagePath $image
if ($existing.Attached) { throw 'Image is already mounted; refusing to detach a pre-existing mount' }
$mounted = $false
try {
    $disk = Mount-DiskImage -ImagePath $image -StorageType ISO -Access ReadOnly -PassThru
    $mounted = $true
    $volumes = @()
    for ($i = 0; $i -lt 30; $i++) {
        $volumes = @($disk | Get-Volume | Where-Object { $_.DriveLetter })
        if ($volumes.Count -gt 0) { break }
        Start-Sleep -Seconds 1
    }
    if ($volumes.Count -ne 1) { throw 'Expected exactly one mounted image volume with a drive letter' }
    $vol = $volumes[0]
    if ($vol.FileSystem -ne 'UDF') { throw 'Mounted image is not UDF' }
    if ($vol.FileSystemLabel -cne $manifest.label) { throw 'Volume label mismatch' }
    $root = [string]$vol.DriveLetter + ':\'
    $expected = New-Object 'System.Collections.Generic.Dictionary[string,object]' ([StringComparer]::Ordinal)
    foreach ($f in $manifest.files) { $expected.Add([string]$f.archive, $f) }
    $seen = 0
    foreach ($item in Get-ChildItem -LiteralPath $root -Recurse -Force -File) {
        $relative = $item.FullName.Substring($root.Length).Replace('\','/')
        if (-not $expected.ContainsKey($relative)) { throw "Unexpected file: $relative" }
        $f = $expected[$relative]
        if ($item.Length -ne [long]$f.bytes) { throw "Size mismatch: $relative" }
        $hash = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash
        if ($hash -ne $f.sha256) { throw "Checksum mismatch: $relative" }
        [void]$expected.Remove($relative)
        $seen++
    }
    if ($expected.Count -ne 0) { throw "Missing files: $($expected.Count)" }
    if ($seen -ne @($manifest.files).Count) { throw 'File count mismatch' }
} finally {
    if ($mounted) { Dismount-DiskImage -ImagePath $image -ErrorAction Stop | Out-Null }
}
