use bevy::prelude::*;

/// Top-level flow for the FINNBALL client.
#[derive(Clone, Copy, Default, Eq, PartialEq, Debug, Hash, States)]
pub enum AppState {
    #[default]
    Splash,
    MainMenu,
    CharacterSelect,
    CourtSelect,
    Playing,
    GameOver,
}

#[derive(Clone, Copy, Default, Eq, PartialEq, Debug, Hash, States)]
pub enum MenuPhase {
    #[default]
    Disabled,
    Root,
}

#[derive(Resource, Default)]
pub struct Paused(pub bool);

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum GameMode {
    #[default]
    QuickMatch,
    Exhibition,
    Practice,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum CameraMode {
    #[default]
    Broadcast,
    Chase,
    Tactical,
    Cinema,
}

#[derive(Resource, Clone)]
pub struct MatchConfig {
    pub mode: GameMode,
    pub arena: crate::arenas::ArenaId,
    pub home: [crate::roster::CharacterId; 3],
    pub away: [crate::roster::CharacterId; 3],
    pub quarter_secs: f32,
    pub shot_clock: f32,
}

impl Default for MatchConfig {
    fn default() -> Self {
        use crate::roster::CharacterId::*;
        Self {
            mode: GameMode::QuickMatch,
            arena: crate::arenas::ArenaId::NeoTokyo,
            home: [KaitoFlash, MikaOrbit, JinGravity],
            away: [ReiWall, YunaSilk, ZeroGhost],
            quarter_secs: 60.0,
            shot_clock: 24.0,
        }
    }
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct CameraSettings {
    pub mode: CameraMode,
}

#[derive(Resource, Default)]
pub struct LineupDraft {
    pub selected: Vec<crate::roster::CharacterId>,
    pub arena_index: usize,
}
