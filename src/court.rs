use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::arenas::{ArenaId, ArenaTheme};
use crate::camera::CameraPostFx;
use crate::courtpaint::{paint_court, PLANE_HALF_LEN, PLANE_HALF_WID};
use crate::sim::{COURT_HALF_LEN, COURT_HALF_WID, HOOP_X, RIM_HEIGHT, RIM_RADIUS};
use crate::states::{AppState, MatchConfig};

/// Texels per meter for the painted hardwood. 64 → 1997x1165 RGBA (~9 MB), fine for WebGL2.
const COURT_PX_PER_M: u32 = 64;

/// WebGL2 has no storage buffers, so every fan is close to one draw call. Thin the
/// crowd there and skip the head spheres; native keeps the full house.
#[cfg(target_arch = "wasm32")]
const FAN_SPACING: f32 = 0.92;
#[cfg(not(target_arch = "wasm32"))]
const FAN_SPACING: f32 = 0.62;
#[cfg(target_arch = "wasm32")]
const FAN_HEADS: bool = false;
#[cfg(not(target_arch = "wasm32"))]
const FAN_HEADS: bool = true;

pub struct CourtPlugin;

impl Plugin for CourtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CourtTextures>()
            .add_systems(
                OnEnter(AppState::Playing),
                (cleanup_arenas, spawn_arena).chain(),
            )
            .add_systems(OnEnter(AppState::Splash), spawn_menu_arena)
            .add_systems(OnEnter(AppState::MainMenu), spawn_menu_arena)
            .add_systems(OnEnter(AppState::CharacterSelect), spawn_menu_arena)
            .add_systems(OnEnter(AppState::CourtSelect), spawn_menu_arena)
            .add_systems(
                Update,
                spin_holo.run_if(
                    in_state(AppState::MainMenu)
                        .or(in_state(AppState::CharacterSelect))
                        .or(in_state(AppState::CourtSelect)),
                ),
            )
            .add_systems(Update, spin_holo.run_if(in_state(AppState::Playing)))
            .add_systems(
                Update,
                (pulse_nets, animate_crowd).run_if(in_state(AppState::Playing)),
            );
    }
}

#[derive(Component)]
pub struct ArenaRoot;

#[derive(Component)]
pub struct Hoop {
    pub home_side: bool,
}

#[derive(Component)]
struct HoloSpin;

#[derive(Component)]
pub struct RimMarker {
    pub home_side: bool,
}

#[derive(Component)]
pub struct NetRipple {
    pub rest_scale: Vec3,
    pub pulse: f32,
}

#[derive(Component)]
struct CrowdFan {
    phase: f32,
    base_y: f32,
    speed: f32,
}

#[derive(Resource, Default)]
struct CourtTextures {
    by_arena: HashMap<ArenaId, Handle<Image>>,
}

fn cleanup_arenas(mut commands: Commands, q: Query<Entity, With<ArenaRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

pub fn spawn_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<CourtTextures>,
    config: Res<MatchConfig>,
) {
    let theme = config.arena.theme();
    let floor = court_texture(&mut images, &mut cache, &theme);
    build_arena(
        &mut commands,
        &mut meshes,
        &mut materials,
        &theme,
        floor,
        true,
    );
}

fn spawn_menu_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<CourtTextures>,
    config: Res<MatchConfig>,
    existing: Query<Entity, With<ArenaRoot>>,
) {
    if !existing.is_empty() {
        return;
    }
    let theme = config.arena.theme();
    let floor = court_texture(&mut images, &mut cache, &theme);
    build_arena(
        &mut commands,
        &mut meshes,
        &mut materials,
        &theme,
        floor,
        true,
    );
}

