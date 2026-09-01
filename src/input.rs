use bevy::prelude::*;

use crate::gameplay::{LiveControl, PlayerIntent, ShotMeter};
use crate::states::{AppState, CameraMode, CameraSettings, Paused};
use crate::units::{Controlled, Player};

pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerIntent>().add_systems(
            Update,
            (read_input, switch_player, toggle_pause, cycle_camera)
                .run_if(in_state(AppState::Playing)),
        );
    }
}

fn read_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut intent: ResMut<PlayerIntent>,
    mut meter: ResMut<ShotMeter>,
) {
    let mut dir = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        dir.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        dir.x += 1.0;
    }
    intent.move_xz = if dir.length_squared() > 0.0 {
        dir.normalize()
    } else {
        Vec2::ZERO
    };
    intent.sprint = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    intent.shoot_held = keys.pressed(KeyCode::Space);
    intent.shoot_released |= keys.just_released(KeyCode::Space);
    intent.pass |= keys.just_pressed(KeyCode::KeyE);
    intent.steal |= keys.just_pressed(KeyCode::KeyQ);
    intent.special |= keys.just_pressed(KeyCode::KeyF);
    intent.block |= keys.just_pressed(KeyCode::KeyR);
    intent.switch |= keys.just_pressed(KeyCode::Tab) || keys.just_pressed(KeyCode::KeyC);
    intent.pass_kind = if keys.pressed(KeyCode::KeyT) {
        crate::sim::PassKind::Lob
    } else if keys.pressed(KeyCode::KeyG) {
        crate::sim::PassKind::Bounce
    } else if intent.sprint && keys.pressed(KeyCode::KeyE) {
        crate::sim::PassKind::Skip
    } else {
        crate::sim::PassKind::Chest
    };
    let _ = meter;
}

fn switch_player(
    mut intent: ResMut<PlayerIntent>,
    mut control: ResMut<LiveControl>,
    mut commands: Commands,
    humans: Query<(Entity, &Player, Option<&Controlled>)>,
) {
    if !intent.switch {
        return;
    }
    intent.switch = false;
    let mut ids: Vec<Entity> = humans
        .iter()
        .filter(|(_, p, _)| p.side == crate::roster::Side::Home)
        .map(|(e, _, _)| e)
        .collect();
    ids.sort_by_key(|e| e.index());
    if ids.is_empty() {
        return;
    }
    let current = control.entity;
    let next = if let Some(cur) = current {
        ids.iter()
            .position(|e| *e == cur)
            .map(|i| ids[(i + 1) % ids.len()])
            .unwrap_or(ids[0])
    } else {
        ids[0]
    };
    for (e, _, had) in &humans {
        if had.is_some() {
            commands.entity(e).remove::<Controlled>();
        }
        if e == next {
            commands.entity(e).insert(Controlled);
        }
    }
    control.entity = Some(next);
}

fn toggle_pause(keys: Res<ButtonInput<KeyCode>>, mut paused: ResMut<Paused>) {
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyP) {
        paused.0 = !paused.0;
    }
}

fn cycle_camera(keys: Res<ButtonInput<KeyCode>>, mut cam: ResMut<CameraSettings>) {
    if keys.just_pressed(KeyCode::KeyV) || keys.just_pressed(KeyCode::Digit1) {
        cam.mode = CameraMode::Broadcast;
    }
    if keys.just_pressed(KeyCode::Digit2) {
        cam.mode = CameraMode::Chase;
    }
    if keys.just_pressed(KeyCode::Digit3) {
        cam.mode = CameraMode::Tactical;
    }
    if keys.just_pressed(KeyCode::Digit4) {
        cam.mode = CameraMode::Cinema;
    }
}
