//! Dialogs used by the Annotate tab's table and drawing-data tools.
//!
//! The working state lives outside the drawing until the user presses OK or
//! Finish.  That keeps Cancel lossless and lets the wizard validate one page
//! before advancing to the next.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::fmt;

use codec::types::Handle;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Border, Element, Fill, Length, Theme};

use crate::app::Message;
use crate::t;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt, form_radio};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableSource {
    #[default]
    Empty,
    DataLink,
    DataExtraction,
}

impl fmt::Display for TableSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Empty => t!("Empty table").into_owned(),
            Self::DataLink => t!("Data link").into_owned(),
            Self::DataExtraction => t!("Data extraction").into_owned(),
        };
        f.write_str(&label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TableInsertion {
    #[default]
    Point,
    Window,
}

impl fmt::Display for TableInsertion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Point => t!("Specify insertion point").into_owned(),
            Self::Window => t!("Specify window").into_owned(),
        };
        f.write_str(&label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataLinkChoice {
    pub handle: Handle,
    pub name: String,
    pub path: String,
    /// What the link takes from its source (sheet, range…).
    pub details: String,
    pub valid: bool,
}

impl fmt::Display for DataLinkChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.valid {
            write!(f, "🔗 {}", self.name)
        } else {
            write!(f, "⚠ {}", self.name)
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableInsertState {
    pub styles: Vec<String>,
    pub style: String,
    pub source: TableSource,
    pub links: Vec<DataLinkChoice>,
    pub link: Option<DataLinkChoice>,
    pub insertion: TableInsertion,
    pub columns: String,
    pub data_rows: String,
    pub column_width: String,
    /// Row height, in lines of the row's text.
    pub row_height: String,
    /// Window insertion keeps the column count (else the column width).
    pub columns_fixed: bool,
    /// Window insertion keeps the data row count (else the row height).
    pub rows_fixed: bool,
    pub preview: bool,
    pub first_row_style: String,
    pub second_row_style: String,
    pub other_row_style: String,
    pub error: String,
}

impl Default for TableInsertState {
    fn default() -> Self {
        Self {
            styles: vec!["Standard".into()],
            style: "Standard".into(),
            source: TableSource::Empty,
            links: Vec::new(),
            link: None,
            insertion: TableInsertion::Point,
            columns: "5".into(),
            data_rows: "1".into(),
            column_width: "63.5".into(),
            row_height: "1".into(),
            columns_fixed: true,
            rows_fixed: true,
            preview: true,
            first_row_style: "Title".into(),
            second_row_style: "Header".into(),
            other_row_style: "Data".into(),
            error: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum TableInsertField {
    Style(String),
    Source(TableSource),
    Link(DataLinkChoice),
    Insertion(TableInsertion),
    Columns(String),
    DataRows(String),
    ColumnWidth(String),
    RowHeight(String),
    ColumnsFixed(bool),
    RowsFixed(bool),
    Preview(bool),
    FirstRowStyle(String),
    SecondRowStyle(String),
    OtherRowStyle(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkPathType {
    #[default]
    Full,
    Relative,
    FileName,
}

impl fmt::Display for LinkPathType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Full => t!("Full path").into_owned(),
            Self::Relative => t!("Relative path").into_owned(),
            Self::FileName => t!("No path").into_owned(),
        };
        f.write_str(&label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LinkRange {
    #[default]
    EntireSheet,
    NamedRange,
    CellRange,
}

impl fmt::Display for LinkRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::EntireSheet => t!("Entire sheet").into_owned(),
            Self::NamedRange => t!("Named range").into_owned(),
            Self::CellRange => t!("Cell range").into_owned(),
        };
        f.write_str(&label)
    }
}

#[derive(Debug, Clone)]
pub struct DataLinkManagerState {
    pub links: Vec<DataLinkChoice>,
    pub selected: Option<DataLinkChoice>,
    pub editing: bool,
    pub editing_handle: Option<Handle>,
    pub name: String,
    pub path: String,
    pub path_type: LinkPathType,
    pub range_kind: LinkRange,
    pub range: String,
    pub sheets: Vec<String>,
    pub sheet: String,
    pub named_ranges: Vec<String>,
    pub allow_write: bool,
    pub use_source_formatting: bool,
    pub update_source_formatting: bool,
    pub insert_table: bool,
    pub more_options: bool,
    pub preview: Vec<Vec<String>>,
    pub status: String,
}

impl Default for DataLinkManagerState {
    fn default() -> Self {
        Self {
            links: Vec::new(),
            selected: None,
            editing: false,
            editing_handle: None,
            name: String::new(),
            path: String::new(),
            path_type: LinkPathType::Full,
            range_kind: LinkRange::EntireSheet,
            range: String::new(),
            sheets: Vec::new(),
            sheet: String::new(),
            named_ranges: Vec::new(),
            allow_write: false,
            use_source_formatting: true,
            update_source_formatting: true,
            insert_table: false,
            more_options: false,
            preview: Vec::new(),
            status: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum DataLinkField {
    Name(String),
    Path(String),
    PathType(LinkPathType),
    RangeKind(LinkRange),
    Range(String),
    Sheet(String),
    AllowWrite(bool),
    UseSourceFormatting(bool),
    UpdateSourceFormatting(bool),
    InsertTable(bool),
    MoreOptions(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionPage {
    #[default]
    Begin,
    Source,
    Objects,
    Properties,
    Refine,
    Output,
    TableStyle,
    Finish,
}

impl ExtractionPage {
    pub const fn number(self) -> usize {
        match self {
            Self::Begin => 1,
            Self::Source => 2,
            Self::Objects => 3,
            Self::Properties => 4,
            Self::Refine => 5,
            Self::Output => 6,
            Self::TableStyle => 7,
            Self::Finish => 8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionBegin {
    #[default]
    New,
    Template,
    Edit,
}

impl fmt::Display for ExtractionBegin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::New => t!("Create a new data extraction").into_owned(),
            Self::Template => t!("Use a previous extraction as a template").into_owned(),
            Self::Edit => t!("Edit an existing data extraction").into_owned(),
        };
        f.write_str(&label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtractionSource {
    #[default]
    CurrentDrawing,
    CurrentSelection,
    DrawingsAndFolders,
}

impl fmt::Display for ExtractionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::CurrentDrawing => t!("Current drawing").into_owned(),
            Self::CurrentSelection => t!("Selected objects in current drawing").into_owned(),
            Self::DrawingsAndFolders => t!("Drawings / folders").into_owned(),
        };
        f.write_str(&label)
    }
}

#[derive(Debug, Clone)]
pub struct ExtractionObject {
    pub name: String,
    pub checked: bool,
    pub count: usize,
    /// A block reference, listed by its block name.
    pub block: bool,
    /// What the object offers: `(key, display name, category)`.
    pub properties: Vec<(String, String, String)>,
}

#[derive(Debug, Clone)]
pub struct ExtractionProperty {
    pub key: String,
    pub name: String,
    pub category: String,
    pub checked: bool,
}

#[derive(Debug, Clone)]
pub struct DataExtractionState {
    pub page: ExtractionPage,
    pub begin: ExtractionBegin,
    pub settings_path: String,
    pub source: ExtractionSource,
    pub include_current: bool,
    pub include_subfolders: bool,
    pub source_files: Vec<String>,
    pub selection_handles: Vec<Handle>,
    pub objects: Vec<ExtractionObject>,
    /// Object list filters: every type, or blocks / non-blocks only.
    pub display_all_types: bool,
    pub blocks_only: bool,
    pub attributed_blocks_only: bool,
    pub in_use_only: bool,
    pub properties: Vec<ExtractionProperty>,
    pub categories: BTreeSet<String>,
    /// Categories the user filtered out of the property list.
    pub hidden_categories: BTreeSet<String>,
    pub combine_identical: bool,
    pub show_count: bool,
    pub show_name: bool,
    pub preview: Vec<Vec<String>>,
    pub output_table: bool,
    pub output_file: bool,
    pub output_path: String,
    pub table_styles: Vec<String>,
    pub table_style: String,
    pub table_title: String,
    pub title_cell_style: String,
    pub header_cell_style: String,
    pub data_cell_style: String,
    pub property_headers: bool,
    /// Object names and property keys a loaded settings file chose; they
    /// decide what starts checked once the objects are listed.
    pub saved_objects: Option<BTreeSet<String>>,
    pub saved_properties: Option<BTreeSet<String>>,
    pub error: String,
}

impl Default for DataExtractionState {
    fn default() -> Self {
        Self {
            page: ExtractionPage::Begin,
            begin: ExtractionBegin::New,
            settings_path: String::new(),
            source: ExtractionSource::DrawingsAndFolders,
            include_current: true,
            include_subfolders: true,
            source_files: Vec::new(),
            selection_handles: Vec::new(),
            objects: Vec::new(),
            display_all_types: true,
            blocks_only: false,
            attributed_blocks_only: false,
            in_use_only: true,
            properties: Vec::new(),
            categories: BTreeSet::new(),
            hidden_categories: BTreeSet::new(),
            combine_identical: true,
            show_count: true,
            show_name: true,
            preview: Vec::new(),
            output_table: false,
            output_file: false,
            output_path: String::new(),
            table_styles: vec!["Standard".into()],
            table_style: "Standard".into(),
            table_title: String::new(),
            title_cell_style: "Title".into(),
            header_cell_style: "Header".into(),
            data_cell_style: "Data".into(),
            property_headers: true,
            saved_objects: None,
            saved_properties: None,
            error: String::new(),
        }
    }
}

impl DataExtractionState {
    /// Whether an object passes the object page's display options.
    pub fn shows_object(&self, object: &ExtractionObject) -> bool {
        if self.display_all_types {
            return !self.attributed_blocks_only || object.block;
        }
        object.block == self.blocks_only && (!self.attributed_blocks_only || object.block)
    }
}

#[derive(Debug, Clone)]
pub enum DataExtractionField {
    Begin(ExtractionBegin),
    SettingsPath(String),
    Source(ExtractionSource),
    IncludeCurrent(bool),
    IncludeSubfolders(bool),
    Object(usize, bool),
    Property(usize, bool),
    Category(String, bool),
    CombineIdentical(bool),
    ShowCount(bool),
    ShowName(bool),
    OutputTable(bool),
    OutputFile(bool),
    OutputPath(String),
    TableStyle(String),
    TableTitle(String),
    DisplayAllTypes(bool),
    BlocksOnly(bool),
    AttributedBlocksOnly(bool),
    InUseOnly(bool),
    TitleCellStyle(String),
    HeaderCellStyle(String),
    DataCellStyle(String),
    PropertyHeaders(bool),
}

fn muted(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.62)),
    }
}

fn group<'a>(title: impl Into<String>, body: Element<'a, Message>) -> Element<'a, Message> {
    container(column![text(title.into()).size(11).style(muted), body].spacing(7))
        .padding(9)
        .width(Fill)
        .style(|theme: &Theme| container::Style {
            border: Border {
                width: 1.0,
                radius: 4.0.into(),
                color: theme.palette().background.strong.color,
            },
            ..Default::default()
        })
        .into()
}

fn labeled_input<'a>(
    label: impl Into<String>,
    value: &'a str,
    msg: fn(String) -> Message,
) -> Element<'a, Message> {
    row![
        text(label.into()).size(11).style(muted).width(118),
        text_input("", value)
            .on_input(msg)
            .padding([4, 7])
            .size(12)
            .width(Fill)
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn cell_style_picker<'a>(
    label: impl Into<String>,
    selected: &'a str,
    msg: fn(String) -> Message,
) -> Element<'a, Message> {
    row![
        text(label.into()).size(11).style(muted).width(118),
        pick_list(
            Some(selected.to_string()),
            vec![
                "Title".to_string(),
                "Header".to_string(),
                "Data".to_string()
            ],
            |value| value.clone(),
        )
        .on_select(msg)
        .padding([3, 6])
        .text_size(12)
        .width(Fill),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn radio_button<'a>(
    selected: bool,
    label: impl Into<String>,
    msg: Message,
) -> Element<'a, Message> {
    form_radio(label, true, selected.then_some(true), move |_| msg.clone()).into()
}

fn preview_grid<'a>(rows: &'a [Vec<String>]) -> Element<'a, Message> {
    if rows.is_empty() {
        return text(t!("No preview available."))
            .size(11)
            .style(muted)
            .into();
    }
    let lines = rows
        .iter()
        .take(8)
        .map(|row| row.join("  |  "))
        .collect::<Vec<_>>()
        .join("\n");
    container(text(lines).size(11))
        .padding(6)
        .width(Fill)
        .into()
}

