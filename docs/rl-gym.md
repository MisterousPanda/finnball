# FINNBALL RL gym — research plan and feasibility spike

Goal: improve the opposition AI (defense) and player skills (shot selection,
passing, stealing, blocking, positioning) with a reinforcement-learning-style
loop, without bloating the WASM build or costing CPU on phones.

Deliverables in this pass:

- this document;
- `src/gym/` — a headless, deterministic, fixed-timestep stepper over the real
  game sim (`Gym::new / reset / step / observe`) with 8 tests, wired in with one
  `mod gym;` line in `src/main.rs`. No other file was touched.

Headline numbers (measured, see §2.6):

| metric | value |
|---|---|
| headless throughput, 1 core, dev/test profile | **~53 000 sim ticks/s ≈ 830× realtime** |
| headless throughput, 1 core, `--release` | **~78 000 sim ticks/s ≈ 1 200× realtime** |
| full 4×60 s match (15 360 ticks) | **0.29 s** dev / **0.20 s** release (≈18 000 matches / core-hour) |
| determinism | bit-identical observations + rewards across two runs with the same seed and actions |
| gym overhead vs. game | none in the shipped binary; the module is dead code the linker drops |

And three sim findings the spike surfaced that must be fixed **before** any
learning is meaningful (§3): the game RNG never returned > 0.004, AI shots were
launched from the dribble position instead of the hand, and consequently an
AI-vs-AI match ended 0-0 after 168 shots. The first two are fixed in this
branch (`GameRng::f32` scales by 2^24; `ai_decisions` moves the ball to the
release point); seed 1 now plays out 28 shots, 26-22.

---

## 1. How the AI decides today

All gameplay runs in Bevy `FixedUpdate` (64 Hz) except a set of `Update`
systems noted in §5.1. Files: `src/ai.rs` (opponent + AI teammates),
`src/gameplay.rs` (rules, human intent, contests), `src/sim.rs` (pure math),
`src/units.rs` (ratings, stamina, separation), `src/ball.rs` (ball physics,
rim), `src/roster.rs` (characters).

### 1.1 Movement — `ai_move` (`ai.rs`)

Runs every tick for every non-controlled player that is not in a busy pose
(`Shoot | Dunk | Pass | Stumble | Celebrate | Block`). Purely positional, no
memory beyond `AiBrain.think` (a timer).

| situation | target | constants |
|---|---|---|
| has the ball | drive to `(hoop_x·0.72, lane_z)`; `lane_z = ±2.4` by slot parity when near the centre line, else `z·0.4` | `0.72`, `2.4`, `0.4`, `0.4` (centre band) |
| on offense, no ball | spot up at `(hoop_x·0.45, (slot−1)·3.4)` | `0.45`, `3.4` |
| on defense | `lerp(ball_xz, own_hoop, 0.35)` — 35 % of the way from the ball to the hoop | `0.35` |
| loose ball | only the closest AI per team (“hunter”) chases the ball; others hold spacing | loose = `Hold::Loose`, or a dead shot/pass with `y < 1.6` and speed `< 7` |
| speed | `move_speed(ratings, dist > 6 ⇒ sprint, stamina) · 0.92`; stops inside `0.35` m | `0.92`, `6.0`, `0.35` |

Notable: **defenders never guard a man, never step to the ball handler, never
attempt a steal or a block** (`steal_attempts` / `block_attempts` in
`gameplay.rs` only fire on `PlayerIntent`, i.e. the human). Opposition defense
is therefore “stand between ball and rim”; contest only happens implicitly
through `contest_factor(def_dist, block)` when the *human* shoots.

### 1.2 Ball decisions — `ai_decisions` (`ai.rs`)

Only for the AI ball-holder, once `brain.think ≥ 0.45 s` (reaction interval).

1. `open` = no opponent within `1.8` m.
2. `rating` = `three` if beyond `THREE_RADIUS` (6.75 m) else `mid`.
3. `should_dunk` = in paint ∧ `dunk > 72` ∧ speed `> 2`.
4. `should_shoot` = `ai_wants_shot(dist, open, rating, shot_clock, dist < 3.2)`:
   - `dist ≥ 9.5` ⇒ only if shot clock `< 8`;
   - else `open ∧ rating > 62`, or shot clock `< 5`, or close range.
