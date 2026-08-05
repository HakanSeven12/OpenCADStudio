struct Uniforms {
    viewport_size: vec2<f32>,
    world_per_pixel: f32,
    lwdisplay_enable: f32,
    flat_shade: f32,
    transparency_enable: f32,
    linetype_scale: f32,
    _pad: f32,
    view_rot: mat4x4<f32>,
    eye_high: vec3<f32>,
    _pad_eh: f32,
    eye_low: vec3<f32>,
    _pad_el: f32,
    light_position_type: array<vec4<f32>, 4>,
    light_direction_intensity: array<vec4<f32>, 4>,
    light_color_hotspot: array<vec4<f32>, 4>,
    light_attenuation: array<vec4<f32>, 4>,
    lighting: vec4<f32>,
    visual_style: vec4<f32>,
    visual_style_color: vec4<f32>,
    viewport_background: vec4<f32>,
    view_tone: vec4<f32>,
    background_top: vec4<f32>,
    background_middle: vec4<f32>,
    background_bottom: vec4<f32>,
    background_aux0: vec4<f32>,
    background_aux1: vec4<f32>,
    background_params: vec4<f32>,
    background_image_params: vec4<f32>,
    background_image_transform: vec4<f32>,
    environment_params: vec4<f32>,
    environment_view: mat4x4<f32>,
    shadow_view_proj: mat4x4<f32>,
    shadow_params: vec4<f32>,
    fog_color: vec4<f32>,
    fog_params: vec4<f32>,
    fog_distances: vec4<f32>,
    light_web_profile_a: array<vec4<f32>, 4>,
    light_web_profile_b: array<vec4<f32>, 4>,
    light_web_rotation: array<vec4<f32>, 4>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var background_texture: texture_2d<f32>;
@group(0) @binding(2) var background_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) ndc: vec2<f32>,
};

var<private> POSITIONS: array<vec2<f32>, 3> = array<vec2<f32>, 3>(
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 3.0, -1.0),
    vec2<f32>(-1.0,  3.0),
);

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = POSITIONS[index];
    var out: VertexOutput;
    out.position = vec4<f32>(position, 0.999, 1.0);
    out.uv = vec2<f32>(position.x * 0.5 + 0.5, 0.5 - position.y * 0.5);
    out.ndc = position;
    return out;
}

fn equirect_uv(ndc: vec2<f32>, rotation: f32) -> vec2<f32> {
    let direction = normalize((u.environment_view * vec4<f32>(ndc, -1.0, 0.0)).xyz);
    let longitude = atan2(direction.x, -direction.z) + rotation;
    let latitude = asin(clamp(direction.y, -1.0, 1.0));
    return vec2<f32>(fract(longitude / (2.0 * 3.14159265359) + 0.5),
                     0.5 - latitude / 3.14159265359);
}

fn image_uv(source_uv: vec2<f32>) -> vec2<f32> {
    var uv = (source_uv - u.background_image_transform.xy)
        / max(abs(u.background_image_transform.zw), vec2<f32>(1e-6));
    if u.background_image_params.y > 0.5 {
        let image_aspect = max(u.background_image_params.w, 1e-6);
        let viewport_aspect = max(u.viewport_size.x / max(u.viewport_size.y, 1.0), 1e-6);
        if image_aspect > viewport_aspect {
            let scale = viewport_aspect / image_aspect;
            uv.y = (uv.y - 0.5) / scale + 0.5;
        } else {
            let scale = image_aspect / viewport_aspect;
            uv.x = (uv.x - 0.5) / scale + 0.5;
        }
    }
    if u.background_image_params.z > 0.5 {
        return fract(uv);
    }
    return clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let mode = u32(max(u.background_params.x, 0.0) + 0.5);
    var color: vec3<f32>;
    if mode == 1u {
        color = u.viewport_background.rgb;
    } else if mode == 2u {
        let angle = u.background_params.w;
        let axis = vec2<f32>(-sin(angle), cos(angle));
        let coordinate = dot(in.uv - vec2<f32>(0.5), axis) + 0.5;
        let horizon = clamp(u.background_params.y, 0.0, 1.0);
        let half_height = max(abs(u.background_params.z) * 0.5, 1e-3);
        let lower = smoothstep(horizon - half_height, horizon, coordinate);
        let upper = smoothstep(horizon, horizon + half_height, coordinate);
        let low_color = mix(u.background_bottom.rgb, u.background_middle.rgb, lower);
        color = mix(low_color, u.background_top.rgb, upper);
    } else if mode == 3u {
        let horizon = 0.5;
        if in.uv.y <= horizon {
            let sky = smoothstep(0.0, horizon, in.uv.y);
            color = mix(u.background_top.rgb, u.background_middle.rgb, sky);
        } else {
            let ground = smoothstep(horizon, 1.0, in.uv.y);
            let underground = mix(u.background_bottom.rgb, u.background_aux0.rgb, ground);
            let distance_tint = mix(u.background_aux1.rgb, u.viewport_background.rgb, ground);
            color = mix(underground, distance_tint, 0.35);
        }
    } else if mode == 4u {
        color = textureSample(background_texture, background_sampler, image_uv(in.uv)).rgb;
    } else if mode == 5u {
        color = textureSample(background_texture, background_sampler,
            equirect_uv(in.ndc, u.background_params.w)).rgb;
    } else if mode == 6u {
        let t = smoothstep(0.0, 1.0, in.uv.y);
        color = mix(u.background_bottom.rgb, u.background_top.rgb, t);
    } else {
        color = u.viewport_background.rgb;
    }
    if u.fog_params.x > 0.5 && u.fog_params.y > 0.5 {
        color = mix(color, u.fog_color.rgb, clamp(u.fog_params.w, 0.0, 1.0));
    }
    return vec4<f32>(color, 1.0);
}
