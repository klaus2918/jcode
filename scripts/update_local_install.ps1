<#
.SYNOPSIS
    Deploy a locally built jcode.exe into the single-exe JCODE_HOME layout and
    keep the user PATH up to date -- appending, never overwriting.

.DESCRIPTION
    This is the update mechanism for the "single exe" JCODE_HOME layout:

        JCODE_HOME\bin\jcode.exe                        (PATH entry)
        JCODE_HOME\builds\current-release-lto\jcode.exe (build slot, same file)

    The script is idempotent and safe to run repeatedly. It:

      1. Resolves the source exe (default: target\release-lto\jcode.exe).
      2. Persists JCODE_HOME as a user environment variable (registry), only
         when the current value differs.
      3. Deploys the same file to both locations.
      4. Verifies SHA256 consistency across source + both deployed copies.
      5. Updates the user PATH by READING the existing value first, dropping
         stale jcode entries, and PREPENDING the launcher dir. It never
         overwrites PATH with a fixed string, so unrelated entries (npm global
         bin, Python, etc.) are always preserved.
      6. Broadcasts WM_SETTINGCHANGE so open apps pick up the new PATH.
      7. Prints a restart hint.

.PARAMETER ExePath
    Path to the jcode.exe to deploy. Defaults to target\release-lto\jcode.exe
    relative to the repository root. Use this for scenario B (new machine with
    a prebuilt exe):  .\scripts\update_local_install.ps1 -ExePath <path>

.PARAMETER JcodeHome
    Override the JCODE_HOME directory. Default: $env:JCODE_HOME if set,
    otherwise ~\.jcode.

.PARAMETER SkipEnvBroadcast
    Skip the WM_SETTINGCHANGE broadcast (used by tests / CI).

.EXAMPLE
    .\scripts\update_local_install.ps1

.EXAMPLE
    .\scripts\update_local_install.ps1 -ExePath D:\dist\jcode.exe
#>
param(
    [string]$ExePath,
    [string]$JcodeHome,
    [switch]$SkipEnvBroadcast
)

$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -lt 5) {
    Write-Host "error: PowerShell 5.1 or later is required" -ForegroundColor Red
    exit 1
}

function Write-Info($msg) { Write-Host $msg -ForegroundColor Blue }
function Write-Err($msg) { throw "error: $msg" }
function Write-Warn($msg) { Write-Host "warning: $msg" -ForegroundColor Yellow }

function Get-JcodeRepoRoot {
    return Split-Path -Parent $PSScriptRoot
}

