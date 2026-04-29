from __future__ import annotations

import argparse
import base64
import hashlib
import hmac
import json
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Protocol, cast
from urllib.parse import unquote, urlsplit

from . import cnvsrc_runtime
from . import check_model
from .model_registry import ERROR_EXIT_CODES, READY_EXIT_CODE, get_model_config


SCHEMA_VERSION = "1.0.0"
MODEL_ID = "cnvsrc2025"
FIXTURE_RUNTIME_ID = "cnvsrc2025-fixture"
MAX_REQUEST_BYTES = 256 * 1024
ALLOWED_BIND_HOST = "127.0.0.1"
QUALITY_REASON_CODES = {
    "face_not_found",
    "mouth_landmarks_missing",
    "crop_bounds_invalid",
    "blur_too_high",
    "brightness_out_of_range",
    "pose_out_of_range",
    "mouth_occluded",
}
READINESS_ERROR_CODES = set(ERROR_EXIT_CODES)


JsonObject = dict[str, Any]


class SidecarBackend(Protocol):
    def status(self) -> JsonObject:
        ...

    def decode(self, request_payload: JsonObject) -> JsonObject:
        ...


class SidecarError(Exception):
    def __init__(self, status: HTTPStatus, error_code: str, message: str, details: JsonObject | None = None):
        super().__init__(message)
        self.status = status
        self.error_code = error_code
        self.message = message
        self.details = details or {}


@dataclass
class SidecarState:
    token: str
    backend: SidecarBackend
    sessions: set[str] = field(default_factory=set)
    stream_sessions: set[str] = field(default_factory=set)
    auth_events: list[JsonObject] = field(default_factory=list)


class SidecarHTTPServer(ThreadingHTTPServer):
    allow_reuse_address: bool = True

    def __init__(
        self,
        server_address: tuple[str, int],
        token: str,
        backend: SidecarBackend,
    ) -> None:
        super().__init__(server_address, SidecarRequestHandler)
        self.state = SidecarState(token=token, backend=backend)


