#!/usr/bin/env bash
# Upload the static web client (www/ + deploy/ + Dockerfile) to Railway.
#
# `railway up` honours .gitignore, and www/game + www/assets are gitignored build
# outputs — uploading with the gitignore in place ships a site with no game in it.
# The gitignore is parked for the duration of the upload; .railwayignore still
# keeps target/, .git and the worktrees out of the bundle.
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -f www/game/finnball_bg.wasm || ! -d www/assets/audio ]]; then
  echo "www/game or www/assets missing — run ./scripts/build-web.sh first" >&2
  exit 1
fi

restore() { [[ -f .gitignore.railway-parked ]] && mv .gitignore.railway-parked .gitignore; }
trap restore EXIT
mv .gitignore .gitignore.railway-parked

railway up --detach "$@"
