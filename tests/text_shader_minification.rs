//! WGSL composition check for the adaptive text-sampling shaders.
//!
//! Table-driven integration test: each shader is an `#[case]`, so a failure
//! names the shader instead of failing an anonymous loop iteration.

use rstest::rstest;

#[rstest]
#[case("text", include_str!("../src/shaders/text.wgsl"))]
#[case("block text", include_str!("../src/shaders/block_text.wgsl"))]
fn adaptive_text_sampling_passes_derivative_uniformity_validation(
    #[case] name: &str,
    #[case] source: &str,
) {
    let draw_order = include_str!("../src/shaders/draw_order.wgsl");
    let composed = format!("{draw_order}\n{source}");

    pretty_assertions::assert_eq!(
        &composed[..draw_order.len()],
        draw_order,
        "the shared draw_order helper must be prepended verbatim to {name}"
    );

    let module = naga::front::wgsl::parse_str(&composed)
        .unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(&composed)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("{name}: {error:?}"));
}
