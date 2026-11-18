struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

struct CompositeUniforms {
    uv_scale: vec2<f32>,
    // Premultiplied colour, so the whole sample scales.
    opacity: f32,
    _pad: f32,
}

var<private> POSITIONS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(-1.0,  1.0),
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0,  1.0),
    vec2<f32>(-1.0, -1.0),
    vec2<f32>( 1.0, -1.0),
    vec2<f32>( 1.0,  1.0),
);

var<private> UVS: array<vec2<f32>, 6> = array<vec2<f32>, 6>(
    vec2<f32>(0.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 0.0),
    vec2<f32>(0.0, 1.0),
    vec2<f32>(1.0, 1.0),
    vec2<f32>(1.0, 0.0),
);

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(POSITIONS[index], 0.0, 1.0);
    output.uv = UVS[index];
    return output;
}

@group(0) @binding(0) var resolved_texture: texture_2d<f32>;
@group(0) @binding(1) var resolved_sampler: sampler;
@group(0) @binding(2) var<uniform> uniforms: CompositeUniforms;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(resolved_texture, resolved_sampler, input.uv * uniforms.uv_scale)
        * uniforms.opacity;
}
