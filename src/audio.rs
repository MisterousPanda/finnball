//! FINNBALL soundscape.
//!
//! Everything audible lives here: a four-bus mixer (music / sfx / crowd / ui), a per-frame
//! one-shot budget so twenty bounces never stack, pitch + level randomisation, broadcast-style
//! stereo panning, a three-layer crowd bed driven by hype, in-game music stems that fade in
//! with intensity, and sidechain-style ducking on big moments.
//!
//! The plugin owns no gameplay state. Anything the game does not announce with a message is
//! derived here by watching state across frames (`Local<T>` caches): shot clock crossing 5 s,
//! pose transitions, ball hold transitions, lead changes, heat igniting, possession flips.
//!
//! All clips are synthesized by `scripts/generate_audio.py` — nothing is sampled.

use std::collections::HashMap;

use bevy::audio::{
    AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, SpatialListener, SpatialScale,
    Volume,
};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::ball::{
    BackboardHitEvent, Ball, BallState, BallVel, BucketEvent, FloorBounceEvent, Hold, RimHitEvent,
    VISUAL_RADIUS,
};
use crate::camera::GameCam;
use crate::crowd::CrowdHype;
use crate::fx::ScreenJuice;
use crate::gameplay::{
    CutSqueak, DribbleTickEvent, MatchClock, PlayCall, Scoreboard, StealEvent, Ticker,
    TipWhistle, ViolationEvent,
};
use crate::roster::Side;
use crate::sim::RIM_HEIGHT;
use crate::states::{AppState, GameMode, MatchConfig, Paused};
use crate::units::{Heat, MoveVel, Player, Pose};

pub struct FinnAudioPlugin;

impl Plugin for FinnAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioMix>()
            .init_resource::<AudioGate>()
            .add_message::<UiClick>()
            .add_systems(Startup, (preload, spawn_pan_listener))
            .add_systems(
                Update,
                (
                    unlock_on_input,
                    track_listener,
                    music_for_state,
                    bucket_sounds,
                    collision_sounds,
                    ball_state_sounds,
                    dribble_sounds,
                    player_foley,
                    game_event_sounds,
                    broadcast_cues,
                    possession_sounds,
                    crowd_dynamics,
                    pause_sounds,
                    play_ui_clicks,
                    fire_pending,
                    duck_and_mix,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Public surface
// ---------------------------------------------------------------------------

#[derive(Message, Clone, Copy)]
pub struct UiClick {
    pub confirm: bool,
}

/// Master mix. Bus volumes are public so a settings screen can drive them.
#[derive(Resource)]
pub struct AudioMix {
    pub music: f32,
    pub sfx: f32,
    pub crowd: f32,
    pub ui: f32,
    /// Live music duck multiplier (1 = no duck).
    pub duck: f32,
    duck_target: f32,
    duck_hold: f32,
    crowd_duck: f32,
    crowd_duck_target: f32,
    crowd_duck_hold: f32,
    /// Crowd excitement 0..1 — crossfades the murmur / excited / roar beds.
    excite: f32,
    excite_target: f32,
    /// Music intensity 0..1 — brings in the drum stem.
    intensity: f32,
    intensity_target: f32,
    /// Crunch-time flag — brings in the rush stem.
    rush_target: f32,
    /// Who has the rock (true = home) as last observed by `possession_sounds`.
    possession_home: Option<bool>,
    any_on_fire: bool,
    cam: Option<CamFrame>,
    budget: Budget,
    /// Loop layers spawned this frame (commands are deferred, so the query cannot see them yet).
    spawned_loops: Vec<LoopKind>,
    rng: u32,
}

impl Default for AudioMix {
    fn default() -> Self {
        Self {
            music: 0.32,
            sfx: 0.9,
            crowd: 0.5,
            ui: 0.7,
            duck: 1.0,
            duck_target: 1.0,
            duck_hold: 0.0,
            crowd_duck: 1.0,
            crowd_duck_target: 1.0,
            crowd_duck_hold: 0.0,
            excite: 0.0,
            excite_target: 0.0,
            intensity: 0.0,
            intensity_target: 0.0,
            rush_target: 0.0,
            possession_home: None,
            any_on_fire: false,
            cam: None,
            budget: Budget::default(),
            spawned_loops: Vec::new(),
            rng: 0x2545_F491,
        }
    }
}

impl AudioMix {
    fn bus_volume(&self, bus: Bus) -> f32 {
        match bus {
            Bus::Music => self.music,
            Bus::Sfx => self.sfx,
            Bus::Crowd => self.crowd,
            Bus::Ui => self.ui,
        }
    }

    /// Sidechain the music down to `depth` for `hold` seconds (deepest request wins).
    fn duck_music(&mut self, depth: f32, hold: f32) {
        self.duck_target = self.duck_target.min(depth.clamp(0.0, 1.0));
        self.duck_hold = self.duck_hold.max(hold);
    }

    fn duck_crowd(&mut self, depth: f32, hold: f32) {
        self.crowd_duck_target = self.crowd_duck_target.min(depth.clamp(0.0, 1.0));
        self.crowd_duck_hold = self.crowd_duck_hold.max(hold);
    }

    /// Instantly lift crowd excitement (it decays on its own).
    fn excite_kick(&mut self, amount: f32) {
        self.excite = self.excite.max(amount.clamp(0.0, 1.0));
    }

    fn rand(&mut self) -> f32 {
        self.rng = xorshift(self.rng);
        (self.rng >> 8) as f32 / 16_777_216.0
    }

    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.rand()
    }

    fn chance(&mut self, p: f32) -> bool {
        self.rand() < p
    }
}

// ---------------------------------------------------------------------------
// Buses, budget, clips
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bus {
    Music,
    Sfx,
    Crowd,
    Ui,
}

/// One-shot categories with a per-frame cap. Keeps a pile-up of simultaneous physics events
/// from turning into a wall of identical transients.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
enum Cat {
    Bounce = 0,
    Foley,
    Impact,
    Whoosh,
    Crowd,
    Stinger,
    Ui,
}

const CAT_COUNT: usize = 7;
const CAT_CAPS: [u8; CAT_COUNT] = [2, 3, 3, 2, 2, 2, 2];
/// Above this many live one-shots we start dropping low-priority categories.
const LIVE_SOFT_CAP: usize = 26;
const LIVE_HARD_CAP: usize = 40;

#[derive(Default, Clone, Copy)]
struct Budget {
    used: [u8; CAT_COUNT],
    spawned: usize,
}

impl Budget {
    fn reset(&mut self) {
        self.used = [0; CAT_COUNT];
        self.spawned = 0;
    }

    /// Returns true (and counts the shot) if this category still has room this frame.
    fn try_take(&mut self, cat: Cat, live: usize) -> bool {
        let total = live + self.spawned;
        if total >= LIVE_HARD_CAP && cat != Cat::Stinger {
            return false;
        }
        if total >= LIVE_SOFT_CAP && matches!(cat, Cat::Bounce | Cat::Foley | Cat::Whoosh) {
            return false;
        }
        let i = cat as usize;
        if self.used[i] >= CAT_CAPS[i] {
            return false;
        }
        self.used[i] += 1;
        self.spawned += 1;
        true
    }
}

macro_rules! clips {
    ($( $name:ident => $path:literal ),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        #[repr(usize)]
        enum Clip { $( $name ),* }

        impl Clip {
            const ALL: &'static [Clip] = &[ $( Clip::$name ),* ];

            fn path(self) -> &'static str {
                match self { $( Clip::$name => $path ),* }
            }
        }
    };
}

