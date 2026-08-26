//! Plot / Print dialog — a full plot setup surface rendered as an in-canvas
//! modal (Plan B). Bundles printer choice, paper, scale, offset, plot style,
//! quality and output options into one dialog; on commit it either sends the
//! current layout to a system printer (with the chosen options) or writes a
//! PDF. Styled to match the other OCS dialogs (dark pills + fields).

use crate::app::Message;
use crate::io::paper_sizes::PaperSize;
use iced::widget::{
    button, checkbox, column, container, mouse_area, row, scrollable, text, text_input,
    Space,
};
use iced::{Background, Border, Element, Fit, Length, Theme};
use crate::t;
use std::borrow::Cow;
use std::fmt;

/// Sentinel entries in the printer dropdown (not real printer names).
pub const OUT_DEFAULT: &str = "System default printer";
pub const OUT_PDF: &str = "Save to PDF file…";

/// Top-of-list entries: no page setup (defaults + PDF), and the last-used
/// settings captured when the dialog opened.
pub const SETUP_NONE: &str = "<none>";
pub const SETUP_PREV: &str = "<previous>";
pub const STYLE_NONE: &str = "<none>";

/// A plot dropdown value keeps its persisted/raw value separate from the
/// localized label shown by Iced. Printer names, scale names, paper sizes, and
/// style-table file names remain verbatim; built-in choices use the catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PlotChoice {
    raw: String,
    localized: bool,
}

impl PlotChoice {
    fn raw(value: impl Into<String>) -> Self {
        Self { raw: value.into(), localized: false }
    }

    fn localized(value: impl Into<String>) -> Self {
        Self { raw: value.into(), localized: true }
    }
}

impl fmt::Display for PlotChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.localized {
            formatter.write_str(crate::i18n::translate(&self.raw).as_ref())
        } else {
            formatter.write_str(&self.raw)
        }
    }
}

/// One of the many boolean plot options (folded into a single message so the
/// dialog needn't carry a variant per checkbox).
#[derive(Debug, Clone, Copy)]
pub enum PlotFlag {
    Background,
    MergeLines,
    FitToPaper,
    Center,
    ScaleLw,
    PlotStyles,
    DisplayStyles,
    UpsideDown,
    Lineweights,
    Transparency,
    PaperspaceLast,
    Stamp,
}

/// Every edit the Plot dialog can emit. Wrapped in `Message::PlotDlg` so the
/// top-level match stays a single arm.
#[derive(Debug, Clone)]
pub enum PlotDlgMsg {
    Close,
    Commit,
    Preview,
    PrinterProperties,
    Printer(String),
    Paper(String),
    Orientation(String),
    Area(String),
    Scale(String),
    Quality(String),
    Shade(String),
    Copies(String),
    OffsetX(String),
    OffsetY(String),
    Flag(PlotFlag),
    LoadStyle,
    SaveStyle,
    Style(String),
    PickWindow,
    // ── Named page-setup manager ─────────────────────────────────────────
    /// Pick a named page setup (loads its values into the editor).
    SelectSetup(String),
    /// Write the current editor values into the active layout.
    SetCurrent,
    /// Create a new named page setup from the current editor values.
    NewSetup,
    /// Duplicate the selected page setup.
    CopySetup,
    /// Begin an inline rename of the given page setup row.
    RenameStart(String),
    /// Delete the selected named page setup.
    DeleteSetup,
    /// Live edit of the new/rename name field.
    NameInput(String),
    /// Confirm the new/rename name.
    NameCommit,
    /// Cancel the new/rename name row.
    NameCancel,
}

