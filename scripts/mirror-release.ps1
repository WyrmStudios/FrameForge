# Mirrors a GitHub release (installer + update bundle + latest.json) to Codeberg and GitLab.
# Called by build-and-release.bat after the GitHub release is published. Best-effort: a failure
# on one platform does not stop the other, and this whole step is skippable (missing token file
# just skips that platform) so a release can always still go out via GitHub alone.
#
# Tokens read from plain-text files (one line, no quotes), same convention as the Tauri signing
# key at %USERPROFILE%\.tauri\frameforge.key:
#   %USERPROFILE%\.codeberg\frameforge.token   (Codeberg API token, scope: repo)
#   %USERPROFILE%\.gitlab\frameforge.token     (GitLab personal access token, scope: api)

param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$ExePath,
    [string]$ZipPath = "",
    [string]$Signature = "",
    [string]$PubDate = "",
    [string]$CodebergToken = "",
    [string]$GitLabToken = "",
    [switch]$Beta
)

# Beta builds get their own floating pointer (latest-beta) so stable users'
# updater never resolves against them - see src-tauri/src/updater.rs.
$FloatingTag = if ($Beta) { "latest-beta" } else { "latest" }

$CodebergOwner    = "Sikewyrm"
$CodebergRepo     = "FrameForge"
$GitLabProject    = "WyrmStudios/FrameForge"
$GitLabProjectEnc = [uri]::EscapeDataString($GitLabProject)
$PackageName      = "frameforge"

$ExeName = Split-Path $ExePath -Leaf
$ZipName = if ($ZipPath) { Split-Path $ZipPath -Leaf } else { "" }
$HasUpdateBundle = ($ZipPath -ne "") -and ($Signature -ne "")

function Read-TokenFile($path) {
    if (Test-Path $path) {
        $t = (Get-Content $path -Raw).Trim()
        if ($t -ne "") { return $t }
    }
    return $null
}

if (-not $CodebergToken) { $CodebergToken = Read-TokenFile "$env:USERPROFILE\.codeberg\frameforge.token" }
if (-not $GitLabToken)   { $GitLabToken   = Read-TokenFile "$env:USERPROFILE\.gitlab\frameforge.token" }

function Write-JsonTempFile($obj) {
    $tmp = [System.IO.Path]::GetTempFileName()
    $json = $obj | ConvertTo-Json -Depth 5
    # Windows PowerShell 5.1's -Encoding utf8 always adds a BOM, which breaks Gitea/GitLab's JSON parsers.
    [System.IO.File]::WriteAllText($tmp, $json, (New-Object System.Text.UTF8Encoding $false))
    return $tmp
}

# ---------------------------------------------------------------- Codeberg
if (-not $CodebergToken) {
    Write-Host "No Codeberg token at $env:USERPROFILE\.codeberg\frameforge.token - skipping Codeberg mirror." -ForegroundColor Yellow
} else {
    Write-Host "Mirroring $Tag to Codeberg..."
    $cbApi = "https://codeberg.org/api/v1/repos/$CodebergOwner/$CodebergRepo"
    $cbAuth = "Authorization: token $CodebergToken"

    function New-CodebergRelease([string]$tagName, [string]$name, [string]$body) {
        $existingJson = & curl.exe -s -H $cbAuth "$cbApi/releases/tags/$tagName"
        $existingId = $null
        try { $existingId = ($existingJson | ConvertFrom-Json).id } catch {}
        if ($existingId) {
            & curl.exe -s -X DELETE -H $cbAuth "$cbApi/releases/$existingId" | Out-Null
        }
        $payloadFile = Write-JsonTempFile @{ tag_name = $tagName; target_commitish = "main"; name = $name; body = $body; draft = $false; prerelease = [bool]$Beta }
        $resp = & curl.exe -s -X POST -H $cbAuth -H "Content-Type: application/json" --data-binary "@$payloadFile" "$cbApi/releases"
        Remove-Item $payloadFile -Force
        return ($resp | ConvertFrom-Json)
    }

    function Add-CodebergAsset([int]$releaseId, [string]$filePath, [string]$fileName) {
        & curl.exe -s -X POST -H $cbAuth -F "attachment=@$filePath;filename=$fileName" "$cbApi/releases/$releaseId/assets?name=$fileName" | Out-Null
    }

    $release = New-CodebergRelease -tagName $Tag -name "FrameForge $Tag" -body "FrameForge $Tag - Windows installer"
    if ($release.id) {
        Add-CodebergAsset -releaseId $release.id -filePath $ExePath -fileName $ExeName

        if ($HasUpdateBundle) {
            Add-CodebergAsset -releaseId $release.id -filePath $ZipPath -fileName $ZipName

            $latestJsonFile = Write-JsonTempFile @{
                version   = $Version
                pub_date  = $PubDate
                platforms = @{
                    "windows-x86_64" = @{
                        url       = "https://codeberg.org/$CodebergOwner/$CodebergRepo/releases/download/$Tag/$ZipName"
                        signature = $Signature
                    }
                }
            }
            Add-CodebergAsset -releaseId $release.id -filePath $latestJsonFile -fileName "latest.json"

            # Floating release: the stable URL the in-app updater actually polls
            # ("latest" for full releases, "latest-beta" for beta/dev builds).
            $floatingBody = if ($Beta) { "Floating pointer to the newest beta/dev build - see the versioned release above for this version's notes." } else { "Floating pointer to the newest full release - see the versioned release above for this version's notes." }
            $floating = New-CodebergRelease -tagName $FloatingTag -name $FloatingTag -body $floatingBody
            if ($floating.id) {
                Add-CodebergAsset -releaseId $floating.id -filePath $latestJsonFile -fileName "latest.json"
            }
            Remove-Item $latestJsonFile -Force
        }
        Write-Host "Codeberg mirror done." -ForegroundColor Green
    } else {
        Write-Host "Codeberg release creation failed - skipping asset upload." -ForegroundColor Red
    }
}

