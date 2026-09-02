use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::arenas::{ArenaId, ArenaTheme};
use crate::courtpaint::{paint_court, PLANE_HALF_LEN, PLANE_HALF_WID};
use crate::crowd::{Batch, CrowdExt, CrowdMaterial, CrowdSection, CrowdStyle, Lcg, Parts};
use crate::sim::{COURT_HALF_LEN, COURT_HALF_WID, HOOP_X, RIM_HEIGHT, RIM_RADIUS};
use crate::states::{AppState, MatchConfig};

/// Texels per meter for the painted hardwood. 64 → 1997x1165 RGBA (~9 MB), fine for WebGL2.
const COURT_PX_PER_M: u32 = 64;

/// The crowd is merged into one mesh per stand section, so seat density is only a
/// vertex-count question. The web build trims the top tiers to keep the upload small.
#[cfg(target_arch = "wasm32")]
const TIERS: usize = 7;
#[cfg(not(target_arch = "wasm32"))]
const TIERS: usize = 9;
const SEAT_PITCH: f32 = 0.56;

pub struct CourtPlugin;

impl Plugin for CourtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CourtTextures>()
            .init_resource::<BuiltArena>()
            .add_systems(OnEnter(AppState::Playing), ensure_arena)
            .add_systems(OnEnter(AppState::Splash), ensure_arena)
            .add_systems(OnEnter(AppState::MainMenu), ensure_arena)
            .add_systems(OnEnter(AppState::CharacterSelect), ensure_arena)
            .add_systems(OnEnter(AppState::CourtSelect), ensure_arena)
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
                (pulse_nets, move_referee).run_if(in_state(AppState::Playing)),
            )
            .add_systems(Update, pulse_ribbons);
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
struct RibbonBoard;

#[derive(Component)]
struct Referee {
    clock: f32,
}

#[derive(Component)]
struct RefLeg(f32);

#[derive(Resource, Default)]
pub struct CourtTextures {
    by_arena: HashMap<ArenaId, Handle<Image>>,
}

/// Which arena theme the live `ArenaRoot` entities were built for. The arena is
/// shared between the menu and the match so pressing PLAY does not tear down and
/// regenerate the whole stadium (hundreds of thousands of crowd vertices) — that
/// rebuild was a multi-second stall on slower machines and phones.
#[derive(Resource, Default)]
struct BuiltArena(Option<ArenaId>);

