use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::Hdr;

use crate::ball::{Ball, BallState, BucketEvent, Hold};
use crate::court::RimMarker;
use crate::gameplay::{MatchClock, PlayCall};
use crate::roster::Side;
use crate::sim::{HOOP_X, RIM_HEIGHT, THREE_RADIUS};
use crate::states::{AppState, CameraMode, CameraSettings, Paused};
use crate::units::{Controlled, Player, Pose};

const FLOOR_Y: f32 = 1.2;
const CEIL_Y: f32 = 32.0;
const MAX_ABS_Z: f32 = 19.0;
const DEFAULT_FOV: f32 = 52.0;

const HOLD_DUNK: f32 = 0.9;
const HOLD_NET_SNAP: f32 = 0.25;
const HOLD_REPLAY: f32 = 0.9;
const HOLD_CELEBRATE: f32 = 1.0;
const HOLD_SHOT: f32 = 0.55;
const HOLD_BLOCK: f32 = 0.45;
const HOLD_STEAL: f32 = 0.4;
const HOLD_INBOUND: f32 = 1.1;
const HOLD_BUZZER: f32 = 0.7;
const REPLAY_COOLDOWN: f32 = 2.2;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraSettings>()
            .init_resource::<CameraDirector>()
            .init_resource::<CameraPostFx>()
            .add_message::<CamTrigger>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                orbit_menu_cam.run_if(not(in_state(AppState::Playing))),
            )
            .add_systems(
                Update,
                (emit_cam_triggers, follow_game_cam)
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

/// Broadcast director language — not just the four user base lenses.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Default)]
pub enum CameraShot {
    #[default]
    BroadcastSideline,
    BroadcastEndzone,
    ChasePrimary,
    TacticalTopDown,
    CinemaLowHero,
    LayupGather,
    DunkTakeoff,
    PosterCine,
    NetSnap,
    StealPunch,
    BlockReject,
    BuzzerBeat,
    ThreePointWide,
    LogoHalfCourt,
    InboundBaseline,
    FastBreakWide,
    HoopCamOut,
    ReplayEnglish,
    CelebrateHold,
    MenuOrbit,
}

#[derive(Resource, Clone, Debug)]
pub struct CameraDirector {
    pub active: CameraShot,
    pub hold: f32,
    pub replay_cooldown: f32,
    chain_dunk: bool,
    in_replay: bool,
}

impl Default for CameraDirector {
    fn default() -> Self {
        Self {
            active: CameraShot::BroadcastSideline,
            hold: 0.0,
            replay_cooldown: 0.0,
            chain_dunk: false,
            in_replay: false,
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct CameraPostFx {
    pub letterbox: f32,
    pub crowd_flash: f32,
    pub shake: f32,
}

impl Default for CameraPostFx {
    fn default() -> Self {
        Self {
            letterbox: 0.0,
            crowd_flash: 0.0,
            shake: 0.0,
        }
    }
}

#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CamTrigger {
    ShotReleased,
    DunkTakeoff,
    Steal,
    Block,
    Inbound,
}

#[derive(Component)]
pub struct GameCam;

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        GameCam,
        Camera3d::default(),
        Msaa::Off,
        Hdr,
        Bloom {
            intensity: 0.18,
            low_frequency_boost: 0.55,
            ..Bloom::NATURAL
        },
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.03, 0.06)),
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: DEFAULT_FOV.to_radians(),
            ..default()
        }),
        Tonemapping::TonyMcMapface,
        Transform::from_xyz(0.0, 10.5, 16.5).looking_at(Vec3::new(0.0, 0.8, 0.0), Vec3::Y),
        DistanceFog {
            color: Color::srgb(0.03, 0.04, 0.1),
            falloff: FogFalloff::Linear {
                start: 28.0,
                end: 70.0,
            },
            ..default()
        },
    ));
}

