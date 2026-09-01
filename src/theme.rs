use bevy::prelude::*;

pub const CYAN: Color = Color::srgb(0.15, 0.95, 1.0);
pub const MAGENTA: Color = Color::srgb(1.0, 0.22, 0.72);
pub const GOLD: Color = Color::srgb(1.0, 0.82, 0.28);
pub const PANEL: Color = Color::srgba(0.02, 0.03, 0.07, 0.88);
pub const PANEL_SOLID: Color = Color::srgb(0.04, 0.05, 0.1);
pub const TEXT: Color = Color::srgb(0.92, 0.96, 1.0);
pub const MUTED: Color = Color::srgb(0.62, 0.72, 0.85);
pub const LIVE: Color = Color::srgb(1.0, 0.18, 0.28);
pub const BTN: Color = Color::srgb(0.07, 0.1, 0.18);
pub const BTN_HOVER: Color = Color::srgb(0.12, 0.22, 0.32);
pub const BTN_PRESS: Color = Color::srgb(0.05, 0.55, 0.55);

pub fn button_node() -> Node {
    Node {
        width: px(340),
        height: px(56),
        margin: UiRect::all(px(8)),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(px(1)),
        ..default()
    }
}

pub fn title_font(size: f32) -> TextFont {
    TextFont {
        font_size: size,
        ..default()
    }
}
