#!/usr/bin/env python3
"""Build a FINNBALL court inside the Unreal Editor from interop/finnball.json.

Run in the editor Output Log:

    py unreal/Scripts/import_finnball.py

Or from a shell that has the Unreal Python API:

    unreal.execute_python_script(".../import_finnball.py")
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

try:
    import unreal
except ImportError:
    unreal = None

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = ROOT / "interop" / "finnball.json"


def bevy_to_unreal(x: float, y: float, z: float):
    return (x * 100.0, z * 100.0, y * 100.0)


def hex_to_linear(value: str):
    r = int(value[1:3], 16) / 255.0
    g = int(value[3:5], 16) / 255.0
    b = int(value[5:7], 16) / 255.0
    return (r, g, b, 1.0)


def _spawn_cube(name, scale, location, color):
    actor = unreal.EditorLevelLibrary.spawn_actor_from_class(
        unreal.StaticMeshActor, unreal.Vector(*location)
    )
    actor.set_actor_label(name)
    mesh = actor.static_mesh_component
    cube = unreal.EditorAssetLibrary.load_asset("/Engine/BasicShapes/Cube")
    if cube:
        mesh.set_static_mesh(cube)
    mesh.set_world_scale3d(unreal.Vector(*scale))
    return actor


def build(arena_id: str = "neo_tokyo", manifest_path: Path = MANIFEST) -> None:
    if unreal is None:
        raise SystemExit("Run this script inside the Unreal Editor Python interpreter.")
    data = json.loads(manifest_path.read_text())
    arena = next(a for a in data["arenas"] if a["id"] == arena_id)
    court = data["court"]
    pal = arena["palette"]

    hl, hw = court["half_length"], court["half_width"]
    _spawn_cube(
        "FINNBALL_Floor",
        (hl * 2, hw * 2, 0.08),
        bevy_to_unreal(0.0, -0.04, 0.0),
        hex_to_linear(pal["floor_a"]),
    )
    for hoop in data["hoops"]:
        sign = 1.0 if hoop["position"]["x"] > 0 else -1.0
        hx, hy, hz = hoop["position"]["x"], hoop["position"]["y"], hoop["position"]["z"]
        paint_x = hx - sign * (court["paint_depth"] * 0.5)
        _spawn_cube(
            f"FINNBALL_Paint_{hoop['id']}",
            (court["paint_depth"], court["paint_half_width"] * 2, 0.02),
            bevy_to_unreal(paint_x, 0.01, hz),
            hex_to_linear(pal["paint"]),
        )
        board_x = hx + sign * court["backboard_offset"]
        _spawn_cube(
            f"FINNBALL_Backboard_{hoop['id']}",
            (0.06, 1.8, 1.05),
            bevy_to_unreal(board_x, hy + 0.15, hz),
            hex_to_linear(pal["line"]),
        )
        _spawn_cube(
            f"FINNBALL_Pole_{hoop['id']}",
            (0.12, 0.12, hy),
            bevy_to_unreal(board_x + sign * 0.35, hy * 0.5, hz),
            (0.12, 0.12, 0.14, 1.0),
        )
    ball = data["spawns"]["ball_home"]
    _spawn_cube(
        "FINNBALL_Ball",
        (court["ball_radius"] * 2,) * 3,
        bevy_to_unreal(ball["x"], ball["y"], ball["z"]),
        (0.85, 0.35, 0.12, 1.0),
    )
    for team in data["teams"]:
        spots = data["spawns"][team["id"]]
        for i, spot in enumerate(spots):
            _spawn_cube(
                f"FINNBALL_Player_{team['id']}_{i}",
                (court["player_radius"] * 2, court["player_radius"] * 2, 1.88),
                bevy_to_unreal(spot["x"], 0.94, spot["z"]),
                hex_to_linear(team["primary"]),
            )
    unreal.log(f"FINNBALL built {arena['name']}")


if __name__ == "__main__":
    arena = sys.argv[1] if len(sys.argv) > 1 else "neo_tokyo"
    if unreal is None:
        data = json.loads(MANIFEST.read_text())
        pt = bevy_to_unreal(-12.55, 3.05, 0.0)
        assert pt == (-1255.0, 0.0, 305.0)
        print(f"dry-run ok ({len(data['roster'])} legends, arena={arena})")
    else:
        build(arena)
