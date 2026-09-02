//! Procedural painters: the hardwood court texture plus the rafters banners, LED ribbon
//! strips and the jumbotron scoreboard. Pure pixel math — no ECS — so everything here is
//! unit-testable and runs identically on native and WASM.

use crate::sim::{
    COURT_HALF_LEN, COURT_HALF_WID, HOOP_X, PAINT_DEPTH, PAINT_HALF_WIDTH, THREE_RADIUS,
};

/// Extra floor beyond the sidelines / baselines, in meters.
pub const APRON: f32 = 1.6;
pub const PLANE_HALF_LEN: f32 = COURT_HALF_LEN + APRON;
pub const PLANE_HALF_WID: f32 = COURT_HALF_WID + APRON;

const LINE_W: f32 = 0.05;
const PLANK_W: f32 = 0.115;
const PLANK_SEAM: f32 = 0.006;
const RESTRICTED_R: f32 = 1.25;
const FT_CIRCLE_R: f32 = 1.8;
const CENTER_R: f32 = 1.8;
/// Three-point straight segments run at this |z| until they meet the arc.
const CORNER_Z: f32 = COURT_HALF_WID - 0.9;

#[derive(Clone, Copy, Debug)]
pub struct CourtPalette {
    pub wood_a: [f32; 3],
    pub wood_b: [f32; 3],
    pub line: [f32; 3],
    pub paint: [f32; 3],
    pub accent: [f32; 3],
    pub apron: [f32; 3],
    /// Stencilled along the sideline aprons next to the FINNBALL wordmark.
    pub arena_name: &'static str,
    /// Team names stencilled on the baseline aprons (home end, away end).
    pub home_name: &'static str,
    pub away_name: &'static str,
    pub home_color: [f32; 3],
    pub away_color: [f32; 3],
}

pub struct CourtImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn hash2(a: i32, b: i32) -> f32 {
    let mut h = (a as u32).wrapping_mul(0x8da6_b343) ^ (b as u32).wrapping_mul(0xd8163841);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65535.0
}

pub fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn scale3(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn seg_dist(px: f32, pz: f32, ax: f32, az: f32, bx: f32, bz: f32) -> f32 {
    let vx = bx - ax;
    let vz = bz - az;
    let wx = px - ax;
    let wz = pz - az;
    let len2 = vx * vx + vz * vz;
    let t = if len2 <= 1e-6 {
        0.0
    } else {
        ((wx * vx + wz * vz) / len2).clamp(0.0, 1.0)
    };
    let cx = ax + vx * t;
    let cz = az + vz * t;
    ((px - cx).powi(2) + (pz - cz).powi(2)).sqrt()
}

fn ring_dist(px: f32, pz: f32, cx: f32, cz: f32, r: f32) -> f32 {
    (((px - cx).powi(2) + (pz - cz).powi(2)).sqrt() - r).abs()
}

/// Distance to the three-point line for the hoop at `hx` (sign = which end).
fn three_dist(px: f32, pz: f32, hx: f32) -> f32 {
    let sign = hx.signum();
    let dz_corner = (THREE_RADIUS * THREE_RADIUS - CORNER_Z * CORNER_Z).sqrt();
    let corner_x = hx - sign * dz_corner;
    let mut d = f32::MAX;
    if pz.abs() <= CORNER_Z || (px - hx) * sign < 0.0 && pz.abs() < THREE_RADIUS {
        // Arc region: only the half facing midcourt
        let ang_ok = (px - hx) * sign <= 0.0;
        if ang_ok {
            d = d.min(ring_dist(px, pz, hx, 0.0, THREE_RADIUS));
        }
    }
    for s in [-1.0, 1.0] {
        d = d.min(seg_dist(
            px,
            pz,
            corner_x,
            s * CORNER_Z,
            sign * COURT_HALF_LEN,
            s * CORNER_Z,
        ));
    }
    d
}

fn beyond_arc(px: f32, pz: f32, hx: f32) -> bool {
    let sign = hx.signum();
    if pz.abs() > CORNER_Z {
        return true;
    }
    let dist = ((px - hx).powi(2) + pz * pz).sqrt();
    (px - hx) * sign <= 0.0 && dist >= THREE_RADIUS
}

fn cover(d: f32, half_w: f32, px_size: f32) -> f32 {
    ((half_w - d) / px_size + 0.5).clamp(0.0, 1.0)
}

/// 5x7 glyphs (MSB = left column) for everything we stencil, paint or scroll.
pub fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111],
        'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100],
        '\'' => [0b00100, 0b00100, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        // diamond separator
        '*' => [0b00100, 0b01110, 0b11111, 0b11111, 0b11111, 0b01110, 0b00100],
        // filled block
        '#' => [0b11111, 0b11111, 0b11111, 0b11111, 0b11111, 0b11111, 0b11111],
        _ => [0; 7],
    }
}

/// Coverage of `text` in a local text frame: `u` runs along the reading direction and
/// `v` increases down the rows. Glyph cells are `cell` units square, advance is 6 cells.
fn text_cover_uv(u: f32, v: f32, text: &str, u0: f32, v0: f32, cell: f32) -> f32 {
    if v < v0 || v >= v0 + 7.0 * cell || u < u0 {
        return 0.0;
    }
    let total = text.chars().count() as f32 * 6.0 * cell;
    if u >= u0 + total {
        return 0.0;
    }
    let mut cu = u0;
    for c in text.chars() {
        let w = 5.0 * cell;
        if u >= cu && u < cu + w {
            let col = (((u - cu) / cell) as usize).min(4);
            let row = (((v - v0) / cell) as usize).min(6);
            let bits = glyph(c)[row];
            return if bits & (1 << (4 - col)) != 0 { 1.0 } else { 0.0 };
        }
        cu += 6.0 * cell;
    }
    0.0
}

