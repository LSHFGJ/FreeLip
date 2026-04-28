# FreeLip Windows Debug Scripts

These scripts create and run a portable debug bundle for internal Windows testing. They are intentionally separate from production installer work so sidecar startup, model discovery, and logs stay easy to inspect.

## Build the debug bundle

From the repository root on Windows:

```powershell
npm run bundle:debug:win
```

The output folder is:

```text
debug-dist/FreeLip-debug/
```

It contains `app/`, `sidecar/`, `python/`, `config/`, `models/`, and `logs/` directories plus `run-debug.ps1`.

## Run the bundle

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File debug-dist/FreeLip-debug/run-debug.ps1 -FixtureMode
```

`-FixtureMode` starts the Python sidecar in deterministic fixture mode. This is useful for validating the IPC/API shape without claiming real model quality.

## Logs and diagnostics

Check these files first when debugging failures:

```text
debug-dist/FreeLip-debug/logs/startup-diagnostics.json
debug-dist/FreeLip-debug/logs/sidecar-startup-diagnostics.json
debug-dist/FreeLip-debug/logs/sidecar.log
```

`CHECKPOINT_MISSING` is expected until approved CNVSRC/MAVSR model checkpoints are installed locally. Do not put model weights, datasets, ROI clips, or credentials in Git.
