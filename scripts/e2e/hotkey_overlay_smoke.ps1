param (
    [string]$Target = "notepad",
    [string]$Out = ".sisyphus/evidence/task-8-hotkey-overlay.json"
)

$ErrorActionPreference = "Stop"

function Write-Evidence {
    param (
        [string]$Status,
        [string]$Reason,
        [string]$Platform,
        [bool]$FocusPreserved,
        [bool]$OverlayShown
    )
    $obj = @{
        schema_version = "1.0.0"
        task = 8
        scenario = "hotkey_overlay_smoke"
        status = $Status
        reason = $Reason
        platform = $Platform
        timestamp = (Get-Date).ToString("o")
        focus_preserved = $FocusPreserved
        overlay_shown = $OverlayShown
        command_to_rerun_on_windows = "powershell -ExecutionPolicy Bypass -File scripts/e2e/hotkey_overlay_smoke.ps1 -Target `"$Target`" -Out `"$Out`""
    }
    
    $dir = Split-Path $Out
    if ($dir -and -not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    
    $obj | ConvertTo-Json -Depth 5 | Out-File -FilePath $Out -Encoding utf8
}

if ($IsLinux -or $IsMacOS -or ([System.Environment]::OSVersion.Platform -ne 'Win32NT')) {
    Write-Evidence -Status "blocked" -Reason "WINDOWS_UI_AUTOMATION_REQUIRED" -Platform "Linux" -FocusPreserved $false -OverlayShown $false
    exit 1
}

Write-Evidence -Status "blocked" -Reason "WINDOWS_FREELIP_INTEGRATION_REQUIRED" -Platform "Windows" -FocusPreserved $false -OverlayShown $false
exit 1
