use bevy::prelude::*;

use crate::camera::CameraPostFx;
use crate::fx::ScreenJuice;
use crate::gameplay::{LiveControl, MatchClock, Scoreboard, ShotMeter, Ticker};
use crate::states::{AppState, CameraSettings, GameMode, MatchConfig, Paused};
use crate::theme::{CYAN, GOLD, LIVE, MAGENTA, MUTED, PANEL, TEXT, title_font};
use crate::units::{BoxLine, Player, Stamina};
use crate::ui::MenuBtn;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Playing), setup_hud)
            .add_systems(OnEnter(AppState::GameOver), setup_over)
            .add_systems(
                Update,
                (refresh_hud, pause_overlay, drive_broadcast_fx).run_if(in_state(AppState::Playing)),
            )
            .add_systems(Update, over_clicks.run_if(in_state(AppState::GameOver)));
    }
}

#[derive(Component)]
struct ScoreText;

#[derive(Component)]
struct ClockText;

#[derive(Component)]
struct ShotText;

#[derive(Component)]
struct TickerText;

#[derive(Component)]
struct PlayerText;

#[derive(Component)]
struct MeterFill;

#[derive(Component)]
struct PauseLayer;

#[derive(Component)]
struct LetterTop;
#[derive(Component)]
struct LetterBot;
#[derive(Component)]
struct CrowdFlash;

#[derive(Component, Clone, Copy)]
enum OverNav {
    Menu,
    Rematch,
}

fn setup_hud(mut commands: Commands) {
    commands.spawn((
        DespawnOnExit(AppState::Playing),
        CrowdFlash,
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.0)),
        ZIndex(20),
        Pickable::IGNORE,
    ));
    commands.spawn((
        DespawnOnExit(AppState::Playing),
        LetterTop,
        Node {
            position_type: PositionType::Absolute,
            top: px(0),
            width: percent(100),
            height: px(0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.88)),
        ZIndex(18),
        Pickable::IGNORE,
    ));
    commands.spawn((
        DespawnOnExit(AppState::Playing),
        LetterBot,
        Node {
            position_type: PositionType::Absolute,
            bottom: px(0),
            width: percent(100),
            height: px(0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.88)),
        ZIndex(18),
        Pickable::IGNORE,
    ));
    commands.spawn((
        DespawnOnExit(AppState::Playing),
        Node {
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16)),
            ..default()
        },
        children![
            (
                Node {
                    width: percent(100),
                    justify_content: JustifyContent::Center,
                    column_gap: px(18),
                    align_items: AlignItems::Center,
                    padding: UiRect::all(px(10)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(CYAN.with_alpha(0.35)),
                children![
                    (Text::new("● LIVE"), title_font(14.0), TextColor(LIVE)),
                    (ScoreText, Text::new("FOX  0  —  0  CRN"), title_font(28.0), TextColor(TEXT)),
                    (ClockText, Text::new("Q1  1:00"), title_font(20.0), TextColor(GOLD)),
                    (ShotText, Text::new("24"), title_font(22.0), TextColor(MAGENTA)),
                ],
            ),
            (
                Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::FlexEnd,
                    ..default()
                },
                children![
                    (
                        Node {
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(12)),
                            border: UiRect::all(px(1)),
                            row_gap: px(4),
                            min_width: px(280),
                            ..default()
                        },
                        BackgroundColor(PANEL),
                        BorderColor::all(MAGENTA.with_alpha(0.4)),
                        children![
                            (PlayerText, Text::new("ON BALL"), title_font(16.0), TextColor(CYAN)),
                            (
                                Node {
                                    width: px(220),
                                    height: px(10),
                                    border: UiRect::all(px(1)),
                                    ..default()
                                },
                                BorderColor::all(CYAN.with_alpha(0.4)),
                                children![
                                    (
                                        MeterFill,
                                        Node {
                                            width: percent(0),
                                            height: percent(100),
                                            ..default()
                                        },
                                        BackgroundColor(GOLD),
                                    ),
                                    (
                                        Node {
                                            position_type: PositionType::Absolute,
                                            left: percent(70.0),
                                            width: px(2),
                                            height: percent(100),
                                            ..default()
                                        },
                                        BackgroundColor(CYAN),
                                    ),
                                ],
                            ),
                            (
                                Text::new("SPACE shot (gold=green)  •  E pass  •  T lob  •  G bounce  •  Q steal  •  F dunk  •  R block"),
                                title_font(12.0),
                                TextColor(MUTED),
                            ),
                        ],
                    ),
                    (
                        TickerText,
                        Text::new("FINNBALL  //  BROADCAST"),
                        title_font(16.0),
                        TextColor(GOLD),
                    ),
                ],
            ),
        ],
    ));
}

