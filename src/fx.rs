use std::collections::HashMap;

use bevy::prelude::*;

use crate::ball::{
    BackboardHitEvent, Ball, BallState, BallVel, BucketEvent, FloorBounceEvent, Hold, RimHitEvent,
};
use crate::camera::{CameraPostFx, GameCam};
use crate::court::Hoop;
use crate::gameplay::{CutSqueak, DribbleTickEvent, StealEvent};
use crate::roster::Side;
use crate::sim::{HOOP_X, RIM_HEIGHT};
use crate::states::{AppState, Paused};
use crate::units::{spawn_digit, spawn_symbol, Heat, Player, Pose};

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenJuice>()
            .init_resource::<JuiceClock>()
            .init_resource::<FxRng>()
            .add_systems(Startup, setup_fx_assets)
            .add_systems(
                Update,
                (
                    (
                        spawn_on_bucket,
                        spawn_ball_trail,
                        spawn_afterimages,
                        pose_transition_fx,
                        cut_dust,
                        steal_fx,
                        rim_fx,
                        board_fx,
                        bounce_dust,
                        dribble_fx,
                        fire_aura,
                    ),
                    (
                        age_particles,
                        animate_flames,
                        shake_rims,
                        age_screen_juice,
                        apply_hitstop,
                    ),
                )
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            )
            .add_systems(OnExit(AppState::Playing), restore_time_scale);
    }
}

/// Flash / hitstop juice for buckets. CameraPostFx also receives crowd_flash + shake.
#[derive(Resource, Default)]
pub struct ScreenJuice {
    pub flash: f32,
    pub hitstop: f32,
}

#[derive(Resource, Default)]
struct JuiceClock {
    ball: f32,
    after: f32,
    streak: f32,
    ember: f32,
}

/// Tiny deterministic RNG so the effects never need an external crate.
#[derive(Resource)]
pub struct FxRng(pub u32);

impl Default for FxRng {
    fn default() -> Self {
        Self(0x2545_F491)
    }
}

impl FxRng {
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 16_777_216.0
    }

    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }

    /// Unit-ish vector on the XZ plane.
    pub fn dir_xz(&mut self) -> Vec3 {
        let a = self.range(0.0, std::f32::consts::TAU);
        Vec3::new(a.cos(), 0.0, a.sin())
    }
}

#[derive(Resource)]
struct FxAssets {
    sphere: Handle<Mesh>,
    cube: Handle<Mesh>,
    quad: Handle<Mesh>,
    cone: Handle<Mesh>,
    ring: Handle<Mesh>,
    disc: Handle<Mesh>,
    capsule: Handle<Mesh>,
    trail: Handle<StandardMaterial>,
    trail_hot: Handle<StandardMaterial>,
    ghost: Handle<StandardMaterial>,
    fire: Handle<StandardMaterial>,
    flame: Handle<StandardMaterial>,
    ember: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
    spark: Handle<StandardMaterial>,
    spark_cyan: Handle<StandardMaterial>,
    sweat: Handle<StandardMaterial>,
    white: Handle<StandardMaterial>,
    streak: Handle<StandardMaterial>,
    shimmer: Handle<StandardMaterial>,
    net_flash: Handle<StandardMaterial>,
    /// [home, away]
    team_ring: [Handle<StandardMaterial>; 2],
    team_score: [Handle<StandardMaterial>; 2],
    /// [home, away] × [primary, secondary]
    confetti: [[Handle<StandardMaterial>; 2]; 2],
}

/// One short-lived visual. Everything here is integrated by `age_particles`; the
/// entity is despawned when `life` runs out so nothing can leak.
#[derive(Component, Clone)]
pub struct Particle {
    pub life: f32,
    pub max: f32,
    pub vel: Vec3,
    pub gravity: f32,
    pub drag: f32,
    pub spin: Vec3,
    pub start: Vec3,
    pub end: Vec3,
    /// Fraction of the lifetime at the end over which the scale collapses to zero.
    pub tail: f32,
    /// Stop dead when reaching this height (confetti settling on the floor).
    pub floor: Option<f32>,
    /// Turn to face the camera each frame (score pops).
    pub billboard: bool,
}

impl Particle {
    pub fn new(life: f32, start: Vec3, end: Vec3) -> Self {
        Self {
            life,
            max: life,
            vel: Vec3::ZERO,
            gravity: 0.0,
            drag: 0.0,
            spin: Vec3::ZERO,
            start,
            end,
            tail: 0.0,
            floor: None,
            billboard: false,
        }
    }

    pub fn vel(mut self, v: Vec3) -> Self {
        self.vel = v;
        self
    }

    pub fn gravity(mut self, g: f32) -> Self {
        self.gravity = g;
        self
    }

    pub fn drag(mut self, d: f32) -> Self {
        self.drag = d;
        self
    }

    pub fn spin(mut self, s: Vec3) -> Self {
        self.spin = s;
        self
    }

    pub fn tail(mut self, t: f32) -> Self {
        self.tail = t;
        self
    }

    pub fn floor(mut self, y: f32) -> Self {
        self.floor = Some(y);
        self
    }

    pub fn billboard(mut self) -> Self {
        self.billboard = true;
        self
    }

    /// Scale for a particle at normalised age `k` (0 = born, 1 = dead).
    pub fn scale_at(&self, k: f32) -> Vec3 {
        let k = k.clamp(0.0, 1.0);
        let base = self.start.lerp(self.end, k);
        if self.tail > 0.0 && k > 1.0 - self.tail {
            base * ((1.0 - k) / self.tail).clamp(0.0, 1.0)
        } else {
            base
        }
    }
}

