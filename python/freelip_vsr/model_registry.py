from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path


READY_EXIT_CODE = 0

ERROR_EXIT_CODES: dict[str, int] = {
    "DATA_UNAVAILABLE": 20,
    "CHECKPOINT_MISSING": 21,
    "CUDA_INCOMPATIBLE": 22,
    "RUNTIME_IMPORT_FAILED": 23,
    "MODEL_OOM": 24,
}


@dataclass(frozen=True)
class ModelSource:
    code_url: str
    huggingface_repo: str | None = None
    modelscope_repo: str | None = None
    license_note: str = ""
    setup_note: str = ""


@dataclass(frozen=True)
class CheckpointSpec:
    filename: str
    default_relative_path: Path
    env_var: str
    expected_sha256: str | None
    expected_size: int | None
    source_url: str
    alternate_url: str | None = None


@dataclass(frozen=True)
class DataSpec:
    default_relative_path: Path
    env_var: str
    description: str


@dataclass(frozen=True)
class ModelConfig:
    model_id: str
    display_name: str
    checkpoint: CheckpointSpec
    data: DataSpec
    source: ModelSource
    manual_fallback_only: bool = False
    fallback_model_id: str | None = None


MODEL_REGISTRY: dict[str, ModelConfig] = {
    "cnvsrc2025": ModelConfig(
        model_id="cnvsrc2025",
        display_name="CNVSRC2025 VSR Baseline",
        checkpoint=CheckpointSpec(
            filename="model_avg_cncvs_2_3_cnvsrc.pth",
            default_relative_path=Path("checkpoints/cnvsrc2025/model_avg_cncvs_2_3_cnvsrc.pth"),
            env_var="FREELIP_CNVSRC2025_CHECKPOINT",
            expected_sha256="577cd9558eea111683a406bc25d69c7161cdb79534c2273fc0d0f044c356231c",
            expected_size=1_137_500_697,
            source_url=(
                "https://huggingface.co/ReflectionL/CNVSRC2025Baseline/resolve/main/"
                "model_avg_cncvs_2_3_cnvsrc.pth"
            ),
            alternate_url=(
                "https://www.modelscope.cn/models/PaintedVeil/CNVSRC2025Baseline"
            ),
        ),
        data=DataSpec(
            default_relative_path=Path("datasets/cnvsrc2025"),
            env_var="FREELIP_CNVSRC2025_DATA_ROOT",
            description="CNVSRC/CN-CVS public challenge data or local ROI fixture directory",
        ),
        source=ModelSource(
            code_url="https://github.com/liu12366262626/CNVSRC2025/tree/main/VSR",
            huggingface_repo="ReflectionL/CNVSRC2025Baseline",
            modelscope_repo="PaintedVeil/CNVSRC2025Baseline",
            license_note="CNVSRC2025 baseline notes non-commercial/benchmarking-only use.",
            setup_note=(
                "Try the active PyTorch/CUDA runtime first. If incompatible, create the "
                "official-style conda env: python==3.10.11, pytorch==2.0.1, "
                "torchvision==0.15.2, torchaudio==2.0.2, pytorch-cuda==11.8, "
                "cudatoolkit==11.8, pytorch-lightning==1.9.3, torchmetrics==0.11.2."
            ),
        ),
        fallback_model_id="mavsr2025",
    ),
    "mavsr2025": ModelConfig(
        model_id="mavsr2025",
        display_name="MAVSR2025 Track1 CAS-VSR-S101 Baseline",
        checkpoint=CheckpointSpec(
            filename="mavsr2025-track1-manual-checkpoint.pth",
            default_relative_path=Path(
                "checkpoints/mavsr2025/mavsr2025-track1-manual-checkpoint.pth"
            ),
            env_var="FREELIP_MAVSR2025_CHECKPOINT",
            expected_sha256=None,
            expected_size=None,
            source_url=(
                "https://github.com/VIPL-Audio-Visual-Speech-Understanding/"
                "MAVSR2025-Track1"
            ),
        ),
        data=DataSpec(
            default_relative_path=Path("datasets/mavsr2025"),
            env_var="FREELIP_MAVSR2025_DATA_ROOT",
            description="CAS-VSR-S101/CAS-VSR-MOV20 data prepared for MAVSR2025 Track1",
        ),
        source=ModelSource(
            code_url=(
                "https://github.com/VIPL-Audio-Visual-Speech-Understanding/"
                "MAVSR2025-Track1"
            ),
            license_note="Manual fallback research gate; verify upstream dataset/model terms before use.",
            setup_note=(
                "MAVSR2025 Track1 baseline documents torch==1.12.1+cu113, "
                "torchvision==0.13.1+cu113, torchaudio==0.12.1, and accelerate."
            ),
        ),
        manual_fallback_only=True,
    ),
}


def get_model_config(model_id: str) -> ModelConfig:
    normalized = model_id.lower()
    try:
        return MODEL_REGISTRY[normalized]
    except KeyError as exc:
        known = ", ".join(sorted(MODEL_REGISTRY))
        raise ValueError(f"unknown model '{model_id}'. Expected one of: {known}") from exc


def model_ids() -> tuple[str, ...]:
    return tuple(sorted(MODEL_REGISTRY))
