//! Application policy for device navigation. All scene writes run on the UI
//! thread, preserving active commands and the scene's viewport locks.
use super::{Message, ModalKind, OpenCADStudio};
use crate::input::spacemouse::{Action, NavigationMode, Target, View};
use crate::scene::view::camera::Projection;
use iced::Task;

#[derive(Default)]
pub(super) struct SelectionCache(
    std::cell::RefCell<Option<(Target, u64, u64, Option<(glam::DVec3, glam::DVec3)>)>>,
);
impl SelectionCache {
    fn bounds(
        &self,
        scene: &crate::scene::Scene,
        target: &Target,
    ) -> Option<(glam::DVec3, glam::DVec3)> {
        let mut cache = self.0.borrow_mut();
        if let Some((stamp, geometry, selection, bounds)) = cache.as_ref() {
            if stamp.same_context(target)
                && *geometry == scene.geometry_epoch
                && *selection == scene.selection_generation
            {
                return *bounds;
            }
        }
        let bounds = scene.navigation_selection_bounds();
        *cache = Some((
            target.clone(),
            scene.geometry_epoch,
            scene.selection_generation,
            bounds,
        ));
        bounds
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "SPACEMOUSE",
        "SPACEMOUSEPAUSE",
        "SPACEMOUSEPAN",
        "SPACEMOUSEPANZOOM",
        "SPACEMOUSEAUTO",
        "SPACEMOUSE3D"
    ],
});

