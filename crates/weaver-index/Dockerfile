# Build stage
FROM rust:1.91-trixie AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build release binary
RUN cargo build --release -p weaver-index --bin indexer

# Runtime stage
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/indexer /app/indexer

# Default environment variables
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1
ENV CLICKHOUSE_URL=http://clickhouse:8123
ENV CLICKHOUSE_DATABASE=default

# Run the indexer
ENTRYPOINT ["/app/indexer"]
