#!/usr/bin/env bash
set -euo pipefail

env_file=${1:?usage: rollback-production.sh <production-env-file>}
backup="${env_file}.rollback"
failed="${env_file}.failed"
[[ -f "$backup" ]] || {
  echo "rollback environment file not found: $backup" >&2
  exit 1
}

rollback_image=$(python3 - "$backup" <<'PY'
import pathlib
import re
import sys

lines = pathlib.Path(sys.argv[1]).read_text().splitlines()
values = [line.split("=", 1)[1].strip() for line in lines if line.startswith("SOL_WATCHER_IMAGE=")]
if len(values) != 1:
    raise SystemExit("rollback environment must contain exactly one SOL_WATCHER_IMAGE entry")
if re.fullmatch(r"\S+:v?\d+\.\d+\.\d+(?:-[0-9A-Za-z][0-9A-Za-z.-]*)?@sha256:[0-9a-f]{64}", values[0]) is None:
    raise SystemExit("rollback SOL_WATCHER_IMAGE must include a semantic version tag and immutable digest")
print(values[0])
PY
)

cp -p "$env_file" "$failed"
cp -p "$backup" "$env_file"
if scripts/production-stack.sh up "$env_file" \
  && scripts/production-stack.sh ready "$env_file"; then
  echo "Production rolled back to $rollback_image"
  echo "Failed rollout environment retained in $failed"
  exit 0
fi

echo "Rollback failed; restoring the pre-rollback environment" >&2
cp -p "$failed" "$env_file"
scripts/production-stack.sh up "$env_file" || true
scripts/production-stack.sh ready "$env_file" || true
exit 1
