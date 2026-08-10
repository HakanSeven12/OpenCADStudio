// 3D solid modelling support on the App: committing Model-tab primitives,
// and the Design-group boolean operations over the scene's session-cached
// B-reps.
//
// The bodies come from the geometry kernel, so every operation here is on
// analytic surfaces rather than on facets — a turned cylinder is still a
// cylinder afterwards, and saves as one.

use acadrust::entities::Solid3D;
use acadrust::kernel::brep::Body;
use acadrust::{EntityType, Handle};
use iced::Task;

use super::Message;
use crate::modules::model::boolean_cmd::BoolOp;
use crate::scene::model::solid_model::{self, Bool};

impl super::OpenCADStudio {
    /// Commit a Model-tab solid: add its acadrust entity to the document, then
    /// register the B-rep (caches it for booleans + tessellates it into the
    /// shaded mesh pipeline). Returns the new entity handle.
    pub(super) fn add_solid_model(&mut self, entity: EntityType, solid: Body) -> Handle {
        let i = self.active_tab;
        let Some(handle) = self.commit_entity_handle(entity) else {
            return Handle::NULL;
        };
        self.tabs[i].scene.register_solid_model(handle, solid);
        handle
    }

    /// Run a boolean (`union` / `subtract` / `intersect`) on exactly two
    /// selected solids whose B-reps are in the session cache.
    pub(super) fn solid_boolean(&mut self, op: BoolOp) -> Task<Message> {
        let i = self.active_tab;
        // Selected entities that have a cached B-rep.
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
        if handles.len() != 2 {
            self.command_line
                .push_error(crate::t!("Boolean: select exactly two solids created this session.").as_ref());
            return Task::none();
        }
        let a = self.tabs[i].scene.solid_models[&handles[0]].clone();
        let b = self.tabs[i].scene.solid_models[&handles[1]].clone();
        let kind = match op {
            BoolOp::Union => Bool::Union,
            BoolOp::Subtract => Bool::Subtract,
            BoolOp::Intersect => Bool::Intersect,
        };
        let Some(result) = solid_model::boolean(kind, &a, &b) else {
            self.command_line
                .push_error(crate::t!("Boolean failed — the solids may not overlap.").as_ref());
            return Task::none();
        };

        self.push_undo_snapshot(i, "BOOLEAN");
        // Remove the two operands (entity + mesh + cached B-rep).
        self.tabs[i].scene.erase_entities(&handles);
        // The result is freshly combined geometry with no ACIS parametrisation,
        // so it lives as a Solid3D whose render/boolean data is the injected
        // mesh + cached B-rep; its edge wires make it pickable.
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&result);
        let handle = self.add_solid_model(EntityType::Solid3D(s3d), result);
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
    /// the same boolean path the Design-group tools use.
    pub(super) fn solid_slice(&mut self, axis: usize, value: f64, keep_low: bool) -> Task<Message> {
        let i = self.active_tab;
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
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
        let handle = self.add_solid_model(EntityType::Solid3D(s3d), result);
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
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
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
                self.add_solid_model(EntityType::Solid3D(s3d), result);
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
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
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
        let handle = self.add_solid_model(EntityType::Solid3D(s3d), rotated);
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

    /// POLYSOLID — build a wall-like solid from a selected polyline: one box per
    /// segment (oriented along it, `width` wide, `height` tall) unioned together,
    /// reusing the box primitive + the boolean union path.
    pub(super) fn solid_polysolid(&mut self, width: f64, height: f64) -> Task<Message> {
        let i = self.active_tab;
        let found: Option<(Handle, Vec<[f64; 2]>, bool)> = self.tabs[i]
            .scene
            .selected_entities()
            .iter()
            .find_map(|(h, e)| match e {
                EntityType::LwPolyline(pl) => Some((
                    *h,
                    pl.vertices
                        .iter()
                        .map(|v| [v.location.x, v.location.y])
                        .collect(),
                    pl.is_closed,
                )),
                _ => None,
            });
        let Some((handle, pts, closed)) = found else {
            self.command_line
                .push_error(crate::t!("POLYSOLID: select a polyline first.").as_ref());
            return Task::none();
        };
        let mut segs: Vec<([f64; 2], [f64; 2])> = pts.windows(2).map(|w| (w[0], w[1])).collect();
        if closed && pts.len() > 2 {
            segs.push((pts[pts.len() - 1], pts[0]));
        }
        let mut acc: Option<Body> = None;
        let mut used = 0usize;
        for (a, b) in &segs {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-9 {
                continue;
            }
            let angle = dy.atan2(dx);
            // Local box: X 0..len, Y -w/2..w/2, Z 0..h, then turned to lie
            // along the segment and moved onto its start.
            let (sin, cos) = angle.sin_cos();
            let Some(bx) = solid_model::box_solid([len / 2.0, 0.0, height / 2.0], len, width, height)
                .and_then(|bx| {
                    solid_model::placed(
                        &bx,
                        [cos, sin, 0.0],
                        [-sin, cos, 0.0],
                        [0.0, 0.0, 1.0],
                        [a[0], a[1], 0.0],
                    )
                })
            else {
                continue;
            };
            acc = Some(match acc {
                None => bx,
                Some(prev) => solid_model::boolean(Bool::Union, &prev, &bx).unwrap_or(prev),
            });
            used += 1;
        }
        let Some(result) = acc else {
            self.command_line
                .push_error(crate::t!("POLYSOLID: the polyline has no usable segments.").as_ref());
            return Task::none();
        };
        self.push_undo_snapshot(i, "POLYSOLID");
        self.tabs[i].scene.erase_entities(&[handle]);
        let mut s3d = Solid3D::new();
        s3d.wires = solid_model::edge_wires(&result);
        let h = self.add_solid_model(EntityType::Solid3D(s3d), result);
        self.tabs[i].scene.deselect_all();
        if !h.is_null() {
            self.tabs[i].scene.select_entity(h, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(crate::tf!(
            "POLYSOLID: built a wall solid from {used} segment(s) (width {width}, height {height})."
        ).as_ref());
        Task::none()
    }

    /// 3DMIRROR — add a mirrored copy of the one selected solid across the plane
    /// perpendicular to the X/Y/Z axis (0/1/2) through its centre, keeping the
    /// original. A reflection loses handedness, so the kernel reverses every
    /// loop on the way through; without that the copy lights black.
    pub(super) fn solid_mirror3d(&mut self, axis: usize) -> Task<Message> {
        let i = self.active_tab;
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
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
        let h = self.add_solid_model(EntityType::Solid3D(s3d), reflected);
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
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
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
        let h = self.add_solid_model(EntityType::Solid3D(s3d), aligned);
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
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
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
        let handle = self.add_solid_model(entity, solid);
        self.tabs[i].scene.deselect_all();
        if !handle.is_null() {
            self.tabs[i].scene.select_entity(handle, false);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        self.command_line.push_output(crate::tf!(
            "PYRAMID: created a {n}-sided pyramid (radius {radius}, height {height})."
        ).as_ref());
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
            .find_map(|(h, e)| match e {
                EntityType::LwPolyline(pl) => Some((
                    *h,
                    pl.vertices
                        .iter()
                        .map(|v| [v.location.x, v.location.y, 0.0])
                        .collect(),
                )),
                _ => None,
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
        ctrl.push(Vector3::new(fit[0][0], fit[0][1], 0.0));
        for seg in 0..m {
            let p0 = p(seg);
            let p1 = p(seg + 1);
            let prev = if seg > 0 { p(seg - 1) } else { p0 };
            let next = if seg + 2 <= m { p(seg + 2) } else { p1 };
            let b1 = p0 + (p1 - prev) / 6.0;
            let b2 = p1 - (next - p0) / 6.0;
            ctrl.push(Vector3::new(b1.x, b1.y, 0.0));
            ctrl.push(Vector3::new(b2.x, b2.y, 0.0));
            ctrl.push(Vector3::new(p1.x, p1.y, 0.0));
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
        spl.fit_points = fit.iter().map(|q| Vector3::new(q[0], q[1], 0.0)).collect();
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
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
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
        let handles: Vec<Handle> = self.tabs[i]
            .scene
            .selected
            .iter()
            .copied()
            .filter(|h| self.tabs[i].scene.solid_models.contains_key(h))
            .collect();
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
