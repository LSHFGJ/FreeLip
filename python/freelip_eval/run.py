from __future__ import annotations

import argparse
import json
import math
import os
import statistics
import sys
from collections import Counter
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import Any, cast

from freelip_eval.usability_rules import check_top5_usability, is_usable, normalize_text


SCHEMA_VERSION = "1.0.0"
TARGET_TOP5_USABILITY = 0.60
DEFAULT_MODEL_ID = "cnvsrc2025"
DEFAULT_RUNTIME_ID = "cnvsrc2025-fixture-cpu-checkpoint-gated"
FIXTURE_PROVIDER = "local_disabled"
DEFAULT_FIXTURE_CANDIDATES: tuple[tuple[str, float], ...] = (
    ("帮我总结这段文字", 0.39),
    ("帮我整理这段文字", 0.32),
    ("请总结这段文字", 0.25),
    ("帮我翻译这段文字", 0.18),
    ("帮我改写这段文字", 0.12),
)

JsonObject = dict[str, Any]


def evaluate_suite(suite_path: str | os.PathLike[str]) -> JsonObject:
    suite = Path(suite_path)
    raw_samples = load_suite_samples(suite)
    readiness = model_readiness_snapshot(DEFAULT_MODEL_ID)
    labeled_samples: list[Mapping[str, Any]] = []
    unlabeled_count = 0
    for sample in raw_samples:
        if is_labeled_sample(sample):
            labeled_samples.append(sample)
        else:
            unlabeled_count += 1

    sample_results = [
        evaluate_sample(sample, index, readiness)
        for index, sample in enumerate(labeled_samples)
    ]
    return build_report(suite, sample_results, unlabeled_count, readiness)


def load_suite_samples(suite_path: Path) -> list[Mapping[str, Any]]:
    paths: list[Path]
    if suite_path.is_dir():
        paths = sorted(
            path
            for path in suite_path.iterdir()
            if path.suffix == ".json" and path.name != "metadata.json"
        )
    else:
        paths = [suite_path]

    samples: list[Mapping[str, Any]] = []
    for path in paths:
        with path.open("r", encoding="utf-8") as file:
            data = cast(object, json.load(file))
        if isinstance(data, list):
            for item in data:
                if isinstance(item, Mapping):
                    samples.append(cast(Mapping[str, Any], item))
        elif isinstance(data, Mapping):
            samples.append(cast(Mapping[str, Any], data))
    return samples


def is_labeled_sample(sample: Mapping[str, Any]) -> bool:
    target_text = sample.get("target_text")
    equivalents = sample.get("acceptable_equivalents")
    return (
        isinstance(target_text, str)
        and bool(target_text.strip())
        and isinstance(equivalents, list)
        and all(isinstance(value, str) for value in equivalents)
    )


