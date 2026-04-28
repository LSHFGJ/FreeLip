from __future__ import annotations

import queue
import threading
import time
from collections.abc import Mapping, Sequence
from typing import Any, Protocol, cast


SCHEMA_VERSION = "1.0.0"
MAX_CANDIDATES = 5
MAX_DICTIONARY_TERMS = 32
DICTIONARY_RANK_BOOST_SCALE = 0.20
ALLOWED_CANDIDATE_SOURCES = {"vsr", "cnvsrc2025", "dictionary", "llm_rerank", "manual"}


JsonObject = dict[str, Any]


class RerankProvider(Protocol):
    def rerank(self, request_payload: JsonObject) -> JsonObject:
        ...


def rerank_candidates(
    *,
    request_id: str,
    session_id: str,
    candidates: Sequence[Mapping[str, Any]],
    context_text: str = "",
    dictionary_terms: Sequence[Mapping[str, Any]] | None = None,
    provider: RerankProvider | None = None,
    enabled: bool = False,
    timeout_seconds: float = 1.0,
    max_candidates: int = MAX_CANDIDATES,
    now_ms: int | None = None,
) -> JsonObject:
    terms = sanitize_dictionary_terms(dictionary_terms or [])
    local_candidates = local_rank_candidates(candidates, terms, max_candidates=max_candidates)
    created_at_ms = now_ms if now_ms is not None else current_time_ms()

    if not enabled or provider is None:
        return build_response(
            request_id=request_id,
            session_id=session_id,
            candidates=local_candidates,
            provider="local_disabled",
            created_at_ms=created_at_ms,
        )

    request_payload = build_llm_rerank_request(
        request_id=request_id,
        session_id=session_id,
        context_text=context_text,
        candidates=local_candidates,
        dictionary_terms=terms,
        max_candidates=max_candidates,
    )
    status, provider_response = call_provider_with_timeout(
        provider=provider,
        request_payload=request_payload,
        timeout_seconds=timeout_seconds,
    )

    if status != "ok" or provider_response is None:
        return build_response(
            request_id=request_id,
            session_id=session_id,
            candidates=local_candidates,
            provider=f"local_fallback:{status}",
            created_at_ms=created_at_ms,
        )

    try:
        return normalize_provider_response(
            request_id=request_id,
            session_id=session_id,
            provider_response=provider_response,
            max_candidates=max_candidates,
            fallback_created_at_ms=created_at_ms,
        )
    except (TypeError, ValueError):
        return build_response(
            request_id=request_id,
            session_id=session_id,
            candidates=local_candidates,
            provider="local_fallback:invalid_response",
            created_at_ms=created_at_ms,
        )


def build_llm_rerank_request(
    *,
    request_id: str,
    session_id: str,
    context_text: str,
    candidates: Sequence[Mapping[str, Any]],
    dictionary_terms: Sequence[Mapping[str, Any]],
    max_candidates: int = MAX_CANDIDATES,
) -> JsonObject:
    clean_candidates = [
        assign_rank(candidate, index + 1)
        for index, candidate in enumerate(sanitize_candidates(candidates)[:MAX_CANDIDATES])
    ]
    terms = needed_dictionary_terms(clean_candidates, sanitize_dictionary_terms(dictionary_terms), context_text)
    return {
        "schema_version": SCHEMA_VERSION,
        "request_id": bounded_text(request_id, 128),
        "session_id": bounded_text(session_id, 128),
        "context_text": bounded_text(context_text, 2048),
        "candidates": clean_candidates[: clamp_int(max_candidates, 1, MAX_CANDIDATES)],
        "dictionary_terms": terms[:MAX_DICTIONARY_TERMS],
        "max_candidates": clamp_int(max_candidates, 1, MAX_CANDIDATES),
    }


