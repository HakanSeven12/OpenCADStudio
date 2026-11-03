// TABLE command — create a styled empty table.
//
// Title/header rows come from the selected table style and are not counted as
// data rows. The command remembers its previous settings and supports either a
// fixed insertion point or a two-corner sizing window.

use codec::entities::table::{CellRange, CellStyle, CellStylePropertyFlags};
use codec::entities::TableBuilder;
use codec::types::Vector3;
use codec::EntityType;
use glam::DVec3;
use std::sync::Mutex;

use crate::command::{CadCommand, CmdResult, InputKind, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/table.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "TABLE",
        label: "Table",
        icon: ICON,
        event: ModuleEvent::Command("TABLE".to_string()),
    }
}

/// Table settings TABLE and -TABLE share and remember: the dialog's column
/// and row settings, its insertion behaviour and its window choices.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TableDefaults {
    pub columns: usize,
    pub data_rows: usize,
    /// Column width; 0 until chosen, then the drawing's default (`cell_width`).
    pub column_width: f64,
    /// Minimum row height, in lines of the row's text.
    pub row_lines: usize,
    pub window: bool,
    /// Window mode keeps the column count (else the column width).
    pub columns_fixed: bool,
    /// Window mode keeps the data row count (else the row height).
    pub rows_fixed: bool,
}

impl Default for TableDefaults {
    fn default() -> Self {
        Self {
            columns: 5,
            data_rows: 1,
            column_width: 0.0,
            row_lines: 1,
            window: false,
            columns_fixed: true,
            rows_fixed: true,
        }
    }
}

impl TableDefaults {
    /// The column width, 2.5 units (63.5 in a metric drawing) until one is set.
    pub fn cell_width(&self, metric: bool) -> f64 {
        if self.column_width > 0.0 {
            self.column_width
        } else if metric {
            63.5
        } else {
            2.5
        }
    }
}

static SESSION: Mutex<TableDefaults> = Mutex::new(TableDefaults {
    columns: 5,
    data_rows: 1,
    column_width: 0.0,
    row_lines: 1,
    window: false,
    columns_fixed: true,
    rows_fixed: true,
});

pub fn session() -> std::sync::MutexGuard<'static, TableDefaults> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// Row heights of a table: a row is `lines` lines of its text — four thirds
/// of the text height each — plus the style's top and bottom margins.
pub fn row_heights(
    style: Option<&codec::objects::TableStyle>,
    title: bool,
    header: bool,
    data_rows: usize,
    lines: usize,
) -> Vec<f64> {
    row_heights_for(style, ["Title", "Header", "Data"], title, header, data_rows, lines)
}

/// Row heights of a table whose first, second and other rows take the
/// named cell styles (`Title`, `Header`, `Data` or a user cell style): each
/// row is `lines` lines of its cell style's text plus its top and bottom
/// margins.
pub fn row_heights_for(
    style: Option<&codec::objects::TableStyle>,
    names: [&str; 3],
    title: bool,
    header: bool,
    data_rows: usize,
    lines: usize,
) -> Vec<f64> {
    let row_height = |slot: usize| {
        let (text, margin) = match style {
            None => ([0.25, 0.18, 0.18][slot], 0.06),
            Some(s) => match s.cell_style(&cell_style_name(names[slot])) {
                Some(cell) => (
                    cell.cell_style.content_format.text_height,
                    cell.cell_style.margins().map_or(s.vertical_margin, |(_, v)| v),
                ),
                None => (
                    [
                        s.title_row_style.text_height,
                        s.header_row_style.text_height,
                        s.data_row_style.text_height,
                    ][slot],
                    s.vertical_margin,
                ),
            },
        };
        text * 4.0 / 3.0 * lines.max(1) as f64 + 2.0 * margin
    };
    let mut rows = Vec::with_capacity(data_rows + 2);
    if title {
        rows.push(row_height(0));
    }
    if header {
        rows.push(row_height(1));
    }
    rows.extend(std::iter::repeat_n(row_height(2), data_rows));
    rows
}

/// The cell style a row style name picks: `Title`, `Header` and `Data` are
/// the title, header and data cell styles; others are user cell styles.
pub fn cell_style_name(name: &str) -> String {
    if name.eq_ignore_ascii_case("Title") {
        codec::objects::TITLE_CELL_STYLE.to_string()
    } else if name.eq_ignore_ascii_case("Header") {
        codec::objects::HEADER_CELL_STYLE.to_string()
    } else if name.eq_ignore_ascii_case("Data") {
        codec::objects::DATA_CELL_STYLE.to_string()
    } else {
        name.to_string()
    }
}

/// The named table styles -TABLE's Style option may pick.
#[derive(Clone)]
pub struct StyleChoice {
    pub name: String,
    pub handle: codec::Handle,
    pub style: codec::objects::TableStyle,
    /// The style's starting table, if it has one.
    pub template: Option<codec::entities::Table>,
}

/// What a table made from a style's starting table takes from it (the
/// -TABLE Style option's insert options; remembered for the session).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemplateOptions {
    pub label_text: bool,
    pub data_text: bool,
    pub formulas: bool,
    pub fields: bool,
    pub data_links: bool,
    pub blocks: bool,
    pub cell_style_overrides: bool,
}

impl Default for TemplateOptions {
    fn default() -> Self {
        Self {
            label_text: true,
            data_text: false,
            formulas: false,
            fields: false,
            data_links: false,
            blocks: false,
            cell_style_overrides: true,
        }
    }
}

static TEMPLATE_OPTIONS: Mutex<TemplateOptions> = Mutex::new(TemplateOptions {
    label_text: true,
    data_text: false,
    formulas: false,
    fields: false,
    data_links: false,
    blocks: false,
    cell_style_overrides: true,
});

pub fn template_options() -> std::sync::MutexGuard<'static, TemplateOptions> {
    TEMPLATE_OPTIONS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The insert options in prompt order: keyword, option word, question
/// subject.
const TEMPLATE_OPTION_KEYS: [(&str, &str, &str); 7] = [
    ("L", "LABEL", "label text"),
    ("D", "DATA", "data text"),
    ("F", "FORMULAS", "formulas"),
    ("I", "FIELDS", "fields"),
    ("N", "LINKS", "data links"),
    ("B", "BLOCKS", "blocks"),
    ("O", "OVERRIDES", "cell style overrides"),
];

