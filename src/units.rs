use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::prelude::*;

use crate::roster::{CharacterId, CharacterProfile, HairStyle, Side};
use crate::sim::{PLAYER_RADIUS, speed_from_rating};
use crate::states::Paused;

pub struct UnitsPlugin;

impl Plugin for UnitsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (face_velocity, animate_rigs, update_face_expr, stamina_regen)
                .run_if(in_state(crate::states::AppState::Playing)),
        );
    }
}

#[derive(Component, Clone, Copy)]
pub struct Player {
    pub id: CharacterId,
    pub side: Side,
    pub slot: u8,
    pub human: bool,
}

#[derive(Component, Clone, Copy)]
pub struct Ratings {
    pub speed: f32,
    pub three: f32,
    pub mid: f32,
    pub dunk: f32,
    pub handle: f32,
    pub pass: f32,
    pub steal: f32,
    pub block: f32,
    pub rebound: f32,
    pub strength: f32,
    pub height: f32,
}

impl From<CharacterProfile> for Ratings {
    fn from(p: CharacterProfile) -> Self {
        Self {
            speed: p.speed as f32,
            three: p.three as f32,
            mid: p.mid as f32,
            dunk: p.dunk as f32,
            handle: p.handle as f32,
            pass: p.pass as f32,
            steal: p.steal as f32,
            block: p.block as f32,
            rebound: p.rebound as f32,
            strength: p.strength as f32,
            height: p.height_m,
        }
    }
}

#[derive(Component, Default, Clone, Copy)]
pub struct MoveVel(pub Vec3);

#[derive(Component)]
pub struct Stamina(pub f32);

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    Idle,
    Run,
    Sprint,
    Shoot,
    Dunk,
    Pass,
    Block,
    Celebrate,
    Stumble,
}

#[derive(Component)]
pub struct PoseClock(pub f32);

#[derive(Component)]
pub struct Controlled;

#[derive(Component)]
pub struct Rig {
    pub torso: Entity,
    pub head: Entity,
    pub l_arm: Entity,
    pub r_arm: Entity,
    pub l_leg: Entity,
    pub r_leg: Entity,
}

/// Face parts live as children; this handle sits on the player root.
#[derive(Component)]
pub struct FaceRig {
    pub brow_l: Entity,
    pub brow_r: Entity,
    pub mouth: Entity,
    pub iris_l: Entity,
    pub iris_r: Entity,
    pub blush_l: Entity,
    pub blush_r: Entity,
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Default)]
pub enum FaceExpr {
    #[default]
    Neutral,
    Focus,
    Celebrate,
    Angry,
    Pain,
}

#[derive(Component)]
pub struct BoxLine {
    pub pts: u32,
    pub ast: u32,
    pub reb: u32,
    pub stl: u32,
    pub blk: u32,
    pub fg_made: u32,
    pub fg_att: u32,
}

impl Default for BoxLine {
    fn default() -> Self {
        Self {
            pts: 0,
            ast: 0,
            reb: 0,
            stl: 0,
            blk: 0,
            fg_made: 0,
            fg_att: 0,
        }
    }
}

