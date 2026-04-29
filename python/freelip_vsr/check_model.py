from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import os
import platform
import sys
import time
from pathlib import Path
from typing import Any

from . import cnvsrc_runtime
from .model_registry import (
    ERROR_EXIT_CODES,
    READY_EXIT_CODE,
    CheckpointSpec,
    ModelConfig,
    get_model_config,
    model_ids,
)


class ReadinessFailure(Exception):
    def __init__(self, error_code: str, message: str, details: dict[str, Any] | None = None):
        super().__init__(message)
        self.error_code = error_code
        self.message = message
        self.details = details or {}


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def path_from_override_env_or_default(
    override: str | None, env_var: str, default_relative_path: Path
) -> Path:
    if override:
        return Path(override).expanduser()
    env_value = os.environ.get(env_var)
    if env_value:
        return Path(env_value).expanduser()
    return repo_root() / default_relative_path


def verify_checkpoint(
    path: Path,
    spec: CheckpointSpec,
    expected_sha256: str | None = None,
    expected_size: int | None = None,
) -> dict[str, Any]:
    expected_hash = expected_sha256 if expected_sha256 is not None else spec.expected_sha256
    expected_bytes = expected_size if expected_size is not None else spec.expected_size
    report: dict[str, Any] = {
        "path": str(path),
        "filename": path.name,
        "expected_filename": spec.filename,
        "exists": path.exists(),
        "expected_size": expected_bytes,
        "actual_size": None,
        "expected_sha256": expected_hash,
        "sha256": None,
        "integrity_verified": False,
        "source_url": spec.source_url,
        "alternate_url": spec.alternate_url,
    }
    if not path.exists():
        raise ReadinessFailure(
            "CHECKPOINT_MISSING",
            f"checkpoint not found at {path}",
            {"checkpoint": report},
        )
    if not path.is_file():
        raise ReadinessFailure(
            "CHECKPOINT_MISSING",
            f"checkpoint path is not a file: {path}",
            {"checkpoint": report},
        )

    actual_size = path.stat().st_size
    report["actual_size"] = actual_size
    if expected_bytes is not None and actual_size != expected_bytes:
        raise ReadinessFailure(
            "CHECKPOINT_MISSING",
            f"checkpoint size mismatch for {path}",
            {"checkpoint": report},
        )

    if expected_hash is None:
        return report

    actual_hash = sha256_file(path)
    report["sha256"] = actual_hash
    if actual_hash.lower() != expected_hash.lower():
        raise ReadinessFailure(
            "CHECKPOINT_MISSING",
            f"checkpoint sha256 mismatch for {path}",
            {"checkpoint": report},
        )
    report["integrity_verified"] = True
    return report


def verify_data_root(path: Path) -> dict[str, Any]:
    exists = path.exists()
    has_entries = exists and path.is_dir() and any(path.iterdir())
    report = {"path": str(path), "exists": exists, "has_entries": bool(has_entries)}
    if not has_entries:
        raise ReadinessFailure(
            "DATA_UNAVAILABLE",
            f"required data/root fixture directory is unavailable at {path}",
            {"data": report},
        )
    return report


def import_torch() -> Any:
    try:
        return importlib.import_module("torch")
    except Exception as exc:  # noqa: BLE001 - native torch imports may fail outside ImportError.
        raise ReadinessFailure(
            "RUNTIME_IMPORT_FAILED",
            f"PyTorch import failed: {exc.__class__.__name__}",
            {"import_error": exc.__class__.__name__},
        ) from exc


def classify_runtime_error(exc: BaseException, default_code: str = "RUNTIME_IMPORT_FAILED") -> str:
    name = exc.__class__.__name__.lower()
    message = str(exc).lower()
    if "outofmemory" in name or "out of memory" in message or "cuda oom" in message:
        return "MODEL_OOM"
    if "cuda" in message:
        return "CUDA_INCOMPATIBLE"
    return default_code