5. Shoot: aim at the rim, “make” roll `0.55 + three/400` (uses `three` even
   for mid-range) × `heat_make_mult`; on a miss add up to ±0.5 m error.
   Ballistic solve from `(x, 1.85, z)` with `flight_time_for_distance`.
6. Otherwise pass to the teammate **closest to the hoop** (not the most open),
   flight `0.35` s, gravity `×0.4`.

There is no dribble move, no shot-clock awareness beyond the two thresholds,
no pass-lane check, no decision to *not* drive.

### 1.3 Human-side mechanics the AI would inherit (`gameplay.rs`, `sim.rs`)

These are the levers a learned/tuned policy would actually pull:

| mechanic | formula / constants |
|---|---|
| shot make chance (human) | `shot_make_chance(rating, dist, contest, meter_err, stamina, is_three)` = `0.18 + 0.74·skill·range·open·meter·gas`, clamped `0.04..0.92`; `open = 1 − 0.62·contest`; `meter = 1 − 1.35·|meter − 0.72|` |
| contest | `contest_factor(d, block)` = `(1 − d/2.4)·(0.45 + block/180)` for `d < 2.4` |
| steal | `steal_chance(steal, handle, d)` = `(0.12 + 0.45·mismatch)·reach`, `reach = 1.15 − 1.8·d` ⇒ only inside `0.64` m |
| block | window ball `1.6 < y < 3.2`, `d < 2.2`, chance `(block/100)·(1.15 − 0.45·d)` |
| pickup | reach `1.05 + 0.12·height + rebound/220`; best `rebound + (2 − d)·20`; **any pickup resets the shot clock** |
| shot classes | `classify_shot` → dunk / layup / floater / fadeaway / three / logo heave; release heights `1.05..2.4`, flight `0.42..1.6` s |
| heat | 3 straight makes ⇒ `×1.18` make chance |
| stamina | drain `0.22/s` sprinting, `0.08/s` shooting, regen `0.16/s`; speed `× (0.65 + 0.35·stamina)` |
| poses | timeouts `Shoot 0.55, Dunk 0.9, Pass 0.35, Block 0.45, Celebrate 1.4, Stumble 0.5` s |
| clock | quarter `60` s ×4, shot clock `24` s, tie ⇒ 30 s OT “first bucket wins” |

### 1.4 Per-character skill (`roster.rs` → `units::Ratings`)

Ten characters, each with `speed, three, mid, dunk, handle, pass, steal, block,
rebound, strength` (0–99) and `height_m`. `Ratings` is a component on every
player entity, so a policy can read them directly. Today the AI uses only
`three/mid` (shot gate), `dunk` (dunk gate), `speed` (movement); `pass`,
`steal`, `block`, `handle`, `strength` do nothing for AI players.

---

## 2. Gym design (implemented in `src/gym/mod.rs`)

### 2.1 Headless app

```
MinimalPlugins + AssetPlugin + StatesPlugin
+ init_asset::<Mesh / StandardMaterial / Image>   (spawn_player builds the rig into these)
+ UnitsPlugin + BallPlugin + GameplayPlugin + AiPlugin
+ add_message::<camera::CamTrigger>               (written by gameplay, owned by CameraPlugin)
+ TimeUpdateStrategy::FixedTimesteps(1)           (one app.update() == exactly one 64 Hz tick)
+ every schedule → ExecutorKind::SingleThreaded   (determinism, see §5.2)
```

No window, renderer, audio, UI, input, camera, crowd, court or FX plugin. This
is the same shape as the existing headless tests (`audio.rs` builds
`MinimalPlugins + AssetPlugin + StatesPlugin`; `units.rs` spawns players into a
bare `World`). Wiring required **no change to any existing file**: the only
cross-plugin dependency was the `CamTrigger` message.

The `Update`-schedule systems (`tick_clock`, `follow_dribble`, `handle_buckets`,
`inbound_after_score`, `pose_timeouts`, `stamina_regen`, `separate_players`,
`animate_rigs`, …) run 1:1 with `FixedUpdate` here. In the shipped game they run
at display rate — see §5.1.

