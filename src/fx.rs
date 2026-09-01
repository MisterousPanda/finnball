use bevy::prelude::*;

use crate::ball::{Ball, BallState, BallVel, BucketEvent, Hold};
use crate::camera::CameraPostFx;
use crate::states::{AppState, Paused};
use crate::units::{Player, Pose};

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScreenJuice>()
            .init_resource::<JuiceClock>()
            .add_systems(Startup, setup_juice_assets)
            .add_systems(
                Update,
                (
                    spawn_on_bucket,
                    spawn_ball_trail,
                    spawn_afterimages,
                    age_sparks,
                    age_trails,
                    age_screen_juice,
                    apply_hitstop,
                )
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
}

#[derive(Resource)]
struct JuiceMeshes {
    sphere: Handle<Mesh>,
    capsule: Handle<Mesh>,
    trail_mat: Handle<StandardMaterial>,
    ghost_mat: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Spark {
    life: f32,
}

#[derive(Component)]
struct FadeTrail {
    life: f32,
}

fn setup_juice_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(JuiceMeshes {
        sphere: meshes.add(Sphere::new(0.08)),
        capsule: meshes.add(Capsule3d::new(0.22, 1.05)),
        trail_mat: materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.55, 0.18),
            emissive: LinearRgba::from(Color::srgb(1.0, 0.55, 0.18).to_linear()) * 3.6,
            unlit: true,
            ..default()
        }),
        ghost_mat: materials.add(StandardMaterial {
            base_color: Color::srgba(0.65, 0.9, 1.0, 0.32),
            emissive: LinearRgba::new(0.25, 0.45, 0.7, 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
    });
}

fn spawn_on_bucket(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut buckets: MessageReader<BucketEvent>,
    mut juice: ResMut<ScreenJuice>,
    mut cam_fx: ResMut<CameraPostFx>,
    ball: Query<&Transform, With<Ball>>,
) {
    for ev in buckets.read() {
        juice.flash = if ev.dunk { 1.0 } else { 0.55 };
        juice.hitstop = if ev.dunk { 0.14 } else { 0.06 };
        cam_fx.crowd_flash = cam_fx.crowd_flash.max(if ev.dunk { 1.0 } else { 0.55 });
        cam_fx.shake = cam_fx.shake.max(if ev.dunk { 0.85 } else { 0.4 });

        let origin = ball
            .single()
            .ok()
            .map(|t| t.translation)
            .unwrap_or(Vec3::new(0.0, 3.0, 0.0));
        let color = if ev.dunk {
            Color::srgb(1.0, 0.45, 0.1)
        } else {
            Color::srgb(0.3, 0.9, 1.0)
        };
        let mat = materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color.to_linear()) * 4.0,
            unlit: true,
            ..default()
        });
        let mesh = meshes.add(Cuboid::new(0.12, 0.12, 0.12));
        for i in 0..14 {
            let a = i as f32 * 0.45;
            commands.spawn((
                Spark { life: 0.7 },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(origin + Vec3::new(a.sin() * 0.2, 0.1, a.cos() * 0.2)),
                crate::court::ArenaRoot,
            ));
        }
    }
}

fn spawn_ball_trail(
    mut commands: Commands,
    time: Res<Time>,
    paused: Res<Paused>,
    assets: Res<JuiceMeshes>,
    mut clock: ResMut<JuiceClock>,
    ball: Query<(&Transform, &BallVel, &BallState), With<Ball>>,
) {
    if paused.0 {
        return;
    }
    let Ok((tf, vel, state)) = ball.single() else {
        return;
    };
    let hot = vel.0.length() > 6.0 || state.hold == Hold::Shot;
    if !hot {
        clock.ball = 0.04;
        return;
    }
    clock.ball += time.delta_secs();
    if clock.ball < 0.04 {
        return;
    }
    clock.ball = 0.0;
    let speed = vel.0.length();
    let scale = (0.055 + (speed * 0.004).min(0.04)).max(0.05);
    commands.spawn((
        FadeTrail { life: 0.28 },
        Mesh3d(assets.sphere.clone()),
        MeshMaterial3d(assets.trail_mat.clone()),
        Transform::from_translation(tf.translation).with_scale(Vec3::splat(scale)),
        crate::court::ArenaRoot,
        DespawnOnExit(AppState::Playing),
    ));
}

fn spawn_afterimages(
    mut commands: Commands,
    time: Res<Time>,
    paused: Res<Paused>,
    assets: Res<JuiceMeshes>,
    mut clock: ResMut<JuiceClock>,
    players: Query<(&Transform, &Pose), With<Player>>,
) {
    if paused.0 {
        return;
    }
    let sprinting = players.iter().any(|(_, pose)| *pose == Pose::Sprint);
    if !sprinting {
        return;
    }
    clock.after += time.delta_secs();
    if clock.after < 0.08 {
        return;
    }
    clock.after = 0.0;
    for (tf, pose) in &players {
        if *pose != Pose::Sprint {
            continue;
        }
        commands.spawn((
            FadeTrail { life: 0.15 },
            Mesh3d(assets.capsule.clone()),
            MeshMaterial3d(assets.ghost_mat.clone()),
            Transform {
                translation: tf.translation + Vec3::Y * 0.95,
                rotation: tf.rotation,
                scale: tf.scale * 0.95,
            },
            crate::court::ArenaRoot,
            DespawnOnExit(AppState::Playing),
        ));
    }
}

fn age_sparks(
    time: Res<Time>,
    paused: Res<Paused>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Spark, &mut Transform), Without<FadeTrail>>,
) {
    if paused.0 {
        return;
    }
    for (e, mut s, mut tf) in &mut q {
        s.life -= time.delta_secs();
        tf.translation.y += time.delta_secs() * 2.4;
        tf.scale *= 1.0 - time.delta_secs() * 1.5;
        if s.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

fn age_trails(
    time: Res<Time>,
    paused: Res<Paused>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut FadeTrail, &mut Transform), Without<Spark>>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    for (e, mut trail, mut tf) in &mut q {
        trail.life -= dt;
        tf.scale *= 1.0 - dt * 5.5;
        if trail.life <= 0.0 || tf.scale.max_element() < 0.01 {
            commands.entity(e).despawn();
        }
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
