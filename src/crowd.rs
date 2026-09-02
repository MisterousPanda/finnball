//! Arena crowd: thousands of seated, individually coloured people merged into a few big
//! vertex-coloured meshes. A tiny vertex shader (assets/shaders/crowd.wgsl) animates every
//! fan by phase, so the whole bowl costs a few dozen draw calls and still breathes, waves,
//! stands up, jumps and lights its phones.
//!
//! The same [`Batch`] + [`CrowdMaterial`] pair is also used for static arena dressing
//! (stairs, rails, tunnels, suites, racks, cables): a vertex weight of 0 leaves geometry
//! still, and the `uv_b` channel carries a per-vertex emissive term so LED strips and
//! EXIT signs can glow inside the same merged mesh.

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
    /// (time, hype, stand, fire)
    #[uniform(100)]
    pub params: Vec4,
    /// (wave position around the ring 0..1, wave strength, extra phone glow, arm raise)
    #[uniform(101)]
    pub params2: Vec4,
}

impl Default for CrowdExt {
    fn default() -> Self {
        Self {
            params: Vec4::ZERO,
            params2: Vec4::ZERO,
        }
    }
}

impl MaterialExtension for CrowdExt {
    fn vertex_shader() -> ShaderRef {
        "shaders/crowd.wgsl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "shaders/crowd.wgsl".into()
    }
}

/// Per-vertex "part id" written to `uv_b.y`; the shader treats each differently.
pub const PART_BODY: f32 = 0.0;
pub const PART_ARM: f32 = 1.0;
pub const PART_HEAD: f32 = 2.0;
pub const PART_PHONE: f32 = 3.0;
pub const PART_LED: f32 = 4.0;
pub const PART_SIGN: f32 = 5.0;
pub const PART_IMPOSTOR: f32 = 6.0;
pub const PART_CHEER: f32 = 7.0;

/// Shared crowd mood, driven from gameplay flashes and player heat.
#[derive(Resource)]
pub struct CrowdHype {
    /// 0..1 how loud the bowl is right now.
    pub level: f32,
    /// 0..1 blend for "everyone on their feet".
    pub stand: f32,
    /// 0..1 whether a player is on fire (phones light up, spotlights sweep).
    pub fire: f32,
    /// 0..1 position of the travelling wave around the ring.
    pub wave_pos: f32,
    /// 0..1 amplitude of the travelling wave.
    pub wave_strength: f32,
    /// Seconds until the next wave starts.
    pub wave_timer: f32,
    /// 0..1 arm pumping at peak hype.
    pub arm_raise: f32,
}

impl Default for CrowdHype {
    fn default() -> Self {
        Self {
            level: 0.0,
            stand: 0.0,
            fire: 0.0,
            wave_pos: 0.0,
            wave_strength: 0.0,
            wave_timer: 6.0,
            arm_raise: 0.0,
        }
    }
}

impl CrowdHype {
    /// Pure timing/blend update so the mood logic is testable without ECS.
    pub fn tick(&mut self, dt: f32, target: f32, fire: bool, in_match: bool, rand: f32) {
        // Fast attack, slow decay: the bowl erupts and then settles.
        if target > self.level {
            self.level += (target - self.level) * (1.0 - (-12.0 * dt).exp());
        } else {
            self.level += (target - self.level) * (1.0 - (-1.6 * dt).exp());
        }
        let fire_t = if fire { 1.0 } else { 0.0 };
        let k = if fire_t > self.fire { 4.0 } else { 1.2 };
        self.fire += (fire_t - self.fire) * (1.0 - (-k * dt).exp());

        let stand_t = smoothstep(0.45, 0.85, self.level).max(self.fire * 0.85);
        let k = if stand_t > self.stand { 2.2 } else { 0.7 };
        self.stand += (stand_t - self.stand) * (1.0 - (-k * dt).exp());

        self.arm_raise = smoothstep(0.7, 1.0, self.level).max(self.fire * 0.6);

        // Stadium wave: runs once around the ring then rests for a while. The menu
        // keeps waving so the orbit shot never looks static.
        if self.wave_strength > 0.0 {
            self.wave_pos += dt * 0.105;
            if self.wave_pos >= 1.0 {
                self.wave_pos = 0.0;
                self.wave_strength = 0.0;
                self.wave_timer = if in_match {
                    16.0 + rand * 18.0
                } else {
                    5.0 + rand * 5.0
                };
            }
        } else {
            self.wave_timer -= dt * if in_match { 1.0 + self.level * 2.0 } else { 1.0 };
            if self.wave_timer <= 0.0 {
                self.wave_strength = if in_match {
                    0.6 + self.level * 0.4
                } else {
                    0.85
                };
                self.wave_pos = 0.0;
            }
        }
    }

