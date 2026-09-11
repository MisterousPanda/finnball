# FINNBALL engine roadmap — Unity, Unreal, Blender → Railway

Working plan for taking FINNBALL beyond the Bevy build using Unity, Unreal, and Blender, testing everything locally, and shipping to Railway. Check boxes as you go.

## 0. Where things stand

| Piece | State |
| --- | --- |
| Bevy game (Rust, WASM) | Shipped at `https://finnball-production.up.railway.app` from `www/` |
| `interop/finnball.json` | Generated from `src/sim.rs` / `roster.rs` / `arenas.rs`; CI fails on drift |
| Unity project `unity/Finnball` | Consumes package `unity/com.finnball.interop`; court scene generated headlessly; 15 EditMode tests mirror `sim.rs`; WebGL build (4.8 MB) staged at `www/unity/` and served by the Railway nginx image at `/unity/` |
| Unreal plugin `unreal/Plugins/Finnball` | Types + coordinate helpers + Python court spawner; no gameplay yet |
| Blender add-on `blender/finnball` | Imports the court/roster proxies from the JSON |

One-liners that already work:

```bash
unity test unity/Finnball --mode EditMode        # C# sim port vs Rust tests
./scripts/build-unity-web.sh                     # scene → WebGL → www/unity/
docker build -t finnball-web . && docker run --rm -p 8089:8080 -e PORT=8080 finnball-web
# open http://localhost:8089/  (Bevy)   http://localhost:8089/unity/  (Unity)
```

## 1. Decision: what each tool is for

Railway has no GPUs and only serves static files / containers, so the browser is the constraint.

| Tool | Role | Ships to Railway? |
| --- | --- | --- |
| **Bevy** | The live game today; stays the reference sim | Yes (`/`) |
| **Unity 6** | The engine port for the browser: WebGL, mobile browsers, Cinemachine, real skinned rigs, easier UI/VFX iteration | Yes (`/unity/`) — this is the path to a richer web game |
| **Blender** | Asset source for *both* Bevy and Unity: player meshes, rigs, animation clips, hoops, arena props → glTF/FBX | Indirectly (assets inside the builds) |
| **Unreal 5** | Cinematics/trailers, a native desktop build, MetaHuman/Lumen showcase. UE5 has no HTML5 target; browser play needs Pixel Streaming on a GPU server (not Railway) | No browser build. Railway can host the landing page + download links only |

Recommended order: **Blender assets → Unity port → (optional) Unreal showcase**. Unity is where the game gets better in the browser; Unreal is a marketing/desktop track.

## 2. Accounts & links (you do these)

Everything below is free unless marked. Fill in the "done" column as you go.

| # | Account / tool | Why | How to link it here | Cost | Done |
| --- | --- | --- | --- | --- | --- |
| 1 | **Unity ID** (already have) | Editor license, Asset Store | `unity auth login` then `unity license` should list Personal. Already working on this Mac. | Personal: free under $200K revenue/funding in 12 mo. Pro $2,310/yr only if you cross that | ☐ |
| 2 | **Unity Hub modules** | WebGL + Mac already installed | For iOS/Android later: `unity install-modules --version 6000.6.0f1 ios android` | free | ☐ |
| 3 | **Railway** (already have) | Hosting | `railway login && railway link` in repo root — done, linked to `finnball`/`production`. For CI: create a project token → GitHub secret `RAILWAY_TOKEN` | Hobby $5/mo (incl. $5 usage). Static nginx fits Hobby. Pro $20/mo only for team features | ☑ |
| 4 | **GitHub Actions secrets for Unity CI** (optional, for automated WebGL builds) | Build WebGL on push | Use `game-ci/unity-builder`. Secrets: `UNITY_EMAIL`, `UNITY_PASSWORD`, `UNITY_LICENSE` (contents of a `.ulf` from `unity license` / manual activation). Public repos get free minutes | free | ☐ |
| 5 | **Epic Games account** + Epic Games Launcher | Unreal Engine, Fab/Quixel assets | Install UE 5.8 (or newest 5.x). Open `unreal/Finnball.uproject` once so it generates project files | free; 5% royalty only after $1M lifetime revenue per product | ☐ |
| 6 | **Blender 4.2+ LTS** | Assets | Install from blender.org. Preferences → Add-ons → Install from Disk → `blender/finnball`. No account needed | free | ☐ |
| 7 | **Adobe ID → Mixamo** | Free humanoid animations (run, jump shot, dunk, celebrate) to retarget in Blender/Unity | mixamo.com → download FBX "Without Skin" for Unity Humanoid | free | ☐ |
| 8 | **VRoid Studio** (pixiv) | Fastest way to make anime-style rigged characters (VRM) that match the roster | Export VRM → Blender (VRM add-on) or Unity (UniVRM) | free | ☐ |
| 9 | **World Labs** (already have key) | Arena panoramas / meshes | `WLT_API_KEY` in env; `--mesh` needs the Pro plan | paid credits; Pro for GLB export | ☐ |
| 10 | Optional: **Blender Market / Fab / Unity Asset Store** | Crowd packs, court props, toon shaders | Buy per item; drop into `unity/Finnball/Assets/ThirdParty/` (gitignored if license forbids redistribution) | $10–60 each | ☐ |
| 11 | Optional: **Meshy / Tripo** | AI meshes for props | Export glTF into Blender for cleanup first | ~$20/mo | ☐ |

