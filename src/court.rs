use bevy::prelude::*;

use crate::arenas::ArenaTheme;
use crate::sim::{COURT_HALF_LEN, COURT_HALF_WID, HOOP_X, PAINT_DEPTH, PAINT_HALF_WIDTH, RIM_HEIGHT, RIM_RADIUS, THREE_RADIUS};
use crate::states::{AppState, MatchConfig};

pub struct CourtPlugin;

impl Plugin for CourtPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(AppState::Playing), (cleanup_arenas, spawn_arena).chain())
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

fn cleanup_arenas(mut commands: Commands, q: Query<Entity, With<ArenaRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

pub fn spawn_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<MatchConfig>,
) {
    let theme = config.arena.theme();
    build_arena(&mut commands, &mut meshes, &mut materials, &theme, true);
}

fn spawn_menu_arena(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    config: Res<MatchConfig>,
    existing: Query<Entity, With<ArenaRoot>>,
) {
    if !existing.is_empty() {
        return;
    }
    let theme = config.arena.theme();
    build_arena(&mut commands, &mut meshes, &mut materials, &theme, false);
}

pub fn despawn_arena(commands: &mut Commands, q: &Query<Entity, With<ArenaRoot>>) {
    for e in q.iter() {
        commands.entity(e).despawn();
    }
}