clips! {
    // ball
    Bounce1 => "audio/ball/bounce_1.wav",
    Bounce2 => "audio/ball/bounce_2.wav",
    Bounce3 => "audio/ball/bounce_3.wav",
    Bounce4 => "audio/ball/bounce_4.wav",
    Bounce5 => "audio/ball/bounce_5.wav",
    Dribble1 => "audio/ball/dribble_1.wav",
    Dribble2 => "audio/ball/dribble_2.wav",
    Dribble3 => "audio/ball/dribble_3.wav",
    Catch => "audio/ball/catch.wav",
    PassWhoosh => "audio/ball/pass_whoosh.wav",
    ShotFlick => "audio/ball/shot_flick.wav",
    RimFront => "audio/ball/rim_front.wav",
    RimBack => "audio/ball/rim_back.wav",
    RimSoft => "audio/ball/rim_soft.wav",
    Backboard => "audio/ball/backboard.wav",
    BackboardHard => "audio/ball/backboard_hard.wav",
    Swish => "audio/ball/swish.wav",
    SwishSoft => "audio/ball/swish_soft.wav",
    Rattle => "audio/ball/rattle.wav",
    Roll => "audio/ball/roll.wav",
    // player
    SqueakShort1 => "audio/player/squeak_short_1.wav",
    SqueakShort2 => "audio/player/squeak_short_2.wav",
    SqueakLong => "audio/player/squeak_long.wav",
    Step1 => "audio/player/step_1.wav",
    Step2 => "audio/player/step_2.wav",
    Step3 => "audio/player/step_3.wav",
    JumpGrunt => "audio/player/jump_grunt.wav",
    Land => "audio/player/land.wav",
    DunkBoom => "audio/player/dunk_boom.wav",
    BlockSlap => "audio/player/block_slap.wav",
    StealRip => "audio/player/steal_rip.wav",
    BodyThud => "audio/player/body_thud.wav",
    // crowd
    BedMurmur => "audio/crowd/bed_murmur.wav",
    BedExcited => "audio/crowd/bed_excited.wav",
    BedRoar => "audio/crowd/bed_roar.wav",
    CheerSmall => "audio/crowd/cheer_small.wav",
    CheerBig => "audio/crowd/cheer_big.wav",
    CheerHuge => "audio/crowd/cheer_huge.wav",
    Oooh => "audio/crowd/oooh.wav",
    Gasp => "audio/crowd/gasp.wav",
    Groan => "audio/crowd/groan.wav",
    Boo => "audio/crowd/boo.wav",
    Anticipation => "audio/crowd/anticipation.wav",
    StompClap => "audio/crowd/stomp_clap.wav",
    Chant => "audio/crowd/chant.wav",
    Airhorn => "audio/crowd/airhorn.wav",
    Whistles => "audio/crowd/whistles.wav",
    // game / broadcast
    ShotTick => "audio/game/shot_tick.wav",
    BuzzerLong => "audio/game/buzzer_long.wav",
    BuzzerShort => "audio/game/buzzer_short.wav",
    WhistleShort => "audio/game/whistle_short.wav",
    WhistleLong => "audio/game/whistle_long.wav",
    WhistleDouble => "audio/game/whistle_double.wav",
    PossessionChime => "audio/game/possession_chime.wav",
    OrganCharge => "audio/game/organ_charge.wav",
    // stingers
    OnFire => "audio/stingers/on_fire.wav",
    LeadChange => "audio/stingers/lead_change.wav",
    Clutch => "audio/stingers/clutch.wav",
    FinalMinute => "audio/stingers/final_minute.wav",
    Anthem => "audio/stingers/anthem.wav",
    FanfareWin => "audio/stingers/fanfare_win.wav",
    FanfareLoss => "audio/stingers/fanfare_loss.wav",
    Downtown => "audio/stingers/downtown.wav",
    Poster => "audio/stingers/poster.wav",
    // ui
    Blip => "audio/ui/blip.wav",
    Confirm => "audio/ui/confirm.wav",
    Pause => "audio/ui/pause.wav",
    Unpause => "audio/ui/unpause.wav",
    // music
    MenuMusic => "audio/music/menu_synthwave.wav",
    IngameBase => "audio/music/ingame_base.wav",
    IngameDrums => "audio/music/ingame_drums.wav",
    IngameRush => "audio/music/ingame_rush.wav",
}

#[derive(Resource)]
struct Sounds {
    handles: Vec<Handle<AudioSource>>,
}

impl Sounds {
    fn get(&self, clip: Clip) -> Handle<AudioSource> {
        self.handles[clip as usize].clone()
    }
}

#[derive(Resource, Default)]
struct AudioGate {
    unlocked: bool,
}

#[derive(Clone, Copy)]
struct CamFrame {
    pos: Vec3,
    right: Vec3,
}

/// Marker on every one-shot entity (used for the live count).
#[derive(Component)]
struct OneShot;

/// Fixed listener at the origin; emitters are placed around it in "pan space".
#[derive(Component)]
struct PanListener;

/// A one-shot scheduled for later (lets cues sequence: buzzer, then chime).
#[derive(Component)]
struct Pending {
    delay: f32,
    clip: Clip,
    shot: Shot,
}

// ---------------------------------------------------------------------------
// Looping layers (music stems, crowd beds, chants, ball roll)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LoopKind {
    MenuMusic,
    IngameBase,
    IngameDrums,
    IngameRush,
    BedMurmur,
    BedExcited,
    BedRoar,
    StompClap,
    Chant,
    Roll,
}

impl LoopKind {
    fn clip(self) -> Clip {
        match self {
            LoopKind::MenuMusic => Clip::MenuMusic,
            LoopKind::IngameBase => Clip::IngameBase,
            LoopKind::IngameDrums => Clip::IngameDrums,
            LoopKind::IngameRush => Clip::IngameRush,
            LoopKind::BedMurmur => Clip::BedMurmur,
            LoopKind::BedExcited => Clip::BedExcited,
            LoopKind::BedRoar => Clip::BedRoar,
            LoopKind::StompClap => Clip::StompClap,
            LoopKind::Chant => Clip::Chant,
            LoopKind::Roll => Clip::Roll,
        }
    }

    fn bus(self) -> Bus {
        match self {
            LoopKind::MenuMusic
            | LoopKind::IngameBase
            | LoopKind::IngameDrums
            | LoopKind::IngameRush => Bus::Music,
            LoopKind::BedMurmur
            | LoopKind::BedExcited
            | LoopKind::BedRoar
            | LoopKind::StompClap
            | LoopKind::Chant => Bus::Crowd,
            LoopKind::Roll => Bus::Sfx,
        }
    }

    /// (attack, release) smoothing rates in 1/s.
    fn rates(self) -> (f32, f32) {
        match self {
            LoopKind::MenuMusic | LoopKind::IngameBase => (4.0, 4.0),
            LoopKind::IngameDrums | LoopKind::IngameRush => (1.6, 1.2),
            LoopKind::BedMurmur | LoopKind::BedExcited | LoopKind::BedRoar => (3.0, 3.0),
            LoopKind::StompClap | LoopKind::Chant => (1.4, 0.9),
            LoopKind::Roll => (12.0, 8.0),
        }
    }

    /// Transient loops despawn once they have faded out; resident ones live with the state.
    fn transient(self) -> bool {
        matches!(self, LoopKind::StompClap | LoopKind::Chant | LoopKind::Roll)
    }

    fn wanted_in(self, state: AppState) -> bool {
        match self {
            LoopKind::MenuMusic => state != AppState::Playing,
            _ => state == AppState::Playing,
        }
    }
}

#[derive(Component)]
struct LoopLayer {
    kind: LoopKind,
    level: f32,
    target: f32,
    speed: f32,
    /// Fading out for good; despawned by the mixer once silent.
    retiring: bool,
}

// ---------------------------------------------------------------------------
// One-shot description
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Shot {
    bus: Bus,
    cat: Cat,
    gain: f32,
    pitch: f32,
    jitter: f32,
    pos: Option<Vec3>,
}

impl Shot {
    fn new(bus: Bus, cat: Cat, gain: f32) -> Self {
        Self {
            bus,
            cat,
            gain,
            pitch: 1.0,
            jitter: 0.04,
            pos: None,
        }
    }

    fn sfx(gain: f32) -> Self {
        Self::new(Bus::Sfx, Cat::Impact, gain)
    }

    fn crowd(gain: f32) -> Self {
        Self::new(Bus::Crowd, Cat::Crowd, gain).jitter(0.03)
    }

    fn stinger(gain: f32) -> Self {
        Self::new(Bus::Sfx, Cat::Stinger, gain).jitter(0.0)
    }

    fn ui(gain: f32) -> Self {
        Self::new(Bus::Ui, Cat::Ui, gain).jitter(0.0)
    }

    fn cat(mut self, cat: Cat) -> Self {
        self.cat = cat;
        self
    }

    fn pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch;
        self
    }

    fn jitter(mut self, jitter: f32) -> Self {
        self.jitter = jitter;
        self
    }

    fn at(mut self, pos: Vec3) -> Self {
        self.pos = Some(pos);
        self
    }
}

/// Everything a system needs to make noise.
#[derive(SystemParam)]
struct Sfx<'w, 's> {
    commands: Commands<'w, 's>,
    sounds: Option<Res<'w, Sounds>>,
    gate: Res<'w, AudioGate>,
    mix: ResMut<'w, AudioMix>,
    live: Query<'w, 's, (), With<OneShot>>,
    loops: Query<'w, 's, (Entity, &'static mut LoopLayer)>,
}

impl Sfx<'_, '_> {
    fn ready(&self) -> bool {
        self.gate.unlocked && self.sounds.is_some()
    }

    fn has_loop(&self, kind: LoopKind) -> bool {
        self.loops
            .iter()
            .any(|(_, l)| l.kind == kind && !l.retiring)
            || self.mix.spawned_loops.contains(&kind)
    }

    /// Fade out every looping layer that does not belong in `state`.
    fn retire_loops_for(&mut self, state: AppState) {
        for (_, mut layer) in &mut self.loops {
            if !layer.kind.wanted_in(state) && !layer.retiring {
                layer.retiring = true;
                layer.target = 0.0;
            }
        }
    }

