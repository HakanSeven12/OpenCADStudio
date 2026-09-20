//! Plot / Print dialog — a full plot setup surface rendered as an in-canvas
//! modal (Plan B). Bundles printer choice, paper, scale, offset, plot style,
//! quality and output options into one dialog; on commit it either sends the
//! current layout to a system printer (with the chosen options) or writes a
//! PDF. Styled to match the other OCS dialogs (dark pills + fields).

use crate::app::Message;
use crate::io::paper_catalog::{self, CustomPaper, Margins, PaperSize, PaperUnits};
use crate::io::plot_device::PrinterCapabilities;
use crate::t;
use crate::ui::style::common::muted_style;
use crate::ui::style::form;
use crate::ui::style::form::{hdivider, vseparator as vsep};
use iced::widget::{
    button, checkbox, column, container, mouse_area, row, scrollable, text, text_input, Space,
};
use iced::{Background, Border, Element, Fit, Length, Theme};
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
/// A paper choice keeps the canonical media name as its value and shows the
/// sheet's human name with its dimensions.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PlotChoice {
    raw: String,
    localized: bool,
    /// Display text when it differs from `raw` (paper sizes).
    display: Option<String>,
}

impl PlotChoice {
    fn raw(value: impl Into<String>) -> Self {
        Self {
            raw: value.into(),
            localized: false,
            display: None,
        }
    }

    fn localized(value: impl Into<String>) -> Self {
        Self {
            raw: value.into(),
            localized: true,
            display: None,
        }
    }

    fn paper(paper: &PaperSize) -> Self {
        Self {
            raw: paper.canonical.to_string(),
            localized: false,
            display: Some(paper.display()),
        }
    }
}

impl fmt::Display for PlotChoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(display) = &self.display {
            formatter.write_str(display)
        } else if self.localized {
            if let Some(name) = self.raw.strip_prefix("View: ") {
                formatter.write_str(crate::tf!("View: {name}").as_ref())
            } else {
                formatter.write_str(crate::i18n::translate(&self.raw).as_ref())
            }
        } else {
            formatter.write_str(&self.raw)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CustomPaperDraft, CustomPaperError, PlotChoice};
    use crate::io::paper_catalog::{resolve, Margins, PaperUnits};

    #[test]
    fn custom_sheet_draft_validates_and_builds() {
        let mut draft = CustomPaperDraft {
            name: "  Roll 24  ".into(),
            width: "1500".into(),
            height: "609,6".into(),
            inches: false,
            margins: ["5".into(), "17".into(), "".into(), "abc".into()],
            error: None,
        };
        let custom = draft.build().expect("valid sheet");
        assert_eq!(custom.name, "Roll 24");
        assert_eq!((custom.width, custom.height), (1500.0, 609.6));
        assert_eq!(custom.units, PaperUnits::Millimeters);
        // Blank or unparsable margins mean no margin, never a refusal.
        assert_eq!(
            custom.margins,
            Margins {
                left: 5.0,
                bottom: 17.0,
                right: 0.0,
                top: 0.0
            }
        );
        assert_eq!(custom.canonical(), "Roll_24_(609.60_x_1500.00_MM)");
        draft.width = "0".into();
        assert_eq!(draft.build().unwrap_err(), CustomPaperError::Size);
        draft.width = "x".into();
        assert_eq!(draft.build().unwrap_err(), CustomPaperError::Size);
    }

    #[test]
    fn custom_sheet_draft_seeds_from_a_sheet_in_its_own_unit() {
        let arch_d = resolve("ARCH_D_(24.00_x_36.00_Inches)").unwrap();
        let draft = CustomPaperDraft::from_paper(&arch_d, Margins::uniform(6.35));
        assert_eq!(draft.name, "ARCH D");
        assert_eq!((draft.width.as_str(), draft.height.as_str()), ("24", "36"));
        assert!(draft.inches);
        assert_eq!(draft.margins, ["0.25", "0.25", "0.25", "0.25"]);
        let a4 = resolve("ISO_A4_(210.00_x_297.00_MM)").unwrap();
        let draft = CustomPaperDraft::from_paper(
            &a4,
            Margins {
                left: 5.0,
                bottom: 17.0,
                right: 6.0,
                top: 18.0,
            },
        );
        assert!(!draft.inches);
        assert_eq!(draft.margins, ["5", "17", "6", "18"]);
    }

    #[test]
    fn named_view_display_preserves_the_saved_plot_area() {
        let name = "Plan: north";
        let choice = PlotChoice::localized(format!("View: {name}"));
        assert_eq!(choice.to_string(), crate::tf!("View: {name}").as_ref());
        assert_eq!(choice.raw.strip_prefix("View: "), Some(name));
        assert_eq!(
            PlotChoice::raw("View: PDF printer").to_string(),
            "View: PDF printer"
        );
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
    /// Hidden-line removal for paper-space objects (the page setup's
    /// "plot hidden" flag).
    HidePaperspace,
    /// Write the dialog into the layout when a plot is sent.
    SaveToLayout,
}