fn orbit_menu_cam(
    time: Res<Time>,
    mut director: ResMut<CameraDirector>,
    mut fx: ResMut<CameraPostFx>,
    mut q: Query<(&mut Transform, &mut Projection), With<GameCam>>,
) {
    director.active = CameraShot::MenuOrbit;
    director.hold = 0.0;
    tick_post_fx(&mut fx, CameraShot::MenuOrbit, time.delta_secs());

    let Ok((mut tf, mut proj)) = q.single_mut() else {
        return;
    };
    let t = time.elapsed_secs() * 0.18;
    let desired = clamp_cam(Vec3::new(
        t.sin() * 18.0,
        9.0 + t.cos() * 0.6,
        t.cos() * 18.0,
    ));
    let look = Vec3::new(0.0, 0.6, 0.0);
    smooth_cam(
        &mut tf,
        &mut proj,
        desired,
        look,
        48.0_f32.to_radians(),
        time.delta_secs(),
        1.8,
        2.2,
        fx.shake,
        time.elapsed_secs(),
    );
}

fn emit_cam_triggers(
    mut triggers: MessageWriter<CamTrigger>,
    players: Query<(Entity, &Transform, &Pose), (With<Player>, Without<GameCam>, Without<Ball>)>,
    ball: Query<&BallState, (With<Ball>, Without<GameCam>, Without<Player>)>,
    mut prev: Local<Vec<(Entity, Pose)>>,
    mut inbound_latched: Local<bool>,
) {
    for (e, _, pose) in &players {
        let was = prev.iter().find(|(id, _)| *id == e).map(|(_, p)| *p);
        if was == Some(*pose) {
            continue;
        }
        match *pose {
            Pose::Dunk => {
                triggers.write(CamTrigger::DunkTakeoff);
            }
            Pose::Shoot => {
                triggers.write(CamTrigger::ShotReleased);
            }
            Pose::Block => {
                triggers.write(CamTrigger::Block);
            }
            _ => {}
        }
    }
    prev.clear();
    prev.extend(players.iter().map(|(e, _, p)| (e, *p)));

    let inbound_now = ball.single().ok().and_then(|st| {
        if st.hold != Hold::Held {
            return None;
        }
        let holder = st.holder?;
        players
            .get(holder)
            .ok()
            .map(|(_, tf, _)| tf.translation.x.abs() > 12.2)
    });
    match inbound_now {
        Some(true) if !*inbound_latched => {
            triggers.write(CamTrigger::Inbound);
            *inbound_latched = true;
        }
        Some(true) => {}
        _ => *inbound_latched = false,
    }
}

