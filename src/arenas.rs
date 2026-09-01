use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArenaId {
    NeoTokyo,
    ToonWorld,
    SkyTemple,
    Underground,
    CrystalColiseum,
}

impl ArenaId {
    pub const ALL: [ArenaId; 5] = [
        Self::NeoTokyo,
        Self::ToonWorld,
        Self::SkyTemple,
        Self::Underground,
        Self::CrystalColiseum,
    ];

    pub fn theme(self) -> ArenaTheme {
        match self {
            Self::NeoTokyo => ArenaTheme {
                id: self,
                name: "NEO-TOKYO DOME",
                subtitle: "Rain-slick cyber court. Broadcast from Shibuya skyline.",
                floor_a: Color::srgb(0.04, 0.07, 0.14),
                floor_b: Color::srgb(0.08, 0.04, 0.12),
                line: Color::srgb(0.2, 0.95, 1.0),
                paint: Color::srgb(0.12, 0.35, 0.55),
                emissive: LinearRgba::new(0.1, 1.4, 2.0, 1.0),
                ambient: Color::srgb(0.25, 0.45, 0.7),
                fog: Color::srgb(0.02, 0.04, 0.1),
                bounce: 0.72,
                hangtime: 1.0,
                crowd: Color::srgb(0.08, 0.12, 0.22),
                sky: Color::srgb(0.015, 0.02, 0.05),
            },
            Self::ToonWorld => ArenaTheme {
                id: self,
                name: "TOON WORLD ARENA",
                subtitle: "Looney physics. The rim is a punchline waiting to happen.",
                floor_a: Color::srgb(0.95, 0.55, 0.15),
                floor_b: Color::srgb(0.2, 0.45, 0.95),
                line: Color::srgb(1.0, 1.0, 1.0),
                paint: Color::srgb(0.95, 0.2, 0.25),
                emissive: LinearRgba::new(2.0, 1.2, 0.2, 1.0),
                ambient: Color::srgb(0.95, 0.85, 0.55),
                fog: Color::srgb(0.55, 0.75, 1.0),
                bounce: 0.88,
                hangtime: 1.22,
                crowd: Color::srgb(0.85, 0.25, 0.55),
                sky: Color::srgb(0.45, 0.75, 1.0),
            },
            Self::SkyTemple => ArenaTheme {
                id: self,
                name: "SKY TEMPLE COURT",
                subtitle: "Moonlit hardwood above the clouds. Cherry-petal drift.",
                floor_a: Color::srgb(0.28, 0.12, 0.16),
                floor_b: Color::srgb(0.18, 0.08, 0.14),
                line: Color::srgb(1.0, 0.82, 0.55),
                paint: Color::srgb(0.45, 0.12, 0.18),
                emissive: LinearRgba::new(1.6, 0.7, 1.4, 1.0),
                ambient: Color::srgb(0.45, 0.35, 0.7),
                fog: Color::srgb(0.08, 0.05, 0.14),
                bounce: 0.7,
                hangtime: 1.12,
                crowd: Color::srgb(0.22, 0.1, 0.18),
                sky: Color::srgb(0.04, 0.03, 0.12),
            },
            Self::Underground => ArenaTheme {
                id: self,
                name: "UNDERGROUND CIRCUIT",
                subtitle: "Chain nets, graffiti walls, illegal wattage.",
                floor_a: Color::srgb(0.18, 0.16, 0.14),
                floor_b: Color::srgb(0.12, 0.11, 0.1),
                line: Color::srgb(1.0, 0.85, 0.2),
                paint: Color::srgb(0.05, 0.05, 0.05),
                emissive: LinearRgba::new(1.8, 1.1, 0.15, 1.0),
                ambient: Color::srgb(0.5, 0.4, 0.25),
                fog: Color::srgb(0.04, 0.04, 0.03),
                bounce: 0.64,
                hangtime: 0.96,
                crowd: Color::srgb(0.12, 0.12, 0.1),
                sky: Color::srgb(0.03, 0.03, 0.025),
            },
            Self::CrystalColiseum => ArenaTheme {
                id: self,
                name: "CRYSTAL COLISEUM",
                subtitle: "Esports cathedral. Holo-ads, glass floor, 40k headsets.",
                floor_a: Color::srgb(0.12, 0.16, 0.22),
                floor_b: Color::srgb(0.18, 0.14, 0.28),
                line: Color::srgb(0.75, 0.95, 1.0),
                paint: Color::srgb(0.25, 0.15, 0.45),
                emissive: LinearRgba::new(0.6, 1.6, 2.2, 1.0),
                ambient: Color::srgb(0.55, 0.7, 0.95),
                fog: Color::srgb(0.05, 0.07, 0.12),
                bounce: 0.74,
                hangtime: 1.04,
                crowd: Color::srgb(0.08, 0.1, 0.18),
                sky: Color::srgb(0.02, 0.03, 0.07),
            },
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArenaTheme {
    pub id: ArenaId,
    pub name: &'static str,
    pub subtitle: &'static str,
    pub floor_a: Color,
    pub floor_b: Color,
    pub line: Color,
    pub paint: Color,
    pub emissive: LinearRgba,
    pub ambient: Color,
    pub fog: Color,
    pub bounce: f32,
    pub hangtime: f32,
    pub crowd: Color,
    pub sky: Color,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_unique_arenas() {
        assert_eq!(ArenaId::ALL.len(), 5);
        assert!(ArenaId::ToonWorld.theme().bounce > ArenaId::Underground.theme().bounce);
    }
}
