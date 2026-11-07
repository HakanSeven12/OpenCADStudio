//! Read-only geometry for the in-place cell editor.
use super::*;
use glam::DVec3;
use document::{EntityType, Handle, MText};

pub(crate) struct CellPicture {
    pub text: MText,
    pub members: Vec<Handle>,
    pub block_name: Option<String>,
    pub transform: document::types::Transform,
    pub corners: [DVec3; 4],
}

/// Resolve the drawn text in the cell's picture, preserving its authored style.
/// No entity is changed: the members are omitted only from a preview cache.
pub(crate) fn cell_picture(
    doc: &document::Document,
    table: &Table,
    row: usize,
    column: usize,
) -> Option<CellPicture> {
    use document::entities::table::CellStylePropertyFlags as P;
    let range = widen_to_merged(table, (row, column, row, column));
    let (row, column, _, _) = range;
    let cell = table.cell(row, column)?;
    let table_row = &table.rows[row];
    let (x0, x1, y0, y1) = cell_range_edges(table, range);
    let style = table.table_style_handle.and_then(|h| match doc.objects.get(&h) {
        Some(document::objects::ObjectType::TableStyle(style)) => Some(style),
        _ => None,
    });
    let up = resolved_flow_up(table, style);
    let sign = if up { 1.0 } else { -1.0 };
    let block_use = crate::scene::render_graph::entity_render_block_uses(
        doc, &EntityType::Table(Box::new(table.clone())), 1.0,
    ).into_iter().find(|u| u.active && u.role == crate::scene::render_graph::BlockRole::TablePicture);
    let block_name = block_use.as_ref().map(|u| u.insert.block_name.clone());
    let transform = block_use.as_ref().map(|u| {
        crate::scene::render_graph::insert_transform(doc, &u.insert)
    }).unwrap_or_else(|| {
        let mut insert = document::entities::Insert::new("", table.insertion_point);
        insert.normal = table.normal;
        insert.rotation = table.horizontal_direction.y.atan2(table.horizontal_direction.x);
        insert.get_transform()
    });
    let world = |x, y| {
        let p = transform.apply(document::types::Vector3::new(x, y, 0.0));
        DVec3::new(p.x, p.y, p.z)
    };
    let corners = [world(x0, sign * y0), world(x1, sign * y0),
        world(x1, sign * y1), world(x0, sign * y1)];
    let mut members = Vec::new();
    let mut authored = None;
    if let Some(block) = block_name.as_deref().and_then(|name| doc.block_records.get(name)) {
        let (mx, my) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
        let mut distance = f64::MAX;
        for &handle in &block.entity_handles {
            let Some(EntityType::MText(text)) = doc.get_entity(handle) else { continue };
            let (x, y) = (text.insertion_point.x, sign * text.insertion_point.y);
            if x < x0 - 1e-6 || x > x1 + 1e-6 || y < y0 - 1e-6 || y > y1 + 1e-6 {
                continue;
            }
            // Prefer a point strictly inside the cell to a neighbour on its edge.
            let d = (x - mx).powi(2) + (y - my).powi(2);
            if d < distance {
                distance = d;
                authored = Some((handle, (**text).clone()));
            }
        }
    }
    let mut text = if let Some((handle, text)) = authored {
        members.push(handle);
        text
    } else {
        let content = cell.contents.first().cloned().unwrap_or_default();
        let role = style.map(|s| match cell_role_kind(table,
            resolved_title_suppressed(table, style), resolved_header_suppressed(table, style), row, column) {
            0 => &s.title_row_style, 1 => &s.header_row_style, _ => &s.data_row_style,
        });
        let height = cell_text_height(table, table_row, column, cell, &content,
            role.map(|s| s.text_height)).unwrap_or(0.18);
        let (hm, vm) = resolved_table_margins(table, style);
        let margin = |p, default: f64| style_for_property(table, table_row, column, cell, p)
            .map(|s| match p { P::MARGIN_LEFT => s.margin_left, P::MARGIN_RIGHT => s.margin_right,
                P::MARGIN_TOP => s.margin_top, _ => s.margin_bottom })
            .unwrap_or(default.max(height * 0.5));
        let alignment = cell_content_alignment(table, table_row, column, cell,
            Some(&content), role.map(|s| s.alignment as i32)).clamp(1, 9);
        let slot = (alignment - 1) as usize;
        let x = [x0 + margin(P::MARGIN_LEFT, hm), (x0 + x1) * 0.5,
            x1 - margin(P::MARGIN_RIGHT, hm)][slot % 3];
        let y = [y0 + margin(P::MARGIN_TOP, vm), (y0 + y1) * 0.5,
            y1 - margin(P::MARGIN_BOTTOM, vm)][slot / 3];
        let mut mt = MText::new();
        mt.height = height;
        mt.insertion_point = document::types::Vector3::new(x, sign * y, 0.0);
        mt.attachment_point = match alignment {
            1 => document::entities::AttachmentPoint::TopLeft,
            2 => document::entities::AttachmentPoint::TopCenter,
            3 => document::entities::AttachmentPoint::TopRight,
            4 => document::entities::AttachmentPoint::MiddleLeft,
            6 => document::entities::AttachmentPoint::MiddleRight,
            7 => document::entities::AttachmentPoint::BottomLeft,
            8 => document::entities::AttachmentPoint::BottomCenter,
            9 => document::entities::AttachmentPoint::BottomRight,
            _ => document::entities::AttachmentPoint::MiddleCenter,
        };
        mt.rectangle_width = (x1 - x0 - margin(P::MARGIN_LEFT, hm) - margin(P::MARGIN_RIGHT, hm)).max(0.0);
        mt.rotation = style_for_property(table, table_row, column, cell, P::ROTATION)
            .map(|s| s.rotation).unwrap_or(cell.rotation);
        if let Some(record) = cell_text_style(doc, table, table_row, column, cell, Some(&content))
            .or_else(|| row_style_text_style(doc, role))
            .and_then(|h| doc.text_styles.iter().find(|s| s.handle == h)) {
            mt.style = record.name.clone();
        }
        mt
    };
    // The buffer contains source, including a formula, rather than its result.
    text.value = cell.contents.first().map(|c| {
        if c.value.text.is_empty() { c.value.display().to_string() } else { c.value.text.clone() }
    }).unwrap_or_default();
    Some(CellPicture { text, members, block_name, transform, corners })
}
