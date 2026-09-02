//! Touch controls: a virtual stick anywhere on the left half of the screen plus
//! on-screen action buttons. Hidden until the first touch is seen so mouse and
//! keyboard players never see them.

use bevy::input::touch::Touches;
use bevy::prelude::*;

use crate::gameplay::PlayerIntent;
use crate::states::{AppState, Paused};
use crate::theme::{title_font, CYAN, GOLD, MAGENTA, TEXT};

pub struct TouchPlugin;

impl Plugin for TouchPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TouchState>()
            .add_systems(Update, detect_touch)
            .add_systems(OnEnter(AppState::Playing), spawn_touch_ui)
            .add_systems(
                Update,
                (reveal_touch_ui, show_pause_buttons).run_if(in_state(AppState::Playing)),
            );
    }
}

#[derive(Resource, Default)]
pub struct TouchState {
    /// Any touch has ever been seen this session.
    pub seen: bool,
    stick_id: Option<u64>,
    stick_origin: Vec2,
    /// Current stick deflection, screen axes (x right, y down), length ≤ 1.
    pub stick: Vec2,
    shoot_was_down: bool,
}

/// Which action a touch button drives.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum TouchBtn {
    Shoot,
    Pass,
    Steal,
    Dunk,
    Block,
    Switch,
    Pause,
    Menu,
}

#[derive(Component)]
pub struct TouchRoot;

#[derive(Component)]
pub struct StickRing;

#[derive(Component)]
pub struct StickKnob;

const STICK_RADIUS: f32 = 64.0;
const DEADZONE: f32 = 0.14;

fn detect_touch(touches: Res<Touches>, mut state: ResMut<TouchState>, mut probed: Local<bool>) {
    if !*probed {
        *probed = true;
        if browser_has_touch_screen() {
            state.seen = true;
        }
    }
    if !state.seen && touches.iter().next().is_some() {
        state.seen = true;
    }
}

/// Phones and tablets report touch points up front; showing the controls
/// immediately there beats waiting for a first touch that might be a tap on
/// a menu button the game never sees as a touch.
#[cfg(target_arch = "wasm32")]
fn browser_has_touch_screen() -> bool {
    web_sys::window()
        .map(|w| w.navigator().max_touch_points() > 0)
        .unwrap_or(false)
}

#[cfg(not(target_arch = "wasm32"))]
fn browser_has_touch_screen() -> bool {
    false
}

/// Runs after keyboard/gamepad input so it only adds to what is already set.
pub fn apply_touch(
    touches: Res<Touches>,
    windows: Query<&Window>,
    buttons: Query<(&Interaction, &TouchBtn)>,
    mut state: ResMut<TouchState>,
    mut intent: ResMut<PlayerIntent>,
    mut knobs: Query<&mut Node, With<StickKnob>>,
    mut rings: Query<(&mut Node, &mut Visibility), (With<StickRing>, Without<StickKnob>)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let width = window.width();
    let height = window.height();

    // Virtual stick: the first touch that begins on the left 55% of the screen.
    if let Some(id) = state.stick_id {
        if let Some(t) = touches.get_pressed(id) {
            let delta = t.position() - state.stick_origin;
            let mag = (delta.length() / STICK_RADIUS).min(1.0);
            state.stick = if mag < DEADZONE {
                Vec2::ZERO
            } else {
                delta.normalize_or_zero() * ((mag - DEADZONE) / (1.0 - DEADZONE))
            };
        } else {
            state.stick_id = None;
            state.stick = Vec2::ZERO;
        }
    }
    if state.stick_id.is_none() {
        for t in touches.iter_just_pressed() {
            let p = t.position();
            if p.x < width * 0.55 && p.y > height * 0.12 {
                state.stick_id = Some(t.id());
                state.stick_origin = p;
                state.stick = Vec2::ZERO;
                break;
            }
        }
    }
    if state.stick != Vec2::ZERO {
        intent.move_xz = state.stick.normalize();
        intent.sprint |= state.stick.length() > 0.92;
    }
    for (mut node, mut vis) in &mut rings {
        if state.stick_id.is_some() {
            *vis = Visibility::Visible;
            node.left = px(state.stick_origin.x - STICK_RADIUS);
            node.top = px(state.stick_origin.y - STICK_RADIUS);
        } else {
            *vis = Visibility::Hidden;
        }
    }
    for mut node in &mut knobs {
        let off = state.stick * (STICK_RADIUS - 18.0);
        node.left = px(STICK_RADIUS - 18.0 + off.x);
        node.top = px(STICK_RADIUS - 18.0 + off.y);
    }

    // SHOOT is a hold: `Interaction::Pressed` stays set while the finger is down,
    // and the release is the frame it stops being set.
    let shoot_down = buttons
        .iter()
        .any(|(i, b)| *b == TouchBtn::Shoot && *i == Interaction::Pressed);
    if shoot_down {
        intent.shoot_held = true;
    } else if state.shoot_was_down {
        intent.shoot_released = true;
    }
    state.shoot_was_down = shoot_down;
}