fn follow_game_cam(
    time: Res<Time>,
    paused: Res<Paused>,
    settings: Res<CameraSettings>,
    clock: Res<MatchClock>,
    mut director: ResMut<CameraDirector>,
    mut fx: ResMut<CameraPostFx>,
    mut buckets: MessageReader<BucketEvent>,
    mut triggers: MessageReader<CamTrigger>,
    mut plays: MessageReader<PlayCall>,
    ball: Query<(&Transform, &BallState), (With<Ball>, Without<GameCam>, Without<Player>)>,
    hero: Query<&Transform, (With<Controlled>, Without<GameCam>, Without<Ball>)>,
    players: Query<(&Transform, &Pose, &Player), (With<Player>, Without<GameCam>, Without<Ball>)>,
    rims: Query<(&Transform, &RimMarker), (Without<GameCam>, Without<Ball>, Without<Player>)>,
    mut cam: Query<(&mut Transform, &mut Projection), With<GameCam>>,
) {
    let dt = time.delta_secs();
    if !paused.0 {
        director.hold = (director.hold - dt).max(0.0);
        director.replay_cooldown = (director.replay_cooldown - dt).max(0.0);
        advance_replay_chain(&mut director);
    }

    let ball_pair = ball.single().ok();
    let ball_pos = ball_pair.map(|(t, _)| t.translation).unwrap_or(Vec3::ZERO);
    let ball_state = ball_pair.map(|(_, s)| s);
    let hero_pos = hero
        .single()
        .ok()
        .map(|t| t.translation)
        .unwrap_or(ball_pos);
    let look_live = ball_pos.lerp(hero_pos, 0.35) + Vec3::Y * 0.6;

    let mut dunk_actor: Option<(Vec3, Side)> = None;
    let mut shoot_actor: Option<(Vec3, Side)> = None;
    let mut block_actor: Option<Vec3> = None;
    let mut stumble_actor: Option<Vec3> = None;
    let mut celebrate_actor: Option<Vec3> = None;
    let mut sprinting = false;
    for (tf, pose, player) in &players {
        match *pose {
            Pose::Dunk => dunk_actor = Some((tf.translation, player.side)),
            Pose::Shoot => shoot_actor = Some((tf.translation, player.side)),
            Pose::Block => block_actor = Some(tf.translation),
            Pose::Stumble => stumble_actor = Some(tf.translation),
            Pose::Celebrate => celebrate_actor = Some(tf.translation),
            Pose::Sprint => sprinting = true,
            _ => {}
        }
    }

    let mut posterize = false;
    for play in plays.read() {
        let t = play.text.to_ascii_uppercase();
        if t.contains("POSTER") {
            posterize = true;
        }
        if t.contains("INBOUND") {
            cut_to(
                &mut director,
                &mut fx,
                CameraShot::InboundBaseline,
                HOLD_INBOUND,
                0.15,
                0.0,
            );
        }
    }

    for ev in buckets.read() {
        if director.replay_cooldown > 0.0 {
            continue;
        }
        director.chain_dunk = ev.dunk;
        director.in_replay = true;
        cut_to(
            &mut director,
            &mut fx,
            CameraShot::NetSnap,
            HOLD_NET_SNAP,
            if ev.dunk { 0.95 } else { 0.7 },
            1.0,
        );
    }

    for trig in triggers.read() {
        match *trig {
            CamTrigger::DunkTakeoff => {
                let shot = if posterize {
                    CameraShot::PosterCine
                } else {
                    CameraShot::DunkTakeoff
                };
                cut_to(&mut director, &mut fx, shot, HOLD_DUNK, 0.5, 0.08);
            }
            CamTrigger::ShotReleased => {
                if let Some((pos, side)) = shoot_actor {
                    let hoop = hoop_for_side(&rims, side);
                    apply_shoot_cut(&mut director, &mut fx, &clock, court_dist(pos, hoop));
                } else {
                    let hoop = nearest_hoop(&rims, ball_pos);
                    apply_shoot_cut(&mut director, &mut fx, &clock, court_dist(ball_pos, hoop));
                }
            }
            CamTrigger::Steal => {
                cut_to(
                    &mut director,
                    &mut fx,
                    CameraShot::StealPunch,
                    HOLD_STEAL,
                    0.4,
                    0.12,
                );
            }
            CamTrigger::Block => {
                cut_to(
                    &mut director,
                    &mut fx,
                    CameraShot::BlockReject,
                    HOLD_BLOCK,
                    0.38,
                    0.06,
                );
            }
            CamTrigger::Inbound => {
                cut_to(
                    &mut director,
                    &mut fx,
                    CameraShot::InboundBaseline,
                    HOLD_INBOUND,
                    0.12,
                    0.0,
                );
            }
        }
    }

    if director.hold <= 0.0 {
        if let Some((pos, side)) = dunk_actor {
            let hoop = hoop_for_side(&rims, side);
            let shot = if posterize || court_dist(pos, hoop) < 2.8 {
                CameraShot::PosterCine
            } else {
                CameraShot::DunkTakeoff
            };
            cut_to(&mut director, &mut fx, shot, HOLD_DUNK, 0.5, 0.08);
        } else if let Some((pos, side)) = shoot_actor {
            let hoop = hoop_for_side(&rims, side);
            apply_shoot_cut(&mut director, &mut fx, &clock, court_dist(pos, hoop));
        } else if block_actor.is_some() {
            cut_to(
                &mut director,
                &mut fx,
                CameraShot::BlockReject,
                HOLD_BLOCK,
                0.3,
                0.04,
            );
        } else if let Some(st) = ball_state {
            pick_live_base(
                &mut director,
                &mut fx,
                settings.mode,
                st,
                ball_pos,
                hero_pos,
                sprinting,
                &rims,
            );
        } else {
            cut_to(
                &mut director,
                &mut fx,
                base_shot(settings.mode, look_live, &rims),
                0.0,
                0.0,
                0.0,
            );
        }
    }

    tick_post_fx(&mut fx, director.active, dt);

    let Ok((mut ctf, mut proj)) = cam.single_mut() else {
        return;
    };

    let hoop_focus = {
        let side = dunk_actor
            .map(|(_, s)| s)
            .or_else(|| shoot_actor.map(|(_, s)| s));
        match side {
            Some(s) => hoop_for_side(&rims, s),
            None => nearest_hoop(&rims, ball_pos),
        }
    };
    let actor = dunk_actor
        .map(|(p, _)| p)
        .or(shoot_actor.map(|(p, _)| p))
        .or(block_actor)
        .or(stumble_actor)
        .or(celebrate_actor)
        .unwrap_or(hero_pos);

    let frame = frame_shot(
        director.active,
        look_live,
        actor,
        ball_pos,
        hoop_focus,
        time.elapsed_secs(),
    );
    smooth_cam(
        &mut ctf,
        &mut proj,
        frame.pos,
        frame.look,
        frame.fov_rad,
        dt,
        frame.pos_lambda,
        frame.rot_lambda,
        fx.shake,
        time.elapsed_secs(),
    );
}

