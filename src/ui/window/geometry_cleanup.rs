//! The Geometry Cleanup palette (GEOMETRYCLEANUPOPEN): the errors that a
//! scan of the selected objects found, by type, with the repairs of the one
//! that is picked. The scan is `ocs_geom::geom2d::cleanup`; the handlers are
//! in `app::commands::geometry_cleanup`.

use crate::app::Message;
use crate::modules::manage::geometry_cleanup_settings::CleanupSettings;
use crate::ui::dock::PanelId;
use crate::ui::kit;
use crate::ui::style::kit_metrics as km;
use document::Handle;
use iced::widget::{button, column, container, mouse_area, row, text, Space};
use iced::{Element, Fill, Padding};
use ocs_geom::geom2d::cleanup::{AngleErrorKind, AngleTolerances, EndErrorKind, Tolerances};

pub const ICON: &[u8] = include_bytes!("../../../assets/icons/geometry_cleanup.svg");

/// The pointer is on an error's marker in the drawing within this many
/// pixels of its centre.
pub const MARKER_REACH: f32 = 8.0;

/// The length types, in the order of the tree and of the settings rows.
pub const KINDS: [EndErrorKind; 3] = [
    EndErrorKind::Gap,
    EndErrorKind::Overshoot,
    EndErrorKind::Undershoot,
];

pub fn kind_label(kind: EndErrorKind) -> &'static str {
    ErrorType::from(kind).label()
}

/// An error type of the palette, in the order of the tree.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ErrorType {
    Gap,
    Overshoot,
    Undershoot,
    AbsoluteAngle,
    RelativeAngle,
}

pub const TYPES: [ErrorType; 5] = [
    ErrorType::Gap,
    ErrorType::Overshoot,
    ErrorType::Undershoot,
    ErrorType::AbsoluteAngle,
    ErrorType::RelativeAngle,
];

impl ErrorType {
    pub fn label(self) -> &'static str {
        match self {
            ErrorType::Gap => "Gap",
            ErrorType::Overshoot => "Overshoot",
            ErrorType::Undershoot => "Undershoot",
            ErrorType::AbsoluteAngle => "Absolute Angle",
            ErrorType::RelativeAngle => "Relative Angle",
        }
    }

    /// The length type, for a length error.
    pub fn end_kind(self) -> Option<EndErrorKind> {
        match self {
            ErrorType::Gap => Some(EndErrorKind::Gap),
            ErrorType::Overshoot => Some(EndErrorKind::Overshoot),
            ErrorType::Undershoot => Some(EndErrorKind::Undershoot),
            ErrorType::AbsoluteAngle | ErrorType::RelativeAngle => None,
        }
    }
}

impl From<EndErrorKind> for ErrorType {
    fn from(kind: EndErrorKind) -> Self {
        match kind {
            EndErrorKind::Gap => ErrorType::Gap,
            EndErrorKind::Overshoot => ErrorType::Overshoot,
            EndErrorKind::Undershoot => ErrorType::Undershoot,
        }
    }
}

impl From<AngleErrorKind> for ErrorType {
    fn from(kind: AngleErrorKind) -> Self {
        match kind {
            AngleErrorKind::Absolute => ErrorType::AbsoluteAngle,
            AngleErrorKind::Relative => ErrorType::RelativeAngle,
        }
    }
}

/// A repair of one error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fix {
    /// Cut the end back to the crossing (an overshoot).
    Trim,
    /// Run the end on to what it falls short of (an undershoot).
    Extend,
    /// Move only the end onto the other end (a gap).
    Stretch,
    /// Move the whole object so that its end meets the other end (a gap).
    Move,
    /// Run both ends on to where their lines cross (a gap at a corner).
    ExtendBoth,
    /// Turn the line about its start onto the angle (an absolute angle).
    RotateAtStart,
    /// Turn the line about its end onto the angle (an absolute angle).
    RotateAtEnd,
    /// Turn the shorter of two lines about their meet (a relative angle).
    RotateShorter,
    /// Turn the longer of two lines about their meet (a relative angle).
    RotateLonger,
}