/// Persistent flame around an on-fire player; animated by `animate_flames`.
#[derive(Component)]
struct Flame {
    phase: f32,
    radius: f32,
    angle: f32,
    ring: bool,
}

#[derive(Component)]
struct FireAura {
    owner: Entity,
}

struct RimShake {
    base: Vec3,
    amp: f32,
    t: f32,
}

fn glow(c: Color, e: f32, alpha: f32) -> StandardMaterial {
    let lin = c.to_linear();
    StandardMaterial {
        base_color: c.with_alpha(alpha),
        emissive: LinearRgba::from(lin) * e,
        unlit: true,
        alpha_mode: if alpha < 1.0 {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        ..default()
    }
}

fn additive(c: Color, e: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: c,
        emissive: LinearRgba::from(c.to_linear()) * e,
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        double_sided: true,
        ..default()
    }
}

fn setup_fx_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let team_mat = |materials: &mut Assets<StandardMaterial>, side: Side, e: f32| {
        materials.add(additive(side.primary(), e))
    };
    let score_mat = |materials: &mut Assets<StandardMaterial>, side: Side| {
        materials.add(glow(side.primary(), 4.0, 1.0))
    };
    let confetti_mat = |materials: &mut Assets<StandardMaterial>, c: Color| {
        materials.add(StandardMaterial {
            base_color: c,
            emissive: LinearRgba::from(c.to_linear()) * 1.2,
            unlit: true,
            cull_mode: None,
            double_sided: true,
            ..default()
        })
    };
    commands.insert_resource(FxAssets {
        sphere: meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap_or_else(|_| Sphere::new(1.0).mesh().uv(12, 8))),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        cone: meshes.add(Cone {
            radius: 0.5,
            height: 1.0,
        }),
        ring: meshes.add(Torus {
            minor_radius: 0.035,
            major_radius: 1.0,
        }),
        disc: meshes.add(Cylinder::new(1.0, 0.01)),
        capsule: meshes.add(Capsule3d::new(0.22, 1.05)),
        trail: materials.add(glow(Color::srgb(1.0, 0.55, 0.18), 3.6, 1.0)),
        trail_hot: materials.add(glow(Color::srgb(1.0, 0.3, 0.05), 6.0, 1.0)),
        ghost: materials.add(glow(Color::srgb(0.65, 0.9, 1.0), 0.5, 0.32)),
        fire: materials.add(glow(Color::srgb(1.0, 0.42, 0.08), 1.4, 0.38)),
        flame: materials.add(additive(Color::srgb(1.0, 0.38, 0.06), 2.6)),
        ember: materials.add(glow(Color::srgb(1.0, 0.75, 0.2), 5.0, 1.0)),
        dust: materials.add(glow(Color::srgb(0.78, 0.72, 0.62), 0.15, 0.55)),
        spark: materials.add(glow(Color::srgb(1.0, 0.85, 0.35), 5.0, 1.0)),
        spark_cyan: materials.add(glow(Color::srgb(0.35, 0.95, 1.0), 5.0, 1.0)),
        sweat: materials.add(glow(Color::srgb(0.7, 0.9, 1.0), 1.5, 0.9)),
        white: materials.add(additive(Color::WHITE, 3.0)),
        streak: materials.add(additive(Color::srgb(0.8, 0.95, 1.0), 2.0)),
        shimmer: materials.add(additive(Color::srgb(0.6, 0.9, 1.0), 1.6)),
        net_flash: materials.add(additive(Color::srgb(0.9, 1.0, 1.0), 3.5)),
        team_ring: [
            team_mat(&mut materials, Side::Home, 3.0),
            team_mat(&mut materials, Side::Away, 3.0),
        ],
        team_score: [
            score_mat(&mut materials, Side::Home),
            score_mat(&mut materials, Side::Away),
        ],
        confetti: [
            [
                confetti_mat(&mut materials, Side::Home.primary()),
                confetti_mat(&mut materials, Side::Home.secondary()),
            ],
            [
                confetti_mat(&mut materials, Side::Away.primary()),
                confetti_mat(&mut materials, Color::srgb(0.9, 0.85, 1.0)),
            ],
        ],
    });
}

fn side_index(side: Side) -> usize {
    match side {
        Side::Home => 0,
        Side::Away => 1,
    }
}

fn spawn_particle(
    commands: &mut Commands,
    mesh: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    pos: Vec3,
    rot: Quat,
    p: Particle,
) -> Entity {
    let scale = p.start;
    commands
        .spawn((
            p,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: pos,
                rotation: rot,
                scale,
            },
            DespawnOnExit(AppState::Playing),
        ))
        .id()
}

/// Flat ring lying on the floor (torus axis = Y).
fn floor_ring(commands: &mut Commands, fx: &FxAssets, mat: &Handle<StandardMaterial>, pos: Vec3, from: f32, to: f32, life: f32) {
    spawn_particle(
        commands,
        &fx.ring,
        mat,
        pos,
        Quat::IDENTITY,
        Particle::new(life, Vec3::new(from, 1.0, from), Vec3::new(to, 0.4, to)).tail(0.6),
    );
}

fn burst(
    commands: &mut Commands,
    rng: &mut FxRng,
    mesh: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    pos: Vec3,
    count: usize,
    speed: f32,
    up: f32,
    size: f32,
    life: f32,
    gravity: f32,
) {
    for _ in 0..count {
        let d = rng.dir_xz() * rng.range(0.4, 1.0) * speed + Vec3::Y * rng.range(0.2, 1.0) * up;
        spawn_particle(
            commands,
            mesh,
            mat,
            pos + Vec3::new(rng.range(-0.05, 0.05), rng.range(-0.05, 0.05), rng.range(-0.05, 0.05)),
            Quat::from_rotation_y(rng.range(0.0, 6.28)),
            Particle::new(
                life * rng.range(0.6, 1.0),
                Vec3::splat(size),
                Vec3::splat(size * 0.2),
            )
            .vel(d)
            .gravity(gravity)
            .drag(1.5)
            .spin(Vec3::new(rng.range(-8.0, 8.0), rng.range(-8.0, 8.0), 0.0)),
        );
    }
}

