from __future__ import annotations

import base64
import http.client
import json
import os
import threading
from http import HTTPStatus
from http.server import HTTPServer
from pathlib import Path
from typing import Any
from urllib.error import HTTPError
from urllib.request import ProxyHandler, Request, build_opener, urlopen

import pytest
from jsonschema import Draft202012Validator

from freelip_vsr import sidecar
from freelip_vsr import cnvsrc_runtime


REPO_ROOT = Path(__file__).resolve().parents[2]
SCHEMAS_DIR = REPO_ROOT / "schemas"
TOKEN = "test-token"
NO_PROXY_OPENER = build_opener(ProxyHandler({}))


class CountingBackend:
    def __init__(self) -> None:
        self.decode_calls = 0

    def status(self) -> dict[str, Any]:
        return {
            "schema_version": "1.0.0",
            "model_id": "cnvsrc2025",
            "runtime_id": "cnvsrc2025-fixture-cpu",
            "device": "cpu",
            "ready": False,
            "status": "CHECKPOINT_MISSING",
            "error_code": "CHECKPOINT_MISSING",
            "backend": "fixture",
        }

    def decode(self, request_payload: dict[str, Any]) -> dict[str, Any]:
        self.decode_calls += 1
        now = 1_777_339_204_050
        return {
            "schema_version": "1.0.0",
            "request_id": request_payload["request_id"],
            "session_id": request_payload["session_id"],
            "model": {
                "model_id": "cnvsrc2025",
                "runtime_id": "cnvsrc2025-fixture-cpu",
                "device": "cpu",
            },
            "candidates": [
                {
                    "schema_version": "1.0.0",
                    "rank": 1,
                    "text": "帮我总结这段文字",
                    "score": 0.82,
                    "source": "cnvsrc2025",
                    "is_auto_insert_eligible": True,
                }
            ],
            "quality_flags": request_payload["quality_flags"],
            "timing_ms": {
                "roi_received_to_first_candidate": 1,
                "roi_received_to_final": 2,
            },
            "created_at_ms": now,
        }


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def validate_candidate_response(payload: dict[str, Any]) -> None:
    schema = load_json(SCHEMAS_DIR / "candidate_response.schema.json")
    errors = sorted(Draft202012Validator(schema).iter_errors(payload), key=str)
    assert errors == []


def serve(server: HTTPServer) -> tuple[str, threading.Thread]:
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    host, port = server.server_address[:2]
    return f"http://{host}:{port}", thread


def request_json(
    method: str,
    url: str,
    payload: dict[str, Any] | None = None,
    token: str | None = TOKEN,
    *,
    x_token: str | None = None,
) -> tuple[int, dict[str, Any] | str]:
    body = None if payload is None else json.dumps(payload).encode("utf-8")
    headers = {}
    if payload is not None:
        headers["Content-Type"] = "application/json"
    if token is not None:
        headers["Authorization"] = f"Bearer {token}"
    if x_token is not None:
        headers["X-FreeLip-Token"] = x_token
    request = Request(url, data=body, headers=headers, method=method)
    try:
        with NO_PROXY_OPENER.open(request, timeout=3) as response:
            raw = response.read().decode("utf-8")
            content_type = response.headers.get("Content-Type", "")
            if "application/json" in content_type:
                return response.status, json.loads(raw)
            return response.status, raw
    except HTTPError as exc:
        raw = exc.read().decode("utf-8")
        return exc.code, json.loads(raw)


def roi_request() -> dict[str, Any]:
    payload = load_json(SCHEMAS_DIR / "fixtures/roi_request.valid.json")
    payload["source"]["kind"] = "fixture"
    payload["roi"]["local_ref"] = "local://fixture/ai-prompt-short-0001"
    return payload


def test_health_is_public_but_status_requires_token() -> None:
    backend = CountingBackend()
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, backend=backend)
    base_url, thread = serve(server)
    try:
        health_status, health_body = request_json("GET", f"{base_url}/health", token=None)
        missing_status, missing_body = request_json("GET", f"{base_url}/status", token=None)
        wrong_status, wrong_body = request_json("GET", f"{base_url}/status", token="wrong")
        ok_status, ok_body = request_json("GET", f"{base_url}/status")
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    assert health_status == 200
    assert health_body == "ok\n"
    assert missing_status == 401
    assert isinstance(missing_body, dict)
    assert missing_body["error_code"] == "AUTH_MISSING"
    assert wrong_status == 403
    assert isinstance(wrong_body, dict)
    assert wrong_body["error_code"] == "AUTH_REJECTED"
    assert ok_status == 200
    assert isinstance(ok_body, dict)
    assert ok_body["model_id"] == "cnvsrc2025"
    assert backend.decode_calls == 0


