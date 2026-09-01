use bevy::prelude::*;

use crate::gameplay::{LiveControl, PlayerIntent, ShotMeter};
use crate::sim::PassKind;
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
    pads: Query<&Gamepad>,
    mut intent: ResMut<PlayerIntent>,
    meter: ResMut<ShotMeter>,
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

    let mut pad_sprint = false;
    let mut pad_shoot = false;
    let mut pad_shoot_up = false;
    let mut pad_pass = false;
    let mut pad_steal = false;
    let mut pad_special = false;
    let mut pad_block = false;
    let mut pad_switch = false;
    let mut pad_kind: Option<PassKind> = None;

    for pad in &pads {
        let stick = pad.left_stick();
        if stick.length() > 0.22 {
            dir.x += stick.x;
            dir.y -= stick.y;
        }
        let dpad = pad.dpad();
        if dpad.length() > 0.45 {
            dir.x += dpad.x;
            dir.y -= dpad.y;
        }
        pad_sprint |=
            pad.pressed(GamepadButton::LeftTrigger) || pad.pressed(GamepadButton::LeftTrigger2);
        pad_shoot |= pad.pressed(GamepadButton::South);
        pad_shoot_up |= pad.just_released(GamepadButton::South);
        pad_pass |= pad.just_pressed(GamepadButton::West);
        pad_steal |= pad.just_pressed(GamepadButton::East);
        pad_special |= pad.just_pressed(GamepadButton::North);
        pad_block |= pad.pressed(GamepadButton::RightTrigger)
            || pad.just_pressed(GamepadButton::RightTrigger2);
        pad_switch |=
            pad.just_pressed(GamepadButton::LeftThumb) || pad.just_pressed(GamepadButton::Select);
        let rs = pad.right_stick();
        if rs.y > 0.55 {
            pad_kind = Some(PassKind::Lob);
        } else if rs.y < -0.55 {
            pad_kind = Some(PassKind::Bounce);
        } else if pad_sprint && pad.pressed(GamepadButton::West) {
            pad_kind = Some(PassKind::Skip);
        }
    }

    intent.move_xz = if dir.length_squared() > 0.0 {
        dir.normalize()
    } else {
        Vec2::ZERO
    };
    intent.sprint =
        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) || pad_sprint;
    intent.shoot_held = keys.pressed(KeyCode::Space) || pad_shoot;
    intent.shoot_released |= keys.just_released(KeyCode::Space) || pad_shoot_up;
    intent.pass |= keys.just_pressed(KeyCode::KeyE) || pad_pass;
    intent.steal |= keys.just_pressed(KeyCode::KeyQ) || pad_steal;
    intent.special |= keys.just_pressed(KeyCode::KeyF) || pad_special;
    intent.block |= keys.just_pressed(KeyCode::KeyR) || pad_block;
    intent.switch |=
        keys.just_pressed(KeyCode::Tab) || keys.just_pressed(KeyCode::KeyC) || pad_switch;
    intent.pass_kind = if keys.pressed(KeyCode::KeyT) {
        PassKind::Lob
    } else if keys.pressed(KeyCode::KeyG) {
        PassKind::Bounce
    } else if intent.sprint && keys.pressed(KeyCode::KeyE) {
        PassKind::Skip
    } else if let Some(kind) = pad_kind {
        kind
    } else {
        PassKind::Chest
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

fn toggle_pause(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut paused: ResMut<Paused>,
) {
    let pad_pause = pads
        .iter()
        .any(|p| p.just_pressed(GamepadButton::Start) || p.just_pressed(GamepadButton::Mode));
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyP) || pad_pause {
        paused.0 = !paused.0;
    }
}

fn cycle_camera(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut cam: ResMut<CameraSettings>,
) {
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
    for pad in &pads {
        if pad.just_pressed(GamepadButton::RightThumb) || pad.just_pressed(GamepadButton::DPadRight)
        {
            cam.mode = match cam.mode {
                CameraMode::Broadcast => CameraMode::Chase,
                CameraMode::Chase => CameraMode::Tactical,
                CameraMode::Tactical => CameraMode::Cinema,
                CameraMode::Cinema => CameraMode::Broadcast,
            };
        }
    }
}
