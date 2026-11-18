// SOLIDEDIT and XEDGES: editing a solid's faces, edges and body, and
// drawing its edges as curves.

use std::collections::HashMap;

use document::{
    entities::{Arc, Circle, Ellipse, EntityCommon, Line, Region, Solid3D, Spline, Surface, SurfaceKind},
    types::{Color, Vector3},
    EntityType, Handle,
};
use glam::DVec3;
use iced::Task;
use ocs_solid::model::{self, r, Brep};

use super::Message;
use crate::command::{FacePick, SolidEditAction, SolidEditRequest};
use crate::scene::convert::acis_import::{body_colors, paint_body};
use crate::scene::model::solid_model;

type FaceColors = HashMap<u32, Color>;
type EdgeColors = HashMap<u32, Color>;

/// A solid's body as its ACIS data holds it, with the colours its faces and
/// edges carry of their own.
struct Painted {
    body: Brep,
    faces: FaceColors,
    edges: EdgeColors,
}

fn painted(acis: &document::entities::AcisData) -> Option<Painted> {
    let document = crate::scene::convert::solid3d_tess::acis_document(acis)?;
    let (mut bodies, loss) = ocs_solid::lift(&document);
    if !loss.is_empty() || bodies.len() != 1 {
        return None;
    }
    let body = bodies.pop()?;
    let (faces, edges) = body_colors(&document, &body);
    Some(Painted { body: body.brep, faces, edges })
}

fn acis_of(entity: &EntityType) -> Option<&document::entities::AcisData> {
    match entity {
        EntityType::Solid3D(solid) => Some(&solid.acis_data),
        EntityType::Region(region) => Some(&region.acis_data),
        EntityType::Surface(surface) => Some(&surface.acis_data),
        EntityType::Body(body) => Some(&body.acis_data),
        _ => None,
    }
}

/// The faces the picks name, in the order picked.
fn picked_faces(body: &Brep, picks: &[FacePick]) -> Option<Vec<u32>> {
    let mut faces: Vec<u32> = Vec::new();
    for pick in picks {
        match *pick {
            FacePick::Add(point) => {
                let face = model::face_at_point(body, point.to_array(), f64::INFINITY)?;
                if !faces.contains(&face) {
                    faces.push(face);
                }
            }
            FacePick::Remove(point) => {
                let face = model::face_at_point(body, point.to_array(), f64::INFINITY)?;
                faces.retain(|other| *other != face);
            }
            FacePick::All => faces = (0..body.faces.len() as u32).collect(),
        }
    }
    (!faces.is_empty()).then_some(faces)
}

/// The edges the picks name, in the order picked.
fn picked_edges(body: &Brep, picks: &[FacePick]) -> Option<Vec<u32>> {
    let mut edges: Vec<u32> = Vec::new();
    for pick in picks {
        match *pick {
            FacePick::Add(point) => {
                let edge = solid_model::nearest_edge(body, point.to_array())?;
                if !edges.contains(&edge) {
                    edges.push(edge);
                }
            }
            FacePick::Remove(point) => {
                let edge = solid_model::nearest_edge(body, point.to_array())?;
                edges.retain(|other| *other != edge);
            }
            FacePick::All => edges = (0..body.edges.len() as u32).collect(),
        }
    }
    (!edges.is_empty()).then_some(edges)
}

/// An own colour, or none for ByLayer and ByBlock: the object's own then.
fn own_color(color: Color) -> Option<Color> {
    (!matches!(color, Color::ByLayer | Color::ByBlock)).then_some(color)
}

fn vector(point: [f64; 3]) -> Vector3 {
    Vector3::new(point[0], point[1], point[2])
}

/// The axes an entity normal's object coordinate system uses.
fn ocs_axes(normal: DVec3) -> (DVec3, DVec3) {
    let (x, y) = crate::scene::view::transform::ocs_axes((normal.x, normal.y, normal.z));
    (DVec3::new(x.0, x.1, x.2), DVec3::new(y.0, y.1, y.2))
}

