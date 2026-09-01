use bevy::prelude::*;

use crate::ball::{Ball, BallState, Hold};
use crate::gameplay::{
    AiBrain, GameRng, LastPass, LiveControl, MatchClock, PlayerIntent, ShotMeter,
};
use crate::roster::Side;
use crate::sim::{ai_wants_shot, clamp_to_court, in_paint, shot_kind, HOOP_X};
use crate::states::{AppState, GameMode, MatchConfig, Paused};
use crate::units::{Heat, MoveVel, Player, Pose, PoseClock, Ratings, Stamina};

pub struct AiPlugin;

impl Plugin for AiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (ai_move, ai_decisions)
                .chain()
                .run_if(in_state(AppState::Playing)),
        );
    }
}

fn ai_move(
    time: Res<Time<Fixed>>,
    paused: Res<Paused>,
    control: Res<LiveControl>,
    ball: Query<(&Transform, &BallState), (With<Ball>, Without<Player>)>,
    mut players: Query<
        (
            Entity,
            &Player,
            &Ratings,
            &mut Transform,
            &mut MoveVel,
            &Pose,
            &Stamina,
            &mut AiBrain,
        ),
        Without<Ball>,
    >,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let Ok((btf, bstate)) = ball.single() else {
        return;
    };
    let holder_side = bstate.holder.and_then(|h| {
        players
            .iter()
            .find(|(e, ..)| *e == h)
            .map(|(_, p, ..)| p.side)
    });

    let snapshot: Vec<(Entity, Side, Vec3, bool)> = players
        .iter()
        .map(|(e, p, _, t, ..)| {
            (
                e,
                p.side,
                t.translation,
                p.human && control.entity == Some(e),
            )
        })
        .collect();

    for (e, p, ratings, mut tf, mut vel, pose, stam, mut brain) in &mut players {
        if control.entity == Some(e) {
            continue;
        }
        if matches!(
            *pose,
            Pose::Shoot | Pose::Dunk | Pose::Pass | Pose::Stumble | Pose::Celebrate | Pose::Block
        ) {
            continue;
        }
        brain.think += dt;
        let hoop_x = if p.side == Side::Home {
            HOOP_X
        } else {
            -HOOP_X
        };
        let on_offense = match holder_side {
            Some(s) => s == p.side,
            None => true,
        };
        let has_ball = bstate.holder == Some(e);

        let target = if has_ball {
            // Attack: drive toward elbow then hoop
            let lane_z = if tf.translation.z.abs() < 0.4 {
                if p.slot % 2 == 0 {
                    2.4
                } else {
                    -2.4
                }
            } else {
                tf.translation.z * 0.4
            };
            Vec3::new(hoop_x * 0.72, 0.0, lane_z)
        } else if on_offense {
            // Space the floor
            let slot_z = (p.slot as f32 - 1.0) * 3.4;
            Vec3::new(hoop_x * 0.45, 0.0, slot_z)
        } else {
            // Defense: between ball and hoop
            let def_hoop = -hoop_x;
            let ball_pos = btf.translation.with_y(0.0);
            let hoop = Vec3::new(def_hoop, 0.0, 0.0);
            ball_pos.lerp(hoop, 0.35)
        };

        // Loose ball hunt
        let dest = if bstate.hold == Hold::Loose {
            btf.translation.with_y(0.0)
        } else {
            target
        };

        let to = dest - tf.translation.with_y(0.0);
        let dist = to.length();
        if dist > 0.35 {
            let n = to.normalize();
            let spd = crate::units::move_speed(ratings, dist > 6.0, stam.0) * 0.92;
            vel.0 = n * spd;
            tf.translation += vel.0 * dt;
        } else {
            vel.0 *= 0.8;
        }
        let (x, z) = clamp_to_court(tf.translation.x, tf.translation.z, 0.55);
        tf.translation.x = x;
        tf.translation.z = z;
        tf.translation.y = 0.0;
        let _ = snapshot;
    }
}

