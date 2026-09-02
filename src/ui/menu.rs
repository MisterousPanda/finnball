use bevy::prelude::*;

use crate::states::{AppState, GameMode, MatchConfig};
use crate::theme::{button_node, title_font, CYAN, GOLD, MAGENTA, MUTED, PANEL, TEXT};
use crate::ui::MenuBtn;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::MainMenu), setup)
            .add_systems(Update, click.run_if(in_state(AppState::MainMenu)));
    }
}

#[derive(Component, Clone, Copy)]
enum Action {
    Quick,
    Exhibition,
    Practice,
    Quit,
}

fn setup(mut commands: Commands, windows: Query<&Window>) {
    // Phones in landscape are ~400 logical px tall; scale the stack so every
    // button stays on screen instead of falling off the bottom.
    let h = windows.single().map(|w| w.height()).unwrap_or(900.0);
    let s = (h / 900.0).clamp(0.5, 1.0);
    commands.spawn((
        DespawnOnExit(AppState::MainMenu),
        Node {
            width: percent(100),
            height: percent(100),
            padding: UiRect::axes(px(40.0 * s), px(16.0 + 24.0 * s)),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        children![top_bar(), center_stack(s), footer(),],
    ));
}

fn top_bar() -> impl Bundle {
    (
        Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            (
                Text::new("● LIVE   FINNBALL SERIES"),
                title_font(14.0),
                TextColor(crate::theme::LIVE),
            ),
            (
                Text::new("NEON FOXES  vs  SHADOW CRANES"),
                title_font(14.0),
                TextColor(MUTED),
            ),
        ],
    )
}

fn center_stack(s: f32) -> impl Bundle {
    (
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexStart,
            row_gap: px(4.0 * s),
            ..default()
        },
        children![
            (Text::new("FINNBALL"), title_font(84.0 * s), TextColor(CYAN),),
            (
                Text::new("TOON PHYSICS. ANIME HEART. ESPORTS FRAME."),
                title_font(18.0 * s.max(0.75)),
                TextColor(MAGENTA),
                Node {
                    margin: UiRect::bottom(px(18.0 * s)),
                    ..default()
                },
            ),
            menu_btn("QUICK MATCH  3v3", Action::Quick, s),
            menu_btn("EXHIBITION  DRAFT + COURT", Action::Exhibition, s),
            menu_btn("PRACTICE  GYM", Action::Practice, s),
            menu_btn("QUIT", Action::Quit, s),
            (
                Text::new("ENTER / SPACE  QUICK MATCH   •   2  EXHIBITION   •   G  GYM   •   PAD: A PLAY  X DRAFT  Y GYM"),
                title_font(12.0),
                TextColor(MUTED),
                Node {
                    margin: UiRect::top(px(10)),
                    ..default()
                },
            ),
        ],
    )
}

fn menu_btn(label: &'static str, action: Action, s: f32) -> impl Bundle {
    let mut node = button_node();
    node.height = px(56.0 * s.max(0.7));
    node.margin = UiRect::all(px(8.0 * s));
    (
        Button,
        MenuBtn,
        action,
        node,
        BackgroundColor(crate::theme::BTN),
        BorderColor::all(CYAN.with_alpha(0.4)),
        children![(Text::new(label), title_font(20.0 * s.max(0.8)), TextColor(TEXT))],
    )
}

fn footer() -> impl Bundle {
    (
        Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
        children![
            (
                Text::new("WASD / STICK  •  SPACE / A SHOOT  •  E / X PASS  •  Q / B STEAL  •  F / Y DUNK  •  LT SPRINT"),
                title_font(13.0),
                TextColor(MUTED),
            ),
            (
                Text::new("V1  //  BROADCAST BUILD"),
                title_font(13.0),
                TextColor(GOLD),
            ),
        ],
    )
}

fn click(
    q: Query<(&Interaction, &Action), (Changed<Interaction>, With<Button>)>,
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut next: ResMut<NextState<AppState>>,
    mut config: ResMut<MatchConfig>,
    mut exit: MessageWriter<AppExit>,
) {
    // Keyboard / gamepad shortcuts mirror the buttons so the menu never depends
    // on pointer hit-testing alone (touch screens, odd DPI scaling, etc.).
    let mut shortcut = None;
    if keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::NumpadEnter)
        || keys.just_pressed(KeyCode::Space)
        || keys.just_pressed(KeyCode::Digit1)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::South) || p.just_pressed(GamepadButton::Start))
    {
        shortcut = Some(Action::Quick);
    } else if keys.just_pressed(KeyCode::Digit2)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::West))
    {
        shortcut = Some(Action::Exhibition);
    } else if keys.just_pressed(KeyCode::KeyG)
        || keys.just_pressed(KeyCode::Digit3)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::North))
    {
        shortcut = Some(Action::Practice);
    }

    let pressed = q
        .iter()
        .filter(|(i, _)| **i == Interaction::Pressed)
        .map(|(_, a)| *a);
    for action in shortcut.into_iter().chain(pressed) {
        match action {
            Action::Quick => {
                config.mode = GameMode::QuickMatch;
                next.set(AppState::Playing);
            }
            Action::Exhibition => {
                config.mode = GameMode::Exhibition;
                next.set(AppState::CharacterSelect);
            }
            Action::Practice => {
                config.mode = GameMode::Practice;
                next.set(AppState::Playing);
            }
            Action::Quit => {
                #[cfg(not(target_arch = "wasm32"))]
                exit.write(AppExit::Success);
                #[cfg(target_arch = "wasm32")]
                let _ = &exit;
            }
        }
    }
    let _ = PANEL;
}
