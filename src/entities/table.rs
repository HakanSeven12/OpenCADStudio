use acadrust::entities::Table;
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{ro_prop as ro, square_grip};
use crate::entities::text_support::{
    layout_mtext, MTextRenderOpts, MTextVAnchor, ResolvedTextStyle,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, Property, PropValue};
use crate::scene::view::transform;
use crate::scene::model::wire_model::SnapHint;
use crate::t;

thread_local! {
    static PROPERTY_CELL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PROPERTY_CELL_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn set_prop_current_cell(index: usize) {
    PROPERTY_CELL.with(|cell| cell.set(index));
}

pub fn set_prop_current_cell_active(active: bool) {
    PROPERTY_CELL_ACTIVE.with(|cell| cell.set(active));
}

fn prop_current_cell() -> usize {
    PROPERTY_CELL.with(std::cell::Cell::get)
}

fn prop_current_cell_active() -> bool {
    PROPERTY_CELL_ACTIVE.with(std::cell::Cell::get)
}

fn v3(v: &acadrust::types::Vector3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

fn table_axes(table: &Table) -> (Vec3, Vec3) {
    let h = v3(&table.horizontal_direction).normalize_or(Vec3::X);
    let normal = v3(&table.normal).normalize_or(Vec3::Z);
    let down = h.cross(normal).normalize_or(Vec3::NEG_Y);
    (h, down)
}

fn merged_owner_and_span(
    table: &Table,
    row: usize,
    column: usize,
) -> Option<(usize, usize, usize, usize)> {
    for range in &table.merged_ranges {
        if range.contains(row, column) {
            return (row == range.top_row && column == range.left_col).then_some((
                range.top_row,
                range.left_col,
                range.bottom_row.min(table.rows.len().saturating_sub(1)),
                range.right_col.min(table.columns.len().saturating_sub(1)),
            ));
        }
    }

    for (owner_row, table_row) in table.rows.iter().enumerate() {
        for (owner_column, cell) in table_row.cells.iter().enumerate() {
            let row_end = owner_row
                .saturating_add(cell.merge_height.max(1) as usize - 1)
                .min(table.rows.len().saturating_sub(1));
            let column_end = owner_column
                .saturating_add(cell.merge_width.max(1) as usize - 1)
                .min(table.columns.len().saturating_sub(1));
            if row >= owner_row
                && row <= row_end
                && column >= owner_column
                && column <= column_end
            {
                return (row == owner_row && column == owner_column).then_some((
                    owner_row,
                    owner_column,
                    row_end,
                    column_end,
                ));
            }
        }
    }

    Some((row, column, row, column))
}

pub(crate) fn style_for_property<'a>(
    table: &'a Table,
    row: &'a acadrust::entities::table::TableRow,
    column: usize,
    cell: &'a acadrust::entities::table::TableCell,
    property: acadrust::entities::table::CellStylePropertyFlags,
) -> Option<&'a acadrust::entities::table::CellStyle> {
    let column_style = table
        .columns
        .get(column)
        .and_then(|column| column.style.as_ref());
    for style in [
        cell.style.as_ref(),
        row.style.as_ref(),
        column_style,
        table.base_style.as_ref(),
    ]
        .into_iter()
        .flatten()
    {
        if style.property_flags.contains(property) {
            return Some(style);
        }
    }
    None
}

fn style_for_border<'a>(
    table: &'a Table,
    row: &'a acadrust::entities::table::TableRow,
    column: usize,
    cell: &'a acadrust::entities::table::TableCell,
    edge: acadrust::entities::table::CellEdgeFlags,
) -> Option<&'a acadrust::entities::table::CellStyle> {
    let column_style = table
        .columns
        .get(column)
        .and_then(|column| column.style.as_ref());
    [
        cell.style.as_ref(),
        row.style.as_ref(),
        column_style,
        table.base_style.as_ref(),
    ]
    .into_iter()
    .flatten()
    .find(|style| style.applied_border_edges.contains(edge))
}

pub(crate) fn resolved_title_suppressed(
    table: &Table,
    table_style: Option<&acadrust::objects::TableStyle>,
) -> bool {
    table
        .legacy_style_override
        .as_ref()
        .and_then(|style| style.title_suppressed)
        .or_else(|| table_style.map(|style| style.title_suppressed))
        .unwrap_or(false)
}

pub(crate) fn resolved_header_suppressed(
    table: &Table,
    table_style: Option<&acadrust::objects::TableStyle>,
) -> bool {
    table
        .legacy_style_override
        .as_ref()
        .and_then(|style| style.header_suppressed)
        .or_else(|| table_style.map(|style| style.header_suppressed))
        .unwrap_or(false)
}

pub(crate) fn resolved_flow_up(
    table: &Table,
    table_style: Option<&acadrust::objects::TableStyle>,
) -> bool {
    use acadrust::entities::table::CellStylePropertyFlags;

    table
        .legacy_style_override
        .as_ref()
        .and_then(|style| style.flow_direction)
        .map(|flow| flow != 0)
        .or_else(|| {
            table.base_style.as_ref().and_then(|style| {
                style
                    .property_flags
                    .contains(CellStylePropertyFlags::FLOW_DIRECTION_BOTTOM_TO_TOP)
                    .then_some(true)
            })
        })
        .unwrap_or_else(|| {
            matches!(
                table_style.map(|style| style.flow_direction),
                Some(acadrust::objects::TableFlowDirection::Up)
            )
        })
}

pub(crate) fn resolved_table_margins(
    table: &Table,
    table_style: Option<&acadrust::objects::TableStyle>,
) -> (f64, f64) {
    let horizontal = table
        .legacy_style_override
        .as_ref()
        .and_then(|style| style.horizontal_cell_margin)
        .or_else(|| table_style.map(|style| style.horizontal_margin))
        .unwrap_or(0.0);
    let vertical = table
        .legacy_style_override
        .as_ref()
        .and_then(|style| style.vertical_cell_margin)
        .or_else(|| table_style.map(|style| style.vertical_margin))
        .unwrap_or(0.0);
    (horizontal, vertical)
}

fn table_offsets(table: &Table, scale: f32) -> (Vec<f32>, Vec<f32>) {
    let mut columns = Vec::with_capacity(table.columns.len() + 1);
    columns.push(0.0);
    for column in &table.columns {
        columns.push(columns.last().copied().unwrap_or(0.0) + column.width as f32 * scale);
    }

    let mut rows = Vec::with_capacity(table.rows.len() + 1);
    rows.push(0.0);
    for row in &table.rows {
        rows.push(rows.last().copied().unwrap_or(0.0) + row.height as f32 * scale);
    }
    (columns, rows)
}

#[derive(Clone, Copy)]
struct TableBreakSegment {
    start_row: usize,
    end_row: usize,
    origin: Vec3,
}

fn table_break_segments(
    table: &Table,
    h: Vec3,
    down: Vec3,
    row_offsets: &[f32],
    scale: f32,
) -> Vec<TableBreakSegment> {
    use acadrust::entities::table::BreakOptionFlags;

    let insertion = v3(&table.insertion_point);
    if table.rows.is_empty() {
        return Vec::new();
    }
    if !table.break_options.contains(BreakOptionFlags::ENABLE_BREAKS) {
        return vec![TableBreakSegment {
            start_row: 0,
            end_row: table.rows.len() - 1,
            origin: insertion,
        }];
    }

    let offset_to_world = |offset: &acadrust::types::Vector3| insertion + v3(offset);
    let mut cached: Vec<_> = table
        .break_ranges
        .iter()
        .filter_map(|range| {
            let start = range.start_row.max(0) as usize;
            let end = range.end_row.max(0) as usize;
            (start <= end && end < table.rows.len()).then(|| TableBreakSegment {
                start_row: start,
                end_row: end,
                origin: {
                    let position = offset_to_world(&range.position);
                    if position.is_finite() {
                        position
                    } else {
                        insertion
                    }
                },
            })
        })
        .collect();
    cached.sort_by_key(|segment| segment.start_row);
    let cached_complete = cached.first().is_some_and(|segment| segment.start_row == 0)
        && cached
            .last()
            .is_some_and(|segment| segment.end_row + 1 == table.rows.len())
        && cached
            .windows(2)
            .all(|pair| pair[0].end_row + 1 == pair[1].start_row);
    if cached_complete {
        return cached;
    }

    let mut segments = Vec::new();
    let mut start_row = 0usize;
    let mut segment = 0usize;
    while start_row < table.rows.len() {
        let manual_heights = table
            .break_options
            .contains(BreakOptionFlags::ALLOW_MANUAL_HEIGHTS);
        let max_height = table
            .break_data
            .get(if manual_heights { segment } else { 0 })
            .map(|data| data.height as f32 * scale)
            .filter(|height| *height > 1e-6)
            .unwrap_or(f32::INFINITY);
        let start_offset = row_offsets.get(start_row).copied().unwrap_or(0.0);
        let mut end_row = start_row;
        while end_row + 1 < table.rows.len()
            && row_offsets[end_row + 2] - start_offset <= max_height
        {
            end_row += 1;
        }
        let manual_positions = table
            .break_options
            .contains(BreakOptionFlags::ALLOW_MANUAL_POSITIONS);
        let manual_origin = manual_positions
            .then(|| table.break_data.get(segment))
            .flatten()
            .map(|data| offset_to_world(&data.position))
            .filter(|position| position.is_finite());
        let origin = manual_origin.unwrap_or_else(|| {
            let spacing = table.break_spacing as f32 * scale;
            let horizontal_step = table.total_width() as f32 * scale + spacing;
            let vertical_step = if max_height.is_finite() {
                max_height + spacing
            } else {
                table.total_height() as f32 * scale + spacing
            };
            match table.break_flow_direction {
                acadrust::entities::table::BreakFlowDirection::Left => {
                    insertion - h * segment as f32 * horizontal_step
                }
                acadrust::entities::table::BreakFlowDirection::Vertical => {
                    insertion + down * segment as f32 * vertical_step
                }
                acadrust::entities::table::BreakFlowDirection::Right => {
                    insertion + h * segment as f32 * horizontal_step
                }
            }
        });
        segments.push(TableBreakSegment {
            start_row,
            end_row,
            origin,
        });
        start_row = end_row.saturating_add(1);
        segment = segment.saturating_add(1);
    }

    segments
}

fn break_frame_for_row(
    table: &Table,
    row: usize,
    h: Vec3,
    down: Vec3,
    row_offsets: &[f32],
    scale: f32,
) -> (Vec3, f32) {
    let insertion = v3(&table.insertion_point);
    if let Some(segment) = table_break_segments(table, h, down, row_offsets, scale)
        .into_iter()
        .find(|segment| row >= segment.start_row && row <= segment.end_row)
    {
        return (
            segment.origin,
            row_offsets.get(row).copied().unwrap_or(0.0)
                - row_offsets
                    .get(segment.start_row)
                    .copied()
                    .unwrap_or(0.0),
        );
    }

    (insertion, row_offsets.get(row).copied().unwrap_or(0.0))
}

fn break_frames_for_row(
    table: &Table,
    row: usize,
    h: Vec3,
    down: Vec3,
    row_offsets: &[f32],
    scale: f32,
    top_label_rows: usize,
    bottom_label_rows: usize,
) -> Vec<(Vec3, f32)> {
    use acadrust::entities::table::BreakOptionFlags;

    let segments = table_break_segments(table, h, down, row_offsets, scale);
    let Some((segment_index, segment)) = segments
        .iter()
        .enumerate()
        .find(|(_, segment)| row >= segment.start_row && row <= segment.end_row)
    else {
        return vec![break_frame_for_row(table, row, h, down, row_offsets, scale)];
    };
    let repeat_top = table
        .break_options
        .contains(BreakOptionFlags::REPEAT_TOP_LABELS)
        && top_label_rows > 0;
    let repeat_bottom = table
        .break_options
        .contains(BreakOptionFlags::REPEAT_BOTTOM_LABELS)
        && bottom_label_rows > 0;
    let top_label_rows = top_label_rows.min(table.rows.len());
    let bottom_label_rows = bottom_label_rows.min(table.rows.len());
    let bottom_start = table.rows.len().saturating_sub(bottom_label_rows);
    let top_height = row_offsets
        .get(top_label_rows)
        .copied()
        .unwrap_or(0.0);
    let mut primary_top = row_offsets.get(row).copied().unwrap_or(0.0)
        - row_offsets
            .get(segment.start_row)
            .copied()
            .unwrap_or(0.0);
    if repeat_top && segment_index > 0 {
        primary_top += top_height;
    }
    let mut frames = vec![(segment.origin, primary_top)];

    if repeat_top && row < top_label_rows {
        for repeated_segment in segments.iter().skip(1) {
            frames.push((
                repeated_segment.origin,
                row_offsets.get(row).copied().unwrap_or(0.0),
            ));
        }
    }
    if repeat_bottom && row >= bottom_start {
        let label_offset = row_offsets.get(row).copied().unwrap_or(0.0)
            - row_offsets.get(bottom_start).copied().unwrap_or(0.0);
        for (index, repeated_segment) in segments
            .iter()
            .enumerate()
            .take(segments.len().saturating_sub(1))
        {
            let content_height = row_offsets
                .get(repeated_segment.end_row + 1)
                .copied()
                .unwrap_or(0.0)
                - row_offsets
                    .get(repeated_segment.start_row)
                    .copied()
                    .unwrap_or(0.0);
            let repeated_top = content_height
                + if repeat_top && index > 0 {
                    top_height
                } else {
                    0.0
                }
                + label_offset;
            frames.push((repeated_segment.origin, repeated_top));
        }
    }
    frames
}

