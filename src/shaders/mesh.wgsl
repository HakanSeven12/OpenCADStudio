// Mesh shader — renders triangle meshes (truck Shell/Solid tessellation).
//
// Vertex layout: position [f32;3], normal [f32;3], color [f32;4]  (40 bytes)
//
// Lighting: viewport default lights or drawing light entities supplied by the
// frame uniform. Two shading paths share this shader, picked per-frame via
// `u.flat_shade`:
//   - 0.0 → per-vertex normals interpolated to the fragment (Gouraud).
//   - 1.0 → per-triangle face normal from screen-space derivatives
//     `cross(dpdx(pos), dpdy(pos))`, so each triangle reads as a single
//     flat shade (FlatShaded).

struct Uniforms {
    viewport_size:       vec2<f32>,
    world_per_pixel:     f32,
    lwdisplay_enable:    f32,
    flat_shade:          f32,
    transparency_enable: f32,
    _pad:                vec2<f32>,
    // Relative-to-eye (double-single): see wire.wgsl.
    view_rot:            mat4x4<f32>,
    eye_high:            vec3<f32>,
    _pad_eh:             f32,
    eye_low:             vec3<f32>,
    _pad_el:             f32,
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

@group(0) @binding(0)
var<uniform> u: Uniforms;
@group(0) @binding(3) var environment_texture: texture_2d<f32>;
@group(0) @binding(4) var environment_sampler: sampler;
@group(0) @binding(5) var shadow_texture: texture_depth_2d;
@group(0) @binding(6) var shadow_sampler: sampler_comparison;

struct MaterialMaps {
    blends0:  vec4<f32>,
    present0: vec4<u32>,
    blends1:  vec4<f32>,
    present1: vec4<u32>,
    tiling0: vec4<u32>,
    tiling1: vec4<u32>,
    render_modes: vec4<u32>,
    source_state: vec4<u32>,
    indirect: vec4<f32>,
};

@group(1) @binding(0) var diffuse_map: texture_2d<f32>;
@group(1) @binding(1) var specular_map: texture_2d<f32>;
@group(1) @binding(2) var reflection_map: texture_2d<f32>;
@group(1) @binding(3) var opacity_map: texture_2d<f32>;
@group(1) @binding(4) var bump_map: texture_2d<f32>;
@group(1) @binding(5) var refraction_map: texture_2d<f32>;
@group(1) @binding(6) var normal_map: texture_2d<f32>;
@group(1) @binding(7) var diffuse_sampler: sampler;
@group(1) @binding(8) var specular_sampler: sampler;
@group(1) @binding(9) var reflection_sampler: sampler;
@group(1) @binding(10) var opacity_sampler: sampler;
@group(1) @binding(11) var bump_sampler: sampler;
@group(1) @binding(12) var refraction_sampler: sampler;
@group(1) @binding(13) var normal_sampler: sampler;
@group(1) @binding(14) var<uniform> maps: MaterialMaps;

struct MeshInstance {
    model_row_0:     vec4<f32>,
    model_row_1:     vec4<f32>,
    model_row_2:     vec4<f32>,
    translation_low: vec4<f32>,
    normal_row_0:    vec4<f32>,
    normal_row_1:    vec4<f32>,
    normal_row_2:    vec4<f32>,
};

@group(1) @binding(15)
var<storage, read> mesh_instances: array<MeshInstance>;

struct VertexIn {
    @location(0) position:     vec3<f32>,
    @location(1) normal:       vec3<f32>,
    @location(2) color:        vec4<f32>,
    @location(3) position_low: vec3<f32>,
    // gloss, reflectivity, self illumination, luminance
    @location(4) material:     vec4<f32>,
    // specular RGB, refraction index
    @location(5) specular:     vec4<f32>,
    @location(6) uv_diffuse:   vec2<f32>,
    // ambient RGB, translucence
    @location(7) ambient:      vec4<f32>,
    // normal strength, bump scale, reflectance scale, transmittance scale
    @location(8) advanced:     vec4<f32>,
    // illumination model, channel flags, material mode, luminance mode
    @location(9) flags:        vec4<u32>,
    @location(10) uv_specular:   vec2<f32>,
    @location(11) uv_reflection: vec2<f32>,
    @location(12) uv_opacity:    vec2<f32>,
    @location(13) uv_bump:       vec2<f32>,
    @location(14) uv_refraction: vec2<f32>,
    @location(15) uv_normal:     vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip_pos:  vec4<f32>,
    @location(0)       color:     vec4<f32>,
    @location(1)       normal:    vec3<f32>,
    @location(2)       world_pos: vec3<f32>,
    @location(3)       material:  vec4<f32>,
    @location(4)       specular:  vec4<f32>,
    @location(5)       eye_vec:   vec3<f32>,
    @location(6)       uv_diffuse: vec2<f32>,
    @location(7)       ambient:   vec4<f32>,
    @location(8)       advanced:  vec4<f32>,
    @location(9) @interpolate(flat) flags: vec4<u32>,
    @location(10)      uv_specular:   vec2<f32>,
    @location(11)      uv_reflection: vec2<f32>,
    @location(12)      uv_opacity:    vec2<f32>,
    @location(13)      uv_bump:       vec2<f32>,
    @location(14)      uv_refraction: vec2<f32>,
    @location(15)      uv_normal:     vec2<f32>,
};

struct EdgeVertexIn {
    @location(0) position:     vec3<f32>,
    @location(2) color:        vec4<f32>,
    @location(3) position_low: vec3<f32>,
};

struct EdgeVertexOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

fn relative_position(
    position: vec3<f32>,
    position_low: vec3<f32>,
    instance: MeshInstance,
) -> vec3<f32> {
    let world_high = vec3<f32>(
        dot(instance.model_row_0.xyz, position) + instance.model_row_0.w,
        dot(instance.model_row_1.xyz, position) + instance.model_row_1.w,
        dot(instance.model_row_2.xyz, position) + instance.model_row_2.w,
    );
    let world_low = vec3<f32>(
        dot(instance.model_row_0.xyz, position_low),
        dot(instance.model_row_1.xyz, position_low),
        dot(instance.model_row_2.xyz, position_low),
    ) + instance.translation_low.xyz;
    return (world_high - u.eye_high) + (world_low - u.eye_low);
}

@vertex
fn vs_main(
    v: VertexIn,
    @builtin(instance_index) instance_index: u32,
) -> VertexOut {
    var out: VertexOut;
    let instance = mesh_instances[instance_index];
    let rel = relative_position(v.position, v.position_low, instance);
    out.clip_pos  = u.view_rot * vec4<f32>(rel, 1.0);
    out.color     = v.color;
    out.normal    = normalize(vec3<f32>(
        dot(instance.normal_row_0.xyz, v.normal),
        dot(instance.normal_row_1.xyz, v.normal),
        dot(instance.normal_row_2.xyz, v.normal),
    ));
    out.world_pos = rel;
    out.material  = v.material;
    out.specular  = v.specular;
    out.eye_vec   = -rel;
    out.uv_diffuse = v.uv_diffuse;
    out.ambient   = v.ambient;
    out.advanced  = v.advanced;
    out.flags     = v.flags;
    out.uv_specular = v.uv_specular;
    out.uv_reflection = v.uv_reflection;
    out.uv_opacity = v.uv_opacity;
    out.uv_bump = v.uv_bump;
    out.uv_refraction = v.uv_refraction;
    out.uv_normal = v.uv_normal;
    return out;
}

@vertex
fn vs_edge(
    v: EdgeVertexIn,
    @builtin(instance_index) instance_index: u32,
) -> EdgeVertexOut {
    var out: EdgeVertexOut;
    let instance = mesh_instances[instance_index];
    let rel = relative_position(v.position, v.position_low, instance);
    out.clip_pos = u.view_rot * vec4<f32>(rel, 1.0);
    out.color = v.color;
    return out;
}

fn map_weight(uv: vec2<f32>, tiling: u32) -> f32 {
    if (tiling == 2u) {
        return select(
            0.0,
            1.0,
            all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0)),
        );
    }
    return 1.0;
}

