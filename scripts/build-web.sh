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
mkdir -p www/assets www/game
rm -rf www/assets/audio
cp -r assets/audio www/assets/audio
# www/pkg is gitignored; Railway `up` honors gitignore, so stage a deploy copy.
cp -f www/pkg/finnball.js www/pkg/finnball_bg.wasm www/game/
echo "Web build ready → www/ (WASM + audio assets)"
echo "Railway: www/game is the uploaded client; then: railway up"
