//! The Drawing Properties dialog (DWGPROPS): edits the drawing's summary
//! info as one undoable step.

use super::{Message, ModalKind, OpenCADStudio};
use crate::ui::window::drawing_properties::{DwgPropsMsg, State, SummaryField, Tab};
use codec::document::SummaryInfo;
use iced::Task;

/// Why a custom property table cannot be stored.
fn check_custom_names(properties: &[(String, String)]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for (index, (name, _)) in properties.iter().enumerate() {
        // Validated trimmed and case-folded, as AutoCAD does; stored as typed.
        let key = name.trim().to_lowercase();
        if key.is_empty() {
            return Err(crate::tr!(
                "dwgprops-dialog",
                "error-empty-name",
                row = (index + 1).to_string()
            ));
        }
        if !seen.insert(key) {
            return Err(crate::tr!(
                "dwgprops-dialog",
                "error-duplicate-name",
                name = name.trim()
            ));
        }
    }
    Ok(())
}

/// A Julian date in the OS long date and 24-hour time; 0 means never set.
fn format_julian(jd: f64) -> String {
    if jd <= 0.0 {
        return crate::tr!("dwgprops-dialog", "unavailable");
    }
    let locale = crate::entities::field::os_date_locale();
    let picture = format!("{}  HH:mm:ss", locale.long_date);
    codec::fields::format_dt_in(codec::fields::julian_parts(jd), &picture, &locale)
}

/// Editing time kept in days, shown as `D days H:MM:SS`.
fn format_editing_time(days: f64) -> String {
    let total = (days.max(0.0) * 86_400.0).round() as u64;
    let (d, h, m, s) = (
        total / 86_400,
        total / 3600 % 24,
        total / 60 % 60,
        total % 60,
    );
    let time = format!("{h}:{m:02}:{s:02}");
    if d > 0 {
        crate::tr!("dwgprops-dialog", "editing-days", days = d.to_string(), time = time)
    } else {
        time
    }
}

impl OpenCADStudio {
    fn general_rows(&self) -> Vec<(String, String)> {
        let tab = &self.tabs[self.active_tab];
        let doc = &tab.scene.document;
        let (name, location, size) = match &tab.current_path {
            Some(path) => (
                path.file_name().map(|n| n.to_string_lossy().into_owned()),
                path.parent().map(|p| p.display().to_string()),
                std::fs::metadata(path)
                    .ok()
                    .map(|m| {
                        crate::tr!("dwgprops-dialog", "size-bytes", bytes = m.len().to_string())
                    }),
            ),
            None => (None, None, None),
        };
        let unsaved = || crate::tr!("dwgprops-dialog", "unsaved");
        let unavailable = || crate::tr!("dwgprops-dialog", "unavailable");
        let or = |value: String, fallback: String| if value.is_empty() { fallback } else { value };
        vec![
            (
                crate::tr!("dwgprops-dialog", "file-name"),
                name.unwrap_or_else(unsaved),
            ),
            (
                crate::tr!("dwgprops-dialog", "location"),
                location.unwrap_or_else(unsaved),
            ),
            (
                crate::tr!("dwgprops-dialog", "size"),
                size.unwrap_or_else(unavailable),
            ),
            (
                crate::tr!("dwgprops-dialog", "created"),
                format_julian(doc.header.create_date_julian),
            ),
            (
                crate::tr!("dwgprops-dialog", "modified"),
                format_julian(doc.header.update_date_julian),
            ),
            (
                crate::tr!("dwgprops-dialog", "editing-time"),
                format_editing_time(doc.header.total_editing_time),
            ),
            (
                crate::tr!("dwgprops-dialog", "last-saved-by"),
                or(doc.summary_info.last_saved_by.clone(), unavailable()),
            ),
            (
                crate::tr!("dwgprops-dialog", "revision"),
                or(doc.summary_info.revision_number.clone(), unavailable()),
            ),
        ]
    }

    pub(in crate::app) fn open_drawing_properties(&mut self) {
        self.drawing_properties = Some(State {
            tab: Tab::General,
            summary: self.tabs[self.active_tab]
                .scene
                .document
                .summary_info
                .clone(),
            general: self.general_rows(),
            error: None,
        });
        self.active_modal = Some(ModalKind::DrawingProperties);
        self.reset_modal_geometry();
    }

