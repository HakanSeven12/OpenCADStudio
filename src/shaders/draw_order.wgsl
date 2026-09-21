// Positive order draws in front; zero retains geometric depth. Keep the usual
// 0.001 NDC offset away from the clipping planes, but reduce it near either
// plane: perspective depth can be much closer to 1 than this fixed offset.
//
// Using order / (1 + abs(order)) for the limited region keeps ordering monotonic
// (including the paper background's -2). Less than half the remaining depth is
// consumed, so valid vertices stay valid. Out-of-range/behind-eye vertices get
// no offset and remain subject to the real near/far clipping planes.
fn apply_draw_order(clip: vec4<f32>, order: f32) -> vec4<f32> {
    let room = 0.5 * max(0.0, min(clip.z, clip.w - clip.z));
    let scale = min(0.001 * max(clip.w, 0.0), room / (1.0 + abs(order)));
    return vec4<f32>(clip.xy, clip.z - order * scale, clip.w);
}
