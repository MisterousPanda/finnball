#!/usr/bin/env bash
# Upload the static web client (www/ + deploy/ + Dockerfile) to Railway.
#
# Two traps, both of which have shipped a game-less or stale site:
#  - `railway up` honours .gitignore by default, and www/game + www/assets are
#    gitignored build outputs. Hence --no-gitignore; .railwayignore keeps
#    target/, .git and the worktrees out of the bundle.
#  - The CLI's project link is keyed by the git root, so from a `git worktree`
#    it archives the *main* checkout (wrong branch, old www/). Hence the explicit
#    path argument with --path-as-root, which archives exactly this directory.
set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -f www/game/finnball_bg.wasm || ! -d www/assets/audio ]]; then
  echo "www/game or www/assets missing — run ./scripts/build-web.sh first" >&2
  exit 1
fi

railway up "$PWD" --path-as-root --detach --no-gitignore "$@"