fn advance_replay_chain(director: &mut CameraDirector) {
    if director.hold > 0.0 || !director.in_replay {
        return;
    }
    match director.active {
        CameraShot::NetSnap => {
            director.active = if director.chain_dunk {
                CameraShot::PosterCine
            } else {
                CameraShot::ReplayEnglish
            };
            director.hold = HOLD_REPLAY;
        }
        CameraShot::ReplayEnglish | CameraShot::PosterCine => {
            director.active = CameraShot::CelebrateHold;
            director.hold = HOLD_CELEBRATE;
            director.chain_dunk = false;
        }
        CameraShot::CelebrateHold => {
            director.replay_cooldown = REPLAY_COOLDOWN;
            director.in_replay = false;
            director.hold = 0.0;
        }
        _ => {
            director.in_replay = false;
        }
    }
}

fn apply_shoot_cut(
    director: &mut CameraDirector,
    fx: &mut CameraPostFx,
    clock: &MatchClock,
    dist: f32,
) {
    if clock.shot < 3.0 {
        cut_to(
            director,
            fx,
            CameraShot::BuzzerBeat,
            HOLD_BUZZER,
            0.22,
            0.35,
        );
        return;
    }
    if dist > 10.0 {
        cut_to(
            director,
            fx,
            CameraShot::LogoHalfCourt,
            HOLD_SHOT,
            0.08,
            0.05,
        );
    } else if dist >= THREE_RADIUS {
        cut_to(
            director,
            fx,
            CameraShot::ThreePointWide,
            HOLD_SHOT,
            0.1,
            0.08,
        );
    } else if dist < 3.5 {
        cut_to(director, fx, CameraShot::LayupGather, HOLD_SHOT, 0.16, 0.04);
    } else {
        cut_to(
            director,
            fx,
            CameraShot::BroadcastSideline,
            HOLD_SHOT * 0.35,
            0.0,
            0.0,
        );
    }
}

fn pick_live_base(
    director: &mut CameraDirector,
    fx: &mut CameraPostFx,
    mode: CameraMode,
    state: &BallState,
    ball_pos: Vec3,
    hero_pos: Vec3,
    sprinting: bool,
    rims: &Query<(&Transform, &RimMarker), (Without<GameCam>, Without<Ball>, Without<Player>)>,
) {
    let hoop = nearest_hoop(rims, ball_pos);
    let dist = court_dist(ball_pos, hoop);

    if matches!(state.hold, Hold::Shot | Hold::Loose) && dist < 3.2 && ball_pos.y > RIM_HEIGHT - 0.4
    {
        cut_to(director, fx, CameraShot::HoopCamOut, 0.28, 0.1, 0.0);
        return;
    }

    if sprinting && ball_pos.x.abs() < 6.5 && hero_pos.distance(ball_pos) < 4.0 {
        cut_to(director, fx, CameraShot::FastBreakWide, 0.0, 0.0, 0.0);
        return;
    }

    if state.hold == Hold::Held && ball_pos.x.abs() > 12.2 {
        cut_to(director, fx, CameraShot::InboundBaseline, 0.0, 0.0, 0.0);
        return;
    }

    let shot = base_shot(mode, ball_pos.lerp(hero_pos, 0.35), rims);
    cut_to(director, fx, shot, 0.0, 0.0, 0.0);
}

