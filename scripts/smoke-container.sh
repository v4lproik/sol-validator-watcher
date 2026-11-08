#!/usr/bin/env bash
set -euo pipefail

image=${1:?usage: smoke-container.sh <local-image> [expected-revision]}
expected_revision=${2:-}
for command in curl docker python3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command is unavailable: $command" >&2
    exit 1
  }
done

image_id=$(docker image inspect "$image" --format '{{.Id}}')
test_dir=$(mktemp -d)
container="sol-validator-watcher-image-smoke-$$"
mock_pid=
cleanup() {
  status=$?
  if [[ "$status" -ne 0 ]] && docker inspect "$container" >/dev/null 2>&1; then
    docker logs "$container" >&2 || true
  fi
  docker rm -f "$container" >/dev/null 2>&1 || true
  if [[ -n "$mock_pid" ]]; then
    kill "$mock_pid" >/dev/null 2>&1 || true
    wait "$mock_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$test_dir"
  return "$status"
}
trap cleanup EXIT

rpc_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("0.0.0.0", 0)); print(s.getsockname()[1]); s.close()')
metrics_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')

python3 - "$rpc_port" <<'PY' &
import http.server
import json
import sys

port = int(sys.argv[1])

class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        request = json.loads(self.rfile.read(length))
        method = request.get("method")
        if method == "getHealth":
            result = "ok"
        elif method == "getEpochInfo":
            result = {
                "epoch": 1,
                "slotIndex": 1,
                "slotsInEpoch": 432000,
                "absoluteSlot": 432001,
            }
        elif method == "getVoteAccounts":
            result = {"current": [], "delinquent": []}
        elif method == "getClusterNodes":
            result = []
        elif method == "getIdentity":
            result = {"identity": "11111111111111111111111111111111"}
        elif method == "getVersion":
            result = {"solana-core": "1.18.0", "feature-set": 1}
        elif method == "getBlockHeight":
            result = 1
        elif method == "getLeaderSchedule":
            result = {}
        else:
            result = None
        response = {"jsonrpc": "2.0", "id": request.get("id", 1), "result": result}
        body = json.dumps(response).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        return

http.server.ThreadingHTTPServer(("0.0.0.0", port), Handler).serve_forever()
PY
mock_pid=$!
curl -fsS --retry 10 --retry-all-errors --retry-delay 1 --retry-max-time 15 \
  -H 'Content-Type: application/json' \
  --data '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":[]}' \
  "http://127.0.0.1:${rpc_port}" >/dev/null

cat >"$test_dir/smoke.toml" <<EOF
network = "container-smoke"
metrics_host = "0.0.0.0"
metrics_port = 8000
poll_interval_sec = 1
liveness_file = "/tmp/sol-validator-watcher.liveness"

[[connectors]]
name = "mock-rpc"
provider = "container-smoke"
features = []
solana_rpc_compatibility = "core"
rpc = { url = "http://host.docker.internal:${rpc_port}", request_timeout_sec = 5, connect_timeout_sec = 2, response_size_limit_bytes = 1048576, commitment = "confirmed" }

[modes.health.commands]
rpc_health = ["mock-rpc"]
ws_health = []
node_status = []

[modes.consensus.commands]
epoch_info = ["mock-rpc"]
vote_accounts = ["mock-rpc"]
recent_performance_samples = []

[modes.economics]
enabled = false
EOF

docker run -d \
  --name "$container" \
  --add-host host.docker.internal:host-gateway \
  --read-only \
  --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  --cap-drop ALL \
  --security-opt no-new-privileges \
  -p "127.0.0.1:${metrics_port}:8000" \
  -v "$test_dir/smoke.toml:/app/config/smoke.toml:ro" \
  "$image" --config /app/config/smoke.toml >/dev/null

container_image_id=$(docker inspect "$container" --format '{{.Image}}')
[[ "$container_image_id" == "$image_id" ]] || {
  echo "container did not run the requested image: expected $image_id, got $container_image_id" >&2
  exit 1
}

curl -fsS --retry 30 --retry-all-errors --retry-delay 1 --retry-max-time 45 \
  "http://127.0.0.1:${metrics_port}/readyz" >/dev/null
curl -fsS "http://127.0.0.1:${metrics_port}/healthz" >/dev/null
curl -fsS "http://127.0.0.1:${metrics_port}/metrics" >"$test_dir/metrics"

python3 - "$test_dir/metrics" "$expected_revision" <<'PY'
import pathlib
import sys

metrics = pathlib.Path(sys.argv[1]).read_text()
expected_revision = sys.argv[2]
if 'sol_watcher_ready{network="container-smoke"} 1' not in metrics:
    raise SystemExit("exact image never became ready")
if "sol_watcher_build_info{" not in metrics:
    raise SystemExit("exact image omitted build identity")
if expected_revision and f'revision="{expected_revision}"' not in metrics:
    raise SystemExit(f"exact image build revision did not match {expected_revision}")
PY

docker stop --time 15 "$container" >/dev/null
exit_code=$(docker inspect "$container" --format '{{.State.ExitCode}}')
[[ "$exit_code" == 0 ]] || {
  docker logs "$container" >&2
  echo "exact image did not stop cleanly: exit $exit_code" >&2
  exit 1
}

echo "Exact image became ready, exposed build identity, and stopped cleanly"
