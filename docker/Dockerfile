# Build callers replace this sentinel with the Rust pin from mise.toml.
ARG RUST_VERSION=required
FROM rust:${RUST_VERSION}-bookworm AS builder

WORKDIR /app
COPY Cargo.toml ./
COPY Cargo.lock ./
COPY src ./src
COPY benches ./benches
RUN cargo build --locked --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && groupadd --gid 10001 watcher \
    && useradd --uid 10001 --gid watcher --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin watcher \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/sol-validator-watcher /usr/local/bin/sol-validator-watcher
COPY config ./config

EXPOSE 8000

HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 CMD ["sol-validator-watcher", "healthcheck"]
STOPSIGNAL SIGINT
USER watcher:watcher

ENTRYPOINT ["sol-validator-watcher"]
CMD ["--config", "/app/config/local/validator.toml"]
