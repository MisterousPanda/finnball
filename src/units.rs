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
            (face_velocity, animate_rigs, stamina_regen).run_if(in_state(crate::states::AppState::Playing)),
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

    let sphere = meshes.add(Sphere::new(1.0));
    let capsule = meshes.add(Capsule3d::new(0.12, 0.38));
    let cuboid = meshes.add(Cuboid::new(1.0, 1.0, 1.0));

    let mut torso_id = Entity::PLACEHOLDER;
    let mut head_id = Entity::PLACEHOLDER;
    let mut l_arm = Entity::PLACEHOLDER;
    let mut r_arm = Entity::PLACEHOLDER;
    let mut l_leg = Entity::PLACEHOLDER;
    let mut r_leg = Entity::PLACEHOLDER;

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
                .id();
            // Anime eyes
            root.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(eye_w.clone()),
                Transform {
                    translation: Vec3::new(-0.08, 1.74, 0.18),
                    scale: Vec3::new(0.07, 0.09, 0.04),
                    ..default()
                },
            ));
            root.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(eye_w),
                Transform {
                    translation: Vec3::new(0.08, 1.74, 0.18),
                    scale: Vec3::new(0.07, 0.09, 0.04),
                    ..default()
                },
            ));
            root.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(eye_i.clone()),
                Transform {
                    translation: Vec3::new(-0.08, 1.735, 0.21),
                    scale: Vec3::splat(0.035),
                    ..default()
                },
            ));
            root.spawn((
                Mesh3d(sphere.clone()),
                MeshMaterial3d(eye_i),
                Transform {
                    translation: Vec3::new(0.08, 1.735, 0.21),
                    scale: Vec3::splat(0.035),
                    ..default()
                },
            ));
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

    commands.entity(root).insert(Rig {
        torso: torso_id,
        head: head_id,
        l_arm,
        r_arm,
        l_leg,
        r_leg,
    });

    if human {
        commands.entity(root).insert(Controlled);
    }

    let _ = PLAYER_RADIUS;
    root
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
    for (vel, pose, mut clock, rig, stam) in &mut players {
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
        head.translation.y = 1.72 + stam.0 * 0.0;
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
