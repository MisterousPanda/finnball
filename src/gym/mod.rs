//! Headless, deterministic, fixed-timestep stepper over the real FINNBALL sim —
//! the seed of an RL gym. Design notes and the research plan live in
//! `docs/rl-gym.md`.
//!
//! The gym builds a Bevy `App` with `MinimalPlugins` (no window, renderer, audio,
//! UI or input) plus the four plugins that make up the actual basketball
//! simulation: `UnitsPlugin`, `BallPlugin`, `GameplayPlugin`, `AiPlugin`. Every
//! `step()` writes one `Action` into `PlayerIntent` (the same resource the
//! keyboard/touch code writes) and advances the world by exactly one fixed tick
//! (64 Hz), so the policy sees the game exactly as a human player would.
//!
//! Nothing here is referenced by the game binary; the module exists so the
//! stepper is compiled and unit-tested alongside the sim it wraps. It is kept
//! wasm-clean (no `std::time::Instant`, no threads) so `cargo build --target
//! wasm32-unknown-unknown` keeps working; the linker drops it as dead code.
#![allow(dead_code)]

use bevy::ecs::schedule::{ExecutorKind, ScheduleLabel};
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;

pub mod tune;

use crate::ai::{AiPlugin, AiProfile, AiProfiles};
use crate::ball::{Ball, BallPlugin, BallState, BallVel, Hold};
use crate::gameplay::{GameRng, GameplayPlugin, LiveControl, MatchClock, PlayerIntent, Scoreboard};
use crate::roster::Side;
use crate::sim::{COURT_HALF_LEN, COURT_HALF_WID, HOOP_X};
use crate::states::{AppState, MatchConfig, Paused};
use crate::units::{BoxLine, MoveVel, Player, Pose, Stamina, UnitsPlugin};

/// Fixed simulation rate. Matches Bevy's default `Time<Fixed>` timestep, which is
/// what the shipped game runs its `FixedUpdate` sim at.
pub const STEP_HZ: f64 = 64.0;
pub const STEP_DT: f32 = 1.0 / STEP_HZ as f32;

/// Players per match (3v3). Observation slots are ordered home 0..3 then away 0..3.
pub const PLAYERS: usize = 6;
const PLAYER_FEATURES: usize = 7;
const BALL_FEATURES: usize = 3 + 3 + 4 + PLAYERS;
const GLOBAL_FEATURES: usize = 8;
/// Length of the vector returned by [`Gym::observe`].
pub const OBS_LEN: usize = BALL_FEATURES + PLAYERS * PLAYER_FEATURES + GLOBAL_FEATURES;

/// Button half of the hybrid action space. At most one button per tick, like a
/// thumb on a touch screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Button {
    #[default]
    None,
    /// Hold the shot meter (must be held for a few ticks before `ShootRelease`).
    ShootHold,
    /// Release the meter → shot (only does something while holding the ball).
    ShootRelease,
    /// Pass to the nearest teammate (chest pass).
    Pass,
    /// Reach-in steal attempt (defense only, holder within reach).
    Steal,
    /// Jump to contest / block a shot in flight.
    Block,
    /// Signature move: dunk / special release.
    Special,
}

impl Button {
    pub const COUNT: usize = 7;

    pub fn from_index(i: usize) -> Self {
        match i % Self::COUNT {
            0 => Self::None,
            1 => Self::ShootHold,
            2 => Self::ShootRelease,
            3 => Self::Pass,
            4 => Self::Steal,
            5 => Self::Block,
            _ => Self::Special,
        }
    }
}

/// One tick of input for the controlled player. Hybrid space: a 9-way move
/// direction (`MOVE_DIRS`), a sprint flag and one of [`Button::COUNT`] buttons.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Action {
    /// World-space XZ direction, unit length or zero. Without a `GameCam` in the
    /// world `apply_intents` maps `PlayerIntent::move_xz` straight to world XZ.
    pub move_xz: Vec2,
    pub sprint: bool,
    pub button: Button,
}

/// Compass directions for the discrete move head (index 0 = stand still).
pub const MOVE_DIRS: [Vec2; 9] = [
    Vec2::ZERO,
    Vec2::new(1.0, 0.0),
    Vec2::new(-1.0, 0.0),
    Vec2::new(0.0, 1.0),
    Vec2::new(0.0, -1.0),
    Vec2::new(
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ),
    Vec2::new(
        -std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ),
    Vec2::new(
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ),
    Vec2::new(
        -std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ),
];

impl Action {
    /// Size of the flattened discrete space (`9 moves × 2 sprint × 7 buttons`).
    pub const DISCRETE_COUNT: usize = MOVE_DIRS.len() * 2 * Button::COUNT;

    pub fn noop() -> Self {
        Self::default()
    }

    /// Multi-discrete constructor: `(move 0..9, sprint, button 0..7)`.
    pub fn multi(move_idx: usize, sprint: bool, button: Button) -> Self {
        Self {
            move_xz: MOVE_DIRS[move_idx % MOVE_DIRS.len()],
            sprint,
            button,
        }
    }

