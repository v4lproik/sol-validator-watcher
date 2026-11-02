set shell := ["bash", "-euo", "pipefail", "-c"]

verify:
  just rust fmt
  just rust clippy
  just rust build
  just rust test

full:
  just rust fmt
  just rust clippy
  just rust test
  just coverage check
  just surfpool test
