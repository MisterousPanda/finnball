use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::relationship::RelatedSpawnerCommands;
use bevy::image::{ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::ball::{Ball, BallState, BucketEvent, Hold};
use crate::roster::{CharacterId, CharacterProfile, HairStyle, Kit, Side};
use crate::sim::{speed_from_rating, PLAYER_RADIUS};
use crate::states::Paused;

pub struct UnitsPlugin;

impl Plugin for UnitsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                (
                    face_velocity,
                    track_misses,
                    animate_rigs,
                    update_face_expr,
                    sway_hair,
                    apply_fire_look,
                )
                    .chain(),
                dress_decals,
                stamina_regen,
                separate_players,
                detect_cuts,
            )
                .run_if(in_state(crate::states::AppState::Playing)),
        );
    }
}

/// Visual "front" of every player is local -Z, which is also `Transform::forward()`
/// and the direction gameplay places the dribble in. Everything below is authored so
/// that positive `rotation_x` on a shoulder swings the arm forward and a negative
/// `rotation_x` on a knee folds the shin backward.
const FRONT: f32 = -1.0;

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

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pose {
    Idle,
    Run,
    Sprint,
    Shoot,
    Dunk,
    Pass,
    Block,
    /// Hands straight up in the shooter's face without leaving the floor
    /// (closeout stance). Movement is still allowed.
    Contest,
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
    pub l_elbow: Entity,
    pub r_elbow: Entity,
    pub l_knee: Entity,
    pub r_knee: Entity,
}

/// Face parts live as children of the head; this handle sits on the player root.
#[derive(Component)]
pub struct FaceRig {
    pub brow_l: Entity,
    pub brow_r: Entity,
    /// Centre lip bar, left/right corners and the dark inner mouth.
    pub mouth: Entity,
    pub mouth_l: Entity,
    pub mouth_r: Entity,
    pub mouth_in: Entity,
    /// Blink pivots (scale y) holding sclera + iris group.
    pub eye_l: Entity,
    pub eye_r: Entity,
    /// Iris groups (iris + pupil + highlight) that slide to look at the ball.
    pub look_l: Entity,
    pub look_r: Entity,
    pub blush_l: Entity,
    pub blush_r: Entity,
    pub sweat: Entity,
    pub vein: Entity,
}

#[derive(Component, Default, Clone, Copy)]
pub struct Heat {
    pub streak: u8,
}

impl Heat {
    pub fn on_fire(self) -> bool {
        self.streak >= 3
    }
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum FaceExpr {
    #[default]
    Neutral,
    Focus,
    Celebrate,
    Angry,
    Pain,
    /// Gassed: half-lidded eyes, panting mouth.
    Tired,
    /// Hang-head after a miss.
    Sad,
}

/// Per-player animation scratch state (blinks, landing crouch, hang-head, spin).
#[derive(Component)]
pub struct AnimState {
    /// Seconds until the next blink starts.
    pub blink_in: f32,
    /// Remaining duration of the current blink (0 = eyes open).
    pub blink_t: f32,
    /// Hang-head timer after a missed shot.
    pub sad: f32,
    /// Landing crouch timer after a jump pose ends.
    pub land: f32,
    pub prev_pose: Pose,
    /// Accumulated yaw from a 360 dunk approach.
    pub spin: f32,
    /// Per-player offset so idle cycles never sync up.
    pub phase: f32,
    /// Smoothed root-local velocity used for hair/arm lag.
    pub lag: Vec3,
}

impl Default for AnimState {
    fn default() -> Self {
        Self {
            blink_in: 2.0,
            blink_t: 0.0,
            sad: 0.0,
            land: 0.0,
            prev_pose: Pose::Idle,
            spin: 0.0,
            phase: 0.0,
            lag: Vec3::ZERO,
        }
    }
}

/// Hair pivot that lags behind the owner's velocity and jumps.
#[derive(Component)]
pub struct HairSway {
    pub owner: Entity,
    pub kind: SwayKind,
    pub base: Quat,
    pub lag: Vec3,
    pub prev_lift: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub enum SwayKind {
    /// Whole-hair lean opposite to acceleration.
    Root,
    /// Free-hanging tail that swings like a pendulum with the run cadence.
    Tail { side: f32 },
}

/// A jersey decal (number / name plate) waiting for its painted texture. Textures need
/// `Assets<Image>`, which `spawn_player` never receives, so the mesh + material are
/// attached one frame later by `dress_decals`.
#[derive(Component, Clone, Copy)]
pub struct JerseyDecal {
    pub id: CharacterId,
    pub back: bool,
}

/// Material pair swapped on `Heat::on_fire` (irises, shoe soles).
#[derive(Component)]
pub struct FireSwap {
    pub owner: Entity,
    pub cool: Handle<StandardMaterial>,
    pub hot: Handle<StandardMaterial>,
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

/// Shared primitive meshes for one player.
struct Prims {
    sphere: Handle<Mesh>,
    cuboid: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    cone: Handle<Mesh>,
    torus: Handle<Mesh>,
    torso: Handle<Mesh>,
    upper_arm: Handle<Mesh>,
    forearm: Handle<Mesh>,
    thigh: Handle<Mesh>,
    shin: Handle<Mesh>,
}

/// Materials for one player, created once and shared by every part.
struct Palette {
    skin: Handle<StandardMaterial>,
    jersey: Handle<StandardMaterial>,
    trim: Handle<StandardMaterial>,
    shorts: Handle<StandardMaterial>,
    hair: Handle<StandardMaterial>,
    hair_dark: Handle<StandardMaterial>,
    eye_white: Handle<StandardMaterial>,
    iris: Handle<StandardMaterial>,
    iris_hot: Handle<StandardMaterial>,
    pupil: Handle<StandardMaterial>,
    highlight: Handle<StandardMaterial>,
    brow: Handle<StandardMaterial>,
    mouth: Handle<StandardMaterial>,
    mouth_in: Handle<StandardMaterial>,
    blush: Handle<StandardMaterial>,
    sweat: Handle<StandardMaterial>,
    vein: Handle<StandardMaterial>,
    shoe_a: Handle<StandardMaterial>,
    shoe_b: Handle<StandardMaterial>,
    sole: Handle<StandardMaterial>,
    sole_hot: Handle<StandardMaterial>,
    sock: Handle<StandardMaterial>,
    sleeve: Handle<StandardMaterial>,
    tights: Handle<StandardMaterial>,
    tattoo: Handle<StandardMaterial>,
    accent: Handle<StandardMaterial>,
    shadow: Handle<StandardMaterial>,
}

fn part(
    s: &mut RelatedSpawnerCommands<ChildOf>,
    mesh: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    pos: Vec3,
    rot: Quat,
    scale: Vec3,
) -> Entity {
    s.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(mat.clone()),
        Transform {
            translation: pos,
            rotation: rot,
            scale,
        },
    ))
    .id()
}

fn lit(c: Color, rough: f32, glow: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: c,
        perceptual_roughness: rough,
        emissive: LinearRgba::from(c.to_linear()) * glow,
        ..default()
    }
}

fn unlit(c: Color, glow: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: c,
        emissive: LinearRgba::from(c.to_linear()) * glow,
        unlit: true,
        ..default()
    }
}

fn palette(materials: &mut Assets<StandardMaterial>, p: &CharacterProfile, kit: &Kit, side: Side) -> Palette {
    let jersey = side.primary();
    let trim = side.secondary();
    let jersey_lin = jersey.to_linear();
    let shorts_c = Color::from(trim.to_linear() * 0.82);
    let hair_lin = p.hair_color.to_linear();
    Palette {
        skin: materials.add(lit(p.skin, 0.68, 0.0)),
        jersey: materials.add(lit(jersey, 0.55, 0.15)),
        trim: materials.add(lit(trim, 0.5, 0.05)),
        shorts: materials.add(lit(shorts_c, 0.9, 0.02)),
        hair: materials.add(lit(p.hair_color, 0.55, 0.25)),
        hair_dark: materials.add(lit(Color::from(hair_lin * 0.55), 0.6, 0.1)),
        eye_white: materials.add(unlit(Color::WHITE, 0.0)),
        iris: materials.add(unlit(p.eye, 1.6)),
        iris_hot: materials.add(unlit(Color::srgb(1.0, 0.45, 0.08), 4.5)),
        pupil: materials.add(unlit(Color::srgb(0.04, 0.03, 0.06), 0.0)),
        highlight: materials.add(unlit(Color::WHITE, 2.0)),
        brow: materials.add(unlit(Color::from(hair_lin * 0.35), 0.0)),
        mouth: materials.add(unlit(Color::srgb(0.58, 0.16, 0.22), 0.0)),
        mouth_in: materials.add(unlit(Color::srgb(0.18, 0.04, 0.06), 0.0)),
        blush: materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 0.42, 0.55, 0.75),
            emissive: LinearRgba::new(0.55, 0.08, 0.16, 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        sweat: materials.add(StandardMaterial {
            base_color: Color::srgba(0.7, 0.9, 1.0, 0.85),
            emissive: LinearRgba::new(0.3, 0.5, 0.8, 1.0),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        }),
        vein: materials.add(unlit(Color::srgb(0.85, 0.12, 0.18), 0.6)),
        shoe_a: materials.add(StandardMaterial {
            base_color: kit.shoe_primary,
            metallic: 0.35,
            perceptual_roughness: 0.35,
            ..default()
        }),
        shoe_b: materials.add(lit(kit.shoe_secondary, 0.4, 0.35)),
        sole: materials.add(lit(Color::srgb(0.96, 0.96, 0.94), 0.6, 0.0)),
        sole_hot: materials.add(unlit(Color::srgb(1.0, 0.55, 0.12), 6.0)),
        sock: materials.add(lit(Color::WHITE, 0.9, 0.0)),
        sleeve: materials.add(lit(Color::from(jersey_lin * 0.35), 0.8, 0.05)),
        tights: materials.add(lit(Color::srgb(0.08, 0.07, 0.12), 0.85, 0.0)),
        tattoo: materials.add(lit(Color::srgb(0.12, 0.2, 0.28), 0.75, 0.0)),
        accent: materials.add(lit(p.accent, 0.5, 0.4)),
        shadow: materials.add(StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.02, 0.5),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    }
}