fn format_cell_value(value: &acadrust::entities::table::CellValue) -> String {
    let display = value.display();
    if !display.is_empty() {
        return display.to_string();
    }
    use acadrust::entities::table::CellValueType;
    match value.value_type {
        CellValueType::Long => format!("{}", value.numeric_value as i64),
        CellValueType::Double | CellValueType::Date => format!("{}", value.numeric_value),
        CellValueType::Point2D => {
            format!("{}, {}", value.point_value.x, value.point_value.y)
        }
        CellValueType::Point3D => format!(
            "{}, {}, {}",
            value.point_value.x, value.point_value.y, value.point_value.z
        ),
        CellValueType::Handle => value
            .handle_value
            .map(|handle| format!("{:X}", handle.value()))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn table_cell_reference(reference: &str) -> Option<(usize, usize)> {
    let reference = reference.trim();
    let split = reference
        .char_indices()
        .find(|(_, ch)| ch.is_ascii_digit())?
        .0;
    let (letters, digits) = reference.split_at(split);
    if letters.is_empty() || digits.is_empty() {
        return None;
    }
    let mut column = 0usize;
    for ch in letters.chars() {
        if !ch.is_ascii_alphabetic() {
            return None;
        }
        column = column
            .checked_mul(26)?
            .checked_add(ch.to_ascii_uppercase() as usize - 'A' as usize + 1)?;
    }
    let row = digits.parse::<usize>().ok()?.checked_sub(1)?;
    Some((row, column.checked_sub(1)?))
}

fn evaluate_table_formula(table: &Table, expression: &str) -> Option<String> {
    let body = expression.trim().strip_prefix('=')?.trim();
    if let Some((row, column)) = table_cell_reference(body) {
        return table.cell(row, column).map(|cell| cell.text_value().to_string());
    }
    let open = body.find('(')?;
    let close = body.rfind(')')?;
    let function = body[..open].trim().to_ascii_uppercase();
    let range = &body[open + 1..close];
    let (first, last) = range.split_once(':').unwrap_or((range, range));
    let (r1, c1) = table_cell_reference(first)?;
    let (r2, c2) = table_cell_reference(last)?;
    let mut values = Vec::new();
    for row in r1.min(r2)..=r1.max(r2) {
        for column in c1.min(c2)..=c1.max(c2) {
            if let Some(value) = table
                .cell(row, column)
                .and_then(|cell| cell.text_value().trim().parse::<f64>().ok())
            {
                values.push(value);
            }
        }
    }
    match function.as_str() {
        "SUM" => Some(values.iter().sum::<f64>().to_string()),
        "AVERAGE" | "AVG" if !values.is_empty() => {
            Some((values.iter().sum::<f64>() / values.len() as f64).to_string())
        }
        "COUNT" => Some(values.len().to_string()),
        "MIN" if !values.is_empty() => {
            Some(values.into_iter().fold(f64::INFINITY, f64::min).to_string())
        }
        "MAX" if !values.is_empty() => Some(
            values
                .into_iter()
                .fold(f64::NEG_INFINITY, f64::max)
                .to_string(),
        ),
        _ => None,
    }
}

fn fallback_content_centers(
    bounds: [f32; 4],
    sizes: &[(f32, f32)],
    layout: acadrust::entities::table::ContentLayoutFlags,
    alignment: i32,
    horizontal_spacing: f32,
    vertical_spacing: f32,
) -> Vec<(f32, f32)> {
    if sizes.is_empty() {
        return Vec::new();
    }
    let [left, top, right, bottom] = bounds;
    let inner_width = (right - left).max(0.0);
    let horiz = ((alignment - 1).rem_euclid(3)) + 1;
    let vert = ((alignment - 1) / 3) + 1;
    let mut rows: Vec<Vec<usize>> = Vec::new();
    if layout.contains(
        acadrust::entities::table::ContentLayoutFlags::STACKED_VERTICAL,
    ) {
        for index in 0..sizes.len() {
            rows.push(vec![index]);
        }
    } else if layout.contains(
        acadrust::entities::table::ContentLayoutFlags::STACKED_HORIZONTAL,
    ) {
        rows.push((0..sizes.len()).collect());
    } else {
        let mut row = Vec::new();
        let mut width = 0.0f32;
        for (index, (item_width, _)) in sizes.iter().copied().enumerate() {
            let next = if row.is_empty() {
                item_width
            } else {
                width + horizontal_spacing + item_width
            };
            if !row.is_empty() && next > inner_width {
                rows.push(std::mem::take(&mut row));
                width = item_width;
            } else {
                width = next;
            }
            row.push(index);
        }
        if !row.is_empty() {
            rows.push(row);
        }
    }

    let row_heights: Vec<f32> = rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|&index| sizes[index].1)
                .fold(0.0f32, f32::max)
        })
        .collect();
    let total_height = row_heights.iter().sum::<f32>()
        + vertical_spacing * rows.len().saturating_sub(1) as f32;
    let mut y = match vert {
        1 => top,
        3 => bottom - total_height,
        _ => (top + bottom - total_height) * 0.5,
    };
    let mut result = vec![(0.0f32, 0.0f32); sizes.len()];
    for (row_index, row) in rows.iter().enumerate() {
        let row_width = row.iter().map(|&index| sizes[index].0).sum::<f32>()
            + horizontal_spacing * row.len().saturating_sub(1) as f32;
        let mut x = match horiz {
            1 => left,
            3 => right - row_width,
            _ => (left + right - row_width) * 0.5,
        };
        for &index in row {
            let (width, height) = sizes[index];
            result[index] =
                (x + width * 0.5, y + (row_heights[row_index] - height) * 0.5 + height * 0.5);
            x += width + horizontal_spacing;
        }
        y += row_heights[row_index] + vertical_spacing;
    }
    result
}

fn content_display_value(
    document: &acadrust::CadDocument,
    table: &Table,
    content: &acadrust::entities::table::CellContent,
) -> String {
    if let Some(handle) = content.field_handle {
        if let Some(value) =
            crate::entities::field::resolve_handle(document, handle, table.common.handle)
        {
            return value;
        }
        if let Some(acadrust::objects::ObjectType::Field(field)) = document.objects.get(&handle) {
            if !field.value_string.is_empty() {
                return field.value_string.clone();
            }
            let value = format_cell_value(&field.value);
            if !value.is_empty() {
                return value;
            }
        }
    }
    let value = format_cell_value(&content.value);
    evaluate_table_formula(table, &value).unwrap_or(value)
}

fn resolved_content_geometry(
    document: &acadrust::CadDocument,
    table: &Table,
    row: usize,
    column: usize,
    cell: &acadrust::entities::table::TableCell,
    content_index: usize,
) -> Option<acadrust::entities::table::CellContentGeometry> {
    if let Some(geometry) = cell
        .contents
        .get(content_index)
        .and_then(|content| content.geometry.clone())
        .or_else(|| cell.geometries.get(content_index).cloned())
        .or_else(|| (content_index == 0).then(|| cell.geometry.clone()).flatten())
    {
        return Some(geometry);
    }

    let handle = cell.geometry_handle?;
    let flat_index = row
        .saturating_mul(table.columns.len())
        .saturating_add(column);
    if let Some(acadrust::objects::ObjectType::DataObject(object)) =
        document.objects.get(&handle)
    {
        if let acadrust::objects::DataObjectData::TableGeometry(geometry) =
            &object.data
        {
            return geometry
                .cells
                .get(flat_index)
                .and_then(|cell| cell.geometry.get(content_index))
                .cloned();
        }
    }
    document.objects.values().find_map(|object| {
        let acadrust::objects::ObjectType::DataObject(object) = object else {
            return None;
        };
        let acadrust::objects::DataObjectData::TableGeometry(geometry) =
            &object.data
        else {
            return None;
        };
        geometry
            .cells
            .iter()
            .find(|geometry_cell| geometry_cell.table_geometry == handle)
            .and_then(|geometry_cell| geometry_cell.geometry.get(content_index))
            .cloned()
    })
}

pub(crate) fn block_cell_inserts(
    table: &Table,
    document: &acadrust::CadDocument,
    anno_scale: f32,
) -> Vec<acadrust::entities::Insert> {
    use acadrust::entities::table::ContentLayoutFlags;
    use acadrust::entities::{AttributeEntity, EntityType, Insert};
    use acadrust::types::Vector3;

    if table.rows.is_empty() || table.columns.is_empty() {
        return Vec::new();
    }
    let (h, down) = table_axes(table);
    let table_style = table.table_style_handle.and_then(|handle| {
        document.objects.get(&handle).and_then(|object| match object {
            acadrust::objects::ObjectType::TableStyle(style) => Some(style),
            _ => None,
        })
    });
    let flow = if resolved_flow_up(table, table_style) {
        -down
    } else {
        down
    };
    let (column_offsets, row_offsets) = table_offsets(table, anno_scale);
    let table_rotation = table
        .horizontal_direction
        .y
        .atan2(table.horizontal_direction.x);
    let mut inserts = Vec::new();

    for (row_index, row) in table.rows.iter().enumerate() {
        for (column_index, cell) in row.cells.iter().enumerate() {
            let Some((_, _, row_end, column_end)) =
                merged_owner_and_span(table, row_index, column_index)
            else {
                continue;
            };
            let block_contents: Vec<_> = cell
                .contents
                .iter()
                .enumerate()
                .filter(|(_, content)| content.block_handle.is_some())
                .collect();
            if block_contents.is_empty() {
                continue;
            }
            let (origin, row_top) = break_frame_for_row(
                table,
                row_index,
                h,
                flow,
                &row_offsets,
                anno_scale,
            );
            let row_bottom = row_top
                + row_offsets
                    .get(row_end + 1)
                    .copied()
                    .unwrap_or(row_offsets[row_index])
                - row_offsets[row_index];
            let column_left = column_offsets[column_index];
            let column_right = column_offsets
                .get(column_end + 1)
                .copied()
                .unwrap_or(column_left);
            let layout_style = style_for_property(
                table,
                row,
                column_index,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::CONTENT_LAYOUT,
            );
            let layout = layout_style
                .map(|style| style.layout_flags)
                .unwrap_or(ContentLayoutFlags::FLOW);
            let count = block_contents.len() as f32;

            for (slot_index, (content_index, content)) in
                block_contents.into_iter().enumerate()
            {
                let Some(block_handle) = content.block_handle else {
                    continue;
                };
                let Some(record) = document
                    .block_records
                    .iter()
                    .find(|record| record.handle == block_handle)
                else {
                    continue;
                };
                let mut x = (column_left + column_right) * 0.5;
                let mut y = (row_top + row_bottom) * 0.5;
                let mut z = 0.0f32;
                if let Some(geometry) = resolved_content_geometry(
                    document,
                    table,
                    row_index,
                    column_index,
                    cell,
                    content_index,
                ) {
                    x = column_left
                        + geometry.distance_to_center.x as f32 * anno_scale;
                    y = row_top
                        - geometry.distance_to_center.y as f32 * anno_scale;
                    z = geometry.distance_to_center.z as f32 * anno_scale;
                } else if count > 1.0 {
                    let index = slot_index as f32;
                    if layout.contains(ContentLayoutFlags::STACKED_VERTICAL) {
                        y = row_top + (index + 0.5) * (row_bottom - row_top) / count;
                    } else {
                        x = column_left + (index + 0.5) * (column_right - column_left) / count;
                    }
                }
                let position =
                    origin + h * x + flow * y + v3(&table.normal).normalize_or(Vec3::Z) * z;
                let scale_style = style_for_property(
                    table,
                    row,
                    column_index,
                    cell,
                    acadrust::entities::table::CellStylePropertyFlags::BLOCK_SCALE,
                );
                let style_scale = scale_style
                    .map(|style| style.scale)
                    .filter(|scale| scale.abs() > 1e-9)
                    .unwrap_or(1.0);
                let content_scale = if content.format_property_flags
                    & acadrust::entities::table::CellStylePropertyFlags::BLOCK_SCALE.bits() as i32
                    != 0
                    && content.scale.abs() > 1e-9
                {
                    content.scale
                } else if cell.block_scale.abs() > 1e-9 {
                    cell.block_scale
                } else {
                    1.0
                };
                let mut scale = if content.format_property_flags
                    & acadrust::entities::table::CellStylePropertyFlags::BLOCK_SCALE.bits() as i32
                    != 0
                {
                    content_scale
                } else {
                    style_scale.max(content_scale)
                } * anno_scale as f64;
                let auto_scale = cell.auto_fit
                    || style_for_property(
                        table,
                        row,
                        column_index,
                        cell,
                        acadrust::entities::table::CellStylePropertyFlags::AUTO_SCALE,
                    )
                    .is_some_and(|style| {
                        style
                            .property_flags
                            .contains(acadrust::entities::table::CellStylePropertyFlags::AUTO_SCALE)
                    });
                let mut block_min =
                    Vector3::new(f64::MAX, f64::MAX, f64::MAX);
                let mut block_max =
                    Vector3::new(f64::MIN, f64::MIN, f64::MIN);
                let mut has_block_bounds = false;
                for &handle in &record.entity_handles {
                    let Some(entity) = document.get_entity(handle) else {
                        continue;
                    };
                    let bounds = entity.as_entity().bounding_box();
                    if bounds.min.x.is_finite()
                        && bounds.min.y.is_finite()
                        && bounds.max.x.is_finite()
                        && bounds.max.y.is_finite()
                    {
                        block_min.x = block_min.x.min(bounds.min.x);
                        block_min.y = block_min.y.min(bounds.min.y);
                        block_min.z = block_min.z.min(bounds.min.z);
                        block_max.x = block_max.x.max(bounds.max.x);
                        block_max.y = block_max.y.max(bounds.max.y);
                        block_max.z = block_max.z.max(bounds.max.z);
                        has_block_bounds = true;
                    }
                }
                if auto_scale && has_block_bounds {
                        let min = block_min;
                        let max = block_max;
                        let width = (max.x - min.x).abs();
                        let height = (max.y - min.y).abs();
                        let margin_left = style_for_property(
                            table,
                            row,
                            column_index,
                            cell,
                            acadrust::entities::table::CellStylePropertyFlags::MARGIN_LEFT,
                        )
                        .map(|style| style.margin_left)
                        .unwrap_or(0.0);
                        let margin_right = style_for_property(
                            table,
                            row,
                            column_index,
                            cell,
                            acadrust::entities::table::CellStylePropertyFlags::MARGIN_RIGHT,
                        )
                        .map(|style| style.margin_right)
                        .unwrap_or(0.0);
                        let margin_top = style_for_property(
                            table,
                            row,
                            column_index,
                            cell,
                            acadrust::entities::table::CellStylePropertyFlags::MARGIN_TOP,
                        )
                        .map(|style| style.margin_top)
                        .unwrap_or(0.0);
                        let margin_bottom = style_for_property(
                            table,
                            row,
                            column_index,
                            cell,
                            acadrust::entities::table::CellStylePropertyFlags::MARGIN_BOTTOM,
                        )
                        .map(|style| style.margin_bottom)
                        .unwrap_or(0.0);
                        let fit_x = ((column_right - column_left) as f64
                            - (margin_left + margin_right) * anno_scale as f64)
                            .max(0.0)
                            / width.max(1e-9);
                        let fit_y = ((row_bottom - row_top) as f64
                            - (margin_top + margin_bottom) * anno_scale as f64)
                            .max(0.0)
                            / height.max(1e-9);
                        scale *= fit_x.min(fit_y);
                }
                let rotation_style = style_for_property(
                    table,
                    row,
                    column_index,
                    cell,
                    acadrust::entities::table::CellStylePropertyFlags::ROTATION,
                );
                let content_rotation_explicit = content.format_property_flags
                    & acadrust::entities::table::CellStylePropertyFlags::ROTATION.bits() as i32
                    != 0;
                let content_rotation = if content_rotation_explicit {
                    content.rotation
                } else {
                    rotation_style
                        .map(|style| style.rotation)
                        .unwrap_or(cell.rotation)
                };
                let rotation = table_rotation + content_rotation;
                let mut insert = Insert::new(
                    record.name.clone(),
                    Vector3::new(position.x as f64, position.y as f64, position.z as f64),
                );
                insert.common = table.common.clone();
                insert.normal = table.normal;
                insert.rotation = rotation;
                insert.set_x_scale(scale);
                insert.set_y_scale(scale);
                insert.set_z_scale(scale);
                let transform = insert.get_transform();
                let local_anchor = if has_block_bounds {
                    (block_min + block_max) * 0.5
                } else {
                    record.base_point
                };
                let transformed_base = transform.apply(local_anchor);
                let transformed_zero = transform.apply(Vector3::ZERO);
                insert.insert_point = insert.insert_point
                    - (transformed_base - transformed_zero);
                let attribute_definitions: Vec<_> = record
                    .entity_handles
                    .iter()
                    .filter_map(|handle| match document.get_entity(*handle) {
                        Some(EntityType::AttributeDefinition(definition)) => {
                            Some(definition)
                        }
                        _ => None,
                    })
                    .collect();
                for attribute in &content.attributes {
                    let definition = match document
                        .get_entity(attribute.definition_handle)
                    {
                        Some(EntityType::AttributeDefinition(definition)) => {
                            Some(definition)
                        }
                        _ => attribute_definitions
                            .get(attribute.index.max(0) as usize)
                            .copied(),
                    };
                    let Some(definition) = definition else {
                        continue;
                    };
                    let mut entity =
                        AttributeEntity::from_definition(definition, Some(attribute.value.clone()));
                    acadrust::Entity::apply_transform(
                        &mut entity,
                        &insert.get_transform(),
                    );
                    entity.common.handle = table.common.handle;
                    insert.attributes.push(entity);
                }
                inserts.push(insert);
            }
        }
    }
    inserts
}

