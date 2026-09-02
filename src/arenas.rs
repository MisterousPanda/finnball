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
                floor_a: Color::srgb(0.38, 0.23, 0.14),
                floor_b: Color::srgb(0.27, 0.15, 0.09),
                accent: Color::srgb(0.1, 0.95, 1.0),
                apron: Color::srgb(0.05, 0.07, 0.13),
                line: Color::srgb(0.2, 0.95, 1.0),
                paint: Color::srgb(0.12, 0.35, 0.55),
                emissive: LinearRgba::new(0.1, 1.4, 2.0, 1.0),
                ambient: Color::srgb(0.25, 0.45, 0.7),
                fog: Color::srgb(0.02, 0.04, 0.1),
                bounce: 0.72,
                hangtime: 1.0,
                crowd: Color::srgb(0.08, 0.12, 0.22),
                roof: Color::srgb(0.02, 0.03, 0.06),
                banner_years: &["2088", "2091", "2093", "2094", "2097"],
                retired: &[("7", "AKIRA"), ("23", "HANA"), ("0", "GHOST")],
                ribbon_words: &["FINNBALL", "NEO-TOKYO DOME", "NEON FOXES", "SHADOW CRANES", "SHIBUYA NIGHTS"],
                suite_glow: Color::srgb(0.3, 0.8, 1.0),
                sky: Color::srgb(0.015, 0.02, 0.05),
            },
            Self::ToonWorld => ArenaTheme {
                id: self,
                name: "TOON WORLD ARENA",
                subtitle: "Looney physics. The rim is a punchline waiting to happen.",
                floor_a: Color::srgb(0.93, 0.68, 0.34),
                floor_b: Color::srgb(0.82, 0.55, 0.24),
                accent: Color::srgb(1.0, 0.9, 0.2),
                apron: Color::srgb(0.16, 0.36, 0.85),
                line: Color::srgb(1.0, 1.0, 1.0),
                paint: Color::srgb(0.95, 0.2, 0.25),
                emissive: LinearRgba::new(2.0, 1.2, 0.2, 1.0),
                ambient: Color::srgb(0.95, 0.85, 0.55),
                fog: Color::srgb(0.55, 0.75, 1.0),
                bounce: 0.88,
                hangtime: 1.22,
                crowd: Color::srgb(0.85, 0.25, 0.55),
                roof: Color::srgb(0.55, 0.8, 1.0),
                banner_years: &["1940", "1953", "1966", "1988", "1996"],
                retired: &[("1", "BUGS"), ("99", "DUCK"), ("33", "TAZ")],
                ribbon_words: &["FINNBALL", "TOON WORLD ARENA", "NEON FOXES", "SHADOW CRANES", "THAT'S ALL FOLKS"],
                suite_glow: Color::srgb(1.0, 0.85, 0.4),
                sky: Color::srgb(0.45, 0.75, 1.0),
            },
            Self::SkyTemple => ArenaTheme {
                id: self,
                name: "SKY TEMPLE COURT",
                subtitle: "Moonlit hardwood above the clouds. Cherry-petal drift.",
                floor_a: Color::srgb(0.56, 0.24, 0.2),
                floor_b: Color::srgb(0.44, 0.17, 0.15),
                accent: Color::srgb(1.0, 0.55, 0.8),
                apron: Color::srgb(0.12, 0.06, 0.15),
                line: Color::srgb(1.0, 0.82, 0.55),
                paint: Color::srgb(0.45, 0.12, 0.18),
                emissive: LinearRgba::new(1.6, 0.7, 1.4, 1.0),
                ambient: Color::srgb(0.45, 0.35, 0.7),
                fog: Color::srgb(0.08, 0.05, 0.14),
                bounce: 0.7,
                hangtime: 1.12,
                crowd: Color::srgb(0.22, 0.1, 0.18),
                roof: Color::srgb(0.05, 0.03, 0.12),
                banner_years: &["1603", "1868", "1964", "2020", "2025"],
                retired: &[("5", "SAKURA"), ("11", "TSUKI"), ("77", "KAGE")],
                ribbon_words: &["FINNBALL", "SKY TEMPLE COURT", "NEON FOXES", "SHADOW CRANES", "MOONLIGHT LEAGUE"],
                suite_glow: Color::srgb(1.0, 0.6, 0.8),
                sky: Color::srgb(0.04, 0.03, 0.12),
            },
            Self::Underground => ArenaTheme {
                id: self,
                name: "UNDERGROUND CIRCUIT",
                subtitle: "Chain nets, graffiti walls, illegal wattage.",
                floor_a: Color::srgb(0.32, 0.31, 0.3),
                floor_b: Color::srgb(0.24, 0.24, 0.25),
                accent: Color::srgb(1.0, 0.85, 0.2),
                apron: Color::srgb(0.08, 0.08, 0.07),
                line: Color::srgb(1.0, 0.85, 0.2),
                paint: Color::srgb(0.05, 0.05, 0.05),
                emissive: LinearRgba::new(1.8, 1.1, 0.15, 1.0),
                ambient: Color::srgb(0.5, 0.4, 0.25),
                fog: Color::srgb(0.04, 0.04, 0.03),
                bounce: 0.64,
                hangtime: 0.96,
                crowd: Color::srgb(0.12, 0.12, 0.1),
                roof: Color::srgb(0.04, 0.04, 0.035),
                banner_years: &["1979", "1984", "1991", "2004", "2016"],
                retired: &[("13", "REBEL"), ("44", "KING"), ("8", "WIRE")],
                ribbon_words: &["FINNBALL", "UNDERGROUND CIRCUIT", "NEON FOXES", "SHADOW CRANES", "NO REFS NO RULES"],
                suite_glow: Color::srgb(1.0, 0.75, 0.25),
                sky: Color::srgb(0.03, 0.03, 0.025),
            },
            Self::CrystalColiseum => ArenaTheme {
                id: self,
                name: "CRYSTAL COLISEUM",
                subtitle: "Esports cathedral. Holo-ads, glass floor, 40k headsets.",
                floor_a: Color::srgb(0.8, 0.68, 0.5),
                floor_b: Color::srgb(0.7, 0.58, 0.4),
                accent: Color::srgb(0.6, 1.0, 1.0),
                apron: Color::srgb(0.1, 0.12, 0.2),
                line: Color::srgb(0.75, 0.95, 1.0),
                paint: Color::srgb(0.25, 0.15, 0.45),
                emissive: LinearRgba::new(0.6, 1.6, 2.2, 1.0),
                ambient: Color::srgb(0.55, 0.7, 0.95),
                fog: Color::srgb(0.05, 0.07, 0.12),
                bounce: 0.74,
                hangtime: 1.04,
                crowd: Color::srgb(0.08, 0.1, 0.18),
                roof: Color::srgb(0.03, 0.05, 0.1),
                banner_years: &["2019", "2021", "2022", "2024", "2026"],
                retired: &[("1", "PRISM"), ("10", "NOVA"), ("42", "GLITCH")],
                ribbon_words: &["FINNBALL", "CRYSTAL COLISEUM", "NEON FOXES", "SHADOW CRANES", "GG WORLDS"],
                suite_glow: Color::srgb(0.5, 0.9, 1.0),
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
    pub accent: Color,
    pub apron: Color,
    pub line: Color,
    pub paint: Color,
    pub emissive: LinearRgba,
    pub ambient: Color,
    pub fog: Color,
    pub bounce: f32,
    pub hangtime: f32,
    pub crowd: Color,
    /// Roof / ceiling colour above the rafters.
    pub roof: Color,
    /// Championship years hung from the truss.
    pub banner_years: &'static [&'static str],
    /// Retired numbers: (number, player name).
    pub retired: &'static [(&'static str, &'static str)],
    /// Words that scroll around the LED ribbon boards.
    pub ribbon_words: &'static [&'static str],
    /// Warm light spilling from the suite-level windows.
    pub suite_glow: Color,
    pub sky: Color,
}

