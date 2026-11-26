//! The Geometry Cleanup Settings dialog: the error types to find, a
//! tolerance for each, and the `.gcs` files that hold them. The settings
//! are `modules::manage::geometry_cleanup_settings`; the handlers are in
//! `app::commands::geometry_cleanup`.

use crate::app::Message;
use crate::modules::manage::geometry_cleanup_settings::{
    angle_kind_name, angle_kind_of, clean_tolerance, CleanupSettings, ToleranceRow,
    DEFAULT_ANGLE_TOLERANCE, MIN_TOLERANCE,
};
use crate::t;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt};
use crate::ui::window::geometry_cleanup::{kind_label, ErrorType, GeometryCleanupMsg, KINDS};
use iced::widget::{button, column, pick_list, row, text, text_input, Space};
use iced::{Element, Fill, Length, Shrink};
use ocs_geom::geom2d::cleanup::{AngleErrorKind, EndErrorKind};

/// A tolerance as the dialog shows it: at most 6 decimals.
fn shown(tolerance: f64) -> String {
    ((tolerance * 1e6).round() / 1e6).to_string()
}

/// An angle row of the dialog: the standard angle and its tolerance, both
/// in degrees, as typed.
#[derive(Debug, Clone, PartialEq)]
pub struct AngleRow {
    pub kind: AngleErrorKind,
    pub angle: String,
    pub tolerance: String,
    /// The fields of the row that OCS does not know.
    extra: serde_json::Map<String, serde_json::Value>,
}

impl AngleRow {
    /// The row as the settings keep it, or `None` while a box does not
    /// hold a value it can take: an angle from 0° up to 180° (not 180°
    /// itself), a tolerance that is a number of 0 or more.
    fn parsed(&self) -> Option<ToleranceRow> {
        let angle = self
            .angle
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|angle| (0.0..180.0).contains(angle))?;
        let tolerance = self
            .tolerance
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|tolerance| *tolerance >= 0.0)?;
        Some(ToleranceRow {
            name: angle_kind_name(self.kind).to_string(),
            sub_name: shown(angle),
            tolerance: clean_tolerance(tolerance)?,
            extra: self.extra.clone(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SettingsDialog {
    /// The length types that are scanned, in the order of the rows, with
    /// the text of their tolerance boxes.
    pub rows: Vec<(EndErrorKind, String)>,
    /// The angle rows, in the order of the settings.
    pub angles: Vec<AngleRow>,
    /// The tolerance proposed for the drawing.
    pub proposed: f64,
    /// The last imported file, for display.
    pub file: Option<String>,
    /// The settings that the dialog opened with. The rows of types that OCS
    /// does not know and the fields that it does not know are written back
    /// from them.
    pub opened: CleanupSettings,
}

impl SettingsDialog {
    pub fn new(settings: &CleanupSettings, proposed: f64, file: Option<String>) -> Self {
        let rows = KINDS
            .into_iter()
            .filter_map(|kind| Some((kind, shown(settings.tolerance(kind)?))))
            .collect();
        let angles = settings
            .tolerances
            .iter()
            .filter_map(|row| {
                Some(AngleRow {
                    kind: angle_kind_of(&row.name)?,
                    angle: row.sub_name.trim().to_string(),
                    tolerance: shown(row.tolerance),
                    extra: row.extra.clone(),
                })
            })
            .collect();
        Self {
            rows,
            angles,
            proposed,
            file,
            opened: settings.clone(),
        }
    }

    /// The settings of the dialog, or `None` while a box does not hold a
    /// number.
    pub fn parsed(&self) -> Option<CleanupSettings> {
        let mut settings = self.opened.clone();
        for kind in self.removed() {
            settings.set(kind, None);
        }
        for (kind, value) in &self.rows {
            let tolerance = clean_tolerance(value.trim().parse::<f64>().ok()?)?;
            settings.set(*kind, Some(tolerance));
        }
        settings
            .tolerances
            .retain(|row| angle_kind_of(&row.name).is_none());
        for row in &self.angles {
            settings.tolerances.push(row.parsed()?);
        }
        Some(settings)
    }

    /// The length types that are not scanned.
    pub fn removed(&self) -> Vec<EndErrorKind> {
        KINDS
            .into_iter()
            .filter(|kind| !self.rows.iter().any(|(shown, _)| shown == kind))
            .collect()
    }

    pub fn set_text(&mut self, kind: EndErrorKind, value: String) {
        if let Some(row) = self.rows.iter_mut().find(|(shown, _)| *shown == kind) {
            row.1 = value;
        }
    }

    pub fn use_proposed(&mut self, kind: EndErrorKind) {
        self.set_text(kind, shown(self.proposed));
    }

    pub fn remove(&mut self, kind: EndErrorKind) {
        self.rows.retain(|(shown, _)| *shown != kind);
    }

    /// Add a type back, with the proposed tolerance, in the order of the
    /// types.
    pub fn add(&mut self, kind: EndErrorKind) {
        if self.rows.iter().any(|(shown, _)| *shown == kind) {
            return;
        }
        let at = self
            .rows
            .iter()
            .position(|(shown, _)| *shown > kind)
            .unwrap_or(self.rows.len());
        self.rows.insert(at, (kind, shown(self.proposed)));
    }

    /// Add an angle row of `kind` after the others, with no angle yet and
    /// the default tolerance.
    pub fn add_angle(&mut self, kind: AngleErrorKind) {
        self.angles.push(AngleRow {
            kind,
            angle: String::new(),
            tolerance: shown(DEFAULT_ANGLE_TOLERANCE),
            extra: Default::default(),
        });
    }

    pub fn set_angle(&mut self, index: usize, value: String) {
        if let Some(row) = self.angles.get_mut(index) {
            row.angle = value;
        }
    }

    pub fn set_angle_tolerance(&mut self, index: usize, value: String) {
        if let Some(row) = self.angles.get_mut(index) {
            row.tolerance = value;
        }
    }

    pub fn remove_angle(&mut self, index: usize) {
        if index < self.angles.len() {
            self.angles.remove(index);
        }
    }
}

fn msg(m: GeometryCleanupMsg) -> Message {
    Message::GeometryCleanup(m)
}

/// A type in the "Add error type" list: a removed length type, or an angle
/// type (it can have any number of rows).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Addable {
    Length(EndErrorKind),
    Angle(AngleErrorKind),
}

impl Addable {
    fn message(self) -> Message {
        msg(match self {
            Addable::Length(kind) => GeometryCleanupMsg::SettingsAdd(kind),
            Addable::Angle(kind) => GeometryCleanupMsg::SettingsAddAngle(kind),
        })
    }
}

impl std::fmt::Display for Addable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match *self {
            Addable::Length(kind) => kind_label(kind),
            Addable::Angle(kind) => ErrorType::from(kind).label(),
        };
        f.write_str(&t!(label))
    }
}