class SidecarRequestHandler(BaseHTTPRequestHandler):
    server_version: str = "FreeLipVsrSidecar/0.1"
    sys_version: str = ""

    def log_message(self, format: str, *args: object) -> None:
        sys.stderr.write("sidecar %s - %s\n" % (self.address_string(), format % args))

    def do_GET(self) -> None:
        self._handle(self._route_get)

    def do_POST(self) -> None:
        self._handle(self._route_post)

    def do_DELETE(self) -> None:
        self._handle(self._route_delete)

    def _handle(self, route: Callable[[], None]) -> None:
        try:
            route()
        except SidecarError as exc:
            self._send_error(exc.status, exc.error_code, exc.message, exc.details)
        except Exception:
            self._send_error(
                HTTPStatus.INTERNAL_SERVER_ERROR,
                "RUNTIME_IMPORT_FAILED",
                "sidecar request failed",
            )

    def _route_get(self) -> None:
        path = self._path()
        if path == "/health":
            self._send_text(HTTPStatus.OK, "ok\n")
            return
        if path == "/stream/ws":
            self._require_token()
            self._accept_websocket_stream()
            return
        if path in {"/status", "/model/status"}:
            self._require_token()
            self._send_json(HTTPStatus.OK, self._state().backend.status())
            return
        raise SidecarError(HTTPStatus.NOT_FOUND, "NOT_FOUND", "endpoint not found")

    def _route_post(self) -> None:
        path = self._path()
        if path == "/decode":
            self._require_token()
            payload = self._read_json()
            validate_roi_request(payload)
            response = self._state().backend.decode(payload)
            self._send_json(HTTPStatus.OK, response)
            return
        if path == "/sessions":
            self._require_token()
            payload = self._read_json()
            session_id = string_field(payload, "session_id", max_length=128)
            self._state().sessions.add(session_id)
            self._send_json(
                HTTPStatus.OK,
                {
                    "schema_version": SCHEMA_VERSION,
                    "session_id": session_id,
                    "status": "open",
                    "model_id": MODEL_ID,
                },
            )
            return
        if path == "/stream/start":
            self._require_token()
            payload = self._read_json()
            session_id = string_field(payload, "session_id", max_length=128)
            self._state().sessions.add(session_id)
            self._state().stream_sessions.add(session_id)
            self._send_json(
                HTTPStatus.OK,
                {
                    "schema_version": SCHEMA_VERSION,
                    "session_id": session_id,
                    "status": "started",
                },
            )
            return
        if path == "/stream/stop":
            self._require_token()
            payload = self._read_json()
            session_id = string_field(payload, "session_id", max_length=128)
            self._state().stream_sessions.discard(session_id)
            self._send_json(
                HTTPStatus.OK,
                {
                    "schema_version": SCHEMA_VERSION,
                    "session_id": session_id,
                    "status": "stopped",
                },
            )
            return
        raise SidecarError(HTTPStatus.NOT_FOUND, "NOT_FOUND", "endpoint not found")

    def _route_delete(self) -> None:
        path = self._path()
        prefix = "/sessions/"
        if path.startswith(prefix):
            self._require_token()
            session_id = validate_session_id(unquote(path[len(prefix) :]))
            self._state().sessions.discard(session_id)
            self._state().stream_sessions.discard(session_id)
            self._send_json(
                HTTPStatus.OK,
                {
                    "schema_version": SCHEMA_VERSION,
                    "session_id": session_id,
                    "status": "closed",
                },
            )
            return
        raise SidecarError(HTTPStatus.NOT_FOUND, "NOT_FOUND", "endpoint not found")

    def _state(self) -> SidecarState:
        return cast(SidecarHTTPServer, self.server).state

    def _accept_websocket_stream(self) -> None:
        if self.headers.get("Upgrade", "").lower() != "websocket":
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "websocket upgrade required")
        connection = self.headers.get("Connection", "").lower()
        if "upgrade" not in connection:
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "websocket connection upgrade required")
        if self.headers.get("Sec-WebSocket-Version") != "13":
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "unsupported websocket version")
        key = self.headers.get("Sec-WebSocket-Key")
        if not key:
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "missing websocket key")
        try:
            decoded = base64.b64decode(key.encode("ascii"), validate=True)
        except (UnicodeEncodeError, ValueError) as exc:
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "invalid websocket key") from exc
        if len(decoded) != 16:
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "invalid websocket key")
        accept = base64.b64encode(
            hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode("ascii")).digest()
        ).decode("ascii")
        self.send_response(HTTPStatus.SWITCHING_PROTOCOLS.value)
        self.send_header("Upgrade", "websocket")
        self.send_header("Connection", "Upgrade")
        self.send_header("Sec-WebSocket-Accept", accept)
        self.end_headers()

    def _path(self) -> str:
        return urlsplit(self.path).path

    def _require_token(self) -> None:
        supplied = token_from_headers(self.headers.get("Authorization"), self.headers.get("X-FreeLip-Token"))
        if supplied is None:
            self._record_auth_event("AUTH_MISSING")
            raise SidecarError(HTTPStatus.UNAUTHORIZED, "AUTH_MISSING", "missing session token")
        if not hmac.compare_digest(supplied, self._state().token):
            self._record_auth_event("AUTH_REJECTED")
            raise SidecarError(HTTPStatus.FORBIDDEN, "AUTH_REJECTED", "invalid session token")

    def _record_auth_event(self, error_code: str) -> None:
        self._state().auth_events.append({"error_code": error_code, "path": self._path(), "method": self.command})
        sys.stderr.write(f"sidecar auth {error_code} {self.command} {self._path()}\n")

    def _read_json(self) -> JsonObject:
        content_length = self.headers.get("Content-Length")
        if content_length is None:
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_JSON", "missing request body")
        try:
            byte_count = int(content_length)
        except ValueError as exc:
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_JSON", "invalid content length") from exc
        if byte_count > MAX_REQUEST_BYTES:
            raise SidecarError(HTTPStatus.REQUEST_ENTITY_TOO_LARGE, "INVALID_REQUEST", "request body too large")
        raw = self.rfile.read(byte_count)
        try:
            payload = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_JSON", "request body must be JSON") from exc
        if not isinstance(payload, dict):
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "request body must be an object")
        return payload

    def _send_json(self, status: HTTPStatus, payload: JsonObject) -> None:
        body = json.dumps(payload, ensure_ascii=False, sort_keys=True).encode("utf-8") + b"\n"
        self.send_response(status.value)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def _send_text(self, status: HTTPStatus, body: str) -> None:
        encoded = body.encode("utf-8")
        self.send_response(status.value)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(encoded)

    def _send_error(self, status: HTTPStatus, error_code: str, message: str, details: JsonObject | None = None) -> None:
        payload: JsonObject = {
            "schema_version": SCHEMA_VERSION,
            "error_code": error_code,
            "message": message,
        }
        if details:
            payload.update(details)
        self._send_json(status, payload)


