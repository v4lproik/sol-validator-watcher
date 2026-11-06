# Build callers replace this sentinel with the Rust pin from mise.toml.
ARG RUST_VERSION=required
FROM rust:${RUST_VERSION}-bookworm@sha256:7085845219c18e7c5b1f34ed9ce89b84f822f2e3d54a02774cedc996ac0f304f AS builder
ARG SOL_WATCHER_BUILD_REVISION=unknown

WORKDIR /app
COPY Cargo.toml ./
COPY Cargo.lock ./
COPY src ./src
COPY benches ./benches
RUN SOL_WATCHER_BUILD_REVISION="${SOL_WATCHER_BUILD_REVISION}" cargo build --locked --release

FROM debian:12.15-slim@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc

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
