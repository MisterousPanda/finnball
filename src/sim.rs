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

/// Arcade shot taxonomy used for release params, ticker copy, and camera language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotType {
    JumpShot,
    Fadeaway,
    Layup,
    ReverseLayup,
    FingerRoll,
    Dunk,
    Hook,
    Underhand,
    Floater,
    Runner,
    ThreePointer,
    LogoHeave,
}

impl ShotType {
    pub fn label(self) -> &'static str {
        match self {
            Self::JumpShot => "PULL-UP J",
            Self::Fadeaway => "FADEAWAY",
            Self::Layup => "LAYUP",
            Self::ReverseLayup => "REVERSE LAYUP",
            Self::FingerRoll => "FINGER ROLL",
            Self::Dunk => "POSTERIZE",
            Self::Hook => "HOOK SHOT",
            Self::Underhand => "UNDERHAND SCOOP",
            Self::Floater => "FLOATER",
            Self::Runner => "RUNNER",
            Self::ThreePointer => "FOR THREE",
            Self::LogoHeave => "FROM THE LOGO",
        }
    }

    pub fn is_dunk(self) -> bool {
        matches!(self, Self::Dunk)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PassKind {
    #[default]
    Chest,
    Bounce,
    Lob,
    Skip,
}

/// Classify a release from court geometry + motion. Physics still decides the make.
pub fn classify_shot(
    dist: f32,
    in_the_paint: bool,
    driving: bool,
    moving_away: bool,
    lateral: bool,
    dunk_rating: f32,
    mid_rating: f32,
    special: bool,
    speed: f32,
) -> ShotType {
    if (special || (in_the_paint && driving && dunk_rating > 58.0)) && dist < PAINT_DEPTH + 0.8 {
        return ShotType::Dunk;
    }
    if dist > 10.0 {
        return ShotType::LogoHeave;
    }
    if dist >= THREE_RADIUS {
        return ShotType::ThreePointer;
    }
    if in_the_paint && dist < 1.5 && speed < 2.2 {
        return ShotType::Underhand;
    }
    if in_the_paint && driving && dist < 2.2 && mid_rating > 75.0 {
        return ShotType::FingerRoll;
    }
    if in_the_paint && driving && dist < 2.8 && lateral {
        return ShotType::ReverseLayup;
    }
    if in_the_paint && driving && dist < 3.5 {
        return ShotType::Layup;
    }
    if in_the_paint && lateral && dist < 5.0 {
        return ShotType::Hook;
    }
    if !in_the_paint && driving && dist < 6.0 {
        return ShotType::Floater;
    }
    if moving_away && dist < 8.0 {
        return ShotType::Fadeaway;
    }
    if speed > 5.5 && dist < 8.0 {
        return ShotType::Runner;
    }
    ShotType::JumpShot
}

pub fn release_height(shot: ShotType) -> f32 {
    match shot {
        ShotType::Dunk => 2.4,
        ShotType::Hook => 2.35,
        ShotType::Underhand => 1.05,
        ShotType::Layup | ShotType::ReverseLayup => 1.45,
        ShotType::FingerRoll => 1.55,
        ShotType::Floater => 1.75,
        ShotType::Runner => 1.55,
        ShotType::Fadeaway => 2.15,
        _ => 1.95,
    }
}

/// Backspin axis is `shot_dir × up` so a +X attack spins around −Z (and vice versa).
pub fn release_spin(shot: ShotType, quality: f32, dir_x: f32, dir_z: f32) -> [f32; 3] {
    let q = quality.clamp(0.5, 1.1);
    let mag = match shot {
        ShotType::Dunk => 4.0,
        ShotType::Underhand => 8.0 * q,
        ShotType::Layup | ShotType::ReverseLayup | ShotType::FingerRoll => 9.0 * q,
        ShotType::Hook => 10.0 * q,
        ShotType::Floater | ShotType::Runner => 12.0 * q,
        ShotType::ThreePointer | ShotType::LogoHeave | ShotType::JumpShot | ShotType::Fadeaway => {
            18.0 * q
        }
    };
    let horiz = (dir_x * dir_x + dir_z * dir_z).sqrt().max(0.001);
    // ω = dir × Y, then flip so it reads as backspin (fingers under the ball)
    let ax = dir_z / horiz;
    let az = -dir_x / horiz;
    [ax * mag, 3.0, az * mag]
}

pub fn meter_accuracy(value: f32) -> f32 {
    (value - 0.72).abs()
}

/// Dribble bounces per second — faster on the run so the ball stays glued.
pub fn dribble_cadence(speed_mps: f32) -> f32 {
    2.8 + speed_mps * 0.35
}

/// Quadratic drag + Magnus (backspin lifts / sidespin curves). Pure step for tests.
pub fn apply_aero(vel: [f32; 3], spin: [f32; 3], dt: f32) -> [f32; 3] {
    const AIR_DENSITY: f32 = 1.2;
    const CD: f32 = 0.47;
    const MASS: f32 = 0.62;
    const MAGNUS: f32 = 0.012;
    let area = std::f32::consts::PI * BALL_RADIUS * BALL_RADIUS;
    let speed = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
    let mut out = vel;
    if speed > 0.01 {
        let drag = 0.5 * AIR_DENSITY * CD * area * speed;
        let inv = 1.0 / speed;
        out[0] -= vel[0] * inv * (drag / MASS) * dt;
        out[1] -= vel[1] * inv * (drag / MASS) * dt;
        out[2] -= vel[2] * inv * (drag / MASS) * dt;
    }
    // Magnus: C * ω × v
    let mx = MAGNUS * (spin[1] * vel[2] - spin[2] * vel[1]) / MASS * dt;
    let my = MAGNUS * (spin[2] * vel[0] - spin[0] * vel[2]) / MASS * dt;
    let mz = MAGNUS * (spin[0] * vel[1] - spin[1] * vel[0]) / MASS * dt;
    out[0] += mx;
    out[1] += my;
    out[2] += mz;
    out
}

/// True if the ball crossed down through the rim cylinder this step.
pub fn cylinder_score(prev: [f32; 3], curr: [f32; 3], vel_y: f32, hoop: [f32; 3]) -> bool {
    if vel_y > 0.15 {
        return false;
    }
    let crossed = prev[1] >= hoop[1] && curr[1] <= hoop[1] + 0.08;
    if !crossed && !rim_score_window(curr, vel_y, hoop) {
        return false;
    }
    let dx = curr[0] - hoop[0];
    let dz = curr[2] - hoop[2];
    dx * dx + dz * dz <= (RIM_RADIUS * 0.78) * (RIM_RADIUS * 0.78)
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

    #[test]
    fn classify_logo_and_layup() {
        assert_eq!(
            classify_shot(12.5, false, false, false, false, 50.0, 80.0, false, 1.0),
            ShotType::LogoHeave
        );
        assert_eq!(
            classify_shot(2.4, true, true, false, false, 50.0, 70.0, false, 4.0),
            ShotType::Layup
        );
        assert_eq!(
            classify_shot(1.8, true, true, false, false, 90.0, 70.0, true, 5.0),
            ShotType::Dunk
        );
        assert_eq!(
            classify_shot(7.1, false, false, false, false, 40.0, 80.0, false, 1.0),
            ShotType::ThreePointer
        );
    }

    #[test]
    fn dribble_speeds_up_on_the_run() {
        assert!(dribble_cadence(8.0) > dribble_cadence(1.0));
    }

    #[test]
    fn aero_step_stays_finite() {
        let spin = release_spin(ShotType::JumpShot, 1.0, 1.0, 0.0);
        let v = apply_aero([8.0, 2.0, 0.0], spin, 0.016);
        assert!(v[0].is_finite() && v[1].is_finite() && v[2].is_finite());
    }

    #[test]
    fn green_home_jumper_still_threads_the_cylinder() {
        let from = [0.0, 1.95, 0.0];
        let hoop = hoop_position(false);
        let t = flight_time_for_distance((hoop[0] - from[0]).abs());
        let mut vel = ballistic_velocity(from, hoop, t, GRAVITY);
        let dt = 1.0 / 64.0;
        let mut pos = from;
        let mut prev = from;
        let mut scored = false;
        for _ in 0..200 {
            vel[1] -= GRAVITY * dt;
            prev = pos;
            pos = [pos[0] + vel[0] * dt, pos[1] + vel[1] * dt, pos[2] + vel[2] * dt];
            if cylinder_score(prev, pos, vel[1], hoop) {
                scored = true;
                break;
            }
            if pos[1] < BALL_RADIUS && vel[1] < 0.0 {
                break;
            }
        }
        assert!(scored, "green make on the solved ballistic must score");
    }

    #[test]
    fn cylinder_needs_downward_cross() {
        let hoop = hoop_position(false);
        assert!(cylinder_score(
            [hoop[0], hoop[1] + 0.1, hoop[2]],
            [hoop[0], hoop[1] - 0.05, hoop[2]],
            -2.0,
            hoop
        ));
        assert!(!cylinder_score(
            [hoop[0], hoop[1] - 0.2, hoop[2]],
            [hoop[0], hoop[1] + 0.1, hoop[2]],
            2.0,
            hoop
        ));
    }
}