fn court_texture(
    images: &mut Assets<Image>,
    cache: &mut CourtTextures,
    theme: &ArenaTheme,
) -> Handle<Image> {
    if let Some(h) = cache.by_arena.get(&theme.id) {
        return h.clone();
    }
    let painted = paint_court(COURT_PX_PER_M, &theme.palette());
    let mut image = Image::new(
        Extent3d {
            width: painted.width,
            height: painted.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        painted.rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
    let handle = images.add(image);
    cache.by_arena.insert(theme.id, handle.clone());
    handle
}

fn build_arena(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    theme: &ArenaTheme,
    floor_tex: Handle<Image>,
    full_crowd: bool,
) {
    commands.insert_resource(GlobalAmbientLight {
        color: theme.ambient,
        brightness: 140.0,
        ..default()
    });
    commands.insert_resource(ClearColor(theme.sky));

    let accent_lin = LinearRgba::from(theme.accent.to_linear());

    let floor_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(floor_tex),
        perceptual_roughness: 0.28,
        metallic: 0.02,
        reflectance: 0.55,
        ..default()
    });
    let slab_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.03, 0.05),
        perceptual_roughness: 0.9,
        ..default()
    });
    let ribbon_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.04),
        emissive: accent_lin * 1.6,
        perceptual_roughness: 0.3,
        ..default()
    });
    let ribbon_dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.05, 0.08),
        emissive: theme.emissive * 0.12,
        ..default()
    });
    let riser_mat = materials.add(StandardMaterial {
        base_color: theme.crowd,
        perceptual_roughness: 0.95,
        ..default()
    });
    let wall_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.035, 0.06),
        perceptual_roughness: 0.95,
        ..default()
    });
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgba(0.7, 0.85, 1.0, 0.18),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.08,
        metallic: 0.4,
        ..default()
    });
    let rim_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.35, 0.08),
        metallic: 0.8,
        perceptual_roughness: 0.25,
        emissive: LinearRgba::new(1.2, 0.25, 0.05, 1.0),
        ..default()
    });
    let board_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.95, 1.0),
        perceptual_roughness: 0.15,
        metallic: 0.05,
        ..default()
    });
    let net_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.95, 0.95, 1.0, 0.55),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.8,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let pole_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.16, 0.2),
        metallic: 0.7,
        perceptual_roughness: 0.3,
        ..default()
    });
    let neon = materials.add(StandardMaterial {
        base_color: theme.accent,
        emissive: accent_lin * 2.4,
        ..default()
    });
    let screen_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.03, 0.06),
        emissive: theme.emissive * 0.8,
        perceptual_roughness: 0.2,
        ..default()
    });
    let lightbank = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::new(6.0, 5.6, 5.0, 1.0),
        unlit: true,
        ..default()
    });

    // --- painted hardwood
    commands.spawn((
        ArenaRoot,
        Mesh3d(
            meshes.add(
                Plane3d::default()
                    .mesh()
                    .size(PLANE_HALF_LEN * 2.0, PLANE_HALF_WID * 2.0),
            ),
        ),
        MeshMaterial3d(floor_mat),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(
            PLANE_HALF_LEN * 2.0 + 0.6,
            0.3,
            PLANE_HALF_WID * 2.0 + 0.6,
        ))),
        MeshMaterial3d(slab_mat.clone()),
        Transform::from_xyz(0.0, -0.16, 0.0),
    ));

    // --- LED ribbon boards around the apron
    let ribbon_h = 0.95;
    for s in [-1.0, 1.0] {
        let z = s * (PLANE_HALF_WID + 0.12);
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(PLANE_HALF_LEN * 2.0, ribbon_h, 0.2))),
            MeshMaterial3d(ribbon_dark.clone()),
            Transform::from_xyz(0.0, ribbon_h * 0.5, z),
        ));
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(PLANE_HALF_LEN * 2.0 - 0.4, 0.34, 0.06))),
            MeshMaterial3d(ribbon_mat.clone()),
            Transform::from_xyz(0.0, ribbon_h * 0.55, z - s * 0.12),
        ));
        let x = s * (PLANE_HALF_LEN + 0.12);
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(0.2, ribbon_h, PLANE_HALF_WID * 2.0))),
            MeshMaterial3d(ribbon_dark.clone()),
            Transform::from_xyz(x, ribbon_h * 0.5, 0.0),
        ));
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(0.06, 0.34, PLANE_HALF_WID * 2.0 - 0.4))),
            MeshMaterial3d(ribbon_mat.clone()),
            Transform::from_xyz(x - s * 0.12, ribbon_h * 0.55, 0.0),
        ));
    }

    for sign in [-1.0, 1.0] {
        spawn_hoop(
            commands,
            meshes,
            sign * HOOP_X,
            sign < 0.0,
            &board_mat,
            &rim_mat,
            &net_mat,
            &pole_mat,
            &glass,
        );
    }

    // --- stands + crowd
    if full_crowd {
        spawn_stands(commands, meshes, materials, theme, &riser_mat, &wall_mat);
    }

    // --- center-hung jumbotron
    let cube_y = 12.5;
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(7.2, 4.0, 7.2))),
        MeshMaterial3d(slab_mat.clone()),
        Transform::from_xyz(0.0, cube_y, 0.0),
    ));
    for (dx, dz, rot) in [
        (0.0, 3.62, 0.0),
        (0.0, -3.62, std::f32::consts::PI),
        (3.62, 0.0, std::f32::consts::FRAC_PI_2),
        (-3.62, 0.0, -std::f32::consts::FRAC_PI_2),
    ] {
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(6.4, 3.2, 0.06))),
            MeshMaterial3d(screen_mat.clone()),
            Transform {
                translation: Vec3::new(dx, cube_y, dz),
                rotation: Quat::from_rotation_y(rot),
                ..default()
            },
        ));
    }
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(7.4, 0.25, 7.4))),
        MeshMaterial3d(neon.clone()),
        Transform::from_xyz(0.0, cube_y - 2.1, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        HoloSpin,
        Mesh3d(meshes.add(Torus {
            minor_radius: 0.12,
            major_radius: 2.6,
        })),
        MeshMaterial3d(neon.clone()),
        Transform::from_xyz(0.0, cube_y - 2.9, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cylinder::new(0.12, 6.0))),
        MeshMaterial3d(pole_mat.clone()),
        Transform::from_xyz(0.0, cube_y + 5.0, 0.0),
    ));

    // --- rigging: light banks + truss ring
    for (x, z) in [(-9.0, 5.5), (9.0, 5.5), (-9.0, -5.5), (9.0, -5.5)] {
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(2.4, 0.3, 1.0))),
            MeshMaterial3d(lightbank.clone()),
            Transform::from_xyz(x, 14.2, z),
        ));
        commands.spawn((
            ArenaRoot,
            SpotLight {
                intensity: 2_200_000.0,
                range: 40.0,
                radius: 0.4,
                color: Color::srgb(1.0, 0.96, 0.9),
                shadows_enabled: false, // WASM + llvmpipe cannot afford shadow maps
                inner_angle: 0.55,
                outer_angle: 1.05,
                ..default()
            },
            Transform::from_xyz(x, 14.0, z).looking_at(Vec3::new(x * 0.55, 0.0, z * 0.2), Vec3::Y),
        ));
    }
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Torus {
            minor_radius: 0.18,
            major_radius: 22.0,
        })),
        MeshMaterial3d(ribbon_mat.clone()),
        Transform::from_xyz(0.0, 15.5, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        DirectionalLight {
            illuminance: 3_800.0,
            shadows_enabled: false,
            color: Color::srgb(1.0, 0.97, 0.92),
            ..default()
        },
        Transform::from_xyz(6.0, 24.0, 14.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // Team-colour wash from the ends
    for (x, c) in [
        (-16.0, Color::srgb(0.2, 0.9, 1.0)),
        (16.0, Color::srgb(0.75, 0.35, 1.0)),
    ] {
        commands.spawn((
            ArenaRoot,
            PointLight {
                intensity: 220_000.0,
                range: 26.0,
                color: c,
                shadows_enabled: false,
                ..default()
            },
            Transform::from_xyz(x, 6.5, 0.0),
        ));
    }

    // Sky temple petals / toon extra
    if matches!(theme.id, ArenaId::SkyTemple) {
        let petal = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.7, 0.85),
            emissive: LinearRgba::new(0.8, 0.3, 0.5, 1.0),
            ..default()
        });
        let m = meshes.add(Cuboid::new(0.18, 0.04, 0.12));
        for i in 0..18 {
            let a = i as f32 * 0.7;
            commands.spawn((
                ArenaRoot,
                HoloSpin,
                Mesh3d(m.clone()),
                MeshMaterial3d(petal.clone()),
                Transform::from_xyz(a.sin() * 8.0, 1.5 + (i % 5) as f32 * 0.4, a.cos() * 6.0),
            ));
        }
    }
}

