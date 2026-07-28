//! Plot / Print dialog — a full plot setup surface rendered as an in-canvas
//! modal (Plan B). Bundles printer choice, paper, scale, offset, plot style,
//! quality and output options into one dialog; on commit it either sends the
//! current layout to a system printer (with the chosen options) or writes a
//! PDF. Styled to match the other OCS dialogs (dark pills + fields).

use crate::app::Message;
use crate::io::paper_sizes::PaperSize;
use iced::widget::{
    button, checkbox, column, container, mouse_area, pick_list, row, scrollable, text, text_input,
    Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};

/// Sentinel entries in the printer dropdown (not real printer names).
pub const OUT_DEFAULT: &str = "System default printer";
pub const OUT_PDF: &str = "Save to PDF file…";

/// Top-of-list entries: no page setup (defaults + PDF), and the last-used
/// settings captured when the dialog opened.
pub const SETUP_NONE: &str = "<none>";
pub const SETUP_PREV: &str = "<previous>";

/// One of the many boolean plot options (folded into a single message so the
/// dialog needn't carry a variant per checkbox).
#[derive(Debug, Clone, Copy)]
pub enum PlotFlag {
    Center,
    ScaleLw,
    Mono,
    Lineweights,
    WithStyles,
    Transparency,
    PaperspaceLast,
    HidePaperspace,
    Stamp,
    SaveLayout,
}

/// Every edit the Plot dialog can emit. Wrapped in `Message::PlotDlg` so the
/// top-level match stays a single arm.
#[derive(Debug, Clone)]
pub enum PlotDlgMsg {
    Close,
    Commit,
    Preview,
    Printer(String),
    Paper(String),
    Orientation(String),
    Rotation(String),
    Area(String),
    Scale(String),
    Quality(String),
    Shade(String),
    Copies(String),
    OffsetX(String),
    OffsetY(String),
    Dpi(String),
    Flag(PlotFlag),
    LoadStyle,
    ClearStyle,
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
    #[serde(skip)]
    pub paper: String,
    #[serde(skip)]
    pub orientation: String,
    #[serde(skip)]
    pub rotation: String,
    pub copies: String,
    pub area: String,
    #[serde(skip)]
    pub center: bool,
    #[serde(skip)]
    pub offset_x: String,
    #[serde(skip)]
    pub offset_y: String,
    pub scale: String,
    pub scale_lw: bool,
    pub quality: String,
    pub dpi: String,
    pub shade: String,
    pub mono: bool,
    pub lineweights: bool,
    pub with_styles: bool,
    pub transparency: bool,
    pub paperspace_last: bool,
    pub hide_paperspace: bool,
    pub stamp: bool,
    pub save_layout: bool,
    /// Display name of the active plot style table ("" = none).
    pub style_name: String,
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
}

impl Default for PlotDialogState {
    fn default() -> Self {
        Self {
            printers: Vec::new(),
            printer: None,
            to_file: false,
            paper: "A4".into(),
            orientation: "Landscape".into(),
            rotation: "0°".into(),
            copies: "1".into(),
            area: "Window".into(),
            center: true,
            offset_x: "0.0".into(),
            offset_y: "0.0".into(),
            scale: "Fit".into(),
            scale_lw: true,
            quality: "Normal".into(),
            dpi: "300".into(),
            shade: "As displayed".into(),
            mono: false,
            lineweights: true,
            with_styles: true,
            transparency: false,
            paperspace_last: false,
            hide_paperspace: false,
            stamp: false,
            save_layout: false,
            style_name: String::new(),
            page_setups: Vec::new(),
            selected_setup: String::new(),
            name_input: None,
            name_rename: false,
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
        self.orientation = o.orientation.clone();
        self.rotation = o.rotation.clone();
        self.copies = o.copies.clone();
        self.area = o.area.clone();
        self.center = o.center;
        self.offset_x = o.offset_x.clone();
        self.offset_y = o.offset_y.clone();
        self.scale = o.scale.clone();
        self.scale_lw = o.scale_lw;
        self.quality = o.quality.clone();
        self.dpi = o.dpi.clone();
        self.shade = o.shade.clone();
        self.mono = o.mono;
        self.lineweights = o.lineweights;
        self.with_styles = o.with_styles;
        self.transparency = o.transparency;
        self.paperspace_last = o.paperspace_last;
        self.hide_paperspace = o.hide_paperspace;
        self.stamp = o.stamp;
        self.save_layout = o.save_layout;
        self.style_name = o.style_name.clone();
    }

}

fn btn(accent: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, st| {
        let palette = theme.extended_palette();
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
    let palette = theme.extended_palette();
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
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn hdivider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

fn section_label<'a>(s: &'static str) -> Element<'a, Message> {
    text(s).size(11).style(muted_style).into()
}

fn vsep<'a>() -> Element<'a, Message> {
    container(Space::new().width(1).height(Fill))
        .width(1)
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

/// One row in the page-setup list: a selectable name, or an inline rename field
/// when this row is being renamed.
fn setup_row<'a>(
    name: &'a str,
    selected: &str,
    renaming: Option<&str>,
    rename_buf: &'a str,
) -> Element<'a, Message> {
    if renaming == Some(name) {
        return text_input("", rename_buf)
            .on_input(|v| Message::PlotDlg(PlotDlgMsg::NameInput(v)))
            .on_submit(Message::PlotDlg(PlotDlgMsg::NameCommit))
            .style(field_style)
            .size(11)
            .padding([4, 8])
            .width(Fill)
            .into();
    }
    let is_sel = name == selected;
    let cell = container(text(name.to_string()).size(11))
        .padding([4, 8])
        .width(Fill)
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: is_sel.then_some(Background::Color(palette.primary.strong.color)),
            text_color: is_sel.then_some(palette.primary.strong.text),
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
    label: &'a str,
    options: Vec<String>,
    selected: Option<String>,
    ctor: fn(String) -> PlotDlgMsg,
) -> Element<'a, Message> {
    let pl = pick_list(options, selected, move |s| Message::PlotDlg(ctor(s)))
        .text_size(12)
        .padding([3, 6])
        .width(Length::Fill);
    row![text(label).size(11).style(muted_style).width(92), pl]
        .spacing(8)
        .align_y(iced::Center)
        .into()
}