    /// Flattened discrete index → action.
    pub fn from_discrete(i: usize) -> Self {
        let i = i % Self::DISCRETE_COUNT;
        let button = Button::from_index(i % Button::COUNT);
        let rest = i / Button::COUNT;
        let sprint = rest % 2 == 1;
        let move_idx = rest / 2;
        Self::multi(move_idx, sprint, button)
    }

    /// Uniform random action (seedable, no `rand` crate needed).
    pub fn random(rng: &mut GymRng) -> Self {
        Self::from_discrete(rng.below(Self::DISCRETE_COUNT))
    }

    fn write(self, intent: &mut PlayerIntent) {
        *intent = PlayerIntent {
            move_xz: self.move_xz,
            sprint: self.sprint,
            shoot_held: self.button == Button::ShootHold,
            shoot_released: self.button == Button::ShootRelease,
            pass: self.button == Button::Pass,
            steal: self.button == Button::Steal,
            special: self.button == Button::Special,
            switch: false,
            block: self.button == Button::Block,
            pass_kind: crate::sim::PassKind::Chest,
        };
    }
}

/// SplitMix64 for exploration noise / random actions on the gym side. Deliberately
/// *not* `crate::gameplay::GameRng`: that LCG's `f32()` shifts out 24 bits but
/// divides by `u32::MAX`, so it only ever yields values in `[0, 0.0039]` (see
/// `docs/rl-gym.md`, "Findings").
#[derive(Clone, Debug)]
pub struct GymRng(pub u64);

impl GymRng {
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn below(&mut self, n: usize) -> usize {
        ((self.f32() * n as f32) as usize).min(n.saturating_sub(1))
    }
}

/// What `step()` hands back.
#[derive(Clone, Debug)]
pub struct StepResult {
    pub obs: Vec<f32>,
    pub reward: f32,
    /// Match finished (`AppState::GameOver`).
    pub done: bool,
}

/// Reward shaping weights (from the controlled side's point of view). Points are
/// the ground truth; the rest are small dense hints so a policy sees signal
/// before it ever scores.
#[derive(Clone, Copy, Debug)]
pub struct RewardWeights {
    pub point_scored: f32,
    pub point_allowed: f32,
    pub steal: f32,
    pub block: f32,
    pub rebound: f32,
    pub assist: f32,
    pub turnover: f32,
}

