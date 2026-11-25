$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$rows = @()
$errors = @()
$master = $null
try {
    $master = New-Object -ComObject IMAPI2.MsftDiscMaster2
    for ($i = 0; $i -lt $master.Count; $i++) {
        $rec = $null
        $format = $null
        try {
            $id = [string]$master.Item($i)
            $rec = New-Object -ComObject IMAPI2.MsftDiscRecorder2
            $rec.InitializeDiscRecorder($id)
            $row = [ordered]@{
                id = $id
                vendor = [string]$rec.VendorId
                product = [string]$rec.ProductId
                revision = [string]$rec.ProductRevision
                paths = @($rec.VolumePathNames | ForEach-Object { [string]$_ })
                profiles = @($rec.SupportedProfiles | ForEach-Object { [int]$_ })
                media = $null
                inspection_error = $null
            }
            try {
                $format = New-Object -ComObject IMAPI2.MsftDiscFormat2Data
                if (-not $format.IsRecorderSupported($rec)) { throw 'Recorder does not support IMAPI data writing' }
                $format.Recorder = $rec
                $supported = [bool]$format.IsCurrentMediaSupported($rec)
                if (-not $supported) { throw 'No supported current medium' }
                $row.media = [ordered]@{
                    physical_type = [int]$format.CurrentPhysicalMediaType
                    supported = $supported
                    physically_blank = [bool]$format.MediaPhysicallyBlank
                    heuristically_blank = [bool]$format.MediaHeuristicallyBlank
                    status = [int]$format.CurrentMediaStatus
                    free_sectors = [long]$format.FreeSectorsOnMedia
                    total_sectors = [long]$format.TotalSectorsOnMedia
                }
            } catch { $row.inspection_error = $_.Exception.Message }
            $rows += $row
        } catch { $errors += $_.Exception.Message }
        finally {
            foreach ($obj in @($format,$rec)) {
                if ($null -ne $obj) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($obj) }
            }
        }
    }
} finally {
    if ($null -ne $master) { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($master) }
}
[ordered]@{recorders=@($rows);errors=@($errors)} | ConvertTo-Json -Depth 8 -Compress