def evaluate_sample(
    sample: Mapping[str, Any],
    index: int,
    readiness: Mapping[str, Any],
) -> JsonObject:
    target_text = str(sample["target_text"])
    acceptable_equivalents = cast(list[str], sample["acceptable_equivalents"])
    prediction = prediction_for_sample(sample, index, readiness)
    candidates = candidate_texts(prediction.get("candidates"))
    pre_rerank_candidates = candidate_texts(prediction.get("pre_rerank_candidates"))
    if not pre_rerank_candidates:
        pre_rerank_candidates = candidates
    top1 = candidates[0] if candidates else ""
    top5_usable = check_top5_usability(candidates, target_text, acceptable_equivalents)
    pre_rerank_top5 = check_top5_usability(
        pre_rerank_candidates, target_text, acceptable_equivalents
    )
    top1_usable = is_usable(top1, target_text, acceptable_equivalents) if top1 else False
    roi_status = bounded_text(prediction.get("roi_status"), "ROI_OK")
    model_status = bounded_text(prediction.get("model_status"), "MODEL_UNAVAILABLE")
    sidecar_error = bounded_text(prediction.get("sidecar_error"), "")
    rerank_provider = bounded_text(prediction.get("rerank_provider"), FIXTURE_PROVIDER)
    insertion = mapping_value(prediction.get("insertion"))
    undo = mapping_value(prediction.get("undo"))
    dictionary_events = string_list(prediction.get("dictionary_learning_events"))

    return {
        "sample_index": index,
        "target_text": target_text,
        "domain_tag": bounded_text(sample.get("domain_tag"), "unknown"),
        "candidate_count": len(candidates),
        "top5_usable": top5_usable,
        "top1_usable": top1_usable,
        "top1_cer": character_error_rate(top1, target_text),
        "latency_ms": non_negative_number(prediction.get("latency_ms")),
        "roi_status": roi_status,
        "model_status": model_status,
        "model_id": bounded_text(prediction.get("model_id"), DEFAULT_MODEL_ID),
        "runtime_id": bounded_text(prediction.get("runtime_id"), DEFAULT_RUNTIME_ID),
        "sidecar_error": sidecar_error,
        "rerank_provider": rerank_provider,
        "rerank_delta": int(top5_usable) - int(pre_rerank_top5),
        "insertion_attempted": bool(insertion.get("attempted", False)),
        "insertion_succeeded": bool(insertion.get("succeeded", False)),
        "insertion_failure_reason": bounded_text(insertion.get("failure_reason"), ""),
        "undo_attempted": bool(undo.get("attempted", False)),
        "undo_succeeded": bool(undo.get("succeeded", False)),
        "undo_failure_reason": bounded_text(undo.get("failure_reason"), ""),
        "dictionary_learning_events": dictionary_events,
    }


def prediction_for_sample(
    sample: Mapping[str, Any],
    index: int,
    readiness: Mapping[str, Any],
) -> JsonObject:
    fixture_prediction = sample.get("fixture_prediction")
    if isinstance(fixture_prediction, Mapping):
        prediction = dict(fixture_prediction)
        prediction.setdefault("model_id", DEFAULT_MODEL_ID)
        prediction.setdefault("runtime_id", DEFAULT_RUNTIME_ID)
        prediction.setdefault("model_status", readiness_status(readiness))
        prediction.setdefault("rerank_provider", FIXTURE_PROVIDER)
        prediction.setdefault("roi_status", "ROI_OK")
        prediction.setdefault("latency_ms", deterministic_latency_ms(index))
        return prediction

    status = readiness_status(readiness)
    ready = readiness.get("ready") is True
    sidecar_error = "" if ready else status
    return {
        "prediction_source": "deterministic_fixture",
        "fixture_mode": True,
        "model_id": DEFAULT_MODEL_ID,
        "runtime_id": DEFAULT_RUNTIME_ID,
        "model_ready": ready,
        "model_status": status if not ready else "FIXTURE_PREDICTIONS_MISSING",
        "sidecar_error": sidecar_error,
        "rerank_provider": FIXTURE_PROVIDER,
        "roi_status": "ROI_OK",
        "latency_ms": deterministic_latency_ms(index),
        "pre_rerank_candidates": fixture_candidates(),
        "candidates": fixture_candidates(),
        "insertion": {
            "attempted": False,
            "succeeded": False,
            "failure_reason": "MODEL_UNAVAILABLE" if not ready else "FIXTURE_ONLY",
        },
        "undo": {"attempted": False, "succeeded": False},
        "dictionary_learning_events": [],
    }


