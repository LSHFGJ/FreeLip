from __future__ import annotations

import copy
import json
import os
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import TypeAlias, cast

import pytest
from freelip_eval import purge_debug_logs as purge_module
from jsonschema import Draft202012Validator


REPO_ROOT = Path(__file__).resolve().parents[2]
SCHEMAS_DIR = REPO_ROOT / "schemas"
FIXTURES_DIR = SCHEMAS_DIR / "fixtures"

JsonValue: TypeAlias = (
    None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)
JsonObject: TypeAlias = dict[str, JsonValue]


FORBIDDEN_TOP_LEVEL_MEDIA_FIELDS: tuple[tuple[str, JsonValue], ...] = (
    ("roi_bytes", "raw-roi-bytes"),
    ("roi_frames", ["base64-frame"]),
    ("video_path", "C:/Users/example/clip.mp4"),
    ("video_bytes", "AAAA"),
    ("image_base64", "base64-image"),
    ("image", "base64-image"),
    ("embedding", [0.1, 0.2, 0.3]),
    ("debug_clip_path", "C:/Users/example/debug.mp4"),
    ("screenshot", "base64-screen"),
)

FORBIDDEN_CANDIDATE_MEDIA_FIELDS: tuple[tuple[str, JsonValue], ...] = (
    ("roi_bytes", "raw-roi-bytes"),
    ("video_path", "C:/Users/example/clip.mp4"),
    ("image_base64", "base64-image"),
    ("embedding", [0.1, 0.2]),
)


def load_json(path: Path) -> JsonValue:
    with path.open("r", encoding="utf-8") as file:
        return cast(JsonValue, json.load(file))


def load_schema(contract_name: str) -> JsonObject:
    schema = load_json(SCHEMAS_DIR / f"{contract_name}.schema.json")
    assert isinstance(schema, dict)
    Draft202012Validator.check_schema(schema)
    return schema


def validation_messages(contract_name: str, payload: JsonValue) -> list[str]:
    schema = load_schema(contract_name)
    return [
        str(error.message)
        for error in sorted(Draft202012Validator(schema).iter_errors(payload), key=str)
    ]


def assert_valid(contract_name: str, payload: JsonValue) -> None:
    assert validation_messages(contract_name, payload) == []


def assert_invalid_for_field(contract_name: str, payload: JsonValue, field_name: str) -> None:
    messages = "\n".join(validation_messages(contract_name, payload))
    assert field_name in messages


@pytest.mark.parametrize("field_name,field_value", FORBIDDEN_TOP_LEVEL_MEDIA_FIELDS)
def test_llm_rerank_request_rejects_top_level_media_fields(
    field_name: str, field_value: JsonValue
) -> None:
    payload = cast(JsonObject, copy.deepcopy(load_json(FIXTURES_DIR / "llm_rerank_request.valid.json")))
    payload[field_name] = field_value

    assert_invalid_for_field("llm_rerank_request", payload, field_name)


@pytest.mark.parametrize("field_name,field_value", FORBIDDEN_CANDIDATE_MEDIA_FIELDS)
def test_llm_rerank_request_rejects_candidate_media_fields(
    field_name: str, field_value: JsonValue
) -> None:
    payload = cast(JsonObject, copy.deepcopy(load_json(FIXTURES_DIR / "llm_rerank_request.valid.json")))
    candidates = payload["candidates"]
    assert isinstance(candidates, list)
    first_candidate = candidates[0]
    assert isinstance(first_candidate, dict)
    first_candidate[field_name] = field_value

    assert_invalid_for_field("llm_rerank_request", payload, field_name)


def test_existing_invalid_media_fixture_still_fails_cloud_payload_validation() -> None:
    payload = load_json(FIXTURES_DIR / "llm_rerank_request.invalid_media.json")
    messages = "\n".join(validation_messages("llm_rerank_request", payload))

    for field_name in ("video_bytes", "roi_frames", "image", "embedding"):
        assert field_name in messages


