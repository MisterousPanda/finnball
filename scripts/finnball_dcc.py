#!/usr/bin/env python3
"""FINNBALL DCC helpers — validate the interchange, convert spaces, write a court glTF.

    python3 scripts/finnball_dcc.py validate
    python3 scripts/finnball_dcc.py gltf --arena neo_tokyo --out interop/court.gltf
    python3 scripts/finnball_dcc.py convert --to unreal --x -12.55 --y 3.05 --z 0
    python3 scripts/finnball_dcc.py --self-test

The JSON is Y-up metres (Bevy / Unity). Blender is Z-up metres. Unreal is Z-up centimetres.
"""
from __future__ import annotations

import argparse
import json
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "interop" / "finnball.json"

SCHEMA = 1
ROSTER_SIZE = 10
ARENA_COUNT = 5


def load_manifest(path: Path = MANIFEST) -> dict:
    data = json.loads(path.read_text())
    validate_manifest(data)
    return data


def validate_manifest(data: dict) -> None:
    if data.get("schema") != SCHEMA:
        raise ValueError(f"unsupported schema {data.get('schema')}, expected {SCHEMA}")
    court = data["court"]
    for key in (
        "half_length",
        "half_width",
        "rim_height",
        "rim_radius",
        "backboard_offset",
        "hoop_x",
        "ball_radius",
        "three_radius",
        "paint_depth",
        "paint_half_width",
        "player_radius",
    ):
        if float(court[key]) <= 0:
            raise ValueError(f"court.{key} must be > 0")
    roster = data["roster"]
    if len(roster) != ROSTER_SIZE:
        raise ValueError(f"expected {ROSTER_SIZE} legends, got {len(roster)}")
    slugs = [c["id"] for c in roster]
    if len(set(slugs)) != len(slugs):
        raise ValueError("duplicate roster ids")
    arenas = data["arenas"]
    if len(arenas) != ARENA_COUNT:
        raise ValueError(f"expected {ARENA_COUNT} arenas, got {len(arenas)}")
    if len(data["hoops"]) != 2:
        raise ValueError("need home and away hoops")
    if len(data["spawns"]["home"]) != 3 or len(data["spawns"]["away"]) != 3:
        raise ValueError("3v3 tip-off needs three spots a side")
    for character in roster:
        ratings = character["ratings"]
        for stat, value in ratings.items():
            if not 0 <= int(value) <= 99:
                raise ValueError(f"{character['id']}.{stat} out of range")
        hex_color(character["look"]["accent"])
    for arena in arenas:
        hex_color(arena["palette"]["floor_a"])
        if "FINNBALL" not in arena["ribbon_words"]:
            raise ValueError(f"{arena['id']} ribbon is missing FINNBALL")


def hex_color(value: str) -> tuple[float, float, float]:
    if not isinstance(value, str) or len(value) != 7 or not value.startswith("#"):
        raise ValueError(f"expected #rrggbb, got {value!r}")
    r = int(value[1:3], 16) / 255.0
    g = int(value[3:5], 16) / 255.0
    b = int(value[5:7], 16) / 255.0
    return r, g, b


def to_blender(x: float, y: float, z: float) -> tuple[float, float, float]:
    """Bevy Y-up metres → Blender Z-up metres."""
    return x, z, y


def to_unreal(x: float, y: float, z: float) -> tuple[float, float, float]:
    """Bevy Y-up metres → Unreal Z-up centimetres."""
    return x * 100.0, z * 100.0, y * 100.0


def to_unity(x: float, y: float, z: float) -> tuple[float, float, float]:
    """Bevy and Unity share Y-up metres."""
    return x, y, z


def arena_by_id(data: dict, slug: str) -> dict:
    for arena in data["arenas"]:
        if arena["id"] == slug:
            return arena
    known = ", ".join(a["id"] for a in data["arenas"])
    raise KeyError(f"unknown arena {slug!r} (have {known})")