/// The page setups of another drawing, offered for import (`PSETUPIN`).
#[derive(Debug, Clone, PartialEq)]
pub struct PageSetupImportDraft {
    /// The drawing they come from, as shown to the user.
    pub file: String,
    /// Each setup with whether it is ticked for import. The settings are
    /// carried along so the source file is read once.
    pub setups: Vec<(String, acadrust::objects::PlotSettings, bool)>,
    /// Why the file gave nothing, when it did not.
    pub error: Option<String>,
}

impl PageSetupImportDraft {
    pub fn selected(&self) -> impl Iterator<Item = (&str, &acadrust::objects::PlotSettings)> {
        self.setups
            .iter()
            .filter(|(_, _, on)| *on)
            .map(|(name, ps, _)| (name.as_str(), ps))
    }
}

/// Edits of the page-setup import chooser.
#[derive(Debug, Clone)]
pub enum PageSetupImportMsg {
    /// Ask for the drawing to import from.
    Pick,
    /// The drawing was read (or could not be): its name and page setups.
    Loaded(Result<(String, Vec<(String, acadrust::objects::PlotSettings)>), String>),
    /// Tick or untick one setup.
    Toggle(String),
    /// Tick or untick every setup.
    All(bool),
    /// Import the ticked setups and close the chooser.
    Apply,
    Cancel,
}

/// Every edit the Plot dialog can emit. Wrapped in `Message::PlotDlg` so the
/// top-level match stays a single arm.
#[derive(Debug, Clone)]
pub enum PlotDlgMsg {
    Close,
    Commit,
    Preview,
    PrinterProperties,
    /// The driver options of a printer arrived (or failed to), for the
    /// in-line properties editor; carries the printer's name so a late
    /// answer for a printer the user has since left is ignored.
    PrinterOptionsLoaded(
        String,
        Result<Vec<crate::io::print_to_printer::PrinterOption>, String>,
    ),
    /// One option of the properties editor was set to a choice keyword.
    PrinterOptionSet(String, String),
    /// Keep the editor's choices for the printer and close it.
    PrinterOptionsApply,
    /// Put every option back to the driver's default.
    PrinterOptionsReset,
    /// Close the editor without keeping its changes.
    PrinterOptionsCancel,
    Printer(String),
    /// A printer answered (or failed) the media query started when it was
    /// selected; carries the printer's name so a stale answer for a printer
    /// the user has since left is ignored.
    PrinterMedia(String, Option<std::sync::Arc<PrinterCapabilities>>),
    Paper(String),
    /// Edits of the inline "custom paper size" editor under the Size row.
    CustomPaper(CustomPaperMsg),
    Orientation(String),
    Area(String),
    Scale(String),
    /// The unit the page setup counts in (offsets, custom scale).
    PaperUnits(String),
    /// The paper side of the custom scale, in the page-setup unit.
    CustomScalePaper(String),
    /// The drawing side of the custom scale.
    CustomScaleDrawing(String),
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
    /// Import named page setups from another drawing (`PSETUPIN`).
    Import(PageSetupImportMsg),
    /// Delete the selected named page setup.
    DeleteSetup,
    /// Live edit of the new/rename name field.
    NameInput(String),
    /// Confirm the new/rename name.
    NameCommit,
    /// Cancel the new/rename name row.
    NameCancel,
}

/// Which side of the printable-margin row a value belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarginSide {
    Left,
    Bottom,
    Right,
    Top,
}

#[derive(Debug, Clone)]
pub enum CustomPaperMsg {
    /// Open the editor, seeded from the selected sheet.
    Open,
    Cancel,
    Name(String),
    Width(String),
    Height(String),
    /// Units picked from the dropdown, by their catalogue label.
    Units(String),
    Margin(MarginSide, String),
    /// Add (or replace) the sheet and select it.
    Add,
    /// Remove the selected user-defined sheet.
    Remove,
}

/// The inline custom-sheet editor's fields, kept as typed text so a half-typed
/// number does not snap to a value while editing. Everything is in the
/// draft's own units.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CustomPaperDraft {
    pub name: String,
    pub width: String,
    pub height: String,
    pub inches: bool,
    /// Left, bottom, right, top.
    pub margins: [String; 4],
    /// Why the last Add was refused, shown under the fields.
    pub error: Option<CustomPaperError>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomPaperError {
    /// Width or height is not a positive number.
    Size,
}

impl CustomPaperDraft {
    fn units(&self) -> PaperUnits {
        if self.inches {
            PaperUnits::Inches
        } else {
            PaperUnits::Millimeters
        }
    }