### 2.2 Stepping API

```rust
let mut gym = Gym::new(MatchConfig::default(), seed);   // starts a match
gym.select(Side::Away, 1);                              // optional: control a defender
let obs: Vec<f32> = gym.observe();                      // OBS_LEN = 66
let StepResult { obs, reward, done } = gym.step(Action { move_xz, sprint, button });
gym.reset(new_seed);                                    // GameOver → Playing, respawn, reseed
```

`step()` writes the action into `PlayerIntent` — the very resource keyboard,
gamepad and touch input write — so the policy is bound by exactly the human's
verbs and the same `apply_intents / shoot_and_pass / steal_attempts /
block_attempts` code paths. Everyone else runs the rule-based AI. This is the
cheapest possible “agent in the loop” and it is already sufficient for options
(a) and the first half of (b) in §4.

### 2.3 Observation (66 floats, unit-ish scale)

| idx | contents |
|---|---|
| 0–2 | ball position `(x/14, y/4, z/7.5)` |
| 3–5 | ball velocity `/15` |
| 6–9 | hold one-hot: loose, held, shot, pass |
| 10–15 | holder one-hot over the six player slots (home 0-2, away 0-2) |
| 16–57 | per player ×6: `x/14, z/7.5, vx/10, vz/10, stamina, busy-pose, is-controlled` |
| 58 | possession `+1` home / `−1` away / `0` |
| 59–61 | shot clock `/24`, quarter time `/quarter_secs`, quarter `/4` |
| 62 | score differential `/20` |
| 63 | controlled side `±1` |
| 64–65 | controlled player → ball distance `/14`, → attacking hoop `/28` |

To add when moving past the spike: the controlled player's `Ratings` (so one
network can serve all ten characters), a frame of history or the previous
action (the sim has hidden state: shot meter value, `AiBrain.think`, pose
timers), and egocentric copies (mirror the court so home/away share weights).

### 2.4 Action space (hybrid → flattened 126-way discrete)

`Action::multi(move 0..9, sprint, button 0..7)`: 8 compass directions + still,
sprint flag, and one button per tick — `None, ShootHold, ShootRelease, Pass,
Steal, Block, Special`. `Action::from_discrete(i)` flattens it for a single
softmax; a multi-head policy can emit the three parts separately. `ShootHold`
must be held ~12 ticks then released to hit the meter sweet spot (`0.72`), so
timing is learnable rather than scripted. Pass target is “nearest teammate”
because that is what the game exposes; a `pass_to: slot` head needs a small
change in `shoot_and_pass` (§2.7).

### 2.5 Reward and episodes

`RewardWeights` (public, tweakable): `+1/pt scored, −1/pt allowed, +0.5 steal,
+0.5 block, +0.3 rebound, +0.3 assist, −0.5 live-ball turnover` (possession
flips without a shot having left the hand and no score). Points come from
`Scoreboard`; the rest from the controlled player's `BoxLine`.

Suggested dense shaping to add for defense (all computable from the obs):
`−k·max(0, 2.4 − dist_to_ball_handler)` inverted (reward proximity to the
handler while on defense), `−k·open_shot_quality` when the opponent releases
(reuse `contest_factor`), `+k` when the shot clock expires on the opponent.
For offense: `+k·shot_make_chance(...)` at release (quality of shot taken, not
whether it dropped, to cut variance).

Episodes: the natural unit is a **possession** (ends on score, turnover, shot
clock, or rebound change of hands; ~5–24 s ≈ 300–1500 ticks) for dense
credit assignment; a **quarter** (3 840 ticks) for scoreboard-level fitness;
a **match** (15 360 ticks + OT) for ES fitness. `done` today is `GameOver`;
truncate at a step cap (a tie loops overtimes forever — the spike caps at
40 000). Frame-skip of 4 ticks (16 Hz decisions) is a good default — human
inputs are not faster, and it quarters the policy compute.

### 2.6 Throughput (measured)

`cargo test --offline gym -- --nocapture` (dev/test profile: crate `opt-level 1`,
dependencies `opt-level 3`), one core, Bevy `multi_threaded` on but executors
single-threaded:

