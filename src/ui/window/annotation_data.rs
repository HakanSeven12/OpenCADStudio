//! Dialogs used by the Annotate tab's table and drawing-data tools.
//!
//! The working state lives outside the drawing until the user presses OK or
//! Finish.  That keeps Cancel lossless and lets the wizard validate one page
//! before advancing to the next.

use std::collections::BTreeSet;
use std::fmt;

use acadrust::types::Handle;
use iced::widget::{button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space};
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
    pub row_height: String,
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
            columns: "3".into(),
            data_rows: "4".into(),
            column_width: "2.0".into(),
            row_height: "0.5".into(),
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
    pub properties: Vec<ExtractionProperty>,
    pub categories: BTreeSet<String>,
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
    pub error: String,
}

impl Default for DataExtractionState {
    fn default() -> Self {
        Self {
            page: ExtractionPage::Begin,
            begin: ExtractionBegin::New,
            settings_path: String::new(),
            source: ExtractionSource::CurrentDrawing,
            include_current: true,
            include_subfolders: true,
            source_files: Vec::new(),
            selection_handles: Vec::new(),
            objects: Vec::new(),
            properties: vec![
                ExtractionProperty { key: "type".into(), name: "Type".into(), category: "General".into(), checked: true },
                ExtractionProperty { key: "handle".into(), name: "Handle".into(), category: "General".into(), checked: true },
                ExtractionProperty { key: "layer".into(), name: "Layer".into(), category: "General".into(), checked: true },
                ExtractionProperty { key: "color".into(), name: "Color".into(), category: "General".into(), checked: true },
                ExtractionProperty { key: "linetype".into(), name: "Linetype".into(), category: "General".into(), checked: true },
                ExtractionProperty { key: "details".into(), name: "Details".into(), category: "Geometry".into(), checked: true },
            ],
            categories: ["General".to_string(), "Geometry".to_string()].into_iter().collect(),
            combine_identical: false,
            show_count: true,
            show_name: true,
            preview: Vec::new(),
            output_table: true,
            output_file: false,
            output_path: String::new(),
            table_styles: vec!["Standard".into()],
            table_style: "Standard".into(),
            table_title: "Data Extraction".into(),
            error: String::new(),
        }
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
            border: Border { width: 1.0, radius: 4.0.into(), color: theme.palette().background.strong.color },
            ..Default::default()
        })
        .into()
}

fn labeled_input<'a>(label: impl Into<String>, value: &'a str, msg: fn(String) -> Message) -> Element<'a, Message> {
    row![text(label.into()).size(11).style(muted).width(118), text_input("", value).on_input(msg).padding([4, 7]).size(12).width(Fill)]
        .spacing(8).align_y(iced::Center).into()
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
            vec!["Title".to_string(), "Header".to_string(), "Data".to_string()],
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

fn radio_button<'a>(selected: bool, label: impl Into<String>, msg: Message) -> Element<'a, Message> {
    form_radio(label, true, selected.then_some(true), move |_| msg.clone()).into()
}

fn preview_grid<'a>(rows: &'a [Vec<String>]) -> Element<'a, Message> {
    if rows.is_empty() {
        return text(t!("No preview available.")).size(11).style(muted).into();
    }
    let lines = rows.iter().take(8).map(|row| row.join("  |  ")).collect::<Vec<_>>().join("\n");
    container(text(lines).size(11)).padding(6).width(Fill).into()
}