/// Builds the arena for the currently selected theme if none exists yet or the
/// theme changed since the last build. Match-only entities (players, ball, HUD)
/// carry `DespawnOnExit(Playing)` and are managed by their own systems.
fn ensure_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut crowd_mats: ResMut<Assets<CrowdMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<CourtTextures>,
    mut built: ResMut<BuiltArena>,
    config: Res<MatchConfig>,
    existing: Query<Entity, With<ArenaRoot>>,
) {
    if built.0 == Some(config.arena) && !existing.is_empty() {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    built.0 = Some(config.arena);
    let theme = config.arena.theme();
    let floor = court_texture(&mut images, &mut cache, &theme);
    build_arena(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut crowd_mats,
        &theme,
        floor,
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
    crowd_mats: &mut Assets<CrowdMaterial>,
    theme: &ArenaTheme,
    floor_tex: Handle<Image>,
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
        base_color: Color::srgb(0.96, 0.96, 1.0),
        perceptual_roughness: 0.85,
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
            RibbonBoard,
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

    // --- stands + crowd + courtside
    spawn_stands(
        commands, meshes, materials, crowd_mats, theme, &riser_mat, &wall_mat,
    );
    spawn_courtside(
        commands,
        meshes,
        materials,
        crowd_mats,
        theme,
        &slab_mat,
        &ribbon_mat,
    );
    spawn_referee(commands, meshes, materials);

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
    crowd_mats: &mut Assets<CrowdMaterial>,
    theme: &ArenaTheme,
    riser_mat: &Handle<StandardMaterial>,
    wall_mat: &Handle<StandardMaterial>,
) {
    let parts = Parts::new();
    let style = CrowdStyle::arena(
        crate::roster::Side::Home.primary(),
        crate::roster::Side::Away.primary(),
        theme.accent,
        theme.crowd,
    );
    let crowd_mat = crowd_mats.add(CrowdMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            ..default()
        },
        extension: CrowdExt::default(),
    });
    let riser = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let step_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.16, 0.17, 0.2),
        perceptual_roughness: 0.95,
        ..default()
    });
    let rail_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.78, 0.82),
        metallic: 0.8,
        perceptual_roughness: 0.3,
        ..default()
    });
    let mut rng = Lcg(0x5EED_1234);

    let tier_rise = 0.82;
    let tier_depth = 1.25;
    let first_y = 0.9;
    let side_start = PLANE_HALF_WID + 1.4;
    let end_start = PLANE_HALF_LEN + 1.9;
    let aisle_every = 12;

    for tier in 0..TIERS {
        let y = first_y + tier as f32 * tier_rise;
        let off = tier as f32 * tier_depth;
        let side_len = (PLANE_HALF_LEN + 1.2 + off) * 2.0;
        let end_len = (PLANE_HALF_WID + 0.6 + off) * 2.0;

        for s in [-1.0f32, 1.0] {
            // long side riser + step lip
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
            commands.spawn((
                ArenaRoot,
                Mesh3d(riser.clone()),
                MeshMaterial3d(step_mat.clone()),
                Transform {
                    translation: Vec3::new(0.0, y - 0.02, z - s * tier_depth * 0.5),
                    scale: Vec3::new(side_len, 0.05, 0.12),
                    ..default()
                },
            ));
            let mut batch = Batch::default();
            let n = (side_len / SEAT_PITCH) as usize;
            for i in 0..n {
                let x = -side_len * 0.5 + (i as f32 + 0.5) * SEAT_PITCH;
                if i % aisle_every == aisle_every / 2 {
                    continue;
                }
                let origin = Vec3::new(x, y, z + s * 0.1);
                let yaw = if s > 0.0 { std::f32::consts::PI } else { 0.0 };
                crate::crowd::seat(&mut batch, &parts, origin, yaw, &style);
                let r = rng.next();
                if r > 0.1 {
                    crate::crowd::fan(&mut batch, &parts, &mut rng, origin, yaw, &style, r > 0.93);
                }
            }
            commands.spawn((
                ArenaRoot,
                CrowdSection,
                Mesh3d(meshes.add(batch.build())),
                MeshMaterial3d(crowd_mat.clone()),
                Transform::IDENTITY,
            ));

            // end riser
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
            let mut batch = Batch::default();
            let n = (end_len / SEAT_PITCH) as usize;
            for i in 0..n {
                let zz = -end_len * 0.5 + (i as f32 + 0.5) * SEAT_PITCH;
                if i % aisle_every == aisle_every / 2 {
                    continue;
                }
                let origin = Vec3::new(x + s * 0.1, y, zz);
                let yaw = if s > 0.0 {
                    -std::f32::consts::FRAC_PI_2
                } else {
                    std::f32::consts::FRAC_PI_2
                };
                crate::crowd::seat(&mut batch, &parts, origin, yaw, &style);
                let r = rng.next();
                if r > 0.12 {
                    crate::crowd::fan(&mut batch, &parts, &mut rng, origin, yaw, &style, r > 0.94);
                }
            }
            commands.spawn((
                ArenaRoot,
                CrowdSection,
                Mesh3d(meshes.add(batch.build())),
                MeshMaterial3d(crowd_mat.clone()),
                Transform::IDENTITY,
            ));
        }
    }

    // Front rail around the lower bowl
    for s in [-1.0f32, 1.0] {
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new((PLANE_HALF_LEN + 1.2) * 2.0, 0.04, 0.04))),
            MeshMaterial3d(rail_mat.clone()),
            Transform::from_xyz(0.0, first_y + 0.9, s * (side_start - tier_depth * 0.5)),
        ));
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(0.04, 0.04, (PLANE_HALF_WID + 0.6) * 2.0))),
            MeshMaterial3d(rail_mat.clone()),
            Transform::from_xyz(s * (end_start - tier_depth * 0.5), first_y + 0.9, 0.0),
        ));
    }

    // Back walls, upper fascia and hanging banners
    let top_y = first_y + TIERS as f32 * tier_rise;
    let far = TIERS as f32 * tier_depth;
    let banner_home = materials.add(StandardMaterial {
        base_color: crate::roster::Side::Home.primary(),
        emissive: LinearRgba::from(crate::roster::Side::Home.primary().to_linear()) * 0.25,
        ..default()
    });
    let banner_away = materials.add(StandardMaterial {
        base_color: crate::roster::Side::Away.primary(),
        emissive: LinearRgba::from(crate::roster::Side::Away.primary().to_linear()) * 0.25,
        ..default()
    });
    let banner_trim = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.9, 0.75),
        emissive: LinearRgba::new(0.6, 0.55, 0.35, 1.0),
        ..default()
    });
    for s in [-1.0f32, 1.0] {
        let wall_z = s * (side_start + far + 0.8);
        commands.spawn((
            ArenaRoot,
            Mesh3d(riser.clone()),
            MeshMaterial3d(wall_mat.clone()),
            Transform {
                translation: Vec3::new(0.0, top_y + 6.0, wall_z),
                scale: Vec3::new((PLANE_HALF_LEN + far + 4.0) * 2.0, 14.0, 0.6),
                ..default()
            },
        ));
        commands.spawn((
            ArenaRoot,
            Mesh3d(riser.clone()),
            MeshMaterial3d(wall_mat.clone()),
            Transform {
                translation: Vec3::new(s * (end_start + far + 0.8), top_y + 6.0, 0.0),
                scale: Vec3::new(0.6, 14.0, (PLANE_HALF_WID + far + 4.0) * 2.0),
                ..default()
            },
        ));
        // Championship banners along the long walls
        for i in 0..9 {
            let x = (i as f32 - 4.0) * 3.4;
            let mat = if i % 2 == 0 {
                &banner_home
            } else {
                &banner_away
            };
            let bz = wall_z - s * 0.5;
            commands.spawn((
                ArenaRoot,
                Mesh3d(meshes.add(Cuboid::new(1.7, 2.8, 0.06))),
                MeshMaterial3d(mat.clone()),
                Transform::from_xyz(x, top_y + 4.2, bz),
            ));
            commands.spawn((
                ArenaRoot,
                Mesh3d(meshes.add(Cuboid::new(1.8, 0.16, 0.1))),
                MeshMaterial3d(banner_trim.clone()),
                Transform::from_xyz(x, top_y + 5.65, bz),
            ));
            commands.spawn((
                ArenaRoot,
                Mesh3d(meshes.add(Cuboid::new(1.2, 0.3, 0.08))),
                MeshMaterial3d(banner_trim.clone()),
                Transform::from_xyz(x, top_y + 4.0, bz),
            ));
        }
    }
}