/// Transient state backing the Plot dialog. Seeded from the layout's plot
/// settings when the dialog opens; consumed on commit.
// The persisted fields form the "plot" section of the app config
// ([`crate::app::config`]); `#[serde(skip)]` marks the runtime-only fields
// (discovered printers, live page/offset choices, name-entry state) so only the
// user's print preferences are written, matching the former plot.txt subset.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PlotDialogState {
    /// Printer names discovered on the system (via `lpstat`), never the
    /// sentinels.
    #[serde(skip)]
    pub printers: Vec<String>,
    /// Chosen printer name, or `None` for the system default.
    pub printer: Option<String>,
    /// Output goes to a PDF file instead of a printer.
    pub to_file: bool,
    pub paper: String,
    #[serde(skip)]
    pub paper_width_mm: f64,
    #[serde(skip)]
    pub paper_height_mm: f64,
    pub orientation: String,
    pub upside_down: bool,
    pub copies: String,
    pub area: String,
    pub center: bool,
    pub offset_x: String,
    pub offset_y: String,
    pub scale: String,
    #[serde(default = "legacy_fit_to_paper_default")]
    pub fit_to_paper: bool,
    #[serde(skip)]
    pub scales: Vec<(String, f64)>,
    #[serde(skip)]
    pub plot_views: Vec<String>,
    pub scale_lw: bool,
    pub quality: String,
    pub shade: String,
    pub background: bool,
    pub merge_lines: bool,
    pub lineweights: bool,
    pub transparency: bool,
    pub paperspace_last: bool,
    pub stamp: bool,
    /// Display name of the active plot style table ("" = none).
    pub style_name: String,
    pub apply_plot_styles: bool,
    pub show_plot_styles: bool,
    /// CTB file names discovered in the per-user plot styles folder.
    #[serde(skip)]
    pub plot_styles: Vec<String>,
    /// The selected setup references a style table that is not loaded.
    #[serde(skip)]
    pub style_missing: bool,
    /// Named page setups in the document (refreshed when the dialog opens).
    #[serde(skip)]
    pub page_setups: Vec<String>,
    /// Currently selected named page setup ("" = none / current layout).
    #[serde(skip)]
    pub selected_setup: String,
    /// When `Some`, a name-entry row is showing (for New / Rename).
    #[serde(skip)]
    pub name_input: Option<String>,
    /// `true` when `name_input` is renaming the selected setup, else creating.
    #[serde(skip)]
    pub name_rename: bool,
    /// Whether the dialog was opened from a paper-space layout.
    #[serde(skip)]
    pub paper_space: bool,
}

impl Default for PlotDialogState {
    fn default() -> Self {
        Self {
            printers: Vec::new(),
            printer: None,
            to_file: false,
            paper: "A4".into(),
            paper_width_mm: 297.0,
            paper_height_mm: 210.0,
            orientation: "Landscape".into(),
            upside_down: false,
            copies: "1".into(),
            area: "Window".into(),
            center: true,
            offset_x: "0.0".into(),
            offset_y: "0.0".into(),
            scale: "1:1".into(),
            fit_to_paper: true,
            scales: Vec::new(),
            plot_views: Vec::new(),
            scale_lw: false,
            quality: "Normal".into(),
            shade: "As displayed".into(),
            background: true,
            merge_lines: false,
            lineweights: true,
            transparency: false,
            paperspace_last: false,
            stamp: false,
            style_name: String::new(),
            apply_plot_styles: true,
            show_plot_styles: false,
            plot_styles: Vec::new(),
            style_missing: false,
            page_setups: Vec::new(),
            selected_setup: String::new(),
            name_input: None,
            name_rename: false,
            paper_space: false,
        }
    }
}

impl PlotDialogState {
    /// Copy the plot-setting fields (paper, scale, output options, …) from
    /// `o`, leaving list / rename / runtime UI state untouched. Used to restore
    /// the `<previous>` snapshot.
    pub fn copy_settings_from(&mut self, o: &PlotDialogState) {
        self.printer = o.printer.clone();
        self.to_file = o.to_file;
        self.paper = o.paper.clone();
        self.paper_width_mm = o.paper_width_mm;
        self.paper_height_mm = o.paper_height_mm;
        self.orientation = o.orientation.clone();
        self.upside_down = o.upside_down;
        self.copies = o.copies.clone();
        self.area = o.area.clone();
        self.center = o.center;
        self.offset_x = o.offset_x.clone();
        self.offset_y = o.offset_y.clone();
        self.scale = o.scale.clone();
        self.fit_to_paper = o.fit_to_paper;
        self.scale_lw = o.scale_lw;
        self.quality = o.quality.clone();
        self.shade = o.shade.clone();
        self.background = o.background;
        self.merge_lines = o.merge_lines;
        self.lineweights = o.lineweights;
        self.transparency = o.transparency;
        self.paperspace_last = o.paperspace_last;
        self.stamp = o.stamp;
        self.style_name = o.style_name.clone();
        self.apply_plot_styles = o.apply_plot_styles;
        self.show_plot_styles = o.show_plot_styles;
        self.style_missing = o.style_missing;
    }

}

