from __future__ import annotations

import argparse
import json
import re
import sys
from collections.abc import Iterable, Sequence
from pathlib import Path
from typing import TypedDict, cast


REQUIRED_CONCEPTS: dict[str, tuple[str, ...]] = {
    "internal-research-only": ("internal research only",),
    "not-production-ready": ("not production ready",),
    "no-commercial-rights": ("no commercial rights",),
    "no-cloud-vsr": ("no cloud VSR",),
    "no-raw-video": ("no raw video",),
    "no-roi": ("no ROI",),
    "text-only-llm-rerank-disabled": ("text-only LLM rerank disabled by default",),
    "retention-7-day": ("7-day retention",),
    "checkpoint-missing": ("CHECKPOINT_MISSING",),
    "windows-camera-required": ("WINDOWS_CAMERA_REQUIRED",),
    "windows-camera-implementation-required": ("WINDOWS_CAMERA_IMPLEMENTATION_REQUIRED",),
    "windows-ui-automation-required": ("WINDOWS_UI_AUTOMATION_REQUIRED",),
    "windows-freelip-integration-required": ("WINDOWS_FREELIP_INTEGRATION_REQUIRED",),
    "mavsr2025": ("MAVSR2025",),
    "cas-vsr-s101": ("CAS-VSR-S101",),
    "hotkey": ("Ctrl+Alt+Space",),
    "loopback": ("127.0.0.1",),
}

SECRET_PATTERNS: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("openai_key", re.compile(r"\bsk-[A-Za-z0-9]{20,}\b")),
    ("github_token", re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{20,}\b")),
    ("generic_api_key", re.compile(r"(?i)\b(?:api[_-]?key|token|secret)\s*=\s*[A-Za-z0-9_./+-]{24,}\b")),
)


class SecretFinding(TypedDict):
    path: str
    line: int
    kind: str


def normalize_requirement(name: str) -> str:
    return name.strip().lower().replace("_", "-")


def read_docs(paths: Sequence[Path]) -> str:
    chunks: list[str] = []
    for path in paths:
        chunks.append(path.read_text(encoding="utf-8"))
    return "\n".join(chunks)


def check_required_concepts(text: str, required: Iterable[str]) -> dict[str, bool]:
    results: dict[str, bool] = {}
    for raw_name in required:
        name = normalize_requirement(raw_name)
        phrases = REQUIRED_CONCEPTS.get(name, (raw_name,))
        results[name] = all(phrase in text for phrase in phrases)
    return results


def scan_for_secret_examples(paths: Sequence[Path]) -> list[SecretFinding]:
    findings: list[SecretFinding] = []
    
    def strip_if_placeholder(m: re.Match[str]) -> str:
        content = m.group(0).lower()
        keywords = ["placeholder", "replace", "local", "optional", "disabled", "example", "dummy", "your-", "changeme"]
        if any(w in content for w in keywords):
            return ""
        return m.group(0)

    for path in paths:
        if not path.exists():
            continue
        for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            raw_match = None
            for kind, pattern in SECRET_PATTERNS:
                if pattern.search(line):
                    raw_match = kind
                    break
            
            if raw_match:
                findings.append({"path": str(path), "line": line_number, "kind": raw_match})
                continue
            
            line_to_scan = re.sub(r"<[^>]+>", strip_if_placeholder, line)
            if line_to_scan != line:
                for kind, pattern in SECRET_PATTERNS:
                    if pattern.search(line_to_scan):
                        findings.append({"path": str(path), "line": line_number, "kind": kind})
                        break
    return findings


def build_report(docs: Sequence[Path], required: Sequence[str]) -> dict[str, object]:
    text = read_docs(docs)
    concept_results = check_required_concepts(text, required)
    secret_findings = scan_for_secret_examples(list(docs) + [Path(".env.example")])
    missing = [name for name, present in concept_results.items() if not present]
    return {
        "schema_version": "1.0.0",
        "docs": [str(path) for path in docs],
        "required": list(required),
        "concepts": concept_results,
        "missing": missing,
        "secret_findings": secret_findings,
        "passed": not missing and not secret_findings,
    }


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Verify FreeLip MVP docs contain required warnings and no example secrets.")
    _ = parser.add_argument("--docs", nargs="+", required=True, type=Path)
    _ = parser.add_argument("--require", nargs="*", default=None)
    _ = parser.add_argument("--out", type=Path, default=None)
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    docs = cast(list[Path], args.docs)
    required_arg = cast(list[str] | None, args.require)
    required = required_arg if required_arg else list(REQUIRED_CONCEPTS)
    report = build_report(docs, required)
    output = json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    out_path = cast(Path | None, args.out)
    if out_path is not None:
        out_path.parent.mkdir(parents=True, exist_ok=True)
        _ = out_path.write_text(output, encoding="utf-8")
    else:
        _ = print(output, end="")
    if report["passed"]:
        return 0
    _ = print("FreeLip docs verification failed", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
