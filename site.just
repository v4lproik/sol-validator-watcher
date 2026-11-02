set shell := ["bash", "-euo", "pipefail", "-c"]

build:
  just coverage html
  bash ./scripts/build-pages-site.sh
