use super::{Message, OpenCADStudio};
use crate::command::{CadCommand, CmdResult};
use crate::scene::block_authoring::edit::{self, Part};
use codec::Handle;
use glam::DVec3;
use iced::{Point, Rectangle, Task};

const EDIT_GRIP_START: usize = usize::MAX / 4;
pub(super) fn is_editor_grip(id: usize) -> bool {
    (EDIT_GRIP_START..EDIT_GRIP_START + 16).contains(&id)
}
pub(super) fn editor_grip_index(id: usize) -> Option<usize> {
    is_editor_grip(id).then(|| id - EDIT_GRIP_START)
}
struct EditCommand {
    document: u64,
    definition: Handle,
    handle: Handle,
    part: Part,
    document_snapshot: codec::Document,
}
impl CadCommand for EditCommand {
    fn name(&self) -> &'static str {
        "BEDITGRIP"
    }
    fn prompt(&self) -> String {
        crate::t!("Block Editor  Specify the new authoring grip location (Escape cancels):")
            .into_owned()
    }
    fn on_point(&mut self, position: DVec3) -> CmdResult {
        CmdResult::EditBlockAuthoring {
            document: self.document,
            definition: self.definition,
            handle: self.handle,
            part: self.part,
            position,
        }
    }
    fn on_preview_wires(&mut self, position: DVec3) -> Vec<crate::scene::WireModel> {
        let mut staged = self.document_snapshot.clone();
        let at = codec::types::Vector3::new(position.x, position.y, 0.0);
        if edit::move_part(&mut staged, self.definition, self.handle, self.part, at).is_err() {
            return Vec::new();
        }
        // A legacy block basepoint is represented by the definition handle.
        // Moving it materializes a native BasePointParameter with its own
        // handle, so resolve that replacement when producing the preview.
        let preview_handle = if self.handle == self.definition {
            crate::scene::block_basepoint::parameter(&staged, self.definition)
                .map(|(handle, _)| handle)
                .unwrap_or(self.handle)
        } else {
            self.handle
        };
        let Some(item) = edit::items(&staged, self.definition)
            .into_iter()
            .find(|item| item.handle == preview_handle)
        else {
            return Vec::new();
        };
        item.graphic
            .lines
            .into_iter()
            .map(|[a, b]| {
                crate::scene::WireModel::solid_f64(
                    format!("authoring-preview-{}", self.handle.value()),
                    vec![[a.x, a.y, a.z], [b.x, b.y, b.z]],
                    crate::scene::WireModel::CYAN,
                    false,
                )
            })
            .collect()
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
impl OpenCADStudio {
    pub(super) fn set_authoring_alignment_type(&mut self, value: &str) -> Task<Message> {
        let i = self.active_tab;
        let Some(definition) = self.tabs[i].active_block_edit_session().map(|session| session.br_handle) else { return Task::none() };
        let Some(parameter) = self.tabs[i].selected_authoring else { return Task::none() };
        let perpendicular = if value.eq_ignore_ascii_case("Perpendicular") || value == crate::t!("Perpendicular").as_ref() { true }
            else if value.eq_ignore_ascii_case("Tangent") || value == crate::t!("Tangent").as_ref() { false }
            else { self.command_line.push_error(crate::t!("Choose Perpendicular or Tangent.").as_ref()); return Task::none() };
        let mut staged = self.tabs[i].scene.document.clone();
        match crate::scene::block_orientation_parameter::set_alignment_type(&mut staged, definition, parameter, perpendicular) {
            Ok(changed) => {
                if changed {
                    self.push_undo_snapshot(i, "Alignment type");
                    self.apply_action_document(i, staged);
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                    self.finish_pending_history(i);
                }
                self.tabs[i].properties.edit_choice_open = false;
                self.refresh_properties();
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }

    pub(super) fn commit_authoring_action_option(&mut self, field: &'static str) -> Task<Message> {
        if field.starts_with("authoring_action_base_") {
            return self.commit_authoring_action_base(field);
        }
        use crate::scene::block_actions::options;
        let i = self.active_tab;
        let Some(session) = self.tabs[i].active_block_edit_session() else { return Task::none() };
        let definition = session.br_handle;
        let Some(handle) = self.tabs[i].selected_authoring else { return Task::none() };
        let key = crate::ui::properties::FieldKey::Geom(field);
        let Some(input) = self.tabs[i].properties.edit_buf.get(&key) else { return Task::none() };
        let Some(before) = options::offsets(&self.tabs[i].scene.document, handle) else { return Task::none() };
        let value = crate::app::expr_eval::eval_to_string(input).parse::<f64>();
        let Ok(value) = value else {
            self.command_line.push_error(crate::t!("Enter a finite action override.").as_ref());
            return Task::none();
        };
        let (multiplier, angle) = match field {
            "authoring_action_multiplier" => (value, before.angle_offset),
            "authoring_action_angle" => (before.distance_multiplier, value.to_radians()),
            _ => return Task::none(),
        };
        let mut staged = self.tabs[i].scene.document.clone();
        match options::set(&mut staged, definition, handle, multiplier, angle) {
            Ok(true) => {
                let staged = match crate::scene::block_actions::instance::refresh(&staged, definition) {
                    Ok(staged) => staged,
                    Err(error) => {
                        self.command_line.push_error(crate::t!(error).as_ref());
                        return Task::none();
                    }
                };
                self.push_undo_snapshot(i, "Edit block action override");
                self.apply_action_document(i, staged);
                self.tabs[i].dirty = true;
                self.tabs[i].scene.bump_geometry();
                self.finish_pending_history(i);
                self.tabs[i].properties.edit_buf.remove(&key);
                self.refresh_properties();
            }
            Ok(false) => { self.tabs[i].properties.edit_buf.remove(&key); }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }

    pub(super) fn commit_authoring_action_base(&mut self, field: &'static str) -> Task<Message> {
        use crate::scene::block_actions::{instance, options};
        let i = self.active_tab;
        let Some(session) = self.tabs[i].active_block_edit_session() else { return Task::none() };
        let definition = session.br_handle;
        let Some(handle) = self.tabs[i].selected_authoring else { return Task::none() };
        let Some(before) = options::base(&self.tabs[i].scene.document, handle) else { return Task::none() };
        let key = crate::ui::properties::FieldKey::Geom(field);
        let editing_offset = matches!(field, "authoring_action_base_offset_x" | "authoring_action_base_offset_y");
        let mut point = if editing_offset { DVec3::new(before.offset.x, before.offset.y, before.offset.z) }
            else { DVec3::new(before.base_point.x, before.base_point.y, before.base_point.z) };
        let dependent = if field == "authoring_action_base_dependent" { !before.dependent } else { before.dependent };
        let point = if field == "authoring_action_base_dependent" { None } else {
            if editing_offset != dependent { return Task::none() }
            let Some(input) = self.tabs[i].properties.edit_buf.get(&key) else { return Task::none() };
            let Ok(value) = crate::app::expr_eval::eval_to_string(input).parse::<f64>() else {
                self.command_line.push_error(crate::t!("Specify a finite action base point.").as_ref());
                return Task::none();
            };
            match field {
                "authoring_action_base_x" => point.x = value,
                "authoring_action_base_y" => point.y = value,
                "authoring_action_base_offset_x" => point.x = value,
                "authoring_action_base_offset_y" => point.y = value,
                _ => return Task::none(),
            }
            Some(point)
        };
        let mut staged = self.tabs[i].scene.document.clone();
        let result = if editing_offset {
            options::set_base_offset(&mut staged, definition, handle, point.unwrap())
        } else { options::set_base(&mut staged, definition, handle, dependent, point) };
        match result {
            Ok(true) => match instance::refresh(&staged, definition) {
                Ok(staged) => {
                    self.push_undo_snapshot(i, "Edit block action base point");
                    self.apply_action_document(i, staged);
                    self.tabs[i].dirty = true;
                    self.tabs[i].scene.bump_geometry();
                    self.finish_pending_history(i);
                    self.tabs[i].properties.edit_buf.remove(&key);
                    self.refresh_properties();
                }
                Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
            },
            Ok(false) => { self.tabs[i].properties.edit_buf.remove(&key); }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }

    fn authoring_label_corners(
        &self,
        graphic: &crate::scene::block_distance_parameter::Graphic,
        anchor: Point,
        label: &str,
        index: usize,
        camera: &crate::scene::view::camera::Camera,
        bounds: Rectangle,
    ) -> [Point; 4] {
        let prefs = &self.block_editor_preferences;
        let angle = if prefs.text_horizontal {
            0.0
        } else {
            graphic
                .dimensions
                .get(index)
                .or_else(|| graphic.dimensions.first())
                .and_then(|[a, b]| {
                    let a = camera.project(DVec3::new(a.x, a.y, a.z), bounds)?;
                    let b = camera.project(DVec3::new(b.x, b.y, b.z), bounds)?;
                    Some(crate::ui::overlay::authoring::readable_angle(
                        (b.y - a.y).atan2(b.x - a.x),
                    ))
                })
                .unwrap_or(0.0)
        };
        crate::ui::overlay::authoring::label_corners(
            anchor,
            label,
            angle,
            prefs.parameter_size,
            &prefs.parameter_font,
        )
    }

    pub(super) fn authoring_grip_hit(
        &self,
        point: Point,
        bounds: Rectangle,
    ) -> Option<(usize, usize, bool, DVec3)> {
        let tab = &self.tabs[self.active_tab];
        tab.active_block_edit_session()?;
        let camera = tab.scene.camera.borrow();
        let half = self.block_editor_preferences.grip_size.max(1) as f32 / 2.0 + 3.0;
        tab.selected_grips
            .iter()
            .enumerate()
            .filter(|(_, g)| is_editor_grip(g.id))
            .filter_map(|(index, g)| {
                let screen = camera.project(g.world, bounds)?;
                let dx = (screen.x - point.x).abs();
                let dy = (screen.y - point.y).abs();
                (dx <= half && dy <= half).then_some((dx.hypot(dy), index, g))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, index, g)| (index, g.id, g.is_midpoint, g.world))
    }
    pub(super) fn authoring_parameter_properties(
        &self,
        handle: Handle,
    ) -> Option<(
        bool,
        Vec<(
            &'static str,
            &'static str,
            &codec::objects::BlockParameterValueSet,
        )>,
    )> {
        let Some(codec::objects::ObjectType::DynamicBlock(object)) = self.tabs[self.active_tab]
            .scene
            .document
            .objects
            .get(&handle)
        else {
            return None;
        };
        let (chain_actions, value_sets) = match &object.data {
            codec::objects::DynamicBlockData::LinearParameter(parameter) => (
                parameter.parameter.parameter.chain_actions,
                vec![("linear_distance", "Distance", &parameter.value_set)],
            ),
            codec::objects::DynamicBlockData::PolarParameter(parameter) => (
                parameter.parameter.parameter.chain_actions,
                vec![
                    ("polar_distance", "Distance", &parameter.distance_value_set),
                    ("polar_angle", "Angle", &parameter.angle_value_set),
                ],
            ),
            codec::objects::DynamicBlockData::XYParameter(parameter) => (
                parameter.parameter.parameter.chain_actions,
                vec![
                    ("xy_x", "X Distance", &parameter.x_value_set),
                    ("xy_y", "Y Distance", &parameter.y_value_set),
                ],
            ),
            codec::objects::DynamicBlockData::RotationParameter(parameter) => (
                parameter.parameter.parameter.chain_actions,
                vec![("rotation_angle", "Angle", &parameter.value_set)],
            ),
            codec::objects::DynamicBlockData::PointParameter(parameter) => {
                (parameter.parameter.parameter.chain_actions, vec![])
            }
            codec::objects::DynamicBlockData::AlignmentParameter(parameter) => {
                (parameter.parameter.parameter.chain_actions, vec![])
            }
            codec::objects::DynamicBlockData::FlipParameter(parameter) => {
                (parameter.parameter.parameter.chain_actions, vec![])
            }
            codec::objects::DynamicBlockData::BasePointParameter(parameter) => {
                (parameter.parameter.parameter.chain_actions, vec![])
            }
            codec::objects::DynamicBlockData::LookupParameter(parameter) => {
                (parameter.parameter.parameter.chain_actions, vec![])
            }
            codec::objects::DynamicBlockData::UserParameter(parameter) => {
                (parameter.parameter.parameter.chain_actions, vec![])
            }
            _ => return None,
        };
        Some((chain_actions, value_sets))
    }

    pub(super) fn toggle_authoring_value_set_flag(&mut self, field: &'static str) -> Task<Message> {
        let i = self.active_tab;
        let Some(handle) = self.tabs[i].selected_authoring else {
            return Task::none();
        };
        let mut document = self.tabs[i].scene.document.clone();
        match toggle_authoring_value_set_flag(&mut document, handle, field) {
            Ok(true) => {
                self.push_undo_snapshot(i, "BEDIT Parameter value set");
                self.tabs[i].scene.document = document;
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].dirty = true;
                self.refresh_properties();
            }
            Ok(false) => {}
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }

    pub(super) fn commit_authoring_value_set(&mut self, field: &'static str) -> Task<Message> {
        let i = self.active_tab;
        let Some(handle) = self.tabs[i].selected_authoring else {
            return Task::none();
        };
        let key = crate::ui::properties::FieldKey::Geom(field);
        let Some(input) = self.tabs[i].properties.edit_buf.get(&key).cloned() else {
            return Task::none();
        };
        let mut document = self.tabs[i].scene.document.clone();
        match set_authoring_value_set_field(&mut document, handle, field, &input) {
            Ok(true) => {
                self.push_undo_snapshot(i, "BEDIT Parameter value set");
                self.tabs[i].scene.document = document;
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].dirty = true;
                self.tabs[i].properties.edit_buf.remove(&key);
                self.refresh_properties();
            }
            Ok(false) => {
                self.tabs[i].properties.edit_buf.remove(&key);
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }

    pub(super) fn commit_authoring_name(&mut self) -> Task<Message> {
        self.commit_authoring_metadata("authoring_name")
    }

    pub(super) fn commit_authoring_metadata(&mut self, key: &'static str) -> Task<Message> {
        use crate::scene::block_authoring::metadata;
        let i = self.active_tab;
        let Some(handle) = self.tabs[i].selected_authoring else {
            return Task::none();
        };
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            return Task::none();
        };
        let definition = session.br_handle;
        let mut staged = self.tabs[i].scene.document.clone();
        let result = if key == "authoring_show_properties" {
            metadata::read(&staged, definition, handle)
                .and_then(|m| {
                    m.visible
                        .ok_or("This parameter has no supported reference property row.")
                })
                .and_then(|visible| {
                    metadata::set_visible(&mut staged, definition, handle, !visible)
                })
        } else {
            let Some(field) = metadata::Field::from_key(key) else {
                return Task::none();
            };
            let Some(input) = self.tabs[i]
                .properties
                .edit_buf
                .get(&crate::ui::properties::FieldKey::Geom(key))
            else {
                return Task::none();
            };
            metadata::set_text(&mut staged, definition, handle, field, input)
        };
        match result {
            Ok(changed) => {
                if changed {
                    self.push_undo_snapshot(i, "Edit block parameter metadata");
                    self.tabs[i].scene.document = staged;
                    self.tabs[i].dirty = true;
                    self.tabs[i].scene.bump_geometry();
                    self.finish_pending_history(i);
                }
                self.tabs[i]
                    .properties
                    .edit_buf
                    .remove(&crate::ui::properties::FieldKey::Geom(key));
                self.refresh_properties();
            }
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        Task::none()
    }
    /// Select the authoring object a selection box catches. A box that also
    /// caught drawing objects keeps those instead (#2217): the authoring
    /// selection would clear them.
    pub(super) fn pick_authoring_box(
        &mut self,
        a: Point,
        b: Point,
        crossing: bool,
        caught_objects: bool,
    ) -> bool {
        let i = self.active_tab;
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            return false;
        };
        if caught_objects {
            return false;
        }
        let items = edit::items(&self.tabs[i].scene.document, session.br_handle);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: self.tabs[i].scene.selection.borrow().view.vp_size.0,
            height: self.tabs[i].scene.selection.borrow().view.vp_size.1,
        };
        let camera = self.tabs[i].scene.camera.borrow();
        let min = Point::new(a.x.min(b.x), a.y.min(b.y));
        let max = Point::new(a.x.max(b.x), a.y.max(b.y));
        let hit = items
            .into_iter()
            .filter(|item| {
                !self.block_editor_preferences.action_bars
                    || !crate::scene::block_action_bars::is_action(
                        &self.tabs[i].scene.document,
                        item.handle,
                    )
            })
            .filter_map(|item| {
                let mut projected = Vec::new();
                for (_, p) in &item.grips {
                    if let Some(p) = camera.project(DVec3::new(p.x, p.y, p.z), bounds) {
                        projected.push(Point::new(p.x, p.y));
                    }
                }
                let segments = item
                    .graphic
                    .lines
                    .iter()
                    .filter_map(|[a, b]| {
                        let a = camera.project(DVec3::new(a.x, a.y, a.z), bounds)?;
                        let b = camera.project(DVec3::new(b.x, b.y, b.z), bounds)?;
                        let segment = [Point::new(a.x, a.y), Point::new(b.x, b.y)];
                        projected.extend(segment);
                        Some(segment)
                    })
                    .collect::<Vec<_>>();
                let labels = item
                    .graphic
                    .labels
                    .iter()
                    .enumerate()
                    .filter_map(|(index, (point, label))| {
                        let point =
                            camera.project(DVec3::new(point.x, point.y, point.z), bounds)?;
                        let point = Point::new(point.x, point.y);
                        let corners = self.authoring_label_corners(
                            &item.graphic,
                            point,
                            label,
                            index,
                            &camera,
                            bounds,
                        );
                        projected.extend(corners);
                        Some(corners)
                    })
                    .collect::<Vec<_>>();
                let inside =
                    |p: Point| p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y;
                let crosses = |[a, b]: [Point; 2]| {
                    if inside(a) || inside(b) {
                        return true;
                    }
                    let (dx, dy) = (b.x - a.x, b.y - a.y);
                    let (mut enter, mut leave) = (0.0_f32, 1.0_f32);
                    for (p, q) in [
                        (-dx, a.x - min.x),
                        (dx, max.x - a.x),
                        (-dy, a.y - min.y),
                        (dy, max.y - a.y),
                    ] {
                        if p.abs() <= f32::EPSILON {
                            if q < 0.0 {
                                return false;
                            }
                            continue;
                        }
                        let t = q / p;
                        if p < 0.0 {
                            enter = enter.max(t);
                        } else {
                            leave = leave.min(t);
                        }
                        if enter > leave {
                            return false;
                        }
                    }
                    true
                };
                let any = projected.iter().any(|p| inside(*p))
                    || segments.iter().copied().any(crosses)
                    || labels.iter().any(|corners| {
                        corners
                            .iter()
                            .zip(corners.iter().cycle().skip(1))
                            .take(4)
                            .any(|(a, b)| crosses([*a, *b]))
                            || crate::ui::overlay::authoring::label_contains(*corners, min)
                    });
                let all = !projected.is_empty() && projected.iter().all(|p| inside(*p));
                (if crossing { any } else { all }).then_some(item.handle)
            })
            .next();
        drop(camera);
        if let Some(handle) = hit {
            self.tabs[i].selected_authoring = Some(handle);
            self.tabs[i].scene.deselect_all();
            self.refresh_properties();
            true
        } else {
            false
        }
    }
    pub(super) fn authoring_label_at(
        &self,
        p: Point,
        bounds: Rectangle,
    ) -> Option<(Handle, String)> {
        let tab = &self.tabs[self.active_tab];
        let session = tab.active_block_edit_session()?;
        let camera = tab.scene.camera.borrow();
        edit::items(&tab.scene.document, session.br_handle)
            .into_iter()
            .filter(|item| {
                !self.block_editor_preferences.action_bars
                    || !crate::scene::block_action_bars::is_action(&tab.scene.document, item.handle)
            })
            .find_map(|item| {
                item.graphic
                    .labels
                    .iter()
                    .enumerate()
                    .find_map(|(index, (point, label))| {
                        let projected =
                            camera.project(DVec3::new(point.x, point.y, point.z), bounds)?;
                        let anchor = Point::new(projected.x, projected.y);
                        crate::ui::overlay::authoring::label_contains(
                            self.authoring_label_corners(
                                &item.graphic,
                                anchor,
                                label,
                                index,
                                &camera,
                                bounds,
                            ),
                            p,
                        )
                        .then(|| (item.handle, label.clone()))
                    })
            })
    }

    pub(super) fn authoring_name_is_editable(&self, handle: Handle) -> bool {
        match self.tabs[self.active_tab]
            .scene
            .document
            .objects
            .get(&handle)
        {
            Some(codec::objects::ObjectType::BlockVisibilityParameter(_)) => true,
            Some(codec::objects::ObjectType::DynamicBlock(object)) => matches!(
                object.data,
                codec::objects::DynamicBlockData::PointParameter(_)
                    | codec::objects::DynamicBlockData::LinearParameter(_)
                    | codec::objects::DynamicBlockData::PolarParameter(_)
                    | codec::objects::DynamicBlockData::XYParameter(_)
                    | codec::objects::DynamicBlockData::RotationParameter(_)
                    | codec::objects::DynamicBlockData::AlignmentParameter(_)
                    | codec::objects::DynamicBlockData::FlipParameter(_)
                    | codec::objects::DynamicBlockData::BasePointParameter(_)
                    | codec::objects::DynamicBlockData::LookupParameter(_)
            ),
            _ => false,
        }
    }

    pub(super) fn pick_authoring_object(&mut self, p: Point, bounds: Rectangle) -> bool {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_cmd.is_some() || tab.active_grip.is_some() {
            return false;
        }
        let Some(session) = tab.active_block_edit_session() else {
            return false;
        };
        let items = edit::items(&tab.scene.document, session.br_handle);
        let camera = tab.scene.camera.borrow();
        let project = |v: codec::types::Vector3| {
            camera
                .project(DVec3::new(v.x, v.y, v.z), bounds)
                .map(|v| Point::new(v.x, v.y))
        };
        let segment = |a: Point, b: Point| {
            let ab = glam::Vec2::new(b.x - a.x, b.y - a.y);
            let ap = glam::Vec2::new(p.x - a.x, p.y - a.y);
            let t = (ap.dot(ab) / ab.length_squared().max(1e-12)).clamp(0., 1.);
            (ap - ab * t).length()
        };
        let hit = items
            .iter()
            .filter(|item| {
                !self.block_editor_preferences.action_bars
                    || !crate::scene::block_action_bars::is_action(&tab.scene.document, item.handle)
            })
            .filter_map(|item| {
                let mut d = f32::INFINITY;
                for (_, v) in &item.grips {
                    if let Some(v) = project(*v) {
                        let distance = segment(v, v);
                        if distance <= self.block_editor_preferences.grip_size as f32 / 2.0 + 3.0 {
                            d = d.min(0.0);
                        }
                    }
                }
                for [a, b] in &item.graphic.lines {
                    if let (Some(a), Some(b)) = (project(*a), project(*b)) {
                        d = d.min(segment(a, b));
                    }
                }
                for (index, (v, label)) in item.graphic.labels.iter().enumerate() {
                    if let Some(v) = project(*v) {
                        if crate::ui::overlay::authoring::label_contains(
                            self.authoring_label_corners(
                                &item.graphic,
                                v,
                                label,
                                index,
                                &camera,
                                bounds,
                            ),
                            p,
                        ) {
                            d = d.min(0.);
                        }
                    }
                }
                (d <= 10.0).then_some((d, item.handle, item.name.clone()))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        drop(camera);
        if let Some((_, handle, name)) = hit {
            if self.shift_down && self.tabs[i].selected_authoring == Some(handle) {
                self.tabs[i].selected_authoring = None;
            } else {
                self.tabs[i].selected_authoring = Some(handle);
            }
            self.tabs[i].scene.deselect_all();
            self.tabs[i]
                .scene
                .selection
                .borrow_mut()
                .clear_left_selection_gesture();
            self.refresh_properties();
            self.command_line.push_info(&format!(
                "{} — {}",
                name,
                crate::t!("Select a grip to edit its location.")
            ));
            true
        } else {
            if self.tabs[i].selected_authoring.take().is_some() {
                self.refresh_properties();
            }
            false
        }
    }
    pub(super) fn refresh_authoring_grips(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        let Some(handle) = self.tabs[i].selected_authoring else {
            return;
        };
        if self.block_editor_preferences.action_bars
            && crate::scene::block_action_bars::is_action(&self.tabs[i].scene.document, handle)
        {
            return;
        }
        let item = self.tabs[i].active_block_edit_session().and_then(|s| {
            edit::items(&self.tabs[i].scene.document, s.br_handle)
                .into_iter()
                .find(|p| p.handle == handle)
        });
        let Some(item) = item.filter(|_| self.tabs[i].scene.selected.is_empty()) else {
            self.tabs[i].selected_authoring = None;
            return;
        };
        for (index, (_, point)) in item.grips.into_iter().enumerate() {
            self.tabs[i]
                .selected_grips
                .push(crate::scene::model::object::GripDef {
                    id: EDIT_GRIP_START + index,
                    world: DVec3::new(point.x - wo[0], point.y - wo[1], point.z - wo[2]),
                    is_midpoint: false,
                    shape: crate::scene::model::object::GripShape::Square,
                    dir: None,
                    axis: None,
                });
            self.tabs[i].selected_grip_handles.push(handle);
        }
    }
    pub(super) fn start_authoring_grip(&mut self, handle: Handle, id: usize) -> bool {
        if !is_editor_grip(id) {
            return false;
        }
        let i = self.active_tab;
        if self.block_editor_preferences.action_bars
            && crate::scene::block_action_bars::is_action(&self.tabs[i].scene.document, handle)
        {
            return false;
        }
        let Some(session) = self.tabs[i].active_block_edit_session() else {
            return false;
        };
        let Some(part) = edit::items(&self.tabs[i].scene.document, session.br_handle)
            .into_iter()
            .find(|p| p.handle == handle)
            .and_then(|p| p.grips.get(id - EDIT_GRIP_START).map(|(part, _)| *part))
        else {
            return false;
        };
        let command = EditCommand {
            document: self.tabs[i].id,
            definition: session.br_handle,
            handle,
            part,
            document_snapshot: self.tabs[i].scene.document.clone(),
        };
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }
    pub(super) fn handle_authoring_edit(&mut self, result: CmdResult) -> Task<Message> {
        let CmdResult::EditBlockAuthoring {
            document,
            definition,
            handle,
            part,
            position,
        } = result
        else {
            unreachable!()
        };
        let i = self.active_tab;
        if self.tabs[i].id != document
            || self.tabs[i]
                .active_block_edit_session()
                .map(|s| s.br_handle)
                != Some(definition)
        {
            self.command_line
                .push_error(crate::t!("The Block Editor session is no longer active.").as_ref());
            return Task::none();
        }
        let mut staged = self.tabs[i].scene.document.clone();
        match edit::move_part(
            &mut staged,
            definition,
            handle,
            part,
            codec::types::Vector3::new(position.x, position.y, position.z),
        ) {
            Ok(changed) => {
                if changed {
                    self.push_undo_snapshot(i, "Block authoring grip");
                    // Keep the live entity-change recorder attached. Basepoint edits
                    // can also relocate INSERT attributes outside the edited block.
                    let doc = &mut self.tabs[i].scene.document;
                    let changed_entities = staged
                        .entities()
                        .filter(|e| doc.get_entity(e.common().handle) != Some(*e))
                        .cloned()
                        .collect::<Vec<_>>();
                    for entity in changed_entities {
                        let handle = entity.common().handle;
                        *doc.get_entity_mut(handle)
                            .expect("authoring edits retain entity identities") = entity;
                    }
                    for (h, o) in staged.objects.iter() {
                        if doc.objects.get(h) != Some(o) {
                            doc.objects.insert(*h, o.clone());
                        }
                    }
                    doc.block_visibility_params = staged.block_visibility_params.clone();
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                    self.finish_pending_history(i);
                }
                self.tabs[i].active_cmd = None;
                self.tabs[i].scene.clear_preview_wire();
                self.refresh_properties();
            }
            Err(e) => self.command_line.push_error(crate::t!(e).as_ref()),
        }
        Task::none()
    }
}

fn authoring_value_set_mut<'a>(
    data: &'a mut codec::objects::DynamicBlockData,
    key: &str,
) -> Option<&'a mut codec::objects::BlockParameterValueSet> {
    use codec::objects::DynamicBlockData as Data;
    match (data, key) {
        (Data::LinearParameter(parameter), "linear_distance") => Some(&mut parameter.value_set),
        (Data::PolarParameter(parameter), "polar_distance") => {
            Some(&mut parameter.distance_value_set)
        }
        (Data::PolarParameter(parameter), "polar_angle") => Some(&mut parameter.angle_value_set),
        (Data::XYParameter(parameter), "xy_x") => Some(&mut parameter.x_value_set),
        (Data::XYParameter(parameter), "xy_y") => Some(&mut parameter.y_value_set),
        (Data::RotationParameter(parameter), "rotation_angle") => Some(&mut parameter.value_set),
        _ => None,
    }
}

fn set_authoring_value_set_field(
    document: &mut codec::Document,
    handle: Handle,
    field: &str,
    input: &str,
) -> Result<bool, &'static str> {
    let rest = field
        .strip_prefix("authoring_set_")
        .ok_or("Unknown parameter value-set field.")?;
    let (set_key, member) = rest
        .rsplit_once('_')
        .ok_or("Unknown parameter value-set field.")?;
    let Some(codec::objects::ObjectType::DynamicBlock(object)) = document.objects.get_mut(&handle)
    else {
        return Err("Select an authoring parameter.");
    };
    let Some(value_set) = authoring_value_set_mut(&mut object.data, set_key) else {
        return Err("This parameter does not have that value set.");
    };
    let changed = match member {
        "minimum" | "maximum" | "increment" => {
            let value = input
                .trim()
                .parse::<f64>()
                .map_err(|_| "Enter a finite number for the parameter value set.")?;
            if !value.is_finite() {
                return Err("Enter a finite number for the parameter value set.");
            }
            match member {
                "minimum" if value != value_set.minimum => {
                    value_set.minimum = value;
                    value_set.flags |= 1;
                }
                "maximum" if value != value_set.maximum => {
                    value_set.maximum = value;
                    value_set.flags |= 2;
                }
                "increment" if value < 0.0 => {
                    return Err("Parameter value-set increment cannot be negative.");
                }
                "increment" if value != value_set.increment => {
                    value_set.increment = value;
                    if value == 0.0 {
                        value_set.flags &= !4;
                    } else {
                        value_set.flags = (value_set.flags | 4) & !8;
                    }
                }
                _ => return Ok(false),
            }
            if value_set.flags & 3 == 3 && value_set.minimum > value_set.maximum {
                return Err("Parameter value-set minimum cannot exceed its maximum.");
            }
            true
        }
        "values" => {
            let values = if input.trim().is_empty() {
                Vec::new()
            } else {
                input
                    .split(',')
                    .map(|item| {
                        let value = item
                            .trim()
                            .parse::<f64>()
                            .map_err(|_| "Enter allowed values as comma-separated numbers.")?;
                        if !value.is_finite() {
                            return Err("Allowed parameter values must be finite.");
                        }
                        Ok(value)
                    })
                    .collect::<Result<Vec<_>, _>>()?
            };
            if values.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err("Allowed parameter values must be in strictly increasing order.");
            }
            if values == value_set.values {
                return Ok(false);
            }
            value_set.values = values;
            if value_set.values.is_empty() {
                value_set.flags &= !8;
            } else {
                value_set.flags = (value_set.flags | 8) & !4;
            }
            true
        }
        _ => return Err("Unknown parameter value-set field."),
    };
    Ok(changed)
}

fn toggle_authoring_value_set_flag(
    document: &mut codec::Document,
    handle: Handle,
    field: &str,
) -> Result<bool, &'static str> {
    let rest = field
        .strip_prefix("authoring_set_")
        .and_then(|field| field.strip_suffix("_enabled"))
        .ok_or("Unknown parameter value-set option.")?;
    let (set_key, member) = rest
        .rsplit_once('_')
        .ok_or("Unknown parameter value-set option.")?;
    let bit = match member {
        "minimum" => 1,
        "maximum" => 2,
        "increment" => 4,
        "values" => 8,
        _ => return Err("Unknown parameter value-set option."),
    };
    let Some(codec::objects::ObjectType::DynamicBlock(object)) = document.objects.get_mut(&handle)
    else {
        return Err("Select an authoring parameter.");
    };
    let Some(value_set) = authoring_value_set_mut(&mut object.data, set_key) else {
        return Err("This parameter does not have that value set.");
    };
    if bit == 8 && value_set.values.is_empty() {
        return Err("Add at least one allowed value before enabling the value list.");
    }
    let enabled = value_set.flags & bit == 0;
    if enabled && bit == 4 && value_set.increment <= 0.0 {
        return Err("Enter a positive increment before enabling the increment.");
    }
    if enabled {
        value_set.flags |= bit;
        if bit == 4 {
            value_set.flags &= !8;
        } else if bit == 8 {
            value_set.flags &= !4;
        }
    } else {
        value_set.flags &= !bit;
    }
    if value_set.flags & 3 == 3 && value_set.minimum > value_set.maximum {
        return Err("Parameter value-set minimum cannot exceed its maximum.");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_basepoint_preview_follows_materialized_parameter() {
        let mut scene = crate::scene::Scene::new();
        scene.define_block_raw(
            "PREVIEW_BASEPOINT",
            codec::types::Vector3::new(2.0, 3.0, 0.0),
            vec![codec::EntityType::Line(codec::entities::Line::from_points(
                codec::types::Vector3::ZERO,
                codec::types::Vector3::new(1.0, 0.0, 0.0),
            ))],
        );
        let definition = scene
            .document
            .block_records
            .get("PREVIEW_BASEPOINT")
            .unwrap()
            .handle;
        assert!(crate::scene::block_basepoint::parameter(&scene.document, definition).is_none());
        let mut command = EditCommand {
            document: 1,
            definition,
            handle: definition,
            part: Part::Point,
            document_snapshot: scene.document.clone(),
        };

        let preview = command.on_preview_wires(DVec3::new(4.0, 5.0, 0.0));

        assert!(!preview.is_empty());
    }
}
