param(
    [int]$DurationSeconds = 5,
    [string]$Out = ".sisyphus/evidence/task-7-camera-roi.json"
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
    $Report | ConvertTo-Json -Depth 8 | Set-Content -Encoding UTF8 -Path $Path
}

$commandToRerun = "powershell -ExecutionPolicy Bypass -File scripts/e2e/camera_roi_smoke.ps1 -DurationSeconds $DurationSeconds -Out $Out"

if (-not $IsWindows) {
    Write-JsonReport -Path $Out -Report @{
        schema_version = "1.0.0"
        task = 7
        status = "blocked"
        reason = "WINDOWS_CAMERA_REQUIRED"
        message = "This smoke test requires Windows camera APIs and cannot execute real camera capture on this host."
        duration_seconds_requested = $DurationSeconds
        frames_captured = 0
        roi_ok_rate = 0.0
        fps_observed = 0.0
        command_to_rerun_on_windows = $commandToRerun
    }
    Write-Output "WINDOWS_CAMERA_REQUIRED wrote $Out"
    exit 0
}

try {
    Add-Type -AssemblyName Windows.Media.Capture | Out-Null
} catch {
    Write-JsonReport -Path $Out -Report @{
        schema_version = "1.0.0"
        task = 7
        status = "blocked"
        reason = "WINDOWS_CAMERA_API_UNAVAILABLE"
        message = "Windows.Media.Capture is unavailable in this shell/session; rerun from an interactive Windows desktop session with camera permissions."
        duration_seconds_requested = $DurationSeconds
        frames_captured = 0
        roi_ok_rate = 0.0
        fps_observed = 0.0
        command_to_rerun_on_windows = $commandToRerun
    }
    Write-Output "WINDOWS_CAMERA_API_UNAVAILABLE wrote $Out"
    exit 0
}

Write-JsonReport -Path $Out -Report @{
    schema_version = "1.0.0"
    task = 7
    status = "blocked"
    reason = "WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED"
    message = "Windows camera API is present, but this smoke script has not yet been wired to real Rust camera capture and ROI processing. This is a blocking Task 7 gap, not a passing camera smoke."
    duration_seconds_requested = $DurationSeconds
    frames_captured = 0
    roi_ok_rate = 0.0
    fps_observed = 0.0
    command_to_rerun_on_windows = $commandToRerun
    implementation_required = "Wire Rust-owned Windows camera capture, ROI quality processing, and normalized local:// ROI metadata emission before this smoke can pass."
}
Write-Error "WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED wrote $Out"
exit 2
