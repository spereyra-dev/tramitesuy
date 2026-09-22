# TrámitesUY multi-stage image (task 88, D-6): builds the Rust workspace
# hermetically (the committed `.sqlx` offline cache means the build never
# touches a database) and ships the `api` and `ingest` binaries plus the
# YAML taxonomy seed on a slim runtime.
#
# Build stage: rustls-based TLS everywhere (sqlx tls-rustls, reqwest
# rustls-tls), so no pkg-config/openssl system packages are required.
#
# Task 40 (S13, design §8): a plain build keeps the native target (dev
# compose builds unchanged, TRIANGULATE); `docker buildx build --platform
# linux/arm64` requests the release ARM64 (aarch64-unknown-linux-gnu) build
# through the GNU cross toolchain — no QEMU emulation of the Rust stage.
# The stage runs on the build machine's architecture ($BUILDPLATFORM) in
# both cases.
ARG TARGETARCH
FROM --platform=$BUILDPLATFORM rust:1.94.1-slim AS build
WORKDIR /build

# Manifests and toolchain first for layer caching of the dependency build.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY apps ./apps
COPY migrations ./migrations
COPY .sqlx ./.sqlx
ENV SQLX_OFFLINE=true
RUN if [ "$TARGETARCH" = "arm64" ] && [ "$(uname -m)" != "aarch64" ]; then \
      rustup target add aarch64-unknown-linux-gnu \
      && apt-get update \
      && apt-get install -y --no-install-recommends gcc-aarch64-linux-gnu libc6-dev-arm64-cross \
      && rm -rf /var/lib/apt/lists/* \
      && CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
         CC_AARCH64_UNKNOWN_LINUX_GNU=aarch64-linux-gnu-gcc \
         CXX_AARCH64_UNKNOWN_LINUX_GNU=aarch64-linux-gnu-g++ \
         cargo build --release --target aarch64-unknown-linux-gnu -p api -p ingest \
      && mkdir -p /build/bin \
      && cp target/aarch64-unknown-linux-gnu/release/api target/aarch64-unknown-linux-gnu/release/ingest /build/bin/ ; \
    else \
      cargo build --release -p api -p ingest \
      && mkdir -p /build/bin \
      && cp target/release/api target/release/ingest /build/bin/ ; \
    fi

# Runtime stage: Debian slim + CA certificates for the live CKAN calls;
# curl serves the /ready-based healthchecks (compose prod profile).
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /build/bin/api /usr/local/bin/api
COPY --from=build /build/bin/ingest /usr/local/bin/ingest
# The YAML taxonomy is the ranker's source of truth (design §4.2) and the
# seed input; it ships with the image (TRAMITESUY_DATA_DIR=/app/data).
COPY data ./data
