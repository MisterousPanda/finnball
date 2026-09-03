//! Device quality tier. Phones (iOS Safari in particular) kill the tab when the
//! WebGL + wasm footprint gets too large, so the arena is built smaller there.

use bevy::prelude::*;

#[derive(Resource, Clone, Copy, Debug)]
pub struct Quality {
    pub mobile: bool,
    /// Lower-bowl seating tiers.
    pub tiers: usize,
    /// Upper-deck rows (closed-roof arenas).
    pub upper_rows: usize,
    /// Court paint resolution.
    pub court_px_per_m: u32,
    /// Share of lower-bowl seats that get a fan (the rest stay empty seats).
    pub crowd_density: f32,
}

impl Quality {
    pub fn detect() -> Self {
        if is_mobile_browser() {
            Self {
                mobile: true,
                tiers: 4,
                upper_rows: 2,
                court_px_per_m: 40,
                crowd_density: 0.6,
            }
        } else if cfg!(target_arch = "wasm32") {
            Self {
                mobile: false,
                tiers: 7,
                upper_rows: 7,
                court_px_per_m: 64,
                crowd_density: 0.9,
            }
        } else {
            Self {
                mobile: false,
                tiers: 9,
                upper_rows: 10,
                court_px_per_m: 64,
                crowd_density: 0.9,
            }
        }
    }
}

impl Default for Quality {
    fn default() -> Self {
        Self::detect()
    }
}

#[cfg(target_arch = "wasm32")]
fn is_mobile_browser() -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    let nav = win.navigator();
    let ua = nav.user_agent().unwrap_or_default();
    let touch = nav.max_touch_points() > 0;
    let narrow = win
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .map(|w| w < 1100.0)
        .unwrap_or(false);
    ua.contains("iPhone")
        || ua.contains("iPad")
        || ua.contains("Android")
        || ua.contains("Mobile")
        // iPadOS reports a desktop Safari UA; the touch points give it away.
        || (ua.contains("Macintosh") && nav.max_touch_points() > 1)
        || (touch && narrow)
}

#[cfg(not(target_arch = "wasm32"))]
fn is_mobile_browser() -> bool {
    std::env::var_os("FINNBALL_MOBILE").is_some()
}
