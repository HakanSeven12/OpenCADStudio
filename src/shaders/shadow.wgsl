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

struct MeshInstance {
    model_row_0: vec4<f32>,
    model_row_1: vec4<f32>,
    model_row_2: vec4<f32>,
    translation_low: vec4<f32>,
    normal_row_0: vec4<f32>,
    normal_row_1: vec4<f32>,
    normal_row_2: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(15) var<storage, read> mesh_instances: array<MeshInstance>;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(3) position_low: vec3<f32>,
};

@vertex
fn vs_main(
    vertex: VertexIn,
    @builtin(instance_index) instance_index: u32,
) -> @builtin(position) vec4<f32> {
    let instance = mesh_instances[instance_index];
    let world_high = vec3<f32>(
        dot(instance.model_row_0.xyz, vertex.position) + instance.model_row_0.w,
        dot(instance.model_row_1.xyz, vertex.position) + instance.model_row_1.w,
        dot(instance.model_row_2.xyz, vertex.position) + instance.model_row_2.w,
    );
    let world_low = vec3<f32>(
        dot(instance.model_row_0.xyz, vertex.position_low),
        dot(instance.model_row_1.xyz, vertex.position_low),
        dot(instance.model_row_2.xyz, vertex.position_low),
    ) + instance.translation_low.xyz;
    let relative = (world_high - u.eye_high) + (world_low - u.eye_low);
    return u.shadow_view_proj * vec4<f32>(relative, 1.0);
}
