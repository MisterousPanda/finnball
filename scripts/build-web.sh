#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
rustup target add wasm32-unknown-unknown
if ! command -v wasm-bindgen >/dev/null 2>&1; then
  cargo install wasm-bindgen-cli --locked
fi
echo "Building FINNBALL wasm (this can take a while)..."
cargo build --profile wasm-release --target wasm32-unknown-unknown
mkdir -p www/pkg
wasm-bindgen --target web --out-dir www/pkg --no-typescript \
  target/wasm32-unknown-unknown/wasm-release/finnball.wasm
echo "Web build ready → www/"
echo "Railway: keep www/pkg local (gitignored), then: railway up"
