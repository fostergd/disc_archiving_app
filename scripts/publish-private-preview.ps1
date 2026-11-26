param([Parameter(Mandatory=$true)][string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Invoke-Gh {
    param([Parameter(Mandatory=$true)][string[]]$Arguments)
    $result = & gh @Arguments
    if ($LASTEXITCODE -ne 0) { throw "GitHub CLI failed: $($Arguments[0]) $($Arguments[1]) (exit $LASTEXITCODE)" }
    return $result
}
if ($env:GITHUB_EVENT_NAME -cne 'workflow_dispatch' -or $env:GITHUB_REF -cne 'refs/heads/main') { throw 'Private release requires a manual main-branch workflow' }
$repo = $env:GITHUB_REPOSITORY
$sha = $env:GITHUB_SHA
if ($repo -cne 'fostergd/disc_archiving_app' -or $sha -notmatch '^[0-9a-f]{40}$') { throw 'Unexpected repository or commit identity' }
if ($env:GITHUB_RUN_ID -notmatch '^[0-9]+$' -or $env:GITHUB_RUN_ATTEMPT -notmatch '^[0-9]+$') { throw 'Unexpected workflow identity' }
$info = (Invoke-Gh @('api',"repos/$repo")) | ConvertFrom-Json
if ($info.private -ne $true -or $info.visibility -cne 'private') { throw 'Refusing to publish outside a private repository' }
$out = [IO.Path]::GetFullPath($OutputDirectory)
$zip = Join-Path $out 'disc-archive-windows-x64-preview.zip'
$checksum = Join-Path $out 'disc-archive-windows-x64-preview.zip.sha256'
$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
if ((Get-Content -LiteralPath $checksum -Raw).Trim() -cne "$hash  disc-archive-windows-x64-preview.zip") { throw 'ZIP checksum mismatch' }
# Unique create-only tag: never overwrite a release or its assets.
$tag = "preview-$($sha.Substring(0,12))-$($env:GITHUB_RUN_ID)-$($env:GITHUB_RUN_ATTEMPT)"
$tagResult = (Invoke-Gh @('api','--method','POST',"repos/$repo/git/refs",'-f',"ref=refs/tags/$tag",'-f',"sha=$sha")) | ConvertFrom-Json
if ($tagResult.ref -cne "refs/tags/$tag" -or $tagResult.object.sha -cne $sha) { throw 'Created tag does not match tested commit' }
$notes = "Unsigned Windows x64 portable preview. Tested commit: $sha. GUI and CLI included. Physical optical hardware acceptance is pending; use disposable fixtures and expendable media. Verify the ZIP checksum and SHA256SUMS.txt before use. No installer."
Invoke-Gh @('release','create',$tag,$zip,$checksum,'--repo',$repo,'--verify-tag','--draft','--prerelease','--latest=false','--title',"Disc Archive Studio Windows x64 preview $($sha.Substring(0,12))",'--notes',$notes)
# Verify the draft contains exactly the two uploaded files with their expected
# byte lengths and server-side SHA-256 digests before making it visible.
$view = (Invoke-Gh @('release','view',$tag,'--repo',$repo,'--json','databaseId,isDraft,tagName')) | ConvertFrom-Json
if (-not $view.isDraft -or $view.tagName -cne $tag) { throw 'Expected newly created draft release' }
$release = (Invoke-Gh @('api',"repos/$repo/releases/$($view.databaseId)")) | ConvertFrom-Json
if (@($release.assets).Count -ne 2 -or -not $release.draft -or -not $release.prerelease) { throw 'Unexpected draft release state' }
foreach ($path in @($zip,$checksum)) {
    $item = Get-Item -LiteralPath $path
    $match = @($release.assets | Where-Object { $_.name -ceq $item.Name })
    $digest = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($match.Count -ne 1 -or $match[0].state -cne 'uploaded' -or $match[0].size -ne $item.Length -or $match[0].digest -cne "sha256:$digest") { throw "Release asset validation failed: $($item.Name)" }
}
# Recheck privacy just before publication. Fail closed without deleting anything.
$info = (Invoke-Gh @('api',"repos/$repo")) | ConvertFrom-Json
if ($info.private -ne $true -or $info.visibility -cne 'private') { throw 'Repository privacy changed' }
Invoke-Gh @('release','edit',$tag,'--repo',$repo,'--draft=false','--prerelease','--latest=false')
$published = (Invoke-Gh @('api',"repos/$repo/releases/$($view.databaseId)")) | ConvertFrom-Json
if ($published.draft -or -not $published.prerelease -or $published.tag_name -cne $tag -or @($published.assets).Count -ne 2) { throw 'Published release verification failed' }
Write-Output "Private preview: $($published.html_url)"
foreach ($asset in $published.assets) { Write-Output "$($asset.name): $($asset.browser_download_url)" }
