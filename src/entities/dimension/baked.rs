// Baked radial geometry and synthesized dimension text.

use super::*;

pub(crate) fn baked_radial_geometry(
    dimension: &Dimension,
    document: &Document,
    annotation_scale: f64,
    arrow1: &ArrowKind,
    arrow2: &ArrowKind,
) -> DimGeom {
    // Same local frame as the render path: the geometry is f32, so build it
    // around a nearby origin and let `DimGeom::origin` carry it back.
    let origin = dimension_precision_origin(dimension);
    let shifted = (origin != Vector3::ZERO).then(|| shift_dimension(dimension, origin));
    let dimension = shifted.as_ref().unwrap_or(dimension);
    let effective_style = dimension_source_style(document, dimension)
        .map(|style| resolved_dimension_style(style, dimension, document));
    let style = effective_style.as_ref();
    let scale = style
        .map(|style| dimension_style_scale(style, annotation_scale))
        .unwrap_or(1.0);
    let text_height = style
        .map(|style| style.dimtxt * scale)
        .unwrap_or(2.5 * scale);
    let arrow_size_raw = style
        .map(|style| style.dimasz * scale)
        .unwrap_or(0.18 * scale);
    let tick_size = style.map(|style| style.dimtsz * scale).unwrap_or(0.0);
    let arrow_size = (arrow_size_raw as f32).max(0.001);
    let text = dimension_text_layout(dimension, style, text_height, scale, Some(document));
    let mut geometry = dimension_geometry(
        dimension,
        arrow1,
        arrow2,
        DimLineParams {
            dimexo: style
                .map(|style| (style.dimexo * scale) as f32)
                .unwrap_or(0.0),
            dimexe: style
                .map(|style| (style.dimexe * scale) as f32)
                .unwrap_or(0.0),
            dimdle: style
                .map(|style| (style.dimdle * scale) as f32)
                .unwrap_or(0.0),
            dimfxl: style
                .map(|style| (style.dimfxl * scale) as f32)
                .unwrap_or(1.0),
            dimfxlon: style.is_some_and(|style| style.dimfxlon),
            dimsoxd: style.is_some_and(|style| style.dimsoxd),
            dimcen: style
                .map(|style| (style.dimcen * scale) as f32)
                .unwrap_or(0.09),
            ticks: tick_size > 1.0e-9
                || (matches!(arrow1, ArrowKind::Tick { .. })
                    && matches!(arrow2, ArrowKind::Tick { .. })),
            arrow_len: arrow_size,
            text_width: text.width,
            text_height: text_height as f32,
            constraint: dynamic_constraint_dimension(dimension),
            dimatfit: style.map(|style| style.dimatfit).unwrap_or(3),
            dimtix: style.is_some_and(|style| style.dimtix),
            dimtofl: style.is_some_and(|style| style.dimtofl),
            flip1: dimension.base().flip_arrow1,
            flip2: dimension.base().flip_arrow2,
            text_position: text.position,
            horizontal_text: text.horizontal,
            text_lifted: style.map(|style| style.dimtad).unwrap_or(1) != 0
                || text_splits_at_line(dimension),
            text_movement: style.map(|style| style.dimtmove).unwrap_or(0),
            text_break: text.break_box,
            leader_anchor: stored_text_point(dimension)
                .filter(|_| style.is_some_and(|style| style.dimtmove == 1))
                .map(vec3_local),
        },
        SuppressFlags {
            ext1: style.is_some_and(|style| style.dimse1),
            ext2: style.is_some_and(|style| style.dimse2),
            dim1: style.is_some_and(|style| style.dimsd1),
            dim2: style.is_some_and(|style| style.dimsd2),
        },
    );
    geometry.origin = origin;
    if !style.is_some_and(|style| style.dimse1) {
        if let Some(points) = crate::scene::dimension_assoc::radial_extension_points(
            document,
            dimension.base().common.handle,
            style.map(|style| style.dimexo * scale).unwrap_or(0.0),
            style.map(|style| style.dimexe * scale).unwrap_or(0.0),
        ) {
            let points: Vec<_> = points
                .into_iter()
                .map(|point| vec3_local(point_from_origin(point, origin)))
                .collect();
            add_polyline(&mut geometry.ext_lines, &points);
        }
    }
    if style.is_some_and(|style| style.dimtmove == 1) {
        if let Some((anchor, endpoint, under_text)) = dimtmove_leader_endpoints(dimension, text) {
            if under_text || anchor.distance(endpoint) > text_height as f32 {
                add_segment_with_text_break(
                    &mut geometry.dim_lines,
                    anchor,
                    endpoint,
                    text.break_box,
                );
            }
        }
    }
    apply_dimension_breaks(
        document,
        dimension.base().common.handle,
        &mut geometry.dim_lines,
        origin,
    );
    apply_dimension_breaks(
        document,
        dimension.base().common.handle,
        &mut geometry.ext_lines,
        origin,
    );
    geometry
}