def build_report(
    suite_path: Path,
    sample_results: Sequence[Mapping[str, Any]],
    unlabeled_count: int,
    readiness: Mapping[str, Any],
) -> JsonObject:
    sample_count = len(sample_results)
    top5_successes = count_true(sample_results, "top5_usable")
    top1_successes = count_true(sample_results, "top1_usable")
    latencies = [float(result["latency_ms"]) for result in sample_results]
    top5_usability = safe_rate(top5_successes, sample_count)
    top1_cer = average(float(result["top1_cer"]) for result in sample_results)
    roi_failure_count = sum(1 for result in sample_results if result["roi_status"] != "ROI_OK")
    target_met = top5_usability >= TARGET_TOP5_USABILITY
    model_counts = counter_for(sample_results, "model_status")
    roi_counts = counter_for(sample_results, "roi_status")
    rerank_counts = counter_for(sample_results, "rerank_provider")
    sidecar_errors = counter_for_non_empty(sample_results, "sidecar_error")
    insertion = insertion_summary(sample_results)
    undo = undo_summary(sample_results)
    dictionary_learning = dictionary_learning_summary(sample_results)
    candidate_usability = {
        "top5_usable": top5_successes,
        "top5_unusable": sample_count - top5_successes,
        "top1_usable": top1_successes,
        "top1_unusable": sample_count - top1_successes,
        "no_candidates": sum(1 for result in sample_results if result["candidate_count"] == 0),
    }
    failure_breakdown = {
        "included": not target_met,
        "reason": None if target_met else "TOP5_TARGET_NOT_MET",
        "roi": dict_sorted(roi_counts),
        "model": dict_sorted(model_counts),
        "rerank": dict_sorted(rerank_counts),
        "candidate_usability": candidate_usability,
        "dominant_blockers": dominant_blockers(roi_counts, model_counts, sidecar_errors, candidate_usability),
    }

    return {
        "schema_version": SCHEMA_VERSION,
        "suite_path": str(suite_path),
        "sample_count": sample_count,
        "unlabeled_sample_count": unlabeled_count,
        "target_top5_usability": TARGET_TOP5_USABILITY,
        "top5_usability": round_metric(top5_usability),
        "target_met": target_met,
        "top1_cer": round_metric(top1_cer),
        "p50_latency_ms": percentile_p50(latencies),
        "p95_latency_ms": percentile_nearest(latencies, 95),
        "roi_failure_rate": round_metric(safe_rate(roi_failure_count, sample_count)),
        "rerank_delta": round_metric(average(float(result["rerank_delta"]) for result in sample_results)),
        "model_id": dominant_value(counter_for(sample_results, "model_id"), DEFAULT_MODEL_ID),
        "runtime_id": dominant_value(counter_for(sample_results, "runtime_id"), DEFAULT_RUNTIME_ID),
        "fixture_mode": True,
        "prediction_source": "deterministic_fixture_when_real_predictions_absent",
        "model_readiness": {
            "ready": readiness.get("ready") is True,
            "status": readiness_status(readiness),
            "error_code": readiness_error(readiness),
        },
        "sidecar": {
            "decode_request_count": sample_count,
            "success_count": sample_count - sum(sidecar_errors.values()),
            "error_count": sum(sidecar_errors.values()),
            "errors": dict_sorted(sidecar_errors),
            "failure_breakdown": dict_sorted(sidecar_errors),
        },
        "insertion": insertion,
        "undo": undo,
        "dictionary_learning": dictionary_learning,
        "failure_breakdown": failure_breakdown,
        "sample_results": list(sample_results),
    }


def insertion_summary(sample_results: Sequence[Mapping[str, Any]]) -> JsonObject:
    attempted = count_true(sample_results, "insertion_attempted")
    succeeded = count_true(sample_results, "insertion_succeeded")
    failure_reasons = counter_for_non_empty(sample_results, "insertion_failure_reason")
    return {
        "attempted_count": attempted,
        "success_count": succeeded,
        "failure_count": max(attempted - succeeded, 0),
        "blocked_count": len(sample_results) - attempted,
        "success_rate": round_metric(safe_rate(succeeded, attempted)),
        "failure_reasons": dict_sorted(failure_reasons),
    }


