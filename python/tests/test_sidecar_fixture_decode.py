from __future__ import annotations

import json
import threading
from http.server import HTTPServer
from pathlib import Path
from typing import Any
from urllib.request import Request, urlopen

from jsonschema import Draft202012Validator

from freelip_vsr import sidecar


REPO_ROOT = Path(__file__).resolve().parents[2]
SCHEMAS_DIR = REPO_ROOT / "schemas"
EVIDENCE_DIR = REPO_ROOT / ".sisyphus/evidence"
TOKEN = "test-token"


def load_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def serve(server: HTTPServer) -> tuple[str, threading.Thread]:
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    host, port = server.server_address[:2]
    return f"http://{host}:{port}", thread


def post_json(url: str, payload: dict[str, Any]) -> dict[str, Any]:
    request = Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Authorization": f"Bearer {TOKEN}", "Content-Type": "application/json"},
        method="POST",
    )
    with urlopen(request, timeout=5) as response:
        assert response.status == 200
        return json.loads(response.read().decode("utf-8"))


def assert_valid_candidate_response(payload: dict[str, Any]) -> None:
    schema = load_json(SCHEMAS_DIR / "candidate_response.schema.json")
    errors = sorted(Draft202012Validator(schema).iter_errors(payload), key=str)
    assert errors == []


def roi_fixture_request() -> dict[str, Any]:
    payload = load_json(SCHEMAS_DIR / "fixtures/roi_request.valid.json")
    payload["source"]["kind"] = "fixture"
    payload["request_id"] = "task-4-fixture-decode"
    payload["session_id"] = "task-4-session"
    payload["roi"]["local_ref"] = "local://fixture/ai-prompt-short-0001"
    payload["requested_at_ms"] = 1_777_339_203_100
    return payload


def test_fixture_decode_uses_explicit_fixture_mode_and_writes_evidence() -> None:
    server = sidecar.create_server("127.0.0.1", 0, token=TOKEN, fixture_mode=True)
    base_url, thread = serve(server)
    request_payload = roi_fixture_request()
    try:
        response_payload = post_json(f"{base_url}/decode", request_payload)
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()

    assert_valid_candidate_response(response_payload)
    assert response_payload["request_id"] == request_payload["request_id"]
    assert response_payload["session_id"] == request_payload["session_id"]
    assert response_payload["model"]["model_id"] == "cnvsrc2025"
    assert response_payload["model"]["runtime_id"]
    assert response_payload["candidates"]
    assert response_payload["candidates"][0]["source"] == "cnvsrc2025"
    assert response_payload["quality_flags"] == request_payload["quality_flags"]
    assert response_payload["timing_ms"]["roi_received_to_first_candidate"] >= 0
    assert response_payload["timing_ms"]["roi_received_to_final"] >= response_payload["timing_ms"]["roi_received_to_first_candidate"]

    EVIDENCE_DIR.mkdir(parents=True, exist_ok=True)
    evidence = {
        "command": "PYTHONPATH=<repo>/python pytest python/tests/test_sidecar_fixture_decode.py -q",
        "fixture_mode": True,
        "request": request_payload,
        "response": response_payload,
        "validated_schema": "schemas/candidate_response.schema.json",
    }
    (EVIDENCE_DIR / "task-4-decode.json").write_text(
        json.dumps(evidence, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
