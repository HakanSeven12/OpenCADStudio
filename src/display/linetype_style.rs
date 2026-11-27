//! The linetype and pattern an object draws with in a viewport: its layer's
//! linetype unless the viewport overrides it, resolved to a dash pattern.

use document::{Document, Handle};
use document::LineType;

pub(crate) fn linetype_name_for_common_viewport<'a>(
    document: &'a Document,
    common: &'a document::entities::EntityCommon,
    viewport: Option<Handle>,
) -> &'a str {
    let elt = &common.linetype;
    if elt.is_empty() || elt.eq_ignore_ascii_case("bylayer") {
        layer_linetype_name_for_viewport(document, &common.layer, viewport)
    } else {
        elt.as_str()
    }
}

/// A layer's linetype name, with its viewport override when it has one.
pub(crate) fn layer_linetype_name_for_viewport<'a>(
    document: &'a Document,
    layer: &str,
    viewport: Option<Handle>,
) -> &'a str {
    if viewport.is_some() {
        if let Some(handle) = viewport_override(
            document,
            layer,
            viewport,
            document::objects::KnownXRecordKind::LayerViewportLinetypeOverride,
        )
        .and_then(|value| value.as_handle())
        {
            if let Some(line_type) = document
                .line_types
                .iter()
                .find(|line_type| line_type.handle == handle)
            {
                return line_type.name.as_str();
            }
        }
    }
    document
        .layers
        .get(layer)
        .map(|l| l.line_type.as_str())
        .unwrap_or("Continuous")
}

pub(crate) fn viewport_override_for_layer(
    document: &Document,
    layer: Option<&document::tables::Layer>,
    viewport: Option<Handle>,
    kind: document::objects::KnownXRecordKind,
) -> Option<document::objects::XRecordValue> {
    let viewport = viewport.filter(|handle| handle.is_valid())?;
    let layer = layer?;
    document
        .layer_viewport_overrides(layer.handle, kind)
        .into_iter()
        .find_map(|(handle, value)| (handle == viewport).then_some(value))
}

pub(crate) fn viewport_override(
    document: &Document,
    layer_name: &str,
    viewport: Option<Handle>,
    kind: document::objects::KnownXRecordKind,
) -> Option<document::objects::XRecordValue> {
    viewport.filter(|handle| handle.is_valid())?;
    viewport_override_for_layer(document, document.layers.get(layer_name), viewport, kind)
}

pub(crate) fn resolve_pattern(
    table: &document::tables::Table<LineType>,
    name: &str,
    scale: f32,
) -> (f32, [f32; 8]) {
    let solid = (0.0, [0.0f32; 8]);
    if name.eq_ignore_ascii_case("continuous")
        || name.eq_ignore_ascii_case("bylayer")
        || name.eq_ignore_ascii_case("byblock")
        || name.is_empty()
    {
        return solid;
    }
    let lt = match table.get(name) {
        Some(lt) => lt,
        None => return solid,
    };
    if lt.is_continuous() || lt.elements.is_empty() {
        return solid;
    }

    // Keep dots (element length exactly 0) as 0.0 so the shader can render
    // them round at the stroke's width; trailing array slots stay 0.0 padding and
    // the shader tells the two apart by position (a 0.0 before the last
    // non-zero element is a dot, trailing 0.0s are padding). The old code
    // encoded dots as `0.01 * scale` — a tiny world-length dash that went
    // sub-pixel at normal zoom and dragged the pattern's `min_elem` below one
    // pixel, so the dash LOD collapsed dotted / dash-dot lines to solid (or,
    // at larger LTSCALE, left only invisible sub-pixel dots between big
    // gaps). (#149)
    let mut pat = [0.0f32; 8];
    let mut pat_len = 0.0f32;
    for (i, el) in lt.elements.iter().take(8).enumerate() {
        // positive = dash, negative = gap, exactly 0 = dot.
        let v = el.length as f32 * scale;
        pat[i] = v;
        pat_len += v.abs();
    }
    if pat_len < 1e-6 {
        return solid;
    }
    (pat_len, pat)
}
