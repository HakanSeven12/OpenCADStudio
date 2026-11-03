use codec::entities::Table;
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{ro_prop as ro, square_grip};
use crate::entities::text_support::{
    layout_mtext, MTextRenderOpts, MTextVAnchor, ResolvedTextStyle,
};
use crate::entities::traits::{Grippable, PropertyEditable, RenderConvertible, Transformable};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;
use crate::scene::view::transform;
use crate::t;

/// Largest cell grid (rows × columns) the property editor may grow a
/// table to. Row/column counts are iteration counts for
/// `apply_geom_prop`'s grow loops, but `parse_f64` accepts anything up
/// to `MAX_TYPED_MAGNITUDE` (1e15) — an unclamped request kept pushing
/// rows/columns until the allocator failed.
const MAX_TABLE_CELLS: usize = 1_000_000;

thread_local! {
    static PROPERTY_CELL: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PROPERTY_CELL_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static PROPERTY_CELL_RANGE: std::cell::Cell<Option<(usize, usize, usize, usize)>> =
        const { std::cell::Cell::new(None) };
}

/// The picked cell range the panel edits (None: the table itself is picked).
pub fn set_prop_cell_range(range: Option<(usize, usize, usize, usize)>) {
    PROPERTY_CELL_RANGE.with(|cell| cell.set(range));
}

pub fn prop_cell_range() -> Option<(usize, usize, usize, usize)> {
    PROPERTY_CELL_RANGE.with(std::cell::Cell::get)
}

/// The colours the cell rows of the panel list by name.
pub const CELL_COLOR_NAMES: [&str; 9] = [
    "ByBlock", "ByLayer", "Red", "Yellow", "Green", "Cyan", "Blue", "Magenta", "White",
];

pub fn cell_color_name(color: codec::types::Color) -> String {
    use codec::types::Color;
    match color {
        Color::ByBlock => "ByBlock".into(),
        Color::ByLayer => "ByLayer".into(),
        Color::Index(i @ 1..=7) => CELL_COLOR_NAMES[i as usize + 1].into(),
        Color::Index(i) => format!("Color {i}"),
        Color::Rgb { r, g, b } => format!("{r},{g},{b}"),
        Color::None => "None".into(),
    }
}

pub fn parse_cell_color(text: &str) -> Option<codec::types::Color> {
    use codec::types::Color;
    let text = text.trim();
    if let Some(index) = CELL_COLOR_NAMES
        .iter()
        .position(|name| name.eq_ignore_ascii_case(text))
    {
        return Some(match index {
            0 => Color::ByBlock,
            1 => Color::ByLayer,
            i => Color::Index((i - 1) as u8),
        });
    }
    if let Some(index) = text
        .strip_prefix("Color ")
        .and_then(|n| n.trim().parse::<u8>().ok())
    {
        return Some(Color::Index(index));
    }
    let parts: Vec<u8> = text
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    (parts.len() == 3).then(|| Color::Rgb {
        r: parts[0],
        g: parts[1],
        b: parts[2],
    })
}

/// The data format categories a cell lists, as (name, value data type,
/// unit type) — the value type and unit bits the file stores.
pub const CELL_DATA_CATEGORIES: [(&str, i32, i32); 9] = [
    ("Angle", 2, 2),
    ("Currency", 2, 16),
    ("Date", 8, 0),
    ("Decimal Number", 2, 0),
    ("General", 512, 0),
    ("Percentage", 2, 32),
    ("Point", 16, 0),
    ("Text", 4, 0),
    ("Whole Number", 1, 0),
];

pub fn cell_data_category(data_type: i32, unit_type: i32) -> &'static str {
    // A 3D point (32) is a point as the 2D one is.
    let data_type = if data_type == 32 { 16 } else { data_type };
    CELL_DATA_CATEGORIES
        .iter()
        .find(|(_, d, u)| *d == data_type && (*u == unit_type || *d != 2))
        .or_else(|| CELL_DATA_CATEGORIES.iter().find(|(_, d, _)| *d == data_type))
        .map_or("General", |(name, _, _)| name)
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

fn v3(v: &codec::types::Vector3) -> Vec3 {
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
            if row >= owner_row && row <= row_end && column >= owner_column && column <= column_end
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

/// The cell style a cell draws with (0 Title, 1 Header, 2 Data): its own
/// cell style, else its column's, else its row's (ids 1/2/3 as the file
/// stores them), else the role of its row — title, header, then data.
pub fn cell_role_kind(
    table: &Table,
    title_suppressed: bool,
    header_suppressed: bool,
    row: usize,
    column: usize,
) -> u8 {
    let explicit = |id: i32| (1..=3).contains(&id).then(|| (id - 1) as u8);
    table
        .cell(row, column)
        .and_then(|cell| explicit(cell.style_id))
        .or_else(|| table.columns.get(column).and_then(|c| explicit(c.style_id)))
        .or_else(|| table.rows.get(row).and_then(|r| explicit(r.style_id)))
        .unwrap_or(match (title_suppressed, header_suppressed, row) {
            (false, _, 0) => 0,
            (false, false, 1) | (true, false, 0) => 1,
            _ => 2,
        })
}

/// The row role a row's index gives it, or its stored row style id.
pub fn row_role_kind(table: &Table, title_suppressed: bool, header_suppressed: bool, row: usize) -> u8 {
    table
        .rows
        .get(row)
        .and_then(|r| (1..=3).contains(&r.style_id).then(|| (r.style_id - 1) as u8))
        .unwrap_or(match (title_suppressed, header_suppressed, row) {
            (false, _, 0) => 0,
            (false, false, 1) | (true, false, 0) => 1,
            _ => 2,
        })
}

pub(crate) fn style_for_property<'a>(
    table: &'a Table,
    row: &'a codec::entities::table::TableRow,
    column: usize,
    cell: &'a codec::entities::table::TableCell,
    property: codec::entities::table::CellStylePropertyFlags,
) -> Option<&'a codec::entities::table::CellStyle> {
    // The cell's own overrides, as either layout (DXF or binary) states them.
    if let Some(style) = cell.style.as_ref().filter(|style| style.sets(property)) {
        return Some(style);
    }
    let column_style = table
        .columns
        .get(column)
        .and_then(|column| column.style.as_ref());
    // A column's style wins over its row's (measured with named cell styles
    // on both: the column's applies where they cross).
    for style in [column_style, row.style.as_ref(), table.base_style.as_ref()]
        .into_iter()
        .flatten()
    {
        if style.property_flags.contains(property) {
            return Some(style);
        }
    }
    None
}

/// A cell's text height: the content's own format when it overrides it,
/// else the cell, row, column or table style that sets it, else the row's
/// cell style (`row_height`), else the content's value.
fn cell_text_height(
    table: &Table,
    row: &codec::entities::table::TableRow,
    column: usize,
    cell: &codec::entities::table::TableCell,
    content: &codec::entities::table::CellContent,
    row_height: Option<f64>,
) -> Option<f64> {
    use codec::entities::table::CellStylePropertyFlags as P;
    let set = |h: f64| (h > 1e-6).then_some(h);
    let own = content.sets(P::TEXT_HEIGHT);
    own.then(|| set(content.text_height))
        .flatten()
        .or_else(|| {
            style_for_property(table, row, column, cell, P::TEXT_HEIGHT)
                .and_then(|s| set(s.text_height))
        })
        .or_else(|| row_height.and_then(set))
        .or_else(|| set(content.text_height))
}

/// A cell's text style: the content's own, else the cell, row, column or
/// table style that sets it (by handle, or by name for a cell override).
fn cell_text_style(
    document: &codec::Document,
    table: &Table,
    row: &codec::entities::table::TableRow,
    column: usize,
    cell: &codec::entities::table::TableCell,
    content: Option<&codec::entities::table::CellContent>,
) -> Option<codec::Handle> {
    use codec::entities::table::CellStylePropertyFlags as P;
    content.and_then(|c| c.text_style_handle).or_else(|| {
        let style = style_for_property(table, row, column, cell, P::TEXT_STYLE)?;
        style.text_style_handle.or_else(|| {
            let name = style.text_style_name.trim();
            document
                .text_styles
                .iter()
                .find(|s| !name.is_empty() && s.name.eq_ignore_ascii_case(name))
                .map(|s| s.handle)
        })
    })
}

/// The text style a table style's row style names: its handle, else the
/// style of its name (a DXF file names it).
fn row_style_text_style(
    document: &codec::Document,
    style: Option<&codec::objects::RowCellStyle>,
) -> Option<codec::Handle> {
    let style = style?;
    style.text_style_handle.or_else(|| {
        let name = style.text_style_name.trim();
        document
            .text_styles
            .iter()
            .find(|s| !name.is_empty() && s.name.eq_ignore_ascii_case(name))
            .map(|s| s.handle)
    })
}

/// The face each text style draws with, resolved the way text of that style
/// resolves anywhere else (a .shx file on disk before the built-in stroke
/// font of its name), so a table's cells match the text of its picture.
fn table_style_fonts(document: &codec::Document) -> rustc_hash::FxHashMap<codec::Handle, String> {
    document
        .text_styles
        .iter()
        .map(|style| {
            let font = crate::entities::text_support::resolve_text_style(&style.name, document)
                .font_name;
            (style.handle, font)
        })
        .collect()
}

fn style_for_border<'a>(
    table: &'a Table,
    row: &'a codec::entities::table::TableRow,
    column: usize,
    cell: &'a codec::entities::table::TableCell,
    edge: codec::entities::table::CellEdgeFlags,
) -> Option<&'a codec::entities::table::CellStyle> {
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
    table_style: Option<&codec::objects::TableStyle>,
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
    table_style: Option<&codec::objects::TableStyle>,
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
    table_style: Option<&codec::objects::TableStyle>,
) -> bool {
    use codec::entities::table::CellStylePropertyFlags;

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
                Some(codec::objects::TableFlowDirection::Up)
            )
        })
}

