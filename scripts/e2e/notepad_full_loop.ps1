param(
    [ValidateSet("fixture-replay")]
    [string]$Mode = "fixture-replay",
    [string]$Out = ".sisyphus/evidence/task-11-notepad-full.json"
)

$ErrorActionPreference = "Stop"

function Write-JsonReport {
    param(
        [hashtable]$Report,
        [string]$Path
    )

    $directory = Split-Path -Parent $Path
    if ($directory) {
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }
    $Report | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 -Path $Path
}

function Test-IsWindowsHost {
    $isWindowsVariable = Get-Variable -Name IsWindows -ErrorAction SilentlyContinue
    if ($isWindowsVariable -ne $null) {
        return [bool]$isWindowsVariable.Value
    }
    return [System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT
}

function Write-BlockedReport {
    param(
        [string]$Reason,
        [string]$Message,
        [string]$IntegrationRequired
    )

    Write-JsonReport -Path $Out -Report @{
        schema_version = "1.0.0"
        task = 11
        scenario = "notepad_full_loop"
        status = "blocked"
        reason = $Reason
        message = $Message
        mode = $Mode
        target_app = "notepad.exe"
        fixture_replay_requested = ($Mode -eq "fixture-replay")
        explicit_sidecar_fixture_mode_required = $true
        hotkey_triggered = $false
        roi_metadata_observed = $false
        sidecar_decode_observed = $false
        local_rerank_observed = $false
        inserted = $false
        undo_recorded = $false
        overlay_shown = $false
        clipboard_preserved = $true
        event_chain_verified = $false
        real_windows_e2e_passed = $false
        command_to_rerun_on_windows = "powershell -ExecutionPolicy Bypass -File scripts/e2e/notepad_full_loop.ps1 -Mode $Mode -Out $Out"
        integration_required = $IntegrationRequired
    }
    Write-Output "$Reason wrote $Out"
}

if (-not (Test-IsWindowsHost)) {
    Write-BlockedReport -Reason "WINDOWS_UI_AUTOMATION_REQUIRED" -Message "This full-loop smoke requires an interactive Windows desktop, the FreeLip app, Notepad, clipboard access, and global-hotkey automation; this host cannot execute it." -IntegrationRequired "Run on Windows after wiring the script to launch FreeLip, request explicit sidecar fixture mode, trigger the configured hotkey, verify ROI metadata, verify insertion/undo, and read the local debug event chain."
    exit 0
}

Write-BlockedReport -Reason "WINDOWS_FREELIP_INTEGRATION_REQUIRED" -Message "Windows desktop automation is available, but this script is not yet wired to the actual FreeLip app and target Notepad session. It refuses to claim a full-loop insertion result from script-only automation." -IntegrationRequired "Drive FreeLip end-to-end: app launch, hotkey start/stop, local ROI fixture replay, explicit fixture-mode sidecar decode, optional local rerank, insertion/undo record, and local debug log verification."
Write-Error "WINDOWS_FREELIP_INTEGRATION_REQUIRED wrote $Out"
exit 2