fn prims(meshes: &mut Assets<Mesh>) -> Prims {
    Prims {
        sphere: meshes.add(Sphere::new(1.0).mesh().ico(3).unwrap_or_else(|_| Sphere::new(1.0).mesh().uv(16, 10))),
        cuboid: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        cylinder: meshes.add(Cylinder::new(0.5, 1.0)),
        cone: meshes.add(Cone {
            radius: 0.5,
            height: 1.0,
        }),
        torus: meshes.add(Torus {
            minor_radius: 0.08,
            major_radius: 1.0,
        }),
        torso: meshes.add(Cuboid::new(0.5, 0.62, 0.3)),
        upper_arm: meshes.add(Capsule3d::new(0.072, 0.2)),
        forearm: meshes.add(Capsule3d::new(0.06, 0.2)),
        thigh: meshes.add(Capsule3d::new(0.1, 0.2)),
        shin: meshes.add(Capsule3d::new(0.08, 0.2)),
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
    let kit = id.kit();
    let ratings = Ratings::from(p);
    let scale = p.height_m / 1.88;
    let pal = palette(materials, &p, &kit, side);
    let pr = prims(meshes);

    let mut rig = Rig {
        torso: Entity::PLACEHOLDER,
        head: Entity::PLACEHOLDER,
        l_arm: Entity::PLACEHOLDER,
        r_arm: Entity::PLACEHOLDER,
        l_leg: Entity::PLACEHOLDER,
        r_leg: Entity::PLACEHOLDER,
        l_elbow: Entity::PLACEHOLDER,
        r_elbow: Entity::PLACEHOLDER,
        l_knee: Entity::PLACEHOLDER,
        r_knee: Entity::PLACEHOLDER,
    };
    let mut face = FaceRig {
        brow_l: Entity::PLACEHOLDER,
        brow_r: Entity::PLACEHOLDER,
        mouth: Entity::PLACEHOLDER,
        mouth_l: Entity::PLACEHOLDER,
        mouth_r: Entity::PLACEHOLDER,
        mouth_in: Entity::PLACEHOLDER,
        eye_l: Entity::PLACEHOLDER,
        eye_r: Entity::PLACEHOLDER,
        look_l: Entity::PLACEHOLDER,
        look_r: Entity::PLACEHOLDER,
        blush_l: Entity::PLACEHOLDER,
        blush_r: Entity::PLACEHOLDER,
        sweat: Entity::PLACEHOLDER,
        vein: Entity::PLACEHOLDER,
    };
    // Entities that need the root id (fire swaps, hair sway) are patched afterwards.
    let mut swaps: Vec<Entity> = Vec::new();
    let mut sways: Vec<(Entity, SwayKind, Quat)> = Vec::new();

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
            Heat::default(),
            BoxLine::default(),
            AnimState {
                phase: slot as f32 * 1.7 + if side == Side::Home { 0.0 } else { 0.9 },
                ..default()
            },
            Transform::from_translation(pos).with_scale(Vec3::splat(scale)),
            Visibility::default(),
            crate::court::ArenaRoot,
            DespawnOnExit(crate::states::AppState::Playing),
        ))
        .with_children(|root| {
            rig.torso = spawn_torso(root, &pr, &pal, id);
            spawn_shorts(root, &pr, &pal);
            let (head, f, sw) = spawn_head(root, &pr, &pal, &p, &kit, &mut swaps);
            rig.head = head;
            face = f;
            sways.extend(sw);
            // Contact shadow (the root never leaves y = 0; jumps are animated on the torso).
            part(
                root,
                &pr.cylinder,
                &pal.shadow,
                Vec3::new(0.0, 0.012, 0.0),
                Quat::IDENTITY,
                Vec3::new(0.9, 0.01, 0.9),
            );
            for sx in [-1.0f32, 1.0] {
                let (shoulder, elbow) = spawn_arm(root, &pr, &pal, &kit, sx);
                let (hip, knee) = spawn_leg(root, &pr, &pal, &kit, sx, &mut swaps);
                if sx < 0.0 {
                    rig.l_arm = shoulder;
                    rig.l_elbow = elbow;
                    rig.l_leg = hip;
                    rig.l_knee = knee;
                } else {
                    rig.r_arm = shoulder;
                    rig.r_elbow = elbow;
                    rig.r_leg = hip;
                    rig.r_knee = knee;
                }
            }
        })
        .id();

    commands.entity(root).insert((rig, face));
    for e in swaps {
        // The swap components were spawned with a placeholder owner; patch in the root.
        commands.entity(e).entry::<FireSwap>().and_modify(move |mut s| s.owner = root);
    }
    for (e, kind, base) in sways {
        commands.entity(e).insert(HairSway {
            owner: root,
            kind,
            base,
            lag: Vec3::ZERO,
            prev_lift: 0.0,
        });
    }

    if human {
        commands.entity(root).insert(Controlled);
    }

    let _ = PLAYER_RADIUS;
    root
}

fn spawn_torso(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    pr: &Prims,
    pal: &Palette,
    id: CharacterId,
) -> Entity {
    root.spawn((
        Mesh3d(pr.torso.clone()),
        MeshMaterial3d(pal.jersey.clone()),
        Transform::from_xyz(0.0, 1.15, 0.0),
        Visibility::default(),
    ))
    .with_children(|t| {
        // Shoulder mass, pecs, collar, straps, side piping, waistband, neck and the
        // decals all ride the torso so the whole upper body lifts on shots and dunks.
        part(
            t,
            &pr.cuboid,
            &pal.jersey,
            Vec3::new(0.0, 0.19, 0.0),
            Quat::IDENTITY,
            Vec3::new(0.57, 0.26, 0.318),
        );
        for sx in [-1.0f32, 1.0] {
            // chest plate under the jersey
            part(
                t,
                &pr.sphere,
                &pal.jersey,
                Vec3::new(sx * 0.115, 0.13, FRONT * 0.12),
                Quat::IDENTITY,
                Vec3::new(0.12, 0.075, 0.075),
            );
            // collar V piping
            part(
                t,
                &pr.cuboid,
                &pal.trim,
                Vec3::new(sx * 0.065, 0.235, FRONT * 0.163),
                Quat::from_rotation_z(-sx * 0.62),
                Vec3::new(0.028, 0.17, 0.012),
            );
            // shoulder straps
            part(
                t,
                &pr.cuboid,
                &pal.trim,
                Vec3::new(sx * 0.15, 0.26, 0.0),
                Quat::IDENTITY,
                Vec3::new(0.11, 0.14, 0.324),
            );
            // side piping
            part(
                t,
                &pr.cuboid,
                &pal.trim,
                Vec3::new(sx * 0.25, -0.03, 0.0),
                Quat::IDENTITY,
                Vec3::new(0.025, 0.53, 0.21),
            );
        }
        part(
            t,
            &pr.cuboid,
            &pal.trim,
            Vec3::new(0.0, -0.31, 0.0),
            Quat::IDENTITY,
            Vec3::new(0.51, 0.062, 0.312),
        );
        part(
            t,
            &pr.cylinder,
            &pal.skin,
            Vec3::new(0.0, 0.35, 0.0),
            Quat::IDENTITY,
            Vec3::new(0.16, 0.12, 0.16),
        );
        // Front number and back name plate: painted textures attached by `dress_decals`.
        t.spawn((
            JerseyDecal { id, back: false },
            Transform {
                translation: Vec3::new(0.0, -0.035, FRONT * 0.1625),
                rotation: Quat::from_rotation_y(std::f32::consts::PI),
                scale: Vec3::new(0.27, 0.27, 1.0),
            },
            Visibility::default(),
        ));
        t.spawn((
            JerseyDecal { id, back: true },
            Transform {
                translation: Vec3::new(0.0, 0.02, -FRONT * 0.1625),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(0.32, 0.36, 1.0),
            },
            Visibility::default(),
        ));
    })
    .id()
}

fn spawn_shorts(root: &mut RelatedSpawnerCommands<ChildOf>, pr: &Prims, pal: &Palette) {
    part(
        root,
        &pr.cuboid,
        &pal.shorts,
        Vec3::new(0.0, 0.72, 0.0),
        Quat::IDENTITY,
        Vec3::new(0.52, 0.34, 0.34),
    );
    for sx in [-1.0f32, 1.0] {
        part(
            root,
            &pr.cuboid,
            &pal.jersey,
            Vec3::new(sx * 0.265, 0.71, 0.0),
            Quat::IDENTITY,
            Vec3::new(0.018, 0.3, 0.13),
        );
    }
}

/// Head-local helpers: the head sphere is scaled by 0.24 so one unit is the head radius.
fn spawn_head(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    pr: &Prims,
    pal: &Palette,
    p: &CharacterProfile,
    kit: &Kit,
    swaps: &mut Vec<Entity>,
) -> (Entity, FaceRig, Vec<(Entity, SwayKind, Quat)>) {
    let mut face = FaceRig {
        brow_l: Entity::PLACEHOLDER,
        brow_r: Entity::PLACEHOLDER,
        mouth: Entity::PLACEHOLDER,
        mouth_l: Entity::PLACEHOLDER,
        mouth_r: Entity::PLACEHOLDER,
        mouth_in: Entity::PLACEHOLDER,
        eye_l: Entity::PLACEHOLDER,
        eye_r: Entity::PLACEHOLDER,
        look_l: Entity::PLACEHOLDER,
        look_r: Entity::PLACEHOLDER,
        blush_l: Entity::PLACEHOLDER,
        blush_r: Entity::PLACEHOLDER,
        sweat: Entity::PLACEHOLDER,
        vein: Entity::PLACEHOLDER,
    };
    let mut sways = Vec::new();
    let f = FRONT;
    let head = root
        .spawn((
            Mesh3d(pr.sphere.clone()),
            MeshMaterial3d(pal.skin.clone()),
            Transform {
                translation: Vec3::new(0.0, 1.72, 0.0),
                scale: Vec3::splat(0.24),
                ..default()
            },
        ))
        .with_children(|head| {
            // ears + nose
            for sx in [-1.0f32, 1.0] {
                part(
                    head,
                    &pr.sphere,
                    &pal.skin,
                    Vec3::new(sx * 0.95, -0.02, 0.04),
                    Quat::IDENTITY,
                    Vec3::new(0.12, 0.19, 0.11),
                );
            }
            part(
                head,
                &pr.sphere,
                &pal.skin,
                Vec3::new(0.0, -0.14, f * 0.96),
                Quat::IDENTITY,
                Vec3::new(0.07, 0.09, 0.07),
            );
            // Eyes: blink pivot -> sclera + look pivot -> iris, pupil, highlight
            for sx in [-1.0f32, 1.0] {
                let mut look = Entity::PLACEHOLDER;
                let mut iris = Entity::PLACEHOLDER;
                let eye = head
                    .spawn((
                        Transform::from_xyz(sx * 0.36, 0.08, f * 0.74),
                        Visibility::default(),
                    ))
                    .with_children(|e| {
                        part(
                            e,
                            &pr.sphere,
                            &pal.eye_white,
                            Vec3::ZERO,
                            Quat::IDENTITY,
                            Vec3::new(0.40, 0.46, 0.20),
                        );
                        look = e
                            .spawn((Transform::from_xyz(0.0, 0.0, f * 0.06), Visibility::default()))
                            .with_children(|l| {
                                iris = part(
                                    l,
                                    &pr.sphere,
                                    &pal.iris,
                                    Vec3::new(0.0, -0.01, f * 0.14),
                                    Quat::IDENTITY,
                                    Vec3::new(0.21, 0.24, 0.07),
                                );
                                part(
                                    l,
                                    &pr.sphere,
                                    &pal.pupil,
                                    Vec3::new(0.0, -0.02, f * 0.18),
                                    Quat::IDENTITY,
                                    Vec3::new(0.105, 0.125, 0.05),
                                );
                                part(
                                    l,
                                    &pr.sphere,
                                    &pal.highlight,
                                    Vec3::new(-0.07, 0.08, f * 0.21),
                                    Quat::IDENTITY,
                                    Vec3::new(0.06, 0.06, 0.035),
                                );
                            })
                            .id();
                    })
                    .id();
                if sx < 0.0 {
                    face.eye_l = eye;
                    face.look_l = look;
                } else {
                    face.eye_r = eye;
                    face.look_r = look;
                }
                swaps.push(iris);
                head.commands().entity(iris).insert(FireSwap {
                    owner: Entity::PLACEHOLDER,
                    cool: pal.iris.clone(),
                    hot: pal.iris_hot.clone(),
                });
            }
            // brows
            face.brow_l = part(
                head,
                &pr.cuboid,
                &pal.brow,
                Vec3::new(-0.36, 0.42, f * 0.80),
                Quat::IDENTITY,
                Vec3::new(0.40, 0.075, 0.10),
            );
            face.brow_r = part(
                head,
                &pr.cuboid,
                &pal.brow,
                Vec3::new(0.36, 0.42, f * 0.80),
                Quat::IDENTITY,
                Vec3::new(0.40, 0.075, 0.10),
            );
            // mouth: lip bar, two corners, dark inner
            face.mouth_in = part(
                head,
                &pr.sphere,
                &pal.mouth_in,
                Vec3::new(0.0, -0.43, f * 0.83),
                Quat::IDENTITY,
                Vec3::new(0.14, 0.01, 0.06),
            );
            face.mouth = part(
                head,
                &pr.cuboid,
                &pal.mouth,
                Vec3::new(0.0, -0.41, f * 0.87),
                Quat::IDENTITY,
                Vec3::new(0.30, 0.065, 0.06),
            );
            face.mouth_l = part(
                head,
                &pr.cuboid,
                &pal.mouth,
                Vec3::new(-0.17, -0.41, f * 0.855),
                Quat::IDENTITY,
                Vec3::new(0.12, 0.06, 0.06),
            );
            face.mouth_r = part(
                head,
                &pr.cuboid,
                &pal.mouth,
                Vec3::new(0.17, -0.41, f * 0.855),
                Quat::IDENTITY,
                Vec3::new(0.12, 0.06, 0.06),
            );
            // blush marks (hidden until celebrating)
            face.blush_l = part(
                head,
                &pr.sphere,
                &pal.blush,
                Vec3::new(-0.56, -0.2, f * 0.64),
                Quat::IDENTITY,
                Vec3::splat(0.001),
            );
            face.blush_r = part(
                head,
                &pr.sphere,
                &pal.blush,
                Vec3::new(0.56, -0.2, f * 0.64),
                Quat::IDENTITY,
                Vec3::splat(0.001),
            );
            // sweat drop on the temple, anger vein above the brow (both hidden at rest)
            face.sweat = part(
                head,
                &pr.sphere,
                &pal.sweat,
                Vec3::new(0.66, 0.42, f * 0.55),
                Quat::IDENTITY,
                Vec3::splat(0.001),
            );
            face.vein = part(
                head,
                &pr.cuboid,
                &pal.vein,
                Vec3::new(-0.55, 0.62, f * 0.62),
                Quat::from_rotation_z(0.7) * Quat::from_rotation_y(-0.5),
                Vec3::splat(0.001),
            );
            if kit.headband {
                part(
                    head,
                    &pr.torus,
                    &pal.accent,
                    Vec3::new(0.0, 0.3, 0.0),
                    Quat::from_rotation_x(-f * 0.12),
                    Vec3::new(0.99, 1.0, 0.99),
                );
            }
            // Hair hangs off a sway pivot so the whole style lags and bounces.
            let hair_root = head
                .spawn((Transform::IDENTITY, Visibility::default()))
                .with_children(|h| {
                    sways.extend(spawn_hair(h, p.hair, pr, pal));
                })
                .id();
            sways.push((hair_root, SwayKind::Root, Quat::IDENTITY));
        })
        .id();
    (head, face, sways)
}