/// The Insert Table dialog's sample: a title, a header and data rows.
struct InsertTablePreview;

impl iced::widget::canvas::Program<Message> for InsertTablePreview {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<iced::widget::canvas::Geometry> {
        use iced::widget::canvas::{Frame, Path, Stroke, Text};
        use iced::Point;
        let mut frame = Frame::new(renderer, bounds.size());
        let ink = theme.palette().background.base.text;
        frame.fill(
            &Path::rectangle(Point::ORIGIN, bounds.size()),
            theme.palette().background.weak.color,
        );
        let (left, top) = (12.0, 8.0);
        let (width, height) = (bounds.width - 24.0, bounds.height - 16.0);
        let rows = 6usize;
        let row_h = height / rows as f32;
        let col_w = width / 3.0;
        let stroke = Stroke::default()
            .with_color(ink.scale_alpha(0.8))
            .with_width(1.0);
        for k in 0..=rows {
            let y = top + k as f32 * row_h;
            frame.stroke(&Path::line(Point::new(left, y), Point::new(left + width, y)), stroke);
        }
        for k in 0..=3 {
            let x = left + k as f32 * col_w;
            // The title row spans the table.
            let y0 = if k == 0 || k == 3 { top } else { top + row_h };
            frame.stroke(&Path::line(Point::new(x, y0), Point::new(x, top + height)), stroke);
        }
        let label = |frame: &mut Frame, content: String, x: f32, y: f32, size: f32| {
            frame.fill_text(Text {
                content,
                position: Point::new(x, y),
                color: ink,
                size: iced::Pixels(size),
                align_x: iced::advanced::text::Alignment::Center,
                align_y: iced::alignment::Vertical::Center,
                ..Default::default()
            });
        };
        label(&mut frame, t!("Title").into_owned(), left + width / 2.0, top + row_h / 2.0, 13.0);
        for row in 1..rows {
            let content = if row == 1 { t!("Header") } else { t!("Data") }.into_owned();
            for column in 0..3 {
                label(
                    &mut frame,
                    content.clone(),
                    left + (column as f32 + 0.5) * col_w,
                    top + (row as f32 + 0.5) * row_h,
                    11.0,
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

pub fn table_insert_view<'a>(
    state: &'a TableInsertState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let style = pick_list(Some(state.style.clone()), state.styles.clone(), |value| {
        value.clone()
    })
    .on_select(Message::TableInsertStyle)
    .padding([3, 6])
    .text_size(12)
    .width(Fill);
    let source = column![
        radio_button(
            state.source == TableSource::Empty,
            t!("Start from empty table"),
            Message::TableInsertField(TableInsertField::Source(TableSource::Empty))
        ),
        radio_button(
            state.source == TableSource::DataLink,
            t!("Start from data link"),
            Message::TableInsertField(TableInsertField::Source(TableSource::DataLink))
        ),
        radio_button(
            state.source == TableSource::DataExtraction,
            t!("Start from data extraction"),
            Message::TableInsertField(TableInsertField::Source(TableSource::DataExtraction))
        ),
    ]
    .spacing(1);
    let link_picker: Element<'_, Message> = if state.source == TableSource::DataLink {
        row![
            text(t!("Data link")).size(11).style(muted).width(118),
            pick_list(state.link.clone(), state.links.clone(), |value| value
                .to_string())
            .on_select(|value| Message::TableInsertField(TableInsertField::Link(value)))
            .padding([3, 6])
            .text_size(12)
            .width(Fill),
            button(text(t!("Manage…")).size(11))
                .on_press(Message::DataLinkManagerOpen)
                .padding([4, 8]),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into()
    } else {
        Space::new().height(0).into()
    };
    let insertion = column![
        radio_button(
            state.insertion == TableInsertion::Point,
            t!("Specify insertion point"),
            Message::TableInsertField(TableInsertField::Insertion(TableInsertion::Point))
        ),
        radio_button(
            state.insertion == TableInsertion::Window,
            t!("Specify window"),
            Message::TableInsertField(TableInsertField::Insertion(TableInsertion::Window))
        ),
    ];
    // In window mode one value of each pair comes from the window ("Auto");
    // the dot marks the one kept.
    let window = state.insertion == TableInsertion::Window;
    let setting = |label: Cow<'static, str>,
                   value: &'a str,
                   kept: bool,
                   keep: Message,
                   suffix: Option<Cow<'static, str>>,
                   msg: fn(String) -> Message|
     -> Element<'a, Message> {
        let input: Element<'a, Message> = if window && !kept {
            text_input("", &t!("Auto"))
                .padding([4, 7])
                .size(12)
                .width(Fill)
                .into()
        } else {
            text_input("", value)
                .on_input(msg)
                .padding([4, 7])
                .size(12)
                .width(Fill)
                .into()
        };
        let mut line = row![].spacing(6).align_y(iced::Center);
        if window {
            line = line.push(form_radio("", true, kept.then_some(true), move |_| keep.clone()));
        }
        line = line.push(text(label).size(11).style(muted).width(92)).push(input);
        if let Some(suffix) = suffix {
            line = line.push(text(suffix).size(11).style(muted));
        }
        line.into()
    };
    let settings = column![
        setting(
            t!("Columns"),
            &state.columns,
            state.columns_fixed,
            Message::TableInsertField(TableInsertField::ColumnsFixed(true)),
            None,
            |v| Message::TableInsertField(TableInsertField::Columns(v))
        ),
        setting(
            t!("Column width"),
            &state.column_width,
            !state.columns_fixed,
            Message::TableInsertField(TableInsertField::ColumnsFixed(false)),
            None,
            |v| Message::TableInsertField(TableInsertField::ColumnWidth(v))
        ),
        setting(
            t!("Data rows"),
            &state.data_rows,
            state.rows_fixed,
            Message::TableInsertField(TableInsertField::RowsFixed(true)),
            None,
            |v| Message::TableInsertField(TableInsertField::DataRows(v))
        ),
        setting(
            t!("Row height"),
            &state.row_height,
            !state.rows_fixed,
            Message::TableInsertField(TableInsertField::RowsFixed(false)),
            Some(t!("Line(s)")),
            |v| Message::TableInsertField(TableInsertField::RowHeight(v))
        ),
    ]
    .spacing(5);
    let cell_styles = column![
        cell_style_picker(t!("First row"), &state.first_row_style, |v| {
            Message::TableInsertField(TableInsertField::FirstRowStyle(v))
        }),
        cell_style_picker(t!("Second row"), &state.second_row_style, |v| {
            Message::TableInsertField(TableInsertField::SecondRowStyle(v))
        }),
        cell_style_picker(t!("All other rows"), &state.other_row_style, |v| {
            Message::TableInsertField(TableInsertField::OtherRowStyle(v))
        }),
    ]
    .spacing(5);
    // Left: what the table starts from and its sample; right: how it is
    // placed and sized, and the row styles.
    let preview: Element<'a, Message> = if state.preview {
        iced::widget::canvas(InsertTablePreview)
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        Space::new().height(Fill).into()
    };
    let left = column![
        group(
            t!("Table style"),
            row![
                style,
                button(text(t!("Table Style…")).size(11))
                    .on_press(Message::TableInsertManageStyles)
                    .padding([4, 8]),
            ]
            .spacing(8)
            .align_y(iced::Center)
            .into()
        ),
        group(
            t!("Insert options"),
            column![source, link_picker].spacing(4).into()
        ),
        checkbox(state.preview)
            .label(t!("Preview"))
            .on_toggle(|v| Message::TableInsertField(TableInsertField::Preview(v)))
            .size(14),
        preview,
    ]
    .spacing(8)
    .width(Fill);
    let right = column![
        group(t!("Insertion behavior"), insertion.into()),
        group(t!("Column & row settings"), settings.into()),
        group(t!("Set cell styles"), cell_styles.into()),
    ]
    .spacing(8)
    .width(Fill);
    column![
        row![left, right].spacing(10).height(Fill),
        text(&state.error).size(11),
        row![
            Space::new().width(Fill),
            dialog_button(t!("Cancel"), Message::CloseModal, false),
            dialog_button(t!("OK"), Message::TableInsertApply, true),
        ]
        .spacing(8),
    ]
    .spacing(8)
    .padding(10)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