impl Fix {
    /// What the repair changes, for its tooltip.
    pub fn tip(self) -> &'static str {
        match self {
            Fix::Trim => "Cut the end back to the object it crosses.",
            Fix::Extend => {
                "Lengthen the end along its own line or arc to the object it stops short of."
            }
            Fix::Stretch => "Move only this end onto the other end. The rest of the object stays.",
            Fix::Move => "Move the whole object so that its end meets the other end.",
            Fix::ExtendBoth => {
                "Lengthen both ends along their own lines to the corner where they meet."
            }
            Fix::RotateAtStart => "Turn the line about its start so that it is on the angle.",
            Fix::RotateAtEnd => "Turn the line about its end so that it is on the angle.",
            Fix::RotateShorter => "Turn the shorter line about the point where the lines meet.",
            Fix::RotateLonger => "Turn the longer line about the point where the lines meet.",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Fix::Trim => "Trim",
            Fix::Extend | Fix::ExtendBoth => "Extend",
            Fix::Stretch => "Stretch",
            Fix::Move => "Move",
            Fix::RotateAtStart => "Rotate at Start",
            Fix::RotateAtEnd => "Rotate at End",
            Fix::RotateShorter => "Rotate Shorter",
            Fix::RotateLonger => "Rotate Longer",
        }
    }
}

/// One error of a scan, with the objects in place of curve indices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finding {
    pub kind: ErrorType,
    pub handle: Handle,
    pub at_start: bool,
    pub location: [f64; 2],
    pub target: [f64; 2],
    /// A gap's other end, or the line that a relative angle meets.
    pub other: Option<(Handle, bool)>,
    /// A gap's corner, for Extend.
    pub corner: Option<[f64; 2]>,
    /// In degrees for an angle error.
    pub deviation: f64,
    /// What an angle error needs for its repair.
    pub angle: Option<AngleFinding>,
    /// Whether Stretch can put the end on `target`: not on an elliptical
    /// arc, whose end only goes round its ellipse.
    pub stretch: bool,
}

/// An angle error's standard angle and turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AngleFinding {
    /// The standard angle, in degrees.
    pub angle: f64,
    /// The turn in radians, counter-clockwise, that puts the line (`handle`)
    /// on the angle; the opposite turn of the other line does it as well.
    pub turn: f64,
    /// Whether the start and the end of the line meet other geometry.
    pub meets: [bool; 2],
    /// The segment of a polyline that is off the angle, and the segment of
    /// the other polyline (Relative); `None` for a line.
    pub segment: Option<usize>,
    pub other_segment: Option<usize>,
}

/// An error that the user keeps as designed: what identifies it across
/// scans, in one drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct KeptError {
    pub kind: ErrorType,
    pub handle: u64,
    pub at_start: bool,
    /// The segments of an angle error; `None` for a length error.
    pub segments: Option<(Option<usize>, Option<usize>)>,
    /// The other line of a relative angle.
    pub other: Option<u64>,
}

impl Finding {
    /// What identifies this error across scans: its type, its object and
    /// end, its segments, and the other line of a relative angle.
    pub fn identity(&self) -> KeptError {
        KeptError {
            kind: self.kind,
            handle: self.handle.value(),
            at_start: self.at_start,
            segments: self.angle.map(|a| (a.segment, a.other_segment)),
            other: self
                .other
                .filter(|_| self.kind == ErrorType::RelativeAngle)
                .map(|o| o.0.value()),
        }
    }

    /// Whether `other` is the same error.
    pub fn same_error(&self, other: &Finding) -> bool {
        self.identity() == other.identity()
    }

