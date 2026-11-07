# Observability changelog

## Unreleased

- Corrected the overview attention count to union delinquency, last-vote lag, and skip-rate conditions per watched validator, so missing samples cannot hide a known unhealthy state and overlapping conditions count once.
- Added a persistent light/dark Pages theme with system-default initialization, pre-paint application, accessible controls, and responsive homepage and 404 surfaces.
- Published content-fingerprinted Pages styles and interactions so the dashboard-led homepage remains coherent across browser and CDN cache boundaries.
- Rebuilt the Pages homepage as a responsive dashboard-led product story with real Overview, Leadership, Consensus, and Economics captures, accessible gallery controls, operator workflow guidance, and a repository-owned local quick start.
- Added build, active-configuration, reload-outcome, command-fallback, block-fee cache, and provider-method request metrics plus tested alerts for rejected reloads, sustained failover, and ineffective block-fee caching.
- Provisioned the production Prometheus scrape, bounded retention, Alertmanager routing/inhibition, and a synthetic notification-delivery check while retaining operator ownership of the external receiver.
- Added watched-validator filtering, reward snapshots, data-availability status, and per-validator inflation-reward, reward-epoch block, and reward-per-block history panels to the economics dashboard while separating configured-target panels and rendering absent reward samples as no data.
- Added watched-validator scheduled leader-slot count, slots-per-hour frequency, and bounded upcoming-schedule metrics plus leadership panels for producer frequency, schedule, and share of all activated network stake; schedules remain populated when either epoch schedule is independently available.
- Added recent validator block-fee reward, slot, bounded-sample average, and sample-count metrics plus current-value and time-range economics panels that include Harmonic priority fees without claiming unavailable provider attribution.
- Separated the configured economics validator's Solana protocol inflation reward and sample epoch from Jito MEV reward and settled epoch in adjacent current-value and time-range history sections.
- Added economics-target Jito MEV reward, commission, and settled-epoch metrics plus mode-gated economics dashboard panels and provider-API connector inventory metadata.
- Added bounded cluster transaction and slot throughput metrics, consensus throughput and stake-distribution panels, and a leadership skip-rate gap against the all-network baseline.
- Made every overview node-local panel distinguish an unconfigured `node_status` route from unavailable node series, preserve missing structured catch-up distance as no data, and identify each network in multi-network selections.
- Added bounded node identity, validator software, block height, health, catch-up distance, and vote-set metrics, an overview dashboard row, and freshness-gated node-behind and node-not-voting alerts.
- Added connector-attributed cumulative WebSocket probe failure metrics, overview dashboard visibility, and a tested repeated-failure alert.
- Preserved last-successful snapshot data and timestamps across recoverable failed polling rounds while failure, mode-health, readiness, and stale-snapshot signals continue to update.
- Added snapshot-age, configured poll-interval, and RPC reliability metrics, an overview freshness panel, and poll-aware freshness-gated Prometheus alerts for watcher, validator, leadership, and VAT-funding risks.
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
