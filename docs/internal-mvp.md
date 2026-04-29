# FreeLip internal MVP notes

This document is for `internal research only`. FreeLip is `not production ready`, provides `no commercial rights`, and must not be represented as a validated medical, accessibility, or production dictation product.

## MVP shape

FreeLip is a local Windows desktop VSR MVP. The app shell is Tauri/Rust, the sidecar is Python, and local endpoints bind to `127.0.0.1`. The default activation path is `Ctrl+Alt+Space`, which opens a candidate overlay rather than silently trusting model output.

## Hardware and operating system split

- Target hardware: Windows desktop or laptop with a camera, GPU-capable VSR runtime where available, and local storage for model artifacts.
- Linux/WSL can run contracts, fixture replay, docs checks, and most Python/Rust tests.
- Real camera capture requires Windows: `WINDOWS_CAMERA_REQUIRED`.
- Rust camera wiring is not complete: `WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED`.
- Real Notepad/browser/editor UI automation cannot be proven on Linux/WSL: `WINDOWS_UI_AUTOMATION_REQUIRED`.
- Full FreeLip hotkey-to-target-app verification still needs app wiring: `WINDOWS_FREELIP_INTEGRATION_REQUIRED`.

## Model fallback and evaluation

The MVP tracks CNVSRC2025 first, then `MAVSR2025` / `CAS-VSR-S101` evaluation work, but this checkout must remain honest when model artifacts are absent. CNVSRC2025 licensing and artifacts are restricted to approved internal research use; they do not grant production, commercial, redistribution, or public-release rights. Missing local checkpoints are reported as `CHECKPOINT_MISSING`; a verified checkpoint without a configured CNVSRC runtime adapter is reported as `RUNTIME_IMPORT_FAILED`; fixture paths may exercise contracts but must not claim real model readiness, Windows E2E success, or Top-5 >= 0.60.

Fallback behavior is contract-first and manual, not automatic runtime switching:

1. Check CNVSRC2025 readiness first with `PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m freelip_vsr.check_model --model cnvsrc2025 --explain`.
2. If CNVSRC2025 is blocked, manually check `MAVSR2025` with `PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m freelip_vsr.check_model --model mavsr2025 --checkpoint <local-mavsr2025-checkpoint-path> --explain`.
3. If both are blocked, document `CAS-VSR-S101` as a manual research fallback only; do not claim commercial readiness or public-release evidence.
4. If model artifacts are missing or runtime probing fails, the sidecar reports the blocker instead of fabricating production candidates.
5. Fixture replay is allowed only for local verification and must stay labeled as fixture/fallback.

Exact evaluation command:

```bash
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m freelip_eval.run --suite fixtures/eval/ai_prompt_short_cn --report .sisyphus/evidence/task-12-top5.json
```

Current Task 12 fixture report is `top5_usability=0.0`, `target_met=false` due to `CHECKPOINT_MISSING`. This reflects missing local checkpoints and fixture fallback, not real model quality.

## Privacy and retention

FreeLip has `no cloud VSR`. The LLM rerank path is text-only and `text-only LLM rerank disabled by default`. Do not send raw video (`no raw video`), ROI media (`no ROI`), frame bytes, screenshots, embeddings, or debug clips to any cloud service. Local ROI/debug metadata is capped by `7-day retention`.

## E2E reruns

### Windows debug bundle

The MVP now provides a portable Windows debug bundle path for local troubleshooting. It is not a signed installer and does not include model weights.

```powershell
npm run bundle:debug:win
powershell -NoProfile -ExecutionPolicy Bypass -File debug-dist/FreeLip-debug/run-debug.ps1
```

The generated folder is `debug-dist/FreeLip-debug/`. It contains `app/`, `sidecar/`, `python/`, `config/`, `models/`, and `logs/`. Check `logs/startup-diagnostics.json`, `logs/sidecar-startup-diagnostics.json`, and `logs/sidecar.log` first when debugging startup or sidecar failures. `CHECKPOINT_MISSING` remains expected until local approved checkpoints are installed under the configured model path. Add `-FixtureMode` only for deterministic sidecar contract checks; fixture mode always reports a fixture backend and must not be used for real checkpoint/adapter validation.

For approved local model artifacts, omit `-FixtureMode` and use the same environment variables as the readiness gate:

```powershell
$env:FREELIP_CNVSRC2025_CHECKPOINT = "C:\path\to\approved\cnvsrc2025.ckpt"
$env:FREELIP_CNVSRC2025_RUNTIME_ADAPTER = "your_cnvsrc_adapter_module:create_runner"
$env:FREELIP_MAVSR2025_CHECKPOINT = "C:\path\to\approved\mavsr2025.ckpt"
```

`FREELIP_CNVSRC2025_RUNTIME_ADAPTER` must point to a local `module:function` adapter that imports the official CNVSRC2025 baseline runtime. The adapter factory receives `checkpoint_path` and `device`, then returns a runner with `runtime_id` and `decode(request_payload)`.

Run these from the repository root after preparing real Windows hardware and model artifacts:

```powershell
# Windows camera/ROI smoke: expect failure until camera implementation is wired.
pwsh -File scripts/e2e/camera_roi_smoke.ps1

# Windows UI automation/full-loop smokes: expect integration-required output until app wiring is complete.
pwsh -File scripts/e2e/notepad_hotkey_insert.ps1
pwsh -File scripts/e2e/notepad_full_loop.ps1
pwsh -File scripts/e2e/browser_textarea_insert.ps1
pwsh -File scripts/e2e/cursor_editor_insert.ps1
pwsh -File scripts/e2e/sidecar_crash_recovery.ps1
```

Run Linux-safe checks with:

```bash
PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m pytest python/tests -q
npm run build
cargo test --workspace
```

## Troubleshooting

| Symptom | Meaning | Action |
| --- | --- | --- |
| `CHECKPOINT_MISSING` | Local CNVSRC/MAVSR artifacts are absent | Install approved checkpoints locally; do not commit them |
| `RUNTIME_IMPORT_FAILED` | PyTorch/CUDA or the CNVSRC runtime adapter is unavailable | Install the official baseline runtime and set `FREELIP_CNVSRC2025_RUNTIME_ADAPTER` |
| `WINDOWS_CAMERA_REQUIRED` | Camera test ran outside Windows | Rerun on Windows hardware |
| `WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED` | Windows camera APIs exist but Rust capture is unwired | Complete camera integration before acceptance |
| `WINDOWS_UI_AUTOMATION_REQUIRED` | Linux/WSL cannot prove target-app insertion | Rerun Windows UI smoke after integration |
| `WINDOWS_FREELIP_INTEGRATION_REQUIRED` | Script cannot drive the real app loop yet | Wire FreeLip app/hotkey path before claiming E2E |

Keep all examples placeholder-only. Never add real API keys, private model paths, raw video, or ROI media to documentation or tests.
