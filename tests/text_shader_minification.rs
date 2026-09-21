#[test]
fn adaptive_text_sampling_passes_derivative_uniformity_validation() {
    let draw_order = include_str!("../src/shaders/draw_order.wgsl");
    for (name, source) in [
        ("text", include_str!("../src/shaders/text.wgsl")),
        ("block text", include_str!("../src/shaders/block_text.wgsl")),
    ] {
        let composed = format!("{draw_order}\n{source}");
        let module = naga::front::wgsl::parse_str(&composed)
            .unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(&composed)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    }
}
