#[test]
fn adaptive_text_sampling_passes_derivative_uniformity_validation() {
    for (name, source) in [
        ("text", include_str!("../src/shaders/text.wgsl")),
        ("block text", include_str!("../src/shaders/block_text.wgsl")),
    ] {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{name}: {error:?}"));
    }
}
