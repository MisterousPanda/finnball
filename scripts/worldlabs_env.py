#!/usr/bin/env python3
"""Generate a World Labs (Marble) environment for each FINNBALL arena.

For every arena theme this asks Marble for a 3D world from a text prompt, waits
for it, and downloads the 360° panorama into assets/env/<arena>.jpg (2048x1024,
what the game wraps around the open-roof stadium as a sky dome). The world ids,
Marble URLs and captions are recorded in assets/env/worlds.json so the same
worlds can be re-exported later (splats, collider mesh, high-quality GLB mesh).

    WLT_API_KEY=... python3 scripts/worldlabs_env.py            # all arenas
    WLT_API_KEY=... python3 scripts/worldlabs_env.py neo_tokyo  # one arena
    WLT_API_KEY=... python3 scripts/worldlabs_env.py --mesh     # also HQ GLB (Pro plan)
    WLT_API_KEY=... python3 scripts/worldlabs_env.py --reuse    # only re-download

API: https://docs.worldlabs.ai/api  (header WLT-Api-Key; generation costs credits).
"""
from __future__ import annotations

import io
import json
import os
import sys
import time
from pathlib import Path

import requests

API = "https://api.worldlabs.ai/marble/v1"
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "env"
PANO_SIZE = (2048, 1024)

# Prompts are written for Marble 1.1: one coherent place, a clear ground plane,
# and lots of sky, because the game shows the upper half of the panorama above
# the stands and through the open roof.
ARENAS: dict[str, dict] = {
    "neo_tokyo": {
        "name": "FINNBALL Neo-Tokyo Dome",
        "prompt": (
            "Standing on a wide rooftop plaza at night in Neo-Tokyo, surrounded on all sides by "
            "towering skyscrapers covered in glowing neon signs and animated holographic billboards, "
            "rain-slick surfaces reflecting cyan and magenta light, flying vehicles with light trails, "
            "a huge open night sky overhead, anime cyberpunk city, cinematic, highly detailed"
        ),
        "tags": ["finnball", "cyberpunk", "city", "night"],
    },
    "toon_world": {
        "name": "FINNBALL Toon World",
        "prompt": (
            "A bright cartoon world seen from a flat grassy clearing: candy-colored rolling hills, "
            "giant lollipop and gumdrop trees, fluffy white clouds with smiling faces, a big rainbow "
            "arching across a saturated blue sky, bouncy toon-shaded look, playful and wide open"
        ),
        "tags": ["finnball", "cartoon", "colorful"],
    },
    "sky_temple": {
        "name": "FINNBALL Sky Temple",
        "prompt": (
            "An ancient floating sky temple at golden hour: a wide stone courtyard with carved pillars, "
            "cherry blossom trees shedding petals, waterfalls pouring off the edge into a sea of clouds, "
            "distant floating islands and torii gates, warm sunlight, anime fantasy style, epic sky"
        ),
        "tags": ["finnball", "fantasy", "sky"],
    },
    "underground": {
        "name": "FINNBALL Underground",
        "prompt": (
            "A vast underground cavern turned into an illegal street basketball hideout: graffiti-covered "
            "concrete pillars, hanging cage lights and strings of bulbs, steam from pipes, subway tunnels "
            "leading away into darkness, moody amber and teal light, gritty anime style"
        ),
        "tags": ["finnball", "underground", "gritty"],
    },
    "crystal_coliseum": {
        "name": "FINNBALL Crystal Coliseum",
        "prompt": (
            "Inside a colossal crystal coliseum open to a starry night sky lit by a green and violet "
            "aurora, towering walls of glowing violet and cyan crystal, icy reflective floor, floating "
            "shards of light, magical anime style, vast and majestic"
        ),
        "tags": ["finnball", "crystal", "magic"],
    },
}


def die(msg: str) -> None:
    print(f"error: {msg}", file=sys.stderr)
    sys.exit(1)


def headers() -> dict:
    key = os.environ.get("WLT_API_KEY")
    if not key:
        die("WLT_API_KEY is not set (World Labs → API keys). Add it as a Cloud Agent secret.")
    return {"WLT-Api-Key": key, "Content-Type": "application/json"}


def load_manifest() -> dict:
    p = OUT / "worlds.json"
    return json.loads(p.read_text()) if p.exists() else {}


def save_manifest(m: dict) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "worlds.json").write_text(json.dumps(m, indent=2, sort_keys=True) + "\n")


def poll(operation_id: str, what: str, timeout_s: int = 45 * 60) -> dict:
    t0 = time.time()
    while True:
        r = requests.get(f"{API}/operations/{operation_id}", headers=headers(), timeout=60)
        if r.status_code >= 400:
            die(f"{what}: operation poll failed {r.status_code}: {r.text[:300]}")
        op = r.json()
        if op.get("done"):
            if op.get("error"):
                die(f"{what}: failed: {json.dumps(op['error'])[:400]}")
            return op
        prog = (op.get("metadata") or {}).get("progress") or {}
        print(f"  … {what}: {prog.get('status', 'RUNNING')} {prog.get('description', '')} "
              f"({int(time.time() - t0)}s)", flush=True)
        if time.time() - t0 > timeout_s:
            die(f"{what}: timed out after {timeout_s}s (operation {operation_id})")
        time.sleep(20)