fn legacy_fit_to_paper_default() -> bool {
    false
}

fn btn(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, st| {
        let palette = theme.palette();
        let pair = match (accent, st) {
            (true, button::Status::Hovered | button::Status::Pressed) => palette.primary.strong,
            (false, button::Status::Hovered | button::Status::Pressed) => {
                palette.background.strong
            }
            (true, _) => palette.primary.base,
            _ => palette.background.weak,
        };
        button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
        snap: false,
        }
    }
}

fn field_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.palette();
    let border = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.neutral.color,
    };
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border { color: border, width: 1.0, radius: 3.0.into() },
        icon: palette.background.base.text,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.68)),
    }
}

fn hdivider<'a>(width: Length) -> Element<'a, Message> {
    container(Space::new().width(width).height(1))
        .width(width)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

fn section_label<'a>(s: Cow<'static, str>) -> Element<'a, Message> {
    text(s).size(11).style(muted_style).into()
}

fn vsep<'a>(height: Length) -> Element<'a, Message> {
    container(Space::new().width(1).height(height))
        .width(1)
        .height(height)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

fn setup_row<'a>(
    name: &'a str,
    selected: &str,
    renaming: Option<&str>,
    rename_buf: &'a str,
) -> Element<'a, Message> {
    if renaming == Some(name) {
        return text_input("", rename_buf)
            .on_input(|value| Message::PlotDlg(PlotDlgMsg::NameInput(value)))
            .on_submit(Message::PlotDlg(PlotDlgMsg::NameCommit))
            .style(field_style)
            .size(11)
            .padding([4, 8])
            .width(Fit)
            .into();
    }
    let is_selected = name == selected;
    let display_name = if name == SETUP_NONE || name == SETUP_PREV {
        crate::i18n::translate(name)
    } else {
        Cow::Owned(name.to_string())
    };
    let cell = container(text(display_name).size(11))
        .padding([4, 8])
        .width(Fit)
        .style(move |theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: is_selected.then_some(Background::Color(palette.primary.strong.color)),
                text_color: is_selected.then_some(palette.primary.strong.text),
                ..Default::default()
            }
        });
    mouse_area(cell)
        .on_press(Message::PlotDlg(PlotDlgMsg::SelectSetup(name.to_string())))
        .on_double_click(Message::PlotDlg(PlotDlgMsg::RenameStart(name.to_string())))
        .into()
}

/// A `label : dropdown` row. `ctor` turns the picked string into a dialog
/// message.
fn drop_row<'a>(
    label: Cow<'static, str>,
    options: Vec<PlotChoice>,
    selected: Option<PlotChoice>,
    ctor: fn(String) -> PlotDlgMsg,
    width: Length,
) -> Element<'a, Message> {
    let pl = iced::widget::pick_list(selected, options, |value| value.to_string())
        .on_select(move |choice| Message::PlotDlg(ctor(choice.raw)))
        .text_size(12)
        .padding([3, 6])
        .width(width);
    row![text(label).size(11).style(muted_style).width(92), pl]
        .spacing(8)
        .align_y(iced::Center)
    .into()
}