/// Edge-triggered wrapper so tap buttons fire once per press.
pub fn tap_buttons(
    q: Query<(&Interaction, &TouchBtn), Changed<Interaction>>,
    mut intent: ResMut<PlayerIntent>,
    mut paused: ResMut<Paused>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (interaction, btn) in &q {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match btn {
            TouchBtn::Shoot => {}
            TouchBtn::Pass => intent.pass = true,
            TouchBtn::Steal => intent.steal = true,
            TouchBtn::Dunk => intent.special = true,
            TouchBtn::Block => intent.block = true,
            TouchBtn::Switch => intent.switch = true,
            TouchBtn::Pause => paused.0 = !paused.0,
            TouchBtn::Menu => {
                paused.0 = false;
                next.set(AppState::MainMenu);
            }
        }
    }
}

fn spawn_touch_ui(mut commands: Commands, state: Res<TouchState>) {
    let display = if state.seen {
        Display::Flex
    } else {
        Display::None
    };
    commands
        .spawn((
            TouchRoot,
            DespawnOnExit(AppState::Playing),
            GlobalZIndex(20),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                width: percent(100),
                height: percent(100),
                display,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|root| {
            // Stick ring + knob (positioned per frame).
            root.spawn((
                StickRing,
                Visibility::Hidden,
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    width: px(STICK_RADIUS * 2.0),
                    height: px(STICK_RADIUS * 2.0),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(CYAN.with_alpha(0.55)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.18)),
                children![(
                    StickKnob,
                    Pickable::IGNORE,
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(36),
                        height: px(36),
                        left: px(STICK_RADIUS - 18.0),
                        top: px(STICK_RADIUS - 18.0),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(CYAN.with_alpha(0.75)),
                )],
            ));

            // Top-right: pause / switch, and menu (pause only).
            root.spawn((
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    right: px(12),
                    top: px(52),
                    column_gap: px(8),
                    ..default()
                },
                children![
                    small_btn("MENU", TouchBtn::Menu, MAGENTA, true),
                    small_btn("SWITCH", TouchBtn::Switch, CYAN, false),
                    small_btn("PAUSE", TouchBtn::Pause, CYAN, false),
                ],
            ));

            // Bottom-right: action cluster.
            root.spawn((
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    right: px(14),
                    bottom: px(18),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FlexEnd,
                    row_gap: px(10),
                    ..default()
                },
                children![
                    (
                        Pickable::IGNORE,
                        Node {
                            column_gap: px(10),
                            ..default()
                        },
                        children![
                            round_btn("BLOCK", TouchBtn::Block, 60.0, TEXT),
                            round_btn("STEAL", TouchBtn::Steal, 60.0, MAGENTA),
                            round_btn("DUNK", TouchBtn::Dunk, 60.0, GOLD),
                        ],
                    ),
                    (
                        Pickable::IGNORE,
                        Node {
                            column_gap: px(10),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        children![
                            round_btn("PASS", TouchBtn::Pass, 66.0, CYAN),
                            round_btn("SHOOT", TouchBtn::Shoot, 96.0, GOLD),
                        ],
                    ),
                ],
            ));
        });
}

fn round_btn(label: &'static str, btn: TouchBtn, size: f32, tint: Color) -> impl Bundle {
    (
        Button,
        btn,
        Node {
            width: px(size),
            height: px(size),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BorderColor::all(tint.with_alpha(0.8)),
        BackgroundColor(Color::srgba(0.02, 0.04, 0.09, 0.62)),
        children![(
            Text::new(label),
            title_font(if size > 80.0 { 16.0 } else { 11.0 }),
            TextColor(tint),
            Pickable::IGNORE,
        )],
    )
}

fn small_btn(label: &'static str, btn: TouchBtn, tint: Color, pause_only: bool) -> impl Bundle {
    let base = (
        Button,
        btn,
        Node {
            padding: UiRect::axes(px(12), px(8)),
            border: UiRect::all(px(1)),
            display: if pause_only {
                Display::None
            } else {
                Display::Flex
            },
            border_radius: BorderRadius::all(px(6)),
            ..default()
        },
        BorderColor::all(tint.with_alpha(0.7)),
        BackgroundColor(Color::srgba(0.02, 0.04, 0.09, 0.62)),
        children![(
            Text::new(label),
            title_font(11.0),
            TextColor(tint),
            Pickable::IGNORE,
        )],
    );
    base
}

fn reveal_touch_ui(state: Res<TouchState>, mut roots: Query<&mut Node, With<TouchRoot>>) {
    if !state.seen {
        return;
    }
    for mut node in &mut roots {
        if node.display != Display::Flex {
            node.display = Display::Flex;
        }
    }
}

fn show_pause_buttons(
    paused: Res<Paused>,
    mut q: Query<(&mut Node, &TouchBtn)>,
) {
    for (mut node, btn) in &mut q {
        if *btn != TouchBtn::Menu {
            continue;
        }
        let want = if paused.0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != want {
            node.display = want;
        }
    }
}