def verify_runtime(device: str, checkpoint_path: Path) -> dict[str, Any]:
    torch = import_torch()
    report: dict[str, Any] = {
        "torch_version": getattr(torch, "__version__", "unknown"),
        "torch_cuda_version": getattr(getattr(torch, "version", None), "cuda", None),
        "cuda_available": None,
        "cuda_device_name": None,
        "checkpoint_deserialized": False,
    }
    try:
        cuda_available = bool(torch.cuda.is_available())
        report["cuda_available"] = cuda_available
        if device == "cuda":
            if not cuda_available:
                raise ReadinessFailure(
                    "CUDA_INCOMPATIBLE",
                    "requested cuda device, but torch.cuda.is_available() is false",
                    {"runtime": report},
                )
            report["cuda_device_name"] = torch.cuda.get_device_name(0)
            _ = torch.empty(1, device="cuda")
    except ReadinessFailure:
        raise
    except Exception as exc:  # noqa: BLE001 - classification must hide raw stack traces.
        error_code = classify_runtime_error(exc, default_code="RUNTIME_IMPORT_FAILED")
        raise ReadinessFailure(
            error_code,
            f"runtime probe failed: {exc.__class__.__name__}",
            {"runtime": report, "runtime_error": exc.__class__.__name__},
        ) from exc
    report["checkpoint_deserialized"] = False
    return report


def verify_model_runtime_adapter(model_id: str, checkpoint_path: Path, device: str) -> dict[str, Any]:
    if model_id != "cnvsrc2025":
        return {"required": False, "configured": False, "factory_resolved": False, "runner_loaded": False}
    try:
        return cnvsrc_runtime.verify_runtime_adapter(checkpoint_path=checkpoint_path, device=device)
    except cnvsrc_runtime.RuntimeAdapterError as exc:
        raise ReadinessFailure(exc.error_code, exc.message, exc.details) from exc


def build_base_report(
    config: ModelConfig,
    device: str,
    checkpoint_path: Path,
    data_root: Path,
    require_data: bool,
) -> dict[str, Any]:
    return {
        "schema_version": "1.0.0",
        "model_id": config.model_id,
        "model_name": config.display_name,
        "device": device,
        "ready": False,
        "status": "UNKNOWN",
        "error_code": None,
        "exit_code": None,
        "message": "",
        "generated_at_unix": time.time(),
        "python": {
            "version": platform.python_version(),
            "executable": sys.executable,
            "platform": sys.platform,
        },
        "checkpoint": {
            "path": str(checkpoint_path),
            "filename": checkpoint_path.name,
            "expected_filename": config.checkpoint.filename,
            "exists": checkpoint_path.exists(),
            "expected_size": config.checkpoint.expected_size,
            "actual_size": checkpoint_path.stat().st_size if checkpoint_path.is_file() else None,
            "expected_sha256": config.checkpoint.expected_sha256,
            "sha256": None,
            "integrity_verified": False,
            "source_url": config.checkpoint.source_url,
            "alternate_url": config.checkpoint.alternate_url,
        },
        "data": {
            "required": require_data,
            "path": str(data_root),
            "exists": data_root.exists(),
            "description": config.data.description,
        },
        "source_verification": {
            "checkpoint_source_verified": False,
            "checkpoint_integrity_verified": False,
            "code_source_url": config.source.code_url,
            "huggingface_repo": config.source.huggingface_repo,
            "modelscope_repo": config.source.modelscope_repo,
            "manual_fallback_only": config.manual_fallback_only,
            "license_note": config.source.license_note,
        },
        "fallback": {
            "automatic_switching": False,
            "manual_fallback_model": config.fallback_model_id,
        },
        "setup_note": config.source.setup_note,
    }


def run_check(args: argparse.Namespace) -> tuple[int, dict[str, Any]]:
    config = get_model_config(args.model)
    checkpoint_path = path_from_override_env_or_default(
        args.checkpoint, config.checkpoint.env_var, config.checkpoint.default_relative_path
    )
    data_root = path_from_override_env_or_default(
        args.data_root, config.data.env_var, config.data.default_relative_path
    )
    report = build_base_report(config, args.device, checkpoint_path, data_root, args.require_data)
    try:
        checkpoint_report = verify_checkpoint(
            checkpoint_path,
            config.checkpoint,
            expected_sha256=args.checkpoint_sha256,
            expected_size=args.checkpoint_size,
        )
        report["checkpoint"].update(checkpoint_report)
        integrity_verified = bool(checkpoint_report["integrity_verified"])
        report["source_verification"]["checkpoint_source_verified"] = integrity_verified
        report["source_verification"]["checkpoint_integrity_verified"] = integrity_verified

        if args.require_data:
            report["data"].update(verify_data_root(data_root))

        report["runtime"] = verify_runtime(args.device, checkpoint_path)
        report["runtime_adapter"] = verify_model_runtime_adapter(config.model_id, checkpoint_path, args.device)
        report.update(
            {
                "ready": True,
                "status": "MODEL_READY",
                "exit_code": READY_EXIT_CODE,
                "message": f"MODEL_READY {config.model_id} {args.device}",
            }
        )
        return READY_EXIT_CODE, report
    except ReadinessFailure as exc:
        for key in ("checkpoint", "data", "source_verification", "runtime", "runtime_adapter"):
            current = report.get(key)
            if not isinstance(current, dict):
                continue
            merged = dict(current)
            detail = exc.details.get(key)
            if isinstance(detail, dict):
                merged.update(detail)
            exc.details[key] = merged
        raise