impl OpenCADStudio {
    fn navigation_target(&self) -> Target {
        let tab = &self.tabs[self.active_tab];
        Target {
            tab: tab.id,
            layout: format!("{}:{:?}", tab.scene.current_layout, tab.active_block_edit),
            viewport: tab.scene.active_viewport.map(|h| h.value()),
            tile: tab.scene.active_model_tile.get(),
            generation: tab.scene.camera_generation,
        }
    }
    pub(super) fn spacemouse_sheet(&self) -> bool {
        let scene = &self.tabs[self.active_tab].scene;
        scene.current_layout != "Model" && scene.active_viewport.is_none()
    }
    pub(super) fn spacemouse_label(&self) -> String {
        if !self.spacemouse_preferences.enabled {
            return crate::t!("Disabled").into_owned();
        }
        if self.spacemouse_paused {
            return crate::t!("Paused").into_owned();
        }
        if self.tabs[self.active_tab].scene.navigation_locked() {
            return crate::t!("Viewport locked").into_owned();
        }
        if self.spacemouse.status() != crate::input::spacemouse::Status::Ready {
            return crate::t!(self.spacemouse.status().label()).into_owned();
        }
        let mode = self.spacemouse_preferences.mode;
        let effective = if self.spacemouse_sheet() && mode.allows_zoom() {
            NavigationMode::PanZoom
        } else if mode == NavigationMode::Auto {
            NavigationMode::Full3D
        } else {
            mode
        };
        crate::t!(effective.label()).into_owned()
    }
    pub(super) fn sync_spacemouse(&self) {
        let tab = &self.tabs[self.active_tab];
        let scene = &tab.scene;
        let available = self.spacemouse_preferences.enabled
            && !tab.is_start
            && self.opening.is_none()
            && self.spacemouse.status() == crate::input::spacemouse::Status::Ready;
        if !available {
            self.spacemouse.sync(None, false);
            return;
        }
        // The driver's settings dialog must still be able to query this view
        // after it takes keyboard focus. Gate input separately from its metadata.
        let allowed = self.spacemouse_focused && self.active_modal.is_none();
        let camera = scene.navigation_camera();
        let (w, h) = scene.selection.borrow().vp_size;
        let bounds = scene.active_model_tile_bounds(w, h);
        let aspect = scene
            .active_viewport
            .and_then(|handle| match scene.document.get_entity(handle) {
                Some(acadrust::EntityType::Viewport(vp)) => Some(vp.width / vp.height.max(1e-6)),
                _ => None,
            })
            .unwrap_or((bounds.width / bounds.height.max(1.)).max(0.01) as f64);
        let center = camera.target;
        let span = glam::DVec3::splat(camera.distance.max(1.) as f64);
        let (min, max) = camera
            .model_bounds
            .or_else(|| {
                scene
                    .model_space_extents()
                    .map(|(min, max)| (min.as_dvec3(), max.as_dvec3()))
            })
            .unwrap_or((center - span, center + span));
        let target = self.navigation_target();
        let selected = self.spacemouse_selection.bounds(scene, &target);
        let pivot = selected
            .map(|(min, max)| (min + max) * 0.5)
            .or_else(|| {
                self.spacemouse_pivot
                    .as_ref()
                    .filter(|(stamp, _)| stamp.same_context(&target))
                    .map(|(_, point)| *point)
            })
            .unwrap_or(center);
        let selection_bounds =
            selected.map(|(min, max)| [min.x, min.y, min.z, max.x, max.y, max.z]);
        self.spacemouse.sync(
            Some(View {
                target,
                eye: camera.eye(),
                rotation: camera.rotation.as_dquat().normalize(),
                distance: camera.distance as f64,
                fov: camera.fov_y as f64,
                perspective: camera.projection == Projection::Perspective,
                aspect,
                model_bounds: [min.x, min.y, min.z, max.x, max.y, max.z],
                selection_bounds,
                pointer: tab.last_cursor_world,
                pivot,
                pivot_visible: self.spacemouse_pivot.is_some(),
                rotate: self
                    .spacemouse_preferences
                    .mode
                    .allows_rotation(self.spacemouse_sheet()),
                zoom: self.spacemouse_preferences.mode.allows_zoom(),
                motion_enabled: !self.spacemouse_paused && !scene.navigation_locked(),
                units_to_meters: super::properties::insunits_to_mm(
                    scene.document.header.insertion_units,
                )
                .unwrap_or(1000.)
                    / 1000.,
                pan_speed: self.spacemouse_preferences.pan_speed.clamp(10, 300) as f64 / 100.
                    * if self.spacemouse_preferences.pan_reversed {
                        -1.
                    } else {
                        1.
                    },
            }),
            allowed,
        );
    }
    pub(super) fn on_spacemouse_wake(&mut self) -> Task<Message> {
        let (view, commands) = self.spacemouse.drain();
        if !self.spacemouse_focused
            || self.active_modal.is_some()
            || !self.spacemouse_preferences.enabled
            || self.tabs[self.active_tab].is_start
        {
            self.spacemouse_was_moving = false;
            self.spacemouse_pivot = None;
            return Task::none();
        }
        let target = self.navigation_target();
        let mut tasks = Vec::new();
        let moving = self.spacemouse.moving() && !self.spacemouse_paused;
        if moving && !self.spacemouse_was_moving {
            self.tabs[self.active_tab].scene.remember_current_view();
            self.clear_navigation_hover(self.active_tab);
        }
        if !moving && self.spacemouse_was_moving {
            self.arm_hover_after_navigation(self.active_tab);
        }
        self.spacemouse_was_moving = moving;
        if !moving {
            self.spacemouse_pivot = None;
        }
        if let Some(view) = view.filter(|view| view.target == target && !self.spacemouse_paused) {
            let i = self.active_tab;
            let mut camera = self.tabs[i].scene.navigation_camera();
            if view.rotate {
                camera.rotation = view.rotation.as_quat();
                camera.sync_yaw_pitch();
            }
            if view.zoom {
                camera.distance = view.distance as f32;
            }
            // Use the same f32 basis/distance as Camera::eye so a callback
            // that only updates the pivot cannot introduce camera drift.
            camera.target =
                view.eye - (camera.rotation * glam::Vec3::Z).as_dvec3() * camera.distance as f64;
            let started = iced::time::Instant::now();
            if self.tabs[i].scene.apply_navigation_camera(camera) {
                self.spacemouse
                    .acknowledge(&view.target, &self.navigation_target());
                self.spacemouse_pivot = (view.pivot_visible && moving)
                    .then_some((self.navigation_target(), view.pivot));
                let (w, h) = self.tabs[i].scene.selection.borrow().vp_size;
                self.reproject_box_anchor(i, w, h);
                // The pointer may be stationary while the left hand navigates.
                // Refresh the active command's snap/preview against the new
                // camera, using the existing pointer path without a click.
                let cursor = {
                    let selection = self.tabs[i].scene.selection.borrow();
                    (!selection.left_down
                        && !selection.middle_down
                        && self.tabs[i].active_cmd.is_some())
                    .then_some(selection.last_move_pos)
                    .flatten()
                };
                if let Some(cursor) = cursor {
                    tasks.push(self.on_viewport_move(cursor));
                }
                self.tabs[i]
                    .scene
                    .record_nav_perf(crate::scene::NavPerfOp::Pan, started);
            }
        }
        for (stamp, command) in commands {
            if stamp.same_context(&self.navigation_target()) && self.active_modal.is_none() {
                tasks.push(self.run_action(&command));
            }
        }
        Task::batch(tasks)
    }

