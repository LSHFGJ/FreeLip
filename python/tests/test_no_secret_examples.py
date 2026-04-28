from __future__ import annotations

from pathlib import Path

from freelip_eval.verify_docs import scan_for_secret_examples


REPO_ROOT = Path(__file__).resolve().parents[2]
EVIDENCE_DIR = REPO_ROOT / ".sisyphus/evidence"


def test_env_example_uses_placeholders_not_real_secrets() -> None:
    findings = scan_for_secret_examples([REPO_ROOT / ".env.example"])

    assert findings == []
    EVIDENCE_DIR.mkdir(parents=True, exist_ok=True)
    _ = (EVIDENCE_DIR / "task-13-no-secrets.txt").write_text(
        "PASS: .env.example contains placeholders only; no fake real-looking API keys/tokens detected.\n",
        encoding="utf-8",
    )


def test_secret_scanner_rejects_real_looking_tokens(tmp_path: Path) -> None:
    sample = tmp_path / "bad.env"
    _ = sample.write_text(
        "OPENAI_API_KEY=sk-1234567890abcdef1234567890abcdef\n"
        + "SESSION_TOKEN=ghp_1234567890abcdef1234567890abcdef123456\n",
        encoding="utf-8",
    )

    findings = scan_for_secret_examples([sample])

    assert {finding["kind"] for finding in findings} == {"openai_key", "github_token"}
    assert all(str(sample) in finding["path"] for finding in findings)

def test_secret_scanner_detects_secrets_alongside_placeholders(tmp_path: Path) -> None:
    sample = tmp_path / "mixed.env"
    _ = sample.write_text(
        "VAR=<placeholder>\n"
        + "FREELIP_LLM_API_KEY=<placeholder> sk-1234567890abcdef1234567890abcdef\n"
        + "OTHER_TOKEN=ghp_1234567890abcdef1234567890abcdef123456 <some-val>\n",
        encoding="utf-8",
    )

    findings = scan_for_secret_examples([sample])

    assert len(findings) == 2
    assert findings[0]["kind"] == "openai_key"
    assert findings[0]["line"] == 2
    assert findings[1]["kind"] == "github_token"
    assert findings[1]["line"] == 3

def test_secret_scanner_detects_secrets_inside_angle_brackets(tmp_path: Path) -> None:
    sample = tmp_path / "angles.env"
    _ = sample.write_text(
        "OPENAI_API_KEY=<sk-local1234567890abcdef1234567890abcdef>\n"
        + "SESSION_TOKEN=<ghp_local1234567890abcdef1234567890abcdef123456>\n"
        + "SAFE_VAR_1=<placeholder>\n"
        + "SAFE_VAR_2=<replace-with-local-token>\n",
        encoding="utf-8",
    )

    findings = scan_for_secret_examples([sample])

    assert len(findings) == 2
    assert findings[0]["kind"] == "openai_key"
    assert findings[0]["line"] == 1
    assert findings[1]["kind"] == "github_token"
    assert findings[1]["line"] == 2
