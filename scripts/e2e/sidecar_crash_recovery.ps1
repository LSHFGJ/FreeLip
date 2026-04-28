param(
    [ValidateSet("fixture-replay")]
    [string]$Mode = "fixture-replay",
    [string]$Out = ".sisyphus/evidence/task-11-sidecar-crash.json"
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
        scenario = "sidecar_crash_recovery"
        status = "blocked"
        reason = $Reason
        message = $Message
        mode = $Mode
        expected_visible_state = "SIDECAR_UNAVAILABLE"
        visible_state_observed = $false
        sidecar_crash_triggered = $false
        hotkey_restart_observed = $false
        session_reset_observed = $false
        partial_insertion = $false
        clipboard_preserved = $true
        event_chain_verified = $false
        real_windows_e2e_passed = $false
        command_to_rerun_on_windows = "powershell -ExecutionPolicy Bypass -File scripts/e2e/sidecar_crash_recovery.ps1 -Mode $Mode -Out $Out"
        integration_required = $IntegrationRequired
    }
    Write-Output "$Reason wrote $Out"
}

if (-not (Test-IsWindowsHost)) {
    Write-BlockedReport -Reason "WINDOWS_UI_AUTOMATION_REQUIRED" -Message "This recovery smoke requires an interactive Windows desktop, the FreeLip app, and sidecar process lifecycle control; this host cannot execute it." -IntegrationRequired "Run on Windows after wiring the script to kill or withhold the sidecar during capture, observe visible SIDECAR_UNAVAILABLE state, verify session reset, verify hotkey restart, and verify no partial insertion."
    exit 0
}

Write-BlockedReport -Reason "WINDOWS_FREELIP_INTEGRATION_REQUIRED" -Message "Windows process control may be available, but this script is not wired to the actual FreeLip app and sidecar lifecycle. It refuses to claim crash recovery from a synthetic process check." -IntegrationRequired "Drive FreeLip capture while stopping the real sidecar, verify SIDECAR_UNAVAILABLE visible state, preserved clipboard, no partial insertion, and reset hotkey/session state."
Write-Error "WINDOWS_FREELIP_INTEGRATION_REQUIRED wrote $Out"
exit 2
