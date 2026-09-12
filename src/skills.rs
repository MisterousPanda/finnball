//! Per-legend signature skills. Ratings stay the gym-tuned baseline; these
//! multipliers only fire in the situations the flavour text already promised.

use crate::roster::CharacterId;
use crate::sim::{ShotType, THREE_RADIUS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureId {
    BlinkDrive,
    LogoGravity,
    FloorRemembers,
    LockedGate,
    WatchTheIdea,
    NeverThere,
    ClutchRitual,
    MissBelongs,
    ImpossibleEasy,
    OnTheRise,
}

#[derive(Clone, Copy, Debug)]
pub struct SignatureSkill {
    pub id: SignatureId,
    pub name: &'static str,
    pub hook: &'static str,
}

impl SignatureId {
    pub fn slug(self) -> &'static str {
        match self {
            Self::BlinkDrive => "blink_drive",
            Self::LogoGravity => "logo_gravity",
            Self::FloorRemembers => "floor_remembers",
            Self::LockedGate => "locked_gate",
            Self::WatchTheIdea => "watch_the_idea",
            Self::NeverThere => "never_there",
            Self::ClutchRitual => "clutch_ritual",
            Self::MissBelongs => "miss_belongs",
            Self::ImpossibleEasy => "impossible_easy",
            Self::OnTheRise => "on_the_rise",
        }
    }

    pub fn from_character(id: CharacterId) -> Self {
        match id {
            CharacterId::KaitoFlash => Self::BlinkDrive,
            CharacterId::MikaOrbit => Self::LogoGravity,
            CharacterId::JinGravity => Self::FloorRemembers,
            CharacterId::ReiWall => Self::LockedGate,
            CharacterId::YunaSilk => Self::WatchTheIdea,
            CharacterId::ZeroGhost => Self::NeverThere,
            CharacterId::LunaEclipse => Self::ClutchRitual,
            CharacterId::TaroTitan => Self::MissBelongs,
            CharacterId::AikoPrism => Self::ImpossibleEasy,
            CharacterId::KenjiVolt => Self::OnTheRise,
        }
    }
}

impl CharacterId {
    pub fn signature(self) -> SignatureSkill {
        match self {
            Self::KaitoFlash => SignatureSkill {
                id: SignatureId::BlinkDrive,
                name: "BLINK DRIVE",
                hook: "Transition speed burst. Green window opens on the break.",
            },
            Self::MikaOrbit => SignatureSkill {
                id: SignatureId::LogoGravity,
                name: "LOGO GRAVITY",
                hook: "Threes from the arc and the logo hang longer and drop more.",
            },
            Self::JinGravity => SignatureSkill {
                id: SignatureId::FloorRemembers,
                name: "FLOOR REMEMBERS",
                hook: "Paint finishes and posterizes shrug off a body.",
            },
            Self::ReiWall => SignatureSkill {
                id: SignatureId::LockedGate,
                name: "LOCKED GATE",
                hook: "Help D in the paint takes a much bigger bite out of the shot.",
            },
            Self::YunaSilk => SignatureSkill {
                id: SignatureId::WatchTheIdea,
                name: "WATCH THE IDEA",
                hook: "Skip and bounce passes are harder to pick off.",
            },
            Self::ZeroGhost => SignatureSkill {
                id: SignatureId::NeverThere,
                name: "NEVER THERE",
                hook: "Reach-in steals connect from a step farther than they should.",
            },
            Self::LunaEclipse => SignatureSkill {
                id: SignatureId::ClutchRitual,
                name: "CLUTCH RITUAL",
                hook: "Last eight seconds of the shot clock: the meter forgives.",
            },
            Self::TaroTitan => SignatureSkill {
                id: SignatureId::MissBelongs,
                name: "MISS BELONGS",
                hook: "Offensive glass and put-backs belong to the Titan.",
            },
            Self::AikoPrism => SignatureSkill {
                id: SignatureId::ImpossibleEasy,
                name: "IMPOSSIBLE EASY",
                hook: "Contested mid-range floaters stay live.",
            },
            Self::KenjiVolt => SignatureSkill {
                id: SignatureId::OnTheRise,
                name: "ON THE RISE",
                hook: "Baseline drives in transition finish above the rim.",
            },
        }
    }
}

