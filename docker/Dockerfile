# Build callers replace this sentinel with the Rust pin from mise.toml.
ARG RUST_VERSION=required
FROM rust:${RUST_VERSION}-bookworm AS builder

WORKDIR /app
COPY Cargo.toml ./
COPY Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/sol-validator-watcher /usr/local/bin/sol-validator-watcher
COPY config ./config

EXPOSE 8000

ENTRYPOINT ["sol-validator-watcher"]
CMD ["--config", "/app/config/local/validator/watcher.yaml"]