fn base_shot(
    mode: CameraMode,
    look: Vec3,
    rims: &Query<(&Transform, &RimMarker), (Without<GameCam>, Without<Ball>, Without<Player>)>,
) -> CameraShot {
    match mode {
        CameraMode::Broadcast => {
            let hoop = nearest_hoop(rims, look);
            if court_dist(look, hoop) < 5.0 {
                CameraShot::BroadcastEndzone
            } else {
                CameraShot::BroadcastSideline
            }
        }
        CameraMode::Chase => CameraShot::ChasePrimary,
        CameraMode::Tactical => CameraShot::TacticalTopDown,
        CameraMode::Cinema => CameraShot::CinemaLowHero,
    }
}

fn cut_to(
    director: &mut CameraDirector,
    fx: &mut CameraPostFx,
    shot: CameraShot,
    hold: f32,
    shake: f32,
    flash: f32,
) {
    let new_p = shot_priority(shot);
    let cur_p = shot_priority(director.active);
    let can = new_p > cur_p || director.hold <= 0.0;
    if !can {
        return;
    }
    if director.active != shot {
        fx.shake = fx.shake.max(shake);
        fx.crowd_flash = fx.crowd_flash.max(flash);
    }
    director.active = shot;
    if hold > 0.0 {
        director.hold = hold.max(director.hold);
    } else if new_p <= shot_priority(CameraShot::BroadcastSideline) {
        director.hold = 0.0;
    }
}

fn shot_priority(shot: CameraShot) -> u8 {
    match shot {
        CameraShot::NetSnap | CameraShot::ReplayEnglish | CameraShot::CelebrateHold => 100,
        CameraShot::DunkTakeoff | CameraShot::PosterCine => 90,
        CameraShot::BuzzerBeat => 80,
        CameraShot::BlockReject => 70,
        CameraShot::StealPunch => 65,
        CameraShot::InboundBaseline => 55,
        CameraShot::LayupGather | CameraShot::ThreePointWide | CameraShot::LogoHalfCourt => 50,
        CameraShot::HoopCamOut => 35,
        CameraShot::FastBreakWide => 25,
        CameraShot::BroadcastEndzone => 12,
        CameraShot::MenuOrbit => 5,
        CameraShot::BroadcastSideline
        | CameraShot::ChasePrimary
        | CameraShot::TacticalTopDown
        | CameraShot::CinemaLowHero => 10,
    }
}

struct ShotFrame {
    pos: Vec3,
    look: Vec3,
    fov_rad: f32,
    pos_lambda: f32,
    rot_lambda: f32,
}