```
gym: 2000 steps in 0.037s = 53670 steps/s (838.6x realtime)
gym: full match 15360 steps in 0.29s = 52186 steps/s, final 6-0 (q4)
gym: AI-vs-AI regulation 168 shots, 0-0     (before the §3.1/§3.2 fixes)
gym: AI-vs-AI regulation 28 shots, 26-22    (after)
```

Same tests with `cargo test --release --offline gym -- --nocapture`:

```
gym: 2000 steps in 0.026s = 77878 steps/s (1216.8x realtime)
gym: full match 15360 steps in 0.20s = 77892 steps/s, final 6-0 (q4)
```

Identical scores and shot counts in both profiles — optimisation level does
not change the trajectory. That includes the full six-player rig animation (`animate_rigs`, hair sway,
face expressions) which is only running because pose timers live there
(§5.1); a sim-only `UnitsPlugin` split should roughly double this. One `Gym`
per thread should scale close to linearly (each has its own `World`; the Bevy
task pools are shared globals but unused by single-threaded executors) — not
yet measured. Budget: ~75 k
ticks/s/core in release ⇒ a 100-candidate ES generation of one match each
≈ 20 s on one core, ~5 s on four.

### 2.7 Changes needed in other files (not made here)

Deliberately out of scope for this pass; listed so the follow-up PR is mechanical.

1. **`Cargo.toml`**: `[features] gym = []` and gate `mod gym;` with
   `#[cfg(any(test, feature = "gym"))]`. Today the module compiles into the
   binary as dead code (`#![allow(dead_code)]`), which the linker/LTO strips;
   a feature is cleaner and keeps `cargo build --target wasm32-unknown-unknown`
   from even parsing it.
2. **`ai.rs`**: lift every literal in §1.1/§1.2 into a `#[derive(Resource)]
   struct AiTuning` with `Default` = today's values. This is the parameter
   vector for option (a) and costs nothing at runtime.
3. **`ai.rs`**: a `Controller` component / `enum Brain { Rules, Scripted,
   Mlp(&'static Weights) }` on each AI player, consulted by `ai_move` /
   `ai_decisions`, so trained policies can drive some players and rules others.
4. **`gameplay.rs`**: make `PlayerIntent` per-entity (component) instead of a
   single resource, and let `LiveControl` hold a set. Required for
   multi-agent training (3 learned defenders at once) and for AI players to
   use the *same* steal/block/pass code paths as the human. Today
   `steal_attempts`/`block_attempts` hard-code `control.entity`.
5. **`gameplay.rs`**: `shoot_and_pass` accepts an optional pass target slot
   (for a `pass_to` action head) instead of always `nearest_teammate`.
6. **`gameplay.rs`**: the AI shot must move the ball to the release point
   (§3.2) and increment `BoxLine.fg_att`, so shooting % is measurable.
7. **`gameplay.rs`**: fix `GameRng::f32` (§3.1).
8. **`units.rs` / `gameplay.rs`**: move `PoseClock` ticking out of
   `animate_rigs` into a `FixedUpdate` system, and move `tick_clock`,
   `follow_dribble`, `handle_buckets`, `inbound_after_score`,
   `pose_timeouts`, `stamina_regen`, `separate_players` to `FixedUpdate`
   (§5.1). This is also a correctness fix for the shipped game.
9. Optional: a native-only bin (`src/bin/gym_server.rs`) or an env-var switch
   in `main.rs` for the Python bridge (§4.3).

---

## 3. Findings from the spike (blockers for learning)

### 3.1 `GameRng::f32()` never exceeded 0.0039 (fixed in this branch)

```rust
// gameplay.rs
pub fn f32(&mut self) -> f32 {
    self.0 = self.0.wrapping_mul(0x5851F42D4C957F2D).wrapping_add(1);
    ((self.0 >> 40) as u32) as f32 / (u32::MAX as f32)   // 24 bits ÷ 2^32
}
```

`>> 40` leaves 24 bits (max 16 777 215) but the divisor is `u32::MAX`, so the
result is in `[0, 0.0039]` (verified: 100 k draws, max 0.003906, mean 0.00195).
Fix applied: `(self.0 >> 40) as f32 / (1u64 << 24) as f32`. Consequences in the
game before the fix:

