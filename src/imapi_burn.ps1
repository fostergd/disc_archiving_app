param([switch]$TelemetrySelfTest)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $TelemetrySelfTest) {
if ($env:DAS_BURN_CONFIRM -cne 'WRITE-ONCE') { throw 'Explicit WRITE-ONCE confirmation required' }
if ([string]::IsNullOrWhiteSpace($env:DAS_BURN_ID)) { throw 'Explicit recorder identity required' }
if ($env:DAS_BURN_HASH -notmatch '^[0-9a-fA-F]{64}$') { throw 'Invalid expected checksum' }
$image = $env:DAS_BURN_IMAGE
if ($image.StartsWith('\\?\UNC\')) { $image = '\\' + $image.Substring(8) }
elseif ($image.StartsWith('\\?\')) { $image = $image.Substring(4) }
$bytes = [long]::Parse($env:DAS_BURN_BYTES)
if ($bytes -le 0 -or $bytes % 2048 -ne 0) { throw 'Invalid image length' }
}
# Open a read-only COM IStream using the system shell API, not a memory buffer.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
using System.Globalization;
using System.Reflection;
public static class DasBurnStream {
    [DllImport("shlwapi.dll", CharSet=CharSet.Unicode, ExactSpelling=true, PreserveSig=true)]
    private static extern int SHCreateStreamOnFileEx(string path, uint mode, uint attributes,
        [MarshalAs(UnmanagedType.Bool)] bool create, IStream template, out IStream stream);
    public static IStream Open(string path) {
        IStream stream;
        int hr=SHCreateStreamOnFileEx(path,0x20,0,false,null,out stream);
        Marshal.ThrowExceptionForHR(hr);
        return stream;
    }
    // Exercise the actual native stream with a caller-owned, disposable fixture.
    public static void CheckFixture(string path) {
        byte[] expected = new byte[4096];
        for (int i = 0; i < expected.Length; i++) expected[i] = (byte)(i % 251);
        using (var file = new System.IO.FileStream(path, System.IO.FileMode.CreateNew, System.IO.FileAccess.Write, System.IO.FileShare.None)) {
            file.Write(expected, 0, expected.Length);
        }
        using (var guard = new System.IO.FileStream(path, System.IO.FileMode.Open, System.IO.FileAccess.Read, System.IO.FileShare.Read)) {
            IStream stream = Open(path);
            IntPtr count = Marshal.AllocHGlobal(4);
            try {
                System.Runtime.InteropServices.ComTypes.STATSTG info;
                stream.Stat(out info, 1);
                if (info.cbSize != expected.Length) throw new InvalidOperationException("Native stream length mismatch");
                byte[] actual = new byte[expected.Length];
                stream.Read(actual, actual.Length, count);
                if (Marshal.ReadInt32(count) != actual.Length) throw new InvalidOperationException("Native stream short read");
                for (int i = 0; i < actual.Length; i++) {
                    if (actual[i] != expected[i]) throw new InvalidOperationException("Native stream byte mismatch");
                }
                stream.Read(new byte[1], 1, count);
                if (Marshal.ReadInt32(count) != 0) throw new InvalidOperationException("Native stream EOF mismatch");
                stream.Seek(0, 0, IntPtr.Zero);
                stream.Read(actual, 1, count);
                if (Marshal.ReadInt32(count) != 1 || actual[0] != expected[0]) throw new InvalidOperationException("Native stream seek mismatch");
            } finally {
                Marshal.FreeHGlobal(count);
                Marshal.ReleaseComObject(stream);
            }
        }
    }
}
public static class DasBurnTelemetry {
    private static readonly Guid Events = new Guid("2735413C-7F64-5B0F-8F00-5D77AFBE261E");
    public static string FormatProgress(long action, long start, long last, long total) {
        // IMAPI reports logical block addresses, not bytes. Do not infer
        // finalization or verification from the data-writing percentage.
        if (action != 4 || start < 0 || last < start || total <= 0) return null;
        long written = last - start;
        if (written > total) return null;
        return String.Format(CultureInfo.InvariantCulture,"DAS_PROGRESS {0} {1}",written,total);
    }
    private static long Property(object o, string name) {
        return Convert.ToInt64(o.GetType().InvokeMember(name,BindingFlags.GetProperty,null,o,null),CultureInfo.InvariantCulture);
    }
    public static void Update(object sender, object progress) {
        // Optional telemetry must never abort a native write.
        try {
            if (Property(progress,"CurrentAction") != 4) return;
            string line = FormatProgress(4,Property(progress,"StartLba"),Property(progress,"LastWrittenLba"),Property(progress,"SectorCount"));
            if (line != null) Console.WriteLine(line);
        } catch (Exception) {}
    }
    public static Action<object,object> Attach(object format) {
        Action<object,object> handler = Update;
        ComEventsHelper.Combine(format,Events,0x200,handler);
        return handler;
    }
    public static void Detach(object format, Action<object,object> handler) {
        if (handler != null) ComEventsHelper.Remove(format,Events,0x200,handler);
    }
}
'@
if ($TelemetrySelfTest) {
    if ([DasBurnTelemetry]::FormatProgress(4,100,125,100) -cne 'DAS_PROGRESS 25 100') { throw 'Telemetry conversion failed' }
    if ($null -ne [DasBurnTelemetry]::FormatProgress(4,100,99,100)) { throw 'Invalid LBA accepted' }
    if ($null -ne [DasBurnTelemetry]::FormatProgress(5,100,125,100)) { throw 'Non-writing event accepted' }
    if ([string]::IsNullOrWhiteSpace($env:DAS_STREAM_TEST_FIXTURE)) { throw 'Fixture path required for native self-test' }
    [DasBurnStream]::CheckFixture($env:DAS_STREAM_TEST_FIXTURE)
    Write-Output 'TELEMETRY_SELFTEST_PASSED'
    return
}
$rec = $null
$format = $null
$stream = $null
$locked = $false
$guard = $null
$telemetry = $null
try {
    $guard = [System.IO.File]::Open($image,[System.IO.FileMode]::Open,[System.IO.FileAccess]::Read,[System.IO.FileShare]::Read)
    if ($guard.Length -ne $bytes) { throw 'Image size changed' }
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try { $hash = [BitConverter]::ToString($sha.ComputeHash($guard)).Replace('-','') }
    finally { $sha.Dispose() }
    if ($hash -ne $env:DAS_BURN_HASH) { throw 'Image checksum changed' }
    [Console]::WriteLine('DAS_STAGE CHECKING_MEDIA')
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
    if ($kind -ne [int]$env:DAS_BURN_MEDIA -or $kind -notin @(2,6,8,9,11,18)) { throw 'Media type changed or is not supported write-once media' }
    if (-not $format.MediaPhysicallyBlank -or -not $format.MediaHeuristicallyBlank) { throw 'Medium is not blank' }
    if ([int]$format.CurrentMediaStatus -ne 2) { throw 'Unsafe media status' }
    $free = [long]$format.FreeSectorsOnMedia
    if ($free -le 0 -or $free -gt [long]$format.TotalSectorsOnMedia -or ($bytes / 2048) -gt $free) { throw 'Insufficient writable capacity' }
    $format.ForceMediaToBeClosed = $true
    $stream = [DasBurnStream]::Open($image)
    [Console]::WriteLine('DAS_STAGE WRITING')
    try { $telemetry = [DasBurnTelemetry]::Attach($format) } catch { [Console]::Error.WriteLine('Detailed progress unavailable; continuing with stage reporting') }
    $format.Write($stream)
    [Console]::WriteLine('DAS_STAGE RELEASING')
} finally {
    if ($null -ne $telemetry) { try { [DasBurnTelemetry]::Detach($format,$telemetry) } catch {} }
    if ($null -ne $stream) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($stream) }
    if ($null -ne $format) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($format) }
    try { if ($locked) { $rec.ReleaseExclusiveAccess() } }
    finally {
        if ($null -ne $rec) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($rec) }
        if ($null -ne $guard) { $guard.Dispose() }
    }
}
Write-Output 'WRITE_COMPLETED'