    /// Seed the editor from a sheet: its dimensions and unit, and the margins
    /// given (in millimetres) converted into that unit.
    pub fn from_paper(paper: &PaperSize, margins_mm: Margins) -> Self {
        let units = paper.units;
        let fmt = |v: f64| {
            let v = units.from_mm(v);
            if (v - v.round()).abs() < 1e-6 {
                format!("{}", v.round() as i64)
            } else {
                format!("{v:.2}")
            }
        };
        let dim = |v: f64| {
            if (v - v.round()).abs() < 1e-6 {
                format!("{}", v.round() as i64)
            } else {
                format!("{v:.2}")
            }
        };
        Self {
            name: paper.label.to_string(),
            width: dim(paper.width),
            height: dim(paper.height),
            inches: units == PaperUnits::Inches,
            margins: [
                fmt(margins_mm.left),
                fmt(margins_mm.bottom),
                fmt(margins_mm.right),
                fmt(margins_mm.top),
            ],
            error: None,
        }
    }

    /// The sheet the fields describe, or the reason they do not describe one.
    pub fn build(&self) -> Result<CustomPaper, CustomPaperError> {
        let number = |text: &str| text.trim().replace(',', ".").parse::<f64>().ok();
        let (Some(width), Some(height)) = (number(&self.width), number(&self.height)) else {
            return Err(CustomPaperError::Size);
        };
        if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
            return Err(CustomPaperError::Size);
        }
        let margin = |text: &str| {
            number(text)
                .filter(|v| v.is_finite() && *v >= 0.0)
                .unwrap_or(0.0)
        };
        Ok(CustomPaper {
            name: self.name.trim().to_string(),
            width,
            height,
            units: self.units(),
            margins: Margins {
                left: margin(&self.margins[0]),
                bottom: margin(&self.margins[1]),
                right: margin(&self.margins[2]),
                top: margin(&self.margins[3]),
            },
        })
    }
}

/// The in-line printer-properties editor: the printer it belongs to, the
/// driver's options, and the choice currently shown for each.
#[derive(Debug, Clone, PartialEq)]
pub struct PrinterOptionsDraft {
    pub printer: String,
    /// `None` while the options are still being fetched.
    pub options: Option<Vec<crate::io::print_to_printer::PrinterOption>>,
    /// Current choice keyword per option key.
    pub choices: std::collections::BTreeMap<String, String>,
    /// Why the options could not be listed, when they could not.
    pub error: Option<String>,
}

impl PrinterOptionsDraft {
    /// The choices that differ from the driver's defaults — all a job needs
    /// to pass along.
    pub fn overrides(&self) -> std::collections::BTreeMap<String, String> {
        let Some(options) = &self.options else {
            return std::collections::BTreeMap::new();
        };
        options
            .iter()
            .filter_map(|option| {
                let choice = self.choices.get(&option.key)?;
                (choice != &option.default).then(|| (option.key.clone(), choice.clone()))
            })
            .collect()
    }
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
    /// Why the system gave no printer list (a stopped spooler, a failing
    /// print service), shown under the printer row so an empty list is
    /// never mistaken for "no printers".
    #[serde(skip)]
    pub printers_error: Option<String>,
    /// Name of the system default printer, shown next to the default entry.
    #[serde(skip)]
    pub default_printer: Option<String>,
    /// Sheets and printable areas the selected printer reported; `None`
    /// while unknown, when the platform cannot ask, or for PDF output.
    #[serde(skip)]
    pub printer_media: Option<std::sync::Arc<PrinterCapabilities>>,
    /// Sheets the user defined, offered for every device and remembered
    /// across sessions.
    pub custom_papers: Vec<CustomPaper>,
    /// The inline custom-sheet editor while it is open.
    #[serde(skip)]
    pub custom_editor: Option<CustomPaperDraft>,
    /// Driver options chosen per printer (`printer → key → choice`), the way
    /// a plotter configuration remembers its device settings; applied to
    /// every job sent to that printer and kept across sessions.
    pub driver_options:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// The in-line printer-properties editor while it is open.
    #[serde(skip)]
    pub printer_editor: Option<PrinterOptionsDraft>,
    /// The page-setup import chooser while it is open.
    #[serde(skip)]
    pub import_draft: Option<PageSetupImportDraft>,
    /// Open the Plot / Page Setup dialog whenever a new layout is created,
    /// the way a page-setup manager is expected to greet a new layout.
    pub page_setup_on_new_layout: bool,
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
    /// The picked world-space plot window (x0, y0, x1, y1) backing
    /// `area == "Window"`. Persisted so reopening the dialog — or the app —
    /// keeps the same window instead of reporting an empty plot area.
    pub window: Option<(f64, f64, f64, f64)>,
    pub center: bool,
    /// Plot offsets in the page-setup unit (`paper_units`); the file keeps
    /// them in millimetres.
    pub offset_x: String,
    pub offset_y: String,
    /// The unit the page setup counts in. Only the offsets and the custom
    /// scale fields are shown in it; the paper space keeps its own unit
    /// because the stored scale is converted along with it.
    pub paper_units: PaperUnits,
    pub scale: String,
    /// The custom scale as `paper = drawing` in the page-setup unit — the
    /// same ratio the picker names, or the one typed here.
    #[serde(skip)]
    pub custom_scale_paper: String,
    #[serde(skip)]
    pub custom_scale_drawing: String,
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
    /// Hidden-line removal for paper-space objects.
    pub hide_paperspace: bool,
    /// Sending a plot also writes the dialog into the layout, the way a
    /// page setup is expected to remember the last plot.
    pub save_to_layout: bool,
    /// `BACKGROUNDPLOT` bit 2: batch plots (PRINTALL / PUBLISH) run in the
    /// background; `background` is bit 1 for single plots.
    pub background_publish: bool,
    /// `PLOTOFFSET`: offsets count from the paper edge instead of the
    /// printable area's corner.
    pub plot_offset_from_edge: bool,
    /// `PAPERUPDATE`: 1 switches to the printer's default sheet when it does
    /// not support the page setup's, 0 keeps the sheet and warns.
    pub paper_update: u8,
    /// `PLOTROTMODE` (0–2): kept for page setups that read it; every
    /// rotation here turns the sheet about the page.
    pub plot_rot_mode: u8,
    /// `PLOTTRANSPARENCYOVERRIDE`: 0 never plots transparency, 1 follows
    /// the dialog, 2 always plots it.
    pub transparency_override: u8,
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
    /// Why the selected table could not be loaded, shown under the Table row.
    #[serde(skip)]
    pub style_error: Option<String>,
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
            printers_error: None,
            default_printer: None,
            printer_media: None,
            custom_papers: Vec::new(),
            custom_editor: None,
            driver_options: std::collections::BTreeMap::new(),
            printer_editor: None,
            import_draft: None,
            page_setup_on_new_layout: false,
            printer: None,
            to_file: false,
            paper: paper_catalog::default_paper().canonical.to_string(),
            paper_width_mm: 297.0,
            paper_height_mm: 210.0,
            orientation: "Landscape".into(),
            upside_down: false,
            copies: "1".into(),
            area: "Window".into(),
            window: None,
            center: true,
            offset_x: "0.0".into(),
            offset_y: "0.0".into(),
            paper_units: PaperUnits::Millimeters,
            scale: "1:1".into(),
            custom_scale_paper: "1".into(),
            custom_scale_drawing: "1".into(),
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
            hide_paperspace: false,
            save_to_layout: true,
            background_publish: false,
            plot_offset_from_edge: false,
            paper_update: 0,
            plot_rot_mode: 2,
            transparency_override: 1,
            style_name: String::new(),
            apply_plot_styles: true,
            show_plot_styles: false,
            plot_styles: Vec::new(),
            style_missing: false,
            style_error: None,
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
        self.window = o.window;
        self.center = o.center;
        self.offset_x = o.offset_x.clone();
        self.offset_y = o.offset_y.clone();
        self.paper_units = o.paper_units;
        self.scale = o.scale.clone();
        self.custom_scale_paper = o.custom_scale_paper.clone();
        self.custom_scale_drawing = o.custom_scale_drawing.clone();
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
        self.hide_paperspace = o.hide_paperspace;
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
    form::button_style(accent)
}