- every `rng.f32() > chance` test is false ⇒ **every shot, human or AI, is aimed
  dead-centre**; `shot_make_chance`, ratings, meter timing, contest, heat and
  stamina have no effect on the aim;
- `rng.f32() < steal_chance(..)` is true whenever the thief is within reach ⇒
  **every reach-in inside 0.64 m is a steal**, regardless of `steal`/`handle`;
- every block inside the window succeeds;
- `rng.range(a, b)` ≈ `a`, so miss scatter is a constant offset;
- seeds are irrelevant: the spike's AI-vs-AI matches are identical for seeds 1, 2, 3.

Fix: `(self.0 >> 40) as f32 / (1u64 << 24) as f32` (or `>> 32` and keep the
`u32::MAX` divisor). Any RL reward built on “points” before this fix would
teach a policy to spam steal and to shoot from wherever the *geometry* happens
to go in, which brings us to:

### 3.2 AI shots launch from the dribble position

`ai_decisions` solves the ballistic velocity from `(x, 1.85, z)` but never
moves the ball there (`&Transform` is immutable in its query); `follow_dribble`
had left the ball at the hand offset (`right·0.46 + forward·0.22`) at dribble
height (~0.2–0.8 m). Measured mean release offset: **1.27 m**. The human path
does `btf.translation = me_pos + Y*height` and is fine. Net effect, measured
over a regulation AI-vs-AI match: **168 shots, 0 points**. This alone explained
“the opposition is weak”. **Fixed in this branch** together with §3.1:
`gym::tests::ai_vs_ai_match_produces_a_real_score` now guards it (seed 1:
28 shots, 26-22, both benches score).

### 3.3 Long shots undershoot even when aimed perfectly

`integrate_ball` is semi-implicit Euler at 1/64 s; a shot solved analytically
arrives `0.5·g·dt·t` low — 0.2 m on a 1.6 s flight — and, at ~12 m/s vertical
speed, ~0.16 m short horizontally, outside the 0.175 m score cylinder. Shots
inside ~6 m are fine; logo heaves clang the front rim by construction (the
`green_home_jumper_still_threads_the_cylinder` test covers 12.5 m, where the
shortfall is ~0.15 m — inside the 0.175 m cylinder with 2 cm to spare). Either compensate in
`ballistic_velocity` (add `0.5·g·dt` to `vy`) or integrate exactly for shots.
A learned shooter would otherwise discover a hard “never beyond X m” rule that
is an integrator artefact.

### 3.4 Arena hangtime changes gravity but not the solver

`integrate_ball` scales gravity by `theme.hangtime.recip()` (1.22, 1.12, 0.96,
1.04 in four of five arenas) while `ai.rs` and `gameplay.rs` solve with plain
`GRAVITY`, so aim is systematically off in every arena but Neo Tokyo. A policy
trained in one arena will not transfer; either pass the effective gravity to
the solver or include the arena in the observation.

### 3.5 Smaller gaps

- AI `fg_att`/`fg_made` are not tracked (only human shots bump the box score).
- `ai_move` excludes the human from the loose-ball hunt via `p.human`, not
  `LiveControl`; when the gym controls an away player, that player can be
  selected as its team's hunter and then skipped, so nobody on that team
  chases until the policy does. Use `LiveControl` in the hunter filter.
- Any pickup — including your own rebound — resets the shot clock (§5.3).
- `MatchConfig::default()` is already the baseline used everywhere; the gym
  takes a `MatchConfig` so lineups/arenas can be randomised per episode.

---

## 4. Algorithm options, ranked by fit

### 4.1 (a) Tune the rule-based AI with evolutionary search — recommended first

**What**: expose `AiTuning` (§2.7-2; ~20–30 scalars: reaction interval, open
radius, shot thresholds, drive depth, spacing, defense lerp, hunter speed
factor, dunk gate, pass preference weights, plus *new* rule terms such as
“close out on the handler when within R” and “attempt steal when
`steal_chance > p`”), and search it with a (μ,λ)-ES or CMA-ES in the gym.

