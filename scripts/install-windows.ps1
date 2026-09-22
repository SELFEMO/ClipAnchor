# Downloads a named ClipAnchor package from the highest pre-release-v / release-v tag and installs it silently.
# The file name stays stable. The version after v is what decides which release is newest.
param(
    [Parameter(Mandatory = $true)]
    [string]$Destination,
    [string]$Package = 'ClipAnchor_Windows_x64.exe'
)

$ErrorActionPreference = 'Stop'

$Destination = $Destination.Trim().TrimEnd('\')
if ([string]::IsNullOrWhiteSpace($Destination)) {
    throw 'Destination is required.'
}
if ($Package -match '[\\/]' -or $Package.Contains('..')) {
    throw 'Package must be a file name from the GitHub release.'
}

function Get-NewestClipAnchorRelease {
    param($Releases)

    $best = $null
    $bestNumbers = $null
    foreach ($release in @($Releases)) {
        if ($release.draft) { continue }
        $tag = [string]$release.tag_name
        if ($tag -notmatch '^(?:pre-release|release)-v(.+)$') { continue }
        $numbers = @([regex]::Matches($Matches[1], '\d+') | ForEach-Object { [int]$_.Value })
        if ($numbers.Count -eq 0) { continue }
        if ($null -eq $best) {
            $best = $release
            $bestNumbers = $numbers
            continue
        }
        $count = [Math]::Max($numbers.Count, $bestNumbers.Count)
        $isNewer = $false
        for ($index = 0; $index -lt $count; $index++) {
            $left = if ($index -lt $numbers.Count) { $numbers[$index] } else { 0 }
            $right = if ($index -lt $bestNumbers.Count) { $bestNumbers[$index] } else { 0 }
            if ($left -gt $right) { $isNewer = $true; break }
            if ($left -lt $right) { break }
        }
        if ($isNewer) {
            $best = $release
            $bestNumbers = $numbers
        }
    }
    return $best
}

function Get-ClipAnchorPackageUrl {
    param([string]$Name)

    $fallback = "https://github.com/SELFEMO/ClipAnchor/releases/latest/download/$Name"
    try {
        $releases = Invoke-RestMethod -Uri 'https://api.github.com/repos/SELFEMO/ClipAnchor/releases' -Headers @{
            Accept = 'application/vnd.github+json'
            'User-Agent' = 'ClipAnchor-Install'
        }
        $release = Get-NewestClipAnchorRelease $releases
        if ($null -eq $release) { return $fallback }
        foreach ($asset in @($release.assets)) {
            if ([string]$asset.name -eq $Name -and $asset.browser_download_url) {
                return [string]$asset.browser_download_url
            }
        }
    } catch {
        Write-Warning "GitHub release lookup failed. Using the unified latest download name. $($_.Exception.Message)"
    }
    return $fallback
}

$packageUrl = Get-ClipAnchorPackageUrl $Package
$downloadPath = Join-Path ([System.IO.Path]::GetTempPath()) $Package
Write-Host "Downloading $packageUrl"
Invoke-WebRequest -Uri $packageUrl -OutFile $downloadPath -UseBasicParsing

$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = $downloadPath
$startInfo.UseShellExecute = $true
$startInfo.Arguments = "/S /NS /D=$Destination"
$process = [System.Diagnostics.Process]::Start($startInfo)
if ($null -eq $process) {
    throw 'The installer did not start.'
}
$process.WaitForExit()
if (@(0, 3010, 1641) -notcontains [int]$process.ExitCode) {
    throw "The installer failed with exit code $($process.ExitCode)."
}
Write-Host "ClipAnchor installed to $Destination"