fn section_label<'a>(s: Cow<'static, str>) -> Element<'a, Message> {
    form::section_label(s)
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
            .style(form::field_style)
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
    form::labeled_pick_list(
        label,
        options,
        selected,
        move |choice| Message::PlotDlg(ctor(choice.raw)),
        width,
    )
}

fn drop_row_enabled<'a>(
    label: Cow<'static, str>,
    options: Vec<PlotChoice>,
    selected: Option<PlotChoice>,
    ctor: fn(String) -> PlotDlgMsg,
    width: Length,
    enabled: bool,
) -> Element<'a, Message> {
    form::labeled_pick_list_enabled(
        label,
        options,
        selected,
        move |choice| Message::PlotDlg(ctor(choice.raw)),
        width,
        enabled,
    )
}

/// A `label : text field` row.
fn field_row<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    ctor: fn(String) -> PlotDlgMsg,
    width: u16,
) -> Element<'a, Message> {
    form::labeled_field(
        label,
        value,
        move |s| Message::PlotDlg(ctor(s)),
        width as f32,
    )
}

fn field_row_enabled<'a>(
    label: Cow<'static, str>,
    value: &'a str,
    ctor: fn(String) -> PlotDlgMsg,
    width: u16,
    enabled: bool,
) -> Element<'a, Message> {
    form::labeled_field_enabled(
        label,
        value,
        move |s| Message::PlotDlg(ctor(s)),
        width as f32,
        enabled,
    )
}

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

