import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

const root = process.cwd();

function readText(relativePath) {
  return readFileSync(path.join(root, relativePath), "utf8");
}

function assertExists(relativePath) {
  assert.equal(
    existsSync(path.join(root, relativePath)),
    true,
    `${relativePath} should exist`,
  );
}

function assertContains(relativePath, expected, reason) {
  const text = readText(relativePath);
  const matches = typeof expected === "string" ? text.includes(expected) : expected.test(text);
  assert.equal(matches, true, `${relativePath} should contain ${reason}`);
}

function assertNotContains(relativePath, unexpected, reason) {
  const text = readText(relativePath);
  const matches =
    typeof unexpected === "string" ? text.includes(unexpected) : unexpected.test(text);
  assert.equal(matches, false, `${relativePath} should not contain ${reason}`);
}

const packageJson = JSON.parse(readText("package.json"));
const debugConfig = JSON.parse(readText("config/freelip.debug.json"));

assert.equal(
  packageJson.scripts?.["bundle:debug:win"],
  "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/build_debug_bundle.ps1",
  "package.json should expose the Windows debug bundle builder",
);
assert.equal(
  packageJson.scripts?.["sidecar:debug:win"],
  "powershell -NoProfile -ExecutionPolicy Bypass -File scripts/windows/run_sidecar_debug.ps1",
  "package.json should expose the Windows sidecar debug launcher",
);
assert.equal(
  packageJson.scripts?.["test:debug-bundle"],
  "node scripts/test_debug_bundle.mjs",
  "package.json should expose debug bundle validation",
);
assert.equal(
  debugConfig.sidecar?.fixture_mode,
  false,
  "debug config should default to real-runtime mode; use -FixtureMode only for contract fixtures",
);
assert.equal(
  debugConfig.sidecar?.port,
  18765,
  "debug config should avoid Windows environments where 127.0.0.1:8765 is unavailable",
);

assertContains(
  "python/pyproject.toml",
  "[project.scripts]",
  "Python console script section for debug launchers",
);
assertContains(
  "python/pyproject.toml",
  "freelip-vsr-sidecar = \"freelip_vsr.sidecar:main\"",
  "sidecar console script entry point",
);
assertExists("requirements.txt");
assertContains(
  "requirements.txt",
  "https://download.pytorch.org/whl/cu118",
  "PyTorch CUDA 11.8 wheel index for one-command runtime bootstrap",
);
assertContains(
  "requirements.txt",
  "torch==2.0.1+cu118",
  "CNVSRC2025 CUDA PyTorch runtime pin",
);
assertContains(
  "requirements.txt",
  "Python 3.10/3.11",
  "Python version target for pinned PyTorch runtime",
);
assertContains(
  "README.md",
  "python -m pip install -r requirements.txt",
  "documented pip requirements bootstrap command",
);
assertContains(
  "python/pyproject.toml",
  "[tool.setuptools.packages.find]",
  "explicit package discovery for editable installs",
);

for (const filePath of [
  "scripts/windows/build_debug_bundle.ps1",
  "scripts/windows/run_sidecar_debug.ps1",
  "scripts/windows/README.md",
  "src-tauri/icons/icon.ico",
  "config/freelip.debug.json",
  "models/DEBUG_BUNDLE_README.md",
]) {
  assertExists(filePath);
}

assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  "startup-diagnostics.json",
  "startup diagnostics output",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  "debug-dist",
  "debug distribution folder",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  "Run-FreeLip.bat",
  "one-click debug launcher batch file",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  'powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-debug.ps1" %*',
  "batch launcher forwarding to run-debug.ps1",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  'pushd "%~dp0"',
  "batch launcher supports UNC bundle paths",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  "FreeLip debug launcher finished. Press any key to close this window.",
  "batch launcher keeps diagnostics visible after successful launch",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  "pause >nul",
  "batch launcher waits before closing",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  'throw "FreeLip executable is missing. See app\\MISSING_WINDOWS_BUILD.txt."',
  "missing app executable fails the launcher instead of flashing closed",
);
assertContains(
  "scripts/windows/README.md",
  "Run-FreeLip.bat",
  "one-click batch launcher documentation",
);
assertContains(
  "README.md",
  "Run-FreeLip.bat",
  "top-level one-click batch launcher instructions",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  "freelip-tauri.exe",
  "actual Tauri debug executable name fallback",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  'Join-Path $repoRoot "target\\debug\\freelip-tauri.exe"',
  "workspace-root Tauri debug executable fallback",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  'Join-Path $repoRoot "target\\debug\\bundle"',
  "workspace-root Tauri debug bundle fallback",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  "Wait-FreeLipSidecarHealth",
  "sidecar health gate before app launch",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  '"-Detached"',
  "bundle launcher starts a detached sidecar process that survives app startup",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "[switch]$Detached",
  "sidecar launcher detached mode for one-click bundle startup",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  'Start-Process -FilePath "python"',
  "detached mode starts Python sidecar directly instead of relying on a long-running PowerShell pipe",
);
assertContains(
  "scripts/windows/build_debug_bundle.ps1",
  'http://`${sidecarHost}:`$sidecarPort/health',
  "sidecar loopback health endpoint derived from debug config",
);
assertNotContains(
  "scripts/windows/build_debug_bundle.ps1",
  "http://127.0.0.1:8765/health",
  "hardcoded legacy sidecar health endpoint",
);
assertContains(
  "src/main.ts",
  'from "./sidecarConfig.ts"',
  "frontend sidecar status endpoint imported from shared debug config",
);
assertNotContains(
  "src/main.ts",
  "http://127.0.0.1:8765/model/status",
  "hardcoded legacy frontend sidecar status endpoint",
);
assertContains(
  "src/modelStatus.ts",
  'from "./sidecarConfig.ts"',
  "model status copy derived from shared sidecar config",
);
assertContains(
  "python/freelip_vsr/sidecar.py",
  "default=18765",
  "Python sidecar CLI default port aligned with Windows-safe debug port",
);
assertContains(
  "src-tauri/tauri.conf.json",
  "icons/icon.ico",
  "explicit Windows Tauri icon path",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "freelip_vsr.sidecar",
  "Python sidecar module invocation",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "freelip.debug.json",
  "debug config loading",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "127.0.0.1",
  "loopback-only sidecar host",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "<redacted>",
  "redacted token diagnostics",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "FREELIP_CNVSRC2025_RUNTIME_ADAPTER",
  "runtime adapter startup diagnostics",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "Set-FreeLipCheckpointEnvFallback",
  "bundle sidecar launcher discovers source checkout checkpoints when env vars are unset",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "model_avg_cncvs_2_3_cnvsrc.pth",
  "CNVSRC2025 checkpoint fallback filename",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "FREELIP_CNVSRC2025_CHECKPOINT",
  "CNVSRC2025 checkpoint env fallback",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "Set-FreeLipEnvValueFallback",
  "bundle sidecar launcher discovers local runtime adapter env when unset",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "freelip_cnvsrc2025_adapter:create_runner",
  "local CNVSRC2025 runtime adapter fallback",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "FREELIP_CNVSRC2025_CODE_ROOT",
  "local CNVSRC2025 code root env fallback",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "PathSeparator",
  "debug sidecar preserves and extends PYTHONPATH instead of replacing it",
);
assertContains(
  "scripts/windows/run_sidecar_debug.ps1",
  "ProviderPath",
  "debug sidecar normalizes PowerShell provider-qualified paths for Windows Python",
);
assertContains(
  "config/freelip.debug.json",
  "CHECKPOINT_MISSING",
  "honest missing-checkpoint diagnostic expectation",
);
assertContains(
  "config/freelip.debug.json",
  "FREELIP_CNVSRC2025_CHECKPOINT",
  "actual CNVSRC checkpoint environment variable",
);
assertContains(
  "config/freelip.debug.json",
  "FREELIP_MAVSR2025_CHECKPOINT",
  "actual MAVSR checkpoint environment variable",
);
assertContains(
  "config/freelip.debug.json",
  "FREELIP_CNVSRC2025_RUNTIME_ADAPTER",
  "actual CNVSRC runtime adapter environment variable",
);
assertContains(
  ".env.example",
  "FREELIP_CNVSRC2025_RUNTIME_ADAPTER",
  "placeholder CNVSRC runtime adapter environment variable",
);
assertContains(
  "models/DEBUG_BUNDLE_README.md",
  "module:function",
  "runtime adapter factory guidance",
);
assertContains(
  "README.md",
  "Windows debug bundle",
  "Windows debug bundle instructions",
);
assertContains(
  "docs/internal-mvp.md",
  "bundle:debug:win",
  "internal debug bundle command",
);