impl RenderConvertible for Table {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        if self.rows.is_empty() || self.columns.is_empty() {
            return None;
        }

        // Lay the table out in a local frame with the origin at zero. The world
        // insertion point is added back as f64 at the widening step below, so
        // large coordinates (UTM etc.) keep full precision instead of snapping
        // onto the coarse f32 grid — which would collide cell-text baselines and
        // overflow the integer border-dedup keys.
        let base = [
            self.insertion_point.x,
            self.insertion_point.y,
            self.insertion_point.z,
        ];
        let origin = Vec3::ZERO;
        let (h, v_down) = table_axes(self);

        let col_offsets: Vec<f32> = {
            let mut off = 0.0f32;
            let mut v = vec![0.0f32];
            for col in &self.columns {
                off += col.width as f32;
                v.push(off);
            }
            v
        };
        let total_w = *col_offsets.last().unwrap_or(&0.0);

        let row_offsets: Vec<f32> = {
            let mut off = 0.0f32;
            let mut v = vec![0.0f32];
            for row in &self.rows {
                off += row.height as f32;
                v.push(off);
            }
            v
        };
        let total_h = *row_offsets.last().unwrap_or(&0.0);

        let mut pts: Vec<[f32; 3]> = Vec::new();
        let mut tris_pts: Vec<[f32; 3]> = Vec::new();

        // Per-cell borders. When a cell carries a CellStyle, honour the
        // visibility / `invisible` flag of each of its four borders so
        // hidden borders disappear from the grid. Cells with no style still
        // emit the standard four borders. To avoid drawing each shared edge
        // twice we coalesce the segments by their (start, end) coordinates.
        use rustc_hash::FxHashSet as HashSet;
        let mut emitted: HashSet<(i32, i32, i32, i32)> = HashSet::default();
        let try_add = |a: Vec3,
                       b: Vec3,
                       vis: bool,
                       emitted: &mut HashSet<(i32, i32, i32, i32)>,
                       pts: &mut Vec<[f32; 3]>| {
            if !vis {
                return;
            }
            let key = (
                (a.x * 1_000.0) as i32,
                (a.y * 1_000.0) as i32,
                (b.x * 1_000.0) as i32,
                (b.y * 1_000.0) as i32,
            );
            let key_rev = (key.2, key.3, key.0, key.1);
            if emitted.contains(&key) || emitted.contains(&key_rev) {
                return;
            }
            emitted.insert(key);
            if !pts.is_empty() {
                pts.push([f32::NAN; 3]);
            }
            pts.push([a.x, a.y, a.z]);
            pts.push([b.x, b.y, b.z]);
        };
        for (ri, row) in self.rows.iter().enumerate() {
            let row_top = row_offsets[ri];
            let row_bot = row_offsets
                .get(ri + 1)
                .copied()
                .unwrap_or(row_top + row.height as f32);
            for (ci, cell) in row.cells.iter().enumerate() {
                let col_left = col_offsets[ci];
                let col_right = col_offsets.get(ci + 1).copied().unwrap_or(
                    col_left + self.columns.get(ci).map(|c| c.width as f32).unwrap_or(1.0),
                );
                // Default to visible when no style override is present.
                let (top_vis, right_vis, bottom_vis, left_vis) = cell
                    .style
                    .as_ref()
                    .map(|s| {
                        (
                            !s.top_border.invisible,
                            !s.right_border.invisible,
                            !s.bottom_border.invisible,
                            !s.left_border.invisible,
                        )
                    })
                    .unwrap_or((true, true, true, true));
                let tl = origin + h * col_left + v_down * row_top;
                let tr = origin + h * col_right + v_down * row_top;
                let br_ = origin + h * col_right + v_down * row_bot;
                let bl = origin + h * col_left + v_down * row_bot;
                try_add(tl, tr, top_vis, &mut emitted, &mut pts);
                try_add(tr, br_, right_vis, &mut emitted, &mut pts);
                try_add(bl, br_, bottom_vis, &mut emitted, &mut pts);
                try_add(tl, bl, left_vis, &mut emitted, &mut pts);
            }
        }
        // Suppress unused-variable warnings now that the simple grid-pass
        // is gone — col/row offsets still feed cell drawing below.
        let _ = (total_w, total_h);

        // Cell text — resolve defaults via TableStyle, then layer per-cell
        // overrides on top. Resolution order (text height, text style, alignment):
        //   1. CellContent.* (per-content explicit override)
        //   2. CellStyle.*   (per-cell explicit override)
        //   3. TableStyle.<row_kind>_row_style.* (table-wide default for this row class)
        //   4. compiled-in fallback (0.18 / "txt" / MiddleCenter)
        //
        // Row classification: row 0 is Title (when not suppressed), row 1 is
        // Header (when not suppressed), everything else is Data. The two
        // suppressed flags shift the leading rows down to Data.
        let lookup_style = |h: acadrust::Handle| -> Option<&acadrust::tables::TextStyle> {
            document.text_styles.iter().find(|s| s.handle == h)
        };
        let table_style: Option<&acadrust::objects::TableStyle> =
            self.table_style_handle.and_then(|h| {
                document.objects.get(&h).and_then(|obj| match obj {
                    acadrust::objects::ObjectType::TableStyle(ts) => Some(ts),
                    _ => None,
                })
            });
        let title_suppressed = resolved_title_suppressed(self, table_style);
        let header_suppressed = resolved_header_suppressed(self, table_style);

