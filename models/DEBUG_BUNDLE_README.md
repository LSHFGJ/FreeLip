# FreeLip Debug Bundle Models Directory

The Windows debug bundle creates this directory so local model artifacts have a predictable place to live during manual testing.

Model checkpoints are **not** included in the repository or generated bundle. Until you copy approved local checkpoints into this directory and configure the sidecar to use them, model readiness should report `CHECKPOINT_MISSING`.

For CNVSRC2025, a checkpoint alone is not enough. Real inference also needs a local runtime adapter that imports the official baseline code and exposes a factory function. Configure it with `module:function` syntax. If the checkpoint verifies but the adapter is missing, readiness should report `RUNTIME_IMPORT_FAILED` rather than pretending fixture output is real model output.

For debug runs, point the sidecar at approved local checkpoints with the actual readiness-gate environment variables:

```powershell
$env:FREELIP_CNVSRC2025_CHECKPOINT = "C:\path\to\approved\cnvsrc2025.ckpt"
$env:FREELIP_CNVSRC2025_RUNTIME_ADAPTER = "your_cnvsrc_adapter_module:create_runner"
$env:FREELIP_MAVSR2025_CHECKPOINT = "C:\path\to\approved\mavsr2025.ckpt"
```

The adapter factory is called with `checkpoint_path` and `device`, and must return an object with:

```python
runtime_id: str
decode(request_payload: dict) -> Sequence[RuntimeCandidate]
```

Do not commit model files, datasets, ROI clips, embeddings, or screenshots.
