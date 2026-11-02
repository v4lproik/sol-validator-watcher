set shell := ["bash", "-euo", "pipefail", "-c"]
set working-directory := '..'

build:
  just coverage html
  bash ./scripts/build-pages-site.sh
