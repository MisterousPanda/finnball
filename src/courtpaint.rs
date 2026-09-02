//! Procedural hardwood court texture. Pure pixel math — no ECS — so it is unit-testable
//! and runs identically on native and WASM.

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

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
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

/// 5x7 glyphs for the letters we stencil on the apron and logo.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        _ => [0; 7],
    }
}

/// Coverage of `text` rendered with cell size `cell` (meters), top-left at (x0, z0),
/// reading left-to-right in +x with rows advancing in +z. `flip` mirrors rows so the text
/// reads correctly from the opposite sideline.
fn text_cover(px: f32, pz: f32, text: &str, x0: f32, z0: f32, cell: f32, flip: bool) -> f32 {
    let mut cx = x0;
    for c in text.chars() {
        let g = glyph(c);
        let w = 5.0 * cell;
        if px >= cx && px < cx + w && pz >= z0 && pz < z0 + 7.0 * cell {
            let col = ((px - cx) / cell) as usize;
            let mut row = ((pz - z0) / cell) as usize;
            if flip {
                row = 6 - row.min(6);
            }
            let col = if flip { 4 - col.min(4) } else { col.min(4) };
            let bits = g[row.min(6)];
            if bits & (1 << (4 - col)) != 0 {
                return 1.0;
            }
            return 0.0;
        }
        cx += 6.0 * cell;
    }
    0.0
}

pub fn paint_court(px_per_m: u32, pal: &CourtPalette) -> CourtImage {
    let width = (PLANE_HALF_LEN * 2.0 * px_per_m as f32).round() as u32;
    let height = (PLANE_HALF_WID * 2.0 * px_per_m as f32).round() as u32;
    let px_size = 1.0 / px_per_m as f32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let line_half = LINE_W * 0.5;
    let ft_x = |sign: f32| sign * (HOOP_X - PAINT_DEPTH);

    for py in 0..height {
        let z = -PLANE_HALF_WID + (py as f32 + 0.5) * px_size;
        for px in 0..width {
            let x = -PLANE_HALF_LEN + (px as f32 + 0.5) * px_size;
            let in_court = x.abs() <= COURT_HALF_LEN && z.abs() <= COURT_HALF_WID;

            // --- base: hardwood planks running along x
            let row = ((z + PLANE_HALF_WID) / PLANK_W).floor() as i32;
            let stagger = hash2(row, 7) * 1.4;
            let seg = ((x + PLANE_HALF_LEN + stagger) / 1.75).floor() as i32;
            let tone = hash2(row, seg);
            let grain = ((x * 37.0 + row as f32 * 3.1).sin() * 0.5 + 0.5) * 0.12
                + ((x * 91.0 + z * 13.0).sin() * 0.5 + 0.5) * 0.05;
            let mut col = lerp3(
                pal.wood_a,
                pal.wood_b,
                (tone * 0.75 + grain).clamp(0.0, 1.0),
            );
            // seam darkening
            let seam_z =
                ((z + PLANE_HALF_WID) % PLANK_W).min(PLANK_W - ((z + PLANE_HALF_WID) % PLANK_W));
            let seam_x = ((x + PLANE_HALF_LEN + stagger) % 1.75)
                .min(1.75 - ((x + PLANE_HALF_LEN + stagger) % 1.75));
            let seam = cover(seam_z.min(seam_x), PLANK_SEAM, px_size);
            col = lerp3(col, scale3(col, 0.62), seam);

            if !in_court {
                // Apron: darker stain with a soft inner glow band next to the lines
                let edge = (x.abs() - COURT_HALF_LEN)
                    .max(z.abs() - COURT_HALF_WID)
                    .max(0.0);
                let band = (1.0 - edge / 0.35).clamp(0.0, 1.0);
                col = lerp3(pal.apron, lerp3(pal.apron, pal.accent, 0.55), band * 0.6);
                // Sponsor stencil along both sidelines
                let cell = 0.16;
                let text_w = 8.0 * 6.0 * cell;
                for s in [-1.0, 1.0] {
                    let z0 = if s > 0.0 {
                        COURT_HALF_WID + 0.25
                    } else {
                        -COURT_HALF_WID - 0.25 - 7.0 * cell
                    };
                    for k in -1..=1 {
                        let x0 = k as f32 * 9.0 - text_w * 0.5;
                        let c = text_cover(x, z, "FINNBALL", x0, z0, cell, false);
                        col = lerp3(col, pal.line, c * 0.85);
                    }
                }
            } else {
                // Outside the arc reads slightly darker — like a real broadcast court
                let outside = beyond_arc(x, z, HOOP_X) || beyond_arc(x, z, -HOOP_X);
                if outside {
                    col = scale3(col, 0.82);
                }
                // Key fill
                for sign in [-1.0f32, 1.0] {
                    let hx = sign * HOOP_X;
                    let _ = hx;
                    let in_key = z.abs() <= PAINT_HALF_WIDTH
                        && x * sign >= HOOP_X - PAINT_DEPTH
                        && x * sign <= COURT_HALF_LEN;
                    if in_key {
                        col = lerp3(col, pal.paint, 0.9);
                    }
                }
                // Center logo disc
                let cd = (x * x + z * z).sqrt();
                if cd < CENTER_R - LINE_W {
                    let disc = lerp3(pal.paint, pal.accent, 0.35);
                    col = lerp3(col, disc, 0.85);
                    let inner = cover((cd - 1.15).abs(), 0.06, px_size);
                    col = lerp3(col, pal.accent, inner);
                    // Bold F lettermark (up = -z as seen from the broadcast camera)
                    let f = text_cover(x, z, "F", -0.4, -0.62, 0.16, false);
                    col = lerp3(col, pal.line, f);
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
}