**Fitness**: point differential of AI-tuned team vs. (i) the current default
AI and (ii) the scripted driver in `gym::tests::scripted` (a human proxy),
averaged over K matches with *common random numbers* (same seeds for every
candidate in a generation; needs §3.1 fixed) to cut variance. Add a
regulariser for “fun”: penalise shot-clock violations, > N s without a pass,
and rubber-band exploits (§5.3).

**Cost**: 100 candidates × 2 matches × 0.2 s ≈ 40 core-seconds/generation; a
few hundred generations is minutes on a workstation. Zero runtime cost:
results ship as the new `Default` constants (or as `Difficulty` presets).
CMA-ES for 30 dims is ~150 lines of Rust (or the `cmaes` crate as a
dev/feature-gated native dependency; no new crate is needed to start — a
simple (1+λ)-ES with Gaussian mutation works at this dimension).

**Why first**: it needs only the gym as-is plus the `AiTuning` resource, it is
robust to reward misspecification (you can eyeball the resulting constants),
and it doubles as a regression harness for the §3 fixes.

### 4.2 (b) Small MLP policy trained natively, shipped as bytes — recommended second

**What**: a 66→64→64→(9+2+7) MLP (≈9.5 k parameters ≈ 38 KB as f32, ≈10 KB
quantised to i8) per role (defender / offense-off-ball / ball handler), trained
in the gym, exported with `include_bytes!`, executed by a ~40-line
hand-written forward pass (ReLU, tanh output) in `ai.rs`. Runtime: 5 players ×
16 Hz × ~10 k MACs ≈ 0.8 M MAC/s — irrelevant even on a phone in WASM, and no
ML crate anywhere in the game binary.

**Training**: two viable routes without leaving Rust —
- *ES on weights* (OpenAI-ES / augmented random search): perturb the weight
  vector, evaluate a possession-level or quarter-level return, update. Needs
  only `Gym::step`; parallelises trivially with `std::thread` (one `Gym` per
  thread); tolerates the discrete/hybrid action space and sparse reward;
  9.5 k dims is well inside ES's comfort zone. Antithetic sampling + rank
  normalisation is ~100 lines.
- *PPO in Rust*: needs autograd. Options: hand-write backprop for a 2-layer
  MLP (feasible, ~200 lines), or make `burn`/`candle` a **native-only,
  feature-gated dev dependency** (`[target.'cfg(not(target_arch =
  "wasm32"))'.dependencies]` under `feature = "train"`). Justified because it
  never enters the WASM build, but ES first avoids the dependency entirely.

**Curriculum**: (1) defender vs. rule-based offense (fix §3.2 first or the
defender learns nothing because nobody scores); (2) ball handler vs. tuned
rule-based defense; (3) alternate sides (self-play lite, keep a pool of past
opponents to avoid cycling). Randomise lineups and arenas per episode; feed
`Ratings` into the observation so one network covers all characters.

**Integration**: `Brain::Mlp` (§2.7-3) reads the same observation function the
gym uses (move `Gym::observe` body into a `pub fn observe(world, entity)` in
`gym/` and call it from `ai.rs`) and writes a per-entity `PlayerIntent`
(§2.7-4). Mixed teams (one learned, two rule-based) are then free.

### 4.3 (c) Python bridge to Stable-Baselines3 / CleanRL — optional, last

**What**: a native-only binary that owns N `Gym`s and speaks a length-prefixed
binary protocol over stdin/stdout (or a Unix socket): `reset(seeds) →
obs[N×66]`, `step(actions[N]) → obs, rewards, dones`. A 60-line `gymnasium.Env`
/ `VecEnv` wrapper on the Python side, then SB3 PPO / DQN as-is.

**Plumbing**: framing without new crates is easy (`f32::to_le_bytes`,
`std::io`); `serde_json` would be convenient but pulls into the binary unless
feature-gated. IPC at ~10–50 k messages/s per process is the bottleneck, not
the sim; batching N = 64–256 envs per message amortises it. Weight export back
to the game is a NumPy → flat f32 file → `include_bytes!` step, plus the same
forward pass as (b).

**When it pays**: if you want to try recurrent policies, curiosity, or
population-based training quickly; the ecosystem does that better than
anything we would write. It does not change what ships (still (b)'s forward
pass), so it can wait until the ES baselines plateau.

