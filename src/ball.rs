use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::sim::{
    apply_aero, cylinder_score, BALL_RADIUS, COURT_HALF_LEN, COURT_HALF_WID, GRAVITY, HOOP_X,
    RIM_HEIGHT, RIM_RADIUS,
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
            )
            .add_systems(
                Update,
                (spin_ball_skin, squash_and_glow).run_if(in_state(AppState::Playing)),
            );
    }
}

/// Visual skin child of the ball: it carries the leather mesh and takes the spin, so
/// the root can be squashed along world axes without the flattening rotating away.
#[derive(Component)]
pub struct BallSkin;

/// Halo child; scales with speed and swaps to the hot look when the shooter is on fire.
#[derive(Component)]
pub struct BallGlow {
    pub cool: Handle<StandardMaterial>,
    pub hot: Handle<StandardMaterial>,
}

/// Floor-bounce squash timer on the ball root.
#[derive(Component, Default)]
pub struct BallSquash {
    pub t: f32,
    pub amount: f32,
}

pub const SQUASH_TIME: f32 = 0.06;

/// Squash factor (1 = round) for a bounce of `impact` m/s at `t` seconds after contact.
pub fn squash_scale(impact: f32, t: f32) -> Vec3 {
    if t <= 0.0 || t > SQUASH_TIME {
        return Vec3::ONE;
    }
    let k = (t / SQUASH_TIME).clamp(0.0, 1.0);
    let amount = (0.15 * (impact / 6.0).clamp(0.6, 1.4)).min(0.2);
    // Full squash right at contact, easing back to round.
    let y = 1.0 - amount * k;
    let xz = 1.0 + amount * 0.6 * k;
    Vec3::new(xz, y, xz)
}

/// Halo radius multiplier for a ball moving at `speed` m/s.
pub fn glow_scale(speed: f32, hot: bool) -> f32 {
    let base = 1.0 + (speed * 0.035).min(0.6);
    if hot {
        base * 1.25
    } else {
        base
    }
}

/// Arcade ball: rendered bigger than the sim radius so it reads from the broadcast lens.
pub const VISUAL_RADIUS: f32 = BALL_RADIUS * 1.55;

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

/// Procedural basketball skin: pebbled orange leather with the classic eight-panel seams.
pub fn paint_ball_texture() -> (u32, u32, Vec<u8>) {
    let (w, h) = (512u32, 256u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let seam_w = 0.012;
    for py in 0..h {
        let v = (py as f32 + 0.5) / h as f32;
        for px in 0..w {
            let u = (px as f32 + 0.5) / w as f32;
            // pebble grain
            let g = ((u * 900.0).sin() * (v * 450.0).cos()
                + (u * 1370.0 + v * 210.0).sin() * 0.6
                + (u * 233.0 - v * 777.0).cos() * 0.4)
                * 0.5
                + 0.5;
            let shade = 0.9 + g * 0.16;
            let mut col = [1.0 * shade, 0.46 * shade, 0.11 * shade];
            // seams: equator, two meridians, two curved panel seams
            let mut d = (v - 0.5).abs();
            for m in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
                d = d.min((u - m).abs() * 0.5);
            }
            let curve = 0.5 + 0.3 * (u * std::f32::consts::TAU).sin();
            let curve2 = 0.5 - 0.3 * (u * std::f32::consts::TAU).sin();
            d = d.min((v - curve).abs() * 0.85);
            d = d.min((v - curve2).abs() * 0.85);
            let cover = ((seam_w - d) / 0.004 + 0.5).clamp(0.0, 1.0);
            let seam = [0.08, 0.05, 0.04];
            for i in 0..3 {
                col[i] = col[i] + (seam[i] - col[i]) * cover;
            }
            let idx = ((py * w + px) * 4) as usize;
            rgba[idx] = (col[0].clamp(0.0, 1.0) * 255.0) as u8;
            rgba[idx + 1] = (col[1].clamp(0.0, 1.0) * 255.0) as u8;
            rgba[idx + 2] = (col[2].clamp(0.0, 1.0) * 255.0) as u8;
            rgba[idx + 3] = 255;
        }
    }
    (w, h, rgba)
}

