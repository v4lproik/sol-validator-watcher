#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v surfpool >/dev/null 2>&1; then
  echo "surfpool CLI not found. Install it with: curl -sL https://run.surfpool.run/ | bash" >&2
  exit 1
fi

SURFPOOL_E2E=1 cargo test "$@"
