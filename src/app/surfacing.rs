//! The surface modelling commands carried out: what each [`SurfaceRequest`] asks, done with
//! Solidcore through [`ocs_brep::surfacing`], its results added to the drawing or put in place
//! of the objects they change, as one undo step.

use std::sync::atomic::{AtomicI16, Ordering};

use document::entities::{Surface, SurfaceKind};
use document::{EntityType, Handle};
use glam::DVec3;
use iced::Task;
use ocs_brep::{surfacing, Body, Circle3, Curve3, EdgeKey, Ellipse3, FaceKey};
use ocs_geom::space::{NurbsCurve3, Plane, PlanarCurve};
use solidcore_record::brep as r;

use crate::app::{Message, OpenCADStudio};
use crate::modules::surface::command::SurfaceRequest;

/// SURFACEMODELINGMODE: 0 makes procedural surfaces, 1 NURBS surfaces.
static MODELING_MODE: AtomicI16 = AtomicI16::new(0);

pub(crate) fn modeling_mode() -> i16 {
    MODELING_MODE.load(Ordering::Relaxed)
}

pub(crate) fn set_modeling_mode(value: i16) {
    MODELING_MODE.store(value.clamp(0, 1), Ordering::Relaxed);
}

/// How closely surfaces are fitted to the curves they are made through, mm.
const TOLERANCE: f64 = 1e-3;

fn new_surface_kind() -> SurfaceKind {
    if modeling_mode() == 1 {
        SurfaceKind::Nurb
    } else {
        SurfaceKind::Generic
    }
}