def undo_summary(sample_results: Sequence[Mapping[str, Any]]) -> JsonObject:
    attempted = count_true(sample_results, "undo_attempted")
    succeeded = count_true(sample_results, "undo_succeeded")
    failure_reasons = counter_for_non_empty(sample_results, "undo_failure_reason")
    return {
        "attempted_count": attempted,
        "success_count": succeeded,
        "failure_count": max(attempted - succeeded, 0),
        "success_rate": round_metric(safe_rate(succeeded, attempted)),
        "failure_reasons": dict_sorted(failure_reasons),
    }


def dictionary_learning_summary(sample_results: Sequence[Mapping[str, Any]]) -> JsonObject:
    events: Counter[str] = Counter()
    for result in sample_results:
        for event in string_list(result.get("dictionary_learning_events")):
            events[event] += 1
    return {
        "event_count": sum(events.values()),
        "samples_with_events": sum(
            1 for result in sample_results if string_list(result.get("dictionary_learning_events"))
        ),
        "events_by_type": dict_sorted(events),
    }


def character_error_rate(candidate: str, target: str) -> float:
    normalized_candidate = normalize_text(candidate)
    normalized_target = normalize_text(target)
    if not normalized_target:
        return 0.0 if not normalized_candidate else 1.0
    distance = levenshtein_distance(normalized_candidate, normalized_target)
    return min(distance / len(normalized_target), 1.0)


def levenshtein_distance(left: str, right: str) -> int:
    if left == right:
        return 0
    if not left:
        return len(right)
    if not right:
        return len(left)
    previous = list(range(len(right) + 1))
    for left_index, left_char in enumerate(left, start=1):
        current = [left_index]
        for right_index, right_char in enumerate(right, start=1):
            insert_cost = current[right_index - 1] + 1
            delete_cost = previous[right_index] + 1
            replace_cost = previous[right_index - 1] + (left_char != right_char)
            current.append(min(insert_cost, delete_cost, replace_cost))
        previous = current
    return previous[-1]


def model_readiness_snapshot(model_id: str) -> JsonObject:
    try:
        from freelip_vsr.sidecar import readiness_report

        return dict(readiness_report(model_id=model_id, device="cpu"))
    except Exception as exc:
        return {
            "ready": False,
            "status": "RUNTIME_IMPORT_FAILED",
            "error_code": "RUNTIME_IMPORT_FAILED",
            "message": f"readiness probe unavailable: {exc.__class__.__name__}",
        }


def readiness_status(readiness: Mapping[str, Any]) -> str:
    if readiness.get("ready") is True:
        return "MODEL_READY"
    return readiness_error(readiness)


def readiness_error(readiness: Mapping[str, Any]) -> str:
    value = readiness.get("error_code") or readiness.get("status") or "MODEL_UNAVAILABLE"
    return str(value) if str(value).strip() else "MODEL_UNAVAILABLE"


def fixture_candidates() -> list[JsonObject]:
    return [
        {
            "schema_version": SCHEMA_VERSION,
            "rank": index + 1,
            "text": text,
            "score": score,
            "source": DEFAULT_MODEL_ID,
            "is_auto_insert_eligible": False,
        }
        for index, (text, score) in enumerate(DEFAULT_FIXTURE_CANDIDATES)
    ]


def candidate_texts(value: object) -> list[str]:
    if not isinstance(value, Sequence) or isinstance(value, (str, bytes)):
        return []
    texts: list[str] = []
    for item in value:
        text = ""
        if isinstance(item, Mapping):
            raw_text = item.get("text")
            if raw_text is not None:
                text = str(raw_text).strip()
        elif item is not None:
            text = str(item).strip()
        if text:
            texts.append(text)
    return texts[:5]


def mapping_value(value: object) -> Mapping[str, Any]:
    if isinstance(value, Mapping):
        return cast(Mapping[str, Any], value)
    return {}


def string_list(value: object) -> list[str]:
    if not isinstance(value, Sequence) or isinstance(value, (str, bytes)):
        return []
    result: list[str] = []
    for item in value:
        if isinstance(item, str) and item.strip():
            result.append(item.strip())
    return result