def write_json_report(path: str | None, report: dict[str, Any]) -> None:
    if not path:
        return
    report_path = Path(path)
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(
        json.dumps(report, indent=2, ensure_ascii=False, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def explain_failure(report: dict[str, Any]) -> str:
    lines = [report.get("message", "model readiness failed")]
    error_code = report.get("error_code")
    if error_code == "CHECKPOINT_MISSING":
        checkpoint = report["checkpoint"]
        lines.append(f"Expected checkpoint: {checkpoint['path']}")
        lines.append(f"Primary source: {checkpoint['source_url']}")
        if checkpoint.get("alternate_url"):
            lines.append(f"Alternate source: {checkpoint['alternate_url']}")
    elif error_code == "DATA_UNAVAILABLE":
        lines.append(f"Expected data root: {report['data']['path']}")
    elif error_code in {"CUDA_INCOMPATIBLE", "RUNTIME_IMPORT_FAILED", "MODEL_OOM"}:
        lines.append(report.get("setup_note", "Check PyTorch/CUDA runtime setup."))
    if report["fallback"].get("manual_fallback_model"):
        lines.append(
            "Manual fallback research gate: rerun with "
            f"--model {report['fallback']['manual_fallback_model']} after preparing its checkpoint."
        )
    return "\n".join(lines)


def parser() -> argparse.ArgumentParser:
    parser_ = argparse.ArgumentParser(
        description="Check local FreeLip VSR model readiness and classify fallback failures."
    )
    parser_.add_argument("--model", choices=model_ids(), required=True)
    parser_.add_argument("--device", choices=("cpu", "cuda"), default="cuda")
    parser_.add_argument("--checkpoint", help="Override checkpoint path for this run.")
    parser_.add_argument("--data-root", help="Override public data or ROI fixture root path.")
    parser_.add_argument(
        "--require-data",
        action="store_true",
        help="Require a non-empty data/fixture directory before runtime probing.",
    )
    parser_.add_argument("--json", help="Write machine-readable readiness report to this path.")
    parser_.add_argument("--explain", action="store_true", help="Print setup guidance on failure.")
    parser_.add_argument(
        "--checkpoint-sha256",
        help="Override expected checkpoint sha256, useful for isolated fixture checks.",
    )
    parser_.add_argument(
        "--checkpoint-size",
        type=int,
        help="Override expected checkpoint size in bytes, useful for isolated fixture checks.",
    )
    return parser_


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        exit_code, report = run_check(args)
    except ReadinessFailure as exc:
        config = get_model_config(args.model)
        checkpoint_path = path_from_override_env_or_default(
            args.checkpoint, config.checkpoint.env_var, config.checkpoint.default_relative_path
        )
        data_root = path_from_override_env_or_default(
            args.data_root, config.data.env_var, config.data.default_relative_path
        )
        report = build_base_report(config, args.device, checkpoint_path, data_root, args.require_data)
        for key, value in exc.details.items():
            if isinstance(value, dict) and isinstance(report.get(key), dict):
                report[key].update(value)
            else:
                report[key] = value
        exit_code = ERROR_EXIT_CODES[exc.error_code]
        report.update(
            {
                "ready": False,
                "status": exc.error_code,
                "error_code": exc.error_code,
                "exit_code": exit_code,
                "message": exc.message,
            }
        )
        write_json_report(args.json, report)
        print(explain_failure(report) if args.explain else f"{exc.error_code}: {exc.message}", file=sys.stderr)
        return exit_code

    write_json_report(args.json, report)
    print(report["message"])
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
