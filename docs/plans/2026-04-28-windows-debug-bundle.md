# Windows Debug Bundle Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a Windows-friendly debug bundle workflow that assembles FreeLip, its Python sidecar launcher, config, model placeholder directory, and logs into one inspectable folder.

**Architecture:** Keep the first deliverable as a portable debug bundle instead of a production installer. The bundle is created by PowerShell scripts under `scripts/windows/`, validated by a static TypeScript test, and documented for manual Windows debugging. It must preserve honest blocker behavior such as `CHECKPOINT_MISSING` and avoid bundling models, credentials, or ROI media.

**Tech Stack:** Tauri v2, npm scripts, PowerShell, Python module sidecar, TypeScript static validation tests.

---

### Task 1: Static Debug Bundle Contract Test

**Files:**
- Create: `scripts/test_debug_bundle.ts`
- Modify: `package.json`

**Step 1:** Write a failing TypeScript test that asserts the Windows debug bundle scripts, config template, docs, and npm commands exist.

**Step 2:** Run `node --experimental-strip-types scripts/test_debug_bundle.ts`; expect failure because the files/scripts do not exist yet.

**Step 3:** Add `test:debug-bundle` to `package.json`.

### Task 2: Windows Debug Bundle Scripts

**Files:**
- Create: `scripts/windows/build_debug_bundle.ps1`
- Create: `scripts/windows/run_sidecar_debug.ps1`
- Create: `scripts/windows/README.md`
- Create: `config/freelip.debug.json`
- Create: `models/DEBUG_BUNDLE_README.md`

**Step 1:** Implement `run_sidecar_debug.ps1` as a deterministic launcher for `python -m freelip_vsr.sidecar` with local-only host, token, logs, and optional fixture mode.

**Step 2:** Implement `build_debug_bundle.ps1` to create `debug-dist/FreeLip-debug/` with `app/`, `sidecar/`, `config/`, `models/`, and `logs/`, then write `startup-diagnostics.json`.

**Step 3:** Ensure generated artifacts are not committed.

### Task 3: Documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/internal-mvp.md`

**Step 1:** Document Windows debug bundle commands and expected outputs.

**Step 2:** Document that this is not a signed installer, does not include model weights, and preserves `CHECKPOINT_MISSING` diagnostics.

### Task 4: Verification

**Commands:**
- `node --experimental-strip-types scripts/test_debug_bundle.ts`
- `npm run test:ui`
- `npm run build`
- LSP diagnostics on TypeScript files.

**Expected:** All Linux-safe checks pass; Windows execution remains documented as requiring a Windows host.
