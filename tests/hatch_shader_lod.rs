const STORAGE_SHADER: &str = include_str!("../src/shaders/hatch.wgsl");
const TEXTURE_SHADER: &str = include_str!("../src/shaders/hatch_texture.wgsl");
const DRAW_ORDER_SHADER: &str = include_str!("../src/shaders/draw_order.wgsl");

fn assert_all_family_lod(source: &str, name: &str) {
    let composed = format!("{DRAW_ORDER_SHADER}\n{source}");
    let module = naga::front::wgsl::parse_str(&composed)
        .unwrap_or_else(|error| panic!("{name} WGSL must parse: {error}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("{name} WGSL must validate: {error}"));

    assert!(
        source.contains("var all_families_subpixel"),
        "{name} must track the all-family LOD transition"
    );
    assert!(
        source.contains("if all_families_subpixel"),
        "{name} must solid-fill only at the all-family transition"
    );
    assert!(
        !source.contains("if is_subpixel") && !source.contains("visible_families"),
        "{name} must not skip individual families before LOD activates"
    );
    assert!(
        !source.contains("min_spacing_world"),
        "{name} must not solid-fill a complex pattern because of one dense family"
    );
}

#[test]
fn storage_hatch_lod_waits_for_every_family() {
    assert_all_family_lod(STORAGE_SHADER, "storage hatch");
}

#[test]
fn texture_hatch_lod_waits_for_every_family() {
    assert_all_family_lod(TEXTURE_SHADER, "texture hatch");
}