def test_log_event_schema_accepts_local_debug_metadata_without_media_payload() -> None:
    payload: JsonObject = {
        "schema_version": "1.0.0",
        "event_id": "log-privacy-0001",
        "session_id": "session-20260428-0001",
        "level": "info",
        "event_type": "roi_debug_event",
        "message": "ROI debug metadata recorded locally",
        "timestamp_ms": 1_800_000_000_000,
        "fields": {
            "request_id": "request-42",
            "quality_flags": ["ROI_OK"],
            "candidate_texts": ["帮我总结这段文字", "帮我整理这段文字"],
            "candidate_count": 2,
            "insertion_outcome": "auto_inserted",
            "undo_outcome": "not_requested",
            "latency_ms": 930,
            "model_id": "cnvsrc2025",
            "failure_reason": None,
            "roi_debug_metadata_path": ".freelip/roi-debug/request-42.json",
        },
    }

    assert_valid("log_event", payload)
    serialized = json.dumps(payload, ensure_ascii=False)
    for forbidden in ("roi_bytes", "video_bytes", "image_base64", "embedding"):
        assert forbidden not in serialized


@pytest.mark.parametrize(
    "field_name,field_value",
    FORBIDDEN_TOP_LEVEL_MEDIA_FIELDS + FORBIDDEN_CANDIDATE_MEDIA_FIELDS,
)
def test_log_event_schema_rejects_media_named_fields(
    field_name: str, field_value: JsonValue
) -> None:
    payload = cast(JsonObject, copy.deepcopy(load_json(FIXTURES_DIR / "log_event.valid.json")))
    fields = payload["fields"]
    assert isinstance(fields, dict)
    fields[field_name] = field_value

    assert_invalid_for_field("log_event", payload, field_name)


def test_purge_debug_logs_dry_run_lists_only_expired_files(tmp_path: Path) -> None:
    now = datetime(2026, 4, 28, 12, 0, tzinfo=timezone.utc)
    expired = tmp_path / "expired.roi-debug.json"
    current = tmp_path / "current-day.roi-debug.json"
    _ = expired.write_text("{}", encoding="utf-8")
    _ = current.write_text("{}", encoding="utf-8")
    set_mtime(expired, now - timedelta(days=8))
    set_mtime(current, now)

    report = purge_module.purge_debug_logs(
        debug_dir=tmp_path,
        older_than_days=7,
        dry_run=True,
        now=now,
    )

    expired_files = {Path(path).name for path in report["expired_files"]}
    retained_files = {Path(path).name for path in report["retained_files"]}
    assert expired_files == {expired.name}
    assert current.name in retained_files
    assert report["removed_count"] == 0
    assert expired.exists()
    assert current.exists()


def test_purge_debug_logs_rejects_retention_beyond_seven_days(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="7 days"):
        _ = purge_module.purge_debug_logs(
            debug_dir=tmp_path,
            older_than_days=8,
            dry_run=True,
        )


def test_purge_debug_logs_ignores_unrelated_old_files(tmp_path: Path) -> None:
    now = datetime(2026, 4, 28, 12, 0, tzinfo=timezone.utc)
    expired_debug = tmp_path / "expired.roi-debug.json"
    unrelated = tmp_path / "notes.txt"
    _ = expired_debug.write_text("{}", encoding="utf-8")
    _ = unrelated.write_text("keep", encoding="utf-8")
    set_mtime(expired_debug, now - timedelta(days=8))
    set_mtime(unrelated, now - timedelta(days=30))

    report = purge_module.purge_debug_logs(
        debug_dir=tmp_path,
        older_than_days=7,
        dry_run=False,
        now=now,
    )

    expired_files = {Path(path).name for path in report["expired_files"]}
    assert expired_files == {expired_debug.name}
    assert report["removed_count"] == 1
    assert not expired_debug.exists()
    assert unrelated.exists()


def test_purge_debug_logs_cli_outputs_json_without_current_day_files(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    now = datetime(2026, 4, 28, 12, 0, tzinfo=timezone.utc)
    expired = tmp_path / "expired-cli.roi-debug.json"
    current = tmp_path / "current-cli.roi-debug.json"
    _ = expired.write_text("{}", encoding="utf-8")
    _ = current.write_text("{}", encoding="utf-8")
    set_mtime(expired, now - timedelta(days=8))
    set_mtime(current, now)

    exit_code = purge_module.main(
        [
            "--debug-dir",
            str(tmp_path),
            "--older-than-days",
            "7",
            "--dry-run",
            "--now-iso",
            now.isoformat(),
        ]
    )

    output = capsys.readouterr().out
    report = json.loads(output)
    expired_names = {Path(path).name for path in report["expired_files"]}
    output_names = {Path(path).name for path in report["expired_files"]}

    assert exit_code == 0
    assert expired_names == {expired.name}
    assert current.name not in output_names
    assert report["dry_run"] is True


def set_mtime(path: Path, value: datetime) -> None:
    timestamp = value.timestamp()
    _ = os.utime(path, (timestamp, timestamp))