/// A record curve over `span` as the drawing object that draws it, moved by `shift`.
pub(super) fn curve_entity(curve: &r::Curve, span: [f64; 2], shift: DVec3) -> Option<EntityType> {
    let at = |t: f64| ocs_solid::geometry::point_on(curve, t).map(|p| DVec3::from_array(p) + shift);
    let whole = span[1] - span[0] >= std::f64::consts::TAU - 1e-9;
    let spline = |degree: usize, points: Vec<[f64; 3]>, knots: Vec<f64>, weights: Option<Vec<f64>>| {
        ocs_geom::space::NurbsCurve3::new(degree, points, knots, weights)
    };
    Some(match curve {
        r::Curve::Line { .. } => {
            EntityType::Line(Line::from_points(vector(at(span[0])?.to_array()), vector(at(span[1])?.to_array())))
        }
        r::Curve::Circle { frame, radius } => {
            let normal = DVec3::from_array(frame.x_axis).cross(DVec3::from_array(frame.y_axis)).try_normalize()?;
            let centre = DVec3::from_array(frame.origin) + shift;
            let (x, y) = ocs_axes(normal);
            let local = |point: DVec3| DVec3::new(point.dot(x), point.dot(y), point.dot(normal));
            if whole {
                let mut entity = Circle::new();
                entity.center = vector(local(centre).to_array());
                entity.radius = *radius;
                entity.normal = vector(normal.to_array());
                EntityType::Circle(entity)
            } else {
                let angle = |point: DVec3| {
                    let offset = point - centre;
                    offset.dot(y).atan2(offset.dot(x))
                };
                let mut entity = Arc::new();
                entity.center = vector(local(centre).to_array());
                entity.radius = *radius;
                entity.start_angle = angle(at(span[0])?).rem_euclid(std::f64::consts::TAU);
                entity.end_angle = angle(at(span[1])?).rem_euclid(std::f64::consts::TAU);
                entity.normal = vector(normal.to_array());
                EntityType::Arc(entity)
            }
        }
        r::Curve::Ellipse { frame, radii } => {
            let x = DVec3::from_array(frame.x_axis);
            let y = DVec3::from_array(frame.y_axis);
            let mut entity = Ellipse::new();
            entity.center = vector((DVec3::from_array(frame.origin) + shift).to_array());
            entity.normal = vector(x.cross(y).normalize_or_zero().to_array());
            // The major axis is the longer; a parameter measured from the
            // other one turns by a quarter.
            let (major, ratio, turn) = if radii[0] >= radii[1] {
                (x * radii[0], radii[1] / radii[0], 0.0)
            } else {
                (y * radii[1], radii[0] / radii[1], -std::f64::consts::FRAC_PI_2)
            };
            entity.major_axis = vector(major.to_array());
            entity.minor_axis_ratio = ratio;
            if whole {
                entity.start_parameter = 0.0;
                entity.end_parameter = std::f64::consts::TAU;
            } else {
                entity.start_parameter = (span[0] + turn).rem_euclid(std::f64::consts::TAU);
                entity.end_parameter = (span[1] + turn).rem_euclid(std::f64::consts::TAU);
            }
            EntityType::Ellipse(entity)
        }
        r::Curve::SurfaceIntersection { leader, .. } => return curve_entity(leader, span, shift),
        r::Curve::Polyline { .. } | r::Curve::Spline { .. } | r::Curve::RationalSpline { .. } => {
            let spatial = match curve {
                r::Curve::Polyline { points } => {
                    // Degree one: segment i over [i, i + 1].
                    let last = points.len().checked_sub(1)? as f64;
                    let mut knots = vec![0.0];
                    knots.extend((0..points.len()).map(|i| i as f64));
                    knots.push(last);
                    spline(1, points.clone(), knots, None)?
                }
                r::Curve::Spline { knots, points } => {
                    spline(knots.len().checked_sub(points.len() + 1)?, points.clone(), knots.clone(), None)?
                }
                r::Curve::RationalSpline { knots, homogeneous } => spline(
                    knots.len().checked_sub(homogeneous.len() + 1)?,
                    homogeneous.iter().map(|h| [h[0] / h[3], h[1] / h[3], h[2] / h[3]]).collect(),
                    knots.clone(),
                    Some(homogeneous.iter().map(|h| h[3]).collect()),
                )?,
                _ => return None,
            };
            let (from, to) = spatial.domain();
            let mut piece = spatial;
            if span[0] > from + 1e-12 {
                piece = piece.split_at(span[0]).map_or(piece.clone(), |(_, right)| right);
            }
            if span[1] < to - 1e-12 {
                piece = piece.split_at(span[1]).map_or(piece.clone(), |(left, _)| left);
            }
            let mut entity = Spline::new();
            entity.degree = piece.degree() as i32;
            entity.knots = piece.knots().to_vec();
            entity.control_points = piece
                .control_points()
                .iter()
                .map(|point| vector((DVec3::from_array(*point) + shift).to_array()))
                .collect();
            if piece.is_rational() {
                entity.weights = piece.weights().to_vec();
                entity.flags.rational = true;
            }
            EntityType::Spline(entity)
        }
    })
}

