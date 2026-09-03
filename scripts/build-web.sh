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
# Smaller wasm = less for Safari to JIT and hold in memory on phones.
if command -v wasm-opt >/dev/null 2>&1; then
  echo "wasm-opt -Os ..."
  wasm-opt -Os --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals --enable-reference-types --enable-simd \
    -o www/pkg/finnball_bg.opt.wasm www/pkg/finnball_bg.wasm
  mv -f www/pkg/finnball_bg.opt.wasm www/pkg/finnball_bg.wasm
else
  echo "wasm-opt not found (apt install binaryen) — shipping unoptimised wasm"
fi
mkdir -p www/assets www/game
rm -rf www/assets/audio www/assets/shaders www/assets/env
cp -r assets/audio www/assets/audio
cp -r assets/shaders www/assets/shaders
# World Labs panoramas (scripts/worldlabs_env.py); optional until generated.
[[ -d assets/env ]] && cp -r assets/env www/assets/env
# www/pkg is gitignored; Railway `up` honors gitignore, so stage a deploy copy.
cp -f www/pkg/finnball.js www/pkg/finnball_bg.wasm www/game/
echo "Web build ready → www/ (WASM + audio + shader + env assets)"
echo "Railway: ./scripts/deploy-railway.sh (plain railway up drops the gitignored www/game)"
