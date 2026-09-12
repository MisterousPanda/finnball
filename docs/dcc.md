# Unity, Unreal, and Blender

FINNBALL's playable game stays in **Bevy**. The court sizes, tip-off spots, ten legends, and five arena palettes are still authored in Rust (`src/sim.rs`, `src/roster.rs`, `src/arenas.rs`). This folder is the art / engine bridge:

```
interop/finnball.json     ← generated from Rust, committed
scripts/finnball_dcc.py   ← validate, convert spaces, write court.gltf
blender/finnball/         ← Blender 4.2 add-on
unity/com.finnball.interop/   ← Unity UPM package
unreal/Plugins/Finnball/  ← Unreal Engine 5.5 plugin
```

If you change a rating, a hoop, or an arena color in Rust, regenerate the JSON:

```bash
UPDATE_INTEROP=1 cargo test interop::
python3 scripts/finnball_dcc.py validate
```

CI fails if the committed JSON drifts from the Rust source.

## Coordinate spaces

| App | Up | Units | Court +X | Player front |
| --- | --- | --- | --- | --- |
| Bevy / Unity | Y | metres | away hoop | −Z |
| Blender | Z | metres | +X | −Y (after convert) |
| Unreal | Z | centimetres | +X | −Y (after convert) |

Convert a Bevy point `(x, y, z)` with:

```bash
python3 scripts/finnball_dcc.py convert --to blender --x -12.55 --y 3.05 --z 0
python3 scripts/finnball_dcc.py convert --to unreal --x -12.55 --y 3.05 --z 0
```

- Blender: `(x, z, y)`
- Unreal: `(x * 100, z * 100, y * 100)`

## Blender

Install `blender/finnball` (Preferences → Add-ons → Install from Disk). **File → Import → FINNBALL Manifest** and pick `interop/finnball.json`, or search for **FINNBALL: Load repo manifest**. That builds an editable court collection you can sculpt, bake, and export as glTF.

## Unity

`Window → Package Manager → + → Add package from disk…` → `unity/com.finnball.interop/package.json`. Then **FINNBALL → Import Court**. Same Y-up metre space as the Bevy sim.

## Unreal

Copy `unreal/Plugins/Finnball` into a UE 5.5 project (or open `unreal/Finnball.uproject`). `UFinnballLibrary::BevyToUnreal` is the coordinate helper. To spawn primitives from the JSON, run `unreal/Scripts/import_finnball.py` in the editor Python console.

## Shared glTF

```bash
python3 scripts/finnball_dcc.py gltf --arena sky_temple --out interop/court.gltf
```

Unity, Unreal (glTF plugin), and Blender all open that file. It is a blocking / layout mesh, not the live Bevy crowd or anime rigs.
