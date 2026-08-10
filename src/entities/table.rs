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

fn style_for_property<'a>(
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
    for style in [cell.style.as_ref(), row.style.as_ref(), column_style]
        .into_iter()
        .flatten()
    {
        if style.property_flags.contains(property) {
            return Some(style);
        }
    }
    table.base_style.as_ref().or_else(|| {
        [cell.style.as_ref(), row.style.as_ref(), column_style]
            .into_iter()
            .flatten()
            .next()
    })
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

fn break_frame_for_row(
    table: &Table,
    row: usize,
    h: Vec3,
    down: Vec3,
    row_offsets: &[f32],
    scale: f32,
) -> (Vec3, f32) {
    use acadrust::entities::table::BreakOptionFlags;

    let insertion = v3(&table.insertion_point);
    let offset_to_world =
        |offset: &acadrust::types::Vector3| insertion + v3(offset);
    if !table.break_options.contains(BreakOptionFlags::ENABLE_BREAKS) {
        return (insertion, row_offsets.get(row).copied().unwrap_or(0.0));
    }

    if let Some(range) = table
        .break_ranges
        .iter()
        .find(|range| row as i32 >= range.start_row && row as i32 <= range.end_row)
    {
        let start = range.start_row.max(0) as usize;
        let position = offset_to_world(&range.position);
        let origin = if position.is_finite() {
            position
        } else {
            insertion
        };
        let top = row_offsets.get(row).copied().unwrap_or(0.0)
            - row_offsets.get(start).copied().unwrap_or(0.0);
        return (origin, top);
    }

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
        if row <= end_row {
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
                let horizontal_step =
                    table.total_width() as f32 * scale + spacing;
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
            return (
                origin,
                row_offsets.get(row).copied().unwrap_or(0.0) - start_offset,
            );
        }
        start_row = end_row.saturating_add(1);
        segment = segment.saturating_add(1);
    }

    (insertion, row_offsets.get(row).copied().unwrap_or(0.0))
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
    table_handle: acadrust::Handle,
    content: &acadrust::entities::table::CellContent,
) -> String {
    if let Some(handle) = content.field_handle {
        if let Some(value) =
            crate::entities::field::resolve_handle(document, handle, table_handle)
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
    format_cell_value(&content.value)
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
    let flow = if matches!(
        table_style.map(|style| style.flow_direction),
        Some(acadrust::objects::TableFlowDirection::Up)
    ) {
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
        let title_suppressed = table_style.map(|t| t.title_suppressed).unwrap_or(false);
        let header_suppressed = table_style.map(|t| t.header_suppressed).unwrap_or(false);

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

/// Coloured synthesis render for tables WITHOUT a baked block (e.g. tables
/// created in-app). Emits one `WireModel` per distinct colour for cell fills,
/// per-cell text, and grid borders, honouring every `TableStyle` /
/// `RowCellStyle` / per-cell `CellStyle` field: fill colour + enable, text
/// colour, border type/weight/colour/visibility (incl. inside borders),
/// margins, and flow direction. Imported tables keep using AutoCAD's baked
/// block (see scene/mod.rs).
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
    let flow_up = matches!(
        table_style.map(|t| t.flow_direction),
        Some(acadrust::objects::TableFlowDirection::Up)
    );
    let v_flow = if flow_up { -v_down } else { v_down };

    let (col_offsets, row_offsets) = table_offsets(tab, anno_scale);

    let title_suppressed = table_style.map(|t| t.title_suppressed).unwrap_or(false);
    let header_suppressed = table_style.map(|t| t.header_suppressed).unwrap_or(false);
    let h_margin = table_style
        .map(|t| t.horizontal_margin as f32)
        .unwrap_or(0.0) * anno_scale;
    let v_margin = table_style.map(|t| t.vertical_margin as f32).unwrap_or(0.0) * anno_scale;

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
            let (origin, row_top) =
                break_frame_for_row(tab, ri, h, v_flow, &row_offsets, anno_scale);
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
            let cell_style = cell
                .style
                .as_ref()
                .or(row.style.as_ref())
                .or_else(|| tab.columns.get(ci).and_then(|column| column.style.as_ref()))
                .or(tab.base_style.as_ref());

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
                if let Some(cs) = cell_style {
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
                    let text =
                        content_display_value(document, tab.common.handle, content);
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

    let name = tab.common.handle.value().to_string();
    let mk =
        |color: [f32; 4], points: Vec<[f32; 3]>, fill_tris: Vec<[f32; 3]>, lw: f32| -> WireModel {
            WireModel {
                taper_widths: Vec::new(),
                world_width: 0.0,
                depth_override: None,
                fill_is_3d: false,
                fill_is_2d_solid: false,
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
        vec![square_grip(
            0,
            glam::DVec3::new(
                self.insertion_point.x,
                self.insertion_point.y,
                self.insertion_point.z,
            ),
        )]
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id == 0 {
            match apply {
                GripApply::Translate(d) => {
                    self.insertion_point.x += d.x as f64;
                    self.insertion_point.y += d.y as f64;
                    self.insertion_point.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    self.insertion_point.x = p.x as f64;
                    self.insertion_point.y = p.y as f64;
                    self.insertion_point.z = p.z as f64;
                }
            }
        }
    }
}

impl PropertyEditable for Table {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        use crate::entities::common::edit_prop as edit;
        use acadrust::entities::table::BreakOptionFlags;

        let fmt_h = |oh: &Option<acadrust::types::Handle>| -> String {
            match oh {
                Some(h) if !h.is_null() => format!("{:X}", h.value()),
                _ => "(none)".to_string(),
            }
        };
        let toggle = |label: &str, field: &'static str, b: bool| -> Property {
            Property {
                label: label.into(),
                field,
                value: PropValue::BoolToggle { field, value: b },
            }
        };
        // Direction = angle of the horizontal direction vector in the XY plane.
        let direction_deg =
            (self.horizontal_direction.y.atan2(self.horizontal_direction.x)).to_degrees();
        let mut content_count = 0usize;
        let mut block_content_count = 0usize;
        let mut field_content_count = 0usize;
        let mut linked_cell_count = 0usize;
        for row in &self.rows {
            for cell in &row.cells {
                content_count += cell.contents.len();
                linked_cell_count += usize::from(cell.has_linked_data);
                for content in &cell.contents {
                    block_content_count += usize::from(content.block_handle.is_some());
                    field_content_count += usize::from(content.field_handle.is_some());
                }
            }
        }
        let break_heights = self
            .break_data
            .iter()
            .map(|data| format!("{:.4}", data.height))
            .collect::<Vec<_>>()
            .join(", ");

        vec![
            PropSection {
                title: t!("Table").into_owned(),
                props: vec![
                    ro(
                        t!("Table style").as_ref(),
                        "tbl_style_handle",
                        fmt_h(&self.table_style_handle),
                    ),
                    ro(t!("Rows").as_ref(), "tbl_rows", self.rows.len().to_string()),
                    ro(t!("Columns").as_ref(), "tbl_cols", self.columns.len().to_string()),
                    ro(t!("Contents").as_ref(), "tbl_contents", content_count.to_string()),
                    ro(
                        t!("Block contents").as_ref(),
                        "tbl_block_contents",
                        block_content_count.to_string(),
                    ),
                    ro(
                        t!("Field contents").as_ref(),
                        "tbl_field_contents",
                        field_content_count.to_string(),
                    ),
                    ro(
                        t!("Merged ranges").as_ref(),
                        "tbl_merged_ranges",
                        self.merged_ranges.len().to_string(),
                    ),
                    ro(
                        t!("Linked cells").as_ref(),
                        "tbl_linked_cells",
                        linked_cell_count.to_string(),
                    ),
                    ro(t!("Direction").as_ref(), "tbl_direction", format!("{:.4}", direction_deg)),
                    ro(
                        t!("Table width").as_ref(),
                        "tbl_width",
                        format!("{:.4}", self.total_width()),
                    ),
                    ro(
                        t!("Table height").as_ref(),
                        "tbl_height",
                        format!("{:.4}", self.total_height()),
                    ),
                ],
            },
            PropSection {
                title: t!("Table Breaks").into_owned(),
                props: vec![
                    toggle(
                        t!("Enabled").as_ref(),
                        "tbl_break_enabled",
                        self.break_options.contains(BreakOptionFlags::ENABLE_BREAKS),
                    ),
                    ro(
                        t!("Direction").as_ref(),
                        "tbl_break_direction",
                        format!("{:?}", self.break_flow_direction),
                    ),
                    toggle(
                        t!("Repeat top labels").as_ref(),
                        "tbl_break_repeat_top",
                        self.break_options
                            .contains(BreakOptionFlags::REPEAT_TOP_LABELS),
                    ),
                    toggle(
                        t!("Repeat bottom labels").as_ref(),
                        "tbl_break_repeat_bottom",
                        self.break_options
                            .contains(BreakOptionFlags::REPEAT_BOTTOM_LABELS),
                    ),
                    toggle(
                        t!("Manual positions").as_ref(),
                        "tbl_break_manual_positions",
                        self.break_options
                            .contains(BreakOptionFlags::ALLOW_MANUAL_POSITIONS),
                    ),
                    toggle(
                        t!("Manual heights").as_ref(),
                        "tbl_break_manual_heights",
                        self.break_options
                            .contains(BreakOptionFlags::ALLOW_MANUAL_HEIGHTS),
                    ),
                    ro(
                        t!("Segments").as_ref(),
                        "tbl_break_segments",
                        self.break_ranges.len().max(self.break_data.len()).to_string(),
                    ),
                    ro(t!("Break heights").as_ref(), "tbl_break_height", break_heights),
                    edit(t!("Spacing").as_ref(), "tbl_break_spacing", self.break_spacing),
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        use crate::entities::common::parse_f64;
        use acadrust::entities::table::BreakOptionFlags;
        let flag = match field {
            "tbl_break_enabled" => Some(BreakOptionFlags::ENABLE_BREAKS),
            "tbl_break_repeat_top" => Some(BreakOptionFlags::REPEAT_TOP_LABELS),
            "tbl_break_repeat_bottom" => Some(BreakOptionFlags::REPEAT_BOTTOM_LABELS),
            "tbl_break_manual_positions" => Some(BreakOptionFlags::ALLOW_MANUAL_POSITIONS),
            "tbl_break_manual_heights" => Some(BreakOptionFlags::ALLOW_MANUAL_HEIGHTS),
            _ => None,
        };
        if let Some(flag) = flag {
            let on = if value == "toggle" {
                !self.break_options.contains(flag)
            } else {
                value == "true"
            };
            self.break_options.set(flag, on);
            return;
        }
        if field == "tbl_break_spacing" {
            if let Some(v) = parse_f64(value) {
                self.break_spacing = v;
            }
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
