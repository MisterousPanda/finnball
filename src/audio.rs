use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::ball::{BackboardHitEvent, BucketEvent, FloorBounceEvent, RimHitEvent};
use crate::gameplay::{CutSqueak, DribbleTickEvent, StealEvent, TipWhistle, ViolationEvent};
use crate::states::{AppState, Paused};

pub struct FinnAudioPlugin;

impl Plugin for FinnAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioMix>()
            .init_resource::<AudioGate>()
            .add_message::<UiClick>()
            .add_systems(Startup, preload)
            .add_systems(
                Update,
                (
                    unlock_on_input,
                    music_for_state,
                    play_bucket,
                    play_collisions,
                    play_dribble,
                    play_cuts,
                    play_game_events,
                    play_tip_whistle,
                    play_ui_clicks,
                    duck_and_mix,
                )
                    .chain(),
            );
    }
}

#[derive(Message, Clone, Copy)]
pub struct UiClick {
    pub confirm: bool,
}

#[derive(Resource, Default)]
struct AudioGate {
    unlocked: bool,
}

#[derive(Resource)]
struct Sounds {
    menu: Handle<AudioSource>,
    ingame: Handle<AudioSource>,
    crowd: Handle<AudioSource>,
    cheer: Handle<AudioSource>,
    gasp: Handle<AudioSource>,
    swish: Handle<AudioSource>,
    rim: Handle<AudioSource>,
    backboard: Handle<AudioSource>,
    bounce: Handle<AudioSource>,
    dribble: Handle<AudioSource>,
    dunk: Handle<AudioSource>,
    squeak: Handle<AudioSource>,
    whistle: Handle<AudioSource>,
    buzzer: Handle<AudioSource>,
    blip: Handle<AudioSource>,
    confirm: Handle<AudioSource>,
    downtown: Handle<AudioSource>,
    poster: Handle<AudioSource>,
}

#[derive(Resource)]
pub struct AudioMix {
    pub music: f32,
    pub sfx: f32,
    pub crowd: f32,
    pub duck: f32,
    duck_target: f32,
    duck_hold: f32,
}

impl Default for AudioMix {
    fn default() -> Self {
        Self {
            music: 0.30,
            sfx: 0.88,
            crowd: 0.42,
            duck: 1.0,
            duck_target: 1.0,
            duck_hold: 0.0,
        }
    }
}

#[derive(Component)]
struct MusicBus;

#[derive(Component)]
struct CrowdBus;

#[derive(Component)]
struct MusicKind(AppState);

fn preload(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(Sounds {
        menu: assets.load("audio/music/menu_synthwave.wav"),
        ingame: assets.load("audio/music/ingame_arcade.wav"),
        crowd: assets.load("audio/crowd/bed.wav"),
        cheer: assets.load("audio/crowd/cheer.wav"),
        gasp: assets.load("audio/crowd/gasp.wav"),
        swish: assets.load("audio/ball/swish.wav"),
        rim: assets.load("audio/ball/rim.wav"),
        backboard: assets.load("audio/ball/backboard.wav"),
        bounce: assets.load("audio/ball/bounce.wav"),
        dribble: assets.load("audio/ball/dribble.wav"),
        dunk: assets.load("audio/ball/dunk.wav"),
        squeak: assets.load("audio/player/squeak.wav"),
        whistle: assets.load("audio/game/whistle.wav"),
        buzzer: assets.load("audio/game/buzzer.wav"),
        blip: assets.load("audio/ui/blip.wav"),
        confirm: assets.load("audio/ui/confirm.wav"),
        downtown: assets.load("audio/stingers/downtown.wav"),
        poster: assets.load("audio/stingers/poster.wav"),
    });
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

fn music_for_state(
    mut commands: Commands,
    gate: Res<AudioGate>,
    state: Res<State<AppState>>,
    sounds: Option<Res<Sounds>>,
    mix: Res<AudioMix>,
    existing: Query<(Entity, &MusicKind), With<MusicBus>>,
    crowd: Query<Entity, With<CrowdBus>>,
) {
    if !gate.unlocked {
        return;
    }
    let Some(sounds) = sounds else {
        return;
    };
    let want = match state.get() {
        AppState::Playing => AppState::Playing,
        _ => AppState::MainMenu,
    };
    let has = existing.iter().any(|(_, k)| k.0 == want);
    if !has {
        for (e, _) in &existing {
            commands.entity(e).despawn();
        }
        let (handle, vol) = if want == AppState::Playing {
            (sounds.ingame.clone(), mix.music)
        } else {
            (sounds.menu.clone(), mix.music * 0.9)
        };
        commands.spawn((
            MusicBus,
            MusicKind(want),
            AudioPlayer::new(handle),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(vol)),
        ));
    }
    if want == AppState::Playing && crowd.iter().next().is_none() {
        commands.spawn((
            CrowdBus,
            AudioPlayer::new(sounds.crowd.clone()),
            PlaybackSettings::LOOP.with_volume(Volume::Linear(mix.crowd)),
        ));
    }
    if want != AppState::Playing {
        for e in &crowd {
            commands.entity(e).despawn();
        }
    }
}

fn play_one(commands: &mut Commands, handle: Handle<AudioSource>, vol: f32) {
    commands.spawn((
        AudioPlayer::new(handle),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(vol)),
    ));
}