/// Each of `edges` as a drawing object in `common`, coloured as the edge is
/// when it has a colour of its own.
fn edge_entities(
    body: &Brep,
    edges: &[u32],
    colors: &EdgeColors,
    common: &EntityCommon,
    shift: DVec3,
) -> Vec<EntityType> {
    edges
        .iter()
        .filter_map(|key| {
            let edge = body.edges.get(*key as usize)?;
            let (curve, interval) = ocs_solid::geometry::labelled(body, edge)?;
            let mut entity = curve_entity(&curve, interval, shift)?;
            let target = entity.common_mut();
            let handle = target.handle;
            *target = common.clone();
            target.handle = handle;
            if let Some(color) = colors.get(key) {
                target.color = *color;
            }
            Some(entity)
        })
        .collect()
}

/// The colours of `before`'s faces and edges carried to `after`: to the face or edge at the same
/// index where it is still on the same surface or curve, else to every one that is.
// ponytail: matched by geometry, so a face an edit moved loses its colour.
fn carried(before: &Brep, after: &Brep, faces: &FaceColors, edges: &EdgeColors) -> (FaceColors, EdgeColors) {
    let mut out_faces = FaceColors::new();
    for (key, color) in faces {
        let Some(old) = before.faces.get(*key as usize) else { continue };
        if after.faces.get(*key as usize).is_some_and(|new| new.surface == old.surface) {
            out_faces.insert(*key, *color);
            continue;
        }
        for (index, new) in after.faces.iter().enumerate() {
            if new.surface == old.surface {
                out_faces.insert(index as u32, *color);
            }
        }
    }
    let mut out_edges = EdgeColors::new();
    for (key, color) in edges {
        let Some(old) = before.edges.get(*key as usize) else { continue };
        let same = |new: &r::Edge| new.curve == old.curve && new.interval == old.interval;
        if after.edges.get(*key as usize).is_some_and(same) {
            out_edges.insert(*key, *color);
            continue;
        }
        for (index, new) in after.edges.iter().enumerate() {
            if same(new) {
                out_edges.insert(index as u32, *color);
            }
        }
    }
    (out_faces, out_edges)
}

/// The planar faces `picked` moved by the rigid motion `point`/`vector`, one at a time: a face
/// sliding within its plane stays, one moving along its normal is offset, one that turns is
/// drafted about where its old and new planes meet. `None` for a curved face.
// ponytail: planar faces only. Solidcore has no general face tweak yet, so a curved face (a
// hole's wall) is refused until it has one.
fn moved_planar_faces(
    body: &Brep,
    picked: &[u32],
    point: impl Fn(DVec3) -> DVec3,
    vector: impl Fn(DVec3) -> DVec3,
) -> Option<Brep> {
    let surfaces = picked
        .iter()
        .map(|face| body.faces.get(*face as usize).map(|node| node.surface.clone()))
        .collect::<Option<Vec<_>>>()?;
    let mut out = body.clone();
    for surface in surfaces {
        // Earlier faces' edits renumber the faces; an unmoved face keeps its surface.
        let face = out.faces.iter().position(|node| node.surface == surface)? as u32;
        let (origin, normal) = model::face_plane(&out, face)?;
        let (p, n) = (DVec3::from_array(origin), DVec3::from_array(normal));
        let (p2, n2) = (point(p), vector(n).try_normalize()?);
        let hinge = n.cross(n2);
        if hinge.length() <= 1e-9 {
            let distance = (p2 - p).dot(n);
            if distance.abs() > 1e-12 {
                out = model::offset_faces(&out, &[face], distance).ok()?;
            }
            continue;
        }
        let h = hinge.normalize();
        let c = n.dot(n2);
        let (a, b) = (n.dot(p), n2.dot(p2));
        let on_hinge = ((a - b * c) * n + (b - a * c) * n2) / (1.0 - c * c);
        // The face is parallel to the pull and turns about the hinge in the neutral plane.
        let pull = h.cross(n);
        let mut along = n2.cross(h);
        if along.dot(pull) < 0.0 {
            along = -along;
        }
        let angle = (-along.dot(n)).atan2(along.dot(pull));
        out = model::draft(&out, &[face], on_hinge.to_array(), pull.to_array(), pull.to_array(), angle).ok()?;
    }
    Some(out)
}