fn template_option(options: &mut TemplateOptions, index: usize) -> &mut bool {
    match index {
        0 => &mut options.label_text,
        1 => &mut options.data_text,
        2 => &mut options.formulas,
        3 => &mut options.fields,
        4 => &mut options.data_links,
        5 => &mut options.blocks,
        _ => &mut options.cell_style_overrides,
    }
}

/// An empty, unmerged cell with the cell style of `cell`.
fn blank_cell_like(cell: &codec::entities::table::TableCell) -> codec::entities::table::TableCell {
    let mut blank = codec::entities::table::TableCell::new();
    blank.style_id = cell.style_id;
    blank.style = cell.style.clone();
    blank
}

/// A table made from a style's starting table: its rows, columns, merges
/// and sizes, the cell contents `options` keep, and `extra_rows` /
/// `extra_columns` more rows and columns like its last ones.
pub fn table_from_template(
    template: &codec::entities::Table,
    style: &codec::objects::TableStyle,
    style_handle: codec::Handle,
    options: TemplateOptions,
    extra_rows: usize,
    extra_columns: usize,
) -> codec::entities::Table {
    use codec::entities::table::TableCellContentType;
    let mut table = template.clone();
    table.common = codec::entities::EntityCommon::default();
    table.table_style_handle = Some(style_handle);
    table.block_record_handle = None;
    table.block_name.clear();
    let class_of = |id: i32| {
        style
            .cell_style_by_id(id)
            .map_or(codec::objects::CELL_CLASS_DATA, |cell| cell.style_type)
    };
    for row in &mut table.rows {
        let row_class = class_of(row.style_id);
        if !options.cell_style_overrides {
            row.style = None;
        }
        for cell in &mut row.cells {
            let class = if cell.style_id != 0 {
                class_of(cell.style_id)
            } else {
                row_class
            };
            let keep_text = if class == codec::objects::CELL_CLASS_LABEL {
                options.label_text
            } else {
                options.data_text
            };
            cell.contents.retain(|content| match content.content_type {
                TableCellContentType::Block => options.blocks,
                _ if content.field_handle.is_some() => options.fields,
                _ if content.value.text.trim_start().starts_with('=') => options.formulas,
                _ => keep_text,
            });
            if !options.data_links {
                cell.data_link_handle = None;
            }
            if !options.cell_style_overrides {
                cell.style = None;
            }
        }
    }
    if !options.cell_style_overrides {
        for column in &mut table.columns {
            column.style = None;
        }
    }
    crate::entities::table::detach_fields(&mut table);
    let old_columns = table.columns.len();
    if extra_columns > 0 {
        if let Some(last) = table.columns.last().cloned() {
            table
                .columns
                .extend(std::iter::repeat_n(last, extra_columns));
        }
        let new_last = table.columns.len().saturating_sub(1);
        for row in &mut table.rows {
            if let Some(last) = row.cells.last() {
                let blank = blank_cell_like(last);
                row.cells.extend(std::iter::repeat_n(blank, extra_columns));
            }
        }
        // A range across the whole width (the title) keeps spanning it.
        for range in &mut table.merged_ranges {
            if range.left_col == 0 && range.right_col + 1 == old_columns {
                range.right_col = new_last;
            }
        }
    }
    if extra_rows > 0 {
        if let Some(last) = table.rows.last().cloned() {
            let mut blank = last;
            blank.cells = blank.cells.iter().map(blank_cell_like).collect();
            table.rows.extend(std::iter::repeat_n(blank, extra_rows));
        }
    }
    // The cells take the merged ranges again (extended ones included).
    let ranges = std::mem::take(&mut table.merged_ranges);
    for range in ranges {
        table.merge_cells(range);
    }
    table
}


#[derive(Clone, Copy, PartialEq)]
enum Step {
    Columns,
    Rows,
    Insertion,
    FirstCorner,
    SecondCorner(DVec3),
    StyleName { window: bool },
    Width { window: bool },
    Height { window: bool },
    LinkName,
    /// "from Style": place the style's starting table.
    FromStyle,
    FromStyleName,
    FromStyleRows,
    FromStyleColumns,
    FromStyleOptions,
    /// The Yes/No question of an insert option (index into the options).
    FromStyleOption(usize),
}

pub struct TableCommand {
    step: Step,
    /// "-TABLE" asks for everything on the command line; "TABLE" places the
    /// table the dialog set up.
    command: &'static str,
    style: Option<StyleChoice>,
    styles: Vec<StyleChoice>,
    links: Vec<(String, codec::Handle)>,
    /// None: Auto (window mode fills the window with it).
    columns: Option<usize>,
    data_rows: Option<usize>,
    cell_width: f64,
    row_lines: usize,
    /// Cell style names of the first, second and other rows.
    row_cell_styles: [String; 3],
    preview_enabled: bool,
    plane: WorkingPlane,
    /// "from Style": the style whose starting table is placed, and the rows
    /// and columns added to it.
    template_style: Option<StyleChoice>,
    extra_rows: usize,
    extra_columns: usize,
}

fn cell_style_from_row(style: &codec::objects::RowCellStyle) -> CellStyle {
    let mut result = CellStyle::new();
    result.property_flags = CellStylePropertyFlags::DATA_TYPE
        | CellStylePropertyFlags::DATA_FORMAT
        | CellStylePropertyFlags::ALIGNMENT
        | CellStylePropertyFlags::CONTENT_COLOR
        | CellStylePropertyFlags::TEXT_STYLE
        | CellStylePropertyFlags::TEXT_HEIGHT
        | CellStylePropertyFlags::BACKGROUND_COLOR;
    result.content_color = style.text_color;
    result.text_style_handle = style.text_style_handle;
    result.text_style_name = style.text_style_name.clone();
    result.text_height = style.text_height;
    result.alignment = style.alignment as i32;
    result.background_color = style.fill_color;
    result.fill_enabled = style.fill_enabled;
    result.value_data_type = style.data_type;
    result.value_unit_type = style.unit_type;
    result.value_format = style.format_string.clone();
    result
}

pub(crate) fn selected_row_style(table_style: &codec::objects::TableStyle, name: &str) -> CellStyle {
    let style = if name.eq_ignore_ascii_case("Title") {
        &table_style.title_row_style
    } else if name.eq_ignore_ascii_case("Header") {
        &table_style.header_row_style
    } else {
        &table_style.data_row_style
    };
    cell_style_from_row(style)
}

/// A whole number the column and row prompts accept.
enum Count {
    Value(usize),
    NotPositive,
    Invalid,
}