    /// The repairs that this error offers.
    pub fn fixes(&self) -> Vec<Fix> {
        match self.kind {
            ErrorType::Overshoot => vec![Fix::Trim],
            ErrorType::Undershoot => vec![Fix::Extend],
            // The end that meets other geometry first: turning about it
            // keeps it there.
            ErrorType::AbsoluteAngle => match self.angle.map(|a| a.meets) {
                Some([false, true]) => vec![Fix::RotateAtEnd, Fix::RotateAtStart],
                _ => vec![Fix::RotateAtStart, Fix::RotateAtEnd],
            },
            ErrorType::RelativeAngle => vec![Fix::RotateShorter, Fix::RotateLonger],
            ErrorType::Gap => {
                let mut fixes = Vec::new();
                if self.corner.is_some() {
                    fixes.push(Fix::ExtendBoth);
                }
                if self.stretch {
                    fixes.push(Fix::Stretch);
                }
                // Moving a polyline so that one end meets its other end
                // cannot close it.
                if self.other.is_some_and(|(other, _)| other != self.handle) {
                    fixes.push(Fix::Move);
                }
                fixes
            }
        }
    }
}

/// The result of the last scan.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    /// The drawing tab that was scanned.
    pub tab: u64,
    /// Its geometry epoch at the scan: the palette scans again when the
    /// drawing has changed since.
    pub epoch: u64,
    /// The objects that were selected for the scan.
    pub objects: Vec<Handle>,
    /// The same objects, to find a changed object among them quickly.
    pub object_set: std::collections::HashSet<Handle>,
    pub tolerances: Tolerances,
    pub angles: AngleTolerances,
    /// All the errors found, listed and kept, in the order of the scan: a
    /// Keep or a Restore splits them again without a scan.
    pub found: Vec<Finding>,
    pub findings: Vec<Finding>,
    /// The errors found that the user keeps as designed: not listed with
    /// the others, and not repaired.
    pub kept: Vec<Finding>,
}

impl Scan {
    /// Split the errors found into those listed and those kept by `marks`.
    pub fn split(&mut self, marks: &std::collections::HashSet<KeptError>) {
        let (kept, findings) = self
            .found
            .iter()
            .copied()
            .partition(|finding| marks.contains(&finding.identity()));
        self.kept = kept;
        self.findings = findings;
    }
}

#[derive(Debug, Default)]
pub struct GeometryCleanupPalette {
    pub show: bool,
    pub scan: Option<Scan>,
    /// The index in the scan's findings of the picked error.
    pub picked: Option<usize>,
    /// The error types whose list is folded.
    pub folded: Vec<ErrorType>,
    /// The saved settings; `None` until the user saves them.
    pub settings: Option<CleanupSettings>,
    /// The file that the saved settings were imported from, for display.
    pub settings_file: Option<String>,
    /// The open Settings dialog.
    pub dialog: Option<crate::ui::window::geometry_cleanup_settings::SettingsDialog>,
    /// The errors kept as designed, by the FINGERPRINTGUID of their drawing:
    /// a handle is unique only in its drawing.
    pub kept: std::collections::BTreeMap<String, Vec<KeptError>>,
    /// Whether the list of the kept errors is open.
    pub kept_open: bool,
    /// The error whose marker menu is open in the drawing.
    pub menu: Option<usize>,
    /// Whether the pointer is on the marker menu, which keeps it open.
    pub menu_hovered: bool,
    /// The repair whose result the drawing previews.
    pub preview: Option<(usize, Fix)>,
    /// Review mode: the picked error's objects are drawn bright and the
    /// rest faded, with the review toolbar over the drawing. A pick starts
    /// it; GEOMETRYERRORREVIEWCLOSE, the toolbar's Close, a new selection
    /// and closing the palette end it.
    pub review: bool,
    /// Whether the drawing shows the review display now, so that ending it
    /// does not clear the display of count mode.
    pub review_shown: bool,
    /// GEOMETRYCLEANUPZOOMPAN 0: a pick zooms only when the error is not in
    /// view. Kept in the user profile.
    pub auto_zoom_off: bool,
}

