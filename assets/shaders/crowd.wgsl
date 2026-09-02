// Crowd vertex animation + emissive fragment. Every fan in the arena lives in a handful
// of merged meshes; per-vertex channels:
//   uv.x   = motion weight (0 = seat/static, 1 = head/arms)
//   uv.y   = the fan's random phase
//   uv_b.x = glow amount (added as emission, scaled by vertex colour)
//   uv_b.y = part id: 0 body, 1 lowered arm, 2 head, 3 phone screen, 4 constant LED,
//            5 held sign, 6 upper-deck impostor, 7 cheer/pom-pom
// params  = (time, hype, stand, fire)
// params2 = (wave position 0..1 around the ring, wave strength, phone glow, arm raise)
#import bevy_pbr::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    view_transformations::position_world_to_clip,
}

#import bevy_pbr::{
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> params: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> params2: vec4<f32>;

const TAU: f32 = 6.2831853;

fn smooth01(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp((x - e0) / (e1 - e0), 0.0, 1.0);
    return t * t * (3.0 - 2.0 * t);
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);

    var pos = vertex.position;
    var weight = 0.0;
    var phase = 0.0;
#ifdef VERTEX_UVS_A
    weight = vertex.uv.x;
    phase = vertex.uv.y * TAU;
#endif
    var part = 0.0;
#ifdef VERTEX_UVS_B
    part = vertex.uv_b.y;
#endif
    let t = params.x;
    let hype = params.y;
    let stand = params.z;
    let fire = params.w;

    if weight > 0.001 {
        // Idle: slow breathing sway plus a gentle rolling murmur through the bowl.
        let breathe = sin(t * 1.7 + phase) * 0.012;
        let murmur = sin(t * 0.9 + pos.x * 0.35 + pos.z * 0.2 + phase * 0.3) * 0.015;
        // Hype: fans bounce out of their seats and pump their arms.
        let bounce = max(sin(t * 7.5 + phase), 0.0) * 0.28 * hype;
        let lean = sin(t * 7.5 + phase) * 0.06 * hype;
        // Individual random jumps: rare, sharp spikes so the bowl never looks uniform.
        let spike = pow(max(sin(t * 1.3 + phase * 9.0), 0.0), 40.0) * (0.12 + hype * 0.25);
        // Stadium wave: a bump travelling around the ring by seat angle (world space).
        let ang = atan2(pos.z, pos.x) / TAU;
        let d = fract(ang - params2.x + 0.5) - 0.5;
        let wave = exp(-d * d * 900.0) * params2.y * 0.42;
        // Standing sections: lift torso/head/arms out of the seat.
        let rise = stand * 0.26 * smooth01(0.1, 0.55, weight);

        pos.y += (breathe + murmur + bounce + spike + wave) * weight + rise;
        pos.x += lean * weight * weight;

        if part > 0.5 && part < 1.5 {
            // Lowered arms pump upward at peak hype.
            pos.y += params2.w * 0.32 * (0.5 + 0.5 * sin(t * 7.5 + phase));
        } else if part > 2.5 && part < 3.5 {
            // Phones sway slowly above the heads.
            pos.x += sin(t * 2.1 + phase) * 0.03;
            pos.y += sin(t * 3.3 + phase) * 0.02 + fire * 0.08;
        } else if part > 4.5 && part < 5.5 {
            // Held signs bob and tilt, more when the bowl is loud.
            let amp = 0.04 + hype * 0.16;
            pos.y += sin(t * 3.0 + phase) * amp;
            pos.x += sin(t * 2.2 + phase * 1.7) * amp * 0.6;
        } else if part > 5.5 && part < 6.5 {
            // Upper deck impostors: cheaper, smaller motion.
            pos.y += max(sin(t * 6.0 + phase), 0.0) * 0.1 * hype;
        } else if part > 6.5 && part < 7.5 {
            // Cheer squad: constant routine regardless of hype.
            pos.y += max(sin(t * 4.0 + phase * 0.25), 0.0) * 0.16 * weight;
            pos.x += sin(t * 2.0 + phase * 0.25) * 0.08 * weight;
        }
    }

    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(pos, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
    return out;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);

    var glow = 0.0;
    var part = 0.0;
    var phase = 0.0;
#ifdef VERTEX_UVS_B
    glow = in.uv_b.x;
    part = in.uv_b.y;
#endif
#ifdef VERTEX_UVS_A
    phase = in.uv.y * TAU;
#endif
    if part > 2.5 && part < 3.5 {
        // Phone screens: dim in a quiet arena, blazing when the bowl is on fire.
        let flicker = 0.85 + 0.15 * sin(params.x * 3.0 + phase * 7.0);
        glow *= (0.35 + params.y * 1.6 + params.w * 2.4 + params2.z) * flicker;
    } else if part > 3.5 && part < 4.5 {
        // Fixed LEDs / EXIT signs: steady with a very soft breathing.
        glow *= 0.92 + 0.08 * sin(params.x * 1.3 + phase);
    }
#ifdef VERTEX_COLORS
    out.color = vec4<f32>(out.color.rgb + in.color.rgb * glow, out.color.a);
#else
    out.color = vec4<f32>(out.color.rgb + vec3<f32>(glow), out.color.a);
#endif

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