def write_court_gltf(data: dict, arena_slug: str, out: Path) -> None:
    """Minimal glTF 2.0 court: floor, paints, backboards, poles, rims, ball."""
    arena = arena_by_id(data, arena_slug)
    pal = arena["palette"]
    court = data["court"]
    builder = GltfBuilder()
    hl, hw = court["half_length"], court["half_width"]
    builder.add_box(
        "floor",
        size=(hl * 2, 0.08, hw * 2),
        center=(0.0, -0.04, 0.0),
        color=hex_color(pal["floor_a"]),
    )
    pd, pw = court["paint_depth"], court["paint_half_width"]
    for hoop in data["hoops"]:
        sign = 1.0 if hoop["position"]["x"] > 0 else -1.0
        paint_x = hoop["position"]["x"] - sign * (pd * 0.5)
        builder.add_box(
            f"paint_{hoop['id']}",
            size=(pd, 0.02, pw * 2),
            center=(paint_x, 0.01, 0.0),
            color=hex_color(pal["paint"]),
        )
        hx, hy, hz = hoop["position"]["x"], hoop["position"]["y"], hoop["position"]["z"]
        board_x = hx + sign * court["backboard_offset"]
        builder.add_box(
            f"backboard_{hoop['id']}",
            size=(0.06, 1.05, 1.8),
            center=(board_x, hy + 0.15, hz),
            color=hex_color(pal["line"]),
        )
        pole_x = board_x + sign * 0.35
        builder.add_box(
            f"pole_{hoop['id']}",
            size=(0.12, hy, 0.12),
            center=(pole_x, hy * 0.5, hz),
            color=(0.12, 0.12, 0.14),
        )
        builder.add_torus(
            f"rim_{hoop['id']}",
            radius=court["rim_radius"],
            tube=0.02,
            center=(hx, hy, hz),
            color=(1.0, 0.45, 0.08),
        )
    ball = data["spawns"]["ball_home"]
    builder.add_sphere(
        "ball",
        radius=court["ball_radius"],
        center=(ball["x"], ball["y"], ball["z"]),
        color=(0.85, 0.35, 0.12),
    )
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(builder.to_gltf(scene_name=f"FINNBALL {arena['name']}"), encoding="utf-8")


class GltfBuilder:
    def __init__(self) -> None:
        self.positions: list[float] = []
        self.indices: list[int] = []
        self.meshes: list[dict] = []
        self.nodes: list[dict] = []
        self.materials: list[dict] = []

    def add_box(self, name: str, size, center, color) -> None:
        sx, sy, sz = (s * 0.5 for s in size)
        cx, cy, cz = center
        corners = [
            (cx - sx, cy - sy, cz - sz),
            (cx + sx, cy - sy, cz - sz),
            (cx + sx, cy + sy, cz - sz),
            (cx - sx, cy + sy, cz - sz),
            (cx - sx, cy - sy, cz + sz),
            (cx + sx, cy - sy, cz + sz),
            (cx + sx, cy + sy, cz + sz),
            (cx - sx, cy + sy, cz + sz),
        ]
        faces = (
            (0, 1, 2, 3),
            (5, 4, 7, 6),
            (4, 0, 3, 7),
            (1, 5, 6, 2),
            (3, 2, 6, 7),
            (4, 5, 1, 0),
        )
        self._add_quads(name, corners, faces, color)

    def add_sphere(self, name: str, radius: float, center, color, seg: int = 12) -> None:
        verts: list[tuple[float, float, float]] = []
        quads: list[tuple[int, int, int, int]] = []
        for iy in range(seg + 1):
            v = iy / seg
            phi = v * 3.14159265
            for ix in range(seg):
                u = ix / seg
                th = u * 6.2831853
                x = center[0] + radius * __import__("math").sin(phi) * __import__("math").cos(th)
                y = center[1] + radius * __import__("math").cos(phi)
                z = center[2] + radius * __import__("math").sin(phi) * __import__("math").sin(th)
                verts.append((x, y, z))
        for iy in range(seg):
            for ix in range(seg):
                a = iy * seg + ix
                b = iy * seg + (ix + 1) % seg
                c = (iy + 1) * seg + (ix + 1) % seg
                d = (iy + 1) * seg + ix
                quads.append((a, b, c, d))
        self._add_quads(name, verts, quads, color)

    def add_torus(self, name: str, radius: float, tube: float, center, color, seg: int = 16) -> None:
        import math

        verts: list[tuple[float, float, float]] = []
        quads: list[tuple[int, int, int, int]] = []
        ring = 10
        for i in range(seg):
            a = i / seg * math.tau
            for j in range(ring):
                b = j / ring * math.tau
                cx = math.cos(a) * radius
                cz = math.sin(a) * radius
                x = center[0] + cx + math.cos(a) * math.cos(b) * tube
                y = center[1] + math.sin(b) * tube
                z = center[2] + cz + math.sin(a) * math.cos(b) * tube
                verts.append((x, y, z))
        for i in range(seg):
            i2 = (i + 1) % seg
            for j in range(ring):
                j2 = (j + 1) % ring
                a = i * ring + j
                b = i2 * ring + j
                c = i2 * ring + j2
                d = i * ring + j2
                quads.append((a, b, c, d))
        self._add_quads(name, verts, quads, color)

    def _add_quads(self, name, verts, quads, color) -> None:
        start = len(self.positions) // 3
        for v in verts:
            self.positions.extend(v)
        index_start = len(self.indices)
        for a, b, c, d in quads:
            self.indices.extend((start + a, start + b, start + c, start + a, start + c, start + d))
        count = len(self.indices) - index_start
        mat = len(self.materials)
        self.materials.append(
            {
                "name": f"{name}_mat",
                "pbrMetallicRoughness": {
                    "baseColorFactor": [color[0], color[1], color[2], 1.0],
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.7,
                },
            }
        )
        self.meshes.append(
            {
                "name": name,
                "primitives": [
                    {
                        "attributes": {"POSITION": 0},
                        "indices": 1,
                        "material": mat,
                    }
                ],
            }
        )
        # One shared POSITION accessor (index 0) and INDICES accessor (index 1).
        # Per-mesh index ranges are applied after packing via extras on the primitive.
        self.meshes[-1]["_index_start"] = index_start
        self.meshes[-1]["_index_count"] = count
        self.nodes.append({"name": name, "mesh": len(self.meshes) - 1})

    def to_gltf(self, scene_name: str) -> str:
        pos_bytes = b"".join(struct.pack("<fff", *self.positions[i : i + 3]) for i in range(0, len(self.positions), 3))
        # align indices to 4
        pad = (4 - (len(pos_bytes) % 4)) % 4
        pos_bytes += b"\x00" * pad
        idx_bytes = b"".join(struct.pack("<I", i) for i in self.indices)
        blob = pos_bytes + idx_bytes
        import base64

        uri = "data:application/octet-stream;base64," + base64.b64encode(blob).decode("ascii")
        xs = self.positions[0::3]
        ys = self.positions[1::3]
        zs = self.positions[2::3]
        accessors = [
            {
                "bufferView": 0,
                "componentType": 5126,
                "count": len(self.positions) // 3,
                "type": "VEC3",
                "max": [max(xs), max(ys), max(zs)],
                "min": [min(xs), min(ys), min(zs)],
            }
        ]
        # One accessor per mesh so primitives don't share the whole index buffer.
        buffer_views = [
            {"buffer": 0, "byteOffset": 0, "byteLength": len(pos_bytes) - pad, "target": 34962},
        ]
        meshes = []
        for mesh in self.meshes:
            start = mesh["_index_start"]
            count = mesh["_index_count"]
            offset = len(pos_bytes) + start * 4
            view_i = len(buffer_views)
            buffer_views.append(
                {"buffer": 0, "byteOffset": offset, "byteLength": count * 4, "target": 34963}
            )
            acc_i = len(accessors)
            accessors.append(
                {
                    "bufferView": view_i,
                    "componentType": 5125,
                    "count": count,
                    "type": "SCALAR",
                }
            )
            prim = mesh["primitives"][0]
            meshes.append(
                {
                    "name": mesh["name"],
                    "primitives": [
                        {
                            "attributes": {"POSITION": 0},
                            "indices": acc_i,
                            "material": prim["material"],
                        }
                    ],
                }
            )
        doc = {
            "asset": {"version": "2.0", "generator": "finnball_dcc"},
            "scene": 0,
            "scenes": [{"name": scene_name, "nodes": list(range(len(self.nodes)))}],
            "nodes": self.nodes,
            "meshes": meshes,
            "materials": self.materials,
            "accessors": accessors,
            "bufferViews": buffer_views,
            "buffers": [{"uri": uri, "byteLength": len(blob)}],
        }
        return json.dumps(doc, indent=2) + "\n"