#[derive(Debug, Clone)]
pub enum GeometryCleanupMsg {
    SelectObjects,
    Rescan,
    /// Pick an error: zoom to it and show its repairs.
    Pick(usize),
    Fix(usize, Fix),
    /// Keep an error as designed: it is not listed again.
    Keep(usize),
    /// List a kept error again, by its index in the scan's kept errors.
    Restore(usize),
    /// List all the kept errors of the drawing again.
    RestoreAll,
    Fold(ErrorType),
    /// Open or close the list of the kept errors.
    FoldKept,
    /// The pointer is on a repair (preview its result) or left it.
    Preview(Option<(usize, Fix)>),
    /// The pointer came onto the marker menu or left it.
    MenuHover(bool),
    /// The review toolbar's Close: end review mode.
    CloseReview,
    /// The review toolbar's Auto Zoom: GEOMETRYCLEANUPZOOMPAN 1 or 0.
    ToggleAutoZoom,
    /// The Settings dialog.
    OpenSettings,
    SettingsTolerance(EndErrorKind, String),
    /// Put the proposed tolerance in the type's box.
    SettingsUseProposed(EndErrorKind),
    SettingsRemove(EndErrorKind),
    SettingsAdd(EndErrorKind),
    /// The angle rows, by their index in the dialog.
    SettingsAngle(usize, String),
    SettingsAngleTolerance(usize, String),
    SettingsRemoveAngle(usize),
    SettingsAddAngle(AngleErrorKind),
    SettingsImport,
    SettingsImported(std::path::PathBuf),
    SettingsExport,
    SettingsExportTo(std::path::PathBuf),
    SettingsCancel,
    /// Save the settings, close the dialog and scan.
    SettingsScan,
}

/// The errors of a scan by type, in the order of the tree: each type with
/// the indices of its errors in `scan.findings`. Types with no error are
/// left out.
pub fn groups(scan: &Scan) -> Vec<(ErrorType, Vec<usize>)> {
    TYPES
        .into_iter()
        .map(|kind| {
            let indices = (0..scan.findings.len())
                .filter(|&index| scan.findings[index].kind == kind)
                .collect::<Vec<_>>();
            (kind, indices)
        })
        .filter(|(_, indices)| !indices.is_empty())
        .collect()
}

/// The rows that the palette shows, top to bottom: the errors of the types
/// that are not folded.
pub fn rows(palette: &GeometryCleanupPalette, scan: &Scan) -> Vec<usize> {
    groups(scan)
        .into_iter()
        .filter(|(kind, _)| !palette.folded.contains(kind))
        .flat_map(|(_, indices)| indices)
        .collect()
}

fn msg(m: GeometryCleanupMsg) -> Message {
    Message::GeometryCleanup(m)
}

fn coordinates(p: [f64; 2]) -> String {
    format!("{:.4}, {:.4}", p[0], p[1])
}

