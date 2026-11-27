//! The vertex of an SDF text quad, as the text pipeline uploads it. Its
//! buffer layout is the pipeline's (`scene::pipeline::text_gpu`).

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TextVertex {
    pub pos: [f32; 3],
    pub pos_low: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub draw_depth: f32,
    /// Pen half-width of a stroke glyph in atlas glyph units (0 = filled).
    pub pen: f32,
} // 56 bytes, no padding holes (all f32)

/// Split an f64 into the double-single (high f32, low residual f32) pair the
/// shaders reconstruct relative-to-eye.
pub fn split_ds(v: f64) -> (f32, f32) {
    let high = v as f32;
    (high, (v - high as f64) as f32)
}