pub fn data_link_view(
    state: &DataLinkManagerState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'_, Message> {
    let mut links = column![text(t!("Excel Links")).size(11).style(muted)].spacing(2);
    links = links.push(
        button(text(t!("Create a new Excel Data Link")).size(12))
            .on_press(Message::DataLinkNew)
            .padding([5, 7])
            .style(button::text)
            .width(Fill),
    );
    for link in &state.links {
        let selected = state
            .selected
            .as_ref()
            .is_some_and(|v| v.handle == link.handle);
        let label = if selected {
            format!("● {link}")
        } else {
            format!("  {link}")
        };
        links = links.push(
            button(text(label).size(12))
                .on_press(Message::DataLinkSelect(link.handle))
                .padding([5, 7])
                .style(button::text)
                .width(Fill),
        );
    }
    let tree = container(scrollable(links).height(Fill))
        .padding(6)
        .width(Length::Fixed(220.0))
        .height(Fill);
    let right: Element<'_, Message> = if state.editing {
        let mut editor = column![
            labeled_input(t!("Data link name"), &state.name, |v| {
                Message::DataLinkField(DataLinkField::Name(v))
            }),
            row![
                text(t!("File")).size(11).style(muted).width(118),
                text_input("", &state.path)
                    .on_input(|v| Message::DataLinkField(DataLinkField::Path(v)))
                    .padding([4, 7])
                    .size(12)
                    .width(Fill),
                button(text("…"))
                    .on_press(Message::DataLinkBrowse)
                    .padding([4, 9])
            ]
            .spacing(6)
            .align_y(iced::Center),
            row![
                text(t!("Path type")).size(11).style(muted).width(118),
                pick_list(
                    Some(state.path_type),
                    vec![
                        LinkPathType::Full,
                        LinkPathType::Relative,
                        LinkPathType::FileName
                    ],
                    |value| value.to_string()
                )
                .on_select(|value| Message::DataLinkField(DataLinkField::PathType(value)))
                .width(Fill)
                .padding([3, 6])
            ]
            .spacing(8),
            row![
                text(t!("Link option")).size(11).style(muted).width(118),
                pick_list(
                    Some(state.range_kind),
                    vec![
                        LinkRange::EntireSheet,
                        LinkRange::NamedRange,
                        LinkRange::CellRange
                    ],
                    |value| value.to_string()
                )
                .on_select(|value| Message::DataLinkField(DataLinkField::RangeKind(value)))
                .width(Fill)
                .padding([3, 6])
            ]
            .spacing(8),
        ]
        .spacing(6);
        if !state.sheets.is_empty() && state.range_kind != LinkRange::NamedRange {
            editor = editor.push(
                row![
                    text(t!("Sheet")).size(11).style(muted).width(118),
                    pick_list(Some(state.sheet.clone()), state.sheets.clone(), |value| {
                        value.clone()
                    })
                    .on_select(|value| Message::DataLinkField(DataLinkField::Sheet(value)))
                    .width(Fill)
                    .padding([3, 6]),
                ]
                .spacing(8),
            );
        }
        if state.range_kind == LinkRange::NamedRange && !state.named_ranges.is_empty() {
            editor = editor.push(
                row![
                    text(t!("Named range")).size(11).style(muted).width(118),
                    pick_list(
                        Some(state.range.clone()),
                        state.named_ranges.clone(),
                        |value| value.clone()
                    )
                    .on_select(|value| Message::DataLinkField(DataLinkField::Range(value)))
                    .width(Fill)
                    .padding([3, 6]),
                ]
                .spacing(8),
            );
        } else if state.range_kind != LinkRange::EntireSheet {
            editor = editor.push(labeled_input(t!("Range"), &state.range, |v| {
                Message::DataLinkField(DataLinkField::Range(v))
            }));
        }
        editor = editor.push(
            checkbox(state.more_options)
                .label(t!("More options"))
                .on_toggle(|v| Message::DataLinkField(DataLinkField::MoreOptions(v)))
                .size(14),
        );
        if state.more_options {
            editor = editor
                .push(
                    checkbox(state.allow_write)
                        .label(t!("Allow writing to source file"))
                        .on_toggle(|v| Message::DataLinkField(DataLinkField::AllowWrite(v)))
                        .size(14),
                )
                .push(
                    checkbox(state.use_source_formatting)
                        .label(t!("Use source formatting"))
                        .on_toggle(|v| {
                            Message::DataLinkField(DataLinkField::UseSourceFormatting(v))
                        })
                        .size(14),
                )
                .push(
                    checkbox(state.update_source_formatting)
                        .label(t!("Keep table updated to source formatting"))
                        .on_toggle(|v| {
                            Message::DataLinkField(DataLinkField::UpdateSourceFormatting(v))
                        })
                        .size(14),
                );
        }
        editor = editor.push(
            checkbox(state.insert_table)
                .label(t!("Insert table after saving link"))
                .on_toggle(|v| Message::DataLinkField(DataLinkField::InsertTable(v)))
                .size(14),
        );
        group(t!("Link settings"), editor.into())
    } else if let Some(link) = &state.selected {
        column![
            group(
                t!("Details"),
                column![
                    text(format!("{} {}", t!("Link name:"), link.name)).size(11),
                    text(format!("{} {}", t!("File name:"), link.path)).size(11),
                    text(format!("{} {}", t!("Link details:"), link.details)).size(11),
                    text(if link.valid {
                        t!("Status: Linked")
                    } else {
                        t!("Status: Source not found")
                    })
                    .size(11)
                ]
                .spacing(5)
                .into()
            ),
            group(t!("Preview"), preview_grid(&state.preview)),
        ]
        .spacing(8)
        .into()
    } else {
        group(
            t!("Details"),
            text(t!("Select a data link or create a new one."))
                .size(11)
                .style(muted)
                .into(),
        )
    };
    let actions = if state.editing {
        row![
            Space::new().width(Fill),
            dialog_button(t!("Cancel"), Message::DataLinkEditCancel, false),
            dialog_button(t!("OK"), Message::DataLinkSave, true),
        ]
        .spacing(8)
    } else {
        let enabled = state.selected.is_some();
        let edit = dialog_button_styled_opt(
            t!("Edit"),
            enabled.then_some(Message::DataLinkEdit),
            button::secondary,
        );
        let delete = dialog_button_styled_opt(
            t!("Delete"),
            enabled.then_some(Message::DataLinkDelete),
            button::secondary,
        );
        let insert = dialog_button_styled_opt(
            t!("Insert Table"),
            enabled.then_some(Message::DataLinkInsert),
            button::primary,
        );
        let close = dialog_button(t!("Close"), Message::DataLinkClose, false);
        row![edit, delete, Space::new().width(Fill), insert, close].spacing(8)
    };
    column![
        row![tree, container(right).width(Fill).height(Fill)]
            .spacing(8)
            .height(Fill),
        text(&state.status).size(11),
        actions
    ]
    .spacing(7)
    .padding(10)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