pub fn table_insert_view(state: &TableInsertState, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let style = pick_list(Some(state.style.clone()), state.styles.clone(), |value| value.clone()).on_select(Message::TableInsertStyle).padding([3, 6]).text_size(12).width(Fill);
    let source = column![
        radio_button(state.source == TableSource::Empty, t!("Start from empty table"), Message::TableInsertField(TableInsertField::Source(TableSource::Empty))),
        radio_button(state.source == TableSource::DataLink, t!("Start from data link"), Message::TableInsertField(TableInsertField::Source(TableSource::DataLink))),
        radio_button(state.source == TableSource::DataExtraction, t!("Start from data extraction"), Message::TableInsertField(TableInsertField::Source(TableSource::DataExtraction))),
    ].spacing(1);
    let link_picker: Element<'_, Message> = if state.source == TableSource::DataLink {
        row![
            text(t!("Data link")).size(11).style(muted).width(118),
            pick_list(state.link.clone(), state.links.clone(), |value| value.to_string()).on_select(|value| Message::TableInsertField(TableInsertField::Link(value))).padding([3, 6]).text_size(12).width(Fill),
            button(text(t!("Manage…")).size(11)).on_press(Message::DataLinkManagerOpen).padding([4, 8]),
        ].spacing(8).align_y(iced::Center).into()
    } else { Space::new().height(0).into() };
    let insertion = column![
        radio_button(state.insertion == TableInsertion::Point, t!("Specify insertion point"), Message::TableInsertField(TableInsertField::Insertion(TableInsertion::Point))),
        radio_button(state.insertion == TableInsertion::Window, t!("Specify window"), Message::TableInsertField(TableInsertField::Insertion(TableInsertion::Window))),
    ];
    let settings = column![
        labeled_input(t!("Columns"), &state.columns, |v| Message::TableInsertField(TableInsertField::Columns(v))),
        labeled_input(t!("Column width"), &state.column_width, |v| Message::TableInsertField(TableInsertField::ColumnWidth(v))),
        labeled_input(t!("Data rows"), &state.data_rows, |v| Message::TableInsertField(TableInsertField::DataRows(v))),
        labeled_input(t!("Row height"), &state.row_height, |v| Message::TableInsertField(TableInsertField::RowHeight(v))),
    ].spacing(5);
    let cell_styles = column![
        cell_style_picker(t!("First row"), &state.first_row_style, |v| Message::TableInsertField(TableInsertField::FirstRowStyle(v))),
        cell_style_picker(t!("Second row"), &state.second_row_style, |v| Message::TableInsertField(TableInsertField::SecondRowStyle(v))),
        cell_style_picker(t!("All other rows"), &state.other_row_style, |v| Message::TableInsertField(TableInsertField::OtherRowStyle(v))),
    ].spacing(5);
    column![
        group(t!("Table style"), row![style].into()),
        group(t!("Insert options"), column![source, link_picker].spacing(4).into()),
        row![group(t!("Insertion behavior"), insertion.into()), group(t!("Column & row settings"), settings.into())].spacing(8),
        group(t!("Set cell styles"), cell_styles.into()),
        checkbox(state.preview).label(t!("Preview")).on_toggle(|v| Message::TableInsertField(TableInsertField::Preview(v))).size(14),
        text(&state.error).size(11),
        row![
            Space::new().width(Fill),
            dialog_button(t!("Cancel"), Message::CloseModal, false),
            dialog_button(t!("OK"), Message::TableInsertApply, true),
        ].spacing(8),
    ].spacing(8).padding(10).width(sizing.width).height(sizing.height).into()
}