fn parse_count(input: &str) -> Count {
    match input.parse::<i64>() {
        Ok(value) if value > 0 => Count::Value(value as usize),
        Ok(_) => Count::NotPositive,
        Err(_) => Count::Invalid,
    }
}

/// `input` is a non-empty prefix of the keyword `word`.
fn keyword(input: &str, word: &str) -> bool {
    !input.is_empty() && word.starts_with(&input.to_ascii_uppercase())
}

impl TableCommand {
    fn new(
        command: &'static str,
        style: Option<StyleChoice>,
        defaults: TableDefaults,
        metric: bool,
    ) -> Self {
        Self {
            step: Step::Columns,
            command,
            row_cell_styles: ["Title", "Header", "Data"].map(str::to_string),
            style,
            styles: Vec::new(),
            links: Vec::new(),
            columns: Some(defaults.columns.max(1)),
            data_rows: Some(defaults.data_rows.max(1)),
            cell_width: defaults.cell_width(metric),
            row_lines: defaults.row_lines.max(1),
            preview_enabled: true,
            plane: WorkingPlane::default(),
            template_style: None,
            extra_rows: 0,
            extra_columns: 0,
        }
    }

    /// TABLE's "From a table style": place the style's starting table.
    pub fn from_style(style: StyleChoice, extra_rows: usize, extra_columns: usize) -> Self {
        let mut command = Self::new("TABLE", Some(style.clone()), TableDefaults::default(), true);
        command.template_style = Some(style);
        command.extra_rows = extra_rows;
        command.extra_columns = extra_columns;
        command.step = Step::FromStyle;
        command
    }

    /// The table "from Style" places at `point`.
    fn template_table(&self, point: DVec3) -> Option<EntityType> {
        let choice = self.template_style.as_ref()?;
        let template = choice.template.as_ref()?;
        let mut table = table_from_template(
            template,
            &choice.style,
            choice.handle,
            *template_options(),
            self.extra_rows,
            self.extra_columns,
        );
        let point = self.plane.to_local(point);
        table.insertion_point = Vector3::new(point.x, point.y, point.z);
        Some(self.plane.place_entity(EntityType::Table(Box::new(table))))
    }

    /// The styles whose starting table "from Style" can place.
    fn template_styles(&self) -> impl Iterator<Item = &StyleChoice> {
        self.styles.iter().filter(|choice| choice.template.is_some())
    }

    /// -TABLE: every setting on the command line, starting from the ones
    /// TABLE and -TABLE last used.
    pub fn command_line(
        style: Option<StyleChoice>,
        styles: Vec<StyleChoice>,
        links: Vec<(String, codec::Handle)>,
        metric: bool,
    ) -> Self {
        let defaults = *session();
        let mut command = Self::new("-TABLE", style, defaults, metric);
        command.styles = styles;
        command.links = links;
        command
    }

    /// The line -TABLE starts with.
    pub fn status_line(&self) -> String {
        t!(
            "Current table style: %{style}  Cell width: %{width}  Cell height: %{lines} line(s)",
            style = self.style.as_ref().map_or("Standard", |s| s.name.as_str()),
            width = crate::entities::common::format_length(self.cell_width),
            lines = self.row_lines
        )
        .into_owned()
    }

    /// TABLE: place the table the Insert Table dialog set up. In window
    /// mode a count left out (`None`) comes from the window.
    #[allow(clippy::too_many_arguments)]
    pub fn configured(
        style: Option<StyleChoice>,
        columns: Option<usize>,
        data_rows: Option<usize>,
        column_width: f64,
        row_lines: usize,
        window: bool,
        preview_enabled: bool,
        row_style_names: [&str; 3],
    ) -> Self {
        let mut command = Self::new("TABLE", style, TableDefaults::default(), true);
        command.columns = columns;
        command.data_rows = data_rows;
        command.cell_width = column_width;
        command.row_lines = row_lines.max(1);
        command.preview_enabled = preview_enabled;
        command.row_cell_styles = row_style_names.map(str::to_string);
        command.step = if window {
            Step::FirstCorner
        } else {
            Step::Insertion
        };
        command
    }

    fn row_names(&self) -> [&str; 3] {
        [
            self.row_cell_styles[0].as_str(),
            self.row_cell_styles[1].as_str(),
            self.row_cell_styles[2].as_str(),
        ]
    }

    fn dash(&self) -> bool {
        self.command == "-TABLE"
    }

    /// A new table always starts with its title and header rows; the
    /// style's legacy suppress flags do not take them away.
    fn title_header(&self) -> (bool, bool) {
        (true, true)
    }

    /// Column widths, row heights and the insertion point (plane
    /// coordinates) of the table placed at `first`, or in the window from
    /// `first` to `second`. The window's first corner is the insertion
    /// point whichever way the window runs.
    fn layout(&self, first: DVec3, second: Option<DVec3>) -> (DVec3, Vec<f64>, Vec<f64>) {
        let first = self.plane.to_local(first);
        let (title, header) = self.title_header();
        let style = self.style.as_ref().map(|choice| &choice.style);
        let Some(second) = second else {
            let widths = vec![self.cell_width; self.columns.unwrap_or(1)];
            let heights =
                row_heights_for(style, self.row_names(), title, header, self.data_rows.unwrap_or(1), self.row_lines);
            return (first, widths, heights);
        };
        let second = self.plane.to_local(second);
        let (width, height) = ((second.x - first.x).abs(), (second.y - first.y).abs());
        let widths = match self.columns {
            Some(count) if width > 1.0e-9 => vec![width / count as f64; count],
            Some(count) => vec![self.cell_width; count],
            None => {
                let count = ((width / self.cell_width + 1.0e-9).floor() as usize).max(1);
                vec![self.cell_width; count]
            }
        };
        let heights = match self.data_rows {
            // A fixed row count fills the window: one-line rows share what
            // is left of its height.
            Some(count) => {
                let mut rows = row_heights_for(style, self.row_names(), title, header, count, 1);
                let spare = (height - rows.iter().sum::<f64>()) / rows.len() as f64;
                if spare > 0.0 {
                    rows.iter_mut().for_each(|row| *row += spare);
                }
                rows
            }
            // Else as many data rows as fit, at least one.
            None => {
                let one = row_heights_for(style, self.row_names(), title, header, 1, self.row_lines);
                let data = *one.last().unwrap_or(&1.0);
                let labels: f64 = one[..one.len() - 1].iter().sum();
                let count = ((height - labels) / data + 1.0e-9).floor().max(1.0) as usize;
                row_heights_for(style, self.row_names(), title, header, count, self.row_lines)
            }
        };
        (first, widths, heights)
    }