pub fn spawn_player(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    id: CharacterId,
    side: Side,
    slot: u8,
    human: bool,
    pos: Vec3,
) -> Entity {
    let p = id.profile();
    let ratings = Ratings::from(p);
    let scale = p.height_m / 1.88;
    let jersey = side.primary();
    let trim = side.secondary();
    let skin = materials.add(StandardMaterial {
        base_color: p.skin,
        perceptual_roughness: 0.7,
        ..default()
    });
    let jersey_mat = materials.add(StandardMaterial {
        base_color: jersey,
        perceptual_roughness: 0.55,
        emissive: LinearRgba::from(jersey.to_linear()) * 0.15,
        ..default()
    });
    let trim_mat = materials.add(StandardMaterial {
        base_color: trim,
        perceptual_roughness: 0.5,
        ..default()
    });
    let hair_mat = materials.add(StandardMaterial {
        base_color: p.hair_color,
        perceptual_roughness: 0.6,
        emissive: LinearRgba::from(p.hair_color.to_linear()) * 0.25,
        ..default()
    });
    let eye_w = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        ..default()
    });
    let eye_i = materials.add(StandardMaterial {
        base_color: p.eye,
        emissive: LinearRgba::from(p.eye.to_linear()) * 2.0,
        unlit: true,
        ..default()
    });
    let shoe = materials.add(StandardMaterial {
        base_color: p.accent,
        metallic: 0.4,
        perceptual_roughness: 0.35,
        ..default()
    });
    let brow_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.07, 0.07),
        unlit: true,
        ..default()
    });
    let mouth_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.62, 0.16, 0.22),
        unlit: true,
        ..default()
    });
    let blush_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 0.42, 0.55, 0.7),
        emissive: LinearRgba::new(0.55, 0.08, 0.16, 1.0),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let number_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::new(0.45, 0.45, 0.5, 1.0),
        unlit: true,
        ..default()
    });

    let sphere = meshes.add(Sphere::new(1.0));
    let capsule = meshes.add(Capsule3d::new(0.12, 0.38));
    let cuboid = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    let mut torso_id = Entity::PLACEHOLDER;
    let mut head_id = Entity::PLACEHOLDER;
    let mut l_arm = Entity::PLACEHOLDER;
    let mut r_arm = Entity::PLACEHOLDER;
    let mut l_leg = Entity::PLACEHOLDER;
    let mut r_leg = Entity::PLACEHOLDER;
    let mut brow_l = Entity::PLACEHOLDER;
    let mut brow_r = Entity::PLACEHOLDER;
    let mut mouth = Entity::PLACEHOLDER;
    let mut iris_l = Entity::PLACEHOLDER;
    let mut iris_r = Entity::PLACEHOLDER;
    let mut blush_l = Entity::PLACEHOLDER;
    let mut blush_r = Entity::PLACEHOLDER;

    let root = commands
        .spawn((
            Player {
                id,
                side,
                slot,
                human,
            },
            ratings,
            MoveVel(Vec3::ZERO),
            Stamina(1.0),
            Pose::Idle,
            PoseClock(0.0),
            FaceExpr::Neutral,
            BoxLine::default(),
            Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
            Visibility::default(),
            crate::court::ArenaRoot,
            DespawnOnExit(crate::states::AppState::Playing),
        ))
        .with_children(|root| {
            torso_id = root
                .spawn((
                    Mesh3d(cuboid.clone()),
                    MeshMaterial3d(jersey_mat.clone()),
                    Transform {
                        translation: Vec3::new(0.0, 1.15, 0.0),
                        scale: Vec3::new(0.55, 0.7, 0.32),
                        ..default()
                    },
                ))
                .id();
            root.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(trim_mat.clone()),
                Transform {
                    translation: Vec3::new(0.0, 1.15, -0.12),
                    scale: Vec3::new(0.22, 0.28, 0.12),
                    ..default()
                },
            ));
            head_id = root
                .spawn((
                    Mesh3d(sphere.clone()),
                    MeshMaterial3d(skin.clone()),
                    Transform {
                        translation: Vec3::new(0.0, 1.72, 0.0),
                        scale: Vec3::splat(0.24),
                        ..default()
                    },
                ))
                .with_children(|head| {
                    // Local to the head so brows/mouth ride the bob.
                    head.spawn((
                        Mesh3d(sphere.clone()),
                        MeshMaterial3d(eye_w.clone()),
                        Transform {
                            translation: Vec3::new(-0.35, 0.10, 0.77),
                            scale: Vec3::new(0.38, 0.46, 0.19),
                            ..default()
                        },
                    ));
                    head.spawn((
                        Mesh3d(sphere.clone()),
                        MeshMaterial3d(eye_w),
                        Transform {
                            translation: Vec3::new(0.35, 0.10, 0.77),
                            scale: Vec3::new(0.38, 0.46, 0.19),
                            ..default()
                        },
                    ));
                    iris_l = head
                        .spawn((
                            Mesh3d(sphere.clone()),
                            MeshMaterial3d(eye_i.clone()),
                            Transform {
                                translation: Vec3::new(-0.35, 0.08, 0.92),
                                scale: Vec3::splat(0.175),
                                ..default()
                            },
                        ))
                        .id();
                    iris_r = head
                        .spawn((
                            Mesh3d(sphere.clone()),
                            MeshMaterial3d(eye_i),
                            Transform {
                                translation: Vec3::new(0.35, 0.08, 0.92),
                                scale: Vec3::splat(0.175),
                                ..default()
                            },
                        ))
                        .id();
                    brow_l = head
                        .spawn((
                            Mesh3d(cuboid.clone()),
                            MeshMaterial3d(brow_mat.clone()),
                            Transform {
                                translation: Vec3::new(-0.35, 0.50, 0.83),
                                scale: Vec3::new(0.38, 0.07, 0.13),
                                ..default()
                            },
                        ))
                        .id();
                    brow_r = head
                        .spawn((
                            Mesh3d(cuboid.clone()),
                            MeshMaterial3d(brow_mat),
                            Transform {
                                translation: Vec3::new(0.35, 0.50, 0.83),
                                scale: Vec3::new(0.38, 0.07, 0.13),
                                ..default()
                            },
                        ))
                        .id();
                    mouth = head
                        .spawn((
                            Mesh3d(cuboid.clone()),
                            MeshMaterial3d(mouth_mat),
                            Transform {
                                translation: Vec3::new(0.0, -0.42, 0.85),
                                scale: Vec3::new(0.33, 0.09, 0.10),
                                ..default()
                            },
                        ))
                        .id();
                    blush_l = head
                        .spawn((
                            Mesh3d(sphere.clone()),
                            MeshMaterial3d(blush_mat.clone()),
                            Transform {
                                translation: Vec3::new(-0.60, -0.25, 0.65),
                                scale: Vec3::splat(0.004),
                                ..default()
                            },
                        ))
                        .id();
                    blush_r = head
                        .spawn((
                            Mesh3d(sphere.clone()),
                            MeshMaterial3d(blush_mat),
                            Transform {
                                translation: Vec3::new(0.60, -0.25, 0.65),
                                scale: Vec3::splat(0.004),
                                ..default()
                            },
                        ))
                        .id();
                })
                .id();
            spawn_jersey_number(
                root,
                &cuboid,
                &number_mat,
                jersey_number(id, slot),
            );
            spawn_hair(root, p.hair, meshes, &cuboid, &sphere, &hair_mat);
            l_arm = root
                .spawn((
                    Mesh3d(capsule.clone()),
                    MeshMaterial3d(skin.clone()),
                    Transform::from_xyz(-0.42, 1.2, 0.0),
                ))
                .id();
            r_arm = root
                .spawn((
                    Mesh3d(capsule.clone()),
                    MeshMaterial3d(skin.clone()),
                    Transform::from_xyz(0.42, 1.2, 0.0),
                ))
                .id();
            l_leg = root
                .spawn((
                    Mesh3d(capsule.clone()),
                    MeshMaterial3d(trim_mat.clone()),
                    Transform::from_xyz(-0.16, 0.48, 0.0),
                ))
                .id();
            r_leg = root
                .spawn((
                    Mesh3d(capsule),
                    MeshMaterial3d(trim_mat),
                    Transform::from_xyz(0.16, 0.48, 0.0),
                ))
                .id();
            root.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(shoe.clone()),
                Transform {
                    translation: Vec3::new(-0.16, 0.08, 0.05),
                    scale: Vec3::new(0.16, 0.08, 0.28),
                    ..default()
                },
            ));
            root.spawn((
                Mesh3d(cuboid),
                MeshMaterial3d(shoe),
                Transform {
                    translation: Vec3::new(0.16, 0.08, 0.05),
                    scale: Vec3::new(0.16, 0.08, 0.28),
                    ..default()
                },
            ));
        })
        .id();

    commands.entity(root).insert((
        Rig {
            torso: torso_id,
            head: head_id,
            l_arm,
            r_arm,
            l_leg,
            r_leg,
        },
        FaceRig {
            brow_l,
            brow_r,
            mouth,
            iris_l,
            iris_r,
            blush_l,
            blush_r,
        },
    ));

    if human {
        commands.entity(root).insert(Controlled);
    }

    let _ = PLAYER_RADIUS;
    root
}

