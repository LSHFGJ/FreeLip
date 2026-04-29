from __future__ import annotations

import json
import sys
import types
from pathlib import Path

import pytest

import freelip_vsr.check_model as check_model
from freelip_vsr.check_model import main
from freelip_vsr.model_registry import ERROR_EXIT_CODES, get_model_config


def test_cnvsrc2025_registry_records_verified_checkpoint_source() -> None:
    config = get_model_config("cnvsrc2025")

    assert config.checkpoint.filename == "model_avg_cncvs_2_3_cnvsrc.pth"
    assert config.checkpoint.expected_size == 1_137_500_697
    assert (
        config.checkpoint.expected_sha256
        == "577cd9558eea111683a406bc25d69c7161cdb79534c2273fc0d0f044c356231c"
    )
    assert config.source.huggingface_repo == "ReflectionL/CNVSRC2025Baseline"
    assert config.source.modelscope_repo == "PaintedVeil/CNVSRC2025Baseline"
    assert "CNVSRC2025/tree/main/VSR" in config.source.code_url


def test_mavsr2025_registry_exists_as_manual_fallback_gate() -> None:
    config = get_model_config("mavsr2025")

    assert config.model_id == "mavsr2025"
    assert config.manual_fallback_only is True
    assert "MAVSR2025-Track1" in config.source.code_url


def test_missing_checkpoint_writes_json_without_stack_trace(
    tmp_path: Path, capsys
) -> None:
    report_path = tmp_path / "missing.json"
    checkpoint_path = tmp_path / "does-not-exist.pth"

    exit_code = main(
        [
            "--model",
            "cnvsrc2025",
            "--device",
            "cuda",
            "--checkpoint",
            str(checkpoint_path),
            "--json",
            str(report_path),
        ]
    )

    output = capsys.readouterr()
    report = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == ERROR_EXIT_CODES["CHECKPOINT_MISSING"]
    assert report["ready"] is False
    assert report["model_id"] == "cnvsrc2025"
    assert report["device"] == "cuda"
    assert report["error_code"] == "CHECKPOINT_MISSING"
    assert report["checkpoint"]["path"] == str(checkpoint_path)
    assert report["source_verification"]["checkpoint_source_verified"] is False
    assert "Traceback" not in output.out
    assert "Traceback" not in output.err


def test_require_data_classifies_missing_dataset_after_checkpoint_verifies(
    tmp_path: Path, capsys
) -> None:
    checkpoint_path = tmp_path / "tiny.pth"
    checkpoint_path.write_bytes(b"ok")
    report_path = tmp_path / "data-missing.json"

    exit_code = main(
        [
            "--model",
            "cnvsrc2025",
            "--device",
            "cpu",
            "--checkpoint",
            str(checkpoint_path),
            "--checkpoint-sha256",
            "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df",
            "--checkpoint-size",
            "2",
            "--require-data",
            "--data-root",
            str(tmp_path / "missing-data"),
            "--json",
            str(report_path),
        ]
    )

    output = capsys.readouterr()
    report = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == ERROR_EXIT_CODES["DATA_UNAVAILABLE"]
    assert report["error_code"] == "DATA_UNAVAILABLE"
    assert report["source_verification"]["checkpoint_source_verified"] is True
    assert "Traceback" not in output.out
    assert "Traceback" not in output.err


def test_torch_native_import_failure_is_classified_without_traceback(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys
) -> None:
    checkpoint_path = tmp_path / "tiny.pth"
    checkpoint_path.write_bytes(b"ok")
    report_path = tmp_path / "runtime-import-failed.json"

    def fail_import(module_name: str) -> object:
        assert module_name == "torch"
        raise OSError("libtorch_cuda.so: cannot open shared object file")

    monkeypatch.setattr(check_model.importlib, "import_module", fail_import)

    exit_code = main(
        [
            "--model",
            "cnvsrc2025",
            "--device",
            "cpu",
            "--checkpoint",
            str(checkpoint_path),
            "--checkpoint-sha256",
            "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df",
            "--checkpoint-size",
            "2",
            "--json",
            str(report_path),
        ]
    )

    output = capsys.readouterr()
    report = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == ERROR_EXIT_CODES["RUNTIME_IMPORT_FAILED"]
    assert report["error_code"] == "RUNTIME_IMPORT_FAILED"
    assert report["source_verification"]["checkpoint_source_verified"] is True
    assert "Traceback" not in output.out
    assert "Traceback" not in output.err


