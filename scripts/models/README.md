# FreeLip model readiness scripts

These helpers are internal runtime notes for Task 1. They do not download or commit checkpoints.

## CNVSRC2025 primary gate

```bash
bash scripts/models/check_model.sh cnvsrc2025 cuda --json .sisyphus/evidence/task-1-model-ready.json
```

Expected local checkpoint path by default:

```text
checkpoints/cnvsrc2025/model_avg_cncvs_2_3_cnvsrc.pth
```

The checkpoint must match Hugging Face `ReflectionL/CNVSRC2025Baseline` file `model_avg_cncvs_2_3_cnvsrc.pth`, size `1137500697`, SHA-256 `577cd9558eea111683a406bc25d69c7161cdb79534c2273fc0d0f044c356231c`.

CNVSRC2025 readiness also requires a local runtime adapter:

```bash
export FREELIP_CNVSRC2025_RUNTIME_ADAPTER="your_cnvsrc_adapter_module:create_runner"
```

The adapter uses `module:function` syntax, imports the official CNVSRC2025 baseline runtime, and returns a runner with `runtime_id` plus `decode(request_payload)`.

## Manual MAVSR2025 fallback research gate

```bash
bash scripts/models/check_model.sh mavsr2025 cuda --json .sisyphus/evidence/task-1-mavsr-ready.json
```

This is a manual gate only. FreeLip must not automatically switch from CNVSRC2025 to MAVSR2025 at runtime.

## Official CNVSRC fallback environment

Prefer the active local PyTorch/CUDA runtime first. If imports or CUDA compatibility fail, the upstream baseline documents an official-style conda environment:

```bash
conda create -y -n cnvsrc python==3.10.11
conda activate cnvsrc
conda install pytorch-lightning==1.9.3 pytorch==2.0.1 torchaudio==2.0.2 torchvision==0.15.2 torchmetrics==0.11.2 pytorch-cuda==11.8 cudatoolkit==11.8 -c pytorch -c nvidia -y
pip install -r reqs.txt
```