pub fn data_link_view(state: &DataLinkManagerState, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let mut links = column![text(t!("Spreadsheet Links")).size(11).style(muted)].spacing(2);
    links = links.push(button(text(t!("Create a New Data Link")).size(12)).on_press(Message::DataLinkNew).padding([5, 7]).style(button::text).width(Fill));
    for link in &state.links {
        let selected = state.selected.as_ref().is_some_and(|v| v.handle == link.handle);
        let label = if selected { format!("● {link}") } else { format!("  {link}") };
        links = links.push(button(text(label).size(12)).on_press(Message::DataLinkSelect(link.handle)).padding([5, 7]).style(button::text).width(Fill));
    }
    let tree = container(scrollable(links).height(Fill)).padding(6).width(Length::Fixed(220.0)).height(Fill);
    let right: Element<'_, Message> = if state.editing {
        let mut editor = column![
            labeled_input(t!("Data link name"), &state.name, |v| Message::DataLinkField(DataLinkField::Name(v))),
            row![text(t!("File")).size(11).style(muted).width(118), text_input("", &state.path).on_input(|v| Message::DataLinkField(DataLinkField::Path(v))).padding([4, 7]).size(12).width(Fill), button(text("…")).on_press(Message::DataLinkBrowse).padding([4, 9])].spacing(6).align_y(iced::Center),
            row![text(t!("Path type")).size(11).style(muted).width(118), pick_list(Some(state.path_type), vec![LinkPathType::Full, LinkPathType::Relative, LinkPathType::FileName], |value| value.to_string()).on_select(|value| Message::DataLinkField(DataLinkField::PathType(value))).width(Fill).padding([3, 6])].spacing(8),
            row![text(t!("Link option")).size(11).style(muted).width(118), pick_list(Some(state.range_kind), vec![LinkRange::EntireSheet, LinkRange::NamedRange, LinkRange::CellRange], |value| value.to_string()).on_select(|value| Message::DataLinkField(DataLinkField::RangeKind(value))).width(Fill).padding([3, 6])].spacing(8),
        ].spacing(6);
        if !state.sheets.is_empty() && state.range_kind != LinkRange::NamedRange {
            editor = editor.push(row![
                text(t!("Sheet")).size(11).style(muted).width(118),
                pick_list(Some(state.sheet.clone()), state.sheets.clone(), |value| value.clone())
                    .on_select(|value| Message::DataLinkField(DataLinkField::Sheet(value)))
                    .width(Fill)
                    .padding([3, 6]),
            ].spacing(8));
        }
        if state.range_kind == LinkRange::NamedRange && !state.named_ranges.is_empty() {
            editor = editor.push(row![
                text(t!("Named range")).size(11).style(muted).width(118),
                pick_list(Some(state.range.clone()), state.named_ranges.clone(), |value| value.clone())
                    .on_select(|value| Message::DataLinkField(DataLinkField::Range(value)))
                    .width(Fill)
                    .padding([3, 6]),
            ].spacing(8));
        } else if state.range_kind != LinkRange::EntireSheet {
            editor = editor.push(labeled_input(t!("Range"), &state.range, |v| Message::DataLinkField(DataLinkField::Range(v))));
        }
        editor = editor.push(checkbox(state.more_options).label(t!("More options")).on_toggle(|v| Message::DataLinkField(DataLinkField::MoreOptions(v))).size(14));
        if state.more_options {
            editor = editor
                .push(checkbox(state.allow_write).label(t!("Allow writing to source file")).on_toggle(|v| Message::DataLinkField(DataLinkField::AllowWrite(v))).size(14))
                .push(checkbox(state.use_source_formatting).label(t!("Use source formatting")).on_toggle(|v| Message::DataLinkField(DataLinkField::UseSourceFormatting(v))).size(14))
                .push(checkbox(state.update_source_formatting).label(t!("Keep table updated to source formatting")).on_toggle(|v| Message::DataLinkField(DataLinkField::UpdateSourceFormatting(v))).size(14));
        }
        editor = editor.push(checkbox(state.insert_table).label(t!("Insert table after saving link")).on_toggle(|v| Message::DataLinkField(DataLinkField::InsertTable(v))).size(14));
        group(t!("Link settings"), editor.into())
    } else if let Some(link) = &state.selected {
        column![
            group(t!("Details"), column![text(format!("{}: {}", t!("Name"), link.name)).size(11), text(format!("{}: {}", t!("File"), link.path)).size(11), text(if link.valid { t!("Status: Linked") } else { t!("Status: Source not found") }).size(11)].spacing(5).into()),
            group(t!("Preview"), preview_grid(&state.preview)),
        ].spacing(8).into()
    } else {
        group(t!("Details"), text(t!("Select a data link or create a new one.")).size(11).style(muted).into())
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
        let edit = dialog_button_styled_opt(t!("Edit"), enabled.then_some(Message::DataLinkEdit), button::secondary);
        let delete = dialog_button_styled_opt(t!("Delete"), enabled.then_some(Message::DataLinkDelete), button::secondary);
        let insert = dialog_button_styled_opt(t!("Insert Table"), enabled.then_some(Message::DataLinkInsert), button::primary);
        let close = dialog_button(t!("Close"), Message::DataLinkClose, false);
        row![edit, delete, Space::new().width(Fill), insert, close].spacing(8)
    };
    column![row![tree, container(right).width(Fill).height(Fill)].spacing(8).height(Fill), text(&state.status).size(11), actions]
        .spacing(7).padding(10).width(sizing.width).height(sizing.height).into()
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
        state.error.is_empty().then_some(if state.page == ExtractionPage::Finish {
            Message::DataExtractionFinish
        } else {
            Message::DataExtractionNext
        }),
        button::primary,
    );
    row![Space::new().width(Fill), back, cancel, next].spacing(8)
}

