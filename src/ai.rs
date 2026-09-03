//! Rule-based AI for every player the human is not controlling.
//!
//! Two systems run in `FixedUpdate`, before the gameplay rules
//! (`GameplaySet`), so anything the AI asks for (a reach-in, a block) is
//! resolved by the same code the human's buttons go through:
//!
//! * [`ai_move`] — positioning. Defense: man assignment with hysteresis,
//!   on-ball pressure with a stance that tightens near the paint, closeouts
//!   with hands up when the handler sets up to shoot, help rotation when the
//!   ball beats its man, ball-you-man denial off the ball, jumping pass lanes,
//!   timed block jumps and reach-in lunges. Offense: spacing to wings /
//!   corners, backdoor cuts, screens that roll, drives that pick the open
//!   side, dribble jukes.
//! * [`ai_decisions`] — the ball handler's shoot / drive / pass call, made every
//!   `AiProfile::reaction` seconds from expected-points estimates that include
//!   contest, shot clock and pass-lane risk. Shots wind up for
//!   `AiProfile::windup` seconds so defenders get a chance to contest, and the
//!   contest at *release* is what decides the make roll.
//!
//! Every numeric knob lives in [`AiProfile`]; [`ROOKIE`], [`PRO`] and
//! [`LEGEND`] are the three shipped presets (`PRO` / `LEGEND` were tuned with
//! the evolutionary search in `gym::tune`). Everything is deterministic given
//! `GameRng`.

use bevy::prelude::*;

use crate::ball::{Ball, BallSpin, BallState, BallVel, Hold};
use crate::gameplay::{
    AiRequests, GameRng, GameplaySet, LastPass, LiveControl, MatchClock, PlayerIntent,
    ShotMeter, Ticker,
};
use crate::roster::Side;
use crate::sim::{
    ballistic_velocity, clamp_to_court, classify_shot, contest_factor, contest_with_hands,
    flight_time_for_distance, heat_make_mult, in_paint, release_height, release_spin, shot_ev,
    shot_kind, shot_make_chance, steal_chance, ShotKind, ShotType, GRAVITY, HOOP_X, RIM_HEIGHT,
    STEAL_REACH_MAX, THREE_RADIUS,
};
use crate::states::{AppState, GameMode, MatchConfig, Paused};
use crate::units::{BoxLine, Heat, MoveVel, Player, Pose, PoseClock, Ratings, Stamina};

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AiProfiles>()
            .add_systems(OnEnter(AppState::Playing), sync_profiles)
            .add_systems(
                FixedUpdate,
                (ai_move, ai_decisions)
                    .chain()
                    .before(GameplaySet)
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

/// Every tunable of the rule-based AI. Distances in metres, times in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AiProfile {
    // --- reaction & skill -------------------------------------------------
    /// Seconds between ball-handler decisions.
    pub reaction: f32,
    /// Seconds between defensive target refreshes (0 = every tick).
    pub def_lag: f32,
    /// Movement speed multiplier.
    pub speed: f32,
    /// Make-chance multiplier on AI shots.
    pub skill: f32,
    /// Equivalent shot-meter error of an AI release (0 = always green).
    pub meter_err: f32,
    /// Seconds the handler holds still before releasing a jumper.
    pub windup: f32,
    // --- defense ------------------------------------------------------------
    /// On-ball stance distance from the handler at the arc.
    pub pressure_dist: f32,
    /// Extra sag per metre the handler stands beyond the arc.
    pub sag: f32,
    /// Hands-up distance when the handler sets up to shoot.
    pub closeout_dist: f32,
    /// Only close out if within this distance of the shooter.
    pub closeout_range: f32,
    /// Handler-to-hoop distance below which help rotates.
    pub help_threshold: f32,
    /// How far the on-ball defender may trail (m along the drive) before the ball counts as "beaten".
    pub help_beaten: f32,
    /// Off-ball: fraction of the man→ball distance to step into the lane.
    pub deny_t: f32,
    /// Off-ball: metres to drop toward the hoop (help side).
    pub deny_sag: f32,
    /// Reach-in lunges per second while on the ball.
    pub steal_rate: f32,
    /// Seconds between lunges.
    pub steal_cooldown: f32,
    /// Probability of jumping at a shot released within reach.
    pub block_aggr: f32,
    /// Jump a pass whose flight comes within this distance.
    pub lane_jump: f32,
    // --- offense ------------------------------------------------------------
    /// Expected points needed to take a shot early in the clock.
    pub shot_ev_min: f32,
    /// Shot clock below which the EV bar drops linearly to zero.
    pub late_clock: f32,
    /// Defender gap that opens a drive.
    pub drive_gap: f32,
    /// A second defender inside this distance while driving → kick out.
    pub kick_dist: f32,
    /// Off-ball defender this far away → backdoor cut.
    pub cut_gap: f32,
    /// Screens per second while the handler probes.
    pub screen_rate: f32,
    /// Weight of receiver openness when picking a pass.
    pub pass_open_w: f32,
    /// Dribble-move probability per decision when pressured.
    pub juke_rate: f32,
}

// The vector view is only walked by the gym's search; the game reads fields.
#[cfg_attr(not(any(test, feature = "gym")), allow(dead_code))]
impl AiProfile {
    /// Number of tunables; the ES search vector length.
    pub const N: usize = 26;
    /// The first `SKILL_KNOBS` entries of `to_array` (reaction, def_lag, speed,
    /// skill, meter_err, windup) are the raw difficulty dial and are held fixed
    /// by the gym search, which only tunes behaviour.
    pub const SKILL_KNOBS: usize = 6;

    /// (min, max) search box per parameter, in `to_array` order. The box is
    /// also a design constraint: a PRO/LEGEND defender must always close out
    /// (`closeout_range` ≥ 2.5 m, hands up inside 1.1 m) and a handler may not
    /// sit on the ball until the buzzer (`late_clock` ≥ 3 s). ROOKIE sits
    /// outside it on purpose.
    pub const BOUNDS: [(f32, f32); Self::N] = [
        (0.10, 0.70), // reaction
        (0.00, 0.50), // def_lag
        (0.80, 1.00), // speed
        (0.80, 1.15), // skill
        (0.00, 0.30), // meter_err
        (0.05, 0.50), // windup
        (0.60, 2.20), // pressure_dist
        (0.00, 0.60), // sag
        (0.45, 1.10), // closeout_dist
        (2.50, 6.00), // closeout_range
        (2.50, 7.50), // help_threshold
        (0.20, 2.50), // help_beaten
        (0.00, 0.80), // deny_t
        (0.00, 1.60), // deny_sag
        (0.00, 0.80), // steal_rate
        (0.30, 3.00), // steal_cooldown
        (0.00, 0.80), // block_aggr
        (0.00, 3.00), // lane_jump
        (0.50, 1.70), // shot_ev_min
        (3.00, 10.0), // late_clock
        (0.60, 3.00), // drive_gap
        (0.60, 3.00), // kick_dist
        (1.20, 5.00), // cut_gap
        (0.00, 2.00), // screen_rate
        (0.00, 1.00), // pass_open_w
        (0.00, 1.00), // juke_rate
    ];

    pub fn to_array(&self) -> [f32; Self::N] {
        [
            self.reaction,
            self.def_lag,
            self.speed,
            self.skill,
            self.meter_err,
            self.windup,
            self.pressure_dist,
            self.sag,
            self.closeout_dist,
            self.closeout_range,
            self.help_threshold,
            self.help_beaten,
            self.deny_t,
            self.deny_sag,
            self.steal_rate,
            self.steal_cooldown,
            self.block_aggr,
            self.lane_jump,
            self.shot_ev_min,
            self.late_clock,
            self.drive_gap,
            self.kick_dist,
            self.cut_gap,
            self.screen_rate,
            self.pass_open_w,
            self.juke_rate,
        ]
    }

