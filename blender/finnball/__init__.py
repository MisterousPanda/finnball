# SPDX-License-Identifier: MIT
"""FINNBALL Blender add-on — build the 3v3 court and roster proxies from interop/finnball.json."""

from __future__ import annotations

import json
import math
from pathlib import Path

import bpy
from bpy.props import EnumProperty, StringProperty
from bpy_extras.io_utils import ImportHelper
from mathutils import Vector

bl_info = {
    "name": "FINNBALL",
    "author": "FINNBALL",
    "version": (0, 1, 0),
    "blender": (4, 2, 0),
    "location": "File > Import > FINNBALL Manifest",
    "description": "Import the Bevy court, arenas, and anime roster as editable meshes",
    "category": "Import-Export",
}


def _hex(color: str) -> tuple[float, float, float, float]:
    r = int(color[1:3], 16) / 255.0
    g = int(color[3:5], 16) / 255.0
    b = int(color[5:7], 16) / 255.0
    return r, g, b, 1.0


ARENA_ITEMS = (
    ("neo_tokyo", "Neo-Tokyo Dome", ""),
    ("toon_world", "Toon World Arena", ""),
    ("sky_temple", "Sky Temple Court", ""),
    ("underground", "Underground Circuit", ""),
    ("crystal_coliseum", "Crystal Coliseum", ""),
)


def _bevy_to_blender(x: float, y: float, z: float) -> Vector:
    return Vector((x, z, y))


def _mat(name: str, color, metallic=0.0, roughness=0.65, emission=None):
    mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    if bsdf:
        bsdf.inputs["Base Color"].default_value = color
        if "Metallic" in bsdf.inputs:
            bsdf.inputs["Metallic"].default_value = metallic
        if "Roughness" in bsdf.inputs:
            bsdf.inputs["Roughness"].default_value = roughness
        if emission and "Emission Color" in bsdf.inputs:
            bsdf.inputs["Emission Color"].default_value = emission
            if "Emission Strength" in bsdf.inputs:
                bsdf.inputs["Emission Strength"].default_value = 2.0
    return mat


def _mesh(name: str, verts, faces, collection, material=None):
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    obj = bpy.data.objects.new(name, mesh)
    collection.objects.link(obj)
    if material:
        obj.data.materials.append(material)
    return obj


def _box(name, size, center, collection, material):
    sx, sy, sz = (s * 0.5 for s in size)
    cx, cy, cz = center
    verts = [
        (cx - sx, cy - sy, cz - sz),
        (cx + sx, cy - sy, cz - sz),
        (cx + sx, cy + sy, cz - sz),
        (cx - sx, cy + sy, cz - sz),
        (cx - sx, cy - sy, cz + sz),
        (cx + sx, cy - sy, cz + sz),
        (cx + sx, cy + sy, cz + sz),
        (cx - sx, cy + sy, cz + sz),
    ]
    faces = [(0, 1, 2, 3), (4, 5, 6, 7), (0, 1, 5, 4), (2, 3, 7, 6), (1, 2, 6, 5), (0, 3, 7, 4)]
    return _mesh(name, verts, faces, collection, material)


def _disc(name, radius, z, segments, collection, material):
    verts = [(0.0, 0.0, z)]
    for i in range(segments):
        a = i / segments * math.tau
        verts.append((math.cos(a) * radius, math.sin(a) * radius, z))
    faces = [(0, i, i + 1 if i + 1 <= segments else 1) for i in range(1, segments + 1)]
    return _mesh(name, verts, faces, collection, material)


