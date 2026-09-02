use bevy::ecs::spawn::SpawnIter;
use bevy::prelude::*;

use crate::arenas::ArenaId;
use crate::roster::CharacterId;
use crate::states::{AppState, LineupDraft, MatchConfig};
use crate::theme::{CYAN, GOLD, MAGENTA, MUTED, PANEL, TEXT, title_font};
use crate::ui::MenuBtn;

pub struct SelectPlugin;

impl Plugin for SelectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LineupDraft>()
            .add_systems(OnEnter(AppState::CharacterSelect), setup_chars)
            .add_systems(OnEnter(AppState::CourtSelect), setup_courts)
            .add_systems(
                Update,
                (char_clicks, lineup_label)
                    .run_if(in_state(AppState::CharacterSelect)),
            )
            .add_systems(Update, court_clicks.run_if(in_state(AppState::CourtSelect)))
            .add_systems(
                Update,
                escape_to_menu.run_if(
                    in_state(AppState::CharacterSelect).or(in_state(AppState::CourtSelect)),
                ),
            );
    }
}

fn escape_to_menu(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut next: ResMut<NextState<AppState>>,
) {
    if keys.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::East))
    {
        next.set(AppState::MainMenu);
    }
}

#[derive(Component)]
struct PickChar(CharacterId);

#[derive(Component)]
struct LineupText;

#[derive(Component, Clone, Copy)]
enum CharNav {
    Confirm,
    Back,
    Clear,
}

#[derive(Component)]
struct PickCourt(ArenaId);

#[derive(Component, Clone, Copy)]
enum CourtNav {
    Confirm,
    Back,
}

fn setup_chars(mut commands: Commands, mut draft: ResMut<LineupDraft>) {
    draft.selected.clear();
    let mut cards = Vec::new();
    for id in CharacterId::ALL {
        let p = id.profile();
        cards.push((
            Button,
            MenuBtn,
            PickChar(id),
            Node {
                width: px(210),
                height: px(118),
                margin: UiRect::all(px(6)),
                padding: UiRect::all(px(10)),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                row_gap: px(4),
                ..default()
            },
            BackgroundColor(PANEL),
            BorderColor::all(p.accent),
            children![
                (Text::new(p.name), title_font(14.0), TextColor(CYAN)),
                (Text::new(p.alias), title_font(12.0), TextColor(MAGENTA)),
                (
                    Text::new(format!(
                        "SPD {:>2}  3PT {:>2}  DUNK {:>2}",
                        p.speed, p.three, p.dunk
                    )),
                    title_font(11.0),
                    TextColor(MUTED),
                ),
            ],
        ));
    }

    commands.spawn((
        DespawnOnExit(AppState::CharacterSelect),
        Node {
            width: percent(100),
            height: percent(100),
            padding: UiRect::all(px(28)),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            ..default()
        },
        children![
            (
                Text::new("DRAFT YOUR THREE"),
                title_font(36.0),
                TextColor(CYAN),
            ),
            (
                Text::new("Click three legends. Home jersey: Neon Foxes. CPU fills Shadow Cranes."),
                title_font(16.0),
                TextColor(MUTED),
            ),
            (
                LineupText,
                Text::new("LINEUP:  —"),
                title_font(18.0),
                TextColor(GOLD),
            ),
            (
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    width: percent(100),
                    ..default()
                },
                Children::spawn(SpawnIter(CharacterId::ALL.into_iter().map(|id| {
                    let p = id.profile();
                    (
                        Button,
                        MenuBtn,
                        PickChar(id),
                        Node {
                            width: px(230),
                            height: px(124),
                            margin: UiRect::all(px(6)),
                            padding: UiRect::all(px(10)),
                            flex_direction: FlexDirection::Column,
                            border: UiRect::all(px(1)),
                            row_gap: px(4),
                            ..default()
                        },
                        BackgroundColor(PANEL),
                        BorderColor::all(p.accent),
                        children![
                            (Text::new(p.name), title_font(14.0), TextColor(CYAN)),
                            (Text::new(p.alias), title_font(12.0), TextColor(MAGENTA)),
                            (
                                Text::new(format!(
                                    "SPD {:>2}   3PT {:>2}   DUNK {:>2}\nSTL {:>2}   BLK {:>2}   REB {:>2}",
                                    p.speed, p.three, p.dunk, p.steal, p.block, p.rebound
                                )),
                                title_font(11.0),
                                TextColor(TEXT),
                            ),
                        ],
                    )
                }))),
            ),
            (
                Node {
                    column_gap: px(12),
                    ..default()
                },
                children![
                    nav_btn("CONFIRM DRAFT", CharNav::Confirm),
                    nav_btn("CLEAR", CharNav::Clear),
                    nav_btn("BACK", CharNav::Back),
                ],
            ),
        ],
    ));
    let _ = cards;
}

