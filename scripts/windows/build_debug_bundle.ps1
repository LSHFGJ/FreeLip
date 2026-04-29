param(
  [switch]$SkipTauriBuild,
  [switch]$FixtureMode,
  [string]$Configuration = "Debug"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Resolve-Path (Join-Path $scriptRoot "..\..")
$distRoot = Join-Path $repoRoot "debug-dist"
$bundleRoot = Join-Path $distRoot "FreeLip-debug"
$appDir = Join-Path $bundleRoot "app"
$sidecarDir = Join-Path $bundleRoot "sidecar"
$configDir = Join-Path $bundleRoot "config"
$modelsDir = Join-Path $bundleRoot "models"
$logsDir = Join-Path $bundleRoot "logs"
$pythonDir = Join-Path $bundleRoot "python"
$diagnosticsPath = Join-Path $logsDir "startup-diagnostics.json"

Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $bundleRoot
New-Item -ItemType Directory -Force -Path $appDir, $sidecarDir, $configDir, $modelsDir, $logsDir, $pythonDir | Out-Null

Push-Location $repoRoot
try {
  if (-not $SkipTauriBuild) {
    npm run build
    npm run tauri -- build --debug
  }
} finally {
  Pop-Location
}

$debugExeCandidates = @(
  (Join-Path $repoRoot "src-tauri\target\debug\freelip.exe"),
  (Join-Path $repoRoot "src-tauri\target\debug\freelip-tauri.exe"),
  (Join-Path $repoRoot "target\debug\freelip.exe"),
  (Join-Path $repoRoot "target\debug\freelip-tauri.exe")
)
$debugExe = $debugExeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $debugExe) {
  $debugExe = $debugExeCandidates[0]
}
$debugBundleCandidates = @(
  (Join-Path $repoRoot "src-tauri\target\debug\bundle"),
  (Join-Path $repoRoot "target\debug\bundle")
)
$debugBundle = $debugBundleCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (Test-Path $debugExe) {
  Copy-Item -Force $debugExe (Join-Path $appDir "freelip.exe")
}
if ($debugBundle -and (Test-Path $debugBundle)) {
  Copy-Item -Recurse -Force $debugBundle (Join-Path $appDir "tauri-bundle")
}
if (-not (Test-Path (Join-Path $appDir "freelip.exe"))) {
  @"
FreeLip Windows debug executable was not found at:
$($debugExeCandidates -join "`n")

Run this script on Windows without -SkipTauriBuild after installing Node, Rust, and Tauri prerequisites.
"@ | Set-Content -Encoding UTF8 -Path (Join-Path $appDir "MISSING_WINDOWS_BUILD.txt")
}

Copy-Item -Force (Join-Path $repoRoot "config\freelip.debug.json") (Join-Path $configDir "freelip.debug.json")
Copy-Item -Force (Join-Path $repoRoot "models\DEBUG_BUNDLE_README.md") (Join-Path $modelsDir "README.md")
Copy-Item -Force (Join-Path $repoRoot "scripts\windows\run_sidecar_debug.ps1") (Join-Path $sidecarDir "run-sidecar-debug.ps1")
Copy-Item -Recurse -Force (Join-Path $repoRoot "python\freelip_vsr") (Join-Path $pythonDir "freelip_vsr")
Copy-Item -Recurse -Force (Join-Path $repoRoot "python\freelip_eval") (Join-Path $pythonDir "freelip_eval")