        let font_for_handle = |handle: Option<acadrust::Handle>| -> Option<String> {
            handle.and_then(|h| lookup_style(h)).and_then(|s| {
                let mut font_name = if !s.true_type_font.trim().is_empty() {
                    s.true_type_font.trim().to_string()
                } else {
                    let file = s.font_file.trim();
                    if !file.is_empty() {
                        let basename = file.rsplit(['/', '\\']).next().unwrap_or(file);
                        let stem = basename.split('.').next().unwrap_or(basename).trim();
                        if !stem.is_empty() {
                            stem.to_string()
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                };
                if !crate::scene::text::lff::is_builtin(&font_name) {
                    if let Some(canonical) = crate::scene::text::sysfont::canonical_family_name(&font_name) {
                        font_name = canonical;
                    }
                }
                Some(font_name)
            })
        };
        // Build a ResolvedTextStyle for the cell — needed by the shared MText
        // pipeline so inline `\W`, `\Q`, etc. compose with the style baseline.
        let resolved_style_for_handle =
            |handle: Option<acadrust::Handle>, font_name: String| -> ResolvedTextStyle {
                let style = handle.and_then(|h| lookup_style(h));
                ResolvedTextStyle {
                    font_name,
                    width_factor: style.map(|s| s.width_factor as f32).unwrap_or(1.0),
                    oblique_angle: style.map(|s| s.oblique_angle as f32).unwrap_or(0.0),
                    is_backward: style.map(|s| s.is_backward()).unwrap_or(false),
                    is_upside_down: style.map(|s| s.is_upside_down()).unwrap_or(false),
                    is_vertical: style.map(|s| s.is_vertical).unwrap_or(false),
                }
            };

        for (ri, row) in self.rows.iter().enumerate() {
            let row_top = row_offsets[ri];
            let row_bot = row_offsets
                .get(ri + 1)
                .copied()
                .unwrap_or(row_top + row.height as f32);
            let row_mid = (row_top + row_bot) * 0.5;

            // Pick the appropriate row_style from TableStyle for this row's role.
            let row_style: Option<&acadrust::objects::RowCellStyle> = table_style.map(|ts| {
                let kind = match (title_suppressed, header_suppressed, ri) {
                    (false, _, 0) => 0,     // title
                    (false, false, 1) => 1, // header
                    (true, false, 0) => 1,  // header pulled up
                    _ => 2,                 // data
                };
                match kind {
                    0 => &ts.title_row_style,
                    1 => &ts.header_row_style,
                    _ => &ts.data_row_style,
                }
            });

            for (ci, cell) in row.cells.iter().enumerate() {
                let text = cell.text_value();
                if text.is_empty() {
                    continue;
                }

                let col_left = col_offsets[ci];
                let col_width = self.columns.get(ci).map(|c| c.width as f32).unwrap_or(1.0);
                let col_right = col_left + col_width;

                // Resolve text height: content → cell-style → row-style → 0.18.
                let content = cell.contents.first();
                let cell_h = content
                    .map(|c| c.text_height)
                    .filter(|h| *h > 1e-6)
                    .or_else(|| {
                        cell.style
                            .as_ref()
                            .map(|s| s.text_height)
                            .filter(|h| *h > 1e-6)
                    })
                    .or_else(|| row_style.map(|s| s.text_height).filter(|h| *h > 1e-6))
                    .map(|h| h as f32)
                    .unwrap_or(0.18);
                let margin = cell_h * 0.5_f32;

                // Resolve text-style handle: content → cell-style → row-style.
                let style_handle = content
                    .and_then(|c| c.text_style_handle)
                    .or_else(|| cell.style.as_ref().and_then(|s| s.text_style_handle))
                    .or_else(|| row_style.and_then(|s| s.text_style_handle));
                let font_owned = font_for_handle(style_handle).unwrap_or_else(|| "txt".to_string());
                let resolved = resolved_style_for_handle(style_handle, font_owned);

                // Alignment resolution: cell.style.alignment (1-9) overrides;
                // otherwise fall back to row_style.alignment, then MiddleCenter.
                let align = cell
                    .style
                    .as_ref()
                    .map(|s| s.alignment)
                    .filter(|a| *a != 0)
                    .or_else(|| row_style.map(|s| s.alignment as i32))
                    .unwrap_or(5);
                let horiz = ((align - 1).rem_euclid(3)) + 1; // 1=left, 2=center, 3=right
                let vert = ((align - 1) / 3) + 1; // 1=top, 2=middle, 3=bottom

                // Position the cell's MText block anchor at the requested
                // alignment corner / midpoint of the cell's content area.
                let (x_offset, attach_h_anchor) = match horiz {
                    1 => (col_left + margin, 0.0_f32),
                    3 => (col_right - margin, 1.0_f32),
                    _ => (col_left + col_width * 0.5, 0.5_f32),
                };
                let (y_offset, v_anchor) = match vert {
                    1 => (row_top + margin, MTextVAnchor::Top),
                    3 => (row_bot - margin, MTextVAnchor::Bottom),
                    _ => (row_mid, MTextVAnchor::Middle),
                };
                let text_origin = origin + h * x_offset + v_down * y_offset;

                // Content rotation (radians) on top of table cell rotation.
                let rot = content.map(|c| c.rotation as f32).unwrap_or(0.0) + cell.rotation as f32;
                let layout = layout_mtext(&MTextRenderOpts {
                    // Not an MTEXT: text in a fixed box, never columnar.
                    columns: Default::default(),
                    value: text,
                    insertion: [text_origin.x as f64, text_origin.y as f64, origin.z as f64],
                    height: cell_h,
                    rect_w: 0.0,
                    rotation: rot,
                    style: &resolved,
                    attach_h_anchor,
                    v_anchor,
                    line_spacing_factor: 1.0,
                    exact_line_spacing: false,
                    rectangle_height: 0.0,
                    vertical_text: false,
                    want_glyph_boxes: false,
                });
                // Flatten TextStroke groups into the table's Lines buffer.
                // Per-run inline `\C` / `\c` colour is dropped here because the
                // table emits a single RenderObject::Lines for borders + text;
                // tracking it would require splitting the table into multiple
                // WireModels per cell colour. Borders + uniform-coloured runs
                // honour the entity's outer colour.
                for ts in &layout.strokes {
                    let ox = ts.origin[0] as f32;
                    let oy = ts.origin[1] as f32;
                    for stroke in &ts.strokes {
                        if stroke.len() < 2 {
                            continue;
                        }
                        if !pts.is_empty() {
                            pts.push([f32::NAN; 3]);
                        }
                        for &[x, y] in stroke {
                            pts.push([x + ox, y + oy, origin.z]);
                        }
                    }
                    for &[x, y] in &ts.fill_tris {
                        tris_pts.push([x + ox, y + oy, origin.z]);
                    }
                }
            }
        }

        // The layout above is in a local f32 frame (small magnitudes). Widen to
        // f64 and add the world insertion so the absolute position carries full
        // precision; tessellate.rs then applies world_offset.
        let pts_f64: Vec<[f64; 3]> = pts
            .into_iter()
            .map(|[x, y, z]| {
                if x.is_nan() {
                    [f64::NAN, f64::NAN, f64::NAN]
                } else {
                    [x as f64 + base[0], y as f64 + base[1], z as f64 + base[2]]
                }
            })
            .collect();
        let fill_tris_f64: Vec<[f64; 3]> = tris_pts
            .into_iter()
            .map(|[x, y, z]| {
                [x as f64 + base[0], y as f64 + base[1], z as f64 + base[2]]
            })
            .collect();
        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(pts_f64),
            snap_pts: vec![(glam::DVec3::new(self.insertion_point.x, self.insertion_point.y, self.insertion_point.z), SnapHint::Insertion)],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: fill_tris_f64,
        })
    }
}