pub fn data_extraction_view(state: &DataExtractionState, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let add_folder = button(text(t!("Add folder…")).size(12)).padding([5, 10]);
    #[cfg(not(target_arch = "wasm32"))]
    let add_folder = add_folder.on_press(Message::DataExtractionAddFolder);

    let body: Element<'_, Message> = match state.page {
        ExtractionPage::Begin => group(t!("Begin"), column![
            radio_button(state.begin == ExtractionBegin::New, t!("Create a new data extraction"), Message::DataExtractionField(DataExtractionField::Begin(ExtractionBegin::New))),
            radio_button(state.begin == ExtractionBegin::Template, t!("Use a previous extraction as a template"), Message::DataExtractionField(DataExtractionField::Begin(ExtractionBegin::Template))),
            radio_button(state.begin == ExtractionBegin::Edit, t!("Edit an existing data extraction"), Message::DataExtractionField(DataExtractionField::Begin(ExtractionBegin::Edit))),
            row![text(t!("Extraction settings")).size(11).style(muted).width(118), text_input("", &state.settings_path).on_input(|v| Message::DataExtractionField(DataExtractionField::SettingsPath(v))).padding([4, 7]).size(12).width(Fill), button(text("…")).on_press(Message::DataExtractionBrowseSettings).padding([4, 9])].spacing(6).align_y(iced::Center),
        ].spacing(5).into()),
        ExtractionPage::Source => group(t!("Define data source"), column![
            radio_button(state.source == ExtractionSource::CurrentDrawing, t!("Current drawing"), Message::DataExtractionField(DataExtractionField::Source(ExtractionSource::CurrentDrawing))),
            radio_button(state.source == ExtractionSource::CurrentSelection, t!("Select objects in the current drawing"), Message::DataExtractionField(DataExtractionField::Source(ExtractionSource::CurrentSelection))),
            radio_button(state.source == ExtractionSource::DrawingsAndFolders, t!("Drawings / folders"), Message::DataExtractionField(DataExtractionField::Source(ExtractionSource::DrawingsAndFolders))),
            checkbox(state.include_current).label(t!("Include current drawing")).on_toggle(|v| Message::DataExtractionField(DataExtractionField::IncludeCurrent(v))).size(14),
            checkbox(state.include_subfolders).label(t!("Include subfolders")).on_toggle(|v| Message::DataExtractionField(DataExtractionField::IncludeSubfolders(v))).size(14),
            row![
                button(text(t!("Add drawings…")).size(12)).on_press(Message::DataExtractionAddDrawings).padding([5, 10]),
                add_folder,
                button(text(t!("Clear")).size(12)).on_press(Message::DataExtractionClearSources).padding([5, 10]),
            ].spacing(6),
            text(state.source_files.join("\n")).size(10),
        ].spacing(6).into()),
        ExtractionPage::Objects => {
            let mut list = column![].spacing(3);
            for (i, item) in state.objects.iter().enumerate() {
                list = list.push(checkbox(item.checked).label(format!("{}  ({})", item.name, item.count)).on_toggle(move |v| Message::DataExtractionField(DataExtractionField::Object(i, v))).size(14));
            }
            group(t!("Select objects"), scrollable(list).height(Fill).into())
        }
        ExtractionPage::Properties => {
            let mut categories = row![].spacing(10);
            for category in ["General", "Geometry"] {
                let checked = state.categories.contains(category);
                let key = category.to_string();
                categories = categories.push(checkbox(checked).label(category).on_toggle(move |v| Message::DataExtractionField(DataExtractionField::Category(key.clone(), v))).size(14));
            }
            let mut list = column![categories].spacing(4);
            for (i, item) in state.properties.iter().enumerate().filter(|(_, p)| state.categories.contains(&p.category)) {
                list = list.push(checkbox(item.checked).label(format!("{}  ·  {}", item.name, item.category)).on_toggle(move |v| Message::DataExtractionField(DataExtractionField::Property(i, v))).size(14));
            }
            group(t!("Select properties"), scrollable(list).height(Fill).into())
        }
        ExtractionPage::Refine => group(t!("Refine data"), column![
            row![checkbox(state.combine_identical).label(t!("Combine identical rows")).on_toggle(|v| Message::DataExtractionField(DataExtractionField::CombineIdentical(v))).size(14), checkbox(state.show_count).label(t!("Show count column")).on_toggle(|v| Message::DataExtractionField(DataExtractionField::ShowCount(v))).size(14), checkbox(state.show_name).label(t!("Show name column")).on_toggle(|v| Message::DataExtractionField(DataExtractionField::ShowName(v))).size(14)].spacing(12),
            scrollable(preview_grid(&state.preview)).height(Fill),
        ].spacing(8).into()),
        ExtractionPage::Output => group(t!("Choose output"), column![
            checkbox(state.output_table).label(t!("Insert data extraction table into drawing")).on_toggle(|v| Message::DataExtractionField(DataExtractionField::OutputTable(v))).size(14),
            checkbox(state.output_file).label(t!("Output data to external file")).on_toggle(|v| Message::DataExtractionField(DataExtractionField::OutputFile(v))).size(14),
            row![text(t!("Output file")).size(11).style(muted).width(118), text_input("", &state.output_path).on_input(|v| Message::DataExtractionField(DataExtractionField::OutputPath(v))).padding([4, 7]).size(12).width(Fill), button(text("…")).on_press(Message::DataExtractionBrowseOutput).padding([4, 9])].spacing(6).align_y(iced::Center),
        ].spacing(7).into()),
        ExtractionPage::TableStyle => group(t!("Table style"), column![
            row![text(t!("Style")).size(11).style(muted).width(118), pick_list(Some(state.table_style.clone()), state.table_styles.clone(), |value| value.clone()).on_select(|value| Message::DataExtractionField(DataExtractionField::TableStyle(value))).padding([3, 6]).width(Fill)].spacing(8),
            labeled_input(t!("Title"), &state.table_title, |v| Message::DataExtractionField(DataExtractionField::TableTitle(v))),
            preview_grid(&state.preview),
        ].spacing(7).into()),
        ExtractionPage::Finish => group(t!("Finish"), column![text(t!("The selected drawing data is ready to be extracted.")).size(12), text(format!("{}: {}", t!("Rows"), state.preview.len().saturating_sub(1))).size(11), text(format!("{}: {}{}", t!("Output"), if state.output_table { t!("Drawing table") } else { "".into() }, if state.output_file { format!("  {}", t!("External file")) } else { String::new() })).size(11)].spacing(7).into()),
    };
    column![
        text(format!("{}  {}/8", t!("Data Extraction"), state.page.number())).size(13),
        container(body).height(Fill).width(Fill),
        text(&state.error).size(11),
        wizard_nav(state),
    ].spacing(8).padding(10).width(sizing.width).height(sizing.height).into()
}
