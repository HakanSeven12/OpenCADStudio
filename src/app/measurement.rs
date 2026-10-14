//! Per-drawing, transient inquiry UI; never serialized into document entities.
use super::OpenCADStudio;
use iced::Point;

#[derive(Clone, Debug)]
pub(super) struct MeasurementReadout {
    pub text: String,
    pub anchor: Point,
}

#[derive(Default)]
pub(super) struct MeasurementDisplay {
    pub result: Option<MeasurementReadout>,
    pub live_visible: bool,
}

impl MeasurementDisplay {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Called on press before ordinary input processing. It cannot capture
    /// the event, so this same press can also pick/select/pan/finish a command.
    pub fn dismiss_result(&mut self) {
        self.result = None;
    }
}

impl OpenCADStudio {
    pub(super) fn remember_measurement(&mut self, text: &str) {
        let tab = &mut self.tabs[self.active_tab];
        if tab
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.has_measurement_readout())
        {
            tab.measurement.result = Some(MeasurementReadout {
                text: text.to_owned(),
                anchor: tab.last_cursor_screen,
            });
        }
    }

    pub(super) fn measurement_readout(&self, index: usize) -> Option<MeasurementReadout> {
        let tab = &self.tabs[index];
        if let Some(result) = &tab.measurement.result {
            return Some(result.clone());
        }
        if !tab.measurement.live_visible {
            return None;
        }
        let text = tab
            .active_cmd
            .as_ref()?
            .measurement_readout(tab.last_cursor_world)?;
        Some(MeasurementReadout {
            text,
            anchor: tab.last_cursor_screen,
        })
    }
}

