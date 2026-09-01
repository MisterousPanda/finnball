use bevy::prelude::*;

use crate::ball::{Ball, BallSpin, BallState, BallVel, BucketEvent, Hold, spawn_ball};
use crate::camera::GameCam;
use crate::roster::Side;
use crate::sim::{
    BALL_RADIUS, GRAVITY, HOOP_X, PAINT_DEPTH, RIM_HEIGHT, PassKind, ShotType, ballistic_velocity,
    clamp_to_court, classify_shot, contest_factor, dribble_cadence, flight_time_for_distance,
    in_paint, meter_accuracy, release_height, release_spin, shot_kind, shot_make_chance,
    steal_chance,
};
use crate::states::{AppState, GameMode, MatchConfig, Paused};
use crate::units::{BoxLine, MoveVel, Player, Pose, PoseClock, Ratings, Stamina, spawn_player};

pub struct GameplayPlugin;

impl Plugin for GameplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MatchClock>()
            .init_resource::<Scoreboard>()
            .init_resource::<ShotMeter>()
            .init_resource::<Ticker>()
            .init_resource::<GameRng>()
            .init_resource::<LiveControl>()
            .init_resource::<LastPass>()
            .add_message::<BucketEvent>()
            .add_message::<PlayCall>()
            .add_message::<DribbleTickEvent>()
            .add_message::<StealEvent>()
            .add_message::<ViolationEvent>()
            .add_message::<TipWhistle>()
            .add_systems(OnEnter(AppState::Playing), start_match)
            .add_systems(
                Update,
                (
                    tick_clock,
                    follow_dribble,
                    handle_buckets,
                    inbound_after_score,
                    tick_meter_freeze,
                    pose_timeouts,
                    maybe_end_game,
                )
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(
                FixedUpdate,
                (
                    apply_intents,
                    pickup_loose_ball,
                    shoot_and_pass,
                    steal_attempts,
                    block_attempts,
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[derive(Resource)]
pub struct MatchClock {
    pub quarter: u8,
    pub remaining: f32,
    pub shot: f32,
    pub running: bool,
}

impl Default for MatchClock {
    fn default() -> Self {
        Self {
            quarter: 1,
            remaining: 60.0,
            shot: 24.0,
            running: true,
        }
    }
}

#[derive(Resource, Default)]
pub struct Scoreboard {
    pub home: u32,
    pub away: u32,
}

#[derive(Resource, Default)]
pub struct ShotMeter {
    pub armed: bool,
    pub value: f32,
    pub rising: bool,
    pub freeze: f32,
}

#[derive(Resource, Default)]
pub struct Ticker {
    pub line: String,
    pub age: f32,
}

#[derive(Resource)]
pub struct GameRng(pub u64);

impl Default for GameRng {
    fn default() -> Self {
        Self(0x9E37_79B9_7F4A_7C15)
    }
}

impl GameRng {
    pub fn f32(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(0x5851F42D4C957F2D).wrapping_add(1);
        ((self.0 >> 40) as u32) as f32 / (u32::MAX as f32)
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f32()
    }
}

#[derive(Resource, Default)]
pub struct LiveControl {
    pub entity: Option<Entity>,
}

#[derive(Resource, Default)]
pub struct PlayerIntent {
    pub move_xz: Vec2,
    pub sprint: bool,
    pub shoot_held: bool,
    pub shoot_released: bool,
    pub pass: bool,
    pub steal: bool,
    pub special: bool,
    pub switch: bool,
    pub block: bool,
    pub pass_kind: PassKind,
}

#[derive(Message, Clone)]
pub struct PlayCall {
    pub text: String,
}

#[derive(Message, Clone, Copy)]
pub struct DribbleTickEvent {
    pub pos: Vec3,
}

#[derive(Message, Clone, Copy)]
pub struct StealEvent {
    pub success: bool,
    pub pos: Vec3,
}

#[derive(Message, Clone, Copy)]
pub struct ViolationEvent;

#[derive(Message, Clone, Copy)]
pub struct TipWhistle;

#[derive(Resource, Default)]
pub struct LastPass {
    pub passer: Option<Entity>,
    pub age: f32,
}

#[derive(Component)]
pub struct AiBrain {
    pub think: f32,
}

fn start_match(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<MatchConfig>,
    mut clock: ResMut<MatchClock>,
    mut score: ResMut<Scoreboard>,
    mut control: ResMut<LiveControl>,
    mut ticker: ResMut<Ticker>,
    mut paused: ResMut<Paused>,
    mut meter: ResMut<ShotMeter>,
    mut whistle: MessageWriter<TipWhistle>,
) {
    paused.0 = false;
    meter.armed = false;
    score.home = 0;
    score.away = 0;
    clock.quarter = 1;
    clock.remaining = if config.mode == GameMode::Practice {
        999.0
    } else {
        config.quarter_secs
    };
    clock.shot = config.shot_clock;
    clock.running = true;
    ticker.line = "TIP-OFF — HOME HAS THE ROCK".into();
    ticker.age = 0.0;
    commands.insert_resource(LastPass::default());
    whistle.write(TipWhistle);

    let home_spots = [
        Vec3::new(-3.5, 0.0, 0.0),
        Vec3::new(-5.5, 0.0, 3.2),
        Vec3::new(-5.5, 0.0, -3.2),
    ];
    let away_spots = [
        Vec3::new(3.5, 0.0, 0.0),
        Vec3::new(5.5, 0.0, 3.2),
        Vec3::new(5.5, 0.0, -3.2),
    ];

    let mut first_human = None;
    for (i, id) in config.home.iter().enumerate() {
        let human = i == 0;
        let e = spawn_player(
            &mut commands,
            &mut meshes,
            &mut materials,
            *id,
            Side::Home,
            i as u8,
            human,
            home_spots[i],
        );
        commands.entity(e).insert(AiBrain { think: i as f32 * 0.2 });
        if human {
            first_human = Some(e);
        }
    }
    if config.mode != GameMode::Practice {
        for (i, id) in config.away.iter().enumerate() {
            let e = spawn_player(
                &mut commands,
                &mut meshes,
                &mut materials,
                *id,
                Side::Away,
                i as u8,
                false,
                away_spots[i],
            );
            commands.entity(e).insert(AiBrain { think: 0.4 + i as f32 * 0.15 });
        }
    }
    control.entity = first_human;
    let ball = spawn_ball(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::new(-3.1, 1.1, 0.0),
    );
    if let Some(holder) = first_human {
        commands.entity(ball).insert(crate::ball::BallState {
            hold: Hold::Held,
            holder: Some(holder),
            shooter: None,
            last_touch: Some(holder),
            last_passer: None,
            dribble_phase: 0.0,
            rim_hits: 0,
            release_was_three: false,
        });
    }
    commands.insert_resource(PlayerIntent::default());
}

fn tick_clock(
    time: Res<Time>,
    paused: Res<Paused>,
    config: Res<MatchConfig>,
    mut clock: ResMut<MatchClock>,
    mut ticker: ResMut<Ticker>,
    mut last_pass: ResMut<LastPass>,
    mut viol: MessageWriter<ViolationEvent>,
    mut next: ResMut<NextState<AppState>>,
    mut score: ResMut<Scoreboard>,
    mut ball: Query<(&mut Transform, &mut BallVel, &mut BallState), (With<Ball>, Without<Player>)>,
    mut players: Query<(Entity, &Player, &mut Transform), Without<Ball>>,
) {
    ticker.age += time.delta_secs();
    last_pass.age += time.delta_secs();
    if paused.0 || config.mode == GameMode::Practice {
        return;
    }
    if !clock.running {
        return;
    }
    clock.remaining -= time.delta_secs();
    clock.shot -= time.delta_secs();
    if clock.shot <= 0.0 {
        clock.shot = config.shot_clock;
        ticker.line = "SHOT CLOCK VIOLATION".into();
        ticker.age = 0.0;
        viol.write(ViolationEvent);
        let give_home = ball
            .single()
            .ok()
            .and_then(|(_, _, st)| st.last_touch)
            .and_then(|e| players.get(e).ok().map(|(_, p, _)| p.side != Side::Home))
            .unwrap_or(true);
        reset_to_half(&mut ball, &mut players, give_home);
    }
    if clock.remaining <= 0.0 {
        if clock.quarter >= 4 {
            if score.home == score.away {
                clock.quarter += 1;
                clock.remaining = 30.0;
                ticker.line = "OVERTIME — FIRST BUCKET WINS THE ROOM".into();
                ticker.age = 0.0;
            } else {
                next.set(AppState::GameOver);
            }
        } else {
            clock.quarter += 1;
            clock.remaining = config.quarter_secs;
            clock.shot = config.shot_clock;
            ticker.line = format!("END OF Q{} — NEXT QUARTER", clock.quarter - 1);
            ticker.age = 0.0;
            reset_to_half(&mut ball, &mut players, true);
        }
    }
    let _ = &mut score;
}

fn reset_to_half(
    ball: &mut Query<(&mut Transform, &mut BallVel, &mut BallState), (With<Ball>, Without<Player>)>,
    players: &mut Query<(Entity, &Player, &mut Transform), Without<Ball>>,
    give_home: bool,
) {
    let mut home_holder = None;
    let mut away_holder = None;
    for (e, p, mut tf) in players.iter_mut() {
        let spots = if p.side == Side::Home {
            [
                Vec3::new(-3.5, 0.0, 0.0),
                Vec3::new(-5.5, 0.0, 3.2),
                Vec3::new(-5.5, 0.0, -3.2),
            ]
        } else {
            [
                Vec3::new(3.5, 0.0, 0.0),
                Vec3::new(5.5, 0.0, 3.2),
                Vec3::new(5.5, 0.0, -3.2),
            ]
        };
        tf.translation = spots[p.slot.min(2) as usize];
        if p.slot == 0 && p.side == Side::Home {
            home_holder = Some(e);
        }
        if p.slot == 0 && p.side == Side::Away {
            away_holder = Some(e);
        }
    }
    let holder = if give_home { home_holder } else { away_holder };
    if let Ok((mut tf, mut vel, mut st)) = ball.single_mut() {
        tf.translation = if give_home {
            Vec3::new(-3.1, 1.1, 0.0)
        } else {
            Vec3::new(3.1, 1.1, 0.0)
        };
        vel.0 = Vec3::ZERO;
        st.hold = if holder.is_some() { Hold::Held } else { Hold::Loose };
        st.holder = holder;
        st.shooter = None;
        st.last_touch = holder;
        st.last_passer = None;
        st.rim_hits = 0;
        st.release_was_three = false;
    }
}

fn apply_intents(
    time: Res<Time<Fixed>>,
    paused: Res<Paused>,
    mut intent: ResMut<PlayerIntent>,
    mut meter: ResMut<ShotMeter>,
    control: Res<LiveControl>,
    cam: Query<&Transform, (With<GameCam>, Without<Player>)>,
    mut q: Query<(
        Entity,
        &Player,
        &Ratings,
        &mut MoveVel,
        &mut Transform,
        &mut Pose,
        &mut PoseClock,
        &mut Stamina,
    ), Without<GameCam>>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let Some(ctrl) = control.entity else {
        intent.shoot_released = false;
        intent.pass = false;
        intent.steal = false;
        intent.special = false;
        intent.switch = false;
        return;
    };
    for (e, _p, ratings, mut vel, mut tf, mut pose, mut clock, stam) in &mut q {
        if e != ctrl {
            continue;
        }
        if matches!(*pose, Pose::Shoot | Pose::Dunk | Pose::Pass | Pose::Block | Pose::Stumble) {
            continue;
        }
        let stick = intent.move_xz;
        let dir = if let Ok(ctf) = cam.single() {
            let mut fwd = Vec3::new(ctf.forward().x, 0.0, ctf.forward().z);
            if fwd.length_squared() < 0.04 {
                fwd = Vec3::new(0.0, 0.0, -1.0);
            } else {
                fwd = fwd.normalize();
            }
            let mut right = Vec3::new(ctf.right().x, 0.0, ctf.right().z);
            if right.length_squared() < 0.04 {
                right = Vec3::new(1.0, 0.0, 0.0);
            } else {
                right = right.normalize();
            }
            right * stick.x + fwd * (-stick.y)
        } else {
            Vec3::new(stick.x, 0.0, stick.y)
        };
        let sprint = intent.sprint && stam.0 > 0.12;
        let spd = crate::units::move_speed(ratings, sprint, stam.0);
        if dir.length_squared() > 0.04 {
            let n = dir.normalize();
            vel.0 = n * spd;
            *pose = if sprint { Pose::Sprint } else { Pose::Run };
        } else {
            vel.0 = vel.0.lerp(Vec3::ZERO, 8.0 * dt);
            *pose = Pose::Idle;
        }
        tf.translation += vel.0 * dt;
        let (x, z) = clamp_to_court(tf.translation.x, tf.translation.z, 0.55);
        tf.translation.x = x;
        tf.translation.z = z;
        tf.translation.y = 0.0;

        if intent.shoot_held {
            meter.armed = true;
            meter.value += dt * 1.15 * if meter.rising { 1.0 } else { -1.0 };
            if meter.value > 1.0 {
                meter.value = 1.0;
                meter.rising = false;
            }
            if meter.value < 0.0 {
                meter.value = 0.0;
                meter.rising = true;
            }
        }
        let _ = &mut clock;
    }
    // switch handled in input
}

fn follow_dribble(
    paused: Res<Paused>,
    time: Res<Time>,
    mut ticks: MessageWriter<DribbleTickEvent>,
    holders: Query<(&Transform, &MoveVel, &Pose), With<Player>>,
    mut ball: Query<(&mut Transform, &mut BallVel, &mut BallState), (With<Ball>, Without<Player>)>,
) {
    if paused.0 {
        return;
    }
    let Ok((mut btf, mut bvel, mut state)) = ball.single_mut() else {
        return;
    };
    let Some(h) = state.holder else {
        return;
    };
    if state.hold != Hold::Held {
        return;
    }
    let Ok((ptf, vel, pose)) = holders.get(h) else {
        return;
    };
    let speed = Vec3::new(vel.0.x, 0.0, vel.0.z).length();
    let cadence = dribble_cadence(speed);
    let prev = state.dribble_phase;
    state.dribble_phase += time.delta_secs() * cadence * std::f32::consts::TAU;
    let phase = state.dribble_phase;
    // Crossed the floor (phase wrap through π)
    let prev_sin = prev.sin();
    if prev_sin > 0.0 && phase.sin() <= 0.0 {
        ticks.write(DribbleTickEvent { pos: btf.translation });
    }
    let peak = (0.38 + speed * 0.015).clamp(0.28, 0.52);
    let bounce = BALL_RADIUS + (1.0 + phase.cos()) * 0.5 * peak;
    let hand = ptf.right() * 0.38 + ptf.forward() * 0.18;
    let height = if matches!(*pose, Pose::Shoot | Pose::Dunk) {
        1.85
    } else {
        bounce
    };
    btf.translation = ptf.translation + Vec3::new(hand.x, height, hand.z);
    bvel.0 = vel.0;
}

fn pickup_loose_ball(
    paused: Res<Paused>,
    mut ticker: ResMut<Ticker>,
    mut last_pass: ResMut<LastPass>,
    mut clock: ResMut<MatchClock>,
    config: Res<MatchConfig>,
    mut ball: Query<(&Transform, &mut BallState, &BallVel), (With<Ball>, Without<Player>)>,
    mut players: Query<(Entity, &Transform, &Ratings, &Player, &mut BoxLine), Without<Ball>>,
) {
    if paused.0 {
        return;
    }
    let Ok((btf, mut state, bvel)) = ball.single_mut() else {
        return;
    };
    if !matches!(state.hold, Hold::Loose | Hold::Shot | Hold::Pass) {
        return;
    }
    if state.hold != Hold::Loose && (bvel.0.length() > 4.8 || btf.translation.y > 1.55) {
        return;
    }
    if state.hold == Hold::Loose && bvel.0.length() > 9.5 && btf.translation.y > 1.4 {
        return;
    }
    let mut best: Option<(Entity, f32)> = None;
    for (e, tf, r, _p, _) in &players {
        let d = tf.translation.distance(btf.translation);
        let reach = 0.85 + r.height * 0.12 + r.rebound / 220.0;
        if d < reach {
            let score = r.rebound + (2.0 - d) * 20.0;
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some((e, score));
            }
        }
    }
    if let Some((e, _)) = best {
        let rebound = state.shooter.is_some() || state.rim_hits > 0;
        state.hold = Hold::Held;
        state.holder = Some(e);
        state.last_touch = Some(e);
        state.shooter = None;
        state.rim_hits = 0;
        clock.shot = config.shot_clock;
        if rebound {
            if let Ok((_, _, _, _, mut boxl)) = players.get_mut(e) {
                boxl.reb += 1;
            }
            last_pass.passer = None;
            last_pass.age = 99.0;
            ticker.line = "REBOUND — THE ROCK IS OURS".into();
        } else {
            last_pass.passer = None;
            last_pass.age = 99.0;
            ticker.line = "POSSESSION CHANGE".into();
        }
        ticker.age = 0.0;
    }
}

fn shoot_and_pass(
    paused: Res<Paused>,
    mut intent: ResMut<PlayerIntent>,
    mut meter: ResMut<ShotMeter>,
    mut rng: ResMut<GameRng>,
    mut ticker: ResMut<Ticker>,
    mut clock: ResMut<MatchClock>,
    control: Res<LiveControl>,
    mut plays: MessageWriter<PlayCall>,
    mut last_pass: ResMut<LastPass>,
    mut ball: Query<(&mut Transform, &mut BallVel, &mut BallSpin, &mut BallState), (With<Ball>, Without<Player>)>,
    mut players: Query<(
        Entity,
        &Player,
        &Ratings,
        &Transform,
        &MoveVel,
        &mut Pose,
        &mut PoseClock,
        &mut BoxLine,
        &Stamina,
    ), Without<Ball>>,
) {
    if paused.0 {
        intent.shoot_released = false;
        intent.pass = false;
        return;
    }
    let Ok((mut btf, mut bvel, mut spin, mut state)) = ball.single_mut() else {
        intent.shoot_released = false;
        return;
    };
    let Some(ctrl) = control.entity else {
        intent.shoot_released = false;
        return;
    };

    // Snapshot needed data to avoid double borrows
    let roster: Vec<(Entity, Side, Vec3, Vec3, f32, f32, f32, f32, f32, f32, bool)> = players
        .iter()
        .map(|(e, p, r, t, v, _, _, _, s)| {
            (
                e,
                p.side,
                t.translation,
                v.0,
                r.three,
                r.mid,
                r.dunk,
                r.pass,
                r.block,
                s.0,
                p.human,
            )
        })
        .collect();

    let Some(me) = roster.iter().find(|x| x.0 == ctrl).cloned() else {
        intent.shoot_released = false;
        return;
    };
    let (me_e, me_side, me_pos, me_vel, three, mid, dunk, pass, _blk, stam, _) = me;

    if intent.special && state.hold == Hold::Held && state.holder == Some(ctrl) {
        intent.shoot_released = true;
    }

    if intent.pass && state.hold == Hold::Held && state.holder == Some(ctrl) {
        if let Some(target) = nearest_teammate(&roster, me_e, me_side, me_pos) {
            let kind = intent.pass_kind;
            let dest = match kind {
                PassKind::Lob => target.2 + Vec3::Y * 2.6,
                PassKind::Bounce => target.2 + Vec3::Y * 0.35,
                _ => target.2 + Vec3::Y * 1.4,
            };
            let (flight, grav) = match kind {
                PassKind::Lob => ((me_pos.distance(dest) / 9.0).clamp(0.45, 0.85), GRAVITY * 0.55),
                PassKind::Bounce => ((me_pos.distance(dest) / 11.0).clamp(0.28, 0.55), GRAVITY * 0.9),
                PassKind::Skip => ((me_pos.distance(dest) / 15.0).clamp(0.16, 0.38), GRAVITY * 0.15),
                PassKind::Chest => ((me_pos.distance(dest) / 13.0).clamp(0.18, 0.5), GRAVITY * 0.28),
            };
            let v = ballistic_velocity(
                [btf.translation.x, 1.4, btf.translation.z],
                [dest.x, dest.y, dest.z],
                flight,
                grav,
            );
            bvel.0 = Vec3::new(v[0], v[1], v[2]);
            state.hold = Hold::Pass;
            state.holder = None;
            state.last_touch = Some(ctrl);
            state.last_passer = Some(ctrl);
            last_pass.passer = Some(ctrl);
            last_pass.age = 0.0;
            spin.0 = Vec3::new(0.0, 18.0, 0.0);
            if let Ok((_, _, _, _, _, mut pose, mut clock, _, _)) = players.get_mut(ctrl) {
                *pose = Pose::Pass;
                clock.0 = 0.0;
            }
            ticker.line = match kind {
                PassKind::Lob => "LOB — LOOKING FOR THE OOP".into(),
                PassKind::Bounce => "BOUNCE PASS — AROUND THE TREE".into(),
                PassKind::Skip => "SKIP PASS — CROSS COURT".into(),
                PassKind::Chest => "SILK DISH — ON THE MOVE".into(),
            };
            ticker.age = 0.0;
        }
        intent.pass = false;
        return;
    }

    if intent.shoot_released && state.hold == Hold::Held && state.holder == Some(ctrl) {
        let hoop_x = if me_side == Side::Home { HOOP_X } else { -HOOP_X };
        let hoop = Vec3::new(hoop_x, RIM_HEIGHT, 0.0);
        let dist = me_pos.distance(Vec3::new(hoop_x, me_pos.y, 0.0));
        let in_the_paint = in_paint(me_pos.x, me_pos.z, hoop_x);
        let to_hoop = (hoop - me_pos).normalize_or_zero();
        let driving = me_vel.dot(to_hoop) > 2.2;
        let moving_away = me_vel.dot(to_hoop) < -1.5;
        let lateral = me_vel.cross(to_hoop).y.abs() > 2.0;
        let speed = me_vel.length();
        let meter_err = meter_accuracy(meter.value);
        meter.armed = true;
        meter.freeze = 0.45;
        meter.rising = true;

        let contest = nearest_contest(&roster, me_e, me_side, me_pos);
        let shot = classify_shot(
            dist,
            in_the_paint,
            driving,
            moving_away,
            lateral,
            dunk,
            mid,
            intent.special,
            speed,
        );

        if shot == ShotType::Dunk && dist < PAINT_DEPTH + 0.8 {
            let t = 0.55;
            let v = ballistic_velocity(
                [btf.translation.x, 1.8, btf.translation.z],
                [hoop.x, hoop.y + 0.12, hoop.z],
                t,
                GRAVITY,
            );
            bvel.0 = Vec3::new(v[0], v[1], v[2]) + me_vel * 0.15;
            spin.0 = Vec3::from_array(release_spin(shot, 1.0, hoop.x - me_pos.x, hoop.z - me_pos.z));
            state.hold = Hold::Shot;
            state.holder = None;
            state.shooter = Some(ctrl);
            state.rim_hits = 0;
            state.release_was_three = false;
            if let Ok((_, _, _, _, _, mut pose, mut clock, mut boxl, _)) = players.get_mut(ctrl) {
                *pose = Pose::Dunk;
                clock.0 = 0.0;
                boxl.fg_att += 1;
            }
            plays.write(PlayCall {
                text: "POSTERIZE ATTEMPT".into(),
            });
            ticker.line = "GRAVITY CHECK — DUNK ATTEMPT".into();
            ticker.age = 0.0;
            intent.shoot_released = false;
            intent.special = false;
            return;
        }

        let is_three = matches!(shot, ShotType::ThreePointer | ShotType::LogoHeave)
            || matches!(shot_kind(me_pos.x, me_pos.z, hoop_x), crate::sim::ShotKind::Three);
        let rating = if is_three { three } else { mid };
        let chance = shot_make_chance(rating, dist, contest, meter_err, stam, is_three);
        let mut target = hoop;
        if rng.f32() > chance {
            target += Vec3::new(rng.range(-0.55, 0.55), rng.range(0.05, 0.45), rng.range(-0.55, 0.55));
        } else {
            target += Vec3::new(rng.range(-0.04, 0.04), 0.02, rng.range(-0.04, 0.04));
        }
        let height = release_height(shot);
        let flight = match shot {
            ShotType::Layup | ShotType::ReverseLayup | ShotType::FingerRoll => 0.42,
            ShotType::Underhand => 0.55,
            ShotType::Floater => 0.62,
            ShotType::Hook => 0.7,
            ShotType::LogoHeave => (flight_time_for_distance(dist) * 1.05).min(1.6),
            _ => flight_time_for_distance(dist),
        };
        let v = ballistic_velocity(
            [me_pos.x, height, me_pos.z],
            [target.x, target.y, target.z],
            flight,
            GRAVITY,
        );
        btf.translation = me_pos + Vec3::Y * height;
        bvel.0 = Vec3::new(v[0], v[1], v[2]) + me_vel * 0.12;
        let quality = (1.0 - meter_err * 1.2).clamp(0.5, 1.1);
        spin.0 = Vec3::from_array(release_spin(shot, quality, hoop.x - me_pos.x, hoop.z - me_pos.z));
        state.hold = Hold::Shot;
        state.holder = None;
        state.shooter = Some(ctrl);
        state.rim_hits = 0;
        state.release_was_three = is_three;
        if let Ok((_, _, _, _, _, mut pose, mut clock, mut boxl, _)) = players.get_mut(ctrl) {
            *pose = Pose::Shoot;
            clock.0 = 0.0;
            boxl.fg_att += 1;
        }
        clock.running = true;
        ticker.line = shot.label().into();
        ticker.age = 0.0;
        let _ = pass;
    }
    intent.shoot_released = false;
    intent.special = false;
}

fn nearest_teammate(
    roster: &[(Entity, Side, Vec3, Vec3, f32, f32, f32, f32, f32, f32, bool)],
    me: Entity,
    side: Side,
    pos: Vec3,
) -> Option<(Entity, Side, Vec3, Vec3, f32, f32, f32, f32, f32, f32, bool)> {
    roster
        .iter()
        .filter(|r| r.0 != me && r.1 == side)
        .min_by(|a, b| {
            a.2.distance(pos)
                .partial_cmp(&b.2.distance(pos))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .cloned()
}

fn nearest_contest(
    roster: &[(Entity, Side, Vec3, Vec3, f32, f32, f32, f32, f32, f32, bool)],
    me: Entity,
    side: Side,
    pos: Vec3,
) -> f32 {
    roster
        .iter()
        .filter(|r| r.0 != me && r.1 != side)
        .map(|r| contest_factor(r.2.distance(pos), r.8))
        .fold(0.0, f32::max)
}

fn steal_attempts(
    paused: Res<Paused>,
    mut intent: ResMut<PlayerIntent>,
    mut rng: ResMut<GameRng>,
    mut ticker: ResMut<Ticker>,
    mut steals: MessageWriter<StealEvent>,
    mut cams: MessageWriter<crate::camera::CamTrigger>,
    mut last_pass: ResMut<LastPass>,
    control: Res<LiveControl>,
    mut ball: Query<&mut BallState, With<Ball>>,
    mut players: Query<(Entity, &Player, &Ratings, &Transform, &mut Pose, &mut PoseClock, &mut BoxLine)>,
) {
    if paused.0 || !intent.steal {
        intent.steal = false;
        return;
    }
    intent.steal = false;
    let Some(ctrl) = control.entity else {
        return;
    };
    let Ok(mut state) = ball.single_mut() else {
        return;
    };
    let Some(holder) = state.holder else {
        return;
    };
    if state.hold != Hold::Held || holder == ctrl {
        return;
    }
    let mut thief_data = None;
    let mut vic_data = None;
    for (e, p, r, t, _, _, _) in &players {
        if e == ctrl {
            thief_data = Some((p.side, t.translation, r.steal));
        }
        if e == holder {
            vic_data = Some((p.side, t.translation, r.handle));
        }
    }
    let Some((_, tp, steal)) = thief_data else {
        return;
    };
    let Some((vs, vp, handle)) = vic_data else {
        return;
    };
    let Some((_, thief_side, _, _, _, _, _)) = players.iter().find(|(e, ..)| *e == ctrl) else {
        return;
    };
    let _ = (vs, thief_side);
    let d = tp.distance(vp);
    if rng.f32() < steal_chance(steal, handle, d) {
        state.holder = Some(ctrl);
        state.last_touch = Some(ctrl);
        steals.write(StealEvent {
            success: true,
            pos: tp,
        });
        cams.write(crate::camera::CamTrigger::Steal);
        last_pass.passer = None;
        last_pass.age = 99.0;
        ticker.line = "STRIPPED — GHOST MODE".into();
        ticker.age = 0.0;
        if let Ok((_, _, _, _, mut pose, mut clock, mut boxl)) = players.get_mut(ctrl) {
            *pose = Pose::Idle;
            clock.0 = 0.0;
            boxl.stl += 1;
        }
        if let Ok((_, _, _, _, mut pose, mut clock, _)) = players.get_mut(holder) {
            *pose = Pose::Stumble;
            clock.0 = 0.0;
        }
    } else if let Ok((_, _, _, _, mut pose, mut clock, _)) = players.get_mut(ctrl) {
        *pose = Pose::Stumble;
        clock.0 = 0.0;
        steals.write(StealEvent {
            success: false,
            pos: tp,
        });
        ticker.line = "REACH-IN — NO WHISTLE, NO BALL".into();
        ticker.age = 0.0;
    }
}

fn block_attempts(
    paused: Res<Paused>,
    mut intent: ResMut<PlayerIntent>,
    mut rng: ResMut<GameRng>,
    mut ticker: ResMut<Ticker>,
    control: Res<LiveControl>,
    mut ball: Query<(&Transform, &mut BallVel, &mut BallState), (With<Ball>, Without<Player>)>,
    mut players: Query<(Entity, &Player, &Ratings, &Transform, &mut Pose, &mut PoseClock, &mut BoxLine), Without<Ball>>,
) {
    if paused.0 || !intent.block {
        intent.block = false;
        return;
    }
    intent.block = false;
    let Some(ctrl) = control.entity else {
        return;
    };
    let Ok((btf, mut bvel, mut state)) = ball.single_mut() else {
        return;
    };
    if !matches!(state.hold, Hold::Shot) {
        if let Ok((_, _, _, _, mut pose, mut clock, _)) = players.get_mut(ctrl) {
            *pose = Pose::Block;
            clock.0 = 0.0;
        }
        return;
    }
    let Ok((_, _, ratings, ptf, mut pose, mut clock, mut boxl)) = players.get_mut(ctrl) else {
        return;
    };
    *pose = Pose::Block;
    clock.0 = 0.0;
    let d = ptf.translation.distance(btf.translation);
    let window = btf.translation.y > 1.6 && btf.translation.y < 3.2;
    let chance = (ratings.block / 100.0) * (1.15 - d * 0.45).clamp(0.0, 1.0);
    if window && d < 2.2 && rng.f32() < chance {
        bvel.0 = Vec3::new(-bvel.0.x * 0.35, bvel.0.y.abs() * 0.2, -bvel.0.z * 0.35)
            + Vec3::new(rng.range(-2.0, 2.0), 1.2, rng.range(-2.0, 2.0));
        state.hold = Hold::Loose;
        state.shooter = None;
        boxl.blk += 1;
        ticker.line = "REJECTED — GET THAT OUTTA HERE".into();
        ticker.age = 0.0;
    }
}

fn handle_buckets(
    mut buckets: MessageReader<BucketEvent>,
    mut score: ResMut<Scoreboard>,
    mut ticker: ResMut<Ticker>,
    mut clock: ResMut<MatchClock>,
    mut last_pass: ResMut<LastPass>,
    config: Res<MatchConfig>,
    mut players: Query<(Entity, &Player, &Transform, &mut Pose, &mut PoseClock, &mut BoxLine)>,
    mut next: ResMut<NextState<AppState>>,
) {
    for ev in buckets.read() {
        let mut pts = 2;
        let mut side = Side::Home;
        if let Some(shooter) = ev.shooter {
            if let Ok((_, p, tf, mut pose, mut clockp, mut boxl)) = players.get_mut(shooter) {
                side = p.side;
                let hoop_x = if p.side == Side::Home { HOOP_X } else { -HOOP_X };
                pts = if ev.dunk {
                    2
                } else if ev.is_three {
                    3
                } else {
                    2
                };
                let _ = (tf, hoop_x);
                boxl.pts += pts;
                boxl.fg_made += 1;
                *pose = Pose::Celebrate;
                clockp.0 = 0.0;
            }
            if last_pass.age < 3.2 {
                if let Some(passer) = last_pass.passer {
                    if Some(passer) != ev.shooter {
                        if let Ok((_, p, _, _, _, mut boxl)) = players.get_mut(passer) {
                            if p.side == side {
                                boxl.ast += 1;
                            }
                        }
                    }
                }
            }
        } else {
            // infer by hoop
            side = if ev.hoop_home { Side::Away } else { Side::Home };
        }
        match side {
            Side::Home => score.home += pts,
            Side::Away => score.away += pts,
        }
        ticker.line = if ev.dunk {
            format!(
                "{} — POSTER DUNK +{}",
                if side == Side::Home { "FOX" } else { "CRN" },
                pts
            )
        } else {
            format!(
                "{} BUCKET +{}  |  {}-{}",
                if side == Side::Home { "NEON FOXES" } else { "SHADOW CRANES" },
                pts,
                score.home,
                score.away
            )
        };
        ticker.age = 0.0;
        last_pass.passer = None;
        last_pass.age = 99.0;
        clock.shot = config.shot_clock;
        if config.mode != GameMode::Practice && clock.quarter > 4 && score.home != score.away {
            next.set(AppState::GameOver);
        }
    }
}

fn inbound_after_score(
    mut buckets: MessageReader<BucketEvent>,
    mut ticker: ResMut<Ticker>,
    mut last_pass: ResMut<LastPass>,
    mut cams: MessageWriter<crate::camera::CamTrigger>,
    mut ball: Query<(&mut Transform, &mut BallVel, &mut BallState), (With<Ball>, Without<Player>)>,
    mut players: Query<(Entity, &Player, &mut Transform), Without<Ball>>,
) {
    for ev in buckets.read() {
        let scoring = if let Some(shooter) = ev.shooter {
            players
                .get(shooter)
                .ok()
                .map(|(_, p, _)| p.side)
                .unwrap_or(if ev.hoop_home { Side::Away } else { Side::Home })
        } else if ev.hoop_home {
            Side::Away
        } else {
            Side::Home
        };
        let inbound_side = if scoring == Side::Home {
            Side::Away
        } else {
            Side::Home
        };
        let mut inbounder = None;
        for (e, p, mut tf) in &mut players {
            if p.side == inbound_side {
                let baseline = if inbound_side == Side::Away { 12.4 } else { -12.4 };
                let z = (p.slot as f32 - 1.0) * 2.4;
                tf.translation = Vec3::new(baseline, 0.0, z);
                if p.slot == 0 {
                    inbounder = Some(e);
                }
            } else {
                let mid = if p.side == Side::Home { -4.0 } else { 4.0 };
                tf.translation = Vec3::new(mid, 0.0, (p.slot as f32 - 1.0) * 3.0);
            }
        }
        if let Ok((mut tf, mut vel, mut st)) = ball.single_mut() {
            let x = if inbound_side == Side::Away { 12.1 } else { -12.1 };
            tf.translation = Vec3::new(x, 1.1, 0.0);
            vel.0 = Vec3::ZERO;
            st.hold = if inbounder.is_some() {
                Hold::Held
            } else {
                Hold::Loose
            };
            st.holder = inbounder;
            st.shooter = None;
            st.last_touch = inbounder;
            st.last_passer = None;
            st.rim_hits = 0;
            st.release_was_three = false;
        }
        last_pass.passer = None;
        last_pass.age = 99.0;
        ticker.line = if inbound_side == Side::Away {
            "INBOUND — CRANES HAVE IT".into()
        } else {
            "INBOUND — FOXES HAVE IT".into()
        };
        ticker.age = 0.0;
        cams.write(crate::camera::CamTrigger::Inbound);
    }
}

fn tick_meter_freeze(time: Res<Time>, mut meter: ResMut<ShotMeter>) {
    if meter.freeze > 0.0 {
        meter.freeze = (meter.freeze - time.delta_secs()).max(0.0);
        if meter.freeze <= 0.0 {
            meter.armed = false;
        }
    }
}

fn pose_timeouts(_time: Res<Time>, paused: Res<Paused>, mut q: Query<(&mut Pose, &mut PoseClock)>) {
    if paused.0 {
        return;
    }
    for (mut pose, mut clock) in &mut q {
        let limit = match *pose {
            Pose::Shoot => 0.55,
            Pose::Dunk => 0.9,
            Pose::Pass => 0.35,
            Pose::Block => 0.45,
            Pose::Celebrate => 1.4,
            Pose::Stumble => 0.5,
            _ => continue,
        };
        if clock.0 > limit {
            *pose = Pose::Idle;
            clock.0 = 0.0;
        }
    }
}

fn maybe_end_game() {}
