use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;

use crate::ball::Ball;
use crate::states::{AppState, CameraMode, CameraSettings};
use crate::units::{Controlled, Player};

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraSettings>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (orbit_menu_cam, follow_game_cam),
            );
    }
}

#[derive(Component)]
pub struct GameCam;

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        GameCam,
        Camera3d::default(),
        Msaa::Off,
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.02, 0.03, 0.06)),
            ..default()
        },
        Tonemapping::TonyMcMapface,
        Transform::from_xyz(0.0, 16.0, 22.0).looking_at(Vec3::new(0.0, 0.8, 0.0), Vec3::Y),
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
    state: Res<State<AppState>>,
    mut q: Query<&mut Transform, With<GameCam>>,
) {
    if matches!(*state.get(), AppState::Playing | AppState::GameOver) {
        return;
    }
    let Ok(mut tf) = q.single_mut() else {
        return;
    };
    let t = time.elapsed_secs() * 0.18;
    let pos = Vec3::new(t.sin() * 18.0, 9.0 + t.cos() * 0.6, t.cos() * 18.0);
    *tf = Transform::from_translation(pos).looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y);
}

fn follow_game_cam(
    time: Res<Time>,
    state: Res<State<AppState>>,
    settings: Res<CameraSettings>,
    ball: Query<&Transform, (With<Ball>, Without<GameCam>, Without<Player>)>,
    hero: Query<&Transform, (With<Controlled>, Without<GameCam>, Without<Ball>)>,
    mut cam: Query<&mut Transform, With<GameCam>>,
) {
    if *state.get() != AppState::Playing {
        return;
    }
    let Ok(mut ctf) = cam.single_mut() else {
        return;
    };
    let ball_pos = ball.single().ok().map(|t| t.translation).unwrap_or(Vec3::ZERO);
    let hero_pos = hero.single().ok().map(|t| t.translation).unwrap_or(ball_pos);
    let look = ball_pos.lerp(hero_pos, 0.35) + Vec3::Y * 0.6;
    let desired = match settings.mode {
        CameraMode::Broadcast => Vec3::new(look.x * 0.15, 11.5, 18.5),
        CameraMode::Chase => hero_pos + Vec3::new(0.0, 4.2, 8.5) - Vec3::new(hero_pos.x, 0.0, 0.0).normalize_or_zero() * 0.0 + Vec3::new(-hero_pos.x.signum() * 2.0, 0.0, 0.0),
        CameraMode::Tactical => Vec3::new(look.x, 28.0, 0.1),
        CameraMode::Cinema => Vec3::new(look.x - 6.0, 2.4, look.z + 7.0),
    };
    let dt = time.delta_secs();
    ctf.translation = ctf.translation.lerp(desired, 1.0 - (-2.8 * dt).exp());
    let current_fwd = ctf.looking_at(look, Vec3::Y);
    ctf.rotation = ctf.rotation.slerp(current_fwd.rotation, 1.0 - (-3.5 * dt).exp());
}
