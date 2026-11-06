//! UCSFOLLOW is a drawing/viewport variable, not an application preference.
use crate::app::{Message, OpenCADStudio};
use iced::Task;

inventory::submit!(crate::command::CommandRegistration {
    names: &["UCSFOLLOW"]
});

impl OpenCADStudio {
    pub(super) fn dispatch_ucsfollow(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").unwrap_or(cmd).trim();
        let mut parts = rest.splitn(2, char::is_whitespace);
        if parts.next()? != "UCSFOLLOW" {
            return None;
        }
        let value = parts.next().map(str::trim).filter(|v| !v.is_empty());
        let current = self.tabs[i].scene.ucs_follow();
        match value {
            None => {
                self.command_line.push_output(
                    crate::tf!(
                        "UCSFOLLOW: enter 0 (keep view) or 1 (follow UCS) <{}>:",
                        u8::from(current)
                    )
                    .as_ref(),
                );
                self.pending_setvar = Some("UCSFOLLOW".into());
            }
            Some("0" | "1") => {
                let next = value == Some("1");
                if next
                    && self.tabs[i].scene.current_layout != "Model"
                    && self.tabs[i].scene.active_viewport.is_none()
                {
                    self.command_line.push_error(
                        crate::t!(
                            "UCSFOLLOW: activate a model viewport; paper space cannot follow the UCS."
                        )
                        .as_ref(),
                    );
                } else {
                    if current != next {
                        self.push_undo_snapshot(i, "UCSFOLLOW");
                        self.tabs[i].scene.set_ucs_follow(next);
                        self.tabs[i].dirty = true;
                    }
                    // Enabling UCSFOLLOW must align an already-defined UCS
                    // immediately, not only follow subsequent UCS edits. Also
                    // realign when the value was already 1 (e.g. after orbit).
                    if next && self.tabs[i].active_block_edit.is_none() {
                        if current == next {
                            self.push_undo_snapshot(i, "UCSFOLLOW");
                        }
                        self.tabs[i].sync_ucs_to_scene();
                        let rotation = self.tabs[i].scene.viewcube_ucs_mat();
                        self.plan_snap(i, rotation);
                        self.tabs[i].dirty = true;
                    }
                    self.command_line
                        .push_output(&format!("UCSFOLLOW = {}", u8::from(next)));
                }
            }
            Some(_) => {
                self.command_line
                    .push_rejection(crate::t!("UCSFOLLOW: only 0 or 1 is allowed.").as_ref());
            }
        }
        Some(Task::none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn variable_is_registered_prompted_and_validated() {
        let mut app = app();
        let i = app.active_tab;
        assert!(!app.tabs[i].scene.ucs_follow());
        let _ = app.run_command_line("UCSFOLLOW");
        assert_eq!(app.pending_setvar.as_deref(), Some("UCSFOLLOW"));
        app.pending_setvar = None;
        let _ = app.run_command_line("SETVAR UCSFOLLOW 1");
        assert!(app.tabs[i].scene.ucs_follow());
        let _ = app.run_command_line("UCSFOLLOW 2");
        assert!(app.tabs[i].scene.ucs_follow());
        let _ = app.run_command_line("UCSFOLLOW 0");
        assert!(!app.tabs[i].scene.ucs_follow());
        assert!(crate::command::all_registered_command_names().contains(&"UCSFOLLOW"));
    }

    #[test]
    fn ucs_changes_align_view_only_when_enabled_and_aliases_work() {
        let mut app = app();
        let i = app.active_tab;
        // Test-only table: never writes or imports into the user's profile.
        app.command_aliases.insert("UF".into(), "UCSFOLLOW".into());
        app.command_aliases.insert("SQ".into(), "UCS".into());
        let before = app.tabs[i].scene.camera.borrow().clone();
        let _ = app.run_command_line("SQ Z 30");
        let unchanged = app.tabs[i].scene.camera.borrow().clone();
        assert_eq!(before.rotation, unchanged.rotation);
        let _ = app.run_command_line("UF 1");
        assert!(app.tabs[i].scene.ucs_follow());
        let _ = app.run_command_line("SQ Z 30");
        let followed = app.tabs[i].scene.camera.borrow().clone();
        assert_ne!(unchanged.rotation, followed.rotation);
        let _ = app.run_command_line("UF 0");
        let _ = app.run_command_line("SQ Z 30");
        assert_eq!(
            followed.rotation,
            app.tabs[i].scene.camera.borrow().rotation
        );
    }

    #[test]
    fn floating_viewport_setting_does_not_leak_into_model_or_paper() {
        let mut app = app();
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Viewport(Box::new(document::entities::Viewport::new())));
        let second = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Viewport(Box::new(document::entities::Viewport::new())));
        app.tabs[i].scene.active_viewport = Some(first);
        let _ = app.run_command_line("UCSFOLLOW 1");
        assert!(app.tabs[i].scene.ucs_follow());
        app.tabs[i].scene.active_viewport = Some(second);
        assert!(!app.tabs[i].scene.ucs_follow());
        app.tabs[i].scene.active_viewport = None;
        assert!(!app.tabs[i].scene.ucs_follow());
        app.tabs[i].scene.current_layout = "Layout1".into();
        let _ = app.run_command_line("UCSFOLLOW 1");
        assert!(!app.tabs[i].scene.ucs_follow());
        app.tabs[i].scene.active_viewport = Some(first);
        assert!(app.tabs[i].scene.ucs_follow());
    }

    #[test]
    fn enabling_after_defining_ucs_aligns_immediately() {
        let mut app = app();
        let i = app.active_tab;
        let before = app.tabs[i].scene.camera.borrow().rotation;
        let _ = app.run_command_line("UCS Z 30");
        assert_eq!(before, app.tabs[i].scene.camera.borrow().rotation);
        let _ = app.run_command_line("UCSFOLLOW 1");
        let aligned = app.tabs[i].scene.camera.borrow().rotation;
        assert_ne!(
            before, aligned,
            "enabling must align the existing UCS immediately"
        );
        // The same orientation must be obtained through the explicit PLAN path.
        let _ = app.run_command_line("PLAN CURRENT");
        assert_eq!(aligned, app.tabs[i].scene.camera.borrow().rotation);
        let _ = app.run_command_line("UCSFOLLOW 0");
        assert_eq!(aligned, app.tabs[i].scene.camera.borrow().rotation);
    }
}