fn jersey_number(id: CharacterId, slot: u8) -> u8 {
    // Character-based broadcast numbers (slot+1 is the simple fallback).
    let _ = slot + 1;
    match id {
        CharacterId::KaitoFlash => 1,
        CharacterId::MikaOrbit => 3,
        CharacterId::JinGravity => 23,
        CharacterId::ReiWall => 33,
        CharacterId::YunaSilk => 8,
        CharacterId::ZeroGhost => 0,
        CharacterId::LunaEclipse => 11,
        CharacterId::TaroTitan => 50,
        CharacterId::AikoPrism => 7,
        CharacterId::KenjiVolt => 24,
    }
}

fn spawn_jersey_number(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    cuboid: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    number: u8,
) {
    if number >= 10 {
        spawn_digit(root, cuboid, mat, number / 10, Vec3::new(-0.07, 1.20, 0.175));
        spawn_digit(root, cuboid, mat, number % 10, Vec3::new(0.07, 1.20, 0.175));
    } else {
        spawn_digit(root, cuboid, mat, number, Vec3::new(0.0, 1.20, 0.175));
    }
}

fn spawn_digit(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    cuboid: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    digit: u8,
    origin: Vec3,
) {
    const A: u8 = 1 << 0;
    const B: u8 = 1 << 1;
    const C: u8 = 1 << 2;
    const D: u8 = 1 << 3;
    const E: u8 = 1 << 4;
    const F: u8 = 1 << 5;
    const G: u8 = 1 << 6;
    let mask = match digit {
        0 => A | B | C | D | E | F,
        1 => B | C,
        2 => A | B | G | E | D,
        3 => A | B | G | C | D,
        4 => F | G | B | C,
        5 => A | F | G | C | D,
        6 => A | F | G | E | C | D,
        7 => A | B | C,
        8 => A | B | C | D | E | F | G,
        9 => A | B | C | D | F | G,
        _ => 0,
    };
    let w = 0.055;
    let h = 0.038;
    let th = 0.016;
    let dz = 0.02;
    let segs = [
        (Vec3::new(0.0, 0.076, 0.0), Vec3::new(w, th, dz)),
        (Vec3::new(0.03, 0.038, 0.0), Vec3::new(th, h, dz)),
        (Vec3::new(0.03, -0.038, 0.0), Vec3::new(th, h, dz)),
        (Vec3::new(0.0, -0.076, 0.0), Vec3::new(w, th, dz)),
        (Vec3::new(-0.03, -0.038, 0.0), Vec3::new(th, h, dz)),
        (Vec3::new(-0.03, 0.038, 0.0), Vec3::new(th, h, dz)),
        (Vec3::new(0.0, 0.0, 0.0), Vec3::new(w, th, dz)),
    ];
    for (i, (off, scale)) in segs.iter().enumerate() {
        if mask & (1 << i) != 0 {
            root.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(mat.clone()),
                Transform {
                    translation: origin + *off,
                    scale: *scale,
                    ..default()
                },
            ));
        }
    }
}

