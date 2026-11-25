$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Use the system IMAPI filesystem builder; no recorder is opened.
Add-Type -TypeDefinition @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;
public static class DasStream {
    public static void Write(object source, string destination, long expected, long limit) {
        if (expected <= 0 || expected > limit) throw new IOException("Image size exceeds limit");
        IStream stream = (IStream)source;
        byte[] buffer = new byte[1024 * 1024];
        IntPtr count = Marshal.AllocHGlobal(4);
        try {
            using (FileStream output = new FileStream(destination, FileMode.CreateNew, FileAccess.Write, FileShare.None)) {
                long total = 0;
                while (total < expected) {
                    int request = (int)Math.Min(buffer.Length, expected-total);
                    Marshal.WriteInt32(count, 0);
                    stream.Read(buffer, request, count);
                    int read = Marshal.ReadInt32(count);
                    if (read <= 0 || read > request) throw new IOException("Unexpected image stream length");
                    output.Write(buffer, 0, read);
                    total += read;
                }
                Marshal.WriteInt32(count, 0);
                stream.Read(buffer, 1, count);
                if (Marshal.ReadInt32(count) != 0) throw new IOException("Image stream exceeds declared length");
                output.Flush(true);
            }
        } finally { Marshal.FreeHGlobal(count); }
    }
}
'@
$fsi = $null
$result = $null
$root = $null
$stream = $null
try {
    $blocks = [long]::Parse($env:DAS_MAX_BLOCKS, [Globalization.CultureInfo]::InvariantCulture)
    if ($blocks -le 0 -or $blocks -gt [int]::MaxValue) { throw 'Unsupported block count' }
    $fsi = New-Object -ComObject IMAPI2FS.MsftFileSystemImage
    $fsi.FileSystemsToCreate = 4
    $fsi.UDFRevision = 0x0201
    $fsi.VolumeName = $env:DAS_LABEL
    $fsi.FreeMediaBlocks = [int]$blocks
    $root = $fsi.Root
    $root.AddTree($env:DAS_STAGE, $false)
    $result = $fsi.CreateResultImage()
    if ($result.BlockSize -ne 2048) { throw 'Unexpected IMAPI block size' }
    $expected = [long]$result.TotalBlocks * [long]$result.BlockSize
    $stream = $result.ImageStream
    [DasStream]::Write($stream, $env:DAS_IMAGE, $expected, ($blocks * 2048))
} finally {
    foreach ($obj in @($stream,$result,$root,$fsi)) {
        if ($null -ne $obj -and [Runtime.InteropServices.Marshal]::IsComObject($obj)) {
            [void][Runtime.InteropServices.Marshal]::ReleaseComObject($obj)
        }
    }
}
