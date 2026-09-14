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

    Besides the PATH helpers this module also owns the launcher deployment
    (Install-JcodeLauncher / Remove-JcodeStaleLauncherBackups), which must
    survive being pointed at a currently running jcode.exe.
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

# --- Launcher deployment (shared with update_local_install.ps1) --------------
# A plain Copy-Item onto the launcher path fails with "the file is in use" when
# that jcode.exe is the currently running process: Windows refuses to overwrite
# a loaded image. It does allow the directory entry to be *renamed* while the
# process keeps running from its existing handle, so the deployment below stages
# the new binary next to the target, renames the live one aside, then moves the
# staged file into the stable PATH location (rolling back if that move fails).
#
# Measured 2026-09-14: update_local_install.ps1 used a bare Copy-Item and aborted
# on a re-run while jcode was alive (PID 110020 held .jcode\bin\jcode.exe), even
# though the first run had succeeded. install.ps1 already had the safe version;
# this module now carries it so both scripts behave the same.

function Remove-JcodeStaleLauncherBackups {
    param(
        [Parameter(Mandatory = $true)][string]$LauncherDir
    )

    Get-ChildItem -LiteralPath $LauncherDir -Filter '.jcode-launcher-old-*.exe' -File -Force -ErrorAction SilentlyContinue |
        Remove-Item -Force -ErrorAction SilentlyContinue
}

function Install-JcodeLauncher {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [Parameter(Mandatory = $true)][string]$LauncherPath
    )

    $launcherDir = Split-Path -Parent $LauncherPath
    New-Item -ItemType Directory -Path $launcherDir -Force | Out-Null

    $operationId = [guid]::NewGuid().ToString('N')
    $tempLauncher = Join-Path $launcherDir (".jcode-launcher-{0}.tmp.exe" -f $operationId)
    $oldLauncher = Join-Path $launcherDir (".jcode-launcher-old-{0}.exe" -f $operationId)
    $movedExistingLauncher = $false
    try {
        Copy-Item -Path $SourcePath -Destination $tempLauncher -Force
        if (Test-Path -LiteralPath $LauncherPath) {
            # Windows will not overwrite a loaded executable, but it does allow
            # the directory entry to be renamed while the process keeps running
            # from its existing file handle. Move the old launcher aside first,
            # then atomically put the new binary at the stable PATH location.
            Move-Item -LiteralPath $LauncherPath -Destination $oldLauncher
            $movedExistingLauncher = $true
        }

        try {
            Move-Item -LiteralPath $tempLauncher -Destination $LauncherPath
        } catch {
            if ($movedExistingLauncher -and -not (Test-Path -LiteralPath $LauncherPath)) {
                Move-Item -LiteralPath $oldLauncher -Destination $LauncherPath
                $movedExistingLauncher = $false
            }
            throw
        }

        if ($movedExistingLauncher) {
            # Removal succeeds immediately for an idle launcher. If an older
            # jcode process still has the renamed executable loaded, Windows
            # keeps it until that process exits and the next install cleans it.
            Remove-Item -LiteralPath $oldLauncher -Force -ErrorAction SilentlyContinue
        }

        # Only prune backups after the stable path contains the new launcher.
        # Doing this before replacement could delete another concurrent
        # installer's rollback file during its short rename window.
        Remove-JcodeStaleLauncherBackups -LauncherDir $launcherDir
    } finally {
        Remove-Item -LiteralPath $tempLauncher -Force -ErrorAction SilentlyContinue
    }

    return $LauncherPath
}