fn wizard_nav(state: &DataExtractionState) -> iced::widget::Row<'_, Message> {
    let back = dialog_button_styled_opt(
        t!("Back"),
        (state.page != ExtractionPage::Begin).then_some(Message::DataExtractionBack),
        button::secondary,
    );
    let cancel = dialog_button(t!("Cancel"), Message::CloseModal, false);
    let next = dialog_button_styled_opt(
        if state.page == ExtractionPage::Finish {
            t!("Finish")
        } else {
            t!("Next")
        },
        state
            .error
            .is_empty()
            .then_some(if state.page == ExtractionPage::Finish {
                Message::DataExtractionFinish
            } else {
                Message::DataExtractionNext
            }),
        button::primary,
    );
    row![Space::new().width(Fill), back, cancel, next].spacing(8)
}

pub fn data_extraction_view(
    state: &DataExtractionState,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'_, Message> {
    let add_folder = button(text(t!("Add folder…")).size(12)).padding([5, 10]);
    #[cfg(not(target_arch = "wasm32"))]
    let add_folder = add_folder.on_press(Message::DataExtractionAddFolder);
    // The web has no folder picker and no directory to walk, so the button
    // stays disabled there — with a tooltip that says why and what to do.
    #[cfg(target_arch = "wasm32")]
    let add_folder = iced::widget::tooltip(
        add_folder,
        container(
            text(t!(
                "Adding folders is not available in the browser. Add drawing files instead."
            ))
            .size(11),
        )
        .padding([3, 6])
        .style(container::bordered_box),
        iced::widget::tooltip::Position::Top,
    )
    .gap(4);

    let body: Element<'_, Message> = match state.page {
        ExtractionPage::Begin => group(
            t!("Begin"),
            column![
                radio_button(
                    state.begin == ExtractionBegin::New,
                    t!("Create a new data extraction"),
                    Message::DataExtractionField(DataExtractionField::Begin(ExtractionBegin::New))
                ),
                radio_button(
                    state.begin == ExtractionBegin::Template,
                    t!("Use a previous extraction as a template (.dxex)"),
                    Message::DataExtractionField(DataExtractionField::Begin(
                        ExtractionBegin::Template
                    ))
                ),
                radio_button(
                    state.begin == ExtractionBegin::Edit,
                    t!("Edit an existing data extraction"),
                    Message::DataExtractionField(DataExtractionField::Begin(ExtractionBegin::Edit))
                ),
                row![
                    text(t!("Extraction settings"))
                        .size(11)
                        .style(muted)
                        .width(118),
                    text_input("", &state.settings_path)
                        .on_input(|v| Message::DataExtractionField(
                            DataExtractionField::SettingsPath(v)
                        ))
                        .padding([4, 7])
                        .size(12)
                        .width(Fill),
                    button(text("…"))
                        .on_press(Message::DataExtractionBrowseSettings)
                        .padding([4, 9])
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(5)
            .into(),
        ),
        ExtractionPage::Source => group(
            t!("Define data source"),
            column![
                radio_button(
                    state.source != ExtractionSource::CurrentSelection,
                    t!("Drawings/Sheet set"),
                    Message::DataExtractionField(DataExtractionField::Source(
                        ExtractionSource::DrawingsAndFolders
                    ))
                ),
                row![
                    Space::new().width(22),
                    checkbox(state.include_current)
                        .label(t!("Include current drawing"))
                        .on_toggle(|v| Message::DataExtractionField(
                            DataExtractionField::IncludeCurrent(v)
                        ))
                        .size(14),
                ],
                radio_button(
                    state.source == ExtractionSource::CurrentSelection,
                    t!("Select objects in the current drawing"),
                    Message::DataExtractionField(DataExtractionField::Source(
                        ExtractionSource::CurrentSelection
                    ))
                ),
                text(t!("Drawing files and folders:")).size(11).style(muted),
                container(scrollable(
                    text(if state.source_files.is_empty() {
                        t!("Only the current drawing.").into_owned()
                    } else {
                        state.source_files.join("\n")
                    })
                    .size(11)
                ))
                .padding(6)
                .height(Length::Fixed(110.0))
                .width(Fill)
                .style(|theme: &Theme| container::Style {
                    border: Border {
                        width: 1.0,
                        radius: 4.0.into(),
                        color: theme.palette().background.strong.color,
                    },
                    ..Default::default()
                }),
                row![
                    button(text(t!("Add drawings…")).size(12))
                        .on_press(Message::DataExtractionAddDrawings)
                        .padding([5, 10]),
                    add_folder,
                    button(text(t!("Remove all")).size(12))
                        .on_press(Message::DataExtractionClearSources)
                        .padding([5, 10]),
                    checkbox(state.include_subfolders)
                        .label(t!("Include subfolders"))
                        .on_toggle(|v| Message::DataExtractionField(
                            DataExtractionField::IncludeSubfolders(v)
                        ))
                        .size(14),
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(6)
            .into(),
        ),
        ExtractionPage::Objects => {
            let header = row![
                Space::new().width(22),
                text(t!("Object")).size(11).style(muted).width(Fill),
                text(t!("Display Name")).size(11).style(muted).width(Fill),
                text(t!("Type")).size(11).style(muted).width(110),
            ]
            .spacing(6);
            let mut list = column![].spacing(3);
            for (i, item) in state.objects.iter().enumerate() {
                if !state.shows_object(item) {
                    continue;
                }
                list = list.push(
                    row![
                        checkbox(item.checked)
                            .on_toggle(move |v| {
                                Message::DataExtractionField(DataExtractionField::Object(i, v))
                            })
                            .size(14),
                        text(format!("{}  ({})", item.name, item.count))
                            .size(12)
                            .width(Fill),
                        text(item.name.as_str()).size(12).width(Fill),
                        text(if item.block {
                            t!("Block")
                        } else {
                            t!("Non-block")
                        })
                        .size(12)
                        .width(110),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                );
            }
            let options = row![
                column![
                    checkbox(state.display_all_types)
                        .label(t!("Display all object types"))
                        .on_toggle(|v| Message::DataExtractionField(
                            DataExtractionField::DisplayAllTypes(v)
                        ))
                        .size(14),
                    row![
                        Space::new().width(22),
                        form_radio(
                            t!("Display blocks only"),
                            true,
                            Some(state.blocks_only),
                            |value| Message::DataExtractionField(DataExtractionField::BlocksOnly(value)),
                        ),
                    ],
                    row![
                        Space::new().width(22),
                        form_radio(
                            t!("Display non-blocks only"),
                            false,
                            Some(state.blocks_only),
                            |value| Message::DataExtractionField(DataExtractionField::BlocksOnly(value)),
                        ),
                    ],
                ]
                .spacing(3)
                .width(Fill),
                column![
                    checkbox(state.attributed_blocks_only)
                        .label(t!("Display blocks with attributes only"))
                        .on_toggle(|v| Message::DataExtractionField(
                            DataExtractionField::AttributedBlocksOnly(v)
                        ))
                        .size(14),
                    checkbox(state.in_use_only)
                        .label(t!("Display objects currently in-use only"))
                        .on_toggle(|v| Message::DataExtractionField(
                            DataExtractionField::InUseOnly(v)
                        ))
                        .size(14),
                ]
                .spacing(3)
                .width(Fill),
            ]
            .spacing(10);
            column![
                group(
                    t!("Select the objects to extract data from"),
                    column![header, scrollable(list).height(Fill)].spacing(4).into()
                ),
                group(t!("Display options"), options.into()),
            ]
            .spacing(8)
            .into()
        }
        ExtractionPage::Properties => {
            let header = row![
                Space::new().width(22),
                text(t!("Property")).size(11).style(muted).width(Fill),
                text(t!("Display Name")).size(11).style(muted).width(Fill),
                text(t!("Category")).size(11).style(muted).width(130),
            ]
            .spacing(6);
            let mut order: Vec<usize> = (0..state.properties.len())
                .filter(|&i| state.categories.contains(&state.properties[i].category))
                .collect();
            order.sort_by_key(|&i| state.properties[i].name.to_ascii_lowercase());
            let mut list = column![].spacing(3);
            for i in order {
                let item = &state.properties[i];
                list = list.push(
                    row![
                        checkbox(item.checked)
                            .on_toggle(move |v| {
                                Message::DataExtractionField(DataExtractionField::Property(i, v))
                            })
                            .size(14),
                        text(item.name.as_str()).size(12).width(Fill),
                        text(item.name.as_str()).size(12).width(Fill),
                        text(item.category.as_str()).size(12).width(130),
                    ]
                    .spacing(6)
                    .align_y(iced::Center),
                );
            }
            let all_categories: BTreeSet<&str> = state
                .properties
                .iter()
                .map(|value| value.category.as_str())
                .collect();
            let mut filter = column![].spacing(3);
            for category in all_categories {
                let key = category.to_string();
                filter = filter.push(
                    checkbox(state.categories.contains(category))
                        .label(category)
                        .on_toggle(move |v| {
                            Message::DataExtractionField(DataExtractionField::Category(
                                key.clone(),
                                v,
                            ))
                        })
                        .size(14),
                );
            }
            row![
                group(
                    t!("Properties"),
                    column![header, scrollable(list).height(Fill)].spacing(4).into()
                ),
                container(group(t!("Category filter"), scrollable(filter).into()))
                    .width(Length::Fixed(190.0)),
            ]
            .spacing(8)
            .into()
        }
        ExtractionPage::Refine => group(
            t!("Refine data"),
            column![
                row![
                    checkbox(state.combine_identical)
                        .label(t!("Combine identical rows"))
                        .on_toggle(|v| Message::DataExtractionField(
                            DataExtractionField::CombineIdentical(v)
                        ))
                        .size(14),
                    checkbox(state.show_count)
                        .label(t!("Show count column"))
                        .on_toggle(|v| Message::DataExtractionField(
                            DataExtractionField::ShowCount(v)
                        ))
                        .size(14),
                    checkbox(state.show_name)
                        .label(t!("Show name column"))
                        .on_toggle(
                            |v| Message::DataExtractionField(DataExtractionField::ShowName(v))
                        )
                        .size(14)
                ]
                .spacing(12),
                scrollable(preview_grid(&state.preview)).height(Fill),
            ]
            .spacing(8)
            .into(),
        ),
        ExtractionPage::Output => group(
            t!("Choose output"),
            column![
                checkbox(state.output_table)
                    .label(t!("Insert data extraction table into drawing"))
                    .on_toggle(
                        |v| Message::DataExtractionField(DataExtractionField::OutputTable(v))
                    )
                    .size(14),
                checkbox(state.output_file)
                    .label(t!("Output data to external file (.xlsx .csv .txt)"))
                    .on_toggle(|v| Message::DataExtractionField(DataExtractionField::OutputFile(v)))
                    .size(14),
                row![
                    text(t!("Output file")).size(11).style(muted).width(118),
                    text_input("", &state.output_path)
                        .on_input(|v| Message::DataExtractionField(
                            DataExtractionField::OutputPath(v)
                        ))
                        .padding([4, 7])
                        .size(12)
                        .width(Fill),
                    button(text("…"))
                        .on_press(Message::DataExtractionBrowseOutput)
                        .padding([4, 9])
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(7)
            .into(),
        ),
        ExtractionPage::TableStyle => group(
            t!("Table style"),
            column![
                row![
                    text(t!("Table style")).size(11).style(muted).width(118),
                    pick_list(
                        Some(state.table_style.clone()),
                        state.table_styles.clone(),
                        |value| value.clone()
                    )
                    .on_select(|value| Message::DataExtractionField(
                        DataExtractionField::TableStyle(value)
                    ))
                    .padding([3, 6])
                    .width(Fill)
                ]
                .spacing(8),
                row![
                    text(t!("Title")).size(11).style(muted).width(118),
                    text_input(t!("<Title>").as_ref(), &state.table_title)
                        .on_input(|v| Message::DataExtractionField(
                            DataExtractionField::TableTitle(v)
                        ))
                        .padding([4, 7])
                        .size(12)
                        .width(Fill)
                ]
                .spacing(8)
                .align_y(iced::Center),
                cell_style_picker(t!("Title cell style"), &state.title_cell_style, |v| {
                    Message::DataExtractionField(DataExtractionField::TitleCellStyle(v))
                }),
                cell_style_picker(t!("Header cell style"), &state.header_cell_style, |v| {
                    Message::DataExtractionField(DataExtractionField::HeaderCellStyle(v))
                }),
                cell_style_picker(t!("Data cell style"), &state.data_cell_style, |v| {
                    Message::DataExtractionField(DataExtractionField::DataCellStyle(v))
                }),
                checkbox(state.property_headers)
                    .label(t!("Use property names as additional column headers"))
                    .on_toggle(|v| Message::DataExtractionField(
                        DataExtractionField::PropertyHeaders(v)
                    ))
                    .size(14),
                preview_grid(&state.preview),
            ]
            .spacing(7)
            .into(),
        ),
        ExtractionPage::Finish => group(
            t!("Finish"),
            column![
                text(t!("Any external files to create will be created when clicking Finish."))
                    .size(12),
                text(t!(
                    "If you chose to insert a table, you will be prompted for an insertion point after you click Finish."
                ))
                .size(12),
                text(format!(
                    "{}: {}",
                    t!("Rows"),
                    state.preview.len().saturating_sub(1)
                ))
                .size(11),
                text(format!(
                    "{}: {}{}",
                    t!("Output"),
                    if state.output_table {
                        t!("Drawing table")
                    } else {
                        "".into()
                    },
                    if state.output_file {
                        format!("  {}", t!("External file"))
                    } else {
                        String::new()
                    }
                ))
                .size(11)
            ]
            .spacing(7)
            .into(),
        ),
    };
    column![
        text(format!(
            "{}  {}/8",
            t!("Data Extraction"),
            state.page.number()
        ))
        .size(13),
        container(body).height(Fill).width(Fill),
        text(&state.error).size(11),
        wizard_nav(state),
    ]
    .spacing(8)
    .padding(10)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

/// The notice DATAEXTRACTION shows on a drawing that was never saved.
pub fn data_extraction_unsaved_view(sizing: crate::ui::modal::ModalSizing) -> Element<'static, Message> {
    let amber = iced::Color::from_rgb8(0xE0, 0x9A, 0x2B);
    let badge = container(
        iced::widget::svg(crate::ui::icons::themed_handle(crate::ui::icons::WARNING))
            .width(18.0)
            .height(18.0)
            .style(move |_: &Theme, _| iced::widget::svg::Style { color: Some(amber) }),
    )
    .width(34)
    .height(34)
    .center_x(34)
    .center_y(34)
    .style(move |_: &Theme| container::Style {
        background: Some(iced::Background::Color(amber.scale_alpha(0.15))),
        border: Border {
            radius: 17.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });
    column![
        row![
            badge,
            column![
                text(t!("Save the drawing first")).size(13).font(iced::Font {
                    weight: iced::font::Weight::Bold,
                    ..iced::Font::DEFAULT
                }),
                text(t!(
                    "You must save the drawing before continuing with data extraction."
                ))
                .size(11)
                .style(muted),
            ]
            .spacing(6),
        ]
        .spacing(12),
        iced::widget::rule::horizontal(1),
        row![
            Space::new().width(Fill),
            dialog_button(t!("Cancel"), Message::CloseModal, false),
            dialog_button(t!("Save"), Message::DataExtractionSaveDrawing, true),
        ]
        .spacing(8),
    ]
    .spacing(12)
    .padding([12, 14])
    .width(sizing.width)
    .into()
}