fn spawn_stands(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    theme: &ArenaTheme,
    riser_mat: &Handle<StandardMaterial>,
    wall_mat: &Handle<StandardMaterial>,
) {
    let fan_mesh = meshes.add(Capsule3d::new(0.19, 0.46));
    let head_mesh = meshes.add(Sphere::new(0.13));
    let riser = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    let home = crate::roster::Side::Home.primary();
    let away = crate::roster::Side::Away.primary();
    let shirts: Vec<Handle<StandardMaterial>> = [
        home,
        home,
        away,
        Color::srgb(0.95, 0.95, 0.98),
        Color::srgb(0.08, 0.08, 0.1),
        Color::srgb(0.9, 0.3, 0.25),
        Color::srgb(0.95, 0.8, 0.2),
        theme.accent,
    ]
    .into_iter()
    .map(|c| {
        materials.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: 0.85,
            ..default()
        })
    })
    .collect();
    let skins: Vec<Handle<StandardMaterial>> = [
        Color::srgb(0.98, 0.85, 0.72),
        Color::srgb(0.85, 0.62, 0.45),
        Color::srgb(0.55, 0.36, 0.25),
        Color::srgb(0.36, 0.22, 0.16),
    ]
    .into_iter()
    .map(|c| {
        materials.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: 0.8,
            ..default()
        })
    })
    .collect();

    let mut n: u32 = 17;
    let mut rnd = move || {
        n = n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (n >> 8) as f32 / 16_777_216.0
    };

    let tiers = 7;
    let tier_rise = 0.92;
    let tier_depth = 1.35;
    let first_y = 0.55;
    let side_start = PLANE_HALF_WID + 1.1;
    let end_start = PLANE_HALF_LEN + 1.6;

    for tier in 0..tiers {
        let y = first_y + tier as f32 * tier_rise;
        let off = tier as f32 * tier_depth;
        // long sides
        let side_len = (PLANE_HALF_LEN + 1.0 + off) * 2.0;
        for s in [-1.0, 1.0] {
            let z = s * (side_start + off);
            commands.spawn((
                ArenaRoot,
                Mesh3d(riser.clone()),
                MeshMaterial3d(riser_mat.clone()),
                Transform {
                    translation: Vec3::new(0.0, y * 0.5, z),
                    scale: Vec3::new(side_len, y, tier_depth),
                    ..default()
                },
            ));
            let mut x = -side_len * 0.5 + 0.4;
            while x < side_len * 0.5 - 0.4 {
                if rnd() > 0.08 {
                    let jx = (rnd() - 0.5) * 0.12;
                    let jz = (rnd() - 0.5) * 0.25;
                    spawn_fan(
                        commands,
                        &fan_mesh,
                        &head_mesh,
                        &shirts,
                        &skins,
                        &mut rnd,
                        Vec3::new(x + jx, y, z + jz),
                        -s,
                    );
                }
                x += FAN_SPACING;
            }
        }
        // ends
        let end_len = (PLANE_HALF_WID + 0.4 + off) * 2.0;
        for s in [-1.0, 1.0] {
            let x = s * (end_start + off);
            commands.spawn((
                ArenaRoot,
                Mesh3d(riser.clone()),
                MeshMaterial3d(riser_mat.clone()),
                Transform {
                    translation: Vec3::new(x, y * 0.5, 0.0),
                    scale: Vec3::new(tier_depth, y, end_len),
                    ..default()
                },
            ));
            let mut z = -end_len * 0.5 + 0.4;
            while z < end_len * 0.5 - 0.4 {
                if rnd() > 0.1 {
                    let jx = (rnd() - 0.5) * 0.25;
                    let jz = (rnd() - 0.5) * 0.12;
                    spawn_fan(
                        commands,
                        &fan_mesh,
                        &head_mesh,
                        &shirts,
                        &skins,
                        &mut rnd,
                        Vec3::new(x + jx, y, z + jz),
                        0.0,
                    );
                }
                z += FAN_SPACING * 1.06;
            }
        }
    }

    // Back walls + upper deck fascia
    let top_y = first_y + tiers as f32 * tier_rise;
    let far = tiers as f32 * tier_depth;
    for s in [-1.0, 1.0] {
        commands.spawn((
            ArenaRoot,
            Mesh3d(riser.clone()),
            MeshMaterial3d(wall_mat.clone()),
            Transform {
                translation: Vec3::new(0.0, top_y + 5.0, s * (side_start + far + 0.8)),
                scale: Vec3::new((PLANE_HALF_LEN + far + 4.0) * 2.0, 12.0, 0.6),
                ..default()
            },
        ));
        commands.spawn((
            ArenaRoot,
            Mesh3d(riser.clone()),
            MeshMaterial3d(wall_mat.clone()),
            Transform {
                translation: Vec3::new(s * (end_start + far + 0.8), top_y + 5.0, 0.0),
                scale: Vec3::new(0.6, 12.0, (PLANE_HALF_WID + far + 4.0) * 2.0),
                ..default()
            },
        ));
    }
}

