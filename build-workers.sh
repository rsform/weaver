#!/usr/bin/env bash
set -euo pipefail

echo "==> Building worker WASMs"
export RUSTFLAGS='--cfg getrandom_backend="wasm_js"'
cargo build -p weaver-app --bin editor_worker --bin embed_worker \
    --target wasm32-unknown-unknown --release \
    --no-default-features --features "web","collab-worker","use-index"

echo "==> Running wasm-bindgen"
wasm-bindgen target/wasm32-unknown-unknown/release/editor_worker.wasm \
    --out-dir crates/weaver-app/public \
    --target no-modules \
    --no-typescript
wasm-bindgen target/wasm32-unknown-unknown/release/embed_worker.wasm \
    --out-dir crates/weaver-app/public \
    --target no-modules \
    --no-typescript

echo "==> Done"
ls -lh crates/weaver-app/public/*worker*
