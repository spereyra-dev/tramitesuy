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
FROM --platform=$BUILDPLATFORM rust:1.94.1-slim AS build
# TARGETARCH must be re-declared inside the stage: a pre-FROM global ARG is
# not in scope inside a stage (Docker's documented scoping), so without this
# line $TARGETARCH expands empty and the native branch below always runs,
# shipping builder-architecture binaries under `--platform linux/arm64`.
ARG TARGETARCH
WORKDIR /build

# Manifests and toolchain first for layer caching of the dependency build.
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
# Only the two Rust workspace members: copying all of apps/ would pull the
# Next.js frontend (node_modules/.next) into the Rust build context.
COPY apps/api ./apps/api
COPY apps/ingest ./apps/ingest
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

# [audit F6] Architecture assertion: the binaries copied to /build/bin must
# match the requested $TARGETARCH, so a builder-architecture artifact fails
# the build loudly instead of shipping. `readelf -h` prints
# `Machine: AArch64` for aarch64 and `Machine: Advanced Micro Devices X86-64`
# for amd64. binutils ships as a dependency of the image's gcc toolchain and
# is installed defensively if a future base drops it.
RUN set -eu; \
    command -v readelf >/dev/null 2>&1 \
      || { apt-get update \
           && apt-get install -y --no-install-recommends binutils \
           && rm -rf /var/lib/apt/lists/*; }; \
    target_arch="${TARGETARCH:-}"; \
    if [ -z "$target_arch" ]; then \
      case "$(uname -m)" in \
        aarch64|arm64) target_arch="arm64" ;; \
        x86_64|amd64) target_arch="amd64" ;; \
        *) echo "unsupported builder architecture '$(uname -m)'" >&2; exit 1 ;; \
      esac; \
    fi; \
    case "$target_arch" in \
      arm64) expected_machine="AArch64" ;; \
      amd64) expected_machine="Advanced Micro Devices X86-64" ;; \
      *) echo "unsupported TARGETARCH='$target_arch' (expected arm64 or amd64)" >&2; exit 1 ;; \
    esac; \
    for binary in /build/bin/api /build/bin/ingest; do \
      actual_machine=$(readelf -h "$binary" \
        | sed -n 's/^[[:space:]]*Machine:[[:space:]]*//p'); \
      echo "architecture assertion: $binary Machine='$actual_machine' expected='$expected_machine' (target=$target_arch)"; \
      [ "$actual_machine" = "$expected_machine" ] \
        || { echo "FATAL: $binary is '$actual_machine', not '$expected_machine' for target='$target_arch'" >&2; exit 1; }; \
    done

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
