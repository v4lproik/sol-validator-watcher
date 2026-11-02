# Observability changelog

## Unreleased

- Added Prometheus network, validator, economics, and connector metrics.
- Added the coverage-backed Pages site.
- Moved Prometheus and Grafana configuration beside their Compose owners under `docker/`.
- Published connector inventory, configured command routes, and successful command sources as separate bounded metrics.
- Removed obsolete validator-mode and WebSocket-toggle labels from watcher configuration metadata; transport labels are derived from connectors.
- Added bounded mode enablement, health, configured-route, and selected-command-source metrics; selected sources reset at the start of every poll.
- Split Grafana into overview, consensus, leadership, and economics dashboards with mode-specific routing and provenance panels.
- Replaced connector-kind labels with provider, transport, and optional feature labels; published declared connector features as a bounded metric and exposed transport separately from features on every route and source dashboard.