/// Text on the floor in world space. `dir` picks the reading direction:
/// 0 = +x (readable from the near sideline), 1 = -x (readable from the far sideline),
/// 2 = +z (readable facing the +x baseline), 3 = -z (readable facing the -x baseline).
/// `(u0, v0)` is the top-left of the text in the local frame; `text_frame` maps a world
/// point into that frame.
fn text_frame(px: f32, pz: f32, dir: u8) -> (f32, f32) {
    match dir {
        0 => (px, pz),
        1 => (-px, -pz),
        2 => (pz, -px),
        _ => (-pz, px),
    }
}

fn text_cover(px: f32, pz: f32, text: &str, u0: f32, v0: f32, cell: f32, dir: u8) -> f32 {
    let (u, v) = text_frame(px, pz, dir);
    text_cover_uv(u, v, text, u0, v0, cell)
}

pub fn text_len(text: &str, cell: f32) -> f32 {
    (text.chars().count() as f32 * 6.0 - 1.0).max(0.0) * cell
}

pub fn paint_court(px_per_m: u32, pal: &CourtPalette) -> CourtImage {
    let width = (PLANE_HALF_LEN * 2.0 * px_per_m as f32).round() as u32;
    let height = (PLANE_HALF_WID * 2.0 * px_per_m as f32).round() as u32;
    let px_size = 1.0 / px_per_m as f32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let line_half = LINE_W * 0.5;
    let ft_x = |sign: f32| sign * (HOOP_X - PAINT_DEPTH);
    let wordmark = "FINNBALL";
    let wm_cell = 0.062;
    let wm_w = text_len(wordmark, wm_cell);

    for py in 0..height {
        let z = -PLANE_HALF_WID + (py as f32 + 0.5) * px_size;
        for px in 0..width {
            let x = -PLANE_HALF_LEN + (px as f32 + 0.5) * px_size;
            let in_court = x.abs() <= COURT_HALF_LEN && z.abs() <= COURT_HALF_WID;

            // --- base: hardwood planks running along x, each with its own grain
            let row = ((z + PLANE_HALF_WID) / PLANK_W).floor() as i32;
            let stagger = hash2(row, 7) * 1.4;
            let seg_len = 1.75;
            let seg = ((x + PLANE_HALF_LEN + stagger) / seg_len).floor() as i32;
            let tone = hash2(row, seg);
            let freq = 26.0 + hash2(row, seg + 101) * 38.0;
            let grain_off = hash2(row, seg + 202) * 6.28;
            let grain = ((x * freq + row as f32 * 3.1 + grain_off).sin() * 0.5 + 0.5) * 0.12
                + ((x * 91.0 + z * 13.0).sin() * 0.5 + 0.5) * 0.05
                + ((x * 7.0 + grain_off).sin() * (z * 55.0).sin() * 0.5 + 0.5) * 0.04;
            let mut col = lerp3(
                pal.wood_a,
                pal.wood_b,
                (tone * 0.75 + grain).clamp(0.0, 1.0),
            );
            // occasional knot: a small dark ellipse somewhere inside the plank
            if hash2(row, seg + 303) > 0.82 {
                let seg_start = seg as f32 * seg_len - stagger - PLANE_HALF_LEN;
                let kx = seg_start + 0.3 + hash2(row, seg + 404) * (seg_len - 0.6);
                let kz = (row as f32 + 0.5) * PLANK_W - PLANE_HALF_WID;
                let dx = (x - kx) / 0.06;
                let dz = (z - kz) / 0.035;
                let d = (dx * dx + dz * dz).sqrt();
                if d < 1.6 {
                    let ring = ((d * 9.0).sin() * 0.5 + 0.5) * 0.3 + 0.5;
                    let k = (1.0 - d / 1.6).clamp(0.0, 1.0) * ring;
                    col = lerp3(col, scale3(pal.wood_b, 0.55), k);
                }
            }
            // seam darkening
            let seam_z =
                ((z + PLANE_HALF_WID) % PLANK_W).min(PLANK_W - ((z + PLANE_HALF_WID) % PLANK_W));
            let seam_x = ((x + PLANE_HALF_LEN + stagger) % seg_len)
                .min(seg_len - ((x + PLANE_HALF_LEN + stagger) % seg_len));
            let seam = cover(seam_z.min(seam_x), PLANK_SEAM, px_size);
            col = lerp3(col, scale3(col, 0.62), seam);

            if !in_court {
                // Apron: darker stain with a soft inner glow band next to the lines
                let edge = (x.abs() - COURT_HALF_LEN)
                    .max(z.abs() - COURT_HALF_WID)
                    .max(0.0);
                let band = (1.0 - edge / 0.35).clamp(0.0, 1.0);
                col = lerp3(pal.apron, lerp3(pal.apron, pal.accent, 0.55), band * 0.6);
                // thin accent pinstripe at the outer edge of the apron
                let outer = (PLANE_HALF_LEN - x.abs()).min(PLANE_HALF_WID - z.abs());
                let pin = cover((outer - 0.18).abs(), 0.02, px_size);
                col = lerp3(col, pal.accent, pin * 0.8);

                if z.abs() > COURT_HALF_WID && x.abs() < COURT_HALF_LEN {
                    // Sideline aprons: FINNBALL wordmark flanked by the arena name.
                    // Both read from the broadcast side (+z); the far band is mirrored
                    // in z so its rows still start 0.25 m outside the sideline.
                    let cell = 0.16;
                    let band_v0 = |c: f32| {
                        let pad = (7.0 * cell - 7.0 * c) * 0.5;
                        if z > 0.0 {
                            COURT_HALF_WID + 0.25 + pad
                        } else {
                            -COURT_HALF_WID - 0.25 - pad - 7.0 * c
                        }
                    };
                    let fb_w = text_len("FINNBALL", cell);
                    let c = text_cover_uv(x, z, "FINNBALL", -fb_w * 0.5, band_v0(cell), cell);
                    col = lerp3(col, pal.line, c * 0.85);
                    let small = 0.11;
                    let name_w = text_len(pal.arena_name, small);
                    for k in [-1.0, 1.0] {
                        let u0 = k * 8.6 - name_w * 0.5;
                        let c = text_cover_uv(x, z, pal.arena_name, u0, band_v0(small), small);
                        col = lerp3(col, pal.accent, c * 0.9);
                    }
                } else if x.abs() > COURT_HALF_LEN && z.abs() < COURT_HALF_WID {
                    // Baseline aprons: team name behind each hoop in team colour.
                    let (name, tint, dir) = if x < 0.0 {
                        (pal.home_name, pal.home_color, 3)
                    } else {
                        (pal.away_name, pal.away_color, 2)
                    };
                    let cell = 0.13;
                    let (u, v) = text_frame(x, z, dir);
                    let w = text_len(name, cell);
                    // Rows advance toward the court (v = -|x|), so the top row sits
                    // farthest out on the apron.
                    let v0 = -(COURT_HALF_LEN + 0.3 + 7.0 * cell);
                    let c = text_cover_uv(u, v, name, -w * 0.5, v0, cell);
                    col = lerp3(col, tint, c * 0.95);
                    // sponsor blocks either side
                    let small = 0.1;
                    let sw = text_len("FINNBALL", small);
                    for k in [-1.0, 1.0] {
                        let c = text_cover_uv(u, v, "FINNBALL", k * 5.0 - sw * 0.5, v0 + 0.2, small);
                        col = lerp3(col, pal.line, c * 0.75);
                    }
                }
            } else {
                // Outside the arc reads slightly darker — like a real broadcast court
                let outside = beyond_arc(x, z, HOOP_X) || beyond_arc(x, z, -HOOP_X);
                if outside {
                    col = scale3(col, 0.82);
                }
                // Key fill with a subtle two-tone lane
                for sign in [-1.0f32, 1.0] {
                    let in_key = z.abs() <= PAINT_HALF_WIDTH
                        && x * sign >= HOOP_X - PAINT_DEPTH
                        && x * sign <= COURT_HALF_LEN;
                    if in_key {
                        col = lerp3(col, pal.paint, 0.9);
                        // faint chevrons in the lane
                        let chev = ((x * sign - z.abs() * 0.5) * 4.0).sin();
                        if chev > 0.85 {
                            col = lerp3(col, scale3(pal.paint, 0.85), 0.5);
                        }
                    }
                }
                // Center logo disc: accent ring, dashed inner ring, FINNBALL wordmark
                let cd = (x * x + z * z).sqrt();
                if cd < CENTER_R - LINE_W {
                    let disc = lerp3(pal.paint, pal.accent, 0.35);
                    col = lerp3(col, disc, 0.85);
                    let ring = cover((cd - 1.6).abs(), 0.045, px_size);
                    col = lerp3(col, pal.accent, ring);
                    let ang = z.atan2(x);
                    if (ang * 12.0).sin() > 0.0 {
                        let dashed = cover((cd - 1.38).abs(), 0.03, px_size);
                        col = lerp3(col, pal.line, dashed * 0.8);
                    }
                    // Wordmark (reads from the broadcast camera on the near sideline)
                    let f = text_cover(x, z, wordmark, -wm_w * 0.5, -0.24, wm_cell, 0);
                    col = lerp3(col, pal.line, f);
                    // Arena name in small caps under the wordmark
                    let small = 0.036;
                    let nw = text_len(pal.arena_name, small);
                    let n = text_cover(x, z, pal.arena_name, -nw * 0.5, 0.36, small, 0);
                    col = lerp3(col, pal.accent, n);
                    // Slash marks above
                    let s = text_cover(x, z, "* * *", -text_len("* * *", 0.05) * 0.5, -0.72, 0.05, 0);
                    col = lerp3(col, pal.accent, s);
                }
                // Small FINNBALL stencils in the corners of the frontcourt
                let cell = 0.07;
                let sw = text_len("FINNBALL", cell);
                for sx in [-1.0f32, 1.0] {
                    for sz in [-1.0f32, 1.0] {
                        let cx = sx * (COURT_HALF_LEN - 2.6);
                        let cz = sz * (COURT_HALF_WID - 0.75);
                        if (x - cx).abs() < sw && (z - cz).abs() < 0.5 {
                            let dir = if sz > 0.0 { 0 } else { 1 };
                            let (u, v) = text_frame(x, z, dir);
                            let (cu, cv) = text_frame(cx, cz, dir);
                            let c = text_cover_uv(u, v, "FINNBALL", cu - sw * 0.5, cv - 3.5 * cell, cell);
                            col = lerp3(col, pal.line, c * 0.55);
                        }
                    }
                }
            }

            // --- lines
            let mut d = f32::MAX;
            // boundary
            if z.abs() <= COURT_HALF_WID + line_half {
                d = d.min((x.abs() - COURT_HALF_LEN).abs());
            }
            if x.abs() <= COURT_HALF_LEN + line_half {
                d = d.min((z.abs() - COURT_HALF_WID).abs());
            }
            if z.abs() <= COURT_HALF_WID {
                d = d.min(x.abs()); // half-court
                d = d.min(ring_dist(x, z, 0.0, 0.0, CENTER_R));
                for sign in [-1.0f32, 1.0] {
                    let hx = sign * HOOP_X;
                    d = d.min(three_dist(x, z, hx));
                    // key rectangle
                    let fx = ft_x(sign);
                    d = d.min(seg_dist(x, z, fx, -PAINT_HALF_WIDTH, fx, PAINT_HALF_WIDTH));
                    for s in [-1.0, 1.0] {
                        d = d.min(seg_dist(
                            x,
                            z,
                            fx,
                            s * PAINT_HALF_WIDTH,
                            sign * COURT_HALF_LEN,
                            s * PAINT_HALF_WIDTH,
                        ));
                        // lane hash marks
                        for k in 0..3 {
                            let mx = fx + sign * (1.0 + k as f32 * 1.2);
                            d = d.min(seg_dist(
                                x,
                                z,
                                mx,
                                s * PAINT_HALF_WIDTH,
                                mx,
                                s * (PAINT_HALF_WIDTH + 0.2),
                            ));
                        }
                    }
                    // free-throw circle (solid half toward court, dashed half inside the key)
                    let ftd = ring_dist(x, z, fx, 0.0, FT_CIRCLE_R);
                    if (x - fx) * sign <= 0.0 {
                        d = d.min(ftd);
                    } else {
                        let ang = (z).atan2((x - fx) * sign);
                        if ((ang * 6.0).sin()) > 0.0 {
                            d = d.min(ftd);
                        }
                    }
                    // restricted area arc
                    if (x - hx) * sign <= 0.0 {
                        d = d.min(ring_dist(x, z, hx, 0.0, RESTRICTED_R));
                    }
                }
            }
            let c = cover(d, line_half, px_size);
            col = lerp3(col, pal.line, c);

            let i = ((py * width + px) * 4) as usize;
            rgba[i] = (col[0].clamp(0.0, 1.0) * 255.0) as u8;
            rgba[i + 1] = (col[1].clamp(0.0, 1.0) * 255.0) as u8;
            rgba[i + 2] = (col[2].clamp(0.0, 1.0) * 255.0) as u8;
            rgba[i + 3] = 255;
        }
    }

    CourtImage {
        width,
        height,
        rgba,
    }
}

