//! A value typed into a Properties field is applied when the field is left —
//! by Enter, Tab, or a click elsewhere — not only on Enter. Esc still drops
//! it (`command.rs`, the palette-cell revert).

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::ui::properties::FieldKey;

impl OpenCADStudio {
    /// Apply what was typed into `key`, as Enter on that field would. A field
    /// with nothing typed changes nothing.
    pub(super) fn commit_prop_field(&mut self, key: FieldKey) -> Task<Message> {
        let i = self.active_tab;
        if !self.tabs[i].properties.edit_buf.contains_key(&key) {
            return Task::none();
        }
        match key {
            FieldKey::Geom(field) => self.on_prop_geom_commit(field),
            FieldKey::Attr(tag) => self.on_prop_attr_commit(tag),
            FieldKey::BlockParameter(key) => self.commit_block_parameter_property(key, None),
            FieldKey::PluginParam(name) => self.on_prop_plugin_param_commit(name),
            FieldKey::Param(index, field) => self.on_prop_param_commit(index, field),
        }
    }

    /// A press in the drawing takes the focus from the panel. The typed value
    /// is applied now, before the press can pick another selection (which
    /// would rebuild the panel and drop the value).
    pub(super) fn commit_active_prop_field(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(key) = self.tabs[i].properties.active_field.take() else {
            return Task::none();
        };
        self.commit_prop_field(key)
    }

    /// Tab / Shift+Tab in a Properties field: apply it, then move to the next
    /// (previous) field on screen, wrapping round. `None` when no Properties
    /// field holds the keyboard, so the key keeps its usual meaning.
    pub(super) fn prop_tab(&mut self, forward: bool) -> Option<Task<Message>> {
        let i = self.active_tab;
        let key = self.tabs[i].properties.active_field.clone()?;
        let commit = self.commit_prop_field(key.clone());
        let panel = &mut self.tabs[i].properties;
        let order = panel.tab_order();
        let next = match order.iter().position(|k| *k == key) {
            Some(at) if forward => order.get((at + 1) % order.len()),
            Some(at) => order.get((at + order.len() - 1) % order.len()),
            None => order.first(),
        }
        .cloned();
        let Some(next) = next else {
            panel.active_field = None;
            return Some(commit);
        };
        let id = next.widget_id();
        panel.active_field = Some(next);
        Some(Task::batch([
            commit,
            iced::widget::operation::focus(id.clone()),
            iced::widget::operation::select_all(id),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::properties::FieldKey;

    /// A drawing with one line (0,0)–(10,0) selected and shown in the panel.
    fn line_selected() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.show_properties = true;
        app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .expect("the drawn line has a handle")
            .to_owned();
        app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
        app.refresh_properties();
        app
    }

    fn start_x(app: &mut OpenCADStudio) -> f64 {
        let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
        q["entities"][0]["start"][0].as_f64().expect("start x")
    }

    fn undo_len(app: &mut OpenCADStudio) -> usize {
        app.finish_all_pending_history();
        app.tabs[app.active_tab].history.undo_stack.len()
    }

    /// Focus lands on `field` and `value` is typed into it.
    fn type_into(app: &mut OpenCADStudio, field: &'static str, value: &str) {
        let key = FieldKey::Geom(field);
        let _ = app.update(Message::PropSyncActive(Some(key.widget_id())));
        assert_eq!(app.tabs[app.active_tab].properties.active_field, Some(key));
        let _ = app.update(Message::PropGeomInput {
            field,
            value: value.to_string(),
        });
    }

    #[test]
    fn a_value_is_applied_when_the_focus_leaves_the_field() {
        let mut app = line_selected();
        type_into(&mut app, "start_x", "3");
        let undo_before = undo_len(&mut app);
        // A click on a widget outside the panel.
        let _ = app.update(Message::PropSyncActive(None));
        assert_eq!(start_x(&mut app), 3.0);
        assert_eq!(undo_len(&mut app), undo_before + 1);
        assert!(app.tabs[app.active_tab].properties.edit_buf.is_empty());
        assert_eq!(app.tabs[app.active_tab].properties.active_field, None);
    }

    #[test]
    fn a_value_is_applied_when_another_field_takes_the_focus() {
        let mut app = line_selected();
        type_into(&mut app, "start_x", "4");
        let next = FieldKey::Geom("start_y");
        let _ = app.update(Message::PropSyncActive(Some(next.widget_id())));
        assert_eq!(start_x(&mut app), 4.0);
        assert_eq!(app.tabs[app.active_tab].properties.active_field, Some(next));
    }

    #[test]
    fn a_click_in_the_drawing_applies_the_value_before_the_pick() {
        let mut app = line_selected();
        type_into(&mut app, "start_x", "2");
        let _ = app.update(Message::ViewportLeftPress);
        assert_eq!(start_x(&mut app), 2.0);
    }

    #[test]
    fn tab_applies_the_value_and_moves_to_the_next_field() {
        let mut app = line_selected();
        let order = app.tabs[app.active_tab].properties.tab_order();
        assert!(!order.is_empty(), "the line has typed-value fields");
        let first = order[0].clone();
        let FieldKey::Geom(field) = first else {
            panic!("a geometry field first: {first:?}");
        };
        type_into(&mut app, field, "1");
        let _ = app.update(Message::ShortcutPressed("TAB".into()));
        let order = app.tabs[app.active_tab].properties.tab_order();
        let active = app.tabs[app.active_tab].properties.active_field.clone();
        assert_eq!(active.as_ref(), order.get(1 % order.len()));
        assert!(app.tabs[app.active_tab].properties.edit_buf.is_empty());

        let _ = app.update(Message::ShortcutPressed("SHIFT+TAB".into()));
        assert_eq!(
            app.tabs[app.active_tab].properties.active_field,
            Some(FieldKey::Geom(field))
        );
    }

    #[test]
    fn escape_still_drops_the_typed_value() {
        let mut app = line_selected();
        type_into(&mut app, "start_x", "6");
        let _ = app.update(Message::CommandEscape);
        let _ = app.update(Message::PropSyncActive(None));
        assert_eq!(start_x(&mut app), 0.0);
    }

    #[test]
    fn an_invalid_value_left_by_a_click_is_handled_as_enter_handles_it() {
        let mut app = line_selected();
        type_into(&mut app, "start_x", "abc");
        let _ = app.update(Message::PropSyncActive(None));
        assert_eq!(start_x(&mut app), 0.0);
        assert!(app.tabs[app.active_tab].properties.edit_buf.is_empty());
    }

    #[test]
    fn tab_without_a_panel_field_keeps_its_usual_meaning() {
        let mut app = line_selected();
        assert_eq!(app.tabs[app.active_tab].properties.active_field, None);
        assert!(app.prop_tab(true).is_none());
    }
}