fn v3(v: &document::types::Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A curve lying in `plane`, as record curves (one per segment of a polyline with arcs).
fn planar_record_curves(planar: &PlanarCurve) -> Option<Vec<r::Curve>> {
    use ocs_geom::geom2d::Curve as C;
    let plane = planar.plane;
    let lift = |p: [f64; 2]| plane.point_at(p);
    let at = |centre: [f64; 2]| Plane {
        origin: lift(centre),
        x_axis: plane.x_axis,
        y_axis: plane.y_axis,
    };
    let circle = |centre: [f64; 2], radius: f64, from: f64, to: f64| {
        surfacing::record_curve(&Curve3::Circle(Circle3 { plane: at(centre), radius }), [from, to])
    };
    Some(match &planar.curve {
        C::Line(l) => vec![r::Curve::Line { start: lift(l.start), end: lift(l.end) }],
        C::Circle(c) => vec![circle(c.centre, c.radius, 0.0, std::f64::consts::TAU)?],
        C::Arc(a) => vec![circle(a.centre, a.radius, a.start_angle, a.start_angle + a.sweep())?],
        C::Ellipse(e) => {
            let el = &e.ellipse;
            let frame = Plane {
                origin: lift(el.centre),
                x_axis: plane.vector_at(el.major_axis),
                y_axis: plane.vector_at(el.minor_axis()),
            };
            let mut to = e.end_parameter;
            while to <= e.start_parameter {
                to += std::f64::consts::TAU;
            }
            let curve = Curve3::Ellipse(Ellipse3 {
                plane: frame,
                major_radius: el.major_radius,
                minor_radius: el.minor_radius,
            });
            vec![surfacing::record_curve(&curve, [e.start_parameter, to])?]
        }
        C::Polyline(p) => {
            let n = p.vertices.len();
            let segments = if p.closed { n } else { n.saturating_sub(1) };
            if p.vertices.iter().take(segments).all(|v| v.bulge.abs() < 1e-12) {
                let mut points: Vec<[f64; 3]> = p.vertices.iter().map(|v| lift(v.position)).collect();
                if p.closed {
                    points.push(lift(p.vertices.first()?.position));
                }
                return Some(vec![r::Curve::Polyline { points }]);
            }
            let mut out = Vec::new();
            for i in 0..segments {
                let a = p.vertices[i].position;
                let b = p.vertices[(i + 1) % n].position;
                match p.segment_arc(i) {
                    Some(arc) => {
                        let (from, to) = if arc.sweep >= 0.0 {
                            (arc.start_angle, arc.start_angle + arc.sweep)
                        } else {
                            (arc.end_angle, arc.end_angle - arc.sweep)
                        };
                        out.push(circle(arc.center, arc.radius, from, to)?);
                    }
                    None => out.push(r::Curve::Line { start: lift(a), end: lift(b) }),
                }
            }
            out
        }
        C::Nurbs(n) => {
            let lifted = NurbsCurve3::new(
                n.degree(),
                n.control_points().iter().map(|p| lift(*p)).collect(),
                n.knots().to_vec(),
                Some(n.weights().to_vec()),
            )?;
            vec![surfacing::record_nurbs(&lifted)]
        }
        _ => return None,
    })
}

/// A drawing curve as record curves, each over its natural range.
fn entity_record_curves(entity: &EntityType) -> Option<Vec<r::Curve>> {
    match entity {
        EntityType::Line(l) => Some(vec![r::Curve::Line { start: v3(&l.start), end: v3(&l.end) }]),
        EntityType::Spline(s) if s.control_points.len() > s.degree.max(1) as usize => {
            let points: Vec<[f64; 3]> = s.control_points.iter().map(v3).collect();
            let weights = (s.weights.len() == points.len()).then(|| s.weights.clone());
            let curve = NurbsCurve3::new(s.degree.max(1) as usize, points, s.knots.clone(), weights)?;
            Some(vec![surfacing::record_nurbs(&curve)])
        }
        // A spline held by the points it passes through.
        EntityType::Spline(s) => Some(vec![surfacing::record_nurbs(&crate::entities::spline::fit_nurbs3(s)?)]),
        EntityType::Polyline3D(p) => {
            let mut points: Vec<[f64; 3]> = p.vertices.iter().map(|v| v3(&v.position)).collect();
            if p.flags.closed {
                points.push(*points.first()?);
            }
            (points.len() >= 2).then(|| vec![r::Curve::Polyline { points }])
        }
        _ => planar_record_curves(&crate::entities::curve::entity_curve(entity)?),
    }
}

impl OpenCADStudio {
    /// The B-rep of a surface, solid or region.
    fn surfacing_body(&mut self, i: usize, handle: Handle) -> Option<Body> {
        self.tabs[i].scene.restore_solid_models(&[handle]);
        if let Some(body) = self.tabs[i].scene.solid_models.get(&handle) {
            return Some(body.clone());
        }
        match self.tabs[i].scene.document.get_entity(handle)? {
            EntityType::Surface(surface) => crate::scene::convert::solid3d_tess::kernel_surface_body(surface),
            _ => None,
        }
    }

    /// The record curves of the drawing curves `handles`, each object's on its own.
    fn surfacing_curves(&self, i: usize, handles: &[Handle]) -> Result<Vec<Vec<r::Curve>>, String> {
        handles
            .iter()
            .map(|h| {
                self.tabs[i]
                    .scene
                    .document
                    .get_entity(*h)
                    .and_then(entity_record_curves)
                    .ok_or_else(|| crate::t!("An object is not a curve these surfaces are made through.").into_owned())
            })
            .collect()
    }

    /// `body` as a new surface entity of `kind`.
    fn surfacing_add_surface(&mut self, body: Body, kind: SurfaceKind) -> Handle {
        self.add_surface_model(EntityType::Surface(Box::new(Surface::new(kind))), body)
    }

    /// `body` in place of the B-rep of `handle`, the entity a surface of `kind` where given.
    fn surfacing_replace(&mut self, i: usize, handle: Handle, body: Body, kind: Option<SurfaceKind>) -> bool {
        let Some(display) = self.tabs[i]
            .scene
            .prepare_solid_model_display(handle, &body)
            .filter(|display| display.0.complete)
        else {
            return false;
        };
        let Some(entity) = self.tabs[i].scene.document.get_entity(handle).cloned() else {
            return false;
        };
        let Some(mut entity) = super::model_ops::entity_with_boolean_body(entity, &body) else {
            return false;
        };
        if let (Some(kind), EntityType::Surface(surface)) = (kind, &mut entity) {
            // The subtype data goes with the kind: a NURBSURFACE record written with another
            // kind's data cannot be read back.
            surface.kind = kind;
            surface.surface_data = Surface::new(kind).surface_data;
        }
        if !self.tabs[i].scene.update_entity(entity) {
            return false;
        }
        self.tabs[i].scene.register_prepared_solid_model(handle, body, display);
        self.tabs[i].dirty = true;
        true
    }

    /// The edge of `handle`'s body nearest `point`.
    fn surfacing_edge(&mut self, i: usize, handle: Handle, point: DVec3) -> Option<(Body, EdgeKey)> {
        let body = self.surfacing_body(i, handle)?;
        let edge = crate::scene::model::solid_model::nearest_edge(&body, point.to_array())?;
        Some((body, edge))
    }

    /// The face of `handle`'s body at `point` (its only face for a one-face surface).
    fn surfacing_face(&mut self, i: usize, handle: Handle, point: DVec3) -> Option<(Body, FaceKey)> {
        let body = self.surfacing_body(i, handle)?;
        let face = ocs_brep::face_at_point(&body, point.to_array())
            .or_else(|| (body.faces.len() == 1).then(|| body.face_keys().next()).flatten())?;
        Some((body, face))
    }

    /// The UCS z axis: the direction curves are projected along.
    fn surfacing_ucs_z(&self, i: usize) -> DVec3 {
        match self.tabs[i].active_ucs.as_ref() {
            Some(ucs) => super::helpers::ucs_z_axis(ucs),
            None => DVec3::Z,
        }
    }

    /// Carries out a surface command.
    pub(super) fn handle_surfacing(&mut self, request: SurfaceRequest) -> Task<Message> {
        let i = self.active_tab;
        // The command has gathered all it asks: it ends here, as THICKEN's does.
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        let name = request.command;
        let outcome = self.surfacing(i, &request);
        match outcome {
            Ok(message) => {
                if !message.is_empty() {
                    self.command_line.push_output(&message);
                }
            }
            Err(error) => self.command_line.push_error(&format!("{name}: {error}")),
        }
        self.refresh_properties();
        Task::none()
    }

    fn surfacing(&mut self, i: usize, req: &SurfaceRequest) -> Result<String, String> {
        let name = req.command;
        let picked = |slot: usize| req.picks[slot].first().copied();
        let refused = |e: String| e;
        match name {
            "PLANESURF" => {
                let mut bodies = Vec::new();
                if req.points.len() == 2 {
                    let z = self.surfacing_ucs_z(i);
                    let (x, _) = crate::scene::view::transform::ocs_axes((z.x, z.y, z.z));
                    let plane = Plane::orthonormal(req.points[0].to_array(), [x.0, x.1, x.2], z.to_array())
                        .ok_or("no plane")?;
                    let corner = plane.project(req.points[1].to_array()).ok_or("no plane")?;
                    use ocs_geom::geom2d::{Curve, Line};
                    let rect = [[0.0, 0.0], [corner[0], 0.0], corner, [0.0, corner[1]]];
                    let loop_: Vec<Curve> = (0..4)
                        .map(|k| Curve::Line(Line { start: rect[k], end: rect[(k + 1) % 4] }))
                        .collect();
                    bodies.push(ocs_brep::planar_region(plane, &[loop_]).ok_or("the corners make no area")?);
                } else {
                    for handle in &req.objects[0] {
                        let Some(planar) = self.tabs[i]
                            .scene
                            .document
                            .get_entity(*handle)
                            .and_then(crate::entities::curve::entity_curve)
                        else {
                            continue;
                        };
                        if let Some(body) = ocs_brep::planar_region(planar.plane, &[vec![planar.curve.clone()]]) {
                            bodies.push(body);
                        }
                    }
                }
                if bodies.is_empty() {
                    return Err(crate::t!("No closed planar objects to make planar surfaces of.").into_owned());
                }
                self.push_undo_snapshot(i, name);
                let count = bodies.len();
                for body in bodies {
                    self.surfacing_add_surface(body, SurfaceKind::Plane);
                }
                Ok(crate::tf!("{count} planar surface(s) created.").into_owned())
            }
            "SURFNETWORK" => {
                let us: Vec<r::Curve> = self.surfacing_curves(i, &req.objects[0])?.into_iter().flatten().collect();
                let vs: Vec<r::Curve> = self.surfacing_curves(i, &req.objects[1])?.into_iter().flatten().collect();
                let body = surfacing::network(&us, &vs, TOLERANCE).map_err(refused)?;
                self.push_undo_snapshot(i, name);
                self.surfacing_add_surface(body, new_surface_kind());
                Ok(String::new())
            }
            "SURFPATCH" => {
                let curves: Vec<r::Curve> = self.surfacing_curves(i, &req.objects[0])?.into_iter().flatten().collect();
                let body = surfacing::patch(&curves, TOLERANCE).map_err(refused)?;
                self.push_undo_snapshot(i, name);
                self.surfacing_add_surface(body, new_surface_kind());
                Ok(String::new())
            }
            "SURFBLEND" => {
                let ((ha, pa), (hb, pb)) = (picked(0).ok_or("no first edge")?, picked(1).ok_or("no second edge")?);
                let (a, ea) = self.surfacing_edge(i, ha, pa).ok_or("the first pick is no surface edge")?;
                let (b, eb) = self.surfacing_edge(i, hb, pb).ok_or("the second pick is no surface edge")?;
                let continuity = req.value("continuity", 1.0).round().clamp(0.0, 2.0) as u8;
                let crown = (req.value("bulge", 0.5).clamp(0.0, 1.0) - 0.5) * 2.0;
                let body = surfacing::blend(&a, ea, &b, eb, [continuity; 2], [crown; 2]).map_err(refused)?;
                self.push_undo_snapshot(i, name);
                self.surfacing_add_surface(body, new_surface_kind());
                Ok(String::new())
            }
            "SURFFILLET" => {
                let ((ha, pa), (hb, pb)) = (picked(0).ok_or("no first surface")?, picked(1).ok_or("no second surface")?);
                let a = self.surfacing_body(i, ha).ok_or("the first pick is no surface")?;
                let b = self.surfacing_body(i, hb).ok_or("the second pick is no surface")?;
                let radius = req.value("radius", 1.0);
                let trim = req.value("trim", 1.0) >= 0.5;
                let picks = Some((pa.to_array(), pb.to_array()));
                let (fillet, trimmed) = surfacing::fillet(&a, &b, radius, trim, picks).map_err(refused)?;
                self.push_undo_snapshot(i, name);
                self.surfacing_add_surface(fillet, new_surface_kind());
                if let Some((ta, tb)) = trimmed {
                    self.surfacing_replace(i, ha, ta, None);
                    self.surfacing_replace(i, hb, tb, None);
                }
                Ok(String::new())
            }
            "SURFEXTEND" => {
                let distance = req.value("distance", 1.0);
                let stretch = req.value("stretch", 0.0) >= 0.5;
                let mut changed: Vec<(Handle, Body)> = Vec::new();
                for (handle, point) in &req.picks[0] {
                    let body = match changed.iter().position(|(h, _)| h == handle) {
                        Some(k) => changed[k].1.clone(),
                        None => self.surfacing_body(i, *handle).ok_or("a pick is no surface")?,
                    };
                    let edge = crate::scene::model::solid_model::nearest_edge(&body, point.to_array())
                        .ok_or("a pick is no surface edge")?;
                    let extended = surfacing::extend(&body, edge, distance, stretch).map_err(refused)?;
                    match changed.iter_mut().find(|(h, _)| h == handle) {
                        Some(slot) => slot.1 = extended,
                        None => changed.push((*handle, extended)),
                    }
                }
                self.push_undo_snapshot(i, name);
                for (handle, body) in changed {
                    self.surfacing_replace(i, handle, body, None);
                }
                Ok(String::new())
            }
            "SURFTRIM" => {
                let (handle, point) = picked(0).ok_or("no area picked")?;
                if !req.objects[0].contains(&handle) {
                    return Err(crate::t!("The area picked is not on a surface being trimmed.").into_owned());
                }
                let (body, face) = self.surfacing_face(i, handle, point).ok_or("the pick is no surface")?;
                let curves: Vec<r::Curve> = self.surfacing_curves(i, &req.objects[1])?.into_iter().flatten().collect();
                let z = self.surfacing_ucs_z(i).to_array();
                // Projected onto the surface along the UCS z axis, or as they are where they lie on it.
                let on: Vec<r::Curve> = match surfacing::project(&body, face, &curves, z) {
                    Ok(projected) if !projected.is_empty() => projected.into_iter().map(|(c, _)| c).collect(),
                    _ => curves,
                };
                let trimmed = surfacing::trim(&body, face, &on, point.to_array()).map_err(refused)?;
                self.push_undo_snapshot(i, name);
                self.surfacing_replace(i, handle, trimmed, None);
                Ok(String::new())
            }
            "SURFUNTRIM" => {
                let (handle, point) = picked(0).ok_or("no surface picked")?;
                let (body, face) = self.surfacing_face(i, handle, point).ok_or("the pick is no surface")?;
                let untrimmed = surfacing::untrim(&body, face, true).map_err(refused)?;
                self.push_undo_snapshot(i, name);
                self.surfacing_replace(i, handle, untrimmed, None);
                Ok(String::new())
            }
            "SURFOFFSET" => {
                let distance = req.value("distance", 1.0);
                let mut made = Vec::new();
                for handle in &req.objects[0] {
                    let body = self.surfacing_body(i, *handle).ok_or("an object is no surface")?;
                    made.push(surfacing::offset(&body, distance).map_err(refused)?);
                }
                self.push_undo_snapshot(i, name);
                for body in made {
                    self.surfacing_add_surface(body, new_surface_kind());
                }
                Ok(String::new())
            }
            "SURFSCULPT" => {
                let bodies: Vec<Body> = req.objects[0]
                    .iter()
                    .map(|h| self.surfacing_body(i, *h).ok_or("an object is no surface"))
                    .collect::<Result<_, _>>()?;
                let refs: Vec<&Body> = bodies.iter().collect();
                let (body, closed) = surfacing::sew(&refs, TOLERANCE).map_err(refused)?;
                if !closed {
                    return Err(crate::t!("The surfaces do not enclose a volume.").into_owned());
                }
                self.push_undo_snapshot(i, name);
                let history = crate::scene::model::solid_history::brep_op(&body);
                self.add_solid_model(crate::modules::insert::solid3d_cmds::empty_solid3d(), body, history);
                Ok(String::new())
            }
            "SURFEXTRACTCURVE" => {
                let (handle, point) = picked(0).ok_or("no surface picked")?;
                let (body, face) = self.surfacing_face(i, handle, point).ok_or("the pick is no surface")?;
                let along_v = req.value("direction", 0.0) >= 0.5;
                let curve = surfacing::isoline(&body, face, point.to_array(), along_v).ok_or("no isoline through the point")?;
                let (a, b) = curve.domain();
                let entity = super::solid_edit_ops::curve_entity(&Curve3::Nurbs(curve), (a, b), DVec3::ZERO)
                    .ok_or("the isoline makes no curve")?;
                self.push_undo_snapshot(i, name);
                self.commit_new_curves(vec![entity]);
                Ok(String::new())
            }
            "CONVTONURBS" => {
                let mut made: Vec<(Handle, Vec<Body>)> = Vec::new();
                for handle in &req.objects[0] {
                    let body = self.surfacing_body(i, *handle).ok_or("an object has no B-rep")?;
                    made.push((*handle, surfacing::to_nurbs_sheets(&body).map_err(refused)?));
                }
                self.push_undo_snapshot(i, name);
                let mut count = 0;
                for (handle, sheets) in made {
                    for sheet in sheets {
                        self.surfacing_add_surface(sheet, SurfaceKind::Nurb);
                        count += 1;
                    }
                    self.tabs[i].scene.erase_entities(&[handle]);
                }
                Ok(crate::tf!("{count} NURBS surface(s) created.").into_owned())
            }
            "CVSHOW" => {
                for handle in &req.objects[0] {
                    if let Some(body) = self.surfacing_body(i, *handle) {
                        if let Ok(net) = surfacing::control_net(&body) {
                            self.tabs[i].scene.show_control_net(*handle, &net);
                        }
                    }
                }
                Ok(String::new())
            }
            "CVHIDE" => {
                self.tabs[i].scene.hide_control_nets();
                Ok(String::new())
            }
            "CVADD" | "CVREMOVE" => {
                let (handle, point) = picked(0).ok_or("no surface picked")?;
                let body = self.surfacing_body(i, handle).ok_or("the pick is no surface")?;
                let along_v = req.value("direction", 0.0) >= 0.5;
                let changed = if name == "CVADD" {
                    surfacing::add_control_row(&body, point.to_array(), along_v).map_err(refused)?
                } else {
                    surfacing::remove_control_row(&body, along_v, TOLERANCE).map_err(refused)?
                };
                self.push_undo_snapshot(i, name);
                self.surfacing_replace(i, handle, changed.clone(), Some(SurfaceKind::Nurb));
                if self.tabs[i].scene.control_net_shown(handle) {
                    if let Ok(net) = surfacing::control_net(&changed) {
                        self.tabs[i].scene.show_control_net(handle, &net);
                    }
                }
                Ok(String::new())
            }
            "CVREBUILD" => {
                let counts = [req.value("u_count", 8.0), req.value("v_count", 8.0)].map(|c| c.round().max(2.0) as usize);
                let degrees = [req.value("u_degree", 3.0), req.value("v_degree", 3.0)].map(|d| d.round().clamp(1.0, 7.0) as usize);
                let spans = [0, 1].map(|k| counts[k].saturating_sub(degrees[k]).max(1));
                let mut made = Vec::new();
                for handle in &req.objects[0] {
                    let body = self.surfacing_body(i, *handle).ok_or("an object is no surface")?;
                    // Rebuilt to the net asked for however far that moves it; how far is said.
                    let (rebuilt, moved) = surfacing::rebuild(&body, degrees, spans, 1e3).map_err(refused)?;
                    made.push((*handle, rebuilt, moved));
                }
                self.push_undo_snapshot(i, name);
                let mut worst = 0.0_f64;
                for (handle, body, moved) in made {
                    worst = worst.max(moved);
                    self.surfacing_replace(i, handle, body, Some(SurfaceKind::Nurb));
                }
                Ok(crate::tf!("Rebuilt; the surface moved at most {}.", crate::entities::common::format_length(worst)).into_owned())
            }
            "3DEDITBAR" => {
                let (handle, point) = picked(0).ok_or("no surface picked")?;
                let target = *req.points.first().ok_or("no new location")?;
                let body = self.surfacing_body(i, handle).ok_or("the pick is no surface")?;
                let moved = surfacing::drag_point(&body, point.to_array(), target.to_array()).map_err(refused)?;
                self.push_undo_snapshot(i, name);
                self.surfacing_replace(i, handle, moved, Some(SurfaceKind::Nurb));
                Ok(String::new())
            }
            "PROJECTGEOMETRY" => {
                let (handle, point) = picked(0).ok_or("no target picked")?;
                let (body, face) = self.surfacing_face(i, handle, point).ok_or("the target is no surface or solid face")?;
                let curves: Vec<r::Curve> = self.surfacing_curves(i, &req.objects[0])?.into_iter().flatten().collect();
                let direction = self.surfacing_ucs_z(i).to_array();
                let projected = surfacing::project(&body, face, &curves, direction).map_err(refused)?;
                let entities: Vec<EntityType> = projected
                    .iter()
                    .filter_map(|(c, interval)| {
                        let (curve, span) = surfacing::model_curve(c, *interval)?;
                        super::solid_edit_ops::curve_entity(&curve, (span[0], span[1]), DVec3::ZERO)
                    })
                    .collect();
                if entities.is_empty() {
                    return Err(crate::t!("Nothing projects onto the target.").into_owned());
                }
                self.push_undo_snapshot(i, name);
                let count = self.commit_new_curves(entities);
                Ok(crate::tf!("{count} curve(s) projected.").into_owned())
            }
            "ANALYSISZEBRA" | "ANALYSISCURVATURE" | "ANALYSISDRAFT" => {
                let mode = match name {
                    "ANALYSISZEBRA" => crate::scene::analysis::Mode::Zebra,
                    "ANALYSISCURVATURE" => crate::scene::analysis::Mode::Curvature,
                    _ => crate::scene::analysis::Mode::Draft,
                };
                let on = crate::scene::analysis::toggle(mode);
                self.tabs[i].scene.invalidate_solid_meshes();
                Ok(if on { crate::t!("Analysis on.").into_owned() } else { crate::t!("Analysis off.").into_owned() })
            }
            "ANALYSISOPTIONS" => {
                crate::scene::analysis::set_options(
                    req.values.get("stripes").copied(),
                    req.values.get("curvature").copied(),
                    req.values.get("draft_min").copied(),
                    req.values.get("draft_max").copied(),
                );
                self.tabs[i].scene.invalidate_solid_meshes();
                Ok(String::new())
            }
            _ => Err(crate::t!("Unknown surface command.").into_owned()),
        }
    }
}

impl OpenCADStudio {
    /// SURFACEMODELINGMODE asked for or set.
    fn set_modeling_mode_variable(&mut self, value: Option<&str>) -> Task<Message> {
        let ask = |app: &mut Self| {
            app.command_line.push_output(&crate::ui::command_line::value_prompt(
                "SURFACEMODELINGMODE",
                &modeling_mode().to_string(),
            ));
            app.pending_setvar = Some("SURFACEMODELINGMODE".to_string());
        };
        match value.map(str::trim).filter(|v| !v.is_empty()) {
            None => ask(self),
            Some(v) => match v.parse::<i16>().ok().filter(|v| (0..=1).contains(v)) {
                Some(v) => set_modeling_mode(v),
                None => {
                    self.command_line.push_rejection("Requires 0 or 1 only.");
                    ask(self);
                }
            },
        }
        Task::none()
    }

    /// Starts a surface command, or sets SURFACEMODELINGMODE; `None` for any other command.
    pub(super) fn dispatch_surface(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let verb = verb.trim_start_matches(['_', '.', '\'']).to_ascii_uppercase();
        let rest = rest.trim();
        if verb == "SURFACEMODELINGMODE" {
            return Some(self.set_modeling_mode_variable(Some(rest).filter(|r| !r.is_empty())));
        }
        if verb == "SETVAR" {
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if name.eq_ignore_ascii_case("SURFACEMODELINGMODE") {
                return Some(self.set_modeling_mode_variable(Some(value)));
            }
            return None;
        }
        let name: &'static str = crate::modules::surface::COMMANDS.iter().copied().find(|c| *c == verb)?;
        let steps = crate::modules::surface::steps(name)?;
        let selected = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(handle, entity)| (handle, entity.clone()))
            .collect();
        let mut command = crate::modules::surface::command::SurfaceCommand::new(name, steps, selected);
        if command.ready() {
            return Some(self.handle_surfacing(command.take_request()));
        }
        use crate::command::CadCommand;
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        Some(Task::none())
    }
}