fn refresh_hud(
    score: Res<Scoreboard>,
    clock: Res<MatchClock>,
    ticker: Res<Ticker>,
    meter: Res<ShotMeter>,
    control: Res<LiveControl>,
    cam: Res<CameraSettings>,
    config: Res<MatchConfig>,
    players: Query<(Entity, &Player, &Stamina, &BoxLine)>,
    mut score_t: Query<&mut Text, (With<ScoreText>, Without<ClockText>, Without<ShotText>, Without<TickerText>, Without<PlayerText>)>,
    mut clock_t: Query<&mut Text, (With<ClockText>, Without<ScoreText>, Without<ShotText>, Without<TickerText>, Without<PlayerText>)>,
    mut shot_t: Query<&mut Text, (With<ShotText>, Without<ScoreText>, Without<ClockText>, Without<TickerText>, Without<PlayerText>)>,
    mut tick_t: Query<&mut Text, (With<TickerText>, Without<ScoreText>, Without<ClockText>, Without<ShotText>, Without<PlayerText>)>,
    mut play_t: Query<&mut Text, (With<PlayerText>, Without<ScoreText>, Without<ClockText>, Without<ShotText>, Without<TickerText>)>,
    mut fill: Query<&mut Node, With<MeterFill>>,
) {
    for mut t in &mut score_t {
        *t = Text::new(format!("FOX  {}  —  {}  CRN", score.home, score.away));
    }
    let secs = clock.remaining.max(0.0) as u32;
    for mut t in &mut clock_t {
        if config.mode == GameMode::Practice {
            *t = Text::new("PRACTICE");
        } else {
            *t = Text::new(format!("Q{}  {}:{}", clock.quarter, secs / 60, format!("{:02}", secs % 60)));
        }
    }
    for mut t in &mut shot_t {
        *t = Text::new(format!("SC {}", clock.shot.max(0.0) as u32));
    }
    for mut t in &mut tick_t {
        *t = Text::new(ticker.line.clone());
    }
    if let Some(e) = control.entity {
        if let Ok((_, p, stam, boxl)) = players.get(e) {
            for mut t in &mut play_t {
                *t = Text::new(format!(
                    "{}\n{}  CAM {:?}  STM {:02.0}%\nPTS {}  AST {}  REB {}  STL {}",
                    p.id.profile().name,
                    p.id.profile().alias,
                    cam.mode,
                    stam.0 * 100.0,
                    boxl.pts,
                    boxl.ast,
                    boxl.reb,
                    boxl.stl
                ));
            }
        }
    }
    for mut n in &mut fill {
        let pct = if meter.armed || meter.freeze > 0.0 {
            meter.value
        } else {
            0.0
        };
        n.width = percent(pct * 100.0);
    }
}

fn drive_broadcast_fx(
    cam: Res<CameraPostFx>,
    juice: Res<ScreenJuice>,
    mut flash: Query<&mut BackgroundColor, (With<CrowdFlash>, Without<LetterTop>, Without<LetterBot>)>,
    mut top: Query<&mut Node, (With<LetterTop>, Without<LetterBot>)>,
    mut bot: Query<&mut Node, (With<LetterBot>, Without<LetterTop>)>,
) {
    let a = (cam.crowd_flash.max(juice.flash) * 0.22).clamp(0.0, 0.35);
    for mut bg in &mut flash {
        *bg = BackgroundColor(Color::srgba(1.0, 0.95, 0.85, a));
    }
    let h = 70.0 * cam.letterbox;
    for mut n in &mut top {
        n.height = px(h);
    }
    for mut n in &mut bot {
        n.height = px(h);
    }
}

fn pause_overlay(
    paused: Res<Paused>,
    mut commands: Commands,
    existing: Query<Entity, With<PauseLayer>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut next: ResMut<NextState<AppState>>,
) {
    if paused.0 {
        if existing.is_empty() {
            commands.spawn((
                PauseLayer,
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
                children![
                    (Text::new("PAUSED"), title_font(48.0), TextColor(CYAN)),
                    (
                        Text::new("ESC resume   •   M menu"),
                        title_font(16.0),
                        TextColor(MUTED),
                    ),
                ],
            ));
        }
        if keys.just_pressed(KeyCode::KeyM) {
            next.set(AppState::MainMenu);
        }
    } else {
        for e in &existing {
            commands.entity(e).despawn();
        }
    }
}

fn setup_over(mut commands: Commands, score: Res<Scoreboard>) {
    let winner = if score.home > score.away {
        "NEON FOXES TAKE THE NIGHT"
    } else if score.away > score.home {
        "SHADOW CRANES STEAL THE SERIES"
    } else {
        "DRAW — THE CROWD WANTS OT"
    };
    commands.spawn((
        DespawnOnExit(AppState::GameOver),
        Node {
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.04, 0.72)),
        children![
            (Text::new("FINAL"), title_font(18.0), TextColor(GOLD)),
            (Text::new(winner), title_font(36.0), TextColor(CYAN)),
            (
                Text::new(format!("{}  —  {}", score.home, score.away)),
                title_font(48.0),
                TextColor(TEXT),
            ),
            (
                Node {
                    column_gap: px(12),
                    margin: UiRect::top(px(18)),
                    ..default()
                },
                children![
                    over_btn("REMATCH", OverNav::Rematch),
                    over_btn("MAIN MENU", OverNav::Menu),
                ],
            ),
        ],
    ));
}

fn over_btn(label: &'static str, nav: OverNav) -> impl Bundle {
    (
        Button,
        MenuBtn,
        nav,
        crate::theme::button_node(),
        BackgroundColor(crate::theme::BTN),
        BorderColor::all(CYAN.with_alpha(0.4)),
        children![(Text::new(label), title_font(18.0), TextColor(TEXT))],
    )
}

fn over_clicks(
    q: Query<(&Interaction, &OverNav), (Changed<Interaction>, With<Button>)>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (i, n) in &q {
        if *i != Interaction::Pressed {
            continue;
        }
        match n {
            OverNav::Menu => next.set(AppState::MainMenu),
            OverNav::Rematch => next.set(AppState::Playing),
        }
    }
}
