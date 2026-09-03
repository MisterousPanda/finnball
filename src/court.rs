//! The arena: painted hardwood, hoops, LED ribbon boards, a live jumbotron, rafters
//! banners, sweeping spotlights, a three-level bowl (lower tiers, suite level, upper
//! deck) full of animated fans, and a busy courtside.
//!
//! Draw calls are kept low for WebGL2 by merging almost everything static into a few
//! vertex-coloured meshes (`Batch` from `crowd.rs`, rendered with the crowd material so
//! LED strips inside the merge can glow) or textured meshes (`TexBatch`, one material
//! per painted atlas).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::arenas::{ArenaId, ArenaTheme};
use crate::quality::Quality;
use crate::courtpaint::{
    paint_banner_atlas, paint_court, paint_ribbon, paint_scoreboard, BannerSpec, CourtImage,
    ScoreboardData, PLANE_HALF_LEN, PLANE_HALF_WID,
};
use crate::crowd::{
    self, lin, Batch, CrowdExt, CrowdHype, CrowdMaterial, CrowdSection, CrowdStyle, FanOpts,
    Lcg, Parts, PART_LED,
};
use crate::sim::{COURT_HALF_LEN, COURT_HALF_WID, HOOP_X, RIM_HEIGHT, RIM_RADIUS};
use crate::states::{AppState, MatchConfig};

/// Texels per meter for the painted hardwood. 64 → 1997x1165 RGBA (~9 MB), fine for WebGL2.

/// The crowd is merged into one mesh per stand section, so seat density is only a
/// vertex-count question. The web build trims the top tiers to keep the upload small.
/// Upper-deck rows (impostor fans, ~10 vertices each).
const SEAT_PITCH: f32 = 0.56;
/// World-space spacing of the aisles (stairs) that cut through every tier.
const AISLE_SPACING: f32 = 6.72;
const AISLE_HALF_W: f32 = 0.62;

const TIER_RISE: f32 = 0.82;
const TIER_DEPTH: f32 = 1.25;
const FIRST_Y: f32 = 0.9;
const SIDE_START: f32 = PLANE_HALF_WID + 1.4;
const END_START: f32 = PLANE_HALF_LEN + 1.9;
const UPPER_RISE: f32 = 0.95;
const UPPER_DEPTH: f32 = 1.0;

/// Jumbotron scoreboard texture size and repaint cadence.
const SCREEN_W: u32 = 384;
const SCREEN_H: u32 = 192;
const SCREEN_REFRESH: f32 = 0.3;
const RIBBON_W: u32 = 2048;
const RIBBON_H: u32 = 256;
/// Meters of ribbon board covered by one texture tile.
const RIBBON_TILE_M: f32 = 8.0;

pub struct CourtPlugin;

impl Plugin for CourtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CourtTextures>()
            .init_resource::<BuiltArena>()
            .init_resource::<ArenaScreens>()
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
            .add_systems(
                Update,
                (
                    scroll_ribbons,
                    update_jumbotron,
                    sweep_spotlights,
                    bounce_mascot,
                    hide_jumbotron_from_above,
                    light_from_sky_dome,
                ),
            );
    }
}

/// Marker: this sky dome's panorama has been sampled for lighting.
#[derive(Component)]
struct SkyTinted;

/// Once a World Labs panorama has loaded, let its colours bleed into the arena:
/// the sky half tints the ambient light and the horizon band tints the distance
/// fog, so a neon night city and a golden-hour temple light the court
/// differently instead of every arena sharing one studio look.
fn light_from_sky_dome(
    mut commands: Commands,
    domes: Query<(Entity, &MeshMaterial3d<StandardMaterial>), (With<SkyDome>, Without<SkyTinted>)>,
    materials: Res<Assets<StandardMaterial>>,
    images: Res<Assets<Image>>,
    config: Res<MatchConfig>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut fogs: Query<&mut DistanceFog>,
) {
    for (entity, mat) in &domes {
        let Some(tex) = materials.get(&mat.0).and_then(|m| m.base_color_texture.clone()) else {
            continue;
        };
        let Some(image) = images.get(&tex) else {
            continue;
        };
        let Some((sky, horizon)) = panorama_bands(image) else {
            commands.entity(entity).insert(SkyTinted);
            continue;
        };
        let theme = config.arena.theme();
        ambient.color = theme.ambient.mix(&sky, 0.45);
        for mut fog in &mut fogs {
            let dim = horizon.to_linear() * 0.35;
            fog.color = theme.fog.mix(&Color::from(dim), 0.6);
        }
        commands.entity(entity).insert(SkyTinted);
    }
}