fn spawn_courtside(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    crowd_mats: &mut Assets<CrowdMaterial>,
    theme: &ArenaTheme,
    slab_mat: &Handle<StandardMaterial>,
    ribbon_mat: &Handle<StandardMaterial>,
) {
    let parts = Parts::new();
    let mut rng = Lcg(0xC0A5_71DE);
    let home = crate::roster::Side::Home;
    let away = crate::roster::Side::Away;
    let crowd_mat = crowd_mats.add(CrowdMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.85,
            ..default()
        },
        extension: CrowdExt::default(),
    });
    let floor_seat_z = PLANE_HALF_WID - 0.55;

    // Scorer's table with LED front
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(7.6, 0.78, 0.7))),
        MeshMaterial3d(slab_mat.clone()),
        Transform::from_xyz(0.0, 0.39, floor_seat_z + 0.25),
    ));
    commands.spawn((
        ArenaRoot,
        RibbonBoard,
        Mesh3d(meshes.add(Cuboid::new(7.4, 0.5, 0.05))),
        MeshMaterial3d(ribbon_mat.clone()),
        Transform::from_xyz(0.0, 0.42, floor_seat_z - 0.12),
    ));
    // Officials behind the table
    let officials = CrowdStyle {
        shirts: vec![[0.05, 0.05, 0.06], [0.9, 0.9, 0.92]],
        ..CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd)
    };
    let mut batch = Batch::default();
    for i in 0..6 {
        let x = (i as f32 - 2.5) * 1.15;
        let o = Vec3::new(x, 0.0, floor_seat_z + 0.95);
        crate::crowd::seat(&mut batch, &parts, o, std::f32::consts::PI, &officials);
        crate::crowd::fan(
            &mut batch,
            &parts,
            &mut rng,
            o,
            std::f32::consts::PI,
            &officials,
            false,
        );
    }

    // Team benches: reserves in full uniform
    let bench_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.1, 0.13),
        perceptual_roughness: 0.7,
        ..default()
    });
    for (sx, side) in [(-1.0f32, home), (1.0, away)] {
        let jersey = side.primary();
        let team = CrowdStyle {
            shirts: vec![
                crate::crowd::lin(jersey),
                crate::crowd::lin(jersey),
                crate::crowd::lin(side.secondary()),
            ],
            pants: vec![crate::crowd::lin(side.secondary())],
            ..CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd)
        };
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(5.2, 0.08, 0.5))),
            MeshMaterial3d(bench_mat.clone()),
            Transform::from_xyz(sx * 7.4, 0.44, floor_seat_z + 0.5),
        ));
        for i in 0..7 {
            let x = sx * (5.0 + i as f32 * 0.8);
            let o = Vec3::new(x, 0.0, floor_seat_z + 0.5);
            crate::crowd::fan(
                &mut batch,
                &parts,
                &mut rng,
                o,
                std::f32::consts::PI,
                &team,
                i == 6,
            );
        }
    }

    // Courtside celebrity row: far sideline, plus the corners on the near side
    let vip = CrowdStyle::arena(
        home.primary(),
        away.primary(),
        theme.accent,
        Color::srgb(0.1, 0.1, 0.12),
    );
    let mut x = -PLANE_HALF_LEN + 1.6;
    while x < PLANE_HALF_LEN - 1.6 {
        let o = Vec3::new(x, 0.0, -floor_seat_z);
        crate::crowd::seat(&mut batch, &parts, o, 0.0, &vip);
        if rng.next() > 0.08 {
            crate::crowd::fan(&mut batch, &parts, &mut rng, o, 0.0, &vip, false);
        }
        x += 0.68;
    }
    for sx in [-1.0f32, 1.0] {
        let mut x = 11.2;
        while x < PLANE_HALF_LEN - 1.4 {
            let o = Vec3::new(sx * x, 0.0, floor_seat_z);
            crate::crowd::seat(&mut batch, &parts, o, std::f32::consts::PI, &vip);
            if rng.next() > 0.1 {
                crate::crowd::fan(
                    &mut batch,
                    &parts,
                    &mut rng,
                    o,
                    std::f32::consts::PI,
                    &vip,
                    false,
                );
            }
            x += 0.68;
        }
    }
    // Baseline photographers sitting on the floor
    let press = CrowdStyle {
        shirts: vec![[0.04, 0.04, 0.05], [0.12, 0.12, 0.14], [0.3, 0.3, 0.33]],
        ..CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd)
    };
    for sx in [-1.0f32, 1.0] {
        for i in 0..8 {
            let z = (i as f32 - 3.5) * 1.3 + if i >= 4 { 1.2 } else { -1.2 };
            let o = Vec3::new(sx * (PLANE_HALF_LEN - 0.55), -0.36, z);
            let yaw = if sx > 0.0 {
                -std::f32::consts::FRAC_PI_2
            } else {
                std::f32::consts::FRAC_PI_2
            };
            crate::crowd::fan(&mut batch, &parts, &mut rng, o, yaw, &press, false);
        }
    }
    commands.spawn((
        ArenaRoot,
        CrowdSection,
        Mesh3d(meshes.add(batch.build())),
        MeshMaterial3d(crowd_mat),
        Transform::IDENTITY,
    ));

    // Broadcast cameras on tripods at the corners
    let cam_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.08, 0.09),
        metallic: 0.5,
        perceptual_roughness: 0.4,
        ..default()
    });
    for (sx, sz) in [(-1.0f32, 1.0f32), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        let base = Vec3::new(
            sx * (COURT_HALF_LEN + 1.0),
            0.0,
            sz * (COURT_HALF_WID + 1.0),
        );
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cylinder::new(0.03, 1.3))),
            MeshMaterial3d(cam_mat.clone()),
            Transform::from_translation(base + Vec3::Y * 0.65),
        ));
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(0.5, 0.28, 0.3))),
            MeshMaterial3d(cam_mat.clone()),
            Transform::from_translation(base + Vec3::Y * 1.42)
                .looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
        ));
    }
}

