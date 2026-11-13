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
    @invariant @builtin(position) position: vec4<f32>,
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

fn world_direction(ndc: vec2<f32>) -> vec3<f32> {
    return normalize((u.environment_view * vec4<f32>(ndc, -1.0, 0.0)).xyz);
}

fn equirect_uv(ndc: vec2<f32>, rotation: f32) -> vec2<f32> {
    let direction = world_direction(ndc);
    let longitude = atan2(direction.x, -direction.z) + rotation;
    let latitude = asin(clamp(direction.y, -1.0, 1.0));
    return vec2<f32>(fract(longitude / (2.0 * 3.14159265359) + 0.5),
                     0.5 - latitude / 3.14159265359);
}

fn image_uv(source_uv: vec2<f32>) -> vec2<f32> {
    var uv = source_uv;
    if u.background_image_params.x < 0.5 {
        // Not fitted: the image at its own pixel size times its scale (a
        // negative scale mirrors it), centred on the viewport and moved by
        // its offset in pixels, right and down.
        let pixel = (source_uv - vec2<f32>(0.5)) * max(u.viewport_size, vec2<f32>(1.0))
            - u.background_image_transform.xy;
        let size = u.background_image_transform.zw;
        let safe = select(size, vec2<f32>(1e-6), abs(size) < vec2<f32>(1e-6));
        uv = pixel / safe + vec2<f32>(0.5);
        return select(uv, fract(uv), u.background_image_params.z > 0.5);
    }
    // Fitted: the image fills the viewport; keeping its aspect ratio it
    // covers the viewport and the overflow is cut off evenly.
    if u.background_image_params.y > 0.5 {
        let image_aspect = max(u.background_image_params.w, 1e-6);
        let viewport_aspect = max(u.viewport_size.x / max(u.viewport_size.y, 1.0), 1e-6);
        if image_aspect > viewport_aspect {
            uv.x = (uv.x - 0.5) * (viewport_aspect / image_aspect) + 0.5;
        } else {
            uv.y = (uv.y - 0.5) * (image_aspect / viewport_aspect) + 0.5;
        }
    }
    if u.background_image_params.z > 0.5 {
        return fract(uv);
    }
    return uv;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let mode = u32(max(u.background_params.x, 0.0) + 0.5);
    var color: vec3<f32>;
    if mode == 1u {
        color = u.viewport_background.rgb;
    } else if mode == 2u {
        // Bottom to top along the "up" direction turned counter-clockwise by
        // the rotation, in pixels from the viewport centre. As the reference
        // draws it: the middle colour sits (horizon - 0.5) viewport widths
        // from the centre and blends linearly into the top and bottom colours
        // over height viewport heights on each side; two colours are one blend
        // over a viewport height on each side of the centre.
        let angle = u.background_params.w;
        let up = vec2<f32>(-sin(angle), cos(angle));
        let size = max(u.viewport_size, vec2<f32>(1.0));
        let pixel = (vec2<f32>(in.uv.x, 1.0 - in.uv.y) - vec2<f32>(0.5)) * size;
        let along = dot(pixel, up);
        let three = abs(u.background_params.z) > 1e-6;
        let middle_at = select(0.0, (u.background_params.y - 0.5) * size.x, three);
        let half_band = select(1.0, abs(u.background_params.z), three) * size.y;
        let middle = select(mix(u.background_bottom.rgb, u.background_top.rgb, 0.5),
            u.background_middle.rgb, three);
        let lower = clamp((along - middle_at + half_band) / half_band, 0.0, 1.0);
        let upper = clamp((along - middle_at) / half_band, 0.0, 1.0);
        color = mix(mix(u.background_bottom.rgb, middle, lower), u.background_top.rgb, upper);
    } else if mode == 3u {
        let direction = world_direction(in.ndc);
        if direction.z >= 0.0 {
            let sky = smoothstep(0.0, 1.0, direction.z);
            color = mix(u.background_middle.rgb, u.background_top.rgb, sky);
        } else {
            let depth = smoothstep(0.0, 1.0, -direction.z);
            let underground = mix(u.background_bottom.rgb, u.background_aux0.rgb, depth);
            let ground = mix(u.viewport_background.rgb, u.background_aux1.rgb, depth);
            color = mix(underground, ground, 0.65);
        }
    } else if mode == 4u {
        let uv = image_uv(in.uv);
        if u.background_image_params.z < 0.5
            && (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
            color = u.viewport_background.rgb;
        } else {
            color = textureSample(background_texture, background_sampler, uv).rgb;
        }
    } else if mode == 5u {
        color = textureSample(background_texture, background_sampler,
            equirect_uv(in.ndc, u.background_params.w)).rgb;
    } else if mode == 6u {
        let direction = world_direction(in.ndc);
        let elevation = clamp(direction.z, 0.0, 1.0);
        color = mix(u.background_bottom.rgb, u.background_top.rgb,
            smoothstep(0.0, 1.0, elevation));
        let sun_length = length(u.background_params.yzw);
        if sun_length > 0.5 {
            let sun_direction = u.background_params.yzw / sun_length;
            let alignment = dot(direction, sun_direction);
            let glow = smoothstep(0.96, 1.0, alignment) * 0.18;
            let disk = smoothstep(0.9997, 0.99995, alignment);
            let intensity = max(u.background_middle.a, 0.0);
            color += u.background_middle.rgb * intensity * (glow + disk);
        }
    } else {
        color = u.viewport_background.rgb;
    }
    if u.fog_params.x > 0.5 && u.fog_params.y > 0.5 {
        color = mix(color, u.fog_color.rgb, clamp(u.fog_params.w, 0.0, 1.0));
    }
    return vec4<f32>(color, 1.0);
}
