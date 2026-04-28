from __future__ import annotations

import argparse
import json
import os
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import TypedDict, cast


DEFAULT_RETENTION_DAYS = 7
DEBUG_LOG_DIR_ENV = "FREELIP_DEBUG_LOG_DIR"
DEBUG_ARTIFACT_SUFFIXES = (".roi-debug.json", ".roi-debug.log", ".roi-debug.mp4")


class PurgeReport(TypedDict):
    debug_dir: str
    older_than_days: int
    dry_run: bool
    now_iso: str
    threshold_iso: str
    expired_files: list[str]
    retained_files: list[str]
    expired_count: int
    retained_count: int
    removed_count: int
    errors: list[str]


def default_debug_log_dir(cwd: Path | None = None) -> Path:
    configured = os.environ.get(DEBUG_LOG_DIR_ENV)
    if configured:
        return Path(configured).expanduser()
    root = cwd if cwd is not None else Path.cwd()
    return root / ".freelip" / "roi-debug"


def purge_debug_logs(
    *,
    debug_dir: str | Path | None = None,
    older_than_days: int = DEFAULT_RETENTION_DAYS,
    dry_run: bool = False,
    now: datetime | None = None,
) -> PurgeReport:
    if older_than_days < 0:
        raise ValueError("--older-than-days must be greater than or equal to 0")
    if older_than_days > DEFAULT_RETENTION_DAYS:
        raise ValueError("debug log retention cannot exceed 7 days")

    current_time = normalize_datetime(now or datetime.now(timezone.utc))
    threshold = current_time - timedelta(days=older_than_days)
    directory = Path(debug_dir) if debug_dir is not None else default_debug_log_dir()
    expired_files: list[str] = []
    retained_files: list[str] = []
    errors: list[str] = []
    removed_count = 0

    if directory.exists():
        for path in sorted(directory.rglob("*")):
            if path.is_symlink() or not path.is_file() or not is_debug_artifact(path):
                continue
            modified = datetime.fromtimestamp(path.stat().st_mtime, tz=timezone.utc)
            if modified < threshold:
                expired_files.append(str(path))
                if not dry_run:
                    try:
                        path.unlink()
                        removed_count += 1
                    except OSError as error:
                        errors.append(f"{path}: {error}")
            else:
                retained_files.append(str(path))

    return {
        "debug_dir": str(directory),
        "older_than_days": older_than_days,
        "dry_run": dry_run,
        "now_iso": current_time.isoformat(),
        "threshold_iso": threshold.isoformat(),
        "expired_files": expired_files,
        "retained_files": retained_files,
        "expired_count": len(expired_files),
        "retained_count": len(retained_files),
        "removed_count": removed_count,
        "errors": errors,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="List or purge local FreeLip ROI/debug log files older than a retention window."
    )
    _ = parser.add_argument("--debug-dir", type=Path, default=None)
    _ = parser.add_argument("--older-than-days", type=int, default=DEFAULT_RETENTION_DAYS)
    _ = parser.add_argument("--dry-run", action="store_true")
    _ = parser.add_argument("--report", type=Path, default=None)
    _ = parser.add_argument("--now-iso", default=None)
    args = parser.parse_args(argv)
    debug_dir = cast(Path | None, args.debug_dir)
    older_than_days = cast(int, args.older_than_days)
    dry_run = cast(bool, args.dry_run)
    now_iso = cast(str | None, args.now_iso)
    report_path = cast(Path | None, args.report)

    try:
        report = purge_debug_logs(
            debug_dir=debug_dir,
            older_than_days=older_than_days,
            dry_run=dry_run,
            now=parse_now(now_iso),
        )
    except ValueError as error:
        print(str(error), file=sys.stderr)
        return 2

    if report_path is not None:
        report_path.parent.mkdir(parents=True, exist_ok=True)
        _ = report_path.write_text(
            json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8"
        )

    print(json.dumps(report, ensure_ascii=False, indent=2))
    return 0


def parse_now(value: str | None) -> datetime | None:
    if value is None:
        return None
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    return normalize_datetime(parsed)


def normalize_datetime(value: datetime) -> datetime:
    if value.tzinfo is None:
        return value.replace(tzinfo=timezone.utc)
    return value.astimezone(timezone.utc)


def is_debug_artifact(path: Path) -> bool:
    return path.name.endswith(DEBUG_ARTIFACT_SUFFIXES)


if __name__ == "__main__":
    raise SystemExit(main())