    fn build_table(&self, first: DVec3, second: Option<DVec3>) -> EntityType {
        let (point, widths, heights) = self.layout(first, second);
        let mut table = TableBuilder::new(heights.len(), widths.len())
            .at(Vector3::new(point.x, point.y, point.z))
            .build();
        for (column, width) in table.columns.iter_mut().zip(&widths) {
            column.width = *width;
        }
        for (row, height) in table.rows.iter_mut().zip(&heights) {
            row.height = *height;
        }
        table.table_style_handle = self.style.as_ref().map(|choice| choice.handle);
        let names = &self.row_cell_styles;
        let mut merged_rows = Vec::new();
        match self.style.as_ref().map(|choice| &choice.style) {
            // Rows name their cell style; the style draws them, so later
            // edits of the style reach the table.
            Some(style) if !style.modern_overrides.is_empty() => {
                for (row_index, row) in table.rows.iter_mut().enumerate() {
                    let name = cell_style_name(&names[row_index.min(2)]);
                    if let Some(cell) = style.cell_style(&name) {
                        row.style_id = cell.id;
                        row.style = None;
                        if cell.cell_style.merge_all() {
                            merged_rows.push(row_index);
                        }
                    }
                }
            }
            Some(style) => {
                for (row_index, row) in table.rows.iter_mut().enumerate() {
                    row.style = Some(selected_row_style(style, &names[row_index.min(2)]));
                }
                // The title spans the table, as the title cell style merges
                // its row.
                if self.title_header().0 {
                    merged_rows.push(0);
                }
            }
            None => {
                if self.title_header().0 {
                    merged_rows.push(0);
                }
            }
        }
        // "Merge cells on row/column creation": the row's cells merge.
        if widths.len() > 1 {
            for row in merged_rows {
                table.merge_cells(CellRange::new(row, 0, row, widths.len() - 1));
            }
        }
        self.plane.place_entity(EntityType::Table(Box::new(table)))
    }

    fn remember(&self) {
        let mut saved = session();
        if let Some(columns) = self.columns {
            saved.columns = columns;
        }
        if let Some(rows) = self.data_rows {
            saved.data_rows = rows;
        }
        saved.column_width = self.cell_width;
        saved.row_lines = self.row_lines;
    }

    fn place(&mut self, first: DVec3, second: Option<DVec3>) -> CmdResult {
        if self.dash() {
            self.remember();
        }
        // Like the dialog's table, the new one opens its first cell.
        CmdResult::CommitAndEditText(self.build_table(first, second))
    }

    fn preview_grid(&self, first: DVec3, second: Option<DVec3>) -> WireModel {
        let (point, widths, heights) = self.layout(first, second);
        let point = self.plane.to_world(point);
        self.grid_wire(point, &widths, &heights)
    }

    /// The outline grid of a table at `point` (world).
    fn grid_wire(&self, point: DVec3, widths: &[f64], heights: &[f64]) -> WireModel {
        let point = self.plane.to_local(point);
        let width: f64 = widths.iter().sum();
        let height: f64 = heights.iter().sum();
        let world = |x: f64, y: f64| {
            self.plane
                .to_world(point + DVec3::new(x, y, 0.0))
                .as_vec3()
                .to_array()
        };
        let mut points = Vec::with_capacity((widths.len() + heights.len() + 2) * 2);
        let mut x = 0.0;
        for column in std::iter::once(0.0).chain(widths.iter().copied()) {
            x += column;
            points.push(world(x, 0.0));
            points.push(world(x, -height));
        }
        let mut y = 0.0;
        for row in std::iter::once(0.0).chain(heights.iter().copied()) {
            y -= row;
            points.push(world(0.0, y));
            points.push(world(width, y));
        }
        WireModel {
            name: "table_preview".into(),
            points,
            color: WireModel::CYAN,
            line_weight_px: 1.0,
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            display_visible: true,
            plot_visible: true,
            ..Default::default()
        }
    }

    /// Back to where the Style, Width or Height option was chosen.
    fn back(window: bool) -> Step {
        if window {
            Step::FirstCorner
        } else {
            Step::Insertion
        }
    }

    fn after_counts(&self) -> Step {
        if self.columns.is_some() && self.data_rows.is_some() {
            Step::Insertion
        } else {
            Step::FirstCorner
        }
    }

    /// The options the current placement prompt offers ("" for none).
    fn placement_options(&self) -> &'static str {
        if !self.dash() {
            return "";
        }
        match (self.step, self.columns, self.data_rows) {
            (Step::Insertion, ..) => "Style/Width/Height",
            (_, None, Some(_)) => "Style/Width",
            (_, Some(_), None) => "Style/Height",
            _ => "",
        }
    }
}