fn nav_btn(label: &'static str, nav: CharNav) -> impl Bundle {
    (
        Button,
        MenuBtn,
        nav,
        crate::theme::button_node(),
        BackgroundColor(crate::theme::BTN),
        BorderColor::all(CYAN.with_alpha(0.4)),
        children![(Text::new(label), title_font(16.0), TextColor(TEXT))],
    )
}

fn char_clicks(
    picks: Query<(&Interaction, &PickChar), (Changed<Interaction>, With<Button>)>,
    nav: Query<(&Interaction, &CharNav), (Changed<Interaction>, With<Button>)>,
    mut draft: ResMut<LineupDraft>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (i, PickChar(id)) in &picks {
        if *i == Interaction::Pressed && draft.selected.len() < 3 && !draft.selected.contains(id) {
            draft.selected.push(*id);
        }
    }
    for (i, n) in &nav {
        if *i != Interaction::Pressed {
            continue;
        }
        match n {
            CharNav::Clear => draft.selected.clear(),
            CharNav::Back => next.set(AppState::MainMenu),
            CharNav::Confirm => {
                if draft.selected.len() == 3 {
                    next.set(AppState::CourtSelect);
                }
            }
        }
    }
}

fn lineup_label(draft: Res<LineupDraft>, mut q: Query<&mut Text, With<LineupText>>) {
    if !draft.is_changed() {
        return;
    }
    let names: Vec<&str> = draft.selected.iter().map(|c| c.profile().name).collect();
    let s = if names.is_empty() {
        "LINEUP:  —".into()
    } else {
        format!("LINEUP:  {}", names.join("  /  "))
    };
    for mut t in &mut q {
        *t = Text::new(s.clone());
    }
}

fn setup_courts(mut commands: Commands) {
    commands.spawn((
        DespawnOnExit(AppState::CourtSelect),
        Node {
            width: percent(100),
            height: percent(100),
            padding: UiRect::all(px(28)),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            ..default()
        },
        children![
            (
                Text::new("CHOOSE THE FLOOR"),
                title_font(36.0),
                TextColor(CYAN),
            ),
            (
                Text::new("Five worlds. Same rules. Different gravity personality."),
                title_font(16.0),
                TextColor(MUTED),
            ),
            (
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                },
                Children::spawn(SpawnIter(ArenaId::ALL.into_iter().map(|id| {
                    let t = id.theme();
                    (
                        Button,
                        MenuBtn,
                        PickCourt(id),
                        Node {
                            width: px(320),
                            height: px(140),
                            margin: UiRect::all(px(8)),
                            padding: UiRect::all(px(14)),
                            flex_direction: FlexDirection::Column,
                            border: UiRect::all(px(1)),
                            row_gap: px(6),
                            ..default()
                        },
                        BackgroundColor(PANEL),
                        BorderColor::all(t.line),
                        children![
                            (Text::new(t.name), title_font(18.0), TextColor(GOLD)),
                            (Text::new(t.subtitle), title_font(13.0), TextColor(TEXT)),
                        ],
                    )
                }))),
            ),
            (
                Node {
                    column_gap: px(12),
                    ..default()
                },
                children![
                    court_nav("TIP OFF", CourtNav::Confirm),
                    court_nav("BACK", CourtNav::Back),
                ],
            ),
        ],
    ));
}

fn court_nav(label: &'static str, nav: CourtNav) -> impl Bundle {
    (
        Button,
        MenuBtn,
        nav,
        crate::theme::button_node(),
        BackgroundColor(crate::theme::BTN),
        BorderColor::all(CYAN.with_alpha(0.4)),
        children![(Text::new(label), title_font(16.0), TextColor(TEXT))],
    )
}

fn court_clicks(
    courts: Query<(&Interaction, &PickCourt), (Changed<Interaction>, With<Button>)>,
    nav: Query<(&Interaction, &CourtNav), (Changed<Interaction>, With<Button>)>,
    mut draft: ResMut<LineupDraft>,
    mut config: ResMut<MatchConfig>,
    mut next: ResMut<NextState<AppState>>,
) {
    for (i, PickCourt(id)) in &courts {
        if *i == Interaction::Pressed {
            config.arena = *id;
            draft.arena_index = ArenaId::ALL.iter().position(|a| a == id).unwrap_or(0);
        }
    }
    for (i, n) in &nav {
        if *i != Interaction::Pressed {
            continue;
        }
        match n {
            CourtNav::Back => next.set(AppState::CharacterSelect),
            CourtNav::Confirm => {
                if draft.selected.len() == 3 {
                    config.home = [draft.selected[0], draft.selected[1], draft.selected[2]];
                    let rest: Vec<_> = CharacterId::ALL
                        .into_iter()
                        .filter(|c| !draft.selected.contains(c))
                        .take(3)
                        .collect();
                    if rest.len() == 3 {
                        config.away = [rest[0], rest[1], rest[2]];
                    }
                }
                next.set(AppState::Playing);
            }
        }
    }
}