/// The import chooser: the source drawing's name, one tick per page setup,
/// All / None, and Import / Cancel.
fn page_setup_import_chooser<'a>(
    draft: &'a PageSetupImportDraft,
    height: Length,
) -> Element<'a, Message> {
    let msg = |m: PageSetupImportMsg| Message::PlotDlg(PlotDlgMsg::Import(m));
    let mut rows = column![].spacing(3);
    if let Some(error) = &draft.error {
        rows = rows.push(text(error.clone()).size(10).style(muted_style));
    } else if draft.setups.is_empty() {
        let none = text(t!("No named page setups in this drawing.")).size(10);
        rows = rows.push(none.style(muted_style));
    }
    for (name, _, on) in &draft.setups {
        let name = name.clone();
        rows = rows.push(
            checkbox(*on)
                .label(name.clone())
                .on_toggle(move |_| msg(PageSetupImportMsg::Toggle(name.clone())))
                .size(13)
                .text_size(11),
        );
    }
    let all = !draft.setups.is_empty() && draft.setups.iter().all(|(_, _, on)| *on);
    let any = draft.setups.iter().any(|(_, _, on)| *on);
    let mut import = button(text(t!("Import")).size(11)).style(btn(true)).padding([4, 8]);
    if any {
        import = import.on_press(msg(PageSetupImportMsg::Apply));
    }
    column![
        text(draft.file.clone()).size(10).style(muted_style),
        scrollable(rows).height(height),
        checkbox(all)
            .label(t!("All"))
            .on_toggle(move |on| msg(PageSetupImportMsg::All(on)))
            .size(13)
            .text_size(11),
        row![
            import,
            button(text(t!("Cancel")).size(11))
                .on_press(msg(PageSetupImportMsg::Cancel))
                .style(btn(false))
                .padding([4, 8]),
        ]
        .spacing(4),
    ]
    .spacing(6)
    .padding(4)
    .into()
}

/// The custom-scale row: `Custom: [paper] mm = [drawing] units`. Editing
/// either side replaces the picked scale with the typed ratio.
fn custom_scale_row<'a>(s: &'a PlotDialogState, enabled: bool) -> Element<'a, Message> {
    let field = |value: &'a str, ctor: fn(String) -> PlotDlgMsg| {
        let mut input = text_input("", value).size(11).padding([3, 6]).width(58);
        if enabled {
            input = input.on_input(move |v| Message::PlotDlg(ctor(v)));
        }
        input
    };
    let unit = match s.paper_units {
        PaperUnits::Inches => t!("Inches"),
        PaperUnits::Millimeters => t!("Millimeters"),
    };
    row![
        text(t!("Custom")).size(11).width(form::LABEL_WIDTH),
        field(&s.custom_scale_paper, PlotDlgMsg::CustomScalePaper),
        text(unit).size(11),
        text("=").size(11),
        field(&s.custom_scale_drawing, PlotDlgMsg::CustomScaleDrawing),
        text(t!("units")).size(11),
    ]
    .spacing(6)
    .align_y(iced::Center)
    .into()
}

/// Why the selected plot style table is not loaded, under the Table row —
/// the file's own problem, or where a copy would be found.
fn style_note<'a>(s: &'a PlotDialogState) -> Element<'a, Message> {
    match &s.style_error {
        Some(error) if s.style_missing => {
            text(error.clone()).size(10).style(muted_style).into()
        }
        _ => Space::new().height(0).into(),
    }
}

fn panel<'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(content).width(Length::Fill).into()
}

fn choices(items: &[&str]) -> Vec<PlotChoice> {
    items
        .iter()
        .map(|&value| PlotChoice::localized(value))
        .collect()
}

/// The in-line printer-properties editor: one `label : choice` row per
/// driver option (media type, colour mode, quality, duplex, trays…), with
/// Reset / Cancel / Apply. Mirrors what LibreOffice's Properties… shows for a
/// CUPS printer, without leaving the plot dialog.
fn printer_options_editor<'a>(
    draft: &'a PrinterOptionsDraft,
    width: Length,
) -> Element<'a, Message> {
    let mut rows = column![section_label(Cow::Owned(
        crate::tf!("Printer properties — {printer}", printer = draft.printer).into_owned()
    ))]
    .spacing(6);
    match (&draft.options, &draft.error) {
        (Some(options), _) if options.is_empty() => {
            rows = rows.push(
                text(t!("This printer reports no driver options."))
                    .size(10)
                    .style(muted_style),
            );
        }
        (Some(options), _) => {
            for option in options {
                let current = draft
                    .choices
                    .get(&option.key)
                    .cloned()
                    .unwrap_or_else(|| option.default.clone());
                let choices: Vec<PlotChoice> = option
                    .choices
                    .iter()
                    .map(|choice| PlotChoice {
                        raw: choice.keyword.clone(),
                        localized: false,
                        display: Some(choice.label.clone()),
                    })
                    .collect();
                let selected = choices.iter().find(|c| c.raw == current).cloned();
                let key = option.key.clone();
                rows = rows.push(form::labeled_pick_list(
                    Cow::Owned(option.label.clone()),
                    choices,
                    selected,
                    move |choice| {
                        Message::PlotDlg(PlotDlgMsg::PrinterOptionSet(key.clone(), choice.raw))
                    },
                    Length::Fill,
                ));
            }
        }
        (None, Some(error)) => {
            rows = rows.push(
                text(crate::tf!("Could not read the printer's options: {error}"))
                    .size(10)
                    .style(|theme: &Theme| iced::widget::text::Style {
                        color: Some(theme.palette().danger.base.color),
                    }),
            );
        }
        (None, None) => {
            rows = rows.push(
                text(t!("Reading the printer's options…"))
                    .size(10)
                    .style(muted_style),
            );
        }
    }
    let mut buttons = row![
        button(text(t!("Reset")).size(11))
            .on_press(Message::PlotDlg(PlotDlgMsg::PrinterOptionsReset))
            .style(btn(false))
            .padding([4, 10]),
        Space::new().width(Length::Fill),
        button(text(t!("Cancel")).size(11))
            .on_press(Message::PlotDlg(PlotDlgMsg::PrinterOptionsCancel))
            .style(btn(false))
            .padding([4, 10]),
    ]
    .spacing(6);
    if draft.options.is_some() {
        buttons = buttons.push(
            button(text(t!("Apply")).size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::PrinterOptionsApply))
                .style(btn(true))
                .padding([4, 12]),
        );
    }
    rows = rows.push(buttons);
    container(rows)
        .padding(8)
        .width(width)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// The inline editor for a user-defined sheet: name, dimensions and unit,