def build_scene(data: dict, arena_id: str) -> None:
    arena = next(a for a in data["arenas"] if a["id"] == arena_id)
    court = data["court"]
    pal = arena["palette"]

    coll = bpy.data.collections.get("FINNBALL") or bpy.data.collections.new("FINNBALL")
    if coll.name not in bpy.context.scene.collection.children:
        bpy.context.scene.collection.children.link(coll)
    for obj in list(coll.objects):
        bpy.data.objects.remove(obj, do_unlink=True)

    floor_mat = _mat("FB_Floor", _hex(pal["floor_a"]), roughness=0.35)
    paint_mat = _mat("FB_Paint", _hex(pal["paint"]))
    line_mat = _mat("FB_Line", _hex(pal["line"]), emission=_hex(pal["accent"]))
    iron_mat = _mat("FB_Iron", (1.0, 0.45, 0.08, 1.0), metallic=0.8, roughness=0.25)
    pole_mat = _mat("FB_Pole", (0.12, 0.12, 0.14, 1.0), metallic=0.4)
    ball_mat = _mat("FB_Ball", (0.85, 0.35, 0.12, 1.0), roughness=0.45)

    hl, hw = court["half_length"], court["half_width"]
    # Blender Z-up: court lies on XY, up is Z. Bevy (x,y,z) → (x,z,y).
    _box("CourtFloor", (hl * 2, hw * 2, 0.08), (0.0, 0.0, -0.04), coll, floor_mat)

    pd, pw = court["paint_depth"], court["paint_half_width"]
    for hoop in data["hoops"]:
        sign = 1.0 if hoop["position"]["x"] > 0 else -1.0
        hx, hy, hz = hoop["position"]["x"], hoop["position"]["y"], hoop["position"]["z"]
        paint_x = hx - sign * (pd * 0.5)
        _box(f"Paint_{hoop['id']}", (pd, pw * 2, 0.02), (paint_x, hz, 0.01), coll, paint_mat)
        board_x = hx + sign * court["backboard_offset"]
        _box(f"Backboard_{hoop['id']}", (0.06, 1.8, 1.05), (board_x, hz, hy + 0.15), coll, line_mat)
        pole_x = board_x + sign * 0.35
        _box(f"Pole_{hoop['id']}", (0.12, 0.12, hy), (pole_x, hz, hy * 0.5), coll, pole_mat)
        _disc(f"Rim_{hoop['id']}", court["rim_radius"], hy, 24, coll, iron_mat).location.x = hx

    three = _disc("ThreeHome", court["three_radius"], 0.015, 48, coll, line_mat)
    three.location.x = -court["hoop_x"]
    three_a = _disc("ThreeAway", court["three_radius"], 0.015, 48, coll, line_mat)
    three_a.location.x = court["hoop_x"]

    ball = data["spawns"]["ball_home"]
    _box(
        "Ball",
        (court["ball_radius"] * 2,) * 3,
        (ball["x"], ball["z"], ball["y"]),
        coll,
        ball_mat,
    )

    roster = {c["id"]: c for c in data["roster"]}
    for side_name, spots in (("home", data["spawns"]["home"]), ("away", data["spawns"]["away"])):
        team = next(t for t in data["teams"] if t["id"] == side_name)
        kit = _mat(f"FB_{side_name}", _hex(team["primary"]))
        for i, spot in enumerate(spots):
            loc = _bevy_to_blender(spot["x"], spot["y"], spot["z"])
            height = 1.88
            slug = f"{side_name}_{i}"
            body = _box(
                f"Player_{slug}",
                (court["player_radius"] * 2, court["player_radius"] * 2, height),
                (loc.x, loc.y, height * 0.5),
                coll,
                kit,
            )
            body["finnball_side"] = side_name
            body["finnball_slot"] = i
            body["finnball_team"] = team["name"]

    world = bpy.context.scene.world or bpy.data.worlds.new("FINNBALLWorld")
    bpy.context.scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    if bg:
        bg.inputs[0].default_value = _hex(pal["sky"])
        bg.inputs[1].default_value = 0.4
    bpy.context.scene["finnball_arena"] = arena["id"]
    bpy.context.scene["finnball_arena_name"] = arena["name"]


class FINNBALL_OT_import(bpy.types.Operator, ImportHelper):
    bl_idname = "import_scene.finnball"
    bl_label = "Import FINNBALL"
    bl_options = {"REGISTER", "UNDO"}
    filename_ext = ".json"
    filter_glob: StringProperty(default="*.json", options={"HIDDEN"})
    filepath: StringProperty(subtype="FILE_PATH")
    arena: EnumProperty(
        name="Arena",
        items=ARENA_ITEMS,
        default="neo_tokyo",
    )

    def execute(self, context):
        path = Path(self.filepath)
        if not path.is_file():
            self.report({"ERROR"}, f"Missing {path}")
            return {"CANCELLED"}
        data = json.loads(path.read_text())
        build_scene(data, self.arena)
        self.report({"INFO"}, f"FINNBALL {self.arena} from {path.name}")
        return {"FINISHED"}


class FINNBALL_OT_import_repo(bpy.types.Operator):
    bl_idname = "finnball.import_repo"
    bl_label = "FINNBALL: Load repo manifest"
    bl_options = {"REGISTER", "UNDO"}
    arena: EnumProperty(
        name="Arena",
        items=ARENA_ITEMS,
        default="neo_tokyo",
    )

    def execute(self, context):
        here = Path(__file__).resolve()
        candidate = here.parents[2] / "interop" / "finnball.json"
        if not candidate.is_file():
            self.report({"ERROR"}, f"Could not find {candidate}")
            return {"CANCELLED"}
        data = json.loads(candidate.read_text())
        build_scene(data, self.arena)
        return {"FINISHED"}


def menu_func(self, _context):
    self.layout.operator(FINNBALL_OT_import.bl_idname, text="FINNBALL Manifest (.json)")


classes = (FINNBALL_OT_import, FINNBALL_OT_import_repo)


def register():
    for cls in classes:
        bpy.utils.register_class(cls)
    bpy.types.TOPBAR_MT_file_import.append(menu_func)


def unregister():
    bpy.types.TOPBAR_MT_file_import.remove(menu_func)
    for cls in reversed(classes):
        bpy.utils.unregister_class(cls)


if __name__ == "__main__":
    register()