/// Average colour of the sky (top 45%) and horizon band (42-58%) of an RGBA8
/// equirectangular image. Returns `None` for images without CPU-side data.
fn panorama_bands(image: &Image) -> Option<(Color, Color)> {
    let data = image.data.as_ref()?;
    let w = image.texture_descriptor.size.width as usize;
    let h = image.texture_descriptor.size.height as usize;
    if w == 0 || h == 0 || data.len() < w * h * 4 {
        return None;
    }
    let band = |y0: f32, y1: f32| -> Color {
        let (mut r, mut g, mut b, mut n) = (0f64, 0f64, 0f64, 0f64);
        let step = (w / 128).max(1);
        for y in ((h as f32 * y0) as usize..(h as f32 * y1) as usize).step_by(step) {
            for x in (0..w).step_by(step) {
                let i = (y * w + x) * 4;
                r += data[i] as f64;
                g += data[i + 1] as f64;
                b += data[i + 2] as f64;
                n += 1.0;
            }
        }
        let n = n.max(1.0) * 255.0;
        Color::srgb((r / n) as f32, (g / n) as f32, (b / n) as f32)
    };
    Some((band(0.0, 0.45), band(0.42, 0.58)))
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
struct Referee {
    clock: f32,
}

#[derive(Component)]
struct RefLeg(f32);

/// Visible spotlight beam hanging from the rigging.
#[derive(Component)]
struct SweepCone {
    pos: Vec3,
    phase: f32,
    fade: f32,
}

/// Center-hung jumbotron parts; hidden when the camera rises above them (tactical
/// top-down) so the cube does not blot out the court.
#[derive(Component)]
struct Jumbotron;

#[derive(Component)]
struct Mascot {
    base: Vec3,
    phase: f32,
}

#[derive(Resource, Default)]
pub struct CourtTextures {
    by_arena: HashMap<ArenaId, Handle<Image>>,
}

/// Live-painted screens shared by the jumbotron and ribbon boards. The image handle is
/// created once and re-painted in place; the materials that use it survive arena
/// rebuilds because they are recreated pointing at the same handle.
#[derive(Resource, Default)]
struct ArenaScreens {
    scoreboard: Option<Handle<Image>>,
    ribbon_mat: Option<Handle<StandardMaterial>>,
    timer: f32,
    last: Option<ScoreboardData>,
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
    mut screens: ResMut<ArenaScreens>,
    asset_server: Res<AssetServer>,
    quality: Res<Quality>,
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
    let floor = court_texture(&mut images, &mut cache, &theme, quality.court_px_per_m);
    build_arena(
        &mut commands,
        &mut meshes,
        &mut materials,
        &mut crowd_mats,
        &mut images,
        &mut screens,
        &asset_server,
        &quality,
        &theme,
        floor,
    );
}

fn image_from(painted: CourtImage, sampler: ImageSamplerDescriptor) -> Image {
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
    image.sampler = ImageSampler::Descriptor(sampler);
    image
}

fn court_texture(
    images: &mut Assets<Image>,
    cache: &mut CourtTextures,
    theme: &ArenaTheme,
    px_per_m: u32,
) -> Handle<Image> {
    if let Some(h) = cache.by_arena.get(&theme.id) {
        return h.clone();
    }
    let painted = paint_court(px_per_m, &theme.palette());
    let handle = images.add(image_from(painted, ImageSamplerDescriptor::linear()));
    cache.by_arena.insert(theme.id, handle.clone());
    handle
}

/// Accumulates textured quads into one mesh (banners, screens, ribbons, signage).
#[derive(Default)]
struct TexBatch {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
}

impl TexBatch {
    /// Quad centred at `center` with half-extent vectors; `uv` = [u0, v0, u1, v1] where
    /// v0 is the top of the image. Front face is `right × up`.
    fn quad(&mut self, center: Vec3, right: Vec3, up: Vec3, uv: [f32; 4]) {
        let n = right.cross(up).normalize_or_zero().to_array();
        let base = self.pos.len() as u32;
        for (sr, su, u, v) in [
            (-1.0, -1.0, uv[0], uv[3]),
            (1.0, -1.0, uv[2], uv[3]),
            (1.0, 1.0, uv[2], uv[1]),
            (-1.0, 1.0, uv[0], uv[1]),
        ] {
            self.pos.push((center + right * sr + up * su).to_array());
            self.nrm.push(n);
            self.uv.push([u, v]);
        }
        self.idx
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn vertex_count(&self) -> usize {
        self.pos.len()
    }

    fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
        .with_inserted_indices(Indices::U32(self.idx))
    }
}

fn to_arr(c: Color) -> [f32; 3] {
    let s = c.to_srgba();
    [s.red, s.green, s.blue]
}

/// Spawns one merged crowd/deco mesh; returns its vertex count for the build log.
fn spawn_batch(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    batch: Batch,
    mat: &Handle<CrowdMaterial>,
) -> usize {
    if batch.is_empty() {
        return 0;
    }
    let verts = batch.vertex_count();
    commands.spawn((
        ArenaRoot,
        CrowdSection,
        Mesh3d(meshes.add(batch.build())),
        MeshMaterial3d(mat.clone()),
        Transform::IDENTITY,
    ));
    verts
}

#[allow(clippy::too_many_arguments)]
fn build_arena(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    crowd_mats: &mut Assets<CrowdMaterial>,
    images: &mut Assets<Image>,
    screens: &mut ArenaScreens,
    asset_server: &AssetServer,
    quality: &Quality,
    theme: &ArenaTheme,
    floor_tex: Handle<Image>,
) {
    commands.insert_resource(GlobalAmbientLight {
        color: theme.ambient,
        brightness: 140.0,
        ..default()
    });
    commands.insert_resource(ClearColor(theme.sky));
    if let Some(pano) = theme.env_pano {
        spawn_sky_dome(commands, meshes, materials, asset_server, pano);
    }

    let accent_lin = LinearRgba::from(theme.accent.to_linear());
    let home = crate::roster::Side::Home;
    let away = crate::roster::Side::Away;

    // Glossy lacquered hardwood: low roughness + clearcoat so the light banks and
    // team-colour washes reflect in the floor.
    let floor_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(floor_tex),
        perceptual_roughness: 0.24,
        metallic: 0.0,
        reflectance: 0.85,
        clearcoat: 0.55,
        clearcoat_perceptual_roughness: 0.12,
        ..default()
    });
    let slab_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.03, 0.05),
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
    let lightbank = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::new(6.0, 5.6, 5.0, 1.0),
        unlit: true,
        ..default()
    });
    let crowd_mat = crowd_mats.add(CrowdMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.9,
            ..default()
        },
        extension: CrowdExt::default(),
    });
    let parts = Parts::new();
    // Everything static and vertex-coloured (stairs, rails, tunnels, suites, racks,
    // cables, LED strips) goes into this one merged mesh.
    let mut deco = Batch::default();

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

    // --- LED ribbon boards around the apron: one textured mesh, UV-scrolled
    let ribbon_tex = images.add(image_from(
        paint_ribbon(
            RIBBON_W,
            RIBBON_H,
            theme.ribbon_words,
            [0.98, 0.98, 1.0],
            [0.02, 0.02, 0.05],
            to_arr(theme.accent),
            to_arr(home.primary()),
        ),
        ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::ClampToEdge,
            ..ImageSamplerDescriptor::linear()
        },
    ));
    let ribbon_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(ribbon_tex.clone()),
        emissive: LinearRgba::WHITE * 1.4,
        emissive_texture: Some(ribbon_tex.clone()),
        perceptual_roughness: 0.3,
        unlit: true,
        ..default()
    });
    screens.ribbon_mat = Some(ribbon_mat.clone());
    let ribbon_h = 0.95;
    let mut ribbons = TexBatch::default();
    let mut u_cursor = 0.0;
    // Perimeter loop: +z board reads toward -x, -x board toward -z, -z toward +x, +x toward +z.
    let side_len = PLANE_HALF_LEN * 2.0 - 0.4;
    let end_len = PLANE_HALF_WID * 2.0 - 0.4;
    let board_y = ribbon_h * 0.55;
    for (center, right, len) in [
        (
            Vec3::new(0.0, board_y, PLANE_HALF_WID),
            Vec3::NEG_X,
            side_len,
        ),
        (
            Vec3::new(-PLANE_HALF_LEN, board_y, 0.0),
            Vec3::NEG_Z,
            end_len,
        ),
        (
            Vec3::new(0.0, board_y, -PLANE_HALF_WID),
            Vec3::X,
            side_len,
        ),
        (
            Vec3::new(PLANE_HALF_LEN, board_y, 0.0),
            Vec3::Z,
            end_len,
        ),
    ] {
        let u0 = u_cursor;
        let u1 = u0 + len / RIBBON_TILE_M;
        ribbons.quad(center, right * (len * 0.5), Vec3::Y * 0.25, [u0, 0.0, u1, 0.5]);
        u_cursor = u1;
    }
    // Dark housings behind the boards (merged into deco)
    let housing = [0.03, 0.03, 0.045];
    for s in [-1.0f32, 1.0] {
        deco.block(
            &parts,
            Vec3::new(0.0, ribbon_h * 0.5, s * (PLANE_HALF_WID + 0.12)),
            Vec3::new(PLANE_HALF_LEN * 2.0 + 0.4, ribbon_h, 0.2),
            0.0,
            housing,
        );
        deco.block(
            &parts,
            Vec3::new(s * (PLANE_HALF_LEN + 0.12), ribbon_h * 0.5, 0.0),
            Vec3::new(0.2, ribbon_h, PLANE_HALF_WID * 2.0 + 0.4),
            0.0,
            housing,
        );
        // Floor-level LED apron strip along the base of the boards
        deco.glow_block(
            &parts,
            Vec3::new(0.0, 0.03, s * (PLANE_HALF_WID - 0.02)),
            Vec3::new(PLANE_HALF_LEN * 2.0, 0.06, 0.05),
            0.0,
            lin(theme.accent),
            0.9,
        );
        deco.glow_block(
            &parts,
            Vec3::new(s * (PLANE_HALF_LEN - 0.02), 0.03, 0.0),
            Vec3::new(0.05, 0.06, PLANE_HALF_WID * 2.0),
            0.0,
            lin(theme.accent),
            0.9,
        );
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
    let bowl = spawn_stands(commands, meshes, &crowd_mat, &parts, &mut deco, theme, quality);
    let mut merged_verts = bowl.verts;
    merged_verts += spawn_courtside(
        commands,
        meshes,
        materials,
        &crowd_mat,
        &parts,
        &mut deco,
        &mut ribbons,
        &ribbon_tex,
        theme,
        &slab_mat,
    );
    spawn_referee(commands, meshes, materials);

    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(ribbons.build())),
        MeshMaterial3d(ribbon_mat.clone()),
        Transform::IDENTITY,
    ));

    // --- rafters: championship + retired-number banners painted into one atlas
    let mut specs: Vec<BannerSpec> = Vec::new();
    let gold = [0.95, 0.85, 0.5];
    let white = [0.97, 0.97, 1.0];
    for (i, year) in theme.banner_years.iter().enumerate() {
        let bg = if i % 2 == 0 {
            to_arr(home.primary())
        } else {
            to_arr(theme.accent)
        };
        specs.push(BannerSpec {
            bg,
            fg: [0.02, 0.02, 0.04],
            trim: gold,
            top: "CHAMPIONS".into(),
            big: (*year).into(),
            bottom: "FINNBALL LEAGUE".into(),
            pennant: false,
        });
    }
    for (num, name) in theme.retired.iter() {
        specs.push(BannerSpec {
            bg: [0.04, 0.04, 0.06],
            fg: white,
            trim: to_arr(away.primary()),
            top: "RETIRED".into(),
            big: (*num).into(),
            bottom: (*name).into(),
            pennant: true,
        });
    }
    // Two team crests for the jumbotron logo ring.
    let crest_first = specs.len();
    specs.push(BannerSpec {
        bg: to_arr(home.primary()),
        fg: [0.02, 0.02, 0.04],
        trim: to_arr(home.secondary()),
        top: "NEON".into(),
        big: home.short().into(),
        bottom: "FOXES".into(),
        pennant: false,
    });
    specs.push(BannerSpec {
        bg: to_arr(away.primary()),
        fg: white,
        trim: to_arr(away.secondary()),
        top: "SHADOW".into(),
        big: away.short().into(),
        bottom: "CRANES".into(),
        pennant: false,
    });
    let (atlas, uvs) = paint_banner_atlas(&specs, 4, 192, 320);
    let atlas_tex = images.add(image_from(atlas, ImageSamplerDescriptor::linear()));
    let banner_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(atlas_tex.clone()),
        emissive: LinearRgba::WHITE * 0.35,
        emissive_texture: Some(atlas_tex.clone()),
        alpha_mode: AlphaMode::Mask(0.5),
        cull_mode: None,
        perceptual_roughness: 0.8,
        ..default()
    });
    let top_y = bowl.top_y;
    let mut banners = TexBatch::default();
    let beam_y = top_y + 7.6;
    let beam = [0.05, 0.05, 0.07];
    let n_champ = theme.banner_years.len();
    let n_ret = theme.retired.len();
    for s in [-1.0f32, 1.0] {
        // Long-side rafter beam over the lower bowl, banners hanging below it.
        let bz = s * (SIDE_START + bowl.far * 0.55);
        deco.block(
            &parts,
            Vec3::new(0.0, beam_y, bz),
            Vec3::new(30.0, 0.25, 0.25),
            0.0,
            beam,
        );
        let total = n_champ + n_ret;
        for i in 0..total {
            let x = (i as f32 - (total as f32 - 1.0) * 0.5) * 2.6;
            let uv = uvs[i];
            let c = Vec3::new(x, beam_y - 1.9, bz);
            // face the court: normal toward -s z
            let right = Vec3::X * -s;
            banners.quad(c, right * 0.85, Vec3::Y * 1.4, uv);
            deco.block(
                &parts,
                Vec3::new(x, beam_y - 0.3, bz),
                Vec3::new(0.05, 0.45, 0.05),
                0.0,
                [0.6, 0.6, 0.65],
            );
        }
        // Retired numbers again on the end walls
        let bx = s * (END_START + bowl.far * 0.55);
        deco.block(
            &parts,
            Vec3::new(bx, beam_y, 0.0),
            Vec3::new(0.25, 0.25, 16.0),
            0.0,
            beam,
        );
        for (k, uv) in uvs.iter().skip(n_champ).take(n_ret).enumerate() {
            let z = (k as f32 - (n_ret as f32 - 1.0) * 0.5) * 2.6;
            let c = Vec3::new(bx, beam_y - 1.9, z);
            let right = Vec3::Z * s;
            banners.quad(c, right * 0.85, Vec3::Y * 1.4, *uv);
        }
    }
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(banners.build())),
        MeshMaterial3d(banner_mat.clone()),
        Transform::IDENTITY,
    ));

    // --- center-hung jumbotron with a live-painted scoreboard on all four faces
    let cube_y = 12.5;
    let scoreboard = match &screens.scoreboard {
        Some(h) => h.clone(),
        None => {
            let h = images.add(image_from(
                paint_scoreboard(SCREEN_W, SCREEN_H, &menu_board(theme, 0.0)),
                ImageSamplerDescriptor::linear(),
            ));
            screens.scoreboard = Some(h.clone());
            h
        }
    };
    screens.last = None;
    let screen_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(scoreboard.clone()),
        emissive: LinearRgba::WHITE * 2.2,
        emissive_texture: Some(scoreboard.clone()),
        unlit: true,
        ..default()
    });
    commands.spawn((
        ArenaRoot,
        Jumbotron,
        Mesh3d(meshes.add(Cuboid::new(7.2, 4.0, 7.2))),
        MeshMaterial3d(slab_mat.clone()),
        Transform::from_xyz(0.0, cube_y, 0.0),
    ));
    let mut screens_mesh = TexBatch::default();
    for (c, right) in [
        (Vec3::new(0.0, cube_y, 3.62), Vec3::X),
        (Vec3::new(0.0, cube_y, -3.62), Vec3::NEG_X),
        (Vec3::new(3.62, cube_y, 0.0), Vec3::NEG_Z),
        (Vec3::new(-3.62, cube_y, 0.0), Vec3::Z),
    ] {
        screens_mesh.quad(c, right * 3.2, Vec3::Y * 1.6, [0.0, 0.0, 1.0, 1.0]);
    }
    commands.spawn((
        ArenaRoot,
        Jumbotron,
        Mesh3d(meshes.add(screens_mesh.build())),
        MeshMaterial3d(screen_mat),
        Transform::IDENTITY,
    ));
    // LED under-ring + accent trim
    commands.spawn((
        ArenaRoot,
        Jumbotron,
        Mesh3d(meshes.add(Cuboid::new(7.4, 0.25, 7.4))),
        MeshMaterial3d(neon.clone()),
        Transform::from_xyz(0.0, cube_y - 2.1, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Jumbotron,
        Mesh3d(meshes.add(Cuboid::new(7.4, 0.12, 7.4))),
        MeshMaterial3d(neon.clone()),
        Transform::from_xyz(0.0, cube_y + 2.05, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Jumbotron,
        HoloSpin,
        Mesh3d(meshes.add(Torus {
            minor_radius: 0.12,
            major_radius: 2.6,
        })),
        MeshMaterial3d(neon.clone()),
        Transform::from_xyz(0.0, cube_y - 2.9, 0.0),
    ));
    // Rotating team-crest ring under the cube
    let mut crest_ring = TexBatch::default();
    let n_tiles = 8;
    for i in 0..n_tiles {
        let a = i as f32 / n_tiles as f32 * std::f32::consts::TAU;
        let dir = Vec3::new(a.sin(), 0.0, a.cos());
        let right = Vec3::new(a.cos(), 0.0, -a.sin());
        let uv = uvs[crest_first + (i % 2)];
        crest_ring.quad(dir * 3.0, right * 0.55, Vec3::Y * 0.75, uv);
    }
    commands.spawn((
        ArenaRoot,
        Jumbotron,
        HoloSpin,
        Mesh3d(meshes.add(crest_ring.build())),
        MeshMaterial3d(banner_mat.clone()),
        Transform::from_xyz(0.0, cube_y - 3.6, 0.0),
    ));
    commands.spawn((
        ArenaRoot,
        Jumbotron,
        Mesh3d(meshes.add(Cylinder::new(0.12, 6.0))),
        MeshMaterial3d(pole_mat.clone()),
        Transform::from_xyz(0.0, cube_y + 5.0, 0.0),
    ));

    // --- rigging: light banks + truss ring + spotlight beams + haze
    // Beam tint follows the arena's emissive signature colour.
    let em = theme.emissive;
    let em_max = em.red.max(em.green).max(em.blue).max(0.01);
    let beam_tint = LinearRgba::new(
        0.55 + 0.45 * em.red / em_max,
        0.55 + 0.45 * em.green / em_max,
        0.55 + 0.45 * em.blue / em_max,
        0.014,
    );
    let cone_mat = materials.add(StandardMaterial {
        base_color: Color::from(beam_tint),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let cone_mesh = meshes.add(Cone {
        radius: 2.6,
        height: 20.0,
    });
    for (i, (x, z)) in [(-9.0, 5.5), (9.0, 5.5), (-9.0, -5.5), (9.0, -5.5)]
        .into_iter()
        .enumerate()
    {
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
        commands.spawn((
            ArenaRoot,
            SweepCone {
                pos: Vec3::new(x, 14.0, z),
                phase: i as f32 * 1.7,
                fade: 0.0,
            },
            Mesh3d(cone_mesh.clone()),
            MeshMaterial3d(cone_mat.clone()),
            Transform::from_xyz(x, 4.0, z),
            Visibility::Hidden,
        ));
    }
    for (i, z) in [-1.0f32, 1.0].into_iter().enumerate() {
        commands.spawn((
            ArenaRoot,
            SweepCone {
                pos: Vec3::new(0.0, 15.4, z * 21.0),
                phase: 4.0 + i as f32 * 2.3,
                fade: 0.0,
            },
            Mesh3d(cone_mesh.clone()),
            MeshMaterial3d(cone_mat.clone()),
            Transform::from_xyz(0.0, 4.0, z * 21.0),
            Visibility::Hidden,
        ));
    }
    let truss_mat = materials.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.04),
        emissive: accent_lin * 1.6,
        perceptual_roughness: 0.3,
        ..default()
    });
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Torus {
            minor_radius: 0.18,
            major_radius: 22.0,
        })),
        MeshMaterial3d(truss_mat),
        Transform::from_xyz(0.0, 15.5, 0.0),
    ));
    let haze_mat = materials.add(StandardMaterial {
        base_color: theme.fog.with_alpha(0.05),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let haze_mesh = meshes.add(Plane3d::default().mesh().size(70.0, 60.0));
    for y in [6.0, 10.5] {
        commands.spawn((
            ArenaRoot,
            Mesh3d(haze_mesh.clone()),
            MeshMaterial3d(haze_mat.clone()),
            Transform::from_xyz(0.0, y, 0.0),
        ));
    }
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
        (-16.0, home.primary()),
        (16.0, away.primary()),
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

    merged_verts += spawn_batch(commands, meshes, deco, &crowd_mat);
    info!(
        "arena '{}' built: {} vertices in merged crowd/deco meshes ({} lower tiers, {} upper rows{})",
        theme.name,
        merged_verts,
        quality.tiers,
        quality.upper_rows,
        if quality.mobile { ", mobile tier" } else { "" }
    );
}

/// Layout facts the rafters need from the bowl.
struct BowlInfo {
    top_y: f32,
    far: f32,
    verts: usize,
}

fn near_aisle(coord: f32) -> bool {
    let k = (coord / AISLE_SPACING).round();
    (coord - k * AISLE_SPACING).abs() < AISLE_HALF_W
}

fn aisle_positions(half_len: f32) -> Vec<f32> {
    let n = ((half_len - 1.0) / AISLE_SPACING).floor() as i32;
    (-n..=n).map(|k| k as f32 * AISLE_SPACING).collect()
}

fn spawn_stands(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    crowd_mat: &Handle<CrowdMaterial>,
    parts: &Parts,
    deco: &mut Batch,
    theme: &ArenaTheme,
    quality: &Quality,
) -> BowlInfo {
    let tiers = quality.tiers;
    let home = crate::roster::Side::Home.primary();
    let away = crate::roster::Side::Away.primary();
    // Home fans fill the -x half, visitors the +x half, mixed in the middle.
    let styles = [
        CrowdStyle::section(home, away, theme.accent, theme.crowd, 0.82),
        CrowdStyle::section(home, away, theme.accent, theme.crowd, 0.5),
        CrowdStyle::section(home, away, theme.accent, theme.crowd, 0.18),
    ];
    let style_for = |x: f32| -> usize {
        if x < -4.5 {
            0
        } else if x > 4.5 {
            2
        } else {
            1
        }
    };
    let usher_style = CrowdStyle::arena(home, away, theme.accent, theme.crowd)
        .with_shirts(vec![[0.95, 0.8, 0.05], [0.95, 0.45, 0.05]])
        .with_pants(vec![[0.04, 0.04, 0.06]])
        .with_props(0.0);
    let mut rng = Lcg(0x5EED_1234);

    let riser_c = lin(theme.crowd);
    let riser_dark = [riser_c[0] * 0.7, riser_c[1] * 0.7, riser_c[2] * 0.7];
    let step_c = [0.16f32.powf(2.2), 0.17f32.powf(2.2), 0.2f32.powf(2.2)];
    let rail_c = [0.55, 0.58, 0.62];
    let tunnel_c = [0.004, 0.004, 0.006];
    let exit_green = [0.1, 1.0, 0.3];

    // Lower bowl: 3 sections per long side (home / centre / away) + one per end,
    // each merged across all tiers.
    let mut side_batches: [[Batch; 3]; 2] = Default::default();
    let mut end_batches: [Batch; 2] = Default::default();

    for tier in 0..tiers {
        let y = FIRST_Y + tier as f32 * TIER_RISE;
        let off = tier as f32 * TIER_DEPTH;
        let side_len = (PLANE_HALF_LEN + 1.2 + off) * 2.0;
        let end_len = (PLANE_HALF_WID + 0.6 + off) * 2.0;

        for (si, s) in [-1.0f32, 1.0].into_iter().enumerate() {
            // long side riser + step lip
            let z = s * (SIDE_START + off);
            deco.block(
                parts,
                Vec3::new(0.0, y * 0.5, z),
                Vec3::new(side_len, y, TIER_DEPTH),
                0.0,
                riser_c,
            );
            deco.block(
                parts,
                Vec3::new(0.0, y - 0.02, z - s * TIER_DEPTH * 0.5),
                Vec3::new(side_len, 0.05, 0.12),
                0.0,
                step_c,
            );
            let n = (side_len / SEAT_PITCH) as usize;
            let yaw = if s > 0.0 { std::f32::consts::PI } else { 0.0 };
            for i in 0..n {
                let x = -side_len * 0.5 + (i as f32 + 0.5) * SEAT_PITCH;
                if near_aisle(x) {
                    continue;
                }
                let origin = Vec3::new(x, y, z + s * 0.1);
                let style = &styles[style_for(x)];
                let batch = &mut side_batches[si][style_for(x)];
                crowd::seat(batch, parts, origin, yaw, style);
                let r = rng.next();
                if r > 1.0 - quality.crowd_density {
                    crowd::fan(batch, parts, &mut rng, origin, yaw, style, r > 0.93);
                }
            }
            // Aisles: stairs, hand rails, ushers, vomitory tunnels with EXIT signs
            for (ai, ax) in aisle_positions(side_len * 0.5).into_iter().enumerate() {
                let front = z - s * TIER_DEPTH * 0.5;
                let back = z + s * TIER_DEPTH * 0.5;
                let vomitory = tier == tiers / 2 && ai % 2 == 1 || tier == tiers - 1 && ai % 2 == 0;
                if !vomitory && tier + 1 < tiers {
                    // two steps up to the next tier
                    deco.block(
                        parts,
                        Vec3::new(ax, y + TIER_RISE * 0.25, back - s * 0.45),
                        Vec3::new(AISLE_HALF_W * 2.0 - 0.1, TIER_RISE * 0.5, 0.3),
                        0.0,
                        step_c,
                    );
                    deco.block(
                        parts,
                        Vec3::new(ax, y + TIER_RISE * 0.75, back - s * 0.15),
                        Vec3::new(AISLE_HALF_W * 2.0 - 0.1, TIER_RISE * 0.5, 0.3),
                        0.0,
                        step_c,
                    );
                    // yellow safety nosing on the top step
                    deco.block(
                        parts,
                        Vec3::new(ax, y + TIER_RISE - 0.005, back - s * 0.29),
                        Vec3::new(AISLE_HALF_W * 2.0 - 0.1, 0.012, 0.04),
                        0.0,
                        [0.9, 0.7, 0.05],
                    );
                }
                // centre hand rail: post + sloped bar to the next tier
                if tier + 1 < tiers && !vomitory {
                    deco.block(
                        parts,
                        Vec3::new(ax, y + 0.45, front + s * 0.3),
                        Vec3::new(0.035, 0.9, 0.035),
                        0.0,
                        rail_c,
                    );
                    let a = Vec3::new(ax, y + 0.9, front + s * 0.3);
                    let b = Vec3::new(ax, y + TIER_RISE + 0.9, front + s * (0.3 + TIER_DEPTH));
                    let dir = (b - a).normalize();
                    deco.push(
                        &parts.block,
                        Transform {
                            translation: (a + b) * 0.5,
                            rotation: Quat::from_rotation_arc(Vec3::Z, dir),
                            scale: Vec3::new(0.035, 0.035, (b - a).length()),
                        },
                        rail_c,
                        0.0,
                        0.0,
                    );
                }
                if vomitory {
                    // tunnel mouth cut into the next riser, EXIT sign above it
                    let mouth_z = back - s * 0.04;
                    deco.block(
                        parts,
                        Vec3::new(ax, y + 1.0, mouth_z),
                        Vec3::new(AISLE_HALF_W * 2.0 + 0.3, 2.0, 0.08),
                        0.0,
                        tunnel_c,
                    );
                    deco.block(
                        parts,
                        Vec3::new(ax, y + 2.06, mouth_z - s * 0.02),
                        Vec3::new(AISLE_HALF_W * 2.0 + 0.5, 0.12, 0.1),
                        0.0,
                        [0.25, 0.25, 0.28],
                    );
                    deco.glow_block(
                        parts,
                        Vec3::new(ax, y + 2.28, mouth_z - s * 0.06),
                        Vec3::new(0.7, 0.2, 0.06),
                        0.0,
                        exit_green,
                        1.3,
                    );
                    // tunnel floor glow strip
                    deco.glow_block(
                        parts,
                        Vec3::new(ax, y + 0.02, mouth_z - s * 0.08),
                        Vec3::new(AISLE_HALF_W * 2.0, 0.03, 0.05),
                        0.0,
                        lin(theme.accent),
                        0.6,
                    );
                }
                if tier % 3 == 1 && ai % 2 == 0 {
                    let o = Vec3::new(ax + 0.25, y, front + s * 0.35);
                    let batch = &mut side_batches[si][style_for(ax)];
                    crowd::fan_with(batch, parts, &mut rng, o, yaw, &usher_style, FanOpts::staff());
                }
            }

            // end riser
            let x = s * (END_START + off);
            deco.block(
                parts,
                Vec3::new(x, y * 0.5, 0.0),
                Vec3::new(TIER_DEPTH, y, end_len),
                0.0,
                riser_c,
            );
            deco.block(
                parts,
                Vec3::new(x - s * TIER_DEPTH * 0.5, y - 0.02, 0.0),
                Vec3::new(0.12, 0.05, end_len),
                0.0,
                step_c,
            );
            let end_style = if s < 0.0 { &styles[0] } else { &styles[2] };
            let batch = &mut end_batches[si];
            let n = (end_len / SEAT_PITCH) as usize;
            let yaw = if s > 0.0 {
                -std::f32::consts::FRAC_PI_2
            } else {
                std::f32::consts::FRAC_PI_2
            };
            for i in 0..n {
                let zz = -end_len * 0.5 + (i as f32 + 0.5) * SEAT_PITCH;
                if near_aisle(zz) {
                    continue;
                }
                let origin = Vec3::new(x + s * 0.1, y, zz);
                crowd::seat(batch, parts, origin, yaw, end_style);
                let r = rng.next();
                if r > 1.02 - quality.crowd_density {
                    crowd::fan(batch, parts, &mut rng, origin, yaw, end_style, r > 0.94);
                }
            }
            for (ai, az) in aisle_positions(end_len * 0.5).into_iter().enumerate() {
                let front = x - s * TIER_DEPTH * 0.5;
                let back = x + s * TIER_DEPTH * 0.5;
                let vomitory = tier == tiers / 2 && ai % 2 == 0;
                if !vomitory && tier + 1 < tiers {
                    deco.block(
                        parts,
                        Vec3::new(back - s * 0.45, y + TIER_RISE * 0.25, az),
                        Vec3::new(0.3, TIER_RISE * 0.5, AISLE_HALF_W * 2.0 - 0.1),
                        0.0,
                        step_c,
                    );
                    deco.block(
                        parts,
                        Vec3::new(back - s * 0.15, y + TIER_RISE * 0.75, az),
                        Vec3::new(0.3, TIER_RISE * 0.5, AISLE_HALF_W * 2.0 - 0.1),
                        0.0,
                        step_c,
                    );
                    deco.block(
                        parts,
                        Vec3::new(front + s * 0.3, y + 0.45, az),
                        Vec3::new(0.035, 0.9, 0.035),
                        0.0,
                        rail_c,
                    );
                }
                if vomitory {
                    let mouth_x = back - s * 0.04;
                    deco.block(
                        parts,
                        Vec3::new(mouth_x, y + 1.0, az),
                        Vec3::new(0.08, 2.0, AISLE_HALF_W * 2.0 + 0.3),
                        0.0,
                        tunnel_c,
                    );
                    deco.glow_block(
                        parts,
                        Vec3::new(mouth_x - s * 0.06, y + 2.28, az),
                        Vec3::new(0.06, 0.2, 0.7),
                        0.0,
                        exit_green,
                        1.3,
                    );
                }
                if tier % 3 == 2 && ai % 2 == 1 {
                    let o = Vec3::new(front + s * 0.35, y, az + 0.25);
                    crowd::fan_with(batch, parts, &mut rng, o, yaw, &usher_style, FanOpts::staff());
                }
            }
        }
    }

    // Front rail around the lower bowl
    for s in [-1.0f32, 1.0] {
        deco.block(
            parts,
            Vec3::new(0.0, FIRST_Y + 0.9, s * (SIDE_START - TIER_DEPTH * 0.5)),
            Vec3::new((PLANE_HALF_LEN + 1.2) * 2.0, 0.04, 0.04),
            0.0,
            rail_c,
        );
        deco.block(
            parts,
            Vec3::new(s * (END_START - TIER_DEPTH * 0.5), FIRST_Y + 0.9, 0.0),
            Vec3::new(0.04, 0.04, (PLANE_HALF_WID + 0.6) * 2.0),
            0.0,
            rail_c,
        );
        // Front-row safety padding in team colour
        deco.block(
            parts,
            Vec3::new(0.0, FIRST_Y * 0.5, s * (SIDE_START - TIER_DEPTH * 0.5 - 0.06)),
            Vec3::new((PLANE_HALF_LEN + 1.2) * 2.0, FIRST_Y, 0.1),
            0.0,
            riser_dark,
        );
    }

    let mut verts = 0;
    for batches in side_batches {
        for b in batches {
            verts += spawn_batch(commands, meshes, b, crowd_mat);
        }
    }
    for b in end_batches {
        verts += spawn_batch(commands, meshes, b, crowd_mat);
    }

    // --- suite level: glass boxes with warm light and silhouettes
    let top_y = FIRST_Y + tiers as f32 * TIER_RISE;
    let far = tiers as f32 * TIER_DEPTH;
    let suite_y0 = top_y + 0.3;
    let suite_h = 2.8;
    let facade = [0.025, 0.028, 0.04];
    let glow_c = lin(theme.suite_glow);
    let sil_c = [0.006, 0.006, 0.01];
    let side_face = SIDE_START + far + 0.35;
    let end_face = END_START + far + 0.35;
    let mut upper = Batch::default();
    for s in [-1.0f32, 1.0] {
        // long side band
        let len = (PLANE_HALF_LEN + 1.2 + far) * 2.0 + 2.0;
        deco.block(
            parts,
            Vec3::new(0.0, suite_y0 + suite_h * 0.5, s * (side_face + 0.4)),
            Vec3::new(len, suite_h, 0.8),
            0.0,
            facade,
        );
        deco.glow_block(
            parts,
            Vec3::new(0.0, suite_y0 + 0.05, s * (side_face - 0.01)),
            Vec3::new(len, 0.08, 0.05),
            0.0,
            lin(theme.accent),
            0.8,
        );
        let n_suites = (len / 3.8).floor() as i32;
        for i in 0..n_suites {
            let x = (i as f32 - (n_suites as f32 - 1.0) * 0.5) * 3.8;
            let wc = Vec3::new(x, suite_y0 + 1.55, s * (side_face - 0.03));
            let right = Vec3::X * -s;
            let lit = rng.next() > 0.15;
            let wc_col = if lit { glow_c } else { [0.02, 0.03, 0.05] };
            deco.quad(wc, right * 1.5, Vec3::Y * 0.8, wc_col, 0.0, 0.0, if lit { 0.35 } else { 0.0 }, PART_LED);
            if lit {
                let n_sil = 1 + (rng.next() * 3.0) as i32;
                for k in 0..n_sil {
                    let sx = x + (k as f32 - (n_sil as f32 - 1.0) * 0.5) * 0.8 + rng.range(-0.15, 0.15);
                    let h = 0.55 + rng.next() * 0.25;
                    let sc = Vec3::new(sx, suite_y0 + 0.75 + h * 0.5, s * (side_face - 0.06));
                    deco.quad(sc, right * 0.18, Vec3::Y * (h * 0.5), sil_c, 0.0, 0.0, 0.0, PART_LED);
                    deco.quad(sc + Vec3::Y * (h * 0.5 + 0.1), right * 0.09, Vec3::Y * 0.1, sil_c, 0.0, 0.0, 0.0, PART_LED);
                }
            }
            // mullion
            deco.block(
                parts,
                Vec3::new(x + 1.9, suite_y0 + 1.55, s * (side_face - 0.02)),
                Vec3::new(0.12, 1.7, 0.1),
                0.0,
                [0.08, 0.08, 0.1],
            );
        }
        // end band
        let elen = (PLANE_HALF_WID + 0.6 + far) * 2.0 + 2.0;
        deco.block(
            parts,
            Vec3::new(s * (end_face + 0.4), suite_y0 + suite_h * 0.5, 0.0),
            Vec3::new(0.8, suite_h, elen),
            0.0,
            facade,
        );
        deco.glow_block(
            parts,
            Vec3::new(s * (end_face - 0.01), suite_y0 + 0.05, 0.0),
            Vec3::new(0.05, 0.08, elen),
            0.0,
            lin(theme.accent),
            0.8,
        );
        let n_suites = (elen / 3.8).floor() as i32;
        for i in 0..n_suites {
            let z = (i as f32 - (n_suites as f32 - 1.0) * 0.5) * 3.8;
            let wc = Vec3::new(s * (end_face - 0.03), suite_y0 + 1.55, z);
            let right = Vec3::Z * s;
            let lit = rng.next() > 0.2;
            let wc_col = if lit { glow_c } else { [0.02, 0.03, 0.05] };
            deco.quad(wc, right * 1.5, Vec3::Y * 0.8, wc_col, 0.0, 0.0, if lit { 0.35 } else { 0.0 }, PART_LED);
            if lit {
                for k in 0..2 {
                    let sz = z + (k as f32 - 0.5) * 0.9;
                    let h = 0.55 + rng.next() * 0.25;
                    let sc = Vec3::new(s * (end_face - 0.06), suite_y0 + 0.75 + h * 0.5, sz);
                    deco.quad(sc, right * 0.18, Vec3::Y * (h * 0.5), sil_c, 0.0, 0.0, 0.0, PART_LED);
                }
            }
        }

        // --- upper deck: steeper rake, impostor fans. Open-air arenas (World
        // Labs panorama) keep only a shallow upper tier so the generated world
        // rises above the bowl instead of a wall of seats.
        let open_air = theme.env_pano.is_some();
        let upper_rows = if open_air { 3.min(quality.upper_rows) } else { quality.upper_rows };
        let up_y0 = suite_y0 + suite_h + 0.3;
        let up_side0 = side_face + 0.9;
        let up_end0 = end_face + 0.9;
        for r in 0..upper_rows {
            let y = up_y0 + r as f32 * UPPER_RISE;
            let zoff = r as f32 * UPPER_DEPTH;
            let ulen = (PLANE_HALF_LEN + 1.2 + far + zoff) * 2.0 + 2.0;
            let z = s * (up_side0 + zoff);
            deco.block(
                parts,
                Vec3::new(0.0, y - UPPER_RISE * 0.5, z),
                Vec3::new(ulen, UPPER_RISE, UPPER_DEPTH),
                0.0,
                if r % 2 == 0 { riser_c } else { riser_dark },
            );
            let yaw = if s > 0.0 { std::f32::consts::PI } else { 0.0 };
            let n = (ulen / 0.5) as usize;
            for i in 0..n {
                let x = -ulen * 0.5 + (i as f32 + 0.5) * 0.5;
                if near_aisle(x) || rng.next() < 0.12 {
                    continue;
                }
                crowd::impostor(&mut upper, &mut rng, Vec3::new(x, y, z), yaw, &styles[style_for(x)]);
            }
            let elen = (PLANE_HALF_WID + 0.6 + far + zoff) * 2.0 + 2.0;
            let x = s * (up_end0 + zoff);
            deco.block(
                parts,
                Vec3::new(x, y - UPPER_RISE * 0.5, 0.0),
                Vec3::new(UPPER_DEPTH, UPPER_RISE, elen),
                0.0,
                if r % 2 == 0 { riser_c } else { riser_dark },
            );
            let yaw = if s > 0.0 {
                -std::f32::consts::FRAC_PI_2
            } else {
                std::f32::consts::FRAC_PI_2
            };
            let n = (elen / 0.5) as usize;
            let st = if s < 0.0 { &styles[0] } else { &styles[2] };
            for i in 0..n {
                let zz = -elen * 0.5 + (i as f32 + 0.5) * 0.5;
                if near_aisle(zz) || rng.next() < 0.14 {
                    continue;
                }
                crowd::impostor(&mut upper, &mut rng, Vec3::new(x, y, zz), yaw, st);
            }
        }
        // Upper-deck aisle lights
        let up_top = up_y0 + upper_rows as f32 * UPPER_RISE;
        for ax in aisle_positions(PLANE_HALF_LEN + far + 2.0) {
            for r in (0..upper_rows).step_by(2) {
                let y = up_y0 + r as f32 * UPPER_RISE;
                let z = s * (up_side0 + r as f32 * UPPER_DEPTH - UPPER_DEPTH * 0.5 + 0.02);
                deco.glow_block(parts, Vec3::new(ax, y - 0.2, z), Vec3::new(0.5, 0.05, 0.04), 0.0, [0.9, 0.9, 1.0], 0.5);
            }
        }

        // --- back walls up to the roof, roof, roof lights. With a World Labs
        // panorama the bowl is open-air: walls stop at a parapet above the upper
        // deck and the roof is only its truss frame, so the generated world
        // shows above the stands and through the opening.
        let roof_y = up_top + 4.5;
        let wall_top = if open_air { up_top + 1.3 } else { roof_y };
        let wall_z = s * (up_side0 + upper_rows as f32 * UPPER_DEPTH + 0.3);
        let wall_len = (PLANE_HALF_LEN + 1.2 + far + upper_rows as f32 * UPPER_DEPTH) * 2.0 + 4.0;
        deco.block(
            parts,
            Vec3::new(0.0, wall_top * 0.5, wall_z),
            Vec3::new(wall_len, wall_top, 0.6),
            0.0,
            [0.02, 0.022, 0.035],
        );
        let wall_x = s * (up_end0 + upper_rows as f32 * UPPER_DEPTH + 0.3);
        let wall_wid = (PLANE_HALF_WID + 0.6 + far + upper_rows as f32 * UPPER_DEPTH) * 2.0 + 4.0;
        deco.block(
            parts,
            Vec3::new(wall_x, wall_top * 0.5, 0.0),
            Vec3::new(0.6, wall_top, wall_wid),
            0.0,
            [0.02, 0.022, 0.035],
        );
        // Wall-mounted team wordmark strip (glowing) above the upper deck
        deco.glow_block(
            parts,
            Vec3::new(0.0, up_top + 1.2, wall_z - s * 0.32),
            Vec3::new(wall_len * 0.6, 0.35, 0.04),
            0.0,
            if s < 0.0 { lin(home) } else { lin(away) },
            0.7,
        );
        if s > 0.0 {
            // roof (normal down) + girders + ring of roof lights
            if open_air {
                // Truss frame around a big opening: four flat bands along the edges.
                let band = 3.0;
                let hx = wall_len * 0.5;
                let hz = wall_wid * 0.5;
                for (c, ex, ez) in [
                    (Vec3::new(0.0, roof_y, hz - band * 0.5), hx, band * 0.5),
                    (Vec3::new(0.0, roof_y, -hz + band * 0.5), hx, band * 0.5),
                    (Vec3::new(hx - band * 0.5, roof_y, 0.0), band * 0.5, hz - band),
                    (Vec3::new(-hx + band * 0.5, roof_y, 0.0), band * 0.5, hz - band),
                ] {
                    deco.quad(c, Vec3::X * ex, Vec3::Z * ez, lin(theme.roof), 0.0, 0.0, 0.0, PART_LED);
                }
            } else {
                deco.quad(
                    Vec3::new(0.0, roof_y, 0.0),
                    Vec3::X * (wall_len * 0.5),
                    Vec3::Z * (wall_wid * 0.5),
                    lin(theme.roof),
                    0.0,
                    0.0,
                    0.0,
                    PART_LED,
                );
            }
            if !open_air {
                for k in -3..=3 {
                    deco.block(
                        parts,
                        Vec3::new(k as f32 * 9.0, roof_y - 0.5, 0.0),
                        Vec3::new(0.5, 0.8, wall_wid - 1.0),
                        0.0,
                        [0.03, 0.03, 0.04],
                    );
                }
            }
            let n_lights = 20;
            for i in 0..n_lights {
                let a = i as f32 / n_lights as f32 * std::f32::consts::TAU;
                let p = Vec3::new(a.cos() * 24.0, roof_y - 1.0, a.sin() * 18.0);
                deco.glow_block(parts, p, Vec3::new(1.2, 0.3, 1.2), a, [1.0, 0.97, 0.9], 1.5);
            }
        }
    }
    verts += spawn_batch(commands, meshes, upper, crowd_mat);

    BowlInfo { top_y, far, verts }
}

/// Wrap the stadium in an equirectangular panorama (a World Labs Marble world).
/// Bevy's UV sphere has its poles on +/-Z with u running counter-clockwise, so
/// the mesh is tipped pole-up and mirrored on X: the mirror turns the winding
/// inside-out and makes the panorama read the right way round from inside.
/// Unlit so the panorama's own lighting shows.
fn spawn_sky_dome(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    pano: &'static str,
) {
    let tex: Handle<Image> = asset_server.load(pano);
    let mesh = meshes.add(Sphere::new(SKY_DOME_RADIUS).mesh().uv(64, 32));
    let mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(tex),
        unlit: true,
        // The camera's distance fog would otherwise swallow a 190 m sphere.
        fog_enabled: false,
        cull_mode: None,
        ..default()
    });
    commands.spawn((
        ArenaRoot,
        SkyDome,
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        Transform::from_xyz(0.0, SKY_DOME_HORIZON_Y, 0.0)
            .with_scale(Vec3::new(-1.0, 1.0, 1.0))
            .with_rotation(
                Quat::from_rotation_y(SKY_DOME_YAW)
                    * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            ),
        bevy::light::NotShadowCaster,
        bevy::light::NotShadowReceiver,
    ));
}