$launcher = @"
param(
  [switch]`$FixtureMode
)

Set-StrictMode -Version Latest
`$ErrorActionPreference = "Stop"

`$bundleRoot = Split-Path -Parent `$MyInvocation.MyCommand.Path
`$sidecarScript = Join-Path `$bundleRoot "sidecar\run-sidecar-debug.ps1"
`$sidecarLog = Join-Path `$bundleRoot "logs\sidecar.log"
`$configPath = Join-Path `$bundleRoot "config\freelip.debug.json"
`$appExe = Join-Path `$bundleRoot "app\freelip.exe"
`$sidecarHealthUrl = "http://127.0.0.1:8765/health"

function Wait-FreeLipSidecarHealth {
  param(
    [string]`$HealthUrl,
    [int]`$TimeoutSeconds = 30
  )

  `$deadline = (Get-Date).AddSeconds(`$TimeoutSeconds)
  do {
    try {
      `$response = Invoke-WebRequest -Uri `$HealthUrl -UseBasicParsing -TimeoutSec 2
      if (`$response.StatusCode -eq 200) {
        return `$true
      }
    } catch {
      Start-Sleep -Milliseconds 500
    }
  } while ((Get-Date) -lt `$deadline)

  return `$false
}

Write-Host "Starting FreeLip debug sidecar..."
`$sidecarArgs = @("-File", `$sidecarScript, "-RepoRoot", `$bundleRoot, "-LogPath", `$sidecarLog, "-ConfigPath", `$configPath)
if (`$FixtureMode) { `$sidecarArgs += "-FixtureMode" }
Start-Process -FilePath "powershell" -ArgumentList `$sidecarArgs -WindowStyle Normal

Write-Host "Waiting for FreeLip sidecar health at `$sidecarHealthUrl..."
if (-not (Wait-FreeLipSidecarHealth -HealthUrl `$sidecarHealthUrl -TimeoutSeconds 45)) {
  throw "FreeLip sidecar did not become healthy. Check logs\sidecar.log and logs\sidecar-startup-diagnostics.json."
}

if (Test-Path `$appExe) {
  Write-Host "Starting FreeLip app..."
  Start-Process -FilePath `$appExe -WorkingDirectory (Split-Path -Parent `$appExe)
} else {
  throw "FreeLip executable is missing. See app\MISSING_WINDOWS_BUILD.txt."
}
"@
$launcher | Set-Content -Encoding UTF8 -Path (Join-Path $bundleRoot "run-debug.ps1")

$batchLauncher = @"
@echo off
setlocal
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-debug.ps1" %*
set FREELIP_EXIT=%ERRORLEVEL%
echo.
if %FREELIP_EXIT% EQU 0 (
  echo FreeLip debug launcher finished. Press any key to close this window.
) else (
  echo FreeLip debug launcher failed with exit code %FREELIP_EXIT%. Press any key to close this window.
)
pause >nul
exit /b %FREELIP_EXIT%
"@
$batchLauncher | Set-Content -Encoding ASCII -Path (Join-Path $bundleRoot "Run-FreeLip.bat")

$diagnostics = [ordered]@{
  schema_version = "1.0.0"
  profile = "windows-debug-bundle"
  generated_at_utc = (Get-Date).ToUniversalTime().ToString("o")
  repo_root = $repoRoot.ToString()
  bundle_root = $bundleRoot
  app_dir = $appDir
  sidecar_dir = $sidecarDir
  config_path = Join-Path $configDir "freelip.debug.json"
  models_dir = $modelsDir
  logs_dir = $logsDir
  python_dir = $pythonDir
  startup_diagnostics = $diagnosticsPath
  fixture_mode_requested = [bool]$FixtureMode
  tauri_debug_exe = $debugExe
  tauri_debug_exe_candidates = $debugExeCandidates
  tauri_debug_bundle = $debugBundle
  tauri_debug_bundle_candidates = $debugBundleCandidates
  tauri_debug_exe_present = Test-Path $debugExe
  expected_missing_checkpoint_code = "CHECKPOINT_MISSING"
  commands = [ordered]@{
    build = "npm run bundle:debug:win"
    run = "powershell -NoProfile -ExecutionPolicy Bypass -File debug-dist\FreeLip-debug\run-debug.ps1"
    run_fixture = "powershell -NoProfile -ExecutionPolicy Bypass -File debug-dist\FreeLip-debug\run-debug.ps1 -FixtureMode"
  }
  notes = @(
    "This is an unsigned internal debug bundle, not a production installer.",
    "Model weights are not bundled; CHECKPOINT_MISSING is expected until local checkpoints are installed.",
    "Logs are written under the bundle logs directory for easy attachment to bug reports."
  )
}

$diagnostics | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 -Path $diagnosticsPath

Write-Host "FreeLip debug bundle created at $bundleRoot"
Write-Host "Startup diagnostics written to $diagnosticsPath"