    pub fn from_array(a: [f32; Self::N]) -> Self {
        Self {
            reaction: a[0],
            def_lag: a[1],
            speed: a[2],
            skill: a[3],
            meter_err: a[4],
            windup: a[5],
            pressure_dist: a[6],
            sag: a[7],
            closeout_dist: a[8],
            closeout_range: a[9],
            help_threshold: a[10],
            help_beaten: a[11],
            deny_t: a[12],
            deny_sag: a[13],
            steal_rate: a[14],
            steal_cooldown: a[15],
            block_aggr: a[16],
            lane_jump: a[17],
            shot_ev_min: a[18],
            late_clock: a[19],
            drive_gap: a[20],
            kick_dist: a[21],
            cut_gap: a[22],
            screen_rate: a[23],
            pass_open_w: a[24],
            juke_rate: a[25],
        }
    }

    pub fn clamped(&self) -> Self {
        let mut a = self.to_array();
        for (v, (lo, hi)) in a.iter_mut().zip(Self::BOUNDS.iter()) {
            *v = v.clamp(*lo, *hi);
        }
        Self::from_array(a)
    }
}

/// The pre-gym opponent: sags off the ball, never helps, never reaches in,
/// takes whatever shot the old `ai_wants_shot` gate allowed. Soft on purpose.
pub const ROOKIE: AiProfile = AiProfile {
    reaction: 0.55,
    def_lag: 0.40,
    speed: 0.88,
    skill: 0.86,
    meter_err: 0.20,
    windup: 0.42,
    pressure_dist: 2.10,
    sag: 0.30,
    closeout_dist: 1.60,
    closeout_range: 1.80,
    help_threshold: 2.50,
    help_beaten: 2.50,
    deny_t: 0.08,
    deny_sag: 0.40,
    steal_rate: 0.08,
    steal_cooldown: 3.00,
    block_aggr: 0.08,
    lane_jump: 0.40,
    shot_ev_min: 0.80,
    late_clock: 5.00,
    drive_gap: 1.00,
    kick_dist: 0.80,
    cut_gap: 5.00,
    screen_rate: 0.00,
    pass_open_w: 0.30,
    juke_rate: 0.00,
};

/// Default opponent and the human's AI teammates. Behaviour knobs tuned by the
/// (1+λ)-ES in `gym::tune` (40 gens × 10 mutants × 4 seeds, winner's-curse
/// confirmation, seed 7) from `gym::tune::HAND_PRO`; `screen_rate` held at 0.35
/// because the search found screens fitness-neutral and they are a behaviour
/// the player should see. See `docs/rl-gym.md` §8.
pub const PRO: AiProfile = AiProfile {
    reaction: 0.28,
    def_lag: 0.10,
    speed: 0.96,
    skill: 1.00,
    meter_err: 0.08,
    windup: 0.22,
    pressure_dist: 1.08,
    sag: 0.15,
    closeout_dist: 0.66,
    closeout_range: 3.31,
    help_threshold: 5.62,
    help_beaten: 0.45,
    deny_t: 0.52,
    deny_sag: 0.82,
    steal_rate: 0.14,
    steal_cooldown: 1.18,
    block_aggr: 0.71,
    lane_jump: 2.01,
    shot_ev_min: 0.98,
    late_clock: 8.57,
    drive_gap: 1.79,
    kick_dist: 1.50,
    cut_gap: 2.09,
    screen_rate: 0.35,
    pass_open_w: 0.75,
    juke_rate: 0.64,
};

/// PRO's tuned positioning with instant reactions, elite touch and more
/// aggression (`gym::tune::legend_from(PRO)`).
pub const LEGEND: AiProfile = AiProfile {
    reaction: 0.12,
    def_lag: 0.00,
    speed: 1.00,
    skill: 1.12,
    meter_err: 0.02,
    windup: 0.13,
    pressure_dist: 0.97,
    sag: 0.15,
    closeout_dist: 0.66,
    closeout_range: 4.81,
    help_threshold: 5.62,
    help_beaten: 0.31,
    deny_t: 0.62,
    deny_sag: 0.82,
    steal_rate: 0.19,
    steal_cooldown: 0.83,
    block_aggr: 0.80,
    lane_jump: 2.51,
    shot_ev_min: 0.98,
    late_clock: 8.57,
    drive_gap: 1.79,
    kick_dist: 1.50,
    cut_gap: 2.09,
    screen_rate: 0.52,
    pass_open_w: 0.75,
    juke_rate: 0.84,
};

/// Which profile each bench plays with. Filled from `MatchConfig::difficulty`
/// on every match start unless `locked` (the gym pins its own pair).
#[derive(Resource, Clone, Copy, Debug)]
pub struct AiProfiles {
    pub home: AiProfile,
    pub away: AiProfile,
    pub locked: bool,
}

impl Default for AiProfiles {
    fn default() -> Self {
        Self {
            home: PRO,
            away: PRO,
            locked: false,
        }
    }
}

impl AiProfiles {
    pub fn for_side(&self, side: Side) -> &AiProfile {
        match side {
            Side::Home => &self.home,
            Side::Away => &self.away,
        }
    }
}

fn sync_profiles(config: Res<MatchConfig>, mut profiles: ResMut<AiProfiles>) {
    if profiles.locked {
        return;
    }
    profiles.home = PRO;
    profiles.away = config.difficulty.profile();
}

// ---------------------------------------------------------------------------
// Per-player memory
// ---------------------------------------------------------------------------

/// What the ball handler is doing between decisions.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Plan {
    /// Hold / probe the perimeter.
    #[default]
    Probe,
    /// Attack the rim on the given side (`±1` = z sign).
    Drive(f32),
    /// Winding up a jumper (`AiBrain::windup` counts down to release).
    Shoot,
    /// Lateral dribble move for `AiBrain::juke_t`, then drive that side.
    Juke(f32),
}

/// Defensive role for this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DefRole {
    #[default]
    None,
    OnBall,
    Deny,
    Help,
    Zone,
}

#[derive(Component, Default)]
pub struct AiBrain {
    /// Seconds since the last ball decision.
    pub think: f32,
    /// Seconds holding the ball this possession.
    pub hold_t: f32,
    pub plan: Plan,
    pub windup: f32,
    pub juke_t: f32,
    /// Current man on defense.
    pub mark: Option<Entity>,
    pub role: DefRole,
    /// Cached defensive target (refreshed every `AiProfile::def_lag`).
    pub target: Vec3,
    pub retarget: f32,
    /// Hands-up seconds remaining.
    pub contest: f32,
    pub steal_cd: f32,
    pub lunge_t: f32,
    pub block_cd: f32,
    pub cut_t: f32,
    pub cut_cd: f32,
    /// > 0: setting a screen; then `roll_t` > 0: rolling to the rim.
    pub screen_t: f32,
    pub roll_t: f32,
    pub screen_cd: f32,
}

