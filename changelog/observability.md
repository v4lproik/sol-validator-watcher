# Observability changelog

## Unreleased

- Split Prometheus registration, state updates, rendering, and tests into an observability module tree without changing the exported metric contract.
- Added Rust documentation for Prometheus registration, reset behavior, watcher-state publication, and completed-snapshot updates.
- Clarified `sol_watcher_ready` help to state that failed RPC attempts and non-2xx responses both reset five-round admission.
- Added Prometheus network, validator, economics, and connector metrics.
- Added the coverage-backed Pages site.
- Moved Prometheus and Grafana configuration beside their Compose owners under `docker/`.
- Published connector inventory, configured command routes, and successful command sources as separate bounded metrics.
- Removed obsolete validator-mode and WebSocket-toggle labels from watcher configuration metadata; transport labels are derived from connectors.
- Added bounded mode enablement, health, configured-route, and selected-command-source metrics; mode cards distinguish disabled, pending, failed, and successful states, the last completed mode and source series remain visible during collection, completed snapshots replace the full set when applied, and collection failures mark affected modes down.
- Split Grafana into overview, consensus, leadership, and economics dashboards with mode-specific routing and provenance panels.
- Replaced connector-kind labels with provider, transport, and optional feature labels; published declared connector features as a bounded metric and exposed transport separately from features on every route and source dashboard.
- Added bounded connector-declared `solana_rpc_compatibility` and command-required `required_solana_rpc_compatibility` labels to connector, route, and selected-source metrics, and exposed both standard Solana method profiles independently of provider features in every routing and provenance dashboard table.
- Published watcher readiness, completed rounds, clean-round streak, failed RPC attempts, HTTP response classes, and exact 200/403/429/500 counts, with overview panels separating process liveness from five-round admission.
- Added per-validator `ACTIVE` and `DELINQUENT` voting-state cards to the overview dashboard's operator-state row, with the detailed watched-validator table directly below that row.
- Added a network-aware, multi-select `Validators` filter to every watched-validator panel in the overview dashboard, with `All` as the default.
- Added epoch progress, slot-time and epoch-end estimates, per-validator next-leader timing, economics-target balances, rent exemption, and 0.8 SOL VAT runway metrics plus operator-facing economics panels.
- Omitted vote-credit sample, total, and efficiency series when no bounded epoch sample is available, preventing missing data from rendering as zero.