Never commit `.ulf`, tokens, or `WLT_API_KEY`. Use `unity auth`, Railway variables, and GitHub secrets.

## 3. Phase A — Blender asset pipeline (feeds Bevy *and* Unity)

Goal: replace procedural capsules/boxes with real anime rigs and hoops without touching gameplay.

- [ ] **Skeleton standard**: Unity Humanoid / Mixamo-compatible bones, 1.88 m reference height (roster scales by `height_m`). Root at feet, faces −Z (matches `units.rs` `FRONT`).
- [ ] **Ten legends**: VRoid or hand-modelled; hair per `HairStyle` slug; colors from `interop/finnball.json` `look`/`kit`. Save `blender/characters/<slug>.blend`.
- [ ] **Animation clips** — one per `Pose` in `units.rs`: `idle, run, sprint, shoot, dunk, pass, block, contest, celebrate, stumble`. Start from Mixamo, fix hands/ball contact in Blender. Export as glTF animations named exactly like the pose.
- [ ] **Court set**: hoop (rim radius 0.225, height 3.05, backboard offset 0.42), stanchion, scorer's table, bench — dimensions from `court` in the JSON.
- [ ] **Export script** `scripts/blender_export.py` (run `blender -b file.blend -P scripts/blender_export.py`) → `assets/models/*.glb` (Bevy) and `unity/Finnball/Assets/Models/*.glb` (Unity via glTFast). Add a size budget check (mobile Safari: keep the total under ~15 MB).
- [ ] **Bevy side**: enable `bevy_gltf`, `bevy_animation`, `bevy_scene` features in `Cargo.toml`; load rigs in `units.rs` behind a `--features gltf_rigs` flag first so the WASM size delta is measured before it ships.
- [ ] **Unity side**: install `com.unity.cloud.gltfast`; `FinnballCourtBuilder` spawns the glTF instead of primitives when the asset exists.

Deliverable: same characters in Bevy and Unity, one source of truth in Blender.

## 4. Phase B — Unity port (the browser engine track)

Goal: a playable 3v3 in Unity WebGL at `/unity/` that plays like the Bevy game.

- [x] Project + package + headless scene + EditMode tests (`FinnballSim` = `sim.rs`)
- [x] WebGL build via `unity build`, staged by `scripts/build-unity-web.sh`, served by nginx at `/unity/`
- [x] First runtime slice: ballistic shot → cylinder score using the shared math
- [ ] **Render pipeline**: URP + a toon/cel shader (URP Toon or a Shader Graph); court paint from `arenas` palette; sky color from `palette.sky`.
- [ ] **Player controller**: port `input.rs` (WASD/sticks, sprint, shot meter hold/release at 0.72 green, pass E/T/G, steal Q, dunk F, block R, switch Tab). Use the Input System package with touch on-screen controls (mirror `touch.rs`).
- [ ] **Ball & possession**: port `ball.rs` (hold, dribble cadence, bounce with `arena.bounce`, rim/backboard, loose ball) using `FinnballSim.ApplyAero` for misses only.
- [ ] **Gameplay**: port `gameplay.rs` — clock (4×60 s, 24 s shot clock), scoring, shot meter, steal/block resolution, tip-off/inbound using `TipoffHome/Away`.
- [ ] **AI**: port `ai.rs` behaviours and the ROOKIE/PRO/LEGEND `AiProfile` constants verbatim (they're gym-tuned; don't retune in Unity).
- [ ] **Cameras**: Cinemachine broadcast/chase/tactical cams matching `camera.rs`.
- [ ] **HUD/menus**: UI Toolkit — splash, draft (roster from JSON), court select (arenas from JSON), HUD, final.
- [ ] **Audio**: reuse `assets/audio/*.wav` (synthesized, no licensing issues) via a mixer with the four buses from `audio.rs`.
- [ ] **Mobile**: test iOS Safari/Android Chrome (Unity 6 supports mobile browsers); ASTC textures, Brotli, cap DPR like `quality.rs`.
- [ ] **CI**: GitHub Actions job with `game-ci/unity-builder` producing the WebGL artifact; `unity test` in the same job.