fn environment_uv(direction: vec3<f32>) -> vec2<f32> {
    let d = normalize(direction);
    let longitude = atan2(d.x, -d.z) + u.environment_params.y;
    let latitude = asin(clamp(d.y, -1.0, 1.0));
    return vec2<f32>(
        fract(longitude / (2.0 * 3.14159265359) + 0.5),
        0.5 - latitude / 3.14159265359,
    );
}

fn shadow_visibility(position: vec3<f32>) -> f32 {
    if u.shadow_params.x < 0.5 {
        return 1.0;
    }
    let clip = u.shadow_view_proj * vec4<f32>(position, 1.0);
    let ndc = clip.xyz / max(abs(clip.w), 1e-6);
    if ndc.z <= 0.0 || ndc.z >= 1.0 || any(abs(ndc.xy) > vec2<f32>(1.0)) {
        return 1.0;
    }
    let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    let dimensions = vec2<f32>(textureDimensions(shadow_texture));
    let texel = 1.0 / max(dimensions, vec2<f32>(1.0));
    let radius = i32(clamp(ceil(u.shadow_params.z * 2.0), 0.0, 2.0));
    var visibility = 0.0;
    var samples = 0.0;
    for (var y = -2; y <= 2; y = y + 1) {
        for (var x = -2; x <= 2; x = x + 1) {
            if abs(x) <= radius && abs(y) <= radius {
                visibility += textureSampleCompare(
                    shadow_texture,
                    shadow_sampler,
                    uv + vec2<f32>(f32(x), f32(y)) * texel,
                    ndc.z - u.shadow_params.y,
                );
                samples += 1.0;
            }
        }
    }
    return visibility / max(samples, 1.0);
}