impl ArenaTheme {
    pub fn palette(&self) -> crate::courtpaint::CourtPalette {
        fn c(col: Color) -> [f32; 3] {
            let s = col.to_srgba();
            [s.red, s.green, s.blue]
        }
        crate::courtpaint::CourtPalette {
            wood_a: c(self.floor_a),
            wood_b: c(self.floor_b),
            line: c(self.line),
            paint: c(self.paint),
            accent: c(self.accent),
            apron: c(self.apron),
            arena_name: self.name,
            home_name: crate::roster::Side::Home.label(),
            away_name: crate::roster::Side::Away.label(),
            home_color: c(crate::roster::Side::Home.primary()),
            away_color: c(crate::roster::Side::Away.primary()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_unique_arenas() {
        assert_eq!(ArenaId::ALL.len(), 5);
        assert!(ArenaId::ToonWorld.theme().bounce > ArenaId::Underground.theme().bounce);
    }

    #[test]
    fn every_arena_has_identity_dressing() {
        for id in ArenaId::ALL {
            let t = id.theme();
            assert!(t.banner_years.len() >= 3, "{:?}", id);
            assert!(t.retired.len() >= 2, "{:?}", id);
            assert!(t.ribbon_words.contains(&"FINNBALL"), "{:?}", id);
            assert!(t.ribbon_words.contains(&t.name), "{:?}", id);
            let pal = t.palette();
            assert_eq!(pal.arena_name, t.name);
        }
    }
}