    fn play(&mut self, clip: Clip, shot: Shot) {
        let Some(sounds) = self.sounds.as_deref() else {
            return;
        };
        if !self.gate.unlocked {
            return;
        }
        let live = self.live.iter().count();
        if !self.mix.budget.try_take(shot.cat, live) {
            return;
        }
        let mut gain = shot.gain * self.mix.bus_volume(shot.bus) * self.mix.range(0.9, 1.0);
        let speed = shot.pitch * (1.0 + shot.jitter * self.mix.range(-1.0, 1.0));
        let pan = shot.pos.and_then(|p| {
            self.mix.cam.map(|cam| {
                let (pan, dist_gain) = pan_from_camera(p, cam.pos, cam.right);
                gain *= dist_gain;
                pan
            })
        });
        let gain = gain.clamp(0.0, 1.0);
        let handle = sounds.get(clip);
        match pan {
            Some(pan) => {
                let tf = Transform::from_translation(pan_emitter_position(pan));
                self.commands.spawn((
                    OneShot,
                    AudioPlayer::new(handle),
                    PlaybackSettings::DESPAWN
                        .with_volume(Volume::Linear((gain * PAN_MAKEUP).min(1.0)))
                        .with_speed(speed)
                        .with_spatial(true)
                        .with_spatial_scale(SpatialScale::new(1.0)),
                    tf,
                    GlobalTransform::from(tf),
                ));
            }
            None => {
                self.commands.spawn((
                    OneShot,
                    AudioPlayer::new(handle),
                    PlaybackSettings::DESPAWN
                        .with_volume(Volume::Linear(gain))
                        .with_speed(speed),
                ));
            }
        }
    }

    fn queue(&mut self, delay: f32, clip: Clip, shot: Shot) {
        if !self.ready() {
            return;
        }
        self.commands.spawn(Pending { delay, clip, shot });
    }