def token_from_headers(authorization: str | None, explicit_token: str | None) -> str | None:
    if explicit_token:
        return explicit_token
    if not authorization:
        return None
    scheme, _, value = authorization.partition(" ")
    if scheme.lower() != "bearer" or not value:
        return None
    return value


def string_field(payload: JsonObject, field_name: str, max_length: int) -> str:
    return validate_string_value(payload.get(field_name), field_name, max_length)


def validate_session_id(value: object) -> str:
    return validate_string_value(value, "session_id", 128)


def validate_string_value(value: object, field_name: str, max_length: int) -> str:
    if not isinstance(value, str) or not value or len(value) > max_length:
        raise SidecarError(
            HTTPStatus.BAD_REQUEST,
            "INVALID_REQUEST",
            f"{field_name} must be a non-empty string up to {max_length} characters",
        )
    return value


def validate_bind_host(host: str) -> str:
    if host != ALLOWED_BIND_HOST:
        raise ValueError(f"sidecar host must be exactly {ALLOWED_BIND_HOST}")
    return host


def create_server(
    host: str,
    port: int,
    token: str,
    backend: SidecarBackend | None = None,
    *,
    model_id: str = MODEL_ID,
    device: str = "cpu",
    fixture_mode: bool = False,
) -> SidecarHTTPServer:
    validate_bind_host(host)
    if not token:
        raise ValueError("sidecar token must be non-empty")
    selected_backend = backend if backend is not None else build_backend(model_id=model_id, device=device, fixture_mode=fixture_mode)
    return SidecarHTTPServer((host, port), token=token, backend=selected_backend)


def build_backend(model_id: str = MODEL_ID, device: str = "cpu", *, fixture_mode: bool = False) -> SidecarBackend:
    report = readiness_report(model_id=model_id, device=device)
    schema_device = normalize_device_for_schema(device)
    if fixture_mode:
        return FixtureCnvsrcBackend(readiness_report=report, device=schema_device)
    if report.get("ready") is True:
        try:
            return CnvsrcBackend(
                readiness_report=report,
                device=schema_device,
                runtime_runner=cnvsrc_runtime.load_runtime_runner(
                    adapter_ref=cnvsrc_runtime.adapter_ref_from_env(),
                    checkpoint_path=checkpoint_path_from_report(report),
                    device=schema_device,
                ),
            )
        except Exception as exc:
            failed_report = dict(report)
            if isinstance(exc, cnvsrc_runtime.RuntimeAdapterError):
                error_code = exc.error_code
                message = exc.message
                details = exc.details
            else:
                error_code = "RUNTIME_IMPORT_FAILED"
                message = f"cnvsrc2025 runtime adapter load failed: {exc.__class__.__name__}"
                details = {"runtime_adapter": {"runner_loaded": False, "factory_error": exc.__class__.__name__}}
            failed_report.update(
                {
                    "ready": False,
                    "status": error_code,
                    "error_code": error_code,
                    "message": message,
                }
            )
            failed_report.update(details)
            return UnavailableCnvsrcBackend(readiness_report=failed_report, device=schema_device)
    return UnavailableCnvsrcBackend(readiness_report=report, device=schema_device)


def checkpoint_path_from_report(report: JsonObject) -> Path:
    checkpoint = report.get("checkpoint")
    if isinstance(checkpoint, dict) and isinstance(checkpoint.get("path"), str):
        return Path(checkpoint["path"])
    return Path("")