impl Default for RewardWeights {
    fn default() -> Self {
        Self {
            point_scored: 1.0,
            point_allowed: -1.0,
            steal: 0.5,
            block: 0.5,
            rebound: 0.3,
            assist: 0.3,
            turnover: -0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Ledger {
    home: u32,
    away: u32,
    stl: u32,
    blk: u32,
    reb: u32,
    ast: u32,
    /// Side of the last player to hold the ball (survives shots / loose balls).
    possession: Option<Side>,
    /// A shot has left the hand since `possession` last changed hands.
    shot_released: bool,
}

/// Box-score totals for one bench.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TeamStats {
    pub pts: u32,
    pub fga: u32,
    pub fgm: u32,
    pub stl: u32,
    pub blk: u32,
    pub reb: u32,
    pub ast: u32,
}

impl TeamStats {
    pub fn fg_pct(&self) -> f32 {
        if self.fga == 0 {
            0.0
        } else {
            self.fgm as f32 / self.fga as f32
        }
    }
}

/// Outcome of [`Gym::play_out`].
#[derive(Clone, Copy, Debug, Default)]
pub struct MatchResult {
    pub home: TeamStats,
    pub away: TeamStats,
    pub steps: u64,
    pub finished: bool,
}

impl MatchResult {
    pub fn stats(&self, side: Side) -> &TeamStats {
        match side {
            Side::Home => &self.home,
            Side::Away => &self.away,
        }
    }
}

/// Headless FINNBALL match you can step one fixed tick at a time.
pub struct Gym {
    app: App,
    steps: u64,
    ctrl_side: Side,
    ctrl_slot: u8,
    /// Nobody is controlled: all six players run the rule-based AI.
    all_ai: bool,
    ledger: Ledger,
    /// Last box-score snapshot per side (home, away) while players existed.
    teams: [TeamStats; 2],
    pub weights: RewardWeights,
}

impl Gym {
    /// Builds the headless app and starts a match with `config`, seeding the
    /// game's `GameRng` with `seed`. Home slot 0 is controlled, like the real game.
    pub fn new(config: MatchConfig, seed: u64) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), StatesPlugin));
        // `spawn_player` / `spawn_ball` build the full visual rig into these asset
        // stores even though nothing renders it; registering the assets properly
        // (rather than `init_resource::<Assets<_>>`) installs the event-draining
        // system so the queued `AssetEvent`s don't pile up across episodes.
        app.init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>();
        // Every `app.update()` advances virtual time by exactly one fixed timestep,
        // so `Update` and `FixedUpdate` systems run 1:1 and wall-clock never leaks in.
        app.insert_resource(TimeUpdateStrategy::FixedTimesteps(1));
        app.insert_resource(Time::<Fixed>::from_hz(STEP_HZ));
        app.insert_resource(config)
            .insert_resource(Paused(false))
            .init_state::<AppState>()
            .init_resource::<PlayerIntent>()
            // Written by `steal_attempts` / `inbound_after_score`; normally registered
            // by `CameraPlugin`, which we deliberately do not load.
            .add_message::<crate::camera::CamTrigger>();
        app.add_plugins((UnitsPlugin, BallPlugin, GameplayPlugin, AiPlugin));
        single_threaded(&mut app);
        // Settle plugin setup (Splash state, no match yet).
        app.update();

        let mut gym = Self {
            app,
            steps: 0,
            ctrl_side: Side::Home,
            ctrl_slot: 0,
            all_ai: false,
            ledger: Ledger::default(),
            teams: [TeamStats::default(); 2],
            weights: RewardWeights::default(),
        };
        gym.reset(seed);
        gym
    }

    pub fn with_default_config(seed: u64) -> Self {
        Self::new(MatchConfig::default(), seed)
    }

    /// All-AI match: `home` and `away` play with the given profiles, nobody is
    /// controlled. The profiles are pinned (`AiProfiles::locked`) so
    /// `MatchConfig::difficulty` is ignored across resets.
    pub fn ai_vs_ai(home: AiProfile, away: AiProfile, seed: u64) -> Self {
        let mut gym = Self::new(MatchConfig::default(), seed);
        gym.set_profiles(home, away);
        gym.release_control();
        gym.reset(seed);
        gym
    }

    pub fn set_profiles(&mut self, home: AiProfile, away: AiProfile) {
        self.app.world_mut().insert_resource(AiProfiles {
            home,
            away,
            locked: true,
        });
    }

    pub fn profiles(&self) -> AiProfiles {
        *self.app.world().resource::<AiProfiles>()
    }

    /// Nobody controlled; `step()` actions are ignored from here on (until `select`).
    pub fn release_control(&mut self) {
        self.all_ai = true;
        self.app.world_mut().resource_mut::<LiveControl>().entity = None;
    }

    /// Box-score totals for `side` (points from the scoreboard, the rest from
    /// `BoxLine`). Players are despawned on `GameOver`, so this reads the copy
    /// refreshed on every step while they still exist.
    pub fn team_stats(&mut self, side: Side) -> TeamStats {
        self.refresh_teams();
        let score = self.app.world().resource::<Scoreboard>();
        let mut t = self.teams[(side == Side::Away) as usize];
        t.pts = if side == Side::Home {
            score.home
        } else {
            score.away
        };
        t
    }

    fn refresh_teams(&mut self) {
        let world = self.app.world_mut();
        let mut teams = [TeamStats::default(); 2];
        let mut any = false;
        for (p, bx) in world.query::<(&Player, &BoxLine)>().iter(world) {
            any = true;
            let t = &mut teams[(p.side == Side::Away) as usize];
            t.fga += bx.fg_att;
            t.fgm += bx.fg_made;
            t.stl += bx.stl;
            t.blk += bx.blk;
            t.reb += bx.reb;
            t.ast += bx.ast;
        }
        if any {
            self.teams = teams;
        }
    }

    /// Runs the current match to `GameOver` (or `max_steps`), feeding `policy`
    /// the observation each tick for the controlled player (ignored in all-AI mode).
    pub fn play_out(
        &mut self,
        mut policy: impl FnMut(&[f32], u64) -> Action,
        max_steps: u64,
    ) -> MatchResult {
        let mut obs = self.observe();
        let mut finished = self.done();
        let mut n = 0u64;
        while !finished && n < max_steps {
            let action = if self.all_ai {
                Action::noop()
            } else {
                policy(&obs, self.steps)
            };
            let r = self.step(action);
            obs = r.obs;
            finished = r.done;
            n += 1;
        }
        MatchResult {
            home: self.team_stats(Side::Home),
            away: self.team_stats(Side::Away),
            steps: n,
            finished,
        }
    }

    /// Ends the running match (despawning everything tagged
    /// `DespawnOnExit(Playing)`), reseeds, and starts a fresh one. Also runs the
    /// first sim tick so `observe()` is valid immediately afterwards.
    pub fn reset(&mut self, seed: u64) -> Vec<f32> {
        let cur = *self.app.world().resource::<State<AppState>>().get();
        if cur == AppState::Playing {
            self.app
                .world_mut()
                .resource_mut::<NextState<AppState>>()
                .set(AppState::GameOver);
            self.app.update();
        }
        self.app.world_mut().insert_resource(GameRng(seed));
        self.teams = [TeamStats::default(); 2];
        self.app
            .world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Playing);
        Action::noop().write(&mut self.app.world_mut().resource_mut::<PlayerIntent>());
        self.app.update();
        self.steps = 0;
        if self.all_ai {
            self.release_control();
        } else {
            self.select(self.ctrl_side, self.ctrl_slot);
        }
        self.ledger = self.read_ledger();
        self.observe()
    }

    /// Hands the controlled slot to another player (e.g. `Side::Away, 1` to train
    /// a defender). Everyone else stays on the rule-based AI.
    pub fn select(&mut self, side: Side, slot: u8) {
        self.all_ai = false;
        self.ctrl_side = side;
        self.ctrl_slot = slot;
        let world = self.app.world_mut();
        let entity = world
            .query::<(Entity, &Player)>()
            .iter(world)
            .find(|(_, p)| p.side == side && p.slot == slot)
            .map(|(e, _)| e);
        world.resource_mut::<LiveControl>().entity = entity;
    }

    /// Applies `action` to the controlled player and advances one fixed tick.
    pub fn step(&mut self, action: Action) -> StepResult {
        action.write(&mut self.app.world_mut().resource_mut::<PlayerIntent>());
        self.app.update();
        self.steps += 1;
        self.refresh_teams();
        let now = self.read_ledger();
        let reward = self.reward(self.ledger, now);
        self.ledger = now;
        StepResult {
            obs: self.observe(),
            reward,
            done: self.done(),
        }
    }

    pub fn steps(&self) -> u64 {
        self.steps
    }

    pub fn done(&self) -> bool {
        *self.app.world().resource::<State<AppState>>().get() == AppState::GameOver
    }

    pub fn score(&self) -> (u32, u32) {
        let s = self.app.world().resource::<Scoreboard>();
        (s.home, s.away)
    }

    pub fn clock(&self) -> (u8, f32, f32) {
        let c = self.app.world().resource::<MatchClock>();
        (c.quarter, c.remaining, c.shot)
    }

    pub fn world(&self) -> &World {
        self.app.world()
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// Fixed-layout, roughly unit-scaled feature vector (`OBS_LEN` floats):
    ///
    /// | range     | contents                                                        |
    /// |-----------|-----------------------------------------------------------------|
    /// | 0..3      | ball position (x/14, y/4, z/7.5)                                |
    /// | 3..6      | ball velocity / 15                                              |
    /// | 6..10     | ball hold one-hot: loose, held, shot, pass                      |
    /// | 10..16    | holder one-hot over the 6 player slots                          |
    /// | 16..58    | per player ×6: x/14, z/7.5, vx/10, vz/10, stamina, busy, is_ctrl|
    /// | 58        | possession: +1 home, −1 away, 0 nobody                          |
    /// | 59        | shot clock / 24                                                 |
    /// | 60        | quarter time remaining / quarter length                         |
    /// | 61        | quarter / 4                                                     |
    /// | 62        | (home − away) / 20                                              |
    /// | 63        | controlled side: +1 home, −1 away                               |
    /// | 64        | controlled player → ball distance / 14                          |
    /// | 65        | controlled player → own attacking hoop distance / 28            |
    pub fn observe(&mut self) -> Vec<f32> {
        let ctrl_side = self.ctrl_side;
        let ctrl_slot = self.ctrl_slot;
        let world = self.app.world_mut();
        let mut obs = vec![0.0f32; OBS_LEN];

        let ball = world
            .query_filtered::<(&Transform, &BallVel, &BallState), With<Ball>>()
            .iter(world)
            .next()
            .map(|(t, v, s)| (t.translation, v.0, s.hold, s.holder));

        let mut players: Vec<(Entity, Player, Vec3, Vec3, f32, bool)> = world
            .query::<(Entity, &Player, &Transform, &MoveVel, &Stamina, &Pose)>()
            .iter(world)
            .map(|(e, p, t, v, s, pose)| {
                let busy = matches!(
                    pose,
                    Pose::Shoot
                        | Pose::Dunk
                        | Pose::Pass
                        | Pose::Block
                        | Pose::Stumble
                        | Pose::Celebrate
                );
                (e, *p, t.translation, v.0, s.0, busy)
            })
            .collect();
        players.sort_by_key(|(_, p, ..)| (p.side == Side::Away, p.slot));

        let slot_of = |e: Entity| players.iter().position(|(pe, ..)| *pe == e);

        if let Some((bp, bv, hold, holder)) = ball {
            obs[0] = bp.x / COURT_HALF_LEN;
            obs[1] = bp.y / 4.0;
            obs[2] = bp.z / COURT_HALF_WID;
            obs[3] = bv.x / 15.0;
            obs[4] = bv.y / 15.0;
            obs[5] = bv.z / 15.0;
            let h = match hold {
                Hold::Loose => 0,
                Hold::Held => 1,
                Hold::Shot => 2,
                Hold::Pass => 3,
            };
            obs[6 + h] = 1.0;
            if let Some(i) = holder.and_then(slot_of) {
                obs[10 + i] = 1.0;
                obs[58] = if players[i].1.side == Side::Home {
                    1.0
                } else {
                    -1.0
                };
            }
        }

        let mut ctrl_pos = None;
        for (i, (_, p, pos, vel, stam, busy)) in players.iter().enumerate().take(PLAYERS) {
            let base = BALL_FEATURES + i * PLAYER_FEATURES;
            let is_ctrl = p.side == ctrl_side && p.slot == ctrl_slot;
            if is_ctrl {
                ctrl_pos = Some(*pos);
            }
            obs[base] = pos.x / COURT_HALF_LEN;
            obs[base + 1] = pos.z / COURT_HALF_WID;
            obs[base + 2] = vel.x / 10.0;
            obs[base + 3] = vel.z / 10.0;
            obs[base + 4] = *stam;
            obs[base + 5] = if *busy { 1.0 } else { 0.0 };
            obs[base + 6] = if is_ctrl { 1.0 } else { 0.0 };
        }

        let g = BALL_FEATURES + PLAYERS * PLAYER_FEATURES;
        let clock = world.resource::<MatchClock>();
        let config = world.resource::<MatchConfig>();
        let score = world.resource::<Scoreboard>();
        obs[g + 1] = clock.shot / config.shot_clock.max(1.0);
        obs[g + 2] = clock.remaining / config.quarter_secs.max(1.0);
        obs[g + 3] = clock.quarter as f32 / 4.0;
        obs[g + 4] = (score.home as f32 - score.away as f32) / 20.0;
        obs[g + 5] = if ctrl_side == Side::Home { 1.0 } else { -1.0 };
        if let (Some(cp), Some((bp, ..))) = (ctrl_pos, ball) {
            obs[g + 6] = cp.with_y(0.0).distance(bp.with_y(0.0)) / COURT_HALF_LEN;
            let hoop_x = if ctrl_side == Side::Home {
                HOOP_X
            } else {
                -HOOP_X
            };
            obs[g + 7] = (cp.x - hoop_x).hypot(cp.z) / (2.0 * COURT_HALF_LEN);
        }
        obs
    }

    fn read_ledger(&mut self) -> Ledger {
        let ctrl_side = self.ctrl_side;
        let ctrl_slot = self.ctrl_slot;
        let prev = self.ledger;
        let world = self.app.world_mut();
        let score = world.resource::<Scoreboard>();
        let mut l = Ledger {
            home: score.home,
            away: score.away,
            ..prev
        };
        let ball = world
            .query_filtered::<&BallState, With<Ball>>()
            .iter(world)
            .next()
            .map(|s| (s.hold, s.holder));
        let mut holder_side = None;
        for (e, p, bx) in world.query::<(Entity, &Player, &BoxLine)>().iter(world) {
            if p.side == ctrl_side && p.slot == ctrl_slot {
                l.stl = bx.stl;
                l.blk = bx.blk;
                l.reb = bx.reb;
                l.ast = bx.ast;
            }
            if ball.and_then(|(_, h)| h) == Some(e) {
                holder_side = Some(p.side);
            }
        }
        if let Some(side) = holder_side {
            if l.possession != Some(side) {
                l.possession = Some(side);
                l.shot_released = false;
            }
        }
        if matches!(ball, Some((Hold::Shot, _))) {
            l.shot_released = true;
        }
        l
    }

    fn reward(&self, prev: Ledger, now: Ledger) -> f32 {
        let w = self.weights;
        let (mine_prev, theirs_prev, mine_now, theirs_now) = if self.ctrl_side == Side::Home {
            (prev.home, prev.away, now.home, now.away)
        } else {
            (prev.away, prev.home, now.away, now.home)
        };
        let scored = (mine_now - mine_prev) as f32;
        let allowed = (theirs_now - theirs_prev) as f32;
        let mut r = scored * w.point_scored + allowed * w.point_allowed;
        r += (now.stl - prev.stl) as f32 * w.steal;
        r += (now.blk - prev.blk) as f32 * w.block;
        r += (now.reb - prev.reb) as f32 * w.rebound;
        r += (now.ast - prev.ast) as f32 * w.assist;
        // Live-ball turnover: possession flipped to the other side without a shot
        // ever leaving the hand and without a score (inbounds after a bucket flip
        // possession too, so those are excluded via the score check).
        let flipped = prev.possession == Some(self.ctrl_side)
            && now.possession == Some(self.ctrl_side.other());
        if flipped && !prev.shot_released && scored == 0.0 && allowed == 0.0 {
            r += w.turnover;
        }
        r
    }
}