fn fresh_common(source: &EntityCommon) -> EntityCommon {
    let mut common = source.clone();
    common.handle = Handle::NULL;
    common.owner_handle = Handle::NULL;
    common
}

impl super::OpenCADStudio {
    pub(super) fn solid_edit(&mut self, request: SolidEditRequest) -> Task<Message> {
        let i = self.active_tab;
        let handle = request.handle;
        if self.reject_locked_edit(i, handle) {
            return Task::none();
        }
        let Some(source) = self.tabs[i].scene.document.get_entity(handle).cloned() else {
            return Task::none();
        };
        if !matches!(source, EntityType::Solid3D(_)) {
            self.command_line
                .push_error(crate::t!("Select a 3D solid.").as_ref());
            return Task::none();
        }
        let solid = acis_of(&source).and_then(painted).or_else(|| {
            self.tabs[i].scene.restore_solid_models(&[handle]);
            let body = self.tabs[i].scene.solid_models.get(&handle)?.clone();
            Some(Painted { body, faces: HashMap::new(), edges: HashMap::new() })
        });
        let Some(Painted { body, mut faces, mut edges }) = solid else {
            self.command_line
                .push_error(crate::t!("The solid geometry could not be restored.").as_ref());
            return Task::none();
        };
        let unresolved = |app: &mut Self| {
            app.command_line
                .push_error(crate::t!("A selected face or edge could not be resolved.").as_ref());
            Task::none()
        };
        let failed = |app: &mut Self| {
            app.command_line.push_error(
                crate::t!("The edit could not be made on this solid; it was left unchanged.")
                    .as_ref(),
            );
            Task::none()
        };

        let edited = match &request.action {
            SolidEditAction::ExtrudeFaces { height, taper } => {
                let Some(picked) = picked_faces(&body, &request.picks) else {
                    return unresolved(self);
                };
                faces.clear();
                edges.clear();
                model::extrude_faces(&body, &picked, *height, *taper).ok()
            }
            SolidEditAction::MoveFaces { from, to } => {
                let Some(picked) = picked_faces(&body, &request.picks) else {
                    return unresolved(self);
                };
                let shift = *to - *from;
                moved_planar_faces(&body, &picked, |p| p + shift, |v| v)
            }
            SolidEditAction::RotateFaces { origin, axis, angle } => {
                let Some(picked) = picked_faces(&body, &request.picks) else {
                    return unresolved(self);
                };
                let Some(axis) = axis.try_normalize() else {
                    return failed(self);
                };
                let (sin, cos) = angle.sin_cos();
                let turn = move |v: DVec3| v * cos + axis.cross(v) * sin + axis * axis.dot(v) * (1.0 - cos);
                let origin = *origin;
                moved_planar_faces(&body, &picked, move |p| origin + turn(p - origin), turn)
            }
            SolidEditAction::OffsetFaces { distance } => {
                let Some(picked) = picked_faces(&body, &request.picks) else {
                    return unresolved(self);
                };
                model::offset_faces(&body, &picked, *distance).ok()
            }
            SolidEditAction::TaperFaces { base, toward, angle } => {
                let Some(picked) = picked_faces(&body, &request.picks) else {
                    return unresolved(self);
                };
                let axis = (*toward - *base).to_array();
                model::draft(&body, &picked, base.to_array(), axis, axis, *angle).ok()
            }
            SolidEditAction::DeleteFaces => {
                let Some(picked) = picked_faces(&body, &request.picks) else {
                    return unresolved(self);
                };
                model::remove_faces(&body, &picked).ok()
            }
            SolidEditAction::ColorFaces(color) => {
                let Some(picked) = picked_faces(&body, &request.picks) else {
                    return unresolved(self);
                };
                for face in picked {
                    match own_color(*color) {
                        Some(color) => faces.insert(face, color),
                        None => faces.remove(&face),
                    };
                }
                Some(body.clone())
            }
            SolidEditAction::ColorEdges(color) => {
                let Some(picked) = picked_edges(&body, &request.picks) else {
                    return unresolved(self);
                };
                for edge in picked {
                    match own_color(*color) {
                        Some(color) => edges.insert(edge, color),
                        None => edges.remove(&edge),
                    };
                }
                Some(body.clone())
            }
            SolidEditAction::Clean => {
                model::clean(&body).ok()
            }
            SolidEditAction::Imprint { tool, delete_tool } => {
                return self.solid_imprint(handle, &source, body, faces, edges, *tool, *delete_tool);
            }
            SolidEditAction::CopyFaces { from, to } => {
                let Some(picked) = picked_faces(&body, &request.picks) else {
                    return unresolved(self);
                };
                return self.copy_faces(&source, &body, &picked, &faces, *to - *from);
            }
            SolidEditAction::CopyEdges { from, to } => {
                let Some(picked) = picked_edges(&body, &request.picks) else {
                    return unresolved(self);
                };
                let entities =
                    edge_entities(&body, &picked, &edges, &fresh_common(source.common()), *to - *from);
                self.push_undo_snapshot(i, "SOLIDEDIT");
                let count = self.commit_new_curves(entities);
                self.command_line
                    .push_output(crate::tf!("SOLIDEDIT: copied {count} edge(s).").as_ref());
                return Task::none();
            }
            SolidEditAction::Separate => {
                return self.separate_solid(handle, &source, &body, &faces, &edges);
            }
            SolidEditAction::Check => {
                let problems = model::check(&body);
                if problems.is_empty() {
                    self.command_line
                        .push_output(crate::t!("This object is a valid solid.").as_ref());
                } else {
                    self.command_line.push_output(
                        crate::tf!("This object is not a valid solid: {}.", problems.join("; "))
                            .as_ref(),
                    );
                }
                return Task::none();
            }
        };
        let Some(result) = edited else {
            return failed(self);
        };
        if self.replace_solid_body(i, handle, source, &body, &result, &faces, &edges) {
            self.command_line
                .push_output(crate::t!("SOLIDEDIT: solid updated.").as_ref());
        }
        Task::none()
    }