/// Far enough that no arena geometry pokes through, near enough to stay inside
/// the camera's far plane.
const SKY_DOME_RADIUS: f32 = 190.0;
/// Panorama horizon sits a little above the concourse so the upper half (sky,
/// skyline tops) is what shows above the stands and through the roof.
const SKY_DOME_HORIZON_Y: f32 = 16.0;
/// Put the panorama's centre (u = 0.5) on the far sideline (-Z), which is what
/// the broadcast and tip-off cameras look towards.
const SKY_DOME_YAW: f32 = std::f32::consts::FRAC_PI_2;

#[derive(Component)]
pub struct SkyDome;

#[allow(clippy::too_many_arguments)]
fn spawn_courtside(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    crowd_mat: &Handle<CrowdMaterial>,
    parts: &Parts,
    deco: &mut Batch,
    ribbons: &mut TexBatch,
    ribbon_tex: &Handle<Image>,
    theme: &ArenaTheme,
    slab_mat: &Handle<StandardMaterial>,
) -> usize {
    let mut rng = Lcg(0xC0A5_71DE);
    let home = crate::roster::Side::Home;
    let away = crate::roster::Side::Away;
    let floor_seat_z = PLANE_HALF_WID - 0.55;
    let mut batch = Batch::default();

    // Scorer's table with LED front (part of the scrolling ribbon mesh)
    commands.spawn((
        ArenaRoot,
        Mesh3d(meshes.add(Cuboid::new(7.6, 0.78, 0.7))),
        MeshMaterial3d(slab_mat.clone()),
        Transform::from_xyz(0.0, 0.39, floor_seat_z + 0.25),
    ));
    ribbons.quad(
        Vec3::new(0.0, 0.42, floor_seat_z - 0.12),
        Vec3::NEG_X * 3.7,
        Vec3::Y * 0.25,
        [0.0, 0.0, 7.4 / RIBBON_TILE_M, 0.5],
    );
    // monitors + mics on the table
    for i in 0..5 {
        let x = (i as f32 - 2.0) * 1.4;
        deco.glow_block(
            parts,
            Vec3::new(x, 0.95, floor_seat_z + 0.3),
            Vec3::new(0.45, 0.3, 0.03),
            0.0,
            [0.4, 0.6, 0.9],
            0.5,
        );
        deco.block(parts, Vec3::new(x, 0.82, floor_seat_z + 0.3), Vec3::new(0.12, 0.08, 0.1), 0.0, [0.05, 0.05, 0.06]);
    }
    // Officials behind the table
    let officials = CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd)
        .with_shirts(vec![[0.05, 0.05, 0.06], [0.9, 0.9, 0.92]])
        .with_props(0.0);
    for i in 0..6 {
        let x = (i as f32 - 2.5) * 1.15;
        let o = Vec3::new(x, 0.0, floor_seat_z + 0.95);
        crowd::seat(&mut batch, parts, o, std::f32::consts::PI, &officials);
        crowd::fan_with(
            &mut batch,
            parts,
            &mut rng,
            o,
            std::f32::consts::PI,
            &officials,
            FanOpts {
                standing: false,
                kids: false,
                props: false,
                cap: false,
                cheer: false,
            },
        );
    }

    // Team benches: reserves in full uniform, coaches standing, towels, bottles, racks
    let bench_c = [0.1f32.powf(2.2), 0.1f32.powf(2.2), 0.13f32.powf(2.2)];
    for (sx, side) in [(-1.0f32, home), (1.0, away)] {
        let jersey = side.primary();
        let mut team = CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd)
            .with_shirts(vec![lin(jersey), lin(jersey), lin(side.secondary())])
            .with_pants(vec![lin(side.secondary())])
            .with_props(0.0);
        team.team = [lin(jersey), lin(side.secondary())];
        let bench_z = floor_seat_z + 0.5;
        deco.block(parts, Vec3::new(sx * 7.4, 0.44, bench_z), Vec3::new(5.2, 0.08, 0.5), 0.0, bench_c);
        for k in 0..3 {
            deco.block(
                parts,
                Vec3::new(sx * (5.2 + k as f32 * 2.2), 0.2, bench_z),
                Vec3::new(0.08, 0.4, 0.4),
                0.0,
                [0.2, 0.2, 0.22],
            );
        }
        for i in 0..7 {
            let x = sx * (5.0 + i as f32 * 0.8);
            let o = Vec3::new(x, 0.0, bench_z);
            crowd::fan(&mut batch, parts, &mut rng, o, std::f32::consts::PI, &team, i == 6);
            // towel over the shoulder or on the bench, water bottle at the feet
            if i % 2 == 0 {
                deco.block(parts, Vec3::new(x + 0.22, 0.5, bench_z + 0.05), Vec3::new(0.16, 0.04, 0.3), 0.0, [0.95, 0.95, 0.97]);
            }
            if i % 3 != 1 {
                deco.push(
                    &parts.limb,
                    Transform {
                        translation: Vec3::new(x - 0.2, 0.1, bench_z - 0.3),
                        rotation: Quat::IDENTITY,
                        scale: Vec3::new(0.035, 0.07, 0.035),
                    },
                    [0.2, 0.55, 0.95],
                    0.0,
                    0.0,
                );
            }
        }
        // Coaches in suits, standing in front of the bench
        let suits = CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd)
            .with_shirts(vec![[0.03, 0.03, 0.05], [0.08, 0.08, 0.12], [0.2, 0.2, 0.25]])
            .with_pants(vec![[0.03, 0.03, 0.05]])
            .with_props(0.0);
        for k in 0..2 {
            let o = Vec3::new(sx * (4.2 + k as f32 * 0.9), 0.0, floor_seat_z - 0.05);
            crowd::fan_with(
                &mut batch,
                parts,
                &mut rng,
                o,
                std::f32::consts::PI + sx * 0.3,
                &suits,
                FanOpts {
                    standing: true,
                    kids: false,
                    props: false,
                    cap: false,
                    cheer: false,
                },
            );
        }
        // Ball rack behind the bench end
        let rx = sx * 10.9;
        let rz = floor_seat_z + 0.75;
        for (dx, dz) in [(-0.35, -0.2), (0.35, -0.2), (-0.35, 0.2), (0.35, 0.2)] {
            deco.block(parts, Vec3::new(rx + dx, 0.45, rz + dz), Vec3::new(0.03, 0.9, 0.03), 0.0, [0.3, 0.3, 0.33]);
        }
        for level in 0..2 {
            let y = 0.35 + level as f32 * 0.45;
            deco.block(parts, Vec3::new(rx, y, rz), Vec3::new(0.8, 0.03, 0.5), 0.0, [0.3, 0.3, 0.33]);
            for b in 0..3 {
                deco.push(
                    &parts.head,
                    Transform {
                        translation: Vec3::new(rx + (b as f32 - 1.0) * 0.26, y + 0.14, rz),
                        rotation: Quat::IDENTITY,
                        scale: Vec3::splat(0.12),
                    },
                    [0.85, 0.3, 0.06],
                    0.0,
                    0.0,
                );
            }
        }
        // Gatorade cooler
        deco.block(parts, Vec3::new(sx * 9.9, 0.3, rz), Vec3::new(0.4, 0.6, 0.4), 0.0, [0.9, 0.45, 0.05]);
    }

    // Mascot by the home bench: bounces via `bounce_mascot`
    let mut mascot = Batch::default();
    crowd::mascot(
        &mut mascot,
        parts,
        lin(home.primary()),
        lin(home.secondary()),
        lin(theme.accent),
    );
    let mascot_base = Vec3::new(-10.4, 0.0, floor_seat_z - 0.4);
    commands.spawn((
        ArenaRoot,
        CrowdSection,
        Mascot {
            base: mascot_base,
            phase: 0.0,
        },
        Mesh3d(meshes.add(mascot.build())),
        MeshMaterial3d(crowd_mat.clone()),
        Transform::from_translation(mascot_base).with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
    ));

    // Courtside celebrity row: far sideline
    let vip = CrowdStyle::arena(
        home.primary(),
        away.primary(),
        theme.accent,
        Color::srgb(0.1, 0.1, 0.12),
    )
    .with_props(0.3);
    let mut x = -PLANE_HALF_LEN + 1.6;
    while x < PLANE_HALF_LEN - 1.6 {
        let o = Vec3::new(x, 0.0, -floor_seat_z);
        crowd::seat(&mut batch, parts, o, 0.0, &vip);
        if rng.next() > 0.08 {
            crowd::fan(&mut batch, parts, &mut rng, o, 0.0, &vip, false);
        }
        x += 0.68;
    }
    // Cheer / dance squads in the near-side corners, facing the court
    let mut cheer_style = CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd);
    cheer_style.team = [lin(home.primary()), lin(home.secondary())];
    cheer_style.hairs = vec![lin(home.secondary()), [0.08, 0.06, 0.05], [0.55, 0.38, 0.2]];
    for sx in [-1.0f32, 1.0] {
        for k in 0..7 {
            let x = sx * (10.3 + k as f32 * 0.62);
            let o = Vec3::new(x, 0.0, floor_seat_z + (k % 2) as f32 * 0.35);
            crowd::fan_with(&mut batch, parts, &mut rng, o, std::f32::consts::PI, &cheer_style, FanOpts::cheer());
        }
    }
    // Baseline photographers sitting on the floor
    let press = CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd)
        .with_shirts(vec![[0.04, 0.04, 0.05], [0.12, 0.12, 0.14], [0.3, 0.3, 0.33]])
        .with_props(0.0);
    for sx in [-1.0f32, 1.0] {
        for i in 0..8 {
            let z = (i as f32 - 3.5) * 1.3 + if i >= 4 { 1.2 } else { -1.2 };
            let o = Vec3::new(sx * (PLANE_HALF_LEN - 0.55), -0.36, z);
            let yaw = if sx > 0.0 {
                -std::f32::consts::FRAC_PI_2
            } else {
                std::f32::consts::FRAC_PI_2
            };
            crowd::fan(&mut batch, parts, &mut rng, o, yaw, &press, false);
            // long lens
            deco.push(
                &parts.limb,
                Transform {
                    translation: o + Vec3::new(-sx * 0.3, 0.75, 0.0),
                    rotation: Quat::from_rotation_z(sx * std::f32::consts::FRAC_PI_2),
                    scale: Vec3::new(0.04, 0.14, 0.04),
                },
                [0.02, 0.02, 0.025],
                0.0,
                0.0,
            );
        }
    }

    // Broadcast cameras on tripods at the corners + operators + cable runs
    let cam_c = [0.02, 0.02, 0.025];
    let crew = CrowdStyle::arena(home.primary(), away.primary(), theme.accent, theme.crowd)
        .with_shirts(vec![[0.04, 0.04, 0.05], [0.1, 0.1, 0.12]])
        .with_pants(vec![[0.04, 0.04, 0.05]])
        .with_props(0.0);
    for (sx, sz) in [(-1.0f32, 1.0f32), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        let base = Vec3::new(sx * (COURT_HALF_LEN + 1.0), 0.0, sz * (COURT_HALF_WID + 1.0));
        // tripod legs
        for k in 0..3 {
            let a = k as f32 * std::f32::consts::TAU / 3.0;
            let foot = Vec3::new(a.cos() * 0.4, 0.0, a.sin() * 0.4);
            let top = Vec3::Y * 1.25;
            let dir = (top - foot).normalize();
            deco.push(
                &parts.block,
                Transform {
                    translation: base + (foot + top) * 0.5,
                    rotation: Quat::from_rotation_arc(Vec3::Y, dir),
                    scale: Vec3::new(0.03, (top - foot).length(), 0.03),
                },
                cam_c,
                0.0,
                0.0,
            );
        }
        let look = Quat::from_rotation_arc(Vec3::NEG_Z, (Vec3::new(0.0, 1.0, 0.0) - (base + Vec3::Y * 1.42)).normalize());
        deco.push(
            &parts.block,
            Transform {
                translation: base + Vec3::Y * 1.42,
                rotation: look,
                scale: Vec3::new(0.3, 0.28, 0.5),
            },
            cam_c,
            0.0,
            0.0,
        );
        // red tally light
        deco.glow_block(parts, base + Vec3::new(0.0, 1.62, 0.0), Vec3::new(0.05, 0.05, 0.05), 0.0, [1.0, 0.05, 0.05], 1.5);
        // operator behind the camera
        let op = base + Vec3::new(sx * 0.55, 0.0, sz * 0.55);
        let yaw = (-(op.x)).atan2(-(op.z));
        crowd::fan_with(&mut batch, parts, &mut rng, op, yaw, &crew, FanOpts::staff());
        // cable run along the floor to the corner of the apron
        let corner = Vec3::new(sx * (PLANE_HALF_LEN - 0.2), 0.0, sz * (PLANE_HALF_WID - 0.2));
        let seg_a = Vec3::new(base.x, 0.0, corner.z);
        for (a, b) in [(base, seg_a), (seg_a, corner)] {
            let d = b - a;
            if d.length() < 0.05 {
                continue;
            }
            deco.push(
                &parts.block,
                Transform {
                    translation: (a + b) * 0.5 + Vec3::Y * 0.015,
                    rotation: Quat::from_rotation_arc(Vec3::Z, d.normalize()),
                    scale: Vec3::new(0.05, 0.03, d.length()),
                },
                [0.015, 0.015, 0.02],
                0.0,
                0.0,
            );
        }
    }

    // Sponsor stanchions: rotating triangular signage at the four apron corners
    let mut prism = TexBatch::default();
    for k in 0..3 {
        let a = k as f32 * std::f32::consts::TAU / 3.0;
        let n = Vec3::new(a.sin(), 0.0, a.cos());
        let right = Vec3::new(a.cos(), 0.0, -a.sin());
        prism.quad(n * 0.26, right * 0.45, Vec3::Y * 0.55, [k as f32 * 0.35, 0.0, k as f32 * 0.35 + 0.9 / RIBBON_TILE_M, 0.5]);
    }
    let prism_mesh = meshes.add(prism.build());
    let stanchion_mat = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(ribbon_tex.clone()),
        emissive: LinearRgba::WHITE * 1.2,
        emissive_texture: Some(ribbon_tex.clone()),
        unlit: true,
        ..default()
    });
    for (sx, sz) in [(-1.0f32, 1.0f32), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        let p = Vec3::new(sx * (PLANE_HALF_LEN - 0.75), 0.0, sz * (PLANE_HALF_WID - 0.75));
        deco.block(parts, p + Vec3::Y * 0.1, Vec3::new(0.7, 0.2, 0.7), 0.0, [0.05, 0.05, 0.07]);
        deco.block(parts, p + Vec3::Y * 0.45, Vec3::new(0.08, 0.5, 0.08), 0.0, [0.3, 0.3, 0.33]);
        commands.spawn((
            ArenaRoot,
            HoloSpin,
            Mesh3d(prism_mesh.clone()),
            MeshMaterial3d(stanchion_mat.clone()),
            Transform::from_translation(p + Vec3::Y * 1.3),
        ));
    }

    spawn_batch(commands, meshes, batch, crowd_mat)
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

