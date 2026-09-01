//! Pure basketball math — no Bevy ECS. Used by gameplay and unit tests.

pub const COURT_HALF_LEN: f32 = 14.0;
pub const COURT_HALF_WID: f32 = 7.5;
pub const RIM_HEIGHT: f32 = 3.05;
pub const RIM_RADIUS: f32 = 0.225;
pub const BACKBOARD_OFFSET: f32 = 0.42;
pub const HOOP_X: f32 = 12.55;
pub const BALL_RADIUS: f32 = 0.121;
pub const THREE_RADIUS: f32 = 6.75;
pub const PAINT_DEPTH: f32 = 5.8;
pub const PAINT_HALF_WIDTH: f32 = 2.45;
pub const GRAVITY: f32 = 16.2;
pub const PLAYER_RADIUS: f32 = 0.38;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotKind {
    Two,
    Three,
}

#[inline]
pub fn hoop_position(home_hoop: bool) -> [f32; 3] {
    let x = if home_hoop { -HOOP_X } else { HOOP_X };
    [x, RIM_HEIGHT, 0.0]
}

#[inline]
pub fn in_bounds(x: f32, z: f32, pad: f32) -> bool {
    x.abs() <= COURT_HALF_LEN - pad && z.abs() <= COURT_HALF_WID - pad
}

#[inline]
pub fn clamp_to_court(x: f32, z: f32, pad: f32) -> (f32, f32) {
    (
        x.clamp(-(COURT_HALF_LEN - pad), COURT_HALF_LEN - pad),
        z.clamp(-(COURT_HALF_WID - pad), COURT_HALF_WID - pad),
    )
}

/// Home attacks +X hoop in even-numbered quarters (0-based). Arcade default: home always
/// attacks +X so camera language stays consistent.
pub fn offensive_hoop_x(home_team: bool) -> f32 {
    if home_team {
        HOOP_X
    } else {
        -HOOP_X
    }
}

pub fn in_paint(x: f32, z: f32, hoop_x: f32) -> bool {
    let along = (x - hoop_x).abs();
    along <= PAINT_DEPTH && z.abs() <= PAINT_HALF_WIDTH
}

pub fn shot_kind(x: f32, z: f32, hoop_x: f32) -> ShotKind {
    let dx = x - hoop_x;
    let dist = (dx * dx + z * z).sqrt();
    if dist >= THREE_RADIUS {
        ShotKind::Three
    } else {
        ShotKind::Two
    }
}

pub fn points_for(kind: ShotKind) -> u32 {
    match kind {
        ShotKind::Two => 2,
        ShotKind::Three => 3,
    }
}

/// Ballistic launch velocity to reach a target in `flight` seconds under gravity.
pub fn ballistic_velocity(
    from: [f32; 3],
    to: [f32; 3],
    flight: f32,
    gravity: f32,
) -> [f32; 3] {
    let t = flight.max(0.18);
    [
        (to[0] - from[0]) / t,
        (to[1] - from[1] + 0.5 * gravity * t * t) / t,
        (to[2] - from[2]) / t,
    ]
}

/// 0..1 make probability after contest, fatigue, meter, and ratings.
pub fn shot_make_chance(
    rating: f32,
    distance: f32,
    contest: f32,
    meter_error: f32,
    stamina: f32,
    is_three: bool,
) -> f32 {
    let range_term = if is_three {
        (1.15 - (distance - THREE_RADIUS).max(0.0) * 0.09).clamp(0.25, 1.15)
    } else {
        (1.2 - distance * 0.055).clamp(0.35, 1.2)
    };
    let skill = (rating / 100.0).clamp(0.2, 1.15);
    let open = (1.0 - contest * 0.62).clamp(0.28, 1.0);
    let meter = (1.0 - meter_error * 1.35).clamp(0.15, 1.08);
    let gas = (0.55 + stamina * 0.45).clamp(0.45, 1.0);
    (0.18 + 0.74 * skill * range_term * open * meter * gas).clamp(0.04, 0.92)
}

pub fn steal_chance(steal_rating: f32, handle_rating: f32, distance: f32) -> f32 {
    let reach = (1.15 - distance * 1.8).clamp(0.0, 1.0);
    let mismatch = ((steal_rating - handle_rating) / 100.0 + 0.5).clamp(0.15, 0.9);
    (0.12 + 0.45 * mismatch) * reach
}

pub fn contest_factor(def_dist: f32, block_rating: f32) -> f32 {
    if def_dist > 2.4 {
        return 0.0;
    }
    let close = (1.0 - def_dist / 2.4).clamp(0.0, 1.0);
    close * (0.45 + block_rating / 180.0)
}

pub fn flight_time_for_distance(distance: f32) -> f32 {
    (0.72 + distance * 0.055).clamp(0.55, 1.55)
}

/// Rim catch: ball must be coming down, near rim center, and inside the cylinder.
pub fn rim_score_window(
    ball: [f32; 3],
    vel_y: f32,
    hoop: [f32; 3],
) -> bool {
    if vel_y > 0.4 {
        return false;
    }
    let dx = ball[0] - hoop[0];
    let dz = ball[2] - hoop[2];
    let dy = ball[1] - hoop[1];
    dx * dx + dz * dz <= (RIM_RADIUS * 0.78) * (RIM_RADIUS * 0.78) && dy.abs() < 0.16
}

pub fn speed_from_rating(rating: f32, sprint: bool) -> f32 {
    let base = 4.2 + rating / 100.0 * 4.4;
    if sprint {
        base * 1.38
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_from_the_logo() {
        assert_eq!(shot_kind(0.0, 0.0, HOOP_X), ShotKind::Three);
        assert_eq!(points_for(ShotKind::Three), 3);
    }

    #[test]
    fn layup_in_paint_is_two() {
        assert_eq!(shot_kind(HOOP_X - 1.2, 0.2, HOOP_X), ShotKind::Two);
        assert!(in_paint(HOOP_X - 2.0, 0.5, HOOP_X));
    }

    #[test]
    fn contest_falls_off_with_distance() {
        assert!(contest_factor(0.4, 90.0) > contest_factor(2.0, 90.0));
        assert_eq!(contest_factor(4.0, 99.0), 0.0);
    }

    #[test]
    fn green_meter_helps_threes() {
        let green = shot_make_chance(92.0, 7.2, 0.05, 0.02, 0.9, true);
        let brick = shot_make_chance(92.0, 7.2, 0.05, 0.8, 0.9, true);
        assert!(green > brick);
        assert!(green > 0.4);
    }

    #[test]
    fn ballistic_reaches_target_on_x() {
        let v = ballistic_velocity([0.0, 2.0, 0.0], [4.0, 3.05, 1.0], 1.0, GRAVITY);
        assert!((v[0] - 4.0).abs() < 0.001);
    }

    #[test]
    fn court_clamp_keeps_players_inside() {
        let (x, z) = clamp_to_court(40.0, -90.0, 0.5);
        assert!(in_bounds(x, z, 0.5));
    }
}
