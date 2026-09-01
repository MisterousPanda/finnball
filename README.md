# FINNBALL

Anime street basketball with Space-Jam energy and a modern esports broadcast frame. Built in **Rust** on **Bevy 0.18**.

You draft a three of original anime legends, pick a court world, and play a full 3v3: dribble, dish, steal, contest, logo threes, and poster dunks. The presentation is a night-game overlay — LIVE pill, shot clock, ticker, stamina, box-score line — not a toy prototype.

## Play (native)

```bash
cargo run --release
```

Needs a GPU (or Mesa llvmpipe) plus the usual Bevy Linux deps: `libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev pkg-config`.

## Play (browser)

```bash
chmod +x scripts/build-web.sh
./scripts/build-web.sh
# then serve the www/ folder, e.g.
python3 -m http.server 8080 --directory www
```

Open `http://localhost:8080`. WebGL2 required.

## Controls

| Input | Action |
| --- | --- |
| WASD / arrows | Move |
| Shift | Sprint |
| Space (hold / release) | Shot meter |
| E | Pass |
| Q | Steal |
| F | Dunk attempt (paint) |
| Tab / C | Switch home player |
| 1 / 2 / 3 / 4 or V | Broadcast / chase / tactical / cinema cam |
| Esc / P | Pause |
| M (while paused) | Main menu |

## Modes

- **Quick match** — 3v3, default Neon Foxes vs Shadow Cranes, Neo-Tokyo Dome
- **Exhibition** — pick three legends, then one of five courts
- **Practice** — gym, no clock pressure, no away AI

Four arcade quarters (60s) + 24s shot clock. Tie goes to a short overtime.

## Roster

Neon-speed point guards, logo snipers, rim breakers, shot-blocking eclipses, no-look queens, pickpockets, and more — ten original anime archetypes with distinct hair meshes, ratings, and quotes.

## Courts

1. **Neo-Tokyo Dome** — rain-slick cyber hardwood  
2. **Toon World Arena** — Looney bounce and hangtime  
3. **Sky Temple Court** — moonlit petals above the clouds  
4. **Underground Circuit** — chain-net street wattage  
5. **Crystal Coliseum** — glass esports cathedral  

## Hosting on Vercel

Vercel **cannot run a native Rust game server**. A Bevy sim is a long-lived GPU client, not a serverless function.

What *does* work: compile FINNBALL to **WebAssembly**, ship `www/` as a static site. That is what `www/vercel.json` is for.

1. `./scripts/build-web.sh` (produces `www/pkg/`)
2. Point a Vercel project at `www/` (`outputDirectory: "www"`, `framework: null`)
3. Keep `Content-Type: application/wasm` for `*.wasm`

If the wasm blob is too large for a Hobby upload, use **GitHub Releases**, **itch.io**, **Cloudflare Pages**, or **Vercel Pro** with the prebuilt `www/pkg` artifacts from CI.

### Why not compile Rust on Vercel?

Bevy release wasm takes many minutes and a full `rustc` + `wasm-bindgen` toolchain. Vercel’s default builders do not include that. Prebuild locally or in GitHub Actions, then deploy the static output.

## Architecture

```
src/
  sim.rs        basketball math (tested without a GPU)
  roster.rs     ten legends + ratings
  arenas.rs     five court themes
  court.rs      3D floor, rims, stadium, lights
  units.rs      procedural anime bodies + animation
  ball.rs       gravity, bounce, iron, backboard, buckets
  gameplay.rs   clock, meter, shoot/pass/steal, scoring
  ai.rs         3v3 offense/defense
  camera.rs     broadcast cameras
  ui/           splash, menu, draft, HUD, final
```

V1 is intentionally arcade-complete rather than NBA-sim complete: no full 5v5 motion offense, no recorded skeletal mocap, no licensed audio. Those are the obvious V2 pillars.

## License

MIT
