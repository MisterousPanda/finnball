use bevy::prelude::*;

use crate::sim::{BALL_RADIUS, COURT_HALF_LEN, COURT_HALF_WID, GRAVITY, HOOP_X, RIM_HEIGHT, RIM_RADIUS, rim_score_window};
use crate::states::{AppState, MatchConfig, Paused};

pub struct BallPlugin;

impl Plugin for BallPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            FixedUpdate,
            (integrate_ball, hoop_collision)
                .chain()
                .run_if(in_state(AppState::Playing)),
        );
    }
}

#[derive(Component)]
pub struct Ball;

#[derive(Component, Default)]
pub struct BallVel(pub Vec3);

#[derive(Component, Default)]
pub struct BallSpin(pub Vec3);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hold {
    Loose,
    Held,
    Shot,
    Pass,
}

#[derive(Component)]
pub struct BallState {
    pub hold: Hold,
    pub holder: Option<Entity>,
    pub shooter: Option<Entity>,
    pub last_touch: Option<Entity>,
}

#[derive(Message, Clone, Copy)]
pub struct BucketEvent {
    pub shooter: Option<Entity>,
    pub hoop_home: bool,
    pub dunk: bool,
}

pub fn spawn_ball(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    pos: Vec3,
) -> Entity {
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.42, 0.12),
        perceptual_roughness: 0.55,
        metallic: 0.05,
        emissive: LinearRgba::new(0.15, 0.04, 0.0, 1.0),
        ..default()
    });
    let stripe = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.08, 0.08),
        ..default()
    });
    commands
        .spawn((
            Ball,
            BallVel(Vec3::ZERO),
            BallSpin(Vec3::new(0.0, 4.0, 0.0)),
            BallState {
                hold: Hold::Loose,
                holder: None,
                shooter: None,
                last_touch: None,
            },
            Mesh3d(meshes.add(Sphere::new(BALL_RADIUS))),
            MeshMaterial3d(mat),
            Transform::from_translation(pos),
            crate::court::ArenaRoot,
            DespawnOnExit(crate::states::AppState::Playing),
        ))
        .with_children(|b| {
            b.spawn((
                Mesh3d(meshes.add(Cuboid::new(BALL_RADIUS * 2.05, 0.012, 0.012))),
                MeshMaterial3d(stripe.clone()),
            ));
            b.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.012, BALL_RADIUS * 2.05, 0.012))),
                MeshMaterial3d(stripe),
            ));
        })
        .id()
}

fn integrate_ball(
    time: Res<Time<Fixed>>,
    paused: Res<Paused>,
    config: Res<MatchConfig>,
    mut q: Query<(&mut Transform, &mut BallVel, &mut BallSpin, &BallState), With<Ball>>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let theme = config.arena.theme();
    for (mut tf, mut vel, mut spin, state) in &mut q {
        if state.hold == Hold::Held {
            continue;
        }
        vel.0.y -= GRAVITY * dt * theme.hangtime.recip().max(0.7);
        tf.translation += vel.0 * dt;
        let omega = spin.0.length();
        if omega > 0.01 {
            tf.rotate(Quat::from_axis_angle(spin.0.normalize(), omega * dt));
        }

        // Floor
        if tf.translation.y < BALL_RADIUS {
            tf.translation.y = BALL_RADIUS;
            if vel.0.y < 0.0 {
                vel.0.y = -vel.0.y * theme.bounce;
                vel.0.x *= 0.82;
                vel.0.z *= 0.82;
                spin.0 *= 0.85;
                if vel.0.y.abs() < 0.6 {
                    vel.0.y = 0.0;
                    vel.0.x *= 0.9;
                    vel.0.z *= 0.9;
                }
            }
        }
        // Sidelines / baselines bounce (keep live ball in play for arcade)
        let (hx, hz) = (COURT_HALF_LEN - 0.2, COURT_HALF_WID - 0.2);
        if tf.translation.x.abs() > hx {
            tf.translation.x = tf.translation.x.clamp(-hx, hx);
            vel.0.x = -vel.0.x * 0.55;
        }
        if tf.translation.z.abs() > hz {
            tf.translation.z = tf.translation.z.clamp(-hz, hz);
            vel.0.z = -vel.0.z * 0.55;
        }
    }
}

fn hoop_collision(
    mut buckets: MessageWriter<BucketEvent>,
    paused: Res<Paused>,
    mut ball_q: Query<(&mut Transform, &mut BallVel, &mut BallState), With<Ball>>,
) {
    if paused.0 {
        return;
    }
    let Ok((mut tf, mut vel, mut state)) = ball_q.single_mut() else {
        return;
    };
    if state.hold == Hold::Held {
        return;
    }
    for home in [true, false] {
        let hoop = Vec3::new(if home { -HOOP_X } else { HOOP_X }, RIM_HEIGHT, 0.0);
        let p = tf.translation;
        if rim_score_window([p.x, p.y, p.z], vel.0.y, [hoop.x, hoop.y, hoop.z])
            && matches!(state.hold, Hold::Shot | Hold::Pass | Hold::Loose)
        {
            let dunk = p.y > RIM_HEIGHT - 0.05 && vel.0.length() > 6.0;
            buckets.write(BucketEvent {
                shooter: state.shooter.or(state.last_touch),
                hoop_home: home,
                dunk,
            });
            state.hold = Hold::Loose;
            state.holder = None;
            state.shooter = None;
            vel.0 = Vec3::new(0.0, -1.5, 0.0);
            tf.translation = hoop + Vec3::new(0.0, -0.15, 0.0);
            continue;
        }
        // Rim iron
        let dx = p.x - hoop.x;
        let dz = p.z - hoop.z;
        let horiz = (dx * dx + dz * dz).sqrt();
        let dy = p.y - hoop.y;
        if (horiz - RIM_RADIUS).abs() < 0.08 && dy.abs() < 0.08 {
            let n = Vec3::new(dx, 0.0, dz).normalize_or_zero();
            if n.dot(vel.0) < 0.0 {
                vel.0 = vel.0 - 1.6 * vel.0.dot(n) * n;
                vel.0.y *= 0.7;
            }
        }
        // Backboard
        let board_x = hoop.x + if home { -0.42 } else { 0.42 };
        if (p.x - board_x).abs() < 0.12 && (p.y - (RIM_HEIGHT + 0.32)).abs() < 0.55 && p.z.abs() < 0.92
        {
            vel.0.x = -vel.0.x * 0.72;
            tf.translation.x = board_x + if home { 0.13 } else { -0.13 };
        }
    }
}
