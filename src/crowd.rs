//! Arena crowd: thousands of seated, individually coloured people merged into a few big
//! vertex-coloured meshes. A tiny vertex shader (assets/shaders/crowd.wgsl) animates every
//! fan by phase, so the whole bowl costs ~30 draw calls and still breathes, waves and jumps.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use crate::camera::CameraPostFx;
use crate::states::AppState;

pub struct CrowdPlugin;

impl Plugin for CrowdPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<CrowdMaterial>::default())
            .init_resource::<CrowdHype>()
            .add_systems(Update, drive_crowd);
    }
}

pub type CrowdMaterial = ExtendedMaterial<StandardMaterial, CrowdExt>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub struct CrowdExt {
    /// (time, hype, wave speed, unused)
    #[uniform(100)]
    pub params: Vec4,
}

impl MaterialExtension for CrowdExt {
    fn vertex_shader() -> ShaderRef {
        "shaders/crowd.wgsl".into()
    }
}

#[derive(Resource, Default)]
pub struct CrowdHype {
    pub level: f32,
}

#[derive(Component)]
pub struct CrowdSection;

/// Deterministic tiny RNG so every arena builds the same crowd.
pub struct Lcg(pub u32);

impl Lcg {
    pub fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / 16_777_216.0
    }
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        let i = (self.next() * items.len() as f32) as usize;
        &items[i.min(items.len() - 1)]
    }
}

/// Primitive meshes reused for every body part.
pub struct Parts {
    head: Mesh,
    hair: Mesh,
    torso: Mesh,
    limb: Mesh,
    block: Mesh,
}

impl Parts {
    pub fn new() -> Self {
        Self {
            head: Sphere::new(1.0).mesh().ico(1).unwrap(),
            hair: Sphere::new(1.0).mesh().ico(0).unwrap(),
            torso: Capsule3d::new(1.0, 1.0)
                .mesh()
                .latitudes(4)
                .longitudes(8)
                .rings(1)
                .build(),
            limb: Capsule3d::new(1.0, 1.0)
                .mesh()
                .latitudes(4)
                .longitudes(6)
                .rings(0)
                .build(),
            block: Cuboid::new(1.0, 1.0, 1.0).mesh().build(),
        }
    }
}