#[allow(clippy::too_many_arguments)]
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
    // Backboard frame + shooter's square (one unit cube, scaled → instanced)
    let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
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
            Mesh3d(unit.clone()),
            MeshMaterial3d(board.clone()),
            Transform {
                translation: Vec3::new(board_x - sign * 0.04, RIM_HEIGHT + 0.32 + dy, dz),
                scale: Vec3::new(0.05, sy, sz),
                ..default()
            },
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

/// Scrolls the ribbon texture around the bowl; flips to the DEFENSE row when the crowd
/// is loud and brightens with hype.
fn scroll_ribbons(
    time: Res<Time>,
    hype: Res<CrowdHype>,
    screens: Res<ArenaScreens>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(handle) = &screens.ribbon_mat else {
        return;
    };
    let Some(mat) = mats.get_mut(handle) else {
        return;
    };
    let t = time.elapsed_secs();
    let defense = hype.level > 0.55 || hype.fire > 0.5;
    let speed = if defense { 0.16 } else { 0.06 };
    let v = if defense { 0.5 } else { 0.0 };
    mat.uv_transform = Affine2::from_translation(Vec2::new(-t * speed, v));
    let pulse = if defense {
        1.6 + (t * 9.0).sin().abs() * 0.8
    } else {
        1.3 + hype.level * 0.6
    };
    mat.emissive = LinearRgba::WHITE * pulse;
}

