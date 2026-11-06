set shell := ["bash", "-euo", "pipefail", "-c"]

# Repository verification and CI workflows.
mod ci 'just/ci.just'
# Container tooling and image builds.
mod container 'just/container.just'
# Rust coverage reports and enforcement.
mod coverage 'just/coverage.just'
# Production deployment lifecycle.
mod production 'just/production.just'
# Rust toolchain, development, and execution.
mod rust 'just/rust.just'
# Dependency, repository, and image security checks.
mod security 'just/security.just'
# Coverage-backed project site generation.
mod site 'just/site.just'
# Docker Compose stack lifecycle.
mod stack 'just/stack.just'
# Surfpool tooling and opt-in tests.
mod surfpool 'just/surfpool.just'

default:
    @just --list --list-submodules
