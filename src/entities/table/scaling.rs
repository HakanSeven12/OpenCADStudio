use super::*;
use document::entities::table::{CellContentGeometry, CellStyle, CellStylePropertyFlags as P};

fn scale_factor(t: &EntityTransform) -> f64 {
    transform::codec_transform(t)
        .apply_rotation(document::types::Vector3::UNIT_X)
        .length()
}

/// Freeze inherited dimensions on this instance before scaling; shared table
/// styles must retain their dimensions for every other table using them.
pub(crate) fn prepare(table: &Table, doc: &document::Document, t: &EntityTransform) -> Option<Table> {
    let factor = scale_factor(t);
    if !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
        return None;
    }
    let ts = table
        .table_style_handle
        .and_then(|h| match doc.objects.get(&h) {
            Some(document::objects::ObjectType::TableStyle(style)) => Some(style),
            _ => None,
        });
    let (hm, vm) = resolved_table_margins(table, ts);
    let title = resolved_title_suppressed(table, ts);
    let header = resolved_header_suppressed(table, ts);
    let mut result = table.clone();
    for (ri, row) in table.rows.iter().enumerate() {
        let row_named = ts.and_then(|ts| {
            let role = match (title, header, ri) {
                (false, _, 0) => document::objects::TITLE_CELL_STYLE,
                (false, false, 1) | (true, false, 0) => document::objects::HEADER_CELL_STYLE,
                _ => document::objects::DATA_CELL_STYLE,
            };
            ts.cell_style_by_id(row.style_id)
                .or_else(|| ts.cell_style(role))
        });
        let margins = row_named
            .and_then(|s| s.cell_style.margins())
            .unwrap_or((hm, vm));
        for (ci, cell) in row.cells.iter().enumerate() {
            let named = ts
                .and_then(|ts| {
                    let id = if cell.style_id != 0 {
                        cell.style_id
                    } else {
                        table.columns.get(ci).map_or(0, |c| c.style_id)
                    };
                    ts.cell_style_by_id(id)
                })
                .or(row_named);
            let projection = named.map(|s| s.cell_style.to_row_style());
            let legacy = ts.map(|ts| match cell_role_kind(table, title, header, ri, ci) {
                0 => &ts.title_row_style,
                1 => &ts.header_row_style,
                _ => &ts.data_row_style,
            });
            let inherited_height = projection
                .as_ref()
                .or(legacy)
                .map_or(0.18, |s| s.text_height);
            let height = style_for_property(table, row, ci, cell, P::TEXT_HEIGHT)
                .map_or(inherited_height, |s| s.text_height);
            let mut style = cell.style.clone().unwrap_or_default();
            macro_rules! dimension {
                ($property:ident, $field:ident, $fallback:expr) => {
                    style.$field = style_for_property(table, row, ci, cell, P::$property)
                        .map_or($fallback, |s| s.$field);
                    style.property_flags.insert(P::$property);
                };
            }
            dimension!(TEXT_HEIGHT, text_height, height);
            dimension!(MARGIN_LEFT, margin_left, margins.0.max(height * 0.5));
            dimension!(MARGIN_RIGHT, margin_right, margins.0.max(height * 0.5));
            dimension!(MARGIN_TOP, margin_top, margins.1.max(height * 0.5));
            dimension!(MARGIN_BOTTOM, margin_bottom, margins.1.max(height * 0.5));
            dimension!(MARGIN_HORIZONTAL_SPACING, horizontal_spacing, 0.0);
            dimension!(MARGIN_VERTICAL_SPACING, vertical_spacing, 0.0);
            for (index, content) in cell.contents.iter().enumerate() {
                let text_height = cell_text_height(
                    table,
                    row,
                    ci,
                    cell,
                    content,
                    projection.as_ref().or(legacy).map(|style| style.text_height),
                )
                .unwrap_or(height);
                let target = &mut result.rows[ri].cells[ci].contents[index];
                target.text_height = text_height;
                target.format_property_flags |= P::TEXT_HEIGHT.bits() as i32;
            }
            result.rows[ri].cells[ci].style = Some(style);
        }
    }
    let legacy = result
        .legacy_style_override
        .get_or_insert_with(Default::default);
    legacy.horizontal_cell_margin = Some(hm);
    legacy.vertical_cell_margin = Some(vm);
    Some(result)
}