/// Builds colored geometry for tables without a stored display block.
pub fn tessellate_table(
    tab: &Table,
    document: &acadrust::CadDocument,
    selected: bool,
    entity_color: [f32; 4],
    line_weight_px: f32,
    // Annotation scale: multiplies the table's paper-size geometry so an
    // annotative table renders at the current annotation scale. 1.0 for a
    // non-annotative table (its geometry is already at model size).
    anno_scale: f32,
) -> Vec<crate::scene::model::wire_model::WireModel> {
    use crate::scene::convert::tess_util::aci_to_rgba;
    use crate::scene::model::wire_model::WireModel;
    use acadrust::types::Color;
    use rustc_hash::FxHashMap as HashMap;

    if tab.rows.is_empty() || tab.columns.is_empty() {
        return Vec::new();
    }

    let rel = |p: Vec3| -> [f32; 3] {
        [
            (p.x as f64) as f32,
            (p.y as f64) as f32,
            (p.z as f64) as f32,
        ]
    };
    let resolve_col = |c: &Color, fallback: [f32; 4]| -> [f32; 4] {
        match c {
            Color::ByLayer | Color::ByBlock => fallback,
            _ => aci_to_rgba(c),
        }
    };
    let key4 = |c: [f32; 4]| -> [u8; 4] {
        [
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
            (c[3] * 255.0) as u8,
        ]
    };
    let lw_px = |w: &acadrust::types::LineWeight| -> f32 {
        match w {
            acadrust::types::LineWeight::Value(v) if *v >= 0 => (*v as f32 / 100.0) * (96.0 / 25.4),
            _ => line_weight_px,
        }
    };

    let (h, v_down) = table_axes(tab);
    // Flow direction: `Up` stacks rows upward instead of downward.
    let table_style: Option<&acadrust::objects::TableStyle> =
        tab.table_style_handle.and_then(|h| {
            document.objects.get(&h).and_then(|obj| match obj {
                acadrust::objects::ObjectType::TableStyle(ts) => Some(ts),
                _ => None,
            })
        });
    let flow_up = resolved_flow_up(tab, table_style);
    let v_flow = if flow_up { -v_down } else { v_down };

    let (col_offsets, row_offsets) = table_offsets(tab, anno_scale);

    let title_suppressed = resolved_title_suppressed(tab, table_style);
    let header_suppressed = resolved_header_suppressed(tab, table_style);
    let top_label_rows = (usize::from(!title_suppressed) + usize::from(!header_suppressed))
        .min(tab.rows.len());
    let bottom_label_rows = usize::from(!tab.rows.is_empty());
    let (horizontal_margin, vertical_margin) = resolved_table_margins(tab, table_style);
    let h_margin = horizontal_margin as f32 * anno_scale;
    let v_margin = vertical_margin as f32 * anno_scale;

    let lookup_style = |hh: acadrust::Handle| -> Option<&acadrust::tables::TextStyle> {
        document.text_styles.iter().find(|s| s.handle == hh)
    };
    let font_for_handle = |handle: Option<acadrust::Handle>| -> Option<String> {
        handle.and_then(lookup_style).and_then(|s| {
            let mut font_name = if !s.true_type_font.trim().is_empty() {
                s.true_type_font.trim().to_string()
            } else {
                let file = s.font_file.trim();
                let basename = file.rsplit(['/', '\\']).next().unwrap_or(file);
                let stem = basename.split('.').next().unwrap_or(basename).trim();
                if !stem.is_empty() {
                    stem.to_string()
                } else {
                    return None;
                }
            };
            if !crate::scene::text::lff::is_builtin(&font_name) {
                if let Some(canonical) = crate::scene::text::sysfont::canonical_family_name(&font_name) {
                    font_name = canonical;
                }
            }
            Some(font_name)
        })
    };
    let resolved_style_for_handle =
        |handle: Option<acadrust::Handle>, font_name: String| -> ResolvedTextStyle {
            let style = handle.and_then(lookup_style);
            ResolvedTextStyle {
                font_name,
                width_factor: style.map(|s| s.width_factor as f32).unwrap_or(1.0),
                oblique_angle: style.map(|s| s.oblique_angle as f32).unwrap_or(0.0),
                is_backward: style.map(|s| s.is_backward()).unwrap_or(false),
                is_upside_down: style.map(|s| s.is_upside_down()).unwrap_or(false),
                is_vertical: style.map(|s| s.is_vertical).unwrap_or(false),
            }
        };

    // Accumulators keyed by quantised colour (+ weight for borders).
    let mut fills: HashMap<[u8; 4], ([f32; 4], Vec<[f32; 3]>)> = HashMap::default();
    // SDF cell text: glyph quads (per-vertex coloured) collected across all
    // cells; emitted as one text-carrying wire at the end.
    let mut text_verts: Vec<crate::scene::pipeline::text_gpu::TextVertex> = Vec::new();
    let mut borders: HashMap<([u8; 4], u32), ([f32; 4], f32, Vec<[f32; 3]>)> = HashMap::default();
    let mut emitted: rustc_hash::FxHashSet<(i32, i32, i32, i32, i32, i32)> =
        rustc_hash::FxHashSet::default();
    let sel_col = WireModel::SELECTED;

    let mut add_edge = |a: Vec3, b: Vec3, col: [f32; 4], lw: f32| {
        let k = (
            (a.x * 1000.0) as i32,
            (a.y * 1000.0) as i32,
            (a.z * 1000.0) as i32,
            (b.x * 1000.0) as i32,
            (b.y * 1000.0) as i32,
            (b.z * 1000.0) as i32,
        );
        let kr = (k.3, k.4, k.5, k.0, k.1, k.2);
        if emitted.contains(&k) || emitted.contains(&kr) {
            return;
        }
        emitted.insert(k);
        let entry = borders
            .entry((key4(col), (lw * 100.0) as u32))
            .or_insert_with(|| (col, lw, Vec::new()));
        if !entry.2.is_empty() {
            entry.2.push([f32::NAN; 3]);
        }
        entry.2.push(rel(a));
        entry.2.push(rel(b));
    };

    let normal = v3(&tab.normal).normalize_or(Vec3::Z);
    for (ri, row) in tab.rows.iter().enumerate() {
        let row_style: Option<&acadrust::objects::RowCellStyle> = table_style.map(|ts| {
            let kind = match (title_suppressed, header_suppressed, ri) {
                (false, _, 0) => 0,
                (false, false, 1) => 1,
                (true, false, 0) => 1,
                _ => 2,
            };
            match kind {
                0 => &ts.title_row_style,
                1 => &ts.header_row_style,
                _ => &ts.data_row_style,
            }
        });

        for (ci, cell) in row.cells.iter().enumerate() {
            let Some((_, _, row_end, column_end)) =
                merged_owner_and_span(tab, ri, ci)
            else {
                continue;
            };
            let frames = break_frames_for_row(
                tab,
                ri,
                h,
                v_flow,
                &row_offsets,
                anno_scale,
                top_label_rows,
                bottom_label_rows,
            );
            for (origin, row_top) in frames {
            let merged_height = row_offsets
                .get(row_end + 1)
                .copied()
                .unwrap_or(row_offsets[ri])
                - row_offsets[ri];
            let row_bot = row_top + merged_height;
            let row_mid = (row_top + row_bot) * 0.5;
            let col_left = col_offsets[ci];
            let col_right = col_offsets
                .get(column_end + 1)
                .copied()
                .unwrap_or(col_left);
            let col_width = col_right - col_left;
            let tl = origin + h * col_left + v_flow * row_top;
            let tr = origin + h * col_right + v_flow * row_top;
            let br_ = origin + h * col_right + v_flow * row_bot;
            let bl = origin + h * col_left + v_flow * row_bot;
            // ── Fill ──────────────────────────────────────────────────────
            let fill_style = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::BACKGROUND_COLOR,
            );
            let (fill_on, fill_color) = if let Some(cs) = fill_style {
                (cs.fill_enabled, cs.background_color)
            } else if let Some(rs) = row_style {
                (rs.fill_enabled, rs.fill_color)
            } else {
                (false, Color::ByLayer)
            };
            if fill_on {
                let col = resolve_col(&fill_color, entity_color);
                let buf = &mut fills
                    .entry(key4(col))
                    .or_insert_with(|| (col, Vec::new()))
                    .1;
                for v in [bl, br_, tr, bl, tr, tl] {
                    buf.push(rel(v));
                }
            }

            // ── Borders (per edge: cell override → row style → default) ───
            // (top, right, bottom, left)
            let edge = |which: u8| -> (bool, [f32; 4], f32) {
                let edge_flag = match which {
                    0 => acadrust::entities::table::CellEdgeFlags::TOP,
                    1 => acadrust::entities::table::CellEdgeFlags::RIGHT,
                    2 => acadrust::entities::table::CellEdgeFlags::BOTTOM,
                    _ => acadrust::entities::table::CellEdgeFlags::LEFT,
                };
                if let Some(cs) = style_for_border(tab, row, ci, cell, edge_flag) {
                    let b = match which {
                        0 => &cs.top_border,
                        1 => &cs.right_border,
                        2 => &cs.bottom_border,
                        _ => &cs.left_border,
                    };
                    (
                        !b.invisible,
                        if selected {
                            sel_col
                        } else {
                            resolve_col(&b.color, entity_color)
                        },
                        lw_px(&b.line_weight),
                    )
                } else if let Some(rs) = row_style {
                    let b = match which {
                        0 => &rs.top_border,
                        1 => &rs.right_border,
                        2 => &rs.bottom_border,
                        _ => &rs.left_border,
                    };
                    (
                        !b.is_invisible,
                        if selected {
                            sel_col
                        } else {
                            resolve_col(&b.color, entity_color)
                        },
                        lw_px(&b.line_weight),
                    )
                } else {
                    (
                        true,
                        if selected { sel_col } else { entity_color },
                        line_weight_px,
                    )
                }
            };
            let (tv, tc, tw) = edge(0);
            if tv {
                add_edge(tl, tr, tc, tw);
            }
            let (rv, rc, rw) = edge(1);
            if rv {
                add_edge(tr, br_, rc, rw);
            }
            let (bv, bc, bw) = edge(2);
            if bv {
                add_edge(bl, br_, bc, bw);
            }
            let (lv, lc, lw) = edge(3);
            if lv {
                add_edge(tl, bl, lc, lw);
            }

            let value_contents: Vec<_> = cell
                .contents
                .iter()
                .enumerate()
                .filter_map(|(index, content)| {
                    let text = content_display_value(document, tab, content);
                    (!text.is_empty()).then_some((index, content, text))
                })
                .collect();
            let fallback_text_height = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::TEXT_HEIGHT,
            )
            .map(|style| style.text_height as f32)
            .or_else(|| row_style.map(|style| style.text_height as f32))
            .filter(|height| *height > 1e-6)
            .unwrap_or(0.18)
                * anno_scale;
            let fallback_margin_left = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::MARGIN_LEFT,
            )
            .map(|style| style.margin_left as f32 * anno_scale)
            .unwrap_or(h_margin.max(fallback_text_height * 0.5));
            let fallback_margin_right = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::MARGIN_RIGHT,
            )
            .map(|style| style.margin_right as f32 * anno_scale)
            .unwrap_or(h_margin.max(fallback_text_height * 0.5));
            let fallback_margin_top = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::MARGIN_TOP,
            )
            .map(|style| style.margin_top as f32 * anno_scale)
            .unwrap_or(v_margin.max(fallback_text_height * 0.5));
            let fallback_margin_bottom = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::MARGIN_BOTTOM,
            )
            .map(|style| style.margin_bottom as f32 * anno_scale)
            .unwrap_or(v_margin.max(fallback_text_height * 0.5));
            let fallback_layout = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::CONTENT_LAYOUT,
            )
            .map(|style| style.layout_flags)
            .unwrap_or(acadrust::entities::table::ContentLayoutFlags::FLOW);
            let fallback_alignment = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::ALIGNMENT,
            )
            .map(|style| style.alignment)
            .or_else(|| row_style.map(|style| style.alignment as i32))
            .unwrap_or(5);
            let fallback_h_spacing = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::MARGIN_HORIZONTAL_SPACING,
            )
            .map(|style| style.horizontal_spacing as f32 * anno_scale)
            .unwrap_or(0.0);
            let fallback_v_spacing = style_for_property(
                tab,
                row,
                ci,
                cell,
                acadrust::entities::table::CellStylePropertyFlags::MARGIN_VERTICAL_SPACING,
            )
            .map(|style| style.vertical_spacing as f32 * anno_scale)
            .unwrap_or(0.0);
            let fallback_sizes: Vec<_> = value_contents
                .iter()
                .map(|(_, content, text)| {
                    let height = if content.text_height > 1e-6 {
                        content.text_height as f32 * anno_scale
                    } else {
                        fallback_text_height
                    };
                    let mut max_chars = 0usize;
                    let mut line_count = 0usize;
                    for line in text.split("\\P") {
                        max_chars = max_chars.max(line.chars().count());
                        line_count += 1;
                    }
                    (
                        (max_chars as f32 * height * 0.6).max(height * 0.5),
                        line_count.max(1) as f32 * height * 1.2,
                    )
                })
                .collect();
            let fallback_centers = fallback_content_centers(
                [
                    col_left + fallback_margin_left,
                    row_top + fallback_margin_top,
                    col_right - fallback_margin_right,
                    row_bot - fallback_margin_bottom,
                ],
                &fallback_sizes,
                fallback_layout,
                fallback_alignment,
                fallback_h_spacing,
                fallback_v_spacing,
            );
            for (slot_index, (content_index, content, text)) in
                value_contents.iter().enumerate()
            {
                let text_height_style = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    acadrust::entities::table::CellStylePropertyFlags::TEXT_HEIGHT,
                );
                let cell_h = (content.text_height > 1e-6)
                    .then_some(content.text_height)
                    .or_else(|| {
                        text_height_style
                            .map(|style| style.text_height)
                            .filter(|height| *height > 1e-6)
                    })
                    .or_else(|| {
                        row_style
                            .map(|style| style.text_height)
                            .filter(|height| *height > 1e-6)
                    })
                    .map(|height| height as f32)
                    .unwrap_or(0.18)
                    * anno_scale;
                let margin_left = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    acadrust::entities::table::CellStylePropertyFlags::MARGIN_LEFT,
                )
                    .map(|style| style.margin_left as f32 * anno_scale)
                    .filter(|margin| *margin > 1e-6)
                    .unwrap_or_else(|| h_margin.max(cell_h * 0.5));
                let margin_right = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    acadrust::entities::table::CellStylePropertyFlags::MARGIN_RIGHT,
                )
                    .map(|style| style.margin_right as f32 * anno_scale)
                    .filter(|margin| *margin > 1e-6)
                    .unwrap_or_else(|| h_margin.max(cell_h * 0.5));
                let margin_top = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    acadrust::entities::table::CellStylePropertyFlags::MARGIN_TOP,
                )
                    .map(|style| style.margin_top as f32 * anno_scale)
                    .filter(|margin| *margin > 1e-6)
                    .unwrap_or_else(|| v_margin.max(cell_h * 0.5));
                let margin_bottom = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    acadrust::entities::table::CellStylePropertyFlags::MARGIN_BOTTOM,
                )
                    .map(|style| style.margin_bottom as f32 * anno_scale)
                    .filter(|margin| *margin > 1e-6)
                    .unwrap_or_else(|| v_margin.max(cell_h * 0.5));
                let style_handle = content.text_style_handle.or_else(|| {
                    style_for_property(
                        tab,
                        row,
                        ci,
                        cell,
                        acadrust::entities::table::CellStylePropertyFlags::TEXT_STYLE,
                    )
                    .and_then(|style| style.text_style_handle)
                })
                    .or_else(|| row_style.and_then(|style| style.text_style_handle));
                let font_owned =
                    font_for_handle(style_handle).unwrap_or_else(|| "txt".to_string());
                let resolved = resolved_style_for_handle(style_handle, font_owned);
                let align = (content.alignment != 0)
                    .then_some(content.alignment)
                    .or_else(|| {
                        style_for_property(
                            tab,
                            row,
                            ci,
                            cell,
                            acadrust::entities::table::CellStylePropertyFlags::ALIGNMENT,
                        )
                            .map(|style| style.alignment)
                            .filter(|alignment| *alignment != 0)
                    })
                    .or_else(|| row_style.map(|style| style.alignment as i32))
                    .unwrap_or(5);
                let horiz = ((align - 1).rem_euclid(3)) + 1;
                let vert = ((align - 1) / 3) + 1;
                let (mut x_offset, mut attach_h_anchor) = match horiz {
                    1 => (col_left + margin_left, 0.0_f32),
                    3 => (col_right - margin_right, 1.0_f32),
                    _ => (col_left + col_width * 0.5, 0.5_f32),
                };
                let (mut y_offset, mut v_anchor) = match vert {
                    1 => (row_top + margin_top, MTextVAnchor::Top),
                    3 => (row_bot - margin_bottom, MTextVAnchor::Bottom),
                    _ => (row_mid, MTextVAnchor::Middle),
                };
                let mut z_offset = 0.0;
                if let Some(geometry) = resolved_content_geometry(
                    document,
                    tab,
                    ri,
                    ci,
                    cell,
                    *content_index,
                ) {
                    x_offset =
                        col_left + geometry.distance_to_center.x as f32 * anno_scale;
                    y_offset =
                        row_top - geometry.distance_to_center.y as f32 * anno_scale;
                    z_offset = geometry.distance_to_center.z as f32 * anno_scale;
                    attach_h_anchor = 0.5;
                    v_anchor = MTextVAnchor::Middle;
                } else if value_contents.len() > 1 {
                    if let Some((x, y)) = fallback_centers.get(slot_index) {
                        x_offset = *x;
                        y_offset = *y;
                        attach_h_anchor = 0.5;
                        v_anchor = MTextVAnchor::Middle;
                    }
                }
                let to = origin
                    + h * x_offset
                    + v_flow * y_offset
                    + normal * z_offset;
                let content_rotation_explicit = content.format_property_flags
                    & acadrust::entities::table::CellStylePropertyFlags::ROTATION.bits() as i32
                    != 0;
                let rot = if content_rotation_explicit {
                    content.rotation as f32
                } else {
                    style_for_property(
                        tab,
                        row,
                        ci,
                        cell,
                        acadrust::entities::table::CellStylePropertyFlags::ROTATION,
                    )
                    .map(|style| style.rotation as f32)
                    .unwrap_or(cell.rotation as f32)
                };
                let layout = layout_mtext(&MTextRenderOpts {
                    columns: Default::default(),
                    value: text,
                    insertion: [to.x as f64, to.y as f64, to.z as f64],
                    height: cell_h,
                    rect_w: (col_width - margin_left - margin_right).max(0.0),
                    rotation: rot,
                    style: &resolved,
                    attach_h_anchor,
                    v_anchor,
                    line_spacing_factor: 1.0,
                    exact_line_spacing: false,
                    rectangle_height: 0.0,
                    vertical_text: false,
                    want_glyph_boxes: false,
                });
                let tcol = if selected {
                    sel_col
                } else if !matches!(
                    content.color,
                    Color::ByLayer | Color::ByBlock
                ) {
                    resolve_col(&content.color, entity_color)
                } else if let Some(style) = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    acadrust::entities::table::CellStylePropertyFlags::CONTENT_COLOR,
                ) {
                    resolve_col(&style.content_color, entity_color)
                } else if let Some(style) = row_style {
                    resolve_col(&style.text_color, entity_color)
                } else {
                    entity_color
                };
                if let Ok(mut atlas) = crate::scene::text::sdf_atlas::text_atlas().lock() {
                    for stroke in &layout.strokes {
                        let Some(run) = &stroke.run else {
                            continue;
                        };
                        let quads = crate::scene::text::glyph_quads::layout_glyph_quads(
                            &mut atlas,
                            run.height,
                            run.rotation,
                            run.width_factor,
                            run.oblique,
                            run.tracking,
                            &run.font,
                            run.bold,
                            &run.text,
                        );
                        crate::scene::pipeline::text_gpu::push_glyph_vertices(
                            &mut text_verts,
                            &quads,
                            [stroke.origin[0], stroke.origin[1], to.z as f64],
                            1.0,
                            tcol,
                            0.0,
                        );
                    }
                }
            }
            }
        }
    }

    let name = tab.common.handle.value().to_string();
    let mk =
        |color: [f32; 4], points: Vec<[f32; 3]>, fill_tris: Vec<[f32; 3]>, lw: f32| -> WireModel {
            WireModel {
                point_marker: None,
                taper_widths: Vec::new(),
                pattern_stations: Vec::new(),
                world_width: 0.0,
                depth_override: None,
                display_visible: true,
                plot_visible: true,
                fill_is_3d: false,
                fill_is_2d_solid: false,
                render_instance: None,
                pick_tris: Vec::new(),
                pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
                name: name.clone(),
                points,
                points_low: Vec::new(),
                color,
                selected,
                pattern_length: 0.0,
                pattern: [0.0; 8],
                line_weight_px: lw,
                aci: 0,
                snap_pts: vec![],
                tangent_geoms: vec![],
                key_vertices: vec![],
                aabb: WireModel::UNBOUNDED_AABB,
                plinegen: true,
                fill_tris,
                // fill_tris_low intentionally empty: this fill renders on the
                // top-level path, where consumers (face3d_gpu, xclip) treat a
                // short low half as all-zero, so it draws at f32 precision
                // (sub-metre error at UTM scale) — not a crash. Follow-up:
                // double-single-split via points_to_ds to match emit_wire.
                fill_tris_low: Vec::new(),
            }
        };

    let mut out: Vec<WireModel> = Vec::new();
    // Fills first (drawn under borders/text).
    for (_, (color, tris)) in fills {
        if !tris.is_empty() {
            out.push(mk(color, vec![], tris, 1.0));
        }
    }
    for (_, (color, lw, pts)) in borders {
        if !pts.is_empty() {
            out.push(mk(color, pts, vec![], lw));
        }
    }
    // SDF cell text: one wire carrying the glyph quads (per-vertex coloured) +
    // a glyph-bounds AABB (f64 accumulate → f32) so the text draws + picks;
    // empty points so it adds no stroke geometry.
    if !text_verts.is_empty() {
        let (mut nx, mut ny, mut xx, mut xy) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for v in &text_verts {
            let x = v.pos[0] as f64 + v.pos_low[0] as f64;
            let y = v.pos[1] as f64 + v.pos_low[1] as f64;
            nx = nx.min(x);
            xx = xx.max(x);
            ny = ny.min(y);
            xy = xy.max(y);
        }
        let mut w = mk(entity_color, vec![], vec![], line_weight_px);
        w.aabb = [nx as f32, ny as f32, xx as f32, xy as f32];
        w.text_verts = text_verts;
        out.push(w);
    }
    out
}