/// printable margins, Add / Cancel. Numbers stay text until Add validates
/// them. Every margin field carries its own side label so the four values
/// cannot be confused, and the unit is repeated where the numbers are typed.
fn custom_paper_editor<'a>(draft: &'a CustomPaperDraft, width: Length) -> Element<'a, Message> {
    let msg = |m: CustomPaperMsg| Message::PlotDlg(PlotDlgMsg::CustomPaper(m));
    let unit = if draft.inches { "in" } else { "mm" };
    let unit_options: Vec<PlotChoice> = vec![
        PlotChoice::localized("Millimeters"),
        PlotChoice::localized("Inches"),
    ];
    let unit_selected = Some(PlotChoice::localized(if draft.inches {
        "Inches"
    } else {
        "Millimeters"
    }));
    const SIDE_LABEL: f32 = 52.0;
    const NUMBER: f32 = 64.0;
    let dimension =
        |label: Cow<'static, str>, value: &'a str, ctor: fn(String) -> CustomPaperMsg| {
            form::labeled_field_compact(label, value, move |v| msg(ctor(v)), SIDE_LABEL, NUMBER)
        };
    let margin = |label: Cow<'static, str>, side: MarginSide, index: usize| {
        form::labeled_field_compact(
            label,
            &draft.margins[index],
            move |v| msg(CustomPaperMsg::Margin(side, v)),
            SIDE_LABEL,
            NUMBER,
        )
    };
    let error: Element<'_, Message> = match draft.error {
        Some(CustomPaperError::Size) => text(t!("Enter a positive width and height."))
            .size(10)
            .style(|theme: &Theme| iced::widget::text::Style {
                color: Some(theme.palette().danger.base.color),
            })
            .into(),
        None => Space::new().height(0).into(),
    };
    container(
        column![
            section_label(t!("Custom paper size")),
            form::labeled_field(
                t!("Name"),
                &draft.name,
                move |v| msg(CustomPaperMsg::Name(v)),
                200.0
            ),
            form::labeled_pick_list(
                t!("Units"),
                unit_options,
                unit_selected,
                move |choice| msg(CustomPaperMsg::Units(choice.raw)),
                Length::Fixed(150.0),
            ),
            row![
                dimension(t!("Width"), &draft.width, CustomPaperMsg::Width),
                dimension(t!("Height"), &draft.height, CustomPaperMsg::Height),
                text(unit).size(11).style(muted_style),
            ]
            .spacing(14)
            .align_y(iced::Center),
            text(format!("{} ({unit})", t!("Printable margins")))
                .size(11)
                .style(muted_style),
            row![
                margin(t!("Left"), MarginSide::Left, 0),
                margin(t!("Right"), MarginSide::Right, 2),
            ]
            .spacing(14),
            row![
                margin(t!("Top"), MarginSide::Top, 3),
                margin(t!("Bottom"), MarginSide::Bottom, 1),
            ]
            .spacing(14),
            error,
            row![
                Space::new().width(Length::Fill),
                button(text(t!("Cancel")).size(11))
                    .on_press(msg(CustomPaperMsg::Cancel))
                    .style(btn(false))
                    .padding([4, 10]),
                button(text(t!("Add")).size(11))
                    .on_press(msg(CustomPaperMsg::Add))
                    .style(btn(true))
                    .padding([4, 12]),
            ]
            .spacing(6),
        ]
        .spacing(6),
    )
    .padding(8)
    .width(width)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    })
    .into()
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
                    .style(form::field_style)
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
    // While an import is being chosen, the list shows the other drawing's
    // setups with a tick each instead of this drawing's.
    let list_body: Element<'_, Message> = match &s.import_draft {
        Some(draft) => page_setup_import_chooser(draft, height),
        None => list_body,
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
    let mut import_button = button(text(t!("Import…")).size(11))
        .style(btn(false))
        .padding([4, 12]);
    if !print_all_options && s.import_draft.is_none() {
        import_button = import_button
            .on_press(Message::PlotDlg(PlotDlgMsg::Import(PageSetupImportMsg::Pick)));
    }
    let left_bar = row![
        new_button,
        copy_button,
        delete_button,
        import_button,
    ]
    .spacing(4);

    // ── Printer / plotter ─────────────────────────────────────────────────
    // The default entry names the printer it resolves to, when known, so the
    // user sees where a plot will go without leaving the dialog.
    let default_entry = match &s.default_printer {
        Some(name) => PlotChoice {
            raw: OUT_DEFAULT.to_string(),
            localized: true,
            display: Some(format!("{} ({name})", crate::i18n::translate(OUT_DEFAULT))),
        },
        None => PlotChoice::localized(OUT_DEFAULT),
    };
    let mut printer_opts = vec![default_entry.clone()];
    printer_opts.extend(s.printers.iter().cloned().map(PlotChoice::raw));
    printer_opts.push(PlotChoice::localized(OUT_PDF));
    let printer_sel = if s.to_file {
        Some(PlotChoice::localized(OUT_PDF))
    } else {
        Some(match &s.printer {
            Some(printer) => PlotChoice::raw(printer.clone()),
            None => default_entry,
        })
    };
    // A printer that reported its media lists those sheets (with the
    // printable areas it will actually honour); PDF output and printers that
    // could not be asked list the catalogue by series. A sheet that only
    // exists in the drawing (a driver's custom size, an old config's bare
    // name) is added at the top so the current selection stays selectable.
    let selected_paper = paper_catalog::from_drawing(&s.paper, s.paper_width_mm, s.paper_height_mm);
    let printer_sheets = (!s.to_file).then_some(()).and(s.printer_media.as_deref());
    let mut paper_opts: Vec<PlotChoice> = match printer_sheets {
        Some(caps) => caps
            .media
            .iter()
            .map(|media| PlotChoice::paper(&media.paper))
            .collect(),
        None => paper_catalog::catalog()
            .iter()
            .map(PlotChoice::paper)
            .collect(),
    };
    // User-defined sheets come last, after whatever the device offers.
    for custom in &s.custom_papers {
        let choice = PlotChoice::paper(&custom.paper());
        if !paper_opts.contains(&choice) {
            paper_opts.push(choice);
        }
    }
    let paper_sel = PlotChoice::paper(&selected_paper);
    if !paper_opts.contains(&paper_sel) {
        paper_opts.insert(0, paper_sel.clone());
    }
    let selected_is_custom = s
        .custom_papers
        .iter()
        .any(|custom| custom.canonical() == selected_paper.canonical);
    let paper_source_note: Element<'_, Message> = if printer_sheets.is_some() {
        text(t!(
            "Sheet sizes and printable areas reported by the printer."
        ))
        .size(10)
        .style(muted_style)
        .width(width)
        .into()
    } else {
        Space::new().height(0).into()
    };
    let paper_note: Element<'_, Message> = if s.area == "Layout" {
        text(t!(
            "Layout plots the current sheet using the selected paper size."
        ))
        .size(10)
        .style(muted_style)
        .width(width)
        .into()
    } else {
        Space::new().height(0).into()
    };
    // Heading and picker share one line — the section has nothing else to
    // announce — and the picker takes the whole row so a long queue name
    // stays readable; the driver's Properties… button sits beneath it.
    let output_field = row![
        text(t!("Printer / plotter"))
            .size(11)
            .style(muted_style)
            .width(form::LABEL_WIDTH),
        iced::widget::pick_list(printer_sel, printer_opts, |value| value.to_string())
            .on_select(|choice| Message::PlotDlg(PlotDlgMsg::Printer(choice.raw)))
            .text_size(12)
            .padding([3, 6])
            .width(Length::Fill),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .width(Length::Fill);
    let printer_controls: Element<'_, Message> = if s.to_file {
        Space::new().height(0).into()
    } else {
        row![
            field_row(t!("Copies"), &s.copies, PlotDlgMsg::Copies, 60),
            Space::new().width(Length::Fill),
            button(text(t!("Properties…")).size(11))
                .on_press(Message::PlotDlg(PlotDlgMsg::PrinterProperties))
                .style(btn(false))
                .padding([4, 10]),
        ]
        .align_y(iced::Center)
        .into()
    };
    // A printer with remembered driver choices says so under its row; the
    // editor itself unfolds there when Properties… is pressed.
    let printer_note: Element<'_, Message> = match (&s.printer_editor, &s.printer) {
        (Some(draft), _) => printer_options_editor(draft, width),
        (None, _) if s.printers_error.is_some() => {
            let error = s.printers_error.clone().unwrap_or_default();
            text(crate::tf!("Could not list printers: {error}"))
                .size(10)
                .style(muted_style)
                .into()
        }
        (None, Some(printer)) if s.driver_options.get(printer).is_some_and(|o| !o.is_empty()) => {
            let count = s.driver_options[printer].len();
            text(crate::tf!("{count} driver option(s) set for this printer."))
                .size(10)
                .style(muted_style)
                .into()
        }
        _ => Space::new().height(0).into(),
    };
    let printer_panel = panel(column![output_field, printer_controls, printer_note].spacing(7));

    // ── Paper, area, offset, scale ────────────────────────────────────────
    // The picker takes the whole row so long media names stay readable; the
    // custom-sheet buttons sit right-aligned beneath it, and the editor
    // unfolds in their place.
    let size_row = drop_row(
        t!("Paper size"),
        paper_opts,
        Some(paper_sel),
        PlotDlgMsg::Paper,
        Length::Fill,
    );
    let custom_controls: Element<'_, Message> = match &s.custom_editor {
        Some(draft) => custom_paper_editor(draft, width),
        None => {
            let mut buttons = row![Space::new().width(Length::Fill)].spacing(6);
            if selected_is_custom {
                buttons = buttons.push(
                    button(text(t!("Remove")).size(11))
                        .on_press(Message::PlotDlg(PlotDlgMsg::CustomPaper(
                            CustomPaperMsg::Remove,
                        )))
                        .style(btn(false))
                        .padding([4, 10]),
                );
            }
            buttons
                .push(
                    button(text(t!("Custom…")).size(11))
                        .on_press(Message::PlotDlg(PlotDlgMsg::CustomPaper(
                            CustomPaperMsg::Open,
                        )))
                        .style(btn(false))
                        .padding([4, 10]),
                )
                .into()
        }
    };
    // Like the printer row: the section is its one picker, so heading and
    // control share a line.
    let paper_panel =
        panel(column![size_row, custom_controls, paper_source_note, paper_note,].spacing(7));

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
                .map(|name| PlotChoice::localized(format!("View: {name}"))),
        );
    }
    let mut area_row = row![
        text(t!("What to plot"))
            .size(11)
            .style(muted_style)
            .width(92),
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
    let inches = s.paper_units == PaperUnits::Inches;
    let (offset_x_label, offset_y_label) = if inches {
        (t!("X (in)"), t!("Y (in)"))
    } else {
        (t!("X (mm)"), t!("Y (mm)"))
    };
    let area_panel = panel(column![
        section_label(t!("Plot area")),
        area_row,
        section_label(t!("Plot offset")),
        column![
            field_row_enabled(offset_x_label, &s.offset_x, PlotDlgMsg::OffsetX, 70, common_area && !s.center),
            field_row_enabled(offset_y_label, &s.offset_y, PlotDlgMsg::OffsetY, 70, common_area && !s.center),
        ]
        .spacing(7),
    );
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
        // The custom scale spells the picked ratio in the page-setup unit
        // and accepts one of its own; the unit picker decides that unit.
        drop_row(
            t!("Units"),
            vec![PlotChoice::localized("Millimeters"), PlotChoice::localized("Inches")],
            Some(PlotChoice::localized(if inches { "Inches" } else { "Millimeters" })),
            PlotDlgMsg::PaperUnits,
            width,
        ),
        custom_scale_row(s, common_area && !s.fit_to_paper),
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
        style_note(s),
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
        .spacing(7),
    );

    let shaded_panel = panel(
        column![
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
        ]
        .spacing(7),
    );

    // ── Output options and orientation ────────────────────────────────────
    let paper_space_option = |on: bool, label: Cow<'static, str>, flag: PlotFlag| -> Element<'_, Message> {
        if s.paper_space {
            check(label, on, flag)
        } else {
            Space::new().height(0).into()
        }
    };
    let options_panel = panel(column![
        section_label(t!("Plot options")),
        row![
            column![
                check(t!("Plot in background"), s.background, PlotFlag::Background),
                check(t!("Object lineweights"), s.lineweights, PlotFlag::Lineweights),
                check(t!("Plot transparency"), s.transparency, PlotFlag::Transparency),
                paper_space_option(s.hide_paperspace, t!("Hide paperspace objects"), PlotFlag::HidePaperspace),
            ]
            .spacing(6)
            .width(width),
            column![
                paper_space_option(s.paperspace_last, t!("Paper space last"), PlotFlag::PaperspaceLast),
                check(t!("Merge overlapping lines"), s.merge_lines, PlotFlag::MergeLines),
                check(t!("Plot stamp"), s.stamp, PlotFlag::Stamp),
                paper_space_option(s.save_to_layout, t!("Save changes to layout"), PlotFlag::SaveToLayout),
            ]
            .spacing(10),
        ]
        .spacing(7),
    );

    let orientation_panel = panel(
        column![
            drop_row(
                t!("Orientation"),
                choices(&["Portrait", "Landscape"]),
                Some(PlotChoice::localized(s.orientation.clone())),
                PlotDlgMsg::Orientation,
                width,
            ),
            check(t!("Plot upside-down"), s.upside_down, PlotFlag::UpsideDown),
        ]
        .spacing(7),
    );

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
    let detail = scrollable(container(row![left, right].spacing(18).width(width)).padding(14))
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
            background: Some(Background::Color(theme.palette().background.base.color)),
            ..Default::default()
        })
        .width(width)
        .height(height)
        .into()
}