### 4.4 Recommended path and milestones

- **M0 — sim correctness** (§3): RNG fix, AI release point, `fg_att` for AI,
  integrator compensation, theme gravity in solver. Re-run the gym baseline
  test: AI-vs-AI must score. Add a test that shot % over 500 AI shots falls in
  a sane band per rating.
- **M1 — gym hardening** (§2.7 1–5, 8): feature flag, `AiTuning`, per-entity
  intents, `Brain` enum, `FixedUpdate` migration, possession-episode mode
  (`Gym::step_possession`), frame-skip, ratings + mirror in observation, a
  `GymStats` (shots, %, turnovers, time-of-possession) for fitness terms.
- **M2 — ES tuning tool**: `--features gym` native bin `finnball-tune` with a
  simple ES/CMA-ES over `AiTuning`, thread-per-`Gym`, CSV log of best
  parameters per generation, `--eval` mode that prints a box score vs. the
  scripted human proxy. Ship the winner as the new defaults and as
  Rookie/Pro/Legend presets (§6).
- **M3 — MLP defender**: hand-written MLP + ES on weights, defender role
  first, export `assets/brains/defender.f32` → `include_bytes!`, `Brain::Mlp`
  on away players, A/B in the game behind a debug key. Then ball handler.
- **M4 (optional) — Python bridge** for PPO/recurrent experiments; weights
  flow back through the same export.

---

## 5. Risks

### 5.1 Sim/render divergence

- `tick_clock`, `follow_dribble`, `handle_buckets`, `inbound_after_score`,
  `pose_timeouts` (gameplay), `stamina_regen`, `separate_players`,
  `face_velocity`, `animate_rigs` (units) run in `Update` — at 60/120/144 Hz in
  the game, at exactly 64 Hz in the gym. `separate_players` pushes half the
  overlap per *frame*, `face_velocity` slerps 0.35 per *frame*, and the ball
  is placed in the hand once per *frame*, so player spacing, dribble height at
  pickup time and pose timing depend on the display rate. A policy trained
  headless will see slightly different dynamics on a 120 Hz iPhone. Fix by
  moving those systems to `FixedUpdate` (§2.7-8); until then, validate
  policies in the real client with `TimeUpdateStrategy::FixedTimesteps(1)`
  behind a debug flag.
- `PoseClock` is advanced inside `animate_rigs`, a visual system. The gym has
  to load the whole rig animation to make pose timeouts work; if `UnitsPlugin`
  is ever split for the headless case, that tick must move.
- Only what is in the observation exists for the policy: the shot meter value
  (`ShotMeter`), `AiBrain.think`, and `PoseClock` are hidden state today.

### 5.2 Determinism

- RNG: the game uses its own LCG (`GameRng`, resource, seeded
  `0x9E3779B97F4A7C15`); `court.rs`/`crowd.rs`/`fx.rs`/`audio.rs` have their
  own visual RNGs that the gym never loads. `Gym::reset(seed)` reseeds
  `GameRng`. The gym uses a separate SplitMix64 (`GymRng`) for exploration so
  action noise never perturbs the game stream.
- Scheduling: Bevy's multi-threaded executor orders ambiguous, conflicting
  systems by thread timing (several exist in `GameplayPlugin`/`UnitsPlugin`
  `Update` tuples). The gym forces `ExecutorKind::SingleThreaded` on every
  schedule; `same_seed_and_actions_are_bit_identical` verifies bit-identical
  traces over 1 500 ticks. The shipped game remains order-ambiguous, which is
  another reason to `.chain()` or move those systems.
- Floats: identical on one machine/target; `sin/cos/exp/hypot` may differ by an
  ULP between x86 and wasm libm, so cross-platform *replay* is not guaranteed
  (irrelevant for training, relevant if you ever ship recorded demos).
- Time: `TimeUpdateStrategy::FixedTimesteps(1)` means wall-clock never enters
  the sim; `Time` and `Time<Fixed>` both advance 1/64 s per step.

### 5.3 Reward hacking the current rules would invite

- **Steal spam** (§3.1): until the RNG is fixed any reach-in inside 0.64 m
  always succeeds and has no foul cost. Add a miss cost (the `Stumble` pose is
  0.5 s) and fix the RNG before rewarding steals.