def self_test() -> None:
    assert to_blender(-12.55, 3.05, 0.0) == (-12.55, 0.0, 3.05)
    assert to_unreal(1.0, 2.0, 3.0) == (100.0, 300.0, 200.0)
    assert to_unity(1.0, 2.0, 3.0) == (1.0, 2.0, 3.0)
    data = load_manifest()
    assert data["court"]["half_length"] == 14.0
    assert data["roster"][0]["id"] == "kaito_flash"
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "court.gltf"
        write_court_gltf(data, "neo_tokyo", out)
        gltf = json.loads(out.read_text())
        assert gltf["asset"]["version"] == "2.0"
        assert len(gltf["nodes"]) >= 8
    print("finnball_dcc self-test ok")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true")
    sub = parser.add_subparsers(dest="cmd")

    sub.add_parser("validate")

    g = sub.add_parser("gltf")
    g.add_argument("--arena", default="neo_tokyo")
    g.add_argument("--out", type=Path, default=ROOT / "interop" / "court.gltf")
    g.add_argument("--manifest", type=Path, default=MANIFEST)

    c = sub.add_parser("convert")
    c.add_argument("--to", choices=("blender", "unreal", "unity"), required=True)
    c.add_argument("--x", type=float, required=True)
    c.add_argument("--y", type=float, required=True)
    c.add_argument("--z", type=float, required=True)

    args = parser.parse_args(argv)
    if args.self_test:
        self_test()
        return 0
    if args.cmd == "validate":
        load_manifest()
        print(f"ok {MANIFEST.relative_to(ROOT)}")
        return 0
    if args.cmd == "gltf":
        data = load_manifest(args.manifest)
        write_court_gltf(data, args.arena, args.out)
        print(f"wrote {args.out}")
        return 0
    if args.cmd == "convert":
        fn = {"blender": to_blender, "unreal": to_unreal, "unity": to_unity}[args.to]
        x, y, z = fn(args.x, args.y, args.z)
        print(f"{x} {y} {z}")
        return 0
    parser.print_help()
    return 2


if __name__ == "__main__":
    sys.exit(main())
