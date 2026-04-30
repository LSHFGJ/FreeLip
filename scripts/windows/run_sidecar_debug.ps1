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

function Get-FreeLipProviderPath {
  param([object]$ResolvedPath)

  if ($null -eq $ResolvedPath) {
    return $null
  }

  if ($ResolvedPath.PSObject.Properties.Name -contains "ProviderPath") {
    return [string]$ResolvedPath.ProviderPath
  }

  return [string]$ResolvedPath
}

if ($HostName -ne "127.0.0.1") {
  throw "FreeLip debug sidecar must bind to 127.0.0.1, got '$HostName'."
}

$scriptRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
if ([string]::IsNullOrWhiteSpace($RepoRoot)) {
  $RepoRoot = Get-FreeLipProviderPath (Resolve-Path (Join-Path $scriptRoot "..\.."))
} else {
  $RepoRoot = Get-FreeLipProviderPath (Resolve-Path $RepoRoot)
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

function Set-FreeLipCheckpointEnvFallback {
  param(
    [string]$EnvVar,
    [string]$SourceRepoRoot,
    [string]$RelativePath
  )

  $currentValue = [Environment]::GetEnvironmentVariable($EnvVar, "Process")
  if (-not [string]::IsNullOrWhiteSpace($currentValue)) {
    return $currentValue
  }

  $candidatePath = Join-Path $SourceRepoRoot $RelativePath
  if (Test-Path $candidatePath) {
    Set-Item -Path "Env:$EnvVar" -Value $candidatePath
    return $candidatePath
  }

  return $null
}

function Set-FreeLipEnvValueFallback {
  param(
    [string]$EnvVar,
    [string]$Value,
    [string]$ProbePath
  )

  $currentValue = [Environment]::GetEnvironmentVariable($EnvVar, "Process")
  if (-not [string]::IsNullOrWhiteSpace($currentValue)) {
    return $currentValue
  }

  if (-not [string]::IsNullOrWhiteSpace($ProbePath) -and -not (Test-Path $ProbePath)) {
    return $null
  }

  Set-Item -Path "Env:$EnvVar" -Value $Value
  return $Value
}

function Add-FreeLipPythonPathEntry {
  param(
    [System.Collections.Generic.List[string]]$Entries,
    [string]$PathValue
  )

  if ([string]::IsNullOrWhiteSpace($PathValue) -or -not (Test-Path $PathValue)) {
    return
  }

  if (-not $Entries.Contains($PathValue)) {
    $Entries.Add($PathValue) | Out-Null
  }
}

$pythonPathEntries = New-Object System.Collections.Generic.List[string]
Add-FreeLipPythonPathEntry -Entries $pythonPathEntries -PathValue (Join-Path $RepoRoot "python")

$sourceRepoRoot = Resolve-Path (Join-Path $RepoRoot "..\..") -ErrorAction SilentlyContinue
if ($sourceRepoRoot) {
  $sourceRepoRootPath = Get-FreeLipProviderPath $sourceRepoRoot
  Set-FreeLipCheckpointEnvFallback -EnvVar "FREELIP_CNVSRC2025_CHECKPOINT" -SourceRepoRoot $sourceRepoRootPath -RelativePath "checkpoints\cnvsrc2025\model_avg_cncvs_2_3_cnvsrc.pth" | Out-Null
  Set-FreeLipCheckpointEnvFallback -EnvVar "FREELIP_MAVSR2025_CHECKPOINT" -SourceRepoRoot $sourceRepoRootPath -RelativePath "checkpoints\mavsr2025\mavsr2025-track1-manual-checkpoint.pth" | Out-Null

  $adapterDir = Join-Path $sourceRepoRootPath ".freelip\adapters"
  $adapterPath = Join-Path $adapterDir "freelip_cnvsrc2025_adapter.py"
  if (Test-Path $adapterPath) {
    Set-FreeLipEnvValueFallback -EnvVar "FREELIP_CNVSRC2025_RUNTIME_ADAPTER" -Value "freelip_cnvsrc2025_adapter:create_runner" -ProbePath $adapterPath | Out-Null
    Add-FreeLipPythonPathEntry -Entries $pythonPathEntries -PathValue $adapterDir
  }

  $cnvsrcCodeRoot = Join-Path $sourceRepoRootPath ".freelip\external\CNVSRC2025\VSR"
  if (Test-Path $cnvsrcCodeRoot) {
    Set-FreeLipEnvValueFallback -EnvVar "FREELIP_CNVSRC2025_CODE_ROOT" -Value $cnvsrcCodeRoot -ProbePath $cnvsrcCodeRoot | Out-Null
    Add-FreeLipPythonPathEntry -Entries $pythonPathEntries -PathValue $cnvsrcCodeRoot
  }
}

$existingPythonPath = [Environment]::GetEnvironmentVariable("PYTHONPATH", "Process")
if (-not [string]::IsNullOrWhiteSpace($existingPythonPath)) {
  foreach ($entry in $existingPythonPath.Split([System.IO.Path]::PathSeparator)) {
    Add-FreeLipPythonPathEntry -Entries $pythonPathEntries -PathValue $entry
  }
}

$logDir = Split-Path -Parent $LogPath
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

$env:PYTHONPATH = $pythonPathEntries -join [System.IO.Path]::PathSeparator
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
  checkpoint_env_values = [ordered]@{
    cnvsrc2025 = [Environment]::GetEnvironmentVariable("FREELIP_CNVSRC2025_CHECKPOINT", "Process")
    mavsr2025 = [Environment]::GetEnvironmentVariable("FREELIP_MAVSR2025_CHECKPOINT", "Process")
  }
  runtime_adapter_env = [ordered]@{
    cnvsrc2025 = "FREELIP_CNVSRC2025_RUNTIME_ADAPTER"
  }
  runtime_adapter_env_values = [ordered]@{
    cnvsrc2025 = [Environment]::GetEnvironmentVariable("FREELIP_CNVSRC2025_RUNTIME_ADAPTER", "Process")
    cnvsrc2025_code_root = [Environment]::GetEnvironmentVariable("FREELIP_CNVSRC2025_CODE_ROOT", "Process")
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