fn spawn_arm(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    pr: &Prims,
    pal: &Palette,
    kit: &Kit,
    sx: f32,
) -> (Entity, Entity) {
    let sleeved = kit.arm_sleeve.on(sx);
    let arm_skin = if sleeved { &pal.sleeve } else { &pal.skin };
    let mut elbow_id = Entity::PLACEHOLDER;
    let shoulder = root
        .spawn((
            Transform::from_xyz(sx * 0.34, 1.45, 0.0),
            Visibility::default(),
        ))
        .with_children(|a| {
            // deltoid cap
            part(
                a,
                &pr.sphere,
                &pal.jersey,
                Vec3::new(sx * 0.01, -0.005, 0.0),
                Quat::IDENTITY,
                Vec3::new(0.115, 0.1, 0.115),
            );
            part(
                a,
                &pr.upper_arm,
                arm_skin,
                Vec3::new(0.0, -0.15, 0.0),
                Quat::IDENTITY,
                Vec3::ONE,
            );
            if kit.tattoo.on(sx) && !sleeved {
                part(
                    a,
                    &pr.cylinder,
                    &pal.tattoo,
                    Vec3::new(0.0, -0.13, 0.0),
                    Quat::from_rotation_z(sx * 0.12),
                    Vec3::new(0.152, 0.07, 0.152),
                );
            }
            elbow_id = a
                .spawn((Transform::from_xyz(0.0, -0.3, 0.0), Visibility::default()))
                .with_children(|e| {
                    part(
                        e,
                        &pr.forearm,
                        arm_skin,
                        Vec3::new(0.0, -0.14, 0.0),
                        Quat::IDENTITY,
                        Vec3::ONE,
                    );
                    part(
                        e,
                        &pr.cylinder,
                        &pal.trim,
                        Vec3::new(0.0, -0.23, 0.0),
                        Quat::IDENTITY,
                        Vec3::new(0.14, 0.05, 0.14),
                    );
                    part(
                        e,
                        &pr.sphere,
                        &pal.skin,
                        Vec3::new(0.0, -0.31, FRONT * 0.01),
                        Quat::IDENTITY,
                        Vec3::new(0.065, 0.078, 0.05),
                    );
                })
                .id();
        })
        .id();
    (shoulder, elbow_id)
}

fn spawn_leg(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    pr: &Prims,
    pal: &Palette,
    kit: &Kit,
    sx: f32,
    swaps: &mut Vec<Entity>,
) -> (Entity, Entity) {
    let f = FRONT;
    let thigh_mat = if kit.tights { &pal.tights } else { &pal.skin };
    let knee_sleeve = kit.knee_sleeve.on(sx);
    let (sock_y, sock_h, stripe_y) = if kit.high_socks {
        (-0.255, 0.19, -0.19)
    } else {
        (-0.3, 0.1, -0.268)
    };
    let mut knee_id = Entity::PLACEHOLDER;
    let hip = root
        .spawn((
            Transform::from_xyz(sx * 0.15, 0.8, 0.0),
            Visibility::default(),
        ))
        .with_children(|l| {
            part(
                l,
                &pr.thigh,
                thigh_mat,
                Vec3::new(0.0, -0.19, 0.0),
                Quat::IDENTITY,
                Vec3::ONE,
            );
            knee_id = l
                .spawn((Transform::from_xyz(0.0, -0.38, 0.0), Visibility::default()))
                .with_children(|k| {
                    // knee (bare or sleeved)
                    part(
                        k,
                        &pr.sphere,
                        if knee_sleeve { &pal.sleeve } else { &pal.skin },
                        Vec3::ZERO,
                        Quat::IDENTITY,
                        if knee_sleeve {
                            Vec3::new(0.097, 0.125, 0.097)
                        } else {
                            Vec3::splat(0.085)
                        },
                    );
                    part(
                        k,
                        &pr.shin,
                        &pal.skin,
                        Vec3::new(0.0, -0.16, 0.0),
                        Quat::IDENTITY,
                        Vec3::ONE,
                    );
                    // calf bulge on the back of the shin
                    part(
                        k,
                        &pr.sphere,
                        &pal.skin,
                        Vec3::new(0.0, -0.1, -f * 0.045),
                        Quat::IDENTITY,
                        Vec3::new(0.075, 0.1, 0.07),
                    );
                    part(
                        k,
                        &pr.cuboid,
                        &pal.sock,
                        Vec3::new(0.0, sock_y, 0.0),
                        Quat::IDENTITY,
                        Vec3::new(0.17, sock_h, 0.17),
                    );
                    part(
                        k,
                        &pr.cuboid,
                        &pal.trim,
                        Vec3::new(0.0, stripe_y, 0.0),
                        Quat::IDENTITY,
                        Vec3::new(0.176, 0.026, 0.176),
                    );
                    // shoe body, sole, outer swoosh, laces
                    part(
                        k,
                        &pr.cuboid,
                        &pal.shoe_a,
                        Vec3::new(0.0, -0.365, f * 0.05),
                        Quat::IDENTITY,
                        Vec3::new(0.18, 0.11, 0.32),
                    );
                    let sole = part(
                        k,
                        &pr.cuboid,
                        &pal.sole,
                        Vec3::new(0.0, -0.405, f * 0.05),
                        Quat::IDENTITY,
                        Vec3::new(0.19, 0.03, 0.34),
                    );
                    swaps.push(sole);
                    k.commands().entity(sole).insert(FireSwap {
                        owner: Entity::PLACEHOLDER,
                        cool: pal.sole.clone(),
                        hot: pal.sole_hot.clone(),
                    });
                    part(
                        k,
                        &pr.cuboid,
                        &pal.shoe_b,
                        Vec3::new(sx * 0.095, -0.36, f * 0.03),
                        Quat::from_rotation_x(f * 0.35),
                        Vec3::new(0.01, 0.045, 0.2),
                    );
                    part(
                        k,
                        &pr.cuboid,
                        &pal.shoe_b,
                        Vec3::new(0.0, -0.305, f * 0.1),
                        Quat::from_rotation_x(-f * 0.25),
                        Vec3::new(0.1, 0.014, 0.13),
                    );
                })
                .id();
        })
        .id();
    (hip, knee_id)
}

pub(crate) fn jersey_number(id: CharacterId) -> u8 {
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

/// First name as printed on the name plate.
pub(crate) fn plate_name(id: CharacterId) -> &'static str {
    id.profile().name.split(' ').next().unwrap_or("")
}

/// Seven-segment digit built from cuboids (used for floating score pops).
pub(crate) fn spawn_digit(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    cuboid: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    digit: u8,
    origin: Vec3,
    mirror: bool,
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
            let off = if mirror {
                Vec3::new(-off.x, off.y, off.z)
            } else {
                *off
            };
            root.spawn((
                Mesh3d(cuboid.clone()),
                MeshMaterial3d(mat.clone()),
                Transform {
                    translation: origin + off,
                    scale: *scale,
                    ..default()
                },
            ));
        }
    }
}

/// `+` and `!` glyphs on the same grid as `spawn_digit`.
pub(crate) fn spawn_symbol(
    root: &mut RelatedSpawnerCommands<ChildOf>,
    cuboid: &Handle<Mesh>,
    mat: &Handle<StandardMaterial>,
    ch: char,
    origin: Vec3,
) {
    let th = 0.016;
    let dz = 0.02;
    let segs: &[(Vec3, Vec3)] = match ch {
        '+' => &[
            (Vec3::ZERO, Vec3::new(0.06, th, dz)),
            (Vec3::ZERO, Vec3::new(th, 0.06, dz)),
        ],
        '!' => &[
            (Vec3::new(0.0, 0.02, 0.0), Vec3::new(th, 0.11, dz)),
            (Vec3::new(0.0, -0.076, 0.0), Vec3::new(th, th, dz)),
        ],
        _ => &[],
    };
    for (off, scale) in segs {
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

// ---------------------------------------------------------------------------
// Jersey decals: 5x7 pixel font painted into small RGBA textures.
// ---------------------------------------------------------------------------

/// 5-wide, 7-tall glyph rows (MSB = leftmost column). Unknown characters are blank.
pub(crate) fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        _ => [0; 7],
    }
}

pub(crate) const DECAL_SIZE: u32 = 64;

/// Width in pixels of `text` rendered at `scale` (1 px gap between glyphs).
pub(crate) fn text_width(text: &str, scale: u32) -> u32 {
    let n = text.chars().count() as u32;
    if n == 0 {
        0
    } else {
        n * 5 * scale + (n - 1) * scale
    }
}

fn stamp_text(mask: &mut [bool], w: u32, text: &str, x0: i32, y0: i32, scale: u32) {
    let mut x = x0;
    for c in text.chars() {
        let g = glyph(c);
        for (row, bits) in g.iter().enumerate() {
            for col in 0..5 {
                if bits & (1 << (4 - col)) == 0 {
                    continue;
                }
                for dy in 0..scale as i32 {
                    for dx in 0..scale as i32 {
                        let px = x + col as i32 * scale as i32 + dx;
                        let py = y0 + row as i32 * scale as i32 + dy;
                        if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < w {
                            mask[(py as u32 * w + px as u32) as usize] = true;
                        }
                    }
                }
            }
        }
        x += (6 * scale) as i32;
    }
}