fn photometric_web(index: u32, cosine: f32) -> f32 {
    if u.light_web_rotation[index].w < 0.5 {
        return 1.0;
    }
    let profile_a = u.light_web_profile_a[index];
    let profile_b = u.light_web_profile_b[index];
    let angle_index = clamp(acos(clamp(cosine, -1.0, 1.0)) / 3.14159265359 * 7.0, 0.0, 7.0);
    let lower = u32(floor(angle_index));
    let upper = min(lower + 1u, 7u);
    let value_at = array<f32, 8>(
        profile_a.x, profile_a.y, profile_a.z, profile_a.w,
        profile_b.x, profile_b.y, profile_b.z, profile_b.w,
    );
    return mix(value_at[lower], value_at[upper], fract(angle_index));
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let two_sided = maps.render_modes.x != 0u;
    var n: vec3<f32>;
    if (u.flat_shade > 0.5) {
        // Per-triangle face normal: derivatives of the interpolated
        // world position are constant across the primitive, so every
        // fragment in the same triangle sees the same normal.
        n = normalize(cross(dpdx(in.world_pos), dpdy(in.world_pos)));
    } else {
        n = normalize(in.normal);
    }
    // Build a derivative tangent frame from the generated material UVs. This
    // works for planar/box/cylindrical/spherical AcDbMaterial projections and
    // does not require ACIS to carry explicit texture-coordinate vertices.
    let dp1 = dpdx(in.world_pos);
    let dp2 = dpdy(in.world_pos);
    let tangent_uv = select(in.uv_bump, in.uv_normal, maps.present1.z != 0u);
    let duv1 = dpdx(tangent_uv);
    let duv2 = dpdy(tangent_uv);
    let det = duv1.x * duv2.y - duv1.y * duv2.x;
    var tangent = normalize(dp1);
    var bitangent = normalize(cross(n, tangent));
    if (abs(det) > 1e-8) {
        tangent = normalize((dp1 * duv2.y - dp2 * duv1.y) / det);
        bitangent = normalize((-dp1 * duv2.x + dp2 * duv1.x) / det);
    }
    if (maps.present1.z != 0u) {
        let sampled = textureSample(normal_map, normal_sampler, in.uv_normal).xyz * 2.0 - 1.0;
        let mapped = normalize(
            tangent * sampled.x + bitangent * sampled.y + n * sampled.z
        );
        let strength = maps.blends1.z
            * map_weight(in.uv_normal, maps.tiling1.z)
            * max(in.advanced.x, 0.0);
        n = normalize(mix(n, mapped, clamp(strength, 0.0, 1.0)));
    } else if (maps.present1.x != 0u) {
        let height = dot(
            textureSample(bump_map, bump_sampler, in.uv_bump).rgb,
            vec3<f32>(0.2126, 0.7152, 0.0722),
        );
        let slope = tangent * dpdx(height) + bitangent * dpdy(height);
        n = normalize(
            n - slope
                * maps.blends1.x
                * map_weight(in.uv_bump, maps.tiling1.x)
                * 4.0
        );
    }

    var albedo = in.color.rgb;
    if (maps.present0.x != 0u) {
        let texel = textureSample(diffuse_map, diffuse_sampler, in.uv_diffuse).rgb;
        let blend = clamp(
            maps.blends0.x * map_weight(in.uv_diffuse, maps.tiling0.x),
            0.0,
            1.0,
        );
        albedo = mix(albedo, texel, blend);
    }

    // Viewport face-colour modes apply to unmaterialed faces. An attached
    // material remains authoritative, including its own diffuse texture.
    if (maps.source_state.z == 0u) {
        let mode = u32(max(u.visual_style.x, 0.0) + 0.5);
        if (mode == 2u) {
            albedo = u.viewport_background.rgb;
        } else if (mode == 3u || mode == 4u) {
            albedo = u.visual_style_color.rgb;
        } else if (mode == 5u) {
            albedo = mix(albedo, u.visual_style_color.rgb, 0.5);
        } else if (mode == 6u) {
            let luminance = dot(albedo, vec3<f32>(0.2126, 0.7152, 0.0722));
            albedo = mix(albedo, vec3<f32>(luminance), 0.3);
        }
    }

    var specular_color = select(in.specular.rgb, albedo, in.flags.x == 1u);
    if (maps.present0.y != 0u) {
        let texel = textureSample(specular_map, specular_sampler, in.uv_specular).rgb;
        let blend = clamp(
            maps.blends0.y * map_weight(in.uv_specular, maps.tiling0.y),
            0.0,
            1.0,
        );
        specular_color = mix(specular_color, texel, blend);
    }
    let view = normalize(in.eye_vec);
    let gloss_exp = mix(2.0, 128.0, clamp(in.material.x, 0.0, 1.0));
    let fresnel0 = pow(
        (max(in.specular.w, 1.0) - 1.0) / (max(in.specular.w, 1.0) + 1.0),
        2.0,
    );
    let fresnel = fresnel0
        + (1.0 - fresnel0) * pow(1.0 - abs(dot(n, view)), 5.0);
    let specular_strength = clamp(1.0 + in.material.y + fresnel, 0.0, 1.5);
    let viewport_highlight = select(
        1.0,
        max(u.visual_style.z, 0.0),
        maps.source_state.z == 0u && u.visual_style.z >= 0.0,
    );

    // The CPU resolves the viewport's default-light policy before upload, so
    // this loop handles both generated distant lights and drawing light data.
    // Diffuse and specular are accumulated from every visible light using the
    // same colour, intensity, cone and attenuation terms.
    var direct_light = vec3<f32>(0.0);
    var direct_specular = vec3<f32>(0.0);
    let shadow = shadow_visibility(in.world_pos);
    let shadow_light_index = u32(max(u.shadow_params.w, 0.0) + 0.5);
    let count = min(u32(u.lighting.x), 4u);
    for (var index = 0u; index < count; index = index + 1u) {
        let position_type = u.light_position_type[index];
        let direction_intensity = u.light_direction_intensity[index];
        let color_hotspot = u.light_color_hotspot[index];
        let attenuation_data = u.light_attenuation[index];
        var light_vector = -normalize(direction_intensity.xyz);
        var attenuation = 1.0;
        if (position_type.w > 1.5) {
            let delta = position_type.xyz - in.world_pos;
            let distance = max(length(delta), 1e-5);
            light_vector = delta / distance;
            if (attenuation_data.x > 1.5) {
                attenuation /= max(distance * distance, 1.0);
            } else if (attenuation_data.x > 0.5) {
                attenuation /= max(distance, 1.0);
            }
            if (attenuation_data.z > attenuation_data.y) {
                attenuation *= 1.0 - smoothstep(
                    attenuation_data.y,
                    attenuation_data.z,
                    distance,
                );
            }
            if (position_type.w > 2.5) {
                let source_to_fragment = -light_vector;
                let cone = dot(
                    source_to_fragment,
                    normalize(direction_intensity.xyz),
                );
                attenuation *= smoothstep(
                    attenuation_data.w,
                    color_hotspot.w,
                    cone,
                );
            }
            attenuation *= photometric_web(
                index,
                dot(-light_vector, normalize(direction_intensity.xyz)),
            );
        }
        let visibility = select(1.0, shadow, index == shadow_light_index);
        let strength = max(direction_intensity.w, 0.0) * attenuation * visibility;
        let response = select(
            max(dot(n, light_vector), 0.0),
            abs(dot(n, light_vector)),
            two_sided,
        );
        direct_light += color_hotspot.rgb * response * strength;
        let half_vec = normalize(light_vector + view);
        let half_response = select(
            max(dot(n, half_vec), 0.0),
            abs(dot(n, half_vec)),
            two_sided,
        );
        direct_specular += color_hotspot.rgb
            * specular_color
            * pow(half_response, gloss_exp)
            * specular_strength
            * viewport_highlight
            * max(in.advanced.z, 0.0)
            * strength;
    }
    let ambient_light = clamp(in.ambient.rgb, vec3<f32>(0.0), vec3<f32>(1.0))
        * clamp(u.lighting.yzw, vec3<f32>(0.0), vec3<f32>(1.0));
    var lit = ambient_light
        + albedo * clamp(direct_light, vec3<f32>(0.0), vec3<f32>(2.0))
        + direct_specular;
    if (u.environment_params.x > 0.5) {
        let diffuse_environment = textureSample(
            environment_texture,
            environment_sampler,
            environment_uv(n),
        ).rgb;
        let reflection_direction = reflect(-view, n);
        let reflected_environment = textureSample(
            environment_texture,
            environment_sampler,
            environment_uv(reflection_direction),
        ).rgb;
        let indirect_enabled = maps.source_state.z == 0u
            || maps.render_modes.z >= 2u
            || maps.render_modes.w != 0u;
        let color_bleed = select(0.0, max(maps.indirect.x, 0.0), indirect_enabled);
        lit += albedo
            * diffuse_environment
            * max(u.environment_params.z, 0.0)
            * color_bleed;
        lit += reflected_environment
            * specular_color
            * specular_strength
            * viewport_highlight
            * max(in.advanced.z, 0.0)
            * max(u.environment_params.w, 0.0);
    }
    if (maps.present0.z != 0u) {
        let reflected = textureSample(
            reflection_map,
            reflection_sampler,
            in.uv_reflection,
        ).rgb;
        lit = mix(
            lit,
            reflected,
            clamp(
                maps.blends0.z
                    * map_weight(in.uv_reflection, maps.tiling0.z)
                    * in.material.y
                    * max(in.advanced.z, 0.0),
                0.0,
                1.0,
            ),
        );
    }
    if (maps.present1.y != 0u) {
        let transmitted = textureSample(
            refraction_map,
            refraction_sampler,
            in.uv_refraction,
        ).rgb;
        lit = mix(
            lit,
            transmitted,
            clamp(
                maps.blends1.y
                    * map_weight(in.uv_refraction, maps.tiling1.y)
                    * in.ambient.a
                    * max(in.advanced.w, 0.0),
                0.0,
                1.0,
            ),
        );
    }
    var rgb = mix(lit, albedo, clamp(in.material.z, 0.0, 1.0));
    if (in.flags.w == 1u) {
        rgb = lit + albedo * max(in.material.w, 0.0);
    }
    let contrast = 1.0 + clamp(u.view_tone.y, -1.0, 1.0);
    rgb = (rgb - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);
    rgb += vec3<f32>(clamp(u.view_tone.x, -1.0, 1.0) * 0.5);
    rgb *= max(1.0 + u.visual_style.w, 0.0);
    if (u.fog_params.x > 0.5) {
        let fog_range = max(u.fog_distances.y - u.fog_distances.x, 1e-6);
        let fog_distance = length(in.world_pos);
        let fog_position = clamp(
            (fog_distance - u.fog_distances.x) / fog_range,
            0.0,
            1.0,
        );
        let fog_density = mix(u.fog_params.z, u.fog_params.w, fog_position);
        rgb = mix(rgb, u.fog_color.rgb, clamp(fog_density, 0.0, 1.0));
    }
    var material_alpha = in.color.a;
    if (maps.present0.w != 0u) {
        let opacity = textureSample(opacity_map, opacity_sampler, in.uv_opacity).r;
        let blend = clamp(
            maps.blends0.w * map_weight(in.uv_opacity, maps.tiling0.w),
            0.0,
            1.0,
        );
        material_alpha *= mix(1.0, opacity, blend);
    }
    material_alpha *= 1.0 - clamp(in.ambient.a * max(in.advanced.w, 0.0), 0.0, 0.95);
    material_alpha *= clamp(u.visual_style.y, 0.0, 1.0);
    let alpha = select(1.0, material_alpha, u.transparency_enable > 0.5);
    return vec4<f32>(rgb, alpha);
}

// Edge fragment (LineList): flat entity colour, no lighting. Used for the
// wireframe and hidden-line edge passes so lines read at their true colour.
@fragment
fn fs_edge(in: EdgeVertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color.rgb, 1.0);
}

// Edge fragment for filled render modes: force black so edges frame the shaded
// fill regardless of the solid's colour.
@fragment
fn fs_edge_black(_in: EdgeVertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

@fragment
fn fs_highlight_selected(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(
        mix(in.color.rgb, vec3<f32>(0.15, 0.55, 1.0), 0.60),
        0.90,
    );
}

@fragment
fn fs_highlight_hover(in: VertexOut) -> @location(0) vec4<f32> {
    return vec4<f32>(
        mix(in.color.rgb, vec3<f32>(0.95, 0.55, 0.10), 0.35),
        0.82,
    );
}