#[cfg(test)]
mod measurement_tests {
    use super::*;
    use crate::app::Message;
    use crate::command::{all_registered_command_names, StepInput};
    use crate::modules::{CadModule, ModuleEvent, RibbonItem};
    use glam::DVec3;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        app.snapper.snap_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        app.dyn_input = false;
        app
    }
    fn finish_distance(app: &mut OpenCADStudio) {
        let _ = app.dispatch_command("DIST");
        let _ = app.feed_command(StepInput::Point(DVec3::ZERO));
        let _ = app.feed_command(StepInput::Point(DVec3::new(3.0, 4.0, 0.0)));
    }

    #[test]
    fn measurement_menu_registry_and_dispatch() {
        let names = all_registered_command_names();
        let module = crate::modules::draw::DrawModule;
        let menu = module
            .ribbon_groups()
            .iter()
            .flat_map(|g| &g.tools)
            .find(|item| {
                matches!(
                    item,
                    RibbonItem::LargeDropdown {
                        id: "MEASURE_MENU",
                        ..
                    }
                )
            })
            .unwrap();
        let RibbonItem::LargeDropdown { items, default, .. } = menu else {
            unreachable!()
        };
        assert_eq!(*default, "DIST");
        assert_eq!(
            items.iter().map(|item| item.0).collect::<Vec<_>>(),
            ["DIST", "MEASURERADIUS", "MEASUREANGLE", "AREA"]
        );
        let mut app = app();
        for name in [
            "DIST",
            "MEASURERADIUS",
            "MEASUREANGLE",
            "AREA",
            "MEASUREGEOM",
        ] {
            assert!(names.contains(&name));
            let _ = app.dispatch_command(name);
            assert_eq!(
                app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
                name
            );
            let _ = app.update(Message::RibbonToolClick {
                tool_id: name.into(),
                event: ModuleEvent::Command(name.into()),
            });
            assert_eq!(
                app.tabs[app.active_tab].active_cmd.as_ref().unwrap().name(),
                name
            );
        }
    }

    #[test]
    fn measurement_result_persists_without_document_changes() {
        let mut app = app();
        let i = app.active_tab;
        let count = app.tabs[i].scene.document.entity_count();
        let dirty = app.tabs[i].dirty;
        let revision = app.tabs[i].edit_revision;
        let _ = app.update(Message::ViewportMove(Point::new(350.0, 250.0)));
        finish_distance(&mut app);
        assert!(app.tabs[i].active_cmd.is_none());
        let result = app.measurement_readout(i).unwrap();
        assert!(app.command_line.history_plain_text().contains(&result.text));
        let _ = app.update(Message::ViewportLeftRelease);
        let _ = app.update(Message::ViewportMove(Point::new(450.0, 300.0)));
        let retained = app.measurement_readout(i).unwrap();
        assert_eq!(result.text, retained.text);
        assert_eq!(result.anchor, retained.anchor);
        assert_eq!(app.tabs[i].scene.document.entity_count(), count);
        assert_eq!(app.tabs[i].dirty, dirty);
        assert_eq!(app.tabs[i].edit_revision, revision);
    }

    #[test]
    fn measurement_dismiss_click_also_starts_selection() {
        let mut app = app();
        let i = app.active_tab;
        finish_distance(&mut app);
        let _ = app.update(Message::ViewportMove(Point::new(350.0, 250.0)));
        let _ = app.update(Message::ViewportLeftPress);
        assert!(app.measurement_readout(i).is_none());
        assert!(app.tabs[i].scene.selection.borrow().input.left_down);
        let _ = app.update(Message::ViewportLeftRelease);
        assert!(app.tabs[i]
            .scene
            .selection
            .borrow()
            .gesture
            .box_anchor
            .is_some());
    }

    #[test]
    fn measurement_final_pick_survives_its_own_release() {
        let mut app = app();
        let i = app.active_tab;
        let _ = app.dispatch_command("DIST");
        for point in [Point::new(350.0, 250.0), Point::new(430.0, 280.0)] {
            let _ = app.update(Message::ViewportMove(point));
            let _ = app.update(Message::ViewportLeftPress);
            let _ = app.update(Message::ViewportLeftRelease);
        }
        assert!(app.tabs[i].active_cmd.is_none());
        assert!(app.measurement_readout(i).is_some());
    }

    #[test]
    fn measurement_live_without_dyn_and_escape() {
        let mut app = app();
        let i = app.active_tab;
        let _ = app.dispatch_command("DIST");
        let _ = app.feed_command(StepInput::Point(DVec3::ZERO));
        let _ = app.update(Message::ViewportMove(Point::new(410.0, 310.0)));
        let first = app.measurement_readout(i).unwrap().text;
        let _ = app.update(Message::ViewportMove(Point::new(510.0, 310.0)));
        assert_ne!(first, app.measurement_readout(i).unwrap().text);
        let _ = app.update(Message::CommandEscape);
        assert!(app.measurement_readout(i).is_none());
    }

    #[test]
    fn measurement_escape_new_command_and_mouse_buttons_clear_result() {
        let mut app = app();
        let i = app.active_tab;
        for event in [
            Message::CommandEscape,
            Message::ViewportRightPress,
            Message::ViewportMiddlePress,
        ] {
            finish_distance(&mut app);
            assert!(app.measurement_readout(i).is_some());
            let _ = app.update(event);
            assert!(app.measurement_readout(i).is_none());
        }
        finish_distance(&mut app);
        let _ = app.dispatch_command("MEASURERADIUS");
        assert!(app.measurement_readout(i).is_none());
    }

    #[test]
    fn measurement_area_add_dismiss_click_is_also_next_corner() {
        let mut app = app();
        let i = app.active_tab;
        let _ = app.dispatch_command("AREA");
        let _ = app.feed_command(StepInput::Text("ADD".into()));
        for p in [DVec3::ZERO, DVec3::X * 4.0, DVec3::Y * 3.0] {
            let _ = app.feed_command(StepInput::Point(p));
        }
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "AREA");
        assert!(app.measurement_readout(i).is_some());
        for p in [Point::new(350.0, 250.0), Point::new(410.0, 250.0)] {
            let _ = app.update(Message::ViewportMove(p));
            let _ = app.update(Message::ViewportLeftPress);
            let _ = app.update(Message::ViewportLeftRelease);
        }
        assert!(app.tabs[i].measurement.result.is_none());
        assert!(
            app.measurement_readout(i).is_some(),
            "both corners must reach AREA"
        );
    }

    #[test]
    fn measurement_radius_and_lines_real_entity_injection() {
        use codec::{
            entities::{Circle, Line},
            types::Vector3,
            EntityType,
        };
        let mut app = app();
        let i = app.active_tab;
        let circle = app.tabs[i]
            .scene
            .add_entity(EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)));
        let first = app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_X,
            )));
        let second = app.tabs[i]
            .scene
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::UNIT_Y,
            )));
        let _ = app.dispatch_command("MEASURERADIUS");
        let _ = app.feed_command(StepInput::EntityPick(circle, DVec3::X * 2.0));
        assert!(app.tabs[i].active_cmd.is_none());
        assert!(app.measurement_readout(i).unwrap().text.contains("Radius"));
        let _ = app.dispatch_command("MEASUREANGLE");
        let _ = app.feed_command(StepInput::Text("LINES".into()));
        let _ = app.feed_command(StepInput::EntityPick(first, DVec3::X));
        let _ = app.feed_command(StepInput::EntityPick(second, DVec3::Y));
        assert!(app.tabs[i].active_cmd.is_none());
        assert!(app.measurement_readout(i).unwrap().text.contains("90"));
    }
}