fn drop_row_enabled<'a>(
    label: Cow<'static, str>,
    options: Vec<PlotChoice>,
    selected: Option<PlotChoice>,
    ctor: fn(String) -> PlotDlgMsg,
    width: Length,
    enabled: bool,
) -> Element<'a, Message> {
    if enabled {
        return drop_row(label, options, selected, ctor, width);
    }
    row![
        text(label).size(11).style(muted_style).width(92),
        crate::ui::read_only::field(
            selected.map(|choice| choice.to_string()).unwrap_or_default().as_str(),
            12.0,
            width,
        ),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// A `label : text field` row.
fn field_row<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    ctor: fn(String) -> PlotDlgMsg,
    width: u16,
) -> Element<'a, Message> {
    row![
        text(label).size(11).style(muted_style).width(92),
        text_input("", value)
            .on_input(move |s| Message::PlotDlg(ctor(s)))
            .style(field_style)
            .size(12)
            .width(width as f32),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

fn field_row_enabled<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    ctor: fn(String) -> PlotDlgMsg,
    width: u16,
    enabled: bool,
) -> Element<'a, Message> {
    if enabled {
        return field_row(label, value, ctor, width);
    }
    row![
        text(label).size(11).style(muted_style).width(92),
        crate::ui::read_only::field(value, 12.0, Length::Fixed(width as f32)),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into()
}

/// A single option checkbox bound to a `PlotFlag`.
fn check<'a>(label: Cow<'static, str>, on: bool, flag: PlotFlag) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .on_toggle(move |_| Message::PlotDlg(PlotDlgMsg::Flag(flag)))
        .size(14)
        .text_size(11)
        .into()
}

fn check_enabled<'a>(
    label: Cow<'static, str>,
    on: bool,
    flag: PlotFlag,
    enabled: bool,
) -> Element<'a, Message> {
    if enabled {
        return check(label, on, flag);
    }
    checkbox(on)
        .label(label)
        .size(14)
        .text_size(11)
        .style(checkbox::primary)
        .into()
}

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .into()
}

fn choices(items: &[&str]) -> Vec<PlotChoice> {
    items.iter().map(|&value| PlotChoice::localized(value)).collect()
}

