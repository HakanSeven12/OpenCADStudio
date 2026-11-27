//! The vertex of an SDF text quad, as the text pipeline uploads it. Its
//! buffer layout is the pipeline's (`scene::pipeline::text_gpu`).

use crate::text::glyph_quads::GlyphQuad;

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

/// Append the two triangles (6 vertices) for each glyph quad of one text run
/// into `out`, placing the run-local quad corners into world space:
/// `world = corner * anno + origin`, then double-single split.
///
/// UV mapping (atlas row 0 = top; quad corners are BL, BR, TR, TL):
///   BL -> (u_min, v_max)  BR -> (u_max, v_max)
///   TR -> (u_max, v_min)  TL -> (u_min, v_min)
pub fn push_glyph_vertices(
    out: &mut Vec<TextVertex>,
    quads: &[GlyphQuad],
    origin: [f64; 3],
    anno: f64,
    color: [f32; 4],
    draw_depth: f32,
) {
    for q in quads {
        let mk = |ci: usize, uv: [f32; 2]| -> TextVertex {
            let c = q.corners[ci];
            let wx = c[0] as f64 * anno + origin[0];
            let wy = c[1] as f64 * anno + origin[1];
            let wz = origin[2];
            let (xh, xl) = split_ds(wx);
            let (yh, yl) = split_ds(wy);
            let (zh, zl) = split_ds(wz);
            TextVertex {
                pos: [xh, yh, zh],
                pos_low: [xl, yl, zl],
                uv,
                color,
                draw_depth,
                pen: q.pen,
            }
        };
        let bl = [q.uv_min[0], q.uv_max[1]];
        let br = [q.uv_max[0], q.uv_max[1]];
        let tr = [q.uv_max[0], q.uv_min[1]];
        let tl = [q.uv_min[0], q.uv_min[1]];
        out.push(mk(0, bl));
        out.push(mk(1, br));
        out.push(mk(2, tr));
        out.push(mk(0, bl));
        out.push(mk(2, tr));
        out.push(mk(3, tl));
    }
}