fn ai_decisions(
    paused: Res<Paused>,
    config: Res<MatchConfig>,
    mut rng: ResMut<GameRng>,
    meter: ResMut<ShotMeter>,
    intent: ResMut<PlayerIntent>,
    control: Res<LiveControl>,
    clock: Res<MatchClock>,
    mut last_pass: ResMut<LastPass>,
    mut ball_q: Query<
        (&Transform, &mut crate::ball::BallVel, &mut BallState),
        (With<Ball>, Without<Player>),
    >,
    mut players: Query<
        (
            Entity,
            &Player,
            &Ratings,
            &Transform,
            &MoveVel,
            &mut Pose,
            &mut PoseClock,
            &Stamina,
            &mut AiBrain,
            &Heat,
        ),
        Without<Ball>,
    >,
) {
    if paused.0 || config.mode == GameMode::Practice {
        return;
    }
    let Ok((btf, mut bvel, mut st)) = ball_q.single_mut() else {
        return;
    };

    // AI that currently holds the ball may shoot/pass without using PlayerIntent (that's for human)
    let Some(holder) = st.holder else {
        return;
    };
    if control.entity == Some(holder) {
        return;
    }

    let mut me = None;
    for (e, p, r, t, v, _, _, s, brain, heat) in &players {
        if e == holder {
            me = Some((
                e,
                p.side,
                t.translation,
                v.0,
                r.clone(),
                s.0,
                brain.think,
                r.pass,
                r.three,
                r.mid,
                r.dunk,
                heat.streak,
            ));
        }
    }
    let Some((e, side, pos, vel, ratings, stam, think, pass, three, mid, dunk, streak)) = me else {
        return;
    };
    if think < 0.45 {
        return;
    }

    let hoop_x = if side == Side::Home { HOOP_X } else { -HOOP_X };
    let dist = (pos.x - hoop_x).hypot(pos.z);
    let open = players
        .iter()
        .filter(|(_, p, _, t, ..)| p.side != side && t.translation.distance(pos) < 1.8)
        .count()
        == 0;
    let rating = if matches!(shot_kind(pos.x, pos.z, hoop_x), crate::sim::ShotKind::Three) {
        three
    } else {
        mid
    };
    let should_dunk = in_paint(pos.x, pos.z, hoop_x) && dunk > 72.0 && vel.length() > 2.0;
    let should_shoot = ai_wants_shot(dist, open, rating, clock.shot, dist < 3.2);

    // Reset brain
    if let Ok((_, _, _, _, _, _, _, _, mut brain, _)) = players.get_mut(e) {
        brain.think = 0.0;
    }

    if should_dunk || should_shoot {
        // Reuse human shoot path by temporarily stealing intent if no human holding
        // Directly launch like gameplay shoot
        let hoop = Vec3::new(hoop_x, crate::sim::RIM_HEIGHT, 0.0);
        let mut target = hoop;
        let make = (0.55 + ratings.three / 400.0) * crate::sim::heat_make_mult(streak);
        if rng.f32() > make {
            target += Vec3::new(
                rng.range(-0.5, 0.5),
                rng.range(0.0, 0.4),
                rng.range(-0.5, 0.5),
            );
        }
        let flight = crate::sim::flight_time_for_distance(dist);
        let from = [pos.x, 1.85, pos.z];
        let v = crate::sim::ballistic_velocity(
            from,
            [target.x, target.y, target.z],
            flight,
            crate::sim::GRAVITY,
        );
        bvel.0 = Vec3::new(v[0], v[1], v[2]);
        st.hold = Hold::Shot;
        st.holder = None;
        st.shooter = Some(e);
        st.rim_hits = 0;
        st.release_was_three = matches!(
            crate::sim::shot_kind(pos.x, pos.z, hoop_x),
            crate::sim::ShotKind::Three
        );
        if let Ok((_, _, _, _, _, mut pose, mut clock, _, _, _)) = players.get_mut(e) {
            *pose = if should_dunk { Pose::Dunk } else { Pose::Shoot };
            clock.0 = 0.0;
        }
        let _ = (meter, intent, btf, pass, stam);
        return;
    }

    // Else pass to a teammate
    let mate = players
        .iter()
        .filter(|(oe, p, _, _t, ..)| *oe != e && p.side == side)
        .min_by(|a, b| {
            a.3.translation
                .distance(Vec3::new(hoop_x, 0.0, 0.0))
                .partial_cmp(&b.3.translation.distance(Vec3::new(hoop_x, 0.0, 0.0)))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    if let Some((mate_e, _, _, mt, ..)) = mate {
        let dest = mt.translation + Vec3::Y * 1.4;
        let t = 0.35;
        let v = crate::sim::ballistic_velocity(
            [pos.x, 1.4, pos.z],
            [dest.x, dest.y, dest.z],
            t,
            crate::sim::GRAVITY * 0.4,
        );
        bvel.0 = Vec3::new(v[0], v[1], v[2]);
        st.hold = Hold::Pass;
        st.holder = None;
        st.last_touch = Some(e);
        st.last_passer = Some(e);
        last_pass.passer = Some(e);
        last_pass.age = 0.0;
        if let Ok((_, _, _, _, _, mut pose, mut clock, _, _, _)) = players.get_mut(e) {
            *pose = Pose::Pass;
            clock.0 = 0.0;
        }
        let _ = mate_e;
    }
}