def local_rank_candidates(
    candidates: Sequence[Mapping[str, Any]],
    dictionary_terms: Sequence[Mapping[str, Any]],
    *,
    max_candidates: int = MAX_CANDIDATES,
) -> list[JsonObject]:
    clean_candidates = sanitize_candidates(candidates)
    if not clean_candidates:
        raise ValueError("at least one candidate is required")

    clean_terms = sanitize_dictionary_terms(dictionary_terms)
    scored: list[tuple[int, JsonObject]] = []
    for index, candidate in enumerate(clean_candidates):
        ranked = dict(candidate)
        ranked["score"] = clamp_float(
            float(ranked["score"]) + dictionary_boost(str(ranked["text"]), clean_terms)
        )
        scored.append((index, ranked))

    scored.sort(key=lambda item: (-float(item[1]["score"]), item[0]))
    limit = clamp_int(max_candidates, 1, MAX_CANDIDATES)
    return [assign_rank(candidate, index + 1) for index, (_, candidate) in enumerate(scored[:limit])]


def call_provider_with_timeout(
    *,
    provider: RerankProvider,
    request_payload: JsonObject,
    timeout_seconds: float,
) -> tuple[str, JsonObject | None]:
    result_queue: queue.Queue[tuple[str, object]] = queue.Queue(maxsize=1)

    def run_provider() -> None:
        try:
            result_queue.put(("ok", provider.rerank(request_payload)))
        except Exception as error:  # provider failure must not block local capture/insertion state
            result_queue.put(("provider_error", error))

    thread = threading.Thread(target=run_provider, daemon=True)
    thread.start()
    thread.join(max(timeout_seconds, 0.0))
    if thread.is_alive():
        return "timeout", None

    try:
        status, value = result_queue.get_nowait()
    except queue.Empty:
        return "provider_error", None
    if status != "ok":
        return status, None
    if not isinstance(value, dict):
        return "invalid_response", None
    return "ok", cast(JsonObject, value)


def normalize_provider_response(
    *,
    request_id: str,
    session_id: str,
    provider_response: Mapping[str, Any],
    max_candidates: int,
    fallback_created_at_ms: int,
) -> JsonObject:
    raw_candidates = provider_response.get("reranked_candidates")
    if not isinstance(raw_candidates, Sequence) or isinstance(raw_candidates, (str, bytes)):
        raise ValueError("provider response must include reranked candidates")
    reranked_candidates = [
        assign_rank(candidate, index + 1)
        for index, candidate in enumerate(sanitize_candidates(cast(Sequence[Mapping[str, Any]], raw_candidates))[:MAX_CANDIDATES])
    ][: clamp_int(max_candidates, 1, MAX_CANDIDATES)]
    if not reranked_candidates:
        raise ValueError("provider response returned no candidates")

    provider_name = provider_response.get("provider")
    created_at_ms = provider_response.get("created_at_ms")
    return build_response(
        request_id=request_id,
        session_id=session_id,
        candidates=reranked_candidates,
        provider=bounded_text(provider_name if isinstance(provider_name, str) else "llm_rerank", 128),
        created_at_ms=created_at_ms if isinstance(created_at_ms, int) and created_at_ms >= 0 else fallback_created_at_ms,
    )


def build_response(
    *,
    request_id: str,
    session_id: str,
    candidates: Sequence[Mapping[str, Any]],
    provider: str,
    created_at_ms: int,
) -> JsonObject:
    clean_candidates = [
        assign_rank(candidate, index + 1)
        for index, candidate in enumerate(sanitize_candidates(candidates)[:MAX_CANDIDATES])
    ]
    if not clean_candidates:
        raise ValueError("at least one reranked candidate is required")
    return {
        "schema_version": SCHEMA_VERSION,
        "request_id": bounded_text(request_id, 128),
        "session_id": bounded_text(session_id, 128),
        "reranked_candidates": clean_candidates,
        "provider": bounded_text(provider, 128),
        "created_at_ms": max(int(created_at_ms), 0),
    }


