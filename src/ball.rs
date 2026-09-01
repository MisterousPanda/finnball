use bevy::prelude::*;

use crate::sim::{
    apply_aero, BALL_RADIUS, COURT_HALF_LEN, COURT_HALF_WID, GRAVITY, HOOP_X, RIM_HEIGHT,
    RIM_RADIUS, cylinder_score,
};
use crate::states::{AppState, MatchConfig, Paused};

pub struct BallPlugin;

impl Plugin for BallPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<RimHitEvent>()
            .add_message::<BackboardHitEvent>()
            .add_message::<FloorBounceEvent>()
            .add_systems(
                FixedUpdate,
                (integrate_ball, hoop_collision, follow_ball_shadow)
                    .chain()
                    .run_if(in_state(AppState::Playing)),
            );
    }
}

#[derive(Component)]
pub struct Ball;

#[derive(Component)]
pub struct BallShadow;

#[derive(Component, Default)]
pub struct BallVel(pub Vec3);

#[derive(Component, Default)]
pub struct BallSpin(pub Vec3);

#[derive(Component, Default)]
pub struct BallPrev(pub Vec3);

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
    pub last_passer: Option<Entity>,
    pub dribble_phase: f32,
    pub rim_hits: u8,
    pub release_was_three: bool,
}

#[derive(Message, Clone, Copy)]
pub struct BucketEvent {
    pub shooter: Option<Entity>,
    pub hoop_home: bool,
    pub dunk: bool,
    pub is_three: bool,
}

#[derive(Message, Clone, Copy)]
pub struct RimHitEvent {
    pub pos: Vec3,
    pub speed: f32,
}

#[derive(Message, Clone, Copy)]
pub struct BackboardHitEvent {
    pub pos: Vec3,
}

#[derive(Message, Clone, Copy)]
pub struct FloorBounceEvent {
    pub pos: Vec3,
    pub speed: f32,
}

pub fn spawn_ball(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    pos: Vec3,
) -> Entity {
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.48, 0.12),
        perceptual_roughness: 0.42,
        metallic: 0.08,
        emissive: LinearRgba::new(0.55, 0.16, 0.02, 1.0),
        ..default()
    });
    let stripe = materials.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.04, 0.05),
        emissive: LinearRgba::new(0.02, 0.02, 0.02, 1.0),
        ..default()
    });
    let glow = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.45, 0.1, 0.22),
        emissive: LinearRgba::new(1.2, 0.35, 0.05, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let shadow_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.0, 0.0, 0.0, 0.45),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });

    let ball = commands
        .spawn((
            Ball,
            BallVel(Vec3::ZERO),
            BallSpin(Vec3::new(0.0, 8.0, 0.0)),
            BallPrev(pos),
            BallState {
                hold: Hold::Loose,
                holder: None,
                shooter: None,
                last_touch: None,
                last_passer: None,
                dribble_phase: 0.0,
                rim_hits: 0,
                release_was_three: false,
            },
            Mesh3d(meshes.add(Sphere::new(BALL_RADIUS * 1.15))),
            MeshMaterial3d(mat),
            Transform::from_translation(pos),
            crate::court::ArenaRoot,
            DespawnOnExit(AppState::Playing),
        ))
        .with_children(|b| {
            b.spawn((
                Mesh3d(meshes.add(Cuboid::new(BALL_RADIUS * 2.4, 0.018, 0.018))),
                MeshMaterial3d(stripe.clone()),
            ));
            b.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.018, BALL_RADIUS * 2.4, 0.018))),
                MeshMaterial3d(stripe.clone()),
            ));
            b.spawn((
                Mesh3d(meshes.add(Cuboid::new(0.018, 0.018, BALL_RADIUS * 2.4))),
                MeshMaterial3d(stripe),
            ));
            b.spawn((
                Mesh3d(meshes.add(Sphere::new(BALL_RADIUS * 1.55))),
                MeshMaterial3d(glow),
            ));
        })
        .id();

    commands.spawn((
        BallShadow,
        Mesh3d(meshes.add(Cylinder::new(0.22, 0.02))),
        MeshMaterial3d(shadow_mat),
        Transform::from_xyz(pos.x, 0.02, pos.z),
        crate::court::ArenaRoot,
        DespawnOnExit(AppState::Playing),
    ));

    ball
}

