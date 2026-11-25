$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:DAS_BURN_CONFIRM -cne 'WRITE-ONCE') { throw 'Explicit WRITE-ONCE confirmation required' }
if ([string]::IsNullOrWhiteSpace($env:DAS_BURN_ID)) { throw 'Explicit recorder identity required' }
if ($env:DAS_BURN_HASH -notmatch '^[0-9a-fA-F]{64}$') { throw 'Invalid expected checksum' }
$image = $env:DAS_BURN_IMAGE
if ($image.StartsWith('\\?\UNC\')) { $image = '\\' + $image.Substring(8) }
elseif ($image.StartsWith('\\?\')) { $image = $image.Substring(4) }
$bytes = [long]::Parse($env:DAS_BURN_BYTES)
if ($bytes -le 0 -or $bytes % 2048 -ne 0) { throw 'Invalid image length' }
# Open a read-only COM IStream using the system shell API, not a memory buffer.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
public static class DasBurnStream {
    [DllImport("shlwapi.dll", CharSet=CharSet.Unicode, PreserveSig=true)]
    private static extern int SHCreateStreamOnFileEx(string path, uint mode, uint attributes,
        [MarshalAs(UnmanagedType.Bool)] bool create, IStream template, out IStream stream);
    public static IStream Open(string path) {
        IStream stream;
        int hr=SHCreateStreamOnFileEx(path,0x20,0,false,null,out stream);
        Marshal.ThrowExceptionForHR(hr);
        return stream;
    }
}
'@
$rec = $null
$format = $null
$stream = $null
$locked = $false
$guard = $null
try {
    $guard = [System.IO.File]::Open($image,[System.IO.FileMode]::Open,[System.IO.FileAccess]::Read,[System.IO.FileShare]::Read)
    if ($guard.Length -ne $bytes) { throw 'Image size changed' }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { $hash = [BitConverter]::ToString($sha.ComputeHash($guard)).Replace('-','') }
    finally { $sha.Dispose() }
    if ($hash -ne $env:DAS_BURN_HASH) { throw 'Image checksum changed' }
    $rec = New-Object -ComObject IMAPI2.MsftDiscRecorder2
    $rec.InitializeDiscRecorder($env:DAS_BURN_ID)
    $rec.AcquireExclusiveAccess($false,'Disc Archive Studio')
    $locked = $true
    $format = New-Object -ComObject IMAPI2.MsftDiscFormat2Data
    if (-not $format.IsRecorderSupported($rec)) { throw 'Unsupported recorder' }
    $format.Recorder = $rec
    $format.ClientName = 'Disc Archive Studio'
    if (-not $format.IsCurrentMediaSupported($rec)) { throw 'Unsupported current medium' }
    $kind = [int]$format.CurrentPhysicalMediaType
    if ($kind -ne [int]$env:DAS_BURN_MEDIA -or $kind -notin @(2,5,8,9,10,18)) { throw 'Media type changed or is not supported write-once media' }
    if (-not $format.MediaPhysicallyBlank -or -not $format.MediaHeuristicallyBlank) { throw 'Medium is not blank' }
    if ([int]$format.CurrentMediaStatus -notin @(0,2)) { throw 'Unsafe media status' }
    $free = [long]$format.FreeSectorsOnMedia
    if ($free -le 0 -or $free -gt [long]$format.TotalSectorsOnMedia -or ($bytes / 2048) -gt $free) { throw 'Insufficient writable capacity' }
    $format.ForceMediaToBeClosed = $true
    $stream = [DasBurnStream]::Open($image)
    $format.Write($stream)
} finally {
    if ($null -ne $stream) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($stream) }
    if ($null -ne $format) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($format) }
    try { if ($locked) { $rec.ReleaseExclusiveAccess() } }
    finally {
        if ($null -ne $rec) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($rec) }
        if ($null -ne $guard) { $guard.Dispose() }
    }
}
Write-Output 'WRITE_COMPLETED'
