param(
    [Parameter(Mandatory = $true)][string]$ArtifactExePath,
    [string]$Version
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$resolvedArtifact = (Resolve-Path -LiteralPath $ArtifactExePath).Path
$originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$originalEnvironment = @{
    LOCALAPPDATA = $env:LOCALAPPDATA
    APPDATA = $env:APPDATA
    USERPROFILE = $env:USERPROFILE
    JCODE_HOME = $env:JCODE_HOME
    JCODE_WINDOWS_SETUP_SKIP_PROCESS_LIFECYCLE = $env:JCODE_WINDOWS_SETUP_SKIP_PROCESS_LIFECYCLE
}

if (-not $Version) {
    $artifactVersionOutput = & $resolvedArtifact --version
    if ($LASTEXITCODE -ne 0) {
        throw "Local Windows artifact failed to run --version"
    }

    $artifactVersionText = ($artifactVersionOutput -join "`n")
    if ($artifactVersionText -notmatch '(?i)\bjcode\s+v?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?)\b') {
        throw "Could not parse jcode version from local artifact output: $artifactVersionText"
    }
    $Version = 'v' + $Matches[1]
} else {
    $Version = 'v' + (($Version.Trim()) -replace '^[vV]', '')
}

$tempBase = if ($env:RUNNER_TEMP) { $env:RUNNER_TEMP } else { $env:TEMP }
$tempRoot = Join-Path $tempBase ("jcode-windows-install-verify-" + [guid]::NewGuid().ToString('N'))
$localAppData = Join-Path $tempRoot 'localappdata'
$appData = Join-Path $tempRoot 'appdata'
$userProfile = Join-Path $tempRoot 'userprofile'
$jcodeHome = Join-Path $tempRoot '.jcode'
$installDir = Join-Path $localAppData 'jcode\bin'

try {
New-Item -ItemType Directory -Force -Path $localAppData, $appData, $userProfile, $jcodeHome | Out-Null

$env:LOCALAPPDATA = $localAppData
$env:APPDATA = $appData
$env:USERPROFILE = $userProfile
$env:JCODE_HOME = $jcodeHome
$env:JCODE_WINDOWS_SETUP_SKIP_PROCESS_LIFECYCLE = '1'

$installScript = Join-Path $repoRoot 'scripts\install.ps1'

& $installScript `
    -InstallDir $installDir `
    -Version $Version `
    -ArtifactExePath $resolvedArtifact

$launcherPath = Join-Path $installDir 'jcode.exe'
$versionDir = Join-Path $localAppData ('jcode\builds\versions\' + $Version.TrimStart('v') + '\jcode.exe')
$stablePath = Join-Path $localAppData 'jcode\builds\stable\jcode.exe'

foreach ($path in @($launcherPath, $versionDir, $stablePath)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Expected installed file missing: $path"
    }
}

$hotkeyDir = Join-Path $jcodeHome 'hotkey'
$startupShortcut = Join-Path $appData 'Microsoft\Windows\Start Menu\Programs\Startup\jcode-hotkey.lnk'
if (Test-Path -LiteralPath $hotkeyDir) {
    throw "Default install unexpectedly created optional hotkey files: $hotkeyDir"
}
if (Test-Path -LiteralPath $startupShortcut) {
    throw "Default install unexpectedly created an optional startup shortcut: $startupShortcut"
}

$versionOutput = & $launcherPath --version
if ($LASTEXITCODE -ne 0) {
    throw "Installed launcher failed to run --version"
}

if ($versionOutput -notmatch 'jcode') {
    throw "Installed launcher returned unexpected version output: $versionOutput"
}

& $installScript `
    -InstallDir $installDir `
    -Version $Version `
    -ArtifactExePath $resolvedArtifact

if (-not (Test-Path -LiteralPath $launcherPath)) {
    throw "Launcher missing after reinstall: $launcherPath"
}

Write-Host "Windows install verification passed for $Version" -ForegroundColor Green
} finally {
    # The installer intentionally persists PATH in HKCU. This verifier uses an
    # isolated filesystem profile, so always restore the caller's actual user
    # PATH and environment even if one of the lifecycle assertions fails.
    [Environment]::SetEnvironmentVariable('Path', $originalUserPath, 'User')
    foreach ($name in $originalEnvironment.Keys) {
        $value = $originalEnvironment[$name]
        if ($null -eq $value) {
            Remove-Item -Path "Env:$name" -ErrorAction SilentlyContinue
        } else {
            Set-Item -Path "Env:$name" -Value $value
        }
    }
    Remove-Item -LiteralPath $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
}