fn integrate_ball(
    time: Res<Time<Fixed>>,
    paused: Res<Paused>,
    config: Res<MatchConfig>,
    mut floor: MessageWriter<FloorBounceEvent>,
    mut q: Query<(&mut Transform, &mut BallVel, &mut BallSpin, &mut BallPrev, &BallState), With<Ball>>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let theme = config.arena.theme();
    for (mut tf, mut vel, mut spin, mut prev, state) in &mut q {
        prev.0 = tf.translation;
        if state.hold == Hold::Held {
            continue;
        }
        vel.0.y -= GRAVITY * dt * theme.hangtime.recip().max(0.7);
        // Shots stay on the solved ballistic so a green make still threads the rim.
        // Loose / pass balls get drag + a little English.
        if state.hold != Hold::Shot {
            let aero = apply_aero(vel.0.to_array(), spin.0.to_array(), dt);
            vel.0 = Vec3::from_array(aero);
        }
        tf.translation += vel.0 * dt;
        let omega = spin.0.length();
        if omega > 0.01 {
            tf.rotate(Quat::from_axis_angle(spin.0.normalize(), omega * dt));
        }

        if tf.translation.y < BALL_RADIUS {
            tf.translation.y = BALL_RADIUS;
            if vel.0.y < 0.0 {
                let impact = vel.0.y.abs();
                vel.0.y = -vel.0.y * theme.bounce;
                vel.0.x *= 0.82;
                vel.0.z *= 0.82;
                spin.0 *= 0.85;
                if impact > 1.2 {
                    floor.write(FloorBounceEvent {
                        pos: tf.translation,
                        speed: impact,
                    });
                }
                if vel.0.y.abs() < 0.6 {
                    vel.0.y = 0.0;
                    vel.0.x *= 0.9;
                    vel.0.z *= 0.9;
                }
            }
        }
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
    mut rims: MessageWriter<RimHitEvent>,
    mut boards: MessageWriter<BackboardHitEvent>,
    paused: Res<Paused>,
    mut ball_q: Query<
        (&mut Transform, &mut BallVel, &mut BallSpin, &BallPrev, &mut BallState),
        With<Ball>,
    >,
) {
    if paused.0 {
        return;
    }
    let Ok((mut tf, mut vel, mut spin, prev, mut state)) = ball_q.single_mut() else {
        return;
    };
    if state.hold == Hold::Held {
        return;
    }
    for home in [true, false] {
        let hoop = Vec3::new(if home { -HOOP_X } else { HOOP_X }, RIM_HEIGHT, 0.0);
        let p = tf.translation;
        if cylinder_score(prev.0.to_array(), p.to_array(), vel.0.y, hoop.to_array())
            && matches!(state.hold, Hold::Shot | Hold::Pass | Hold::Loose)
        {
            let dunk = p.y > RIM_HEIGHT - 0.08 && vel.0.length() > 5.5 && state.rim_hits == 0;
            buckets.write(BucketEvent {
                shooter: state.shooter.or(state.last_touch),
                hoop_home: home,
                dunk,
                is_three: state.release_was_three && !dunk,
            });
            state.hold = Hold::Loose;
            state.holder = None;
            state.shooter = None;
            state.rim_hits = 0;
            vel.0 = Vec3::new(0.0, -1.5, 0.0);
            spin.0 = Vec3::new(0.0, 10.0, 0.0);
            tf.translation = hoop + Vec3::new(0.0, -0.18, 0.0);
            continue;
        }

        // Rim torus: iron + backspin-friendly inward roll
        let dx = p.x - hoop.x;
        let dz = p.z - hoop.z;
        let horiz = (dx * dx + dz * dz).sqrt();
        let dy = p.y - hoop.y;
        if (horiz - RIM_RADIUS).abs() < 0.09 && dy.abs() < 0.09 && state.rim_hits < 4 {
            let n = Vec3::new(dx, 0.0, dz).normalize_or_zero();
            if n.dot(vel.0) < 0.0 {
                let speed = vel.0.length();
                vel.0 = vel.0 - 1.55 * vel.0.dot(n) * n;
                vel.0.y *= 0.68;
                // Shooter's roll: backspin pulls the ball toward the cylinder
                let backspin = (-spin.0.x).max(0.0);
                if backspin > 10.0 && vel.0.y < 0.2 {
                    let inward = Vec3::new(-dx, 0.0, -dz).normalize_or_zero();
                    vel.0 += inward * (0.9 + backspin * 0.04);
                    vel.0.y -= 0.35;
                }
                spin.0 *= 0.82;
                state.rim_hits = state.rim_hits.saturating_add(1);
                rims.write(RimHitEvent {
                    pos: p,
                    speed,
                });
            }
        }

        let board_x = hoop.x + if home { -0.42 } else { 0.42 };
        if (p.x - board_x).abs() < 0.12
            && (p.y - (RIM_HEIGHT + 0.32)).abs() < 0.55
            && p.z.abs() < 0.92
            && vel.0.x * (board_x - p.x) > 0.0
        {
            vel.0.x = -vel.0.x * 0.68;
            tf.translation.x = board_x + if home { 0.13 } else { -0.13 };
            // Bank alley: if hitting the square with downward motion, bias toward the cylinder
            if (p.z).abs() < 0.28 && (p.y - (RIM_HEIGHT + 0.18)).abs() < 0.22 && vel.0.y < 0.0 {
                let to_rim = (hoop - tf.translation).normalize_or_zero();
                vel.0 = vel.0.lerp(to_rim * vel.0.length() * 0.85, 0.35);
            }
            boards.write(BackboardHitEvent { pos: p });
        }
    }
}

fn follow_ball_shadow(
    ball: Query<&Transform, (With<Ball>, Without<BallShadow>)>,
    mut shadow: Query<&mut Transform, (With<BallShadow>, Without<Ball>)>,
) {
    let Ok(btf) = ball.single() else {
        return;
    };
    let Ok(mut st) = shadow.single_mut() else {
        return;
    };
    let height = btf.translation.y.max(BALL_RADIUS);
    let scale = (0.55 + height * 0.12).clamp(0.45, 1.6);
    st.translation = Vec3::new(btf.translation.x, 0.025, btf.translation.z);
    st.scale = Vec3::new(scale, 1.0, scale);
}
