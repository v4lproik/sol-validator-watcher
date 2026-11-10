set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '..'

_require-cli:
  if ! command -v surfpool >/dev/null 2>&1; then echo "surfpool CLI not found. Run: just surfpool install" >&2; exit 1; fi

install destination="":
  destination={{quote(destination)}}; if [[ -n "${destination}" ]]; then scripts/install-surfpool.sh "${destination}"; else scripts/install-surfpool.sh; fi

doctor:
  command -v surfpool >/dev/null 2>&1
  surfpool --version

test scope="smoke": _require-cli
  scope={{quote(scope)}}; case "${scope}" in smoke) SURFPOOL_E2E=1 mise exec -- cargo test -p sol-validator-watcher surfpool_smoke_test_collects_snapshot_and_renders_metrics -- --nocapture ;; all) SURFPOOL_E2E=1 mise exec -- cargo test --workspace ;; *) echo "Surfpool test scope must be smoke or all: ${scope}" >&2; exit 2 ;; esac