    pub(super) fn spacemouse_pivot_overlay(&self) -> Option<iced::Element<'_, Message>> {
        let (target, point) = self.spacemouse_pivot.as_ref()?;
        if target != &self.navigation_target() || self.active_modal.is_some() {
            return None;
        }
        let scene = &self.tabs[self.active_tab].scene;
        let (w, h) = scene.selection.borrow().vp_size;
        let rect = scene
            .active_viewport
            .and_then(|handle| scene.viewport_screen_rect(handle, (w, h)))
            .unwrap_or_else(|| scene.active_model_tile_bounds(w, h));
        let p = scene.navigation_camera().project(*point, rect)?;
        if p.x < 0. || p.y < 0. || p.x > rect.width || p.y > rect.height {
            return None;
        }
        let marker = iced::widget::container(iced::widget::Space::new())
            .width(10)
            .height(10)
            .style(|theme: &iced::Theme| iced::widget::container::Style {
                border: iced::Border {
                    color: theme.palette().primary.base.color,
                    width: 2.,
                    radius: 5.0.into(),
                },
                ..Default::default()
            });
        Some(
            iced::widget::pin(marker)
                .position(iced::Point::new(rect.x + p.x - 5., rect.y + p.y - 5.))
                .into(),
        )
    }
    pub(super) fn open_spacemouse_preferences(&mut self) {
        self.options_tab = crate::ui::window::options::OptionsTab::UserPreferences;
        self.active_modal = Some(ModalKind::Options);
    }
    pub(super) fn open_spacemouse_driver_settings(&self) -> Task<Message> {
        #[cfg(windows)]
        {
            if let Some(program_files) =
                std::env::var_os("ProgramW6432").or_else(|| std::env::var_os("ProgramFiles"))
            {
                let path = std::path::PathBuf::from(program_files)
                    .join("3Dconnexion/3DxWare/3DxWinCore/3DxSmartUi.exe");
                if path.is_file() {
                    return Task::perform(
                        async move {
                            let _ = std::process::Command::new(path).spawn();
                        },
                        |_| Message::SpaceMouseWake,
                    );
                }
            }
        }
        crate::sys::open_url("https://3dconnexion.com/drivers/", self.main_window)
    }
}