pub fn view<'a>(
    palette: &'a GeometryCleanupPalette,
    tab: u64,
    width: f32,
    chrome: crate::ui::dock::Chrome,
) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(
        PanelId::GeometryCleanup,
        crate::t!("Geometry Cleanup").into_owned(),
        chrome,
    );
    let scan = palette.scan.as_ref().filter(|scan| scan.tab == tab);
    let tools = row![
        kit::text_button(
            crate::t!("Select Objects").into_owned(),
            Some(msg(GeometryCleanupMsg::SelectObjects)),
            scan.is_none(),
        ),
        kit::text_button(
            crate::t!("Rescan").into_owned(),
            scan.map(|_| msg(GeometryCleanupMsg::Rescan)),
            false,
        ),
        kit::tool_button(
            kit::tool_icon(crate::ui::icons::GEAR),
            crate::t!("Geometry Cleanup Settings").into_owned(),
            Some(msg(GeometryCleanupMsg::OpenSettings)),
            false,
        ),
        Space::new().width(Fill),
    ]
    .spacing(km::TOOL_GAP)
    .align_y(iced::Center);

    let Some(scan) = scan else {
        return kit::palette(
            title_bar,
            column![
                kit::band(tools),
                kit::empty_state(crate::t!(
                    "Select objects to find gaps, overshoots, undershoots and lines off an angle."
                )),
            ],
            width,
        );
    };

    let mut list = column![];
    if scan.findings.is_empty() {
        list = list.push(kit::empty_state(crate::t!("No errors found.")));
    } else {
        list = list.push(kit::name_text(
            crate::tf!("Remaining Errors: {}", scan.findings.len()).into_owned(),
        ));
    }
    for (kind, indices) in groups(scan) {
        let folded = palette.folded.contains(&kind);
        // A length type shows its tolerance; an angle type has one for
        // each angle.
        let label = match kind.end_kind() {
            Some(end) => format!(
                "{}: {} ({})",
                crate::t!(kind.label()),
                indices.len(),
                scan.tolerances.get(end).unwrap_or_default()
            ),
            None => format!("{}: {}", crate::t!(kind.label()), indices.len()),
        };
        list = list.push(kit::category_bar(
            label,
            folded,
            msg(GeometryCleanupMsg::Fold(kind)),
        ));
        if folded {
            continue;
        }
        for index in indices {
            let finding = &scan.findings[index];
            list = list.push(
                kit::tree_row(place(finding))
                    .depth(1)
                    .selected(palette.picked == Some(index))
                    .trailing(fix_buttons(index, finding))
                    .on_press(msg(GeometryCleanupMsg::Pick(index)))
                    .view(),
            );
        }
    }

    if !scan.kept.is_empty() {
        list = list.push(kit::category_bar(
            crate::tf!("Kept as Designed: {}", scan.kept.len()).into_owned(),
            !palette.kept_open,
            msg(GeometryCleanupMsg::FoldKept),
        ));
        if palette.kept_open {
            list = list.push(
                row![
                    Space::new().width(Fill),
                    kit::text_button(
                        crate::t!("Restore All").into_owned(),
                        Some(msg(GeometryCleanupMsg::RestoreAll)),
                        false,
                    ),
                ]
                .align_y(iced::Center),
            );
            for (index, finding) in scan.kept.iter().enumerate() {
                list = list.push(
                    kit::tree_row(format!(
                        "{}: {}",
                        crate::t!(finding.kind.label()),
                        place(finding)
                    ))
                    .depth(1)
                    .trailing(row_button(
                        crate::t!("Restore").into_owned(),
                        crate::t!("List this error again.").into_owned(),
                        msg(GeometryCleanupMsg::Restore(index)),
                    ))
                    .view(),
                );
            }
        }
    }

    let body = column![
        kit::band(tools),
        container(kit::scroll_area(list)).height(Fill),
    ];
    kit::palette(title_bar, body, width)
}

/// Where an error is, for its row.
fn place(finding: &Finding) -> String {
    match finding.angle {
        Some(angle) => format!("{} ({}°)", coordinates(finding.location), angle.angle),
        None => coordinates(finding.location),
    }
}

/// A small button at the end of a row, with its tooltip.
fn row_button<'a>(label: String, tip: String, press: Message) -> Element<'a, Message> {
    let button = button(text(label).size(crate::ui::style::text::UI_SMALL))
        .padding(Padding::from([0, 4]))
        .height(km::ROW_H - 2.0)
        .style(kit::text_button_style(false))
        .on_press(press);
    kit::tooltip(button, tip, kit::TipPosition::Bottom)
}

/// `content`, which previews repair `fix` of error `index` while the
/// pointer is on it.
fn previews<'a>(content: Element<'a, Message>, index: usize, fix: Fix) -> Element<'a, Message> {
    mouse_area(content)
        .on_enter(msg(GeometryCleanupMsg::Preview(Some((index, fix)))))
        .on_exit(msg(GeometryCleanupMsg::Preview(None)))
        .into()
}