- **Camping under the rim**: no defensive three-seconds, no out-of-bounds
  (walls bounce), and inbounds teleport players, so a defender that parks
  under the hoop is never punished. Reward proximity-to-handler and
  time-to-close-out, not just points allowed.
- **Shot-clock laundering**: every pickup resets the shot clock, including your
  own miss/rebound and a pass to yourself off the wall. Reset only on change of
  possession or make it a `MatchConfig` option for the gym.
- **Integrator exploits** (§3.3/§3.4): a shooter will learn the exact ranges
  where the Euler drop lands the ball in the cylinder rather than learning to
  get open.
- **Reward scale**: points are sparse (~10 per match today); use
  possession-level episodes and shot-quality shaping so ES/PPO gets signal
  before the first bucket.

### 5.4 Keeping the WASM binary small

- Nothing training-related in the game: ES/PPO/Python live behind
  `cfg(not(target_arch = "wasm32"))` + a `train`/`gym` feature; the shipped
  policy is a flat weight blob + a 40-line forward pass.
- Weight blobs: 38 KB f32 per role, ~10 KB as i8 with a per-layer scale; keep
  ≤ 3 roles ≈ 30–120 KB, gzip-friendly. Compare with the current ~24 MB
  `www/game/finnball_bg.wasm` (`wasm-release`, before `wasm-opt`) — noise.
- `src/gym/mod.rs` is wasm-clean (no `Instant`, threads or files) and dead in
  the binary; the feature flag (§2.7-1) removes even the compile cost.
- Never route the observation through allocation-heavy paths per tick in the
  client: write a fixed `[f32; OBS_LEN]` variant of `observe` for `Brain::Mlp`.

---

## 6. Difficulty and per-character skill knobs

Whether the brain is tuned rules or an MLP, expose these at the *controller*
layer so one policy yields many opponents:

| knob | mechanism | feel |
|---|---|---|
| reaction delay | act on an observation `d` ticks old (ring buffer); today `AiBrain.think ≥ 0.45 s` for ball decisions and 0 for movement | Rookie 0.35 s, Pro 0.2 s, Legend 0.1 s; scale by `ratings.speed` for individuality |
| action noise ε | with prob ε per decision take a random/“held” action | sloppy vs. sharp |
| policy temperature | sample from softmax/T instead of argmax | T high = erratic, T→0 = optimal |
| observation noise | jitter ball/opponent positions by σ before feeding the net | “misreads” the play |
| skill curves | `shot_make_chance` already maps `rating → %`; make its `0.18 + 0.74·skill` slope a per-difficulty curve, fix `ai_decisions` to use it instead of `0.55 + three/400` | ratings actually matter |
| archetype rewards | train/tune with per-character reward mixes: dunk bonus for Jin/Kenji, three bonus for Mika, steal bonus for Zero, rebound for Taro | distinct personalities from one net |
| ratings in obs | feed `Ratings` (11 floats) so the same weights behave differently per body | no per-character retraining |
| rule-tuning presets | three `AiTuning` sets from (a) | cheap difficulty ladder with zero runtime cost |

Ship as `Difficulty { reaction_s, epsilon, temperature, obs_sigma, tuning:
AiTuning }` in `MatchConfig`, selectable from the menu and overridable per
character (`CharacterProfile` gains a `Personality` with reaction/aggression
multipliers).

---

## 7. Running the spike

```
CARGO_TARGET_DIR=/workspace/target cargo test --offline gym -- --nocapture   # 8 tests, prints steps/s
CARGO_TARGET_DIR=/workspace/target cargo test --offline                      # whole suite (75)
CARGO_TARGET_DIR=/workspace/target cargo check --offline
```

Tests in `src/gym/mod.rs`: observation layout/finite; 2 000-step stability +
throughput; bit-identical replay; reset; controlling an away defender; full
match reaches `GameOver` with buckets and reward; AI-vs-AI match produces a real
score (regression guard for the RNG + release-point fixes);
`GymRng` uniformity.

Files: `docs/rl-gym.md` (new), `src/gym/mod.rs` (new), `src/main.rs`
(`mod gym;` added).
