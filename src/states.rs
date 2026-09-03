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

/// Opposition AI difficulty. Each level maps to an [`crate::ai::AiProfile`]
/// (`Difficulty::profile`); the human's AI teammates always play at PRO.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Hash)]
pub enum Difficulty {
    Rookie,
    #[default]
    Pro,
    Legend,
}

impl Difficulty {
    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Rookie => "ROOKIE",
            Difficulty::Pro => "PRO",
            Difficulty::Legend => "LEGEND",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Difficulty::Rookie => Difficulty::Pro,
            Difficulty::Pro => Difficulty::Legend,
            Difficulty::Legend => Difficulty::Rookie,
        }
    }

    pub fn profile(self) -> crate::ai::AiProfile {
        match self {
            Difficulty::Rookie => crate::ai::ROOKIE,
            Difficulty::Pro => crate::ai::PRO,
            Difficulty::Legend => crate::ai::LEGEND,
        }
    }
}

#[derive(Resource, Clone)]
pub struct MatchConfig {
    pub mode: GameMode,
    pub arena: crate::arenas::ArenaId,
    pub home: [crate::roster::CharacterId; 3],
    pub away: [crate::roster::CharacterId; 3],
    pub quarter_secs: f32,
    pub shot_clock: f32,
    pub difficulty: Difficulty,
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
            difficulty: Difficulty::Pro,
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