def test_decode_requires_token_before_backend_inference() -> None:
    backend = CountingBackend()
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, backend=backend)
    base_url, thread = serve(server)
    payload = roi_request()
    try:
        missing_status, missing_body = request_json("POST", f"{base_url}/decode", payload, token=None)
        wrong_status, wrong_body = request_json("POST", f"{base_url}/decode", payload, token="wrong")
        ok_status, ok_body = request_json("POST", f"{base_url}/decode", payload)
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    assert missing_status == 401
    assert isinstance(missing_body, dict)
    assert missing_body["error_code"] == "AUTH_MISSING"
    assert wrong_status == 403
    assert isinstance(wrong_body, dict)
    assert wrong_body["error_code"] == "AUTH_REJECTED"
    assert backend.decode_calls == 1
    assert ok_status == 200
    assert isinstance(ok_body, dict)
    validate_candidate_response(ok_body)
    assert ok_body["candidates"][0]["source"] == "cnvsrc2025"


def test_session_and_stream_endpoints_are_protected() -> None:
    backend = CountingBackend()
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, backend=backend)
    base_url, thread = serve(server)
    try:
        missing_status, missing_body = request_json(
            "POST", f"{base_url}/sessions", {"session_id": "session-1"}, token=None
        )
        session_status, session_body = request_json(
            "POST", f"{base_url}/sessions", {"session_id": "session-1"}
        )
        stream_status, stream_body = request_json(
            "POST", f"{base_url}/stream/start", {"session_id": "session-1"}
        )
        stop_status, stop_body = request_json(
            "POST", f"{base_url}/stream/stop", {"session_id": "session-1"}
        )
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    assert missing_status == 401
    assert isinstance(missing_body, dict)
    assert missing_body["error_code"] == "AUTH_MISSING"
    assert session_status == 200
    assert isinstance(session_body, dict)
    assert session_body["status"] == "open"
    assert stream_status == 200
    assert isinstance(stream_body, dict)
    assert stream_body["status"] == "started"
    assert stop_status == 200
    assert isinstance(stop_body, dict)
    assert stop_body["status"] == "stopped"


def test_model_status_alias_and_x_freelip_token_header_work() -> None:
    backend = CountingBackend()
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, backend=backend)
    base_url, thread = serve(server)
    try:
        status, body = request_json("GET", f"{base_url}/model/status", token=None, x_token=TOKEN)
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    assert status == 200
    assert isinstance(body, dict)
    assert body["model_id"] == "cnvsrc2025"


def test_delete_session_validates_session_id() -> None:
    backend = CountingBackend()
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, backend=backend)
    base_url, thread = serve(server)
    oversized = "s" * 129
    try:
        ok_status, ok_body = request_json("DELETE", f"{base_url}/sessions/session-1")
        missing_status, missing_body = request_json("DELETE", f"{base_url}/sessions/")
        oversized_status, oversized_body = request_json("DELETE", f"{base_url}/sessions/{oversized}")
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    assert ok_status == 200
    assert isinstance(ok_body, dict)
    assert ok_body["status"] == "closed"
    assert missing_status == 400
    assert isinstance(missing_body, dict)
    assert missing_body["error_code"] == "INVALID_REQUEST"
    assert oversized_status == 400
    assert isinstance(oversized_body, dict)
    assert oversized_body["error_code"] == "INVALID_REQUEST"