fn frame_shot(
    shot: CameraShot,
    look: Vec3,
    actor: Vec3,
    ball: Vec3,
    hoop: Vec3,
    t: f32,
) -> ShotFrame {
    let hoop_sign = if hoop.x.abs() < 0.1 {
        1.0
    } else {
        hoop.x.signum()
    };
    let (pos, look_at, fov, pos_l, rot_l): (Vec3, Vec3, f32, f32, f32) = match shot {
        CameraShot::BroadcastSideline => (
            Vec3::new(look.x * 0.55, 6.9, 11.8),
            look + Vec3::Y * 0.3,
            46.0,
            2.8,
            3.5,
        ),
        CameraShot::BroadcastEndzone => (
            Vec3::new(hoop.x + hoop_sign * 3.6, 5.8, 3.6),
            look + Vec3::Y * 0.4,
            48.0,
            2.6,
            3.2,
        ),
        CameraShot::ChasePrimary => (
            actor + Vec3::new(-actor.x.signum() * 2.0, 4.2, 8.5),
            look,
            50.0,
            3.4,
            4.0,
        ),
        CameraShot::TacticalTopDown => (Vec3::new(look.x * 0.6, 21.0, 4.0), look, 52.0, 2.2, 2.8),
        CameraShot::CinemaLowHero => (
            Vec3::new(look.x - 6.0, 2.4, look.z + 7.0),
            look + Vec3::Y * 0.2,
            48.0,
            2.4,
            3.0,
        ),
        CameraShot::LayupGather => {
            let mid = actor.lerp(hoop, 0.28);
            (
                mid + Vec3::new(0.0, 3.6, 7.2),
                hoop.lerp(actor, 0.45) + Vec3::Y * 0.4,
                44.0,
                4.2,
                5.0,
            )
        }
        CameraShot::DunkTakeoff => (
            actor + Vec3::new(-2.1, 1.65, 4.6),
            actor.lerp(hoop, 0.35) + Vec3::Y * 1.35,
            40.0,
            5.4,
            6.0,
        ),
        CameraShot::PosterCine => (
            hoop + Vec3::new(-hoop_sign * 3.4, 1.45, 5.4),
            hoop + Vec3::Y * 0.15,
            36.0,
            5.8,
            6.5,
        ),
        CameraShot::NetSnap => (
            hoop + Vec3::new(-hoop_sign * 1.7, 3.35, 2.35),
            hoop,
            34.0,
            8.0,
            9.0,
        ),
        CameraShot::ReplayEnglish => {
            let a = t * 0.85;
            (
                hoop + Vec3::new(a.cos() * 6.4, 4.6 + a.sin() * 0.35, a.sin() * 6.4),
                hoop.lerp(ball, 0.25) + Vec3::Y * 0.2,
                42.0,
                1.7,
                2.0,
            )
        }
        CameraShot::CelebrateHold => (
            actor + Vec3::new(-3.4, 2.25, 5.4),
            actor + Vec3::Y * 1.25,
            45.0,
            2.5,
            3.0,
        ),
        CameraShot::StealPunch => (
            actor + Vec3::new(1.4, 2.05, 3.9),
            actor + Vec3::Y * 1.0,
            38.0,
            6.5,
            7.0,
        ),
        CameraShot::BlockReject => (
            actor + Vec3::new(2.1, 1.35, 3.15),
            actor + Vec3::Y * 2.05,
            40.0,
            5.6,
            6.2,
        ),
        CameraShot::BuzzerBeat => (
            Vec3::new(look.x * 0.5, 6.6, 11.4),
            look + Vec3::Y * 0.5,
            46.0,
            3.6,
            4.2,
        ),
        CameraShot::ThreePointWide => (
            Vec3::new(look.x * 0.4, 8.2, 13.6),
            look + Vec3::Y * 0.8,
            52.0,
            2.5,
            3.0,
        ),
        CameraShot::LogoHalfCourt => (
            Vec3::new(look.x * 0.3, 9.6, 14.6),
            look + Vec3::Y * 1.0,
            54.0,
            2.3,
            2.8,
        ),
        CameraShot::InboundBaseline => (
            Vec3::new(actor.x.signum() * 16.2, 5.6, actor.z * 0.25),
            actor + Vec3::Y * 1.1,
            50.0,
            3.0,
            3.4,
        ),
        CameraShot::FastBreakWide => (
            Vec3::new(look.x * 0.55 - actor.x.signum() * 1.5, 7.2, 12.6),
            look + Vec3::Y * 0.4,
            50.0,
            3.2,
            3.6,
        ),
        CameraShot::HoopCamOut => (
            hoop + Vec3::new(-hoop_sign * 3.2, 2.4, 2.4),
            ball,
            48.0,
            4.8,
            5.2,
        ),
        CameraShot::MenuOrbit => (
            Vec3::new(t.sin() * 18.0, 9.0, t.cos() * 18.0),
            Vec3::new(0.0, 0.6, 0.0),
            48.0,
            1.8,
            2.2,
        ),
    };
    ShotFrame {
        pos: clamp_cam(pos),
        look: look_at,
        fov_rad: fov.to_radians(),
        pos_lambda: pos_l,
        rot_lambda: rot_l,
    }
}