fn scale_style(style: &mut CellStyle, factor: f64) {
    style.text_height *= factor;
    style.scale *= factor;
    style.margin_left *= factor;
    style.margin_right *= factor;
    style.margin_top *= factor;
    style.margin_bottom *= factor;
    style.horizontal_spacing *= factor;
    style.vertical_spacing *= factor;
    for border in [
        &mut style.top_border,
        &mut style.right_border,
        &mut style.bottom_border,
        &mut style.left_border,
    ] {
        border.double_spacing *= factor;
    }
    for (_, border) in &mut style.additional_borders {
        border.double_spacing *= factor;
    }
}

fn scale_geometry(geometry: &mut CellContentGeometry, factor: f64) {
    geometry.distance_to_top_left = geometry.distance_to_top_left * factor;
    geometry.distance_to_center = geometry.distance_to_center * factor;
    geometry.width *= factor;
    geometry.height *= factor;
    geometry.outer_width *= factor;
    geometry.outer_height *= factor;
}

/// The document transform handles the insertion, axes, row heights and column
/// widths. Complete its dimensional data and request a fresh picture block.
pub(super) fn finish(table: &mut Table, t: &EntityTransform) {
    let factor = scale_factor(t);
    if !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
        return;
    }
    for style in table
        .base_style
        .iter_mut()
        .chain(table.columns.iter_mut().filter_map(|c| c.style.as_mut()))
    {
        scale_style(style, factor);
    }
    for row in &mut table.rows {
        if let Some(style) = &mut row.style {
            scale_style(style, factor);
        }
        for cell in &mut row.cells {
            if let Some(style) = &mut cell.style {
                scale_style(style, factor);
            }
            cell.block_scale *= factor;
            cell.geometry_width_with_gap *= factor;
            cell.geometry_height_with_gap *= factor;
            for geometry in cell.geometry.iter_mut().chain(cell.geometries.iter_mut()) {
                scale_geometry(geometry, factor);
            }
            for content in &mut cell.contents {
                content.text_height *= factor;
                content.scale *= factor;
                if let Some(geometry) = &mut content.geometry {
                    scale_geometry(geometry, factor);
                }
            }
        }
    }
    if let Some(legacy) = &mut table.legacy_style_override {
        for margin in [
            &mut legacy.horizontal_cell_margin,
            &mut legacy.vertical_cell_margin,
        ] {
            if let Some(value) = margin {
                *value *= factor;
            }
        }
        for height in &mut legacy.row_heights {
            *height *= factor;
        }
    }
    table.break_spacing *= factor;
    for data in &mut table.break_data {
        data.position = data.position * factor;
        data.height *= factor;
    }
    for range in &mut table.break_ranges {
        range.position = range.position * factor;
    }
    table.block_record_handle = None;
}

pub(crate) fn picture_members(table: &Table, doc: &document::Document) -> Vec<document::Handle> {
    table
        .block_record_handle
        .and_then(|h| doc.block_records.iter().find(|record| record.handle == h))
        .or_else(|| doc.block_records.get(&table.block_name))
        .map_or_else(Vec::new, |record| record.entity_handles.clone())
}

pub(super) fn prepare_unstyled(table: &mut Table, t: &EntityTransform) {
    let dimensions = P::TEXT_HEIGHT
        | P::MARGIN_LEFT
        | P::MARGIN_RIGHT
        | P::MARGIN_TOP
        | P::MARGIN_BOTTOM
        | P::MARGIN_HORIZONTAL_SPACING
        | P::MARGIN_VERTICAL_SPACING;
    if table.table_style_handle.is_none()
        && table.rows.iter().any(|row| {
            row.cells.iter().any(|cell| {
                !cell
                    .style
                    .as_ref()
                    .is_some_and(|style| style.property_flags.contains(dimensions))
            })
        })
    {
        if let Some(prepared) = prepare(table, &document::Document::new(), t) {
            *table = prepared;
        }
    }
}