    pub(in crate::app) fn on_drawing_properties(&mut self, msg: DwgPropsMsg) -> Task<Message> {
        let Some(state) = self.drawing_properties.as_mut() else {
            return Task::none();
        };
        match msg {
            DwgPropsMsg::Tab(tab) => state.tab = tab,
            DwgPropsMsg::Summary(field, value) => {
                let summary = &mut state.summary;
                match field {
                    SummaryField::Title => summary.title = value,
                    SummaryField::Subject => summary.subject = value,
                    SummaryField::Author => summary.author = value,
                    SummaryField::Keywords => summary.keywords = value,
                    SummaryField::Comments => summary.comments = value,
                    SummaryField::HyperlinkBase => summary.hyperlink_base = value,
                }
            }
            DwgPropsMsg::CustomName(row, value) => {
                if let Some(entry) = state.summary.custom_properties.get_mut(row) {
                    entry.0 = value;
                }
            }
            DwgPropsMsg::CustomValue(row, value) => {
                if let Some(entry) = state.summary.custom_properties.get_mut(row) {
                    entry.1 = value;
                }
            }
            DwgPropsMsg::AddRow => state
                .summary
                .custom_properties
                .push((String::new(), String::new())),
            DwgPropsMsg::RemoveRow(row) => {
                if row < state.summary.custom_properties.len() {
                    state.summary.custom_properties.remove(row);
                }
            }
            DwgPropsMsg::Ok => return self.apply_drawing_properties(),
        }
        Task::none()
    }

    /// OK: store the edited summary as one undo step, or say why not.
    fn apply_drawing_properties(&mut self) -> Task<Message> {
        let Some(state) = self.drawing_properties.as_mut() else {
            return Task::none();
        };
        if let Err(message) = check_custom_names(&state.summary.custom_properties) {
            state.error = Some(message);
            return Task::none();
        }
        let edited: SummaryInfo = state.summary.clone();
        self.drawing_properties = None;
        self.active_modal = None;
        let i = self.active_tab;
        if self.tabs[i].scene.document.summary_info == edited {
            return Task::none();
        }
        self.push_undo_snapshot(i, "DWGPROPS");
        self.tabs[i].scene.document.summary_info = edited;
        self.tabs[i].dirty = true;
        // Fields showing Title, Author or a custom property read it again.
        self.tabs[i].scene.bump_geometry();
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ModalKind;
    use crate::ui::window::drawing_properties::{SummaryField, Tab};

    fn app_with_summary() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let info = &mut app.tabs[i].scene.document.summary_info;
        info.title = "Plan".into();
        info.author = "Ana".into();
        info.custom_properties = vec![
            ("Cost".into(), "12,5".into()),
            ("cOde".into(), " A-1 ".into()),
        ];
        app
    }