fn dust_puff(commands: &mut Commands, rng: &mut FxRng, fx: &FxAssets, pos: Vec3, count: usize, size: f32) {
    for _ in 0..count {
        let d = rng.dir_xz() * rng.range(0.3, 1.1) + Vec3::Y * rng.range(0.3, 0.9);
        spawn_particle(
            commands,
            &fx.sphere,
            &fx.dust,
            pos + Vec3::new(rng.range(-0.15, 0.15), 0.04, rng.range(-0.15, 0.15)),
            Quat::IDENTITY,
            Particle::new(rng.range(0.35, 0.55), Vec3::splat(size * 0.5), Vec3::splat(size * 1.6))
                .vel(d)
                .drag(3.0)
                .tail(0.5),
        );
    }
}

fn spawn_on_bucket(
    mut commands: Commands,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut buckets: MessageReader<BucketEvent>,
    mut juice: ResMut<ScreenJuice>,
    mut cam_fx: ResMut<CameraPostFx>,
    players: Query<&Player>,
) {
    for ev in buckets.read() {
        let big = ev.dunk || ev.is_three;
        juice.flash = if ev.dunk { 1.0 } else if ev.is_three { 0.7 } else { 0.55 };
        juice.hitstop = if ev.dunk { 0.14 } else { 0.06 };
        cam_fx.crowd_flash = cam_fx.crowd_flash.max(if ev.dunk { 1.0 } else { 0.55 });
        cam_fx.shake = cam_fx.shake.max(if ev.dunk { 0.85 } else { 0.4 });

        let side = ev
            .shooter
            .and_then(|e| players.get(e).ok())
            .map(|p| p.side)
            .unwrap_or(if ev.hoop_home { Side::Away } else { Side::Home });
        let si = side_index(side);
        let hoop_x = if ev.hoop_home { -HOOP_X } else { HOOP_X };
        let rim = Vec3::new(hoop_x, RIM_HEIGHT, 0.0);
        let toward_court = if ev.hoop_home { 1.0 } else { -1.0 };

        // Net flash: inverted cone of light dropping through the net.
        spawn_particle(
            &mut commands,
            &fx.cone,
            &fx.net_flash,
            rim + Vec3::new(0.0, -0.3, 0.0),
            Quat::from_rotation_x(std::f32::consts::PI),
            Particle::new(0.3, Vec3::new(0.5, 0.5, 0.5), Vec3::new(0.7, 0.9, 0.7))
                .vel(Vec3::new(0.0, -1.5, 0.0))
                .tail(0.7),
        );
        // Radial shock ring in the rim plane.
        spawn_particle(
            &mut commands,
            &fx.ring,
            &fx.white,
            rim,
            Quat::IDENTITY,
            Particle::new(0.35, Vec3::new(0.25, 1.0, 0.25), Vec3::new(2.2, 0.5, 2.2)).tail(0.6),
        );
        // Floor light ring under the hoop in team colour.
        floor_ring(
            &mut commands,
            &fx,
            &fx.team_ring[si],
            Vec3::new(hoop_x, 0.03, 0.0),
            0.3,
            if big { 3.2 } else { 2.2 },
            if big { 0.7 } else { 0.5 },
        );
        // Sparks off the rim.
        burst(
            &mut commands,
            &mut rng,
            &fx.cube,
            &fx.spark,
            rim,
            if big { 22 } else { 12 },
            2.6,
            3.2,
            0.07,
            0.7,
            7.0,
        );
        // Floating "+N!" rising above the hoop.
        let pts = if ev.dunk {
            2
        } else if ev.is_three {
            3
        } else {
            2
        };
        let pop_scale = if big { 3.0 } else { 2.2 };
        commands
            .spawn((
                Particle::new(
                    1.3,
                    Vec3::splat(pop_scale * 0.6),
                    Vec3::splat(pop_scale * 1.15),
                )
                .vel(Vec3::new(toward_court * 0.3, 1.1, 0.0))
                .tail(0.3)
                .billboard(),
                Transform::from_translation(rim + Vec3::new(toward_court * 0.6, 0.75, 0.0)),
                Visibility::default(),
                DespawnOnExit(AppState::Playing),
            ))
            .with_children(|c| {
                let mat = &fx.team_score[si];
                spawn_symbol(c, &fx.cube, mat, '+', Vec3::new(-0.1, 0.0, 0.0));
                spawn_digit(c, &fx.cube, mat, pts, Vec3::ZERO, false);
                spawn_symbol(c, &fx.cube, mat, '!', Vec3::new(0.085, 0.0, 0.0));
            });
        // Confetti for threes and dunks: team-coloured quads with gravity and spin.
        if big {
            let n = if ev.dunk { 44 } else { 36 };
            for i in 0..n {
                let mat = &fx.confetti[si][i % 2];
                let d = rng.dir_xz() * rng.range(1.0, 3.8) + Vec3::Y * rng.range(2.5, 5.5);
                spawn_particle(
                    &mut commands,
                    &fx.quad,
                    mat,
                    rim + Vec3::new(0.0, 0.3, 0.0),
                    Quat::from_euler(EulerRot::XYZ, rng.range(0.0, 6.28), rng.range(0.0, 6.28), 0.0),
                    Particle::new(rng.range(1.5, 2.2), Vec3::new(0.1, 0.06, 1.0), Vec3::new(0.1, 0.06, 1.0))
                        .vel(d)
                        .gravity(5.5)
                        .drag(1.4)
                        .spin(Vec3::new(rng.range(-9.0, 9.0), rng.range(-9.0, 9.0), rng.range(-6.0, 6.0)))
                        .floor(0.02)
                        .tail(0.25),
                );
            }
        }
    }
}