def sanitize_candidates(candidates: Sequence[Mapping[str, Any]]) -> list[JsonObject]:
    clean: list[JsonObject] = []
    for index, candidate in enumerate(candidates):
        text = bounded_text(candidate.get("text"), 256)
        if not text:
            continue
        source = candidate.get("source")
        clean.append(
            {
                "schema_version": SCHEMA_VERSION,
                "rank": clamp_int(candidate.get("rank"), 1, MAX_CANDIDATES) if "rank" in candidate else index + 1,
                "text": text,
                "score": clamp_float(candidate.get("score")),
                "source": source if isinstance(source, str) and source in ALLOWED_CANDIDATE_SOURCES else "vsr",
                "is_auto_insert_eligible": bool(candidate.get("is_auto_insert_eligible", False)),
            }
        )
    return clean


def sanitize_dictionary_terms(dictionary_terms: Sequence[Mapping[str, Any]]) -> list[JsonObject]:
    clean: list[JsonObject] = []
    seen: set[str] = set()
    for term in dictionary_terms:
        surface = bounded_text(term.get("surface"), 128)
        if not surface or surface in seen:
            continue
        seen.add(surface)
        clean.append(
            {
                "surface": surface,
                "weight": clamp_float(term.get("weight")),
                "tags": sanitize_tags(term.get("tags")),
            }
        )
        if len(clean) == MAX_DICTIONARY_TERMS:
            break
    return clean


def needed_dictionary_terms(
    candidates: Sequence[Mapping[str, Any]],
    dictionary_terms: Sequence[Mapping[str, Any]],
    context_text: str,
) -> list[JsonObject]:
    haystack = bounded_text(context_text, 2048) + "\n" + "\n".join(str(candidate.get("text", "")) for candidate in candidates)
    return [term for term in sanitize_dictionary_terms(dictionary_terms) if str(term["surface"]) in haystack]


def dictionary_boost(candidate_text: str, dictionary_terms: Sequence[Mapping[str, Any]]) -> float:
    total = 0.0
    for term in dictionary_terms:
        surface = str(term.get("surface", ""))
        if surface and surface in candidate_text:
            total += clamp_float(term.get("weight")) * DICTIONARY_RANK_BOOST_SCALE
    return clamp_float(total)


def assign_rank(candidate: Mapping[str, Any], rank: int) -> JsonObject:
    ranked = dict(candidate)
    ranked["schema_version"] = SCHEMA_VERSION
    ranked["rank"] = clamp_int(rank, 1, MAX_CANDIDATES)
    ranked["score"] = clamp_float(ranked.get("score"))
    ranked["text"] = bounded_text(ranked.get("text"), 256)
    source = ranked.get("source")
    ranked["source"] = source if isinstance(source, str) and source in ALLOWED_CANDIDATE_SOURCES else "vsr"
    ranked["is_auto_insert_eligible"] = bool(ranked.get("is_auto_insert_eligible", False))
    return ranked


def sanitize_tags(value: object) -> list[str]:
    if not isinstance(value, Sequence) or isinstance(value, (str, bytes)):
        return []
    clean: list[str] = []
    for tag in value:
        text = bounded_text(tag, 64)
        if text and text not in clean:
            clean.append(text)
        if len(clean) == 16:
            break
    return clean


def bounded_text(value: object, max_length: int) -> str:
    if value is None:
        return ""
    return str(value).strip()[:max_length]


def clamp_float(value: object) -> float:
    if not isinstance(value, (int, float, str)):
        return 0.0
    try:
        number = float(value)
    except ValueError:
        return 0.0
    if number != number:
        return 0.0
    return min(max(number, 0.0), 1.0)


def clamp_int(value: object, minimum: int, maximum: int) -> int:
    if not isinstance(value, (int, float, str)):
        return minimum
    try:
        number = int(value)
    except ValueError:
        return minimum
    return min(max(number, minimum), maximum)


def current_time_ms() -> int:
    return int(time.time() * 1000)