pub(super) fn actions() -> Vec<Action> {
    use crate::modules::{IconKind, ModuleEvent, RibbonItem, ToolDef};
    use std::collections::BTreeMap;
    let mut names = crate::command::all_registered_command_names();
    names.extend([
        "UNDO",
        "REDO",
        "CANCEL",
        "QSAVE",
        "OPEN",
        "ZOOM",
        "PLAN",
        "SPACEMOUSEFIT",
        "SPACEMOUSETOP",
    ]);
    names.sort_unstable();
    names.dedup();
    let mut actions: BTreeMap<String, Action> = names
        .into_iter()
        .map(|name| {
            let label = match name {
                "SPACEMOUSE" => "SpaceMouse preferences",
                "SPACEMOUSEPAUSE" => "Pause / resume SpaceMouse",
                "SPACEMOUSEPAN" => "SpaceMouse: pan only",
                "SPACEMOUSEPANZOOM" => "SpaceMouse: pan and zoom",
                "SPACEMOUSEAUTO" => "SpaceMouse: follow context",
                "SPACEMOUSE3D" => "SpaceMouse: 3D navigation",
                "SPACEMOUSEFIT" => "Fit view",
                "SPACEMOUSETOP" => "Top view",
                _ => name,
            };
            (
                name.into(),
                Action {
                    id: name.into(),
                    label: crate::t!(label).into_owned(),
                    description: format!("{} ({name})", crate::t!(label)),
                    icon: name
                        .starts_with("SPACEMOUSE")
                        .then_some(crate::ui::window::options::spacemouse::ICON),
                },
            )
        })
        .collect();
    fn describe(
        actions: &mut BTreeMap<String, Action>,
        command: &str,
        label: &str,
        icon: IconKind,
    ) {
        if let Some(action) = actions.get_mut(command) {
            action.label = crate::t!(label).into_owned();
            action.description = format!("{} ({command})", action.label);
            action.icon = match icon {
                IconKind::Svg(bytes) => Some(bytes),
                _ => None,
            };
        }
    }
    fn tool(actions: &mut BTreeMap<String, Action>, tool: &ToolDef) {
        if let ModuleEvent::Command(command) = &tool.event {
            describe(actions, command, tool.label, tool.icon);
        }
    }
    for module in crate::modules::registry::all_modules() {
        for group in module.ribbon_groups() {
            for item in &group.tools {
                match item {
                    RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => {
                        tool(&mut actions, t)
                    }
                    RibbonItem::Dropdown { items, .. }
                    | RibbonItem::LabeledDropdown { items, .. }
                    | RibbonItem::LargeDropdown { items, .. } => {
                        for (label, command, icon) in items {
                            describe(&mut actions, command, label, *icon);
                        }
                    }
                    RibbonItem::ToolGrid { columns }
                    | RibbonItem::StyleComboGroup { rows: columns, .. } => {
                        for t in columns.iter().flatten() {
                            tool(&mut actions, t);
                        }
                    }
                    RibbonItem::LayerComboGroup { row2, row3 } => {
                        for t in row2.iter().chain(row3) {
                            tool(&mut actions, t);
                        }
                    }
                    RibbonItem::PropertiesGroup { match_prop } => tool(&mut actions, match_prop),
                }
            }
        }
    }
    actions.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app.tabs[app.active_tab]
            .scene
            .selection
            .borrow_mut()
            .vp_size = (800., 600.);
        app.spacemouse_focused = true;
        app.spacemouse.test_connect();
        app.sync_spacemouse();
        app
    }

    #[test]
    fn spacemouse_navigation_and_buttons_preserve_a_live_polyline() {
        let mut app = app();
        let _ = app.dispatch_command("PLINE");
        for point in [
            DVec3::ZERO,
            DVec3::new(10., 0., 0.),
            DVec3::new(10., 10., 0.),
        ] {
            let _ = app.feed_command(crate::command::StepInput::Point(point));
        }
        let i = app.active_tab;
        let before = app.tabs[i].scene.navigation_camera();
        let epoch = app.tabs[i].scene.geometry_epoch;
        app.sync_spacemouse();
        app.spacemouse.test_propose(|v| v.eye.x += 3.);
        let _ = app.on_spacemouse_wake();
        assert_eq!(
            app.tabs[i].scene.navigation_camera().target.x,
            before.target.x + 3.
        );
        assert_eq!(app.tabs[i].scene.geometry_epoch, epoch);
        assert!(app.tabs[i].active_cmd.is_some());
        let _ = app.run_action("UNDO");
        assert!(app.tabs[i].active_cmd.is_some());
        // Finish after Undo: the last point was removed, leaving one segment.
        let _ = app.feed_command(crate::command::StepInput::Enter);
        let polyline = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|e| {
                if let acadrust::EntityType::LwPolyline(p) = e {
                    Some(p)
                } else {
                    None
                }
            })
            .expect("finished polyline");
        assert_eq!(polyline.vertices.len(), 2);
    }

    #[test]
    fn spacemouse_refreshes_a_live_command_at_a_stationary_pointer() {
        let mut app = app();
        app.snapper.snap_enabled = false;
        app.snapper.grid_snap_on = false;
        app.snapper.otrack_enabled = false;
        app.ortho_mode = false;
        app.polar_mode = false;
        let _ = app.dispatch_command("LINE");
        let _ = app.feed_command(crate::command::StepInput::Point(DVec3::ZERO));
        let _ = app.on_viewport_move(iced::Point::new(480., 300.));
        let i = app.active_tab;
        let before = app.tabs[i].last_cursor_world;
        let count = app.tabs[i].scene.document.entities().count();
        app.sync_spacemouse();
        app.spacemouse.test_propose(|v| v.eye.x += 3.);
        let _ = app.on_spacemouse_wake();
        assert!((app.tabs[i].last_cursor_world.x - before.x - 3.).abs() < 1e-4);
        assert!(app.tabs[i].active_cmd.is_some());
        assert_eq!(app.tabs[i].scene.document.entities().count(), count);
    }

    #[test]
    fn spacemouse_stale_camera_modal_and_pause_block_pending_motion() {
        for guard in ["camera", "modal", "pause", "focus"] {
            let mut app = app();
            let before = app.tabs[app.active_tab].scene.navigation_camera().target;
            app.spacemouse.test_propose(|v| v.eye.x += 30.);
            match guard {
                "camera" => app.tabs[app.active_tab].scene.camera_generation += 1,
                "modal" => app.active_modal = Some(ModalKind::Options),
                "pause" => app.spacemouse_paused = true,
                _ => app.spacemouse_focused = false,
            }
            let _ = app.on_spacemouse_wake();
            assert_eq!(
                app.tabs[app.active_tab].scene.navigation_camera().target,
                before,
                "{guard}"
            );
        }
    }

    #[test]
    fn spacemouse_settings_dialog_retains_view_metadata_without_accepting_motion() {
        for modal in [false, true] {
            let mut app = app();
            app.spacemouse_preferences.mode = NavigationMode::Full3D;
            app.sync_spacemouse();
            let before = app.tabs[app.active_tab].scene.navigation_camera();
            if modal {
                app.active_modal = Some(ModalKind::Options);
            } else {
                app.spacemouse_focused = false;
            }
            app.sync_spacemouse();
            let snapshot = app
                .spacemouse
                .test_view()
                .expect("driver can describe the inactive view");
            assert!(snapshot.rotate);
            assert_eq!(snapshot.distance, before.distance as f64);
            app.spacemouse.test_propose(|v| v.eye.x += 10.);
            let _ = app.on_spacemouse_wake();
            assert_eq!(
                app.tabs[app.active_tab].scene.navigation_camera().target,
                before.target
            );
        }
    }

    #[test]
    fn spacemouse_floating_camera_round_trip_and_lock() {
        let mut app = app();
        let scene = &mut app.tabs[app.active_tab].scene;
        let paper_camera = scene.camera.borrow().clone();
        let mut vp = acadrust::entities::Viewport::default();
        vp.width = 200.;
        vp.height = 100.;
        vp.view_height = 50.;
        let handle = scene.add_entity(acadrust::EntityType::Viewport(vp));
        scene.active_viewport = Some(handle);
        let mut camera = scene.navigation_camera();
        camera.target = DVec3::new(120., 30., 8.);
        camera.rotation = glam::Quat::from_euler(glam::EulerRot::XYZ, 0.7, 0.3, 0.2);
        camera.distance = 80.;
        assert!(scene.apply_navigation_camera(camera.clone()));
        let round_trip = scene.navigation_camera();
        assert!(round_trip.target.distance(camera.target) < 1e-6);
        assert!(round_trip.rotation.abs_diff_eq(camera.rotation, 1e-5));
        assert!((round_trip.distance - camera.distance).abs() < 1e-4);
        assert_eq!(scene.camera.borrow().target, paper_camera.target);
        assert_eq!(scene.camera.borrow().distance, paper_camera.distance);
        if let Some(acadrust::EntityType::Viewport(vp)) = scene.document.get_entity_mut(handle) {
            vp.status.locked = true;
        }
        camera.target.x += 100.;
        assert!(!scene.apply_navigation_camera(camera));
        assert_eq!(scene.navigation_camera().target, round_trip.target);
    }

    #[test]
    fn spacemouse_push_and_tilt_move_the_drawing_in_screen_space_continuously() {
        for mode in [NavigationMode::PanOnly, NavigationMode::PanZoom] {
            for projection in [Projection::Orthographic, Projection::Perspective] {
                for reversed in [false, true] {
                    for (axes, direction) in [
                        ([350, 0, 0, 0, 0, 0], glam::Vec2::X),
                        ([0, 0, 0, 0, -350, 0], glam::Vec2::X),
                        ([-350, 0, 0, 0, 0, 0], -glam::Vec2::X),
                        ([0, 0, 0, 0, 350, 0], -glam::Vec2::X),
                        ([0, -350, 0, 0, 0, 0], -glam::Vec2::Y),
                        ([0, 0, 0, -350, 0, 0], -glam::Vec2::Y),
                        ([0, 350, 0, 0, 0, 0], glam::Vec2::Y),
                        ([0, 0, 0, 350, 0, 0], glam::Vec2::Y),
                    ] {
                        let mut app = app();
                        app.spacemouse_preferences.mode = mode;
                        app.spacemouse_preferences.pan_reversed = reversed;
                        let i = app.active_tab;
                        {
                            let mut camera = app.tabs[i].scene.camera.borrow_mut();
                            camera.rotation =
                                glam::Quat::from_euler(glam::EulerRot::XYZ, 0.7, 0.4, 0.2);
                            camera.projection = projection;
                        }
                        app.sync_spacemouse();
                        let before = app.tabs[i].scene.navigation_camera();
                        let bounds = iced::Rectangle::with_size(iced::Size::new(800., 600.));
                        let point = before.target;
                        let screen = before.project(point, bounds).unwrap();
                        let direction = direction * if reversed { -1. } else { 1. };
                        let mut last = glam::Vec2::new(screen.x, screen.y);
                        for frame in 0..20 {
                            app.spacemouse.test_pan_report(axes);
                            app.spacemouse.frame(frame as f64 * 16.);
                            let _ = app.on_spacemouse_wake();
                            app.sync_spacemouse();
                            let camera = app.tabs[i].scene.navigation_camera();
                            let moved = camera.project(point, bounds).unwrap();
                            let moved = glam::Vec2::new(moved.x, moved.y);
                            let displacement = moved - last;
                            assert!(displacement.dot(direction) > 0., "{mode:?}: {axes:?}");
                            assert!(displacement.perp_dot(direction).abs() < 0.001);
                            assert_eq!(camera.rotation, before.rotation);
                            assert_eq!(camera.distance, before.distance);
                            last = moved;
                        }
                        let stopped = app.tabs[i].scene.navigation_camera().target;
                        app.spacemouse.test_pan_report([0, 0, 0, 0, 0, 350]);
                        app.spacemouse.frame(400.);
                        let _ = app.on_spacemouse_wake();
                        assert_eq!(app.tabs[i].scene.navigation_camera().target, stopped);
                    }
                }
            }
        }
    }

    #[test]
    fn spacemouse_panzoom_lift_and_pressure_zoom_about_the_center_and_release_stops() {
        for projection in [Projection::Orthographic, Projection::Perspective] {
            for mode in [NavigationMode::PanOnly, NavigationMode::PanZoom] {
                for pressure in [-350, 350] {
                    let mut app = app();
                    app.spacemouse_preferences.mode = mode;
                    let i = app.active_tab;
                    {
                        let mut camera = app.tabs[i].scene.camera.borrow_mut();
                        camera.rotation =
                            glam::Quat::from_euler(glam::EulerRot::XYZ, 0.7, 0.4, 0.2);
                        camera.projection = projection;
                    }
                    app.sync_spacemouse();
                    let before = app.tabs[i].scene.navigation_camera();
                    let mut distance = before.distance;
                    for frame in 0..20 {
                        app.spacemouse.test_pan_report([0, 0, pressure, 0, 0, 0]);
                        app.spacemouse.frame(frame as f64 * 16.);
                        let _ = app.on_spacemouse_wake();
                        app.sync_spacemouse();
                        let camera = app.tabs[i].scene.navigation_camera();
                        assert!(camera.target.distance(before.target) < 1e-4);
                        assert_eq!(camera.rotation, before.rotation);
                        if mode == NavigationMode::PanOnly {
                            assert_eq!(camera.distance, distance);
                        } else {
                            assert!((camera.distance - distance) * pressure as f32 > 0.);
                        }
                        distance = camera.distance;
                    }
                    app.spacemouse.test_pan_report([0; 6]);
                    app.spacemouse.frame(400.);
                    let _ = app.on_spacemouse_wake();
                    assert_eq!(app.tabs[i].scene.navigation_camera().distance, distance);
                    assert!(!app.spacemouse.moving());
                }
            }
        }
    }

    #[test]
    fn spacemouse_preferences_round_trip_and_old_settings_default() {
        let old: super::super::settings::UserSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            old.spacemouse,
            crate::input::spacemouse::Preferences::default()
        );
        let mut app = app();
        app.spacemouse_preferences.mode = NavigationMode::PanOnly;
        app.spacemouse_preferences.pan_speed = 170;
        app.spacemouse_preferences.pan_reversed = true;
        let encoded = serde_json::to_string(&app.current_settings()).unwrap();
        let restored: super::super::settings::UserSettings =
            serde_json::from_str(&encoded).unwrap();
        app.spacemouse_preferences.mode = NavigationMode::Auto;
        app.apply_settings(&restored);
        assert_eq!(app.spacemouse_preferences.mode, NavigationMode::PanOnly);
        assert_eq!(app.spacemouse_preferences.pan_speed, 170);
        assert!(app.spacemouse_preferences.pan_reversed);
    }

    #[test]
    fn spacemouse_action_catalog_has_stable_unique_ids_and_ui_icons() {
        let actions = actions();
        let ids: std::collections::HashSet<_> = actions.iter().map(|a| &a.id).collect();
        assert_eq!(ids.len(), actions.len());
        for id in [
            "LINE",
            "UNDO",
            "SPACEMOUSEPAN",
            "SPACEMOUSEPAUSE",
            "SPACEMOUSEFIT",
        ] {
            assert!(actions.iter().any(|a| a.id == id && !a.label.is_empty()));
        }
        assert!(actions.iter().any(|a| a.id == "LINE" && a.icon.is_some()));
        assert!(crate::app::commands::is_transparent("SPACEMOUSEPAN"));
        assert!(crate::app::commands::start_allowed("SPACEMOUSE"));
    }
}