fn play_bucket(
    mut commands: Commands,
    sounds: Option<Res<Sounds>>,
    mut mix: ResMut<AudioMix>,
    mut buckets: MessageReader<BucketEvent>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    for ev in buckets.read() {
        mix.duck_target = 0.18;
        mix.duck_hold = 0.45;
        if ev.dunk {
            play_one(&mut commands, sounds.dunk.clone(), mix.sfx);
            play_one(&mut commands, sounds.poster.clone(), mix.sfx * 0.85);
        } else {
            play_one(&mut commands, sounds.swish.clone(), mix.sfx);
            play_one(&mut commands, sounds.cheer.clone(), mix.crowd * 1.1);
        }
        play_one(&mut commands, sounds.downtown.clone(), mix.sfx * 0.55);
    }
}

fn play_collisions(
    mut commands: Commands,
    sounds: Option<Res<Sounds>>,
    mix: Res<AudioMix>,
    mut rims: MessageReader<RimHitEvent>,
    mut boards: MessageReader<BackboardHitEvent>,
    mut floors: MessageReader<FloorBounceEvent>,
    mut cool: Local<(f32, f32, f32)>,
    time: Res<Time>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    cool.0 = (cool.0 - time.delta_secs()).max(0.0);
    cool.1 = (cool.1 - time.delta_secs()).max(0.0);
    cool.2 = (cool.2 - time.delta_secs()).max(0.0);
    for ev in rims.read() {
        if cool.0 > 0.0 {
            continue;
        }
        cool.0 = 0.08;
        let vol = (mix.sfx * (0.45 + ev.speed * 0.04)).min(1.0);
        play_one(&mut commands, sounds.rim.clone(), vol);
    }
    for _ in boards.read() {
        if cool.1 > 0.0 {
            continue;
        }
        cool.1 = 0.1;
        play_one(&mut commands, sounds.backboard.clone(), mix.sfx * 0.75);
    }
    for ev in floors.read() {
        if cool.2 > 0.0 {
            continue;
        }
        cool.2 = 0.12;
        let vol = (mix.sfx * (0.35 + ev.speed * 0.05)).min(0.9);
        play_one(&mut commands, sounds.bounce.clone(), vol);
    }
}

fn play_dribble(
    mut commands: Commands,
    sounds: Option<Res<Sounds>>,
    mix: Res<AudioMix>,
    mut ticks: MessageReader<DribbleTickEvent>,
    mut cool: Local<f32>,
    time: Res<Time>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    *cool = (*cool - time.delta_secs()).max(0.0);
    for _ in ticks.read() {
        if *cool > 0.0 {
            continue;
        }
        *cool = 0.07;
        play_one(&mut commands, sounds.dribble.clone(), mix.sfx * 0.55);
    }
}

fn play_cuts(
    mut commands: Commands,
    sounds: Option<Res<Sounds>>,
    mix: Res<AudioMix>,
    mut cuts: MessageReader<CutSqueak>,
    mut cool: Local<f32>,
    time: Res<Time>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    *cool = (*cool - time.delta_secs()).max(0.0);
    for _ in cuts.read() {
        if *cool > 0.0 {
            continue;
        }
        *cool = 0.16;
        play_one(&mut commands, sounds.squeak.clone(), mix.sfx * 0.52);
    }
}

fn play_game_events(
    mut commands: Commands,
    sounds: Option<Res<Sounds>>,
    mut mix: ResMut<AudioMix>,
    mut steals: MessageReader<StealEvent>,
    mut viol: MessageReader<ViolationEvent>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    for ev in steals.read() {
        if ev.success {
            play_one(&mut commands, sounds.whistle.clone(), mix.sfx * 0.7);
            play_one(&mut commands, sounds.squeak.clone(), mix.sfx * 0.6);
            play_one(&mut commands, sounds.gasp.clone(), mix.crowd);
        } else {
            play_one(&mut commands, sounds.squeak.clone(), mix.sfx * 0.45);
        }
    }
    for _ in viol.read() {
        mix.duck_target = 0.2;
        mix.duck_hold = 0.8;
        play_one(&mut commands, sounds.buzzer.clone(), mix.sfx);
    }
}

fn play_tip_whistle(
    mut commands: Commands,
    sounds: Option<Res<Sounds>>,
    mix: Res<AudioMix>,
    mut tips: MessageReader<TipWhistle>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    for _ in tips.read() {
        play_one(&mut commands, sounds.whistle.clone(), mix.sfx * 0.8);
    }
}

fn play_ui_clicks(
    mut commands: Commands,
    sounds: Option<Res<Sounds>>,
    mix: Res<AudioMix>,
    mut clicks: MessageReader<UiClick>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    for ev in clicks.read() {
        let h = if ev.confirm {
            sounds.confirm.clone()
        } else {
            sounds.blip.clone()
        };
        play_one(&mut commands, h, mix.sfx * 0.65);
    }
}

fn duck_and_mix(
    time: Res<Time>,
    paused: Res<Paused>,
    state: Res<State<AppState>>,
    mut mix: ResMut<AudioMix>,
    mut music: Query<&mut AudioSink, (With<MusicBus>, Without<CrowdBus>)>,
    mut crowd: Query<&mut AudioSink, (With<CrowdBus>, Without<MusicBus>)>,
) {
    let dt = time.delta_secs();
    mix.duck_hold = (mix.duck_hold - dt).max(0.0);
    if mix.duck_hold <= 0.0 {
        mix.duck_target = 1.0;
    }
    mix.duck += (mix.duck_target - mix.duck) * (1.0 - (-8.0 * dt).exp());
    let pause_duck = if paused.0 && *state.get() == AppState::Playing {
        0.45
    } else {
        1.0
    };
    let music_v = Volume::Linear(mix.music * mix.duck * pause_duck);
    for mut sink in &mut music {
        sink.set_volume(music_v);
    }
    let crowd_v = Volume::Linear(mix.crowd * pause_duck);
    for mut sink in &mut crowd {
        sink.set_volume(crowd_v);
    }
}
