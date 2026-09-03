#!/usr/bin/env bash
# Upload the static web client (www/ + deploy/ + Dockerfile) to Railway.
#
# `railway up` honours .gitignore by default, and www/game + www/assets are
# gitignored build outputs — uploading with the gitignore in place ships a site
# with no game in it (every /game/* request 404s). Parking .gitignore is not
# enough either: inside a `git worktree` the CLI resolves the *parent* repo's
# .gitignore. So the upload ignores gitignore entirely and relies on
# .railwayignore, which keeps target/, .git and the worktrees out of the bundle
# (verified: a 300 MB decoy in target/ is not uploaded).
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -f www/game/finnball_bg.wasm || ! -d www/assets/audio ]]; then
  echo "www/game or www/assets missing — run ./scripts/build-web.sh first" >&2
  exit 1
fi

railway up --detach --no-gitignore "$@"