def readiness_report(model_id: str, device: str) -> JsonObject:
    config = get_model_config(model_id)
    args = argparse.Namespace(
        model=model_id,
        device="cuda" if device == "cuda" else "cpu",
        checkpoint=None,
        data_root=None,
        require_data=False,
        checkpoint_sha256=None,
        checkpoint_size=None,
    )
    checkpoint_path = check_model.path_from_override_env_or_default(
        None, config.checkpoint.env_var, config.checkpoint.default_relative_path
    )
    data_root = check_model.path_from_override_env_or_default(
        None, config.data.env_var, config.data.default_relative_path
    )
    try:
        exit_code, report = check_model.run_check(args)
        report["exit_code"] = exit_code
        return report
    except check_model.ReadinessFailure as exc:
        report = check_model.build_base_report(
            config,
            args.device,
            checkpoint_path,
            data_root,
            args.require_data,
        )
        for key, value in exc.details.items():
            if isinstance(value, dict) and isinstance(report.get(key), dict):
                report[key].update(value)
            else:
                report[key] = value
        exit_code = ERROR_EXIT_CODES.get(exc.error_code, ERROR_EXIT_CODES["RUNTIME_IMPORT_FAILED"])
        report.update(
            {
                "ready": False,
                "status": exc.error_code,
                "error_code": exc.error_code,
                "exit_code": exit_code,
                "message": exc.message,
            }
        )
        return report
    except Exception as exc:
        report = check_model.build_base_report(
            config,
            args.device,
            checkpoint_path,
            data_root,
            args.require_data,
        )
        report.update(
            {
                "ready": False,
                "status": "RUNTIME_IMPORT_FAILED",
                "error_code": "RUNTIME_IMPORT_FAILED",
                "exit_code": ERROR_EXIT_CODES["RUNTIME_IMPORT_FAILED"],
                "message": f"readiness probe failed: {exc.__class__.__name__}",
            }
        )
        return report


def normalize_device_for_schema(device: str) -> str:
    return device if device in {"cpu", "cuda", "directml"} else "cpu"


@dataclass
class CnvsrcBackend:
    readiness_report: JsonObject
    device: str = "cpu"
    runtime_runner: cnvsrc_runtime.RuntimeRunner | None = None

    def status(self) -> JsonObject:
        return {
            "schema_version": SCHEMA_VERSION,
            "model_id": MODEL_ID,
            "runtime_id": self.runtime_id(),
            "device": self.device,
            "ready": True,
            "status": "MODEL_READY",
            "error_code": None,
            "backend": "cnvsrc2025",
            "fallback_active": False,
            "exit_code": self.readiness_report.get("exit_code"),
        }

    def decode(self, request_payload: JsonObject) -> JsonObject:
        if self.runtime_runner is None:
            raise SidecarError(
                HTTPStatus.SERVICE_UNAVAILABLE,
                "RUNTIME_IMPORT_FAILED",
                "cnvsrc2025 runtime runner is not configured",
                {"candidates": []},
            )
        started = time.perf_counter_ns()
        try:
            candidates = list(self.runtime_runner.decode(request_payload))
        except cnvsrc_runtime.RuntimeDecodeError as exc:
            details = {"candidates": []}
            details.update(exc.details)
            raise SidecarError(HTTPStatus.SERVICE_UNAVAILABLE, exc.error_code, exc.message, details) from exc
        except Exception as exc:
            raise SidecarError(
                HTTPStatus.SERVICE_UNAVAILABLE,
                "INFERENCE_FAILED",
                f"cnvsrc2025 inference failed: {exc.__class__.__name__}",
                {"candidates": []},
            ) from exc
        return build_runtime_candidate_response(
            request_payload=request_payload,
            runtime_id=self.runtime_id(),
            device=self.device,
            runtime_candidates=candidates,
            started_ns=started,
        )

    def runtime_id(self) -> str:
        if self.runtime_runner is not None:
            return self.runtime_runner.runtime_id
        return f"cnvsrc2025-runtime-{self.device}"


@dataclass
class UnavailableCnvsrcBackend:
    readiness_report: JsonObject
    device: str = "cpu"

    def status(self) -> JsonObject:
        readiness_error = readiness_error_code(self.readiness_report)
        return {
            "schema_version": SCHEMA_VERSION,
            "model_id": MODEL_ID,
            "runtime_id": self.runtime_id(),
            "device": self.device,
            "ready": False,
            "status": readiness_error,
            "error_code": readiness_error,
            "backend": "cnvsrc2025",
            "fallback_active": False,
            "exit_code": self.readiness_report.get("exit_code"),
        }

    def decode(self, request_payload: JsonObject) -> JsonObject:
        readiness_error = readiness_error_code(self.readiness_report)
        raise SidecarError(
            HTTPStatus.SERVICE_UNAVAILABLE,
            "MODEL_UNAVAILABLE",
            "cnvsrc2025 model is unavailable; decode did not run",
            {"readiness_error_code": readiness_error, "candidates": []},
        )

    def runtime_id(self) -> str:
        return f"cnvsrc2025-unavailable-{self.device}"