impl CadCommand for TableCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        self.command
    }

    fn prompt(&self) -> String {
        let n = self.command;
        match self.step {
            Step::Columns => t!(
                "%{n}  Enter number of columns or [Auto/from Style/data Link] <%{cols}>:",
                n = n,
                cols = self.columns.unwrap_or(1)
            )
            .into_owned(),
            Step::Rows => t!(
                "%{n}  Enter number of rows or [Auto] <%{rows}>:",
                n = n,
                rows = session().data_rows
            )
            .into_owned(),
            Step::Insertion | Step::FirstCorner => {
                let options = self.placement_options();
                match (self.step == Step::Insertion, options.is_empty()) {
                    (true, true) => t!("%{n}  Specify insertion point:", n = n).into_owned(),
                    (true, false) => t!(
                        "%{n}  Specify insertion point or [%{options}]:",
                        n = n,
                        options = options
                    )
                    .into_owned(),
                    (false, true) => t!("%{n}  Specify first corner:", n = n).into_owned(),
                    (false, false) => t!(
                        "%{n}  Specify first corner or [%{options}]:",
                        n = n,
                        options = options
                    )
                    .into_owned(),
                }
            }
            Step::SecondCorner(_) => t!("%{n}  Specify second corner:", n = n).into_owned(),
            Step::StyleName { .. } => t!(
                "%{n}  Enter table style name <%{style}>:",
                n = n,
                style = self.style.as_ref().map_or("Standard", |s| s.name.as_str())
            )
            .into_owned(),
            Step::Width { .. } => t!(
                "%{n}  Enter cell width <%{width}>:",
                n = n,
                width = crate::entities::common::format_length(self.cell_width)
            )
            .into_owned(),
            Step::Height { .. } => t!(
                "%{n}  Enter minimum cell height <%{lines} line>:",
                n = n,
                lines = self.row_lines
            )
            .into_owned(),
            Step::LinkName => {
                t!("%{n}  Enter the name of a data link or [?]:", n = n).into_owned()
            }
            Step::FromStyle => t!(
                "%{n}  Specify insertion point or [Style/Rows/Columns/Options]:",
                n = n
            )
            .into_owned(),
            Step::FromStyleName => t!(
                "%{n}  Enter the name of a table style or [?] <%{style}>:",
                n = n,
                style = self
                    .template_style
                    .as_ref()
                    .map_or("Standard", |s| s.name.as_str())
            )
            .into_owned(),
            Step::FromStyleRows => t!(
                "%{n}  Enter number of additional rows <%{rows}>:",
                n = n,
                rows = self.extra_rows
            )
            .into_owned(),
            Step::FromStyleColumns => t!(
                "%{n}  Enter number of additional columns <%{columns}>:",
                n = n,
                columns = self.extra_columns
            )
            .into_owned(),
            Step::FromStyleOptions => t!(
                "%{n}  Enter an insert option [Label text/Data text/Formulas/fIelds/data liNks/Blocks/cell style Overrides] <exit>:",
                n = n
            )
            .into_owned(),
            Step::FromStyleOption(index) => {
                let on = *template_option(&mut template_options(), index);
                t!(
                    "%{n}  Include %{what} in inserted table? [Yes/No] <%{current}>:",
                    n = n,
                    what = t!(TEMPLATE_OPTION_KEYS[index].2),
                    current = if on { t!("Yes") } else { t!("No") }
                )
                .into_owned()
            }
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Insertion | Step::FirstCorner | Step::SecondCorner(_) | Step::FromStyle => {
                InputKind::Point
            }
            Step::StyleName { .. } | Step::LinkName | Step::FromStyleName => InputKind::FreeText,
            _ => InputKind::SingleToken,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim();
        let error = |message: &str| Some(CmdResult::ReportError(t!(message).into_owned()));
        match self.step {
            Step::Columns | Step::Rows => {
                let columns = self.step == Step::Columns;
                let value = if input.is_empty() {
                    Some(if columns {
                        self.columns.unwrap_or(1)
                    } else {
                        session().data_rows
                    })
                } else if keyword(input, "AUTO") {
                    None
                } else if columns && keyword(input, "STYLE") {
                    // The current style's starting table, else the first
                    // style that has one.
                    let current = self
                        .style
                        .as_ref()
                        .filter(|choice| choice.template.is_some())
                        .cloned()
                        .or_else(|| self.template_styles().next().cloned());
                    let Some(choice) = current else {
                        return Some(CmdResult::Measurement(
                            t!("There are no table styles containing tables.  Add a table to a table style by using the TABLESTYLE command.").into_owned(),
                        ));
                    };
                    self.template_style = Some(choice);
                    self.step = Step::FromStyle;
                    return Some(CmdResult::NeedPoint);
                } else if columns && keyword(input, "LINK") {
                    if self.links.is_empty() {
                        return Some(CmdResult::Measurement(format!(
                            "{}\n{}",
                            t!("No data links exist in the current drawing."),
                            t!("To create a data link use the DATALINK command.")
                        )));
                    }
                    self.step = Step::LinkName;
                    return Some(CmdResult::NeedPoint);
                } else {
                    match parse_count(input) {
                        Count::Value(value) => Some(value),
                        Count::NotPositive => return error("Value must be positive and nonzero."),
                        Count::Invalid => {
                            return error("Requires a positive nonzero integer or option keyword.")
                        }
                    }
                };
                if columns {
                    self.columns = value;
                    self.step = Step::Rows;
                } else {
                    self.data_rows = value;
                    self.step = self.after_counts();
                }
            }
            Step::Insertion | Step::FirstCorner => {
                let window = self.step == Step::FirstCorner;
                let options = self.placement_options();
                let offered = |word: &str| options.split('/').any(|o| o == word);
                if offered("Style") && keyword(input, "STYLE") {
                    self.step = Step::StyleName { window };
                } else if offered("Width") && keyword(input, "WIDTH") {
                    self.step = Step::Width { window };
                } else if offered("Height") && keyword(input, "HEIGHT") {
                    self.step = Step::Height { window };
                } else if options.is_empty() {
                    return error("Invalid point.");
                } else {
                    return error("Point or option keyword required.");
                }
            }
            Step::SecondCorner(_) => return error("Invalid point."),
            Step::StyleName { window } => {
                self.step = Self::back(window);
                if !input.is_empty() {
                    let Some(choice) = self
                        .styles
                        .iter()
                        .find(|choice| choice.name.eq_ignore_ascii_case(input))
                        .cloned()
                    else {
                        return error("Invalid table style name.");
                    };
                    self.style = Some(choice);
                }
            }
            Step::Width { window } => {
                if !input.is_empty() {
                    match input.parse::<f64>() {
                        Ok(value) if value > 0.0 && value.is_finite() => self.cell_width = value,
                        Ok(_) => return error("Value must be positive and nonzero."),
                        Err(_) => return error("Requires numeric distance or two points."),
                    }
                }
                self.step = Self::back(window);
            }
            Step::Height { window } => {
                if !input.is_empty() {
                    match parse_count(input) {
                        Count::Value(value) => self.row_lines = value,
                        Count::NotPositive => return error("Value must be positive and nonzero."),
                        Count::Invalid => return error("Requires a positive nonzero integer."),
                    }
                }
                self.step = Self::back(window);
            }
            Step::FromStyle => {
                if keyword(input, "STYLE") {
                    self.step = Step::FromStyleName;
                } else if keyword(input, "ROWS") {
                    self.step = Step::FromStyleRows;
                } else if keyword(input, "COLUMNS") {
                    self.step = Step::FromStyleColumns;
                } else if keyword(input, "OPTIONS") {
                    self.step = Step::FromStyleOptions;
                } else {
                    return error("Point or option keyword required.");
                }
            }
            Step::FromStyleName => {
                if input == "?" {
                    let names: Vec<&str> =
                        self.template_styles().map(|choice| choice.name.as_str()).collect();
                    return Some(CmdResult::ReportMeasurement(format!(
                        "{}\n-----------------\n{}",
                        t!("Table Style Name"),
                        names.join("\n")
                    )));
                }
                if !input.is_empty() {
                    let Some(choice) = self
                        .template_styles()
                        .find(|choice| choice.name.eq_ignore_ascii_case(input))
                        .cloned()
                    else {
                        return error("Table style does not contain a table.");
                    };
                    self.template_style = Some(choice);
                }
                self.step = Step::FromStyle;
            }
            Step::FromStyleRows | Step::FromStyleColumns => {
                if !input.is_empty() {
                    let value = match parse_count(input) {
                        Count::Value(value) => value,
                        Count::NotPositive => return error("Value must be positive and nonzero."),
                        Count::Invalid => return error("Requires a positive nonzero integer."),
                    };
                    if self.step == Step::FromStyleRows {
                        self.extra_rows = value;
                    } else {
                        self.extra_columns = value;
                    }
                }
                self.step = Step::FromStyle;
            }
            Step::FromStyleOptions => {
                if input.is_empty() {
                    self.step = Step::FromStyle;
                    return Some(CmdResult::NeedPoint);
                }
                let upper = input.to_ascii_uppercase();
                let picked = TEMPLATE_OPTION_KEYS.iter().position(|(key, word, _)| {
                    upper == *key || (upper.len() > 1 && word.starts_with(&upper))
                });
                match picked {
                    Some(index) => self.step = Step::FromStyleOption(index),
                    None => return error("Invalid option keyword."),
                }
            }
            Step::FromStyleOption(index) => {
                let value = if input.is_empty() {
                    None
                } else if keyword(input, "YES") {
                    Some(true)
                } else if keyword(input, "NO") {
                    Some(false)
                } else {
                    return error("Invalid option keyword.");
                };
                if let Some(value) = value {
                    *template_option(&mut template_options(), index) = value;
                }
                self.step = Step::FromStyleOptions;
            }
            Step::LinkName => {
                if input == "?" {
                    let names: Vec<&str> =
                        self.links.iter().map(|(name, _)| name.as_str()).collect();
                    return Some(CmdResult::ReportMeasurement(format!(
                        "{}\n---------------\n{}",
                        t!("Data Link Name"),
                        names.join("\n")
                    )));
                }
                let Some((_, handle)) = self
                    .links
                    .iter()
                    .find(|(name, _)| !input.is_empty() && name.eq_ignore_ascii_case(input))
                else {
                    return error("Data link not found.");
                };
                let style = self
                    .style
                    .as_ref()
                    .map_or_else(|| "Standard".to_string(), |s| s.name.clone());
                return Some(CmdResult::Dispatch(format!(
                    "-TABLE _LINK {:X} {}",
                    handle.value(),
                    style
                )));
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::Insertion => self.place(point, None),
            Step::FirstCorner => {
                self.step = Step::SecondCorner(point);
                CmdResult::NeedPoint
            }
            Step::SecondCorner(first) => self.place(first, Some(point)),
            Step::FromStyle => match self.template_table(point) {
                Some(table) => CmdResult::CommitAndExit(table),
                None => CmdResult::NeedPoint,
            },
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        if !self.preview_enabled {
            return None;
        }
        match self.step {
            Step::FromStyle => {
                let EntityType::Table(table) = self.template_table(point)? else {
                    return None;
                };
                let widths: Vec<f64> = table.columns.iter().map(|c| c.width).collect();
                let heights: Vec<f64> = table.rows.iter().map(|r| r.height).collect();
                Some(self.grid_wire(point, &widths, &heights))
            }
            Step::Insertion => Some(self.preview_grid(point, None)),
            Step::SecondCorner(first) => Some(self.preview_grid(first, Some(point))),
            _ => None,
        }
    }
}

/// The Table Style editor's "select table to start from": one object is
/// picked and handed back to the window (none when the pick is ended).
pub struct TableTemplatePickCommand;

impl CadCommand for TableTemplatePickCommand {
    fn name(&self) -> &'static str {
        "TABLESTYLE"
    }

    fn prompt(&self) -> String {
        t!("Select a table:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: codec::Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        CmdResult::TableTemplatePick(Some(handle))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::TableTemplatePick(None)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::TableTemplatePick(None)
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["TABLE", "-TABLE"] });

/// A table cell resolved from a pick: grid coordinates plus whether its
/// content is locked against editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TableCellHit {
    pub row: usize,
    pub column: usize,
    pub locked: bool,
}

/// Outcome of launching the table cell editor for a pick (double-click or
/// TABLEDIT). Distinguishes "cell found but locked" — the caller should
/// stop, the indicator is armed — from "nothing to edit" — the caller may
/// fall through to other handlers or re-prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableCellEditStart {
    /// Cell editor launched.
    Started,
    /// Cell resolved but content-locked/read-only: indicator armed, no
    /// editor.
    LockedCell,
    /// Not a table, or the pick missed the grid.
    NoCell,
}