fn spawn_fan(
    commands: &mut Commands,
    body: &Handle<Mesh>,
    head: &Handle<Mesh>,
    shirts: &[Handle<StandardMaterial>],
    skins: &[Handle<StandardMaterial>],
    rnd: &mut impl FnMut() -> f32,
    pos: Vec3,
    face_z: f32,
) {
    let shirt = shirts[(rnd() * shirts.len() as f32) as usize % shirts.len()].clone();
    let skin = skins[(rnd() * skins.len() as f32) as usize % skins.len()].clone();
    let h = 0.9 + rnd() * 0.25;
    let yaw = if face_z.abs() > 0.5 {
        if face_z > 0.0 {
            0.0
        } else {
            std::f32::consts::PI
        }
    } else if pos.x > 0.0 {
        -std::f32::consts::FRAC_PI_2
    } else {
        std::f32::consts::FRAC_PI_2
    };
    commands
        .spawn((
            ArenaRoot,
            CrowdFan {
                phase: rnd() * std::f32::consts::TAU,
                base_y: pos.y,
                speed: 1.6 + rnd() * 1.4,
            },
            Mesh3d(body.clone()),
            MeshMaterial3d(shirt),
            Transform {
                translation: Vec3::new(pos.x, pos.y + 0.42 * h, pos.z),
                rotation: Quat::from_rotation_y(yaw),
                scale: Vec3::new(1.0, h, 1.0),
            },
            Visibility::default(),
        ))
        .with_children(|p| {
            if FAN_HEADS {
                p.spawn((
                    Mesh3d(head.clone()),
                    MeshMaterial3d(skin),
                    Transform::from_xyz(0.0, 0.46 / h + 0.16, 0.0),
                ));
            }
        });
}

