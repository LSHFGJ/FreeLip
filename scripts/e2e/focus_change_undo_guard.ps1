param(
    [string]$InsertedText = "帮我总结这段文字",
    [string]$UnrelatedText = "不应该删除的文本",
    [string]$Out = ".sisyphus/evidence/task-10-focus-change.json"
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
        scenario = "focus_change_undo_guard"
        status = "blocked"
        reason = $Reason
        message = $Message
        undo_blocked = $false
        unrelated_text_deleted = $null
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

function Activate-ProcessWindow {
    param([System.Diagnostics.Process]$Process)

    [Microsoft.VisualBasic.Interaction]::AppActivate($Process.Id) | Out-Null
    Start-Sleep -Milliseconds 250
}

function Paste-Text {
    param([string]$Text)

    Set-Clipboard -Value $Text -ErrorAction Stop
    [System.Windows.Forms.SendKeys]::SendWait("^v")
    Start-Sleep -Milliseconds 250
}

function Read-ActiveTextViaClipboard {
    [System.Windows.Forms.SendKeys]::SendWait("^a")
    Start-Sleep -Milliseconds 150
    [System.Windows.Forms.SendKeys]::SendWait("^c")
    Start-Sleep -Milliseconds 150
    return (Get-Clipboard -Raw -ErrorAction Stop)
}

$commandToRerun = "powershell -ExecutionPolicy Bypass -File scripts/e2e/focus_change_undo_guard.ps1 -Out $Out"

$isWindowsHost = Test-IsWindowsHost
if (-not $isWindowsHost) {
    Write-BlockedReport -Path $Out -Reason "WINDOWS_UI_AUTOMATION_REQUIRED" -Message "This smoke test requires an interactive Windows desktop with Notepad, clipboard access, and window focus automation; this host cannot execute it." -CommandToRerun $commandToRerun
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
    scenario = "focus_change_undo_guard"
    status = "blocked"
    reason = "WINDOWS_FREELIP_INTEGRATION_REQUIRED"
    message = "Windows UI automation is available, but this scaffold is not yet wired to the actual FreeLip undo state machine. It refuses to claim focus-change guard pass from script-only window-handle checks."
    inserted = $false
    undo_within_3s = $false
    undo_blocked = $false
    clipboard_restored = $false
    command_to_rerun_on_windows = $commandToRerun
    integration_required = "Wire this smoke to trigger the FreeLip undo command after changing focus and verify the Rust state-machine reason FOCUS_CHANGED before it may report pass."
}
Write-Error "WINDOWS_FREELIP_INTEGRATION_REQUIRED wrote $Out"
exit 2

$first = $null
$second = $null
$previousClipboard = $null
$clipboardSnapshotAvailable = $false
$clipboardRestoreAttempted = $false
$clipboardRestored = $false
$undoBlocked = $false
$unrelatedTextDeleted = $null

try {
    try {
        $previousClipboard = Get-Clipboard -Raw -ErrorAction Stop
        $clipboardSnapshotAvailable = $true
    } catch {
        Write-BlockedReport -Path $Out -Reason "CLIPBOARD_SAVE_FAILED" -Message "The smoke test could not save the existing text clipboard, so it refused to mutate the clipboard." -CommandToRerun $commandToRerun
        exit 0
    }

    $first = Start-Process -FilePath "notepad.exe" -PassThru
    $second = Start-Process -FilePath "notepad.exe" -PassThru
    if ((-not (Wait-ForMainWindow -Process $first)) -or (-not (Wait-ForMainWindow -Process $second))) {
        Write-BlockedReport -Path $Out -Reason "NOTEPAD_WINDOW_UNAVAILABLE" -Message "Notepad did not expose both main windows in time for focus-change automation." -CommandToRerun $commandToRerun
        exit 0
    }

    Activate-ProcessWindow -Process $first
    Paste-Text -Text $InsertedText
    $insertedWindowHandle = $first.MainWindowHandle

    Activate-ProcessWindow -Process $second
    Paste-Text -Text $UnrelatedText
    $activeWindowHandle = $second.MainWindowHandle

    if ($activeWindowHandle -ne $insertedWindowHandle) {
        $undoBlocked = $true
    } else {
        [System.Windows.Forms.SendKeys]::SendWait("^z")
        Start-Sleep -Milliseconds 250
    }

    $secondContent = Read-ActiveTextViaClipboard
    $unrelatedTextDeleted = (-not $secondContent.Contains($UnrelatedText))
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
    foreach ($process in @($first, $second)) {
        if ($process -ne $null -and -not $process.HasExited) {
            Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
        }
    }
}

$status = if ($undoBlocked -and (-not $unrelatedTextDeleted) -and $clipboardRestored) { "pass" } else { "fail" }
$reason = if ($undoBlocked) { "FOCUS_CHANGED" } else { "FOCUS_GUARD_FAILED" }
Write-JsonReport -Path $Out -Report @{
    schema_version = "1.0.0"
    task = 10
    scenario = "focus_change_undo_guard"
    status = $status
    reason = $reason
    undo_blocked = $undoBlocked
    unrelated_text_deleted = $unrelatedTextDeleted
    clipboard_restore_attempted = $clipboardRestoreAttempted
    clipboard_restored = $clipboardRestored
    command_to_rerun_on_windows = $commandToRerun
}

if ($status -eq "pass") {
    Write-Output "TASK_10_FOCUS_GUARD_PASS wrote $Out"
    exit 0
}

Write-Error "FOCUS_CHANGE_UNDO_GUARD_FAILED wrote $Out"
exit 1