def generate(arena: str, spec: dict, model: str) -> dict:
    body = {
        "display_name": spec["name"][:64],
        "model": model,
        "tags": spec["tags"],
        "permission": {"public": False},
        "world_prompt": {"type": "text", "text_prompt": spec["prompt"]},
    }
    r = requests.post(f"{API}/worlds:generate", headers=headers(), json=body, timeout=60)
    if r.status_code == 401:
        die("API key rejected (401). Check WLT_API_KEY.")
    if r.status_code in (402, 403):
        die(f"generation refused ({r.status_code}): {r.text[:300]} — add credits/payment on worldlabs.ai")
    if r.status_code >= 400:
        die(f"generate {arena}: {r.status_code}: {r.text[:400]}")
    op = r.json()
    print(f"  operation {op['operation_id']} started for {arena} ({model})", flush=True)
    op = poll(op["operation_id"], f"{arena} world")
    return op["response"]


def fetch_world(world_id: str) -> dict:
    r = requests.get(f"{API}/worlds/{world_id}", headers=headers(), timeout=60)
    if r.status_code >= 400:
        die(f"fetch world {world_id}: {r.status_code}: {r.text[:300]}")
    return r.json()


def download(url: str) -> bytes:
    r = requests.get(url, timeout=300)
    r.raise_for_status()
    return r.content


def save_pano(arena: str, pano_bytes: bytes) -> Path:
    from PIL import Image

    im = Image.open(io.BytesIO(pano_bytes)).convert("RGB")
    im = im.resize(PANO_SIZE, Image.LANCZOS)
    out = OUT / f"{arena}.jpg"
    im.save(out, "JPEG", quality=88, optimize=True, progressive=False)
    return out


def save_thumb(arena: str, thumb_bytes: bytes) -> Path:
    from PIL import Image

    im = Image.open(io.BytesIO(thumb_bytes)).convert("RGB")
    im.thumbnail((512, 512))
    out = OUT / f"{arena}_thumb.jpg"
    im.save(out, "JPEG", quality=85)
    return out


def export_mesh(arena: str, world_id: str) -> Path | None:
    body = {"asset_type": "mesh", "format": "glb"}
    r = requests.post(f"{API}/worlds/{world_id}:export", headers=headers(), json=body, timeout=60)
    if r.status_code >= 400:
        print(f"  mesh export for {arena} refused ({r.status_code}): {r.text[:200]}", flush=True)
        return None
    op = r.json()
    op = poll(op["operation_id"], f"{arena} HQ mesh", timeout_s=90 * 60) if not op.get("done") else op
    url = (op.get("response") or {}).get("url")
    if not url:
        world = fetch_world(world_id)
        url = ((world.get("assets") or {}).get("mesh") or {}).get("hq_mesh_url")
    if not url:
        print(f"  no mesh url for {arena}", flush=True)
        return None
    out = OUT / f"{arena}.glb"
    out.write_bytes(download(url))
    return out


def main(argv: list[str]) -> None:
    want_mesh = "--mesh" in argv
    reuse = "--reuse" in argv
    model = "marble-1.1"
    for a in argv:
        if a.startswith("--model="):
            model = a.split("=", 1)[1]
    picked = [a for a in argv if not a.startswith("--")]
    arenas = picked or list(ARENAS)
    for a in arenas:
        if a not in ARENAS:
            die(f"unknown arena {a!r}; choose from {', '.join(ARENAS)}")

    manifest = load_manifest()
    for arena in arenas:
        spec = ARENAS[arena]
        print(f"[{arena}] {spec['name']}", flush=True)
        entry = manifest.get(arena) or {}
        if reuse and entry.get("world_id"):
            world = fetch_world(entry["world_id"])
        elif entry.get("world_id") and not picked and (OUT / f"{arena}.jpg").exists():
            print("  already generated; pass the arena name explicitly to regenerate", flush=True)
            continue
        else:
            world = generate(arena, spec, model)

        assets = world.get("assets") or {}
        pano_url = (assets.get("imagery") or {}).get("pano_url")
        if not pano_url:
            die(f"{arena}: world has no panorama yet: {json.dumps(world)[:300]}")
        OUT.mkdir(parents=True, exist_ok=True)
        pano = save_pano(arena, download(pano_url))
        print(f"  panorama → {pano.relative_to(ROOT)} ({pano.stat().st_size // 1024} KB)", flush=True)
        if assets.get("thumbnail_url"):
            try:
                save_thumb(arena, download(assets["thumbnail_url"]))
            except Exception as e:  # thumbnails are optional
                print(f"  thumbnail skipped: {e}", flush=True)

        entry.update(
            {
                "world_id": world.get("id"),
                "marble_url": world.get("world_marble_url"),
                "caption": assets.get("caption"),
                "model": model,
                "prompt": spec["prompt"],
                "collider_mesh_url": (assets.get("mesh") or {}).get("collider_mesh_url"),
            }
        )
        manifest[arena] = entry
        save_manifest(manifest)

        if want_mesh and world.get("id"):
            mesh = export_mesh(arena, world["id"])
            if mesh:
                print(f"  mesh → {mesh.relative_to(ROOT)} ({mesh.stat().st_size // 1024} KB)", flush=True)
    print("done. Set ArenaTheme::env_pano for the arenas that now have assets/env/<arena>.jpg.")


if __name__ == "__main__":
    main(sys.argv[1:])