pub fn spawn_ball(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    pos: Vec3,
) -> Entity {
    let (w, h, rgba) = paint_ball_texture();
    let mut img = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    let skin = images.add(img);
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(skin),
        perceptual_roughness: 0.62,
        metallic: 0.0,
        emissive: LinearRgba::new(0.32, 0.1, 0.015, 1.0),
        ..default()
    });
    let glow = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.45, 0.1, 0.16),
        emissive: LinearRgba::new(1.6, 0.45, 0.06, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let glow_hot = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.3, 0.05, 0.3),
        emissive: LinearRgba::new(4.5, 1.1, 0.1, 1.0),
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
            BallSquash::default(),
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
            Transform::from_translation(pos),
            Visibility::default(),
            crate::court::ArenaRoot,
            DespawnOnExit(AppState::Playing),
        ))
        .with_children(|b| {
            b.spawn((
                BallSkin,
                Mesh3d(meshes.add(Sphere::new(VISUAL_RADIUS).mesh().uv(40, 24))),
                MeshMaterial3d(mat),
                Transform::IDENTITY,
            ));
            b.spawn((
                BallGlow {
                    cool: glow.clone(),
                    hot: glow_hot,
                },
                Mesh3d(meshes.add(Sphere::new(VISUAL_RADIUS * 1.32))),
                MeshMaterial3d(glow),
                Transform::IDENTITY,
            ));
        })
        .id();

    commands.spawn((
        BallShadow,
        Mesh3d(meshes.add(Cylinder::new(0.26, 0.02))),
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
    mut q: Query<
        (
            &mut Transform,
            &mut BallVel,
            &mut BallSpin,
            &mut BallPrev,
            &mut BallState,
        ),
        With<Ball>,
    >,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let theme = config.arena.theme();
    for (mut tf, mut vel, mut spin, mut prev, mut state) in &mut q {
        prev.0 = tf.translation;
        if state.hold == Hold::Held {
            continue;
        }
        // A shot or pass that has died on the floor is a live loose ball again, so the AI
        // hunts it instead of leaving it parked at midcourt.
        if matches!(state.hold, Hold::Shot | Hold::Pass)
            && tf.translation.y < 0.6
            && vel.0.length() < 2.4
        {
            state.hold = Hold::Loose;
        }
        vel.0.y -= GRAVITY * dt * theme.hangtime.recip().max(0.7);
        // Shots stay on the solved ballistic so a green make still threads the rim.
        // Loose / pass balls get drag + a little English.
        if state.hold != Hold::Shot {
            let aero = apply_aero(vel.0.to_array(), spin.0.to_array(), dt);
            vel.0 = Vec3::from_array(aero);
        }
        tf.translation += vel.0 * dt;
        // Spin is applied to the `BallSkin` child in `spin_ball_skin`.

        if tf.translation.y < VISUAL_RADIUS {
            tf.translation.y = VISUAL_RADIUS;
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
        (
            &mut Transform,
            &mut BallVel,
            &mut BallSpin,
            &BallPrev,
            &mut BallState,
        ),
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
                rims.write(RimHitEvent { pos: p, speed });
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

/// Rotates the leather skin by the sim spin; a held ball keeps a lazy roll so the seams
/// still move while dribbling.
fn spin_ball_skin(
    time: Res<Time>,
    paused: Res<Paused>,
    ball: Query<(&BallSpin, &BallState, &BallVel), With<Ball>>,
    mut skin: Query<&mut Transform, (With<BallSkin>, Without<Ball>)>,
) {
    if paused.0 {
        return;
    }
    let Ok((spin, state, vel)) = ball.single() else {
        return;
    };
    let dt = time.delta_secs();
    let omega = if state.hold == Hold::Held {
        // roll about the axis perpendicular to travel, like a dribbled ball would
        let v = Vec3::new(vel.0.x, 0.0, vel.0.z);
        let axis = Vec3::Y.cross(v);
        if axis.length_squared() > 0.01 {
            axis.normalize() * (v.length() / VISUAL_RADIUS) * 0.35 + Vec3::Y * 2.0
        } else {
            Vec3::Y * 2.5
        }
    } else {
        spin.0
    };
    let len = omega.length();
    if len > 0.01 {
        for mut tf in &mut skin {
            tf.rotate(Quat::from_axis_angle(omega / len, len * dt));
        }
    }
}

/// Floor-bounce squash on the root and a speed / heat driven halo.
fn squash_and_glow(
    time: Res<Time>,
    paused: Res<Paused>,
    mut floor: MessageReader<FloorBounceEvent>,
    heats: Query<&crate::units::Heat>,
    mut ball: Query<(&mut Transform, &mut BallSquash, &BallVel, &BallState), With<Ball>>,
    mut glow: Query<
        (&mut Transform, &BallGlow, &mut MeshMaterial3d<StandardMaterial>),
        (Without<Ball>, Without<BallSkin>),
    >,
) {
    if paused.0 {
        return;
    }
    let Ok((mut tf, mut squash, vel, state)) = ball.single_mut() else {
        return;
    };
    for ev in floor.read() {
        squash.t = SQUASH_TIME;
        squash.amount = ev.speed;
    }
    let dt = time.delta_secs();
    tf.scale = squash_scale(squash.amount, squash.t);
    squash.t = (squash.t - dt).max(0.0);

    let hot = state
        .shooter
        .or(state.holder)
        .and_then(|e| heats.get(e).ok())
        .map(|h| h.on_fire())
        .unwrap_or(false);
    let speed = vel.0.length();
    let flicker = if hot {
        1.0 + (time.elapsed_secs() * 21.0).sin() * 0.06
    } else {
        1.0
    };
    for (mut gtf, g, mut mat) in &mut glow {
        let s = glow_scale(speed, hot) * flicker;
        gtf.scale = Vec3::splat(s);
        let want = if hot { &g.hot } else { &g.cool };
        if mat.0 != *want {
            mat.0 = want.clone();
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
    let height = btf.translation.y.max(VISUAL_RADIUS);
    let scale = (0.55 + height * 0.12).clamp(0.45, 1.6);
    st.translation = Vec3::new(btf.translation.x, 0.025, btf.translation.z);
    st.scale = Vec3::new(scale, 1.0, scale);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ball_texture_is_orange_with_dark_seams() {
        let (w, h, rgba) = paint_ball_texture();
        assert_eq!(rgba.len(), (w * h * 4) as usize);
        // A pixel in the middle of a panel is orange…
        let px = |u: f32, v: f32| {
            let x = (u * w as f32) as u32;
            let y = (v * h as f32) as u32;
            let i = ((y * w + x) * 4) as usize;
            (rgba[i], rgba[i + 1], rgba[i + 2])
        };
        let panel = px(0.125, 0.42);
        assert!(panel.0 > 200 && panel.2 < 60);
        // …and the equator seam is dark.
        let seam = px(0.125, 0.5);
        assert!(seam.0 < 60);
    }

    #[test]
    fn squash_flattens_then_recovers() {
        let contact = squash_scale(6.0, SQUASH_TIME);
        assert!(contact.y < 0.9 && contact.y > 0.8, "y = {}", contact.y);
        assert!(contact.x > 1.0 && (contact.x - contact.z).abs() < 1e-6);
        let half = squash_scale(6.0, SQUASH_TIME * 0.5);
        assert!(half.y > contact.y);
        assert_eq!(squash_scale(6.0, 0.0), Vec3::ONE);
        assert_eq!(squash_scale(6.0, 1.0), Vec3::ONE);
        // Harder impacts squash more, but never past the cap.
        assert!(squash_scale(12.0, SQUASH_TIME).y <= squash_scale(3.0, SQUASH_TIME).y);
        assert!(squash_scale(100.0, SQUASH_TIME).y >= 0.8);
    }

    #[test]
    fn glow_grows_with_speed_and_heat() {
        assert!((glow_scale(0.0, false) - 1.0).abs() < 1e-6);
        assert!(glow_scale(8.0, false) > glow_scale(2.0, false));
        assert!(glow_scale(50.0, false) <= 1.6 + 1e-6);
        assert!(glow_scale(5.0, true) > glow_scale(5.0, false));
    }
}
