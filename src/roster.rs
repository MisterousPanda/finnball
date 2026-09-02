use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CharacterId {
    KaitoFlash,
    MikaOrbit,
    JinGravity,
    ReiWall,
    YunaSilk,
    ZeroGhost,
    LunaEclipse,
    TaroTitan,
    AikoPrism,
    KenjiVolt,
}

impl CharacterId {
    pub const ALL: [CharacterId; 10] = [
        Self::KaitoFlash,
        Self::MikaOrbit,
        Self::JinGravity,
        Self::ReiWall,
        Self::YunaSilk,
        Self::ZeroGhost,
        Self::LunaEclipse,
        Self::TaroTitan,
        Self::AikoPrism,
        Self::KenjiVolt,
    ];

    pub fn profile(self) -> CharacterProfile {
        match self {
            Self::KaitoFlash => CharacterProfile {
                id: self,
                name: "KAITO FLASH",
                alias: "The Neon Point",
                quote: "If you blink, I already scored.",
                style: "Space-jam speedster. Blurs past bigs, finishes with a smirk.",
                height_m: 1.86,
                speed: 99,
                three: 78,
                mid: 84,
                dunk: 62,
                handle: 91,
                pass: 88,
                steal: 80,
                block: 28,
                rebound: 42,
                strength: 58,
                hair: HairStyle::Spikes,
                hair_color: Color::srgb(0.15, 0.85, 1.0),
                skin: Color::srgb(0.96, 0.82, 0.72),
                accent: Color::srgb(0.2, 1.0, 0.95),
                eye: Color::srgb(0.1, 0.85, 1.0),
            },
            Self::MikaOrbit => CharacterProfile {
                id: self,
                name: "MIKA ORBIT",
                alias: "Logo Sniper",
                quote: "Gravity is a suggestion.",
                style: "Logo threes, floaty hangtime, galaxy twin-tails.",
                height_m: 1.78,
                speed: 82,
                three: 99,
                mid: 90,
                dunk: 40,
                handle: 86,
                pass: 80,
                steal: 70,
                block: 22,
                rebound: 38,
                strength: 48,
                hair: HairStyle::TwinTails,
                hair_color: Color::srgb(0.95, 0.35, 0.85),
                skin: Color::srgb(0.99, 0.88, 0.82),
                accent: Color::srgb(1.0, 0.45, 0.9),
                eye: Color::srgb(0.85, 0.25, 0.7),
            },
            Self::JinGravity => CharacterProfile {
                id: self,
                name: "JIN GRAVITY",
                alias: "Rim Breaker",
                quote: "The floor remembers me.",
                style: "Toon-physics dunks. Turns the paint into a cutscene.",
                height_m: 2.04,
                speed: 68,
                three: 32,
                mid: 62,
                dunk: 99,
                handle: 55,
                pass: 48,
                steal: 40,
                block: 78,
                rebound: 86,
                strength: 97,
                hair: HairStyle::Buzz,
                hair_color: Color::srgb(0.12, 0.12, 0.14),
                skin: Color::srgb(0.55, 0.38, 0.28),
                accent: Color::srgb(1.0, 0.45, 0.1),
                eye: Color::srgb(0.95, 0.85, 0.2),
            },
            Self::ReiWall => CharacterProfile {
                id: self,
                name: "REI WALL",
                alias: "The Eclipse",
                quote: "This paint is a locked gate.",
                style: "Shot-blocking center. Quiet, terrifying help D.",
                height_m: 2.11,
                speed: 54,
                three: 28,
                mid: 55,
                dunk: 80,
                handle: 42,
                pass: 50,
                steal: 36,
                block: 99,
                rebound: 96,
                strength: 94,
                hair: HairStyle::Long,
                hair_color: Color::srgb(0.08, 0.08, 0.12),
                skin: Color::srgb(0.93, 0.86, 0.8),
                accent: Color::srgb(0.55, 0.35, 1.0),
                eye: Color::srgb(0.7, 0.55, 1.0),
            },
            Self::YunaSilk => CharacterProfile {
                id: self,
                name: "YUNA SILK",
                alias: "No-Look Queen",
                quote: "Don't watch the ball. Watch the idea.",
                style: "Playmaker who threads bounce passes through traffic.",
                height_m: 1.74,
                speed: 88,
                three: 74,
                mid: 86,
                dunk: 30,
                handle: 96,
                pass: 99,
                steal: 84,
                block: 18,
                rebound: 40,
                strength: 44,
                hair: HairStyle::Ponytail,
                hair_color: Color::srgb(0.98, 0.92, 0.55),
                skin: Color::srgb(0.98, 0.86, 0.78),
                accent: Color::srgb(1.0, 0.9, 0.3),
                eye: Color::srgb(0.2, 0.7, 0.45),
            },
            Self::ZeroGhost => CharacterProfile {
                id: self,
                name: "ZERO GHOST",
                alias: "The Pickpocket",
                quote: "I was never there. Your ball was.",
                style: "Lockdown wing. Steals that trigger the broadcast graphic.",
                height_m: 1.96,
                speed: 90,
                three: 70,
                mid: 80,
                dunk: 74,
                handle: 82,
                pass: 68,
                steal: 99,
                block: 60,
                rebound: 58,
                strength: 72,
                hair: HairStyle::Messy,
                hair_color: Color::srgb(0.55, 0.15, 0.7),
                skin: Color::srgb(0.78, 0.62, 0.5),
                accent: Color::srgb(0.8, 0.2, 1.0),
                eye: Color::srgb(0.9, 0.9, 1.0),
            },
            Self::LunaEclipse => CharacterProfile {
                id: self,
                name: "LUNA ECLIPSE",
                alias: "All-Court Witch",
                quote: "Every possession is a ritual.",
                style: "Two-way wing, night-sky aesthetic, clutch gene.",
                height_m: 1.88,
                speed: 84,
                three: 86,
                mid: 84,
                dunk: 70,
                handle: 80,
                pass: 78,
                steal: 74,
                block: 55,
                rebound: 64,
                strength: 70,
                hair: HairStyle::Bob,
                hair_color: Color::srgb(0.25, 0.2, 0.55),
                skin: Color::srgb(0.94, 0.84, 0.78),
                accent: Color::srgb(0.45, 0.7, 1.0),
                eye: Color::srgb(0.7, 0.85, 1.0),
            },
            Self::TaroTitan => CharacterProfile {
                id: self,
                name: "TARO TITAN",
                alias: "Glass Cleaner",
                quote: "The miss belongs to me.",
                style: "Old-soul center. Offensive boards and put-backs.",
                height_m: 2.16,
                speed: 48,
                three: 18,
                mid: 50,
                dunk: 88,
                handle: 35,
                pass: 40,
                steal: 28,
                block: 84,
                rebound: 99,
                strength: 99,
                hair: HairStyle::Afro,
                hair_color: Color::srgb(0.2, 0.12, 0.08),
                skin: Color::srgb(0.72, 0.5, 0.36),
                accent: Color::srgb(0.9, 0.2, 0.2),
                eye: Color::srgb(0.4, 0.2, 0.1),
            },
            Self::AikoPrism => CharacterProfile {
                id: self,
                name: "AIKO PRISM",
                alias: "Highlight Reel",
                quote: "Make it look impossible, then easy.",
                style: "And-1 handles, rainbow floaters, twin drills.",
                height_m: 1.7,
                speed: 92,
                three: 88,
                mid: 94,
                dunk: 55,
                handle: 98,
                pass: 84,
                steal: 78,
                block: 20,
                rebound: 36,
                strength: 46,
                hair: HairStyle::Drills,
                hair_color: Color::srgb(0.15, 0.95, 0.55),
                skin: Color::srgb(0.99, 0.9, 0.86),
                accent: Color::srgb(0.2, 1.0, 0.55),
                eye: Color::srgb(0.1, 0.9, 0.5),
            },
            Self::KenjiVolt => CharacterProfile {
                id: self,
                name: "KENJI VOLT",
                alias: "Baseline Lightning",
                quote: "Catch me on the rise.",
                style: "Slash-first wing. Tomahawks and transition hammers.",
                height_m: 1.93,
                speed: 94,
                three: 66,
                mid: 76,
                dunk: 92,
                handle: 78,
                pass: 62,
                steal: 72,
                block: 48,
                rebound: 60,
                strength: 80,
                hair: HairStyle::Mohawk,
                hair_color: Color::srgb(1.0, 0.9, 0.2),
                skin: Color::srgb(0.9, 0.74, 0.6),
                accent: Color::srgb(1.0, 0.85, 0.1),
                eye: Color::srgb(1.0, 0.7, 0.1),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HairStyle {
    /// Upswept anime spikes fanning back from the hairline.
    Spikes,
    /// Two swinging tails tied high on the head.
    TwinTails,
    /// Tight buzz cut hugging the skull.
    Buzz,
    /// Long straight curtain with swept bangs.
    Long,
    /// High ponytail with a swinging tail.
    Ponytail,
    /// Messy layered mop with swept bangs.
    Messy,
    /// Chin-length bob with a fringe.
    Bob,
    /// Round afro worn over a bandana.
    Afro,
    /// Twin corkscrew drills hanging past the shoulders.
    Drills,
    /// Tall lightning-shaped mohawk.
    Mohawk,
}

/// Which limbs a piece of kit is worn on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wear {
    None,
    Left,
    Right,
    Both,
}

impl Wear {
    /// `sx` is -1 for the left limb and +1 for the right one.
    pub fn on(self, sx: f32) -> bool {
        match self {
            Wear::None => false,
            Wear::Left => sx < 0.0,
            Wear::Right => sx > 0.0,
            Wear::Both => true,
        }
    }
}

/// Cosmetic loadout: compression gear, ink, socks and the shoe colorway.
#[derive(Clone, Copy, Debug)]
pub struct Kit {
    pub arm_sleeve: Wear,
    pub knee_sleeve: Wear,
    pub tattoo: Wear,
    pub tights: bool,
    pub headband: bool,
    pub high_socks: bool,
    pub shoe_primary: Color,
    pub shoe_secondary: Color,
    /// 0 = fist pump, 1 = double-bicep flex, 2 = point to the crowd.
    pub celebration: u8,
}

impl CharacterId {
    pub fn kit(self) -> Kit {
        let p = self.profile();
        let white = Color::srgb(0.96, 0.96, 0.94);
        let black = Color::srgb(0.08, 0.08, 0.1);
        match self {
            Self::KaitoFlash => Kit {
                arm_sleeve: Wear::Right,
                knee_sleeve: Wear::None,
                tattoo: Wear::None,
                tights: false,
                headband: true,
                high_socks: false,
                shoe_primary: p.accent,
                shoe_secondary: white,
                celebration: 0,
            },
            Self::MikaOrbit => Kit {
                arm_sleeve: Wear::None,
                knee_sleeve: Wear::Left,
                tattoo: Wear::None,
                tights: true,
                headband: false,
                high_socks: false,
                shoe_primary: white,
                shoe_secondary: p.accent,
                celebration: 2,
            },
            Self::JinGravity => Kit {
                arm_sleeve: Wear::None,
                knee_sleeve: Wear::None,
                tattoo: Wear::Both,
                tights: false,
                headband: true,
                high_socks: false,
                shoe_primary: black,
                shoe_secondary: p.accent,
                celebration: 1,
            },
            Self::ReiWall => Kit {
                arm_sleeve: Wear::Both,
                knee_sleeve: Wear::None,
                tattoo: Wear::None,
                tights: false,
                headband: false,
                high_socks: true,
                shoe_primary: black,
                shoe_secondary: p.accent,
                celebration: 1,
            },
            Self::YunaSilk => Kit {
                arm_sleeve: Wear::None,
                knee_sleeve: Wear::Right,
                tattoo: Wear::None,
                tights: false,
                headband: true,
                high_socks: false,
                shoe_primary: p.accent,
                shoe_secondary: white,
                celebration: 2,
            },
            Self::ZeroGhost => Kit {
                arm_sleeve: Wear::Left,
                knee_sleeve: Wear::None,
                tattoo: Wear::Right,
                tights: true,
                headband: false,
                high_socks: false,
                shoe_primary: black,
                shoe_secondary: p.accent,
                celebration: 0,
            },
            Self::LunaEclipse => Kit {
                arm_sleeve: Wear::None,
                knee_sleeve: Wear::Both,
                tattoo: Wear::None,
                tights: true,
                headband: false,
                high_socks: false,
                shoe_primary: p.accent,
                shoe_secondary: black,
                celebration: 2,
            },
            Self::TaroTitan => Kit {
                arm_sleeve: Wear::None,
                knee_sleeve: Wear::Both,
                tattoo: Wear::Both,
                tights: false,
                headband: true,
                high_socks: true,
                shoe_primary: p.accent,
                shoe_secondary: white,
                celebration: 1,
            },
            Self::AikoPrism => Kit {
                arm_sleeve: Wear::None,
                knee_sleeve: Wear::Left,
                tattoo: Wear::None,
                tights: false,
                headband: false,
                high_socks: true,
                shoe_primary: white,
                shoe_secondary: p.accent,
                celebration: 2,
            },
            Self::KenjiVolt => Kit {
                arm_sleeve: Wear::Right,
                knee_sleeve: Wear::None,
                tattoo: Wear::Left,
                tights: false,
                headband: true,
                high_socks: false,
                shoe_primary: black,
                shoe_secondary: p.accent,
                celebration: 0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CharacterProfile {
    pub id: CharacterId,
    pub name: &'static str,
    pub alias: &'static str,
    pub quote: &'static str,
    pub style: &'static str,
    pub height_m: f32,
    pub speed: u8,
    pub three: u8,
    pub mid: u8,
    pub dunk: u8,
    pub handle: u8,
    pub pass: u8,
    pub steal: u8,
    pub block: u8,
    pub rebound: u8,
    pub strength: u8,
    pub hair: HairStyle,
    pub hair_color: Color,
    pub skin: Color,
    pub accent: Color,
    pub eye: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Home,
    Away,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Home => Side::Away,
            Side::Away => Side::Home,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Side::Home => "NEON FOXES",
            Side::Away => "SHADOW CRANES",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Side::Home => "FOX",
            Side::Away => "CRN",
        }
    }

    pub fn primary(self) -> Color {
        match self {
            Side::Home => Color::srgb(0.05, 0.85, 0.95),
            Side::Away => Color::srgb(0.62, 0.22, 1.0),
        }
    }

    pub fn secondary(self) -> Color {
        match self {
            Side::Home => Color::srgb(1.0, 0.2, 0.55),
            Side::Away => Color::srgb(0.08, 0.05, 0.16),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roster_is_ten_unique_names() {
        let mut names: Vec<_> = CharacterId::ALL.iter().map(|c| c.profile().name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 10);
    }

    #[test]
    fn sniper_has_elite_three() {
        assert!(CharacterId::MikaOrbit.profile().three >= 95);
    }

    #[test]
    fn wear_sides_resolve() {
        assert!(Wear::Both.on(-1.0) && Wear::Both.on(1.0));
        assert!(Wear::Left.on(-1.0) && !Wear::Left.on(1.0));
        assert!(!Wear::Right.on(-1.0) && Wear::Right.on(1.0));
        assert!(!Wear::None.on(-1.0) && !Wear::None.on(1.0));
    }

    #[test]
    fn every_character_has_a_kit_and_distinct_hair() {
        let mut hair: Vec<_> = CharacterId::ALL.iter().map(|c| c.profile().hair).collect();
        hair.dedup();
        assert_eq!(hair.len(), 10, "each character wears a unique hairstyle");
        for c in CharacterId::ALL {
            assert!(c.kit().celebration <= 2);
        }
    }
}