/// Situational bag passed into the multipliers. Keep this Bevy-free.
#[derive(Clone, Copy, Debug, Default)]
pub struct SkillCtx {
    pub is_three: bool,
    pub dist: f32,
    pub contest: f32,
    pub shot_clock: f32,
    pub in_paint: bool,
    pub speed: f32,
    pub shot: Option<ShotType>,
}

impl SkillCtx {
    pub fn transition(self) -> bool {
        self.speed > 6.0
    }
}

impl SignatureId {
    /// Applied after `shot_make_chance` × heat. 1.0 = no change.
    pub fn make_mult(self, ctx: SkillCtx) -> f32 {
        match self {
            Self::BlinkDrive if ctx.transition() => 1.10,
            Self::LogoGravity if ctx.is_three && ctx.dist > 10.0 => 1.22,
            Self::LogoGravity if ctx.is_three && ctx.dist >= THREE_RADIUS => 1.16,
            Self::FloorRemembers
                if ctx.in_paint
                    && matches!(
                        ctx.shot,
                        Some(ShotType::Dunk | ShotType::Layup | ShotType::ReverseLayup)
                    ) =>
            {
                1.18
            }
            Self::ClutchRitual if ctx.shot_clock < 8.0 => 1.14,
            Self::ImpossibleEasy
                if ctx.contest > 0.30
                    && !ctx.is_three
                    && matches!(
                        ctx.shot,
                        Some(ShotType::Floater | ShotType::Runner | ShotType::JumpShot)
                    ) =>
            {
                1.15
            }
            Self::OnTheRise if ctx.transition() && ctx.in_paint => 1.16,
            Self::MissBelongs if ctx.in_paint && ctx.dist < 2.4 => 1.12,
            _ => 1.0,
        }
    }

    pub fn steal_mult(self) -> f32 {
        match self {
            Self::NeverThere => 1.28,
            _ => 1.0,
        }
    }

    /// Passer-side intercept resistance. <1 means the lane is harder to pick.
    pub fn intercept_resist(self) -> f32 {
        match self {
            Self::WatchTheIdea => 0.82,
            _ => 1.0,
        }
    }

    pub fn contest_mult(self, defender_in_paint: bool) -> f32 {
        match self {
            Self::LockedGate if defender_in_paint => 1.35,
            _ => 1.0,
        }
    }

    pub fn rebound_mult(self) -> f32 {
        match self {
            Self::MissBelongs => 1.25,
            _ => 1.0,
        }
    }

    pub fn speed_mult(self, sprint: bool) -> f32 {
        match self {
            Self::BlinkDrive if sprint => 1.08,
            _ => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_unique_slugs() {
        let mut slugs: Vec<_> = CharacterId::ALL
            .iter()
            .map(|c| c.signature().id.slug())
            .collect();
        slugs.sort();
        slugs.dedup();
        assert_eq!(slugs.len(), 10);
    }

    #[test]
    fn mika_beats_kaito_from_the_logo() {
        let ctx = SkillCtx {
            is_three: true,
            dist: 11.0,
            ..SkillCtx::default()
        };
        assert!(SignatureId::LogoGravity.make_mult(ctx) > SignatureId::BlinkDrive.make_mult(ctx));
        assert!((SignatureId::LogoGravity.make_mult(ctx) - 1.22).abs() < f32::EPSILON);
    }

    #[test]
    fn rei_locks_the_paint() {
        assert!((SignatureId::LockedGate.contest_mult(true) - 1.35).abs() < f32::EPSILON);
        assert_eq!(SignatureId::LockedGate.contest_mult(false), 1.0);
    }

    #[test]
    fn zero_picks_pockets() {
        assert!((SignatureId::NeverThere.steal_mult() - 1.28).abs() < f32::EPSILON);
        assert_eq!(SignatureId::BlinkDrive.steal_mult(), 1.0);
    }

    #[test]
    fn luna_wakes_up_late_clock() {
        let late = SkillCtx {
            shot_clock: 4.0,
            ..SkillCtx::default()
        };
        let early = SkillCtx {
            shot_clock: 18.0,
            ..SkillCtx::default()
        };
        assert!(SignatureId::ClutchRitual.make_mult(late) > SignatureId::ClutchRitual.make_mult(early));
    }
}
