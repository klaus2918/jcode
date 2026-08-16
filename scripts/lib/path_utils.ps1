<#
.SYNOPSIS
    Shared PATH helpers for the jcode Windows PowerShell scripts.
.DESCRIPTION
    Extracted from install.ps1, uninstall.ps1 and update_local_install.ps1 so
    every jcode script manages exactly the same set of user PATH keys and a
    mixed use of the three scripts cannot leave a stale second jcode entry on
    PATH. PATH is treated as a list of independent entries: read the persisted
    value, split it, drop stale jcode-managed entries, prepend the launcher
    dir, write it back. Never rebuild PATH from a hard-coded string.

    Usage from a sibling script (scripts\...):
        . (Join-Path $PSScriptRoot 'lib\path_utils.ps1')

    Kept in sync with the inline fallback copies inside install.ps1 and
    uninstall.ps1, which must stay self-contained because they are also run via
    `irm <url> | iex` with no local files.
#>

function Get-JcodeLocalAppDataDir {
    if ($env:LOCALAPPDATA) {
        return $env:LOCALAPPDATA
    }

    $localAppData = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    if ($localAppData) {
        return $localAppData
    }

    if ($env:USERPROFILE) {
        return (Join-Path $env:USERPROFILE "AppData\Local")
    }

    return (Join-Path ([Environment]::GetFolderPath("UserProfile")) "AppData\Local")
}

function Get-DefaultJcodeInstallDir {
    return (Join-Path (Get-JcodeLocalAppDataDir) "jcode\bin")
}

function ConvertTo-JcodePathKey([string]$PathValue) {
    if (-not $PathValue) {
        return ""
    }

    $clean = [Environment]::ExpandEnvironmentVariables($PathValue.Trim().Trim('"'))
    if (-not $clean) {
        return ""
    }

    try {
        $clean = [System.IO.Path]::GetFullPath($clean)
    } catch {
    }

    $clean = $clean.TrimEnd([System.IO.Path]::DirectorySeparatorChar, [System.IO.Path]::AltDirectorySeparatorChar)
    return $clean.ToUpperInvariant()
}

function Split-JcodePathList([string]$PathValue) {
    if (-not $PathValue) {
        return @()
    }

    $entries = @()
    foreach ($entry in ($PathValue -split ';')) {
        $clean = $entry.Trim().Trim('"')
        if ($clean) {
            $entries += $clean
        }
    }
    return $entries
}

function Join-JcodePathList([string[]]$Entries) {
    if (-not $Entries -or $Entries.Count -eq 0) {
        return ""
    }

    return ($Entries -join ';')
}

function Get-JcodeManagedPathKeys {
    param(
        [string]$InstallDir,
        [string]$JcodeHome
    )

    # Union of every key the three scripts ever managed, so install.ps1,
    # uninstall.ps1 and update_local_install.ps1 converge on one set:
    #   - the launcher dir passed to install/uninstall and its default
    #     (%LOCALAPPDATA%\jcode\bin)
    #   - the legacy install layout root %LOCALAPPDATA%\jcode
    #   - the single-exe JCODE_HOME layout: bin and builds\current-release-lto,
    #     from the explicit parameter and/or the JCODE_HOME env var
    $keys = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase)
    $candidates = @()

    foreach ($candidate in @($InstallDir, (Get-DefaultJcodeInstallDir))) {
        if ($candidate) { $candidates += $candidate }
    }

    $localAppData = Get-JcodeLocalAppDataDir
    if ($localAppData) {
        $candidates += (Join-Path $localAppData 'jcode')
        $candidates += (Join-Path $localAppData 'jcode\bin')
    }

    foreach ($jcodeHomeCandidate in @($JcodeHome, $env:JCODE_HOME)) {
        if (-not $jcodeHomeCandidate) { continue }
        $candidates += (Join-Path $jcodeHomeCandidate 'bin')
        $candidates += (Join-Path $jcodeHomeCandidate 'builds\current-release-lto')
    }

    foreach ($candidate in $candidates) {
        $key = ConvertTo-JcodePathKey $candidate
        if ($key) {
            [void]$keys.Add($key)
        }
    }
    return $keys
}

function Send-JcodeEnvironmentChangedBroadcast {
    if ($env:JCODE_DISABLE_ENV_BROADCAST -eq "1") {
        return $false
    }

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
    [Jcode.EnvironmentBroadcast]::SendMessageTimeout([IntPtr]0xffff, 0x001A, [UIntPtr]::Zero, "Environment", 0x0002, 5000, [ref]$result) | Out-Null
    return $true
}