/// Build measurement text for a saved dimension block.
pub(crate) fn baked_dimension_text_entity(
    dim: &Dimension,
    document: &Document,
    anno_scale: f64,
) -> Option<EntityType> {
    let (ent, _) = baked_dimension_text(dim, document, anno_scale)?;
    let mut ent = ent;
    if let EntityType::Text(t) = &mut ent {
        t.alignment_point = Some(t.insertion_point);
    }
    Some(ent)
}

/// The measurement text of a saved dimension block, with the DIMINSPECT
/// label, rate and frame when the dimension has them.
pub(crate) fn baked_dimension_text_entities(
    dim: &Dimension,
    document: &Document,
    anno_scale: f64,
) -> Vec<EntityType> {
    let Some((ent, gap)) = baked_dimension_text(dim, document, anno_scale) else {
        return Vec::new();
    };
    let mut entities = match crate::entities::dim_inspect::read(&dim.base().common.extended_data) {
        Some(inspection) => crate::entities::dim_inspect::layout(&inspection, ent, document, gap),
        None => vec![ent],
    };
    for ent in &mut entities {
        if let EntityType::Text(t) = ent {
            t.alignment_point = Some(t.insertion_point);
        }
    }
    entities
}

/// The measurement text entity and the scaled DIMGAP of a saved dimension.
pub(crate) fn baked_dimension_text(
    dim: &Dimension,
    document: &Document,
    anno_scale: f64,
) -> Option<(EntityType, f64)> {
    let effective_style = dimension_source_style(document, dim)
        .map(|style| resolved_dimension_style(style, dim, document));
    let style = effective_style.as_ref();
    let dim_scale = style
        .map(|style| dimension_style_scale(style, anno_scale))
        .unwrap_or(1.0);
    let dim_txt = style
        .map(|s| s.dimtxt * dim_scale)
        .unwrap_or(2.5 * dim_scale);
    let gap = style.map(|s| s.dimgap.abs()).unwrap_or(0.0) * dim_scale;
    Some((
        dimension_text_entity(dim, dim_txt, style, document, dim_scale)?,
        gap,
    ))
}

pub(crate) fn baked_arc_length_symbol_points(
    dim: &Dimension,
    document: &Document,
    anno_scale: f64,
) -> Vec<Vector3> {
    if !matches!(dim, Dimension::Arc(_)) {
        return Vec::new();
    }
    let style = dimension_source_style(document, dim);
    let dim_scale = style
        .map(|style| {
            if style.dimscale > 1.0e-6 {
                style.dimscale
            } else {
                anno_scale
            }
        })
        .unwrap_or(1.0);
    let text_height = style
        .map(|style| style.dimtxt * dim_scale)
        .unwrap_or(2.5 * dim_scale);
    let symbol_position = crate::entities::dim_override::int(
        &dim.base().common.extended_data,
        crate::entities::dim_override::DIMARCSYM,
    )
    .or_else(|| style.map(|style| style.dimarcsym))
    .unwrap_or(0);

    arc_length_symbol_points(
        dim,
        style,
        text_height,
        dim_scale,
        symbol_position,
        Some(document),
    )
    .unwrap_or_default()
    .into_iter()
    .map(|point| Vector3::new(point.x as f64, point.y as f64, point.z as f64))
    .collect()
}

pub(crate) fn dimension_text_grip_position(
    dim: &Dimension,
    document: &Document,
    anno_scale: f64,
) -> Option<Vector3> {
    // A stored text point is where the text is drawn; the grip goes there.
    if let Some(p) = stored_text_point(dim) {
        return Some(p);
    }

    // For automatic text, use the same text-building path used by the
    // dimension picture so the grip follows the actual DIMSTYLE placement,
    // including annotation scaling and fit behaviour.
    match baked_dimension_text_entity(dim, document, anno_scale)? {
        EntityType::Text(text) => Some(text.insertion_point),
        EntityType::MText(text) => Some(text.insertion_point),
        _ => None,
    }
}