fn spawn_ball_trail(
    mut commands: Commands,
    time: Res<Time>,
    paused: Res<Paused>,
    fx: Res<FxAssets>,
    mut clock: ResMut<JuiceClock>,
    heats: Query<&Heat>,
    ball: Query<(&Transform, &BallVel, &BallState), With<Ball>>,
) {
    if paused.0 {
        return;
    }
    let Ok((tf, vel, state)) = ball.single() else {
        return;
    };
    let hot_shooter = state
        .shooter
        .and_then(|e| heats.get(e).ok())
        .map(|h| h.on_fire())
        .unwrap_or(false);
    let flying = vel.0.length() > 6.0 || state.hold == Hold::Shot;
    if !flying {
        clock.ball = 0.03;
        return;
    }
    clock.ball += time.delta_secs();
    if clock.ball < if hot_shooter { 0.02 } else { 0.03 } {
        return;
    }
    clock.ball = 0.0;
    let speed = vel.0.length();
    let scale = (0.055 + (speed * 0.004).min(0.04)).max(0.05) * if hot_shooter { 1.5 } else { 1.0 };
    let mat = if hot_shooter { &fx.trail_hot } else { &fx.trail };
    spawn_particle(
        &mut commands,
        &fx.sphere,
        mat,
        tf.translation,
        Quat::IDENTITY,
        Particle::new(if hot_shooter { 0.4 } else { 0.28 }, Vec3::splat(scale), Vec3::ZERO)
            .vel(if hot_shooter { Vec3::Y * 0.8 } else { Vec3::ZERO }),
    );
}

fn spawn_afterimages(
    mut commands: Commands,
    time: Res<Time>,
    paused: Res<Paused>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut clock: ResMut<JuiceClock>,
    players: Query<(&Transform, &Pose, &Heat, &crate::units::MoveVel), With<Player>>,
) {
    if paused.0 {
        return;
    }
    let hot = players.iter().any(|(_, pose, heat, _)| {
        *pose == Pose::Sprint || (heat.on_fire() && matches!(*pose, Pose::Run | Pose::Sprint))
    });
    if !hot {
        return;
    }
    let dt = time.delta_secs();
    clock.after += dt;
    clock.streak += dt;
    let ghosts = clock.after >= 0.08;
    let streaks = clock.streak >= 0.045;
    if ghosts {
        clock.after = 0.0;
    }
    if streaks {
        clock.streak = 0.0;
    }
    for (tf, pose, heat, vel) in &players {
        let fire = heat.on_fire() && matches!(*pose, Pose::Run | Pose::Sprint | Pose::Shoot);
        if *pose != Pose::Sprint && !fire {
            continue;
        }
        if ghosts {
            spawn_particle(
                &mut commands,
                &fx.capsule,
                if fire { &fx.fire } else { &fx.ghost },
                tf.translation + Vec3::Y * 0.95 * tf.scale.y,
                tf.rotation,
                Particle::new(if fire { 0.22 } else { 0.15 }, tf.scale * 0.95, tf.scale * 0.25),
            );
        }
        // Anime speed lines trailing behind a sprinter.
        if streaks && *pose == Pose::Sprint {
            let v = Vec3::new(vel.0.x, 0.0, vel.0.z);
            if v.length_squared() > 1.0 {
                let dir = v.normalize();
                let right = dir.cross(Vec3::Y);
                for _ in 0..2 {
                    let len = rng.range(0.5, 1.1);
                    let pos = tf.translation
                        + Vec3::Y * rng.range(0.4, 1.7) * tf.scale.y
                        + right * rng.range(-0.55, 0.55)
                        - dir * (0.4 + len * 0.5);
                    spawn_particle(
                        &mut commands,
                        &fx.cube,
                        &fx.streak,
                        pos,
                        Quat::from_rotation_arc(Vec3::Z, dir),
                        Particle::new(0.18, Vec3::new(0.02, 0.02, len), Vec3::new(0.004, 0.004, len * 1.4))
                            .vel(-dir * 2.0),
                    );
                }
            }
        }
    }
}