/// The ✕ that removes a row.
fn remove_button<'a>(m: GeometryCleanupMsg) -> Element<'a, Message> {
    crate::ui::kit::tooltip(
        button(text("✕").size(12))
            .on_press(msg(m))
            .style(button::text),
        t!("Remove").into_owned(),
        crate::ui::kit::TipPosition::Bottom,
    )
}

pub fn view_window(
    dialog: &SettingsDialog,
    rescan: bool,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'_, Message> {
    let field_width = if matches!(sizing.width, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let file = dialog
        .file
        .clone()
        .unwrap_or_else(|| t!("(default)").into_owned());
    let import = row![
        text_input("", &file).padding([6, 8]).size(12).width(260),
        dialog_button(t!("Browse"), msg(GeometryCleanupMsg::SettingsImport), false),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let header = row![
        text(t!("Error type")).size(12).width(110),
        text(t!("Tolerance")).size(12).width(110),
        text(t!("Proposed tolerance")).size(12).width(130),
    ]
    .spacing(8);
    let mut types = column![header].spacing(6);
    for (kind, value) in &dialog.rows {
        let kind = *kind;
        types = types.push(
            row![
                text(t!(kind_label(kind))).size(12).width(110),
                text_input(&shown(MIN_TOLERANCE), value)
                    .on_input(move |value| msg(GeometryCleanupMsg::SettingsTolerance(kind, value)))
                    .padding([4, 8])
                    .size(12)
                    .width(110),
                button(text(shown(dialog.proposed)).size(12))
                    .on_press(msg(GeometryCleanupMsg::SettingsUseProposed(kind)))
                    .style(button::text)
                    .width(130),
                remove_button(GeometryCleanupMsg::SettingsRemove(kind)),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
    }
    if !dialog.angles.is_empty() {
        types = types.push(
            row![
                text(t!("Error type")).size(12).width(110),
                text(t!("Angle")).size(12).width(110),
                text(t!("Tolerance")).size(12).width(130),
            ]
            .spacing(8),
        );
    }
    for (index, angle) in dialog.angles.iter().enumerate() {
        types = types.push(
            row![
                text(t!(ErrorType::from(angle.kind).label()))
                    .size(12)
                    .width(110),
                text_input("0", &angle.angle)
                    .on_input(move |value| msg(GeometryCleanupMsg::SettingsAngle(index, value)))
                    .padding([4, 8])
                    .size(12)
                    .width(110),
                text_input(&shown(MIN_TOLERANCE), &angle.tolerance)
                    .on_input(move |value| {
                        msg(GeometryCleanupMsg::SettingsAngleTolerance(index, value))
                    })
                    .padding([4, 8])
                    .size(12)
                    .width(130),
                remove_button(GeometryCleanupMsg::SettingsRemoveAngle(index)),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
    }
    let addable: Vec<Addable> = dialog
        .removed()
        .into_iter()
        .map(Addable::Length)
        .chain([AngleErrorKind::Absolute, AngleErrorKind::Relative].map(Addable::Angle))
        .collect();
    types = types.push(
        pick_list(None::<Addable>, addable, |choice| choice.to_string())
            .on_select(Addable::message)
            .placeholder(t!("Add error type").into_owned())
            .text_size(12),
    );

    let ready = dialog.parsed().is_some();
    column![
        text(t!("Import settings")).size(13),
        import,
        text(t!("Select error types and tolerances")).size(13),
        types,
        row![
            Space::new().width(field_width),
            dialog_button(t!("Cancel"), msg(GeometryCleanupMsg::SettingsCancel), false),
            dialog_button_styled_opt(
                t!("Export"),
                ready.then_some(msg(GeometryCleanupMsg::SettingsExport)),
                button::secondary,
            ),
            dialog_button_styled_opt(
                if rescan { t!("Rescan") } else { t!("Scan") },
                ready.then_some(msg(GeometryCleanupMsg::SettingsScan)),
                button::primary,
            ),
        ]
        .spacing(8)
        .align_y(iced::Center),
    ]
    .spacing(10)
    .padding(12)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