    pub fn params(&self, time: f32, menu_glow: f32) -> (Vec4, Vec4) {
        (
            Vec4::new(time, self.level, self.stand, self.fire),
            Vec4::new(self.wave_pos, self.wave_strength, menu_glow, self.arm_raise),
        )
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    pub fn chance(&mut self, p: f32) -> bool {
        self.next() < p
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

/// Primitive meshes reused for every body part.
pub struct Parts {
    pub head: Mesh,
    pub hair: Mesh,
    pub torso: Mesh,
    pub limb: Mesh,
    pub block: Mesh,
    pub disc: Mesh,
}

impl Default for Parts {
    fn default() -> Self {
        Self::new()
    }
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
            disc: Cylinder::new(1.0, 1.0).mesh().resolution(8).build(),
        }
    }
}

/// Accumulates transformed primitives into one vertex-coloured mesh.
#[derive(Default)]
pub struct Batch {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    uv1: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Batch {
    pub fn push(&mut self, mesh: &Mesh, tf: Transform, color: [f32; 3], weight: f32, phase: f32) {
        self.push_ex(mesh, tf, color, weight, phase, 0.0, PART_BODY);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_ex(
        &mut self,
        mesh: &Mesh,
        tf: Transform,
        color: [f32; 3],
        weight: f32,
        phase: f32,
        glow: f32,
        part: f32,
    ) {
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
            self.uv1.push([glow, part]);
            self.col.push([color[0], color[1], color[2], 1.0]);
        }
        if let Some(ind) = mesh.indices() {
            self.idx.extend(ind.iter().map(|i| base + i as u32));
        } else {
            self.idx.extend((0..p.len() as u32).map(|i| base + i));
        }
    }

    /// One flat quad. `right` and `up` are half-extent vectors; the face points along
    /// `right × up` (counter-clockwise winding is the front face).
    #[allow(clippy::too_many_arguments)]
    pub fn quad(
        &mut self,
        center: Vec3,
        right: Vec3,
        up: Vec3,
        color: [f32; 3],
        weight: f32,
        phase: f32,
        glow: f32,
        part: f32,
    ) {
        let n = right.cross(up).normalize_or_zero().to_array();
        let base = self.pos.len() as u32;
        for (sr, su) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = center + right * sr + up * su;
            self.pos.push(p.to_array());
            self.nrm.push(n);
            self.uv.push([weight, phase]);
            self.uv1.push([glow, part]);
            self.col.push([color[0], color[1], color[2], 1.0]);
        }
        self.idx
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Static axis-aligned (optionally yawed) box — the workhorse for arena dressing.
    pub fn block(&mut self, parts: &Parts, center: Vec3, size: Vec3, yaw: f32, color: [f32; 3]) {
        self.push(
            &parts.block,
            Transform {
                translation: center,
                rotation: Quat::from_rotation_y(yaw),
                scale: size,
            },
            color,
            0.0,
            0.0,
        );
    }

