//! How the elements of a multiline draw: each element styled like any other
//! line, with complex linetypes walked along it.

use document::entities::MLine;

use crate::entities::mline::{element_common, element_linetype_name};

/// How an element draws in `viewport`: its [`element_common`] line styled like
/// any other entity, as `(colour, pattern length, pattern, ACI)`. A complex
/// linetype is walked along the element rather than dashed, so it has no pattern.
pub(crate) fn element_style(
    document: &document::Document,
    m: &MLine,
    color: document::types::Color,
    linetype: &str,
    viewport: Option<document::Handle>,
) -> ([f32; 4], f32, [f32; 8], u8) {
    let common = element_common(m, color, linetype, document);
    let (color, pattern_length, pattern, _, aci) =
        crate::scene::view::render::render_style_for_common_viewport(document, &common, viewport);
    let drawn = element_linetype_name(document, &m.common.layer, linetype, viewport);
    if crate::catalog::linetypes::resolve_complex_lt(document, drawn).is_some() {
        (color, 0.0, [0.0; 8], aci)
    } else {
        (color, pattern_length, pattern, aci)
    }
}
