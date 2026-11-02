# Observability changelog

## Unreleased

- Added Prometheus network, validator, economics, connector, and selected-source metrics.
- Added provisioned Grafana overview and breakdown dashboards plus the coverage-backed Pages site.
- Moved Prometheus and Grafana configuration beside their Compose owners under `docker/`.
- Read connector metadata directly from the sole ordered configuration list while preserving connector-kind labels and precedence.
- Removed obsolete validator-mode and WebSocket-toggle labels from watcher configuration metadata; transport labels are derived from connectors.
