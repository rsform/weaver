# Build stage with cargo-chef for dependency caching
FROM rust:1-trixie AS chef
# Pin nightly version for reproducibility
RUN rustup default nightly-2025-12-04 && rustup component add rust-src --toolchain nightly-2025-12-04
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install wasm target and tools
RUN rustup target add wasm32-unknown-unknown
RUN cargo install wasm-bindgen-cli

# Install dioxus-cli
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall dioxus-cli --root /usr/local -y --force

# Cook dependencies first (cached layer)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Copy source code
COPY . .

# Use production env for build-time constants
RUN cp crates/weaver-app/.env-prod crates/weaver-app/.env

# Build wasm workers
RUN RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo build -p weaver-app --bin editor_worker --bin embed_worker \
    --target wasm32-unknown-unknown --release \
    --no-default-features --features "web"

# Run wasm-bindgen on workers
RUN wasm-bindgen target/wasm32-unknown-unknown/release/editor_worker.wasm \
    --out-dir crates/weaver-app/public \
    --target no-modules \
    --no-typescript
RUN wasm-bindgen target/wasm32-unknown-unknown/release/embed_worker.wasm \
    --out-dir crates/weaver-app/public \
    --target no-modules \
    --no-typescript

# Bundle the app
RUN dx bundle --release --debug-symbols false -p weaver-app

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the bundled app
COPY --from=builder /app/target/dx/weaver-app/release/web/ /app/

ENV PORT=8080
ENV IP=0.0.0.0

EXPOSE 8080

ENTRYPOINT ["/app/weaver-app"]