def test_oversized_body_and_invalid_roi_extra_fields_rejected() -> None:
    backend = CountingBackend()
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, backend=backend)
    base_url, thread = serve(server)
    invalid_payload = roi_request()
    invalid_payload["roi"]["raw_media"] = "not allowed"
    try:
        host, port = server.server_address[:2]
        assert isinstance(host, str)
        assert isinstance(port, int)
        conn = http.client.HTTPConnection(host, port, timeout=3)
        conn.putrequest("POST", "/decode")
        conn.putheader("Authorization", f"Bearer {TOKEN}")
        conn.putheader("Content-Type", "application/json")
        conn.putheader("Content-Length", str(sidecar.MAX_REQUEST_BYTES + 1))
        conn.endheaders()
        response = conn.getresponse()
        oversized_status = response.status
        oversized_body = json.loads(response.read().decode("utf-8"))
        conn.close()
        invalid_status, invalid_body = request_json("POST", f"{base_url}/decode", invalid_payload)
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    assert oversized_status == 413
    assert oversized_body["error_code"] == "INVALID_REQUEST"
    assert invalid_status == 400
    assert isinstance(invalid_body, dict)
    assert invalid_body["error_code"] == "INVALID_REQUEST"
    assert backend.decode_calls == 0


def test_default_missing_artifact_backend_fails_closed(monkeypatch: pytest.MonkeyPatch) -> None:
    report = {"ready": False, "error_code": "CHECKPOINT_MISSING", "status": "CHECKPOINT_MISSING", "exit_code": 2}
    monkeypatch.setattr(sidecar, "readiness_report", lambda model_id, device: report)
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN)
    base_url, thread = serve(server)
    try:
        status, body = request_json("POST", f"{base_url}/decode", roi_request())
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()
    assert status == 503
    assert isinstance(body, dict)
    assert body["error_code"] == "MODEL_UNAVAILABLE"
    assert body["readiness_error_code"] == "CHECKPOINT_MISSING"
    assert body["candidates"] == []
    assert "Traceback" not in json.dumps(body)
    schema = load_json(SCHEMAS_DIR / "candidate_response.schema.json")
    errors = sorted(Draft202012Validator(schema).iter_errors(body), key=str)
    assert errors


def test_ready_readiness_selects_cnvsrc_backend(monkeypatch: pytest.MonkeyPatch) -> None:
    report = {
        "ready": True,
        "error_code": None,
        "status": "MODEL_READY",
        "exit_code": 0,
        "checkpoint": {"path": "/models/cnvsrc2025.pth"},
    }

    class FakeRunner:
        runtime_id = "cnvsrc2025-official-fake-cpu"

        def decode(self, request_payload: dict[str, Any]) -> list[cnvsrc_runtime.RuntimeCandidate]:
            return [cnvsrc_runtime.RuntimeCandidate(text="打开设置", score=0.91)]

    monkeypatch.setattr(sidecar, "readiness_report", lambda model_id, device: report)
    monkeypatch.setattr(
        sidecar.cnvsrc_runtime,
        "load_runtime_runner",
        lambda *, adapter_ref, checkpoint_path, device: FakeRunner(),
    )

    backend = sidecar.build_backend(model_id="cnvsrc2025", device="cpu")

    assert backend.status()["backend"] == "cnvsrc2025"
    assert backend.status()["ready"] is True


def test_ready_cnvsrc_backend_uses_runtime_runner(monkeypatch: pytest.MonkeyPatch) -> None:
    calls: list[dict[str, Any]] = []
    report = {
        "ready": True,
        "error_code": None,
        "status": "MODEL_READY",
        "exit_code": 0,
        "checkpoint": {"path": "/models/cnvsrc2025.pth"},
    }

    class FakeRunner:
        runtime_id = "cnvsrc2025-official-fake-cpu"

        def decode(self, request_payload: dict[str, Any]) -> list[cnvsrc_runtime.RuntimeCandidate]:
            calls.append(request_payload)
            return [
                cnvsrc_runtime.RuntimeCandidate(text="打开设置", score=0.91),
                cnvsrc_runtime.RuntimeCandidate(text="打开射灯", score=0.22),
            ]

    monkeypatch.setattr(sidecar, "readiness_report", lambda model_id, device: report)
    monkeypatch.setattr(
        sidecar.cnvsrc_runtime,
        "load_runtime_runner",
        lambda *, adapter_ref, checkpoint_path, device: FakeRunner(),
    )

    backend = sidecar.build_backend(model_id="cnvsrc2025", device="cpu")
    response = backend.decode(roi_request())

    validate_candidate_response(response)
    assert calls == [roi_request()]
    assert response["model"]["runtime_id"] == "cnvsrc2025-official-fake-cpu"
    assert [candidate["text"] for candidate in response["candidates"]] == ["打开设置", "打开射灯"]
    assert response["candidates"][0]["source"] == "cnvsrc2025"
    assert response["candidates"][0]["is_auto_insert_eligible"] is True