impl TableCellHit {
    /// Flat cell index, matching the Properties palette ordering.
    pub fn index(&self, table: &codec::entities::Table) -> usize {
        self.row * table.column_count() + self.column
    }
}

/// Resolve the table cell under `click_world`. Shared by the double-click
/// shortcut and the TABLEDIT command so both agree on grid coordinates,
/// flow direction and locked-cell detection. `table_style` is the style
/// object the table references, when the caller has the document at hand;
/// the table's embedded base style covers the common fallbacks.
pub(crate) fn table_cell_at(
    table: &codec::entities::Table,
    table_style: Option<&codec::objects::TableStyle>,
    click_world: DVec3,
) -> Option<TableCellHit> {
    let horizontal = glam::DVec3::new(
        table.horizontal_direction.x,
        table.horizontal_direction.y,
        table.horizontal_direction.z,
    )
    .normalize_or(glam::DVec3::X);
    let normal = glam::DVec3::new(table.normal.x, table.normal.y, table.normal.z)
        .normalize_or(glam::DVec3::Z);
    let mut down = horizontal.cross(normal).normalize_or(glam::DVec3::NEG_Y);
    if crate::entities::table::resolved_flow_up(table, table_style) {
        down = -down;
    }
    let origin = glam::DVec3::new(
        table.insertion_point.x,
        table.insertion_point.y,
        table.insertion_point.z,
    );
    let relative = click_world - origin;
    let x = relative.dot(horizontal);
    let y = relative.dot(down);
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let column = table
        .columns
        .iter()
        .scan(0.0, |offset, column| {
            *offset += column.width;
            Some(*offset)
        })
        .position(|end| x <= end)?;
    let row = table
        .rows
        .iter()
        .scan(0.0, |offset, row| {
            *offset += row.height;
            Some(*offset)
        })
        .position(|end| y <= end)?;
    Some(TableCellHit {
        row,
        column,
        locked: cell_locked(table, row, column),
    })
}

