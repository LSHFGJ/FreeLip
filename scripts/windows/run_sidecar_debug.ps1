param(
  [string]$HostName = "127.0.0.1",
  [int]$Port = 18765,
  [string]$Token = "debug-local-token-change-before-sharing",
  [ValidateSet("cpu", "cuda")]
  [string]$Device = "cpu",
  [switch]$FixtureMode,
  [switch]$Detached,
  [string]$RepoRoot,
  [string]$LogPath,
  [string]$ConfigPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($HostName -ne "127.0.0.1") {
  throw "FreeLip debug sidecar must bind to 127.0.0.1, got '$HostName'."
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
  $RepoRoot = Resolve-Path (Join-Path $scriptRoot "..\..")
} else {
  $RepoRoot = Resolve-Path $RepoRoot
}

if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
  $candidateConfig = Join-Path $RepoRoot "config\freelip.debug.json"
  if (Test-Path $candidateConfig) {
    $ConfigPath = $candidateConfig
  }
}

if (-not [string]::IsNullOrWhiteSpace($ConfigPath) -and (Test-Path $ConfigPath)) {
  $config = Get-Content -Raw -Path $ConfigPath | ConvertFrom-Json
  if ($PSBoundParameters.ContainsKey("HostName") -eq $false -and $null -ne $config.sidecar.host) {
    $HostName = [string]$config.sidecar.host
  }
  if ($PSBoundParameters.ContainsKey("Port") -eq $false -and $null -ne $config.sidecar.port) {
    $Port = [int]$config.sidecar.port
  }
  if ($PSBoundParameters.ContainsKey("Token") -eq $false -and $null -ne $config.sidecar.token) {
    $Token = [string]$config.sidecar.token
  }
  if ($PSBoundParameters.ContainsKey("FixtureMode") -eq $false -and $null -ne $config.sidecar.fixture_mode) {
    $FixtureMode = [bool]$config.sidecar.fixture_mode
  }
}

if ($HostName -ne "127.0.0.1") {
  throw "FreeLip debug sidecar must bind to 127.0.0.1, got '$HostName'."
}

if ([string]::IsNullOrWhiteSpace($LogPath)) {
  $LogPath = Join-Path $RepoRoot "logs\sidecar.log"
}

$logDir = Split-Path -Parent $LogPath
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

$env:PYTHONPATH = Join-Path $RepoRoot "python"
$arguments = @(
  "-m", "freelip_vsr.sidecar",
  "--host", $HostName,
  "--port", $Port.ToString(),
  "--token", $Token,
  "--model", "cnvsrc2025",
  "--device", $Device
)

if ($FixtureMode) {
  $arguments += "--fixture-mode"
}

$diagnostic = [ordered]@{
  schema_version = "1.0.0"
  command = "python -m freelip_vsr.sidecar --host $HostName --port $Port --token <redacted> --model cnvsrc2025 --device $Device$($(if ($FixtureMode) { ' --fixture-mode' } else { '' }))"
  repo_root = $RepoRoot.ToString()
  pythonpath = $env:PYTHONPATH
  config_path = $ConfigPath
  host = $HostName
  port = $Port
  device = $Device
  fixture_mode = [bool]$FixtureMode
  detached = [bool]$Detached
  log_path = $LogPath
  error_log_path = [System.IO.Path]::ChangeExtension($LogPath, ".err.log")
  expected_missing_checkpoint_code = "CHECKPOINT_MISSING"
  checkpoint_env = [ordered]@{
    cnvsrc2025 = "FREELIP_CNVSRC2025_CHECKPOINT"
    mavsr2025 = "FREELIP_MAVSR2025_CHECKPOINT"
  }
  runtime_adapter_env = [ordered]@{
    cnvsrc2025 = "FREELIP_CNVSRC2025_RUNTIME_ADAPTER"
  }
}

$diagnostic | ConvertTo-Json -Depth 4 | Set-Content -Encoding UTF8 -Path (Join-Path $logDir "sidecar-startup-diagnostics.json")

Write-Host "Starting FreeLip sidecar on http://$HostName`:$Port"
Write-Host "Writing sidecar log to $LogPath"
if ($Detached) {
  $errorLogPath = [System.IO.Path]::ChangeExtension($LogPath, ".err.log")
  Remove-Item -Force -ErrorAction SilentlyContinue $LogPath, $errorLogPath
  Start-Process -FilePath "python" -ArgumentList $arguments -WindowStyle Hidden -RedirectStandardOutput $LogPath -RedirectStandardError $errorLogPath | Out-Null
  Write-Host "Detached FreeLip sidecar process started. Writing sidecar stderr to $errorLogPath"
  exit 0
}
& python @arguments 2>&1 | Tee-Object -FilePath $LogPath