// ---------------------------------------------------------------------------------------
// Canvas: a tiny RGBA raster painter for banners, ribbons and the scoreboard.
// ---------------------------------------------------------------------------------------

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

fn to_u8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            rgba: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn fill(&mut self, col: [f32; 3]) {
        self.rect(0, 0, self.width as i32, self.height as i32, col);
    }

    pub fn clear_alpha(&mut self) {
        for px in self.rgba.chunks_mut(4) {
            px[3] = 0;
        }
    }

    pub fn put(&mut self, x: i32, y: i32, col: [f32; 3], alpha: u8) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = ((y as u32 * self.width + x as u32) * 4) as usize;
        self.rgba[i] = to_u8(col[0]);
        self.rgba[i + 1] = to_u8(col[1]);
        self.rgba[i + 2] = to_u8(col[2]);
        self.rgba[i + 3] = alpha;
    }

    pub fn pixel(&self, x: i32, y: i32) -> [u8; 4] {
        let x = x.clamp(0, self.width as i32 - 1) as u32;
        let y = y.clamp(0, self.height as i32 - 1) as u32;
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.rgba[i],
            self.rgba[i + 1],
            self.rgba[i + 2],
            self.rgba[i + 3],
        ]
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, col: [f32; 3]) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.width as i32);
        let y1 = (y + h).min(self.height as i32);
        let r = to_u8(col[0]);
        let g = to_u8(col[1]);
        let b = to_u8(col[2]);
        for yy in y0..y1 {
            for xx in x0..x1 {
                let i = ((yy as u32 * self.width + xx as u32) * 4) as usize;
                self.rgba[i] = r;
                self.rgba[i + 1] = g;
                self.rgba[i + 2] = b;
                self.rgba[i + 3] = 255;
            }
        }
    }

    /// Horizontal gradient band from `a` to `b`.
    pub fn gradient(&mut self, x: i32, y: i32, w: i32, h: i32, a: [f32; 3], b: [f32; 3]) {
        for xx in x.max(0)..(x + w).min(self.width as i32) {
            let t = (xx - x) as f32 / w.max(1) as f32;
            let c = lerp3(a, b, t);
            self.rect(xx, y, 1, h, c);
        }
    }

    /// Pixel width of `text` at glyph `cell` size.
    pub fn text_width(text: &str, cell: i32) -> i32 {
        ((text.chars().count() as i32 * 6 - 1).max(0)) * cell
    }

    /// Draw `text` with its top-left at (x, y); each glyph pixel is `cell` canvas pixels.
    pub fn text(&mut self, x: i32, y: i32, cell: i32, text: &str, col: [f32; 3]) {
        let mut cx = x;
        for c in text.chars() {
            let g = glyph(c);
            for (row, bits) in g.iter().enumerate() {
                for colu in 0..5 {
                    if bits & (1 << (4 - colu)) != 0 {
                        self.rect(cx + colu * cell, y + row as i32 * cell, cell, cell, col);
                    }
                }
            }
            cx += 6 * cell;
        }
    }

    /// Centered text inside a box.
    pub fn text_centered(&mut self, cx: i32, cy: i32, cell: i32, text: &str, col: [f32; 3]) {
        let w = Self::text_width(text, cell);
        self.text(cx - w / 2, cy - 7 * cell / 2, cell, text, col);
    }

    /// Largest glyph cell so `text` fits in `max_w` x `max_h` pixels.
    pub fn fit_cell(text: &str, max_w: i32, max_h: i32) -> i32 {
        let n = text.chars().count().max(1) as i32;
        let by_w = max_w / (n * 6 - 1).max(1);
        let by_h = max_h / 7;
        by_w.min(by_h).max(1)
    }

    pub fn into_image(self) -> CourtImage {
        CourtImage {
            width: self.width,
            height: self.height,
            rgba: self.rgba,
        }
    }
}

