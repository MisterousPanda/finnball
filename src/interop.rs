//! Stable interchange for Unity, Unreal, and Blender.
//!
//! Court numbers, roster, and arena palettes are authored in Rust. This module
//! dumps them as `interop/finnball.json` so the DCC/engine packages stay in lockstep
//! with Bevy. Coordinates are Y-up metres (Bevy / Unity). Unreal and Blender convert
//! on import — see `docs/dcc.md`.

use std::fmt::Write as _;
use std::path::PathBuf;

use bevy::prelude::Color;

use crate::arenas::ArenaId;
use crate::roster::{CharacterId, Side};
use crate::sim::{
    BACKBOARD_OFFSET, BALL_RADIUS, COURT_HALF_LEN, COURT_HALF_WID, HOOP_X, PAINT_DEPTH,
    PAINT_HALF_WIDTH, PLAYER_RADIUS, RIM_HEIGHT, RIM_RADIUS, THREE_RADIUS, TIPOFF_AWAY,
    TIPOFF_BALL_AWAY, TIPOFF_BALL_HOME, TIPOFF_HOME,
};

pub const SCHEMA: u32 = 1;
pub const MANIFEST_REL: &str = "interop/finnball.json";

pub fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(MANIFEST_REL)
}

pub fn manifest_json() -> String {
    let mut j = Json::new();
    j.begin_object();
    j.key("schema").int(SCHEMA as i64);
    j.key("name").string("FINNBALL");
    j.key("description").string(
        "Anime street basketball interchange. Y-up metres, X along the court, player front is -Z.",
    );
    j.key("engine");
    j.begin_object();
    j.key("bevy").string("0.18");
    j.key("up").string("y");
    j.key("units").string("meters");
    j.key("forward").string("-z");
    j.end_object();

    j.key("court");
    j.begin_object();
    j.key("half_length").num(COURT_HALF_LEN);
    j.key("half_width").num(COURT_HALF_WID);
    j.key("rim_height").num(RIM_HEIGHT);
    j.key("rim_radius").num(RIM_RADIUS);
    j.key("backboard_offset").num(BACKBOARD_OFFSET);
    j.key("hoop_x").num(HOOP_X);
    j.key("ball_radius").num(BALL_RADIUS);
    j.key("three_radius").num(THREE_RADIUS);
    j.key("paint_depth").num(PAINT_DEPTH);
    j.key("paint_half_width").num(PAINT_HALF_WIDTH);
    j.key("player_radius").num(PLAYER_RADIUS);
    j.end_object();

    j.key("hoops");
    j.begin_array();
    hoop(&mut j, "home", -HOOP_X, 1.0);
    hoop(&mut j, "away", HOOP_X, -1.0);
    j.end_array();

    j.key("spawns");
    j.begin_object();
    j.key("home").vec3_list(&TIPOFF_HOME);
    j.key("away").vec3_list(&TIPOFF_AWAY);
    j.key("ball_home").vec3(TIPOFF_BALL_HOME);
    j.key("ball_away").vec3(TIPOFF_BALL_AWAY);
    j.end_object();

    j.key("teams");
    j.begin_array();
    team(&mut j, Side::Home);
    team(&mut j, Side::Away);
    j.end_array();

    j.key("roster");
    j.begin_array();
    for id in CharacterId::ALL {
        character(&mut j, id);
    }
    j.end_array();

    j.key("arenas");
    j.begin_array();
    for id in ArenaId::ALL {
        arena(&mut j, id);
    }
    j.end_array();

    j.end_object();
    j.finish()
}

fn hoop(j: &mut Json, id: &str, x: f32, faces: f32) {
    j.begin_object();
    j.key("id").string(id);
    j.key("position").vec3([x, RIM_HEIGHT, 0.0]);
    j.key("faces").num(faces);
    j.end_object();
}

fn team(j: &mut Json, side: Side) {
    j.begin_object();
    j.key("id").string(match side {
        Side::Home => "home",
        Side::Away => "away",
    });
    j.key("name").string(side.label());
    j.key("abbrev").string(side.short());
    j.key("primary").string(&hex_srgb(side.primary()));
    j.key("secondary").string(&hex_srgb(side.secondary()));
    j.end_object();
}

