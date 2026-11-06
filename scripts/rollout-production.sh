#!/usr/bin/env bash
set -euo pipefail

env_file=${1:?usage: rollout-production.sh <production-env-file> <candidate-image@sha256:digest>}
candidate=${2:?usage: rollout-production.sh <production-env-file> <candidate-image@sha256:digest>}
if [[ ! "$candidate" =~ ^[^[:space:]]+@sha256:[0-9a-f]{64}$ ]]; then
  echo "rollout image must be pinned by sha256 digest: $candidate" >&2
  exit 1
fi
[[ -f "$env_file" ]] || {
  echo "production environment file not found: $env_file" >&2
  exit 1
}

scripts/canary-production.sh "$env_file" "$candidate"

backup="${env_file}.rollback"
previous=$(python3 - "$env_file" "$backup" "$candidate" <<'PY'
import os
import re
import pathlib
import shutil
import sys
import tempfile

env_path = pathlib.Path(sys.argv[1])
backup_path = pathlib.Path(sys.argv[2])
candidate = sys.argv[3]
lines = env_path.read_text().splitlines(keepends=True)
indices = [index for index, line in enumerate(lines) if line.startswith("SOL_WATCHER_IMAGE=")]
if len(indices) != 1:
    raise SystemExit("production environment must contain exactly one SOL_WATCHER_IMAGE entry")
index = indices[0]
previous = lines[index].split("=", 1)[1].strip()
if re.fullmatch(r"\S+@sha256:[0-9a-f]{64}", previous) is None:
    raise SystemExit("current SOL_WATCHER_IMAGE is not digest-pinned")
shutil.copy2(env_path, backup_path)
lines[index] = f"SOL_WATCHER_IMAGE={candidate}\n"
fd, temporary = tempfile.mkstemp(prefix=f".{env_path.name}.", dir=env_path.parent)
try:
    with os.fdopen(fd, "w") as output:
        output.writelines(lines)
        output.flush()
        os.fsync(output.fileno())
    os.chmod(temporary, env_path.stat().st_mode & 0o777)
    os.replace(temporary, env_path)
finally:
    if os.path.exists(temporary):
        os.unlink(temporary)
print(previous)
PY
)

if scripts/production-stack.sh up "$env_file" \
  && scripts/production-stack.sh ready "$env_file"; then
  echo "Production now runs $candidate"
  echo "Rollback image retained in $backup: $previous"
  exit 0
fi

echo "Production rollout failed; restoring $previous" >&2
cp -p "$backup" "$env_file"
scripts/production-stack.sh up "$env_file"
scripts/production-stack.sh ready "$env_file"
exit 1