// ---------------------------------------------------------------------------------------
// Rafters banners
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct BannerSpec {
    pub bg: [f32; 3],
    pub fg: [f32; 3],
    pub trim: [f32; 3],
    pub top: String,
    pub big: String,
    pub bottom: String,
    /// Pennant cut at the bottom (alpha-masked triangle).
    pub pennant: bool,
}

/// Paints `specs` into a `cols x rows` atlas; each cell is `cell_w x cell_h` pixels.
/// Returns the atlas and the UV rectangle (u0, v0, u1, v1) of every banner.
pub fn paint_banner_atlas(
    specs: &[BannerSpec],
    cols: u32,
    cell_w: u32,
    cell_h: u32,
) -> (CourtImage, Vec<[f32; 4]>) {
    let rows = ((specs.len() as u32).max(1) + cols - 1) / cols;
    let mut canvas = Canvas::new(cols * cell_w, rows * cell_h);
    canvas.clear_alpha();
    let mut uvs = Vec::with_capacity(specs.len());
    for (i, spec) in specs.iter().enumerate() {
        let cx = (i as u32 % cols) as i32 * cell_w as i32;
        let cy = (i as u32 / cols) as i32 * cell_h as i32;
        let w = cell_w as i32;
        let h = cell_h as i32;
        let border = (w / 18).max(2);
        canvas.rect(cx, cy, w, h, spec.trim);
        canvas.rect(cx + border, cy + border, w - 2 * border, h - 2 * border, spec.bg);
        // inner pinstripe
        let p2 = border * 2;
        for yy in [cy + p2, cy + h - p2 - 1] {
            canvas.rect(cx + p2, yy, w - 2 * p2, 1.max(border / 3), spec.trim);
        }
        let inner_w = w - 4 * border;
        let small = Canvas::fit_cell(&spec.top, inner_w, h / 9).max(1);
        canvas.text_centered(cx + w / 2, cy + h * 3 / 16, small, &spec.top, spec.fg);
        let big = Canvas::fit_cell(&spec.big, inner_w, h * 5 / 16);
        canvas.text_centered(cx + w / 2, cy + h * 7 / 16, big, &spec.big, spec.fg);
        let small_b = Canvas::fit_cell(&spec.bottom, inner_w, h / 10).max(1);
        canvas.text_centered(cx + w / 2, cy + h * 21 / 32, small_b, &spec.bottom, spec.fg);
        // divider stripes
        canvas.rect(cx + w / 4, cy + h * 9 / 32, w / 2, border / 2 + 1, spec.trim);
        canvas.rect(cx + w / 4, cy + h * 19 / 32, w / 2, border / 2 + 1, spec.trim);
        if spec.pennant {
            // Cut a V into the bottom quarter by clearing alpha.
            let cut_top = cy + h * 3 / 4;
            for yy in cut_top..(cy + h) {
                let t = (yy - cut_top) as f32 / (h / 4) as f32;
                let half = (w as f32 * 0.5 * t) as i32;
                for xx in 0..half {
                    canvas.put(cx + xx, yy, [0.0; 3], 0);
                    canvas.put(cx + w - 1 - xx, yy, [0.0; 3], 0);
                }
            }
        }
        let u0 = cx as f32 / canvas.width as f32;
        let v0 = cy as f32 / canvas.height as f32;
        uvs.push([
            u0,
            v0,
            u0 + cell_w as f32 / canvas.width as f32,
            v0 + cell_h as f32 / canvas.height as f32,
        ]);
    }
    (canvas.into_image(), uvs)
}

