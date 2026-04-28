# FreeLip Debug Bundle Models Directory

The Windows debug bundle creates this directory so local model artifacts have a predictable place to live during manual testing.

Model checkpoints are **not** included in the repository or generated bundle. Until you copy approved local checkpoints into this directory and configure the sidecar to use them, model readiness should report `CHECKPOINT_MISSING`.

For debug runs, point the sidecar at approved local checkpoints with the actual readiness-gate environment variables:

```powershell
$env:FREELIP_CNVSRC2025_CHECKPOINT = "C:\path\to\approved\cnvsrc2025.ckpt"
$env:FREELIP_MAVSR2025_CHECKPOINT = "C:\path\to\approved\mavsr2025.ckpt"
```

Do not commit model files, datasets, ROI clips, embeddings, or screenshots.
