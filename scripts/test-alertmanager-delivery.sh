#!/usr/bin/env bash
set -euo pipefail

image=${1:?usage: test-alertmanager-delivery.sh <alertmanager-image@sha256:digest>}
if [[ ! "$image" =~ ^[^[:space:]]+@sha256:[0-9a-f]{64}$ ]]; then
  echo "Alertmanager smoke image must be pinned by sha256 digest: $image" >&2
  exit 1
fi

for command in curl docker python3; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "required command is unavailable: $command" >&2
    exit 1
  }
done

test_dir=$(mktemp -d)
container="sol-validator-watcher-alert-smoke-$$"
receiver_pid=
cleanup() {
  docker rm -f "$container" >/dev/null 2>&1 || true
  if [[ -n "$receiver_pid" ]]; then
    kill "$receiver_pid" >/dev/null 2>&1 || true
    wait "$receiver_pid" >/dev/null 2>&1 || true
  fi
  rm -rf "$test_dir"
}
trap cleanup EXIT

receiver_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
alertmanager_port=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
payload_file="$test_dir/payload.json"

python3 - "$receiver_port" "$payload_file" <<'PY' &
import http.server
import pathlib
import sys

port = int(sys.argv[1])
payload = pathlib.Path(sys.argv[2])

class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        payload.write_bytes(self.rfile.read(length))
        self.send_response(200)
        self.end_headers()

    def log_message(self, _format, *_args):
        return

http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
PY
receiver_pid=$!

printf 'http://host.docker.internal:%s/alerts\n' "$receiver_port" >"$test_dir/webhook-url"

docker run --rm -d \
  --name "$container" \
  --add-host host.docker.internal:host-gateway \
  -p "127.0.0.1:${alertmanager_port}:9093" \
  -v "$PWD/docker/alertmanager/alertmanager.yml:/etc/alertmanager/alertmanager.yml:ro" \
  -v "$test_dir/webhook-url:/run/secrets/alertmanager_webhook_url:ro" \
  "$image" \
  --config.file=/etc/alertmanager/alertmanager.yml \
  --storage.path=/alertmanager \
  --web.listen-address=0.0.0.0:9093 >/dev/null

curl -fsS --retry 30 --retry-all-errors --retry-delay 1 --retry-max-time 45 \
  "http://127.0.0.1:${alertmanager_port}/-/ready" >/dev/null

curl -fsS -X POST -H 'Content-Type: application/json' \
  --data '[{"labels":{"alertname":"SolValidatorWatcherAlertDeliverySmoke","deployment":"smoke","network":"testnet","severity":"warning"},"annotations":{"summary":"alert delivery smoke"},"generatorURL":"https://example.invalid/smoke"}]' \
  "http://127.0.0.1:${alertmanager_port}/api/v2/alerts" >/dev/null

delivered=false
for _ in $(seq 1 75); do
  if [[ -s "$payload_file" ]]; then
    python3 - "$payload_file" <<'PY'
import json
import pathlib
import sys

payload = json.loads(pathlib.Path(sys.argv[1]).read_text())
alerts = payload.get("alerts", [])
if not any(alert.get("labels", {}).get("alertname") == "SolValidatorWatcherAlertDeliverySmoke" for alert in alerts):
    raise SystemExit("Alertmanager webhook payload omitted the smoke alert")
PY
    delivered=true
    break
  fi
  sleep 1
done
[[ "$delivered" == true ]] || {
  echo "Alertmanager did not deliver the synthetic alert within 75 seconds" >&2
  exit 1
}

curl -fsS -X POST -H 'Content-Type: application/json' \
  --data '[{"labels":{"alertname":"AlertmanagerInhibitionCriticalSmoke","deployment":"smoke","network":"testnet","severity":"critical"}},{"labels":{"alertname":"AlertmanagerInhibitionWarningSmoke","deployment":"smoke","network":"testnet","severity":"warning"}}]' \
  "http://127.0.0.1:${alertmanager_port}/api/v2/alerts" >/dev/null

inhibited=false
for _ in $(seq 1 15); do
  curl -fsS "http://127.0.0.1:${alertmanager_port}/api/v2/alerts" >"$test_dir/alerts.json"
  if python3 - "$test_dir/alerts.json" <<'PY'
import json
import pathlib
import sys

alerts = json.loads(pathlib.Path(sys.argv[1]).read_text())
warning = next(
    (
        alert
        for alert in alerts
        if alert.get("labels", {}).get("alertname")
        == "AlertmanagerInhibitionWarningSmoke"
    ),
    None,
)
if warning is None:
    raise SystemExit(1)
status = warning.get("status", {})
if status.get("state") != "suppressed" or not status.get("inhibitedBy"):
    raise SystemExit(1)
PY
  then
    inhibited=true
    break
  fi
  sleep 1
done
[[ "$inhibited" == true ]] || {
  echo "Alertmanager did not inhibit the warning alert" >&2
  exit 1
}

echo "Alertmanager delivered the synthetic alert and inhibited the warning"
