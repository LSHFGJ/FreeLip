param(
    [ValidateSet("fixture-replay")]
    [string]$Mode = "fixture-replay",
    [string]$Browser = "edge-or-chrome",
    [string]$Out = ".sisyphus/evidence/task-11-browser-textarea.json"
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
        scenario = "browser_textarea_insert"
        status = "blocked"
        reason = $Reason
        message = $Message
        mode = $Mode
        target_app = $Browser
        target_control = "textarea"
        fixture_replay_requested = ($Mode -eq "fixture-replay")
        explicit_sidecar_fixture_mode_required = $true
        hotkey_triggered = $false
        textarea_verified = $false
        inserted = $false
        overlay_shown = $false
        clipboard_preserved = $true
        event_chain_verified = $false
        real_windows_e2e_passed = $false
        command_to_rerun_on_windows = "powershell -ExecutionPolicy Bypass -File scripts/e2e/browser_textarea_insert.ps1 -Mode $Mode -Browser $Browser -Out $Out"
        integration_required = $IntegrationRequired
    }
    Write-Output "$Reason wrote $Out"
}

if (-not (Test-IsWindowsHost)) {
    Write-BlockedReport -Reason "WINDOWS_UI_AUTOMATION_REQUIRED" -Message "This textarea smoke requires an interactive Windows desktop browser plus FreeLip global-hotkey automation; this host cannot execute it." -IntegrationRequired "Run on Windows after wiring FreeLip to a real Edge/Chrome textarea target and verifying target text through browser/desktop automation."
    exit 0
}

Write-BlockedReport -Reason "WINDOWS_FREELIP_INTEGRATION_REQUIRED" -Message "Windows browser automation may be available, but this script is not wired to the actual FreeLip app/hotkey path. It refuses to claim textarea insertion from standalone page scripting." -IntegrationRequired "Drive FreeLip against a real browser textarea, verify the target received the selected or auto-inserted candidate, and verify the local debug event chain."
Write-Error "WINDOWS_FREELIP_INTEGRATION_REQUIRED wrote $Out"
exit 2