/// Accumulates transformed primitives into one vertex-coloured mesh.
#[derive(Default)]
pub struct Batch {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Batch {
    pub fn push(&mut self, mesh: &Mesh, tf: Transform, color: [f32; 3], weight: f32, phase: f32) {
        let Some(p) = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
        else {
            return;
        };
        let Some(n) = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(|a| a.as_float3())
        else {
            return;
        };
        let base = self.pos.len() as u32;
        let m = tf.to_matrix();
        let rot = tf.rotation;
        for (pp, nn) in p.iter().zip(n.iter()) {
            let wp = m.transform_point3(Vec3::from(*pp));
            // Non-uniform scale is small here; rotating the normal is close enough.
            let wn = (rot * Vec3::from(*nn)).normalize_or_zero();
            self.pos.push(wp.to_array());
            self.nrm.push(wn.to_array());
            self.uv.push([weight, phase]);
            self.col.push([color[0], color[1], color[2], 1.0]);
        }
        if let Some(ind) = mesh.indices() {
            self.idx.extend(ind.iter().map(|i| base + i as u32));
        } else {
            self.idx.extend((0..p.len() as u32).map(|i| base + i));
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.pos.len()
    }

    pub fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
        .with_inserted_indices(Indices::U32(self.idx))
    }
}

pub struct CrowdStyle {
    pub shirts: Vec<[f32; 3]>,
    pub skins: Vec<[f32; 3]>,
    pub hairs: Vec<[f32; 3]>,
    pub pants: Vec<[f32; 3]>,
    pub seat: [f32; 3],
}

pub fn lin(c: Color) -> [f32; 3] {
    srgb(c)
}

fn srgb(c: Color) -> [f32; 3] {
    // Vertex colours are linear; StandardMaterial multiplies them straight into base colour.
    let l = c.to_linear();
    [l.red, l.green, l.blue]
}

impl CrowdStyle {
    pub fn arena(home: Color, away: Color, accent: Color, seat: Color) -> Self {
        Self {
            shirts: vec![
                srgb(home),
                srgb(home),
                srgb(home),
                srgb(away),
                srgb(away),
                srgb(accent),
                srgb(Color::srgb(0.95, 0.95, 0.97)),
                srgb(Color::srgb(0.9, 0.9, 0.92)),
                srgb(Color::srgb(0.08, 0.08, 0.1)),
                srgb(Color::srgb(0.12, 0.12, 0.16)),
                srgb(Color::srgb(0.85, 0.25, 0.2)),
                srgb(Color::srgb(0.95, 0.75, 0.15)),
                srgb(Color::srgb(0.2, 0.55, 0.3)),
                srgb(Color::srgb(0.35, 0.35, 0.4)),
                srgb(Color::srgb(0.55, 0.3, 0.6)),
            ],
            skins: vec![
                srgb(Color::srgb(0.98, 0.86, 0.74)),
                srgb(Color::srgb(0.92, 0.74, 0.6)),
                srgb(Color::srgb(0.82, 0.6, 0.45)),
                srgb(Color::srgb(0.62, 0.42, 0.3)),
                srgb(Color::srgb(0.45, 0.29, 0.2)),
                srgb(Color::srgb(0.32, 0.2, 0.14)),
            ],
            hairs: vec![
                srgb(Color::srgb(0.08, 0.06, 0.05)),
                srgb(Color::srgb(0.12, 0.08, 0.05)),
                srgb(Color::srgb(0.3, 0.18, 0.1)),
                srgb(Color::srgb(0.55, 0.38, 0.2)),
                srgb(Color::srgb(0.85, 0.7, 0.4)),
                srgb(Color::srgb(0.6, 0.6, 0.62)),
                srgb(Color::srgb(0.7, 0.2, 0.15)),
                srgb(accent),
            ],
            pants: vec![
                srgb(Color::srgb(0.12, 0.14, 0.25)),
                srgb(Color::srgb(0.08, 0.08, 0.1)),
                srgb(Color::srgb(0.3, 0.3, 0.32)),
                srgb(Color::srgb(0.45, 0.4, 0.35)),
            ],
            seat: srgb(seat),
        }
    }
}

/// One stadium seat at `origin` (tier floor, seat centreline), facing local +z.
pub fn seat(batch: &mut Batch, parts: &Parts, origin: Vec3, yaw: f32, style: &CrowdStyle) {
    let rot = Quat::from_rotation_y(yaw);
    let place = |local: Vec3| origin + rot * local;
    let mut tint = style.seat;
    tint[0] *= 0.9;
    tint[1] *= 0.9;
    tint[2] *= 0.9;
    batch.push(
        &parts.block,
        Transform {
            translation: place(Vec3::new(0.0, 0.42, 0.0)),
            rotation: rot,
            scale: Vec3::new(0.5, 0.06, 0.46),
        },
        style.seat,
        0.0,
        0.0,
    );
    batch.push(
        &parts.block,
        Transform {
            translation: place(Vec3::new(0.0, 0.66, -0.22)),
            rotation: rot * Quat::from_rotation_x(-0.12),
            scale: Vec3::new(0.5, 0.48, 0.06),
        },
        tint,
        0.0,
        0.0,
    );
}

/// A person on the seat at `origin`. `standing` fans are on their feet with arms up.
pub fn fan(
    batch: &mut Batch,
    parts: &Parts,
    rng: &mut Lcg,
    origin: Vec3,
    yaw: f32,
    style: &CrowdStyle,
    standing: bool,
) {
    let rot = Quat::from_rotation_y(yaw);
    let place = |local: Vec3| origin + rot * local;
    let phase = rng.next();
    let shirt = *rng.pick(&style.shirts);
    let skin = *rng.pick(&style.skins);
    let hair = *rng.pick(&style.hairs);
    let pants = *rng.pick(&style.pants);
    let size = 0.92 + rng.next() * 0.18;
    let arms_up = standing || rng.next() < 0.18;
    let hat = rng.next() < 0.12;

    let (torso_y, torso_len, head_y) = if standing {
        (1.05 * size, 0.42, 1.52 * size)
    } else {
        (0.8 * size, 0.3, 1.2 * size)
    };
    let lean = if standing { 0.0 } else { -0.1 };

    // torso
    batch.push(
        &parts.torso,
        Transform {
            translation: place(Vec3::new(0.0, torso_y, -0.04)),
            rotation: rot * Quat::from_rotation_x(lean),
            scale: Vec3::new(0.19, torso_len * 0.5, 0.15),
        },
        shirt,
        0.55,
        phase,
    );
    // shoulders as a flattened capsule across the top of the torso
    batch.push(
        &parts.limb,
        Transform {
            translation: place(Vec3::new(0.0, torso_y + torso_len * 0.45, -0.05)),
            rotation: rot * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            scale: Vec3::new(0.09, 0.17, 0.09),
        },
        shirt,
        0.7,
        phase,
    );
    // neck + head + hair
    batch.push(
        &parts.limb,
        Transform {
            translation: place(Vec3::new(0.0, head_y - 0.14, -0.03)),
            rotation: rot,
            scale: Vec3::new(0.045, 0.04, 0.045),
        },
        skin,
        0.9,
        phase,
    );
    batch.push(
        &parts.head,
        Transform {
            translation: place(Vec3::new(0.0, head_y, -0.02)),
            rotation: rot,
            scale: Vec3::splat(0.115),
        },
        skin,
        1.0,
        phase,
    );
    let hair_col = if hat { *rng.pick(&style.shirts) } else { hair };
    batch.push(
        &parts.hair,
        Transform {
            translation: place(Vec3::new(0.0, head_y + 0.035, -0.055)),
            rotation: rot,
            scale: if hat {
                Vec3::new(0.125, 0.09, 0.125)
            } else {
                Vec3::new(0.122, 0.105, 0.12)
            },
        },
        hair_col,
        1.0,
        phase,
    );
    // arms
    for sx in [-1.0f32, 1.0] {
        let (center, rotation, len) = if arms_up {
            (
                Vec3::new(sx * 0.24, torso_y + torso_len * 0.5 + 0.2, -0.02),
                rot * Quat::from_rotation_z(-sx * 0.25),
                0.16,
            )
        } else {
            (
                Vec3::new(sx * 0.22, torso_y - 0.02, 0.1),
                rot * Quat::from_rotation_x(0.95) * Quat::from_rotation_z(-sx * 0.08),
                0.15,
            )
        };
        batch.push(
            &parts.limb,
            Transform {
                translation: place(center),
                rotation,
                scale: Vec3::new(0.055, len, 0.055),
            },
            shirt,
            if arms_up { 1.0 } else { 0.6 },
            phase,
        );
        // hand
        let hand = if arms_up {
            center + Vec3::new(sx * 0.04, len + 0.03, 0.0)
        } else {
            center + Vec3::new(0.0, -0.1, 0.12)
        };
        batch.push(
            &parts.hair,
            Transform {
                translation: place(hand),
                rotation: rot,
                scale: Vec3::splat(0.045),
            },
            skin,
            if arms_up { 1.0 } else { 0.6 },
            phase,
        );
    }
    // legs
    for sx in [-1.0f32, 1.0] {
        if standing {
            batch.push(
                &parts.limb,
                Transform {
                    translation: place(Vec3::new(sx * 0.1, 0.42 * size, 0.0)),
                    rotation: rot,
                    scale: Vec3::new(0.07, 0.36 * size, 0.07),
                },
                pants,
                0.15,
                phase,
            );
        } else {
            // thigh forward, shin down
            batch.push(
                &parts.limb,
                Transform {
                    translation: place(Vec3::new(sx * 0.1, 0.5 * size, 0.2)),
                    rotation: rot * Quat::from_rotation_x(1.45),
                    scale: Vec3::new(0.07, 0.17, 0.07),
                },
                pants,
                0.25,
                phase,
            );
            batch.push(
                &parts.limb,
                Transform {
                    translation: place(Vec3::new(sx * 0.1, 0.22 * size, 0.38)),
                    rotation: rot,
                    scale: Vec3::new(0.06, 0.18, 0.06),
                },
                pants,
                0.0,
                phase,
            );
        }
    }
}

fn drive_crowd(
    time: Res<Time>,
    fx: Res<CameraPostFx>,
    state: Res<State<AppState>>,
    mut hype: ResMut<CrowdHype>,
    mut mats: ResMut<Assets<CrowdMaterial>>,
) {
    let target = if *state.get() == AppState::Playing {
        fx.crowd_flash.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dt = time.delta_secs();
    // Fast attack, slow decay: the bowl erupts and then settles.
    if target > hype.level {
        hype.level += (target - hype.level) * (1.0 - (-12.0 * dt).exp());
    } else {
        hype.level += (target - hype.level) * (1.0 - (-1.6 * dt).exp());
    }
    let params = Vec4::new(time.elapsed_secs(), hype.level, 0.9, 0.0);
    for (_, mat) in mats.iter_mut() {
        mat.extension.params = params;
    }
}
