# syntax=docker/dockerfile:1

# Nest server — multi-stage build.
# Produces a small Debian-based image with the `nest` binary and a `/data`
# volume for the SQLite database and Egg archives.

# ---------------------------------------------------------------------------
# Builder stage
# ---------------------------------------------------------------------------
FROM rust:1-bookworm AS builder

WORKDIR /usr/src/nest

# Install system dependencies for the Rust build.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    pkg-config \
    libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency layer: copy workspace manifests and lock file first.
COPY Cargo.toml Cargo.lock ./
COPY nest/Cargo.toml ./nest/Cargo.toml
COPY shared/Cargo.toml ./shared/Cargo.toml
COPY bird/src-tauri/Cargo.toml ./bird/src-tauri/Cargo.toml

# Dummy sources so `cargo build` can compile dependencies.
RUN mkdir -p nest/src shared/src bird/src-tauri/src && \
    echo "fn main() {}" > nest/src/main.rs && \
    echo "" > shared/src/lib.rs && \
    echo "fn main() {}" > bird/src-tauri/src/main.rs

# Build dependencies. This will be cached unless manifests change.
RUN cargo build --release -p nest-server && \
    rm -rf nest/src shared/src bird/src-tauri/src

# Now copy the real source and build the final binary.
COPY nest ./nest
COPY shared ./shared
COPY migrations ./migrations

# Touch the main file to force a rebuild of the real source.
RUN touch nest/src/main.rs && \
    cargo build --release -p nest-server

# ---------------------------------------------------------------------------
# Runtime stage
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -r nest && useradd -r -g nest -s /sbin/nologin -d /data nest

WORKDIR /data
VOLUME ["/data"]

COPY --from=builder /usr/src/nest/target/release/nest /usr/local/bin/nest

EXPOSE 8140

ENV NEST_BIND_ADDR=0.0.0.0:8140 \
    NEST_DATA_DIR=/data \
    NEST_LOG=info \
    RUST_BACKTRACE=1

USER nest

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD ["sh", "-c", "curl -fsS http://127.0.0.1:8140/health || exit 1"]

ENTRYPOINT ["nest"]