    /// Writes `result` into the solid `handle`, carrying the face and edge
    /// colours over to the nodes it still has, and draws it from that data.
    fn replace_solid_body(
        &mut self,
        i: usize,
        handle: Handle,
        mut source: EntityType,
        before: &Brep,
        result: &Brep,
        faces: &FaceColors,
        edges: &EdgeColors,
    ) -> bool {
        let Some(mut document) = crate::scene::convert::acis_export::solid_to_sat(result) else {
            self.command_line
                .push_error(crate::t!("The result could not be encoded as ACIS.").as_ref());
            return false;
        };
        let (faces, edges) = carried(before, result, faces, edges);
        paint_body(&mut document, result, &faces, &edges);
        if self.tabs[i]
            .scene
            .prepare_solid_model_display(handle, result)
            .is_none_or(|display| !display.0.complete)
        {
            self.command_line.push_error(
                crate::t!("The result could not be displayed completely. The original solid was retained.")
                    .as_ref(),
            );
            return false;
        }
        let EntityType::Solid3D(entity) = &mut source else {
            return false;
        };
        entity.wires = solid_model::edge_wires(result);
        entity.silhouettes.clear();
        entity.history_handle = None;
        entity.set_sat_document(&document);
        self.push_undo_snapshot(i, "SOLIDEDIT");
        // A history replay would rebuild the body without the colours its
        // data now carries.
        self.tabs[i].scene.delete_solid_history(handle);
        if !self.tabs[i].scene.update_entity(source) {
            self.command_line
                .push_error(crate::t!("The solid could not be updated.").as_ref());
            return false;
        }
        self.tabs[i].scene.deselect_all();
        self.tabs[i].scene.select_entity(handle, false);
        self.tabs[i].dirty = true;
        self.refresh_properties();
        true
    }