fn spawn_hoop(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    hoop_x: f32,
    home_side: bool,
    board: &Handle<StandardMaterial>,
    rim: &Handle<StandardMaterial>,
    net: &Handle<StandardMaterial>,
    pole: &Handle<StandardMaterial>,
    glass: &Handle<StandardMaterial>,
) {
    let sign = if hoop_x > 0.0 { 1.0 } else { -1.0 };
    let board_x = hoop_x + sign * 0.42;
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(0.12, 1.05, 1.83))),
        MeshMaterial3d(glass.clone()),
        Transform::from_xyz(board_x, RIM_HEIGHT + 0.32, 0.0),
    ));
    // Backboard frame + shooter's square
    for (dy, sy, dz, sz) in [
        (0.52, 0.05, 0.0, 1.83),
        (-0.52, 0.05, 0.0, 1.83),
        (0.0, 1.05, 0.915, 0.05),
        (0.0, 1.05, -0.915, 0.05),
        (0.2, 0.04, 0.0, 0.6),
        (-0.24, 0.04, 0.0, 0.6),
        (-0.02, 0.45, 0.3, 0.04),
        (-0.02, 0.45, -0.3, 0.04),
    ] {
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(0.05, sy, sz))),
            MeshMaterial3d(board.clone()),
            Transform::from_xyz(board_x - sign * 0.04, RIM_HEIGHT + 0.32 + dy, dz),
        ));
    }
    commands.spawn((
        ArenaRoot,
        Hoop { home_side },
        RimMarker { home_side },
        Mesh3d(meshes.add(Torus {
            minor_radius: 0.022,
            major_radius: RIM_RADIUS,
        })),
        MeshMaterial3d(rim.clone()),
        Transform {
            translation: Vec3::new(hoop_x, RIM_HEIGHT, 0.0),
            ..default()
        },
    ));
    // rim bracket
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(0.22, 0.05, 0.12))),
        MeshMaterial3d(rim.clone()),
        Transform::from_xyz(hoop_x + sign * (RIM_RADIUS + 0.1), RIM_HEIGHT - 0.02, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        NetRipple {
            rest_scale: Vec3::ONE,
            pulse: 0.0,
        },
        Mesh3d(meshes.add(Cone::new(RIM_RADIUS * 0.95, 0.45))),
        MeshMaterial3d(net.clone()),
        Transform {
            translation: Vec3::new(hoop_x, RIM_HEIGHT - 0.24, 0.0),
            rotation: Quat::from_rotation_x(std::f32::consts::PI),
            ..default()
        },
    ));
    // Stanchion: padded base, angled arm, pole
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(1.1, 0.5, 1.2))),
        MeshMaterial3d(pole.clone()),
        Transform::from_xyz(hoop_x + sign * 2.2, 0.25, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cylinder::new(0.09, 3.6))),
        MeshMaterial3d(pole.clone()),
        Transform::from_xyz(hoop_x + sign * 2.2, 1.8, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(1.9, 0.09, 0.09))),
        MeshMaterial3d(pole.clone()),
        Transform::from_xyz(hoop_x + sign * 1.3, RIM_HEIGHT + 0.5, 0.0),
    ));
}

