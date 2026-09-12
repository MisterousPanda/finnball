# FINNBALL Unity package

Add this folder as a local UPM package (`Window → Package Manager → + → Add package from disk…`, pick `package.json`).

Then **FINNBALL → Import Court**, point it at `interop/finnball.json`, and pick an arena. The builder spawns a Y-up metre court that matches the Bevy sim: floor, paints, hoops, ball, and six capsule players on the tip-off spots.

You can also drop in `interop/court.gltf` from `python3 scripts/finnball_dcc.py gltf` if you want a mesh instead of primitives.