/// Watches pose changes for events gameplay never emits explicitly: block contests,
/// dunk take-offs, sprint starts and landings.
fn pose_transition_fx(
    mut commands: Commands,
    paused: Res<Paused>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut juice: ResMut<ScreenJuice>,
    mut cam_fx: ResMut<CameraPostFx>,
    mut prev: Local<HashMap<Entity, Pose>>,
    cam: Query<&Transform, With<GameCam>>,
    players: Query<(Entity, &Transform, &Pose, &crate::units::MoveVel), With<Player>>,
) {
    if paused.0 {
        return;
    }
    let cam_tf = cam.single().ok().copied();
    for (e, tf, pose, vel) in &players {
        let old = prev.insert(e, *pose);
        let Some(old) = old else {
            continue;
        };
        if old == *pose {
            continue;
        }
        let feet = Vec3::new(tf.translation.x, 0.03, tf.translation.z);
        let s = tf.scale.y;
        // landing after any jump pose
        if matches!(old, Pose::Shoot | Pose::Dunk | Pose::Block) {
            dust_puff(&mut commands, &mut rng, &fx, feet, if old == Pose::Dunk { 8 } else { 5 }, 0.13);
        }
        match *pose {
            Pose::Block => {
                let hands = tf.translation + Vec3::Y * 2.35 * s;
                burst(&mut commands, &mut rng, &fx.cube, &fx.spark, hands, 12, 2.8, 2.0, 0.055, 0.5, 6.0);
                spawn_particle(
                    &mut commands,
                    &fx.ring,
                    &fx.white,
                    hands,
                    Quat::IDENTITY,
                    Particle::new(0.25, Vec3::new(0.15, 1.0, 0.15), Vec3::new(1.3, 0.4, 1.3)).tail(0.6),
                );
                dust_puff(&mut commands, &mut rng, &fx, feet, 3, 0.1);
            }
            Pose::Dunk => {
                // Manga impact frame: white flash + radial streaks in the camera plane.
                juice.flash = juice.flash.max(0.4);
                cam_fx.crowd_flash = cam_fx.crowd_flash.max(0.35);
                cam_fx.shake = cam_fx.shake.max(0.25);
                let centre = tf.translation + Vec3::Y * 1.3 * s;
                if let Some(cam_tf) = cam_tf {
                    let right = cam_tf.right().as_vec3();
                    let up = cam_tf.up().as_vec3();
                    for i in 0..18 {
                        let a = i as f32 / 18.0 * std::f32::consts::TAU + rng.range(-0.1, 0.1);
                        let dir = right * a.cos() + up * a.sin();
                        let len = rng.range(0.7, 1.3);
                        spawn_particle(
                            &mut commands,
                            &fx.cube,
                            &fx.white,
                            centre + dir * (1.0 + len * 0.5),
                            Quat::from_rotation_arc(Vec3::Y, dir),
                            Particle::new(0.22, Vec3::new(0.035, len, 0.035), Vec3::new(0.005, len * 1.8, 0.005))
                                .vel(dir * 7.0),
                        );
                    }
                }
                dust_puff(&mut commands, &mut rng, &fx, feet, 6, 0.14);
            }
            Pose::Sprint => {
                // Speed-line ring punched through as the sprint starts.
                let v = Vec3::new(vel.0.x, 0.0, vel.0.z);
                let dir = if v.length_squared() > 0.1 {
                    v.normalize()
                } else {
                    tf.forward().as_vec3()
                };
                spawn_particle(
                    &mut commands,
                    &fx.ring,
                    &fx.streak,
                    tf.translation + Vec3::Y * 1.0 * s - dir * 0.3,
                    Quat::from_rotation_arc(Vec3::Y, dir),
                    Particle::new(0.28, Vec3::new(0.35, 1.0, 0.35), Vec3::new(1.5, 0.3, 1.5))
                        .vel(-dir * 2.5)
                        .tail(0.5),
                );
                dust_puff(&mut commands, &mut rng, &fx, feet, 3, 0.11);
            }
            Pose::Shoot => {
                dust_puff(&mut commands, &mut rng, &fx, feet, 2, 0.09);
            }
            _ => {}
        }
    }
    prev.retain(|e, _| players.get(*e).is_ok());
}

fn cut_dust(
    mut commands: Commands,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut cuts: MessageReader<CutSqueak>,
) {
    for ev in cuts.read() {
        let feet = Vec3::new(ev.pos.x, 0.03, ev.pos.z);
        dust_puff(&mut commands, &mut rng, &fx, feet, 5, 0.12);
    }
}

fn steal_fx(
    mut commands: Commands,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut cam_fx: ResMut<CameraPostFx>,
    mut steals: MessageReader<StealEvent>,
) {
    for ev in steals.read() {
        let hands = ev.pos + Vec3::Y * 1.0;
        if ev.success {
            cam_fx.shake = cam_fx.shake.max(0.3);
            burst(&mut commands, &mut rng, &fx.cube, &fx.spark_cyan, hands, 16, 3.0, 2.5, 0.06, 0.55, 6.0);
            spawn_particle(
                &mut commands,
                &fx.ring,
                &fx.white,
                hands,
                Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                Particle::new(0.3, Vec3::new(0.2, 1.0, 0.2), Vec3::new(1.6, 0.4, 1.6)).tail(0.6),
            );
        }
        // Sweat flies off whoever lost the handle.
        burst(
            &mut commands,
            &mut rng,
            &fx.sphere,
            &fx.sweat,
            hands + Vec3::Y * 0.5,
            if ev.success { 9 } else { 5 },
            1.6,
            2.4,
            0.035,
            0.6,
            9.0,
        );
    }
}

fn rim_fx(
    mut commands: Commands,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut rims: MessageReader<RimHitEvent>,
) {
    for ev in rims.read() {
        let n = (ev.speed * 1.6).clamp(5.0, 16.0) as usize;
        burst(&mut commands, &mut rng, &fx.cube, &fx.spark, ev.pos, n, 2.2, 2.6, 0.05, 0.45, 8.0);
        // Iron ring flash around the contact point.
        spawn_particle(
            &mut commands,
            &fx.ring,
            &fx.white,
            ev.pos,
            Quat::IDENTITY,
            Particle::new(0.2, Vec3::new(0.08, 1.0, 0.08), Vec3::new(0.5, 0.4, 0.5)).tail(0.6),
        );
    }
}

#[derive(Default)]
struct RimShakes(HashMap<Entity, RimShake>);

