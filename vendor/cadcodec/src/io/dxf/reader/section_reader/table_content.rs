use crate::entities::table::{
    BorderPropertyFlags, BorderType, CellBorder, CellContent, CellContentGeometry, CellEdgeFlags,
    CellStateFlags, CellStyle, CellStylePropertyFlags, CellStyleType, CellType, CellValue,
    CellValueType, ContentLayoutFlags, TableAttribute, TableCell, TableCellContentType,
    TableColumn, TableCustomData, TableRow, ValueUnitType,
};
use crate::entities::Table;
use crate::error::Result;
use crate::types::{Color, LineWeight, Vector3};

use super::{append_hex_bytes, parse_dxf_handle, SectionReader};

impl<'a> SectionReader<'a> {
    fn read_table_value_dxf(&mut self) -> Result<CellValue> {
        let mut value = CellValue::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 304 {
                break;
            }
            match pair.code {
                93 => value.flags = pair.as_i32().unwrap_or(0),
                90 => {
                    value.raw_type_code = pair.as_i32().unwrap_or(0);
                    value.value_type = CellValueType::from(value.raw_type_code as u32);
                }
                91 => value.numeric_value = pair.as_i32().unwrap_or(0) as f64,
                140 => value.numeric_value = pair.as_double().unwrap_or(0.0),
                1 | 2 => value.text.push_str(&pair.value_string),
                92 => value.data_size = pair.as_i32().unwrap_or(0),
                11 => value.point_value.x = pair.as_double().unwrap_or(0.0),
                21 => value.point_value.y = pair.as_double().unwrap_or(0.0),
                31 => value.point_value.z = pair.as_double().unwrap_or(0.0),
                94 => {
                    value.raw_unit_type_code = pair.as_i32().unwrap_or(0);
                    value.unit_type = ValueUnitType::from(value.raw_unit_type_code as u32);
                }
                300 => value.format = pair.value_string.clone(),
                302 if value.raw_unit_type_code != 12 => {
                    value.formatted_value = pair.value_string.clone();
                }
                310..=319 => {
                    append_hex_bytes(&mut value.binary_value, &pair.value_string);
                }
                330 => {
                    let handle = parse_dxf_handle(&pair.value_string);
                    value.handle_value = (!handle.is_null()).then_some(handle);
                }
                _ => {}
            }
        }
        Ok(value)
    }

    fn read_table_custom_data_dxf(&mut self) -> Result<Vec<TableCustomData>> {
        let mut values = Vec::new();
        let mut name = String::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 309 && pair.value_string == "DATAMAP_END" {
                break;
            }
            match pair.code {
                300 => name = pair.value_string.clone(),
                301 if pair.value_string == "DATAMAP_VALUE" => {
                    values.push(TableCustomData {
                        name: std::mem::take(&mut name),
                        value: self.read_table_value_dxf()?,
                    });
                }
                _ => {}
            }
        }
        Ok(values)
    }

    fn read_table_content_format_dxf(&mut self) -> Result<CellContent> {
        let mut content = CellContent::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 309 && pair.value_string == "CONTENTFORMAT_END" {
                break;
            }
            match pair.code {
                90 => content.format_override_flags = pair.as_i32().unwrap_or(0),
                91 => content.format_property_flags = pair.as_i32().unwrap_or(0),
                92 => content.format_value_data_type = pair.as_i32().unwrap_or(0),
                93 => content.format_value_unit_type = pair.as_i32().unwrap_or(0),
                300 => content.value_format = pair.value_string.clone(),
                40 => content.rotation = pair.as_double().unwrap_or(0.0),
                140 => content.scale = pair.as_double().unwrap_or(1.0),
                94 => content.alignment = pair.as_i32().unwrap_or(0),
                62 => {
                    content.color = Color::from_index(pair.as_i16().unwrap_or(0));
                }
                340 => {
                    let handle = parse_dxf_handle(&pair.value_string);
                    content.text_style_handle = (!handle.is_null()).then_some(handle);
                }
                7 => content.text_style_name = pair.value_string.clone(),
                144 => content.text_height = pair.as_double().unwrap_or(0.18),
                _ => {}
            }
        }
        Ok(content)
    }

    fn apply_table_content_format_to_style(style: &mut CellStyle, format: CellContent) {
        style.content_format_override_flags = format.format_override_flags;
        style.content_property_flags = format.format_property_flags;
        style.value_data_type = format.format_value_data_type;
        style.value_unit_type = format.format_value_unit_type;
        style.value_format = format.value_format;
        style.rotation = format.rotation;
        style.scale = format.scale;
        style.alignment = format.alignment;
        style.content_color = format.color;
        style.text_style_handle = format.text_style_handle;
        style.text_style_name = format.text_style_name;
        style.text_height = format.text_height;
    }

    fn read_table_grid_format_dxf(&mut self) -> Result<CellBorder> {
        let mut border = CellBorder::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 309 && pair.value_string == "GRIDFORMAT_END" {
                break;
            }
            match pair.code {
                90 => {
                    border.override_flags =
                        BorderPropertyFlags::from_bits_retain(pair.as_i32().unwrap_or(0) as u32);
                }
                91 => {
                    border.border_type = BorderType::from(pair.as_i32().unwrap_or(1) as i16);
                }
                62 => {
                    border.color = Color::from_index(pair.as_i16().unwrap_or(0));
                }
                92 => {
                    border.line_weight = LineWeight::from_value(pair.as_i32().unwrap_or(-1) as i16);
                }
                340 => {
                    let handle = parse_dxf_handle(&pair.value_string);
                    border.line_type_handle = (!handle.is_null()).then_some(handle);
                }
                93 => border.invisible = pair.as_bool().unwrap_or(false),
                40 => border.double_spacing = pair.as_double().unwrap_or(0.0),
                _ => {}
            }
        }
        Ok(border)
    }

    fn read_table_margins_dxf(&mut self, style: &mut CellStyle) -> Result<()> {
        let mut margins = Vec::new();
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 309 && pair.value_string == "CELLMARGIN_END" {
                break;
            }
            if pair.code == 40 {
                margins.push(pair.as_double().unwrap_or(0.0));
            }
        }
        if let Some(value) = margins.first() {
            style.margin_top = *value;
        }
        if let Some(value) = margins.get(1) {
            style.margin_left = *value;
        }
        if let Some(value) = margins.get(2) {
            style.margin_bottom = *value;
        }
        if let Some(value) = margins.get(3) {
            style.margin_right = *value;
        }
        if let Some(value) = margins.get(4) {
            style.horizontal_spacing = *value;
        }
        if let Some(value) = margins.get(5) {
            style.vertical_spacing = *value;
        }
        Ok(())
    }

    fn read_table_cell_style_override_dxf(&mut self) -> Result<Option<CellStyle>> {
        let mut style = CellStyle::new();
        let mut has_data = false;
        let mut current_edge = 0u32;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 309 && pair.value_string == "TABLEFORMAT_END" {
                break;
            }
            match pair.code {
                90 => {
                    style.style_type = CellStyleType::from(pair.as_i32().unwrap_or(1) as u8);
                }
                170 => has_data = pair.as_i16().unwrap_or(0) != 0,
                91 => style.override_flags = pair.as_i32().unwrap_or(0),
                92 => {
                    style.property_flags =
                        CellStylePropertyFlags::from_bits_retain(pair.as_i32().unwrap_or(0) as u32);
                }
                62 => {
                    style.background_color = Color::from_index(pair.as_i16().unwrap_or(0));
                    style.fill_enabled =
                        !matches!(style.background_color, Color::ByBlock | Color::None);
                }
                93 => {
                    style.layout_flags =
                        ContentLayoutFlags::from_bits_retain(pair.as_i32().unwrap_or(0) as u32);
                }
                300 if pair.value_string == "CONTENTFORMAT" => {
                    let format = self.read_table_content_format_dxf()?;
                    Self::apply_table_content_format_to_style(&mut style, format);
                }
                171 => {
                    style.margin_override_flags = pair.as_i16().unwrap_or(0);
                }
                301 if pair.value_string == "MARGIN" => {
                    self.read_table_margins_dxf(&mut style)?;
                }
                95 => current_edge = pair.as_i32().unwrap_or(0) as u32,
                302 if pair.value_string == "GRIDFORMAT" => {
                    let border = self.read_table_grid_format_dxf()?;
                    style.applied_border_edges |= CellEdgeFlags::from_bits_retain(current_edge);
                    match current_edge {
                        1 => style.top_border = border,
                        2 => style.right_border = border,
                        4 => style.bottom_border = border,
                        8 => style.left_border = border,
                        _ => style.additional_borders.push((current_edge, border)),
                    }
                }
                _ => {}
            }
        }
        Ok(has_data.then_some(style))
    }

    fn read_table_modern_content_dxf(&mut self) -> Result<CellContent> {
        let mut content = CellContent::new();
        let mut pending_attribute: Option<usize> = None;
        let mut in_core = false;
        let mut in_format = false;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match (pair.code, pair.value_string.as_str()) {
                (1, "CELLCONTENT_BEGIN") => {
                    in_core = true;
                    continue;
                }
                (309, "CELLCONTENT_END") => {
                    in_core = false;
                    continue;
                }
                (1, "FORMATTEDCELLCONTENT_BEGIN") => {
                    in_format = true;
                    continue;
                }
                (309, "FORMATTEDCELLCONTENT_END") => break,
                _ => {}
            }
            if in_core {
                match pair.code {
                    90 => {
                        content.content_type =
                            TableCellContentType::from(pair.as_i32().unwrap_or(0) as u8);
                    }
                    300 if pair.value_string == "VALUE" => {
                        content.value = self.read_table_value_dxf()?;
                    }
                    340 => {
                        let handle = parse_dxf_handle(&pair.value_string);
                        match content.content_type {
                            TableCellContentType::Field => {
                                content.field_handle = (!handle.is_null()).then_some(handle);
                            }
                            TableCellContentType::Block => {
                                content.block_handle = (!handle.is_null()).then_some(handle);
                            }
                            _ => {}
                        }
                    }
                    330 => {
                        let index = content.attributes.len();
                        content.attributes.push(TableAttribute {
                            definition_handle: parse_dxf_handle(&pair.value_string),
                            value: String::new(),
                            index: index as i32,
                        });
                        pending_attribute = Some(index);
                    }
                    301 => {
                        if let Some(index) = pending_attribute {
                            content.attributes[index].value = pair.value_string.clone();
                        }
                    }
                    92 => {
                        if let Some(index) = pending_attribute.take() {
                            content.attributes[index].index = pair.as_i32().unwrap_or(index as i32);
                        }
                    }
                    _ => {}
                }
            } else if in_format {
                if pair.code == 300 && pair.value_string == "CONTENTFORMAT" {
                    let format = self.read_table_content_format_dxf()?;
                    content.format_override_flags = format.format_override_flags;
                    content.format_property_flags = format.format_property_flags;
                    content.format_value_data_type = format.format_value_data_type;
                    content.format_value_unit_type = format.format_value_unit_type;
                    content.value_format = format.value_format;
                    content.rotation = format.rotation;
                    content.scale = format.scale;
                    content.alignment = format.alignment;
                    content.color = format.color;
                    content.text_style_handle = format.text_style_handle;
                    content.text_style_name = format.text_style_name;
                    content.text_height = format.text_height;
                }
            }
        }
        Ok(content)
    }

    fn read_table_modern_cell_dxf(&mut self) -> Result<TableCell> {
        let mut cell = TableCell::new();
        let mut in_linked = false;
        let mut in_table_cell = false;
        let mut geometry_flag_seen = false;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match (pair.code, pair.value_string.as_str()) {
                (1, "LINKEDTABLEDATACELL_BEGIN") => {
                    in_linked = true;
                    continue;
                }
                (309, "LINKEDTABLEDATACELL_END") => {
                    in_linked = false;
                    continue;
                }
                (1, "TABLEFORMAT_BEGIN") => {
                    cell.style = self.read_table_cell_style_override_dxf()?;
                    continue;
                }
                (1, "TABLECELL_BEGIN") => {
                    in_table_cell = true;
                    continue;
                }
                (309, "TABLECELL_END") => break,
                _ => {}
            }

            if in_linked {
                match pair.code {
                    90 => {
                        cell.state =
                            CellStateFlags::from_bits_retain(pair.as_i32().unwrap_or(0) as u32);
                    }
                    300 => cell.tooltip = pair.value_string.clone(),
                    91 => cell.custom_data = pair.as_i32().unwrap_or(0),
                    301 if pair.value_string == "CUSTOMDATA" => {
                        cell.custom_data_items = self.read_table_custom_data_dxf()?;
                    }
                    92 => cell.has_linked_data = pair.as_i32().unwrap_or(0) != 0,
                    340 if cell.has_linked_data => {
                        let handle = parse_dxf_handle(&pair.value_string);
                        cell.data_link_handle = (!handle.is_null()).then_some(handle);
                    }
                    93 if cell.has_linked_data => {
                        cell.data_link_rows = pair.as_i32().unwrap_or(0);
                    }
                    94 if cell.has_linked_data => {
                        cell.data_link_columns = pair.as_i32().unwrap_or(0);
                    }
                    96 if cell.has_linked_data => {
                        cell.data_link_unknown = pair.as_i32().unwrap_or(0);
                    }
                    302 if pair.value_string == "CONTENT" => {
                        cell.contents.push(self.read_table_modern_content_dxf()?);
                    }
                    _ => {}
                }
            } else if in_table_cell {
                match pair.code {
                    90 => cell.style_id = pair.as_i32().unwrap_or(0),
                    91 if !geometry_flag_seen => {
                        geometry_flag_seen = true;
                    }
                    91 => cell.geometry_data_flag = pair.as_i32().unwrap_or(0),
                    40 => {
                        cell.geometry_width_with_gap = pair.as_double().unwrap_or(0.0);
                    }
                    41 => {
                        cell.geometry_height_with_gap = pair.as_double().unwrap_or(0.0);
                    }
                    330 => {
                        let handle = parse_dxf_handle(&pair.value_string);
                        cell.geometry_handle = (!handle.is_null()).then_some(handle);
                    }
                    92 => {
                        cell.geometry_flags = pair.as_i32().unwrap_or(0).max(0);
                        cell.geometries.reserve(cell.geometry_flags as usize);
                    }
                    10 => {
                        cell.geometries.push(CellContentGeometry {
                            distance_to_top_left: Vector3::new(
                                pair.as_double().unwrap_or(0.0),
                                0.0,
                                0.0,
                            ),
                            distance_to_center: Vector3::ZERO,
                            width: 0.0,
                            height: 0.0,
                            outer_width: 0.0,
                            outer_height: 0.0,
                            flags: 0,
                        });
                    }
                    20 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.distance_to_top_left.y = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    30 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.distance_to_top_left.z = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    11 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.distance_to_center.x = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    21 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.distance_to_center.y = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    31 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.distance_to_center.z = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    43 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.width = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    44 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.height = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    45 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.outer_width = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    46 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.outer_height = pair.as_double().unwrap_or(0.0);
                        }
                    }
                    95 => {
                        if let Some(value) = cell.geometries.last_mut() {
                            value.flags = pair.as_i32().unwrap_or(0);
                        }
                    }
                    _ => {}
                }
            }
        }

        if cell.geometry_flags == 0 {
            cell.geometry_flags = cell.geometries.len() as i32;
        }
        cell.geometry = cell.geometries.first().cloned();
        cell.cell_type = if cell
            .contents
            .iter()
            .any(|content| content.content_type == TableCellContentType::Block)
        {
            CellType::Block
        } else {
            CellType::Text
        };
        Ok(cell)
    }

    fn read_table_modern_column_dxf(&mut self) -> Result<TableColumn> {
        let mut column = TableColumn::new();
        let mut in_linked = false;
        let mut in_table_column = false;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match (pair.code, pair.value_string.as_str()) {
                (1, "LINKEDTABLEDATACOLUMN_BEGIN") => {
                    in_linked = true;
                    continue;
                }
                (309, "LINKEDTABLEDATACOLUMN_END") => {
                    in_linked = false;
                    continue;
                }
                (1, "TABLEFORMAT_BEGIN") => {
                    column.style = self.read_table_cell_style_override_dxf()?;
                    continue;
                }
                (1, "TABLECOLUMN_BEGIN") => {
                    in_table_column = true;
                    continue;
                }
                (309, "TABLECOLUMN_END") => break,
                _ => {}
            }
            if in_linked {
                match pair.code {
                    300 => column.name = pair.value_string.clone(),
                    91 => column.custom_data = pair.as_i32().unwrap_or(0),
                    301 if pair.value_string == "CUSTOMDATA" => {
                        column.custom_data_items = self.read_table_custom_data_dxf()?;
                    }
                    _ => {}
                }
            } else if in_table_column {
                match pair.code {
                    90 => column.style_id = pair.as_i32().unwrap_or(0),
                    40 => column.width = pair.as_double().unwrap_or(column.width),
                    _ => {}
                }
            }
        }
        Ok(column)
    }

    fn read_table_modern_row_dxf(&mut self) -> Result<TableRow> {
        let mut row = TableRow::new(0);
        let mut in_linked = false;
        let mut in_table_row = false;
        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            match (pair.code, pair.value_string.as_str()) {
                (1, "LINKEDTABLEDATAROW_BEGIN") => {
                    in_linked = true;
                    continue;
                }
                (309, "LINKEDTABLEDATAROW_END") => {
                    in_linked = false;
                    continue;
                }
                (1, "TABLEFORMAT_BEGIN") => {
                    row.style = self.read_table_cell_style_override_dxf()?;
                    continue;
                }
                (1, "TABLEROW_BEGIN") => {
                    in_table_row = true;
                    continue;
                }
                (309, "TABLEROW_END") => break,
                _ => {}
            }
            if in_linked {
                match pair.code {
                    300 if pair.value_string == "CELL" => {
                        row.cells.push(self.read_table_modern_cell_dxf()?);
                    }
                    91 => row.custom_data = pair.as_i32().unwrap_or(0),
                    301 if pair.value_string == "CUSTOMDATA" => {
                        row.custom_data_items = self.read_table_custom_data_dxf()?;
                    }
                    _ => {}
                }
            } else if in_table_row {
                match pair.code {
                    90 => row.style_id = pair.as_i32().unwrap_or(0),
                    40 => row.height = pair.as_double().unwrap_or(row.height),
                    _ => {}
                }
            }
        }
        Ok(row)
    }

    pub(super) fn read_table_content_object_typed_dxf(&mut self) -> Result<Option<Table>> {
        let mut table = Table::new(Vector3::ZERO, 0, 0);
        table.rows.clear();
        table.columns.clear();
        let mut section = String::new();
        let mut expected_columns: Option<usize> = None;
        let mut expected_rows: Option<usize> = None;
        let mut field_refs_remaining = 0usize;
        let mut merged_ranges_remaining = 0usize;
        let mut merged_range: Option<[usize; 4]> = None;

        while let Some(pair) = self.reader.read_pair()? {
            if pair.code == 0 {
                self.reader.push_back(pair);
                break;
            }
            if pair.code == 100 {
                section = pair.value_string.clone();
                continue;
            }
            if pair.code == 5 || pair.code == 102 {
                self.try_read_common_entity_code(&pair, &mut table.common)?;
                continue;
            }
            if pair.code == 330 && section.is_empty() && table.common.owner_handle.is_null() {
                table.common.owner_handle = parse_dxf_handle(&pair.value_string);
                continue;
            }

            match section.as_str() {
                "AcDbLinkedData" => match pair.code {
                    1 => table.name = pair.value_string.clone(),
                    300 => table.description = pair.value_string.clone(),
                    _ => {}
                },
                "AcDbLinkedTableData" => match pair.code {
                    90 if expected_columns.is_none() => {
                        expected_columns = Some(pair.as_i32().unwrap_or(0).max(0) as usize);
                    }
                    300 if pair.value_string == "COLUMN" => {
                        table.columns.push(self.read_table_modern_column_dxf()?);
                    }
                    91 if expected_rows.is_none()
                        && expected_columns == Some(table.columns.len()) =>
                    {
                        expected_rows = Some(pair.as_i32().unwrap_or(0).max(0) as usize);
                    }
                    301 if pair.value_string == "ROW" => {
                        table.rows.push(self.read_table_modern_row_dxf()?);
                    }
                    92 if expected_rows == Some(table.rows.len()) => {
                        field_refs_remaining = pair.as_i32().unwrap_or(0).max(0) as usize;
                    }
                    330 | 340 | 360 if field_refs_remaining > 0 => {
                        let handle = parse_dxf_handle(&pair.value_string);
                        if !handle.is_null() {
                            table.field_handles.push(handle);
                        }
                        field_refs_remaining -= 1;
                    }
                    _ => {}
                },
                "AcDbFormattedTableData" => match pair.code {
                    300 if pair.value_string == "TABLEFORMAT" => {}
                    1 if pair.value_string == "TABLEFORMAT_BEGIN" => {
                        table.base_style = self.read_table_cell_style_override_dxf()?;
                    }
                    90 => {
                        merged_ranges_remaining = pair.as_i32().unwrap_or(0).max(0) as usize;
                    }
                    91 => {
                        if merged_range.is_none() && merged_ranges_remaining > 0 {
                            merged_range = Some([0; 4]);
                        }
                        if let Some(range) = merged_range.as_mut() {
                            range[0] = pair.as_i32().unwrap_or(0).max(0) as usize;
                        }
                    }
                    92 => {
                        if let Some(range) = merged_range.as_mut() {
                            range[1] = pair.as_i32().unwrap_or(0).max(0) as usize;
                        }
                    }
                    93 => {
                        if let Some(range) = merged_range.as_mut() {
                            range[2] = pair.as_i32().unwrap_or(0).max(0) as usize;
                        }
                    }
                    94 => {
                        if let Some(mut range) = merged_range.take() {
                            range[3] = pair.as_i32().unwrap_or(0).max(0) as usize;
                            table
                                .merged_ranges
                                .push(crate::entities::table::CellRange::new(
                                    range[0], range[1], range[2], range[3],
                                ));
                            merged_ranges_remaining = merged_ranges_remaining.saturating_sub(1);
                        }
                    }
                    _ => {}
                },
                "AcDbTableContent" => {
                    if pair.code == 340 {
                        let handle = parse_dxf_handle(&pair.value_string);
                        table.table_style_handle = (!handle.is_null()).then_some(handle);
                    }
                }
                _ => {}
            }
        }
        Ok(Some(table))
    }
}