impl Grippable for Table {
    fn grips(&self) -> Vec<GripDef> {
        let origin = glam::DVec3::new(
            self.insertion_point.x,
            self.insertion_point.y,
            self.insertion_point.z,
        );
        let horizontal = glam::DVec3::new(
            self.horizontal_direction.x,
            self.horizontal_direction.y,
            self.horizontal_direction.z,
        )
        .normalize_or(glam::DVec3::X);
        let normal = glam::DVec3::new(self.normal.x, self.normal.y, self.normal.z)
            .normalize_or(glam::DVec3::Z);
        let down = horizontal.cross(normal).normalize_or(glam::DVec3::NEG_Y);
        let width = self.total_width();
        let height = self.total_height();
        let mut grips = vec![square_grip(
            0,
            origin,
        )];
        grips.push(square_grip(1, origin + horizontal * width));
        grips.push(square_grip(2, origin + down * height));
        let mut offset = 0.0;
        for (column, definition) in self.columns.iter().enumerate() {
            offset += definition.width;
            if column + 1 < self.columns.len() {
                grips.push(square_grip(
                    100 + column,
                    origin + horizontal * offset + down * (height * 0.5),
                ));
            }
        }
        offset = 0.0;
        for (row, definition) in self.rows.iter().enumerate() {
            offset += definition.height;
            if row + 1 < self.rows.len() {
                grips.push(square_grip(
                    1000 + row,
                    origin + horizontal * (width * 0.5) + down * offset,
                ));
            }
        }
        for (index, data) in self.break_data.iter().enumerate() {
            grips.push(square_grip(
                2000 + index,
                origin + glam::DVec3::new(data.position.x, data.position.y, data.position.z),
            ));
        }
        grips
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if let GripApply::Translate(delta) = apply {
            if let Some(grip) = self.grips().into_iter().find(|grip| grip.id == grip_id) {
                self.apply_grip(grip_id, GripApply::Absolute(grip.world + delta));
            }
            return;
        }
        let GripApply::Absolute(point) = apply else {
            return;
        };
        if grip_id == 0 {
            self.insertion_point.x = point.x;
            self.insertion_point.y = point.y;
            self.insertion_point.z = point.z;
            return;
        }
        let origin = glam::DVec3::new(
            self.insertion_point.x,
            self.insertion_point.y,
            self.insertion_point.z,
        );
        let horizontal = glam::DVec3::new(
            self.horizontal_direction.x,
            self.horizontal_direction.y,
            self.horizontal_direction.z,
        )
        .normalize_or(glam::DVec3::X);
        let normal = glam::DVec3::new(self.normal.x, self.normal.y, self.normal.z)
            .normalize_or(glam::DVec3::Z);
        let down = horizontal.cross(normal).normalize_or(glam::DVec3::NEG_Y);
        if grip_id == 1 {
            let width = (point - origin).dot(horizontal).max(1.0e-6);
            let current = self.total_width();
            if current > 1.0e-12 {
                for column in &mut self.columns {
                    column.width *= width / current;
                }
            }
        } else if grip_id == 2 {
            let height = (point - origin).dot(down).max(1.0e-6);
            let current = self.total_height();
            if current > 1.0e-12 {
                for row in &mut self.rows {
                    row.height *= height / current;
                }
            }
        } else if (100..1000).contains(&grip_id) {
            let column = grip_id - 100;
            let before: f64 = self.columns.iter().take(column).map(|item| item.width).sum();
            if let Some(definition) = self.columns.get_mut(column) {
                definition.width = ((point - origin).dot(horizontal) - before).max(1.0e-6);
            }
        } else if (1000..2000).contains(&grip_id) {
            let row = grip_id - 1000;
            let before: f64 = self.rows.iter().take(row).map(|item| item.height).sum();
            if let Some(definition) = self.rows.get_mut(row) {
                definition.height = ((point - origin).dot(down) - before).max(1.0e-6);
            }
        } else if grip_id >= 2000 {
            if let Some(data) = self.break_data.get_mut(grip_id - 2000) {
                let offset = point - origin;
                data.position = acadrust::types::Vector3::new(offset.x, offset.y, offset.z);
            }
        }
    }
}