/// Rattles the nearest rim on a hit; the rim always returns exactly to its rest spot.
fn shake_rims(
    time: Res<Time>,
    mut rims: MessageReader<RimHitEvent>,
    mut shakes: Local<RimShakes>,
    mut hoops: Query<(Entity, &mut Transform), With<Hoop>>,
) {
    let dt = time.delta_secs();
    for ev in rims.read() {
        let mut best: Option<(Entity, f32, Vec3)> = None;
        for (e, tf) in &hoops {
            let d = tf.translation.distance(ev.pos);
            if best.map(|b| d < b.1).unwrap_or(true) {
                best = Some((e, d, tf.translation));
            }
        }
        if let Some((e, d, pos)) = best {
            if d < 1.5 {
                let existing = shakes.0.get(&e).map(|s| s.base).unwrap_or(pos);
                let amp = (ev.speed * 0.006).clamp(0.012, 0.045);
                let entry = shakes.0.entry(e).or_insert(RimShake {
                    base: existing,
                    amp: 0.0,
                    t: 0.0,
                });
                entry.amp = entry.amp.max(amp);
                entry.t = 0.0;
            }
        }
    }
    let mut done = Vec::new();
    for (e, s) in shakes.0.iter_mut() {
        s.t += dt;
        let decay = (-s.t * 7.0).exp();
        let Ok((_, mut tf)) = hoops.get_mut(*e) else {
            done.push(*e);
            continue;
        };
        if s.amp * decay < 0.0006 {
            tf.translation = s.base;
            done.push(*e);
            continue;
        }
        let a = s.amp * decay;
        tf.translation = s.base
            + Vec3::new(
                (s.t * 58.0).sin() * a,
                (s.t * 74.0).sin() * a * 0.55,
                (s.t * 49.0).cos() * a,
            );
    }
    for e in done {
        shakes.0.remove(&e);
    }
}

fn board_fx(
    mut commands: Commands,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut boards: MessageReader<BackboardHitEvent>,
) {
    for ev in boards.read() {
        let home = ev.pos.x < 0.0;
        let board_x = if home { -HOOP_X - 0.42 } else { HOOP_X + 0.42 };
        let sign = if home { -1.0 } else { 1.0 };
        // Glass shimmer: a translucent sheet flashing over the board face.
        spawn_particle(
            &mut commands,
            &fx.quad,
            &fx.shimmer,
            Vec3::new(board_x - sign * 0.06, RIM_HEIGHT + 0.32, 0.0),
            Quat::from_rotation_y(-sign * std::f32::consts::FRAC_PI_2),
            Particle::new(0.3, Vec3::new(1.75, 1.0, 1.0), Vec3::new(1.95, 1.15, 1.0)).tail(1.0),
        );
        // Ripple ring on the glass around the impact point.
        spawn_particle(
            &mut commands,
            &fx.ring,
            &fx.white,
            Vec3::new(board_x - sign * 0.07, ev.pos.y, ev.pos.z),
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            Particle::new(0.3, Vec3::new(0.1, 1.0, 0.1), Vec3::new(0.7, 0.3, 0.7)).tail(0.6),
        );
        burst(&mut commands, &mut rng, &fx.cube, &fx.spark_cyan, ev.pos, 7, 1.6, 1.6, 0.04, 0.4, 7.0);
    }
}

fn bounce_dust(
    mut commands: Commands,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut bounces: MessageReader<FloorBounceEvent>,
) {
    for ev in bounces.read() {
        if ev.speed < 3.0 {
            continue;
        }
        let feet = Vec3::new(ev.pos.x, 0.03, ev.pos.z);
        floor_ring(&mut commands, &fx, &fx.dust, feet, 0.1, 0.5 + ev.speed * 0.03, 0.3);
        if ev.speed > 6.0 {
            dust_puff(&mut commands, &mut rng, &fx, feet, 3, 0.09);
        }
    }
}

fn dribble_fx(
    mut commands: Commands,
    fx: Res<FxAssets>,
    mut ticks: MessageReader<DribbleTickEvent>,
) {
    for ev in ticks.read() {
        let feet = Vec3::new(ev.pos.x, 0.025, ev.pos.z);
        spawn_particle(
            &mut commands,
            &fx.disc,
            &fx.dust,
            feet,
            Quat::IDENTITY,
            Particle::new(0.22, Vec3::new(0.12, 1.0, 0.12), Vec3::new(0.42, 1.0, 0.42)).tail(0.8),
        );
    }
}