    fn undo_len(app: &mut OpenCADStudio) -> usize {
        app.finish_all_pending_history();
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    fn state(app: &OpenCADStudio) -> &crate::ui::window::drawing_properties::State {
        app.drawing_properties.as_ref().expect("dialog state")
    }

    #[test]
    fn dwgprops_opens_the_dialog_loaded_from_the_summary() {
        let mut app = app_with_summary();
        let _ = app.dispatch_command("DWGPROPS");
        assert!(matches!(
            app.active_modal,
            Some(ModalKind::DrawingProperties)
        ));
        let s = &state(&app).summary;
        assert_eq!(s.title, "Plan");
        assert_eq!(s.author, "Ana");
        assert_eq!(
            s.custom_properties,
            vec![
                ("Cost".to_string(), "12,5".to_string()),
                ("cOde".into(), " A-1 ".into())
            ]
        );
    }

    #[test]
    fn cancel_leaves_the_document_untouched() {
        let mut app = app_with_summary();
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.summary_info.clone();
        let (dirty, undo) = (app.tabs[i].dirty, undo_len(&mut app));
        let _ = app.dispatch_command("DWGPROPS");
        let _ = app.update(Message::DwgProps(DwgPropsMsg::Summary(
            SummaryField::Title,
            "X".into(),
        )));
        let _ = app.update(Message::DwgProps(DwgPropsMsg::AddRow));
        let _ = app.update(Message::CloseModal);
        assert!(app.active_modal.is_none());
        assert_eq!(app.tabs[i].scene.document.summary_info, before);
        assert_eq!(app.tabs[i].dirty, dirty);
        assert_eq!(undo_len(&mut app), undo);
    }

    #[test]
    fn ok_with_changes_is_one_undo_step() {
        let mut app = app_with_summary();
        let i = app.active_tab;
        let before = app.tabs[i].scene.document.summary_info.clone();
        app.tabs[i].dirty = false;
        let undo = undo_len(&mut app);
        let _ = app.dispatch_command("DWGPROPS");
        let _ = app.update(Message::DwgProps(DwgPropsMsg::Tab(Tab::Summary)));
        let _ = app.update(Message::DwgProps(DwgPropsMsg::Summary(
            SummaryField::Title,
            "New".into(),
        )));
        let _ = app.update(Message::DwgProps(DwgPropsMsg::CustomValue(0, "9".into())));
        let _ = app.update(Message::DwgProps(DwgPropsMsg::AddRow));
        let _ = app.update(Message::DwgProps(DwgPropsMsg::CustomName(
            2,
            "Extra".into(),
        )));
        let _ = app.update(Message::DwgProps(DwgPropsMsg::Ok));
        assert!(app.active_modal.is_none());
        let info = &app.tabs[i].scene.document.summary_info;
        assert_eq!(info.title, "New");
        assert_eq!(
            info.custom_properties[0],
            ("Cost".to_string(), "9".to_string())
        );
        assert_eq!(
            info.custom_properties[2],
            ("Extra".to_string(), String::new())
        );
        assert!(app.tabs[i].dirty);
        assert_eq!(undo_len(&mut app), undo + 1);
        let _ = app.automation_op(r#"{"op":"undo"}"#);
        assert_eq!(app.tabs[i].scene.document.summary_info, before);
    }

    #[test]
    fn ok_without_changes_keeps_imported_values_and_history() {
        let mut app = app_with_summary();
        let i = app.active_tab;
        app.tabs[i].dirty = false;
        let before = app.tabs[i].scene.document.summary_info.clone();
        let undo = undo_len(&mut app);
        let _ = app.dispatch_command("DWGPROPS");
        let _ = app.update(Message::DwgProps(DwgPropsMsg::Ok));
        assert!(app.active_modal.is_none());
        assert_eq!(app.tabs[i].scene.document.summary_info, before);
        assert!(!app.tabs[i].dirty);
        assert_eq!(undo_len(&mut app), undo);
    }

    #[test]
    fn invalid_names_keep_the_dialog_open_and_the_document_unchanged() {
        for (a, b) in [("", "x"), ("  ", "x"), ("Cost", "COST")] {
            let mut app = app_with_summary();
            let i = app.active_tab;
            app.tabs[i].dirty = false;
            let before = app.tabs[i].scene.document.summary_info.clone();
            let undo = undo_len(&mut app);
            let _ = app.dispatch_command("DWGPROPS");
            let _ = app.update(Message::DwgProps(DwgPropsMsg::Summary(
                SummaryField::Title,
                "Changed".into(),
            )));
            let _ = app.update(Message::DwgProps(DwgPropsMsg::CustomName(0, a.into())));
            let _ = app.update(Message::DwgProps(DwgPropsMsg::CustomName(1, b.into())));
            let _ = app.update(Message::DwgProps(DwgPropsMsg::Ok));
            assert!(
                matches!(app.active_modal, Some(ModalKind::DrawingProperties)),
                "{a:?}/{b:?}"
            );
            assert!(
                state(&app).error.as_deref().is_some_and(|e| !e.is_empty()),
                "{a:?}/{b:?}"
            );
            assert_eq!(app.tabs[i].scene.document.summary_info, before);
            assert!(!app.tabs[i].dirty);
            assert_eq!(undo_len(&mut app), undo);
        }
    }
}