impl PropertyEditable for Table {
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection> {
        use crate::entities::common::edit_prop as edit;
        use acadrust::entities::table::{BreakOptionFlags, CellStateFlags, CellStylePropertyFlags};
        let bool_text = |value: bool| if value { t!("Yes") } else { t!("No") }.into_owned();
        let toggle = |label: &str, field: &'static str, value: bool, enabled: bool| -> Property {
            Property {
                label: label.into(),
                field,
                value: if enabled {
                    PropValue::BoolToggle { field, value }
                } else {
                    PropValue::ReadOnly(bool_text(value))
                },
            }
        };
        let choice = |label: &str,
                      field: &'static str,
                      selected: String,
                      options: Vec<String>,
                      enabled: bool|
         -> Property {
            Property {
                label: label.into(),
                field,
                value: if enabled {
                    PropValue::Choice { selected, options }
                } else {
                    PropValue::ReadOnly(selected)
                },
            }
        };
        let number = |label: &str, field: &'static str, value: f64, enabled: bool| -> Property {
            if enabled {
                edit(label, field, value)
            } else {
                ro(label, field, crate::entities::common::format_length(value))
            }
        };
        let text = |label: &str, field: &'static str, value: String, enabled: bool| -> Property {
            Property {
                label: label.into(),
                field,
                value: if enabled {
                    PropValue::PlainText(value)
                } else {
                    PropValue::ReadOnly(value)
                },
            }
        };
        let color_text = |color: acadrust::types::Color| match color {
            acadrust::types::Color::None => "None".to_string(),
            acadrust::types::Color::ByLayer => "ByLayer".to_string(),
            acadrust::types::Color::ByBlock => "ByBlock".to_string(),
            acadrust::types::Color::Index(index) => index.to_string(),
            acadrust::types::Color::Rgb { r, g, b } => format!("{r},{g},{b}"),
        };
        let color = |label: &str,
                     field: &'static str,
                     value: acadrust::types::Color,
                     enabled: bool|
         -> Property {
            Property {
                label: label.into(),
                field,
                value: if enabled {
                    PropValue::ColorChoice(value)
                } else {
                    PropValue::ReadOnly(color_text(value))
                },
            }
        };
        // Direction = angle of the horizontal direction vector in the XY plane.
        let direction_deg =
            (self.horizontal_direction.y.atan2(self.horizontal_direction.x)).to_degrees();
        let break_height = self.break_data.first().map(|data| data.height).unwrap_or(0.0);
        let breaks_enabled = self.break_options.contains(BreakOptionFlags::ENABLE_BREAKS);
        let manual_positions = self
            .break_options
            .contains(BreakOptionFlags::ALLOW_MANUAL_POSITIONS);
        let manual_heights = self
            .break_options
            .contains(BreakOptionFlags::ALLOW_MANUAL_HEIGHTS);
        let cell_count = self.rows.len().saturating_mul(self.columns.len());
        let cell_index = prop_current_cell().min(cell_count.saturating_sub(1));
        let cell_row = if self.columns.is_empty() {
            0
        } else {
            cell_index / self.columns.len()
        };
        let cell_column = if self.columns.is_empty() {
            0
        } else {
            cell_index % self.columns.len()
        };
        let table_row = self.rows.get(cell_row);
        let cell = self.cell(cell_row, cell_column);
        let row_style = table_row.and_then(|row| row.style.as_ref());
        let column_style = self
            .columns
            .get(cell_column)
            .and_then(|column| column.style.as_ref());
        let style_for = |property| {
            table_row.and_then(|row| {
                cell.and_then(|cell| {
                    style_for_property(self, row, cell_column, cell, property)
                })
            })
        };
        let alignment_style = style_for(CellStylePropertyFlags::ALIGNMENT);
        let alignment = match alignment_style.map(|style| style.alignment).unwrap_or(5) {
            1 => "Top Left",
            2 => "Top Center",
            3 => "Top Right",
            4 => "Middle Left",
            6 => "Middle Right",
            7 => "Bottom Left",
            8 => "Bottom Center",
            9 => "Bottom Right",
            _ => "Middle Center",
        };
        let state = cell.map(|cell| cell.state).unwrap_or_default();
        let content_editable = !state.intersects(
            CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY,
        );
        let format_editable = !state.intersects(
            CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY,
        );
        let immutable_lock = state.intersects(
            CellStateFlags::CONTENT_READ_ONLY | CellStateFlags::FORMAT_READ_ONLY,
        );
        let cell_locked = state.intersects(
            CellStateFlags::CONTENT_LOCKED
                | CellStateFlags::CONTENT_READ_ONLY
                | CellStateFlags::FORMAT_LOCKED
                | CellStateFlags::FORMAT_READ_ONLY,
        );
        let table_flow_up = self
            .legacy_style_override
            .as_ref()
            .and_then(|style| style.flow_direction)
            .map(|flow| flow != 0)
            .unwrap_or_else(|| {
                self.base_style.as_ref().is_some_and(|style| {
                    style
                        .property_flags
                        .contains(CellStylePropertyFlags::FLOW_DIRECTION_BOTTOM_TO_TOP)
                })
            });
        let legacy = self.legacy_style_override.as_ref();
        let title_suppressed = legacy
            .and_then(|style| style.title_suppressed)
            .unwrap_or(false);
        let header_suppressed = legacy
            .and_then(|style| style.header_suppressed)
            .unwrap_or(false);
        let horizontal_margin = legacy
            .and_then(|style| style.horizontal_cell_margin)
            .or_else(|| self.base_style.as_ref().map(|style| style.margin_left))
            .unwrap_or(0.06);
        let vertical_margin = legacy
            .and_then(|style| style.vertical_cell_margin)
            .or_else(|| self.base_style.as_ref().map(|style| style.margin_top))
            .unwrap_or(0.06);
        let uniform_column_width = self.columns.first().map(|column| column.width).unwrap_or(0.0);
        let columns_uniform = self
            .columns
            .iter()
            .all(|column| (column.width - uniform_column_width).abs() <= 1.0e-9);
        let uniform_row_height = self.rows.first().map(|row| row.height).unwrap_or(0.0);
        let rows_uniform = self
            .rows
            .iter()
            .all(|row| (row.height - uniform_row_height).abs() <= 1.0e-9);
        let uniform_number = |label: &str,
                              field: &'static str,
                              value: f64,
                              uniform: bool|
         -> Property {
            if uniform {
                edit(label, field, value)
            } else {
                Property {
                    label: label.into(),
                    field,
                    value: PropValue::PlainText(t!("Varies").into_owned()),
                }
            }
        };
        let override_count = self
            .rows
            .iter()
            .filter(|row| row.style.is_some())
            .count()
            + self
                .columns
                .iter()
                .filter(|column| column.style.is_some())
                .count()
            + self
                .rows
                .iter()
                .flat_map(|row| row.cells.iter())
                .filter(|cell| cell.style.is_some())
                .count()
            + usize::from(self.base_style.is_some() || self.legacy_style_override.is_some());

        let mut sections = vec![
            PropSection {
                title: t!("Table").into_owned(),
                props: vec![
                    ro(t!("Table style").as_ref(), "tbl_style_handle", "Standard"),
                    toggle(
                        t!("Title suppressed").as_ref(),
                        "tbl_title_suppressed",
                        title_suppressed,
                        true,
                    ),
                    toggle(
                        t!("Header suppressed").as_ref(),
                        "tbl_header_suppressed",
                        header_suppressed,
                        true,
                    ),
                    choice(
                        t!("Flow direction").as_ref(),
                        "tbl_flow_direction",
                        if table_flow_up { "Up" } else { "Down" }.to_string(),
                        vec!["Down".into(), "Up".into()],
                        true,
                    ),
                    edit(t!("Direction").as_ref(), "tbl_direction", direction_deg),
                    edit(t!("Rows").as_ref(), "tbl_rows", self.rows.len() as f64),
                    edit(t!("Columns").as_ref(), "tbl_cols", self.columns.len() as f64),
                    uniform_number(
                        t!("Column width").as_ref(),
                        "tbl_column_width",
                        uniform_column_width,
                        columns_uniform,
                    ),
                    uniform_number(
                        t!("Row height").as_ref(),
                        "tbl_row_height",
                        uniform_row_height,
                        rows_uniform,
                    ),
                    edit(t!("Table width").as_ref(), "tbl_width", self.total_width()),
                    edit(t!("Table height").as_ref(), "tbl_height", self.total_height()),
                    edit(
                        t!("Horizontal cell margin").as_ref(),
                        "tbl_horizontal_margin",
                        horizontal_margin,
                    ),
                    edit(
                        t!("Vertical cell margin").as_ref(),
                        "tbl_vertical_margin",
                        vertical_margin,
                    ),
                    ro(
                        t!("Table overrides").as_ref(),
                        "tbl_overrides",
                        if override_count == 0 {
                            t!("None").into_owned()
                        } else {
                            override_count.to_string()
                        },
                    ),
                ],
            },
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    edit(t!("Insertion X").as_ref(), "tbl_insert_x", self.insertion_point.x),
                    edit(t!("Insertion Y").as_ref(), "tbl_insert_y", self.insertion_point.y),
                    edit(t!("Insertion Z").as_ref(), "tbl_insert_z", self.insertion_point.z),
                    ro(t!("Normal X").as_ref(), "tbl_normal_x", format!("{:.4}", self.normal.x)),
                    ro(t!("Normal Y").as_ref(), "tbl_normal_y", format!("{:.4}", self.normal.y)),
                    ro(t!("Normal Z").as_ref(), "tbl_normal_z", format!("{:.4}", self.normal.z)),
                ],
            },
        ];

        if prop_current_cell_active() {
            let cell_type = match cell.map(|cell| cell.cell_type) {
                Some(acadrust::entities::table::CellType::Block) => "Block",
                _ => "Text",
            };
            let data_type = cell
                .and_then(|cell| cell.contents.first())
                .map(|content| match content.value.value_type {
                    acadrust::entities::table::CellValueType::Long => "Integer",
                    acadrust::entities::table::CellValueType::Double => "Decimal",
                    acadrust::entities::table::CellValueType::Date => "Date",
                    acadrust::entities::table::CellValueType::Point2D => "Point 2D",
                    acadrust::entities::table::CellValueType::Point3D => "Point 3D",
                    acadrust::entities::table::CellValueType::Handle => "Handle",
                    _ => "Text",
                })
                .unwrap_or("Text");
            let style_source = if cell.and_then(|cell| cell.style.as_ref()).is_some() {
                "Cell override"
            } else if row_style.is_some() {
                "Row override"
            } else if column_style.is_some() {
                "Column override"
            } else {
                "Table style"
            };
            let text_style = style_for(CellStylePropertyFlags::TEXT_STYLE)
                .map(|style| style.text_style_name.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "Standard".to_string());
            let mut available_text_styles = text_style_names.to_vec();
            if !available_text_styles
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&text_style))
            {
                available_text_styles.insert(0, text_style.clone());
            }
            let rotation = style_for(CellStylePropertyFlags::ROTATION)
                .map(|style| style.rotation)
                .or_else(|| cell.map(|cell| cell.rotation))
                .unwrap_or(0.0)
                .to_degrees();
            sections.push(PropSection {
                title: t!("Cell").into_owned(),
                props: vec![
                    Property {
                        label: t!("Current cell").into_owned(),
                        field: "tbl_current_cell",
                        value: PropValue::Stepper {
                            field: "tbl_current_cell",
                            display: format!(
                                "{},{}  ({} / {})",
                                cell_row,
                                cell_column,
                                cell_index.saturating_add(1),
                                cell_count
                            ),
                        },
                    },
                    ro(t!("Row").as_ref(), "tbl_cell_row", cell_row.to_string()),
                    ro(
                        t!("Column").as_ref(),
                        "tbl_cell_column",
                        cell_column.to_string(),
                    ),
                    ro(t!("Cell style").as_ref(), "tbl_cell_style", style_source),
                    choice(
                        t!("Cell type").as_ref(),
                        "tbl_cell_type",
                        cell_type.to_string(),
                        vec!["Text".into(), "Block".into()],
                        content_editable,
                    ),
                    text(
                        t!("Contents").as_ref(),
                        "tbl_cell_text",
                        cell.map(|cell| cell.text_value().to_string()).unwrap_or_default(),
                        content_editable,
                    ),
                    choice(
                        t!("Alignment").as_ref(),
                        "tbl_cell_alignment",
                        alignment.to_string(),
                        vec![
                            "Top Left".into(),
                            "Top Center".into(),
                            "Top Right".into(),
                            "Middle Left".into(),
                            "Middle Center".into(),
                            "Middle Right".into(),
                            "Bottom Left".into(),
                            "Bottom Center".into(),
                            "Bottom Right".into(),
                        ],
                        format_editable,
                    ),
                    choice(
                        t!("Text style").as_ref(),
                        "tbl_cell_text_style",
                        text_style,
                        available_text_styles,
                        format_editable,
                    ),
                    choice(
                        t!("Text rotation").as_ref(),
                        "tbl_cell_rotation",
                        format!("{rotation:.0}"),
                        vec!["0".into(), "90".into(), "180".into(), "270".into()],
                        format_editable,
                    ),
                    number(
                        t!("Text height").as_ref(),
                        "tbl_cell_text_height",
                        style_for(CellStylePropertyFlags::TEXT_HEIGHT)
                            .map(|style| style.text_height)
                            .unwrap_or(0.18),
                        format_editable,
                    ),
                    color(
                        t!("Content color").as_ref(),
                        "tbl_cell_content_color",
                        style_for(CellStylePropertyFlags::CONTENT_COLOR)
                            .map(|style| style.content_color)
                            .unwrap_or(acadrust::types::Color::ByBlock),
                        format_editable,
                    ),
                    color(
                        t!("Background color").as_ref(),
                        "tbl_cell_background_color",
                        style_for(CellStylePropertyFlags::BACKGROUND_COLOR)
                            .map(|style| style.background_color)
                            .unwrap_or(acadrust::types::Color::ByBlock),
                        format_editable,
                    ),
                    choice(
                        t!("Data type").as_ref(),
                        "tbl_cell_data_type",
                        data_type.to_string(),
                        vec![
                            "Text".into(),
                            "Integer".into(),
                            "Decimal".into(),
                            "Date".into(),
                            "Point 2D".into(),
                            "Point 3D".into(),
                            "Handle".into(),
                        ],
                        format_editable,
                    ),
                    text(
                        t!("Data format").as_ref(),
                        "tbl_cell_format",
                        style_for(CellStylePropertyFlags::DATA_FORMAT)
                            .map(|style| style.value_format.clone())
                            .unwrap_or_default(),
                        format_editable,
                    ),
                    toggle(
                        t!("Background fill").as_ref(),
                        "tbl_cell_fill",
                        style_for(CellStylePropertyFlags::BACKGROUND_COLOR)
                            .is_some_and(|style| style.fill_enabled),
                        format_editable,
                    ),
                    number(
                        t!("Left margin").as_ref(),
                        "tbl_cell_margin_left",
                        style_for(CellStylePropertyFlags::MARGIN_LEFT)
                            .map(|style| style.margin_left)
                            .unwrap_or(horizontal_margin),
                        format_editable,
                    ),
                    number(
                        t!("Top margin").as_ref(),
                        "tbl_cell_margin_top",
                        style_for(CellStylePropertyFlags::MARGIN_TOP)
                            .map(|style| style.margin_top)
                            .unwrap_or(vertical_margin),
                        format_editable,
                    ),
                    number(
                        t!("Right margin").as_ref(),
                        "tbl_cell_margin_right",
                        style_for(CellStylePropertyFlags::MARGIN_RIGHT)
                            .map(|style| style.margin_right)
                            .unwrap_or(horizontal_margin),
                        format_editable,
                    ),
                    number(
                        t!("Bottom margin").as_ref(),
                        "tbl_cell_margin_bottom",
                        style_for(CellStylePropertyFlags::MARGIN_BOTTOM)
                            .map(|style| style.margin_bottom)
                            .unwrap_or(vertical_margin),
                        format_editable,
                    ),
                    toggle(
                        t!("Top border").as_ref(),
                        "tbl_cell_border_top",
                        table_row
                            .and_then(|row| {
                                cell.and_then(|cell| {
                                    style_for_border(
                                        self,
                                        row,
                                        cell_column,
                                        cell,
                                        acadrust::entities::table::CellEdgeFlags::TOP,
                                    )
                                })
                            })
                            .is_none_or(|style| !style.top_border.invisible),
                        format_editable,
                    ),
                    toggle(
                        t!("Right border").as_ref(),
                        "tbl_cell_border_right",
                        table_row
                            .and_then(|row| {
                                cell.and_then(|cell| {
                                    style_for_border(
                                        self,
                                        row,
                                        cell_column,
                                        cell,
                                        acadrust::entities::table::CellEdgeFlags::RIGHT,
                                    )
                                })
                            })
                            .is_none_or(|style| !style.right_border.invisible),
                        format_editable,
                    ),
                    toggle(
                        t!("Bottom border").as_ref(),
                        "tbl_cell_border_bottom",
                        table_row
                            .and_then(|row| {
                                cell.and_then(|cell| {
                                    style_for_border(
                                        self,
                                        row,
                                        cell_column,
                                        cell,
                                        acadrust::entities::table::CellEdgeFlags::BOTTOM,
                                    )
                                })
                            })
                            .is_none_or(|style| !style.bottom_border.invisible),
                        format_editable,
                    ),
                    toggle(
                        t!("Left border").as_ref(),
                        "tbl_cell_border_left",
                        table_row
                            .and_then(|row| {
                                cell.and_then(|cell| {
                                    style_for_border(
                                        self,
                                        row,
                                        cell_column,
                                        cell,
                                        acadrust::entities::table::CellEdgeFlags::LEFT,
                                    )
                                })
                            })
                            .is_none_or(|style| !style.left_border.invisible),
                        format_editable,
                    ),
                    toggle(
                        t!("Locked").as_ref(),
                        "tbl_cell_locked",
                        cell_locked,
                        !immutable_lock,
                    ),
                ],
            });
        }

        sections.push(
            PropSection {
                title: t!("Table Breaks").into_owned(),
                props: vec![
                    toggle(
                        t!("Enabled").as_ref(),
                        "tbl_break_enabled",
                        breaks_enabled,
                        true,
                    ),
                    choice(
                        t!("Direction").as_ref(),
                        "tbl_break_direction",
                        match self.break_flow_direction {
                            acadrust::entities::table::BreakFlowDirection::Right => "Right",
                            acadrust::entities::table::BreakFlowDirection::Left => "Left",
                            acadrust::entities::table::BreakFlowDirection::Vertical => "Down",
                        }
                        .to_string(),
                        vec!["Right".into(), "Left".into(), "Down".into()],
                        breaks_enabled && !manual_positions,
                    ),
                    toggle(
                        t!("Repeat top labels").as_ref(),
                        "tbl_break_repeat_top",
                        self.break_options
                            .contains(BreakOptionFlags::REPEAT_TOP_LABELS),
                        breaks_enabled,
                    ),
                    toggle(
                        t!("Repeat bottom labels").as_ref(),
                        "tbl_break_repeat_bottom",
                        self.break_options
                            .contains(BreakOptionFlags::REPEAT_BOTTOM_LABELS),
                        breaks_enabled,
                    ),
                    toggle(
                        t!("Manual positions").as_ref(),
                        "tbl_break_manual_positions",
                        manual_positions,
                        breaks_enabled,
                    ),
                    toggle(
                        t!("Manual heights").as_ref(),
                        "tbl_break_manual_heights",
                        manual_heights,
                        breaks_enabled,
                    ),
                    number(
                        t!("Maximum height").as_ref(),
                        "tbl_break_height",
                        break_height,
                        breaks_enabled && !manual_heights,
                    ),
                    number(
                        t!("Spacing").as_ref(),
                        "tbl_break_spacing",
                        self.break_spacing,
                        breaks_enabled && !manual_positions,
                    ),
                ],
            },
        );

        sections
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        use crate::entities::common::parse_f64;
        use acadrust::entities::table::{
            BreakOptionFlags, CellStateFlags, CellStyle, CellStylePropertyFlags,
        };
        let flag = match field {
            "tbl_break_enabled" => Some(BreakOptionFlags::ENABLE_BREAKS),
            "tbl_break_repeat_top" => Some(BreakOptionFlags::REPEAT_TOP_LABELS),
            "tbl_break_repeat_bottom" => Some(BreakOptionFlags::REPEAT_BOTTOM_LABELS),
            "tbl_break_manual_positions" => Some(BreakOptionFlags::ALLOW_MANUAL_POSITIONS),
            "tbl_break_manual_heights" => Some(BreakOptionFlags::ALLOW_MANUAL_HEIGHTS),
            _ => None,
        };
        if let Some(flag) = flag {
            if flag != BreakOptionFlags::ENABLE_BREAKS
                && !self.break_options.contains(BreakOptionFlags::ENABLE_BREAKS)
            {
                return;
            }
            let on = if value == "toggle" {
                !self.break_options.contains(flag)
            } else {
                value == "true"
            };
            self.break_options.set(flag, on);
            self.break_ranges.clear();
            return;
        }
        match field {
            "tbl_title_suppressed" | "tbl_header_suppressed" => {
                let enabled = if value == "toggle" {
                    let current = if field == "tbl_title_suppressed" {
                        self.legacy_style_override
                            .as_ref()
                            .and_then(|style| style.title_suppressed)
                            .unwrap_or(false)
                    } else {
                        self.legacy_style_override
                            .as_ref()
                            .and_then(|style| style.header_suppressed)
                            .unwrap_or(false)
                    };
                    !current
                } else {
                    value == "true"
                };
                let style = self.legacy_style_override.get_or_insert_with(Default::default);
                if field == "tbl_title_suppressed" {
                    style.flags |= 0x0001;
                    style.title_suppressed = Some(enabled);
                } else {
                    style.flags |= 0x0002;
                    style.header_suppressed = Some(enabled);
                }
                self.override_flag = true;
                return;
            }
            "tbl_flow_direction" => {
                let up = value.trim().eq_ignore_ascii_case("up");
                let style = self.base_style.get_or_insert_with(Default::default);
                style
                    .property_flags
                    .set(CellStylePropertyFlags::FLOW_DIRECTION_BOTTOM_TO_TOP, up);
                let legacy = self.legacy_style_override.get_or_insert_with(Default::default);
                legacy.flags |= 0x0004;
                legacy.flow_direction = Some(if up { 1 } else { 0 });
                self.override_flag = true;
                return;
            }
            _ => {}
        }
        let cell_index = prop_current_cell();
        let columns = self.columns.len();
        let cell_position = (columns > 0).then_some((cell_index / columns, cell_index % columns));
        if let Some((row, column)) = cell_position {
            let state = self
                .cell(row, column)
                .map(|cell| cell.state)
                .unwrap_or_default();
            let content_editable = !state.intersects(
                CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY,
            );
            let format_editable = !state.intersects(
                CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY,
            );
            match field {
                "tbl_cell_text" => {
                    if !content_editable {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        cell.set_text(value);
                    }
                    return;
                }
                "tbl_cell_type" => {
                    if !content_editable {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        cell.cell_type = if value.trim().eq_ignore_ascii_case("block") {
                            acadrust::entities::table::CellType::Block
                        } else {
                            acadrust::entities::table::CellType::Text
                        };
                    }
                    return;
                }
                "tbl_cell_alignment" => {
                    if !format_editable {
                        return;
                    }
                    let alignment = match value.trim().to_ascii_uppercase().as_str() {
                        "TOP LEFT" => 1,
                        "TOP CENTER" => 2,
                        "TOP RIGHT" => 3,
                        "MIDDLE LEFT" => 4,
                        "MIDDLE RIGHT" => 6,
                        "BOTTOM LEFT" => 7,
                        "BOTTOM CENTER" => 8,
                        "BOTTOM RIGHT" => 9,
                        _ => 5,
                    };
                    if let Some(cell) = self.cell_mut(row, column) {
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style.alignment = alignment;
                        style.property_flags.insert(
                            acadrust::entities::table::CellStylePropertyFlags::ALIGNMENT,
                        );
                    }
                    return;
                }
                "tbl_cell_text_style" => {
                    if !format_editable {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style.text_style_name = value.trim().to_string();
                        style
                            .property_flags
                            .insert(CellStylePropertyFlags::TEXT_STYLE);
                    }
                    return;
                }
                "tbl_cell_rotation" => {
                    if !format_editable {
                        return;
                    }
                    if let Some(rotation) = parse_f64(value) {
                        if let Some(cell) = self.cell_mut(row, column) {
                            let style = cell.style.get_or_insert_with(CellStyle::new);
                            style.rotation = rotation.to_radians();
                            style
                                .property_flags
                                .insert(CellStylePropertyFlags::ROTATION);
                        }
                    }
                    return;
                }
                "tbl_cell_data_type" => {
                    if !format_editable {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        if cell.contents.is_empty() {
                            cell.set_text("");
                        }
                        if let Some(content) = cell.contents.first_mut() {
                            content.value.value_type = match value.trim().to_ascii_uppercase().as_str() {
                                "INTEGER" => acadrust::entities::table::CellValueType::Long,
                                "DECIMAL" => acadrust::entities::table::CellValueType::Double,
                                "DATE" => acadrust::entities::table::CellValueType::Date,
                                "POINT 2D" => acadrust::entities::table::CellValueType::Point2D,
                                "POINT 3D" => acadrust::entities::table::CellValueType::Point3D,
                                "HANDLE" => acadrust::entities::table::CellValueType::Handle,
                                _ => acadrust::entities::table::CellValueType::String,
                            };
                            content.value.raw_type_code = content.value.value_type as i32;
                        }
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style
                            .property_flags
                            .insert(CellStylePropertyFlags::DATA_TYPE);
                    }
                    return;
                }
                "tbl_cell_format" => {
                    if !format_editable {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style.value_format = value.to_string();
                        style.property_flags.insert(
                            acadrust::entities::table::CellStylePropertyFlags::DATA_FORMAT,
                        );
                    }
                    return;
                }
                "tbl_cell_fill" => {
                    if !format_editable {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style.fill_enabled = if value == "toggle" {
                            !style.fill_enabled
                        } else {
                            value == "true"
                        };
                        style.property_flags.insert(
                            acadrust::entities::table::CellStylePropertyFlags::BACKGROUND_COLOR,
                        );
                    }
                    return;
                }
                "tbl_cell_border_top"
                | "tbl_cell_border_right"
                | "tbl_cell_border_bottom"
                | "tbl_cell_border_left" => {
                    if !format_editable {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        use acadrust::entities::table::{
                            BorderPropertyFlags, CellEdgeFlags,
                        };
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        let (border, edge) = match field {
                            "tbl_cell_border_top" => (&mut style.top_border, CellEdgeFlags::TOP),
                            "tbl_cell_border_right" => {
                                (&mut style.right_border, CellEdgeFlags::RIGHT)
                            }
                            "tbl_cell_border_bottom" => {
                                (&mut style.bottom_border, CellEdgeFlags::BOTTOM)
                            }
                            _ => (&mut style.left_border, CellEdgeFlags::LEFT),
                        };
                        let visible = if value == "toggle" {
                            border.invisible
                        } else {
                            value == "true"
                        };
                        border.invisible = !visible;
                        border.override_flags.insert(BorderPropertyFlags::INVISIBILITY);
                        style.applied_border_edges.insert(edge);
                    }
                    return;
                }
                "tbl_cell_locked" => {
                    if state.intersects(
                        CellStateFlags::CONTENT_READ_ONLY | CellStateFlags::FORMAT_READ_ONLY,
                    ) {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        let locked = if value == "toggle" {
                            !cell.state.intersects(
                                CellStateFlags::CONTENT_LOCKED | CellStateFlags::FORMAT_LOCKED,
                            )
                        } else {
                            value == "true"
                        };
                        cell.state.set(CellStateFlags::CONTENT_LOCKED, locked);
                        cell.state.set(CellStateFlags::FORMAT_LOCKED, locked);
                    }
                    return;
                }
                _ => {}
            }
        }
        let Some(number) = parse_f64(value) else {
            if field == "tbl_break_direction" {
                if !self.break_options.contains(BreakOptionFlags::ENABLE_BREAKS)
                    || self
                        .break_options
                        .contains(BreakOptionFlags::ALLOW_MANUAL_POSITIONS)
                {
                    return;
                }
                self.break_flow_direction = match value.trim().to_ascii_uppercase().as_str() {
                    "LEFT" => acadrust::entities::table::BreakFlowDirection::Left,
                    "DOWN" | "VERTICAL" => {
                        acadrust::entities::table::BreakFlowDirection::Vertical
                    }
                    _ => acadrust::entities::table::BreakFlowDirection::Right,
                };
                self.break_ranges.clear();
            }
            return;
        };
        match field {
            "tbl_insert_x" => self.insertion_point.x = number,
            "tbl_insert_y" => self.insertion_point.y = number,
            "tbl_insert_z" => self.insertion_point.z = number,
            "tbl_direction" => {
                let radians = number.to_radians();
                self.horizontal_direction.x = radians.cos();
                self.horizontal_direction.y = radians.sin();
                self.horizontal_direction.z = 0.0;
            }
            "tbl_rows" => {
                let requested = number.round().max(1.0) as usize;
                while self.rows.len() < requested {
                    self.add_row();
                }
                while self.rows.len() > requested {
                    self.remove_row(self.rows.len() - 1);
                }
                self.break_ranges.clear();
            }
            "tbl_cols" => {
                let requested = number.round().max(1.0) as usize;
                let width = self.columns.last().map(|column| column.width).unwrap_or(2.0);
                while self.columns.len() < requested {
                    self.add_column(width);
                }
                while self.columns.len() > requested {
                    self.remove_column(self.columns.len() - 1);
                }
                self.break_ranges.clear();
            }
            "tbl_column_width" if number > 0.0 => {
                for column in &mut self.columns {
                    column.width = number;
                }
                self.break_ranges.clear();
            }
            "tbl_row_height" if number > 0.0 => {
                for row in &mut self.rows {
                    row.height = number;
                }
                self.break_ranges.clear();
            }
            "tbl_width" if number > 0.0 => {
                let current = self.total_width();
                if current > 1.0e-12 {
                    for column in &mut self.columns {
                        column.width *= number / current;
                    }
                }
                self.break_ranges.clear();
            }
            "tbl_height" if number > 0.0 => {
                let current = self.total_height();
                if current > 1.0e-12 {
                    for row in &mut self.rows {
                        row.height *= number / current;
                    }
                }
                self.break_ranges.clear();
            }
            "tbl_horizontal_margin" if number >= 0.0 => {
                let style = self.base_style.get_or_insert_with(Default::default);
                style.margin_left = number;
                style.margin_right = number;
                style.property_flags.insert(
                    CellStylePropertyFlags::MARGIN_LEFT | CellStylePropertyFlags::MARGIN_RIGHT,
                );
                let legacy = self.legacy_style_override.get_or_insert_with(Default::default);
                legacy.flags |= 0x0008;
                legacy.horizontal_cell_margin = Some(number);
                self.override_flag = true;
            }
            "tbl_vertical_margin" if number >= 0.0 => {
                let style = self.base_style.get_or_insert_with(Default::default);
                style.margin_top = number;
                style.margin_bottom = number;
                style.property_flags.insert(
                    CellStylePropertyFlags::MARGIN_TOP | CellStylePropertyFlags::MARGIN_BOTTOM,
                );
                let legacy = self.legacy_style_override.get_or_insert_with(Default::default);
                legacy.flags |= 0x0010;
                legacy.vertical_cell_margin = Some(number);
                self.override_flag = true;
            }
            "tbl_cell_text_height" if number > 0.0 => {
                if let Some((row, column)) = cell_position {
                    let state = self
                        .cell(row, column)
                        .map(|cell| cell.state)
                        .unwrap_or_default();
                    if state.intersects(
                        CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY,
                    ) {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style.text_height = number;
                        style.property_flags.insert(
                            acadrust::entities::table::CellStylePropertyFlags::TEXT_HEIGHT,
                        );
                    }
                }
            }
            "tbl_cell_margin_left"
            | "tbl_cell_margin_top"
            | "tbl_cell_margin_right"
            | "tbl_cell_margin_bottom"
                if number >= 0.0 =>
            {
                if let Some((row, column)) = cell_position {
                    let state = self
                        .cell(row, column)
                        .map(|cell| cell.state)
                        .unwrap_or_default();
                    if state.intersects(
                        CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY,
                    ) {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        let flag = match field {
                            "tbl_cell_margin_left" => {
                                style.margin_left = number;
                                CellStylePropertyFlags::MARGIN_LEFT
                            }
                            "tbl_cell_margin_top" => {
                                style.margin_top = number;
                                CellStylePropertyFlags::MARGIN_TOP
                            }
                            "tbl_cell_margin_right" => {
                                style.margin_right = number;
                                CellStylePropertyFlags::MARGIN_RIGHT
                            }
                            _ => {
                                style.margin_bottom = number;
                                CellStylePropertyFlags::MARGIN_BOTTOM
                            }
                        };
                        style.property_flags.insert(flag);
                    }
                }
            }
            "tbl_break_height"
                if number >= 0.0
                    && self.break_options.contains(BreakOptionFlags::ENABLE_BREAKS)
                    && !self
                        .break_options
                        .contains(BreakOptionFlags::ALLOW_MANUAL_HEIGHTS) =>
            {
                if self.break_data.is_empty() {
                    self.break_data.push(acadrust::entities::table::TableBreakData {
                        position: acadrust::types::Vector3::ZERO,
                        height: number,
                        flags: 0,
                    });
                } else {
                    for data in &mut self.break_data {
                        data.height = number;
                    }
                }
                self.break_ranges.clear();
            }
            "tbl_break_spacing"
                if number >= 0.0
                    && self.break_options.contains(BreakOptionFlags::ENABLE_BREAKS)
                    && !self
                        .break_options
                        .contains(BreakOptionFlags::ALLOW_MANUAL_POSITIONS) =>
            {
                self.break_spacing = number;
                self.break_ranges.clear();
            }
            _ => {}
        }
    }
}

impl Transformable for Table {
    fn apply_transform(&mut self, t: &EntityTransform) {
        transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            transform::reflect_xy_point(
                &mut entity.insertion_point.x,
                &mut entity.insertion_point.y,
                p1,
                p2,
            );
            // Reflect the horizontal direction by reflecting a tip point
            let mut tip_x = entity.insertion_point.x + entity.horizontal_direction.x;
            let mut tip_y = entity.insertion_point.y + entity.horizontal_direction.y;
            transform::reflect_xy_point(&mut tip_x, &mut tip_y, p1, p2);
            entity.horizontal_direction.x = tip_x - entity.insertion_point.x;
            entity.horizontal_direction.y = tip_y - entity.insertion_point.y;
        });
    }
}
