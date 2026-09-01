mod ai;
mod arenas;
mod audio;
mod ball;
mod camera;
mod court;
mod courtpaint;
mod fx;
mod gameplay;
mod input;
mod roster;
mod sim;
mod states;
mod theme;
mod ui;
mod units;

use bevy::prelude::*;
use bevy::window::{PresentMode, WindowResolution};

fn main() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "FINNBALL".into(),
                        resolution: WindowResolution::new(1600, 900),
                        present_mode: PresentMode::AutoVsync,
                        canvas: Some("#bevy".into()),
                        fit_canvas_to_parent: true,
                        prevent_default_event_handling: true,
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_nearest()),
        )
        .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.06)))
        .insert_resource(states::MatchConfig::default())
        .insert_resource(states::Paused(false))
        .init_state::<states::AppState>()
        .add_plugins((
            camera::CameraPlugin,
            court::CourtPlugin,
            units::UnitsPlugin,
            ball::BallPlugin,
            gameplay::GameplayPlugin,
            ai::AiPlugin,
            input::InputPlugin,
            fx::FxPlugin,
            audio::FinnAudioPlugin,
            ui::UiPlugin,
        ))
        .run();
}
