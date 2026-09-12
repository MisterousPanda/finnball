#!/usr/bin/env bash
# Build the Unity WebGL client headlessly and stage it at www/unity/ so the same
# nginx container that serves the Bevy game also serves the Unity port at /unity/.
#
#   ./scripts/build-unity-web.sh            # scene + WebGL build + stage
#   ./scripts/build-unity-web.sh --stage    # only copy an existing build
#
# Needs the `unity` CLI (https://unity.com) with editor 6000.6.0f1 + WebGL module.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
PROJECT="unity/Finnball"
BUILD="$PROJECT/Build/WebGL"

if [[ "${1:-}" != "--stage" ]]; then
  command -v unity >/dev/null 2>&1 || { echo "unity CLI not found on PATH" >&2; exit 1; }
  echo "Regenerating court scene from interop/finnball.json ..."
  unity --no-banner --non-interactive run "$PROJECT" --timeout 900 -- \
    -executeMethod Finnball.Editor.FinnballBatch.BuildCourtScene -finnballArena "${FINNBALL_ARENA:-neo_tokyo}"
  echo "Building WebGL (several minutes) ..."
  unity --no-banner --non-interactive build "$PROJECT" --target WebGL \
    --execute-method Finnball.Editor.FinnballBatch.BuildWebGL -o "$ROOT/$BUILD" --no-tail \
    -l "$PROJECT/Logs/build-webgl.log"
fi

[[ -f "$BUILD/index.html" ]] || { echo "no build at $BUILD" >&2; exit 1; }
rm -rf www/unity
mkdir -p www/unity
cp -R "$BUILD"/. www/unity/
# Unity writes the compressed players 0600; nginx's worker runs unprivileged.
chmod -R a+rX www/unity
du -sh www/unity
echo "Unity WebGL staged → www/unity/  (serve www/ and open /unity/)"
