use bevy::prelude::*;

use crate::states::AppState;
use crate::theme::{CYAN, GOLD, MAGENTA, TEXT, title_font};

pub struct SplashPlugin;

impl Plugin for SplashPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Splash), setup)
            .add_systems(Update, tick.run_if(in_state(AppState::Splash)));
    }
}

#[derive(Resource)]
struct SplashTimer(Timer);

fn setup(mut commands: Commands) {
    commands.insert_resource(SplashTimer(Timer::from_seconds(2.4, TimerMode::Once)));
    commands.spawn((
        DespawnOnExit(AppState::Splash),
        Node {
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.02, 0.55)),
        children![
            (
                Text::new("FINNBALL"),
                title_font(92.0),
                TextColor(CYAN),
            ),
            (
                Text::new("ANIME STREET  ×  THE COURT"),
                title_font(22.0),
                TextColor(MAGENTA),
                Node {
                    margin: UiRect::top(px(8)),
                    ..default()
                },
            ),
            (
                Text::new("ESPORTS NIGHT  •  TIP-OFF LOADING"),
                title_font(16.0),
                TextColor(GOLD),
                Node {
                    margin: UiRect::top(px(28)),
                    ..default()
                },
            ),
            (
                Text::new("tap, click or press any key  •  sound on"),
                title_font(14.0),
                TextColor(TEXT),
                Node {
                    margin: UiRect::top(px(48)),
                    ..default()
                },
            ),
        ],
    ));
}

fn tick(
    time: Res<Time>,
    mut timer: ResMut<SplashTimer>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    touches: Res<bevy::input::touch::Touches>,
    mut next: ResMut<NextState<AppState>>,
) {
    timer.0.tick(time.delta());
    if timer.0.is_finished()
        || keys.get_just_pressed().next().is_some()
        || mouse.just_pressed(MouseButton::Left)
        || touches.any_just_pressed()
    {
        next.set(AppState::MainMenu);
    }
}
