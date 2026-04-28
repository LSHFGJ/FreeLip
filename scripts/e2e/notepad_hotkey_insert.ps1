param(
    [string]$InsertedText = "帮我总结这段文字",
    [string]$Out = ".sisyphus/evidence/task-10-notepad.json"
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
        [string]$Path,
        [string]$CommandToRerun
    )

    Write-JsonReport -Path $Path -Report @{
        schema_version = "1.0.0"
        task = 10
        scenario = "notepad_hotkey_insert"
        status = "blocked"
        reason = $Reason
        message = $Message
        inserted = $false
        undo_within_3s = $false
        clipboard_restored = $false
        command_to_rerun_on_windows = $CommandToRerun
    }
    Write-Output "$Reason wrote $Path"
}

function Wait-ForMainWindow {
    param([System.Diagnostics.Process]$Process)

    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        $Process.Refresh()
        if ($Process.MainWindowHandle -ne 0) {
            return $true
        }
        Start-Sleep -Milliseconds 250
    }
    return $false
}

function Read-ActiveTextViaClipboard {
    [System.Windows.Forms.SendKeys]::SendWait("^a")
    Start-Sleep -Milliseconds 150
    [System.Windows.Forms.SendKeys]::SendWait("^c")
    Start-Sleep -Milliseconds 150
    return (Get-Clipboard -Raw -ErrorAction Stop)
}

$commandToRerun = "powershell -ExecutionPolicy Bypass -File scripts/e2e/notepad_hotkey_insert.ps1 -Out $Out"

$isWindowsHost = Test-IsWindowsHost
if (-not $isWindowsHost) {
    Write-BlockedReport -Path $Out -Reason "WINDOWS_UI_AUTOMATION_REQUIRED" -Message "This smoke test requires an interactive Windows desktop, Notepad, clipboard access, and SendKeys/UI Automation; this host cannot execute it." -CommandToRerun $commandToRerun
    exit 0
}

try {
    Add-Type -AssemblyName System.Windows.Forms | Out-Null
    Add-Type -AssemblyName Microsoft.VisualBasic | Out-Null
} catch {
    Write-BlockedReport -Path $Out -Reason "WINDOWS_AUTOMATION_ASSEMBLY_UNAVAILABLE" -Message "System.Windows.Forms or Microsoft.VisualBasic automation assemblies are unavailable in this PowerShell session." -CommandToRerun $commandToRerun
    exit 0
}


Write-JsonReport -Path $Out -Report @{
    schema_version = "1.0.0"
    task = 10
    scenario = "notepad_hotkey_insert"
    status = "blocked"
    reason = "WINDOWS_FREELIP_INTEGRATION_REQUIRED"
    message = "Windows UI automation is available, but this scaffold is not yet wired to the actual FreeLip hotkey/app insertion path. It refuses to claim Notepad insertion pass from script-only SendKeys."
    inserted = $false
    undo_within_3s = $false
    undo_blocked = $false
    clipboard_restored = $false
    command_to_rerun_on_windows = $commandToRerun
    integration_required = "Wire this smoke to launch the FreeLip desktop app, trigger its configured hotkey path, observe InsertRecord/undo state, and verify SendInput fallback before it may report pass."
}
Write-Error "WINDOWS_FREELIP_INTEGRATION_REQUIRED wrote $Out"
exit 2

$process = $null
$previousClipboard = $null
$clipboardSnapshotAvailable = $false
$clipboardRestoreAttempted = $false
$clipboardRestored = $false

try {
    try {
        $previousClipboard = Get-Clipboard -Raw -ErrorAction Stop
        $clipboardSnapshotAvailable = $true
    } catch {
        Write-BlockedReport -Path $Out -Reason "CLIPBOARD_SAVE_FAILED" -Message "The smoke test could not save the existing text clipboard, so it refused to mutate the clipboard." -CommandToRerun $commandToRerun
        exit 0
    }

    $process = Start-Process -FilePath "notepad.exe" -PassThru
    if (-not (Wait-ForMainWindow -Process $process)) {
        Write-BlockedReport -Path $Out -Reason "NOTEPAD_WINDOW_UNAVAILABLE" -Message "Notepad did not expose a main window in time for interactive automation." -CommandToRerun $commandToRerun
        exit 0
    }

    [Microsoft.VisualBasic.Interaction]::AppActivate($process.Id) | Out-Null
    Start-Sleep -Milliseconds 300

    Set-Clipboard -Value $InsertedText -ErrorAction Stop
    [System.Windows.Forms.SendKeys]::SendWait("^v")
    Start-Sleep -Milliseconds 300
    $contentAfterInsert = Read-ActiveTextViaClipboard
    $inserted = $contentAfterInsert.Contains($InsertedText)

    [System.Windows.Forms.SendKeys]::SendWait("^z")
    Start-Sleep -Milliseconds 300
    $contentAfterUndo = Read-ActiveTextViaClipboard
    $undoWithin3s = $inserted -and (-not $contentAfterUndo.Contains($InsertedText))
} finally {
    if ($clipboardSnapshotAvailable) {
        try {
            Set-Clipboard -Value $previousClipboard -ErrorAction Stop
            $clipboardRestoreAttempted = $true
            $restoredValue = Get-Clipboard -Raw -ErrorAction Stop
            $clipboardRestored = ($restoredValue -eq $previousClipboard)
        } catch {
            $clipboardRestoreAttempted = $true
            $clipboardRestored = $false
        }
    }
    if ($process -ne $null -and -not $process.HasExited) {
        Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
}

$status = if ($inserted -and $undoWithin3s -and $clipboardRestored) { "pass" } else { "fail" }
$reason = if ($status -eq "pass") { $null } else { "NOTEPAD_INSERT_UNDO_FAILED" }
Write-JsonReport -Path $Out -Report @{
    schema_version = "1.0.0"
    task = 10
    scenario = "notepad_hotkey_insert"
    status = $status
    reason = $reason
    inserted = $inserted
    undo_within_3s = $undoWithin3s
    clipboard_restore_attempted = $clipboardRestoreAttempted
    clipboard_restored = $clipboardRestored
    target_app = "notepad.exe"
    insertion_method = "clipboard_paste"
    send_input_fallback_attempted = $false
    command_to_rerun_on_windows = $commandToRerun
}

if ($status -eq "pass") {
    Write-Output "TASK_10_NOTEPAD_PASS wrote $Out"
    exit 0
}

Write-Error "NOTEPAD_INSERT_UNDO_FAILED wrote $Out"
exit 1