pub fn view_window(
    s: &PlotDialogState,
    print_all_options: bool,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'_, Message> {
    let width = sizing.width;
    let height = sizing.height;
    let action = if print_all_options {
        t!("Apply")
    } else if s.to_file {
        t!("Export PDF")
    } else {
        t!("Print")
    };
    let is_special = s.selected_setup == SETUP_NONE || s.selected_setup == SETUP_PREV;
    let sel_is_layout = s.selected_setup.len() >= 2
        && s.selected_setup.starts_with('*')
        && s.selected_setup.ends_with('*');
    let can_copy = !s.selected_setup.is_empty() && !is_special;
    let is_named = can_copy && !sel_is_layout;

    // ── Page setup sidebar ────────────────────────────────────────────────
    let renaming = if s.name_rename && !s.selected_setup.is_empty() {
        Some(s.selected_setup.as_str())
    } else {
        None
    };
    let rename_buf = s.name_input.as_deref().unwrap_or("");
    let rows: Vec<Element<'_, Message>> = s
        .page_setups
        .iter()
        .map(|name| setup_row(name, &s.selected_setup, renaming, rename_buf))
        .collect();
    let list_body: Element<'_, Message> = if rows.is_empty() {
        container(text(t!("(no page setups)")).size(11).style(muted_style))
            .padding([6, 8])
            .into()
    } else {
        scrollable(column(rows).spacing(1)).height(height).into()
    };
    let new_name: Element<'_, Message> =
        if !s.name_rename && s.name_input.is_some() {
            row![
                text_input("", rename_buf)
                    .on_input(|value| Message::PlotDlg(PlotDlgMsg::NameInput(value)))
                    .on_submit(Message::PlotDlg(PlotDlgMsg::NameCommit))
                    .style(field_style)
                    .size(11)
                    .padding([4, 8])
                    .width(Length::Fill),
                button(text(t!("Save")).size(11))
                    .on_press(Message::PlotDlg(PlotDlgMsg::NameCommit))
                    .style(btn(true))
                    .padding([4, 8]),
                button(text("×").size(11))
                    .on_press(Message::PlotDlg(PlotDlgMsg::NameCancel))
                    .style(btn(false))
                    .padding([4, 8]),
            ]
            .spacing(4)
            .into()
        } else {
            Space::new().height(0).into()
        };
    let list_panel = container(
        column![
            text(t!("Page setups")).size(10).style(muted_style),
            new_name,
            container(list_body)
                .style(|theme: &Theme| {
                    let palette = theme.palette();
                    container::Style {
                        background: Some(Background::Color(palette.background.weak.color)),
                        border: Border {
                            color: palette.background.neutral.color,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .width(Length::Fill)
                .height(height)
                .padding(2),
        ]
        .spacing(4)
        .height(height),
    )
    .width(160)
    .height(height)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    let mut new_button = button(text(t!("New")).size(11))
        .style(btn(false))
        .padding([4, 12]);
    if !print_all_options {
        new_button = new_button.on_press(Message::PlotDlg(PlotDlgMsg::NewSetup));
    }
    let mut copy_button = button(text(t!("Copy")).size(11))
        .style(btn(false))
        .padding([4, 12]);
    if can_copy && !print_all_options {
        copy_button = copy_button.on_press(Message::PlotDlg(PlotDlgMsg::CopySetup));
    }
    let mut delete_button = button(text(t!("Delete")).size(11))
        .style(btn(false))
        .padding([4, 12]);
    if is_named && !print_all_options {
        delete_button = delete_button.on_press(Message::PlotDlg(PlotDlgMsg::DeleteSetup));
    }
    let left_bar = row![
        new_button,
        copy_button,
        delete_button,
    ]
    .spacing(4);

    // ── Printer / plotter ─────────────────────────────────────────────────
    let mut printer_opts = vec![PlotChoice::localized(OUT_DEFAULT)];
    printer_opts.extend(s.printers.iter().cloned().map(PlotChoice::raw));
    printer_opts.push(PlotChoice::localized(OUT_PDF));
    let printer_sel = if s.to_file {
        Some(PlotChoice::localized(OUT_PDF))
    } else {
        Some(match &s.printer {
            Some(printer) => PlotChoice::raw(printer.clone()),
            None => PlotChoice::localized(OUT_DEFAULT),
        })
    };
    let mut paper_opts: Vec<String> = PaperSize::ALL.iter().map(|p| p.label().to_string()).collect();
    if !paper_opts.iter().any(|name| name == &s.paper) {
        paper_opts.push(s.paper.clone());
    }
    let paper_note: Element<'_, Message> = if s.area == "Layout" {
        text(t!("Layout plots the current sheet using the selected paper size."))
            .size(10)
            .style(muted_style)
            .width(width)
            .into()
    } else {
        Space::new().height(0).into()
    };
    let mut output_row = row![
        iced::widget::pick_list(printer_sel, printer_opts, |value| value.to_string())
            .on_select(|choice| Message::PlotDlg(PlotDlgMsg::Printer(choice.raw)))
            .text_size(12)
            .padding([3, 6])
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Center);
    if !s.to_file {
        output_row = output_row.push(
            button(text(t!("Properties…")).size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::PrinterProperties))
                .style(btn(false))
                .padding([4, 8]),
        );
    }
    let output_field = column![
        text(t!("Output")).size(11).style(muted_style),
        output_row,
    ]
    .spacing(4)
    .width(Length::Fill);
    let copies_row: Element<'_, Message> = if s.to_file {
        Space::new().height(0).into()
    } else {
        field_row(t!("Copies"), &s.copies, PlotDlgMsg::Copies, 60)
    };
    let printer_panel = panel(
        column![
            section_label(t!("Printer / plotter")),
            output_field,
            copies_row,
        ]
        .spacing(7),
    );

    // ── Paper, area, offset, scale ────────────────────────────────────────
    let paper_panel = panel(column![
        section_label(t!("Paper")),
        drop_row(
            t!("Size"),
            paper_opts.into_iter().map(PlotChoice::raw).collect(),
            Some(PlotChoice::raw(s.paper.clone())),
            PlotDlgMsg::Paper,
            width,
        ),
        paper_note,
    ].spacing(7));

    let mut area_options = if print_all_options {
        choices(&["Layout"])
    } else {
        choices(&["Extents", "Limits", "Display", "Window"])
    };
    if s.paper_space && !print_all_options {
        area_options.insert(0, PlotChoice::localized("Layout"));
    }
    if !print_all_options {
        area_options.extend(
            s.plot_views
                .iter()
                .map(|name| PlotChoice::raw(format!("View: {name}"))),
        );
    }
    let mut area_row = row![
        text(t!("What to plot")).size(11).style(muted_style).width(92),
        iced::widget::pick_list(
            Some(PlotChoice::localized(s.area.clone())),
            area_options,
            |value| value.to_string(),
        )
            .on_select(|choice| Message::PlotDlg(PlotDlgMsg::Area(choice.raw)))
            .text_size(12)
            .padding([3, 6])
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Center);
    if s.area == "Window" && !print_all_options {
        area_row = area_row.push(
            button(text(t!("Pick…")).size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::PickWindow))
                .style(btn(false))
                .padding([4, 10]),
        );
    }
    let common_area = s.area != "Layout";
    let area_panel = panel(column![
        section_label(t!("Plot area")),
        area_row,
        section_label(t!("Plot offset")),
        column![
            field_row_enabled(t!("X (mm)"), &s.offset_x, PlotDlgMsg::OffsetX, 70, common_area && !s.center),
            field_row_enabled(t!("Y (mm)"), &s.offset_y, PlotDlgMsg::OffsetY, 70, common_area && !s.center),
        ]
        .spacing(7),
        check_enabled(t!("Center the plot"), s.center, PlotFlag::Center, common_area),
    ].spacing(7));
    let scale_options = s
        .scales
        .iter()
        .map(|(name, _)| PlotChoice::raw(name.clone()))
        .collect();
    let scale_panel = panel(column![
        section_label(t!("Plot scale")),
        check_enabled(
            t!("Fit to paper"),
            s.fit_to_paper,
            PlotFlag::FitToPaper,
            common_area,
        ),
        // "Scale" intentionally stays untranslated: it would clash with the
        // ribbon's zoom tool label under the same lookup key.
        drop_row_enabled(
            Cow::Borrowed("Scale"),
            scale_options,
            Some(PlotChoice::raw(s.scale.clone())),
            PlotDlgMsg::Scale,
            width,
            common_area && !s.fit_to_paper,
        ),
        check_enabled(
            t!("Scale lineweights"),
            s.scale_lw && !s.fit_to_paper,
            PlotFlag::ScaleLw,
            !s.fit_to_paper,
        ),
    ].spacing(7));

    // ── Style and shaded viewport settings ───────────────────────────────
    let mut style_options = vec![PlotChoice::localized(STYLE_NONE)];
    style_options.extend(s.plot_styles.iter().cloned().map(PlotChoice::raw));
    if !s.style_name.is_empty()
        && !style_options
            .iter()
            .any(|choice| choice.raw.eq_ignore_ascii_case(&s.style_name))
    {
        style_options.push(PlotChoice::raw(s.style_name.clone()));
    }
    let style_selected = if s.style_name.is_empty() {
        PlotChoice::localized(STYLE_NONE)
    } else {
        PlotChoice::raw(s.style_name.clone())
    };
    let style_panel = panel(column![
        section_label(t!("Plot style table (pen assignments)")),
        drop_row(
            t!("Table"),
            style_options,
            Some(style_selected),
            PlotDlgMsg::Style,
            width,
        ),
        check_enabled(
            t!("Plot with plot styles"),
            s.apply_plot_styles,
            PlotFlag::PlotStyles,
            !s.style_name.is_empty(),
        ),
        check_enabled(
            t!("Display plot styles"),
            s.show_plot_styles,
            PlotFlag::DisplayStyles,
            s.paper_space && !s.style_name.is_empty(),
        ),
        row![
            button(text(t!("Load…")).size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::LoadStyle))
                .style(btn(false))
                .padding([4, 10]),

            button(text(t!("Edit…")).size(11))
                .on_press(Message::PlotStylePanelOpen)
                .style(btn(false))
                .padding([4, 10]),
        ]
        .spacing(6)
        .align_y(iced::Center),
    ].spacing(7));

    let shaded_panel = panel(column![
        section_label(t!("Shaded viewport options")),
        drop_row(
            t!("Shade plot"),
            choices(&[
                "As displayed",
                "2D Wireframe",
                "3D Wireframe",
                "Hidden Line",
                "Flat Shaded",
                "Gouraud Shaded",
                "Flat Shaded + Edges",
                "Gouraud Shaded + Edges",
            ]),
            Some(PlotChoice::localized(s.shade.clone())),
            PlotDlgMsg::Shade,
            width,
        ),
        drop_row(
            t!("Quality"),
            choices(&["Low", "Normal", "High"]),
            Some(PlotChoice::localized(s.quality.clone())),
            PlotDlgMsg::Quality,
            width,
        ),
    ].spacing(7));

    // ── Output options and orientation ────────────────────────────────────
    let paper_order_option: Element<'_, Message> = if s.paper_space {
        check(
            t!("Paper space last"),
            s.paperspace_last,
            PlotFlag::PaperspaceLast,
        )
    } else {
        Space::new().height(0).into()
    };
    let options_panel = panel(column![
        section_label(t!("Plot options")),
        row![
            column![
                check(t!("Plot in background"), s.background, PlotFlag::Background),
                check(t!("Object lineweights"), s.lineweights, PlotFlag::Lineweights),
                check(t!("Plot transparency"), s.transparency, PlotFlag::Transparency),
            ]
            .spacing(6)
            .width(width),
            column![
                paper_order_option,
                check(t!("Merge overlapping lines"), s.merge_lines, PlotFlag::MergeLines),
                check(t!("Plot stamp"), s.stamp, PlotFlag::Stamp),
            ]
            .spacing(6)
            .width(width),
        ]
        .spacing(10),
    ].spacing(7));

    let orientation_panel = panel(column![
        drop_row(
            t!("Orientation"),
            choices(&["Portrait", "Landscape"]),
            Some(PlotChoice::localized(s.orientation.clone())),
            PlotDlgMsg::Orientation,
            width,
        ),
        check(t!("Plot upside-down"), s.upside_down, PlotFlag::UpsideDown),
    ].spacing(7));

    let left = column![
        printer_panel,
        hdivider(width),
        paper_panel,
        orientation_panel,
        hdivider(width),
        area_panel,
    ]
        .spacing(9)
        .width(width);
    let right = column![
        scale_panel,
        hdivider(width),
        style_panel,
        hdivider(width),
        shaded_panel,
        hdivider(width),
        options_panel,
    ]
        .spacing(9)
        .width(width);
    let detail = scrollable(
        container(row![left, right].spacing(18).width(width)).padding(14),
    )
    .width(width)
    .height(height);
    let body = row![list_panel, vsep(height), detail]
        .width(width)
        .height(height);
    let mut toolbar_row = row![left_bar, Space::new().width(width)]
    .align_y(iced::Center)
    .push(
        button(text(t!("Set current")).size(11))
            .on_press(Message::PlotDlg(PlotDlgMsg::SetCurrent))
            .style(btn(false))
            .padding([4, 12]),
    )
    .push(Space::new().width(6))
    .push(
        button(text(t!("Preview")).size(11))
            .on_press(Message::PlotDlg(PlotDlgMsg::Preview))
            .style(btn(false))
            .padding([4, 12]),
    )
    .push(Space::new().width(6));
    toolbar_row = toolbar_row.push(
        button(text(action).size(11))
            .on_press(Message::PlotDlg(PlotDlgMsg::Commit))
            .style(btn(true))
            .padding([4, 18]),
    );
    let toolbar = container(toolbar_row)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .padding([5, 10])
        .width(width);

    container(column![toolbar, hdivider(width), body].spacing(0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.base.color
            )),
            ..Default::default()
        })
        .width(width)
        .height(height)
        .into()
}
