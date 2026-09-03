mod ai;
mod arenas;
mod audio;
mod ball;
mod camera;
mod court;
mod courtpaint;
mod crowd;
mod fx;
mod gameplay;
mod input;
mod quality;
mod roster;
mod sim;
mod states;
mod theme;
mod touch;
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
                .set(ImagePlugin::default_nearest())
                // No `.meta` sidecars ship with the game; on the web a static host would
                // answer those probes with index.html and the loader would choke on it.
                .set(AssetPlugin {
                    meta_check: bevy::asset::AssetMetaCheck::Never,
                    ..default()
                }),
        )
        .insert_resource(ClearColor(Color::srgb(0.02, 0.03, 0.06)))
        .insert_resource(quality::Quality::detect())
        .insert_resource(states::MatchConfig::default())
        .insert_resource(states::Paused(false))
        .init_state::<states::AppState>()
        .add_plugins((
            camera::CameraPlugin,
            court::CourtPlugin,
            crowd::CrowdPlugin,
            units::UnitsPlugin,
            ball::BallPlugin,
            gameplay::GameplayPlugin,
            ai::AiPlugin,
            input::InputPlugin,
            touch::TouchPlugin,
            fx::FxPlugin,
            audio::FinnAudioPlugin,
            ui::UiPlugin,
        ))
        .run();
}