fn character(j: &mut Json, id: CharacterId) {
    let p = id.profile();
    let kit = id.kit();
    j.begin_object();
    j.key("id").string(id.slug());
    j.key("name").string(p.name);
    j.key("alias").string(p.alias);
    j.key("quote").string(p.quote);
    j.key("style").string(p.style);
    j.key("height_m").num(p.height_m);
    j.key("ratings");
    j.begin_object();
    j.key("speed").int(p.speed as i64);
    j.key("three").int(p.three as i64);
    j.key("mid").int(p.mid as i64);
    j.key("dunk").int(p.dunk as i64);
    j.key("handle").int(p.handle as i64);
    j.key("pass").int(p.pass as i64);
    j.key("steal").int(p.steal as i64);
    j.key("block").int(p.block as i64);
    j.key("rebound").int(p.rebound as i64);
    j.key("strength").int(p.strength as i64);
    j.end_object();
    j.key("look");
    j.begin_object();
    j.key("hair").string(p.hair.slug());
    j.key("hair_color").string(&hex_srgb(p.hair_color));
    j.key("skin").string(&hex_srgb(p.skin));
    j.key("accent").string(&hex_srgb(p.accent));
    j.key("eye").string(&hex_srgb(p.eye));
    j.end_object();
    j.key("kit");
    j.begin_object();
    j.key("arm_sleeve").string(kit.arm_sleeve.slug());
    j.key("knee_sleeve").string(kit.knee_sleeve.slug());
    j.key("tattoo").string(kit.tattoo.slug());
    j.key("tights").bool(kit.tights);
    j.key("headband").bool(kit.headband);
    j.key("high_socks").bool(kit.high_socks);
    j.key("shoe_primary").string(&hex_srgb(kit.shoe_primary));
    j.key("shoe_secondary")
        .string(&hex_srgb(kit.shoe_secondary));
    j.key("celebration").int(kit.celebration as i64);
    j.end_object();
    j.end_object();
}

fn arena(j: &mut Json, id: ArenaId) {
    let t = id.theme();
    j.begin_object();
    j.key("id").string(id.slug());
    j.key("name").string(t.name);
    j.key("subtitle").string(t.subtitle);
    j.key("bounce").num(t.bounce);
    j.key("hangtime").num(t.hangtime);
    j.key("env_pano").optional_string(t.env_pano);
    j.key("palette");
    j.begin_object();
    j.key("floor_a").string(&hex_srgb(t.floor_a));
    j.key("floor_b").string(&hex_srgb(t.floor_b));
    j.key("accent").string(&hex_srgb(t.accent));
    j.key("apron").string(&hex_srgb(t.apron));
    j.key("line").string(&hex_srgb(t.line));
    j.key("paint").string(&hex_srgb(t.paint));
    j.key("ambient").string(&hex_srgb(t.ambient));
    j.key("fog").string(&hex_srgb(t.fog));
    j.key("crowd").string(&hex_srgb(t.crowd));
    j.key("roof").string(&hex_srgb(t.roof));
    j.key("sky").string(&hex_srgb(t.sky));
    j.key("suite_glow").string(&hex_srgb(t.suite_glow));
    j.key("emissive");
    j.begin_array();
    j.num(t.emissive.red);
    j.num(t.emissive.green);
    j.num(t.emissive.blue);
    j.num(t.emissive.alpha);
    j.end_array();
    j.end_object();
    j.key("banner_years").strings(t.banner_years);
    j.key("retired");
    j.begin_array();
    for (num, name) in t.retired {
        j.begin_object();
        j.key("number").string(num);
        j.key("name").string(name);
        j.end_object();
    }
    j.end_array();
    j.key("ribbon_words").strings(t.ribbon_words);
    j.end_object();
}

pub fn hex_srgb(color: Color) -> String {
    let s = color.to_srgba();
    format!(
        "#{:02x}{:02x}{:02x}",
        (s.red * 255.0).round().clamp(0.0, 255.0) as u8,
        (s.green * 255.0).round().clamp(0.0, 255.0) as u8,
        (s.blue * 255.0).round().clamp(0.0, 255.0) as u8
    )
}

struct Json {
    out: String,
    stack: Vec<Frame>,
    indent: usize,
}

enum Frame {
    Object { first: bool },
    Array { first: bool },
}

impl Json {
    fn new() -> Self {
        Self {
            out: String::with_capacity(16 * 1024),
            stack: Vec::new(),
            indent: 0,
        }
    }

    fn begin_object(&mut self) -> &mut Self {
        self.prepare_value();
        self.out.push('{');
        self.stack.push(Frame::Object { first: true });
        self.indent += 1;
        self
    }

    fn end_object(&mut self) -> &mut Self {
        self.indent -= 1;
        self.stack.pop();
        self.newline();
        self.out.push('}');
        self
    }

    fn begin_array(&mut self) -> &mut Self {
        self.prepare_value();
        self.out.push('[');
        self.stack.push(Frame::Array { first: true });
        self.indent += 1;
        self
    }