    /// Static box that also emits light (LED strip, EXIT sign, suite window).
    pub fn glow_block(
        &mut self,
        parts: &Parts,
        center: Vec3,
        size: Vec3,
        yaw: f32,
        color: [f32; 3],
        glow: f32,
    ) {
        self.push_ex(
            &parts.block,
            Transform {
                translation: center,
                rotation: Quat::from_rotation_y(yaw),
                scale: size,
            },
            color,
            0.0,
            0.0,
            glow,
            PART_LED,
        );
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn vertex_count(&self) -> usize {
        self.pos.len()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn index_count(&self) -> usize {
        self.idx.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    pub fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.uv1)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
        .with_inserted_indices(Indices::U32(self.idx))
    }
}

#[derive(Clone)]
pub struct CrowdStyle {
    pub shirts: Vec<[f32; 3]>,
    pub skins: Vec<[f32; 3]>,
    pub hairs: Vec<[f32; 3]>,
    pub pants: Vec<[f32; 3]>,
    pub seat: [f32; 3],
    /// Team colours used for scarves, signs and foam fingers: (primary, secondary).
    pub team: [[f32; 3]; 2],
    /// Chance a fan holds something up (phone / sign / foam finger).
    pub prop_chance: f32,
}

pub fn lin(c: Color) -> [f32; 3] {
    srgb(c)
}

fn srgb(c: Color) -> [f32; 3] {
    // Vertex colours are linear; StandardMaterial multiplies them straight into base colour.
    let l = c.to_linear();
    [l.red, l.green, l.blue]
}

fn mul3(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

impl CrowdStyle {
    pub fn arena(home: Color, away: Color, accent: Color, seat: Color) -> Self {
        Self::section(home, away, accent, seat, 0.5)
    }

    /// A stand section whose jerseys lean towards the home team (`home_bias` → 1) or the
    /// visitors (`home_bias` → 0). Civilian clothes fill the rest.
    pub fn section(home: Color, away: Color, accent: Color, seat: Color, home_bias: f32) -> Self {
        let h = srgb(home);
        let a = srgb(away);
        let mut shirts = Vec::with_capacity(20);
        let n_home = (home_bias * 8.0).round() as usize;
        let n_away = 8 - n_home;
        for _ in 0..n_home {
            shirts.push(h);
        }
        for _ in 0..n_away {
            shirts.push(a);
        }
        shirts.extend([
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
            srgb(Color::srgb(0.25, 0.4, 0.75)),
            srgb(Color::srgb(0.9, 0.55, 0.3)),
        ]);
        let team = if home_bias >= 0.5 {
            [h, srgb(Color::srgb(0.95, 0.95, 0.97))]
        } else {
            [a, srgb(Color::srgb(0.95, 0.95, 0.97))]
        };
        Self {
            shirts,
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
                srgb(Color::srgb(0.2, 0.5, 0.9)),
            ],
            pants: vec![
                srgb(Color::srgb(0.12, 0.14, 0.25)),
                srgb(Color::srgb(0.08, 0.08, 0.1)),
                srgb(Color::srgb(0.3, 0.3, 0.32)),
                srgb(Color::srgb(0.45, 0.4, 0.35)),
                srgb(Color::srgb(0.2, 0.22, 0.3)),
            ],
            seat: srgb(seat),
            team,
            prop_chance: 0.22,
        }
    }

    pub fn with_shirts(mut self, shirts: Vec<[f32; 3]>) -> Self {
        self.shirts = shirts;
        self
    }

    pub fn with_pants(mut self, pants: Vec<[f32; 3]>) -> Self {
        self.pants = pants;
        self
    }

    pub fn with_props(mut self, chance: f32) -> Self {
        self.prop_chance = chance;
        self
    }
}

/// Options for [`fan_with`].
#[derive(Clone, Copy)]
pub struct FanOpts {
    pub standing: bool,
    /// Allow this seat to hold a child (smaller, no props except foam fingers).
    pub kids: bool,
    /// Allow phones / signs / foam fingers / scarves.
    pub props: bool,
    /// Force a cap (ushers, staff).
    pub cap: bool,
    /// Cheer squad routine (pom-poms, skirt, part id 7).
    pub cheer: bool,
}

impl FanOpts {
    pub const fn seated() -> Self {
        Self {
            standing: false,
            kids: true,
            props: true,
            cap: false,
            cheer: false,
        }
    }
    pub const fn standing() -> Self {
        Self {
            standing: true,
            kids: false,
            props: true,
            cap: false,
            cheer: false,
        }
    }
    pub const fn staff() -> Self {
        Self {
            standing: true,
            kids: false,
            props: false,
            cap: true,
            cheer: false,
        }
    }
    pub const fn cheer() -> Self {
        Self {
            standing: true,
            kids: false,
            props: false,
            cap: false,
            cheer: true,
        }
    }
}

/// One stadium seat at `origin` (tier floor, seat centreline), facing local +z.
pub fn seat(batch: &mut Batch, parts: &Parts, origin: Vec3, yaw: f32, style: &CrowdStyle) {
    let rot = Quat::from_rotation_y(yaw);
    let place = |local: Vec3| origin + rot * local;
    let tint = mul3(style.seat, 0.9);
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
    let opts = if standing {
        FanOpts::standing()
    } else {
        FanOpts::seated()
    };
    fan_with(batch, parts, rng, origin, yaw, style, opts);
}

#[derive(Clone, Copy, PartialEq)]
enum Prop {
    None,
    Phone,
    Sign,
    Foam,
}

#[derive(Clone, Copy, PartialEq)]
enum Hair {
    Short,
    Cap,
    Beanie,
    Ponytail,
    Bun,
    Afro,
    Bald,
    Long,
}

/// Full-variety fan: body size, kids, hats, glasses, beards, hairstyles, scarves,
/// phones, signs, foam fingers and cheer routines.
pub fn fan_with(
    batch: &mut Batch,
    parts: &Parts,
    rng: &mut Lcg,
    origin: Vec3,
    yaw: f32,
    style: &CrowdStyle,
    opts: FanOpts,
) {
    let rot = Quat::from_rotation_y(yaw);
    let place = |local: Vec3| origin + rot * local;
    let phase = rng.next();
    let standing = opts.standing;
    let kid = opts.kids && !standing && rng.chance(0.12);
    let shirt = if opts.cheer {
        style.team[0]
    } else {
        *rng.pick(&style.shirts)
    };
    let skin = *rng.pick(&style.skins);
    let hair_col = *rng.pick(&style.hairs);
    let pants = if opts.cheer {
        style.team[1]
    } else {
        *rng.pick(&style.pants)
    };
    let size = if kid {
        0.62 + rng.next() * 0.1
    } else {
        0.9 + rng.next() * 0.2
    };
    let bulk = if kid { 0.8 } else { 0.85 + rng.next() * 0.35 };
    let body_part = if opts.cheer { PART_CHEER } else { PART_BODY };
    let head_part = if opts.cheer { PART_CHEER } else { PART_HEAD };

    let prop = if opts.props && rng.chance(style.prop_chance) {
        let r = rng.next();
        if kid {
            Prop::Foam
        } else if r < 0.45 {
            Prop::Phone
        } else if r < 0.7 {
            Prop::Sign
        } else {
            Prop::Foam
        }
    } else {
        Prop::None
    };
    // Standing fans celebrate with both arms up; staff (ushers, camera crews) keep
    // theirs down.
    let arms_up = opts.cheer || (standing && !opts.cap) || prop == Prop::Sign || rng.chance(0.16);
    let hair = if opts.cap {
        Hair::Cap
    } else if kid {
        *rng.pick(&[Hair::Short, Hair::Cap, Hair::Ponytail, Hair::Short])
    } else {
        let r = rng.next();
        if r < 0.36 {
            Hair::Short
        } else if r < 0.5 {
            Hair::Cap
        } else if r < 0.58 {
            Hair::Beanie
        } else if r < 0.7 {
            Hair::Ponytail
        } else if r < 0.78 {
            Hair::Bun
        } else if r < 0.86 {
            Hair::Afro
        } else if r < 0.92 {
            Hair::Bald
        } else {
            Hair::Long
        }
    };
    let glasses = !kid && rng.chance(0.14);
    let beard = !kid && !opts.cheer && rng.chance(0.12);
    let scarf = opts.props && !kid && rng.chance(0.12);

    let (torso_y, torso_len, head_y) = if standing {
        (1.05 * size, 0.42 * size, 1.52 * size)
    } else {
        (0.8 * size, 0.3 * size, 1.2 * size)
    };
    let lean = if standing { 0.0 } else { -0.1 };

    // torso
    batch.push_ex(
        &parts.torso,
        Transform {
            translation: place(Vec3::new(0.0, torso_y, -0.04)),
            rotation: rot * Quat::from_rotation_x(lean),
            scale: Vec3::new(0.19 * bulk, torso_len * 0.5, 0.15 * bulk),
        },
        shirt,
        0.55,
        phase,
        0.0,
        body_part,
    );
    // shoulders as a flattened capsule across the top of the torso
    batch.push_ex(
        &parts.limb,
        Transform {
            translation: place(Vec3::new(0.0, torso_y + torso_len * 0.45, -0.05)),
            rotation: rot * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            scale: Vec3::new(0.09, 0.17 * bulk, 0.09),
        },
        shirt,
        0.7,
        phase,
        0.0,
        body_part,
    );
    if opts.cheer {
        // skirt
        batch.push_ex(
            &parts.block,
            Transform {
                translation: place(Vec3::new(0.0, torso_y - torso_len * 0.55, -0.02)),
                rotation: rot,
                scale: Vec3::new(0.34, 0.16, 0.26),
            },
            pants,
            0.35,
            phase,
            0.0,
            PART_CHEER,
        );
    }
    // neck + head
    batch.push_ex(
        &parts.limb,
        Transform {
            translation: place(Vec3::new(0.0, head_y - 0.14 * size, -0.03)),
            rotation: rot,
            scale: Vec3::new(0.045, 0.04, 0.045),
        },
        skin,
        0.9,
        phase,
        0.0,
        head_part,
    );
    if scarf {
        let c = if rng.chance(0.5) {
            style.team[0]
        } else {
            style.team[1]
        };
        batch.push_ex(
            &parts.block,
            Transform {
                translation: place(Vec3::new(0.0, head_y - 0.15 * size, -0.03)),
                rotation: rot,
                scale: Vec3::new(0.27, 0.06, 0.2),
            },
            c,
            0.85,
            phase,
            0.0,
            body_part,
        );
    }
    let head_r = 0.115 * if kid { 0.9 } else { 1.0 };
    batch.push_ex(
        &parts.head,
        Transform {
            translation: place(Vec3::new(0.0, head_y, -0.02)),
            rotation: rot,
            scale: Vec3::splat(head_r),
        },
        skin,
        1.0,
        phase,
        0.0,
        head_part,
    );
    // hair / hat
    let team_or_shirt = if rng.chance(0.6) {
        style.team[0]
    } else {
        *rng.pick(&style.shirts)
    };
    match hair {
        Hair::Bald => {}
        Hair::Short => batch.push_ex(
            &parts.hair,
            Transform {
                translation: place(Vec3::new(0.0, head_y + 0.035, -0.055)),
                rotation: rot,
                scale: Vec3::new(head_r * 1.06, head_r * 0.92, head_r * 1.05),
            },
            hair_col,
            1.0,
            phase,
            0.0,
            head_part,
        ),
        Hair::Long => {
            batch.push_ex(
                &parts.hair,
                Transform {
                    translation: place(Vec3::new(0.0, head_y + 0.02, -0.06)),
                    rotation: rot,
                    scale: Vec3::new(head_r * 1.12, head_r * 1.1, head_r * 1.1),
                },
                hair_col,
                1.0,
                phase,
                0.0,
                head_part,
            );
            batch.push_ex(
                &parts.block,
                Transform {
                    translation: place(Vec3::new(0.0, head_y - 0.12, -0.11)),
                    rotation: rot,
                    scale: Vec3::new(0.2, 0.22, 0.06),
                },
                hair_col,
                0.95,
                phase,
                0.0,
                head_part,
            );
        }
        Hair::Afro => batch.push_ex(
            &parts.hair,
            Transform {
                translation: place(Vec3::new(0.0, head_y + 0.05, -0.04)),
                rotation: rot,
                scale: Vec3::splat(head_r * 1.45),
            },
            hair_col,
            1.0,
            phase,
            0.0,
            head_part,
        ),
        Hair::Ponytail => {
            batch.push_ex(
                &parts.hair,
                Transform {
                    translation: place(Vec3::new(0.0, head_y + 0.035, -0.055)),
                    rotation: rot,
                    scale: Vec3::new(head_r * 1.06, head_r * 0.92, head_r * 1.05),
                },
                hair_col,
                1.0,
                phase,
                0.0,
                head_part,
            );
            batch.push_ex(
                &parts.limb,
                Transform {
                    translation: place(Vec3::new(0.0, head_y - 0.06, -0.15)),
                    rotation: rot * Quat::from_rotation_x(-0.3),
                    scale: Vec3::new(0.03, 0.08, 0.03),
                },
                hair_col,
                1.0,
                phase,
                0.0,
                head_part,
            );
        }
        Hair::Bun => {
            batch.push_ex(
                &parts.hair,
                Transform {
                    translation: place(Vec3::new(0.0, head_y + 0.035, -0.055)),
                    rotation: rot,
                    scale: Vec3::new(head_r * 1.06, head_r * 0.92, head_r * 1.05),
                },
                hair_col,
                1.0,
                phase,
                0.0,
                head_part,
            );
            batch.push_ex(
                &parts.hair,
                Transform {
                    translation: place(Vec3::new(0.0, head_y + 0.12, -0.08)),
                    rotation: rot,
                    scale: Vec3::splat(0.05),
                },
                hair_col,
                1.0,
                phase,
                0.0,
                head_part,
            );
        }
        Hair::Cap => {
            batch.push_ex(
                &parts.hair,
                Transform {
                    translation: place(Vec3::new(0.0, head_y + 0.045, -0.02)),
                    rotation: rot,
                    scale: Vec3::new(head_r * 1.1, head_r * 0.8, head_r * 1.1),
                },
                team_or_shirt,
                1.0,
                phase,
                0.0,
                head_part,
            );
            // brim
            batch.push_ex(
                &parts.block,
                Transform {
                    translation: place(Vec3::new(0.0, head_y + 0.03, 0.13)),
                    rotation: rot,
                    scale: Vec3::new(0.2, 0.02, 0.14),
                },
                team_or_shirt,
                1.0,
                phase,
                0.0,
                head_part,
            );
        }
        Hair::Beanie => batch.push_ex(
            &parts.hair,
            Transform {
                translation: place(Vec3::new(0.0, head_y + 0.06, -0.03)),
                rotation: rot,
                scale: Vec3::new(head_r * 1.12, head_r * 1.05, head_r * 1.12),
            },
            team_or_shirt,
            1.0,
            phase,
            0.0,
            head_part,
        ),
    }
    if glasses {
        batch.push_ex(
            &parts.block,
            Transform {
                translation: place(Vec3::new(0.0, head_y + 0.01, head_r * 0.9)),
                rotation: rot,
                scale: Vec3::new(0.2, 0.035, 0.03),
            },
            [0.02, 0.02, 0.025],
            1.0,
            phase,
            0.0,
            head_part,
        );
    }
    if beard {
        batch.push_ex(
            &parts.hair,
            Transform {
                translation: place(Vec3::new(0.0, head_y - 0.07, head_r * 0.55)),
                rotation: rot,
                scale: Vec3::new(0.085, 0.06, 0.07),
            },
            hair_col,
            1.0,
            phase,
            0.0,
            head_part,
        );
    }

    // arms — which hand holds the prop (phones and foam fingers are one-handed).
    let prop_hand = if rng.chance(0.5) { -1.0 } else { 1.0 };
    for sx in [-1.0f32, 1.0] {
        let this_up = arms_up || (prop != Prop::None && sx == prop_hand);
        let (center, rotation, len) = if this_up {
            (
                Vec3::new(sx * 0.24 * bulk, torso_y + torso_len * 0.5 + 0.2 * size, -0.02),
                rot * Quat::from_rotation_z(-sx * 0.25),
                0.16 * size,
            )
        } else {
            (
                Vec3::new(sx * 0.22 * bulk, torso_y - 0.02, 0.1),
                rot * Quat::from_rotation_x(0.95) * Quat::from_rotation_z(-sx * 0.08),
                0.15 * size,
            )
        };
        let part = if opts.cheer {
            PART_CHEER
        } else if this_up {
            PART_BODY
        } else {
            PART_ARM
        };
        batch.push_ex(
            &parts.limb,
            Transform {
                translation: place(center),
                rotation,
                scale: Vec3::new(0.055, len, 0.055),
            },
            shirt,
            if this_up { 1.0 } else { 0.6 },
            phase,
            0.0,
            part,
        );
        // hand
        let hand = if this_up {
            center + Vec3::new(sx * 0.04, len + 0.03, 0.0)
        } else {
            center + Vec3::new(0.0, -0.1, 0.12)
        };
        batch.push_ex(
            &parts.hair,
            Transform {
                translation: place(hand),
                rotation: rot,
                scale: Vec3::splat(0.045),
            },
            skin,
            if this_up { 1.0 } else { 0.6 },
            phase,
            0.0,
            part,
        );
        if opts.cheer {
            // pom-pom
            batch.push_ex(
                &parts.head,
                Transform {
                    translation: place(hand + Vec3::new(sx * 0.02, 0.07, 0.0)),
                    rotation: rot,
                    scale: Vec3::splat(0.09),
                },
                if sx > 0.0 { style.team[1] } else { style.team[0] },
                1.0,
                phase,
                0.0,
                PART_CHEER,
            );
        } else if sx == prop_hand {
            match prop {
                Prop::Phone => {
                    let p = hand + Vec3::new(0.0, 0.07, 0.01);
                    batch.push_ex(
                        &parts.block,
                        Transform {
                            translation: place(p),
                            rotation: rot,
                            scale: Vec3::new(0.06, 0.11, 0.012),
                        },
                        [0.03, 0.03, 0.035],
                        1.0,
                        phase,
                        0.0,
                        PART_PHONE,
                    );
                    // screen facing the court (local +z)
                    batch.quad(
                        place(p + Vec3::new(0.0, 0.0, 0.008)),
                        rot * Vec3::new(0.026, 0.0, 0.0),
                        rot * Vec3::new(0.0, 0.05, 0.0),
                        [0.75, 0.85, 1.0],
                        1.0,
                        phase,
                        1.0,
                        PART_PHONE,
                    );
                }
                Prop::Foam => {
                    let c = style.team[0];
                    let p = hand + Vec3::new(0.0, 0.14, 0.0);
                    batch.push_ex(
                        &parts.block,
                        Transform {
                            translation: place(p),
                            rotation: rot,
                            scale: Vec3::new(0.1, 0.2, 0.05),
                        },
                        c,
                        1.0,
                        phase,
                        0.0,
                        PART_BODY,
                    );
                    batch.push_ex(
                        &parts.block,
                        Transform {
                            translation: place(p + Vec3::new(sx * 0.01, 0.15, 0.0)),
                            rotation: rot,
                            scale: Vec3::new(0.045, 0.12, 0.05),
                        },
                        c,
                        1.0,
                        phase,
                        0.0,
                        PART_BODY,
                    );
                }
                _ => {}
            }
        }
    }
    if prop == Prop::Sign {
        // Sign held over the head with both hands: team-colour board with contrasting
        // stripes standing in for letters. Faces the court (local +z).
        let bg = style.team[0];
        let fg = style.team[1];
        let top = head_y + 0.16 * size;
        let sign_c = Vec3::new(0.0, top + 0.2, 0.02);
        batch.push_ex(
            &parts.block,
            Transform {
                translation: place(sign_c),
                rotation: rot,
                scale: Vec3::new(0.56, 0.36, 0.02),
            },
            bg,
            1.0,
            phase,
            0.08,
            PART_SIGN,
        );
        for (dy, w) in [(0.08, 0.42), (-0.04, 0.3), (-0.12, 0.38)] {
            batch.quad(
                place(sign_c + Vec3::new(0.0, dy, 0.012)),
                rot * Vec3::new(w * 0.5, 0.0, 0.0),
                rot * Vec3::new(0.0, 0.028, 0.0),
                fg,
                1.0,
                phase,
                0.06,
                PART_SIGN,
            );
        }
        // stick
        batch.push_ex(
            &parts.limb,
            Transform {
                translation: place(Vec3::new(0.0, top + 0.02, 0.01)),
                rotation: rot,
                scale: Vec3::new(0.015, 0.09, 0.015),
            },
            [0.6, 0.5, 0.35],
            1.0,
            phase,
            0.0,
            PART_SIGN,
        );
    }
    // legs
    for sx in [-1.0f32, 1.0] {
        if standing {
            batch.push_ex(
                &parts.limb,
                Transform {
                    translation: place(Vec3::new(sx * 0.1, 0.42 * size, 0.0)),
                    rotation: rot,
                    scale: Vec3::new(0.07, 0.36 * size, 0.07),
                },
                pants,
                0.15,
                phase,
                0.0,
                body_part,
            );
        } else {
            // thigh forward, shin down
            batch.push_ex(
                &parts.limb,
                Transform {
                    translation: place(Vec3::new(sx * 0.1, 0.5 * size, 0.2 * size)),
                    rotation: rot * Quat::from_rotation_x(1.45),
                    scale: Vec3::new(0.07, 0.17 * size, 0.07),
                },
                pants,
                0.25,
                phase,
                0.0,
                body_part,
            );
            batch.push_ex(
                &parts.limb,
                Transform {
                    translation: place(Vec3::new(sx * 0.1, 0.22 * size, 0.38 * size)),
                    rotation: rot,
                    scale: Vec3::new(0.06, 0.18 * size, 0.06),
                },
                pants,
                0.0,
                phase,
                0.0,
                body_part,
            );
        }
    }
}

/// Upper-deck impostor: two camera-agnostic quads (body + head) facing local +z.
/// Eight vertices per fan, so a whole upper bowl costs less than one lower-bowl section.
pub fn impostor(batch: &mut Batch, rng: &mut Lcg, origin: Vec3, yaw: f32, style: &CrowdStyle) {
    let rot = Quat::from_rotation_y(yaw);
    let phase = rng.next();
    let shirt = *rng.pick(&style.shirts);
    let skin = *rng.pick(&style.skins);
    let hair = *rng.pick(&style.hairs);
    let h = 0.9 + rng.next() * 0.2;
    let right = rot * Vec3::X;
    let body_c = origin + Vec3::Y * 0.62 * h;
    batch.quad(
        body_c,
        right * 0.21,
        Vec3::Y * 0.3 * h,
        shirt,
        0.5,
        phase,
        0.0,
        PART_IMPOSTOR,
    );
    let head_c = origin + Vec3::Y * 1.07 * h + rot * Vec3::new(0.0, 0.0, -0.01);
    batch.quad(
        head_c,
        right * 0.1,
        Vec3::Y * 0.115,
        skin,
        1.0,
        phase,
        0.0,
        PART_IMPOSTOR,
    );
    if rng.chance(0.85) {
        batch.quad(
            head_c + Vec3::Y * 0.09 + rot * Vec3::new(0.0, 0.0, -0.005),
            right * 0.105,
            Vec3::Y * 0.04,
            hair,
            1.0,
            phase,
            0.0,
            PART_IMPOSTOR,
        );
    }
}

/// Team mascot: giant head, jersey, big shoes, raised arms. Built at the origin so the
/// caller can bounce/spin the whole mesh with a `Transform`.
pub fn mascot(batch: &mut Batch, parts: &Parts, primary: [f32; 3], secondary: [f32; 3], fur: [f32; 3]) {
    let id = Quat::IDENTITY;
    // shoes
    for sx in [-1.0f32, 1.0] {
        batch.push(
            &parts.block,
            Transform {
                translation: Vec3::new(sx * 0.22, 0.09, 0.08),
                rotation: id,
                scale: Vec3::new(0.26, 0.18, 0.46),
            },
            secondary,
            0.0,
            0.0,
        );
        batch.push(
            &parts.limb,
            Transform {
                translation: Vec3::new(sx * 0.2, 0.5, 0.0),
                rotation: id,
                scale: Vec3::new(0.12, 0.28, 0.12),
            },
            fur,
            0.0,
            0.0,
        );
    }
    // body + jersey
    batch.push(
        &parts.torso,
        Transform {
            translation: Vec3::new(0.0, 1.12, 0.0),
            rotation: id,
            scale: Vec3::new(0.42, 0.3, 0.36),
        },
        primary,
        0.0,
        0.0,
    );
    batch.push(
        &parts.block,
        Transform {
            translation: Vec3::new(0.0, 1.0, 0.0),
            rotation: id,
            scale: Vec3::new(0.7, 0.3, 0.6),
        },
        secondary,
        0.0,
        0.0,
    );
    // jersey number stripe
    batch.push(
        &parts.block,
        Transform {
            translation: Vec3::new(0.0, 1.22, 0.36),
            rotation: id,
            scale: Vec3::new(0.34, 0.12, 0.03),
        },
        [0.97, 0.97, 0.97],
        0.0,
        0.0,
    );
    // arms up, huge mitts
    for sx in [-1.0f32, 1.0] {
        batch.push(
            &parts.limb,
            Transform {
                translation: Vec3::new(sx * 0.55, 1.55, 0.0),
                rotation: Quat::from_rotation_z(-sx * 0.55),
                scale: Vec3::new(0.11, 0.3, 0.11),
            },
            fur,
            0.0,
            0.0,
        );
        batch.push(
            &parts.head,
            Transform {
                translation: Vec3::new(sx * 0.74, 1.92, 0.0),
                rotation: id,
                scale: Vec3::splat(0.16),
            },
            secondary,
            0.0,
            0.0,
        );
    }
    // giant head
    batch.push(
        &parts.head,
        Transform {
            translation: Vec3::new(0.0, 2.05, 0.0),
            rotation: id,
            scale: Vec3::new(0.5, 0.46, 0.48),
        },
        fur,
        0.0,
        0.0,
    );
    // muzzle
    batch.push(
        &parts.head,
        Transform {
            translation: Vec3::new(0.0, 1.9, 0.36),
            rotation: id,
            scale: Vec3::new(0.26, 0.18, 0.22),
        },
        [0.96, 0.92, 0.85],
        0.0,
        0.0,
    );
    // eyes
    for sx in [-1.0f32, 1.0] {
        batch.push(
            &parts.head,
            Transform {
                translation: Vec3::new(sx * 0.19, 2.14, 0.38),
                rotation: id,
                scale: Vec3::splat(0.11),
            },
            [1.0, 1.0, 1.0],
            0.0,
            0.0,
        );
        batch.push(
            &parts.hair,
            Transform {
                translation: Vec3::new(sx * 0.2, 2.14, 0.47),
                rotation: id,
                scale: Vec3::splat(0.05),
            },
            [0.02, 0.02, 0.03],
            0.0,
            0.0,
        );
        // ears
        batch.push(
            &parts.limb,
            Transform {
                translation: Vec3::new(sx * 0.32, 2.5, -0.05),
                rotation: Quat::from_rotation_z(-sx * 0.35),
                scale: Vec3::new(0.09, 0.2, 0.05),
            },
            fur,
            0.0,
            0.0,
        );
    }
    // headband in primary
    batch.push(
        &parts.disc,
        Transform {
            translation: Vec3::new(0.0, 2.3, 0.0),
            rotation: id,
            scale: Vec3::new(0.46, 0.08, 0.44),
        },
        primary,
        0.0,
        0.0,
    );
}

fn drive_crowd(
    time: Res<Time>,
    fx: Res<CameraPostFx>,
    state: Res<State<AppState>>,
    heats: Query<&crate::units::Heat>,
    mut hype: ResMut<CrowdHype>,
    mut mats: ResMut<Assets<CrowdMaterial>>,
) {
    let in_match = *state.get() == AppState::Playing;
    let target = if in_match {
        fx.crowd_flash.clamp(0.0, 1.0)
    } else {
        // A little life in the menu orbit.
        0.1
    };
    let fire = in_match && heats.iter().any(|h| h.on_fire());
    let dt = time.delta_secs().min(0.1);
    // Cheap deterministic jitter for wave rest periods.
    let rand = (time.elapsed_secs() * 7.31).sin().abs();
    hype.tick(dt, target, fire, in_match, rand);
    let menu_glow = if in_match { 0.0 } else { 0.45 };
    let (p, p2) = hype.params(time.elapsed_secs(), menu_glow);
    for (_, mat) in mats.iter_mut() {
        mat.extension.params = p;
        mat.extension.params2 = p2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> CrowdStyle {
        CrowdStyle::section(
            Color::srgb(0.1, 0.8, 0.9),
            Color::srgb(0.6, 0.2, 1.0),
            Color::srgb(1.0, 0.9, 0.2),
            Color::srgb(0.1, 0.1, 0.2),
            0.75,
        )
    }

    #[test]
    fn batch_quad_is_two_triangles() {
        let mut b = Batch::default();
        b.quad(Vec3::ZERO, Vec3::X, Vec3::Y, [1.0, 1.0, 1.0], 0.0, 0.0, 0.0, PART_BODY);
        assert_eq!(b.vertex_count(), 4);
        assert_eq!(b.index_count(), 6);
        let mesh = b.build();
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_some());
        assert_eq!(mesh.count_vertices(), 4);
    }

    #[test]
    fn impostor_is_cheap() {
        let mut b = Batch::default();
        let mut rng = Lcg(1);
        for i in 0..100 {
            impostor(&mut b, &mut rng, Vec3::new(i as f32, 0.0, 0.0), 0.0, &style());
        }
        // 2 or 3 quads per fan, never more.
        assert!(b.vertex_count() <= 100 * 12);
        assert!(b.vertex_count() >= 100 * 8);
    }

    #[test]
    fn fans_are_deterministic_and_bounded() {
        let parts = Parts::new();
        let s = style();
        let build = || {
            let mut b = Batch::default();
            let mut rng = Lcg(42);
            for i in 0..200 {
                fan(&mut b, &parts, &mut rng, Vec3::new(i as f32 * 0.6, 0.0, 0.0), 0.0, &s, i % 9 == 0);
            }
            b
        };
        let a = build();
        let b = build();
        assert_eq!(a.vertex_count(), b.vertex_count());
        // Full-variety fans stay well under 1k vertices each on average.
        let avg = a.vertex_count() / 200;
        println!("average vertices per fan: {avg}");
        assert!(avg < 900, "avg {avg}");
        assert_eq!(a.index_count() % 3, 0);
    }

    #[test]
    fn section_bias_changes_jersey_mix() {
        let home = Color::srgb(0.1, 0.8, 0.9);
        let away = Color::srgb(0.6, 0.2, 1.0);
        let h = lin(home);
        let a = lin(away);
        let count = |st: &CrowdStyle, c: [f32; 3]| st.shirts.iter().filter(|s| **s == c).count();
        let home_side = CrowdStyle::section(home, away, Color::WHITE, Color::BLACK, 0.85);
        let away_side = CrowdStyle::section(home, away, Color::WHITE, Color::BLACK, 0.15);
        assert!(count(&home_side, h) > count(&home_side, a));
        assert!(count(&away_side, a) > count(&away_side, h));
        assert_eq!(home_side.team[0], h);
        assert_eq!(away_side.team[0], a);
    }

    #[test]
    fn hype_wave_travels_and_rests() {
        let mut hype = CrowdHype::default();
        hype.wave_timer = 0.0;
        hype.tick(0.016, 0.0, false, true, 0.5);
        assert!(hype.wave_strength > 0.0);
        let mut t = 0.0;
        while hype.wave_strength > 0.0 && t < 30.0 {
            hype.tick(0.05, 0.0, false, true, 0.5);
            t += 0.05;
        }
        assert!(t < 12.0, "wave should complete a lap in under 12 s, took {t}");
        assert!(hype.wave_timer > 10.0);
    }

    #[test]
    fn fire_makes_crowd_stand() {
        let mut hype = CrowdHype::default();
        for _ in 0..200 {
            hype.tick(0.05, 0.2, true, true, 0.3);
        }
        assert!(hype.fire > 0.95);
        assert!(hype.stand > 0.7);
        let (p, p2) = hype.params(1.0, 0.0);
        assert_eq!(p.x, 1.0);
        assert!(p.w > 0.95);
        assert!(p2.w > 0.5);
    }

    #[test]
    fn mascot_builds() {
        let mut b = Batch::default();
        mascot(&mut b, &Parts::new(), [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.5, 0.2]);
        assert!(b.vertex_count() > 100 && b.vertex_count() < 3000);
    }
}
