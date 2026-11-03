# Runtime changelog

## Unreleased

- Changed the default CLI profile path to `config/local/validator.toml` while preserving local-validator behavior.
- Added supervised polling, metrics HTTP serving, configuration reload, liveness updates, signal handling, and cooperative task shutdown.
- Built connector clients and compiled command routes once in a reusable source chain.
- Rebuilt the complete source chain transactionally when connector definitions or mode command routes change on reload.
- Verified declared connector RPC capabilities before the first poll and before committing a changed reload candidate; startup failures terminate through task supervision, while failed reload probes preserve the last-known-good runtime.