/// Paints white outlined text into a square RGBA canvas. `lines` are (text, y, scale).
pub(crate) fn paint_decal(lines: &[(&str, i32, u32)]) -> Vec<u8> {
    let w = DECAL_SIZE;
    let mut mask = vec![false; (w * w) as usize];
    for (text, y, scale) in lines {
        let tw = text_width(text, *scale) as i32;
        stamp_text(&mut mask, w, text, (w as i32 - tw) / 2, *y, *scale);
    }
    let mut rgba = vec![0u8; (w * w * 4) as usize];
    for y in 0..w as i32 {
        for x in 0..w as i32 {
            let i = (y as u32 * w + x as u32) as usize;
            let (r, g, b, a) = if mask[i] {
                (255, 255, 255, 255)
            } else {
                let mut edge = false;
                for dy in -2..=2i32 {
                    for dx in -2..=2i32 {
                        if dx.abs() + dy.abs() > 3 {
                            continue;
                        }
                        let nx = x + dx;
                        let ny = y + dy;
                        if nx >= 0
                            && ny >= 0
                            && nx < w as i32
                            && ny < w as i32
                            && mask[(ny as u32 * w + nx as u32) as usize]
                        {
                            edge = true;
                        }
                    }
                }
                if edge {
                    (14, 10, 26, 235)
                } else {
                    (0, 0, 0, 0)
                }
            };
            rgba[i * 4] = r;
            rgba[i * 4 + 1] = g;
            rgba[i * 4 + 2] = b;
            rgba[i * 4 + 3] = a;
        }
    }
    rgba
}

pub(crate) fn decal_pixels(id: CharacterId, back: bool) -> Vec<u8> {
    let number = jersey_number(id).to_string();
    if back {
        paint_decal(&[(plate_name(id), 3, 2), (number.as_str(), 25, 4)])
    } else {
        paint_decal(&[(number.as_str(), 18, 4)])
    }
}

#[derive(Default)]
struct DecalCache {
    quad: Option<Handle<Mesh>>,
    mats: HashMap<(CharacterId, bool), Handle<StandardMaterial>>,
}

fn dress_decals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: Local<DecalCache>,
    pending: Query<(Entity, &JerseyDecal), Without<Mesh3d>>,
) {
    if pending.is_empty() {
        return;
    }
    let quad = cache
        .quad
        .get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0)))
        .clone();
    for (e, decal) in &pending {
        let key = (decal.id, decal.back);
        let mat = cache
            .mats
            .entry(key)
            .or_insert_with(|| {
                let mut img = Image::new(
                    Extent3d {
                        width: DECAL_SIZE,
                        height: DECAL_SIZE,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    decal_pixels(decal.id, decal.back),
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::RENDER_WORLD,
                );
                img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::nearest());
                let tex = images.add(img);
                materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    base_color_texture: Some(tex),
                    emissive: LinearRgba::new(0.35, 0.35, 0.4, 1.0),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    ..default()
                })
            })
            .clone();
        commands
            .entity(e)
            .insert((Mesh3d(quad.clone()), MeshMaterial3d(mat)));
    }
}

// ---------------------------------------------------------------------------
// Hair
// ---------------------------------------------------------------------------

/// Skull cap that fills the scalp behind the hairline without covering the face.
fn hair_cap(h: &mut RelatedSpawnerCommands<ChildOf>, pr: &Prims, mat: &Handle<StandardMaterial>, size: f32) {
    part(
        h,
        &pr.sphere,
        mat,
        Vec3::new(0.0, 0.42, -FRONT * 0.18),
        Quat::IDENTITY,
        Vec3::new(0.98 * size, 0.78 * size, 0.98 * size),
    );
}

/// Angled bang piece over the forehead. `sx` picks the side it sweeps toward.
fn bangs(h: &mut RelatedSpawnerCommands<ChildOf>, pr: &Prims, mat: &Handle<StandardMaterial>, sx: f32, len: f32) {
    part(
        h,
        &pr.cuboid,
        mat,
        Vec3::new(sx * 0.32, 0.72 - len * 0.2, FRONT * 0.86),
        Quat::from_rotation_z(-sx * 0.45) * Quat::from_rotation_x(FRONT * 0.18),
        Vec3::new(0.42, len, 0.16),
    );
}

