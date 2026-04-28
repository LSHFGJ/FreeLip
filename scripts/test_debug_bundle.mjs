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

const packageJson = JSON.parse(readText("package.json"));

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
assertContains(
  "python/pyproject.toml",
  "[tool.setuptools.packages.find]",
  "explicit package discovery for editable installs",
);

for (const filePath of [
  "scripts/windows/build_debug_bundle.ps1",
  "scripts/windows/run_sidecar_debug.ps1",
  "scripts/windows/README.md",
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
  "README.md",
  "Windows debug bundle",
  "Windows debug bundle instructions",
);
assertContains(
  "docs/internal-mvp.md",
  "bundle:debug:win",
  "internal debug bundle command",
);