    fn end_array(&mut self) -> &mut Self {
        self.indent -= 1;
        self.stack.pop();
        self.newline();
        self.out.push(']');
        self
    }

    fn key(&mut self, k: &str) -> &mut Self {
        self.comma();
        self.newline();
        write_escaped(&mut self.out, k);
        self.out.push_str(": ");
        self
    }

    fn string(&mut self, s: &str) -> &mut Self {
        self.prepare_value();
        write_escaped(&mut self.out, s);
        self
    }

    fn optional_string(&mut self, s: Option<&str>) -> &mut Self {
        match s {
            Some(s) => self.string(s),
            None => {
                self.prepare_value();
                self.out.push_str("null");
                self
            }
        }
    }

    fn bool(&mut self, v: bool) -> &mut Self {
        self.prepare_value();
        self.out.push_str(if v { "true" } else { "false" });
        self
    }

    fn int(&mut self, n: i64) -> &mut Self {
        self.prepare_value();
        let _ = write!(self.out, "{n}");
        self
    }

    fn num(&mut self, n: f32) -> &mut Self {
        self.prepare_value();
        write_f32(&mut self.out, n);
        self
    }

    fn vec3(&mut self, p: [f32; 3]) -> &mut Self {
        self.begin_object();
        self.key("x").num(p[0]);
        self.key("y").num(p[1]);
        self.key("z").num(p[2]);
        self.end_object();
        self
    }

    fn vec3_list(&mut self, pts: &[[f32; 3]]) -> &mut Self {
        self.begin_array();
        for p in pts {
            self.vec3(*p);
        }
        self.end_array();
        self
    }

    fn strings(&mut self, items: &[&str]) -> &mut Self {
        self.begin_array();
        for s in items {
            self.string(s);
        }
        self.end_array();
        self
    }

    fn prepare_value(&mut self) {
        if let Some(Frame::Array { first }) = self.stack.last_mut() {
            if *first {
                *first = false;
            } else {
                self.out.push(',');
            }
            self.newline();
        }
    }

    fn comma(&mut self) {
        if let Some(Frame::Object { first }) = self.stack.last_mut() {
            if *first {
                *first = false;
            } else {
                self.out.push(',');
            }
        }
    }

    fn newline(&mut self) {
        self.out.push('\n');
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
    }

    fn finish(self) -> String {
        let mut s = self.out;
        if !s.ends_with('\n') {
            s.push('\n');
        }
        s
    }
}

fn write_f32(out: &mut String, n: f32) {
    if !n.is_finite() {
        out.push_str("0");
        return;
    }
    if n == n.round() && n.abs() < 1_000_000.0 {
        let _ = write!(out, "{n:.1}");
        return;
    }
    let raw = format!("{n:.6}");
    let trimmed = raw.trim_end_matches('0').trim_end_matches('.');
    out.push_str(trimmed);
}

fn write_escaped(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_rrggbb() {
        assert_eq!(hex_srgb(Color::srgb(1.0, 0.0, 0.5)), "#ff0080");
        assert_eq!(hex_srgb(Color::srgb(0.0, 0.0, 0.0)), "#000000");
    }

    #[test]
    fn manifest_covers_the_shipped_game() {
        let json = manifest_json();
        assert!(json.contains("\"schema\": 1"));
        assert!(json.contains("\"half_length\": 14.0"));
        assert!(json.contains("\"half_width\": 7.5"));
        assert!(json.contains("\"rim_height\": 3.05"));
        assert!(json.contains("\"kaito_flash\""));
        assert!(json.contains("\"neo_tokyo\""));
        assert!(json.contains("KAITO FLASH"));
        assert!(json.contains("NEO-TOKYO DOME"));
        assert_eq!(json.matches("\"id\": \"kaito_flash\"").count(), 1);
        assert_eq!(CharacterId::ALL.len(), 10);
        assert_eq!(ArenaId::ALL.len(), 5);
        for id in CharacterId::ALL {
            assert!(json.contains(&format!("\"id\": \"{}\"", id.slug())));
        }
        for id in ArenaId::ALL {
            assert!(json.contains(&format!("\"id\": \"{}\"", id.slug())));
        }
    }

    #[test]
    fn committed_manifest_matches_source() {
        let expected = manifest_json();
        let path = manifest_path();
        if std::env::var("UPDATE_INTEROP").is_ok() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&path, &expected).unwrap();
            return;
        }
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            on_disk, expected,
            "interop/finnball.json is stale — run UPDATE_INTEROP=1 cargo test interop::"
        );
    }
}