/// The cell (its merged owner) whose inside holds `point`, at least `margin`
/// away from the cell's edges: a pick near a grid line takes the table, one
/// inside a cell takes the cell.
pub(crate) fn table_cell_inside(
    table: &codec::entities::Table,
    table_style: Option<&codec::objects::TableStyle>,
    point: DVec3,
    margin: f64,
) -> Option<(usize, usize)> {
    let hit = table_cell_at(table, table_style, point)?;
    let (r0, c0, r1, c1) =
        crate::entities::table::widen_to_merged(table, (hit.row, hit.column, hit.row, hit.column));
    let horizontal = glam::DVec3::new(
        table.horizontal_direction.x,
        table.horizontal_direction.y,
        table.horizontal_direction.z,
    )
    .normalize_or(glam::DVec3::X);
    let normal = glam::DVec3::new(table.normal.x, table.normal.y, table.normal.z)
        .normalize_or(glam::DVec3::Z);
    let mut down = horizontal.cross(normal).normalize_or(glam::DVec3::NEG_Y);
    if crate::entities::table::resolved_flow_up(table, table_style) {
        down = -down;
    }
    let origin = glam::DVec3::new(
        table.insertion_point.x,
        table.insertion_point.y,
        table.insertion_point.z,
    );
    let x = (point - origin).dot(horizontal);
    let y = (point - origin).dot(down);
    let left: f64 = table.columns[..c0].iter().map(|c| c.width).sum();
    let right: f64 = left + table.columns[c0..=c1].iter().map(|c| c.width).sum::<f64>();
    let top: f64 = table.rows[..r0].iter().map(|r| r.height).sum();
    let bottom: f64 = top + table.rows[r0..=r1].iter().map(|r| r.height).sum::<f64>();
    (x > left + margin && x < right - margin && y > top + margin && y < bottom - margin)
        .then_some((r0, c0))
}

/// True when the cell's content may not be edited (locked or read-only).
pub(crate) fn cell_locked(table: &codec::entities::Table, row: usize, column: usize) -> bool {
    use codec::entities::table::CellStateFlags;
    table.cell(row, column).is_some_and(|cell| {
        cell.state
            .intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY)
    })
}

/// TABLEDIT — edit a table cell's text by picking it. The pick only
/// collects (table, point); the host resolves the cell and launches
/// `TableCellEditCommand`, so the command-first flow and the double-click
/// shortcut share one resolver and one editor setup.
pub struct TableditCommand;

impl TableditCommand {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TableditCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl CadCommand for TableditCommand {
    fn name(&self) -> &'static str {
        "TABLEDIT"
    }

    fn prompt(&self) -> String {
        t!("TABLEDIT  Select a table cell:").into_owned()
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        CmdResult::EditTableCell {
            handle: codec::Handle::NULL,
            point,
        }
    }

    // A bare Enter at the pick prompt ends the command (AutoCAD leaves the
    // select-until-valid loop on Enter).
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["TABLEDIT"]
});

pub struct TableCellEditCommand {
    handle: codec::Handle,
    table: codec::entities::Table,
    row: usize,
    column: usize,
}

impl TableCellEditCommand {
    pub fn new(
        handle: codec::Handle,
        table: &codec::entities::Table,
        row: usize,
        column: usize,
    ) -> Self {
        Self {
            handle,
            table: table.clone(),
            row,
            column,
        }
    }

    /// The cell takes the given data type and picture with the text put
    /// in, in the same undo step.
    pub fn with_value_type(mut self, data_type: i32, unit_type: i32, picture: &str) -> Self {
        self.set_value_type(data_type, unit_type, picture);
        self
    }

    fn set_value_type(&mut self, data_type: i32, unit_type: i32, picture: &str) {
        use codec::entities::table::CellStylePropertyFlags as P;
        use crate::entities::table::{cell_data_category, cell_data_kind, CELL_DATA_CATEGORIES};
        // A cell whose type turns into a number aligns Top Right, whatever
        // its table style's alignment (measured with Top Center, Middle
        // Left, Bottom Center, Top Left, Middle Right, Bottom Left); an
        // alignment set on the cell itself stays.
        let before = cell_data_kind(&self.table, self.row, self.column).map(|(kind, _)| CELL_DATA_CATEGORIES[kind].0);
        let after = cell_data_category(data_type, unit_type);
        let own_alignment = self
            .table
            .rows
            .get(self.row)
            .and_then(|row| row.cells.get(self.column))
            .and_then(|cell| cell.style.as_ref())
            .is_some_and(|style| style.sets(P::ALIGNMENT));
        let right = (before != Some(after)
            && !own_alignment
            && matches!(after, "Whole Number" | "Decimal Number" | "Currency" | "Percentage"))
            .then_some(3);
        if let Some(cell) = self.table.cell_mut(self.row, self.column) {
            if let Some(alignment) = right {
                let style = cell.style.get_or_insert_with(Default::default);
                style.alignment = alignment;
                style.property_flags.insert(P::ALIGNMENT);
            }
            let bits = (P::DATA_TYPE | P::DATA_FORMAT).bits() as i32;
            for content in &mut cell.contents {
                content.format_property_flags &= !bits;
                content.format_override_flags &= !bits;
            }
            let style = cell.style.get_or_insert_with(Default::default);
            style.value_data_type = data_type;
            style.value_unit_type = unit_type;
            style.value_format = picture.to_string();
            style.property_flags.insert(P::DATA_TYPE | P::DATA_FORMAT);
        }
    }
}

impl CadCommand for TableCellEditCommand {
    fn table_cell(&self) -> Option<(codec::Handle, usize, usize)> {
        Some((self.handle, self.row, self.column))
    }