def readiness_error_code(report: JsonObject) -> str:
    value = report.get("error_code") or report.get("status") or "MODEL_UNAVAILABLE"
    if not isinstance(value, str) or not value:
        return "MODEL_UNAVAILABLE"
    return value if value in READINESS_ERROR_CODES else "MODEL_UNAVAILABLE"


@dataclass
class FixtureCnvsrcBackend:
    readiness_report: JsonObject
    device: str = "cpu"

    def status(self) -> JsonObject:
        error_code = self.readiness_report.get("error_code")
        readiness_status = self.readiness_report.get("status") or "CHECKPOINT_MISSING"
        if error_code is not None and error_code not in READINESS_ERROR_CODES:
            error_code = "RUNTIME_IMPORT_FAILED"
        return {
            "schema_version": SCHEMA_VERSION,
            "model_id": MODEL_ID,
            "runtime_id": self.runtime_id(),
            "device": self.device,
            "ready": False,
            "status": "FIXTURE_MODE",
            "error_code": error_code,
            "readiness_status": readiness_status,
            "fixture_mode": True,
            "backend": "fixture",
            "fallback_active": True,
            "exit_code": self.readiness_report.get("exit_code"),
        }

    def decode(self, request_payload: JsonObject) -> JsonObject:
        return build_candidate_response(
            request_payload=request_payload,
            runtime_id=self.runtime_id(),
            device=self.device,
        )

    def runtime_id(self) -> str:
        suffix = self.device
        if not self.readiness_report.get("ready"):
            suffix = f"{suffix}-checkpoint-gated"
        return f"{FIXTURE_RUNTIME_ID}-{suffix}"


def build_candidate_response(request_payload: JsonObject, runtime_id: str, device: str) -> JsonObject:
    started = time.perf_counter_ns()
    quality_flags = request_payload["quality_flags"]
    candidates = fixture_candidates(quality_flags)
    first_ms = elapsed_ms(started)
    final_ms = max(first_ms, elapsed_ms(started))
    return {
        "schema_version": SCHEMA_VERSION,
        "request_id": request_payload["request_id"],
        "session_id": request_payload["session_id"],
        "model": {
            "model_id": MODEL_ID,
            "runtime_id": runtime_id,
            "device": device,
        },
        "candidates": candidates,
        "quality_flags": quality_flags,
        "timing_ms": {
            "roi_received_to_first_candidate": first_ms,
            "roi_received_to_final": final_ms,
        },
        "created_at_ms": int(time.time() * 1000),
    }


def build_runtime_candidate_response(
    *,
    request_payload: JsonObject,
    runtime_id: str,
    device: str,
    runtime_candidates: list[cnvsrc_runtime.RuntimeCandidate],
    started_ns: int,
) -> JsonObject:
    candidates = runtime_candidates[:5]
    if not candidates:
        raise SidecarError(
            HTTPStatus.SERVICE_UNAVAILABLE,
            "INFERENCE_FAILED",
            "cnvsrc2025 inference returned no candidates",
            {"candidates": []},
        )
    quality_flags = request_payload["quality_flags"]
    quality_ok = quality_allows_auto_insert(quality_flags)
    first_ms = elapsed_ms(started_ns)
    final_ms = max(first_ms, elapsed_ms(started_ns))
    return {
        "schema_version": SCHEMA_VERSION,
        "request_id": request_payload["request_id"],
        "session_id": request_payload["session_id"],
        "model": {
            "model_id": MODEL_ID,
            "runtime_id": runtime_id,
            "device": device,
        },
        "candidates": [
            {
                "schema_version": SCHEMA_VERSION,
                "rank": index + 1,
                "text": candidate.text,
                "score": clamp_score(candidate.score),
                "source": MODEL_ID,
                "is_auto_insert_eligible": quality_ok and index == 0 and candidate.score >= 0.80,
            }
            for index, candidate in enumerate(candidates)
        ],
        "quality_flags": quality_flags,
        "timing_ms": {
            "roi_received_to_first_candidate": first_ms,
            "roi_received_to_final": final_ms,
        },
        "created_at_ms": int(time.time() * 1000),
    }