fn menu_board(theme: &ArenaTheme, t: f32) -> ScoreboardData {
    ScoreboardData {
        home_short: crate::roster::Side::Home.short().into(),
        away_short: crate::roster::Side::Away.short().into(),
        home: 0,
        away: 0,
        quarter: 1,
        clock: 0.0,
        shot: 0.0,
        home_color: to_arr(crate::roster::Side::Home.primary()),
        away_color: to_arr(crate::roster::Side::Away.primary()),
        accent: to_arr(theme.accent),
        headline: "FINNBALL".into(),
        subline: theme.name.into(),
        hype: 0.0,
        fire: false,
        t,
    }
}

/// Repaints the jumbotron image a few times a second with the live score and clocks.
fn update_jumbotron(
    time: Res<Time>,
    state: Res<State<AppState>>,
    config: Res<MatchConfig>,
    clock: Option<Res<crate::gameplay::MatchClock>>,
    score: Option<Res<crate::gameplay::Scoreboard>>,
    hype: Res<CrowdHype>,
    mut screens: ResMut<ArenaScreens>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(handle) = screens.scoreboard.clone() else {
        return;
    };
    screens.timer -= time.delta_secs();
    if screens.timer > 0.0 {
        return;
    }
    screens.timer = SCREEN_REFRESH;
    let theme = config.arena.theme();
    let t = time.elapsed_secs();
    let mut data = menu_board(&theme, t);
    match state.get() {
        AppState::Playing | AppState::GameOver => {
            data.headline.clear();
            data.subline.clear();
            if let Some(c) = &clock {
                data.quarter = c.quarter;
                data.clock = c.remaining;
                data.shot = c.shot;
            }
            if let Some(s) = &score {
                data.home = s.home;
                data.away = s.away;
            }
            data.hype = hype.level;
            data.fire = hype.fire > 0.5;
            if *state.get() == AppState::GameOver {
                data.headline = "FINAL".into();
                data.subline = format!("{} {} - {} {}", data.home_short, data.home, data.away, data.away_short);
            }
        }
        AppState::CharacterSelect => {
            data.headline = "PICK YOUR SQUAD".into();
            data.subline = format!(
                "{} VS {}",
                crate::roster::Side::Home.label(),
                crate::roster::Side::Away.label()
            );
        }
        AppState::CourtSelect => {
            data.headline = theme.name.into();
            data.subline = "CHOOSE YOUR ARENA".into();
        }
        _ => {}
    }
    // Static headline boards only repaint when their text changes; live boards animate
    // (hype meter, blinking clock) so they repaint every refresh tick.
    let static_board = !data.headline.is_empty();
    let unchanged = screens.last.as_ref().is_some_and(|l| {
        let mut probe = l.clone();
        probe.t = data.t;
        probe == data
    });
    if static_board && unchanged && (t * 0.5).fract() > 0.2 {
        return;
    }
    if let Some(img) = images.get_mut(&handle) {
        let painted = paint_scoreboard(SCREEN_W, SCREEN_H, &data);
        img.data = Some(painted.rgba);
    }
    screens.last = Some(data);
}