fn tick_post_fx(fx: &mut CameraPostFx, shot: CameraShot, dt: f32) {
    let target_lb = match shot {
        CameraShot::NetSnap => 0.85,
        CameraShot::ReplayEnglish | CameraShot::PosterCine => 0.7,
        CameraShot::CelebrateHold | CameraShot::BuzzerBeat => 0.55,
        CameraShot::DunkTakeoff => 0.42,
        CameraShot::StealPunch | CameraShot::BlockReject => 0.28,
        CameraShot::MenuOrbit => 0.12,
        _ => 0.0,
    };
    let k = 1.0 - (-6.0 * dt).exp();
    fx.letterbox = (fx.letterbox + (target_lb - fx.letterbox) * k).clamp(0.0, 1.0);
    fx.crowd_flash = (fx.crowd_flash - dt * 1.8).clamp(0.0, 1.0);
    fx.shake = (fx.shake - dt * 2.4).max(0.0);
}

fn smooth_cam(
    tf: &mut Transform,
    proj: &mut Projection,
    desired: Vec3,
    look: Vec3,
    fov_rad: f32,
    dt: f32,
    pos_lambda: f32,
    rot_lambda: f32,
    shake: f32,
    elapsed: f32,
) {
    let pos_k = 1.0 - (-pos_lambda * dt).exp();
    let rot_k = 1.0 - (-rot_lambda * dt).exp();
    let fov_k = 1.0 - (-3.4 * dt).exp();

    tf.translation = tf.translation.lerp(desired, pos_k);
    let shaken = if shake > 0.01 {
        Vec3::new(
            (elapsed * 47.0).sin() * shake * 0.09,
            (elapsed * 61.0).cos() * shake * 0.055,
            (elapsed * 53.0).sin() * shake * 0.07,
        )
    } else {
        Vec3::ZERO
    };
    tf.translation = clamp_cam(tf.translation + shaken);

    let aimed = Transform::from_translation(tf.translation).looking_at(look, Vec3::Y);
    tf.rotation = tf.rotation.slerp(aimed.rotation, rot_k);

    if let Projection::Perspective(p) = proj {
        p.fov = p.fov + (fov_rad - p.fov) * fov_k;
    }
}

fn clamp_cam(mut p: Vec3) -> Vec3 {
    p.y = p.y.clamp(FLOOR_Y, CEIL_Y);
    p.z = p.z.clamp(-MAX_ABS_Z, MAX_ABS_Z);
    p
}

fn court_dist(a: Vec3, hoop: Vec3) -> f32 {
    Vec2::new(a.x - hoop.x, a.z - hoop.z).length()
}

fn nearest_hoop(
    rims: &Query<(&Transform, &RimMarker), (Without<GameCam>, Without<Ball>, Without<Player>)>,
    near: Vec3,
) -> Vec3 {
    let mut best = fallback_hoop(near);
    let mut best_d = f32::MAX;
    for (tf, _) in rims.iter() {
        let d = tf.translation.distance(near);
        if d < best_d {
            best_d = d;
            best = tf.translation;
        }
    }
    best
}

fn hoop_for_side(
    rims: &Query<(&Transform, &RimMarker), (Without<GameCam>, Without<Ball>, Without<Player>)>,
    side: Side,
) -> Vec3 {
    // Home attacks the away rim (+X); away attacks the home rim (−X).
    let want_home_rim = matches!(side, Side::Away);
    for (tf, rim) in rims.iter() {
        if rim.home_side == want_home_rim {
            return tf.translation;
        }
    }
    fallback_hoop_for_side(side)
}

fn fallback_hoop(near: Vec3) -> Vec3 {
    let home = Vec3::new(-HOOP_X, RIM_HEIGHT, 0.0);
    let away = Vec3::new(HOOP_X, RIM_HEIGHT, 0.0);
    if near.distance(home) < near.distance(away) {
        home
    } else {
        away
    }
}

fn fallback_hoop_for_side(side: Side) -> Vec3 {
    match side {
        Side::Home => Vec3::new(HOOP_X, RIM_HEIGHT, 0.0),
        Side::Away => Vec3::new(-HOOP_X, RIM_HEIGHT, 0.0),
    }
}