/// Keeps a flame aura attached to every on-fire player and drops it when they cool.
fn fire_aura(
    mut commands: Commands,
    time: Res<Time>,
    paused: Res<Paused>,
    fx: Res<FxAssets>,
    mut rng: ResMut<FxRng>,
    mut clock: ResMut<JuiceClock>,
    mut auras: Query<(Entity, &FireAura, &mut Transform), Without<Player>>,
    players: Query<(Entity, &Transform, &Heat), With<Player>>,
) {
    if paused.0 {
        return;
    }
    let mut have: HashMap<Entity, Entity> = HashMap::new();
    for (aura_e, aura, mut tf) in &mut auras {
        match players.get(aura.owner) {
            Ok((_, ptf, heat)) if heat.on_fire() => {
                tf.translation = ptf.translation;
                tf.scale = ptf.scale;
                have.insert(aura.owner, aura_e);
            }
            _ => {
                commands.entity(aura_e).despawn();
            }
        }
    }
    clock.ember += time.delta_secs();
    let embers = clock.ember >= 0.05;
    if embers {
        clock.ember = 0.0;
    }
    for (e, ptf, heat) in &players {
        if !heat.on_fire() {
            continue;
        }
        if !have.contains_key(&e) {
            commands
                .spawn((
                    FireAura { owner: e },
                    Transform::from_translation(ptf.translation).with_scale(ptf.scale),
                    Visibility::default(),
                    DespawnOnExit(AppState::Playing),
                ))
                .with_children(|a| {
                    for i in 0..8 {
                        let angle = i as f32 / 8.0 * std::f32::consts::TAU;
                        a.spawn((
                            Flame {
                                phase: i as f32 * 0.8,
                                radius: 0.42 + (i % 2) as f32 * 0.1,
                                angle,
                                ring: false,
                            },
                            Mesh3d(fx.cone.clone()),
                            MeshMaterial3d(fx.flame.clone()),
                            Transform::from_xyz(angle.cos() * 0.45, 0.3, angle.sin() * 0.45)
                                .with_scale(Vec3::new(0.22, 0.5, 0.22)),
                        ));
                    }
                    a.spawn((
                        Flame {
                            phase: 0.0,
                            radius: 0.0,
                            angle: 0.0,
                            ring: true,
                        },
                        Mesh3d(fx.ring.clone()),
                        MeshMaterial3d(fx.flame.clone()),
                        Transform::from_xyz(0.0, 0.03, 0.0).with_scale(Vec3::new(0.6, 1.0, 0.6)),
                    ));
                });
        }
        if embers {
            let p = ptf.translation
                + Vec3::new(rng.range(-0.35, 0.35), rng.range(0.2, 1.6) * ptf.scale.y, rng.range(-0.35, 0.35));
            spawn_particle(
                &mut commands,
                &fx.cube,
                &fx.ember,
                p,
                Quat::from_rotation_y(rng.range(0.0, 6.28)),
                Particle::new(rng.range(0.4, 0.7), Vec3::splat(0.035), Vec3::splat(0.005))
                    .vel(Vec3::new(rng.range(-0.4, 0.4), rng.range(1.2, 2.4), rng.range(-0.4, 0.4)))
                    .spin(Vec3::new(4.0, 7.0, 0.0)),
            );
        }
    }
}

fn animate_flames(
    time: Res<Time>,
    paused: Res<Paused>,
    mut flames: Query<(&mut Flame, &mut Transform)>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (mut f, mut tf) in &mut flames {
        if f.ring {
            let pulse = 0.6 + (t * 6.0).sin() * 0.06;
            tf.scale = Vec3::new(pulse, 1.0, pulse);
            tf.rotation = Quat::from_rotation_y(t * 1.2);
            continue;
        }
        f.angle += dt * 1.6;
        let flicker = (t * 9.0 + f.phase).sin() * 0.5 + (t * 23.0 + f.phase * 2.0).sin() * 0.25;
        let h = 0.45 + flicker * 0.18;
        tf.translation = Vec3::new(
            f.angle.cos() * f.radius,
            0.25 + flicker * 0.1 + (t * 3.0 + f.phase).sin() * 0.08,
            f.angle.sin() * f.radius,
        );
        tf.scale = Vec3::new(0.2 + flicker.abs() * 0.05, h, 0.2 + flicker.abs() * 0.05);
        tf.rotation = Quat::from_rotation_y(-f.angle) * Quat::from_rotation_x(flicker * 0.25);
    }
}

fn age_particles(
    time: Res<Time>,
    paused: Res<Paused>,
    mut commands: Commands,
    cam: Query<&Transform, (With<GameCam>, Without<Particle>)>,
    mut q: Query<(Entity, &mut Particle, &mut Transform), Without<GameCam>>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let cam_pos = cam.single().ok().map(|t| t.translation);
    for (e, mut p, mut tf) in &mut q {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let k = 1.0 - p.life / p.max;
        if p.gravity != 0.0 {
            p.vel.y -= p.gravity * dt;
        }
        if p.drag != 0.0 {
            let f = (1.0 - p.drag * dt).max(0.0);
            p.vel *= f;
        }
        tf.translation += p.vel * dt;
        if let Some(fl) = p.floor {
            if tf.translation.y < fl {
                tf.translation.y = fl;
                p.vel = Vec3::ZERO;
                p.spin = Vec3::ZERO;
                p.gravity = 0.0;
            }
        }
        let w = p.spin.length();
        if w > 1e-4 {
            tf.rotate(Quat::from_axis_angle(p.spin / w, w * dt));
        }
        if p.billboard {
            if let Some(c) = cam_pos {
                // +Z faces the camera so glyphs built in the XY plane read correctly.
                let away = tf.translation * 2.0 - c;
                tf.look_at(away, Vec3::Y);
            }
        }
        tf.scale = p.scale_at(k);
    }
}

fn age_screen_juice(time: Res<Time<Real>>, paused: Res<Paused>, mut juice: ResMut<ScreenJuice>) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    juice.flash = (juice.flash - dt * 3.2).max(0.0);
    juice.hitstop = (juice.hitstop - dt * 1.15).max(0.0);
}

fn apply_hitstop(juice: Res<ScreenJuice>, paused: Res<Paused>, mut virt: ResMut<Time<Virtual>>) {
    if paused.0 {
        virt.set_relative_speed(0.0);
        return;
    }
    if juice.hitstop > 0.0 {
        virt.set_relative_speed(0.18);
    } else {
        virt.set_relative_speed(1.0);
    }
}