def clamp_score(score: float) -> float:
    return min(1.0, max(0.0, float(score)))


def elapsed_ms(started_ns: int) -> int:
    return max(0, int((time.perf_counter_ns() - started_ns) / 1_000_000))


def fixture_candidates(quality_flags: JsonObject) -> list[JsonObject]:
    quality_ok = quality_allows_auto_insert(quality_flags)
    base: tuple[tuple[str, float], ...] = (
        ("帮我总结这段文字", 0.82),
        ("帮我整理这段文字", 0.61),
        ("请总结这段文字", 0.47),
    )
    return [
        {
            "schema_version": SCHEMA_VERSION,
            "rank": index + 1,
            "text": text,
            "score": score if quality_ok else min(score, 0.39),
            "source": MODEL_ID,
            "is_auto_insert_eligible": quality_ok and index == 0 and score >= 0.80,
        }
        for index, (text, score) in enumerate(base)
    ]


def quality_allows_auto_insert(quality_flags: JsonObject) -> bool:
    bool_fields = (
        "face_found",
        "mouth_landmarks_found",
        "crop_bounds_valid",
        "blur_ok",
        "brightness_ok",
        "pose_ok",
        "occlusion_ok",
    )
    return (
        all(quality_flags.get(field_name) is True for field_name in bool_fields)
        and float(quality_flags.get("landmark_confidence", 0.0)) >= 0.80
        and quality_flags.get("rejection_reasons") == []
    )


def validate_roi_request(payload: JsonObject) -> None:
    allowed_top_level = {
        "schema_version",
        "request_id",
        "session_id",
        "source",
        "roi",
        "quality_flags",
        "requested_at_ms",
    }
    require_exact_keys(payload, allowed_top_level, "RoiRequest")
    if payload.get("schema_version") != SCHEMA_VERSION:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "unsupported schema_version")
    string_field(payload, "request_id", max_length=128)
    string_field(payload, "session_id", max_length=128)
    requested_at_ms = payload.get("requested_at_ms")
    if not isinstance(requested_at_ms, int) or requested_at_ms < 0:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "requested_at_ms must be a non-negative integer")
    validate_source(payload.get("source"))
    validate_roi(payload.get("roi"))
    validate_quality_flags(payload.get("quality_flags"))


def require_exact_keys(payload: JsonObject, allowed_keys: set[str], label: str) -> None:
    keys = set(payload)
    missing = sorted(allowed_keys - keys)
    extra = sorted(keys - allowed_keys)
    if missing or extra:
        parts = []
        if missing:
            parts.append(f"missing {', '.join(missing)}")
        if extra:
            parts.append(f"unexpected {', '.join(extra)}")
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"{label} has {'; '.join(parts)}")


def validate_source(value: object) -> None:
    if not isinstance(value, dict):
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "source must be an object")
    source = dict(value)
    allowed = {"kind", "device_id_hash", "started_at_ms"}
    if not {"kind", "started_at_ms"}.issubset(source):
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "source is missing required fields")
    require_subset_keys(source, allowed, "source")
    if source.get("kind") not in {"camera", "public_video", "fixture"}:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "source.kind is invalid")
    started_at_ms = source.get("started_at_ms")
    if not isinstance(started_at_ms, int) or started_at_ms < 0:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "source.started_at_ms must be a non-negative integer")
    device_id_hash = source.get("device_id_hash")
    if device_id_hash is not None and (not isinstance(device_id_hash, str) or len(device_id_hash) > 128):
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "source.device_id_hash is invalid")