pub(crate) fn resolved_table_margins(
    table: &Table,
    table_style: Option<&codec::objects::TableStyle>,
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

/// The space between a broken table's fragments: the table's own, else five
/// and a half data text heights (what the table shows when it stores none).
pub(crate) fn resolved_break_spacing(table: &Table, document: &codec::Document) -> f64 {
    if table.break_spacing > 0.0 {
        return table.break_spacing;
    }
    let data_height = if table.rows.len() > 2 { data_text_height(table, document) } else { 0.18 };
    5.5 * data_height
}

/// The text height of the table style's data cells (0.18 without a style).
pub(crate) fn data_text_height(table: &Table, document: &codec::Document) -> f64 {
    table
        .table_style_handle
        .and_then(|handle| match document.objects.get(&handle) {
            Some(codec::objects::ObjectType::TableStyle(style)) => Some(style),
            _ => None,
        })
        .map_or(0.18, |style| style.data_row_style.text_height)
}

fn table_break_segments(
    table: &Table,
    document: &codec::Document,
    h: Vec3,
    down: Vec3,
    row_offsets: &[f32],
    scale: f32,
) -> Vec<TableBreakSegment> {
    use codec::entities::table::BreakOptionFlags;

    let insertion = v3(&table.insertion_point);
    if table.rows.is_empty() {
        return Vec::new();
    }
    if !table
        .break_options
        .contains(BreakOptionFlags::ENABLE_BREAKS)
    {
        return vec![TableBreakSegment {
            start_row: 0,
            end_row: table.rows.len() - 1,
            origin: insertion,
        }];
    }

    let offset_to_world = |offset: &codec::types::Vector3| insertion + v3(offset);
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
            let spacing = resolved_break_spacing(table, document) as f32 * scale;
            let horizontal_step = table.total_width() as f32 * scale + spacing;
            let vertical_step = if max_height.is_finite() {
                max_height + spacing
            } else {
                table.total_height() as f32 * scale + spacing
            };
            match table.break_flow_direction {
                codec::entities::table::BreakFlowDirection::Left => {
                    insertion - h * segment as f32 * horizontal_step
                }
                codec::entities::table::BreakFlowDirection::Vertical => {
                    insertion + down * segment as f32 * vertical_step
                }
                codec::entities::table::BreakFlowDirection::Right => {
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
    document: &codec::Document,
    row: usize,
    h: Vec3,
    down: Vec3,
    row_offsets: &[f32],
    scale: f32,
) -> (Vec3, f32) {
    let insertion = v3(&table.insertion_point);
    if let Some(segment) = table_break_segments(table, document, h, down, row_offsets, scale)
        .into_iter()
        .find(|segment| row >= segment.start_row && row <= segment.end_row)
    {
        return (
            segment.origin,
            row_offsets.get(row).copied().unwrap_or(0.0)
                - row_offsets.get(segment.start_row).copied().unwrap_or(0.0),
        );
    }

    (insertion, row_offsets.get(row).copied().unwrap_or(0.0))
}

fn break_frames_for_row(
    table: &Table,
    document: &codec::Document,
    row: usize,
    h: Vec3,
    down: Vec3,
    row_offsets: &[f32],
    scale: f32,
    top_label_rows: usize,
    bottom_label_rows: usize,
) -> Vec<(Vec3, f32)> {
    use codec::entities::table::BreakOptionFlags;

    let segments = table_break_segments(table, document, h, down, row_offsets, scale);
    let Some((segment_index, segment)) = segments
        .iter()
        .enumerate()
        .find(|(_, segment)| row >= segment.start_row && row <= segment.end_row)
    else {
        return vec![break_frame_for_row(table, document, row, h, down, row_offsets, scale)];
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
    let top_height = row_offsets.get(top_label_rows).copied().unwrap_or(0.0);
    let mut primary_top = row_offsets.get(row).copied().unwrap_or(0.0)
        - row_offsets.get(segment.start_row).copied().unwrap_or(0.0);
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

fn format_cell_value(value: &codec::entities::table::CellValue) -> String {
    let display = value.display();
    if !display.is_empty() {
        return display.to_string();
    }
    use codec::entities::table::CellValueType;
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
        return table
            .cell(row, column)
            .map(|cell| cell.text_value().to_string());
    }
    let open = body.find('(')?;
    let close = body.rfind(')')?;
    let function = body[..open].trim().to_ascii_uppercase();
    // `find('(')` scans forward while `rfind(')')` scans backward, so a
    // malformed body like `)(` orders them reversed; the safe `get` yields
    // None (fall back to the literal text) instead of a slice panic.
    let range = body.get(open + 1..close)?;
    let (first, last) = range.split_once(':').unwrap_or((range, range));
    let (r1, c1) = table_cell_reference(first)?;
    let (r2, c2) = table_cell_reference(last)?;
    let mut values = Vec::new();
    // A range can name any `usize` (`A1:A18446744073709551615`): `cell()`
    // returns None outside the table, so clamping the walk to the table's
    // own rows/columns collects exactly the same values — every skipped
    // cell would have been None — while keeping an unbounded walk off the
    // render path (1.8e19 iterations of `..=`).
    let row_end = r1.max(r2).min(table.rows.len().saturating_sub(1));
    let col_end = c1.max(c2).min(table.columns.len().saturating_sub(1));
    for row in r1.min(r2)..=row_end {
        for column in c1.min(c2)..=col_end {
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
    layout: codec::entities::table::ContentLayoutFlags,
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
    if layout.contains(codec::entities::table::ContentLayoutFlags::STACKED_VERTICAL) {
        for index in 0..sizes.len() {
            rows.push(vec![index]);
        }
    } else if layout.contains(codec::entities::table::ContentLayoutFlags::STACKED_HORIZONTAL) {
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
    let total_height =
        row_heights.iter().sum::<f32>() + vertical_spacing * rows.len().saturating_sub(1) as f32;
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
            result[index] = (
                x + width * 0.5,
                y + (row_heights[row_index] - height) * 0.5 + height * 0.5,
            );
            x += width + horizontal_spacing;
        }
        y += row_heights[row_index] + vertical_spacing;
    }
    result
}

fn content_display_value(
    document: &codec::Document,
    table: &Table,
    content: &codec::entities::table::CellContent,
) -> String {
    if let Some(handle) = content.field_handle {
        if let Some(value) =
            crate::entities::field::cell_text(document, handle, table.common.handle)
        {
            return value;
        }
        if let Some(codec::objects::ObjectType::Field(field)) = document.objects.get(&handle) {
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
    document: &codec::Document,
    table: &Table,
    row: usize,
    column: usize,
    cell: &codec::entities::table::TableCell,
    content_index: usize,
) -> Option<codec::entities::table::CellContentGeometry> {
    if let Some(geometry) = cell
        .contents
        .get(content_index)
        .and_then(|content| content.geometry.clone())
        .or_else(|| cell.geometries.get(content_index).cloned())
        .or_else(|| {
            (content_index == 0)
                .then(|| cell.geometry.clone())
                .flatten()
        })
    {
        return Some(geometry);
    }

    let handle = cell.geometry_handle?;
    let flat_index = row
        .saturating_mul(table.columns.len())
        .saturating_add(column);
    if let Some(codec::objects::ObjectType::DataObject(object)) = document.objects.get(&handle) {
        if let codec::objects::DataObjectData::TableGeometry(geometry) = &object.data {
            return geometry
                .cells
                .get(flat_index)
                .and_then(|cell| cell.geometry.get(content_index))
                .cloned();
        }
    }
    document.objects.values().find_map(|object| {
        let codec::objects::ObjectType::DataObject(object) = object else {
            return None;
        };
        let codec::objects::DataObjectData::TableGeometry(geometry) = &object.data else {
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
    document: &codec::Document,
    anno_scale: f32,
) -> Vec<codec::entities::Insert> {
    use codec::entities::table::ContentLayoutFlags;
    use codec::entities::{AttributeEntity, EntityType, Insert};
    use codec::types::Vector3;

    if table.rows.is_empty() || table.columns.is_empty() {
        return Vec::new();
    }
    let (h, down) = table_axes(table);
    let table_style = table.table_style_handle.and_then(|handle| {
        document
            .objects
            .get(&handle)
            .and_then(|object| match object {
                codec::objects::ObjectType::TableStyle(style) => Some(style),
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
            let (origin, row_top) =
                break_frame_for_row(table, document, row_index, h, flow, &row_offsets, anno_scale);
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
                codec::entities::table::CellStylePropertyFlags::CONTENT_LAYOUT,
            );
            let layout = layout_style
                .map(|style| style.layout_flags)
                .unwrap_or(ContentLayoutFlags::FLOW);
            let count = block_contents.len() as f32;

            for (slot_index, (content_index, content)) in block_contents.into_iter().enumerate() {
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
                    x = column_left + geometry.distance_to_center.x as f32 * anno_scale;
                    y = row_top - geometry.distance_to_center.y as f32 * anno_scale;
                    z = geometry.distance_to_center.z as f32 * anno_scale;
                } else if count > 1.0 {
                    let index = slot_index as f32;
                    if layout.contains(ContentLayoutFlags::STACKED_VERTICAL) {
                        y = row_top + (index + 0.5) * (row_bottom - row_top) / count;
                    } else {
                        x = column_left + (index + 0.5) * (column_right - column_left) / count;
                    }
                }
                let mut position =
                    origin + h * x + flow * y + v3(&table.normal).normalize_or(Vec3::Z) * z;
                let scale_style = style_for_property(
                    table,
                    row,
                    column_index,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::BLOCK_SCALE,
                );
                let style_scale = scale_style
                    .map(|style| style.scale)
                    .filter(|scale| scale.abs() > 1e-9)
                    .unwrap_or(1.0);
                let content_scale = if content.format_property_flags
                    & codec::entities::table::CellStylePropertyFlags::BLOCK_SCALE.bits() as i32
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
                    & codec::entities::table::CellStylePropertyFlags::BLOCK_SCALE.bits() as i32
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
                        codec::entities::table::CellStylePropertyFlags::AUTO_SCALE,
                    )
                    .is_some_and(|style| {
                        style
                            .property_flags
                            .contains(codec::entities::table::CellStylePropertyFlags::AUTO_SCALE)
                    });
                let mut block_min = Vector3::new(f64::MAX, f64::MAX, f64::MAX);
                let mut block_max = Vector3::new(f64::MIN, f64::MIN, f64::MIN);
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
                // A cell's margin: its style override, else the table's.
                let (table_h_margin, table_v_margin) = resolved_table_margins(
                    table,
                    table.table_style_handle.and_then(|handle| {
                        match document.objects.get(&handle) {
                            Some(codec::objects::ObjectType::TableStyle(style)) => Some(style),
                            _ => None,
                        }
                    }),
                );
                let margin = |property| {
                    use codec::entities::table::CellStylePropertyFlags as P;
                    let horizontal = matches!(property, P::MARGIN_LEFT | P::MARGIN_RIGHT);
                    style_for_property(table, row, column_index, cell, property)
                        .map(|style| match property {
                            P::MARGIN_LEFT => style.margin_left,
                            P::MARGIN_RIGHT => style.margin_right,
                            P::MARGIN_TOP => style.margin_top,
                            _ => style.margin_bottom,
                        })
                        .unwrap_or(if horizontal { table_h_margin } else { table_v_margin })
                        * anno_scale as f64
                };
                let margin_left = margin(codec::entities::table::CellStylePropertyFlags::MARGIN_LEFT);
                let margin_right = margin(codec::entities::table::CellStylePropertyFlags::MARGIN_RIGHT);
                let margin_top = margin(codec::entities::table::CellStylePropertyFlags::MARGIN_TOP);
                let margin_bottom =
                    margin(codec::entities::table::CellStylePropertyFlags::MARGIN_BOTTOM);
                if auto_scale && has_block_bounds {
                    let width = (block_max.x - block_min.x).abs();
                    let height = (block_max.y - block_min.y).abs();
                    let fit_x = ((column_right - column_left) as f64 - margin_left - margin_right)
                        .max(0.0)
                        / width.max(1e-9);
                    let fit_y = ((row_bottom - row_top) as f64 - margin_top - margin_bottom)
                        .max(0.0)
                        / height.max(1e-9);
                    scale *= fit_x.min(fit_y);
                }
                let rotation_style = style_for_property(
                    table,
                    row,
                    column_index,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::ROTATION,
                );
                let content_rotation_explicit = content.format_property_flags
                    & codec::entities::table::CellStylePropertyFlags::ROTATION.bits() as i32
                    != 0;
                let content_rotation = if content_rotation_explicit {
                    content.rotation
                } else {
                    rotation_style
                        .map(|style| style.rotation)
                        .unwrap_or(cell.rotation)
                };
                let rotation = table_rotation + content_rotation;
                // A lone block sits where the cell's alignment puts it: its
                // rotated, scaled extents against the cell's margins.
                if has_block_bounds && count <= 1.0 && resolved_content_geometry(
                    document,
                    table,
                    row_index,
                    column_index,
                    cell,
                    content_index,
                )
                .is_none()
                {
                    let alignment = style_for_property(
                        table,
                        row,
                        column_index,
                        cell,
                        codec::entities::table::CellStylePropertyFlags::ALIGNMENT,
                    )
                    .map(|style| style.alignment)
                    .unwrap_or(5);
                    let (sin, cos) = content_rotation.sin_cos();
                    let width = (block_max.x - block_min.x).abs() * scale;
                    let height = (block_max.y - block_min.y).abs() * scale;
                    let half_x = ((width * cos).abs() + (height * sin).abs()) * 0.5;
                    let half_y = ((width * sin).abs() + (height * cos).abs()) * 0.5;
                    let cell_x = match (alignment - 1).rem_euclid(3) {
                        0 => column_left as f64 + margin_left + half_x,
                        2 => column_right as f64 - margin_right - half_x,
                        _ => (column_left + column_right) as f64 * 0.5,
                    };
                    let cell_y = match (alignment - 1).div_euclid(3) {
                        0 => row_top as f64 + margin_top + half_y,
                        2 => row_bottom as f64 - margin_bottom - half_y,
                        _ => (row_top + row_bottom) as f64 * 0.5,
                    };
                    let shift = h * (cell_x as f32 - x) + flow * (cell_y as f32 - y);
                    position = position + shift;
                }
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
                let local_anchor = if has_block_bounds {
                    (block_min + block_max) * 0.5
                } else {
                    crate::scene::render_graph::block_base_point(document, &record.name)
                };
                let transformed_anchor =
                    crate::scene::render_graph::insert_transform(document, &insert)
                        .apply(local_anchor);
                insert.insert_point = insert.insert_point
                    + Vector3::new(
                        position.x as f64 - transformed_anchor.x,
                        position.y as f64 - transformed_anchor.y,
                        position.z as f64 - transformed_anchor.z,
                    );
                let attribute_definitions: Vec<_> = record
                    .entity_handles
                    .iter()
                    .filter_map(|handle| match document.get_entity(*handle) {
                        Some(EntityType::AttributeDefinition(definition)) => Some(definition),
                        _ => None,
                    })
                    .collect();
                for attribute in &content.attributes {
                    let definition = match document.get_entity(attribute.definition_handle) {
                        Some(EntityType::AttributeDefinition(definition)) => Some(definition),
                        _ => attribute_definitions
                            .get(attribute.index.max(0) as usize)
                            .copied(),
                    };
                    let Some(definition) = definition else {
                        continue;
                    };
                    let mut entity =
                        AttributeEntity::from_definition(definition, Some(attribute.value.clone()));
                    codec::Entity::apply_transform(
                        &mut entity,
                        &crate::scene::render_graph::insert_transform(document, &insert),
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
    fn to_render(&self, document: &codec::Document) -> Option<RenderEntity> {
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
        let lookup_style = |h: codec::Handle| -> Option<&codec::tables::TextStyle> {
            document.text_styles.iter().find(|s| s.handle == h)
        };
        let table_style: Option<&codec::objects::TableStyle> =
            self.table_style_handle.and_then(|h| {
                document.objects.get(&h).and_then(|obj| match obj {
                    codec::objects::ObjectType::TableStyle(ts) => Some(ts),
                    _ => None,
                })
            });
        let title_suppressed = resolved_title_suppressed(self, table_style);
        let header_suppressed = resolved_header_suppressed(self, table_style);

        let style_fonts = table_style_fonts(document);
        let font_for_handle = |handle: Option<codec::Handle>| -> Option<String> {
            handle.and_then(|h| style_fonts.get(&h).cloned())
        };
        // Build a ResolvedTextStyle for the cell — needed by the shared MText
        // pipeline so inline `\W`, `\Q`, etc. compose with the style baseline.
        let resolved_style_for_handle =
            |handle: Option<codec::Handle>, font_name: String| -> ResolvedTextStyle {
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
            let legacy_row_style: Option<&codec::objects::RowCellStyle> = table_style.map(|ts| {
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
            // A row naming a cell style of the table style draws with it.
            let named_row_style = table_style
                .filter(|_| row.style_id != 0)
                .and_then(|ts| ts.cell_style_by_id(row.style_id))
                .map(|cell| cell.cell_style.to_row_style());
            let row_style = named_row_style.as_ref().or(legacy_row_style);

            for (ci, cell) in row.cells.iter().enumerate() {
                let text = cell.text_value();
                if text.is_empty() {
                    continue;
                }
                // The cell style the cell draws with: its own id, else its
                // column's, else its row's; a named (non-role) style by id.
                let first_id = [
                    cell.style_id,
                    self.columns.get(ci).map_or(0, |column| column.style_id),
                    row.style_id,
                ]
                .into_iter()
                .find(|id| *id != 0);
                let named_cell = table_style
                    .zip(first_id.filter(|id| *id > 3))
                    .and_then(|(ts, id)| ts.cell_style_by_id(id))
                    .map(|named| named.cell_style.to_row_style());
                let row_style = named_cell
                    .as_ref()
                    .or_else(|| {
                        table_style.map(|ts| {
                            match cell_role_kind(self, title_suppressed, header_suppressed, ri, ci) {
                                0 => &ts.title_row_style,
                                1 => &ts.header_row_style,
                                _ => &ts.data_row_style,
                            }
                        })
                    })
                    .or(row_style);

                let col_left = col_offsets[ci];
                let col_width = self.columns.get(ci).map(|c| c.width as f32).unwrap_or(1.0);
                let col_right = col_left + col_width;

                // Resolve text height: content → cell-style → row-style → 0.18.
                let content = cell.contents.first();
                let cell_h = content
                    .and_then(|c| cell_text_height(self, row, ci, cell, c, row_style.map(|s| s.text_height)))
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
                let style_handle = cell_text_style(document, self, row, ci, cell, content)
                    .or_else(|| cell.style.as_ref().and_then(|s| s.text_style_handle))
                    .or_else(|| row_style_text_style(document, row_style));
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
                    layer_color: None,
                    // Not an MTEXT: text in a fixed box, never columnar.
                    columns: Default::default(),
                    value: text,
                    insertion: [text_origin.x as f64, text_origin.y as f64, origin.z as f64],
                    height: cell_h,
                    // Wraps within the cell like the drawn table does.
                    rect_w: (col_width - 2.0 * margin).max(0.0),
                    rotation: rot,
                    style: &resolved,
                    attach_h_anchor,
                    v_anchor,
                    line_spacing_factor: 1.0,
                    exact_line_spacing: false,
                    rectangle_height: 0.0,
                    vertical_text: false,
                    want_glyph_boxes: false,
                    break_long_words: true,
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
            .map(|[x, y, z]| [x as f64 + base[0], y as f64 + base[1], z as f64 + base[2]])
            .collect();
        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(pts_f64),
            snap_pts: vec![(
                glam::DVec3::new(
                    self.insertion_point.x,
                    self.insertion_point.y,
                    self.insertion_point.z,
                ),
                SnapHint::Insertion,
            )],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: fill_tris_f64,
        })
    }
}

/// Builds colored geometry for tables without a stored display block.
pub fn tessellate_table(
    tab: &Table,
    document: &codec::Document,
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
    use codec::types::Color;
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
    let lw_px = |w: &codec::types::LineWeight| -> f32 {
        match w {
            codec::types::LineWeight::Value(v) if *v >= 0 => (*v as f32 / 100.0) * (96.0 / 25.4),
            _ => line_weight_px,
        }
    };

    let (h, v_down) = table_axes(tab);
    // Flow direction: `Up` stacks rows upward instead of downward.
    let table_style: Option<&codec::objects::TableStyle> = tab.table_style_handle.and_then(|h| {
        document.objects.get(&h).and_then(|obj| match obj {
            codec::objects::ObjectType::TableStyle(ts) => Some(ts),
            _ => None,
        })
    });
    let flow_up = resolved_flow_up(tab, table_style);
    let v_flow = if flow_up { -v_down } else { v_down };

    let (col_offsets, row_offsets) = table_offsets(tab, anno_scale);

    let title_suppressed = resolved_title_suppressed(tab, table_style);
    let header_suppressed = resolved_header_suppressed(tab, table_style);
    let top_label_rows =
        (usize::from(!title_suppressed) + usize::from(!header_suppressed)).min(tab.rows.len());
    let bottom_label_rows = usize::from(!tab.rows.is_empty());
    let (horizontal_margin, vertical_margin) = resolved_table_margins(tab, table_style);
    let h_margin = horizontal_margin as f32 * anno_scale;
    let v_margin = vertical_margin as f32 * anno_scale;

    let lookup_style = |hh: codec::Handle| -> Option<&codec::tables::TextStyle> {
        document.text_styles.iter().find(|s| s.handle == hh)
    };
    let style_fonts = table_style_fonts(document);
    let font_for_handle = |handle: Option<codec::Handle>| -> Option<String> {
        handle.and_then(|h| style_fonts.get(&h).cloned())
    };
    let resolved_style_for_handle =
        |handle: Option<codec::Handle>, font_name: String| -> ResolvedTextStyle {
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
    let mut field_fills: Vec<[f32; 3]> = Vec::new();
    // SDF cell text: glyph quads (per-vertex coloured) collected across all
    // cells; emitted as one text-carrying wire at the end.
    let mut text_verts: Vec<crate::scene::pipeline::text_gpu::TextVertex> = Vec::new();
    let mut searchable: Vec<crate::scene::model::wire_model::SearchableTextRun> = Vec::new();
    #[allow(clippy::type_complexity)]
    let mut borders: HashMap<([u8; 4], u32, u64), ([f32; 4], f32, Vec<[f32; 3]>)> =
        HashMap::default();
    let mut emitted: rustc_hash::FxHashSet<(i32, i32, i32, i32, i32, i32)> =
        rustc_hash::FxHashSet::default();
    let sel_col = WireModel::SELECTED;

    #[allow(clippy::type_complexity)]
    let mut add_edge =
        |a: Vec3,
         b: Vec3,
         col: [f32; 4],
         lw: f32,
         linetype: u64,
         borders: &mut HashMap<([u8; 4], u32, u64), ([f32; 4], f32, Vec<[f32; 3]>)>| {
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
                .entry((key4(col), (lw * 100.0) as u32, linetype))
                .or_insert_with(|| (col, lw, Vec::new()));
            if !entry.2.is_empty() {
                entry.2.push([f32::NAN; 3]);
            }
            entry.2.push(rel(a));
            entry.2.push(rel(b));
        };

    let normal = v3(&tab.normal).normalize_or(Vec3::Z);
    // Each row's named cell style: the one its id names, else the title,
    // header or data style its place gives it.
    let row_cells: Vec<Option<&codec::objects::NamedTableCellStyle>> = tab
        .rows
        .iter()
        .enumerate()
        .map(|(ri, row)| {
            let ts = table_style?;
            let kind = match (title_suppressed, header_suppressed, ri) {
                (false, _, 0) => codec::objects::TITLE_CELL_STYLE,
                (false, false, 1) | (true, false, 0) => codec::objects::HEADER_CELL_STYLE,
                _ => codec::objects::DATA_CELL_STYLE,
            };
            (row.style_id != 0)
                .then(|| ts.cell_style_by_id(row.style_id))
                .flatten()
                .or_else(|| ts.cell_style(kind))
        })
        .collect();
    let row_projections: Vec<Option<codec::objects::RowCellStyle>> = row_cells
        .iter()
        .map(|cell| cell.map(|cell| cell.cell_style.to_row_style()))
        .collect();
    let column_count = tab.columns.len();
    let line_types = &document.line_types;
    for (ri, row) in tab.rows.iter().enumerate() {
        let legacy_row_style: Option<&codec::objects::RowCellStyle> = table_style.map(|ts| {
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
        let row_cell = row_cells[ri];
        let row_style: Option<&codec::objects::RowCellStyle> =
            row_projections[ri].as_ref().or(legacy_row_style);
        // The cell style's margins, where it has its own.
        let cell_margins = row_cell.and_then(|cell| cell.cell_style.margins());
        let h_margin = cell_margins.map_or(h_margin, |(h, _)| h as f32 * anno_scale);
        let v_margin = cell_margins.map_or(v_margin, |(_, v)| v as f32 * anno_scale);

        for (ci, cell) in row.cells.iter().enumerate() {
            let Some((_, _, row_end, column_end)) = merged_owner_and_span(tab, ri, ci) else {
                continue;
            };
            // A cell's own cell style id, else its column's (a column's
            // wins over its row's), names the cell style it draws with.
            let cell_projection = table_style.and_then(|ts| {
                let id = if cell.style_id != 0 {
                    cell.style_id
                } else {
                    tab.columns.get(ci).map_or(0, |column| column.style_id)
                };
                (id != 0)
                    .then(|| ts.cell_style_by_id(id))
                    .flatten()
                    .map(|named| named.cell_style.to_row_style())
            });
            let row_style = cell_projection.as_ref().or(row_style);
            let frames = break_frames_for_row(
                tab,
                document,
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
                let col_right = col_offsets.get(column_end + 1).copied().unwrap_or(col_left);
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
                    codec::entities::table::CellStylePropertyFlags::BACKGROUND_COLOR,
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
                // A row edge shared with a row of the same cell style is
                // its inside line; a column edge inside the table likewise.
                let same_above =
                    ri > 0 && row_cells[ri - 1].map(|c| c.id) == row_cell.map(|c| c.id);
                let same_below = row_end + 1 < tab.rows.len()
                    && row_cells[row_end + 1].map(|c| c.id) == row_cell.map(|c| c.id);
                let grid_edge = |which: u8| -> i32 {
                    match which {
                        0 if same_above => 2,
                        0 => 1,
                        2 if same_below => 2,
                        2 => 4,
                        1 if column_end + 1 < column_count => 16,
                        1 => 32,
                        _ if ci > 0 => 16,
                        _ => 8,
                    }
                };
                // (visible, colour, weight, double spacing, linetype).
                let edge = |which: u8| -> (bool, [f32; 4], f32, f32, u64) {
                    let edge_flag = match which {
                        0 => codec::entities::table::CellEdgeFlags::TOP,
                        1 => codec::entities::table::CellEdgeFlags::RIGHT,
                        2 => codec::entities::table::CellEdgeFlags::BOTTOM,
                        _ => codec::entities::table::CellEdgeFlags::LEFT,
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
                            if b.border_type == codec::entities::table::BorderType::Double {
                                b.double_spacing as f32 * anno_scale
                            } else {
                                0.0
                            },
                            b.line_type_handle.map_or(0, |handle| handle.value()),
                        )
                    } else if let Some(cs) = row_cell {
                        let mask = grid_edge(which);
                        let data = &cs.cell_style;
                        let grid = data.grid(mask);
                        let double = grid.is_some_and(|g| {
                            g.border.border_type == codec::objects::TableBorderType::Double
                        });
                        (
                            !data.edge_hidden(mask),
                            if selected {
                                sel_col
                            } else {
                                grid.map_or(entity_color, |g| {
                                    resolve_col(&g.border.color, entity_color)
                                })
                            },
                            grid.map_or(line_weight_px, |g| lw_px(&g.border.line_weight)),
                            if double {
                                grid.map_or(0.0, |g| g.border.double_line_spacing as f32)
                                    * anno_scale
                            } else {
                                0.0
                            },
                            grid.map_or(0, |g| g.line_type.value()),
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
                            0.0,
                            0,
                        )
                    } else {
                        (
                            true,
                            if selected { sel_col } else { entity_color },
                            line_weight_px,
                            0.0,
                            0,
                        )
                    }
                };
                // A double edge is two lines its spacing apart, about the
                // edge line.
                let mut draw_edge = |a: Vec3, b: Vec3, which: u8| {
                    let (visible, color, weight, spacing, linetype) = edge(which);
                    if !visible {
                        return;
                    }
                    if spacing > 0.0 {
                        let along = (b - a).normalize_or(h);
                        let offset = normal.cross(along) * (spacing * 0.5);
                        add_edge(a + offset, b + offset, color, weight, linetype, &mut borders);
                        add_edge(a - offset, b - offset, color, weight, linetype, &mut borders);
                    } else {
                        add_edge(a, b, color, weight, linetype, &mut borders);
                    }
                };
                draw_edge(tl, tr, 0);
                draw_edge(tr, br_, 1);
                draw_edge(bl, br_, 2);
                draw_edge(tl, bl, 3);

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
                    codec::entities::table::CellStylePropertyFlags::TEXT_HEIGHT,
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
                    codec::entities::table::CellStylePropertyFlags::MARGIN_LEFT,
                )
                .map(|style| style.margin_left as f32 * anno_scale)
                .unwrap_or(h_margin.max(fallback_text_height * 0.5));
                let fallback_margin_right = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::MARGIN_RIGHT,
                )
                .map(|style| style.margin_right as f32 * anno_scale)
                .unwrap_or(h_margin.max(fallback_text_height * 0.5));
                let fallback_margin_top = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::MARGIN_TOP,
                )
                .map(|style| style.margin_top as f32 * anno_scale)
                .unwrap_or(v_margin.max(fallback_text_height * 0.5));
                let fallback_margin_bottom = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::MARGIN_BOTTOM,
                )
                .map(|style| style.margin_bottom as f32 * anno_scale)
                .unwrap_or(v_margin.max(fallback_text_height * 0.5));
                let fallback_layout = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::CONTENT_LAYOUT,
                )
                .map(|style| style.layout_flags)
                .unwrap_or(codec::entities::table::ContentLayoutFlags::FLOW);
                let fallback_alignment = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::ALIGNMENT,
                )
                .map(|style| style.alignment)
                .or_else(|| row_style.map(|style| style.alignment as i32))
                .unwrap_or(5);
                let fallback_h_spacing = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::MARGIN_HORIZONTAL_SPACING,
                )
                .map(|style| style.horizontal_spacing as f32 * anno_scale)
                .unwrap_or(0.0);
                let fallback_v_spacing = style_for_property(
                    tab,
                    row,
                    ci,
                    cell,
                    codec::entities::table::CellStylePropertyFlags::MARGIN_VERTICAL_SPACING,
                )
                .map(|style| style.vertical_spacing as f32 * anno_scale)
                .unwrap_or(0.0);
                let fallback_sizes: Vec<_> = value_contents
                    .iter()
                    .map(|(_, content, text)| {
                        let height = cell_text_height(tab, row, ci, cell, content, row_style.map(|s| s.text_height))
                            .map_or(fallback_text_height, |h| h as f32 * anno_scale);
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
                    let cell_h = cell_text_height(tab, row, ci, cell, content, row_style.map(|s| s.text_height))
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
                        codec::entities::table::CellStylePropertyFlags::MARGIN_LEFT,
                    )
                    .map(|style| style.margin_left as f32 * anno_scale)
                    .filter(|margin| *margin > 1e-6)
                    .unwrap_or_else(|| h_margin.max(cell_h * 0.5));
                    let margin_right = style_for_property(
                        tab,
                        row,
                        ci,
                        cell,
                        codec::entities::table::CellStylePropertyFlags::MARGIN_RIGHT,
                    )
                    .map(|style| style.margin_right as f32 * anno_scale)
                    .filter(|margin| *margin > 1e-6)
                    .unwrap_or_else(|| h_margin.max(cell_h * 0.5));
                    let margin_top = style_for_property(
                        tab,
                        row,
                        ci,
                        cell,
                        codec::entities::table::CellStylePropertyFlags::MARGIN_TOP,
                    )
                    .map(|style| style.margin_top as f32 * anno_scale)
                    .filter(|margin| *margin > 1e-6)
                    .unwrap_or_else(|| v_margin.max(cell_h * 0.5));
                    let margin_bottom = style_for_property(
                        tab,
                        row,
                        ci,
                        cell,
                        codec::entities::table::CellStylePropertyFlags::MARGIN_BOTTOM,
                    )
                    .map(|style| style.margin_bottom as f32 * anno_scale)
                    .filter(|margin| *margin > 1e-6)
                    .unwrap_or_else(|| v_margin.max(cell_h * 0.5));
                    let style_handle = cell_text_style(document, tab, row, ci, cell, Some(content))
                        .or_else(|| row_style_text_style(document, row_style));
                    let font_owned =
                        font_for_handle(style_handle).unwrap_or_else(|| "txt".to_string());
                    let resolved = resolved_style_for_handle(style_handle, font_owned);
                    let align = cell_content_alignment(
                        tab,
                        row,
                        ci,
                        cell,
                        Some(content),
                        row_style.map(|style| style.alignment as i32),
                    );
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
                    if let Some(geometry) =
                        resolved_content_geometry(document, tab, ri, ci, cell, *content_index)
                    {
                        x_offset = col_left + geometry.distance_to_center.x as f32 * anno_scale;
                        y_offset = row_top - geometry.distance_to_center.y as f32 * anno_scale;
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
                    let to = origin + h * x_offset + v_flow * y_offset + normal * z_offset;
                    let content_rotation_explicit = content.format_property_flags
                        & codec::entities::table::CellStylePropertyFlags::ROTATION.bits() as i32
                        != 0;
                    let rot = if content_rotation_explicit {
                        content.rotation as f32
                    } else {
                        style_for_property(
                            tab,
                            row,
                            ci,
                            cell,
                            codec::entities::table::CellStylePropertyFlags::ROTATION,
                        )
                        .map(|style| style.rotation as f32)
                        .unwrap_or(cell.rotation as f32)
                    };
                    // FIELDDISPLAY boxes come from this same layout's cells.
                    let field_boxes =
                        content.field_handle.is_some() && crate::entities::field::display();
                    let layout = layout_mtext(&MTextRenderOpts {
                        layer_color: None,
                        columns: Default::default(),
                        value: text,
                        insertion: [to.x as f64, to.y as f64, to.z as f64],
                        height: cell_h,
                        // Wraps inside the cell's own margins (the table's
                        // where it sets none), as its row height is fitted.
                        rect_w: {
                            use codec::entities::table::CellStylePropertyFlags as P;
                            let side = |property: P| {
                                style_for_property(tab, row, ci, cell, property)
                                    .map(|style| {
                                        if property == P::MARGIN_LEFT {
                                            style.margin_left
                                        } else {
                                            style.margin_right
                                        }
                                    })
                                    .unwrap_or(horizontal_margin)
                                    as f32
                                    * anno_scale
                            };
                            (col_width - side(P::MARGIN_LEFT) - side(P::MARGIN_RIGHT)).max(0.0)
                        },
                        rotation: rot,
                        style: &resolved,
                        attach_h_anchor,
                        v_anchor,
                        line_spacing_factor: 1.0,
                        exact_line_spacing: false,
                        rectangle_height: 0.0,
                        vertical_text: false,
                        want_glyph_boxes: field_boxes,
                        break_long_words: true,
                    });
                    // FIELDDISPLAY: a field cell's text on a gray box, one per line,
                    // from the same layout's character cells (screen only).
                    if field_boxes {
                        let cells = &layout.glyph_boxes;
                        let (sin_r, cos_r) = (rot as f64).sin_cos();
                        let mut lines: Vec<[f64; 4]> = Vec::new();
                        for b in cells {
                            let [l, base, r, top] = b.local.map(|v| v as f64);
                            match lines.last_mut() {
                                Some(c) if (c[1] - base).abs() <= 1e-3 * (top - base).max(c[3]) => {
                                    c[0] = c[0].min(l);
                                    c[2] = c[2].max(r);
                                    c[3] = c[3].max(top - base);
                                }
                                _ => lines.push([l, base, r, top - base]),
                            }
                        }
                        for [l, base, r, h] in lines.into_iter().filter(|[l, _, r, _]| r > l) {
                            let (b, t) = (
                                base - crate::entities::field::BOX_BELOW * h,
                                base + crate::entities::field::BOX_ABOVE * h,
                            );
                            let w = |x: f64, y: f64| {
                                [
                                    (to.x as f64 + x * cos_r - y * sin_r) as f32,
                                    (to.y as f64 + x * sin_r + y * cos_r) as f32,
                                    to.z as f32,
                                ]
                            };
                            let q = [w(l, b), w(r, b), w(r, t), w(l, t)];
                            field_fills.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
                        }
                    }
                    let tcol = if selected {
                        sel_col
                    } else if !matches!(content.color, Color::ByLayer | Color::ByBlock) {
                        resolve_col(&content.color, entity_color)
                    } else if let Some(style) = style_for_property(
                        tab,
                        row,
                        ci,
                        cell,
                        codec::entities::table::CellStylePropertyFlags::CONTENT_COLOR,
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
                            if crate::scene::text::web_font::requires_shaping(&run.text) {
                                if !stroke.fill_tris.is_empty() {
                                    let entry = fills
                                        .entry(key4(tcol))
                                        .or_insert_with(|| (tcol, Vec::new()));
                                    for &[x, y] in &stroke.fill_tris {
                                        entry.1.push([
                                            stroke.origin[0] as f32 + x,
                                            stroke.origin[1] as f32 + y,
                                            to.z as f32,
                                        ]);
                                    }
                                }
                                if !stroke.strokes.is_empty() {
                                    let entry = borders
                                        .entry((key4(tcol), (line_weight_px * 100.0) as u32, 0))
                                        .or_insert_with(|| (tcol, line_weight_px, Vec::new()));
                                    for s in &stroke.strokes {
                                        if s.len() < 2 {
                                            continue;
                                        }
                                        if !entry.2.is_empty() {
                                            entry.2.push([f32::NAN; 3]);
                                        }
                                        for &[x, y] in s {
                                            entry.2.push([
                                                stroke.origin[0] as f32 + x,
                                                stroke.origin[1] as f32 + y,
                                                to.z as f32,
                                            ]);
                                        }
                                    }
                                }
                                continue;
                            }
                            let (quads, pen_adv) =
                                crate::scene::text::glyph_quads::layout_glyph_quads(
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
                            {
                                use crate::scene::model::wire_model::{
                                    clean_searchable_text, SearchableTextRun,
                                };
                                let visible = clean_searchable_text(&run.text);
                                if !visible.is_empty() {
                                    searchable.push(SearchableTextRun {
                                        text: visible,
                                        origin: [stroke.origin[0], stroke.origin[1], to.z as f64],
                                        height: run.height,
                                        rotation: run.rotation,
                                        color: tcol,
                                        bold: run.bold,
                                        font: run.font.clone(),
                                        adv_width: pen_adv,
                                    });
                                }
                            }
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
                bg_adapt: None,
                point_marker: None,
                taper_widths: Vec::new(),
                pattern_stations: Vec::new(),
                world_width: 0.0,
                depth_override: None,
                display_visible: true,
                snap_only: false,
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

                ..Default::default()
            }
        };

    let mut out: Vec<WireModel> = Vec::new();
    // Fills first (drawn under borders/text).
    for (_, (color, tris)) in fills {
        if !tris.is_empty() {
            out.push(mk(color, vec![], tris, 1.0));
        }
    }
    if !field_fills.is_empty() {
        let mut background = mk(crate::entities::field::BACKGROUND, vec![], field_fills, 1.0);
        background.plot_visible = false;
        out.push(background);
    }
    for ((_, _, linetype), (color, lw, pts)) in borders {
        if !pts.is_empty() {
            let mut wire = mk(color, pts, vec![], lw);
            if let Some(line_type) = line_types.iter().find(|lt| lt.handle.value() == linetype) {
                let (length, pattern) = crate::scene::view::render::resolve_pattern(
                    line_types,
                    &line_type.name,
                    document.header.linetype_scale as f32,
                );
                wire.pattern_length = length;
                wire.pattern = pattern;
            }
            out.push(wire);
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
        w.searchable_text = searchable;
        out.push(w);
    }
    out
}

impl Grippable for Table {
    /// The reference's table grips: the insertion point; arrows that
    /// stretch the width (top right), the height (bottom left) or both
    /// (bottom right); a square on every column line; and the arrow at the
    /// bottom middle that breaks the table.
    fn grips(&self) -> Vec<GripDef> {
        use crate::entities::common::oriented_triangle_grip;
        let (origin, horizontal, down) = grip_frame(self);
        let width = self.total_width();
        let height = self.total_height();
        let mut grips = vec![
            square_grip(0, origin),
            oriented_triangle_grip(1, origin + horizontal * width, horizontal),
            oriented_triangle_grip(2, origin + down * height, down),
            oriented_triangle_grip(
                3,
                origin + horizontal * width + down * height,
                (horizontal + down).normalize_or(down),
            ),
            oriented_triangle_grip(4, origin + horizontal * (width * 0.5) + down * height, down),
        ];
        let line_y = column_grip_offset(self);
        let mut offset = 0.0;
        for line in 0..=self.columns.len() {
            if line > 0 {
                offset += self.columns[line - 1].width;
            }
            grips.push(square_grip(
                100 + line,
                origin + horizontal * offset + down * line_y,
            ));
        }
        grips
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id >= CELL_GRIP_LEFT {
            let Some(range) = prop_cell_range() else {
                return;
            };
            let point = match apply {
                GripApply::Absolute(point) => point,
                GripApply::Translate(delta) => {
                    let Some(grip) = cell_range_grips(self, range)
                        .into_iter()
                        .find(|grip| grip.id == grip_id)
                    else {
                        return;
                    };
                    grip.world + delta
                }
            };
            apply_cell_grip(self, range, grip_id, point);
            // The saved picture is drawn again from the cells.
            self.block_record_handle = None;
            return;
        }
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
        let (origin, horizontal, down) = grip_frame(self);
        let along = (point - origin).dot(horizontal);
        let below = (point - origin).dot(down);
        let scale_columns = |table: &mut Self, width: f64| {
            let current = table.total_width();
            if current > 1.0e-12 {
                for column in &mut table.columns {
                    column.width *= width.max(1.0e-6) / current;
                }
            }
        };
        let scale_rows = |table: &mut Self, height: f64| {
            let current = table.total_height();
            if current > 1.0e-12 {
                for row in &mut table.rows {
                    row.height *= height.max(1.0e-6) / current;
                }
            }
        };
        match grip_id {
            1 => scale_columns(self, along),
            2 => scale_rows(self, below),
            3 => {
                scale_columns(self, along);
                scale_rows(self, below);
            }
            // Breaking: the table splits where the arrow is left.
            4 => {
                use codec::entities::table::BreakOptionFlags;
                if below > 1.0e-6 && below < self.total_height() {
                    self.break_options.insert(BreakOptionFlags::ENABLE_BREAKS);
                    self.apply_geom_prop("tbl_break_height", &below.to_string());
                }
            }
            // The left line moves the table's left edge, the others the
            // right edge of the column before them.
            100 => {
                if let Some(first) = self.columns.first_mut() {
                    let width = (first.width - along).max(1.0e-6);
                    let shift = horizontal * (first.width - width);
                    first.width = width;
                    self.insertion_point.x += shift.x;
                    self.insertion_point.y += shift.y;
                    self.insertion_point.z += shift.z;
                }
            }
            id if id > 100 => {
                let column = id - 101;
                let before: f64 = self.columns.iter().take(column).map(|c| c.width).sum();
                if let Some(definition) = self.columns.get_mut(column) {
                    definition.width = (along - before).max(1.0e-6);
                }
            }
            _ => {}
        }
        // Resize previews draw from the cells; commit refills the picture by
        // name, just as a cell edit does. A move keeps its authored picture.
        self.block_record_handle = None;
    }
}

/// Insertion point and the table's across and downward directions.
fn grip_frame(table: &Table) -> (glam::DVec3, glam::DVec3, glam::DVec3) {
    let origin = glam::DVec3::new(
        table.insertion_point.x,
        table.insertion_point.y,
        table.insertion_point.z,
    );
    let horizontal = glam::DVec3::new(
        table.horizontal_direction.x,
        table.horizontal_direction.y,
        table.horizontal_direction.z,
    )
    .normalize_or(glam::DVec3::X);
    let normal =
        glam::DVec3::new(table.normal.x, table.normal.y, table.normal.z).normalize_or(glam::DVec3::Z);
    let down = horizontal.cross(normal).normalize_or(glam::DVec3::NEG_Y);
    (origin, horizontal, down)
}

/// The column grips sit on the top edge of the first row whose cells do
/// not span columns (below a merged title).
fn column_grip_offset(table: &Table) -> f64 {
    let mut offset = 0.0;
    for (index, row) in table.rows.iter().enumerate() {
        let spans = table.merged_ranges.iter().any(|range| {
            range.top_row <= index && index <= range.bottom_row && range.right_col > range.left_col
        });
        if !spans {
            return offset;
        }
        offset += row.height;
    }
    0.0
}

impl PropertyEditable for Table {
    fn geometry_properties(&self, text_style_names: &[String]) -> Vec<PropSection> {
        use crate::entities::common::edit_prop as edit;
        use codec::entities::table::{BreakOptionFlags, CellStateFlags, CellStylePropertyFlags};
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
        let color_text = |color: codec::types::Color| match color {
            codec::types::Color::None => "None".to_string(),
            codec::types::Color::ByLayer => "ByLayer".to_string(),
            codec::types::Color::ByBlock => "ByBlock".to_string(),
            codec::types::Color::Index(index) => index.to_string(),
            codec::types::Color::Rgb { r, g, b } => format!("{r},{g},{b}"),
        };
        let color = |label: &str,
                     field: &'static str,
                     value: codec::types::Color,
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
        let break_height = self
            .break_data
            .first()
            .map(|data| data.height)
            .unwrap_or(0.0);
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
                cell.and_then(|cell| style_for_property(self, row, cell_column, cell, property))
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
        let content_editable =
            !state.intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY);
        let format_editable =
            !state.intersects(CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY);
        let immutable_lock =
            state.intersects(CellStateFlags::CONTENT_READ_ONLY | CellStateFlags::FORMAT_READ_ONLY);
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
        let horizontal_margin = legacy
            .and_then(|style| style.horizontal_cell_margin)
            .or_else(|| self.base_style.as_ref().map(|style| style.margin_left))
            .unwrap_or(0.06);
        let vertical_margin = legacy
            .and_then(|style| style.vertical_cell_margin)
            .or_else(|| self.base_style.as_ref().map(|style| style.margin_top))
            .unwrap_or(0.06);
        // Picked cells: the panel lists only the cells (as the reference
        // palette does), the first picked cell's values standing for all.
        if let Some(range) = prop_cell_range() {
            let (r0, c0, r1, c1) = widen_to_merged(self, range);
            let span_width: f64 = self.columns[c0..=c1.min(self.columns.len() - 1)]
                .iter()
                .map(|column| column.width)
                .sum();
            let span_height: f64 = self.rows[r0..=r1.min(self.rows.len() - 1)]
                .iter()
                .map(|row| row.height)
                .sum();
            let kind_name =
                |kind: u8| ["Title", "Header", "Data"][kind.min(2) as usize].to_string();
            let explicit = |id: i32| (1..=3).contains(&id).then(|| kind_name((id - 1) as u8));
            let roles = || vec!["Title".to_string(), "Header".into(), "Data".into()];
            let mut cell_styles = vec!["By Row/Column".to_string()];
            cell_styles.extend(roles());
            let mut column_styles = vec!["(none)".to_string()];
            column_styles.extend(roles());
            let fill = style_for(CellStylePropertyFlags::BACKGROUND_COLOR);
            let fill_name = match fill {
                Some(style) if style.fill_enabled => cell_color_name(style.background_color),
                _ => "None".to_string(),
            };
            let border = table_row.and_then(|row| {
                cell.and_then(|cell| {
                    style_for_border(
                        self,
                        row,
                        cell_column,
                        cell,
                        codec::entities::table::CellEdgeFlags::TOP,
                    )
                })
            });
            let border_color =
                border.map_or(codec::types::Color::ByBlock, |s| s.top_border.color);
            let border_lw = border.map_or(codec::types::LineWeight::ByBlock, |s| {
                s.top_border.line_weight
            });
            let named_colors = |current: String| {
                let mut options: Vec<String> =
                    CELL_COLOR_NAMES.iter().map(|name| name.to_string()).collect();
                if !options.contains(&current) {
                    options.push(current);
                }
                options
            };
            let mut fill_options = vec!["None".to_string()];
            fill_options.extend(named_colors(fill_name.clone()).into_iter().filter(|o| o != "None"));
            let locking = match (
                state.intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY),
                state.intersects(CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY),
            ) {
                (false, false) => "Unlocked",
                (true, false) => "Content locked",
                (false, true) => "Format locked",
                (true, true) => "Content and format locked",
            };
            let category = table_row
                .zip(cell)
                .and_then(|(row, cell)| cell_value_type(row, cell_column, self, cell))
                .map(|(data_type, unit_type)| cell_data_category(data_type, unit_type))
                .unwrap_or("General");
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
            let linked = cell
                .is_some_and(|cell| cell.has_linked_data || cell.data_link_handle.is_some());
            let alignments: Vec<String> = [
                "Top Left",
                "Top Center",
                "Top Right",
                "Middle Left",
                "Middle Center",
                "Middle Right",
                "Bottom Left",
                "Bottom Center",
                "Bottom Right",
            ]
            .iter()
            .map(|a| a.to_string())
            .collect();
            return vec![
                PropSection {
                    title: t!("Cell").into_owned(),
                    props: vec![
                        choice(
                            t!("Cell style").as_ref(),
                            "tbl_cell_style_name",
                            cell.and_then(|cell| explicit(cell.style_id))
                                .unwrap_or_else(|| "By Row/Column".to_string()),
                            cell_styles,
                            format_editable,
                        ),
                        choice(
                            t!("Row style").as_ref(),
                            "tbl_cell_row_style",
                            kind_name(row_role_kind(self, false, false, r0)),
                            roles(),
                            true,
                        ),
                        choice(
                            t!("Column style").as_ref(),
                            "tbl_cell_column_style",
                            self.columns
                                .get(c0)
                                .and_then(|column| explicit(column.style_id))
                                .unwrap_or_else(|| "(none)".to_string()),
                            column_styles,
                            true,
                        ),
                        number(t!("Cell width").as_ref(), "tbl_cell_width", span_width, true),
                        number(t!("Cell height").as_ref(), "tbl_cell_height", span_height, true),
                        choice(
                            t!("Alignment").as_ref(),
                            "tbl_cell_alignment",
                            alignment.to_string(),
                            alignments,
                            format_editable,
                        ),
                        choice(
                            t!("Background fill").as_ref(),
                            "tbl_cell_bg",
                            fill_name,
                            fill_options,
                            format_editable,
                        ),
                        choice(
                            t!("Border color").as_ref(),
                            "tbl_cell_border_color",
                            cell_color_name(border_color),
                            named_colors(cell_color_name(border_color)),
                            format_editable,
                        ),
                        Property {
                            label: t!("Border lineweight").into_owned(),
                            field: "tbl_cell_border_lw",
                            value: if format_editable {
                                PropValue::FieldLwChoice {
                                    field: "tbl_cell_border_lw",
                                    value: border_lw,
                                }
                            } else {
                                PropValue::ReadOnly(format!("{border_lw:?}"))
                            },
                        },
                        choice(
                            t!("Border linetype").as_ref(),
                            "tbl_cell_border_lt",
                            "ByBlock".to_string(),
                            vec!["ByBlock".into(), "ByLayer".into(), "Continuous".into()],
                            format_editable,
                        ),
                        number(
                            t!("Horizontal cell margin").as_ref(),
                            "tbl_cell_hmargin",
                            style_for(CellStylePropertyFlags::MARGIN_LEFT)
                                .map(|style| style.margin_left)
                                .unwrap_or(horizontal_margin),
                            format_editable,
                        ),
                        number(
                            t!("Vertical cell margin").as_ref(),
                            "tbl_cell_vmargin",
                            style_for(CellStylePropertyFlags::MARGIN_TOP)
                                .map(|style| style.margin_top)
                                .unwrap_or(vertical_margin),
                            format_editable,
                        ),
                        choice(
                            t!("Cell locking").as_ref(),
                            "tbl_cell_lock",
                            locking.to_string(),
                            vec![
                                "Unlocked".into(),
                                "Content locked".into(),
                                "Format locked".into(),
                                "Content and format locked".into(),
                            ],
                            !immutable_lock,
                        ),
                        ro(
                            t!("Cell data link").as_ref(),
                            "tbl_cell_link",
                            if linked { "Linked" } else { "Not linked" },
                        ),
                    ],
                },
                PropSection {
                    title: t!("Content").into_owned(),
                    props: vec![
                        ro(
                            t!("Cell type").as_ref(),
                            "tbl_cell_type",
                            match cell.map(|cell| cell.cell_type) {
                                Some(codec::entities::table::CellType::Block) => "Block",
                                _ => "Text",
                            },
                        ),
                        text(
                            t!("Contents").as_ref(),
                            "tbl_cell_text",
                            cell.map(|cell| cell.text_value().to_string())
                                .unwrap_or_default(),
                            content_editable,
                        ),
                        choice(
                            t!("Text style").as_ref(),
                            "tbl_cell_text_style",
                            text_style,
                            available_text_styles,
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
                        choice(
                            t!("Text rotation").as_ref(),
                            "tbl_cell_rotation",
                            format!("{rotation:.0}"),
                            vec!["0".into(), "90".into(), "180".into(), "270".into()],
                            format_editable,
                        ),
                        color(
                            t!("Text color").as_ref(),
                            "tbl_cell_content_color",
                            style_for(CellStylePropertyFlags::CONTENT_COLOR)
                                .map(|style| style.content_color)
                                .unwrap_or(codec::types::Color::ByBlock),
                            format_editable,
                        ),
                        choice(
                            t!("Data type").as_ref(),
                            "tbl_cell_data_category",
                            category.to_string(),
                            CELL_DATA_CATEGORIES
                                .iter()
                                .map(|(name, _, _)| name.to_string())
                                .collect(),
                            format_editable,
                        ),
                        text(
                            t!("Additional format").as_ref(),
                            "tbl_cell_format",
                            table_row
                                .zip(cell)
                                .map(|(row, cell)| cell_value_picture(row, cell_column, self, cell))
                                .unwrap_or_default(),
                            format_editable,
                        ),
                    ],
                },
            ];
        }
        // The table and its place, as the reference palette lists them: the
        // row and column counts are read-only, the size and flow editable.
        let mut sections = vec![
            PropSection {
                title: t!("Table").into_owned(),
                props: vec![
                    ro(t!("Table style").as_ref(), "tbl_style_handle", "Standard"),
                    ro(t!("Rows").as_ref(), "tbl_rows", self.rows.len().to_string()),
                    ro(
                        t!("Columns").as_ref(),
                        "tbl_cols",
                        self.columns.len().to_string(),
                    ),
                    choice(
                        t!("Direction").as_ref(),
                        "tbl_flow_direction",
                        if table_flow_up { "Up" } else { "Down" }.to_string(),
                        vec!["Down".into(), "Up".into()],
                        true,
                    ),
                    edit(t!("Table width").as_ref(), "tbl_width", self.total_width()),
                    edit(
                        t!("Table height").as_ref(),
                        "tbl_height",
                        self.total_height(),
                    ),
                ],
            },
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    edit(
                        t!("Position X").as_ref(),
                        "tbl_insert_x",
                        self.insertion_point.x,
                    ),
                    edit(
                        t!("Position Y").as_ref(),
                        "tbl_insert_y",
                        self.insertion_point.y,
                    ),
                    edit(
                        t!("Position Z").as_ref(),
                        "tbl_insert_z",
                        self.insertion_point.z,
                    ),
                ],
            },
        ];

        if prop_current_cell_active() {
            let cell_type = match cell.map(|cell| cell.cell_type) {
                Some(codec::entities::table::CellType::Block) => "Block",
                _ => "Text",
            };
            let data_type = cell
                .and_then(|cell| cell.contents.first())
                .map(|content| match content.value.value_type {
                    codec::entities::table::CellValueType::Long => "Integer",
                    codec::entities::table::CellValueType::Double => "Decimal",
                    codec::entities::table::CellValueType::Date => "Date",
                    codec::entities::table::CellValueType::Point2D => "Point 2D",
                    codec::entities::table::CellValueType::Point3D => "Point 3D",
                    codec::entities::table::CellValueType::Handle => "Handle",
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
                        cell.map(|cell| cell.text_value().to_string())
                            .unwrap_or_default(),
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
                            .unwrap_or(codec::types::Color::ByBlock),
                        format_editable,
                    ),
                    color(
                        t!("Background color").as_ref(),
                        "tbl_cell_background_color",
                        style_for(CellStylePropertyFlags::BACKGROUND_COLOR)
                            .map(|style| style.background_color)
                            .unwrap_or(codec::types::Color::ByBlock),
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
                                        codec::entities::table::CellEdgeFlags::TOP,
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
                                        codec::entities::table::CellEdgeFlags::RIGHT,
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
                                        codec::entities::table::CellEdgeFlags::BOTTOM,
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
                                        codec::entities::table::CellEdgeFlags::LEFT,
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

        sections.push(PropSection {
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
                        codec::entities::table::BreakFlowDirection::Right => "Right",
                        codec::entities::table::BreakFlowDirection::Left => "Left",
                        codec::entities::table::BreakFlowDirection::Vertical => "Down",
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
                    t!("Break height").as_ref(),
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
        });

        sections
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        use crate::entities::common::parse_f64;
        use codec::entities::table::{
            BreakOptionFlags, CellStateFlags, CellStyle, CellStylePropertyFlags,
        };
        // A picked range shows its whole width and height: a new value
        // scales the range's columns (rows) together to reach it.
        if matches!(field, "tbl_cell_width" | "tbl_cell_height")
            && !self.rows.is_empty()
            && !self.columns.is_empty()
        {
            if let (Some(range), Some(number)) =
                (prop_cell_range(), parse_f64(value).filter(|n| *n > 0.0))
            {
                let (r0, c0, r1, c1) = widen_to_merged(self, range);
                let sizes: Vec<&mut f64> = if field == "tbl_cell_width" {
                    self.columns[c0..=c1].iter_mut().map(|c| &mut c.width).collect()
                } else {
                    self.rows[r0..=r1].iter_mut().map(|r| &mut r.height).collect()
                };
                let span: f64 = sizes.iter().map(|size| **size).sum();
                if span > 1e-12 {
                    for size in sizes {
                        *size *= number / span;
                    }
                }
                return;
            }
        }
        // A picked range: the value goes into every picked cell.
        if field.starts_with("tbl_cell_") && value != "toggle" {
            if let Some(range) = prop_cell_range() {
                let (r0, c0, r1, c1) = widen_to_merged(self, range);
                let columns = self.columns.len();
                if (r0, c0) != (r1, c1) && columns > 0 {
                    let saved = prop_current_cell();
                    set_prop_cell_range(None);
                    for row in r0..=r1 {
                        for column in c0..=c1 {
                            let (o0, oc, _, _) = widen_to_merged(self, (row, column, row, column));
                            if (o0, oc) != (row, column) {
                                continue;
                            }
                            set_prop_current_cell(row * columns + column);
                            self.apply_geom_prop(field, value);
                        }
                    }
                    set_prop_current_cell(saved);
                    set_prop_cell_range(Some(range));
                    return;
                }
            }
        }
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
                let style = self
                    .legacy_style_override
                    .get_or_insert_with(Default::default);
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
                let legacy = self
                    .legacy_style_override
                    .get_or_insert_with(Default::default);
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
            let content_editable = !state
                .intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY);
            let format_editable =
                !state.intersects(CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY);
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
                            codec::entities::table::CellType::Block
                        } else {
                            codec::entities::table::CellType::Text
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
                        style
                            .property_flags
                            .insert(codec::entities::table::CellStylePropertyFlags::ALIGNMENT);
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
                            content.value.value_type =
                                match value.trim().to_ascii_uppercase().as_str() {
                                    "INTEGER" => codec::entities::table::CellValueType::Long,
                                    "DECIMAL" => codec::entities::table::CellValueType::Double,
                                    "DATE" => codec::entities::table::CellValueType::Date,
                                    "POINT 2D" => codec::entities::table::CellValueType::Point2D,
                                    "POINT 3D" => codec::entities::table::CellValueType::Point3D,
                                    "HANDLE" => codec::entities::table::CellValueType::Handle,
                                    _ => codec::entities::table::CellValueType::String,
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
                        release_content_format(cell, CellStylePropertyFlags::DATA_FORMAT);
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style.value_format = value.to_string();
                        style
                            .property_flags
                            .insert(codec::entities::table::CellStylePropertyFlags::DATA_FORMAT);
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
                            codec::entities::table::CellStylePropertyFlags::BACKGROUND_COLOR,
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
                        use codec::entities::table::{BorderPropertyFlags, CellEdgeFlags};
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
                        border
                            .override_flags
                            .insert(BorderPropertyFlags::INVISIBILITY);
                        style.applied_border_edges.insert(edge);
                    }
                    return;
                }
                "tbl_cell_style_name" | "tbl_cell_row_style" | "tbl_cell_column_style" => {
                    let id = match value.trim().to_ascii_lowercase().as_str() {
                        "title" => 1,
                        "header" => 2,
                        "data" => 3,
                        _ => 0,
                    };
                    match field {
                        "tbl_cell_style_name" if format_editable => {
                            if let Some(cell) = self.cell_mut(row, column) {
                                cell.style_id = id;
                            }
                        }
                        "tbl_cell_row_style" if id > 0 => {
                            if let Some(target) = self.rows.get_mut(row) {
                                target.style_id = id;
                            }
                        }
                        "tbl_cell_column_style" => {
                            if let Some(target) = self.columns.get_mut(column) {
                                target.style_id = id;
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                "tbl_cell_bg" => {
                    if !format_editable {
                        return;
                    }
                    let color = parse_cell_color(value);
                    if color.is_none() && !value.trim().eq_ignore_ascii_case("none") {
                        return;
                    }
                    if let Some(cell) = self.cell_mut(row, column) {
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style.fill_enabled = color.is_some();
                        if let Some(color) = color {
                            style.background_color = color;
                        }
                        style
                            .property_flags
                            .insert(CellStylePropertyFlags::BACKGROUND_COLOR);
                    }
                    return;
                }
                "tbl_cell_border_color" => {
                    if !format_editable {
                        return;
                    }
                    if let Some(color) = parse_cell_color(value) {
                        self.edit_cell_borders(row, column, |border| {
                            border.color = color;
                            codec::entities::table::BorderPropertyFlags::COLOR
                        });
                    }
                    return;
                }
                "tbl_cell_lock" => {
                    if state.intersects(
                        CellStateFlags::CONTENT_READ_ONLY | CellStateFlags::FORMAT_READ_ONLY,
                    ) {
                        return;
                    }
                    let text = value.trim().to_ascii_lowercase();
                    let both = text.starts_with("content and");
                    let content = both || text.starts_with("content");
                    let format = both || text.starts_with("format");
                    if let Some(cell) = self.cell_mut(row, column) {
                        cell.state.set(CellStateFlags::CONTENT_LOCKED, content);
                        cell.state.set(CellStateFlags::FORMAT_LOCKED, format);
                    }
                    return;
                }
                "tbl_cell_data_category" => {
                    if !format_editable {
                        return;
                    }
                    let Some((_, data_type, unit_type)) = CELL_DATA_CATEGORIES
                        .iter()
                        .find(|(name, _, _)| name.eq_ignore_ascii_case(value.trim()))
                    else {
                        return;
                    };
                    if let Some(cell) = self.cell_mut(row, column) {
                        release_content_format(cell, CellStylePropertyFlags::DATA_TYPE);
                        let style = cell.style.get_or_insert_with(CellStyle::new);
                        style.value_data_type = *data_type;
                        style.value_unit_type = *unit_type;
                        style
                            .property_flags
                            .insert(CellStylePropertyFlags::DATA_TYPE);
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
                    "LEFT" => codec::entities::table::BreakFlowDirection::Left,
                    "DOWN" | "VERTICAL" => codec::entities::table::BreakFlowDirection::Vertical,
                    _ => codec::entities::table::BreakFlowDirection::Right,
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
                // Grow only up to the cell budget (`requested` may be
                // 1e15): the grow loop is an iteration count, not
                // geometry, and used to push rows until the allocator
                // failed. Shrinking to a smaller requested count stays
                // exact — `grow_to` is never above `requested`, so the
                // two loops cannot fight.
                let grow_to = requested.min(MAX_TABLE_CELLS / self.columns.len().max(1));
                while self.rows.len() < grow_to {
                    self.add_row();
                }
                while self.rows.len() > requested {
                    self.remove_row(self.rows.len() - 1);
                }
                self.break_ranges.clear();
            }
            "tbl_cols" => {
                let requested = number.round().max(1.0) as usize;
                let grow_to = requested.min(MAX_TABLE_CELLS / self.rows.len().max(1));
                let width = self
                    .columns
                    .last()
                    .map(|column| column.width)
                    .unwrap_or(2.0);
                while self.columns.len() < grow_to {
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
                let legacy = self
                    .legacy_style_override
                    .get_or_insert_with(Default::default);
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
                let legacy = self
                    .legacy_style_override
                    .get_or_insert_with(Default::default);
                legacy.flags |= 0x0010;
                legacy.vertical_cell_margin = Some(number);
                self.override_flag = true;
            }
            "tbl_cell_width" | "tbl_cell_height" if number > 0.0 => {
                if let Some((row, column)) = cell_position {
                    let (r0, c0, r1, c1) = widen_to_merged(self, (row, column, row, column));
                    if field == "tbl_cell_width" {
                        let others: f64 =
                            self.columns[c0..c1].iter().map(|column| column.width).sum();
                        if let Some(last) = self.columns.get_mut(c1) {
                            last.width = (number - others).max(1e-6);
                        }
                    } else {
                        let others: f64 = self.rows[r0..r1].iter().map(|row| row.height).sum();
                        if let Some(last) = self.rows.get_mut(r1) {
                            last.height = (number - others).max(1e-6);
                        }
                    }
                }
            }
            "tbl_cell_hmargin" | "tbl_cell_vmargin" if number >= 0.0 => {
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
                        if field == "tbl_cell_hmargin" {
                            style.margin_left = number;
                            style.margin_right = number;
                            style.property_flags.insert(
                                CellStylePropertyFlags::MARGIN_LEFT
                                    | CellStylePropertyFlags::MARGIN_RIGHT,
                            );
                        } else {
                            style.margin_top = number;
                            style.margin_bottom = number;
                            style.property_flags.insert(
                                CellStylePropertyFlags::MARGIN_TOP
                                    | CellStylePropertyFlags::MARGIN_BOTTOM,
                            );
                        }
                    }
                }
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
                        style
                            .property_flags
                            .insert(codec::entities::table::CellStylePropertyFlags::TEXT_HEIGHT);
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
                    self.break_data
                        .push(codec::entities::table::TableBreakData {
                            position: codec::types::Vector3::ZERO,
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

/// Keep the per-cell merge dimensions (what DXF stores) in step with
/// `merged_ranges` (what DWG stores) after a scripted create or edit. Cells a
/// script did not touch keep their merge state when the ranges are unchanged.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(crate) fn normalize_scripted_table(old: Option<&Table>, new: &mut Table) {
    if old.is_some_and(|old| old.merged_ranges == new.merged_ranges) {
        return;
    }
    for row in &mut new.rows {
        for cell in &mut row.cells {
            cell.merge_width = 1;
            cell.merge_height = 1;
        }
    }
    let ranges = new.merged_ranges.clone();
    for range in &ranges {
        if let Some(cell) = new
            .rows
            .get_mut(range.top_row)
            .and_then(|row| row.cells.get_mut(range.left_col))
        {
            cell.merge_width = range.col_count() as i32;
            cell.merge_height = range.row_count() as i32;
        }
    }
}

/// The column label of a 0-based column: A … Z, AA, AB …
pub fn column_label(mut column: usize) -> String {
    let mut label = Vec::new();
    loop {
        label.push(b'A' + (column % 26) as u8);
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    label.reverse();
    String::from_utf8(label).unwrap_or_default()
}

/// A copy of a table keeps the text its fields show as plain values: the
/// FIELD objects belong to the table it was copied from.
pub fn detach_fields(table: &mut Table) {
    use codec::entities::table::TableCellContentType;
    let contents = table
        .rows
        .iter_mut()
        .flat_map(|row| row.cells.iter_mut())
        .flat_map(|cell| cell.contents.iter_mut());
    for content in contents {
        if content.field_handle.take().is_some()
            && content.content_type == TableCellContentType::Field
        {
            content.content_type = TableCellContentType::Value;
        }
    }
    table.field_handles.clear();
}

/// A rectangular cell range, inclusive, widened to whole merged cells.
pub fn widen_to_merged(
    table: &Table,
    (mut r0, mut c0, mut r1, mut c1): (usize, usize, usize, usize),
) -> (usize, usize, usize, usize) {
    let last_row = table.rows.len().saturating_sub(1);
    let last_column = table.columns.len().saturating_sub(1);
    (r0, r1) = (r0.min(last_row), r1.min(last_row));
    (c0, c1) = (c0.min(last_column), c1.min(last_column));
    loop {
        let before = (r0, c0, r1, c1);
        for row in r0..=r1.min(last_row) {
            for column in c0..=c1.min(last_column) {
                let Some(owner) = (0..=row)
                    .rev()
                    .flat_map(|r| (0..=column).rev().map(move |c| (r, c)))
                    .find_map(|(r, c)| {
                        merged_owner_and_span(table, r, c)
                            .filter(|&(_, _, re, ce)| row <= re && column <= ce)
                    })
                else {
                    continue;
                };
                r0 = r0.min(owner.0);
                c0 = c0.min(owner.1);
                r1 = r1.max(owner.2);
                c1 = c1.max(owner.3);
            }
        }
        if before == (r0, c0, r1, c1) {
            return (r0, c0, r1.min(last_row), c1.min(last_column));
        }
    }
}

/// The row and column labels a selected table shows round its grid: a band
/// of column letters before the first row and a band of row numbers left of
/// the first column, sized from the smallest row text height and the table
/// margins (band height: text height plus both vertical margins; band width:
/// three digits plus both horizontal margins).
pub struct TableIndicator {
    /// Each label cell: corners and whether it is in the selected range.
    pub boxes: Vec<([Vec3; 4], bool)>,
    /// The label cells' edges.
    pub lines: Vec<[Vec3; 2]>,
    /// Label text and its centre.
    pub labels: Vec<(String, Vec3)>,
    pub text_height: f32,
    pub style: ResolvedTextStyle,
    /// Text rotation (the table's horizontal direction).
    pub rotation: f32,
    /// The selected range's outline (corners in order).
    pub outline: Option<[Vec3; 4]>,
    /// The selected range's outline in every fragment it shows in.
    pub outlines: Vec<[Vec3; 4]>,
}

pub fn table_indicator(
    tab: &Table,
    document: &codec::Document,
    range: Option<(usize, usize, usize, usize)>,
) -> Option<TableIndicator> {
    if tab.rows.is_empty() || tab.columns.is_empty() {
        return None;
    }
    let table_style: Option<&codec::objects::TableStyle> = tab.table_style_handle.and_then(|h| {
        match document.objects.get(&h)? {
            codec::objects::ObjectType::TableStyle(ts) => Some(ts),
            _ => None,
        }
    });
    let (h, down) = table_axes(tab);
    let flow = if resolved_flow_up(tab, table_style) { -down } else { down };
    let (col_offsets, row_offsets) = table_offsets(tab, 1.0);
    let (hm, vm) = resolved_table_margins(tab, table_style);
    let title_suppressed = resolved_title_suppressed(tab, table_style);
    let header_suppressed = resolved_header_suppressed(tab, table_style);
    use codec::entities::table::CellStylePropertyFlags as P;
    let mut text_height = f64::MAX;
    let mut style_handle = None;
    for (ri, row) in tab.rows.iter().enumerate() {
        let legacy = table_style.map(|ts| match (title_suppressed, header_suppressed, ri) {
            (false, _, 0) => &ts.title_row_style,
            (false, false, 1) | (true, false, 0) => &ts.header_row_style,
            _ => &ts.data_row_style,
        });
        let height = row
            .cells
            .first()
            .and_then(|cell| style_for_property(tab, row, 0, cell, P::TEXT_HEIGHT))
            .map(|style| style.text_height)
            .filter(|h| *h > 1e-9)
            .or_else(|| legacy.map(|style| style.text_height));
        if let Some(height) = height.filter(|h| *h > 1e-9) {
            text_height = text_height.min(height);
        }
        style_handle = row
            .cells
            .first()
            .and_then(|cell| cell_text_style(document, tab, row, 0, cell, None))
            .or_else(|| legacy.and_then(|style| style.text_style_handle))
            .or(style_handle);
    }
    if !text_height.is_finite() || text_height == f64::MAX {
        text_height = 0.18;
    }
    let style_name = style_handle
        .and_then(|handle| document.text_styles.iter().find(|s| s.handle == handle))
        .map_or_else(|| "Standard".to_string(), |s| s.name.clone());
    let style = crate::entities::text_support::resolve_text_style(&style_name, document);
    let text_h = text_height as f32;
    // The row numbers' column: the ink width of three of the font's widest
    // digits, plus the cell margins (measured at height 4.5: txt 12, romans
    // 11.57, Arial 9.95 — twice that at height 9).
    let ink = crate::entities::text_support::text_local_bounds(
        &style.font_name,
        "888",
        text_h,
        style.width_factor,
        0.0,
    )
    .map(|bounds| bounds.ink_max[0] - bounds.ink_min[0])
    .filter(|width| *width > 0.0);
    let digits = ink.unwrap_or_else(|| layout_mtext(&MTextRenderOpts {
        layer_color: None,
        columns: Default::default(),
        value: "000",
        insertion: [0.0; 3],
        height: text_h,
        rect_w: 0.0,
        rotation: 0.0,
        style: &style,
        attach_h_anchor: 0.0,
        v_anchor: MTextVAnchor::Middle,
        line_spacing_factor: 1.0,
        exact_line_spacing: false,
        rectangle_height: 0.0,
        vertical_text: false,
        want_glyph_boxes: false,
        break_long_words: true,
    })
    .line_widths
    .first()
    .copied()
    .unwrap_or(text_h * 3.0));
    let band_h = text_h + 2.0 * vm as f32;
    let band_w = digits + 2.0 * hm as f32;
    let range = range.map(|r| widen_to_merged(tab, r));
    let in_range = |index: usize, row: bool| {
        range.is_some_and(|(r0, c0, r1, c1)| {
            if row { (r0..=r1).contains(&index) } else { (c0..=c1).contains(&index) }
        })
    };
    let width = *col_offsets.last()?;
    let mut indicator = TableIndicator {
        boxes: Vec::new(),
        lines: Vec::new(),
        labels: Vec::new(),
        text_height: text_h,
        style,
        rotation: h.y.atan2(h.x),
        outline: None,
        outlines: Vec::new(),
    };
    // Each fragment of a broken table carries its own strips: its rows
    // (repeated label rows included) and the column letters above it.
    let top_label_rows =
        (usize::from(!title_suppressed) + usize::from(!header_suppressed)).min(tab.rows.len());
    let bottom_label_rows = usize::from(!tab.rows.is_empty());
    let mut fragments: Vec<(Vec3, Vec<(usize, f32)>)> = Vec::new();
    for ri in 0..tab.rows.len() {
        for (origin, top) in break_frames_for_row(
            tab,
            document,
            ri,
            h,
            flow,
            &row_offsets,
            1.0,
            top_label_rows,
            bottom_label_rows,
        ) {
            match fragments
                .iter_mut()
                .find(|(o, _)| (*o - origin).length_squared() < 1e-8)
            {
                Some((_, rows)) => rows.push((ri, top)),
                None => fragments.push((origin, vec![(ri, top)])),
            }
        }
    }
    for (origin, rows) in &fragments {
        let at = |x: f32, y: f32| *origin + h * x + flow * y;
        let quad =
            |x0: f32, y0: f32, x1: f32, y1: f32| [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
        let bottom = rows
            .iter()
            .map(|&(ri, top)| top + (row_offsets[ri + 1] - row_offsets[ri]))
            .fold(0.0f32, f32::max);
        indicator.boxes.push((quad(-band_w, -band_h, 0.0, 0.0), false));
        for (ci, pair) in col_offsets.windows(2).enumerate() {
            indicator.boxes.push((quad(pair[0], -band_h, pair[1], 0.0), in_range(ci, false)));
            indicator.labels.push((column_label(ci), at((pair[0] + pair[1]) * 0.5, -band_h * 0.5)));
        }
        for &(ri, top) in rows {
            let bottom = top + (row_offsets[ri + 1] - row_offsets[ri]);
            indicator.boxes.push((quad(-band_w, top, 0.0, bottom), in_range(ri, true)));
            indicator.labels.push(((ri + 1).to_string(), at(-band_w * 0.5, (top + bottom) * 0.5)));
            indicator.lines.push([at(-band_w, bottom), at(0.0, bottom)]);
            indicator.lines.push([at(-band_w, top), at(0.0, top)]);
        }
        for x in std::iter::once(-band_w).chain(col_offsets.iter().copied()) {
            indicator.lines.push([at(x, -band_h), at(x, 0.0)]);
        }
        indicator.lines.push([at(-band_w, -band_h), at(0.0, -band_h)]);
        indicator.lines.push([at(-band_w, -band_h), at(width, -band_h)]);
        indicator.lines.push([at(-band_w, -band_h), at(-band_w, bottom)]);
        // The picked range's part in this fragment.
        if let Some((r0, c0, r1, c1)) = range {
            let picked: Vec<_> = rows.iter().filter(|(ri, _)| (r0..=r1).contains(ri)).collect();
            if let (Some(first), Some(last)) = (
                picked.iter().map(|(_, top)| *top).reduce(f32::min),
                picked
                    .iter()
                    .map(|&&(ri, top)| top + (row_offsets[ri + 1] - row_offsets[ri]))
                    .reduce(f32::max),
            ) {
                let outline = quad(col_offsets[c0], first, col_offsets[c1 + 1], last);
                indicator.outline.get_or_insert(outline);
                indicator.outlines.push(outline);
            }
        }
    }
    Some(indicator)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_table() -> Table {
        let mut table = Table::new(codec::types::Vector3::new(0.0, 0.0, 0.0), 2, 2);
        table.set_cell_text(0, 0, "10");
        table.set_cell_text(0, 1, "20");
        table.set_cell_text(1, 0, "30");
        table.set_cell_text(1, 1, "40");
        table
    }

    /// Delimiters located independently (`find('(')` forward, `rfind(')')`
    /// backward) must not produce a reversed slice: `=)(` used to build
    /// `&body[2..0]` — `slice index starts at 2 but ends at 0` — killing the
    /// render pass from a single pasted cell value.
    #[test]
    fn malformed_formula_is_ignored_instead_of_panicking() {
        let table = sample_table();
        for expression in ["=)(", "=a)b(c", "=)"] {
            assert_eq!(
                evaluate_table_formula(&table, expression),
                None,
                "{expression} should not evaluate"
            );
        }
    }

    /// Well-formed formulas keep evaluating across the whole file — the
    /// guard must only reject malformed delimiters.
    #[test]
    fn well_formed_formulas_still_evaluate() {
        let table = sample_table();
        assert_eq!(
            evaluate_table_formula(&table, "=SUM(A1:B2)"),
            Some("100".to_string())
        );
        assert_eq!(
            evaluate_table_formula(&table, "=AVERAGE(A1:B2)"),
            Some("25".to_string())
        );
        assert_eq!(
            evaluate_table_formula(&table, "=A1"),
            Some("10".to_string())
        );
        assert_eq!(evaluate_table_formula(&table, "not a formula"), None);
    }

    /// A cell range can name any `usize` — `A1:A18446744073709551615`
    /// parses to row ≈ 1.8e19. `cell()` returns None for out-of-bounds
    /// indices, but the range loop still *walked* every index on the
    /// render path. Clamping the walk to the table's own rows/columns
    /// must collect exactly the same values (each skipped cell would
    /// have been None) and stay instant; the elapsed guard keeps a
    /// future un-clamped loop from creeping back in.
    #[test]
    fn formula_range_evaluation_is_bounded_by_the_table() {
        let table = sample_table();
        let start = std::time::Instant::now();
        assert_eq!(
            evaluate_table_formula(&table, "=SUM(A1:A18446744073709551615)"),
            Some("40".to_string()),
            "A1:A… is column A only: 10 + 30"
        );
        assert_eq!(
            evaluate_table_formula(&table, "=COUNT(A1:A1000000000)"),
            Some("2".to_string()),
            "column A holds two numeric cells"
        );
        assert_eq!(
            evaluate_table_formula(&table, "=SUM(A1:Z999999999)"),
            Some("100".to_string())
        );
        assert_eq!(
            evaluate_table_formula(&table, "=AVERAGE(A1:ZZ18446744073709551615)"),
            Some("25".to_string())
        );
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "formula evaluation took {elapsed:?} — the range walk is unbounded again"
        );
    }

    /// A column count driven to `MAX_TYPED_MAGNITUDE` (1e15) grows one
    /// column per loop iteration until the allocator fails: the magnitude
    /// check bounds geometry, not iteration counts. Bound the cell grid
    /// instead — column growth is limited by the rows already present.
    #[test]
    fn tbl_column_count_stays_within_budget() {
        let mut table = Table::new(codec::types::Vector3::new(0.0, 0.0, 0.0), 1, 1);
        let cap = MAX_TABLE_CELLS / table.rows.len().max(1);
        table.apply_geom_prop("tbl_cols", &(cap + 10).to_string());
        assert!(
            table.columns.len() <= cap,
            "tbl_cols grew to {} columns (budget {} cells over {} rows)",
            table.columns.len(),
            MAX_TABLE_CELLS,
            table.rows.len()
        );
        table.apply_geom_prop("tbl_cols", "1000000000000000");
        assert!(
            table.columns.len() <= cap,
            "tbl_cols grew to {} columns from a hostile value",
            table.columns.len()
        );
    }

    /// Same budget applied to row growth: with 10 columns the table may
    /// hold `MAX_TABLE_CELLS / 10` rows, never the requested 1e15.
    #[test]
    fn tbl_row_count_stays_within_budget() {
        let mut table = Table::new(codec::types::Vector3::new(0.0, 0.0, 0.0), 1, 1);
        table.apply_geom_prop("tbl_cols", "10");
        let cap = MAX_TABLE_CELLS / table.columns.len().max(1);
        table.apply_geom_prop("tbl_rows", &(cap + 10).to_string());
        assert!(
            table.rows.len() <= cap,
            "tbl_rows grew to {} rows (budget {} cells over {} columns)",
            table.rows.len(),
            MAX_TABLE_CELLS,
            table.columns.len()
        );
        table.apply_geom_prop("tbl_rows", "1000000000000000");
        assert!(
            table.rows.len() <= cap,
            "tbl_rows grew to {} rows from a hostile value",
            table.rows.len()
        );
    }
}


/// Border edits on one cell: each of its four borders starts from the border
/// it draws with now (cell, row, column or table style), takes the change and
/// becomes the cell's own override for that edge.
pub trait TableCellBordersExt {
    fn edit_cell_borders(
        &mut self,
        row: usize,
        column: usize,
        edit: impl Fn(
            &mut codec::entities::table::CellBorder,
        ) -> codec::entities::table::BorderPropertyFlags,
    ) {
        use codec::entities::table::CellEdgeFlags as E;
        self.edit_cell_borders_on(row, column, E::TOP | E::RIGHT | E::BOTTOM | E::LEFT, edit);
    }

    /// The same on the cell's `edges` only.
    fn edit_cell_borders_on(
        &mut self,
        row: usize,
        column: usize,
        edges: codec::entities::table::CellEdgeFlags,
        edit: impl Fn(
            &mut codec::entities::table::CellBorder,
        ) -> codec::entities::table::BorderPropertyFlags,
    );
}

impl TableCellBordersExt for Table {
    fn edit_cell_borders_on(
        &mut self,
        row: usize,
        column: usize,
        wanted: codec::entities::table::CellEdgeFlags,
        edit: impl Fn(
            &mut codec::entities::table::CellBorder,
        ) -> codec::entities::table::BorderPropertyFlags,
    ) {
        use codec::entities::table::{CellBorder, CellEdgeFlags, CellStyle};
        let edges = [
            CellEdgeFlags::TOP,
            CellEdgeFlags::RIGHT,
            CellEdgeFlags::BOTTOM,
            CellEdgeFlags::LEFT,
        ];
        let pick = |style: &CellStyle, edge: CellEdgeFlags| -> CellBorder {
            if edge == CellEdgeFlags::TOP {
                style.top_border.clone()
            } else if edge == CellEdgeFlags::RIGHT {
                style.right_border.clone()
            } else if edge == CellEdgeFlags::BOTTOM {
                style.bottom_border.clone()
            } else {
                style.left_border.clone()
            }
        };
        let current: Vec<CellBorder> = {
            let (Some(table_row), Some(cell)) = (self.rows.get(row), self.cell(row, column)) else {
                return;
            };
            edges
                .iter()
                .map(|edge| {
                    style_for_border(self, table_row, column, cell, *edge)
                        .map(|style| pick(style, *edge))
                        .unwrap_or_else(CellBorder::new)
                })
                .collect()
        };
        let Some(cell) = self.cell_mut(row, column) else {
            return;
        };
        use codec::entities::table::CellStateFlags;
        if cell
            .state
            .intersects(CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY)
        {
            return;
        }
        let style = cell.style.get_or_insert_with(CellStyle::new);
        for (edge, mut border) in edges.into_iter().zip(current) {
            if !wanted.contains(edge) {
                continue;
            }
            let flag = edit(&mut border);
            border.override_flags.insert(flag);
            if edge == CellEdgeFlags::TOP {
                style.top_border = border;
            } else if edge == CellEdgeFlags::RIGHT {
                style.right_border = border;
            } else if edge == CellEdgeFlags::BOTTOM {
                style.bottom_border = border;
            } else {
                style.left_border = border;
            }
            style.applied_border_edges.insert(edge);
        }
    }
}

/// Grip ids of a picked cell range: its left, right, top and bottom edge
/// midpoints (column width / row height) and the fill diamond at the
/// bottom-right corner. Far above the column grips (`101 + column`).
pub const CELL_GRIP_LEFT: usize = 0x7fff_ff00;
pub const CELL_GRIP_RIGHT: usize = CELL_GRIP_LEFT + 1;
pub const CELL_GRIP_TOP: usize = CELL_GRIP_LEFT + 2;
pub const CELL_GRIP_BOTTOM: usize = CELL_GRIP_LEFT + 3;
pub const CELL_GRIP_FILL: usize = CELL_GRIP_LEFT + 4;

/// The picked range's edges along and down the table: (x0, x1, y0, y1).
fn cell_range_edges(table: &Table, range: (usize, usize, usize, usize)) -> (f64, f64, f64, f64) {
    let (r0, c0, r1, c1) = range;
    let x0: f64 = table.columns.iter().take(c0).map(|c| c.width).sum();
    let x1 = x0
        + table
            .columns
            .iter()
            .skip(c0)
            .take(c1 + 1 - c0)
            .map(|c| c.width)
            .sum::<f64>();
    let y0: f64 = table.rows.iter().take(r0).map(|r| r.height).sum();
    let y1 = y0
        + table
            .rows
            .iter()
            .skip(r0)
            .take(r1 + 1 - r0)
            .map(|r| r.height)
            .sum::<f64>();
    (x0, x1, y0, y1)
}

/// The grips a picked cell range shows.
pub fn cell_range_grips(table: &Table, range: (usize, usize, usize, usize)) -> Vec<GripDef> {
    if table.rows.is_empty() || table.columns.is_empty() {
        return Vec::new();
    }
    let range = widen_to_merged(table, range);
    let (origin, horizontal, down) = grip_frame(table);
    let (x0, x1, y0, y1) = cell_range_edges(table, range);
    let at = |x: f64, y: f64| origin + horizontal * x + down * y;
    let (xm, ym) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let mut fill = square_grip(CELL_GRIP_FILL, at(x1, y1));
    fill.shape = crate::scene::model::object::GripShape::Diamond;
    vec![
        square_grip(CELL_GRIP_LEFT, at(x0, ym)),
        square_grip(CELL_GRIP_RIGHT, at(x1, ym)),
        square_grip(CELL_GRIP_TOP, at(xm, y0)),
        square_grip(CELL_GRIP_BOTTOM, at(xm, y1)),
        fill,
    ]
}

/// A cell grip moved to `point`: an edge grip sizes the column / row on its
/// side of the range by how far the edge moves (the left and top edges as
/// well: the column / row narrows as its edge moves in, the others keep
/// their size), the diamond fills the cells it is dragged over from the
/// picked ones.
pub fn apply_cell_grip(
    table: &mut Table,
    range: (usize, usize, usize, usize),
    grip_id: usize,
    point: glam::DVec3,
) {
    const MIN: f64 = 1.0e-6;
    if table.rows.is_empty() || table.columns.is_empty() {
        return;
    }
    let range = widen_to_merged(table, range);
    let (r0, c0, r1, c1) = range;
    let (origin, horizontal, down) = grip_frame(table);
    let (x0, x1, y0, y1) = cell_range_edges(table, range);
    let along = (point - origin).dot(horizontal);
    let below = (point - origin).dot(down);
    match grip_id {
        CELL_GRIP_RIGHT => {
            let before = x1 - table.columns[c1].width;
            table.columns[c1].width = (along - before).max(MIN);
        }
        CELL_GRIP_BOTTOM => {
            let before = y1 - table.rows[r1].height;
            table.rows[r1].height = (below - before).max(MIN);
        }
        CELL_GRIP_LEFT => {
            table.columns[c0].width = (table.columns[c0].width - (along - x0)).max(MIN);
        }
        CELL_GRIP_TOP => {
            table.rows[r0].height = (table.rows[r0].height - (below - y0)).max(MIN);
        }
        CELL_GRIP_FILL => {
            // The cell under the point decides how far the fill reaches.
            let column_at = |x: f64| {
                let mut edge = 0.0;
                for (index, column) in table.columns.iter().enumerate() {
                    edge += column.width;
                    if x < edge {
                        return index;
                    }
                }
                table.columns.len() - 1
            };
            let row_at = |y: f64| {
                let mut edge = 0.0;
                for (index, row) in table.rows.iter().enumerate() {
                    edge += row.height;
                    if y < edge {
                        return index;
                    }
                }
                table.rows.len() - 1
            };
            let (to_row, to_column) = (row_at(below.max(0.0)), column_at(along.max(0.0)));
            let down_rows = to_row.saturating_sub(r1);
            let right_columns = to_column.saturating_sub(c1);
            if down_rows >= right_columns && down_rows > 0 {
                for column in c0..=c1 {
                    let sources: Vec<(usize, usize)> = (r0..=r1).map(|row| (row, column)).collect();
                    let targets: Vec<(usize, usize)> =
                        (r1 + 1..=to_row).map(|row| (row, column)).collect();
                    fill_series(table, &sources, &targets);
                }
            } else if right_columns > 0 {
                for row in r0..=r1 {
                    let sources: Vec<(usize, usize)> = (c0..=c1).map(|column| (row, column)).collect();
                    let targets: Vec<(usize, usize)> =
                        (c1 + 1..=to_column).map(|column| (row, column)).collect();
                    fill_series(table, &sources, &targets);
                }
            }
        }
        _ => {}
    }
}

/// Fill target cells from source cells: numeric values continue their series
/// (a single number counts up by one), anything else — text that reads as a
/// number too — repeats; the source cell formatting goes along. Locked
/// targets keep their content.
fn fill_series(table: &mut Table, sources: &[(usize, usize)], targets: &[(usize, usize)]) {
    use codec::entities::table::CellStateFlags;
    let cells: Vec<codec::entities::table::TableCell> = sources
        .iter()
        .filter_map(|&(row, column)| table.cell(row, column).cloned())
        .collect();
    if cells.is_empty() {
        return;
    }
    let numbers: Option<Vec<f64>> = cells
        .iter()
        .map(|cell| {
            let value = &cell.contents.first()?.value;
            matches!(
                value.value_type,
                codec::entities::table::CellValueType::Long
                    | codec::entities::table::CellValueType::Double
            )
            .then(|| cell.text_value().trim().parse::<f64>().ok())
            .flatten()
        })
        .collect();
    let step = match numbers.as_deref() {
        Some([.., a, b]) => Some(b - a),
        Some([_]) => Some(1.0),
        _ => None,
    };
    let decimals = cells
        .iter()
        .map(|cell| {
            let text = cell.text_value();
            text.trim()
                .split_once('.')
                .map_or(0, |(_, fraction)| fraction.len())
        })
        .max()
        .unwrap_or(0);
    for (index, &(row, column)) in targets.iter().enumerate() {
        let source = &cells[index % cells.len()];
        let text = match (&numbers, step) {
            (Some(values), Some(step)) => {
                let value = values[values.len() - 1] + step * (index as f64 + 1.0);
                format!("{value:.decimals$}")
            }
            _ => source.text_value().to_string(),
        };
        let Some(target) = table.cell_mut(row, column) else {
            continue;
        };
        if target
            .state
            .intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY)
        {
            continue;
        }
        if !target
            .state
            .intersects(CellStateFlags::FORMAT_LOCKED | CellStateFlags::FORMAT_READ_ONLY)
        {
            target.style = source.style.clone();
        }
        target.set_text(&text);
    }
}

/// The data type a cell's value reads as, as the Cell Data Format window
/// lists it (0 Angle, 1 Currency, 2 Date, 3 Decimal Number, 4 General,
/// 5 Percentage, 6 Point, 7 Text, 8 Whole Number), and its picture; `None`
/// when no style of the cell sets a data type.
pub(crate) fn cell_data_kind(
    table: &Table,
    row: usize,
    column: usize,
) -> Option<(usize, String)> {
    let table_row = table.rows.get(row)?;
    let cell = table_row.cells.get(column)?;
    let kind_of = |data_type: i32, unit_type: i32| {
        crate::ui::window::table_cell_dialog::FORMAT_KINDS
            .iter()
            .position(|name| *name == cell_data_category(data_type, unit_type))
    };
    let (data_type, unit_type) = cell_value_type(table_row, column, table, cell)?;
    Some((kind_of(data_type, unit_type)?, cell_value_picture(table_row, column, table, cell)))
}

/// Where a cell's content sits in the cell (1 top left … 9 bottom right):
/// the content's own alignment when its format sets one (a file stores
/// an unflagged default there), else the cell, column, row or table style
/// that sets it, else the row's cell style, else middle center.
pub(crate) fn cell_content_alignment(
    table: &Table,
    row: &codec::entities::table::TableRow,
    column: usize,
    cell: &codec::entities::table::TableCell,
    content: Option<&codec::entities::table::CellContent>,
    row_style: Option<i32>,
) -> i32 {
    use codec::entities::table::CellStylePropertyFlags as P;
    content
        .filter(|content| content.alignment != 0 && content.sets(P::ALIGNMENT))
        .map(|content| content.alignment)
        .or_else(|| {
            style_for_property(table, row, column, cell, P::ALIGNMENT)
                .map(|style| style.alignment)
                .filter(|alignment| *alignment != 0)
        })
        .or(row_style)
        .unwrap_or(5)
}

/// The data type and unit a cell's value is kept as: its content's own
/// format first (other drawings keep them there, and it is the cell's
/// most specific setting), else the cell, column, row or table style that
/// sets them.
pub(crate) fn cell_value_type(
    row: &codec::entities::table::TableRow,
    column: usize,
    table: &Table,
    cell: &codec::entities::table::TableCell,
) -> Option<(i32, i32)> {
    use codec::entities::table::CellStylePropertyFlags as P;
    match cell.contents.first().filter(|content| content.sets(P::DATA_TYPE)) {
        Some(content) => Some((content.format_value_data_type, content.format_value_unit_type)),
        None => style_for_property(table, row, column, cell, P::DATA_TYPE)
            .map(|style| (style.value_data_type, style.value_unit_type)),
    }
}

/// The picture a cell's value is shown with, found as [`cell_value_type`]
/// finds the type; empty when nothing sets one.
pub(crate) fn cell_value_picture(
    row: &codec::entities::table::TableRow,
    column: usize,
    table: &Table,
    cell: &codec::entities::table::TableCell,
) -> String {
    use codec::entities::table::CellStylePropertyFlags as P;
    match cell.contents.first().filter(|content| content.sets(P::DATA_FORMAT)) {
        Some(content) => content.value_format.clone(),
        None => style_for_property(table, row, column, cell, P::DATA_FORMAT)
            .map(|style| style.value_format.clone())
            .unwrap_or_default(),
    }
}

/// Before the cell's own style takes a data type or picture, the content
/// lets go of its own, which would otherwise still win.
pub(crate) fn release_content_format(
    cell: &mut codec::entities::table::TableCell,
    property: codec::entities::table::CellStylePropertyFlags,
) {
    let bits = property.bits() as i32;
    for content in &mut cell.contents {
        content.format_property_flags &= !bits;
        content.format_override_flags &= !bits;
    }
}

/// A typed date and its time, as a date cell reads it: `y-m-d` when the
/// first number has four digits, otherwise the day, month and year in the
/// order of the regional short date (so `11/2/2026` is 11 February where
/// the day comes first), any of `/` `.` `-` between them; an optional
/// `H:mm` or `H:mm:ss` after a space. The flag tells whether a time was
/// given.
fn parse_cell_date(text: &str, short_date: &str) -> Option<((i64, u32, u32, u32, u32, u32), bool)> {
    let mut words = text.split_whitespace();
    let date = words.next()?;
    let time = words.next();
    if words.next().is_some() {
        return None;
    }
    let parts: Vec<&str> = date.split(['/', '.', '-']).collect();
    let numbers: Vec<i64> = parts.iter().map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [a, b, c] = numbers[..] else { return None };
    let (y, m, d) = if parts[0].len() == 4 {
        (a, b, c)
    } else {
        // The order of d, M and y in the regional picture; month first
        // where it does not tell.
        let at = |letter: char| short_date.find(letter).unwrap_or(usize::MAX);
        let mut order = [('d', at('d')), ('M', at('M')), ('y', at('y'))];
        if order.iter().any(|(_, p)| *p == usize::MAX) {
            order = [('M', 0), ('d', 1), ('y', 2)];
        }
        order.sort_by_key(|(_, p)| *p);
        let at = |letter: char| order.iter().position(|(l, _)| *l == letter).unwrap_or(0);
        let value = |letter: char| [a, b, c][at(letter)];
        // A year of one or two digits: 00–49 is 2000–2049, 50–99 is
        // 1950–1999.
        let y = value('y');
        let y = match (parts[at('y')].len() <= 2, y) {
            (true, 0..=49) => y + 2000,
            (true, 50..=99) => y + 1900,
            _ => y,
        };
        (y, value('M'), value('d'))
    };
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        _ => return None,
    };
    if y <= 0 || !(1..=days).contains(&d) {
        return None;
    }
    let (h, mi, s) = match time {
        None => (0, 0, 0),
        Some(time) => {
            let t: Vec<u32> = time.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
            match t[..] {
                [h, mi] if h < 24 && mi < 60 => (h, mi, 0),
                [h, mi, s] if h < 24 && mi < 60 && s < 60 => (h, mi, s),
                _ => return None,
            }
        }
    };
    Some(((y, m as u32, d as u32, h, mi, s), time.is_some()))
}

/// A date cell value as the file keeps it: the date and time as sixteen
/// bytes (year, month, weekday from Sunday, day, hour, minute, second,
/// millisecond — two bytes each) and the text the picture shows; an empty
/// picture is the regional short date, or date and time.
fn date_cell_value(
    base: &codec::entities::table::CellValue,
    dt: (i64, u32, u32, u32, u32, u32),
    has_time: bool,
    picture: &str,
) -> codec::entities::table::CellValue {
    use codec::entities::table::{CellValueType as V, ValueUnitType as U};
    let (y, m, d, h, mi, s) = dt;
    let weekday = {
        // Sakamoto: 0 = Sunday.
        let t = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let yy = if m < 3 { y - 1 } else { y };
        (yy + yy / 4 - yy / 100 + yy / 400 + t[(m - 1) as usize] + d as i64).rem_euclid(7)
    };
    let picture = match picture {
        "" if has_time => "%c",
        "" => "%x",
        picture => picture,
    };
    let mut value = base.clone();
    value.value_type = V::Date;
    value.raw_type_code = V::Date as i32;
    value.unit_type = U::NoUnits;
    value.raw_unit_type_code = U::NoUnits as i32;
    value.flags = 4;
    value.data_size = 16;
    value.binary_value = [y, m as i64, weekday, d as i64, h as i64, mi as i64, s as i64, 0]
        .iter()
        .flat_map(|x| (*x as u16).to_le_bytes())
        .collect();
    value.format = picture.to_string();
    value.formatted_value =
        codec::fields::format_dt_in(dt, picture, &crate::entities::field::os_date_locale());
    value.text.clear();
    value.numeric_value = 0.0;
    value
}

/// The date and time a date cell value keeps.
fn stored_cell_date(value: &codec::entities::table::CellValue) -> Option<(i64, u32, u32, u32, u32, u32)> {
    let b = &value.binary_value;
    if value.value_type != codec::entities::table::CellValueType::Date || b.len() < 14 {
        return None;
    }
    let w = |k: usize| u16::from_le_bytes([b[2 * k], b[2 * k + 1]]) as u32;
    Some((w(0) as i64, w(1), w(3), w(4), w(5), w(6)))
}

/// A date cell that read as no date: emptied, typed as a date.
pub(crate) fn empty_date_cell_value() -> codec::entities::table::CellValue {
    use codec::entities::table::CellValueType as V;
    let mut value = codec::entities::table::CellValue::new();
    value.value_type = V::Date;
    value.raw_type_code = V::Date as i32;
    value.flags = 7;
    value
}

/// A typed point: two or three numbers apart by `,` `;` or `/`.
fn parse_cell_point(text: &str) -> Option<[f64; 3]> {
    let values: Vec<f64> = text
        .split([',', ';', '/'])
        .map(|p| p.trim().parse().ok())
        .collect::<Option<_>>()?;
    match values.as_slice() {
        [x, y] => Some([*x, *y, 0.0]),
        [x, y, z] => Some([*x, *y, *z]),
        _ => None,
    }
}

/// Whether text typed into a cell reads as the cell's data type: any text
/// fits a cell without one (General, Text); otherwise the text has to read
/// as the type, as the cell's editor checks it before it is put in.
pub(crate) fn typed_text_fits(
    document: &codec::Document,
    table: &Table,
    row: usize,
    column: usize,
    text: &str,
) -> bool {
    let Some((kind, picture)) = cell_data_kind(table, row, column) else {
        return true;
    };
    if matches!(kind, 4 | 7) || text.trim().is_empty() {
        return true;
    }
    // Numbers apart by commas are a point, never a number.
    if matches!(kind, 0 | 1 | 3 | 5 | 8) && text.contains(',') {
        return false;
    }
    let value = codec::entities::table::CellValue::text(text);
    matches!(typed_cell_value(document, &value, kind, &picture), TypedValue::Converted(_))
}

/// A typed number's unit sign: `$` before or after it (currency), `%`
/// after it (percentage). Returns the number's text, the unit and the
/// picture affixes (`[$,]`, `[,$]`, `[,%]`).
fn unit_sign(text: &str) -> (&str, i32, &'static str) {
    if let Some(body) = text.strip_prefix('$') {
        (body, 16, "[$,]")
    } else if let Some(body) = text.strip_suffix('$') {
        (body, 16, "[,$]")
    } else if let Some(body) = text.strip_suffix('%') {
        (body, 32, "[,%]")
    } else {
        (text, 0, "")
    }
}

/// The data type, unit and picture of a typed value (measured), which a
/// cell takes when its type changes to fit it and a General cell takes
/// when it is typed in: a whole number Whole, a number Decimal in the
/// current units (scientific when typed with an exponent), a number with a
/// `$` or `%` sign currency or percentage (`$12` Whole `%ps[$,]`, `12.5%`
/// Decimal `%lu2%pr2%ps[,%]`), two or three numbers apart by commas a 3D
/// point, a date Date, else Text.
pub(crate) fn entered_value_type(text: &str) -> (i32, i32, String) {
    let text = text.trim();
    let number = |part: &str| {
        let part = part.trim();
        let plain = !part.is_empty() && part.chars().all(|c| c.is_ascii_digit() || "+-.eE".contains(c));
        plain.then(|| part.parse::<f64>().ok()).flatten()
    };
    let (body, unit, affixes) = unit_sign(text);
    if unit != 0 {
        return match number(body) {
            Some(_) if body.parse::<i64>().is_ok() => (1, unit, format!("%ps{affixes}")),
            Some(_) => (2, unit, format!("%lu2%pr2%ps{affixes}")),
            None => (4, 0, String::new()),
        };
    }
    let numbers: Vec<Option<f64>> = text.split(',').map(number).collect();
    let (data_type, unit_type, picture) = match numbers.as_slice() {
        [Some(_)] if text.parse::<i64>().is_ok() => (1, 0, ""),
        [Some(_)] if text.contains(['e', 'E']) => (2, 0, "%lu1"),
        [Some(_)] => (2, 0, "%lu6"),
        [Some(_), Some(_)] | [Some(_), Some(_), Some(_)] => (32, 0, "%lu6%ls[;]"),
        _ if parse_cell_date(text, &crate::entities::field::os_date_locale().short_date).is_some() => {
            (8, 0, "%x")
        }
        _ => (4, 0, ""),
    };
    (data_type, unit_type, picture.to_string())
}

/// The data type and picture a cell takes when text is typed into it
/// (measured): a General cell takes the typed value's own (`12` Whole,
/// `abc` Text); a Currency, Decimal, Percentage or Whole cell without a
/// picture takes its type's default picture when the value comes with
/// another unit (`$12` in a Decimal cell shows `%lu6`, `12` in a Currency
/// cell the currency picture), and keeps its picture otherwise. `None`
/// leaves the cell's type as it is.
pub(crate) fn typed_entry_type(table: &Table, row: usize, column: usize, text: &str) -> Option<(i32, i32, String)> {
    if text.trim().is_empty() {
        return None;
    }
    let (kind, picture) = cell_data_kind(table, row, column).unwrap_or((4, String::new()));
    if kind == 4 {
        return Some(entered_value_type(text));
    }
    if !picture.is_empty() || !matches!(kind, 1 | 3 | 5 | 8) {
        return None;
    }
    let (_, cell_data_type, cell_unit) = CELL_DATA_CATEGORIES[kind];
    let default = match kind {
        1 => format!("%lu2%pr2%ps[{},]", crate::entities::field::os_currency_symbol()),
        3 => "%lu6".to_string(),
        5 => "%lu2%pr2%ps[,%]".to_string(),
        _ => String::new(),
    };
    (entered_value_type(text).1 != cell_unit && !default.is_empty()).then_some((cell_data_type, cell_unit, default))
}

/// Outcome of reading a cell value as a data type.
pub(crate) enum TypedValue {
    /// The value as the type, formatted by the picture.
    Converted(codec::entities::table::CellValue),
    /// The type has nothing to convert (General, an empty cell).
    Unchanged,
    /// The text does not read as the type (a date or point that is not one).
    Invalid,
}

/// A cell value read as the data type `kind` and formatted by `picture`:
/// typed numbers become numbers, an angle typed in degrees is kept in
/// radians, a whole number drops its fraction, a date or point is read
/// from its text.
pub(crate) fn typed_cell_value(
    document: &codec::Document,
    value: &codec::entities::table::CellValue,
    kind: usize,
    picture: &str,
) -> TypedValue {
    use codec::entities::table::{CellValueType as V, ValueUnitType as U};
    let shown = value.display().trim().to_string();
    if shown.is_empty() && !matches!(value.value_type, V::Long | V::Double | V::Point2D | V::Point3D) {
        return TypedValue::Unchanged;
    }
    let number = match value.value_type {
        V::Long | V::Double => Some(value.numeric_value),
        // A Currency, Decimal, Percentage or Whole cell reads a number with
        // a `$` or `%` sign (measured; an Angle cell does not).
        _ if kind != 0 => unit_sign(&shown).0.replace(',', "").parse::<f64>().ok(),
        _ => shown.replace(',', "").parse::<f64>().ok(),
    };
    let mut next = value.clone();
    next.format = picture.to_string();
    let mut point = value.point_value;
    let (value_type, unit, numeric, text) = match kind {
        0 => {
            let Some(number) = number else { return TypedValue::Unchanged };
            let radians = if value.value_type == V::Double && value.unit_type == U::Angle {
                number
            } else {
                number.to_radians()
            };
            // Without a picture an angle shows in radians, six decimals
            // (measured: 45 typed shows 0.785398; no picture is assigned).
            let text = if picture.is_empty() {
                format!("{radians:.6}")
            } else {
                document.format_with_picture(radians, picture)
            };
            (V::Double, U::Angle, radians, text)
        }
        1 | 3 | 5 => {
            let Some(number) = number else { return TypedValue::Unchanged };
            let unit = match kind {
                1 => U::Currency,
                5 => U::Percentage,
                _ => U::NoUnits,
            };
            let text = if picture.is_empty() && kind == 3 {
                // A number without a picture shows six decimals.
                format!("{number:.6}")
            } else if picture.is_empty() {
                shown.clone()
            } else {
                document.format_with_picture(number, picture)
            };
            (V::Double, unit, number, text)
        }
        // A date keeps its date and time, shown by the picture.
        2 => {
            let (dt, has_time) = match stored_cell_date(value) {
                Some(dt) => (dt, dt.3 + dt.4 + dt.5 > 0),
                None => match parse_cell_date(&shown, &crate::entities::field::os_date_locale().short_date) {
                    Some(parsed) => parsed,
                    None => return TypedValue::Invalid,
                },
            };
            return TypedValue::Converted(date_cell_value(value, dt, has_time, picture));
        }
        6 => {
            let p = match value.value_type {
                V::Point2D | V::Point3D => [point.x, point.y, point.z],
                _ => match parse_cell_point(&shown) {
                    Some(p) => p,
                    None => return TypedValue::Invalid,
                },
            };
            point = codec::types::Vector3::new(p[0], p[1], p[2]);
            (V::Point3D, U::NoUnits, 0.0, document.format_point_with_picture(p, picture))
        }
        7 => (V::String, U::NoUnits, 0.0, value.display().to_string()),
        8 => {
            let Some(number) = number else { return TypedValue::Unchanged };
            let whole = number.trunc();
            // A picture with only a sign (`%ps[$,]`) puts it around the number.
            let text = match picture.strip_prefix("%ps[").and_then(|p| p.strip_suffix(']')).and_then(|p| p.split_once(',')) {
                Some((prefix, suffix)) => format!("{prefix}{}{suffix}", whole as i64),
                None => format!("{}", whole as i64),
            };
            (V::Long, U::NoUnits, whole, text)
        }
        _ => return TypedValue::Unchanged,
    };
    next.value_type = value_type;
    next.raw_type_code = value_type as i32;
    next.unit_type = unit;
    next.raw_unit_type_code = unit as i32;
    next.numeric_value = numeric;
    next.point_value = point;
    next.formatted_value = if value_type == V::String { String::new() } else { text.clone() };
    next.text = text;
    TypedValue::Converted(next)
}

/// After an edit of a table: text typed into a cell that has a data type
/// is read as that type and shown in its format, and a row lower than the
/// wrapped text of its cells grows to hold it (rows never shrink — the
/// stored height is the row's minimum). Returns whether the table changed.
pub(crate) fn settle_table(table: &mut Table, document: &codec::Document) -> bool {
    use codec::entities::table::CellValueType as V;
    let mut changed = false;
    for row in 0..table.rows.len() {
        for column in 0..table.columns.len() {
            let Some((kind, picture)) = cell_data_kind(table, row, column) else {
                continue;
            };
            if matches!(kind, 4 | 7) {
                continue;
            }
            let Some(content) = table
                .cell(row, column)
                .and_then(|cell| cell.contents.first())
                .filter(|c| c.block_handle.is_none() && c.field_handle.is_none())
                .filter(|c| matches!(c.value.value_type, V::String | V::Unknown))
            else {
                continue;
            };
            if let TypedValue::Converted(value) =
                typed_cell_value(document, &content.value, kind, &picture)
            {
                if value.display() != content.value.display() || value.value_type != content.value.value_type {
                    if let Some(content) = table
                        .cell_mut(row, column)
                        .and_then(|cell| cell.contents.first_mut())
                    {
                        content.value = value;
                        changed = true;
                    }
                }
            }
        }
    }
    let needed = fitted_row_heights(table, document);
    for (row, height) in table.rows.iter_mut().zip(needed) {
        if height > row.height + 1e-9 {
            row.height = height;
            changed = true;
        }
    }
    changed
}

/// The laid-out text of a table cell: the cell (first row and column of a
/// merge, and its last row and column), the text block's height by its
/// lines, the width and height of the drawn ink, and the cell's left,
/// right, top and bottom margins.
struct CellTextExtent {
    row: usize,
    column: usize,
    last_row: usize,
    last_column: usize,
    height: f64,
    empty: bool,
    ink: [f64; 2],
    margins: [f64; 4],
}

/// Lays out the text of every text cell in its wrapped width. A cell
/// whose text content is empty has no ink: no width, one text height.
// ponytail: lays out every text cell; cache per cell if large tables slow edits.
fn cell_text_extents(table: &Table, document: &codec::Document) -> Vec<CellTextExtent> {
    use codec::entities::table::CellStylePropertyFlags as P;
    let table_style = table.table_style_handle.and_then(|handle| {
        match document.objects.get(&handle) {
            Some(codec::objects::ObjectType::TableStyle(style)) => Some(style),
            _ => None,
        }
    });
    let title_suppressed = resolved_title_suppressed(table, table_style);
    let header_suppressed = resolved_header_suppressed(table, table_style);
    let (h_margin, v_margin) = resolved_table_margins(table, table_style);
    let fonts = table_style_fonts(document);
    let mut extents = Vec::new();
    for (ri, row) in table.rows.iter().enumerate() {
        for (ci, cell) in row.cells.iter().enumerate() {
            let Some((r0, c0, r1, c1)) = merged_owner_and_span(table, ri, ci) else {
                continue;
            };
            if r0 != ri || c0 != ci {
                continue;
            }
            let Some(content) = cell
                .contents
                .first()
                .filter(|content| content.block_handle.is_none())
            else {
                continue;
            };
            let text = cell.text_value();
            let role = table_style.map(|ts| {
                match cell_role_kind(table, title_suppressed, header_suppressed, ri, ci) {
                    0 => &ts.title_row_style,
                    1 => &ts.header_row_style,
                    _ => &ts.data_row_style,
                }
            });
            let height = cell_text_height(table, row, ci, cell, content, role.map(|s| s.text_height))
                .unwrap_or(0.18);
            let style_handle = cell_text_style(document, table, row, ci, cell, Some(content))
                .or_else(|| row_style_text_style(document, role));
            let font = style_handle
                .and_then(|handle| fonts.get(&handle).cloned())
                .unwrap_or_else(|| "txt".to_string());
            let record = style_handle.and_then(|h| document.text_styles.iter().find(|s| s.handle == h));
            let resolved = ResolvedTextStyle {
                font_name: font,
                width_factor: record.map(|s| s.width_factor as f32).unwrap_or(1.0),
                oblique_angle: record.map(|s| s.oblique_angle as f32).unwrap_or(0.0),
                is_backward: false,
                is_upside_down: false,
                is_vertical: false,
            };
            let margin = |property: P, fallback: f64| {
                style_for_property(table, row, ci, cell, property)
                    .map(|style| match property {
                        P::MARGIN_LEFT => style.margin_left,
                        P::MARGIN_RIGHT => style.margin_right,
                        P::MARGIN_TOP => style.margin_top,
                        _ => style.margin_bottom,
                    })
                    .unwrap_or(fallback)
            };
            let width: f64 = table.columns[ci..=c1.min(table.columns.len() - 1)]
                .iter()
                .map(|column| column.width)
                .sum::<f64>()
                - margin(P::MARGIN_LEFT, h_margin)
                - margin(P::MARGIN_RIGHT, h_margin);
            let layout = layout_mtext(&MTextRenderOpts {
                layer_color: None,
                columns: Default::default(),
                value: &text,
                insertion: [0.0; 3],
                height: height as f32,
                rect_w: width.max(0.0) as f32,
                rotation: 0.0,
                style: &resolved,
                attach_h_anchor: 0.0,
                v_anchor: MTextVAnchor::Top,
                line_spacing_factor: 1.0,
                exact_line_spacing: false,
                rectangle_height: 0.0,
                vertical_text: false,
                want_glyph_boxes: false,
                break_long_words: true,
            });
            let lines = layout.line_count.max(1) as f64;
            let (mut low, mut high) = ([f64::MAX; 2], [f64::MIN; 2]);
            for run in &layout.strokes {
                for point in run.strokes.iter().flatten().chain(&run.fill_tris) {
                    for axis in 0..2 {
                        let at = run.origin[axis] + f64::from(point[axis]);
                        low[axis] = low[axis].min(at);
                        high[axis] = high[axis].max(at);
                    }
                }
            }
            // Ink left of the text start (a first glyph's negative side
            // bearing: Arial A, j) is not counted (measured: ABC 4130 units).
            // Upright, the box spans at least the lines' text height, from
            // the first line's top to the last baseline (txt "x": 4.5).
            let lines_height = height + (lines - 1.0) * layout.line_height as f64;
            let ink = if low[0] <= high[0] && !text.trim().is_empty() {
                [high[0] - low[0].max(0.0), high[1].max(0.0) - low[1].min(-lines_height)]
            } else {
                [0.0, height]
            };
            extents.push(CellTextExtent {
                row: ri,
                column: ci,
                last_row: r1,
                last_column: c1,
                height: height + (lines - 1.0) * layout.line_height as f64,
                empty: text.trim().is_empty(),
                ink,
                margins: [
                    margin(P::MARGIN_LEFT, h_margin),
                    margin(P::MARGIN_RIGHT, h_margin),
                    margin(P::MARGIN_TOP, v_margin),
                    margin(P::MARGIN_BOTTOM, v_margin),
                ],
            });
        }
    }
    extents
}

/// The height each row needs for the wrapped text of its single-row cells:
/// the text height, the line spacing for each further line, and the
/// cell's top and bottom margins (measured: 4.5 text, 1.5 margins — two
/// lines 15, four 30).
fn fitted_row_heights(table: &Table, document: &codec::Document) -> Vec<f64> {
    let mut needed = vec![0.0f64; table.rows.len()];
    for extent in cell_text_extents(table, document) {
        if extent.last_row == extent.row && !extent.empty {
            let need = extent.height + extent.margins[2] + extent.margins[3];
            needed[extent.row] = needed[extent.row].max(need);
        }
    }
    needed
}

/// Stores where each cell's content sits, as a DXF file keeps it beside
/// the table (measured): every cell its size with the margins around its
/// text; a text cell its text block too — the box of the drawn ink, the
/// offsets of the box's top left corner and of its alignment point from
/// the cell's top left corner, and the box's width and height. Cells
/// without content keep only the margins. Each cell's flag is 7 when the
/// table changed since it was opened or last saved (`modified`), else the
/// flag it holds (a file's 7 stays until the table changes and saves), 3
/// by default.
pub(crate) fn store_cell_geometry(table: &mut Table, document: &codec::Document, modified: bool) {
    let table_style = table.table_style_handle.and_then(|handle| {
        match document.objects.get(&handle) {
            Some(codec::objects::ObjectType::TableStyle(style)) => Some(style.clone()),
            _ => None,
        }
    });
    let (h_margin, v_margin) = resolved_table_margins(table, table_style.as_ref());
    let title_suppressed = resolved_title_suppressed(table, table_style.as_ref());
    let header_suppressed = resolved_header_suppressed(table, table_style.as_ref());
    let mut placed = Vec::new();
    for extent in cell_text_extents(table, document) {
        let row = &table.rows[extent.row];
        let cell = &row.cells[extent.column];
        let role = table_style.as_ref().map(|ts| {
            match cell_role_kind(table, title_suppressed, header_suppressed, extent.row, extent.column) {
                0 => ts.title_row_style.alignment as i32,
                1 => ts.header_row_style.alignment as i32,
                _ => ts.data_row_style.alignment as i32,
            }
        });
        let align = cell_content_alignment(table, row, extent.column, cell, cell.contents.first(), role);
        let last_column = extent.last_column.min(table.columns.len() - 1);
        let last_row = extent.last_row.min(table.rows.len() - 1);
        let cell_width: f64 = table.columns[extent.column..=last_column]
            .iter()
            .map(|column| column.width)
            .sum();
        let cell_height: f64 = table.rows[extent.row..=last_row]
            .iter()
            .map(|row| row.height)
            .sum();
        let [left, right, top, bottom] = extent.margins;
        // The stored width runs 0.1 past the ink of any text that has
        // width (measured alike for txt, romans and Arial at heights 2.5 to
        // 10; a single stroke, like txt's "i", stays 0).
        let [ink_width, h] = extent.ink;
        let w = if ink_width > 1e-9 { ink_width + 0.1 } else { 0.0 };
        let (column_step, row_step) = ((align - 1).rem_euclid(3), (align - 1).div_euclid(3));
        let x = match column_step {
            0 => left,
            1 => (cell_width - w) / 2.0,
            _ => cell_width - right - w,
        };
        let y = match row_step {
            0 => -top,
            1 => -(cell_height - h) / 2.0,
            _ => -(cell_height - bottom - h),
        };
        placed.push((
            extent.row,
            extent.column,
            w + left + right,
            h + top + bottom,
            codec::entities::table::CellContentGeometry {
                distance_to_top_left: codec::types::Vector3::new(x, y, 0.0),
                distance_to_center: codec::types::Vector3::new(
                    x + w * column_step as f64 / 2.0,
                    y - h * row_step as f64 / 2.0,
                    0.0,
                ),
                width: w,
                height: h,
                outer_width: w,
                outer_height: h,
                flags: 0,
            },
        ));
    }
    for row in &mut table.rows {
        for cell in &mut row.cells {
            cell.geometry_data_flag = if modified || cell.geometry_data_flag & 4 != 0 { 7 } else { 3 };
            cell.geometry_width_with_gap = 2.0 * h_margin;
            cell.geometry_height_with_gap = 2.0 * v_margin;
            cell.geometries.clear();
        }
    }
    for (ri, ci, width, height, geometry) in placed {
        if let Some(cell) = table.rows.get_mut(ri).and_then(|row| row.cells.get_mut(ci)) {
            cell.geometry_width_with_gap = width;
            cell.geometry_height_with_gap = height;
            cell.geometries = vec![geometry];
        }
    }
}

