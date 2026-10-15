#!/usr/bin/env bash
# Budget-only workload; both cleanup passes use the repository's declaration.
set -euo pipefail
cd "$(dirname "$0")/.."
export PYTHONPATH
PYTHONPATH="$(just --evaluate PYTHONPATH)"
exec uv run -q python pool-maintenance/measure.py
