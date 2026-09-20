//! The Options window's OK / Apply / Close. Settings still change the moment
//! a control is touched, so a theme, a cursor size or a language is seen at
//! once — but nothing is committed until Apply or OK. Close (and the window's
//! × or Esc) puts everything back the way it was when the window opened, or
//! when Apply was last pressed, asking first when something would be lost.
//!
//! The commit point is a snapshot of the persisted configuration plus the
//! few drawing variables the window edits (the 3D Modeling page writes into
//! the active drawing's header); "dirty" is simply "the current snapshot
//! differs from the saved one", so every control the window gains later is
//! covered without registering it here.

use super::OpenCADStudio;

/// The drawing-header variables the Options window edits, for the active
/// tab, with the tab's modified flag so a revert leaves it as it found it.
#[derive(Clone, PartialEq)]
struct HeaderVars {
    isolines: i16,
    display_silhouette: bool,
    surface_u_density: i16,
    surface_v_density: i16,
    surface_type: i16,
    record_solid_history: bool,
    show_solid_history: i16,
    tab_dirty: bool,
}

/// What the Options window can change, as it stood at the last commit.
#[derive(Clone, PartialEq)]
pub(super) struct OptionsSnapshot {
    config: super::config::AppConfig,
    header: Option<HeaderVars>,
    textfill: bool,
}

impl OpenCADStudio {
    fn options_snapshot(&self) -> OptionsSnapshot {
        let header = self.tabs.get(self.active_tab).map(|tab| {
            let h = &tab.scene.document.header;
            HeaderVars {
                isolines: h.isolines,
                display_silhouette: h.display_silhouette,
                surface_u_density: h.surface_u_density,
                surface_v_density: h.surface_v_density,
                surface_type: h.surface_type,
                record_solid_history: h.record_solid_history,
                show_solid_history: h.show_solid_history,
                tab_dirty: tab.dirty,
            }
        });
        OptionsSnapshot {
            config: self.current_config(),
            header,
            textfill: crate::scene::text::sdf_atlas::textfill(),
        }
    }

    /// The window opens: remember the state to come back to.
    pub(super) fn options_open(&mut self) {
        self.active_modal = Some(super::ModalKind::Options);
        self.options_saved = Some(self.options_snapshot());
        self.options_close_confirm = false;
    }

    /// Whether anything changed since the window opened or Apply was pressed.
    pub(super) fn options_dirty(&self) -> bool {
        match &self.options_saved {
            Some(saved) => *saved != self.options_snapshot(),
            None => false,
        }
    }

    /// Apply: the current state becomes the state to come back to, and is
    /// written to disk.
    pub(super) fn options_apply(&mut self) {
        self.options_saved = Some(self.options_snapshot());
        self.options_close_confirm = false;
        self.save_config();
    }

    /// Close: put everything back to the last commit. The caller closes the
    /// window afterwards.
    pub(super) fn options_discard(&mut self) {
        self.options_close_confirm = false;
        if let Some(saved) = self.options_saved.take() {
            self.options_revert_to(saved);
        }
    }

    /// Forget the commit point without reverting (OK, or a close that had
    /// nothing to lose).
    pub(super) fn options_forget(&mut self) {
        self.options_saved = None;
        self.options_close_confirm = false;
    }

