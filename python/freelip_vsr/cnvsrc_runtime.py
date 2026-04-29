from __future__ import annotations

import importlib
import os
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Protocol


CNVSRC_RUNTIME_ADAPTER_ENV = "FREELIP_CNVSRC2025_RUNTIME_ADAPTER"


@dataclass(frozen=True)
class RuntimeCandidate:
    text: str
    score: float


class RuntimeRunner(Protocol):
    runtime_id: str

    def decode(self, request_payload: dict[str, Any]) -> Sequence[RuntimeCandidate]:
        ...


class RuntimeDecodeError(Exception):
    def __init__(self, error_code: str, message: str, details: dict[str, Any] | None = None):
        super().__init__(message)
        self.error_code = error_code
        self.message = message
        self.details = details or {}


class RuntimeAdapterError(RuntimeDecodeError):
    pass


RuntimeFactory = Callable[..., RuntimeRunner]


def adapter_ref_from_env() -> str | None:
    value = os.environ.get(CNVSRC_RUNTIME_ADAPTER_ENV)
    if value is None or not value.strip():
        return None
    return value.strip()


def resolve_runtime_factory(adapter_ref: str) -> RuntimeFactory:
    module_name, separator, factory_name = adapter_ref.partition(":")
    if not module_name or not separator or not factory_name:
        raise RuntimeAdapterError(
            "RUNTIME_IMPORT_FAILED",
            f"{CNVSRC_RUNTIME_ADAPTER_ENV} must use module:function syntax",
            {"runtime_adapter": {"configured": True, "adapter_ref": adapter_ref}},
        )
    try:
        module = importlib.import_module(module_name)
    except Exception as exc:  # noqa: BLE001 - external model imports may fail natively.
        raise RuntimeAdapterError(
            "RUNTIME_IMPORT_FAILED",
            f"CNVSRC2025 runtime adapter import failed: {exc.__class__.__name__}",
            {
                "runtime_adapter": {
                    "configured": True,
                    "adapter_ref": adapter_ref,
                    "import_error": exc.__class__.__name__,
                }
            },
        ) from exc
    factory = getattr(module, factory_name, None)
    if not callable(factory):
        raise RuntimeAdapterError(
            "RUNTIME_IMPORT_FAILED",
            f"CNVSRC2025 runtime adapter factory is not callable: {adapter_ref}",
            {"runtime_adapter": {"configured": True, "adapter_ref": adapter_ref}},
        )
    return factory


def verify_runtime_adapter(*, checkpoint_path: Path, device: str, adapter_ref: str | None = None) -> dict[str, Any]:
    selected_ref = adapter_ref or adapter_ref_from_env()
    report: dict[str, Any] = {
        "required": True,
        "configured": selected_ref is not None,
        "env_var": CNVSRC_RUNTIME_ADAPTER_ENV,
        "adapter_ref": selected_ref,
        "factory_resolved": False,
        "runner_loaded": False,
        "runtime_id": None,
    }
    if selected_ref is None:
        raise RuntimeAdapterError(
            "RUNTIME_IMPORT_FAILED",
            f"{CNVSRC_RUNTIME_ADAPTER_ENV} is not configured; real CNVSRC2025 inference cannot run",
            {"runtime_adapter": report},
        )

    runner = load_runtime_runner(adapter_ref=selected_ref, checkpoint_path=checkpoint_path, device=device)
    report["factory_resolved"] = True
    report["runner_loaded"] = True
    report["runtime_id"] = runner.runtime_id
    return report


def load_runtime_runner(*, adapter_ref: str | None, checkpoint_path: Path, device: str) -> RuntimeRunner:
    selected_ref = adapter_ref or adapter_ref_from_env()
    if selected_ref is None:
        raise RuntimeAdapterError(
            "RUNTIME_IMPORT_FAILED",
            f"{CNVSRC_RUNTIME_ADAPTER_ENV} is not configured; real CNVSRC2025 inference cannot run",
        )
    factory = resolve_runtime_factory(selected_ref)
    try:
        runner = factory(checkpoint_path=checkpoint_path, device=device)
    except RuntimeAdapterError:
        raise
    except Exception as exc:  # noqa: BLE001 - third-party runtime factories may fail natively.
        raise RuntimeAdapterError(
            "RUNTIME_IMPORT_FAILED",
            f"CNVSRC2025 runtime adapter factory failed: {exc.__class__.__name__}",
            {
                "runtime_adapter": {
                    "configured": True,
                    "adapter_ref": selected_ref,
                    "factory_resolved": True,
                    "runner_loaded": False,
                    "factory_error": exc.__class__.__name__,
                }
            },
        ) from exc
    decode = getattr(runner, "decode", None)
    runtime_id = getattr(runner, "runtime_id", None)
    if not callable(decode) or not isinstance(runtime_id, str) or not runtime_id:
        raise RuntimeAdapterError(
            "RUNTIME_IMPORT_FAILED",
            "CNVSRC2025 runtime adapter must return a runner with runtime_id and decode()",
            {
                "runtime_adapter": {
                    "configured": True,
                    "adapter_ref": selected_ref,
                    "factory_resolved": True,
                    "runner_loaded": False,
                }
            },
        )
    return runner