// ---------------------------------------------------------------------------------------
// LED ribbon strip
// ---------------------------------------------------------------------------------------

/// Two-row ribbon texture: the top half cycles `words` separated by diamonds, the bottom
/// half is the "DEFENSE" chant strip. The texture tiles horizontally.
pub fn paint_ribbon(
    width: u32,
    height: u32,
    words: &[&str],
    fg: [f32; 3],
    bg: [f32; 3],
    accent: [f32; 3],
    alt: [f32; 3],
) -> CourtImage {
    let mut canvas = Canvas::new(width, height);
    let half = height as i32 / 2;
    canvas.rect(0, 0, width as i32, half, bg);
    // LED pixel grid feel: darker scanlines
    let dark = scale3(bg, 0.7);
    let mut yy = 0;
    while yy < half {
        canvas.rect(0, yy, width as i32, 1, dark);
        yy += 4;
    }
    let cell = (half * 3 / 5 / 7).max(1);
    let text_y = (half - 7 * cell) / 2;
    let mut x = 12;
    let mut i = 0;
    let n = words.len().max(1);
    while x < width as i32 {
        let word = words.get(i % n).copied().unwrap_or("FINNBALL");
        let col = if i % 2 == 0 { fg } else { accent };
        canvas.text(x, text_y, cell, word, col);
        x += Canvas::text_width(word, cell) + 4 * cell;
        canvas.text(x, text_y, cell, "*", alt);
        x += 5 * cell + 4 * cell;
        i += 1;
    }
    // Bottom row: DEFENSE blocks alternating inverted colours.
    let seg_w = (Canvas::text_width("DEFENSE", cell) + 8 * cell).max(1);
    let mut x = 0;
    let mut k = 0;
    while x < width as i32 {
        let (bgc, fgc) = if k % 2 == 0 { (accent, bg) } else { (bg, fg) };
        canvas.rect(x, half, seg_w, half, bgc);
        canvas.text(x + 4 * cell, half + text_y, cell, "DEFENSE", fgc);
        x += seg_w;
        k += 1;
    }
    canvas.into_image()
}