def test_cnvsrc_requires_runtime_adapter_after_checkpoint_and_torch_pass(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys
) -> None:
    checkpoint_path = tmp_path / "tiny.pth"
    checkpoint_path.write_bytes(b"ok")
    report_path = tmp_path / "runtime-adapter-missing.json"
    fake_torch = types.SimpleNamespace(
        __version__="fake",
        version=types.SimpleNamespace(cuda=None),
        cuda=types.SimpleNamespace(is_available=lambda: False),
    )
    monkeypatch.setattr(check_model.importlib, "import_module", lambda _: fake_torch)
    monkeypatch.delenv("FREELIP_CNVSRC2025_RUNTIME_ADAPTER", raising=False)

    exit_code = main(
        [
            "--model",
            "cnvsrc2025",
            "--device",
            "cpu",
            "--checkpoint",
            str(checkpoint_path),
            "--checkpoint-sha256",
            "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df",
            "--checkpoint-size",
            "2",
            "--json",
            str(report_path),
        ]
    )

    output = capsys.readouterr()
    report = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == ERROR_EXIT_CODES["RUNTIME_IMPORT_FAILED"]
    assert report["ready"] is False
    assert report["error_code"] == "RUNTIME_IMPORT_FAILED"
    assert report["runtime"]["checkpoint_deserialized"] is False
    assert report["runtime_adapter"]["configured"] is False
    assert "FREELIP_CNVSRC2025_RUNTIME_ADAPTER" in report["message"]
    assert "Traceback" not in output.out
    assert "Traceback" not in output.err


def test_cnvsrc_runtime_adapter_factory_failure_blocks_readiness(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys
) -> None:
    checkpoint_path = tmp_path / "tiny.pth"
    checkpoint_path.write_bytes(b"ok")
    report_path = tmp_path / "runtime-adapter-factory-failed.json"
    fake_torch = types.SimpleNamespace(
        __version__="fake",
        version=types.SimpleNamespace(cuda=None),
        cuda=types.SimpleNamespace(is_available=lambda: False),
    )
    fake_module = types.ModuleType("fake_cnvsrc_adapter")

    def create_runner(*, checkpoint_path: Path, device: str) -> object:
        raise RuntimeError("adapter failed while loading official baseline")

    fake_module.create_runner = create_runner
    monkeypatch.setattr(check_model.importlib, "import_module", lambda name: fake_torch if name == "torch" else fake_module)
    monkeypatch.setitem(sys.modules, "fake_cnvsrc_adapter", fake_module)
    monkeypatch.setenv("FREELIP_CNVSRC2025_RUNTIME_ADAPTER", "fake_cnvsrc_adapter:create_runner")

    exit_code = main(
        [
            "--model",
            "cnvsrc2025",
            "--device",
            "cpu",
            "--checkpoint",
            str(checkpoint_path),
            "--checkpoint-sha256",
            "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df",
            "--checkpoint-size",
            "2",
            "--json",
            str(report_path),
        ]
    )

    output = capsys.readouterr()
    report = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == ERROR_EXIT_CODES["RUNTIME_IMPORT_FAILED"]
    assert report["ready"] is False
    assert report["error_code"] == "RUNTIME_IMPORT_FAILED"
    assert report["runtime_adapter"]["configured"] is True
    assert report["runtime_adapter"]["factory_resolved"] is True
    assert report["runtime_adapter"]["runner_loaded"] is False
    assert "RuntimeError" in report["message"]
    assert "adapter failed while loading official baseline" not in report["message"]
    assert "Traceback" not in output.out
    assert "Traceback" not in output.err