Order of work inside B: controller + ball first (feel), then gameplay/clock, then AI, then presentation.

## 5. Phase C — Unreal (showcase / native)

- [ ] Open `unreal/Finnball.uproject` in UE 5.8+; build the `Finnball` plugin.
- [ ] Import Blender glTF/FBX rigs; retarget Mixamo clips with IK Retargeter.
- [ ] Rebuild one arena (Neo-Tokyo) with Lumen + Niagara crowd/confetti; use the World Labs panorama as the sky.
- [ ] Sequencer trailer (30–60 s) for the landing page and stores.
- [ ] Package a macOS/Windows desktop build → GitHub Releases / itch.io. Link from `www/index.html`.
- [ ] Only if you want browser play in Unreal: Pixel Streaming on a GPU host (AWS g4dn / Vagon / SimplyStream). Not Railway.

## 6. Local test matrix

| What | Command |
| --- | --- |
| Rust sim + interop | `cargo test` (fails if `interop/finnball.json` is stale → `UPDATE_INTEROP=1 cargo test interop::`) |
| DCC tools | `python3 scripts/finnball_dcc.py --self-test` |
| Unity tests | `unity test unity/Finnball --mode EditMode --output /tmp/unity-tests.xml` |
| Unity scene | `unity run unity/Finnball -- -executeMethod Finnball.Editor.FinnballBatch.BuildCourtScene -finnballArena sky_temple` |
| Unity in editor | `unity open unity/Finnball` (Play `Assets/Scenes/Court.unity`) |
| Unity WebGL | `./scripts/build-unity-web.sh` then `python3 -m http.server 8080 --directory www` → `/unity/` |
| Bevy WebGL | `./scripts/build-web.sh` then the same server → `/` |
| Exact Railway image | `docker build -t finnball-web . && docker run --rm -p 8089:8080 -e PORT=8080 finnball-web` |
| Blender add-on | Blender → File → Import → FINNBALL Manifest → `interop/finnball.json` |
| Unreal spawner | `python3 unreal/Scripts/import_finnball.py` (dry run) / same file in the UE Python console |

## 7. Railway deploy

One nginx service serves both clients from `www/`:

```
/          Bevy WASM   (www/game, www/assets)
/unity/    Unity WebGL (www/unity)
```

- [x] `railway link -p finnball -e production` (service auto-selected: `finnball`, Dockerfile builder, region ams)
- [x] `./scripts/build-web.sh` (Bevy) and `./scripts/build-unity-web.sh` (Unity) so `www/game` and `www/unity` both exist
- [x] `./scripts/deploy-railway.sh` — uploads `www/` + `Dockerfile` + `deploy/`; `.railwayignore` drops `/unity`, `/unreal`, `/blender`, `/src`, `target/` (patterns are anchored — a bare `unity` would also drop `www/unity`)
- [x] Verify: `curl -I https://finnball-production.up.railway.app/unity/Build/WebGL.loader.js` → 200 `application/javascript` (2026-09-11: Bevy at `/` and Unity at `/unity/` both live)
- Gotchas hit on first deploy: `wasm-bindgen-cli` must match the `wasm-bindgen` version in `Cargo.lock` (0.2.127 → `cargo install wasm-bindgen-cli --version 0.2.127`); install `binaryen` (`brew install binaryen`) or `build-web.sh` ships a ~10% larger unoptimised wasm.
- [ ] Optional later: a second Railway service (Rust/Node) for leaderboards / WebSocket relay + Railway Postgres; both clients read the same `interop/finnball.json` so the API can validate box scores against the sim.
- [ ] Optional: turn off `decompressionFallback` in `FinnballBatch.BuildWebGL` once nginx `.br` headers are confirmed on Railway (faster loads; the config already sets `Content-Encoding: br`).

## 8. Budget summary

| Item | Monthly |
| --- | --- |
| Railway Hobby (static nginx, low egress) | $5 |
| Unity Personal, Unreal, Blender, Mixamo, VRoid | $0 |
| World Labs credits (only when regenerating arenas) | variable |
| Optional asset packs / AI mesh tools | $0–60 one-off / ~$20 |

Verify prices at checkout; they're current as of Sep 2026.