def test_runtime_failure_is_returned_without_traceback(monkeypatch: pytest.MonkeyPatch) -> None:
    report = {
        "ready": True,
        "error_code": None,
        "status": "MODEL_READY",
        "exit_code": 0,
        "checkpoint": {"path": "/models/cnvsrc2025.pth"},
    }

    class FailingRunner:
        runtime_id = "cnvsrc2025-official-failing-cpu"

        def decode(self, request_payload: dict[str, Any]) -> list[cnvsrc_runtime.RuntimeCandidate]:
            raise cnvsrc_runtime.RuntimeDecodeError("INFERENCE_FAILED", "shape mismatch")

    monkeypatch.setattr(sidecar, "readiness_report", lambda model_id, device: report)
    monkeypatch.setattr(
        sidecar.cnvsrc_runtime,
        "load_runtime_runner",
        lambda *, adapter_ref, checkpoint_path, device: FailingRunner(),
    )

    backend = sidecar.build_backend(model_id="cnvsrc2025", device="cpu")

    with pytest.raises(sidecar.SidecarError) as exc_info:
        backend.decode(roi_request())

    assert exc_info.value.status == HTTPStatus.SERVICE_UNAVAILABLE
    assert exc_info.value.error_code == "INFERENCE_FAILED"
    assert exc_info.value.details["candidates"] == []
    assert "Traceback" not in str(exc_info.value.details)


def test_runtime_factory_failure_selects_unavailable_backend(monkeypatch: pytest.MonkeyPatch) -> None:
    report = {
        "ready": True,
        "error_code": None,
        "status": "MODEL_READY",
        "exit_code": 0,
        "checkpoint": {"path": "/models/cnvsrc2025.pth"},
    }

    def fail_load(*, adapter_ref: str | None, checkpoint_path: Path, device: str) -> object:
        raise RuntimeError("adapter exploded while loading model")

    monkeypatch.setattr(sidecar, "readiness_report", lambda model_id, device: report)
    monkeypatch.setattr(sidecar.cnvsrc_runtime, "load_runtime_runner", fail_load)

    backend = sidecar.build_backend(model_id="cnvsrc2025", device="cpu")
    status = backend.status()

    assert status["ready"] is False
    assert status["error_code"] == "RUNTIME_IMPORT_FAILED"
    with pytest.raises(sidecar.SidecarError) as exc_info:
        backend.decode(roi_request())
    assert exc_info.value.error_code == "MODEL_UNAVAILABLE"
    assert exc_info.value.details["readiness_error_code"] == "RUNTIME_IMPORT_FAILED"


def test_unready_readiness_selects_unavailable_backend_by_default(monkeypatch: pytest.MonkeyPatch) -> None:
    report = {"ready": False, "error_code": "CHECKPOINT_MISSING", "status": "CHECKPOINT_MISSING", "exit_code": 2}
    monkeypatch.setattr(sidecar, "readiness_report", lambda model_id, device: report)
    backend = sidecar.build_backend(model_id="cnvsrc2025", device="cpu")
    status = backend.status()
    assert status["backend"] == "cnvsrc2025"
    assert status["ready"] is False
    assert status["fallback_active"] is False
    with pytest.raises(sidecar.SidecarError) as exc_info:
        backend.decode(roi_request())
    assert exc_info.value.status == HTTPStatus.SERVICE_UNAVAILABLE
    assert exc_info.value.error_code == "MODEL_UNAVAILABLE"
    assert exc_info.value.details["readiness_error_code"] == "CHECKPOINT_MISSING"
    assert exc_info.value.details["candidates"] == []


