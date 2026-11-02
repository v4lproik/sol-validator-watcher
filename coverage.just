set shell := ["bash", "-euo", "pipefail", "-c"]

summary:
  just rust _ensure-components
  mise exec -- cargo llvm-cov --workspace --all-features --summary-only --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

check threshold="80":
  just rust _ensure-components
  mise exec -- cargo llvm-cov --workspace --all-features --summary-only --fail-under-lines {{quote(threshold)}} --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

html:
  just rust _ensure-components
  mise exec -- cargo llvm-cov --workspace --all-features --html --output-dir coverage --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'

lcov:
  just rust _ensure-components
  mise exec -- cargo llvm-cov --workspace --all-features --lcov --output-path lcov.info --ignore-filename-regex '(^|/)src/(main|surfpool_tests)\.rs$'