fn spawn_referee(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let black = materials.add(StandardMaterial {
        base_color: Color::srgb(0.06, 0.06, 0.07),
        perceptual_roughness: 0.8,
        ..default()
    });
    let white = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.95, 0.95),
        perceptual_roughness: 0.8,
        ..default()
    });
    let skin = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.65, 0.5),
        perceptual_roughness: 0.75,
        ..default()
    });
    let grey = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.45, 0.48),
        perceptual_roughness: 0.8,
        ..default()
    });
    let cuboid = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let sphere = meshes.add(Sphere::new(1.0));
    let leg = meshes.add(Capsule3d::new(0.08, 0.5));
    let arm = meshes.add(Capsule3d::new(0.06, 0.42));

    commands
        .spawn((
            ArenaRoot,
            Referee { clock: 0.0 },
            Transform::from_xyz(0.0, 0.0, COURT_HALF_WID + 0.6),
            Visibility::default(),
        ))
        .with_children(|r| {
            // striped shirt
            for i in 0..6 {
                let mat = if i % 2 == 0 { &black } else { &white };
                r.spawn((
                    Mesh3d(cuboid.clone()),
                    MeshMaterial3d(mat.clone()),
                    Transform {
                        translation: Vec3::new((i as f32 - 2.5) * 0.085, 1.2, 0.0),
                        scale: Vec3::new(0.085, 0.6, 0.3),
                        ..default()
                    },
                ));
            }
            r.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(grey.clone()),
                Transform {
                    translation: Vec3::new(0.0, 0.8, 0.0),
                    scale: Vec3::new(0.48, 0.28, 0.3),
                    ..default()
                },
            ));
            r.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(skin.clone()),
                Transform {
                    translation: Vec3::new(0.0, 1.7, 0.0),
                    scale: Vec3::splat(0.19),
                    ..default()
                },
            ));
            for sx in [-1.0f32, 1.0] {
                r.spawn((
                    RefLeg(sx),
                    Mesh3d(leg.clone()),
                    MeshMaterial3d(black.clone()),
                    Transform::from_xyz(sx * 0.14, 0.4, 0.0),
                ));
                r.spawn((
                    Mesh3d(arm.clone()),
                    MeshMaterial3d(skin.clone()),
                    Transform::from_xyz(sx * 0.32, 1.15, 0.0),
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
    // Net: hanging strings + two rings under a root that the ripple system scales
    let string = meshes.add(Cuboid::new(0.012, 1.0, 0.012));
    let ring_a = meshes.add(Torus {
        minor_radius: 0.006,
        major_radius: RIM_RADIUS * 0.78,
    });
    let ring_b = meshes.add(Torus {
        minor_radius: 0.006,
        major_radius: RIM_RADIUS * 0.55,
    });
    let net_len = 0.44;
    let bottom_r = RIM_RADIUS * 0.5;
    commands
        .spawn((
            ArenaRoot,
            NetRipple {
                rest_scale: Vec3::ONE,
                pulse: 0.0,
            },
            Transform::from_xyz(hoop_x, RIM_HEIGHT - 0.02, 0.0),
            Visibility::default(),
        ))
        .with_children(|n| {
            let count = 14;
            for i in 0..count {
                let a = i as f32 / count as f32 * std::f32::consts::TAU;
                let top = Vec3::new(
                    a.cos() * RIM_RADIUS * 0.97,
                    0.0,
                    a.sin() * RIM_RADIUS * 0.97,
                );
                let bot = Vec3::new(a.cos() * bottom_r, -net_len, a.sin() * bottom_r);
                let mid = (top + bot) * 0.5;
                let dir = (bot - top).normalize();
                // strings zig-zag: alternate lean left/right so they read as a mesh
                let twist = if i % 2 == 0 { 0.18 } else { -0.18 };
                let rot = Quat::from_rotation_arc(Vec3::NEG_Y, dir) * Quat::from_rotation_y(twist);
                n.spawn((
                    Mesh3d(string.clone()),
                    MeshMaterial3d(net.clone()),
                    Transform {
                        translation: mid,
                        rotation: rot,
                        scale: Vec3::new(1.0, (bot - top).length(), 1.0),
                    },
                ));
            }
            n.spawn((
                Mesh3d(ring_a.clone()),
                MeshMaterial3d(net.clone()),
                Transform::from_xyz(0.0, -net_len * 0.4, 0.0),
            ));
            n.spawn((
                Mesh3d(ring_b.clone()),
                MeshMaterial3d(net.clone()),
                Transform::from_xyz(0.0, -net_len * 0.85, 0.0),
            ));
        });
    // Backboard bottom padding
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(0.16, 0.08, 1.9))),
        MeshMaterial3d(rim.clone()),
        Transform::from_xyz(board_x, RIM_HEIGHT + 0.32 - 0.56, 0.0),
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

fn move_referee(
    time: Res<Time>,
    ball: Query<&Transform, (With<crate::ball::Ball>, Without<Referee>)>,
    mut refs: Query<(&mut Transform, &mut Referee, &Children)>,
    mut legs: Query<(&RefLeg, &mut Transform), (Without<Referee>, Without<crate::ball::Ball>)>,
) {
    let Ok(btf) = ball.single() else {
        return;
    };
    let dt = time.delta_secs();
    for (mut tf, mut r, children) in &mut refs {
        let target_x = (btf.translation.x * 0.8).clamp(-11.5, 11.5);
        let dx = target_x - tf.translation.x;
        let step = dx.clamp(-4.5 * dt, 4.5 * dt);
        tf.translation.x += step;
        let moving = step.abs() > 0.002;
        if moving {
            r.clock += dt * 9.0;
        }
        let face = if dx.abs() > 0.3 {
            Quat::from_rotation_y(if dx > 0.0 {
                std::f32::consts::FRAC_PI_2
            } else {
                -std::f32::consts::FRAC_PI_2
            })
        } else {
            Quat::from_rotation_y(std::f32::consts::PI)
        };
        tf.rotation = tf.rotation.slerp(face, 1.0 - (-8.0 * dt).exp());
        for child in children.iter() {
            if let Ok((leg, mut ltf)) = legs.get_mut(child) {
                let swing = if moving {
                    (r.clock
                        + if leg.0 > 0.0 {
                            std::f32::consts::PI
                        } else {
                            0.0
                        })
                    .sin()
                        * 0.5
                } else {
                    0.0
                };
                ltf.rotation = Quat::from_rotation_x(swing);
            }
        }
    }
}

fn pulse_ribbons(
    time: Res<Time>,
    config: Res<MatchConfig>,
    boards: Query<&MeshMaterial3d<StandardMaterial>, With<RibbonBoard>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(handle) = boards.iter().next() else {
        return;
    };
    let Some(mat) = mats.get_mut(&handle.0) else {
        return;
    };
    let t = time.elapsed_secs();
    let accent = LinearRgba::from(config.arena.theme().accent.to_linear());
    let home = LinearRgba::from(crate::roster::Side::Home.primary().to_linear());
    let away = LinearRgba::from(crate::roster::Side::Away.primary().to_linear());
    // Cycle accent → home → away every few seconds, with a soft shimmer on top.
    let cycle = (t * 0.25).fract() * 3.0;
    let (a, b, k) = if cycle < 1.0 {
        (accent, home, cycle)
    } else if cycle < 2.0 {
        (home, away, cycle - 1.0)
    } else {
        (away, accent, cycle - 2.0)
    };
    let k = (k * std::f32::consts::PI).sin().powi(4);
    let mix = LinearRgba::new(
        a.red + (b.red - a.red) * k,
        a.green + (b.green - a.green) * k,
        a.blue + (b.blue - a.blue) * k,
        1.0,
    );
    let shimmer = 1.4 + (t * 6.0).sin() * 0.2;
    mat.emissive = mix * shimmer;
}

pub const _COURT_EXTENT: (f32, f32) = (COURT_HALF_LEN, COURT_HALF_WID);
