use bevy::prelude::*;

use crate::ball::BucketEvent;
use crate::states::{AppState, Paused};

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_on_bucket, age_sparks).run_if(in_state(AppState::Playing)));
    }
}

#[derive(Component)]
struct Spark {
    life: f32,
}

fn spawn_on_bucket(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut buckets: MessageReader<BucketEvent>,
    ball: Query<&Transform, With<crate::ball::Ball>>,
) {
    for ev in buckets.read() {
        let origin = ball.single().ok().map(|t| t.translation).unwrap_or(Vec3::new(0.0, 3.0, 0.0));
        let color = if ev.dunk {
            Color::srgb(1.0, 0.45, 0.1)
        } else {
            Color::srgb(0.3, 0.9, 1.0)
        };
        let mat = materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color.to_linear()) * 4.0,
            unlit: true,
            ..default()
        });
        let mesh = meshes.add(Cuboid::new(0.12, 0.12, 0.12));
        for i in 0..14 {
            let a = i as f32 * 0.45;
            commands.spawn((
                Spark { life: 0.7 },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(origin + Vec3::new(a.sin() * 0.2, 0.1, a.cos() * 0.2)),
                crate::court::ArenaRoot,
            ));
        }
    }
}

fn age_sparks(
    time: Res<Time>,
    paused: Res<Paused>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Spark, &mut Transform)>,
) {
    if paused.0 {
        return;
    }
    for (e, mut s, mut tf) in &mut q {
        s.life -= time.delta_secs();
        tf.translation.y += time.delta_secs() * 2.4;
        tf.scale *= 1.0 - time.delta_secs() * 1.5;
        if s.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}
