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
use iced::widget::{button, column, container, row, text, Space};
use iced::{Element, Fill, Padding};
use ocs_geom::geom2d::cleanup::{EndErrorKind, Tolerances};

pub const ICON: &[u8] = include_bytes!("../../../assets/icons/geometry_cleanup.svg");

/// The error types, in the order of the tree.
pub const KINDS: [EndErrorKind; 3] = [
    EndErrorKind::Gap,
    EndErrorKind::Overshoot,
    EndErrorKind::Undershoot,
];

pub fn kind_label(kind: EndErrorKind) -> &'static str {
    match kind {
        EndErrorKind::Gap => "Gap",
        EndErrorKind::Overshoot => "Overshoot",
        EndErrorKind::Undershoot => "Undershoot",
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
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Fix::Trim => "Trim",
            Fix::Extend | Fix::ExtendBoth => "Extend",
            Fix::Stretch => "Stretch",
            Fix::Move => "Move",
        }
    }
}

/// One error of a scan, with the objects in place of curve indices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finding {
    pub kind: EndErrorKind,
    pub handle: Handle,
    pub at_start: bool,
    pub location: [f64; 2],
    pub target: [f64; 2],
    /// A gap's other end.
    pub other: Option<(Handle, bool)>,
    /// A gap's corner, for Extend.
    pub corner: Option<[f64; 2]>,
    pub deviation: f64,
}

impl Finding {
    /// Whether `other` is the same error: the same type at the same end.
    pub fn same_error(&self, other: &Finding) -> bool {
        self.kind == other.kind && self.handle == other.handle && self.at_start == other.at_start
    }

    /// The repairs that this error offers.
    pub fn fixes(&self) -> Vec<Fix> {
        match self.kind {
            EndErrorKind::Overshoot => vec![Fix::Trim],
            EndErrorKind::Undershoot => vec![Fix::Extend],
            EndErrorKind::Gap => {
                let mut fixes = Vec::new();
                if self.corner.is_some() {
                    fixes.push(Fix::ExtendBoth);
                }
                fixes.push(Fix::Stretch);
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
    pub tolerances: Tolerances,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Default)]
pub struct GeometryCleanupPalette {
    pub show: bool,
    pub scan: Option<Scan>,
    /// The index in the scan's findings of the picked error.
    pub picked: Option<usize>,
    /// The error types whose list is folded.
    pub folded: Vec<EndErrorKind>,
    /// The saved settings; `None` until the user saves them.
    pub settings: Option<CleanupSettings>,
    /// The file that the saved settings were imported from, for display.
    pub settings_file: Option<String>,
    /// The open Settings dialog.
    pub dialog: Option<crate::ui::window::geometry_cleanup_settings::SettingsDialog>,
}

#[derive(Debug, Clone)]
pub enum GeometryCleanupMsg {
    SelectObjects,
    Rescan,
    /// Pick an error: zoom to it and show its repairs.
    Pick(usize),
    Fix(usize, Fix),
    Fold(EndErrorKind),
    /// The Settings dialog.
    OpenSettings,
    SettingsTolerance(EndErrorKind, String),
    /// Put the proposed tolerance in the type's box.
    SettingsUseProposed(EndErrorKind),
    SettingsRemove(EndErrorKind),
    SettingsAdd(EndErrorKind),
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
pub fn groups(scan: &Scan) -> Vec<(EndErrorKind, Vec<usize>)> {
    KINDS
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
                    "Select objects to find gaps, overshoots and undershoots."
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
        list = list.push(kit::category_bar(
            format!(
                "{}: {} ({})",
                crate::t!(kind_label(kind)),
                indices.len(),
                scan.tolerances.get(kind).unwrap_or_default()
            ),
            folded,
            msg(GeometryCleanupMsg::Fold(kind)),
        ));
        if folded {
            continue;
        }
        for index in indices {
            let finding = &scan.findings[index];
            list = list.push(
                kit::tree_row(coordinates(finding.location))
                    .depth(1)
                    .selected(palette.picked == Some(index))
                    .trailing(fix_buttons(index, finding))
                    .on_press(msg(GeometryCleanupMsg::Pick(index)))
                    .view(),
            );
        }
    }

    let body = column![
        kit::band(tools),
        container(kit::scroll_area(list)).height(Fill),
    ];
    kit::palette(title_bar, body, width)
}

/// The repairs of one error, at the end of its row.
fn fix_buttons<'a>(index: usize, finding: &Finding) -> Element<'a, Message> {
    let mut fixes = row![].spacing(km::TOOL_GAP / 2.0).align_y(iced::Center);
    for fix in finding.fixes() {
        let press = button(
            text(crate::t!(fix.label()).into_owned()).size(crate::ui::style::text::UI_SMALL),
        )
        .padding(Padding::from([0, 4]))
        .height(km::ROW_H - 2.0)
        .style(kit::text_button_style(false))
        .on_press(msg(GeometryCleanupMsg::Fix(index, fix)));
        fixes = fixes.push(kit::tooltip(
            press,
            crate::t!(fix.tip()).into_owned(),
            kit::TipPosition::Bottom,
        ));
    }
    fixes.into()
}
