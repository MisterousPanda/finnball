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
| E | Pass (hold W for lob, S for bounce, Shift for skip) |
| Q | Steal |
| F | Dunk attempt (paint) |
| R | Block / contest |
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

## Hosting (Railway)

Vercel is a poor fit for this project: it will not run a native Bevy window, and it will not compile a Bevy WASM client in a typical serverless build. **Railway** is the intended host.

Railway still cannot pop a GPU window in the cloud. What it *can* do is run a tiny nginx container that serves the prebuilt **WebGL / WASM** client from `www/`.

```bash
./scripts/build-web.sh          # produces www/pkg/*.wasm (gitignored, ~28MB)
railway login && railway link   # once
railway up                      # uploads the local www/ tree, including pkg/
```

The Dockerfile + `deploy/` scripts bind nginx to Railway’s `$PORT` and set `Content-Type: application/wasm`. `www/pkg` is gitignored on purpose — do not let Railpack compile the Bevy crate; there is no GPU, and the compile is too large for a normal service build.

Open **https://finnball-production.up.railway.app** in a WebGL2 browser.

### Other options

- Native desktop: `cargo run --release` (needs a GPU)
- itch.io / GitHub Releases: ship the WASM zip or native binaries
- Cloudflare Pages: same static `www/` folder if you do not want a container

Vercel can still serve `www/` as static files if you prebuild WASM, but Railway is the path this repo is wired for.


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
