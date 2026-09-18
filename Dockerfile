# TrámitesUY multi-stage image (task 88, D-6): builds the Rust workspace
# hermetically (the committed `.sqlx` offline cache means the build never
# touches a database) and ships the `api` and `ingest` binaries plus the
# YAML taxonomy seed on a slim runtime.
#
# Build stage: rustls-based TLS everywhere (sqlx tls-rustls, reqwest
# rustls-tls), so no pkg-config/openssl system packages are required.
FROM rust:1.94.1-slim AS build
WORKDIR /build

# Manifests and toolchain first for layer caching of the dependency build.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY apps ./apps
COPY migrations ./migrations
COPY .sqlx ./.sqlx
ENV SQLX_OFFLINE=true
RUN cargo build --release -p api -p ingest

# Runtime stage: Debian slim + CA certificates for the live CKAN calls.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /build/target/release/api /usr/local/bin/api
COPY --from=build /build/target/release/ingest /usr/local/bin/ingest
# The YAML taxonomy is the ranker's source of truth (design §4.2) and the
# seed input; it ships with the image (TRAMITESUY_DATA_DIR=/app/data).
COPY data ./data