def test_cnvsrc_runtime_adapter_bad_runner_blocks_readiness(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys
) -> None:
    checkpoint_path = tmp_path / "tiny.pth"
    checkpoint_path.write_bytes(b"ok")
    report_path = tmp_path / "runtime-adapter-bad-runner.json"
    fake_torch = types.SimpleNamespace(
        __version__="fake",
        version=types.SimpleNamespace(cuda=None),
        cuda=types.SimpleNamespace(is_available=lambda: False),
    )
    fake_module = types.ModuleType("bad_cnvsrc_adapter")
    fake_module.create_runner = lambda *, checkpoint_path, device: object()
    monkeypatch.setattr(check_model.importlib, "import_module", lambda name: fake_torch if name == "torch" else fake_module)
    monkeypatch.setitem(sys.modules, "bad_cnvsrc_adapter", fake_module)
    monkeypatch.setenv("FREELIP_CNVSRC2025_RUNTIME_ADAPTER", "bad_cnvsrc_adapter:create_runner")

    exit_code = main(
        [
            "--model",
            "cnvsrc2025",
            "--device",
            "cpu",
            "--checkpoint",
            str(checkpoint_path),
            "--checkpoint-sha256",
            "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df",
            "--checkpoint-size",
            "2",
            "--json",
            str(report_path),
        ]
    )

    output = capsys.readouterr()
    report = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == ERROR_EXIT_CODES["RUNTIME_IMPORT_FAILED"]
    assert report["ready"] is False
    assert report["error_code"] == "RUNTIME_IMPORT_FAILED"
    assert report["runtime_adapter"]["runner_loaded"] is False
    assert "must return a runner" in report["message"]
    assert "Traceback" not in output.out
    assert "Traceback" not in output.err


def test_cuda_probe_failure_is_classified_as_cuda_incompatible(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys
) -> None:
    checkpoint_path = tmp_path / "tiny.pth"
    checkpoint_path.write_bytes(b"ok")
    report_path = tmp_path / "cuda-incompatible.json"

    fake_torch = types.SimpleNamespace(
        __version__="fake",
        version=types.SimpleNamespace(cuda="13.1"),
        cuda=types.SimpleNamespace(
            is_available=lambda: True,
            get_device_name=lambda _: "Fake CUDA GPU",
        ),
        empty=lambda *_args, **_kwargs: (_ for _ in ()).throw(RuntimeError("CUDA driver error")),
    )
    monkeypatch.setattr(check_model.importlib, "import_module", lambda _: fake_torch)

    exit_code = main(
        [
            "--model",
            "cnvsrc2025",
            "--device",
            "cuda",
            "--checkpoint",
            str(checkpoint_path),
            "--checkpoint-sha256",
            "2689367b205c16ce32ed4200942b8b8b1e262dfc70d9bc9fbc77c49699a4f1df",
            "--checkpoint-size",
            "2",
            "--json",
            str(report_path),
        ]
    )

    output = capsys.readouterr()
    report = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == ERROR_EXIT_CODES["CUDA_INCOMPATIBLE"]
    assert report["error_code"] == "CUDA_INCOMPATIBLE"
    assert report["runtime"]["cuda_available"] is True
    assert "Traceback" not in output.out
    assert "Traceback" not in output.err


def test_mavsr_checkpoint_without_known_hash_can_reach_runtime_gate(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys
) -> None:
    checkpoint_path = tmp_path / "mavsr.pth"
    checkpoint_path.write_bytes(b"manual fallback")
    report_path = tmp_path / "mavsr-ready.json"

    fake_torch = types.SimpleNamespace(
        __version__="fake",
        version=types.SimpleNamespace(cuda=None),
        cuda=types.SimpleNamespace(is_available=lambda: False),
    )
    monkeypatch.setattr(check_model.importlib, "import_module", lambda _: fake_torch)

    exit_code = main(
        [
            "--model",
            "mavsr2025",
            "--device",
            "cpu",
            "--checkpoint",
            str(checkpoint_path),
            "--json",
            str(report_path),
        ]
    )

    output = capsys.readouterr()
    report = json.loads(report_path.read_text(encoding="utf-8"))

    assert exit_code == 0
    assert report["ready"] is True
    assert report["source_verification"]["checkpoint_source_verified"] is False
    assert report["source_verification"]["checkpoint_integrity_verified"] is False
    assert "MODEL_READY mavsr2025 cpu" in output.out