/// Builds the hairstyle in head-local units (1.0 = head radius) and returns any tail
/// pivots that should swing with a pendulum motion.
fn spawn_hair(
    h: &mut RelatedSpawnerCommands<ChildOf>,
    style: HairStyle,
    pr: &Prims,
    pal: &Palette,
) -> Vec<(Entity, SwayKind, Quat)> {
    let f = FRONT;
    let hair = &pal.hair;
    let dark = &pal.hair_dark;
    let mut tails = Vec::new();
    match style {
        HairStyle::Spikes => {
            hair_cap(h, pr, hair, 1.0);
            for i in 0..5 {
                let a = -0.6 + i as f32 * 0.3;
                part(
                    h,
                    &pr.cone,
                    hair,
                    Vec3::new(a * 0.9, 0.95 + (1.0 - a.abs()) * 0.15, f * 0.25 + a.abs() * 0.15),
                    Quat::from_rotation_z(-a * 0.9) * Quat::from_rotation_x(-f * 0.55),
                    Vec3::new(0.5, 1.25 - a.abs() * 0.3, 0.5),
                );
            }
        }
        HairStyle::TwinTails => {
            hair_cap(h, pr, hair, 1.0);
            bangs(h, pr, hair, -1.0, 0.55);
            bangs(h, pr, hair, 1.0, 0.55);
            for sx in [-1.0f32, 1.0] {
                let tie = h
                    .spawn((
                        Transform::from_xyz(sx * 0.9, 0.55, -f * 0.1),
                        Visibility::default(),
                    ))
                    .with_children(|t| {
                        part(t, &pr.sphere, &pal.accent, Vec3::ZERO, Quat::IDENTITY, Vec3::splat(0.2));
                        part(
                            t,
                            &pr.sphere,
                            hair,
                            Vec3::new(sx * 0.12, -0.75, 0.0),
                            Quat::IDENTITY,
                            Vec3::new(0.34, 0.85, 0.34),
                        );
                    })
                    .id();
                tails.push((tie, SwayKind::Tail { side: sx }, Quat::from_rotation_z(-sx * 0.28)));
            }
        }
        HairStyle::Buzz => {
            hair_cap(h, pr, dark, 1.02);
        }
        HairStyle::Long => {
            hair_cap(h, pr, hair, 1.02);
            bangs(h, pr, hair, -1.0, 0.7);
            bangs(h, pr, hair, 1.0, 0.7);
            for sx in [-1.0f32, 1.0] {
                part(
                    h,
                    &pr.cuboid,
                    hair,
                    Vec3::new(sx * 0.9, -0.35, 0.05),
                    Quat::from_rotation_z(-sx * 0.05),
                    Vec3::new(0.3, 1.9, 0.75),
                );
            }
            let curtain = h
                .spawn((Transform::from_xyz(0.0, 0.5, -f * 0.75), Visibility::default()))
                .with_children(|c| {
                    part(
                        c,
                        &pr.cuboid,
                        hair,
                        Vec3::new(0.0, -1.2, 0.15),
                        Quat::IDENTITY,
                        Vec3::new(1.5, 2.9, 0.5),
                    );
                })
                .id();
            tails.push((curtain, SwayKind::Tail { side: 0.0 }, Quat::IDENTITY));
        }
        HairStyle::Ponytail => {
            hair_cap(h, pr, hair, 1.0);
            bangs(h, pr, hair, 1.0, 0.6);
            let tail = h
                .spawn((Transform::from_xyz(0.0, 0.85, -f * 0.7), Visibility::default()))
                .with_children(|t| {
                    part(t, &pr.sphere, &pal.accent, Vec3::ZERO, Quat::IDENTITY, Vec3::splat(0.22));
                    part(
                        t,
                        &pr.cuboid,
                        hair,
                        Vec3::new(0.0, -0.75, 0.2),
                        Quat::from_rotation_x(-f * 0.25),
                        Vec3::new(0.38, 1.7, 0.36),
                    );
                    part(
                        t,
                        &pr.cone,
                        hair,
                        Vec3::new(0.0, -1.85, 0.45),
                        Quat::from_rotation_x(std::f32::consts::PI + f * 0.25),
                        Vec3::new(0.42, 0.6, 0.42),
                    );
                })
                .id();
            tails.push((tail, SwayKind::Tail { side: 0.0 }, Quat::from_rotation_x(-f * 0.45)));
        }
        HairStyle::Messy => {
            hair_cap(h, pr, hair, 1.04);
            bangs(h, pr, hair, -1.0, 0.75);
            for (x, y, z, rz, rx) in [
                (-0.6, 0.9, 0.2, 0.7, -0.3),
                (0.55, 0.95, 0.1, -0.6, -0.5),
                (0.1, 1.05, -0.45, 0.15, 0.6),
                (-0.3, 0.85, -0.7, 0.4, 0.9),
            ] {
                part(
                    h,
                    &pr.cuboid,
                    hair,
                    Vec3::new(x, y, z),
                    Quat::from_rotation_z(rz) * Quat::from_rotation_x(rx),
                    Vec3::new(0.35, 0.8, 0.3),
                );
            }
        }
        HairStyle::Bob => {
            hair_cap(h, pr, hair, 1.06);
            // straight fringe
            part(
                h,
                &pr.cuboid,
                hair,
                Vec3::new(0.0, 0.6, f * 0.84),
                Quat::from_rotation_x(f * 0.1),
                Vec3::new(1.2, 0.42, 0.28),
            );
            for sx in [-1.0f32, 1.0] {
                part(
                    h,
                    &pr.cuboid,
                    hair,
                    Vec3::new(sx * 0.92, -0.05, -f * 0.05),
                    Quat::from_rotation_z(sx * 0.08),
                    Vec3::new(0.34, 1.3, 1.2),
                );
            }
            part(
                h,
                &pr.cuboid,
                hair,
                Vec3::new(0.0, -0.05, -f * 0.9),
                Quat::IDENTITY,
                Vec3::new(1.6, 1.3, 0.4),
            );
        }
        HairStyle::Afro => {
            part(
                h,
                &pr.sphere,
                hair,
                Vec3::new(0.0, 0.42, -f * 0.1),
                Quat::IDENTITY,
                Vec3::new(1.38, 1.22, 1.38),
            );
            // bandana under the afro with a knot at the back
            part(
                h,
                &pr.torus,
                &pal.accent,
                Vec3::new(0.0, 0.18, 0.0),
                Quat::from_rotation_x(-f * 0.1),
                Vec3::new(1.02, 1.6, 1.02),
            );
            part(
                h,
                &pr.cuboid,
                &pal.accent,
                Vec3::new(0.0, 0.05, -f * 1.05),
                Quat::from_rotation_x(-f * 0.4),
                Vec3::new(0.34, 0.5, 0.14),
            );
        }
        HairStyle::Drills => {
            hair_cap(h, pr, hair, 1.0);
            bangs(h, pr, hair, 1.0, 0.5);
            for sx in [-1.0f32, 1.0] {
                let drill = h
                    .spawn((
                        Transform::from_xyz(sx * 0.95, 0.3, -f * 0.05),
                        Visibility::default(),
                    ))
                    .with_children(|d| {
                        part(
                            d,
                            &pr.sphere,
                            hair,
                            Vec3::new(sx * 0.15, -0.55, 0.0),
                            Quat::from_rotation_z(sx * 0.3),
                            Vec3::new(0.36, 0.75, 0.36),
                        );
                        part(
                            d,
                            &pr.cone,
                            dark,
                            Vec3::new(sx * 0.25, -1.6, 0.0),
                            Quat::from_rotation_x(std::f32::consts::PI),
                            Vec3::new(0.6, 1.3, 0.6),
                        );
                    })
                    .id();
                tails.push((drill, SwayKind::Tail { side: sx }, Quat::IDENTITY));
            }
        }
        HairStyle::Mohawk => {
            // shaved sides, tall zig-zag crest
            part(
                h,
                &pr.sphere,
                dark,
                Vec3::new(0.0, 0.42, -f * 0.18),
                Quat::IDENTITY,
                Vec3::new(0.99, 0.78, 0.99),
            );
            for (i, (z, tilt)) in [(-0.55, 0.65), (-0.15, 0.25), (0.25, -0.2), (0.6, -0.6)]
                .iter()
                .enumerate()
            {
                let zig = if i % 2 == 0 { 0.25 } else { -0.25 };
                part(
                    h,
                    &pr.cone,
                    hair,
                    Vec3::new(0.0, 1.35, f * z),
                    Quat::from_rotation_x(-f * tilt) * Quat::from_rotation_z(zig),
                    Vec3::new(0.42, 1.6, 0.55),
                );
            }
        }
    }
    tails
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

fn separate_players(paused: Res<Paused>, mut q: Query<(Entity, &mut Transform), With<Player>>) {
    if paused.0 {
        return;
    }
    let mut pts: Vec<(Entity, Vec3)> = q.iter().map(|(e, t)| (e, t.translation)).collect();
    let min = PLAYER_RADIUS * 2.05;
    for i in 0..pts.len() {
        for j in (i + 1)..pts.len() {
            let a = pts[i].1;
            let b = pts[j].1;
            let mut d = Vec3::new(a.x - b.x, 0.0, a.z - b.z);
            let len = d.length();
            if len < 0.001 {
                d = Vec3::new(0.12, 0.0, 0.08);
            }
            if len < min {
                let push = d.normalize_or_zero() * ((min - len.max(0.001)) * 0.5);
                pts[i].1 += push;
                pts[j].1 -= push;
            }
        }
    }
    for (e, mut tf) in &mut q {
        if let Some((_, p)) = pts.iter().find(|(id, _)| *id == e) {
            let (x, z) = crate::sim::clamp_to_court(p.x, p.z, 0.55);
            tf.translation.x = x;
            tf.translation.z = z;
        }
    }
}

fn detect_cuts(
    paused: Res<Paused>,
    mut prev: Local<HashMap<Entity, Vec3>>,
    q: Query<(Entity, &Transform, &MoveVel), With<Player>>,
    mut cuts: MessageWriter<crate::gameplay::CutSqueak>,
) {
    if paused.0 {
        return;
    }
    for (e, tf, vel) in &q {
        let now = Vec3::new(vel.0.x, 0.0, vel.0.z);
        if let Some(old) = prev.get(&e).copied() {
            let a = old.length();
            let b = now.length();
            if a > 3.4 && b > 3.4 {
                let dot = old.normalize_or_zero().dot(now.normalize_or_zero());
                if dot < 0.12 {
                    cuts.write(crate::gameplay::CutSqueak {
                        pos: tf.translation,
                    });
                }
            }
        }
        prev.insert(e, now);
    }
    prev.retain(|e, _| q.get(*e).is_ok());
}

/// Who has the ball, where, and for which side.
fn holder_info(
    ball: &Query<(&Transform, &BallState), With<Ball>>,
    players: impl Iterator<Item = (Entity, Vec3, Side)>,
) -> Option<(Entity, Vec3, Side)> {
    let (_, state) = ball.single().ok()?;
    let h = state.holder?;
    players.into_iter().find(|(e, _, _)| *e == h)
}

/// Defenders square up to the ball handler instead of turning their back on him.
fn face_velocity(
    paused: Res<Paused>,
    ball: Query<(&Transform, &BallState), With<Ball>>,
    mut q: Query<(Entity, &Player, &MoveVel, &mut Transform, &Pose), Without<Ball>>,
) {
    if paused.0 {
        return;
    }
    let holder = holder_info(
        &ball,
        q.iter().map(|(e, p, _, t, _)| (e, t.translation, p.side)),
    );
    for (e, p, vel, mut tf, pose) in &mut q {
        if matches!(
            *pose,
            Pose::Shoot | Pose::Dunk | Pose::Celebrate | Pose::Block
        ) {
            continue;
        }
        let v = Vec3::new(vel.0.x, 0.0, vel.0.z);
        let mut target = None;
        if let Some((h, hpos, hside)) = holder {
            if h != e && hside != p.side {
                let to = Vec3::new(hpos.x - tf.translation.x, 0.0, hpos.z - tf.translation.z);
                if to.length_squared() < 5.5 * 5.5 && to.length_squared() > 0.01 && v.length() < 5.0 {
                    target = Some(tf.translation + to.normalize());
                }
            }
        }
        if target.is_none() && v.length_squared() > 0.4 {
            target = Some(tf.translation + v.normalize());
        }
        if let Some(t) = target {
            let want = Transform::from_translation(tf.translation)
                .looking_at(t, Vec3::Y)
                .rotation;
            tf.rotation = tf.rotation.slerp(want, 0.35);
        }
    }
}

/// Marks the shooter for a hang-head when a shot dies without a bucket.
fn track_misses(
    ball: Query<&BallState, With<Ball>>,
    mut buckets: MessageReader<BucketEvent>,
    mut last: Local<Option<Entity>>,
    mut anim: Query<&mut AnimState>,
) {
    let scored = buckets.read().count() > 0;
    let Ok(state) = ball.single() else {
        return;
    };
    if state.hold == Hold::Shot {
        if let Some(s) = state.shooter {
            *last = Some(s);
        }
        return;
    }
    if let Some(shooter) = last.take() {
        if !scored {
            if let Ok(mut a) = anim.get_mut(shooter) {
                a.sad = 1.6;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn animate_rigs(
    time: Res<Time>,
    paused: Res<Paused>,
    ball: Query<(&Transform, &BallState), With<Ball>>,
    mut players: Query<
        (
            Entity,
            &mut Transform,
            &MoveVel,
            &Pose,
            &mut PoseClock,
            &Rig,
            &Stamina,
            &Player,
            &Ratings,
            &mut AnimState,
        ),
        Without<Ball>,
    >,
    mut xforms: Query<&mut Transform, (Without<Player>, Without<Ball>)>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let roster: Vec<(Entity, Vec3, Side)> = players
        .iter()
        .map(|(e, t, _, _, _, _, _, p, _, _)| (e, t.translation, p.side))
        .collect();
    let ball_info = ball.single().ok().map(|(t, s)| (t.translation, s.holder));
    let holder = holder_info(&ball, roster.iter().copied());

    for (entity, mut ptf, vel, pose, mut clock, rig, stam, player, ratings, mut anim) in
        &mut players
    {
        clock.0 += dt;
        let t = clock.0;
        let tp = now + anim.phase;
        let base_scale = ptf.scale.x;
        let spd = Vec3::new(vel.0.x, 0.0, vel.0.z).length();
        let run = (spd / 6.0).clamp(0.0, 1.6);
        let pump = t * (8.0 + run * 6.0);
        let holding = matches!(ball_info, Some((_, Some(h))) if h == entity);
        let inv = ptf.rotation.inverse();
        let local = |w: Vec3| inv * (w - ptf.translation) / base_scale;
        let vel_local = inv * vel.0;
        let lag = anim.lag;
        anim.lag = lag + (vel_local - lag) * (1.0 - (-7.0 * dt).exp());
        // Ball in the handler's frame: height drives the dribbling arm, x picks the hand.
        let ball_local = ball_info.map(|(bp, _)| local(bp));
        let ball_h = ball_local
            .map(|b| ((b.y - 0.18) / 0.75).clamp(0.0, 1.0))
            .unwrap_or(0.5);
        let right_hand = ball_local.map(|b| b.x >= -0.05).unwrap_or(true);
        // Nearest opponent (for defensive stance / ball protection)
        let nearest_opp = roster
            .iter()
            .filter(|(e, _, s)| *e != entity && *s != player.side)
            .map(|(_, p, _)| p.distance(ptf.translation))
            .fold(f32::INFINITY, f32::min);
        let defending = !holding
            && matches!(holder, Some((_, _, s)) if s != player.side)
            && holder.map(|(_, p, _)| p.distance(ptf.translation) < 3.4).unwrap_or(false);
        let tired = ((0.38 - stam.0) / 0.3).clamp(0.0, 1.0);
        let sad = (anim.sad / 1.6).clamp(0.0, 1.0);

        // Pose transitions: landing crouch after any jump pose.
        if anim.prev_pose != *pose {
            if matches!(anim.prev_pose, Pose::Shoot | Pose::Dunk | Pose::Block) {
                anim.land = 0.32;
            }
            if *pose == Pose::Dunk {
                anim.spin = 0.0;
            }
            anim.prev_pose = *pose;
        }
        anim.land = (anim.land - dt).max(0.0);
        anim.sad = (anim.sad - dt).max(0.0);
        let land = (anim.land / 0.32).clamp(0.0, 1.0);

        let ids = [
            rig.torso,
            rig.head,
            rig.l_arm,
            rig.r_arm,
            rig.l_leg,
            rig.r_leg,
            rig.l_elbow,
            rig.r_elbow,
            rig.l_knee,
            rig.r_knee,
        ];
        let Ok(mut parts) = xforms.get_many_mut(ids) else {
            continue;
        };
        let [torso, head, l_arm, r_arm, l_leg, r_leg, l_elbow, r_elbow, l_knee, r_knee] =
            &mut parts;

        // Forward = -Z. Positive rotation_x swings a hanging limb forward.
        let splay = |sx: f32| Quat::from_rotation_z(sx * 0.14);
        let fwd = Quat::from_rotation_x;
        let knee = |c: f32| Quat::from_rotation_x(-c);
        let lean = |c: f32| Quat::from_rotation_x(-c);
        let sway = |c: f32| Quat::from_rotation_z(c);

        let mut torso_rot = Quat::IDENTITY;
        let mut torso_y = 1.15;
        let mut torso_x = 0.0;
        let mut arm_l = splay(-1.0) * fwd((tp * 1.4).sin() * 0.06);
        let mut arm_r = splay(1.0) * fwd((tp * 1.4).cos() * 0.06);
        let mut elbow_l = fwd(0.35);
        let mut elbow_r = fwd(0.35);
        let mut leg_l = Quat::IDENTITY;
        let mut leg_r = Quat::IDENTITY;
        let (mut knee_l, mut knee_r);
        let mut head_rot: Option<Quat> = None;
        let mut track_ball = true;
        let mut stretch = 0.0;
        let mut spin_dt = 0.0;

        let dribble = |arm: &mut Quat, elbow: &mut Quat, sx: f32| {
            *arm = splay(sx) * fwd(0.25 + ball_h * 0.55) * sway(-sx * 0.12);
            *elbow = fwd(0.15 + ball_h * 0.55);
        };
        let protect = |arm: &mut Quat, elbow: &mut Quat, sx: f32| {
            *arm = splay(sx) * fwd(0.85) * sway(sx * 0.55);
            *elbow = fwd(0.7);
        };

        match *pose {
            Pose::Idle => {
                // breathing, weight shift, ready stance
                torso_y = 1.15 + (tp * 2.4).sin() * 0.012;
                torso_x = (tp * 0.8).sin() * 0.02;
                torso_rot = sway((tp * 0.8).sin() * 0.03);
                leg_l = sway(-(tp * 0.8).sin() * 0.04);
                leg_r = sway(-(tp * 0.8).sin() * 0.04);
                knee_l = knee(0.18);
                knee_r = knee(0.18);
                if holding {
                    if right_hand {
                        dribble(&mut arm_r, &mut elbow_r, 1.0);
                        if nearest_opp < 1.8 {
                            protect(&mut arm_l, &mut elbow_l, -1.0);
                        } else {
                            arm_l = splay(-1.0) * fwd(0.55);
                            elbow_l = fwd(1.1);
                        }
                    } else {
                        dribble(&mut arm_l, &mut elbow_l, -1.0);
                        if nearest_opp < 1.8 {
                            protect(&mut arm_r, &mut elbow_r, 1.0);
                        } else {
                            arm_r = splay(1.0) * fwd(0.55);
                            elbow_r = fwd(1.1);
                        }
                    }
                } else if defending {
                    // wide stance, knees bent, arms out, torso low
                    let shuffle = (tp * 6.0).sin() * 0.05;
                    knee_l = knee(0.62);
                    knee_r = knee(0.62);
                    leg_l = fwd(0.28) * sway(-0.22 + shuffle);
                    leg_r = fwd(0.28) * sway(0.22 + shuffle);
                    torso_y = 1.02;
                    torso_rot = lean(0.22);
                    arm_l = splay(-1.0) * fwd(0.55) * sway(-1.05);
                    arm_r = splay(1.0) * fwd(0.55) * sway(1.05);
                    elbow_l = fwd(0.45);
                    elbow_r = fwd(0.45);
                } else {
                    arm_l = splay(-1.0) * fwd(0.35);
                    arm_r = splay(1.0) * fwd(0.35);
                    elbow_l = fwd(1.0);
                    elbow_r = fwd(1.0);
                }
                if tired > 0.0 && !holding {
                    // hands on knees, heaving
                    let heave = (tp * 5.0).sin() * 0.02 * tired;
                    torso_rot = lean(0.42 * tired);
                    torso_y = 1.15 - 0.06 * tired + heave;
                    arm_l = arm_l.slerp(splay(-1.0) * fwd(0.95), tired);
                    arm_r = arm_r.slerp(splay(1.0) * fwd(0.95), tired);
                    elbow_l = elbow_l.slerp(fwd(0.1), tired);
                    elbow_r = elbow_r.slerp(fwd(0.1), tired);
                    knee_l = knee(0.18 + 0.3 * tired);
                    knee_r = knee(0.18 + 0.3 * tired);
                }
                if sad > 0.0 && !holding {
                    torso_rot = torso_rot * lean(0.18 * sad);
                    arm_l = arm_l.slerp(splay(-1.0) * fwd(0.05), sad);
                    arm_r = arm_r.slerp(splay(1.0) * fwd(0.05), sad);
                    elbow_l = elbow_l.slerp(fwd(0.15), sad);
                    elbow_r = elbow_r.slerp(fwd(0.15), sad);
                    head_rot = Some(Quat::from_rotation_x(-0.55 * sad));
                    track_ball = false;
                }
            }
            Pose::Run | Pose::Sprint => {
                let backpedal = vel_local.z > 0.6 && spd > 0.8;
                let lateral = vel_local.x.abs() > vel_local.z.abs() * 1.4 && spd > 0.8;
                let mut amp = if *pose == Pose::Sprint { 0.8 } else { 0.55 };
                if backpedal {
                    amp *= 0.55;
                }
                let swing = pump.sin();
                if lateral {
                    // defensive slide: legs open and close sideways
                    let slide = pump.sin() * 0.28;
                    leg_l = sway(-0.22 - slide) * fwd(0.15);
                    leg_r = sway(0.22 - slide) * fwd(0.15);
                    knee_l = knee(0.55);
                    knee_r = knee(0.55);
                    arm_l = splay(-1.0) * fwd(0.55) * sway(-0.95);
                    arm_r = splay(1.0) * fwd(0.55) * sway(0.95);
                    elbow_l = fwd(0.45);
                    elbow_r = fwd(0.45);
                    torso_y = 1.05 + swing.abs() * 0.015;
                    torso_rot = lean(0.18);
                } else {
                    leg_l = fwd(swing * amp);
                    leg_r = fwd(-swing * amp);
                    // Knee folds hardest while the leg recovers from behind.
                    let fold_l = 0.2 + 0.95 * (-(pump - 0.7).sin()).max(0.0) * amp;
                    let fold_r = 0.2 + 0.95 * ((pump - 0.7).sin()).max(0.0) * amp;
                    knee_l = knee(fold_l);
                    knee_r = knee(fold_r);
                    // arms counter-swing the legs, shoulders counter-rotate the hips
                    arm_l = splay(-1.0) * fwd(-swing * amp * 0.9 + 0.2);
                    arm_r = splay(1.0) * fwd(swing * amp * 0.9 + 0.2);
                    elbow_l = fwd(1.15 - tired * 0.6);
                    elbow_r = fwd(1.15 - tired * 0.6);
                    torso_rot = Quat::from_rotation_y(swing * 0.14 * amp) * lean(0.14 * run + 0.2 * tired);
                    torso_y = 1.15 + swing.abs() * 0.02;
                    if backpedal {
                        torso_rot = lean(-0.1);
                        arm_l = splay(-1.0) * fwd(0.4) * sway(-0.5);
                        arm_r = splay(1.0) * fwd(0.4) * sway(0.5);
                        elbow_l = fwd(0.8);
                        elbow_r = fwd(0.8);
                        torso_y = 1.1;
                    }
                }
                if holding {
                    if right_hand {
                        dribble(&mut arm_r, &mut elbow_r, 1.0);
                        if nearest_opp < 1.6 {
                            protect(&mut arm_l, &mut elbow_l, -1.0);
                        }
                    } else {
                        dribble(&mut arm_l, &mut elbow_l, -1.0);
                        if nearest_opp < 1.6 {
                            protect(&mut arm_r, &mut elbow_r, 1.0);
                        }
                    }
                }
            }
            Pose::Shoot => {
                let k = (t / 0.3).clamp(0.0, 1.0);
                arm_l = splay(-1.0) * fwd(2.3 + k * 0.25);
                arm_r = splay(1.0) * fwd(2.45 + k * 0.3);
                // Elbows extend through the release, wrist snaps last
                elbow_l = fwd(1.5 - k * 1.15);
                elbow_r = fwd(1.6 - k * 1.45);
                knee_l = knee(0.6 * (1.0 - k) + 0.15 * k);
                knee_r = knee(0.6 * (1.0 - k) + 0.15 * k);
                leg_l = fwd(-0.12 * k);
                leg_r = fwd(-0.12 * k);
                torso_y = 1.15 + 0.28 * k;
                torso_rot = lean(0.05);
                stretch = 0.06 * (k * std::f32::consts::PI).sin();
                head_rot = Some(Quat::from_rotation_x(0.35));
                track_ball = false;
            }
            Pose::Dunk => {
                let k = (t / 0.55).clamp(0.0, 1.0);
                let tuck = 1.0 - k;
                arm_l = splay(-1.0) * fwd(2.7);
                arm_r = splay(1.0) * fwd(2.9) * sway(0.15);
                elbow_l = fwd(0.35 + tuck * 0.6);
                elbow_r = fwd(0.2 + tuck * 0.9);
                knee_l = knee(1.1 + tuck * 0.4);
                knee_r = knee(0.7 + tuck * 0.5);
                leg_l = fwd(0.5);
                leg_r = fwd(-0.2);
                torso_y = 1.38 + tuck * 0.05;
                torso_rot = Quat::from_rotation_z(0.2) * lean(0.1);
                stretch = 0.07 * tuck;
                head_rot = Some(Quat::from_rotation_x(0.45));
                track_ball = false;
                // Elite dunkers 360 on the way up.
                if ratings.dunk >= 88.0 && t < 0.55 {
                    spin_dt = dt * std::f32::consts::TAU / 0.55;
                }
            }
            Pose::Pass => {
                let k = (t / 0.2).clamp(0.0, 1.0);
                arm_l = splay(-1.0) * fwd(1.1 + k * 0.5);
                arm_r = splay(1.0) * fwd(1.1 + k * 0.5);
                elbow_l = fwd(1.2 - k * 1.0);
                elbow_r = fwd(1.2 - k * 1.0);
                knee_l = knee(0.3);
                knee_r = knee(0.3);
                torso_rot = lean(0.12);
            }
            Pose::Block => {
                arm_l = splay(-1.0) * fwd(3.0) * sway(-0.2);
                arm_r = splay(1.0) * fwd(3.0) * sway(0.2);
                elbow_l = fwd(0.1);
                elbow_r = fwd(0.1);
                knee_l = knee(0.35);
                knee_r = knee(0.35);
                leg_l = fwd(-0.15);
                leg_r = fwd(-0.15);
                torso_y = 1.38;
                stretch = 0.06;
                head_rot = Some(Quat::from_rotation_x(0.5));
                track_ball = false;
            }
            Pose::Contest => {
                // Closeout: wide, low base with both arms reaching straight up —
                // the "wall" the shooter sees before the release.
                let reach = (tp * 9.0).sin() * 0.04;
                arm_l = splay(-1.0) * fwd(2.85 + reach) * sway(-0.3);
                arm_r = splay(1.0) * fwd(2.95 - reach) * sway(0.3);
                elbow_l = fwd(0.12);
                elbow_r = fwd(0.12);
                knee_l = knee(0.5);
                knee_r = knee(0.5);
                leg_l = fwd(0.2) * sway(-0.24);
                leg_r = fwd(0.2) * sway(0.24);
                torso_y = 1.1;
                torso_rot = lean(0.1);
                stretch = 0.04;
            }
            Pose::Celebrate => {
                let bounce = (t * 8.0).sin();
                match player.id.kit().celebration {
                    0 => {
                        // fist pump
                        arm_r = splay(1.0) * fwd(2.2 + bounce * 0.35) * sway(0.25);
                        elbow_r = fwd(1.6);
                        arm_l = splay(-1.0) * fwd(0.4);
                        elbow_l = fwd(0.9);
                        torso_rot = Quat::from_rotation_y(-0.2) * sway(-0.1);
                    }
                    1 => {
                        // double-bicep flex
                        arm_l = splay(-1.0) * fwd(1.55) * sway(-1.25);
                        arm_r = splay(1.0) * fwd(1.55) * sway(1.25);
                        elbow_l = fwd(2.3 + bounce * 0.1);
                        elbow_r = fwd(2.3 + bounce * 0.1);
                        torso_rot = sway(bounce * 0.08) * lean(-0.08);
                    }
                    _ => {
                        // point to the crowd, other hand on hip
                        arm_r = splay(1.0) * fwd(1.75) * sway(0.5 + bounce * 0.05);
                        elbow_r = fwd(0.05);
                        arm_l = splay(-1.0) * fwd(0.25) * sway(-0.55);
                        elbow_l = fwd(1.5);
                        torso_rot = Quat::from_rotation_y(-0.3);
                        head_rot = Some(Quat::from_rotation_y(-0.5) * Quat::from_rotation_x(0.2));
                        track_ball = false;
                    }
                }
                knee_l = knee(0.25 + bounce.abs() * 0.3);
                knee_r = knee(0.25 + bounce.abs() * 0.3);
                torso_y = 1.15 + bounce.abs() * 0.14;
                if head_rot.is_none() {
                    head_rot = Some(Quat::from_rotation_x(0.25));
                    track_ball = false;
                }
            }
            Pose::Stumble => {
                torso_rot = Quat::from_rotation_x(0.42);
                arm_l = splay(-1.0) * fwd(1.2) * sway(-0.4);
                arm_r = splay(1.0) * fwd(0.9) * sway(0.5);
                elbow_l = fwd(0.6);
                elbow_r = fwd(0.6);
                knee_l = knee(0.55);
                knee_r = knee(0.35);
                leg_l = fwd(0.3);
                head_rot = Some(Quat::from_rotation_x(0.3) * Quat::from_rotation_z(0.2));
                track_ball = false;
            }
        }

        // Landing crouch after a jump.
        if land > 0.0 {
            let c = (land * std::f32::consts::PI).sin();
            knee_l = knee_l * knee(0.5 * c);
            knee_r = knee_r * knee(0.5 * c);
            torso_y -= 0.1 * c;
            torso_rot = torso_rot * lean(0.12 * c);
            stretch -= 0.07 * c;
        }

        // Smooth toward targets so pose switches never pop.
        let k = 1.0 - (-18.0 * dt).exp();
        torso.translation.y += (torso_y - torso.translation.y) * k;
        torso.translation.x += (torso_x - torso.translation.x) * k;
        torso.rotation = torso.rotation.slerp(torso_rot, k);
        torso.scale = Vec3::splat(1.0 + (tp * 2.4).sin() * 0.008);
        l_arm.rotation = l_arm.rotation.slerp(arm_l, k);
        r_arm.rotation = r_arm.rotation.slerp(arm_r, k);
        l_elbow.rotation = l_elbow.rotation.slerp(elbow_l, k);
        r_elbow.rotation = r_elbow.rotation.slerp(elbow_r, k);
        l_leg.rotation = l_leg.rotation.slerp(leg_l, k);
        r_leg.rotation = r_leg.rotation.slerp(leg_r, k);
        l_knee.rotation = l_knee.rotation.slerp(knee_l, k);
        r_knee.rotation = r_knee.rotation.slerp(knee_r, k);

        // Squash & stretch on the root; spin on dunk approach.
        let sy = base_scale * (1.0 + stretch);
        ptf.scale.y += (sy - ptf.scale.y) * k;
        if spin_dt > 0.0 {
            let step = spin_dt.min(std::f32::consts::TAU - anim.spin).max(0.0);
            anim.spin += step;
            ptf.rotation = Quat::from_rotation_y(step) * ptf.rotation;
        }

        let bob = match *pose {
            Pose::Idle => (tp * 2.4).sin() * 0.012,
            Pose::Run | Pose::Sprint => pump.sin().abs() * (0.016 + run * 0.01),
            Pose::Celebrate => (t * 8.0).sin().abs() * 0.03,
            Pose::Shoot | Pose::Dunk | Pose::Block => 0.01,
            _ => (t * 3.5).sin() * 0.008,
        };
        // Head rides the torso lift (shots, dunks, blocks) instead of staying planted.
        head.translation.y = 1.72 + bob + (torso.translation.y - 1.15);
        head.translation.x = torso.translation.x;

        // Head tracking: look at the ball, clamped to a believable neck range.
        let target_rot = if let Some(r) = head_rot {
            r
        } else if let (true, Some(b)) = (track_ball, ball_local) {
            let to = b - Vec3::new(0.0, 1.72, 0.0);
            let horiz = Vec3::new(to.x, 0.0, to.z).length().max(0.05);
            let yaw = (-to.x).atan2(-to.z).clamp(-1.15, 1.15);
            let pitch = to.y.atan2(horiz).clamp(-0.55, 0.6);
            Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch)
        } else {
            Quat::IDENTITY
        };
        let tired_nod = Quat::from_rotation_x(-0.35 * tired * (1.0 - holding as u8 as f32));
        let hk = 1.0 - (-10.0 * dt).exp();
        head.rotation = head.rotation.slerp(target_rot * tired_nod, hk);
    }
}

fn update_face_expr(
    time: Res<Time>,
    paused: Res<Paused>,
    ball: Query<(&Transform, &BallState), With<Ball>>,
    mut players: Query<
        (
            &Transform,
            &Pose,
            &FaceRig,
            &mut FaceExpr,
            &Heat,
            &Stamina,
            &mut AnimState,
            &Rig,
        ),
        With<Player>,
    >,
    mut xforms: Query<&mut Transform, (Without<Player>, Without<Ball>)>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let ball_pos = ball.single().ok().map(|(t, _)| t.translation);
    for (ptf, pose, face, mut expr, heat, stam, mut anim, rig) in &mut players {
        *expr = match *pose {
            Pose::Shoot | Pose::Block | Pose::Contest | Pose::Pass => FaceExpr::Focus,
            Pose::Celebrate => FaceExpr::Celebrate,
            Pose::Stumble => FaceExpr::Pain,
            Pose::Dunk => FaceExpr::Angry,
            Pose::Sprint => FaceExpr::Focus,
            _ if anim.sad > 0.0 => FaceExpr::Sad,
            _ if stam.0 < 0.3 => FaceExpr::Tired,
            _ if heat.on_fire() => FaceExpr::Angry,
            _ => FaceExpr::Neutral,
        };

        // Blink clock
        anim.blink_in -= dt;
        if anim.blink_in <= 0.0 {
            anim.blink_t = 0.13;
            anim.blink_in = 2.4 + ((now * 7.3 + anim.phase).sin() * 0.5 + 0.5) * 2.6;
        }
        anim.blink_t = (anim.blink_t - dt).max(0.0);
        let blink = if anim.blink_t > 0.0 {
            (anim.blink_t / 0.13 * std::f32::consts::PI).sin()
        } else {
            0.0
        };

        // Head-local ball direction for the iris slide.
        let head_rot = xforms.get(rig.head).map(|t| t.rotation).unwrap_or(Quat::IDENTITY);
        let look = ball_pos
            .map(|bp| {
                let inv = (ptf.rotation * head_rot).inverse();
                let d = inv * (bp - (ptf.translation + ptf.rotation * Vec3::new(0.0, 1.72 * ptf.scale.y, 0.0)));
                let horiz = Vec3::new(d.x, 0.0, d.z).length().max(0.05);
                let yaw = (-d.x).atan2(-d.z).clamp(-0.9, 0.9);
                let pitch = d.y.atan2(horiz).clamp(-0.7, 0.7);
                Vec2::new(-yaw * 0.11, pitch * 0.07)
            })
            .unwrap_or(Vec2::ZERO);

        let ids = [
            face.brow_l,
            face.brow_r,
            face.mouth,
            face.mouth_l,
            face.mouth_r,
            face.mouth_in,
            face.eye_l,
            face.eye_r,
            face.look_l,
            face.look_r,
            face.blush_l,
            face.blush_r,
            face.sweat,
            face.vein,
        ];
        let Ok(mut parts) = xforms.get_many_mut(ids) else {
            continue;
        };
        let [brow_l, brow_r, mouth, mouth_l, mouth_r, mouth_in, eye_l, eye_r, look_l, look_r, blush_l, blush_r, sweat, vein] =
            &mut parts;

        // (brow_y, brow_angle: + = inner ends down, lid, iris scale, blush,
        //  mouth bar width, corner angle: + = smile, inner mouth (w, h))
        struct Face {
            brow_y: f32,
            brow_a: f32,
            lid: f32,
            iris: f32,
            blush: f32,
            bar: f32,
            corner: f32,
            inner: Vec2,
        }
        let f = match *expr {
            FaceExpr::Neutral => Face {
                brow_y: 0.42,
                brow_a: 0.0,
                lid: 1.0,
                iris: 1.0,
                blush: 0.0,
                bar: 0.30,
                corner: 0.05,
                inner: Vec2::new(0.0, 0.0),
            },
            FaceExpr::Focus => Face {
                brow_y: 0.36,
                brow_a: 0.22,
                lid: 0.72,
                iris: 0.8,
                blush: 0.0,
                bar: 0.22,
                corner: -0.15,
                inner: Vec2::new(0.0, 0.0),
            },
            FaceExpr::Celebrate => Face {
                brow_y: 0.50,
                brow_a: -0.2,
                lid: 0.6,
                iris: 1.15,
                blush: 1.0,
                bar: 0.34,
                corner: 0.6,
                inner: Vec2::new(0.2, 0.11),
            },
            FaceExpr::Angry => Face {
                brow_y: 0.32,
                brow_a: 0.42,
                lid: 0.85,
                iris: 0.7,
                blush: 0.0,
                bar: 0.30,
                corner: -0.5,
                inner: Vec2::new(0.22, 0.17),
            },
            FaceExpr::Pain => Face {
                brow_y: 0.47,
                brow_a: -0.3,
                lid: 0.45,
                iris: 0.75,
                blush: 0.4,
                bar: 0.13,
                corner: 0.0,
                inner: Vec2::new(0.14, 0.18),
            },
            FaceExpr::Tired => Face {
                brow_y: 0.40,
                brow_a: -0.12,
                lid: 0.62,
                iris: 0.95,
                blush: 0.35,
                bar: 0.26,
                corner: -0.25,
                inner: Vec2::new(0.13, 0.11),
            },
            FaceExpr::Sad => Face {
                brow_y: 0.45,
                brow_a: -0.32,
                lid: 0.8,
                iris: 0.9,
                blush: 0.0,
                bar: 0.26,
                corner: -0.45,
                inner: Vec2::new(0.0, 0.0),
            },
        };
        let on_fire = heat.on_fire();
        let k = 1.0 - (-14.0 * dt).exp();
        let ease = |cur: &mut f32, target: f32| *cur += (target - *cur) * k;

        ease(&mut brow_l.translation.y, f.brow_y);
        ease(&mut brow_r.translation.y, f.brow_y);
        brow_l.rotation = brow_l.rotation.slerp(Quat::from_rotation_z(-f.brow_a), k);
        brow_r.rotation = brow_r.rotation.slerp(Quat::from_rotation_z(f.brow_a), k);

        ease(&mut mouth.scale.x, f.bar);
        let corner_lift = f.corner.sin() * 0.05;
        mouth_l.translation.x = -(f.bar * 0.5 + 0.045);
        mouth_r.translation.x = f.bar * 0.5 + 0.045;
        ease(&mut mouth_l.translation.y, -0.41 + corner_lift);
        ease(&mut mouth_r.translation.y, -0.41 + corner_lift);
        mouth_l.rotation = mouth_l.rotation.slerp(Quat::from_rotation_z(-f.corner), k);
        mouth_r.rotation = mouth_r.rotation.slerp(Quat::from_rotation_z(f.corner), k);
        let shout = if *pose == Pose::Dunk { 1.3 } else { 1.0 };
        ease(&mut mouth_in.scale.x, (f.inner.x * shout).max(0.001));
        ease(&mut mouth_in.scale.y, (f.inner.y * shout).max(0.001));
        mouth_in.translation.y = -0.41 - f.inner.y * 0.5;

        // Eyelids: expression lid × blink; pain squeezes one eye shut.
        let lid = f.lid * (1.0 - blink * 0.92);
        let lid_l = if *expr == FaceExpr::Pain { lid * 0.25 } else { lid };
        eye_l.scale.y = lid_l.max(0.05);
        eye_r.scale.y = lid.max(0.05);
        let iris = f.iris * if on_fire { 1.2 } else { 1.0 };
        look_l.scale = look_l.scale.lerp(Vec3::splat(iris), k);
        look_r.scale = look_r.scale.lerp(Vec3::splat(iris), k);
        let look_off = Vec3::new(look.x, look.y, FRONT * 0.06);
        look_l.translation = look_l.translation.lerp(look_off, k);
        look_r.translation = look_r.translation.lerp(look_off, k);

        let blush_s = Vec3::new(0.17, 0.10, 0.06) * f.blush.max(0.006);
        blush_l.scale = blush_l.scale.lerp(blush_s, k);
        blush_r.scale = blush_r.scale.lerp(blush_s, k);

        // Sweat: grows as stamina drains, slides down the temple and resets.
        let sweat_amt = ((0.62 - stam.0) / 0.5).clamp(0.0, 1.0);
        let slide = ((now * 1.3 + anim.phase) % 1.0).clamp(0.0, 1.0);
        sweat.scale = Vec3::new(0.09, 0.14 + slide * 0.04, 0.09) * sweat_amt.max(0.006);
        sweat.translation.y = 0.42 - slide * 0.3;

        // Anger vein pops when angry or on fire.
        let vein_on = *expr == FaceExpr::Angry || on_fire;
        let vein_s = if vein_on {
            Vec3::new(0.2, 0.06, 0.06) * (1.0 + (now * 9.0).sin() * 0.1)
        } else {
            Vec3::splat(0.001)
        };
        vein.scale = vein.scale.lerp(vein_s, k);
    }
}

/// Hair lags behind acceleration and bounces on jumps; tails swing with the stride.
fn sway_hair(
    time: Res<Time>,
    paused: Res<Paused>,
    players: Query<(&Transform, &MoveVel, &Pose, &Rig, &AnimState), (With<Player>, Without<HairSway>)>,
    torsos: Query<&Transform, (Without<HairSway>, Without<Player>)>,
    mut hair: Query<(&mut Transform, &mut HairSway)>,
) {
    if paused.0 {
        return;
    }
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    for (mut tf, mut sway) in &mut hair {
        let Ok((ptf, vel, pose, rig, anim)) = players.get(sway.owner) else {
            continue;
        };
        let local = ptf.rotation.inverse() * vel.0;
        let lag = sway.lag;
        sway.lag = lag + (local - lag) * (1.0 - (-5.0 * dt).exp());
        let lift = torsos.get(rig.torso).map(|t| t.translation.y - 1.15).unwrap_or(0.0);
        let lift_v = (lift - sway.prev_lift) / dt.max(1e-4);
        sway.prev_lift = lift;
        let spd = Vec3::new(local.x, 0.0, local.z).length();
        let run = (spd / 6.0).clamp(0.0, 1.5);
        let cadence = now * (8.0 + run * 6.0) + anim.phase;
        let flutter = (cadence * 1.0).sin() * 0.06 * run + (now * 23.0).sin() * 0.015 * run;
        match sway.kind {
            SwayKind::Root => {
                // moving forward (-Z) trails the hair back (+Z); jumps compress / stretch it
                let tilt_x = (-sway.lag.z * 0.045).clamp(-0.35, 0.35) + flutter;
                let tilt_z = (sway.lag.x * 0.045).clamp(-0.3, 0.3);
                let target = Quat::from_rotation_x(tilt_x) * Quat::from_rotation_z(tilt_z);
                tf.rotation = tf.rotation.slerp(target, 1.0 - (-12.0 * dt).exp());
                let bounce = (-lift_v * 0.05).clamp(-0.1, 0.1);
                tf.translation.y += (bounce - tf.translation.y) * (1.0 - (-8.0 * dt).exp());
            }
            SwayKind::Tail { side } => {
                let swing = (cadence * 0.5).sin() * (0.12 + run * 0.28);
                let drag = (sway.lag.z * 0.06).clamp(-0.5, 0.5);
                let celebrate = if *pose == Pose::Celebrate {
                    (now * 8.0).sin() * 0.3
                } else {
                    0.0
                };
                let jump = (-lift_v * 0.12).clamp(-0.4, 0.4);
                let target = sway.base
                    * Quat::from_rotation_x(-FRONT * (drag + swing * 0.4 + jump + celebrate))
                    * Quat::from_rotation_z(swing * (0.6 + side.abs() * 0.4) + side * 0.05 * (cadence * 0.5).cos());
                tf.rotation = tf.rotation.slerp(target, 1.0 - (-9.0 * dt).exp());
            }
        }
    }
}

/// Swaps iris / sole materials to the hot variant while the owner is on fire.
fn apply_fire_look(
    players: Query<&Heat, With<Player>>,
    mut swaps: Query<(&FireSwap, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    for (swap, mut mat) in &mut swaps {
        let Ok(heat) = players.get(swap.owner) else {
            continue;
        };
        let want = if heat.on_fire() { &swap.hot } else { &swap.cool };
        if mat.0 != *want {
            mat.0 = want.clone();
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    /// Hard ceiling for the web build: one player = one entity tree of at most this
    /// many drawable meshes (decals count once they are dressed).
    const MESH_BUDGET: usize = 80;

    fn spawn_in_world(id: CharacterId) -> (World, Entity) {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        let root = world
            .run_system_once(
                move |mut commands: Commands,
                      mut meshes: ResMut<Assets<Mesh>>,
                      mut mats: ResMut<Assets<StandardMaterial>>| {
                    spawn_player(
                        &mut commands,
                        &mut meshes,
                        &mut mats,
                        id,
                        Side::Home,
                        0,
                        false,
                        Vec3::ZERO,
                    )
                },
            )
            .expect("spawn system runs");
        (world, root)
    }

    fn descends_from(world: &World, mut e: Entity, root: Entity) -> bool {
        loop {
            if e == root {
                return true;
            }
            match world.get::<ChildOf>(e) {
                Some(c) => e = c.parent(),
                None => return false,
            }
        }
    }

    #[test]
    fn every_character_fits_the_mesh_budget() {
        for id in CharacterId::ALL {
            let (mut world, root) = spawn_in_world(id);
            let meshes: Vec<Entity> = world
                .query_filtered::<Entity, With<Mesh3d>>()
                .iter(&world)
                .collect();
            let decals: Vec<Entity> = world
                .query_filtered::<Entity, With<JerseyDecal>>()
                .iter(&world)
                .collect();
            let owned = meshes
                .iter()
                .chain(decals.iter())
                .filter(|e| descends_from(&world, **e, root))
                .count();
            assert_eq!(owned, meshes.len() + decals.len(), "all parts hang off the root");
            assert!(
                owned <= MESH_BUDGET,
                "{:?} spawns {} drawable parts (budget {})",
                id,
                owned,
                MESH_BUDGET
            );
            assert!(owned >= 60, "{:?} looks too plain ({} parts)", id, owned);
            assert_eq!(decals.len(), 2, "front number + back name plate");
        }
    }

    #[test]
    fn rig_and_face_handles_are_resolved() {
        let (mut world, root) = spawn_in_world(CharacterId::MikaOrbit);
        let rig = world.get::<Rig>(root).expect("rig inserted");
        for e in [
            rig.torso, rig.head, rig.l_arm, rig.r_arm, rig.l_leg, rig.r_leg, rig.l_elbow,
            rig.r_elbow, rig.l_knee, rig.r_knee,
        ] {
            assert!(world.get::<Transform>(e).is_some());
        }
        let face = world.get::<FaceRig>(root).expect("face inserted");
        for e in [
            face.brow_l, face.brow_r, face.mouth, face.mouth_l, face.mouth_r, face.mouth_in,
            face.eye_l, face.eye_r, face.look_l, face.look_r, face.blush_l, face.blush_r,
            face.sweat, face.vein,
        ] {
            assert!(world.get::<Transform>(e).is_some());
        }
        // Fire swaps and hair sway pivots are patched to point at the root.
        let swaps: Vec<Entity> = world
            .query_filtered::<Entity, With<FireSwap>>()
            .iter(&world)
            .collect();
        assert_eq!(swaps.len(), 4, "two irises + two soles");
        for e in swaps {
            assert_eq!(world.get::<FireSwap>(e).unwrap().owner, root);
        }
        let sways: Vec<&HairSway> = world.query::<&HairSway>().iter(&world).collect();
        assert!(sways.iter().any(|s| s.kind == SwayKind::Root));
        assert!(sways.iter().all(|s| s.owner == root));
        // Twin tails swing independently.
        assert_eq!(
            sways
                .iter()
                .filter(|s| matches!(s.kind, SwayKind::Tail { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn glyphs_cover_names_and_numbers() {
        for id in CharacterId::ALL {
            for c in plate_name(id).chars() {
                assert_ne!(glyph(c), [0; 7], "missing glyph for {c}");
            }
            assert!(text_width(plate_name(id), 2) <= DECAL_SIZE, "{:?} name too wide", id);
        }
        assert_eq!(text_width("23", 4), 44);
        assert_eq!(text_width("", 4), 0);
    }

    #[test]
    fn decal_paints_white_text_with_dark_outline() {
        let px = decal_pixels(CharacterId::JinGravity, false);
        assert_eq!(px.len(), (DECAL_SIZE * DECAL_SIZE * 4) as usize);
        let white = px
            .chunks(4)
            .filter(|p| p[0] == 255 && p[3] == 255)
            .count();
        let outline = px.chunks(4).filter(|p| p[3] > 0 && p[0] < 40).count();
        let clear = px.chunks(4).filter(|p| p[3] == 0).count();
        assert!(white > 100, "text pixels: {white}");
        assert!(outline > white / 2, "outline pixels: {outline}");
        assert!(clear > 1500, "transparent background: {clear}");
        // Back plate carries the name as well, so it has more ink.
        let back = decal_pixels(CharacterId::JinGravity, true);
        let back_white = back.chunks(4).filter(|p| p[0] == 255 && p[3] == 255).count();
        assert!(back_white > white);
    }

    /// Runs the rig animation headless for `frames` with a ball held by the player.
    fn animate_holding(id: CharacterId, frames: usize) -> (World, Entity) {
        let (mut world, root) = spawn_in_world(id);
        world.insert_resource(Paused(false));
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_millis(16));
        world.insert_resource(time);
        world.spawn((
            Ball,
            BallState {
                hold: Hold::Held,
                holder: Some(root),
                shooter: None,
                last_touch: None,
                last_passer: None,
                dribble_phase: 0.0,
                rim_hits: 0,
                release_was_three: false,
            },
            // Ball at the right hand, waist high, in front (-Z).
            Transform::from_xyz(0.46, 0.6, -0.22),
        ));
        for _ in 0..frames {
            world.run_system_once(animate_rigs).expect("animate runs");
            let mut t = world.resource_mut::<Time<()>>();
            t.advance_by(std::time::Duration::from_millis(16));
        }
        (world, root)
    }

    fn hanging_dir(world: &World, pivot: Entity) -> Vec3 {
        world.get::<Transform>(pivot).unwrap().rotation * Vec3::NEG_Y
    }

    #[test]
    fn idle_ball_handler_keeps_arms_forward_and_dribbles_right() {
        let (world, root) = animate_holding(CharacterId::KaitoFlash, 90);
        let rig = world.get::<Rig>(root).unwrap();
        let l = hanging_dir(&world, rig.l_arm);
        let r = hanging_dir(&world, rig.r_arm);
        // Forward is -Z: both upper arms swing forward, never back or out sideways.
        assert!(l.z < -0.2, "left upper arm forward, got {l}");
        assert!(r.z < -0.05, "right (dribble) arm forward, got {r}");
        assert!(l.x.abs() < 0.35 && r.x.abs() < 0.35, "arms stay in the sagittal plane: {l} {r}");
        // Elbows fold forward too.
        let le = hanging_dir(&world, rig.l_elbow);
        assert!(le.z < -0.5, "left forearm folds forward, got {le}");
        // Knees fold backward (+Z) and the head looks toward the ball (down, right).
        let lk = hanging_dir(&world, rig.l_knee);
        assert!(lk.z > 0.05, "shin swings back when the knee bends, got {lk}");
        let head = world.get::<Transform>(rig.head).unwrap().rotation * Vec3::NEG_Z;
        assert!(head.y < -0.1 && head.x > 0.05, "head tracks the ball, got {head}");
        // Scale stays uniform-ish and finite.
        let tf = world.get::<Transform>(root).unwrap();
        assert!(tf.scale.is_finite() && (tf.scale.y / tf.scale.x - 1.0).abs() < 0.1);
    }

    #[test]
    fn jersey_numbers_are_two_digits_max() {
        for id in CharacterId::ALL {
            assert!(jersey_number(id) < 100);
        }
    }
}