fn spawn_hair(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    style: HairStyle,
    _meshes: &Assets<Mesh>,
    cuboid: &Handle<Mesh>,
    sphere: &Handle<Mesh>,
    hair: &Handle<StandardMaterial>,
) {
    let y = 1.88;
    match style {
        HairStyle::Spikes => {
            for i in 0..5 {
                let a = -0.3 + i as f32 * 0.15;
                root.spawn((
                    Mesh3d(cuboid.clone()),
                    MeshMaterial3d(hair.clone()),
                    Transform {
                        translation: Vec3::new(a, y + 0.08, -0.02),
                        rotation: Quat::from_rotation_z(a * 0.8) * Quat::from_rotation_x(-0.4),
                        scale: Vec3::new(0.08, 0.28, 0.08),
                    },
                ));
            }
        }
        HairStyle::TwinTails => {
            for s in [-1.0, 1.0] {
                root.spawn((
                    Mesh3d(sphere.clone()),
                    MeshMaterial3d(hair.clone()),
                    Transform {
                        translation: Vec3::new(s * 0.22, 1.7, -0.05),
                        scale: Vec3::new(0.12, 0.35, 0.12),
                        ..default()
                    },
                ));
            }
        }
        HairStyle::Buzz => {
            root.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(hair.clone()),
                Transform {
                    translation: Vec3::new(0.0, 1.8, 0.0),
                    scale: Vec3::new(0.26, 0.12, 0.26),
                    ..default()
                },
            ));
        }
        HairStyle::Long => {
            root.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(hair.clone()),
                Transform {
                    translation: Vec3::new(0.0, 1.5, -0.12),
                    scale: Vec3::new(0.28, 0.7, 0.12),
                    ..default()
                },
            ));
        }
        HairStyle::Ponytail => {
            root.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(hair.clone()),
                Transform {
                    translation: Vec3::new(0.0, 1.82, 0.0),
                    scale: Vec3::new(0.26, 0.16, 0.26),
                    ..default()
                },
            ));
            root.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(hair.clone()),
                Transform {
                    translation: Vec3::new(0.0, 1.55, -0.2),
                    rotation: Quat::from_rotation_x(0.5),
                    scale: Vec3::new(0.1, 0.45, 0.1),
                },
            ));
        }
        HairStyle::Messy => {
            root.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(hair.clone()),
                Transform {
                    translation: Vec3::new(-0.06, 1.84, 0.02),
                    scale: Vec3::new(0.28, 0.2, 0.24),
                    ..default()
                },
            ));
        }
        HairStyle::Bob => {
            root.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(hair.clone()),
                Transform {
                    translation: Vec3::new(0.0, 1.78, 0.0),
                    scale: Vec3::new(0.3, 0.22, 0.28),
                    ..default()
                },
            ));
        }
        HairStyle::Bandana => {
            root.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(hair.clone()),
                Transform {
                    translation: Vec3::new(0.0, 1.82, 0.02),
                    scale: Vec3::new(0.32, 0.08, 0.28),
                    ..default()
                },
            ));
        }
        HairStyle::Drills => {
            for s in [-1.0, 1.0] {
                root.spawn((
                    Mesh3d(cuboid.clone()),
                    MeshMaterial3d(hair.clone()),
                    Transform {
                        translation: Vec3::new(s * 0.28, 1.55, 0.0),
                        rotation: Quat::from_rotation_z(-s * 0.35),
                        scale: Vec3::new(0.14, 0.55, 0.14),
                    },
                ));
            }
        }
        HairStyle::Lightning => {
            root.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(hair.clone()),
                Transform {
                    translation: Vec3::new(0.1, 1.98, 0.0),
                    rotation: Quat::from_rotation_z(-0.45),
                    scale: Vec3::new(0.1, 0.42, 0.08),
                },
            ));
        }
    }
}

