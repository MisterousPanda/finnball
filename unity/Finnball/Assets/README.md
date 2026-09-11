# FINNBALL Unity project

All code lives in the local package `unity/com.finnball.interop`. This folder only holds generated content:

- `Scenes/Court.unity` — written by `FinnballBatch.BuildCourtScene`
- `StreamingAssets/finnball.json` — synced copy of `interop/finnball.json` (gitignored)

Regenerate from the repo root:

```bash
unity run unity/Finnball -- -quit -executeMethod Finnball.Editor.FinnballBatch.BuildCourtScene -finnballArena neo_tokyo
unity test unity/Finnball --mode EditMode
unity build unity/Finnball --target WebGL --execute-method Finnball.Editor.FinnballBatch.BuildWebGL -o "$PWD/unity/Finnball/Build/WebGL"
# or simply: ./scripts/build-unity-web.sh
```