    /// Adds a body as a new solid in `common`, painted.
    fn add_painted_solid(
        &mut self,
        i: usize,
        common: &EntityCommon,
        before: &Brep,
        body: &Brep,
        faces: &FaceColors,
        edges: &EdgeColors,
    ) -> Option<Handle> {
        let mut document = crate::scene::convert::acis_export::solid_to_sat(body)?;
        let (faces, edges) = carried(before, body, faces, edges);
        paint_body(&mut document, body, &faces, &edges);
        let mut solid = Solid3D::new();
        solid.common = fresh_common(common);
        solid.wires = solid_model::edge_wires(body);
        solid.set_sat_document(&document);
        let handle = self.commit_entity_handle_preserve_style(EntityType::Solid3D(solid))?;
        self.tabs[i].scene.refresh_meshes_for_handles(&[handle]);
        Some(handle)
    }

    pub(super) fn commit_new_curves(&mut self, entities: Vec<EntityType>) -> usize {
        let i = self.active_tab;
        let mut count = 0;
        for entity in entities {
            if self.commit_entity_handle_preserve_style(entity).is_some() {
                count += 1;
            }
        }
        if count > 0 {
            self.tabs[i].dirty = true;
        }
        count
    }

    #[allow(clippy::too_many_arguments)]
    fn solid_imprint(
        &mut self,
        handle: Handle,
        source: &EntityType,
        body: Brep,
        faces: FaceColors,
        edges: EdgeColors,
        tool: Handle,
        delete_tool: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        let Some(tool_entity) = self.tabs[i].scene.document.get_entity(tool).cloned() else {
            return Task::none();
        };
        let result = if let Some(tool_body) = acis_of(&tool_entity).and_then(painted) {
            model::imprint(&body, &tool_body.body).ok().filter(|(_, changed)| *changed).map(|(target, _)| target)
        } else {
            entity_curves(&tool_entity)
                .and_then(|curves| model::imprint_curves(&body, &curves).ok())
                .filter(|(_, cut)| *cut)
                .map(|(target, _)| target)
        };
        let Some(result) = result else {
            self.command_line.push_error(
                crate::t!("The object does not cross any face of the solid; nothing was imprinted.")
                    .as_ref(),
            );
            return Task::none();
        };
        if !self.replace_solid_body(i, handle, source.clone(), &body, &result, &faces, &edges) {
            return Task::none();
        }
        if delete_tool {
            self.tabs[i].scene.erase_entities(&[tool]);
        }
        self.command_line
            .push_output(crate::t!("IMPRINT: the object was imprinted on the solid.").as_ref());
        Task::none()
    }

    fn copy_faces(
        &mut self,
        source: &EntityType,
        body: &Brep,
        picked: &[u32],
        colors: &FaceColors,
        shift: DVec3,
    ) -> Task<Message> {
        let i = self.active_tab;
        self.push_undo_snapshot(i, "SOLIDEDIT");
        let place = model::Placement { origin: shift.to_array(), ..model::Placement::IDENTITY };
        let mut count = 0;
        for face in picked {
            let Some(sheet) = model::face_sheet(body, &[*face])
                .and_then(|sheet| model::placed(&sheet, &place).ok())
            else {
                continue;
            };
            let mut common = fresh_common(source.common());
            if let Some(color) = colors.get(face) {
                common.color = *color;
            }
            let planar = model::face_plane(body, *face).is_some();
            let handle = if planar {
                let mut region = Region::new();
                region.common = common;
                self.add_region_model_preserving_style(region, sheet)
            } else {
                let mut surface = Surface::new(SurfaceKind::Generic);
                surface.common = common;
                self.add_surface_model_preserving_style(EntityType::Surface(Box::new(surface)), sheet)
            };
            if !handle.is_null() {
                count += 1;
            }
        }
        if count > 0 {
            self.tabs[i].dirty = true;
        }
        self.command_line
            .push_output(crate::tf!("SOLIDEDIT: copied {count} face(s).").as_ref());
        Task::none()
    }

