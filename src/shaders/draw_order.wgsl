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
    // The page of a layout (order below -1, see `Scene::paper_sheet_fill`) is
    // the canvas: inside the depth range it goes to the far end, behind
    // whatever the sheet holds at any Z. Paper space is seen straight down,
    // and a sheet can hold objects far off its plane; a bias alone left those
    // behind the page's plane hidden by it. Outside the range it stays
    // clipped, like any other geometry.
    if order < -1.5 && clip.z >= 0.0 && clip.z <= clip.w {
        return vec4<f32>(clip.xy, clip.w * (1.0 - 1.0e-6), clip.w);
    }
    let room = 0.5 * max(0.0, min(clip.z, clip.w - clip.z));
    let scale = min(0.02 * max(clip.w, 0.0), room / (1.0 + abs(order)));
    return vec4<f32>(clip.xy, clip.z - order * scale, clip.w);
}
