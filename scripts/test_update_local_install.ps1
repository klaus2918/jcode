param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
$updateScript = Join-Path $repoRoot 'scripts\update_local_install.ps1'

$originalLocalAppData = $env:LOCALAPPDATA
$originalImportOnly = $env:JCODE_UPDATE_PS1_IMPORT_ONLY
$originalJcodeHome = $env:JCODE_HOME

function Assert-True($Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-Equal($Expected, $Actual, [string]$Message) {
    if ($Expected -ne $Actual) {
        throw "$Message`nExpected: $Expected`nActual:   $Actual"
    }
}

function Assert-PathCount([string]$PathValue, [string]$Entry, [int]$ExpectedCount, [string]$Message) {
    $entryKey = ConvertTo-JcodePathKey $Entry
    $count = 0
    foreach ($candidate in (Split-JcodePathList $PathValue)) {
        if ((ConvertTo-JcodePathKey $candidate) -eq $entryKey) { $count += 1 }
    }
    Assert-Equal $ExpectedCount $count $Message
}

try {
    $env:LOCALAPPDATA = 'C:\Users\Test User\AppData\Local'
    $env:JCODE_UPDATE_PS1_IMPORT_ONLY = '1'
    $env:JCODE_HOME = $null
    . $updateScript

    $jcHome = 'C:\Users\Test User\.jcode'
    $launcherDir = Join-Path $jcHome 'bin'
    $npmDir = 'C:\Users\Test User\AppData\Roaming\npm'
    $oldLocalAppDataBin = Join-Path $env:LOCALAPPDATA 'jcode\bin'

    Write-Host 'test_path_update_appends_and_preserves_unrelated_entries'
    # The regression this guards: PATH must be read first and rebuilt by
    # prepending jcode's launcher dir, never overwritten with a fixed string.
    # Unrelated entries (npm global bin, Python, System32) must survive.
    $current = "$npmDir;C:\Python312;$oldLocalAppDataBin;C:\Windows\System32"
    $update = Resolve-JcodePathUpdate -LauncherDir $launcherDir -CurrentPath $current -JcodeHome $jcHome
    Assert-Equal "$launcherDir;$npmDir;C:\Python312;C:\Windows\System32" $update.Path 'update should prepend launcher dir and preserve unrelated entries in order'
    Assert-PathCount $update.Path $launcherDir 1 'updated PATH should contain exactly one launcher dir'
    Assert-PathCount $update.Path $npmDir 1 'npm global bin must appear exactly once'
    Assert-PathCount $update.Path 'C:\Python312' 1 'Python entry must be preserved'
    Assert-PathCount $update.Path 'C:\Windows\System32' 1 'System32 entry must be preserved'
    Assert-PathCount $update.Path $oldLocalAppDataBin 0 'stale LOCALAPPDATA\jcode\bin entry must be removed'
    Assert-Equal 1 $update.RemovedManagedEntries 'exactly one stale jcode entry should be removed'

    Write-Host 'test_path_update_is_idempotent'
    $secondUpdate = Resolve-JcodePathUpdate -LauncherDir $launcherDir -CurrentPath $update.Path -JcodeHome $jcHome
    Assert-Equal $false $secondUpdate.Changed 're-running the update on an already-correct PATH should be a no-op'
    Assert-PathCount $secondUpdate.Path $npmDir 1 'npm global bin must still appear exactly once after no-op'

    Write-Host 'test_path_update_dedupes_case_and_trailing_slash_variants'
    $variantPath = "$launcherDir\;$npmDir;C:\Users\test user\.JCODE\BIN;C:\Windows"
    $variantUpdate = Resolve-JcodePathUpdate -LauncherDir $launcherDir -CurrentPath $variantPath -JcodeHome $jcHome
    Assert-PathCount $variantUpdate.Path $launcherDir 1 'case- and slash-variants of the launcher dir must collapse to one entry'
    Assert-Equal "$launcherDir;$npmDir;C:\Windows" $variantUpdate.Path 'variant update should produce canonical PATH'

    Write-Host 'test_path_update_null_path'
    $nullUpdate = Resolve-JcodePathUpdate -LauncherDir $launcherDir -CurrentPath $null -JcodeHome $jcHome
    Assert-Equal $launcherDir $nullUpdate.Path 'null current PATH should yield just the launcher dir'
    Assert-Equal $true $nullUpdate.Changed 'null PATH should be reported as changed'

    Write-Host 'test_path_update_removes_old_layout_root_and_build_slot'
    $managedKeys = Get-JcodeManagedPathKeys -JcodeHome $jcHome
    Assert-True ($managedKeys.Contains((ConvertTo-JcodePathKey $launcherDir))) 'launcher bin dir should be managed'
    Assert-True ($managedKeys.Contains((ConvertTo-JcodePathKey (Join-Path $jcHome 'builds\current-release-lto')))) 'build slot dir should be managed'
    Assert-True ($managedKeys.Contains((ConvertTo-JcodePathKey $oldLocalAppDataBin))) 'legacy LOCALAPPDATA\jcode\bin should be managed'
    Assert-True ($managedKeys.Contains((ConvertTo-JcodePathKey (Join-Path $env:LOCALAPPDATA 'jcode')))) 'legacy LOCALAPPDATA\jcode root should be managed'
    Assert-True (-not $managedKeys.Contains((ConvertTo-JcodePathKey $npmDir))) 'unrelated npm dir must never be managed'

    Write-Host 'test_path_update_survives_expandable_and_quoted_entries'
    $expandable = '%USERPROFILE%\AppData\Roaming\npm;"C:\Python 3.12";' + $oldLocalAppDataBin
    $expandableUpdate = Resolve-JcodePathUpdate -LauncherDir $launcherDir -CurrentPath $expandable -JcodeHome $jcHome
    Assert-Equal "$launcherDir;%USERPROFILE%\AppData\Roaming\npm;C:\Python 3.12" $expandableUpdate.Path 'expandable and quoted unrelated entries must be preserved (quotes normalized like install.ps1)'

    Write-Host 'test_all_scripts_share_one_path_utils_module'
    $pathUtils = Join-Path $repoRoot 'scripts\lib\path_utils.ps1'
    Assert-True (Test-Path -LiteralPath $pathUtils) 'scripts/lib/path_utils.ps1 must exist'
    foreach ($script in @(
        $updateScript,
        (Join-Path $repoRoot 'scripts\install.ps1'),
        (Join-Path $repoRoot 'scripts\uninstall.ps1')
    )) {
        $text = Get-Content -LiteralPath $script -Raw
        Assert-True ($text -match 'path_utils\.ps1') "$script must reference the shared path_utils module instead of a private copy"
    }

    Write-Host 'test_unified_managed_keys_converge_across_call_styles'
    # The union of every managed key must be reachable from both call styles:
    # install/uninstall pass -InstallDir, update_local_install passes -JcodeHome,
    # and the JCODE_HOME env var is honored when the single-exe layout is active.
    $installManaged = Get-JcodeManagedPathKeys -InstallDir $oldLocalAppDataBin
    foreach ($expected in @(
        $oldLocalAppDataBin,
        (Join-Path $env:LOCALAPPDATA 'jcode')
    )) {
        Assert-True ($installManaged.Contains((ConvertTo-JcodePathKey $expected))) "InstallDir call style must manage $expected"
    }
    Assert-True (-not $installManaged.Contains((ConvertTo-JcodePathKey $npmDir))) 'unrelated npm dir must never be managed via the InstallDir call style'
    $env:JCODE_HOME = $jcHome
    try {
        $envHomeManaged = Get-JcodeManagedPathKeys -InstallDir $oldLocalAppDataBin
        Assert-True ($envHomeManaged.Contains((ConvertTo-JcodePathKey $launcherDir))) 'JCODE_HOME env var must be managed via the InstallDir call style too'
        Assert-True ($envHomeManaged.Contains((ConvertTo-JcodePathKey (Join-Path $jcHome 'builds\current-release-lto')))) 'JCODE_HOME build slot must be managed via the InstallDir call style too'
    } finally {
        $env:JCODE_HOME = $null
    }
    $jcodeHomeManaged = Get-JcodeManagedPathKeys -JcodeHome $jcHome
    Assert-True ($jcodeHomeManaged.Contains((ConvertTo-JcodePathKey $oldLocalAppDataBin))) 'JcodeHome call style must also manage the legacy LOCALAPPDATA bin dir'

    Write-Host 'All update_local_install PATH tests passed.'
} finally {
    $env:LOCALAPPDATA = $originalLocalAppData
    $env:JCODE_UPDATE_PS1_IMPORT_ONLY = $originalImportOnly
    $env:JCODE_HOME = $originalJcodeHome
}
