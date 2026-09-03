use bevy::prelude::*;

use crate::audio::UiClick;

mod hud;
pub mod menu;
mod select;
mod splash;

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            splash::SplashPlugin,
            menu::MenuPlugin,
            select::SelectPlugin,
            hud::HudPlugin,
        ))
        .add_systems(Update, (button_visuals, fit_ui_scale));
    }
}

/// The HUD and menus are laid out for a ~1600x900 window. Phones are 400-850
/// logical px on a side, so shrink the whole UI to keep panels from covering
/// the court; desktops stay at 1.0.
fn fit_ui_scale(windows: Query<&Window>, mut scale: ResMut<UiScale>) {
    let Ok(win) = windows.single() else {
        return;
    };
    let target = (win.width() / 1400.0).min(win.height() / 800.0).clamp(0.42, 1.0);
    if (scale.0 - target).abs() > 0.01 {
        scale.0 = target;
    }
}

#[derive(Component)]
pub struct MenuBtn;

fn button_visuals(
    mut q: Query<(&Interaction, &mut BackgroundColor, &mut BorderColor), (Changed<Interaction>, With<Button>)>,
    mut clicks: MessageWriter<UiClick>,
) {
    for (interaction, mut bg, mut border) in &mut q {
        match *interaction {
            Interaction::Pressed => {
                *bg = crate::theme::BTN_PRESS.into();
                *border = BorderColor::all(crate::theme::CYAN);
                clicks.write(UiClick { confirm: true });
            }
            Interaction::Hovered => {
                *bg = crate::theme::BTN_HOVER.into();
                *border = BorderColor::all(crate::theme::GOLD);
                clicks.write(UiClick { confirm: false });
            }
            Interaction::None => {
                *bg = crate::theme::BTN.into();
                *border = BorderColor::all(crate::theme::CYAN.with_alpha(0.35));
            }
        }
    }
}
