#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <cnvsrc2025|mavsr2025> <cpu|cuda> [freelip_vsr.check_model args...]" >&2
  exit 64
fi

MODEL_ID="$1"
DEVICE="$2"
shift 2

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
export PYTHONPATH="${REPO_ROOT}/python${PYTHONPATH:+:${PYTHONPATH}}"

exec python3 -m freelip_vsr.check_model --model "${MODEL_ID}" --device "${DEVICE}" "$@"
