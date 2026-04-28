from __future__ import annotations

import json
import importlib
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
EVIDENCE_DIR = REPO_ROOT / ".sisyphus/evidence"


def test_report_counts_only_labeled_samples_and_explains_failures(tmp_path: Path) -> None:
    run = importlib.import_module("freelip_eval.run")

    suite_dir = tmp_path / "suite"
    suite_dir.mkdir()
    samples = [
        {
            "target_text": "帮我写个请假条",
            "acceptable_equivalents": ["给我写个请假条"],
            "punctuation_policy": "ignore",
            "source": "synthetic",
            "license_note": "text-only internal eval",
            "domain_tag": "assistant",
            "split": "test",
            "fixture_prediction": {
                "pre_rerank_candidates": ["错误候选", "帮我写个请假条"],
                "candidates": [
                    {"text": "错误候选", "score": 0.91, "source": "cnvsrc2025"},
                    {"text": "帮我写个请假条", "score": 0.75, "source": "cnvsrc2025"},
                ],
                "latency_ms": 40,
                "roi_status": "ROI_OK",
                "model_status": "MODEL_READY",
                "model_id": "cnvsrc2025",
                "runtime_id": "fixture-test-runtime",
                "rerank_provider": "local_disabled",
                "insertion": {"attempted": True, "succeeded": True},
                "undo": {"attempted": True, "succeeded": True},
                "dictionary_learning_events": ["auto_insert_not_undone"],
            },
        },
        {
            "target_text": "打开静音模式",
            "acceptable_equivalents": ["开启静音"],
            "punctuation_policy": "ignore",
            "source": "synthetic",
            "license_note": "text-only internal eval",
            "domain_tag": "device_control",
            "split": "test",
            "fixture_prediction": {
                "pre_rerank_candidates": ["打开视频模式", "关闭静音模式"],
                "candidates": [
                    {"text": "打开视频模式", "score": 0.62, "source": "cnvsrc2025"},
                    {"text": "关闭静音模式", "score": 0.58, "source": "cnvsrc2025"},
                ],
                "latency_ms": 80,
                "roi_status": "NO_FACE",
                "model_status": "CHECKPOINT_MISSING",
                "model_id": "cnvsrc2025",
                "runtime_id": "fixture-test-runtime",
                "sidecar_error": "CHECKPOINT_MISSING",
                "rerank_provider": "local_disabled",
                "insertion": {"attempted": False, "succeeded": False, "failure_reason": "MODEL_UNAVAILABLE"},
                "undo": {"attempted": False, "succeeded": False},
                "dictionary_learning_events": ["undo_negative"],
            },
        },
        {
            "source": "unlabeled-public-video-placeholder",
            "fixture_prediction": {
                "candidates": [{"text": "不应计入指标", "score": 0.99, "source": "cnvsrc2025"}],
                "latency_ms": 1,
                "roi_status": "ROI_OK",
            },
        },
    ]
    (suite_dir / "samples.json").write_text(
        json.dumps(samples, ensure_ascii=False, indent=2), encoding="utf-8"
    )

    report = run.evaluate_suite(suite_dir)

    assert report["sample_count"] == 2
    assert report["unlabeled_sample_count"] == 1
    assert report["top5_usability"] == 0.5
    assert report["target_top5_usability"] == 0.6
    assert report["target_met"] is False
    assert report["top1_cer"] > 0
    assert report["p50_latency_ms"] == 60
    assert report["p95_latency_ms"] == 80
    assert report["roi_failure_rate"] == 0.5
    assert report["rerank_delta"] == 0.0
    assert report["model_id"] == "cnvsrc2025"

    breakdown = report["failure_breakdown"]
    assert breakdown["included"] is True
    assert breakdown["roi"]["NO_FACE"] == 1
    assert breakdown["model"]["CHECKPOINT_MISSING"] == 1
    assert breakdown["rerank"]["local_disabled"] == 2
    assert breakdown["candidate_usability"]["top5_unusable"] == 1
    assert breakdown["candidate_usability"]["top1_unusable"] == 2

    assert report["sidecar"]["error_count"] == 1
    assert report["sidecar"]["errors"]["CHECKPOINT_MISSING"] == 1
    assert report["insertion"]["attempted_count"] == 1
    assert report["insertion"]["success_count"] == 1
    assert report["undo"]["success_count"] == 1
    assert report["dictionary_learning"]["event_count"] == 2
    assert report["dictionary_learning"]["events_by_type"]["auto_insert_not_undone"] == 1
    assert report["dictionary_learning"]["events_by_type"]["undo_negative"] == 1

    EVIDENCE_DIR.mkdir(parents=True, exist_ok=True)
    (EVIDENCE_DIR / "task-12-failure-breakdown.json").write_text(
        json.dumps(
            {
                "command": "PYTHONPATH=/home/lshfgj/FreeLip/python python3 -m pytest python/tests/test_eval_failure_breakdown.py -q",
                "sample_count": report["sample_count"],
                "unlabeled_sample_count": report["unlabeled_sample_count"],
                "top5_usability": report["top5_usability"],
                "failure_breakdown": breakdown,
            },
            ensure_ascii=False,
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
    )