    fn separate_solid(
        &mut self,
        handle: Handle,
        source: &EntityType,
        body: &Brep,
        faces: &FaceColors,
        edges: &EdgeColors,
    ) -> Task<Message> {
        let i = self.active_tab;
        let pieces = model::separated(body);
        if pieces.len() < 2 {
            self.command_line.push_output(
                crate::t!("The solid is one piece; there is nothing to separate.").as_ref(),
            );
            return Task::none();
        }
        let mut pieces = pieces.into_iter();
        let first = pieces.next().expect("two pieces");
        if !self.replace_solid_body(i, handle, source.clone(), body, &first, faces, edges) {
            return Task::none();
        }
        let mut count = 1;
        for piece in pieces {
            if self.add_painted_solid(i, source.common(), body, &piece, faces, edges).is_some() {
                count += 1;
            }
        }
        self.command_line
            .push_output(crate::tf!("SOLIDEDIT: separated into {count} solids.").as_ref());
        Task::none()
    }

    /// XEDGES: every edge of the solids, regions and surfaces in `handles`
    /// drawn as curves in each object's layer and colour.
    pub(super) fn extract_edges(&mut self, handles: &[Handle]) -> Task<Message> {
        let i = self.active_tab;
        let mut entities = Vec::new();
        for handle in handles {
            let Some(entity) = self.tabs[i].scene.document.get_entity(*handle) else {
                continue;
            };
            let common = fresh_common(entity.common());
            let solid = acis_of(entity).and_then(painted).or_else(|| {
                self.tabs[i].scene.restore_solid_models(&[*handle]);
                let body = self.tabs[i].scene.solid_models.get(handle)?.clone();
                Some(Painted { body, faces: HashMap::new(), edges: HashMap::new() })
            });
            let Some(solid) = solid else {
                continue;
            };
            let keys: Vec<u32> = (0..solid.body.edges.len() as u32).collect();
            entities.extend(edge_entities(&solid.body, &keys, &solid.edges, &common, DVec3::ZERO));
        }
        if entities.is_empty() {
            self.command_line
                .push_output(crate::t!("XEDGES: the selection has no edges to extract.").as_ref());
            return Task::none();
        }
        self.push_undo_snapshot(i, "XEDGES");
        let count = self.commit_new_curves(entities);
        self.command_line
            .push_output(crate::tf!("XEDGES: extracted {count} edge(s).").as_ref());
        Task::none()
    }
}

/// The curves an object draws, each with its middle, for imprinting.
fn entity_curves(entity: &EntityType) -> Option<Vec<(solidcore_ops::sweep::Curve, [f64; 3])>> {
    use solidcore_ops::sweep::Curve as S;
    let tau = std::f64::consts::TAU;
    let point = |vector: &Vector3| DVec3::new(vector.x, vector.y, vector.z);
    let arc = |center: &Vector3, normal: &Vector3, radius: f64, start: f64, end: f64| {
        let normal = point(normal).try_normalize()?;
        let (x, y) = ocs_axes(normal);
        let centre = x * center.x + y * center.y + normal * center.z;
        let half = 0.5 * (start + end);
        let middle = centre + (x * half.cos() + y * half.sin()) * radius;
        Some((
            S::Arc {
                center: centre.to_array(),
                axis: normal.to_array(),
                reference: x.to_array(),
                radius,
                start,
                end,
            },
            middle.to_array(),
        ))
    };
    Some(match entity {
        EntityType::Line(line) => {
            let (start, end) = (point(&line.start), point(&line.end));
            vec![(S::Line { start: start.to_array(), end: end.to_array() }, ((start + end) * 0.5).to_array())]
        }
        EntityType::Circle(entity) => vec![arc(&entity.center, &entity.normal, entity.radius, 0.0, tau)?],
        EntityType::Arc(entity) => {
            let start = entity.start_angle;
            let end = start + (entity.end_angle - start).rem_euclid(tau);
            vec![arc(&entity.center, &entity.normal, entity.radius, start, end)?]
        }
        _ => return None,
    })
}
