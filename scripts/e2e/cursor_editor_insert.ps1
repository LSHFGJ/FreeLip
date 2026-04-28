param(
    [ValidateSet("fixture-replay")]
    [string]$Mode = "fixture-replay",
    [string]$Editor = "cursor-or-vscode",
    [string]$Out = ".sisyphus/evidence/task-11-cursor-editor.json"
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
        scenario = "cursor_editor_insert"
        status = "blocked"
        reason = $Reason
        message = $Message
        mode = $Mode
        target_app = $Editor
        target_control = "editor"
        fixture_replay_requested = ($Mode -eq "fixture-replay")
        explicit_sidecar_fixture_mode_required = $true
        hotkey_triggered = $false
        editor_verified = $false
        inserted = $false
        overlay_shown = $false
        clipboard_preserved = $true
        event_chain_verified = $false
        unsupported_app_reason = $null
        real_windows_e2e_passed = $false
        command_to_rerun_on_windows = "powershell -ExecutionPolicy Bypass -File scripts/e2e/cursor_editor_insert.ps1 -Mode $Mode -Editor $Editor -Out $Out"
        integration_required = $IntegrationRequired
    }
    Write-Output "$Reason wrote $Out"
}

if (-not (Test-IsWindowsHost)) {
    Write-BlockedReport -Reason "WINDOWS_UI_AUTOMATION_REQUIRED" -Message "This editor smoke requires an interactive Windows desktop editor plus FreeLip global-hotkey automation; this host cannot execute it." -IntegrationRequired "Run on Windows after wiring FreeLip to Cursor or VS Code and verifying the editor buffer through desktop automation."
    exit 0
}

Write-BlockedReport -Reason "WINDOWS_FREELIP_INTEGRATION_REQUIRED" -Message "Windows editor automation may be available, but this script is not wired to the actual FreeLip app/hotkey path. It refuses to claim Cursor/VS Code insertion from standalone clipboard scripting." -IntegrationRequired "Drive FreeLip against Cursor or VS Code, verify insertion or record a scoped unsupported-app reason with fallback evidence, and verify the local debug event chain."
Write-Error "WINDOWS_FREELIP_INTEGRATION_REQUIRED wrote $Out"
exit 2
