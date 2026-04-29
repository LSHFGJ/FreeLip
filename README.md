# FreeLip

FreeLip is a Windows desktop MVP for local Chinese visual speech recognition (VSR). It combines a Tauri/Rust shell, a Python sidecar, local ROI processing, top-5 candidate display, personal dictionary reranking, and conservative text insertion.

> [!WARNING]
> FreeLip is **internal research only**, **not production ready**, and grants **no commercial rights**. It uses **no cloud VSR**, sends **no raw video** to cloud services, and does not send ROI media (`no ROI`) to any LLM path.

## What works today

- Local dev shell served on `127.0.0.1` with the default hotkey `Ctrl+Alt+Space`.
- Python sidecar contracts for model readiness, loopback auth, fixture decode, evaluation, and privacy checks.
- Rust core state machines for ROI metadata, hotkey overlay, candidate ranking, insertion/undo planning, and full-loop fixture replay.
- Evaluation harness for the `MAVSR2025` / `CAS-VSR-S101` internal MVP path, with honest fallback reporting when checkpoints are absent. CNVSRC2025, MAVSR2025, and CAS-VSR-S101 references are research-only and are not commercial/public-release evidence.

## Current blockers

| Area | Status | Blocker |
| --- | --- | --- |
| Model readiness | Blocked locally | `CHECKPOINT_MISSING` until real checkpoints are installed; `RUNTIME_IMPORT_FAILED` until the CNVSRC runtime adapter is configured |
| Camera capture | Windows-only | `WINDOWS_CAMERA_REQUIRED` and `WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED` |
| UI automation | Windows-only | `WINDOWS_UI_AUTOMATION_REQUIRED` |
| End-to-end app loop | Integration pending | `WINDOWS_FREELIP_INTEGRATION_REQUIRED` |


## Model fallback notes

Fallback is manual and research-only; FreeLip does not automatically switch production runtimes. The intended check sequence is:

```bash
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m freelip_vsr.check_model --model cnvsrc2025 --explain
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m freelip_vsr.check_model --model mavsr2025 --checkpoint <local-mavsr2025-checkpoint-path> --explain
```

If CNVSRC2025 and the manual `MAVSR2025` check are blocked, document `CAS-VSR-S101` as a manual research fallback only. Current Task 12 fixture evidence reports `top5_usability=0.0` and `target_met=false` because of `CHECKPOINT_MISSING`; that is not a real model-quality result.

## Privacy and safety defaults

- `text-only LLM rerank disabled by default`; local ranking is the default MVP path.
- Debug metadata uses `7-day retention` and must not include raw frames, crops, embeddings, screenshots, or secrets.
- `.env.example` contains placeholders only; do not put real tokens in docs, fixtures, or examples.

## Setup

```bash
npm install
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m pytest python/tests -q
npm run build
```

For development:

```bash
npm run dev
npm run tauri dev
```

## Windows debug bundle

For Windows debugging, build a portable debug bundle instead of a production installer:

```powershell
npm run bundle:debug:win
```

This creates:

```text
debug-dist/FreeLip-debug/
```

Run it with:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File debug-dist/FreeLip-debug/run-debug.ps1
```

The bundle keeps the app, Python sidecar source, debug config, model placeholder directory, and logs in one folder so failures are easy to inspect. It is unsigned, internal-only, does not include model checkpoints, and should report `CHECKPOINT_MISSING` until approved local model artifacts are installed. Add `-FixtureMode` only when you want deterministic contract fixtures; fixture mode never validates real model inference.

To test approved local checkpoints, omit `-FixtureMode` and set the readiness-gate environment variables before launching the sidecar or app:

```powershell
$env:FREELIP_CNVSRC2025_CHECKPOINT = "C:\path\to\approved\cnvsrc2025.ckpt"
$env:FREELIP_CNVSRC2025_RUNTIME_ADAPTER = "your_cnvsrc_adapter_module:create_runner"
$env:FREELIP_MAVSR2025_CHECKPOINT = "C:\path\to\approved\mavsr2025.ckpt"
```

The CNVSRC adapter imports the official baseline runtime and returns a runner used by `/decode`; fixture mode remains contract-only and is not real model inference.

Useful debug files:

```text
debug-dist/FreeLip-debug/logs/startup-diagnostics.json
debug-dist/FreeLip-debug/logs/sidecar-startup-diagnostics.json
debug-dist/FreeLip-debug/logs/sidecar.log
```

## Verification commands

```bash
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m freelip_eval.verify_docs --docs README.md docs/internal-mvp.md
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m freelip_eval.verify_docs --docs README.md docs/internal-mvp.md --require internal-research-only --out .sisyphus/evidence/task-13-docs.json
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m pytest python/tests/test_no_secret_examples.py -q
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m pytest python/tests -q
```

See [`docs/internal-mvp.md`](docs/internal-mvp.md) for hardware, fallback, privacy, rerun, and troubleshooting details.