fn spin_holo(time: Res<Time>, mut q: Query<&mut Transform, With<HoloSpin>>) {
    for mut t in &mut q {
        t.rotate_y(time.delta_secs() * 0.6);
    }
}

fn pulse_nets(
    time: Res<Time>,
    mut buckets: MessageReader<crate::ball::BucketEvent>,
    mut q: Query<(&mut Transform, &mut NetRipple)>,
) {
    let scored = buckets.read().count() > 0;
    for (mut tf, mut net) in &mut q {
        if scored {
            net.pulse = 1.0;
        }
        net.pulse = (net.pulse - time.delta_secs() * 3.2).max(0.0);
        let s = 1.0 + net.pulse * 0.45;
        tf.scale = net.rest_scale * Vec3::new(s, 1.0 + net.pulse * 0.7, s);
    }
}

fn animate_crowd(
    time: Res<Time>,
    fx: Res<CameraPostFx>,
    mut q: Query<(&CrowdFan, &mut Transform)>,
) {
    let t = time.elapsed_secs();
    let hype = fx.crowd_flash.clamp(0.0, 1.0);
    for (fan, mut tf) in &mut q {
        let idle = ((t * fan.speed + fan.phase).sin() * 0.5 + 0.5) * 0.04;
        let jump = ((t * 9.0 + fan.phase).sin().max(0.0)) * 0.42 * hype;
        let h = tf.scale.y;
        tf.translation.y = fan.base_y + 0.42 * h + idle + jump;
    }
}

pub const _COURT_EXTENT: (f32, f32) = (COURT_HALF_LEN, COURT_HALF_WID);
