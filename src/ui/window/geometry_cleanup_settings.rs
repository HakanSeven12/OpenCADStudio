//! The Geometry Cleanup Settings dialog: the error types to find, a
//! tolerance for each, and the `.gcs` files that hold them. The settings
//! are `modules::manage::geometry_cleanup_settings`; the handlers are in
//! `app::commands::geometry_cleanup`.

use crate::app::Message;
use crate::modules::manage::geometry_cleanup_settings::{
    clean_tolerance, kind_of, CleanupSettings, ToleranceRow, MIN_TOLERANCE,
};
use crate::t;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt};
use crate::ui::window::geometry_cleanup::{kind_label, GeometryCleanupMsg, KINDS};
use iced::widget::{button, column, pick_list, row, text, text_input, Space};
use iced::{Element, Fill, Length, Shrink};
use ocs_geom::geom2d::cleanup::EndErrorKind;

/// A tolerance as the dialog shows it: at most 6 decimals.
fn shown(tolerance: f64) -> String {
    ((tolerance * 1e6).round() / 1e6).to_string()
}

#[derive(Debug, Clone)]
pub struct SettingsDialog {
    /// The length types that are scanned, in the order of the rows, with
    /// the text of their tolerance boxes.
    pub rows: Vec<(EndErrorKind, String)>,
    /// The tolerance proposed for the drawing.
    pub proposed: f64,
    /// The last imported file, for display.
    pub file: Option<String>,
    /// The rows that are not length types (the angle types): kept and
    /// written back, not shown.
    pub hidden: Vec<ToleranceRow>,
}

impl SettingsDialog {
    pub fn new(settings: &CleanupSettings, proposed: f64, file: Option<String>) -> Self {
        let rows = KINDS
            .into_iter()
            .filter_map(|kind| Some((kind, shown(settings.tolerance(kind)?))))
            .collect();
        let hidden = settings
            .tolerances
            .iter()
            .filter(|row| kind_of(&row.name).is_none())
            .cloned()
            .collect();
        Self {
            rows,
            proposed,
            file,
            hidden,
        }
    }

    /// The settings of the dialog, or `None` while a box does not hold a
    /// number.
    pub fn parsed(&self) -> Option<CleanupSettings> {
        let mut settings = CleanupSettings {
            tolerances: self.hidden.clone(),
        };
        for (kind, value) in &self.rows {
            let tolerance = clean_tolerance(value.trim().parse::<f64>().ok()?)?;
            settings.set(*kind, Some(tolerance));
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
}

fn msg(m: GeometryCleanupMsg) -> Message {
    Message::GeometryCleanup(m)
}

/// A removed type in the "Add error type" list.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Addable(EndErrorKind);

impl std::fmt::Display for Addable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(kind_label(self.0)))
    }
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
                crate::ui::kit::tooltip(
                    button(text("✕").size(12))
                        .on_press(msg(GeometryCleanupMsg::SettingsRemove(kind)))
                        .style(button::text),
                    t!("Remove").into_owned(),
                    crate::ui::kit::TipPosition::Bottom,
                ),
            ]
            .spacing(8)
            .align_y(iced::Center),
        );
    }
    let removed: Vec<Addable> = dialog.removed().into_iter().map(Addable).collect();
    if !removed.is_empty() {
        types = types.push(
            pick_list(None::<Addable>, removed, |choice| choice.to_string())
                .on_select(|choice| msg(GeometryCleanupMsg::SettingsAdd(choice.0)))
                .placeholder(t!("Add error type").into_owned())
                .text_size(12),
        );
    }

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
