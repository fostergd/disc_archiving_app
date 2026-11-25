$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$manifest = Get-Content -LiteralPath $env:DAS_READBACK_MANIFEST -Raw -Encoding UTF8 | ConvertFrom-Json
$rec = $null
$locked = $false
try {
    $rec = New-Object -ComObject IMAPI2.MsftDiscRecorder2
    $rec.InitializeDiscRecorder($env:DAS_READBACK_ID)
    $paths = @($rec.VolumePathNames | Where-Object { $_ -match '^[A-Za-z]:\\$' })
    if ($paths.Count -ne 1) { throw 'Expected one recorder drive-letter path' }
    $root = [string]$paths[0]
    $drive = New-Object System.IO.DriveInfo($root)
    if ($drive.DriveType -ne [System.IO.DriveType]::CDRom -or -not $drive.IsReady) { throw 'Recorder volume not ready' }
    if ($drive.DriveFormat -ne 'UDF') { throw 'Recorder volume is not UDF' }
    if ($drive.VolumeLabel -cne $manifest.label) { throw 'Volume label mismatch' }
    # Do not acquire exclusive access: filesystem reads require normal volume access.
    # Recheck the recorder mapping and volume metadata after reading.
    $expected = New-Object 'System.Collections.Generic.Dictionary[string,object]' ([StringComparer]::Ordinal)
    foreach ($f in $manifest.files) { $expected.Add([string]$f.archive, $f) }
    $seen = 0
    foreach ($item in Get-ChildItem -LiteralPath $root -Recurse -Force -File) {
        $relative = $item.FullName.Substring($root.Length).Replace('\','/')
        if (-not $expected.ContainsKey($relative)) { throw "Unexpected file: $relative" }
        $f = $expected[$relative]
        if ($item.Length -ne [long]$f.bytes) { throw "Size mismatch: $relative" }
        # Stream through .NET cryptography instead of depending on Get-FileHash.
        $hasher = [System.Security.Cryptography.SHA256]::Create()
        try {
            $inputStream = [System.IO.File]::OpenRead($item.FullName)
            try {
                $hash = [System.BitConverter]::ToString($hasher.ComputeHash($inputStream)).Replace('-', '')
            } finally {
                $inputStream.Dispose()
            }
        } finally {
            $hasher.Dispose()
        }
        if ($hash -ne $f.sha256) { throw "Checksum mismatch: $relative" }
        [void]$expected.Remove($relative)
        $seen++
    }
    if ($expected.Count -ne 0) { throw "Missing files: $($expected.Count)" }
    if ($seen -ne @($manifest.files).Count) { throw 'File count mismatch' }
    $pathsAfter = @($rec.VolumePathNames | Where-Object { $_ -match '^[A-Za-z]:\\$' })
    if ($pathsAfter.Count -ne 1 -or $pathsAfter[0] -ine $root) { throw 'Recorder mapping changed' }
    $after = New-Object System.IO.DriveInfo($root)
    if (-not $after.IsReady -or $after.DriveFormat -ne 'UDF' -or $after.VolumeLabel -cne $manifest.label) { throw 'Recorder volume changed' }
} finally {
    if ($null -ne $rec) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($rec) }
}
Write-Output 'READBACK_COMPLETED'