/// The toolbar over the drawing in review mode: Auto Zoom
/// (GEOMETRYCLEANUPZOOMPAN) and Close (GEOMETRYERRORREVIEWCLOSE).
pub fn review_toolbar<'a>(palette: &GeometryCleanupPalette) -> Element<'a, Message> {
    let bar_button = |icon: Element<'a, Message>, tip: String, press: Message| {
        kit::tooltip(
            button(icon)
                .style(button::text)
                .padding([3, 5])
                .on_press(press),
            tip,
            kit::TipPosition::Bottom,
        )
    };
    let zoom_tip = if palette.auto_zoom_off {
        crate::t!("Auto Zoom is off: a picked error is zoomed to only when it is not in view.")
    } else {
        crate::t!("Auto Zoom is on: the view zooms to each picked error.")
    };
    let zoom_icon = if palette.auto_zoom_off {
        crate::ui::icons::themed_disabled(crate::ui::icons::zoom_icon(), 16.0)
    } else {
        crate::ui::icons::themed_success(crate::ui::icons::zoom_icon(), 16.0)
    };
    let bar = row![
        text(crate::t!("Geometry Cleanup: Review").into_owned()).size(13),
        bar_button(
            zoom_icon,
            zoom_tip.into_owned(),
            msg(GeometryCleanupMsg::ToggleAutoZoom)
        ),
        bar_button(
            crate::ui::icons::themed_secondary(crate::ui::icons::CLOSE, 12.0),
            crate::t!("Close").into_owned(),
            msg(GeometryCleanupMsg::CloseReview)
        ),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let bar = container(bar)
        .padding([4, 10])
        .style(|theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(crate::ui::style::chrome::panel(
                theme,
            ))),
            border: iced::Border {
                color: crate::ui::style::chrome::border(theme),
                width: km::line(),
                radius: 4.0.into(),
            },
            ..Default::default()
        });
    container(iced::widget::opaque(bar))
        .width(Fill)
        .align_x(iced::Center)
        .padding([10, 0])
        .into()
}

/// The menu on the marker of error `index` in the drawing: its repairs,
/// each previewed while the pointer is on it, and Keep as Designed.
pub fn marker_menu<'a>(index: usize, finding: &Finding) -> Element<'a, Message> {
    let item = |label: String, press: Message| -> Element<'a, Message> {
        button(text(label).size(crate::ui::style::text::UI_SMALL))
            .padding(Padding::from([2, 8]))
            .width(Fill)
            .style(kit::text_button_style(false))
            .on_press(press)
            .into()
    };
    let mut items = column![].spacing(2);
    for fix in finding.fixes() {
        let press = msg(GeometryCleanupMsg::Fix(index, fix));
        items = items.push(kit::tooltip(
            previews(item(crate::t!(fix.label()).into_owned(), press), index, fix),
            crate::t!(fix.tip()).into_owned(),
            kit::TipPosition::Right,
        ));
    }
    items = items.push(item(
        crate::t!("Keep as Designed").into_owned(),
        msg(GeometryCleanupMsg::Keep(index)),
    ));
    let panel = container(items)
        .padding(4)
        .width(iced::Length::Fixed(160.0))
        .style(container::bordered_box);
    mouse_area(panel)
        .on_enter(msg(GeometryCleanupMsg::MenuHover(true)))
        .on_exit(msg(GeometryCleanupMsg::MenuHover(false)))
        .into()
}

/// The repairs of one error, and Keep, at the end of its row.
fn fix_buttons<'a>(index: usize, finding: &Finding) -> Element<'a, Message> {
    let mut fixes = row![].spacing(km::TOOL_GAP / 2.0).align_y(iced::Center);
    for fix in finding.fixes() {
        fixes = fixes.push(previews(
            row_button(
                crate::t!(fix.label()).into_owned(),
                crate::t!(fix.tip()).into_owned(),
                msg(GeometryCleanupMsg::Fix(index, fix)),
            ),
            index,
            fix,
        ));
    }
    fixes = fixes.push(row_button(
        crate::t!("Keep").into_owned(),
        crate::t!("Keep as designed: the drawing does not change, and later scans do not list this error.").into_owned(),
        msg(GeometryCleanupMsg::Keep(index)),
    ));
    fixes.into()
}