fn build_arena(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    theme: &ArenaTheme,
    full_crowd: bool,
) {
    commands.insert_resource(GlobalAmbientLight {
        color: theme.ambient,
        brightness: 90.0,
        ..default()
    });
    commands.insert_resource(ClearColor(theme.sky));

    let floor_mat_a = materials.add(StandardMaterial {
        base_color: theme.floor_a,
        perceptual_roughness: 0.35,
        metallic: 0.18,
        ..default()
    });
    let floor_mat_b = materials.add(StandardMaterial {
        base_color: theme.floor_b,
        perceptual_roughness: 0.35,
        metallic: 0.18,
        ..default()
    });
    let line_mat = materials.add(StandardMaterial {
        base_color: theme.line,
        emissive: theme.emissive * 0.35,
        perceptual_roughness: 0.4,
        ..default()
    });
    let paint_mat = materials.add(StandardMaterial {
        base_color: theme.paint,
        perceptual_roughness: 0.5,
        ..default()
    });
    let neon = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.9, 1.0),
        emissive: theme.emissive,
        unlit: false,
        ..default()
    });
    let crowd_mat = materials.add(StandardMaterial {
        base_color: theme.crowd,
        perceptual_roughness: 0.9,
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
        base_color: Color::srgba(0.95, 0.95, 1.0, 0.45),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.8,
        ..default()
    });
    let pole_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.16, 0.2),
        metallic: 0.7,
        perceptual_roughness: 0.3,
        ..default()
    });

    let mut root = commands.spawn((
        ArenaRoot,
        DespawnOnExit(AppState::Playing),
        Transform::default(),
        Visibility::default(),
    ));

    // Menu arenas should live until Playing starts; tag them to despawn when leaving menus
    // by not using DespawnOnExit(Playing) only. We'll use a unique marker and despawn
    // on Playing enter if leftover. For menu we attach DespawnOnExit of multiple? Bevy
    // only supports one. Menu courts persist until Playing spawn replaces them.
    let _ = &mut root;

    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(COURT_HALF_LEN, 0.18, COURT_HALF_WID * 2.0))),
        MeshMaterial3d(floor_mat_a),
        Transform::from_xyz(-COURT_HALF_LEN * 0.5, -0.09, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(COURT_HALF_LEN, 0.18, COURT_HALF_WID * 2.0))),
        MeshMaterial3d(floor_mat_b),
        Transform::from_xyz(COURT_HALF_LEN * 0.5, -0.09, 0.0),
    ));

    // Boundary + center line
    spawn_line(commands, meshes, &line_mat, 0.0, COURT_HALF_LEN * 2.0, 0.08, COURT_HALF_WID * 2.0);
    spawn_line(commands, meshes, &line_mat, 0.0, 0.08, COURT_HALF_LEN * 2.0, 0.12);
    spawn_line(commands, meshes, &line_mat, 0.0, COURT_HALF_LEN * 2.0 + 0.16, 0.1, 0.12);
    spawn_line(commands, meshes, &line_mat, 0.0, 0.1, 0.12, COURT_HALF_WID * 2.0 + 0.16);
    spawn_line(commands, meshes, &line_mat, COURT_HALF_LEN, 0.1, 0.12, COURT_HALF_WID * 2.0 + 0.16);
    spawn_line(commands, meshes, &line_mat, -COURT_HALF_LEN, 0.1, 0.12, COURT_HALF_WID * 2.0 + 0.16);
    spawn_line(commands, meshes, &line_mat, 0.0, COURT_HALF_LEN * 2.0 + 0.16, 0.1, 0.12);
    spawn_line(commands, meshes, &line_mat, 0.0, COURT_HALF_WID, COURT_HALF_LEN * 2.0 + 0.16, 0.1);
    spawn_line(commands, meshes, &line_mat, 0.0, -COURT_HALF_WID, COURT_HALF_LEN * 2.0 + 0.16, 0.1);

    // Paints
    for sign in [-1.0, 1.0] {
        let hoop_x = sign * HOOP_X;
        let paint_x = hoop_x - sign * PAINT_DEPTH * 0.5;
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(PAINT_DEPTH, 0.04, PAINT_HALF_WIDTH * 2.0))),
            MeshMaterial3d(paint_mat.clone()),
            Transform::from_xyz(paint_x, 0.03, 0.0),
        ));
        spawn_arc(commands, meshes, &line_mat, hoop_x, THREE_RADIUS, sign);
        spawn_hoop(
            commands,
            meshes,
            materials,
            hoop_x,
            sign < 0.0,
            &board_mat,
            &rim_mat,
            &net_mat,
            &pole_mat,
            &glass,
        );
    }

    // Center circle
    spawn_circle_ring(commands, meshes, &line_mat, 0.0, 0.0, 1.8);

    // Center logo disc
    commands.spawn((
        ArenaRoot,
        HoloSpin,
        Mesh3d(meshes.add(Cylinder::new(1.15, 0.04))),
        MeshMaterial3d(neon.clone()),
        Transform::from_xyz(0.0, 0.05, 0.0),
    ));

    // Stadium bowl
    if full_crowd {
        let step = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        for ring in 0..4 {
            let y = 1.2 + ring as f32 * 1.15;
            let depth = 18.0 + ring as f32 * 2.4;
            let width = 22.0 + ring as f32 * 3.0;
            commands.spawn((
                ArenaRoot,
                Mesh3d(step.clone()),
                MeshMaterial3d(crowd_mat.clone()),
                Transform {
                    translation: Vec3::new(0.0, y, depth),
                    scale: Vec3::new(width, 1.0, 1.6),
                    ..default()
                },
            ));
            commands.spawn((
                ArenaRoot,
                Mesh3d(step.clone()),
                MeshMaterial3d(crowd_mat.clone()),
                Transform {
                    translation: Vec3::new(0.0, y, -depth),
                    scale: Vec3::new(width, 1.0, 1.6),
                    ..default()
                },
            ));
            commands.spawn((
                ArenaRoot,
                Mesh3d(step.clone()),
                MeshMaterial3d(crowd_mat.clone()),
                Transform {
                    translation: Vec3::new(depth + 2.0, y, 0.0),
                    scale: Vec3::new(1.6, 1.0, width * 0.7),
                    ..default()
                },
            ));
            commands.spawn((
                ArenaRoot,
                Mesh3d(step.clone()),
                MeshMaterial3d(crowd_mat.clone()),
                Transform {
                    translation: Vec3::new(-(depth + 2.0), y, 0.0),
                    scale: Vec3::new(1.6, 1.0, width * 0.7),
                    ..default()
                },
            ));
        }

        // Jumbotrons
        let screen = materials.add(StandardMaterial {
            base_color: Color::srgb(0.05, 0.08, 0.12),
            emissive: theme.emissive * 0.55,
            ..default()
        });
        commands.spawn((
            ArenaRoot,
            Mesh3d(meshes.add(Cuboid::new(8.0, 3.2, 0.2))),
            MeshMaterial3d(screen.clone()),
            Transform::from_xyz(0.0, 9.5, 0.0),
        ));
        commands.spawn((
            ArenaRoot,
            HoloSpin,
            Mesh3d(meshes.add(Cuboid::new(2.2, 0.18, 2.2))),
            MeshMaterial3d(neon),
            Transform::from_xyz(0.0, 11.4, 0.0),
        ));
    }

    // Light rig
    commands.spawn((
        ArenaRoot,
        DirectionalLight {
            illuminance: 4_500.0,
            shadows_enabled: false, // keep off — WASM + llvmpipe cannot afford cascaded shadows
            color: theme.ambient,
            ..default()
        },
        Transform::from_xyz(12.0, 28.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    for (x, z, c) in [
        (0.0, 0.0, Color::srgb(1.0, 0.95, 0.85)),
        (-8.0, 6.0, Color::srgb(0.4, 0.8, 1.0)),
        (8.0, -6.0, Color::srgb(1.0, 0.4, 0.8)),
    ] {
        commands.spawn((
            ArenaRoot,
            PointLight {
                intensity: 1_200_000.0,
                range: 40.0,
                color: c,
                shadows_enabled: false, // keep off — WASM + llvmpipe cannot afford cascaded shadows
                ..default()
            },
            Transform::from_xyz(x, 12.0, z),
        ));
    }

    // LED advertising ribbon
    let ribbon = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.04),
        emissive: theme.emissive * 0.4,
        ..default()
    });
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(COURT_HALF_LEN * 2.0 + 1.0, 0.7, 0.18))),
        MeshMaterial3d(ribbon.clone()),
        Transform::from_xyz(0.0, 0.45, COURT_HALF_WID + 0.6),
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(COURT_HALF_LEN * 2.0 + 1.0, 0.7, 0.18))),
        MeshMaterial3d(ribbon),
        Transform::from_xyz(0.0, 0.45, -(COURT_HALF_WID + 0.6)),
    ));

    // Sky temple petals / toon extra
    if matches!(theme.id, crate::arenas::ArenaId::SkyTemple) {
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

fn spawn_line(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<StandardMaterial>,
    x: f32,
    z_or_len: f32,
    sx: f32,
    sz: f32,
) {
    // Overload-ish helper: if sx small it's a long z line etc.
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(sx.max(z_or_len.min(sx + COURT_HALF_LEN * 4.0)), 0.05, sz))),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(
            if sx <= 0.15 { x } else { 0.0 }.max(x) * if sx > 1.0 { 0.0 } else { 1.0 } + if sx > 1.0 { 0.0 } else { x },
            0.04,
            if sz > 1.0 && sx <= 0.15 { 0.0 } else { 0.0 },
        ),
    ));
}