/// Spotlight beams sweep the bowl during menus and whenever a player is on fire /
/// the crowd is at full volume; otherwise they fade out.
fn sweep_spotlights(
    time: Res<Time>,
    state: Res<State<AppState>>,
    hype: Res<CrowdHype>,
    mut q: Query<(&mut Transform, &mut Visibility, &mut SweepCone)>,
) {
    let in_match = *state.get() == AppState::Playing;
    let active = !in_match || hype.fire > 0.3 || hype.level > 0.8;
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (mut tf, mut vis, mut cone) in &mut q {
        let target = if active { 1.0 } else { 0.0 };
        cone.fade += (target - cone.fade) * (1.0 - (-2.5 * dt).exp());
        if cone.fade < 0.02 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let aim = Vec3::new(
            (t * 0.45 + cone.phase).sin() * 11.0,
            0.0,
            (t * 0.31 + cone.phase * 1.3).cos() * 7.0,
        );
        let dir = (aim - cone.pos).normalize();
        let half = 10.0;
        tf.translation = cone.pos + dir * half;
        tf.rotation = Quat::from_rotation_arc(Vec3::Y, -dir);
        tf.scale = Vec3::new(cone.fade, 1.0, cone.fade);
    }
}

/// The tactical top-down camera sits above the jumbotron; hide the cube so it does not
/// cover center court from that angle.
fn hide_jumbotron_from_above(
    cam: Query<&Transform, With<crate::camera::GameCam>>,
    mut parts: Query<&mut Visibility, With<Jumbotron>>,
) {
    let Ok(cam) = cam.single() else {
        return;
    };
    let want = if cam.translation.y > 14.0 {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    for mut v in &mut parts {
        if *v != want {
            *v = want;
        }
    }
}

fn bounce_mascot(time: Res<Time>, hype: Res<CrowdHype>, mut q: Query<(&mut Transform, &mut Mascot)>) {
    let dt = time.delta_secs();
    for (mut tf, mut m) in &mut q {
        m.phase += dt * (3.2 + hype.level * 3.0);
        let hop = m.phase.sin().max(0.0);
        let amp = 0.12 + hype.level * 0.4 + hype.fire * 0.3;
        tf.translation = m.base + Vec3::Y * hop * amp;
        let wobble = (m.phase * 0.5).sin() * (0.25 + hype.level * 0.4);
        tf.rotation = Quat::from_rotation_y(std::f32::consts::PI + wobble)
            * Quat::from_rotation_z((m.phase * 2.0).sin() * 0.06 * (1.0 + hype.level));
        let squash = 1.0 - hop * 0.08;
        tf.scale = Vec3::new(1.0 / squash.sqrt(), squash, 1.0 / squash.sqrt());
    }
}

pub const _COURT_EXTENT: (f32, f32) = (COURT_HALF_LEN, COURT_HALF_WID);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panorama_bands_split_sky_and_horizon() {
        // Top half pure blue sky, bottom half green ground.
        let (w, h) = (64u32, 100u32);
        let mut data = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                if y < h / 2 {
                    data[i + 2] = 255;
                } else {
                    data[i + 1] = 255;
                }
                data[i + 3] = 255;
            }
        }
        let image = Image::new(
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::all(),
        );
        let (sky, horizon) = panorama_bands(&image).expect("cpu data");
        let sky = sky.to_srgba();
        assert!(sky.blue > 0.95 && sky.green < 0.05, "sky {:?}", sky);
        let hz = horizon.to_srgba();
        assert!(hz.blue > 0.3 && hz.green > 0.3, "horizon mixes both bands: {:?}", hz);

    }

    #[test]
    fn aisles_are_regular_and_symmetric() {
        assert!(near_aisle(0.0));
        assert!(near_aisle(AISLE_SPACING + 0.3));
        assert!(!near_aisle(AISLE_SPACING * 0.5));
        let a = aisle_positions(16.0);
        assert_eq!(a.len(), 5);
        assert_eq!(a[0], -a[4]);
    }

    #[test]
    fn tex_batch_quads_are_indexed() {
        let mut b = TexBatch::default();
        b.quad(Vec3::ZERO, Vec3::X, Vec3::Y, [0.0, 0.0, 1.0, 1.0]);
        b.quad(Vec3::Z, Vec3::X, Vec3::Y, [0.5, 0.0, 1.0, 0.5]);
        assert_eq!(b.vertex_count(), 8);
        assert_eq!(b.idx.len(), 12);
        // v0 is the image top: the upper-left corner of the first quad samples (u0, v0)
        assert_eq!(b.uv[3], [0.0, 0.0]);
        assert_eq!(b.uv[0], [0.0, 1.0]);
        let mesh = b.build();
        assert_eq!(mesh.count_vertices(), 8);
    }

    #[test]
    fn menu_board_names_the_arena() {
        let theme = ArenaId::SkyTemple.theme();
        let d = menu_board(&theme, 0.0);
        assert_eq!(d.headline, "FINNBALL");
        assert_eq!(d.subline, theme.name);
    }
}
