from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import TypeAlias, cast

import pytest
from jsonschema import Draft202012Validator


REPO_ROOT = Path(__file__).resolve().parents[2]
SCHEMAS_DIR = REPO_ROOT / "schemas"
FIXTURES_DIR = SCHEMAS_DIR / "fixtures"


CONTRACT_FIXTURES = {
    "candidate": "candidate.valid.json",
    "quality_flags": "quality_flags.valid.json",
    "roi_request": "roi_request.valid.json",
    "candidate_response": "candidate_response.valid.json",
    "insert_record": "insert_record.valid.json",
    "dictionary_entry": "dictionary_entry.valid.json",
    "log_event": "log_event.valid.json",
    "llm_rerank_request": "llm_rerank_request.valid.json",
    "llm_rerank_response": "llm_rerank_response.valid.json",
}

JsonValue: TypeAlias = (
    None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]
)
JsonObject: TypeAlias = dict[str, JsonValue]


def load_json(path: Path) -> JsonValue:
    with path.open("r", encoding="utf-8") as file:
        return cast(JsonValue, json.load(file))


def load_schema(contract_name: str) -> JsonObject:
    path = SCHEMAS_DIR / f"{contract_name}.schema.json"
    assert path.exists(), f"missing schema: {path.relative_to(REPO_ROOT)}"
    schema = load_json(path)
    assert isinstance(schema, dict), f"schema must be an object: {path.name}"
    Draft202012Validator.check_schema(schema)
    return schema


def assert_valid(contract_name: str, payload: JsonValue) -> None:
    schema = load_schema(contract_name)
    errors = sorted(Draft202012Validator(schema).iter_errors(payload), key=str)
    assert errors == []


def assert_invalid(contract_name: str, payload: JsonValue) -> list[str]:
    schema = load_schema(contract_name)
    errors = sorted(Draft202012Validator(schema).iter_errors(payload), key=str)
    assert errors, f"{contract_name} unexpectedly accepted invalid payload"
    return [str(error.message) for error in errors]


@pytest.mark.parametrize("contract_name,fixture_name", CONTRACT_FIXTURES.items())
def test_contract_schema_accepts_valid_fixture(contract_name: str, fixture_name: str) -> None:
    payload = load_json(FIXTURES_DIR / fixture_name)

    assert_valid(contract_name, payload)


@pytest.mark.parametrize("contract_name,fixture_name", CONTRACT_FIXTURES.items())
def test_contract_schema_rejects_unknown_top_level_fields(
    contract_name: str, fixture_name: str
) -> None:
    payload = cast(JsonObject, copy.deepcopy(load_json(FIXTURES_DIR / fixture_name)))
    payload["unexpected_media_blob"] = "not allowed"

    messages = assert_invalid(contract_name, payload)

    assert any("Additional properties" in message for message in messages)


def test_llm_rerank_payload_rejects_media_and_embedding_fields() -> None:
    payload = load_json(FIXTURES_DIR / "llm_rerank_request.invalid_media.json")

    messages = assert_invalid("llm_rerank_request", payload)

    joined = "\n".join(messages)
    assert "video_bytes" in joined
    assert "roi_frames" in joined
    assert "image" in joined
    assert "embedding" in joined


def test_candidate_requires_schema_version() -> None:
    payload = cast(JsonObject, copy.deepcopy(load_json(FIXTURES_DIR / "candidate.valid.json")))
    _ = payload.pop("schema_version", None)

    messages = assert_invalid("candidate", payload)

    assert any("schema_version" in message for message in messages)


def test_quality_flags_requires_schema_version() -> None:
    payload = cast(
        JsonObject,
        copy.deepcopy(load_json(FIXTURES_DIR / "quality_flags.valid.json")),
    )
    _ = payload.pop("schema_version", None)

    messages = assert_invalid("quality_flags", payload)

    assert any("schema_version" in message for message in messages)


@pytest.mark.parametrize(
    "contract_name,fixture_name",
    [
        ("quality_flags", "quality_flags.valid.json"),
        ("roi_request", "roi_request.valid.json"),
        ("candidate_response", "candidate_response.valid.json"),
    ],
)
def test_quality_flags_rejects_unknown_rejection_reasons_everywhere(
    contract_name: str, fixture_name: str
) -> None:
    payload = cast(JsonObject, copy.deepcopy(load_json(FIXTURES_DIR / fixture_name)))
    quality_flags = payload if contract_name == "quality_flags" else payload["quality_flags"]
    assert isinstance(quality_flags, dict)
    quality_flags["rejection_reasons"] = cast(
        JsonValue,
        ["unexpected_quality_reason"],
    )

    messages = assert_invalid(contract_name, payload)

    assert any("unexpected_quality_reason" in message for message in messages)


@pytest.mark.parametrize(
    "contract_name,fixture_name,candidate_path",
    [
        ("candidate", "candidate.valid.json", []),
        ("candidate_response", "candidate_response.valid.json", ["candidates", 0]),
        ("llm_rerank_request", "llm_rerank_request.valid.json", ["candidates", 0]),
        (
            "llm_rerank_response",
            "llm_rerank_response.valid.json",
            ["reranked_candidates", 0],
        ),
    ],
)
def test_candidate_source_accepts_cnvsrc2025_everywhere(
    contract_name: str, fixture_name: str, candidate_path: list[str | int]
) -> None:
    payload = cast(JsonObject, copy.deepcopy(load_json(FIXTURES_DIR / fixture_name)))
    candidate: JsonValue = payload
    for segment in candidate_path:
        if isinstance(segment, int):
            assert isinstance(candidate, list)
            candidate = cast(list[JsonValue], candidate)[segment]
        else:
            assert isinstance(candidate, dict)
            candidate = candidate[segment]
    assert isinstance(candidate, dict)
    candidate["source"] = "cnvsrc2025"

    assert_valid(contract_name, payload)


def test_dev_shell_declares_inline_favicon() -> None:
    html = (REPO_ROOT / "index.html").read_text(encoding="utf-8")

    assert 'rel="icon"' in html
    assert "data:image/svg+xml" in html