/// A `label : text field` row.
fn field_row<'a>(
    label: &'a str,
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

/// A single option checkbox bound to a `PlotFlag`.
fn check<'a>(label: &'a str, on: bool, flag: PlotFlag) -> Element<'a, Message> {
    checkbox(on)
        .label(label)
        .on_toggle(move |_| Message::PlotDlg(PlotDlgMsg::Flag(flag)))
        .size(14)
        .text_size(11)
        .into()
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub fn view_window(s: &PlotDialogState) -> Element<'_, Message> {
    // ── Toolbar: Cancel … Preview  Print/Export ──────────────────────────
    let action = if s.to_file { "Export PDF" } else { "Print" };
    // `<none>` / `<previous>` are pseudo-entries; layout rows are `*name*`.
    let is_special = s.selected_setup == SETUP_NONE || s.selected_setup == SETUP_PREV;
    let sel_is_layout = s.selected_setup.len() >= 2
        && s.selected_setup.starts_with('*')
        && s.selected_setup.ends_with('*');
    let can_copy = !s.selected_setup.is_empty() && !is_special;
    let is_named = can_copy && !sel_is_layout;
    let mut left_bar = row![button(text("New").size(11))
        .on_press(Message::PlotDlg(PlotDlgMsg::NewSetup))
        .style(btn(false))
        .padding([4, 12])]
    .spacing(4);
    if can_copy {
        left_bar = left_bar.push(
            button(text("Copy").size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::CopySetup))
                .style(btn(false))
                .padding([4, 12]),
        );
    }
    if is_named {
        left_bar = left_bar.push(
            button(text("Delete").size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::DeleteSetup))
                .style(btn(false))
                .padding([4, 12]),
        );
    }
    let toolbar = container(
        row![
            left_bar,
            Space::new().width(Fill),
            button(text("Set current").size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::SetCurrent))
                .style(btn(false))
                .padding([4, 12]),
            Space::new().width(6),
            button(text("Preview").size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::Preview))
                .style(btn(false))
                .padding([4, 12]),
            Space::new().width(6),
            button(text(action).size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::Commit))
                .style(btn(true))
                .padding([4, 18]),
        ]
        .align_y(iced::Center),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.weak.color
        )),
        ..Default::default()
    })
    .width(Fill)
    .padding([5, 10]);

    // ── Printer dropdown: default + discovered printers + PDF sentinel ────
    let mut printer_opts = vec![OUT_DEFAULT.to_string()];
    printer_opts.extend(s.printers.iter().cloned());
    printer_opts.push(OUT_PDF.to_string());
    let printer_sel = if s.to_file {
        Some(OUT_PDF.to_string())
    } else {
        Some(s.printer.clone().unwrap_or_else(|| OUT_DEFAULT.to_string()))
    };

    let paper_opts: Vec<String> = PaperSize::ALL.iter().map(|p| p.label().to_string()).collect();

    // ── Left panel: named page-setup list (click selects, double-click renames)
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
        container(text("(no page setups)").size(11).style(muted_style))
            .padding([6, 8])
            .into()
    } else {
        scrollable(column(rows).spacing(1)).height(Fill).into()
    };
    let list_panel = container(
        column![
            text("Page setups").size(10).style(muted_style),
            container(list_body)
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
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
                .width(Fill)
                .height(Fill)
                .padding(2),
        ]
        .spacing(4)
        .height(Fill),
    )
    .width(160)
    .height(Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    // ── Left column ──────────────────────────────────────────────────────
    let style_label: String = if s.style_name.is_empty() {
        "(none)".into()
    } else {
        s.style_name.clone()
    };
    let left = column![
        section_label("Printer / plotter"),
        drop_row("Output", printer_opts, printer_sel, PlotDlgMsg::Printer),
        field_row("Copies", &s.copies, PlotDlgMsg::Copies, 60),
        hdivider(),
        section_label("Paper"),
        drop_row("Size", paper_opts, Some(s.paper.clone()), PlotDlgMsg::Paper),
        drop_row(
            "Orientation",
            strs(&["Portrait", "Landscape"]),
            Some(s.orientation.clone()),
            PlotDlgMsg::Orientation,
        ),
        drop_row(
            "Rotation",
            strs(&["0°", "90°", "180°", "270°"]),
            Some(s.rotation.clone()),
            PlotDlgMsg::Rotation,
        ),
        hdivider(),
        section_label("Plot area"),
        row![
            container(
                pick_list(
                    strs(&["Layout", "Extents", "Display", "Window"]),
                    Some(s.area.clone()),
                    move |v| Message::PlotDlg(PlotDlgMsg::Area(v)),
                )
                .text_size(12)
                .padding([3, 6])
                .width(Length::Fill)
            )
            .width(Fill),
            button(text("Pick…").size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::PickWindow))
                .style(btn(false))
                .padding([4, 10]),
        ]
        .spacing(8)
        .align_y(iced::Center),
        section_label("Plot offset"),
        column![
            field_row("X (mm)", &s.offset_x, PlotDlgMsg::OffsetX, 70),
            field_row("Y (mm)", &s.offset_y, PlotDlgMsg::OffsetY, 70),
        ]
        .spacing(9),
        check("Center the plot", s.center, PlotFlag::Center),
    ]
    .spacing(9)
    .width(Fill);

    // ── Right column ─────────────────────────────────────────────────────
    let right = column![
        section_label("Plot scale"),
        drop_row(
            "Scale",
            strs(&["Fit", "1:1", "1:2", "1:5", "1:10", "1:20", "1:50", "1:100", "2:1"]),
            Some(s.scale.clone()),
            PlotDlgMsg::Scale,
        ),
        check("Scale lineweights", s.scale_lw, PlotFlag::ScaleLw),
        hdivider(),
        section_label("Plot style table (pen assignments)"),
        row![
            container(text(style_label).size(12))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.base.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                    }
                })
                .padding([4, 8])
                .width(Fill),
            button(text("Load…").size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::LoadStyle))
                .style(btn(false))
                .padding([4, 10]),
            button(text("Clear").size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::ClearStyle))
                .style(btn(false))
                .padding([4, 10]),
        ]
        .spacing(6)
        .align_y(iced::Center),
        hdivider(),
        section_label("Quality"),
        row![
            container(
                pick_list(
                    strs(&["Draft", "Normal", "High", "Maximum"]),
                    Some(s.quality.clone()),
                    move |v| Message::PlotDlg(PlotDlgMsg::Quality(v)),
                )
                .text_size(12)
                .padding([3, 6])
                .width(Length::Fill)
            )
            .width(Fill),
            text("DPI").size(11).style(muted_style),
            text_input("", &s.dpi)
                .on_input(move |v| Message::PlotDlg(PlotDlgMsg::Dpi(v)))
                .style(field_style)
                .size(12)
                .width(56.0),
        ]
        .spacing(8)
        .align_y(iced::Center),
        drop_row(
            "Shaded plot",
            strs(&["As displayed", "Wireframe", "Hidden", "Rendered"]),
            Some(s.shade.clone()),
            PlotDlgMsg::Shade,
        ),
        hdivider(),
        section_label("Plot options"),
        row![
            column![
                check("Object lineweights", s.lineweights, PlotFlag::Lineweights),
                check("Plot with styles", s.with_styles, PlotFlag::WithStyles),
                check("Monochrome", s.mono, PlotFlag::Mono),
                check("Plot transparency", s.transparency, PlotFlag::Transparency),
            ]
            .spacing(6)
            .width(Fill),
            column![
                check("Paperspace last", s.paperspace_last, PlotFlag::PaperspaceLast),
                check("Hide paperspace", s.hide_paperspace, PlotFlag::HidePaperspace),
                check("Plot stamp", s.stamp, PlotFlag::Stamp),
                check("Save to layout", s.save_layout, PlotFlag::SaveLayout),
            ]
            .spacing(6)
            .width(Fill),
        ]
        .spacing(10),
    ]
    .spacing(9)
    .width(Fill);

    let detail = scrollable(
        container(row![left, right].spacing(18).width(Fill)).padding(14),
    )
    .width(Fill)
    .height(Fill);
    let body = row![list_panel, vsep(), detail].height(Fill);

    container(column![toolbar, hdivider(), body].spacing(0))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color
            )),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into()
}