fn face_velocity(paused: Res<Paused>, mut q: Query<(&MoveVel, &mut Transform, &Pose), With<Player>>) {
    if paused.0 {
        return;
    }
    for (vel, mut tf, pose) in &mut q {
        if matches!(*pose, Pose::Shoot | Pose::Dunk | Pose::Celebrate) {
            continue;
        }
        let v = Vec3::new(vel.0.x, 0.0, vel.0.z);
        if v.length_squared() > 0.4 {
            let fwd = v.normalize();
            let target = tf.translation + fwd;
            tf.look_at(target, Vec3::Y);
        }
    }
}

fn animate_rigs(
    time: Res<Time>,
    paused: Res<Paused>,
    mut players: Query<(&MoveVel, &Pose, &mut PoseClock, &Rig, &Stamina)>,
    mut xforms: Query<&mut Transform, Without<Player>>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    for (vel, pose, mut clock, rig, _stam) in &mut players {
        clock.0 += dt;
        let t = clock.0;
        let spd = Vec3::new(vel.0.x, 0.0, vel.0.z).length();
        let run = (spd / 6.0).clamp(0.0, 1.6);
        let pump = t * (8.0 + run * 6.0);
        let ids = [rig.torso, rig.head, rig.l_arm, rig.r_arm, rig.l_leg, rig.r_leg];
        let Ok(mut parts) = xforms.get_many_mut(ids) else {
            continue;
        };
        let [torso, head, l_arm, r_arm, l_leg, r_leg] = &mut parts;
        match *pose {
            Pose::Idle => {
                torso.translation.y = 1.15 + t.sin() * 0.03;
                l_arm.rotation = Quat::from_rotation_x((t * 1.4).sin() * 0.08);
                r_arm.rotation = Quat::from_rotation_x((t * 1.4).cos() * 0.08);
                l_leg.rotation = Quat::IDENTITY;
                r_leg.rotation = Quat::IDENTITY;
            }
            Pose::Run | Pose::Sprint => {
                let amp = if *pose == Pose::Sprint { 0.7 } else { 0.5 };
                l_leg.rotation = Quat::from_rotation_x(pump.sin() * amp);
                r_leg.rotation = Quat::from_rotation_x(-pump.sin() * amp);
                l_arm.rotation = Quat::from_rotation_x(-pump.sin() * amp * 0.8);
                r_arm.rotation = Quat::from_rotation_x(pump.sin() * amp * 0.8);
                torso.rotation = Quat::from_rotation_x(-0.12 * run);
            }
            Pose::Shoot => {
                l_arm.rotation = Quat::from_rotation_x(-2.2);
                r_arm.rotation = Quat::from_rotation_x(-2.4);
                torso.translation.y = 1.15 + 0.25;
            }
            Pose::Dunk => {
                l_arm.rotation = Quat::from_rotation_x(-2.6);
                r_arm.rotation = Quat::from_rotation_x(-2.8);
                torso.translation.y = 1.35;
                torso.rotation = Quat::from_rotation_z(0.25);
            }
            Pose::Pass => {
                r_arm.rotation = Quat::from_rotation_y(-1.2) * Quat::from_rotation_x(-0.6);
            }
            Pose::Block => {
                l_arm.rotation = Quat::from_rotation_x(-2.8);
                r_arm.rotation = Quat::from_rotation_x(-2.8);
                torso.translation.y = 1.35;
            }
            Pose::Celebrate => {
                l_arm.rotation = Quat::from_rotation_x(-2.5);
                r_arm.rotation = Quat::from_rotation_x(-2.5);
                torso.translation.y = 1.15 + (t * 8.0).sin().abs() * 0.12;
            }
            Pose::Stumble => {
                torso.rotation = Quat::from_rotation_x(0.4);
            }
        }
        let bob = match *pose {
            Pose::Idle => (t * 2.4).sin() * 0.012,
            Pose::Run | Pose::Sprint => pump.sin().abs() * (0.016 + run * 0.01),
            Pose::Celebrate => (t * 8.0).sin().abs() * 0.03,
            Pose::Shoot | Pose::Dunk | Pose::Block => 0.01,
            _ => (t * 3.5).sin() * 0.008,
        };
        head.translation.y = 1.72 + bob;
    }
}

