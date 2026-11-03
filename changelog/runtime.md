# Runtime changelog

## Unreleased

- Split the binary entrypoint into runtime application, HTTP, polling, reload state, and single-owner watcher-state modules while preserving process behavior.
- Changed the default CLI profile path to `config/local/validator.toml` while preserving local-validator behavior.
- Added supervised polling, metrics HTTP serving, configuration reload, liveness updates, signal handling, and cooperative task shutdown.
- Built connector clients and compiled command routes once in a reusable source chain.
- Rebuilt the complete source chain transactionally when connector definitions or mode command routes change on reload.
- Verified declared connector RPC capabilities before the first poll and before committing a changed reload candidate; startup failures terminate through task supervision, while failed reload probes preserve the last-known-good runtime.
- Added a supervised single-owner watcher-state actor with 30-second reports and a `/readyz` admission gate that requires five consecutive required polling rounds without a failed RPC attempt; transport, protocol, and non-success HTTP failures plus accepted reloads reset admission while `/healthz` retains process-liveness semantics.