fn spawn_arc(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<StandardMaterial>,
    hoop_x: f32,
    radius: f32,
    sign: f32,
) {
    let segs = 18;
    let cube = meshes.add(Cuboid::new(0.55, 0.05, 0.1));
    for i in 0..segs {
        let t = i as f32 / (segs as f32 - 1.0);
        let ang = -std::f32::consts::FRAC_PI_2 + t * std::f32::consts::PI;
        // Arc opens toward midcourt
        let x = hoop_x - sign * radius * ang.cos().abs() * 0.15 - sign * (radius * (1.0 - (ang.sin()).abs() * 0.0));
        let world_x = hoop_x - sign * (radius * ang.cos().max(0.15));
        let z = radius * ang.sin();
        let _ = x;
        commands.spawn((
            ArenaRoot,
            Mesh3d(cube.clone()),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: Vec3::new(world_x, 0.05, z),
                rotation: Quat::from_rotation_y(-ang * sign),
                ..default()
            },
        ));
    }
}

fn spawn_circle_ring(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    mat: &Handle<StandardMaterial>,
    cx: f32,
    cz: f32,
    r: f32,
) {
    let n = 20;
    let boxm = meshes.add(Cuboid::new(0.45, 0.05, 0.08));
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        commands.spawn((
            ArenaRoot,
            Mesh3d(boxm.clone()),
            MeshMaterial3d(mat.clone()),
            Transform {
                translation: Vec3::new(cx + a.cos() * r, 0.05, cz + a.sin() * r),
                rotation: Quat::from_rotation_y(-a + std::f32::consts::FRAC_PI_2),
                ..default()
            },
        ));
    }
}

fn spawn_hoop(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    _materials: &mut Assets<StandardMaterial>,
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
        MeshMaterial3d(board.clone()),
        Transform::from_xyz(board_x, RIM_HEIGHT + 0.32, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(0.04, 0.55, 0.9))),
        MeshMaterial3d(glass.clone()),
        Transform::from_xyz(board_x - sign * 0.08, RIM_HEIGHT + 0.28, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Hoop { home_side },
        RimMarker { home_side },
        Mesh3d(meshes.add(Torus::new(0.02, RIM_RADIUS))),
        MeshMaterial3d(rim.clone()),
        Transform {
            translation: Vec3::new(hoop_x, RIM_HEIGHT, 0.0),
            rotation: Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            ..default()
        },
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cone::new(RIM_RADIUS * 0.95, 0.45))),
        MeshMaterial3d(net.clone()),
        Transform {
            translation: Vec3::new(hoop_x, RIM_HEIGHT - 0.28, 0.0),
            rotation: Quat::from_rotation_x(std::f32::consts::PI),
            ..default()
        },
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cylinder::new(0.08, 3.2))),
        MeshMaterial3d(pole.clone()),
        Transform::from_xyz(hoop_x + sign * 1.15, 1.6, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(1.1, 0.08, 0.08))),
        MeshMaterial3d(pole.clone()),
        Transform::from_xyz(hoop_x + sign * 0.7, RIM_HEIGHT, 0.0),
    ));
}

fn spin_holo(time: Res<Time>, mut q: Query<&mut Transform, With<HoloSpin>>) {
    for mut t in &mut q {
        t.rotate_y(time.delta_secs() * 0.6);
        t.translation.y += (time.elapsed_secs() * 2.0).sin() * 0.0008;
    }
}