    /// Fade a looping layer toward `target` (spawning it silently if needed).
    fn set_loop(&mut self, kind: LoopKind, target: f32, speed: f32) {
        let target = target.clamp(0.0, 1.0);
        for (_, mut layer) in &mut self.loops {
            if layer.kind == kind && !layer.retiring {
                layer.target = target;
                layer.speed = speed;
                return;
            }
        }
        // Spawned earlier this frame by another system; the command has not applied yet.
        if self.mix.spawned_loops.contains(&kind) {
            return;
        }
        if target <= 0.0 || !self.ready() {
            return;
        }
        let Some(sounds) = self.sounds.as_deref() else {
            return;
        };
        self.mix.spawned_loops.push(kind);
        self.commands.spawn((
            LoopLayer {
                kind,
                level: 0.0,
                target,
                speed,
                retiring: false,
            },
            AudioPlayer::new(sounds.get(kind.clip())),
            PlaybackSettings::LOOP
                .with_volume(Volume::Linear(0.0))
                .with_speed(speed),
        ));
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested)
// ---------------------------------------------------------------------------

/// Ear gap of the pan-space listener. Small on purpose: rodio's spatial law only pans through
/// the distance term, so a narrow head gives a broadcast-style partial pan (~0.5 / 0.9).
const PAN_EAR_GAP: f32 = 0.1;
/// rodio attenuates the centre position to ~0.75 per channel; make up for it.
const PAN_MAKEUP: f32 = 1.3;
/// Court half-width (in metres) that maps to a full pan.
const PAN_WIDTH_M: f32 = 11.0;

fn xorshift(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    if x == 0 {
        0x9E37_79B9
    } else {
        x
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Exponential approach with separate attack/release rates.
fn approach(current: f32, target: f32, dt: f32, attack: f32, release: f32) -> f32 {
    let rate = if target > current { attack } else { release };
    current + (target - current) * (1.0 - (-rate * dt).exp())
}

/// Screen-relative pan (-1 = hard left, 1 = hard right) and a gentle distance roll-off.
fn pan_from_camera(pos: Vec3, cam_pos: Vec3, cam_right: Vec3) -> (f32, f32) {
    let rel = pos - cam_pos;
    let right = Vec3::new(cam_right.x, 0.0, cam_right.z).normalize_or_zero();
    let pan = (rel.dot(right) / PAN_WIDTH_M).clamp(-1.0, 1.0);
    let dist = rel.length();
    let dist_gain = (1.0 - (dist - 14.0) / 45.0).clamp(0.72, 1.0);
    (pan, dist_gain)
}

/// Emitter position in pan space: unit distance from the origin listener.
///
/// rodio 0.20's `Spatial` computes `left = ((dL - dR)/gap + 1)/4 + 0.5` — the *farther* ear
/// gets the larger "diff" factor, which is only undone by the `1/d²` distance term. At unit
/// distance the closer ear clamps to 1.0 while the far ear is attenuated, so to make a
/// right-hand sound come out of the right speaker the emitter is mirrored onto -x.
fn pan_emitter_position(pan: f32) -> Vec3 {
    let x = -pan.clamp(-1.0, 1.0) * 0.98;
    let z = -(1.0 - x * x).max(0.0).sqrt();
    Vec3::new(x, 0.0, z)
}

/// Faithful copy of rodio 0.20 `Spatial::set_positions` (left, right channel gains).
/// Kept here so the mirroring above is pinned by a test.
#[cfg(test)]
fn rodio_pan_gains(emitter: Vec3, gap: f32) -> (f32, f32) {
    let left_ear = Vec3::new(-gap / 2.0, 0.0, 0.0);
    let right_ear = Vec3::new(gap / 2.0, 0.0, 0.0);
    let l2 = left_ear.distance_squared(emitter);
    let r2 = right_ear.distance_squared(emitter);
    let max_diff = left_ear.distance(right_ear);
    let (ld, rd) = (l2.sqrt(), r2.sqrt());
    let l_diff = (((ld - rd) / max_diff + 1.0) / 4.0 + 0.5).min(1.0);
    let r_diff = (((rd - ld) / max_diff + 1.0) / 4.0 + 0.5).min(1.0);
    let l_dist = (1.0 / l2).min(1.0);
    let r_dist = (1.0 / r2).min(1.0);
    (l_diff * l_dist, r_diff * r_dist)
}

/// Crossfade weights for the murmur / excited / roar beds at a given excitement.
/// The clips are peak-normalised alike, so the weights also carry the loudness curve:
/// a calm bowl sits well under the music, a roaring one climbs on top of it.
fn bed_weights(excite: f32) -> [f32; 3] {
    let e = excite.clamp(0.0, 1.0);
    let murmur = (1.0 - smoothstep(0.15, 0.7, e) * 0.85) * 0.55;
    let excited = smoothstep(0.08, 0.42, e) * (1.0 - smoothstep(0.6, 1.0, e) * 0.55) * 0.85;
    let roar = smoothstep(0.42, 0.9, e);
    [murmur, excited, roar]
}

/// Floor bounce: (variant index 0 = hard .. 4 = soft, gain) from vertical impact speed.
fn bounce_variant(speed: f32) -> (usize, f32) {
    let idx = ((11.0 - speed) / 2.3).floor().clamp(0.0, 4.0) as usize;
    let gain = (0.3 + speed * 0.065).min(1.0);
    (idx, gain)
}

/// Shot-clock tick pitch rises as the seconds run out (5 → 1).
fn shot_tick_pitch(seconds_left: i32) -> f32 {
    1.0 + (5 - seconds_left.clamp(1, 5)) as f32 * 0.09
}

fn lead_sign(home: u32, away: u32) -> i8 {
    match home.cmp(&away) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
    }
}

/// Excitement the crowd bed should sit at given the match situation.
fn crowd_excite_target(
    hype: f32,
    flash: f32,
    shot_clock: f32,
    remaining: f32,
    late_quarter: bool,
    on_fire: bool,
) -> f32 {
    let mut e = hype.max(flash * 0.8);
    if shot_clock <= 6.0 {
        e = e.max(0.32);
    }
    if late_quarter && remaining <= 30.0 {
        e = e.max(0.5);
    }
    if on_fire {
        e = e.max(0.55);
    }
    e.clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Startup
// ---------------------------------------------------------------------------

fn preload(mut commands: Commands, assets: Res<AssetServer>) {
    let handles = Clip::ALL.iter().map(|c| assets.load(c.path())).collect();
    commands.insert_resource(Sounds { handles });
}

fn spawn_pan_listener(mut commands: Commands) {
    commands.spawn((
        PanListener,
        SpatialListener::new(PAN_EAR_GAP),
        Transform::IDENTITY,
        GlobalTransform::IDENTITY,
    ));
}

fn unlock_on_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    pads: Query<&Gamepad>,
    mut gate: ResMut<AudioGate>,
) {
    if gate.unlocked {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (keys, mouse, pads);
        gate.unlocked = true;
        return;
    }
    #[cfg(target_arch = "wasm32")]
    {
        let pad = pads
            .iter()
            .any(|p| p.get_just_pressed().next().is_some() || p.left_stick().length() > 0.35);
        if keys.get_just_pressed().next().is_some()
            || mouse.get_just_pressed().next().is_some()
            || pad
        {
            gate.unlocked = true;
        }
    }
}

fn track_listener(cam: Query<&GlobalTransform, With<GameCam>>, mut mix: ResMut<AudioMix>) {
    if let Some(gt) = cam.iter().next() {
        mix.cam = Some(CamFrame {
            pos: gt.translation(),
            right: *gt.right(),
        });
    }
}

// ---------------------------------------------------------------------------
// Music + beds per state
// ---------------------------------------------------------------------------

fn music_for_state(
    state: Res<State<AppState>>,
    audio: Res<Assets<AudioSource>>,
    mut sfx: Sfx,
) {
    if !sfx.ready() {
        return;
    }
    let st = *state.get();
    sfx.retire_loops_for(st);
    if st == AppState::Playing {
        // The three stems must start on the same frame to stay phase-locked, so wait until
        // every one of them is decoded. (Crowd beds are owned by `crowd_dynamics`.)
        let stems = [
            LoopKind::IngameBase,
            LoopKind::IngameDrums,
            LoopKind::IngameRush,
        ];
        if !sfx.has_loop(LoopKind::IngameBase) {
            let loaded = stems.iter().all(|k| {
                sfx.sounds
                    .as_deref()
                    .map(|s| audio.contains(s.get(k.clip()).id()))
                    .unwrap_or(false)
            });
            if loaded {
                sfx.set_loop(LoopKind::IngameBase, 1.0, 1.0);
                sfx.set_loop(LoopKind::IngameDrums, 0.001, 1.0);
                sfx.set_loop(LoopKind::IngameRush, 0.001, 1.0);
            }
        }
    } else {
        let level = if st == AppState::GameOver { 0.6 } else { 0.9 };
        sfx.set_loop(LoopKind::MenuMusic, level, 1.0);
    }
}

// ---------------------------------------------------------------------------
// Ball
// ---------------------------------------------------------------------------

fn side_of(players: &Query<&Player>, e: Option<Entity>) -> Option<Side> {
    e.and_then(|e| players.get(e).ok()).map(|p| p.side)
}

fn bucket_sounds(
    mut buckets: MessageReader<BucketEvent>,
    ball: Query<&Transform, With<Ball>>,
    players: Query<&Player>,
    mut sfx: Sfx,
) {
    for ev in buckets.read() {
        let pos = ball.iter().next().map(|t| t.translation);
        let side = side_of(&players, ev.shooter).unwrap_or(if ev.hoop_home {
            Side::Away
        } else {
            Side::Home
        });
        let home = side == Side::Home;
        let at = |s: Shot| match pos {
            Some(p) => s.at(p),
            None => s,
        };
        if ev.dunk {
            sfx.play(Clip::DunkBoom, at(Shot::sfx(1.0).jitter(0.03)));
            sfx.play(Clip::Poster, Shot::stinger(0.85));
            sfx.queue(0.12, Clip::RimBack, at(Shot::sfx(0.5).pitch(0.96)));
            sfx.mix.duck_music(0.12, 0.9);
            sfx.mix.excite_kick(if home { 1.0 } else { 0.55 });
        } else {
            let (clip, gain) = if ev.is_three {
                (Clip::Swish, 0.95)
            } else if sfx.mix.chance(0.5) {
                (Clip::Swish, 0.85)
            } else {
                (Clip::SwishSoft, 0.9)
            };
            sfx.play(clip, at(Shot::sfx(gain).jitter(0.05)));
            if ev.is_three {
                sfx.play(Clip::Downtown, Shot::stinger(0.7));
            }
            sfx.mix.duck_music(0.2, 0.55);
            sfx.mix.excite_kick(if home {
                if ev.is_three {
                    0.95
                } else {
                    0.75
                }
            } else {
                0.4
            });
        }
        // Home crowd: bigger for the highlight plays. Away bucket: polite, with some boos.
        if home {
            let cheer = if ev.dunk || ev.is_three {
                Clip::CheerHuge
            } else {
                Clip::CheerBig
            };
            sfx.play(cheer, Shot::crowd(1.0));
            if ev.dunk {
                sfx.queue(0.3, Clip::Whistles, Shot::crowd(0.5));
            }
        } else {
            sfx.play(Clip::CheerSmall, Shot::crowd(0.55));
            sfx.queue(0.25, Clip::Boo, Shot::crowd(0.5));
        }
    }
}

#[derive(Default)]
struct CollisionCool {
    rim: f32,
    board: f32,
    floor: f32,
    oooh: f32,
    rattle_lock: f32,
}

fn collision_sounds(
    time: Res<Time>,
    mut rims: MessageReader<RimHitEvent>,
    mut boards: MessageReader<BackboardHitEvent>,
    mut floors: MessageReader<FloorBounceEvent>,
    ball: Query<(&BallVel, &BallState), With<Ball>>,
    mut cool: Local<CollisionCool>,
    mut sfx: Sfx,
) {
    let dt = time.delta_secs();
    cool.rim = (cool.rim - dt).max(0.0);
    cool.board = (cool.board - dt).max(0.0);
    cool.floor = (cool.floor - dt).max(0.0);
    cool.oooh = (cool.oooh - dt).max(0.0);
    cool.rattle_lock = (cool.rattle_lock - dt).max(0.0);
    let (ball_speed, rim_hits) = ball
        .iter()
        .next()
        .map(|(v, s)| (v.0.length(), s.rim_hits))
        .unwrap_or((0.0, 0));

    for ev in rims.read() {
        if cool.rim > 0.0 || cool.rattle_lock > 0.0 {
            continue;
        }
        cool.rim = 0.07;
        let hard = ev.speed > 6.5;
        if rim_hits >= 2 {
            // The ball is dancing on the iron: one designed rattle instead of a clank pile-up.
            cool.rattle_lock = 0.85;
            sfx.play(Clip::Rattle, Shot::sfx(0.85).at(ev.pos).jitter(0.05));
            if cool.oooh <= 0.0 {
                cool.oooh = 2.2;
                sfx.play(Clip::Oooh, Shot::crowd(0.8));
                sfx.mix.excite_kick(0.45);
            }
            continue;
        }
        let clip = if hard {
            Clip::RimFront
        } else if sfx.mix.chance(0.55) {
            Clip::RimBack
        } else {
            Clip::RimSoft
        };
        let gain = (0.42 + ev.speed * 0.05).min(1.0);
        sfx.play(clip, Shot::sfx(gain).at(ev.pos).jitter(0.06));
        if hard && cool.oooh <= 0.0 {
            cool.oooh = 2.2;
            sfx.play(Clip::Oooh, Shot::crowd(0.65));
            sfx.mix.excite_kick(0.35);
        }
    }
    for ev in boards.read() {
        if cool.board > 0.0 {
            continue;
        }
        cool.board = 0.1;
        let clip = if ball_speed > 7.0 {
            Clip::BackboardHard
        } else {
            Clip::Backboard
        };
        let gain = (0.5 + ball_speed * 0.04).min(1.0);
        sfx.play(clip, Shot::sfx(gain).at(ev.pos).jitter(0.05));
    }
    for ev in floors.read() {
        if cool.floor > 0.0 {
            continue;
        }
        cool.floor = 0.06;
        let (idx, gain) = bounce_variant(ev.speed);
        let clip = [
            Clip::Bounce1,
            Clip::Bounce2,
            Clip::Bounce3,
            Clip::Bounce4,
            Clip::Bounce5,
        ][idx];
        sfx.play(
            clip,
            Shot::sfx(gain).cat(Cat::Bounce).at(ev.pos).jitter(0.07),
        );
    }
}

struct BallTrack {
    prev_hold: Hold,
    prev_speed: f32,
    prev_pos: Vec3,
    miss_flagged: bool,
    was_three: bool,
}

impl Default for BallTrack {
    fn default() -> Self {
        Self {
            prev_hold: Hold::Loose,
            prev_speed: 0.0,
            prev_pos: Vec3::ZERO,
            miss_flagged: false,
            was_three: false,
        }
    }
}

fn ball_state_sounds(
    state: Res<State<AppState>>,
    paused: Res<Paused>,
    ball: Query<(&Transform, &BallVel, &BallState), With<Ball>>,
    players: Query<&Player>,
    poses: Query<&Pose>,
    mut buckets: MessageReader<BucketEvent>,
    mut track: Local<BallTrack>,
    mut sfx: Sfx,
) {
    let scored = buckets.read().count() > 0;
    let Some((tf, vel, st)) = ball.iter().next() else {
        sfx.set_loop(LoopKind::Roll, 0.0, 1.0);
        *track = BallTrack::default();
        return;
    };
    if *state.get() != AppState::Playing || paused.0 {
        sfx.set_loop(LoopKind::Roll, 0.0, 1.0);
        return;
    }
    let pos = tf.translation;
    let speed = vel.0.length();
    let prev = track.prev_hold;
    let hold = st.hold;

    if hold != prev {
        match (prev, hold) {
            (Hold::Held, Hold::Pass) => {
                let pitch = (0.85 + speed * 0.03).clamp(0.8, 1.35);
                sfx.play(
                    Clip::PassWhoosh,
                    Shot::sfx(0.55).cat(Cat::Whoosh).pitch(pitch).jitter(0.05).at(pos),
                );
            }
            (Hold::Held, Hold::Shot) => {
                let dunking = st
                    .shooter
                    .and_then(|e| poses.get(e).ok())
                    .map(|p| *p == Pose::Dunk)
                    .unwrap_or(false);
                if !dunking {
                    sfx.play(Clip::ShotFlick, Shot::sfx(0.7).cat(Cat::Whoosh).at(pos));
                    sfx.play(
                        Clip::PassWhoosh,
                        Shot::sfx(0.22).cat(Cat::Whoosh).pitch(1.35).at(pos),
                    );
                    let home = side_of(&players, st.shooter) == Some(Side::Home);
                    if st.release_was_three && home {
                        sfx.play(Clip::Anticipation, Shot::crowd(0.7));
                    }
                }
                track.miss_flagged = false;
                track.was_three = st.release_was_three;
            }
            (Hold::Loose | Hold::Shot | Hold::Pass, Hold::Held) => {
                let gain = (0.35 + track.prev_speed * 0.05).min(0.9);
                sfx.play(Clip::Catch, Shot::sfx(gain).cat(Cat::Foley).jitter(0.06).at(pos));
            }
            _ => {}
        }
    }

    // A shot dropping below the rim without a bucket is a miss: the bowl reacts.
    if hold == Hold::Shot && !track.miss_flagged && !scored {
        if vel.0.y < 0.0 && pos.y < RIM_HEIGHT - 0.55 && track.prev_pos.y >= RIM_HEIGHT - 0.55 {
            track.miss_flagged = true;
            let home = side_of(&players, st.shooter) == Some(Side::Home);
            if home {
                if st.rim_hits == 0 {
                    sfx.play(Clip::Groan, Shot::crowd(0.55));
                } else {
                    sfx.play(Clip::Oooh, Shot::crowd(0.5));
                }
            } else {
                sfx.play(Clip::CheerSmall, Shot::crowd(0.5));
            }
        }
    }
    if hold != Hold::Shot {
        track.miss_flagged = false;
    }

    // Loose ball rolling on the hardwood.
    let horiz = Vec3::new(vel.0.x, 0.0, vel.0.z).length();
    let grounded = pos.y < VISUAL_RADIUS + 0.05;
    let rolling = hold == Hold::Loose && grounded && horiz > 0.3 && horiz < 7.5;
    if rolling {
        let level = (horiz / 5.0).clamp(0.15, 1.0) * 0.8;
        let rate = (0.85 + horiz * 0.06).clamp(0.8, 1.3);
        sfx.set_loop(LoopKind::Roll, level, rate);
    } else {
        sfx.set_loop(LoopKind::Roll, 0.0, 1.0);
    }

    track.prev_hold = hold;
    track.prev_speed = speed;
    track.prev_pos = pos;
}

fn dribble_sounds(
    time: Res<Time>,
    mut ticks: MessageReader<DribbleTickEvent>,
    ball: Query<&BallState, With<Ball>>,
    movers: Query<&MoveVel>,
    mut cool: Local<f32>,
    mut sfx: Sfx,
) {
    *cool = (*cool - time.delta_secs()).max(0.0);
    let holder_speed = ball
        .iter()
        .next()
        .and_then(|s| s.holder)
        .and_then(|h| movers.get(h).ok())
        .map(|v| Vec3::new(v.0.x, 0.0, v.0.z).length())
        .unwrap_or(0.0);
    for ev in ticks.read() {
        if *cool > 0.0 {
            continue;
        }
        *cool = 0.07;
        let clip = match (sfx.mix.rand() * 3.0) as u32 {
            0 => Clip::Dribble1,
            1 => Clip::Dribble2,
            _ => Clip::Dribble3,
        };
        let gain = (0.38 + holder_speed * 0.035).min(0.75);
        sfx.play(
            clip,
            Shot::sfx(gain).cat(Cat::Bounce).jitter(0.06).at(ev.pos),
        );
    }
}

// ---------------------------------------------------------------------------
// Players
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct PlayerTrack {
    pose: Pose,
    stride: f32,
    on_fire: bool,
}

fn player_foley(
    time: Res<Time>,
    state: Res<State<AppState>>,
    paused: Res<Paused>,
    players: Query<(Entity, &Transform, &MoveVel, &Pose, &Heat, &Player)>,
    mut cuts: MessageReader<CutSqueak>,
    mut tracks: Local<HashMap<Entity, PlayerTrack>>,
    mut cut_cool: Local<f32>,
    mut sfx: Sfx,
) {
    let dt = time.delta_secs();
    *cut_cool = (*cut_cool - dt).max(0.0);
    if *state.get() != AppState::Playing {
        tracks.clear();
        sfx.mix.any_on_fire = false;
        return;
    }
    let mut any_fire = false;
    for (e, tf, vel, pose, heat, player) in &players {
        let pos = tf.translation;
        let speed = Vec3::new(vel.0.x, 0.0, vel.0.z).length();
        let fire = heat.on_fire();
        any_fire |= fire;
        let mut tr = tracks.get(&e).copied().unwrap_or(PlayerTrack {
            pose: *pose,
            stride: sfx.mix.rand(),
            on_fire: fire,
        });
        if paused.0 {
            tracks.insert(e, tr);
            continue;
        }

        // Footsteps: stride phase accumulates with distance covered.
        if speed > 1.2 && matches!(*pose, Pose::Idle | Pose::Run | Pose::Sprint) {
            tr.stride += dt * speed * 0.6;
            if tr.stride >= 1.0 {
                tr.stride -= 1.0;
                let clip = match (sfx.mix.rand() * 3.0) as u32 {
                    0 => Clip::Step1,
                    1 => Clip::Step2,
                    _ => Clip::Step3,
                };
                let gain = (0.1 + speed * 0.035).min(0.4) * if player.human { 1.1 } else { 0.85 };
                sfx.play(
                    clip,
                    Shot::sfx(gain).cat(Cat::Foley).jitter(0.09).at(pos),
                );
            }
        } else {
            tr.stride = tr.stride.min(0.6);
        }

        // Pose transitions.
        if *pose != tr.pose {
            match *pose {
                Pose::Dunk => {
                    sfx.play(Clip::JumpGrunt, Shot::sfx(0.7).cat(Cat::Foley).jitter(0.07).at(pos));
                    sfx.play(
                        Clip::SqueakShort1,
                        Shot::sfx(0.45).cat(Cat::Foley).jitter(0.1).at(pos),
                    );
                }
                Pose::Block => {
                    sfx.play(Clip::JumpGrunt, Shot::sfx(0.55).cat(Cat::Foley).pitch(1.08).at(pos));
                    sfx.play(
                        Clip::SqueakShort2,
                        Shot::sfx(0.4).cat(Cat::Foley).jitter(0.1).at(pos),
                    );
                }
                Pose::Shoot => {
                    sfx.play(Clip::SqueakShort2, Shot::sfx(0.25).cat(Cat::Foley).jitter(0.12).at(pos));
                }
                Pose::Stumble => {
                    sfx.play(Clip::BodyThud, Shot::sfx(0.7).cat(Cat::Impact).jitter(0.06).at(pos));
                    sfx.play(Clip::SqueakLong, Shot::sfx(0.35).cat(Cat::Foley).jitter(0.08).at(pos));
                }
                _ => {}
            }
            match tr.pose {
                Pose::Dunk => sfx.play(Clip::Land, Shot::sfx(0.8).cat(Cat::Foley).at(pos)),
                Pose::Block => sfx.play(Clip::Land, Shot::sfx(0.5).cat(Cat::Foley).at(pos)),
                Pose::Shoot => sfx.play(Clip::Land, Shot::sfx(0.32).cat(Cat::Foley).pitch(1.1).at(pos)),
                _ => {}
            }
            tr.pose = *pose;
        }

        // Heat check: someone just caught fire.
        if fire && !tr.on_fire {
            sfx.play(Clip::Airhorn, Shot::crowd(0.9));
            sfx.play(Clip::OnFire, Shot::stinger(0.9));
            sfx.queue(0.35, Clip::Whistles, Shot::crowd(0.6));
            sfx.mix.duck_music(0.3, 1.1);
            sfx.mix.excite_kick(0.9);
        }
        tr.on_fire = fire;
        tracks.insert(e, tr);
    }
    tracks.retain(|e, _| players.get(*e).is_ok());
    sfx.mix.any_on_fire = any_fire;

    for ev in cuts.read() {
        if *cut_cool > 0.0 {
            continue;
        }
        *cut_cool = 0.12;
        let clip = if sfx.mix.chance(0.5) {
            Clip::SqueakShort1
        } else {
            Clip::SqueakShort2
        };
        sfx.play(clip, Shot::sfx(0.5).cat(Cat::Foley).jitter(0.1).at(ev.pos));
    }
}

// ---------------------------------------------------------------------------
// Game events
// ---------------------------------------------------------------------------

#[derive(Default)]
struct TickerTrack {
    line: String,
    age: f32,
}

fn game_event_sounds(
    mut steals: MessageReader<StealEvent>,
    mut viol: MessageReader<ViolationEvent>,
    mut tips: MessageReader<TipWhistle>,
    mut plays: MessageReader<PlayCall>,
    ticker: Res<Ticker>,
    ball: Query<(&Transform, &BallState), With<Ball>>,
    players: Query<&Player>,
    mut tt: Local<TickerTrack>,
    mut sfx: Sfx,
) {
    let ball_pos = ball.iter().next().map(|(t, _)| t.translation);
    let last_touch_home = ball
        .iter()
        .next()
        .and_then(|(_, s)| side_of(&players, s.last_touch))
        .map(|s| s == Side::Home);

    for ev in steals.read() {
        if ev.success {
            sfx.play(Clip::StealRip, Shot::sfx(0.9).at(ev.pos));
            sfx.play(Clip::SqueakShort1, Shot::sfx(0.5).cat(Cat::Foley).jitter(0.1).at(ev.pos));
            // The thief is the controlled (home) player.
            sfx.play(Clip::CheerSmall, Shot::crowd(0.8));
            sfx.mix.excite_kick(0.5);
            sfx.mix.duck_music(0.45, 0.35);
        } else {
            sfx.play(Clip::SqueakShort2, Shot::sfx(0.45).cat(Cat::Foley).jitter(0.1).at(ev.pos));
            sfx.play(Clip::BodyThud, Shot::sfx(0.3).cat(Cat::Foley).pitch(1.1).at(ev.pos));
        }
    }
    for _ in viol.read() {
        sfx.play(Clip::BuzzerShort, Shot::new(Bus::Ui, Cat::Stinger, 0.95));
        sfx.mix.duck_music(0.2, 0.9);
        sfx.mix.duck_crowd(0.6, 0.6);
        match last_touch_home {
            Some(false) => sfx.queue(0.3, Clip::CheerSmall, Shot::crowd(0.6)),
            _ => sfx.queue(0.25, Clip::Groan, Shot::crowd(0.85)),
        }
    }
    for _ in tips.read() {
        sfx.play(Clip::WhistleLong, Shot::new(Bus::Ui, Cat::Stinger, 0.8));
        sfx.queue(0.15, Clip::Anthem, Shot::stinger(0.85));
        sfx.queue(0.2, Clip::CheerBig, Shot::crowd(0.7));
        sfx.mix.duck_music(0.25, 3.2);
        sfx.mix.excite_kick(0.6);
    }
    for ev in plays.read() {
        if ev.text.starts_with("POSTERIZE") {
            sfx.play(Clip::Anticipation, Shot::crowd(0.85));
            sfx.mix.excite_kick(0.5);
        }
    }

    // Ticker lines that carry information no message does.
    let fresh = ticker.line != tt.line || ticker.age < tt.age;
    if fresh && ticker.age < 0.2 && ticker.line.starts_with("REJECTED") {
        let shot = match ball_pos {
            Some(p) => Shot::sfx(0.95).at(p),
            None => Shot::sfx(0.95),
        };
        sfx.play(Clip::BlockSlap, shot);
        sfx.queue(0.05, Clip::Gasp, Shot::crowd(0.9));
        sfx.queue(0.5, Clip::CheerSmall, Shot::crowd(0.5));
        sfx.mix.duck_music(0.4, 0.45);
        sfx.mix.excite_kick(0.6);
    }
    tt.line.clone_from(&ticker.line);
    tt.age = ticker.age;
}

// ---------------------------------------------------------------------------
// Broadcast cues derived from the clock, score and state machine
// ---------------------------------------------------------------------------

struct BroadcastTrack {
    prev_state: AppState,
    quarter: u8,
    last_tick_sec: i32,
    crunch_done: bool,
    clutch_done: bool,
    last_leader: i8,
    prev_score: (u32, u32),
}

impl Default for BroadcastTrack {
    fn default() -> Self {
        Self {
            prev_state: AppState::Splash,
            quarter: 1,
            last_tick_sec: 99,
            crunch_done: false,
            clutch_done: false,
            last_leader: 0,
            prev_score: (0, 0),
        }
    }
}

fn broadcast_cues(
    state: Res<State<AppState>>,
    paused: Res<Paused>,
    clock: Res<MatchClock>,
    score: Res<Scoreboard>,
    config: Res<MatchConfig>,
    mut tr: Local<BroadcastTrack>,
    mut sfx: Sfx,
) {
    let st = *state.get();

    // State machine edges.
    if st != tr.prev_state {
        if tr.prev_state == AppState::Playing && st == AppState::GameOver {
            sfx.play(Clip::BuzzerLong, Shot::new(Bus::Ui, Cat::Stinger, 1.0));
            sfx.mix.duck_music(0.1, 4.5);
            let won = score.home > score.away;
            if won {
                sfx.queue(0.9, Clip::FanfareWin, Shot::stinger(0.95));
                sfx.queue(0.4, Clip::CheerHuge, Shot::crowd(1.0));
                sfx.queue(1.2, Clip::Whistles, Shot::crowd(0.6));
                sfx.queue(1.6, Clip::Airhorn, Shot::crowd(0.7));
            } else {
                sfx.queue(0.9, Clip::FanfareLoss, Shot::stinger(0.9));
                sfx.queue(0.4, Clip::Groan, Shot::crowd(0.9));
                sfx.queue(1.3, Clip::Boo, Shot::crowd(0.4));
            }
        }
        if st == AppState::Playing {
            // Fresh match: forget last game's leader/quarter so cues fire correctly.
            tr.quarter = 1;
            tr.last_leader = 0;
            tr.prev_score = (0, 0);
            tr.crunch_done = false;
            tr.clutch_done = false;
            tr.last_tick_sec = 99;
            sfx.mix.rush_target = 0.0;
            sfx.mix.intensity_target = 0.0;
        }
        tr.prev_state = st;
    }
    if st != AppState::Playing {
        return;
    }
    let timed = config.mode != GameMode::Practice;

    // Lead changes (independent of the clock).
    let cur = (score.home, score.away);
    if cur != tr.prev_score {
        let sign = lead_sign(cur.0, cur.1);
        if sign != 0 {
            if tr.last_leader != 0 && sign != tr.last_leader {
                sfx.queue(0.9, Clip::LeadChange, Shot::stinger(0.85));
                sfx.queue(1.0, Clip::CheerSmall, Shot::crowd(0.6));
                sfx.mix.duck_music(0.35, 1.4);
            }
            tr.last_leader = sign;
        }
        tr.prev_score = cur;
    }

    if paused.0 || !timed || !clock.running {
        return;
    }

    // Shot clock: the last five seconds tick, rising in pitch.
    let sec = clock.shot.ceil() as i32;
    if clock.shot > 5.0 {
        tr.last_tick_sec = 99;
    } else if sec >= 1 && sec != tr.last_tick_sec && clock.shot > 0.0 {
        tr.last_tick_sec = sec;
        let pitch = shot_tick_pitch(sec);
        let gain = 0.5 + (5 - sec) as f32 * 0.07;
        sfx.play(Clip::ShotTick, Shot::ui(gain).cat(Cat::Ui).pitch(pitch));
    }

    // Period changes (quarter counter moved).
    if clock.quarter != tr.quarter {
        let overtime = clock.quarter > 4;
        tr.quarter = clock.quarter;
        tr.crunch_done = false;
        tr.clutch_done = false;
        sfx.play(Clip::BuzzerLong, Shot::new(Bus::Ui, Cat::Stinger, 0.9));
        sfx.mix.duck_music(0.2, 1.6);
        sfx.mix.duck_crowd(0.7, 0.8);
        sfx.queue(1.35, Clip::WhistleDouble, Shot::ui(0.55));
        if overtime {
            sfx.queue(1.4, Clip::Clutch, Shot::stinger(0.9));
            sfx.queue(1.4, Clip::CheerBig, Shot::crowd(0.85));
            sfx.mix.rush_target = 1.0;
        } else {
            sfx.queue(1.6, Clip::PossessionChime, Shot::ui(0.5));
            sfx.queue(0.5, Clip::CheerSmall, Shot::crowd(0.5));
        }
    }

    // Crunch time: last 30 s of Q4 / OT — rush stem, dramatic stab, crowd on its feet.
    let late = clock.quarter >= 4;
    if late && clock.remaining <= 30.0 && clock.remaining > 0.0 && !tr.crunch_done {
        tr.crunch_done = true;
        sfx.play(Clip::FinalMinute, Shot::stinger(0.8));
        sfx.mix.duck_music(0.35, 1.2);
        sfx.mix.excite_kick(0.6);
    }
    sfx.mix.rush_target = if late && clock.remaining <= 30.0 { 1.0 } else { 0.0 };

    // Game point: tight score inside the last 15 s.
    let diff = score.home.abs_diff(score.away);
    if late && clock.remaining <= 15.0 && clock.remaining > 0.0 && diff <= 3 && !tr.clutch_done {
        tr.clutch_done = true;
        sfx.play(Clip::Clutch, Shot::stinger(0.85));
        sfx.mix.duck_music(0.3, 1.3);
        sfx.mix.excite_kick(0.75);
    }

    // Music intensity follows the situation: close + late = hot.
    let closeness = 1.0 - (diff as f32 / 8.0).clamp(0.0, 1.0);
    let quarter_frac = 1.0 - (clock.remaining / config.quarter_secs.max(1.0)).clamp(0.0, 1.0);
    let period_weight = (clock.quarter as f32 - 1.0) / 3.0;
    let mut intensity = 0.25 * period_weight.clamp(0.0, 1.0) + 0.35 * quarter_frac * closeness;
    if late {
        intensity += 0.3;
    }
    sfx.mix.intensity_target = intensity.clamp(0.0, 1.0);
}

// ---------------------------------------------------------------------------
// Possession
// ---------------------------------------------------------------------------

#[derive(Default)]
struct PossessionTrack {
    side: Option<bool>,
    organ_cool: f32,
    settled: f32,
}

fn possession_sounds(
    time: Res<Time>,
    state: Res<State<AppState>>,
    clock: Res<MatchClock>,
    ball: Query<&BallState, With<Ball>>,
    players: Query<&Player>,
    mut tr: Local<PossessionTrack>,
    mut sfx: Sfx,
) {
    let dt = time.delta_secs();
    tr.organ_cool = (tr.organ_cool - dt).max(0.0);
    if *state.get() != AppState::Playing {
        tr.side = None;
        tr.settled = 0.0;
        sfx.mix.possession_home = None;
        return;
    }
    tr.settled += dt;
    let Some(st) = ball.iter().next() else {
        return;
    };
    if st.hold != Hold::Held {
        return;
    }
    let Some(side) = side_of(&players, st.holder) else {
        return;
    };
    let home = side == Side::Home;
    sfx.mix.possession_home = Some(home);
    if let Some(prev) = tr.side {
        if prev != home && tr.settled > 0.5 {
            sfx.queue(0.45, Clip::PossessionChime, Shot::ui(0.45));
            if home && tr.organ_cool <= 0.0 && clock.shot > 18.0 && sfx.mix.chance(0.45) {
                tr.organ_cool = 26.0;
                sfx.queue(0.9, Clip::OrganCharge, Shot::new(Bus::Crowd, Cat::Stinger, 0.55));
                sfx.queue(2.1, Clip::CheerSmall, Shot::crowd(0.35));
            }
        }
    }
    tr.side = Some(home);
}

// ---------------------------------------------------------------------------
// Crowd dynamics: bed crossfade, defense chant, on-fire chant
// ---------------------------------------------------------------------------

fn crowd_dynamics(
    time: Res<Time<Real>>,
    state: Res<State<AppState>>,
    paused: Res<Paused>,
    hype: Res<CrowdHype>,
    juice: Res<ScreenJuice>,
    clock: Res<MatchClock>,
    config: Res<MatchConfig>,
    mut sfx: Sfx,
) {
    let dt = time.delta_secs();
    if *state.get() != AppState::Playing {
        sfx.mix.excite_target = 0.0;
        sfx.mix.excite = approach(sfx.mix.excite, 0.0, dt, 2.0, 2.0);
        return;
    }
    let timed = config.mode != GameMode::Practice;
    let late = timed && clock.quarter >= 4;
    let on_fire = sfx.mix.any_on_fire;
    let target = crowd_excite_target(
        hype.level,
        juice.flash,
        if timed { clock.shot } else { 24.0 },
        clock.remaining,
        late,
        on_fire,
    );
    sfx.mix.excite_target = target;
    sfx.mix.excite = approach(sfx.mix.excite, target, dt, 5.0, 0.7);
    let w = bed_weights(sfx.mix.excite);
    sfx.set_loop(LoopKind::BedMurmur, w[0], 1.0);
    sfx.set_loop(LoopKind::BedExcited, w[1], 1.0);
    sfx.set_loop(LoopKind::BedRoar, w[2], 1.0);

    // DE-FENSE: opponent has the rock with the shot clock winding down, or crunch time.
    let defense = timed
        && !paused.0
        && ((clock.shot <= 7.0 && sfx.mix.possession_home == Some(false))
            || (late && clock.remaining <= 30.0 && clock.remaining > 0.0));
    sfx.set_loop(LoopKind::StompClap, if defense { 0.9 } else { 0.0 }, 1.0);

    // Somebody is on fire: the building sings.
    sfx.set_loop(LoopKind::Chant, if on_fire && !paused.0 { 0.85 } else { 0.0 }, 1.0);
}

// ---------------------------------------------------------------------------
// UI
// ---------------------------------------------------------------------------

fn pause_sounds(
    paused: Res<Paused>,
    state: Res<State<AppState>>,
    mut prev: Local<bool>,
    mut sfx: Sfx,
) {
    if *state.get() == AppState::Playing && paused.0 != *prev {
        let clip = if paused.0 { Clip::Pause } else { Clip::Unpause };
        sfx.play(clip, Shot::ui(0.7));
    }
    *prev = paused.0;
}

fn play_ui_clicks(mut clicks: MessageReader<UiClick>, mut sfx: Sfx) {
    for ev in clicks.read() {
        let clip = if ev.confirm { Clip::Confirm } else { Clip::Blip };
        sfx.play(clip, Shot::ui(0.65).jitter(0.02));
    }
}

// ---------------------------------------------------------------------------
// Scheduler + mixer
// ---------------------------------------------------------------------------

fn fire_pending(
    time: Res<Time<Real>>,
    mut pending: Query<(Entity, &mut Pending)>,
    mut sfx: Sfx,
) {
    let dt = time.delta_secs();
    let mut due = Vec::new();
    for (e, mut p) in &mut pending {
        p.delay -= dt;
        if p.delay <= 0.0 {
            due.push((e, p.clip, p.shot));
        }
    }
    for (e, clip, shot) in due {
        sfx.play(clip, shot);
        sfx.commands.entity(e).despawn();
    }
}

fn duck_and_mix(
    time: Res<Time<Real>>,
    paused: Res<Paused>,
    state: Res<State<AppState>>,
    mut commands: Commands,
    mut mix: ResMut<AudioMix>,
    mut layers: Query<(Entity, &mut LoopLayer, Option<&mut AudioSink>)>,
) {
    let dt = time.delta_secs().min(0.1);

    // Sidechain: fast dive (the "release" rate applies when heading down), slow recovery.
    mix.duck_hold = (mix.duck_hold - dt).max(0.0);
    if mix.duck_hold <= 0.0 {
        mix.duck_target = 1.0;
    }
    mix.duck = approach(mix.duck, mix.duck_target, dt, 3.2, 18.0);
    mix.crowd_duck_hold = (mix.crowd_duck_hold - dt).max(0.0);
    if mix.crowd_duck_hold <= 0.0 {
        mix.crowd_duck_target = 1.0;
    }
    mix.crowd_duck = approach(mix.crowd_duck, mix.crowd_duck_target, dt, 2.5, 14.0);

    // Music intensity -> stem levels.
    mix.intensity = approach(mix.intensity, mix.intensity_target, dt, 0.6, 0.35);
    let playing = *state.get() == AppState::Playing;
    let in_pause = paused.0 && playing;
    let pause_duck = if in_pause { 0.4 } else { 1.0 };
    let drums_level = smoothstep(0.22, 0.75, mix.intensity).max(mix.rush_target * 0.8);
    let rush_level = mix.rush_target.max(smoothstep(0.85, 1.0, mix.intensity) * 0.6);

    for (e, mut layer, sink) in &mut layers {
        if !layer.retiring {
            match layer.kind {
                LoopKind::IngameDrums => layer.target = drums_level,
                LoopKind::IngameRush => layer.target = rush_level,
                _ => {}
            }
        }
        let (attack, release) = layer.kind.rates();
        layer.level = approach(layer.level, layer.target, dt, attack, release);
        let faded = layer.target <= 0.0 && layer.level < 0.004;
        if faded && (layer.retiring || layer.kind.transient()) {
            commands.entity(e).despawn();
            continue;
        }
        let bus = layer.kind.bus();
        let duck = match bus {
            Bus::Music => mix.duck * pause_duck,
            Bus::Crowd => mix.crowd_duck * if in_pause { 0.55 } else { 1.0 },
            _ => pause_duck,
        };
        let vol = (mix.bus_volume(bus) * layer.level * duck).clamp(0.0, 1.0);
        if let Some(mut sink) = sink {
            sink.set_volume(Volume::Linear(vol));
            if layer.kind == LoopKind::Roll {
                sink.set_speed(layer.speed);
            }
        }
    }

    mix.budget.reset();
    mix.spawned_loops.clear();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the plugin into a headless app and ticks it. Bevy validates system parameter
    /// access (e.g. two queries fighting over `&mut LoopLayer`) only when the schedule runs,
    /// so this catches a class of bug `cargo check` cannot.
    #[test]
    fn plugin_schedules_and_ticks_headless() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::state::app::StatesPlugin,
            bevy::audio::AudioPlugin::default(),
        ));
        app.init_state::<AppState>()
            .insert_resource(MatchConfig::default())
            .insert_resource(Paused(false))
            .init_resource::<MatchClock>()
            .init_resource::<Scoreboard>()
            .init_resource::<Ticker>()
            .init_resource::<CrowdHype>()
            .init_resource::<ScreenJuice>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .add_message::<BucketEvent>()
            .add_message::<RimHitEvent>()
            .add_message::<BackboardHitEvent>()
            .add_message::<FloorBounceEvent>()
            .add_message::<PlayCall>()
            .add_message::<DribbleTickEvent>()
            .add_message::<StealEvent>()
            .add_message::<ViolationEvent>()
            .add_message::<TipWhistle>()
            .add_message::<CutSqueak>()
            .add_plugins(FinnAudioPlugin);
        for _ in 0..3 {
            app.update();
        }
        // Drive it through a match start and a few frames of play with events flowing.
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Playing);
        app.update();
        app.world_mut().write_message(TipWhistle);
        app.world_mut().write_message(BucketEvent {
            shooter: None,
            hoop_home: false,
            dunk: true,
            is_three: false,
        });
        app.world_mut().write_message(RimHitEvent {
            pos: Vec3::new(12.0, 3.0, 0.0),
            speed: 7.0,
        });
        app.world_mut().write_message(FloorBounceEvent {
            pos: Vec3::ZERO,
            speed: 4.0,
        });
        app.world_mut().write_message(ViolationEvent);
        app.world_mut().write_message(UiClick { confirm: true });
        app.world_mut().resource_mut::<MatchClock>().shot = 4.5;
        for _ in 0..4 {
            app.update();
        }
        let mix = app.world().resource::<AudioMix>();
        assert!(mix.duck < 1.0, "a dunk + violation must have ducked the music");
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::GameOver);
        for _ in 0..3 {
            app.update();
        }
    }

    #[test]
    fn clip_table_indices_match_discriminants() {
        for (i, c) in Clip::ALL.iter().enumerate() {
            assert_eq!(*c as usize, i, "{c:?} is out of order in the clip table");
            assert!(c.path().starts_with("audio/"), "{c:?} path should be asset-relative");
            assert!(c.path().ends_with(".wav"));
        }
        let sounds_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for c in Clip::ALL {
            assert!(
                sounds_dir.join(c.path()).exists(),
                "missing generated clip {} — run scripts/generate_audio.py",
                c.path()
            );
        }
    }

    #[test]
    fn budget_caps_per_category_and_live_count() {
        let mut b = Budget::default();
        assert!(b.try_take(Cat::Bounce, 0));
        assert!(b.try_take(Cat::Bounce, 0));
        assert!(!b.try_take(Cat::Bounce, 0), "third bounce in one frame must be dropped");
        assert!(b.try_take(Cat::Impact, 0));
        b.reset();
        assert!(b.try_take(Cat::Bounce, 0));
        // Soft cap drops foley/bounce but still lets impacts and stingers through.
        assert!(!b.try_take(Cat::Foley, LIVE_SOFT_CAP));
        assert!(b.try_take(Cat::Impact, LIVE_SOFT_CAP));
        // Hard cap: only stingers survive.
        b.reset();
        assert!(!b.try_take(Cat::Impact, LIVE_HARD_CAP));
        assert!(b.try_take(Cat::Stinger, LIVE_HARD_CAP));
    }

    #[test]
    fn pan_space_comes_out_of_the_right_speaker() {
        let (l_c, r_c) = rodio_pan_gains(pan_emitter_position(0.0), PAN_EAR_GAP);
        assert!((l_c - r_c).abs() < 0.02, "centre should be balanced: {l_c} {r_c}");
        assert!(l_c > 0.6 && l_c < 0.85, "centre gain {l_c} should be ~0.75");
        let (l_r, r_r) = rodio_pan_gains(pan_emitter_position(1.0), PAN_EAR_GAP);
        assert!(r_r > l_r * 1.4, "hard right must favour the right channel: {l_r} {r_r}");
        let (l_l, r_l) = rodio_pan_gains(pan_emitter_position(-1.0), PAN_EAR_GAP);
        assert!(l_l > r_l * 1.4, "hard left must favour the left channel: {l_l} {r_l}");
        assert!(r_r <= 1.0 && l_l <= 1.0);
        // Emitter always sits at unit distance so the level never explodes.
        for p in [-1.0f32, -0.5, 0.0, 0.3, 1.0] {
            assert!((pan_emitter_position(p).length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn camera_pan_maps_court_x_to_screen() {
        let cam = Vec3::new(0.0, 9.0, 22.0);
        let right = Vec3::X;
        let (pan_l, _) = pan_from_camera(Vec3::new(-11.0, 0.0, 0.0), cam, right);
        let (pan_c, g_c) = pan_from_camera(Vec3::new(0.0, 0.0, 0.0), cam, right);
        let (pan_r, _) = pan_from_camera(Vec3::new(12.5, 0.0, 0.0), cam, right);
        assert!(pan_l <= -0.99 && pan_c.abs() < 1e-6 && pan_r >= 0.99);
        assert!(g_c > 0.7 && g_c <= 1.0);
        // Flipped camera flips the pan.
        let (pan_flip, _) = pan_from_camera(Vec3::new(12.5, 0.0, 0.0), cam, -Vec3::X);
        assert!(pan_flip <= -0.99);
    }

    #[test]
    fn bed_weights_crossfade_sanely() {
        let calm = bed_weights(0.0);
        assert!(calm[0] > 0.5 && calm[1] < 0.05 && calm[2] < 0.01);
        let mid = bed_weights(0.5);
        assert!(mid[1] > 0.7, "excited layer should carry the middle: {mid:?}");
        let wild = bed_weights(1.0);
        assert!(wild[2] > 0.95 && wild[0] < 0.2);
        let total = |w: [f32; 3]| w.iter().sum::<f32>();
        assert!(
            total(wild) > total(calm) * 1.8,
            "a roaring bowl must be clearly louder than a murmuring one"
        );
        let mut last_sum = 0.0;
        for i in 0..=40 {
            let w = bed_weights(i as f32 / 40.0);
            let sum = total(w);
            assert!(sum > 0.5 && sum < 2.0, "sum {sum} at {i}");
            assert!(w.iter().all(|x| (0.0..=1.0).contains(x)));
            assert!(sum >= last_sum - 0.08, "no audible dip in the crossfade at {i}");
            last_sum = sum;
        }
    }

    #[test]
    fn bounce_variants_scale_with_impact() {
        let (hard, g_hard) = bounce_variant(11.0);
        let (soft, g_soft) = bounce_variant(1.3);
        assert_eq!(hard, 0);
        assert_eq!(soft, 4);
        assert!(g_hard > g_soft);
        assert!(g_hard <= 1.0);
        let (mid, _) = bounce_variant(6.0);
        assert!(mid >= 1 && mid <= 3);
    }

    #[test]
    fn shot_clock_ticks_rise() {
        let mut last = 0.0;
        for s in (1..=5).rev() {
            let p = shot_tick_pitch(s);
            assert!(p > last, "pitch must rise as seconds fall");
            last = p;
        }
        assert_eq!(shot_tick_pitch(9), 1.0);
    }

    #[test]
    fn lead_sign_and_excite_targets() {
        assert_eq!(lead_sign(10, 8), 1);
        assert_eq!(lead_sign(8, 10), -1);
        assert_eq!(lead_sign(7, 7), 0);
        assert!(crowd_excite_target(0.0, 0.0, 24.0, 60.0, false, false) < 1e-6);
        assert!(crowd_excite_target(0.0, 0.0, 3.0, 60.0, false, false) >= 0.3);
        assert!(crowd_excite_target(0.2, 0.0, 24.0, 20.0, true, false) >= 0.5);
        assert!(crowd_excite_target(0.0, 0.0, 24.0, 60.0, false, true) >= 0.55);
        assert!(crowd_excite_target(2.0, 0.0, 24.0, 60.0, false, false) <= 1.0);
    }

    #[test]
    fn approach_is_monotone_and_bounded() {
        let mut v = 0.0;
        for _ in 0..200 {
            let n = approach(v, 1.0, 1.0 / 60.0, 8.0, 2.0);
            assert!(n >= v && n <= 1.0);
            v = n;
        }
        assert!(v > 0.99);
        let down = approach(1.0, 0.0, 0.1, 8.0, 2.0);
        let up = approach(0.0, 1.0, 0.1, 8.0, 2.0);
        assert!(up > 1.0 - down, "attack should be faster than release");
    }

    #[test]
    fn rng_is_well_behaved() {
        let mut mix = AudioMix::default();
        let mut lo = 1.0f32;
        let mut hi = 0.0f32;
        for _ in 0..2000 {
            let r = mix.rand();
            assert!((0.0..1.0).contains(&r));
            lo = lo.min(r);
            hi = hi.max(r);
        }
        assert!(lo < 0.05 && hi > 0.95);
        assert_ne!(xorshift(0), 0);
    }

    #[test]
    fn ducking_requests_take_the_deepest() {
        let mut mix = AudioMix::default();
        mix.duck_music(0.5, 0.3);
        mix.duck_music(0.2, 0.1);
        assert!((mix.duck_target - 0.2).abs() < 1e-6);
        assert!((mix.duck_hold - 0.3).abs() < 1e-6);
        mix.duck_crowd(0.7, 1.0);
        assert!((mix.crowd_duck_target - 0.7).abs() < 1e-6);
    }
}
