from __future__ import annotations

import json
import time
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator

from freelip_eval import rerank


REPO_ROOT = Path(__file__).resolve().parents[2]
SCHEMAS_DIR = REPO_ROOT / "schemas"
FORBIDDEN_MEDIA_FIELDS = (
    "roi_bytes",
    "roi_frames",
    "video_bytes",
    "video_path",
    "image_base64",
    "screenshot",
    "embedding",
    "debug_clip_path",
)


class SlowProvider:
    def __init__(self) -> None:
        self.requests: list[dict[str, Any]] = []

    def rerank(self, request_payload: dict[str, Any]) -> dict[str, Any]:
        self.requests.append(request_payload)
        time.sleep(0.25)
        return {
            "schema_version": "1.0.0",
            "request_id": request_payload["request_id"],
            "session_id": request_payload["session_id"],
            "reranked_candidates": request_payload["candidates"],
            "provider": "slow-provider",
            "created_at_ms": 1_800_000_000_000,
        }


class FailingProvider:
    def rerank(self, request_payload: dict[str, Any]) -> dict[str, Any]:
        raise RuntimeError("provider unavailable")


class ForbiddenProvider:
    def rerank(self, request_payload: dict[str, Any]) -> dict[str, Any]:
        raise AssertionError("provider should not be called when rerank is disabled")


def test_llm_rerank_disabled_by_default_uses_local_ranking() -> None:
    result = rerank.rerank_candidates(
        request_id="rerank-disabled-1",
        session_id="session-1",
        candidates=candidates(),
        context_text="正在输入 FreeLip 提示词。",
        dictionary_terms=dictionary_terms(),
        provider=ForbiddenProvider(),
        now_ms=1_800_000_000_000,
    )

    assert result["provider"] == "local_disabled"
    assert result["reranked_candidates"][0]["text"] == "帮我写 FreeLip 周报"
    assert_response_schema_valid(result)
    assert_no_forbidden_fields(result)


def test_llm_timeout_falls_back_to_local_ranking_with_text_only_payload() -> None:
    provider = SlowProvider()

    result = rerank.rerank_candidates(
        request_id="rerank-timeout-1",
        session_id="session-1",
        candidates=candidates(),
        context_text="在编辑器中输入 FreeLip AI 提示词。",
        dictionary_terms=dictionary_terms(),
        provider=provider,
        enabled=True,
        timeout_seconds=0.01,
        now_ms=1_800_000_000_100,
    )

    assert result["provider"] == "local_fallback:timeout"
    assert result["reranked_candidates"][0]["text"] == "帮我写 FreeLip 周报"
    assert result["reranked_candidates"][0]["rank"] == 1
    assert_response_schema_valid(result)
    assert_no_forbidden_fields(result)

    assert provider.requests, "provider should receive the text-only request before timing out"
    payload = provider.requests[0]
    assert set(payload) == {
        "schema_version",
        "request_id",
        "session_id",
        "context_text",
        "candidates",
        "dictionary_terms",
        "max_candidates",
    }
    assert payload["dictionary_terms"] == [
        {"surface": "FreeLip", "weight": 0.9, "tags": ["product"]}
    ]
    assert_no_forbidden_fields(payload)


def test_llm_provider_failure_falls_back_to_local_ranking() -> None:
    result = rerank.rerank_candidates(
        request_id="rerank-failure-1",
        session_id="session-1",
        candidates=candidates(),
        context_text="正在输入 FreeLip 提示词。",
        dictionary_terms=dictionary_terms(),
        provider=FailingProvider(),
        enabled=True,
        timeout_seconds=0.10,
        now_ms=1_800_000_000_200,
    )

    assert result["provider"] == "local_fallback:provider_error"
    assert result["reranked_candidates"][0]["text"] == "帮我写 FreeLip 周报"
    assert_response_schema_valid(result)
    assert_no_forbidden_fields(result)


def candidates() -> list[dict[str, Any]]:
    return [
        {
            "schema_version": "1.0.0",
            "rank": 1,
            "text": "帮我写普通周报",
            "score": 0.78,
            "source": "cnvsrc2025",
            "is_auto_insert_eligible": True,
            "roi_bytes": "must not be copied",
        },
        {
            "schema_version": "1.0.0",
            "rank": 2,
            "text": "帮我写 FreeLip 周报",
            "score": 0.67,
            "source": "cnvsrc2025",
            "is_auto_insert_eligible": False,
            "embedding": [0.1, 0.2],
        },
        {
            "schema_version": "1.0.0",
            "rank": 3,
            "text": "帮我整理会议纪要",
            "score": 0.60,
            "source": "vsr",
            "is_auto_insert_eligible": False,
        },
    ]


def dictionary_terms() -> list[dict[str, Any]]:
    return [
        {"surface": "FreeLip", "weight": 0.9, "tags": ["product"]},
        {"surface": "无关词", "weight": 1.0, "tags": ["unused"]},
    ]


def assert_response_schema_valid(payload: dict[str, Any]) -> None:
    schema = json.loads((SCHEMAS_DIR / "llm_rerank_response.schema.json").read_text(encoding="utf-8"))
    errors = sorted(Draft202012Validator(schema).iter_errors(payload), key=str)
    assert errors == []


def assert_no_forbidden_fields(payload: dict[str, Any]) -> None:
    serialized = json.dumps(payload, ensure_ascii=False)
    for field_name in FORBIDDEN_MEDIA_FIELDS:
        assert field_name not in serialized