    fn options_revert_to(&mut self, saved: OptionsSnapshot) {
        let current = self.options_snapshot();
        if current == saved {
            return;
        }
        if current.config != saved.config {
            // `apply_config` distributes every persisted preference, the
            // theme and the language the way start-up does.
            self.apply_config(saved.config.clone());
        }
        if current.textfill != saved.textfill {
            // The glyph atlas was re-baked for the other setting.
            self.invalidate_text_everywhere();
        }
        if let (Some(now), Some(then)) = (&current.header, &saved.header) {
            if now != then {
                let i = self.active_tab;
                if let Some(tab) = self.tabs.get_mut(i) {
                    let h = &mut tab.scene.document.header;
                    h.isolines = then.isolines;
                    h.display_silhouette = then.display_silhouette;
                    h.surface_u_density = then.surface_u_density;
                    h.surface_v_density = then.surface_v_density;
                    h.surface_type = then.surface_type;
                    h.record_solid_history = then.record_solid_history;
                    h.show_solid_history = then.show_solid_history;
                    tab.dirty = then.tab_dirty;
                    if now.show_solid_history != then.show_solid_history
                        || now.display_silhouette != then.display_silhouette
                    {
                        tab.scene.bump_geometry();
                    }
                }
                if now.isolines != then.isolines {
                    self.isolines_awaiting_regen = false;
                    self.regenerate_meshes();
                }
            }
        }
        self.save_config();
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn changes_show_at_once_but_close_puts_them_back() {
        let mut app = app();
        let _ = app.update(Message::OptionsOpen);
        assert!(!app.options_dirty());
        let before = app.zoom_factor;
        let _ = app.update(Message::ZoomFactorChanged(before + 7));
        assert_eq!(app.zoom_factor, before + 7, "the change is live");
        assert!(app.options_dirty());
        // The first Close asks; the discard puts the value back.
        let _ = app.update(Message::OptionsClose);
        assert!(app.options_close_confirm);
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::Options));
        let _ = app.update(Message::OptionsCloseDiscard);
        assert_eq!(app.zoom_factor, before);
        assert_eq!(app.active_modal, None);
        assert!(!app.options_dirty());
    }

    #[test]
    fn apply_makes_the_change_the_new_baseline() {
        let mut app = app();
        let _ = app.update(Message::OptionsOpen);
        let before = app.zoom_factor;
        let _ = app.update(Message::ZoomFactorChanged(before + 7));
        let _ = app.update(Message::OptionsApply);
        assert!(!app.options_dirty(), "Apply commits");
        let _ = app.update(Message::ZoomFactorChanged(before + 9));
        let _ = app.update(Message::OptionsClose);
        let _ = app.update(Message::OptionsCloseDiscard);
        assert_eq!(app.zoom_factor, before + 7, "back to the applied value, not the original");
    }

    #[test]
    fn ok_commits_and_closes_and_a_clean_close_asks_nothing() {
        let mut app = app();
        let _ = app.update(Message::OptionsOpen);
        let before = app.zoom_factor;
        let _ = app.update(Message::ZoomFactorChanged(before + 7));
        let _ = app.update(Message::OptionsOk);
        assert_eq!(app.active_modal, None);
        assert_eq!(app.zoom_factor, before + 7);
        let _ = app.update(Message::OptionsOpen);
        let _ = app.update(Message::OptionsClose);
        assert_eq!(app.active_modal, None, "nothing to lose, nothing to ask");
        assert!(!app.options_close_confirm);
        // Keep editing returns to the window with the change intact.
        let _ = app.update(Message::OptionsOpen);
        let _ = app.update(Message::ZoomFactorChanged(before + 11));
        let _ = app.update(Message::CloseModal);
        assert!(app.options_close_confirm, "the × asks like Close does");
        let _ = app.update(Message::OptionsCloseKeep);
        assert_eq!(app.active_modal, Some(crate::app::ModalKind::Options));
        assert_eq!(app.zoom_factor, before + 11);
    }

    #[test]
    fn drawing_variables_of_the_modeling_page_are_reverted_too() {
        let mut app = app();
        let i = app.active_tab;
        let isolines = app.tabs[i].scene.document.header.isolines;
        let silhouette = app.tabs[i].scene.document.header.display_silhouette;
        let _ = app.update(Message::OptionsOpen);
        let _ = app.update(Message::IsolinesChanged(isolines + 4));
        let _ = app.update(Message::DispSilhChanged(!silhouette));
        assert!(app.options_dirty());
        assert!(app.tabs[i].dirty);
        let _ = app.update(Message::OptionsClose);
        let _ = app.update(Message::OptionsCloseDiscard);
        let h = &app.tabs[i].scene.document.header;
        assert_eq!(h.isolines, isolines);
        assert_eq!(h.display_silhouette, silhouette);
        assert!(!app.tabs[i].dirty, "the drawing is as unmodified as it was");
    }
}