# ---------------------------------------------------------------- GitLab
if (-not $GitLabToken) {
    Write-Host "No GitLab token at $env:USERPROFILE\.gitlab\frameforge.token - skipping GitLab mirror." -ForegroundColor Yellow
} else {
    Write-Host "Mirroring $Tag to GitLab..."
    $glAuth = "PRIVATE-TOKEN: $GitLabToken"
    $glPkgBase = "https://gitlab.com/api/v4/projects/$GitLabProjectEnc/packages/generic/$PackageName"
    $glApi = "https://gitlab.com/api/v4/projects/$GitLabProjectEnc"

    function Send-GitLabPackageFile([string]$version, [string]$filePath, [string]$fileName) {
        & curl.exe -s --upload-file $filePath -H $glAuth "$glPkgBase/$version/$fileName" | Out-Null
    }

    Send-GitLabPackageFile -version $Tag -filePath $ExePath -fileName $ExeName

    if ($HasUpdateBundle) {
        Send-GitLabPackageFile -version $Tag -filePath $ZipPath -fileName $ZipName

        $latestJsonFile = Write-JsonTempFile @{
            version   = $Version
            pub_date  = $PubDate
            platforms = @{
                "windows-x86_64" = @{
                    url       = "$glPkgBase/$Tag/$ZipName"
                    signature = $Signature
                }
            }
        }
        Send-GitLabPackageFile -version $Tag -filePath $latestJsonFile -fileName "latest.json"
        # Floating package version: the stable URL the in-app updater actually polls
        # ("latest" for full releases, "latest-beta" for beta/dev builds).
        Send-GitLabPackageFile -version $FloatingTag -filePath $latestJsonFile -fileName "latest.json"
        Remove-Item $latestJsonFile -Force

        # Best-effort human-browsable Release object (not required for the updater itself).
        $relPayloadFile = Write-JsonTempFile @{ tag_name = $Tag; name = "FrameForge $Tag"; description = "FrameForge $Tag - Windows installer"; ref = "main" }
        & curl.exe -s -X POST -H $glAuth -H "Content-Type: application/json" --data-binary "@$relPayloadFile" "$glApi/releases" | Out-Null
        Remove-Item $relPayloadFile -Force

        foreach ($asset in @(
            @{ name = $ExeName; url = "$glPkgBase/$Tag/$ExeName" },
            @{ name = $ZipName; url = "$glPkgBase/$Tag/$ZipName" },
            @{ name = "latest.json"; url = "$glPkgBase/$Tag/latest.json" }
        )) {
            $linkPayloadFile = Write-JsonTempFile $asset
            & curl.exe -s -X POST -H $glAuth -H "Content-Type: application/json" --data-binary "@$linkPayloadFile" "$glApi/releases/$Tag/assets/links" | Out-Null
            Remove-Item $linkPayloadFile -Force
        }
    }
    Write-Host "GitLab mirror done." -ForegroundColor Green
}

Write-Host "Mirroring complete."
