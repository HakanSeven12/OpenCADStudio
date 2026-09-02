// Kernel B-rep solid modelling and exact ACIS persistence.

use acadrust::{
    entities::Solid3D, objects::SolidHistoryOperation, EntityType, Handle,
};
use cadkernel::brep::Body;
use iced::Task;

use super::Message;
use crate::modules::model::boolean_cmd::BoolOp;
use crate::scene::model::solid_history;
use crate::scene::model::solid_model::{self, Bool};

impl super::OpenCADStudio {
    /// Add a solid and register its persistent B-rep.
    pub(super) fn add_solid_model(
        &mut self,
        mut entity: EntityType,
        solid: Body,
        history: SolidHistoryOperation,
    ) -> Handle {
        let i = self.active_tab;
        let EntityType::Solid3D(inner) = &mut entity else {
            return Handle::NULL;
        };
        inner.common.plotstyle_flags = 2;
        inner.wires = solid_model::edge_wires(&solid);
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&solid)
        else {
            self.command_line
                .push_error(crate::t!("The solid could not be encoded as ACIS.").as_ref());
            return Handle::NULL;
        };
        inner.set_sat_document(&document);
        let Some(handle) = self.commit_entity_handle(entity) else {
            return Handle::NULL;
        };
        self.tabs[i].scene.create_solid_history(handle, history);
        self.tabs[i].scene.register_solid_model(handle, solid);
        handle
    }

    /// Add an open sheet body as a persistent Surface entity and register its
    /// exact B-rep for shaded and wireframe display.
    pub(super) fn add_surface_model(
        &mut self,
        mut entity: EntityType,
        surface: Body,
    ) -> Handle {
        let i = self.active_tab;
        let EntityType::Surface(inner) = &mut entity else {
            return Handle::NULL;
        };
        inner.common.plotstyle_flags = 2;
        inner.wires = solid_model::edge_wires(&surface);
        let Some(document) = crate::scene::convert::acis_export::solid_to_sat(&surface)
        else {
            self.command_line
                .push_error(crate::t!("The surface could not be encoded as ACIS.").as_ref());
            return Handle::NULL;
        };
        inner.acis_data = acadrust::entities::AcisData::from_sat(&document.to_sat_string());
        let Some(handle) = self.commit_entity_handle(entity) else {
            return Handle::NULL;
        };
        self.tabs[i].scene.register_solid_model(handle, surface);
        handle
    }

    fn selected_solid_handles(&mut self) -> Vec<Handle> {
        let i = self.active_tab;
        let mut handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected_handles_in_order()
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .filter(|handle| {
                matches!(
                    self.tabs[i].scene.document.get_entity(*handle),
                    Some(EntityType::Solid3D(_))
                )
            })
            .collect();
        self.tabs[i].scene.restore_solid_models(&handles);
        handles.retain(|handle| self.tabs[i].scene.solid_models.contains_key(handle));
        handles
    }

    fn replace_solid_body(&mut self, handle: Handle, result: Body, label: &str) -> bool {
        let i = self.active_tab;
        let Some(document) =
            crate::scene::convert::acis_export::solid_to_sat(&result)
        else {
            self.command_line
                .push_error(crate::t!("The result could not be encoded as ACIS.").as_ref());
            return false;
        };
        let Some(EntityType::Solid3D(mut entity)) =
            self.tabs[i].scene.document.get_entity(handle).cloned()
        else {
            return false;
        };
        entity.wires = solid_model::edge_wires(&result);
        entity.silhouettes.clear();
        entity.history_handle = None;
        entity.set_sat_document(&document);

        self.push_undo_snapshot(i, label);
        self.tabs[i].scene.delete_solid_history(handle);
        if !self.tabs[i]
            .scene
            .update_entity(EntityType::Solid3D(entity))
        {
            self.command_line
                .push_error(crate::t!("The solid could not be updated.").as_ref());
            return false;
        }
        let history = solid_history::brep_op(&result);
        self.tabs[i].scene.create_solid_history(handle, history);
        self.tabs[i].scene.register_solid_model(handle, result);
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.select_entity(handle, false);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        true
    }

    pub(super) fn solid_edge_blend(
        &mut self,
        handle: Handle,
        pick: glam::DVec3,
        value: f64,
        fillet: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Task::none();
        }
        if !matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(EntityType::Solid3D(_))
        ) {
            self.command_line
                .push_error(crate::t!("Select a 3D solid edge.").as_ref());
            return Task::none();
        }
        self.tabs[i].scene.restore_solid_models(&[handle]);
        let Some(body) = self.tabs[i].scene.solid_models.get(&handle).cloned() else {
            self.command_line
                .push_error(crate::t!("The solid geometry could not be restored.").as_ref());
            return Task::none();
        };
        let Some(edge) = solid_model::nearest_edge(&body, pick.to_array()) else {
            self.command_line
                .push_error(crate::t!("Select a solid edge.").as_ref());
            return Task::none();
        };
        let result = if fillet {
            cadkernel::brep::fillet(&body, edge, value)
        } else {
            cadkernel::brep::chamfer(&body, edge, value)
        };
        let Some(result) = result else {
            self.command_line.push_error(
                crate::t!("Edge operation failed. Use a convex planar solid and a smaller value.")
                    .as_ref(),
            );
            return Task::none();
        };
        let label = if fillet { "SOLIDFILLET" } else { "SOLIDCHAMFER" };
        if self.replace_solid_body(handle, result, label) {
            self.command_line
                .push_output(crate::tf!("{label}: solid updated.").as_ref());
        }
        Task::none()
    }

    pub(super) fn solid_face_presspull(
        &mut self,
        handle: Handle,
        pick: glam::DVec3,
        distance: f64,
        drag: Option<glam::DVec3>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Task::none();
        }
        self.tabs[i].scene.restore_solid_models(&[handle]);
        let Some(body) = self.tabs[i].scene.solid_models.get(&handle).cloned() else {
            self.command_line
                .push_error(crate::t!("The solid geometry could not be restored.").as_ref());
            return Task::none();
        };
        let Some(face) = solid_model::nearest_planar_face(&body, pick.to_array()) else {
            self.command_line
                .push_error(crate::t!("Select a planar solid face.").as_ref());
            return Task::none();
        };
        let distance = match drag {
            Some(point) => {
                let Some(normal) = solid_model::planar_face_normal(&body, face) else {
                    self.command_line
                        .push_error(crate::t!("Select a planar solid face.").as_ref());
                    return Task::none();
                };
                (point - pick).dot(glam::DVec3::from_array(normal))
            }
            None => distance,
        };
        if !distance.is_finite() || distance.abs() <= 1e-6 {
            self.command_line.push_error(
                crate::t!("PRESSPULL: drag along the selected face normal.").as_ref(),
            );
            return Task::none();
        }
        let Some(result) = cadkernel::brep::presspull(&body, face, distance) else {
            self.command_line.push_error(
                crate::t!("PRESSPULL: the face move would collapse or invalidate the solid.")
                    .as_ref(),
            );
            return Task::none();
        };
        if self.replace_solid_body(handle, result, "PRESSPULL") {
            self.command_line
                .push_output(crate::t!("PRESSPULL: solid updated.").as_ref());
        }
        Task::none()
    }

    /// Run a boolean over the selected solids in selection order.
    pub(super) fn solid_boolean(&mut self, op: BoolOp) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() < 2 {
            self.command_line
                .push_error(crate::t!("Boolean: select at least two solids.").as_ref());
            return Task::none();
        }
        let kind = match op {
            BoolOp::Union => Bool::Union,
            BoolOp::Subtract => Bool::Subtract,
            BoolOp::Intersect => Bool::Intersect,
        };
        let mut operands = handles
            .iter()
            .filter_map(|handle| self.tabs[i].scene.solid_models.get(handle).cloned());
        let mut result = operands.next().expect("at least two restored solids");
        for operand in operands {
            let Some(combined) = solid_model::boolean(kind, &result, &operand) else {
                self.command_line.push_error(
                    crate::t!("Boolean failed while combining the selected solids.").as_ref(),
                );
                return Task::none();
            };
            result = combined;
        }
        if crate::scene::convert::acis_export::solid_to_sat(&result).is_none() {
            self.command_line
                .push_error(crate::t!("The boolean result could not be encoded as ACIS.").as_ref());
            return Task::none();
        }

        self.push_undo_snapshot(i, "BOOLEAN");
        self.tabs[i].scene.erase_entities(&handles);
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&result);
        let history = solid_history::brep_op(&result);
        let handle = self.add_solid_model(EntityType::Solid3D(s3d), result, history);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        Task::none()
    }

    pub(super) fn solid_subtract(
        &mut self,
        bases: &[Handle],
        cutters: &[Handle],
    ) -> Task<Message> {
        let i = self.active_tab;
        let valid_solid = |handle: &Handle| {
            !self.tabs[i].scene.is_layer_locked(*handle)
                && matches!(
                    self.tabs[i].scene.document.get_entity(*handle),
                    Some(EntityType::Solid3D(_))
                )
        };
        let mut base_handles: Vec<_> = bases.iter().copied().filter(valid_solid).collect();
        let mut cutter_handles: Vec<_> = cutters.iter().copied().filter(valid_solid).collect();
        cutter_handles.retain(|handle| !base_handles.contains(handle));

        let mut operands = base_handles.clone();
        operands.extend(cutter_handles.iter().copied());
        self.tabs[i].scene.restore_solid_models(&operands);
        base_handles.retain(|handle| self.tabs[i].scene.solid_models.contains_key(handle));
        cutter_handles.retain(|handle| self.tabs[i].scene.solid_models.contains_key(handle));
        if base_handles.is_empty() || cutter_handles.is_empty() {
            self.command_line.push_error(
                crate::t!("SUBTRACT: select at least one base solid and one cutter solid.")
                    .as_ref(),
            );
            return Task::none();
        }

        let mut result = self.tabs[i].scene.solid_models[&base_handles[0]].clone();
        for handle in &base_handles[1..] {
            let operand = &self.tabs[i].scene.solid_models[handle];
            let Some(combined) = solid_model::boolean(Bool::Union, &result, operand) else {
                self.command_line.push_error(
                    crate::t!("SUBTRACT failed while combining the base solids.").as_ref(),
                );
                return Task::none();
            };
            result = combined;
        }
        for handle in &cutter_handles {
            let operand = &self.tabs[i].scene.solid_models[handle];
            let Some(difference) = solid_model::boolean(Bool::Subtract, &result, operand) else {
                self.command_line.push_error(
                    crate::t!("SUBTRACT failed while removing the selected solids.").as_ref(),
                );
                return Task::none();
            };
            result = difference;
        }
        if crate::scene::convert::acis_export::solid_to_sat(&result).is_none() {
            self.command_line
                .push_error(crate::t!("The boolean result could not be encoded as ACIS.").as_ref());
            return Task::none();
        }

        operands = base_handles;
        operands.extend(cutter_handles);
        self.push_undo_snapshot(i, "SUBTRACT");
        self.tabs[i].scene.erase_entities(&operands);
        let mut entity = Solid3D::new();
        entity.wires = solid_model::edge_wires(&result);
        let history = solid_history::brep_op(&result);
        let handle = self.add_solid_model(EntityType::Solid3D(entity), result, history);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        Task::none()
    }

    /// Slice the one selected solid with an axis-aligned plane (axis 0/1/2 =
    /// X/Y/Z at `value`), keeping the lower side when `keep_low` is true. The
    /// kept half is the intersection of the solid with a half-space box, reusing
    /// the same boolean path as the modelling tools.
    pub(super) fn solid_slice(&mut self, axis: usize, value: f64, keep_low: bool) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("SLICE: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        // Bounding box from the solid's edge wires.
        let Some((min, max)) = solid_model::extent(&solid) else {
            self.command_line
                .push_error(crate::t!("SLICE: could not determine the solid's extent.").as_ref());
            return Task::none();
        };
        // Generous margin so the box fully spans the solid in the free axes.
        let m = [
            (max[0] - min[0]).max(1.0),
            (max[1] - min[1]).max(1.0),
            (max[2] - min[2]).max(1.0),
        ];
        let mut lo = [min[0] - m[0], min[1] - m[1], min[2] - m[2]];
        let mut hi = [max[0] + m[0], max[1] + m[1], max[2] + m[2]];
        if keep_low {
            hi[axis] = value;
        } else {
            lo[axis] = value;
        }
        if hi[axis] <= lo[axis] {
            self.command_line
                .push_error(crate::t!("SLICE: the plane does not cross the solid on the kept side.").as_ref());
            return Task::none();
        }
        let center = [
            (lo[0] + hi[0]) / 2.0,
            (lo[1] + hi[1]) / 2.0,
            (lo[2] + hi[2]) / 2.0,
        ];
        let halfspace = solid_model::box_solid(center, hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
        let Some(result) = halfspace
            .as_ref()
            .and_then(|half| solid_model::boolean(Bool::Intersect, &solid, half))
        else {
            self.command_line
                .push_error(crate::t!("SLICE failed — the plane may not cross the solid.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "SLICE");
        self.tabs[i].scene.erase_entities(&handles);
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&result);
        let history = solid_history::brep_op(&result);
        let handle = self.add_solid_model(EntityType::Solid3D(s3d), result, history);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        let ax = ["X", "Y", "Z"][axis];
        self.command_line.push_output(crate::tf!(
            "SLICE: cut at {ax}={value}, kept the {} half.",
            if keep_low { "lower" } else { "upper" }
        ).as_ref());
        Task::none()
    }

    /// INTERFERE — create a solid from the overlap of the two selected solids,
    /// leaving the originals in place (a non-destructive boolean intersect).
    pub(super) fn solid_interfere(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 2 {
            self.command_line
                .push_error(crate::t!("INTERFERE: select exactly two solids created this session.").as_ref());
            return Task::none();
        }
        let a = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let b = self.tabs[i].scene.solid_models[&handles[1]].clone();
        match solid_model::boolean(Bool::Intersect, &a, &b) {
            Some(result) => {
                self.push_undo_snapshot(i, "INTERFERE");
                // Keep both originals; add the interference solid.
                let mut s3d = Solid3D::new();
                s3d.wires = solid_model::edge_wires(&result);
                let history = solid_history::brep_op(&result);
                self.add_solid_model(EntityType::Solid3D(s3d), result, history);
                self.tabs[i].dirty = true;
                self.refresh_properties();
                self.command_line
                    .push_output(crate::t!("INTERFERE: created an interference solid from the overlap.").as_ref());
            }
            None => self
                .command_line
                .push_output(crate::t!("INTERFERE: the selected solids do not overlap.").as_ref()),
        }
        Task::none()
    }

    /// 3DROTATE — rotate the one selected solid about the X/Y/Z axis (0/1/2)
    /// through its centre by `angle_deg` degrees. Rotation preserves the solid's
    /// orientation, so it reuses the cached B-rep directly.
    pub(super) fn solid_rotate3d(&mut self, axis: usize, angle_deg: f64) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("3DROTATE: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let Some(middle) = solid_model::centre(&solid) else {
            self.command_line
                .push_error(crate::t!("3DROTATE: could not determine the solid's extent.").as_ref());
            return Task::none();
        };
        let Some(rotated) =
            solid_model::turned(&solid, axis, angle_deg.to_radians(), middle)
        else {
            self.command_line
                .push_error(crate::t!("3DROTATE: could not turn the solid.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "3DROTATE");
        self.tabs[i].scene.erase_entities(&handles);
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&rotated);
        let history = solid_history::brep_op(&rotated);
        let handle = self.add_solid_model(EntityType::Solid3D(s3d), rotated, history);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(crate::tf!(
            "3DROTATE: rotated {angle_deg}° about the {} axis.",
            ["X", "Y", "Z"][axis]
        ).as_ref());
        Task::none()
    }

    /// 3DMIRROR — add a mirrored copy of the one selected solid across the plane
    /// perpendicular to the X/Y/Z axis (0/1/2) through its centre, keeping the
    /// original. A reflection loses handedness, so the kernel reverses every
    /// loop on the way through; without that the copy lights black.
    pub(super) fn solid_mirror3d(&mut self, axis: usize) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("3DMIRROR: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let Some(middle) = solid_model::centre(&solid) else {
            self.command_line
                .push_error(crate::t!("3DMIRROR: could not determine the solid's extent.").as_ref());
            return Task::none();
        };
        let Some(reflected) = solid_model::mirrored(&solid, axis, middle) else {
            self.command_line
                .push_error(crate::t!("3DMIRROR: could not mirror the solid.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "3DMIRROR");
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&reflected);
        let history = solid_history::brep_op(&reflected);
        let h = self.add_solid_model(EntityType::Solid3D(s3d), reflected, history);
        self.tabs[i].scene.deselect_all();
        if !h.is_null() {
            self.tabs[i].scene.select_entity(h, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(crate::tf!(
            "3DMIRROR: added a mirror across the {} plane.",
            ["X", "Y", "Z"][axis]
        ).as_ref());
        Task::none()
    }

    /// 3DALIGN — move/rotate the one selected solid so its three source points
    /// land on the three destination points. The frame-to-frame transform is
    /// computed in glam (`M = D · S⁻¹`); both frames are right-handed, so the
    /// result is a pure rotation and translation and the kernel accepts it.
    pub(super) fn solid_align3d(
        &mut self,
        src: [[f64; 3]; 3],
        dst: [[f64; 3]; 3],
    ) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("3DALIGN: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        // Build a right-handed frame (origin + orthonormal axes) from 3 points.
        let frame = |p: [[f64; 3]; 3]| -> Option<glam::DMat4> {
            let p1 = glam::DVec3::from_array(p[0]);
            let p2 = glam::DVec3::from_array(p[1]);
            let p3 = glam::DVec3::from_array(p[2]);
            let x = (p2 - p1).normalize_or_zero();
            let z = (p2 - p1).cross(p3 - p1).normalize_or_zero();
            if x.length_squared() < 1e-12 || z.length_squared() < 1e-12 {
                return None; // coincident or collinear points
            }
            let y = z.cross(x);
            Some(glam::DMat4::from_cols(
                x.extend(0.0),
                y.extend(0.0),
                z.extend(0.0),
                p1.extend(1.0),
            ))
        };
        let (Some(s), Some(d)) = (frame(src), frame(dst)) else {
            self.command_line
                .push_error(crate::t!("3DALIGN: each point triple must be non-coincident and non-collinear.").as_ref());
            return Task::none();
        };
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let Some(aligned) = solid_model::by_matrix(&solid, (d * s.inverse()).to_cols_array())
        else {
            self.command_line
                .push_error(crate::t!("3DALIGN: could not align the solid.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "3DALIGN");
        self.tabs[i].scene.erase_entities(&handles);
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&aligned);
        let history = solid_history::brep_op(&aligned);
        let h = self.add_solid_model(EntityType::Solid3D(s3d), aligned, history);
        self.tabs[i].scene.deselect_all();
        if !h.is_null() {
            self.tabs[i].scene.select_entity(h, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::t!("3DALIGN: aligned the solid to the destination points.").as_ref());
        Task::none()
    }

    /// SECTION — draw the cross-section outline where an axis-aligned plane
    /// (X/Y/Z = `axis` at `value`) cuts the one selected solid, as Line
    /// entities.
    pub(super) fn solid_section(&mut self, axis: usize, value: f64) -> Task<Message> {
        use acadrust::types::Vector3;
        use acadrust::Line;

        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.len() != 1 {
            self.command_line
                .push_error(crate::t!("SECTION: select exactly one solid created this session.").as_ref());
            return Task::none();
        }
        let solid = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let Some((min, max)) = solid_model::extent(&solid) else {
            self.command_line
                .push_error(crate::t!("SECTION: could not determine the solid's extent.").as_ref());
            return Task::none();
        };
        // The plane has to actually reach the solid to cut it.
        if value < min[axis] || value > max[axis] {
            self.command_line
                .push_output(crate::t!("SECTION: the plane does not cross the solid.").as_ref());
            return Task::none();
        }
        let segs = solid_model::section(&solid, axis, value);
        if segs.is_empty() {
            self.command_line
                .push_output(crate::t!("SECTION: the plane does not cross the solid.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, "SECTION");
        for (p1, p2) in &segs {
            let line = Line::from_points(
                Vector3::new(p1[0], p1[1], p1[2]),
                Vector3::new(p2[0], p2[1], p2[2]),
            );
            self.tabs[i].scene.add_entity(EntityType::Line(line));
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(crate::tf!(
            "SECTION: created {} section line(s) at {}={value}.",
            segs.len(),
            ["X", "Y", "Z"][axis]
        ).as_ref());
        Task::none()
    }

    /// PYRAMID — create an `n`-sided pyramid: a regular polygon base of the
    /// given circumradius with its apex at `height`.
    ///
    /// A real B-rep rather than a bag of faces, so it joins the boolean tools
    /// and the exact-geometry save path like every other primitive.
    pub(super) fn solid_pyramid(
        &mut self,
        radius: f64,
        height: f64,
        sides: usize,
    ) -> Task<Message> {
        use crate::modules::insert::solid3d_cmds::empty_solid3d;

        let i = self.active_tab;
        let n = sides.max(3);
        let Some(solid) = solid_model::pyramid_solid([0.0; 3], radius, height, n) else {
            self.command_line
                .push_error(crate::t!("PYRAMID: radius and height must be positive.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "PYRAMID");
        let mut entity = empty_solid3d();
        if let EntityType::Solid3D(inner) = &mut entity {
            inner.wires = solid_model::edge_wires(&solid);
        }
        let history = solid_history::pyramid_op(
            glam::DMat4::IDENTITY.to_cols_array(),
            radius,
            0.0,
            height,
            n,
            true,
        );
        let handle = self.add_solid_model(entity, solid, history);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
            self.tabs[i].dirty = true;
            self.refresh_properties();
            self.command_line.push_output(crate::tf!(
                "PYRAMID: created a {n}-sided pyramid (radius {radius}, height {height})."
            ).as_ref());
        }
        Task::none()
    }

    /// SPLINEFIT — replace the selected polyline with a cubic spline that passes
    /// through its vertices. Control points come from the Catmull-Rom → cubic
    /// Bézier formula (the curve provably interpolates each vertex), with a
    /// clamped piecewise-Bézier knot vector the spline renderer reads directly.
    pub(super) fn fit_spline(&mut self) -> Task<Message> {
        use acadrust::entities::Spline;
        use acadrust::types::Vector3;

        let i = self.active_tab;
        let found: Option<(Handle, Vec<[f64; 3]>)> = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .filter(|(h, _)| !self.tabs[i].scene.is_layer_locked(*h))
            .find_map(|(h, e)| {
                let EntityType::LwPolyline(_) = e else {
                    return None;
                };
                let curve = crate::entities::curve::entity_curve(e)?;
                let cadkernel::geom2d::Curve::Polyline(polyline) = &curve.curve else {
                    return None;
                };
                Some((
                    *h,
                    polyline
                        .vertices
                        .iter()
                        .map(|vertex| curve.plane.point_at(vertex.position))
                        .collect(),
                ))
            });
        let Some((handle, fit)) = found else {
            self.command_line
                .push_error(crate::t!("SPLINEFIT: select a polyline to fit a spline through.").as_ref());
            return Task::none();
        };
        if fit.len() < 3 {
            self.command_line
                .push_error(crate::t!("SPLINEFIT: need at least 3 points.").as_ref());
            return Task::none();
        }
        let n = fit.len();
        let m = n - 1; // Bézier segments
        let p = |k: usize| glam::DVec3::new(fit[k][0], fit[k][1], fit[k][2]);
        // Catmull-Rom → cubic Bézier control points: [P0, b1,b2,P1, b1,b2,P2, …].
        let mut ctrl: Vec<Vector3> = Vec::with_capacity(3 * m + 1);
        ctrl.push(Vector3::new(fit[0][0], fit[0][1], fit[0][2]));
        for seg in 0..m {
            let p0 = p(seg);
            let p1 = p(seg + 1);
            let prev = if seg > 0 { p(seg - 1) } else { p0 };
            let next = if seg + 2 <= m { p(seg + 2) } else { p1 };
            let b1 = p0 + (p1 - prev) / 6.0;
            let b2 = p1 - (next - p0) / 6.0;
            ctrl.push(Vector3::new(b1.x, b1.y, b1.z));
            ctrl.push(Vector3::new(b2.x, b2.y, b2.z));
            ctrl.push(Vector3::new(p1.x, p1.y, p1.z));
        }
        // Clamped piecewise-Bézier knots (degree 3): len == ctrl.len()+degree+1.
        let mut knots: Vec<f64> = vec![0.0; 4];
        for s in 1..m {
            knots.extend_from_slice(&[s as f64, s as f64, s as f64]);
        }
        knots.extend_from_slice(&[m as f64; 4]);
        let mut spl = Spline::new();
        spl.degree = 3;
        spl.control_points = ctrl;
        spl.knots = knots;
        spl.fit_points = fit
            .iter()
            .map(|q| Vector3::new(q[0], q[1], q[2]))
            .collect();
        // flags.rational defaults to false (non-rational) — exactly what we want.
        self.push_undo_snapshot(i, "SPLINEFIT");
        self.tabs[i].scene.erase_entities(&[handle]);
        self.tabs[i].scene.add_entity(EntityType::Spline(spl));
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::tf!("SPLINEFIT: fit a spline through {n} points.").as_ref());
        Task::none()
    }

    /// FLATSHOT — project the selected solid's edges onto the XY plane (Z=0) as
    /// Line entities, giving a flattened 2D shot of the model. Reuses the cached
    /// solid's edge wires (the same source SECTION uses).
    pub(super) fn solid_flatshot(&mut self) -> Task<Message> {
        use acadrust::types::Vector3;
        use acadrust::Line;
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.is_empty() {
            self.command_line
                .push_error(crate::t!("FLATSHOT: select a solid created this session.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, "FLATSHOT");
        let mut n = 0usize;
        for h in &handles {
            let solid = self.tabs[i].scene.solid_models[h].clone();
            for w in solid_model::edge_wires(&solid) {
                for seg in w.points.windows(2) {
                    let line = Line::from_points(
                        Vector3::new(seg[0].x, seg[0].y, 0.0),
                        Vector3::new(seg[1].x, seg[1].y, 0.0),
                    );
                    self.tabs[i].scene.add_entity(EntityType::Line(line));
                    n += 1;
                }
            }
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::tf!("FLATSHOT: created {n} projected edge(s) at Z=0.").as_ref());
        Task::none()
    }

    /// CONVTOSURFACE — convert the selected solid(s) into Surface entities,
    /// carrying the solid's edge wires (reuses the cached B-rep's edge wires).
    pub(super) fn solid_convtosurface(&mut self) -> Task<Message> {
        use acadrust::entities::{Surface, SurfaceKind, Wire as AWire};
        use acadrust::types::Vector3;
        let i = self.active_tab;
        let handles = self.selected_solid_handles();
        if handles.is_empty() {
            self.command_line
                .push_error(crate::t!("CONVTOSURFACE: select a solid created this session.").as_ref());
            return Task::none();
        }
        let mut surfaces: Vec<Surface> = Vec::new();
        for h in &handles {
            let solid = self.tabs[i].scene.solid_models[h].clone();
            let awires: Vec<AWire> = solid_model::edge_wires(&solid)
                .into_iter()
                .map(|w| {
                    let mut aw = AWire::new();
                    aw.points = w
                        .points
                        .iter()
                        .map(|p| Vector3::new(p.x, p.y, p.z))
                        .collect();
                    aw
                })
                .collect();
            let mut surf = Surface::new(SurfaceKind::Generic);
            surf.wires = awires;
            surf.common.layer = self.tabs[i].active_layer.clone();
            surfaces.push(surf);
        }
        self.push_undo_snapshot(i, "CONVTOSURFACE");
        self.tabs[i].scene.erase_entities(&handles);
        let n = surfaces.len();
        for surf in surfaces {
            self.tabs[i].scene.add_entity(EntityType::Surface(surf));
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line
            .push_output(crate::tf!("CONVTOSURFACE: converted {n} solid(s) to surface(s).").as_ref());
        Task::none()
    }
}