def bounded_text(value: object, fallback: str) -> str:
    if value is None:
        return fallback
    text = str(value).strip()
    return text if text else fallback


def non_negative_number(value: object) -> float:
    if isinstance(value, (int, float, str)):
        try:
            number = float(value)
        except ValueError:
            return 0.0
        if math.isfinite(number):
            return max(number, 0.0)
    return 0.0


def deterministic_latency_ms(index: int) -> int:
    return 35 + (index % 11) * 4


def count_true(sample_results: Sequence[Mapping[str, Any]], key: str) -> int:
    return sum(1 for result in sample_results if result.get(key) is True)


def counter_for(sample_results: Sequence[Mapping[str, Any]], key: str) -> Counter[str]:
    return Counter(str(result.get(key, "unknown")) for result in sample_results)


def counter_for_non_empty(sample_results: Sequence[Mapping[str, Any]], key: str) -> Counter[str]:
    values: Counter[str] = Counter()
    for result in sample_results:
        value = result.get(key)
        if isinstance(value, str) and value:
            values[value] += 1
    return values


def dict_sorted(counter: Mapping[str, int]) -> JsonObject:
    return {key: counter[key] for key in sorted(counter)}


def dominant_value(counter: Counter[str], fallback: str) -> str:
    if not counter:
        return fallback
    return sorted(counter.items(), key=lambda item: (-item[1], item[0]))[0][0]


def dominant_blockers(
    roi_counts: Counter[str],
    model_counts: Counter[str],
    sidecar_errors: Counter[str],
    candidate_usability: Mapping[str, int],
) -> list[JsonObject]:
    blockers: list[tuple[str, str, int]] = []
    for reason, count in roi_counts.items():
        if reason != "ROI_OK":
            blockers.append(("roi", reason, count))
    for reason, count in model_counts.items():
        if reason != "MODEL_READY":
            blockers.append(("model", reason, count))
    for reason, count in sidecar_errors.items():
        blockers.append(("sidecar", reason, count))
    top5_unusable = int(candidate_usability.get("top5_unusable", 0))
    if top5_unusable:
        blockers.append(("candidate_usability", "top5_unusable", top5_unusable))
    blockers.sort(key=lambda item: (-item[2], item[0], item[1]))
    return [
        {"category": category, "reason": reason, "count": count}
        for category, reason, count in blockers[:8]
    ]


def safe_rate(numerator: int, denominator: int) -> float:
    return 0.0 if denominator <= 0 else numerator / denominator


def average(values: Sequence[float] | Any) -> float:
    value_list = list(values)
    if not value_list:
        return 0.0
    return sum(float(value) for value in value_list) / len(value_list)


def percentile_p50(values: Sequence[float]) -> float:
    if not values:
        return 0.0
    return round_metric(float(statistics.median(sorted(values))))


def percentile_nearest(values: Sequence[float], percentile: int) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    rank = max(1, math.ceil((percentile / 100) * len(ordered)))
    return round_metric(float(ordered[min(rank - 1, len(ordered) - 1)]))


def round_metric(value: float) -> float:
    rounded = round(float(value), 6)
    return 0.0 if rounded == -0.0 else rounded


def write_report(report: Mapping[str, Any], report_path: str | os.PathLike[str]) -> None:
    path = Path(report_path)
    if path.parent:
        path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Evaluate FreeLip fixture predictions.")
    parser.add_argument("--suite", required=True, help="Path to evaluation suite directory or JSON file.")
    parser.add_argument("--report", required=True, help="Path to write JSON evaluation report.")
    args = parser.parse_args(argv)

    report = evaluate_suite(str(args.suite))
    write_report(report, str(args.report))
    print(
        "Evaluation report written: "
        f"sample_count={report['sample_count']} "
        f"top5_usability={report['top5_usability']} "
        f"target_met={report['target_met']} "
        f"report={args.report}"
    )
    return 0 if int(report["sample_count"]) > 0 else 1


if __name__ == "__main__":
    sys.exit(main())
