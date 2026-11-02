set shell := ["bash", "-euo", "pipefail", "-c"]

# Repository verification and CI workflows.
mod ci
# Container tooling and image builds.
mod container
# Rust coverage reports and enforcement.
mod coverage
# Rust toolchain, development, and execution.
mod rust
# Coverage-backed project site generation.
mod site
# Docker Compose stack lifecycle.
mod stack
# Surfpool tooling and opt-in tests.
mod surfpool

default:
    @just --list --list-submodules