fn update_face_expr(
    mut players: Query<(&Pose, &FaceRig, &mut FaceExpr)>,
    mut xforms: Query<&mut Transform, Without<Player>>,
) {
    for (pose, face, mut expr) in &mut players {
        *expr = match *pose {
            Pose::Shoot | Pose::Block => FaceExpr::Focus,
            Pose::Celebrate => FaceExpr::Celebrate,
            Pose::Stumble => FaceExpr::Pain,
            Pose::Dunk => FaceExpr::Angry,
            _ => FaceExpr::Neutral,
        };
        let ids = [
            face.brow_l,
            face.brow_r,
            face.mouth,
            face.iris_l,
            face.iris_r,
            face.blush_l,
            face.blush_r,
        ];
        let Ok(mut parts) = xforms.get_many_mut(ids) else {
            continue;
        };
        let [brow_l, brow_r, mouth, iris_l, iris_r, blush_l, blush_r] = &mut parts;

        let (brow_y, brow_z, mouth_sy, iris_s, blush_s) = match *expr {
            FaceExpr::Neutral => (0.50, 0.0, 0.09, 0.175, 0.004),
            FaceExpr::Focus => (0.44, 0.22, 0.06, 0.13, 0.004),
            FaceExpr::Celebrate => (0.56, -0.18, 0.23, 0.20, 0.18),
            FaceExpr::Angry => (0.41, 0.38, 0.075, 0.23, 0.004),
            FaceExpr::Pain => (0.53, -0.28, 0.29, 0.12, 0.08),
        };

        brow_l.translation.y = brow_y;
        brow_r.translation.y = brow_y;
        brow_l.rotation = Quat::from_rotation_z(brow_z);
        brow_r.rotation = Quat::from_rotation_z(-brow_z);
        mouth.scale.y = mouth_sy;
        iris_l.scale = Vec3::splat(iris_s);
        iris_r.scale = Vec3::splat(iris_s);
        blush_l.scale = Vec3::splat(blush_s);
        blush_r.scale = Vec3::splat(blush_s);
    }
}

fn stamina_regen(
    time: Res<Time>,
    paused: Res<Paused>,
    mut q: Query<(&MoveVel, &Pose, &mut Stamina)>,
) {
    if paused.0 {
        return;
    }
    for (vel, pose, mut stam) in &mut q {
        let drain = if matches!(*pose, Pose::Sprint) || vel.0.length() > 7.0 {
            0.22
        } else if matches!(*pose, Pose::Dunk | Pose::Shoot) {
            0.08
        } else {
            -0.16
        };
        stam.0 = (stam.0 - drain * time.delta_secs()).clamp(0.05, 1.0);
    }
}

pub fn move_speed(ratings: &Ratings, sprint: bool, stamina: f32) -> f32 {
    speed_from_rating(ratings.speed, sprint && stamina > 0.12) * (0.65 + 0.35 * stamina)
}
