set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '..'

_ensure-components:
  mise exec -- rustup component add rustfmt clippy llvm-tools

doctor: _ensure-components
  command -v mise >/dev/null 2>&1
  command -v just >/dev/null 2>&1
  mise exec -- cargo --version
  mise exec -- cargo fmt --version
  mise exec -- cargo clippy --version
  mise exec -- cargo llvm-cov --version
  just --version

fmt: _ensure-components
  mise exec -- cargo fmt

check:
  mise exec -- cargo check

clippy: _ensure-components
  mise exec -- cargo clippy --all-targets --all-features -- -D warnings

test:
  mise exec -- cargo test

build profile="debug":
  profile={{quote(profile)}}; case "${profile}" in debug) mise exec -- cargo build ;; release) mise exec -- cargo build --release ;; *) echo "profile must be debug or release: ${profile}" >&2; exit 2 ;; esac

run profile="debug" config="config/local/validator/watcher.yaml":
  profile={{quote(profile)}}; \
  case "${profile}" in debug|release) ;; *) echo "profile must be debug or release: ${profile}" >&2; exit 2 ;; esac; \
  set -a; \
  [ ! -f .env ] || source .env; \
  set +a; \
  if [[ "${profile}" == release ]]; then \
    mise exec -- cargo run --release -- --config {{quote(config)}}; \
  else \
    mise exec -- cargo run -- --config {{quote(config)}}; \
  fi

clean:
  mise exec -- cargo clean