    fn name(&self) -> &'static str {
        "TABLE CELL"
    }

    fn prompt(&self) -> String {
        let current = self
            .table
            .cell_text(self.row, self.column)
            .unwrap_or("")
            .replace("\\P", "/n");
        t!(
            "TABLE CELL  Enter text for [%{row},%{column}] <%{current}>:",
            row = self.row,
            column = self.column,
            current = current
        )
        .into_owned()
    }

    // Cell content is free-form prose ("Foo Bar"): Space must type a
    // literal space instead of submitting, Enter finishes, and
    // /n or \n inserts a line break.
    fn input_kind(&self) -> InputKind {
        InputKind::FreeText
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        use codec::entities::table::CellStateFlags;
        // Typed text sets a General cell's type, or a number cell's picture.
        if let Some((data_type, unit_type, picture)) =
            crate::entities::table::typed_entry_type(&self.table, self.row, self.column, text)
        {
            self.set_value_type(data_type, unit_type, &picture);
        }
        let cell = self.table.cell_mut(self.row, self.column)?;
        if cell
            .state
            .intersects(CellStateFlags::CONTENT_LOCKED | CellStateFlags::CONTENT_READ_ONLY)
        {
            return Some(CmdResult::Cancel);
        }
        let formatted = text
            .replace("/n", "\\P")
            .replace("\\n", "\\P")
            .replace('\n', "\\P");
        cell.set_text(&formatted);
        self.table.block_record_handle = None;
        Some(CmdResult::ReplaceMany(
            vec![(
                self.handle,
                vec![EntityType::Table(Box::new(self.table.clone()))],
            )],
            Vec::new(),
        ))
    }

    // Bare Enter (empty buffer) ends the edit *without* writing — the
    // host routes an empty submit here instead of on_text_input(""), so
    // pressing Enter on an untouched cell keeps its current content.
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Done
    }
}

#[cfg(test)]
mod tabledit_tests {
    //! `table_cell_at` is the one resolver shared by the double-click
    //! shortcut and the TABLEDIT pick, so its grid math is pinned here.
    //! `Table::new` seeds 2.5-unit columns and 0.25-unit rows with the
    //! default orientation (horizontal +X, flow down).
    use super::{table_cell_at, TableCellEditCommand, TableCellHit, TableditCommand};
    use crate::command::{CadCommand, CmdResult};
    use codec::entities::table::CellStateFlags;
    use codec::entities::Table;
    use codec::types::Vector3;
    use glam::DVec3;

    fn table_2x2() -> Table {
        Table::new(Vector3::new(0.0, 0.0, 0.0), 2, 2)
    }

    #[test]
    fn resolves_row_and_column_from_point() {
        let table = table_2x2();
        // Centers of the four cells.
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(1.25, -0.125, 0.0)),
            Some(TableCellHit {
                row: 0,
                column: 0,
                locked: false
            })
        );
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(3.75, -0.125, 0.0)),
            Some(TableCellHit {
                row: 0,
                column: 1,
                locked: false
            })
        );
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(1.25, -0.375, 0.0)),
            Some(TableCellHit {
                row: 1,
                column: 0,
                locked: false
            })
        );
    }

    #[test]
    fn misses_outside_the_grid() {
        let table = table_2x2();
        // Before the insertion point, beyond the last column/row, and a
        // point in the plane's empty space all miss.
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(-0.1, -0.1, 0.0)),
            None
        );
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(6.0, -0.1, 0.0)),
            None
        );
        assert_eq!(
            table_cell_at(&table, None, DVec3::new(1.0, -9.0, 0.0)),
            None
        );
        assert_eq!(table_cell_at(&table, None, DVec3::new(1.0, 1.0, 0.0)), None);
    }

    #[test]
    fn reports_locked_cells() {
        let mut table = table_2x2();
        if let Some(cell) = table.cell_mut(0, 1) {
            cell.state |= CellStateFlags::CONTENT_LOCKED;
        }
        let hit = table_cell_at(&table, None, DVec3::new(3.75, -0.125, 0.0))
            .expect("locked cell still resolves");
        assert!(hit.locked);
        // Its neighbour stays editable through the same pick path.
        assert!(
            !table_cell_at(&table, None, DVec3::new(1.25, -0.125, 0.0))
                .expect("neighbour resolves")
                .locked
        );
    }

    #[test]
    fn tabledit_point_hands_point_to_host() {
        let mut cmd = TableditCommand::new();
        assert!(!cmd.needs_entity_pick());
        match cmd.on_point(DVec3::new(1.0, 2.0, 0.0)) {
            CmdResult::EditTableCell { handle, point } => {
                assert_eq!(handle, codec::Handle::NULL);
                assert_eq!(point, DVec3::new(1.0, 2.0, 0.0));
            }
            _ => panic!("expected EditTableCell"),
        }
        // A bare Enter at the pick prompt ends the command.
        assert!(matches!(cmd.on_enter(), CmdResult::Cancel));
    }

    #[test]
    fn table_cell_edit_formats_newlines() {
        let table = table_2x2();
        let handle = codec::Handle::new(42);
        let mut edit_cmd = TableCellEditCommand::new(handle, &table, 0, 0);
        let result = edit_cmd
            .on_text_input("Line1/nLine2\\nLine3\nLine4")
            .expect("cell edit commits");
        match result {
            CmdResult::ReplaceMany(repl, _) => {
                assert_eq!(repl.len(), 1);
                if let codec::EntityType::Table(t) = &repl[0].1[0] {
                    assert_eq!(t.cell_text(0, 0).unwrap(), "Line1\\PLine2\\PLine3\\PLine4");
                } else {
                    panic!("expected Table entity");
                }
            }
            _ => panic!("expected ReplaceMany"),
        }
    }
}

/// Collects table cell picks for a picked-cell tool and hands them to
/// `<name> x y z …` (a host that wants more picks starts it again).
pub struct CellPointsCommand {
    name: &'static str,
    prompts: Vec<&'static str>,
    points: Vec<DVec3>,
}

impl CellPointsCommand {
    pub fn new(name: &'static str, prompts: Vec<&'static str>) -> Self {
        Self {
            name,
            prompts,
            points: Vec::new(),
        }
    }
}

impl CadCommand for CellPointsCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        let prompt = self.prompts[self.points.len().min(self.prompts.len() - 1)];
        t!(prompt).into_owned()
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        self.points.push(point);
        if self.points.len() < self.prompts.len() {
            return CmdResult::NeedPoint;
        }
        let points: Vec<String> = self
            .points
            .iter()
            .map(|p| format!("{} {} {}", p.x, p.y, p.z))
            .collect();
        CmdResult::Dispatch(format!("{} {}", self.name, points.join(" ")))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
