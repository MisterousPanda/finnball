// Crowd vertex animation. Every fan in the arena lives in a handful of merged meshes;
// uv.x is the per-vertex "how much this part moves" weight (0 = seat, 1 = head/arms),
// uv.y is the fan's random phase. params = (time, hype, wave_speed, unused).
#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> params: vec4<f32>;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);

    var pos = vertex.position;
    let weight = vertex.uv.x;
    let phase = vertex.uv.y * 6.2831853;
    let t = params.x;
    let hype = params.y;

    // Idle: slow breathing sway plus a travelling wave that rolls around the bowl.
    let breathe = sin(t * 1.7 + phase) * 0.012;
    let wave = sin(t * params.z + pos.x * 0.35 + pos.z * 0.2 + phase * 0.3) * 0.015;
    // Hype: fans bounce out of their seats and pump their arms.
    let bounce = max(sin(t * 7.5 + phase), 0.0) * 0.28 * hype;
    let lean = sin(t * 7.5 + phase) * 0.06 * hype;

    pos.y += (breathe + wave + bounce) * weight;
    pos.x += lean * weight * weight;

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(pos, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
    return out;
}