impl AiBrain {
    pub fn with_think(think: f32) -> Self {
        Self {
            think,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Court helpers
// ---------------------------------------------------------------------------

fn attack_hoop_x(side: Side) -> f32 {
    match side {
        Side::Home => HOOP_X,
        Side::Away => -HOOP_X,
    }
}

/// Unit x direction from midcourt toward `side`'s attacking hoop.
fn attack_dir(side: Side) -> f32 {
    attack_hoop_x(side).signum()
}

fn ground(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

fn flat_dist(a: Vec3, b: Vec3) -> f32 {
    (a.x - b.x).hypot(a.z - b.z)
}

/// Distance from `p` to the segment `a→b` (XZ plane).
fn seg_dist(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = ground(b - a);
    let l2 = ab.length_squared();
    if l2 < 1e-4 {
        return flat_dist(p, a);
    }
    let t = (ground(p - a).dot(ab) / l2).clamp(0.0, 1.0);
    flat_dist(p, a + ab * t)
}

fn sign_or(v: f32, fallback: f32) -> f32 {
    if v.abs() < 0.3 {
        fallback
    } else {
        v.signum()
    }
}

#[derive(Clone, Copy)]
struct Snap {
    e: Entity,
    side: Side,
    slot: u8,
    pos: Vec3,
    vel: Vec3,
    r: Ratings,
    pose: Pose,
    ctrl: bool,
    mark: Option<Entity>,
    winding_up: bool,
    hands_up: bool,
}

fn snap_of(snaps: &[Snap], e: Entity) -> Option<&Snap> {
    snaps.iter().find(|s| s.e == e)
}

fn nearest_opp_dist(snaps: &[Snap], side: Side, pos: Vec3) -> f32 {
    snaps
        .iter()
        .filter(|s| s.side != side)
        .map(|s| flat_dist(s.pos, pos))
        .fold(f32::INFINITY, f32::min)
}

/// Strongest contest on a shooter at `pos` from the other side.
fn contest_on(snaps: &[Snap], me: Entity, side: Side, pos: Vec3) -> f32 {
    snaps
        .iter()
        .filter(|s| s.e != me && s.side != side)
        .map(|s| contest_with_hands(s.pos.distance(pos), s.r.block, s.hands_up))
        .fold(0.0, f32::max)
}

/// Defenders within `r` of the pass line `from → to`.
fn lane_risk(snaps: &[Snap], side: Side, from: Vec3, to: Vec3, r: f32) -> f32 {
    snaps
        .iter()
        .filter(|s| s.side != side)
        .map(|s| {
            let d = seg_dist(s.pos, from, to);
            if d < r {
                1.0 - d / r
            } else {
                0.0
            }
        })
        .fold(0.0, f32::max)
}

/// Expected points if `shooter` shoots from where he stands right now.
fn shot_value(
    snaps: &[Snap],
    shooter: &Snap,
    stam: f32,
    streak: u8,
    prof: &AiProfile,
    hands_matter: bool,
) -> (f32, f32, bool) {
    let hoop_x = attack_hoop_x(shooter.side);
    let dist = (shooter.pos.x - hoop_x).hypot(shooter.pos.z);
    let is_three = shot_kind(shooter.pos.x, shooter.pos.z, hoop_x) == ShotKind::Three;
    let rating = if is_three {
        shooter.r.three
    } else if dist < 3.0 {
        shooter.r.mid.max(shooter.r.dunk * 0.85).max(60.0)
    } else {
        shooter.r.mid
    };
    let contest = if hands_matter {
        contest_on(snaps, shooter.e, shooter.side, shooter.pos)
    } else {
        snaps
            .iter()
            .filter(|s| s.side != shooter.side)
            .map(|s| contest_factor(s.pos.distance(shooter.pos), s.r.block))
            .fold(0.0, f32::max)
    };
    let chance = (shot_make_chance(rating, dist, contest, prof.meter_err, stam, is_three)
        * heat_make_mult(streak)
        * prof.skill)
        .min(0.94);
    (shot_ev(chance, is_three), chance, is_three)
}

// ---------------------------------------------------------------------------
// Movement
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    snaps: &'a [Snap],
    bpos: Vec3,
    bvel: Vec3,
    hold: Hold,
    holder: Option<Entity>,
    shooter_side: Option<Side>,
    /// The handler has started his shot (AI windup or human holding the meter).
    set_shot: bool,
    shot_clock: f32,
    dt: f32,
}

struct Move {
    dest: Vec3,
    sprint: bool,
    /// Speed multiplier for probing / holding spots.
    ease: f32,
}

#[allow(clippy::too_many_arguments)]
fn ai_move(
    time: Res<Time<Fixed>>,
    paused: Res<Paused>,
    control: Res<LiveControl>,
    profiles: Res<AiProfiles>,
    meter: Res<ShotMeter>,
    intent: Res<PlayerIntent>,
    clock: Res<MatchClock>,
    mut ticker: ResMut<Ticker>,
    mut reqs: ResMut<AiRequests>,
    mut rng: ResMut<GameRng>,
    ball: Query<(&Transform, &BallState, &BallVel), (With<Ball>, Without<Player>)>,
    mut players: Query<
        (
            Entity,
            &Player,
            &Ratings,
            &mut Transform,
            &mut MoveVel,
            &mut Pose,
            &mut PoseClock,
            &Stamina,
            &mut AiBrain,
        ),
        Without<Ball>,
    >,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let Ok((btf, bstate, bvel)) = ball.single() else {
        return;
    };

    let mut snaps: Vec<Snap> = players
        .iter()
        .map(|(e, p, r, t, v, pose, _, _, brain)| Snap {
            e,
            side: p.side,
            slot: p.slot,
            pos: t.translation,
            vel: v.0,
            r: *r,
            pose: *pose,
            ctrl: control.entity == Some(e),
            mark: brain.mark,
            winding_up: brain.plan == Plan::Shoot,
            hands_up: matches!(*pose, Pose::Block | Pose::Contest),
        })
        .collect();
    snaps.sort_by_key(|s| (s.side == Side::Away, s.slot));

    let holder = bstate.holder;
    let holder_side = holder.and_then(|h| snap_of(&snaps, h)).map(|s| s.side);
    let shooter_side = bstate.shooter.and_then(|h| snap_of(&snaps, h)).map(|s| s.side);
    // Who is "on offense" while the ball is in the air / loose: last team to touch it.
    let poss_side = holder_side.or_else(|| {
        bstate
            .last_touch
            .and_then(|e| snap_of(&snaps, e))
            .map(|s| s.side)
    });
    // Defensive assignments key off the handler, or the passer while a pass flies
    // so marks and lane-jumps carry through the catch.
    let anchor = holder.or(if bstate.hold == Hold::Pass {
        bstate.last_passer
    } else {
        None
    });
    let set_shot = holder
        .and_then(|h| snap_of(&snaps, h))
        .map(|h| h.winding_up || (h.ctrl && meter.armed && intent.shoot_held))
        .unwrap_or(false);
    let ctx = Ctx {
        snaps: &snaps,
        bpos: btf.translation,
        bvel: bvel.0,
        hold: bstate.hold,
        holder,
        shooter_side,
        set_shot,
        shot_clock: clock.shot,
        dt,
    };

    let ball_live_loose = bstate.hold == Hold::Loose
        || (matches!(bstate.hold, Hold::Shot | Hold::Pass)
            && btf.translation.y < 1.6
            && bvel.0.length() < 7.0);
    let ball_flat = ground(btf.translation);
    let hunter_for = |side: Side| -> Option<Entity> {
        snaps
            .iter()
            .filter(|s| s.side == side && !s.ctrl)
            .min_by(|a, b| {
                flat_dist(a.pos, ball_flat)
                    .partial_cmp(&flat_dist(b.pos, ball_flat))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|s| s.e)
    };
    let hunters = [hunter_for(Side::Home), hunter_for(Side::Away)];

    // Defensive assignments for whichever side is defending.
    let assignments: Vec<(Entity, Option<Entity>, DefRole)> = match (anchor, poss_side) {
        (Some(h), Some(hs)) if bstate.hold != Hold::Shot => {
            let def_side = hs.other();
            assign_defense(&snaps, h, def_side, profiles.for_side(def_side))
        }
        _ => Vec::new(),
    };

    for (e, p, ratings, mut tf, mut vel, mut pose, mut pclock, stam, mut brain) in &mut players {
        // Timers tick for everyone (including the human's brain, harmlessly).
        brain.steal_cd = (brain.steal_cd - dt).max(0.0);
        brain.block_cd = (brain.block_cd - dt).max(0.0);
        brain.cut_cd = (brain.cut_cd - dt).max(0.0);
        brain.screen_cd = (brain.screen_cd - dt).max(0.0);
        brain.contest = (brain.contest - dt).max(0.0);
        brain.retarget -= dt;
        if holder != Some(e) {
            brain.hold_t = 0.0;
            brain.windup = 0.0;
            if matches!(brain.plan, Plan::Shoot | Plan::Drive(_) | Plan::Juke(_)) {
                brain.plan = Plan::Probe;
            }
        }
        if control.entity == Some(e) {
            continue;
        }
        if matches!(
            *pose,
            Pose::Shoot | Pose::Dunk | Pose::Pass | Pose::Stumble | Pose::Celebrate | Pose::Block
        ) {
            vel.0 *= 0.7;
            continue;
        }
        let prof = profiles.for_side(p.side);
        let me = *snap_of(&snaps, e).expect("snapshot covers every player");
        let has_ball = holder == Some(e);
        let on_offense = poss_side.map(|s| s == p.side).unwrap_or(true);

        let mv = if ball_live_loose && hunters.contains(&Some(e)) {
            brain.role = DefRole::None;
            Move {
                dest: ball_flat,
                sprint: true,
                ease: 1.0,
            }
        } else if ctx.hold == Hold::Shot {
            brain.role = DefRole::None;
            shot_in_air_move(&ctx, &me, prof, &mut brain, &mut reqs, &mut rng)
        } else if has_ball {
            brain.role = DefRole::None;
            handler_move(&ctx, &me, &mut brain)
        } else if on_offense {
            brain.role = DefRole::None;
            offball_move(&ctx, &me, prof, &mut brain, &mut rng)
        } else {
            let (mark, role) = assignments
                .iter()
                .find(|(d, ..)| *d == e)
                .map(|(_, m, r)| (*m, *r))
                .unwrap_or((None, DefRole::Zone));
            let was = brain.role;
            brain.mark = mark;
            brain.role = role;
            let mv = defend_move(&ctx, &me, prof, &mut brain, &mut reqs, &mut rng);
            // HUD cues: only for the opposition, only when the human is on the ball side.
            if p.side == Side::Away && ticker.age > 1.6 {
                if role == DefRole::Help && was != DefRole::Help {
                    ticker.line = "HELP D — CRANES ROTATE TO THE PAINT".into();
                    ticker.age = 0.0;
                } else if role == DefRole::OnBall
                    && brain.contest > 0.0
                    && ctx.set_shot
                    && flat_dist(me.pos, ctx.bpos) < 2.4
                {
                    ticker.line = "CONTESTED — HANDS IN YOUR FACE".into();
                    ticker.age = 0.0;
                }
            }
            mv
        };

        // Reaction lag: defenders act on a target refreshed every `def_lag` seconds.
        let dest = if !on_offense && !has_ball && ctx.hold != Hold::Shot && !ball_live_loose {
            if brain.retarget <= 0.0 || brain.target == Vec3::ZERO {
                brain.target = mv.dest;
                brain.retarget = prof.def_lag;
            }
            brain.target
        } else {
            brain.target = mv.dest;
            brain.retarget = 0.0;
            mv.dest
        };

        let to = dest - ground(tf.translation);
        let dist = to.length();
        let sprint = (mv.sprint || dist > 6.0) && stam.0 > 0.12;
        if dist > 0.3 {
            let n = to.normalize();
            let spd = crate::units::move_speed(ratings, sprint, stam.0) * prof.speed * mv.ease;
            // Ease into short hops so spots are held without jitter.
            let spd = spd.min(dist / dt * 0.5 + 0.5);
            vel.0 = n * spd;
            tf.translation += vel.0 * dt;
        } else {
            vel.0 *= 0.7;
        }
        let (x, z) = clamp_to_court(tf.translation.x, tf.translation.z, 0.55);
        tf.translation.x = x;
        tf.translation.z = z;
        tf.translation.y = 0.0;

        // Locomotion / contest pose (never overrides a shot, pass, block, …).
        if matches!(*pose, Pose::Idle | Pose::Run | Pose::Sprint | Pose::Contest) {
            let speed = ground(vel.0).length();
            let want = if brain.contest > 0.0 && !on_offense {
                Pose::Contest
            } else if speed > 6.4 {
                Pose::Sprint
            } else if speed > 0.6 {
                Pose::Run
            } else {
                Pose::Idle
            };
            if want == Pose::Contest {
                if *pose != Pose::Contest {
                    pclock.0 = 0.0;
                }
                pclock.0 = pclock.0.min(0.2);
            }
            if *pose != want {
                *pose = want;
            }
        }
    }
}

/// Man assignment for `def_side` against the handler `h`: closest defender takes
/// the ball, the other two are matched to the remaining attackers by total
/// distance with a 1 m hysteresis on the previous marks. Then help / zone roles
/// when the ball has beaten its man near the rim.
fn assign_defense(
    snaps: &[Snap],
    h: Entity,
    def_side: Side,
    prof: &AiProfile,
) -> Vec<(Entity, Option<Entity>, DefRole)> {
    let Some(handler) = snap_of(snaps, h) else {
        return Vec::new();
    };
    let hoop = Vec3::new(attack_hoop_x(handler.side), 0.0, 0.0);
    let defs: Vec<&Snap> = snaps.iter().filter(|s| s.side == def_side).collect();
    let atts: Vec<&Snap> = snaps
        .iter()
        .filter(|s| s.side != def_side && s.e != h)
        .collect();
    if defs.is_empty() {
        return Vec::new();
    }
    // On-ball: whoever is closest (the human counts, so AI teammates fill around him).
    let onball = defs
        .iter()
        .min_by(|a, b| {
            flat_dist(a.pos, handler.pos)
                .partial_cmp(&flat_dist(b.pos, handler.pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|s| s.e)
        .unwrap();
    let mut out: Vec<(Entity, Option<Entity>, DefRole)> = vec![(onball, Some(h), DefRole::OnBall)];
    let others: Vec<&Snap> = defs.iter().copied().filter(|s| s.e != onball).collect();
    match (others.len(), atts.len()) {
        (2, 2) => {
            let (d0, d1, a0, a1) = (others[0], others[1], atts[0], atts[1]);
            let c_straight = flat_dist(d0.pos, a0.pos) + flat_dist(d1.pos, a1.pos);
            let c_cross = flat_dist(d0.pos, a1.pos) + flat_dist(d1.pos, a0.pos);
            let prev_straight = d0.mark == Some(a0.e) && d1.mark == Some(a1.e);
            let prev_cross = d0.mark == Some(a1.e) && d1.mark == Some(a0.e);
            let straight = if prev_straight {
                c_cross + 1.0 >= c_straight
            } else if prev_cross {
                c_straight + 1.0 < c_cross
            } else {
                c_straight <= c_cross
            };
            if straight {
                out.push((d0.e, Some(a0.e), DefRole::Deny));
                out.push((d1.e, Some(a1.e), DefRole::Deny));
            } else {
                out.push((d0.e, Some(a1.e), DefRole::Deny));
                out.push((d1.e, Some(a0.e), DefRole::Deny));
            }
        }
        _ => {
            for (i, d) in others.iter().enumerate() {
                out.push((d.e, atts.get(i).map(|a| a.e), DefRole::Deny));
            }
        }
    }

    // Help: the ball is near the rim and the on-ball defender is not in front of it.
    let hd = flat_dist(handler.pos, hoop);
    let ob = snap_of(snaps, onball).unwrap();
    let to_hoop = ground(hoop - handler.pos).normalize_or_zero();
    let along = ground(ob.pos - handler.pos).dot(to_hoop);
    let beaten = along < -prof.help_beaten * 0.3 || flat_dist(ob.pos, handler.pos) > 1.6 + prof.help_beaten;
    if hd < prof.help_threshold && beaten && out.len() == 3 {
        let help_spot = handler.pos.lerp(hoop, 0.5);
        let (i_help, _) = out
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, (d, ..))| (i, flat_dist(snap_of(snaps, *d).unwrap().pos, help_spot)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();
        out[i_help].2 = DefRole::Help;
        for (i, o) in out.iter_mut().enumerate().skip(1) {
            if i != i_help {
                o.2 = DefRole::Zone;
            }
        }
    }
    out
}

fn defend_move(
    ctx: &Ctx,
    me: &Snap,
    prof: &AiProfile,
    brain: &mut AiBrain,
    reqs: &mut AiRequests,
    rng: &mut GameRng,
) -> Move {
    let snaps = ctx.snaps;
    let att_side = me.side.other();
    let hoop = Vec3::new(attack_hoop_x(att_side), 0.0, 0.0);
    // The ball itself stands in for the handler while a pass is in the air.
    let handler = ctx.holder.and_then(|h| snap_of(snaps, h));
    let hpos = handler.map(|h| ground(h.pos)).unwrap_or(ground(ctx.bpos));
    let hd = flat_dist(hpos, hoop);
    let to_hoop = ground(hoop - hpos).normalize_or_zero();

    // Jump a pass whose flight passes within reach.
    if ctx.hold == Hold::Pass && prof.lane_jump > 0.0 {
        let mut best: Option<(f32, Vec3)> = None;
        for i in 0..8 {
            let t = i as f32 * 0.06;
            let p = ground(ctx.bpos + ctx.bvel * t);
            let d = flat_dist(me.pos, p);
            if best.map(|(bd, _)| d < bd).unwrap_or(true) {
                best = Some((d, p));
            }
        }
        if let Some((d, p)) = best {
            if d < prof.lane_jump {
                return Move {
                    dest: p,
                    sprint: true,
                    ease: 1.0,
                };
            }
        }
    }

    match brain.role {
        DefRole::OnBall => {
            let d_me = flat_dist(me.pos, hpos);
            // Stance tightens as the handler gets closer to the paint.
            let tight = ((hd - 2.0) / 6.0).clamp(0.0, 1.0);
            let mut stand = prof.pressure_dist * (0.55 + 0.45 * tight);
            stand += prof.sag * (hd - THREE_RADIUS).max(0.0);
            let mut sprint = d_me > 3.0;
            if let Some(handler) = handler {
                // Closeout: he is setting up to shoot → sprint in, hands up.
                if ctx.set_shot && d_me < prof.closeout_range {
                    stand = prof.closeout_dist;
                    sprint = true;
                    brain.contest = 0.3;
                } else if hd < 8.5 && ground(handler.vel).length() < 0.8 && d_me < prof.closeout_range
                {
                    // Standing in range: crowd him without leaving the feet.
                    stand = stand.min(prof.closeout_dist + 0.25);
                }
                // Reach-in lunge.
                if brain.lunge_t > 0.0 {
                    brain.lunge_t -= ctx.dt;
                    if d_me < STEAL_REACH_MAX * 0.85 {
                        reqs.steals.push(me.e);
                        brain.lunge_t = 0.0;
                    } else {
                        return Move {
                            dest: hpos,
                            sprint: true,
                            ease: 1.0,
                        };
                    }
                } else if brain.steal_cd <= 0.0
                    && d_me < 2.0
                    && !ctx.set_shot
                    && steal_chance(me.r.steal, handler.r.handle, STEAL_REACH_MAX * 0.6) > 0.1
                    && rng.f32() < prof.steal_rate * ctx.dt
                {
                    brain.lunge_t = 0.35;
                    brain.steal_cd = prof.steal_cooldown;
                }
            }
            Move {
                dest: hpos + to_hoop * stand,
                sprint,
                ease: 1.0,
            }
        }
        DefRole::Help => {
            let spot = hpos.lerp(hoop, 0.5);
            let spot = if flat_dist(spot, hoop) < 1.2 {
                hoop + (spot - hoop).normalize_or_zero() * 1.2
            } else {
                spot
            };
            if ctx.set_shot && flat_dist(me.pos, hpos) < 2.4 {
                brain.contest = 0.3;
            }
            Move {
                dest: spot,
                sprint: true,
                ease: 1.0,
            }
        }
        DefRole::Zone => {
            let others: Vec<Vec3> = snaps
                .iter()
                .filter(|s| s.side == att_side && Some(s.e) != ctx.holder)
                .map(|s| ground(s.pos))
                .collect();
            let mid = if others.is_empty() {
                hoop
            } else {
                others.iter().sum::<Vec3>() / others.len() as f32
            };
            Move {
                dest: mid.lerp(hoop, 0.3),
                sprint: false,
                ease: 1.0,
            }
        }
        DefRole::Deny | DefRole::None => {
            let Some(man) = brain.mark.and_then(|m| snap_of(snaps, m)) else {
                return Move {
                    dest: hoop.lerp(hpos, 0.4),
                    sprint: false,
                    ease: 1.0,
                };
            };
            let man_pos = ground(man.pos);
            let to_ball = ground(ctx.bpos - man.pos);
            let ball_d = to_ball.length().max(0.1);
            let toward_ball = to_ball / ball_d;
            let toward_hoop = ground(hoop - man.pos).normalize_or_zero();
            let mut dest = man_pos + toward_ball * (prof.deny_t * ball_d.min(3.0)) + toward_hoop * prof.deny_sag;
            // Weak side far from the ball: drop toward the paint (help side).
            if ball_d > 7.0 {
                dest = dest.lerp(hoop + (man_pos - hoop).normalize_or_zero() * 3.2, 0.4);
            }
            // Cutter heading to the rim: stay attached.
            let man_speed = ground(man.vel).length();
            if man_speed > 3.0 && ground(man.vel).dot(toward_hoop) > 0.5 {
                dest = man_pos + ground(man.vel).normalize_or_zero() * 0.6 + toward_hoop * 0.5;
            }
            let sprint = flat_dist(me.pos, dest) > 2.2 || man_speed > 5.0;
            Move {
                dest,
                sprint,
                ease: 1.0,
            }
        }
    }
}

/// Ball in the air: contest / block if it just left the hand near me, otherwise
/// crash the glass (bigs) or hold a spot.
fn shot_in_air_move(
    ctx: &Ctx,
    me: &Snap,
    prof: &AiProfile,
    brain: &mut AiBrain,
    reqs: &mut AiRequests,
    rng: &mut GameRng,
) -> Move {
    let defending = ctx.shooter_side.map(|s| s != me.side).unwrap_or(false);
    let d_ball = flat_dist(me.pos, ctx.bpos);
    if defending
        && d_ball < 2.0
        && ctx.bpos.y > 1.3
        && ctx.bpos.y < 3.1
        && ctx.bvel.y > 0.0
        && brain.block_cd <= 0.0
    {
        brain.block_cd = 1.2;
        if rng.f32() < prof.block_aggr {
            reqs.blocks.push(me.e);
        }
    }
    let hoop_x = if ctx.bvel.x > 0.0 { HOOP_X } else { -HOOP_X };
    let hoop = Vec3::new(hoop_x, 0.0, 0.0);
    let ring = hoop + ground(me.pos - hoop).normalize_or_zero() * 1.6;
    let crash = me.r.rebound > 60.0 || flat_dist(me.pos, hoop) < 4.5;
    Move {
        dest: if crash { ring } else { ground(me.pos) },
        sprint: false,
        ease: 0.9,
    }
}

fn handler_move(ctx: &Ctx, me: &Snap, brain: &mut AiBrain) -> Move {
    let hoop = Vec3::new(attack_hoop_x(me.side), 0.0, 0.0);
    let dir = attack_dir(me.side);
    let hd = flat_dist(me.pos, hoop);
    match brain.plan {
        Plan::Shoot => Move {
            dest: ground(me.pos),
            sprint: false,
            ease: 0.0,
        },
        Plan::Drive(side) => {
            // Around the defender, then finish at the front of the rim.
            let gate = hoop + Vec3::new(-dir * 3.2, 0.0, side * 1.6);
            let finish = hoop + Vec3::new(-dir * 0.9, 0.0, side * 0.5);
            let dest = if hd > 3.6 && flat_dist(me.pos, gate) > 0.8 {
                gate
            } else {
                finish
            };
            Move {
                dest,
                sprint: true,
                ease: 1.0,
            }
        }
        Plan::Juke(side) => {
            brain.juke_t -= ctx.dt;
            if brain.juke_t <= 0.0 {
                brain.plan = Plan::Drive(side);
            }
            Move {
                dest: ground(me.pos) + Vec3::new(0.0, 0.0, side * 2.0),
                sprint: true,
                ease: 1.0,
            }
        }
        Plan::Probe => {
            // Bring the ball up to the top of the arc, then hold with a slow drift in.
            if hd > 8.8 {
                let z = if me.pos.z.abs() < 0.5 { 0.0 } else { me.pos.z * 0.5 };
                Move {
                    dest: hoop + Vec3::new(-dir * 7.8, 0.0, z),
                    sprint: ctx.shot_clock < 14.0,
                    ease: 1.0,
                }
            } else {
                let to_hoop = ground(hoop - me.pos).normalize_or_zero();
                Move {
                    dest: ground(me.pos) + to_hoop * 0.5,
                    sprint: false,
                    ease: 0.5,
                }
            }
        }
    }
}

fn offball_move(
    ctx: &Ctx,
    me: &Snap,
    prof: &AiProfile,
    brain: &mut AiBrain,
    rng: &mut GameRng,
) -> Move {
    let snaps = ctx.snaps;
    let hoop_x = attack_hoop_x(me.side);
    let hoop = Vec3::new(hoop_x, 0.0, 0.0);
    let dir = attack_dir(me.side);
    let corner = |s: f32| Vec3::new(hoop_x - dir * 1.0, 0.0, s * 6.3);
    let wing = |s: f32| Vec3::new(hoop_x - dir * 5.4, 0.0, s * 4.7);
    let rim = |s: f32| Vec3::new(hoop_x - dir * 1.5, 0.0, s * 0.7);

    let handler = ctx.holder.and_then(|h| snap_of(snaps, h));
    let mates: Vec<&Snap> = snaps
        .iter()
        .filter(|s| s.side == me.side && Some(s.e) != ctx.holder)
        .collect();
    let my_idx = mates.iter().position(|s| s.e == me.e).unwrap_or(0);
    let my_def_d = nearest_opp_dist(snaps, me.side, me.pos);

    // Screen-and-roll timers.
    if brain.screen_t > 0.0 {
        brain.screen_t -= ctx.dt;
        if brain.screen_t <= 0.0 {
            brain.roll_t = 1.3;
        }
        if let Some(h) = handler {
            // Body up against the on-ball defender, on the side the handler will use.
            let ob = snaps
                .iter()
                .filter(|s| s.side != me.side)
                .min_by(|a, b| {
                    flat_dist(a.pos, h.pos)
                        .partial_cmp(&flat_dist(b.pos, h.pos))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
            if let Some(ob) = ob {
                let to_h = ground(h.pos - ob.pos).normalize_or_zero();
                let perp = Vec3::new(-to_h.z, 0.0, to_h.x);
                let side = sign_or(-h.pos.z, 1.0);
                let dest = ground(ob.pos) + perp * (0.8 * side) - to_h * 0.2;
                return Move {
                    dest,
                    sprint: true,
                    ease: 1.0,
                };
            }
        }
    }
    if brain.roll_t > 0.0 {
        brain.roll_t -= ctx.dt;
        return Move {
            dest: rim(sign_or(me.pos.z, 1.0)),
            sprint: true,
            ease: 1.0,
        };
    }

    // Backdoor cut when my man is asleep or has left to help.
    if brain.cut_t > 0.0 {
        brain.cut_t -= ctx.dt;
        return Move {
            dest: rim(sign_or(me.pos.z, 1.0) * 0.8),
            sprint: true,
            ease: 1.0,
        };
    }
    if let Some(h) = handler {
        let h_driving = flat_dist(h.pos, hoop) < 4.5;
        let my_def_helping = snaps
            .iter()
            .filter(|s| s.side != me.side)
            .min_by(|a, b| {
                flat_dist(a.pos, me.pos)
                    .partial_cmp(&flat_dist(b.pos, me.pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|d| flat_dist(d.pos, h.pos) + 1.2 < flat_dist(d.pos, me.pos))
            .unwrap_or(false);
        let rim_crowd = snaps
            .iter()
            .filter(|s| s.side != me.side && flat_dist(s.pos, hoop) < 3.0)
            .count();
        if brain.cut_cd <= 0.0
            && !h_driving
            && rim_crowd <= 1
            && flat_dist(me.pos, hoop) > 4.0
            && (my_def_d > prof.cut_gap || my_def_helping)
        {
            brain.cut_t = 1.4;
            brain.cut_cd = 4.0;
            return Move {
                dest: rim(sign_or(me.pos.z, 1.0) * 0.8),
                sprint: true,
                ease: 1.0,
            };
        }
        // Screen: the stronger mate, while the handler probes at the arc.
        let strongest = mates
            .iter()
            .max_by(|a, b| {
                a.r.strength
                    .partial_cmp(&b.r.strength)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|s| s.e);
        if strongest == Some(me.e)
            && brain.screen_cd <= 0.0
            && prof.screen_rate > 0.0
            && !h.ctrl
            && !h.winding_up
            && ctx.shot_clock > 9.0
            && flat_dist(h.pos, hoop) > 5.5
            && flat_dist(h.pos, hoop) < 9.0
            && rng.f32() < prof.screen_rate * ctx.dt
        {
            brain.screen_t = 1.3;
            brain.screen_cd = 7.0;
        }
    }

    // Spacing: weak-side wing + strong-side corner; both corners when the ball drives.
    let strong = handler.map(|h| sign_or(h.pos.z, 1.0)).unwrap_or(1.0);
    let h_inside = handler
        .map(|h| flat_dist(h.pos, hoop) < 4.5)
        .unwrap_or(false);
    let dest = if h_inside {
        if my_idx == 0 {
            corner(-strong)
        } else {
            corner(strong)
        }
    } else if my_idx == 0 {
        wing(-strong)
    } else {
        corner(strong)
    };
    Move {
        dest,
        sprint: flat_dist(me.pos, dest) > 5.0,
        ease: 1.0,
    }
}

// ---------------------------------------------------------------------------
// Ball decisions
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn ai_decisions(
    time: Res<Time<Fixed>>,
    paused: Res<Paused>,
    config: Res<MatchConfig>,
    profiles: Res<AiProfiles>,
    mut rng: ResMut<GameRng>,
    control: Res<LiveControl>,
    clock: Res<MatchClock>,
    mut last_pass: ResMut<LastPass>,
    mut ticker: ResMut<Ticker>,
    mut ball_q: Query<
        (&mut Transform, &mut BallVel, &mut BallSpin, &mut BallState),
        (With<Ball>, Without<Player>),
    >,
    mut players: Query<
        (
            Entity,
            &Player,
            &Ratings,
            &Transform,
            &MoveVel,
            &mut Pose,
            &mut PoseClock,
            &Stamina,
            &mut AiBrain,
            &Heat,
            &mut BoxLine,
        ),
        Without<Ball>,
    >,
) {
    if paused.0 || config.mode == GameMode::Practice {
        return;
    }
    let dt = time.delta_secs();
    let Ok((mut btf, mut bvel, mut spin, mut st)) = ball_q.single_mut() else {
        return;
    };
    if st.hold != Hold::Held {
        return;
    }
    let Some(holder) = st.holder else {
        return;
    };
    if control.entity == Some(holder) {
        return;
    }

    let mut snaps: Vec<Snap> = players
        .iter()
        .map(|(e, p, r, t, v, pose, _, _, brain, _, _)| Snap {
            e,
            side: p.side,
            slot: p.slot,
            pos: t.translation,
            vel: v.0,
            r: *r,
            pose: *pose,
            ctrl: control.entity == Some(e),
            mark: brain.mark,
            winding_up: brain.plan == Plan::Shoot,
            hands_up: matches!(*pose, Pose::Block | Pose::Contest),
        })
        .collect();
    snaps.sort_by_key(|s| (s.side == Side::Away, s.slot));
    let Some(me) = snap_of(&snaps, holder).copied() else {
        return;
    };
    let Ok((_, _, _, _, _, _, _, stam, mut brain, heat, _)) = players.get_mut(holder) else {
        return;
    };
    let stam = stam.0;
    let streak = heat.streak;
    let prof = *profiles.for_side(me.side);
    brain.think += dt;
    brain.hold_t += dt;

    let hoop_x = attack_hoop_x(me.side);
    let hoop_g = Vec3::new(hoop_x, 0.0, 0.0);
    let dist = flat_dist(me.pos, hoop_g);
    let paint = in_paint(me.pos.x, me.pos.z, hoop_x);
    let (shot_ev_now, shot_chance, is_three) = shot_value(&snaps, &me, stam, streak, &prof, true);

    // Release a wound-up jumper; the contest *now* decides the roll.
    if brain.plan == Plan::Shoot {
        brain.windup -= dt;
        if brain.windup > 0.0 {
            return;
        }
        brain.plan = Plan::Probe;
        brain.think = 0.0;
        drop(brain);
        release_shot(
            &me,
            shot_chance,
            is_three,
            false,
            &mut rng,
            &mut btf,
            &mut bvel,
            &mut spin,
            &mut st,
            &mut players,
            &mut ticker,
        );
        return;
    }

    if brain.think < prof.reaction {
        return;
    }
    brain.think = 0.0;

    // --- options -----------------------------------------------------------
    let my_def = snaps
        .iter()
        .filter(|s| s.side != me.side)
        .min_by(|a, b| {
            flat_dist(a.pos, me.pos)
                .partial_cmp(&flat_dist(b.pos, me.pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .copied();
    let def_gap = my_def.map(|d| flat_dist(d.pos, me.pos)).unwrap_or(9.0);
    let to_hoop = ground(hoop_g - me.pos).normalize_or_zero();
    let def_in_front = my_def
        .map(|d| ground(d.pos - me.pos).normalize_or_zero().dot(to_hoop) > 0.35 && def_gap < 2.6)
        .unwrap_or(false);
    let helpers_near = snaps
        .iter()
        .filter(|s| s.side != me.side && Some(s.e) != my_def.map(|d| d.e))
        .filter(|s| flat_dist(s.pos, me.pos) < prof.kick_dist)
        .count();
    let rim_crowd = snaps
        .iter()
        .filter(|s| s.side != me.side && flat_dist(s.pos, hoop_g) < 3.2)
        .count();

    // Best pass: receiver's catch-and-shoot value, discounted by lane risk and openness.
    let mut best_pass: Option<(Entity, f32)> = None;
    for mate in snaps.iter().filter(|s| s.side == me.side && s.e != me.e) {
        if matches!(
            mate.pose,
            Pose::Shoot | Pose::Dunk | Pose::Pass | Pose::Stumble | Pose::Celebrate | Pose::Block
        ) {
            continue;
        }
        let (ev, _, _) = shot_value(&snaps, mate, 0.9, 0, &prof, false);
        let open = nearest_opp_dist(&snaps, me.side, mate.pos);
        let risk = lane_risk(&snaps, me.side, me.pos, mate.pos, 1.1);
        let cutting = flat_dist(mate.pos, hoop_g) < 3.5
            && ground(mate.vel).dot(ground(hoop_g - mate.pos).normalize_or_zero()) > 2.0;
        let mut score = ev * (1.0 - 0.7 * risk) + prof.pass_open_w * (open - 1.5).clamp(-1.0, 1.5) * 0.4;
        if cutting && open > 1.2 {
            score += 0.45;
        }
        if open < 1.0 {
            score -= 0.5;
        }
        if best_pass.map(|(_, s)| score > s).unwrap_or(true) {
            best_pass = Some((mate.e, score));
        }
    }
    let pass_ev = best_pass.map(|(_, s)| s).unwrap_or(0.0);

    let drive_ev = {
        let lane_open = !def_in_front || def_gap > prof.drive_gap;
        let base = 1.35 * (0.55 + 0.45 * me.r.speed / 100.0) * (0.7 + 0.3 * me.r.handle / 100.0);
        let crowd = 1.0 - 0.28 * rim_crowd as f32;
        if lane_open && dist > 3.2 {
            base * crowd.max(0.2)
        } else {
            0.35 * crowd.max(0.2)
        }
    };

    let late = clock.shot < prof.late_clock;
    let bar = if late {
        prof.shot_ev_min * (clock.shot / prof.late_clock).max(0.0)
    } else {
        prof.shot_ev_min
    };
    let contest_now = contest_on(&snaps, me.e, me.side, me.pos);
    // A rim protector with his hands up turns a dunk into a coin flip, so the
    // handler only goes up through light traffic (or when the clock says so).
    let dunk_chance = (0.86 * prof.skill * (1.0 - 0.5 * contest_now)).min(0.96);
    let can_dunk = paint
        && dist < 2.8
        && me.r.dunk > 72.0
        && ground(me.vel).length() > 2.0
        && (contest_now < 0.6 || late || rim_crowd == 0);
    let layup_ok = paint && dist < 2.8 && (contest_now < 0.55 || late || !def_in_front);

    enum Choice {
        Shoot,
        Dunk,
        Pass(Entity),
        Drive(f32),
        Juke(f32),
        Probe,
    }
    let drive_side = my_def
        .map(|d| -sign_or(d.pos.z - me.pos.z, 1.0))
        .unwrap_or(1.0);
    let choice = if clock.shot < 1.2 {
        if dist < 2.8 && me.r.dunk > 72.0 {
            Choice::Dunk
        } else {
            Choice::Shoot
        }
    } else if can_dunk {
        Choice::Dunk
    } else if layup_ok {
        Choice::Shoot
    } else if matches!(brain.plan, Plan::Drive(_))
        && helpers_near > 0
        && best_pass.is_some()
        && pass_ev > shot_ev_now * 0.9
    {
        // Drive-and-kick: help came, the ball goes out.
        Choice::Pass(best_pass.unwrap().0)
    } else if matches!(brain.plan, Plan::Drive(_)) && dist > 3.2 && brain.hold_t < 3.5 {
        Choice::Drive(drive_side)
    } else if shot_ev_now >= bar && shot_ev_now + 0.1 >= pass_ev && dist < 9.8 {
        Choice::Shoot
    } else if best_pass.is_some() && pass_ev > shot_ev_now + 0.12 && pass_ev > 0.9 && brain.hold_t > 0.3
    {
        Choice::Pass(best_pass.unwrap().0)
    } else if drive_ev > shot_ev_now && drive_ev > pass_ev - 0.1 && dist > 3.2 && dist < 10.0 {
        Choice::Drive(drive_side)
    } else if def_gap < 1.2 && dist < 9.5 && rng.f32() < prof.juke_rate {
        Choice::Juke(drive_side)
    } else if brain.hold_t > 4.5 && best_pass.is_some() && pass_ev > 0.6 {
        Choice::Pass(best_pass.unwrap().0)
    } else if brain.hold_t > 5.5 {
        Choice::Drive(drive_side)
    } else if late && clock.shot < 3.0 {
        Choice::Shoot
    } else {
        Choice::Probe
    };

    match choice {
        Choice::Shoot => {
            brain.plan = Plan::Shoot;
            brain.windup = if clock.shot < 1.5 {
                0.02
            } else if dist < 2.8 {
                prof.windup * 0.4
            } else {
                prof.windup
            };
        }
        Choice::Dunk => {
            brain.plan = Plan::Probe;
            drop(brain);
            release_shot(
                &me,
                dunk_chance,
                false,
                true,
                &mut rng,
                &mut btf,
                &mut bvel,
                &mut spin,
                &mut st,
                &mut players,
                &mut ticker,
            );
        }
        Choice::Drive(side) => brain.plan = Plan::Drive(side),
        Choice::Juke(side) => {
            brain.plan = Plan::Juke(side);
            brain.juke_t = 0.3;
        }
        Choice::Probe => brain.plan = Plan::Probe,
        Choice::Pass(mate_e) => {
            brain.plan = Plan::Probe;
            drop(brain);
            let Some(mate) = snap_of(&snaps, mate_e) else {
                return;
            };
            let from = Vec3::new(me.pos.x, 1.4, me.pos.z);
            let d = flat_dist(me.pos, mate.pos);
            let flight = (d / 13.0).clamp(0.2, 0.5);
            let dest = mate.pos + Vec3::Y * 1.4 + ground(mate.vel) * flight * 0.85;
            btf.translation = from;
            let v = ballistic_velocity(
                from.to_array(),
                [dest.x, dest.y, dest.z],
                flight,
                GRAVITY * 0.28,
            );
            bvel.0 = Vec3::new(v[0], v[1], v[2]);
            spin.0 = Vec3::new(0.0, 18.0, 0.0);
            st.hold = Hold::Pass;
            st.holder = None;
            st.last_touch = Some(me.e);
            st.last_passer = Some(me.e);
            last_pass.passer = Some(me.e);
            last_pass.age = 0.0;
            if let Ok((_, _, _, _, _, mut pose, mut pclock, _, _, _, _)) = players.get_mut(me.e) {
                *pose = Pose::Pass;
                pclock.0 = 0.0;
            }
        }
    }
}

/// Launch an AI shot from the hands with the human path's release heights and
/// flight times. `chance` already includes contest, skill and heat.
#[allow(clippy::too_many_arguments)]
fn release_shot(
    me: &Snap,
    chance: f32,
    is_three: bool,
    dunk: bool,
    rng: &mut GameRng,
    btf: &mut Transform,
    bvel: &mut BallVel,
    spin: &mut BallSpin,
    st: &mut BallState,
    players: &mut Query<
        (
            Entity,
            &Player,
            &Ratings,
            &Transform,
            &MoveVel,
            &mut Pose,
            &mut PoseClock,
            &Stamina,
            &mut AiBrain,
            &Heat,
            &mut BoxLine,
        ),
        Without<Ball>,
    >,
    ticker: &mut Ticker,
) {
    let hoop_x = attack_hoop_x(me.side);
    let hoop = Vec3::new(hoop_x, RIM_HEIGHT, 0.0);
    let dist = flat_dist(me.pos, Vec3::new(hoop_x, 0.0, 0.0));
    let paint = in_paint(me.pos.x, me.pos.z, hoop_x);
    let to_hoop = (hoop - me.pos).normalize_or_zero();
    let driving = me.vel.dot(to_hoop) > 2.2;
    let moving_away = me.vel.dot(to_hoop) < -1.5;
    let lateral = me.vel.cross(to_hoop).y.abs() > 2.0;
    let speed = me.vel.length();
    let shot = if dunk {
        ShotType::Dunk
    } else {
        let s = classify_shot(
            dist,
            paint,
            driving,
            moving_away,
            lateral,
            me.r.dunk,
            me.r.mid,
            false,
            speed,
        );
        if s == ShotType::Dunk {
            ShotType::Layup
        } else {
            s
        }
    };

    let mut target = hoop;
    let made = rng.f32() <= chance;
    if dunk {
        // A stuffed dunk clanks off the front iron toward the shooter.
        if !made {
            target += ground(me.pos - hoop).normalize_or_zero() * 0.32 + Vec3::Y * 0.05;
        }
    } else if !made {
        target += Vec3::new(
            rng.range(-0.55, 0.55),
            rng.range(0.05, 0.45),
            rng.range(-0.55, 0.55),
        );
    } else {
        target += Vec3::new(rng.range(-0.04, 0.04), 0.02, rng.range(-0.04, 0.04));
    }
    let (height, flight) = if dunk {
        (1.8, 0.55)
    } else {
        (
            release_height(shot),
            match shot {
                ShotType::Layup | ShotType::ReverseLayup | ShotType::FingerRoll => 0.42,
                ShotType::Underhand => 0.55,
                ShotType::Floater => 0.62,
                ShotType::Hook => 0.7,
                ShotType::LogoHeave => (flight_time_for_distance(dist) * 1.05).min(1.6),
                _ => flight_time_for_distance(dist),
            },
        )
    };
    let from = Vec3::new(me.pos.x, height, me.pos.z);
    btf.translation = from;
    let aim = if dunk && made {
        [hoop.x, hoop.y + 0.12, hoop.z]
    } else {
        [target.x, target.y, target.z]
    };
    let v = ballistic_velocity(from.to_array(), aim, flight, GRAVITY);
    bvel.0 = Vec3::new(v[0], v[1], v[2]) + me.vel * if dunk { 0.15 } else { 0.12 };
    spin.0 = Vec3::from_array(release_spin(
        shot,
        1.0,
        hoop.x - me.pos.x,
        hoop.z - me.pos.z,
    ));
    st.hold = Hold::Shot;
    st.holder = None;
    st.shooter = Some(me.e);
    st.rim_hits = 0;
    st.release_was_three = is_three && !dunk;
    if let Ok((_, _, _, _, _, mut pose, mut pclock, _, _, _, mut boxl)) = players.get_mut(me.e) {
        *pose = if dunk { Pose::Dunk } else { Pose::Shoot };
        pclock.0 = 0.0;
        boxl.fg_att += 1;
    }
    if ticker.age > 1.2 {
        ticker.line = shot.label().into();
        ticker.age = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_array_roundtrip_and_bounds() {
        for p in [ROOKIE, PRO, LEGEND] {
            let back = AiProfile::from_array(p.to_array());
            assert_eq!(back, p);
        }
        for p in [PRO, LEGEND] {
            assert_eq!(p.clamped(), p, "tuned preset must sit inside the search box");
        }
        assert!(LEGEND.reaction < PRO.reaction && PRO.reaction < ROOKIE.reaction);
        assert!(LEGEND.skill > PRO.skill && PRO.skill > ROOKIE.skill);
    }

    #[test]
    fn pass_lane_risk_sees_a_defender_in_the_lane() {
        let a = Vec3::new(0.0, 0.0, 0.0);
        let b = Vec3::new(6.0, 0.0, 0.0);
        assert!(seg_dist(Vec3::new(3.0, 0.0, 0.4), a, b) < 0.5);
        assert!(seg_dist(Vec3::new(3.0, 0.0, 3.0), a, b) > 2.5);
        assert!(seg_dist(Vec3::new(-2.0, 0.0, 0.0), a, b) > 1.9);
    }
}