def validate_roi(value: object) -> None:
    if not isinstance(value, dict):
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "roi must be an object")
    roi = dict(value)
    allowed = {"local_ref", "format", "width", "height", "fps", "frame_count", "duration_ms"}
    require_exact_keys(roi, allowed, "roi")
    local_ref = roi.get("local_ref")
    if not isinstance(local_ref, str) or not local_ref.startswith("local://") or len(local_ref) > 512:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "roi.local_ref must use local scheme")
    if roi.get("format") not in {"grayscale_u8", "rgb_u8", "tensor_f32_nchw"}:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "roi.format is invalid")
    require_int_range(roi, "width", minimum=1, maximum=512)
    require_int_range(roi, "height", minimum=1, maximum=512)
    require_number_range(roi, "fps", exclusive_minimum=0.0, maximum=120.0)
    require_int_range(roi, "frame_count", minimum=1, maximum=1200)
    require_int_range(roi, "duration_ms", minimum=1, maximum=30000)


def validate_quality_flags(value: object) -> None:
    if not isinstance(value, dict):
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "quality_flags must be an object")
    flags = dict(value)
    allowed = {
        "schema_version",
        "face_found",
        "mouth_landmarks_found",
        "crop_bounds_valid",
        "blur_ok",
        "brightness_ok",
        "pose_ok",
        "occlusion_ok",
        "landmark_confidence",
        "rejection_reasons",
    }
    require_exact_keys(flags, allowed, "quality_flags")
    if flags.get("schema_version") != SCHEMA_VERSION:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "quality_flags.schema_version is invalid")
    for field_name in (
        "face_found",
        "mouth_landmarks_found",
        "crop_bounds_valid",
        "blur_ok",
        "brightness_ok",
        "pose_ok",
        "occlusion_ok",
    ):
        if not isinstance(flags.get(field_name), bool):
            raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"{field_name} must be boolean")
    require_number_range(flags, "landmark_confidence", minimum=0.0, maximum=1.0)
    reasons = flags.get("rejection_reasons")
    if not isinstance(reasons, list) or any(not isinstance(reason, str) for reason in reasons):
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "rejection_reasons must be a string array")
    if len(set(reasons)) != len(reasons):
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", "rejection_reasons must be unique")
    unknown = sorted(set(reasons) - QUALITY_REASON_CODES)
    if unknown:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"unknown rejection reason {unknown[0]}")


def require_subset_keys(payload: JsonObject, allowed_keys: set[str], label: str) -> None:
    extra = sorted(set(payload) - allowed_keys)
    if extra:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"{label} has unexpected {', '.join(extra)}")


def require_int_range(payload: JsonObject, field_name: str, *, minimum: int, maximum: int) -> None:
    value = payload.get(field_name)
    if not isinstance(value, int) or value < minimum or value > maximum:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"{field_name} is out of range")


def require_number_range(
    payload: JsonObject,
    field_name: str,
    *,
    minimum: float | None = None,
    exclusive_minimum: float | None = None,
    maximum: float,
) -> None:
    value = payload.get(field_name)
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"{field_name} must be numeric")
    numeric = float(value)
    if minimum is not None and numeric < minimum:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"{field_name} is out of range")
    if exclusive_minimum is not None and numeric <= exclusive_minimum:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"{field_name} is out of range")
    if numeric > maximum:
        raise SidecarError(HTTPStatus.BAD_REQUEST, "INVALID_REQUEST", f"{field_name} is out of range")


def parser() -> argparse.ArgumentParser:
    parser_ = argparse.ArgumentParser(description="Run the FreeLip local-only VSR sidecar.")
    parser_.add_argument("--host", default=ALLOWED_BIND_HOST, help="Bind host; must be the local loopback address.")
    parser_.add_argument("--port", type=int, default=8765, help="Bind port.")
    parser_.add_argument("--token", required=True, help="Per-session token supplied by the Rust app.")
    parser_.add_argument("--model", default=MODEL_ID, choices=(MODEL_ID,), help="VSR model backend.")
    parser_.add_argument("--device", default="cpu", choices=("cpu", "cuda"), help="Requested model device.")
    parser_.add_argument("--fixture-mode", action="store_true", help="DEV/TEST ONLY: return deterministic fixture candidates when model assets are unavailable.")
    return parser_


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        server = create_server(args.host, args.port, token=args.token, model_id=args.model, device=args.device, fixture_mode=args.fixture_mode)
    except ValueError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
    print(f"FreeLip VSR sidecar listening on http://{args.host}:{args.port}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return READY_EXIT_CODE


if __name__ == "__main__":
    raise SystemExit(main())