function Resolve-JcodeHome {
    param([string]$Override)
    if ($Override) { return $Override }
    if ($env:JCODE_HOME) { return $env:JCODE_HOME }
    if ($env:USERPROFILE) { return (Join-Path $env:USERPROFILE '.jcode') }
    return (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.jcode')
}

function Resolve-JcodeSourceExe {
    param([string]$Override)
    if ($Override) { return (Resolve-Path -LiteralPath $Override -ErrorAction Stop).Path }
    $repoRoot = Get-JcodeRepoRoot
    $candidate = Join-Path $repoRoot 'target\release-lto\jcode.exe'
    if (-not (Test-Path -LiteralPath $candidate)) {
        Write-Err "Default source exe not found at $candidate. Build first (cargo build --profile release-lto -p jcode --bin jcode) or pass -ExePath."
    }
    return (Resolve-Path -LiteralPath $candidate).Path
}

# --- PATH helpers (mirror scripts/install.ps1; kept self-contained) ---------
# PATH is treated as a list of independent entries: we read the persisted user
# value, split it, drop stale jcode-managed entries, and prepend the launcher
# dir. We never rebuild PATH from a hard-coded string.

function ConvertTo-JcodePathKey([string]$PathValue) {
    if (-not $PathValue) { return '' }
    $clean = [Environment]::ExpandEnvironmentVariables($PathValue.Trim().Trim('"'))
    if (-not $clean) { return '' }
    try {
        $clean = [System.IO.Path]::GetFullPath($clean)
    } catch {
    }
    $clean = $clean.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    return $clean.ToUpperInvariant()
}

function Split-JcodePathList([string]$PathValue) {
    if (-not $PathValue) { return @() }
    $entries = @()
    foreach ($entry in ($PathValue -split ';')) {
        $clean = $entry.Trim().Trim('"')
        if ($clean) { $entries += $clean }
    }
    return $entries
}

function Join-JcodePathList([string[]]$Entries) {
    if (-not $Entries -or $Entries.Count -eq 0) { return '' }
    return ($Entries -join ';')
}

function Get-JcodeManagedPathKeys([string]$JcodeHome) {
    $keys = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase)
    $candidates = @(
        (Join-Path $JcodeHome 'bin'),
        (Join-Path $JcodeHome 'builds\current-release-lto')
    )
    $localAppData = if ($env:LOCALAPPDATA) {
        $env:LOCALAPPDATA
    } else {
        [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    }
    if ($localAppData) {
        $candidates += (Join-Path $localAppData 'jcode\bin')
        $candidates += (Join-Path $localAppData 'jcode')
    }
    foreach ($candidate in $candidates) {
        $key = ConvertTo-JcodePathKey $candidate
        if ($key) { [void]$keys.Add($key) }
    }
    return $keys
}

function Resolve-JcodePathUpdate {
    param(
        [Parameter(Mandatory = $true)][string]$LauncherDir,
        [AllowNull()][string]$CurrentPath,
        [string]$JcodeHome
    )

    $managedKeys = Get-JcodeManagedPathKeys -JcodeHome $JcodeHome
    $nextEntries = @()
    $removedManaged = 0

    foreach ($entry in (Split-JcodePathList $CurrentPath)) {
        $key = ConvertTo-JcodePathKey $entry
        if (-not $key) { continue }
        if ($managedKeys.Contains($key)) {
            $removedManaged += 1
            continue
        }
        $nextEntries += $entry
    }

    $nextEntries = @($LauncherDir) + $nextEntries
    $nextPath = Join-JcodePathList $nextEntries
    $changed = ($nextPath -ne ([string]$CurrentPath))

    return [pscustomobject]@{
        Path = $nextPath
        Changed = $changed
        RemovedManagedEntries = $removedManaged
        AddedLauncherEntry = $true
        LauncherDir = $LauncherDir
    }
}

function Send-JcodeEnvironmentChangedBroadcast {
    if (-not ("Jcode.EnvironmentBroadcast" -as [type])) {
        Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
namespace Jcode {
    public static class EnvironmentBroadcast {
        [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
        public static extern IntPtr SendMessageTimeout(
            IntPtr hWnd,
            UInt32 Msg,
            UIntPtr wParam,
            string lParam,
            UInt32 fuFlags,
            UInt32 uTimeout,
            out UIntPtr lpdwResult);
    }
}
"@
    }

    $result = [UIntPtr]::Zero
    [Jcode.EnvironmentBroadcast]::SendMessageTimeout([IntPtr]0xffff, 0x001A, [UIntPtr]::Zero, 'Environment', 0x0002, 5000, [ref]$result) | Out-Null
    return $true
}

# --- Main --------------------------------------------------------------------

function Invoke-JcodeLocalUpdate {
    param(
        [string]$ExePath,
        [string]$JcodeHome,
        [bool]$SkipEnvBroadcast
    )

    $jcodeHome = Resolve-JcodeHome -Override $JcodeHome
    $source = Resolve-JcodeSourceExe -Override $ExePath

    $binDir = Join-Path $jcodeHome 'bin'
    $buildSlotDir = Join-Path $jcodeHome 'builds\current-release-lto'
    $launcherPath = Join-Path $binDir 'jcode.exe'
    $buildSlotPath = Join-Path $buildSlotDir 'jcode.exe'

    # 1. Validate the source binary exists and looks like jcode.
    $sourceHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Info "Source: $source"

    # 2. Persist JCODE_HOME (user-level), only when it differs.
    $currentJcodeHome = [Environment]::GetEnvironmentVariable('JCODE_HOME', 'User')
    if ($currentJcodeHome -ne $jcodeHome) {
        [Environment]::SetEnvironmentVariable('JCODE_HOME', $jcodeHome, 'User')
        Write-Info "JCODE_HOME persisted (User): $jcodeHome"
    } else {
        Write-Info "JCODE_HOME already correct (User): $jcodeHome"
    }

    # 3. Deploy the same file to both locations (idempotent overwrite).
    New-Item -ItemType Directory -Path $binDir -Force | Out-Null
    New-Item -ItemType Directory -Path $buildSlotDir -Force | Out-Null
    Copy-Item -LiteralPath $source -Destination $launcherPath -Force
    Copy-Item -LiteralPath $source -Destination $buildSlotPath -Force

    # 4. SHA256 consistency across all three copies.
    $launcherHash = (Get-FileHash -LiteralPath $launcherPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $buildSlotHash = (Get-FileHash -LiteralPath $buildSlotPath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($launcherHash -ne $sourceHash -or $buildSlotHash -ne $sourceHash) {
        Write-Err "SHA256 mismatch after deploy: source=$sourceHash launcher=$launcherHash buildSlot=$buildSlotHash"
    }
    Write-Info "SHA256 consistent (3 copies): $sourceHash"

    # 5. Update the user PATH by reading the existing value and prepending.
    $currentUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $update = Resolve-JcodePathUpdate -LauncherDir $binDir -CurrentPath $currentUserPath -JcodeHome $jcodeHome

    if ($update.Changed) {
        [Environment]::SetEnvironmentVariable('Path', $update.Path, 'User')
        Write-Info "User PATH updated: prepended $binDir (removed $($update.RemovedManagedEntries) stale jcode entr$(if ($update.RemovedManagedEntries -eq 1) { 'y' } else { 'ies' }))"
        if (-not $SkipEnvBroadcast) {
            Send-JcodeEnvironmentChangedBroadcast | Out-Null
            Write-Info "Environment change broadcast sent"
        }
    } else {
        Write-Info "User PATH already correct: $binDir present, no stale entries"
    }

    # 6. Surface the final PATH for a quick sanity check.
    $finalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $npmHint = if ($finalUserPath -match '(?i)AppData\\Roaming\\npm') { 'npm global bin preserved' } else { 'note: no npm global bin entry found in user PATH' }
    Write-Info "Final user PATH: $finalUserPath"
    Write-Host "  -> $npmHint" -ForegroundColor Yellow

    return [pscustomobject]@{
        JcodeHome = $jcodeHome
        Source = $source
        LauncherPath = $launcherPath
        BuildSlotPath = $buildSlotPath
        Sha256 = $sourceHash
        PathUpdate = $update
    }
}

if ($env:JCODE_UPDATE_PS1_IMPORT_ONLY -ne '1') {
    $result = Invoke-JcodeLocalUpdate -ExePath $ExePath -JcodeHome $JcodeHome -SkipEnvBroadcast:$SkipEnvBroadcast
    Write-Host ""
    Write-Info "Deploy complete. Restart jcode (and open a new terminal) for the new exe to take effect."
    Write-Host "  which jcode -> $($result.LauncherPath)" -ForegroundColor Green
}