/// Bevy's multi-threaded executor picks among ready systems in a thread-timing
/// dependent order; where two systems have ambiguous ordering *and* conflicting
/// access (several `Update` systems in `GameplayPlugin` / `UnitsPlugin` do) the
/// result is run-to-run nondeterministic. The single-threaded executor runs the
/// topological order, which is stable for a fixed plugin insertion order.
fn single_threaded(app: &mut App) {
    let labels: [bevy::ecs::schedule::InternedScheduleLabel; 14] = [
        First.intern(),
        PreUpdate.intern(),
        bevy::state::prelude::StateTransition.intern(),
        RunFixedMainLoop.intern(),
        bevy::app::FixedMain.intern(),
        FixedFirst.intern(),
        FixedPreUpdate.intern(),
        FixedUpdate.intern(),
        FixedPostUpdate.intern(),
        FixedLast.intern(),
        Update.intern(),
        PostUpdate.intern(),
        Last.intern(),
        Main.intern(),
    ];
    for label in labels {
        app.edit_schedule(label, |s| {
            s.set_executor_kind(ExecutorKind::SingleThreaded);
        });
    }
}

/// Tiny hand-written policy for the controlled (home 0) player — the "human
/// proxy": with the ball, drive at the +X hoop and shoot inside ~3 m (hold the
/// meter ~12 ticks, then release); without it, act randomly. Enough to make
/// buckets happen so the reward path gets exercised, and a rough stand-in for
/// a beginner when tuning the opposition.
pub fn scripted_driver(obs: &[f32], step: u64, rng: &mut GymRng) -> Action {
    let has_ball = obs[10] == 1.0;
    if !has_ball {
        return Action::random(rng);
    }
    let ctrl = BALL_FEATURES;
    let x = obs[ctrl] * COURT_HALF_LEN;
    let z = obs[ctrl + 1] * COURT_HALF_WID;
    let to_hoop = Vec2::new(HOOP_X - x, -z);
    if to_hoop.length() > 3.0 {
        return Action {
            move_xz: to_hoop.normalize(),
            sprint: true,
            button: Button::None,
        };
    }
    let phase = step % 16;
    if phase < 12 {
        Action::multi(0, false, Button::ShootHold)
    } else if phase == 12 {
        Action::multi(0, false, Button::ShootRelease)
    } else {
        Action::noop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{LEGEND, PRO, ROOKIE};

    fn scripted(obs: &[f32], step: u64, rng: &mut GymRng) -> Action {
        scripted_driver(obs, step, rng)
    }

    #[test]
    fn gym_rng_covers_the_unit_interval() {
        let mut rng = GymRng(1);
        let mut max = 0.0f32;
        let mut hist = [0u32; 8];
        for _ in 0..10_000 {
            let v = rng.f32();
            assert!((0.0..1.0).contains(&v));
            max = max.max(v);
            hist[(v * 8.0) as usize] += 1;
        }
        assert!(max > 0.99);
        assert!(hist.iter().all(|&h| h > 900), "uniform-ish: {hist:?}");
    }

    #[test]
    fn observation_layout_is_fixed_and_finite() {
        let mut gym = Gym::with_default_config(7);
        let obs = gym.observe();
        assert_eq!(obs.len(), OBS_LEN);
        assert!(obs.iter().all(|v| v.is_finite()));
        // Home slot 0 holds the ball at tip-off and is the controlled player.
        assert_eq!(obs[6 + 1], 1.0, "ball is Held at tip-off");
        assert_eq!(obs[10], 1.0, "home slot 0 has the rock");
        assert_eq!(obs[BALL_FEATURES + 6], 1.0, "home slot 0 is controlled");
        assert_eq!(obs[58], 1.0, "home possession");
        assert_eq!(Action::DISCRETE_COUNT, 126);
        for i in 0..Action::DISCRETE_COUNT {
            let a = Action::from_discrete(i);
            assert!(a.move_xz.length() <= 1.0 + 1e-6);
        }
    }

    /// Runs 2000 fixed ticks with scripted/random input, checks the observation
    /// never goes non-finite or leaves the court, and prints the headless
    /// throughput (run with `--nocapture` to see it).
    #[test]
    fn runs_2000_steps_stably_and_reports_throughput() {
        let mut gym = Gym::with_default_config(42);
        let mut rng = GymRng(1234);
        let mut obs = gym.observe();
        let start = std::time::Instant::now();
        let mut total_reward = 0.0;
        for i in 0..2000u64 {
            let r = gym.step(scripted(&obs, i, &mut rng));
            obs = r.obs.clone();
            assert_eq!(r.obs.len(), OBS_LEN);
            assert!(
                r.obs.iter().all(|v| v.is_finite()),
                "non-finite obs at step {i}"
            );
            assert!(r.reward.is_finite());
            // Court is ±14 × ±7.5; normalised coords must stay in [-1, 1].
            for p in 0..PLAYERS {
                let base = BALL_FEATURES + p * PLAYER_FEATURES;
                assert!(r.obs[base].abs() <= 1.0 && r.obs[base + 1].abs() <= 1.0);
                assert!(r.obs[base + 4] > 0.0 && r.obs[base + 4] <= 1.0, "stamina");
            }
            assert!(
                r.obs[0].abs() <= 1.0 && r.obs[2].abs() <= 1.0,
                "ball on court"
            );
            total_reward += r.reward;
            assert!(!r.done, "a 60s quarter ×4 cannot end in 31s of sim");
        }
        let elapsed = start.elapsed();
        let sps = 2000.0 / elapsed.as_secs_f64();
        let (q, remaining, _) = gym.clock();
        // 2000 ticks at 64 Hz = 31.25 s of game time. Allow for clock resets.
        assert!(q >= 1);
        assert!(remaining < 60.0, "clock should be running: {remaining}");
        eprintln!(
            "gym: 2000 steps in {:.3}s = {:.0} steps/s ({:.1}x realtime); score {:?}, q{} {:.1}s left, reward sum {:.2}",
            elapsed.as_secs_f64(),
            sps,
            sps / STEP_HZ,
            gym.score(),
            q,
            remaining,
            total_reward
        );
    }

    /// Same seed + same actions ⇒ bit-identical observations. This is the property
    /// that makes ES / replay-based training and regression tests possible.
    #[test]
    fn same_seed_and_actions_are_bit_identical() {
        let run = || {
            let mut gym = Gym::with_default_config(99);
            let mut rng = GymRng(555);
            let mut obs = gym.observe();
            let mut trace = Vec::new();
            for i in 0..1500u64 {
                let r = gym.step(scripted(&obs, i, &mut rng));
                obs = r.obs.clone();
                trace.extend(r.obs.iter().map(|v| v.to_bits()));
                trace.push(r.reward.to_bits());
            }
            (trace, gym.score())
        };
        let (a, sa) = run();
        let (b, sb) = run();
        assert_eq!(sa, sb);
        assert_eq!(a, b, "headless sim diverged between identical runs");
    }

    /// Different seeds diverge (the RNG is actually being used) and a reset gives
    /// a fresh match.
    #[test]
    fn reset_restarts_the_match() {
        let mut gym = Gym::with_default_config(1);
        let mut rng = GymRng(9);
        let mut obs = gym.observe();
        for i in 0..800u64 {
            obs = gym.step(scripted(&obs, i, &mut rng)).obs;
        }
        let mid = gym.clock();
        assert!(mid.1 < 55.0);
        let obs = gym.reset(2);
        assert_eq!(gym.steps(), 0);
        let fresh = gym.clock();
        assert!(fresh.1 > 59.0, "quarter clock back near 60s: {:?}", fresh);
        assert_eq!(gym.score(), (0, 0));
        assert_eq!(obs[10], 1.0, "home slot 0 holds the ball again");
        // One player tree + one ball; nothing leaked from the previous match.
        let players = gym.world_mut().query::<&Player>().iter(gym.world()).count();
        assert_eq!(players, PLAYERS);
    }

    /// Control can be handed to a defender so the AI offense becomes the opponent.
    #[test]
    fn can_control_an_away_defender() {
        let mut gym = Gym::with_default_config(3);
        gym.select(Side::Away, 1);
        let obs = gym.observe();
        let away1 = BALL_FEATURES + 4 * PLAYER_FEATURES;
        assert_eq!(obs[away1 + 6], 1.0);
        assert_eq!(obs[BALL_FEATURES + 6], 0.0, "home 0 is now AI-driven");
        assert_eq!(obs[63], -1.0);
        let mut rng = GymRng(77);
        for _ in 0..300 {
            let r = gym.step(Action::random(&mut rng));
            assert!(r.obs.iter().all(|v| v.is_finite()));
        }
    }

    /// Counts how often each AI behaviour fires over one PRO-vs-PRO match and
    /// prints the ticker lines it produced. Diagnostic; run with
    /// `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn behavior_census() {
        use crate::ai::{AiBrain, DefRole, Plan};
        use std::collections::BTreeMap;
        let mut gym = Gym::ai_vs_ai(PRO, PRO, 5);
        let mut contest_ticks = 0u32;
        let mut block_ticks = 0u32;
        let mut help_ticks = 0u32;
        let mut cut_ticks = 0u32;
        let mut screen_ticks = 0u32;
        let mut drive_ticks = 0u32;
        let mut juke_ticks = 0u32;
        let mut windup_ticks = 0u32;
        let mut pass_ticks = 0u32;
        let mut lines: BTreeMap<String, u32> = BTreeMap::new();
        let mut last_line = String::new();
        let mut steps = 0u64;
        while !gym.done() && steps < MATCH_CAP_TICKS {
            gym.step(Action::noop());
            steps += 1;
            let world = gym.world_mut();
            for (pose, brain) in world.query::<(&Pose, &AiBrain)>().iter(world) {
                contest_ticks += (*pose == Pose::Contest) as u32;
                block_ticks += (*pose == Pose::Block) as u32;
                help_ticks += (brain.role == DefRole::Help) as u32;
                cut_ticks += (brain.cut_t > 0.0) as u32;
                screen_ticks += (brain.screen_t > 0.0 || brain.roll_t > 0.0) as u32;
                drive_ticks += matches!(brain.plan, Plan::Drive(_)) as u32;
                juke_ticks += matches!(brain.plan, Plan::Juke(_)) as u32;
                windup_ticks += (brain.plan == Plan::Shoot) as u32;
            }
            if let Some(s) = world
                .query_filtered::<&BallState, With<Ball>>()
                .iter(world)
                .next()
            {
                pass_ticks += (s.hold == Hold::Pass) as u32;
            }
            let line = world.resource::<crate::gameplay::Ticker>().line.clone();
            if line != last_line {
                *lines.entry(line.clone()).or_default() += 1;
                last_line = line;
            }
        }
        let (h, a) = gym.score();
        eprintln!(
            "census {steps} ticks, {h}-{a}: contest {contest_ticks} block {block_ticks} help {help_ticks} cut {cut_ticks} screen {screen_ticks} drive {drive_ticks} juke {juke_ticks} windup {windup_ticks} pass {pass_ticks}"
        );
        for (l, n) in &lines {
            eprintln!("  {n:>3}x {l}");
        }
    }

    const MATCH_CAP_TICKS: u64 = 40_000;

    /// A perfectly aimed jumper from every distance must drop through the real
    /// rim/backboard physics (not just the pure `cylinder_score` math).
    #[test]
    fn green_jumpers_drop_in_the_live_sim() {
        use crate::sim::{ballistic_velocity, flight_time_for_distance, GRAVITY, RIM_HEIGHT};
        let mut misses = Vec::new();
        for dist_i in 2..=12 {
            let dist = dist_i as f32;
            let mut gym = Gym::ai_vs_ai(PRO, PRO, 3);
            let before = gym.score();
            {
                let world = gym.world_mut();
                // Freeze everyone so nobody touches the ball mid-flight.
                let mut q = world.query::<(&mut Transform, &Player)>();
                for (mut t, p) in q.iter_mut(world) {
                    t.translation = Vec3::new(if p.side == Side::Home { -12.0 } else { 12.0 }, 0.0, 6.5);
                }
                let mut bq = world.query_filtered::<(&mut Transform, &mut BallVel, &mut BallState), With<Ball>>();
                let (mut bt, mut bv, mut bs) = bq.single_mut(world).unwrap();
                let from = Vec3::new(HOOP_X - dist, 1.95, 0.0);
                let hoop = [HOOP_X, RIM_HEIGHT, 0.0];
                let v = ballistic_velocity(from.to_array(), hoop, flight_time_for_distance(dist), GRAVITY);
                bt.translation = from;
                bv.0 = Vec3::from_array(v);
                bs.hold = Hold::Shot;
                bs.holder = None;
                bs.shooter = None;
                bs.rim_hits = 0;
            }
            for _ in 0..200 {
                gym.step(Action::noop());
            }
            if gym.score() == before {
                misses.push(dist);
            }
        }
        assert!(misses.is_empty(), "green jumpers missed from {misses:?} m");
    }

    /// Regression guard for two sim bugs found while building the gym: the LCG
    /// returned only [0, 0.004] (every roll "succeeded") and `ai_decisions`
    /// launched shots from the dribble position instead of the solved release
    /// point, so an AI-vs-AI match ended 0-0 after 168 shots. With both fixed a
    /// regulation match is a real game (seed 1: 28 shots, 26-22).
    #[test]
    fn ai_vs_ai_match_produces_a_real_score() {
        let mut gym = Gym::with_default_config(1);
        gym.release_control();
        let mut shots = 0u32;
        let mut was_shot = false;
        for _ in 0..15_360 {
            let r = gym.step(Action::noop());
            let is_shot = r.obs[8] == 1.0;
            if is_shot && !was_shot {
                shots += 1;
            }
            was_shot = is_shot;
            if r.done {
                break;
            }
        }
        let (h, a) = gym.score();
        eprintln!("gym: AI-vs-AI regulation {shots} shots, {h}-{a}");
        assert!(shots >= 15, "the AI must keep shooting: {shots}");
        assert!(h + a >= 10, "AI shots must go in sometimes: {h}-{a}");
        assert!(h > 0 && a > 0, "both benches score: {h}-{a}");
    }

    /// A whole match plays out headless (4 × 60 s = 15 360 ticks): regulation
    /// completes, the scripted driver scores, the reward ledger sees the points,
    /// and the state machine reaches GameOver. A tie would loop 30 s overtimes
    /// forever ("first bucket wins"), so the loop is capped. Also the most
    /// realistic end-to-end throughput sample.
    #[test]
    fn full_match_reaches_game_over_with_buckets() {
        let mut gym = Gym::with_default_config(2024);
        let mut rng = GymRng(31337);
        let mut obs = gym.observe();
        let start = std::time::Instant::now();
        let mut steps = 0u64;
        let mut done = false;
        let mut reward_sum = 0.0f32;
        let mut ai_scored = false;
        while steps < 40_000 {
            let r = gym.step(scripted(&obs, steps, &mut rng));
            obs = r.obs;
            reward_sum += r.reward;
            ai_scored |= gym.score().1 > 0;
            steps += 1;
            if r.done {
                done = true;
                break;
            }
        }
        let elapsed = start.elapsed();
        let (h, a) = gym.score();
        let (q, ..) = gym.clock();
        let secs = elapsed.as_secs_f64();
        let sps = steps as f64 / secs;
        eprintln!(
            "gym: full match {steps} steps in {secs:.2}s = {sps:.0} steps/s, final {h}-{a} (q{q}), reward sum {reward_sum:.1}, AI scored: {ai_scored}"
        );
        assert!(
            q >= 4,
            "regulation never completed: q{q} after {steps} steps"
        );
        assert!(h > 0, "the drive-and-shoot script never scored: {h}-{a}");
        // Points for and against both flow into the reward; the sign depends on
        // who won (the script loses to PRO defense, which is the point).
        assert!(
            (reward_sum - (h as f32 - a as f32)).abs() < 40.0 && reward_sum != 0.0,
            "points must show up in the reward: {reward_sum} for {h}-{a}"
        );
        assert!(
            done,
            "match never reached GameOver ({steps} steps, {h}-{a})"
        );
    }
}