fn restore_time_scale(mut virt: ResMut<Time<Virtual>>) {
    virt.set_relative_speed(1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particle_scale_interpolates_and_collapses_in_tail() {
        let p = Particle::new(1.0, Vec3::splat(1.0), Vec3::splat(3.0)).tail(0.5);
        assert_eq!(p.scale_at(0.0), Vec3::splat(1.0));
        assert!((p.scale_at(0.5).x - 2.0).abs() < 1e-5);
        // Half-way through the tail the scale is halved.
        assert!((p.scale_at(0.75).x - 2.5 * 0.5).abs() < 1e-5);
        assert_eq!(p.scale_at(1.0), Vec3::ZERO);
        // Without a tail the end scale is reached.
        let q = Particle::new(1.0, Vec3::ONE, Vec3::splat(2.0));
        assert_eq!(q.scale_at(1.0), Vec3::splat(2.0));
        assert_eq!(q.scale_at(2.0), Vec3::splat(2.0));
    }

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let mut a = FxRng::default();
        let mut b = FxRng::default();
        for _ in 0..100 {
            let x = a.next();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x, b.next());
            let r = a.range(-2.0, 3.0);
            assert!((-2.0..=3.0).contains(&r));
            b.range(-2.0, 3.0);
            let d = a.dir_xz();
            b.dir_xz();
            assert!((d.length() - 1.0).abs() < 1e-4 && d.y == 0.0);
        }
    }

    #[test]
    fn side_index_maps_teams() {
        assert_eq!(side_index(Side::Home), 0);
        assert_eq!(side_index(Side::Away), 1);
    }

    /// Minimal headless world with the FX asset table and the resources the
    /// event-driven FX systems read.
    fn fx_world() -> World {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<FxRng>();
        world.init_resource::<ScreenJuice>();
        world.init_resource::<CameraPostFx>();
        world.insert_resource(Paused(false));
        world.insert_resource(Time::<()>::default());
        world.init_resource::<Messages<BucketEvent>>();
        world.init_resource::<Messages<RimHitEvent>>();
        world.init_resource::<Messages<BackboardHitEvent>>();
        world.run_system_once(setup_fx_assets).expect("assets");
        world
    }

    fn count_particles(world: &mut World) -> usize {
        world.query::<&Particle>().iter(world).count()
    }

    fn count_meshes(world: &mut World) -> usize {
        world.query::<&Mesh3d>().iter(world).count()
    }

    /// Every burst spawned by the event FX must age out completely — nothing
    /// may leak into the next possession.
    #[test]
    fn event_fx_spawn_and_fully_despawn() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = fx_world();
        world.write_message(BucketEvent {
            shooter: None,
            hoop_home: false,
            dunk: true,
            is_three: false,
        });
        world.write_message(RimHitEvent {
            pos: Vec3::new(HOOP_X, RIM_HEIGHT, 0.2),
            speed: 7.0,
        });
        world.write_message(BackboardHitEvent {
            pos: Vec3::new(HOOP_X + 0.4, RIM_HEIGHT + 0.3, 0.1),
        });
        world.run_system_once(spawn_on_bucket).expect("bucket fx");
        world.run_system_once(rim_fx).expect("rim fx");
        world.run_system_once(board_fx).expect("board fx");

        let spawned = count_particles(&mut world);
        assert!(spawned >= 40, "expected a rich burst, got {spawned} particles");
        assert!(count_meshes(&mut world) >= spawned);
        let juice = world.resource::<ScreenJuice>();
        assert!(juice.flash > 0.0 && juice.hitstop > 0.0);

        // Age at 50 ms steps; the longest-lived particle is well under 4 s.
        for _ in 0..80 {
            {
                let mut t = world.resource_mut::<Time<()>>();
                t.advance_by(std::time::Duration::from_millis(50));
            }
            world.run_system_once(age_particles).expect("age");
        }
        assert_eq!(count_particles(&mut world), 0, "particles leaked");
        // Score-pop glyph children ride along with their particle root.
        assert_eq!(count_meshes(&mut world), 0, "mesh children leaked");
    }

    /// The fire aura appears once the streak ignites, follows its owner and
    /// tears down (flames included) as soon as the streak breaks.
    #[test]
    fn fire_aura_follows_heat() {
        use bevy::ecs::system::RunSystemOnce;
        use crate::roster::CharacterId;
        let mut world = fx_world();
        world.init_resource::<JuiceClock>();
        let player = world
            .spawn((
                Player {
                    id: CharacterId::KaitoFlash,
                    side: Side::Home,
                    slot: 0,
                    human: true,
                },
                Heat { streak: 3 },
                Transform::from_xyz(1.0, 0.0, 2.0),
            ))
            .id();
        world.run_system_once(fire_aura).expect("aura");
        let auras: Vec<(Entity, Vec3)> = world
            .query::<(Entity, &FireAura, &Transform)>()
            .iter(&world)
            .map(|(e, _, t)| (e, t.translation))
            .collect();
        assert_eq!(auras.len(), 1);
        assert_eq!(auras[0].1, Vec3::new(1.0, 0.0, 2.0));
        let flames = world.query::<&Flame>().iter(&world).count();
        assert_eq!(flames, 9, "eight tongues plus the floor ring");

        // Move the owner; the aura root tracks and no second aura is spawned.
        world.get_mut::<Transform>(player).unwrap().translation.x = 4.0;
        world.run_system_once(fire_aura).expect("aura");
        let roots: Vec<Vec3> = world
            .query::<(&FireAura, &Transform)>()
            .iter(&world)
            .map(|(_, t)| t.translation)
            .collect();
        assert_eq!(roots, vec![Vec3::new(4.0, 0.0, 2.0)]);

        // Streak breaks: aura and its flames are gone, only embers remain to age out.
        world.get_mut::<Heat>(player).unwrap().streak = 0;
        world.run_system_once(fire_aura).expect("aura");
        assert_eq!(world.query::<&FireAura>().iter(&world).count(), 0);
        assert_eq!(world.query::<&Flame>().iter(&world).count(), 0);
        for _ in 0..30 {
            {
                let mut t = world.resource_mut::<Time<()>>();
                t.advance_by(std::time::Duration::from_millis(50));
            }
            world.run_system_once(age_particles).expect("age");
        }
        assert_eq!(count_particles(&mut world), 0, "embers leaked");
    }
}
