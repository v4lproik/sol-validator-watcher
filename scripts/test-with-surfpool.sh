#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v just >/dev/null 2>&1; then
  echo "just CLI not found. Run: mise install" >&2
  exit 1
fi

just surfpool test
