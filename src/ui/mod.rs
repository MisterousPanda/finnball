use bevy::prelude::*;

use crate::audio::UiClick;

mod hud;
mod menu;
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
        .add_systems(Update, button_visuals);
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