def test_unready_readiness_fixture_mode_returns_deterministic_candidates(monkeypatch: pytest.MonkeyPatch) -> None:
    report = {"ready": False, "error_code": "CHECKPOINT_MISSING", "status": "CHECKPOINT_MISSING", "exit_code": 2}
    monkeypatch.setattr(sidecar, "readiness_report", lambda model_id, device: report)
    backend = sidecar.build_backend(model_id="cnvsrc2025", device="cpu", fixture_mode=True)
    status = backend.status()
    assert status["backend"] == "fixture"
    assert status["ready"] is False
    assert status["status"] == "FIXTURE_MODE"
    assert status["fixture_mode"] is True
    assert status["readiness_status"] == "CHECKPOINT_MISSING"
    assert status["fallback_active"] is True
    response = backend.decode(roi_request())
    validate_candidate_response(response)
    assert response["model"]["runtime_id"] == "cnvsrc2025-fixture-cpu-checkpoint-gated"
    assert response["candidates"][0]["source"] == "cnvsrc2025"


def test_ready_readiness_fixture_mode_still_reports_fixture_backend(monkeypatch: pytest.MonkeyPatch) -> None:
    report = {"ready": True, "error_code": None, "status": "MODEL_READY", "exit_code": 0}
    monkeypatch.setattr(sidecar, "readiness_report", lambda model_id, device: report)
    backend = sidecar.build_backend(model_id="cnvsrc2025", device="cpu", fixture_mode=True)
    status = backend.status()

    assert status["backend"] == "fixture"
    assert status["ready"] is False
    assert status["status"] == "FIXTURE_MODE"
    assert status["readiness_status"] == "MODEL_READY"
    assert status["fixture_mode"] is True
    assert status["fallback_active"] is True


def test_auth_rejections_are_logged_without_token_values() -> None:
    backend = CountingBackend()
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, backend=backend)
    base_url, thread = serve(server)
    try:
        request_json("POST", f"{base_url}/decode", roi_request(), token=None)
        request_json("POST", f"{base_url}/decode", roi_request(), token="wrong-token-value")
    finally:
        events = list(server.state.auth_events)
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    assert [event["error_code"] for event in events] == ["AUTH_MISSING", "AUTH_REJECTED"]
    serialized = json.dumps(events)
    assert TOKEN not in serialized
    assert "wrong-token-value" not in serialized
    assert backend.decode_calls == 0

    evidence_dir = REPO_ROOT / ".sisyphus/evidence"
    evidence_dir.mkdir(parents=True, exist_ok=True)
    (evidence_dir / "task-4-auth.json").write_text(
        json.dumps(
            {
                "events": events,
                "backend_decode_calls": backend.decode_calls,
                "token_values_redacted": TOKEN not in serialized and "wrong-token-value" not in serialized,
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )


def test_websocket_stream_endpoint_requires_token_and_accepts_upgrade() -> None:
    backend = CountingBackend()
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, backend=backend)
    _base_url, thread = serve(server)
    host, port = server.server_address[:2]
    assert isinstance(host, str)
    assert isinstance(port, int)
    key = base64.b64encode(os.urandom(16)).decode("ascii")
    headers = {
        "Connection": "Upgrade",
        "Upgrade": "websocket",
        "Sec-WebSocket-Key": key,
        "Sec-WebSocket-Version": "13",
    }
    try:
        missing_connection = http.client.HTTPConnection(host, port, timeout=3)
        missing_connection.request("GET", "/stream/ws", headers=headers)
        missing_response = missing_connection.getresponse()
        missing_status = missing_response.status
        missing_body = json.loads(missing_response.read().decode("utf-8"))
        missing_connection.close()
        ok_connection = http.client.HTTPConnection(host, port, timeout=3)
        ok_connection.request("GET", "/stream/ws", headers={**headers, "Authorization": f"Bearer {TOKEN}"})
        ok_response = ok_connection.getresponse()
        ok_status = ok_response.status
        accept_header = ok_response.getheader("Sec-WebSocket-Accept")
        ok_connection.close()
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()
    assert missing_status == 401
    assert missing_body["error_code"] == "AUTH_MISSING"
    assert ok_status == 101
    assert accept_header


def test_non_loopback_bind_host_is_rejected_before_serving() -> None:
    with pytest.raises(ValueError, match=r"127\.0\.0\.1"):
        sidecar.validate_bind_host("192.168.1.25")
    with pytest.raises(ValueError, match=r"127\.0\.0\.1"):
        sidecar.validate_bind_host("localhost")
    with pytest.raises(ValueError, match=r"127\.0\.0\.1"):
        sidecar.validate_bind_host("::1")
