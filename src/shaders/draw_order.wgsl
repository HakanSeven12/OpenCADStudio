// Positive draw order moves geometry towards the camera. In dense blocks,
// child ranks occupy a narrow fraction of the INSERT depth band: a 0.001
// NDC bias can quantize two overlapping lines/masks to the same depth value,
// especially after changing view/zoom. Use a wider, yet bounded range for
// CAD 2D draw order. Limit displacement to less than half the available
// clip-space room, so we never push valid geometry across near/far planes.
// This is a screen-only GPU change; PDF order uses its own stable sorting.
fn apply_draw_order(clip: vec4<f32>, order: f32) -> vec4<f32> {
    // An instance hidden in place carries a depth no entity ever has: send it
    // outside the clip volume so it draws nothing until it is written back.
    if order > 1.0e30 {
        return vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    let room = 0.5 * max(0.0, min(clip.z, clip.w - clip.z));
    let scale = min(0.02 * max(clip.w, 0.0), room / (1.0 + abs(order)));
    return vec4<f32>(clip.xy, clip.z - order * scale, clip.w);
}