// ---------------------------------------------------------------------------------------
// Jumbotron scoreboard
// ---------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct ScoreboardData {
    pub home_short: String,
    pub away_short: String,
    pub home: u32,
    pub away: u32,
    pub quarter: u8,
    pub clock: f32,
    pub shot: f32,
    pub home_color: [f32; 3],
    pub away_color: [f32; 3],
    pub accent: [f32; 3],
    /// Big headline instead of the scores (menu / hype prompts). Empty = live board.
    pub headline: String,
    pub subline: String,
    /// 0..1 crowd hype; drives the meter along the bottom.
    pub hype: f32,
    pub fire: bool,
    /// Animation phase in seconds; used for the blinking colon and hype chevrons.
    pub t: f32,
}

pub fn format_clock(secs: f32) -> String {
    let s = secs.max(0.0);
    let m = (s / 60.0).floor() as u32;
    let r = (s - m as f32 * 60.0).floor() as u32;
    format!("{m}:{r:02}")
}

pub fn paint_scoreboard(width: u32, height: u32, d: &ScoreboardData) -> CourtImage {
    let mut c = Canvas::new(width, height);
    let w = width as i32;
    let h = height as i32;
    let ink = [0.01, 0.012, 0.02];
    let white = [0.96, 0.97, 1.0];
    c.fill(ink);
    // LED pixel structure
    let mut yy = 0;
    while yy < h {
        c.rect(0, yy, w, 1, [0.03, 0.03, 0.05]);
        yy += 3;
    }
    // frame
    c.rect(0, 0, w, 3, d.accent);
    c.rect(0, h - 3, w, 3, d.accent);

    if !d.headline.is_empty() {
        let cell = Canvas::fit_cell(&d.headline, w * 9 / 10, h * 2 / 5);
        let pulse = 0.85 + 0.15 * (d.t * 2.0).sin();
        c.text_centered(w / 2, h * 2 / 5, cell, &d.headline, scale3(white, pulse));
        if !d.subline.is_empty() {
            let cell = Canvas::fit_cell(&d.subline, w * 8 / 10, h / 7).max(1);
            c.text_centered(w / 2, h * 3 / 4, cell, &d.subline, d.accent);
        }
        // team colour chevrons racing along the bottom
        let n = 24;
        let sw = w / n;
        for i in 0..n {
            let ph = ((i as f32 / n as f32) - d.t * 0.5).rem_euclid(1.0);
            let col = if ph < 0.5 { d.home_color } else { d.away_color };
            c.rect(i * sw, h - 8, sw - 2, 4, col);
        }
        return c.into_image();
    }

    // Team name blocks in team colours, scores centered under them.
    let col_w = w * 3 / 8;
    let name_cell = Canvas::fit_cell(&d.home_short, col_w - 16, h / 6).max(1);
    c.rect(4, 6, col_w - 8, h / 5, d.home_color);
    c.text_centered(col_w / 2, 6 + h / 10, name_cell, &d.home_short, ink);
    c.rect(w - col_w + 4, 6, col_w - 8, h / 5, d.away_color);
    c.text_centered(w - col_w / 2, 6 + h / 10, name_cell, &d.away_short, ink);

    let score_cell = Canvas::fit_cell("000", col_w - 16, h * 2 / 5);
    let hs = d.home.to_string();
    let as_ = d.away.to_string();
    let sy = h / 5 + 6 + h / 5;
    c.text_centered(col_w / 2, sy, score_cell, &hs, white);
    c.text_centered(w - col_w / 2, sy, score_cell, &as_, white);

    // Middle column: quarter + game clock (blinking colon) + shot clock.
    let mid_x = w / 2;
    let q = format!("Q{}", d.quarter.max(1));
    let q_cell = Canvas::fit_cell(&q, w / 5, h / 8).max(1);
    c.text_centered(mid_x, 6 + h / 10, q_cell, &q, d.accent);
    let mut clock = format_clock(d.clock);
    if d.clock < 10.0 && (d.t * 4.0).fract() < 0.5 {
        clock = clock.replace(':', " ");
    }
    let clk_cell = Canvas::fit_cell("00:00", w / 4, h / 4).max(1);
    let clock_col = if d.clock < 10.0 { [1.0, 0.3, 0.2] } else { white };
    c.text_centered(mid_x, sy, clk_cell, &clock, clock_col);
    let shot = format!("{:02}", d.shot.ceil().max(0.0) as u32);
    let shot_col = if d.shot <= 5.0 { [1.0, 0.25, 0.15] } else { d.accent };
    let shot_cell = Canvas::fit_cell("00", w / 6, h / 6).max(1);
    c.rect(mid_x - w / 10, sy + h / 7, w / 5, h / 5 + 2, [0.06, 0.06, 0.09]);
    c.text_centered(mid_x, sy + h / 7 + h / 10, shot_cell, &shot, shot_col);

    // Bottom strip: hype meter or ON FIRE chant.
    let strip_y = h - h / 6;
    if d.fire {
        let blink = (d.t * 6.0).fract() < 0.6;
        let col = if blink { [1.0, 0.45, 0.1] } else { [1.0, 0.85, 0.3] };
        c.rect(0, strip_y, w, h / 6 - 3, [0.15, 0.03, 0.0]);
        let cell = Canvas::fit_cell("ON FIRE!", w / 2, h / 8).max(1);
        c.text_centered(w / 2, strip_y + h / 12, cell, "ON FIRE!", col);
    } else if d.hype > 0.55 {
        c.rect(0, strip_y, w, h / 6 - 3, scale3(d.accent, 0.35));
        let cell = Canvas::fit_cell("DEFENSE", w / 2, h / 8).max(1);
        let shift = ((d.t * 3.0).fract() * 2.0 - 1.0) * (w / 40) as f32;
        c.text_centered(w / 2 + shift as i32, strip_y + h / 12, cell, "DEFENSE", white);
    } else {
        let n = 20;
        let sw = w / n;
        let lit = (d.hype * n as f32).round() as i32;
        for i in 0..n {
            let col = if i < lit {
                lerp3(d.home_color, d.away_color, i as f32 / n as f32)
            } else {
                [0.08, 0.08, 0.12]
            };
            c.rect(i * sw + 2, strip_y + h / 24, sw - 4, h / 12, col);
        }
    }
    c.into_image()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal() -> CourtPalette {
        CourtPalette {
            wood_a: [0.6, 0.4, 0.2],
            wood_b: [0.5, 0.32, 0.16],
            line: [1.0, 1.0, 1.0],
            paint: [0.1, 0.2, 0.6],
            accent: [0.0, 0.9, 1.0],
            apron: [0.1, 0.1, 0.12],
            arena_name: "TEST DOME",
            home_name: "NEON FOXES",
            away_name: "SHADOW CRANES",
            home_color: [0.0, 0.8, 0.9],
            away_color: [0.6, 0.2, 1.0],
        }
    }

    fn sample(img: &CourtImage, px_per_m: u32, x: f32, z: f32) -> [u8; 3] {
        let px = ((x + PLANE_HALF_LEN) * px_per_m as f32) as u32;
        let py = ((z + PLANE_HALF_WID) * px_per_m as f32) as u32;
        let i = ((py.min(img.height - 1) * img.width + px.min(img.width - 1)) * 4) as usize;
        [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
    }

    #[test]
    fn court_texture_has_expected_size() {
        let img = paint_court(8, &pal());
        assert_eq!(img.width, (PLANE_HALF_LEN * 16.0).round() as u32);
        assert_eq!(img.height, (PLANE_HALF_WID * 16.0).round() as u32);
        assert_eq!(img.rgba.len(), (img.width * img.height * 4) as usize);
    }

    #[test]
    fn lines_are_white_and_wood_is_not() {
        let ppm = 32;
        let img = paint_court(ppm, &pal());
        // half-court line
        let on_line = sample(&img, ppm, 0.0, 3.0);
        assert!(on_line[0] > 230 && on_line[1] > 230 && on_line[2] > 230);
        // plain hardwood between the arc and the key
        let wood = sample(&img, ppm, 4.0, 4.0);
        assert!(wood[0] > wood[2], "wood should be warm");
        // three-point arc at midcourt-facing point
        let arc = sample(&img, ppm, HOOP_X - THREE_RADIUS, 0.0);
        assert!(arc[0] > 230 && arc[1] > 230);
    }

    #[test]
    fn key_is_filled_with_paint() {
        let ppm = 32;
        let img = paint_court(ppm, &pal());
        let p = sample(&img, ppm, HOOP_X - 2.0, 0.8);
        assert!(p[2] > p[0], "key should be painted team blue");
    }

    #[test]
    fn three_point_geometry_matches_sim() {
        assert!(beyond_arc(0.0, 0.0, HOOP_X));
        assert!(!beyond_arc(HOOP_X - 3.0, 0.0, HOOP_X));
        assert!(beyond_arc(HOOP_X - 1.0, COURT_HALF_WID - 0.3, HOOP_X));
    }

    #[test]
    fn center_wordmark_is_painted() {
        let ppm = 64;
        let img = paint_court(ppm, &pal());
        // The first 'F' of FINNBALL: its left stem is a solid column.
        let wm_w = text_len("FINNBALL", 0.062);
        let stem_x = -wm_w * 0.5 + 0.031;
        let on = sample(&img, ppm, stem_x, -0.24 + 0.2);
        assert!(on[0] > 230 && on[1] > 230 && on[2] > 230, "stem {on:?}");
        // The accent logo ring at r = 1.6 is painted with the accent colour (cyan here).
        let ring = sample(&img, ppm, 1.6, 0.0);
        assert!(ring[2] > 200 && ring[0] < 60, "ring {ring:?}");
    }

    #[test]
    fn baseline_aprons_carry_team_names() {
        let ppm = 32;
        let img = paint_court(ppm, &pal());
        // Somewhere in the away-name band there must be an away-colour pixel.
        let mut hit = false;
        let mut z = -3.0;
        while z < 3.0 {
            let p = sample(&img, ppm, COURT_HALF_LEN + 0.3 + 0.13 * 3.5, z);
            if p[2] > 200 && p[0] > 120 && p[1] < 90 {
                hit = true;
                break;
            }
            z += 0.02;
        }
        assert!(hit, "away team name should be stencilled on the +x apron");
    }

    #[test]
    fn glyphs_cover_alphabet_and_digits() {
        for c in "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
            assert!(glyph(c).iter().any(|r| *r != 0), "glyph {c} is blank");
        }
        assert_eq!(glyph(' '), [0; 7]);
        assert_eq!(glyph('a'), glyph('A'));
    }

    #[test]
    fn canvas_text_and_fit() {
        let mut c = Canvas::new(64, 16);
        c.fill([0.0, 0.0, 0.0]);
        c.text(1, 1, 2, "I", [1.0, 1.0, 1.0]);
        // top bar of the I spans all five columns
        assert_eq!(c.pixel(1, 1)[0], 255);
        assert_eq!(c.pixel(9, 1)[0], 255);
        // centre column below the bar
        assert_eq!(c.pixel(5, 5)[0], 255);
        assert_eq!(c.pixel(1, 5)[0], 0);
        assert_eq!(Canvas::text_width("AB", 3), 33);
        assert_eq!(Canvas::fit_cell("AB", 33, 100), 3);
        assert_eq!(Canvas::fit_cell("AB", 1000, 14), 2);
    }

    #[test]
    fn banner_atlas_layout_and_pennant() {
        let spec = BannerSpec {
            bg: [0.1, 0.2, 0.8],
            fg: [1.0, 1.0, 1.0],
            trim: [0.9, 0.8, 0.5],
            top: "CHAMPIONS".into(),
            big: "2024".into(),
            bottom: "FINNBALL".into(),
            pennant: true,
        };
        let specs = vec![spec.clone(), spec.clone(), spec];
        let (img, uvs) = paint_banner_atlas(&specs, 2, 64, 128);
        assert_eq!(img.width, 128);
        assert_eq!(img.height, 256);
        assert_eq!(uvs.len(), 3);
        assert_eq!(uvs[1][0], 0.5);
        assert_eq!(uvs[2][1], 0.5);
        // pennant corner is transparent, centre column near the bottom is not
        let px = |x: u32, y: u32| {
            let i = ((y * img.width + x) * 4) as usize;
            img.rgba[i + 3]
        };
        assert_eq!(px(0, 127), 0);
        assert_eq!(px(32, 100), 255);
        // unused atlas cell stays transparent
        assert_eq!(px(100, 200), 0);
    }

    #[test]
    fn ribbon_tiles_and_has_defense_row() {
        let img = paint_ribbon(
            512,
            64,
            &["FINNBALL", "NEON FOXES"],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.1],
            [0.0, 0.9, 1.0],
            [1.0, 0.3, 0.6],
        );
        assert_eq!(img.rgba.len(), 512 * 64 * 4);
        // some accent pixels exist in the bottom row (DEFENSE blocks)
        let mut accent = 0;
        for x in 0..512u32 {
            let i = ((40 * 512 + x) * 4) as usize;
            if img.rgba[i + 2] > 200 && img.rgba[i] < 30 {
                accent += 1;
            }
        }
        assert!(accent > 50);
    }

    #[test]
    fn scoreboard_paints_live_and_headline() {
        let mut d = ScoreboardData {
            home_short: "FOX".into(),
            away_short: "CRN".into(),
            home: 12,
            away: 9,
            quarter: 2,
            clock: 43.2,
            shot: 12.0,
            home_color: [0.0, 0.8, 0.9],
            away_color: [0.6, 0.2, 1.0],
            accent: [0.0, 0.9, 1.0],
            headline: String::new(),
            subline: String::new(),
            hype: 0.3,
            fire: false,
            t: 1.0,
        };
        let live = paint_scoreboard(256, 128, &d);
        assert_eq!(live.rgba.len(), 256 * 128 * 4);
        // home colour block at the top-left
        let i = ((10 * 256 + 20) * 4) as usize;
        assert!(live.rgba[i + 1] > 180 && live.rgba[i] < 30);
        d.headline = "FINNBALL".into();
        d.subline = "PRESS PLAY".into();
        let menu = paint_scoreboard(256, 128, &d);
        assert_ne!(menu.rgba, live.rgba);
        assert_eq!(format_clock(65.9), "1:05");
        assert_eq!(format_clock(0.0), "0:00");
        assert_eq!(format_clock(-3.0), "0:00");
    }
}
