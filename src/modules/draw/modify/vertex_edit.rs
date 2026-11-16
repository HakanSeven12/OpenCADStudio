// ADDVERTEX / DELVERTEX — add or remove polyline vertices by picking.
//
// ADDVERTEX: pick a segment; a new vertex starts on it at the pick and
// follows the cursor until the next click (Enter keeps it at the pick).
// DELVERTEX: pick near a vertex; the nearest vertex is removed.
// Both repeat until Esc or Enter at the pick prompt, one undo step each.
// The edits are the grips' Add Vertex / Remove Vertex.

use document::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::scene::model::object::{GripApply, GripMenuAction};
use crate::entities::traits::EntityTypeOps;
use crate::scene::view::dispatch;

/// Vertex count and closed flag of a polyline the commands edit.
fn polyline_shape(entity: &EntityType) -> Option<(usize, bool)> {
    match entity {
        EntityType::LwPolyline(p) => Some((p.vertices.len(), p.is_closed)),
        EntityType::Polyline2D(p) => Some((p.vertices.len(), p.is_closed())),
        EntityType::Polyline3D(p) => Some((p.vertices.len(), p.is_closed())),
        _ => None,
    }
}

/// World positions of the vertex grips (ids `0..n`) and of the segment
/// midpoint grips where the entity has them (ids `n..`).
fn vertex_points(entity: &EntityType, n: usize) -> (Vec<DVec3>, Vec<Option<DVec3>>) {
    let grips = dispatch::grips(entity);
    let at = |id: usize| grips.iter().find(|g| g.id == id).map(|g| g.world);
    let vertices: Vec<DVec3> = (0..n).filter_map(at).collect();
    let mids = (0..n).map(|k| at(n + k)).collect();
    (vertices, mids)
}

fn segment_distance(p: DVec3, a: DVec3, b: DVec3) -> f64 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (a + ab * t).distance(p)
}

/// The segment nearest `pick` (index of its first vertex), measured on the
/// chord, or through the midpoint grip for an arc segment.
fn nearest_segment(entity: &EntityType, pick: DVec3) -> Option<usize> {
    let (n, closed) = polyline_shape(entity)?;
    let (vertices, mids) = vertex_points(entity, n);
    if vertices.len() != n || n < 2 {
        return None;
    }
    let count = if closed { n } else { n - 1 };
    (0..count)
        .map(|k| {
            let (a, b) = (vertices[k], vertices[(k + 1) % n]);
            let d = match mids[k] {
                Some(m) => segment_distance(pick, a, m).min(segment_distance(pick, m, b)),
                None => segment_distance(pick, a, b),
            };
            (k, d)
        })
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(k, _)| k)
}

/// The bulge of segment `seg` before a vertex is added on it.
fn segment_bulge(entity: &EntityType, seg: usize) -> f64 {
    match entity {
        EntityType::LwPolyline(p) => p.vertices.get(seg).map_or(0.0, |v| v.bulge),
        EntityType::Polyline2D(p) => p.vertices.get(seg).map_or(0.0, |v| v.bulge),
        _ => 0.0,
    }
}

/// `entity` with a vertex added on segment `seg` at world point `at`.
fn with_vertex_at(entity: &EntityType, seg: usize, at: DVec3) -> Option<EntityType> {
    let (n, _) = polyline_shape(entity)?;
    let mut edited = entity.clone();
    edited.apply_grip_menu(seg, GripMenuAction::AddVertex);
    if polyline_shape(&edited)?.0 != n + 1 {
        return None;
    }
    dispatch::apply_grip(&mut edited, seg + 1, GripApply::Absolute(at));
    let bulge = segment_bulge(entity, seg);
    if bulge.abs() >= 1e-9 {
        crate::entities::lwpolyline::refit_added_arc_vertex(&mut edited, seg + 1, bulge);
    }
    Some(edited)
}

/// The world point on segment `seg` nearest `pick`: the new vertex's place
/// when it is kept where it was picked.
fn point_on_segment(entity: &EntityType, seg: usize, pick: DVec3) -> Option<DVec3> {
    let (n, _) = polyline_shape(entity)?;
    let (vertices, mids) = vertex_points(entity, n);
    let (a, b) = (*vertices.get(seg)?, *vertices.get((seg + 1) % n)?);
    let project = |a: DVec3, b: DVec3| {
        let ab = b - a;
        let t = if ab.length_squared() > 0.0 {
            ((pick - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
        } else {
            0.0
        };
        a + ab * t
    };
    Some(match mids.get(seg).copied().flatten() {
        // An arc: the nearer half's chord point, pushed out onto the arc
        // through the circle of the three grips.
        Some(m) => {
            let near = if segment_distance(pick, a, m) <= segment_distance(pick, m, b) {
                project(a, m)
            } else {
                project(m, b)
            };
            circle_through(a, m, b).map_or(near, |(center, radius)| {
                let dir = near - center;
                if dir.length_squared() > 0.0 {
                    center + dir.normalize() * radius
                } else {
                    near
                }
            })
        }
        None => project(a, b),
    })
}

/// Centre and radius of the circle through three points, if they are not
/// in a line.
fn circle_through(a: DVec3, b: DVec3, c: DVec3) -> Option<(DVec3, f64)> {
    let (ab, ac) = (b - a, c - a);
    let normal = ab.cross(ac);
    let denom = 2.0 * normal.length_squared();
    if denom <= 1e-18 {
        return None;
    }
    let center = a
        + (normal.cross(ab) * ac.length_squared() + ac.cross(normal) * ab.length_squared())
            / denom;
    Some((center, center.distance(a)))
}

/// The vertex of `entity` nearest `pick`.
fn nearest_vertex(entity: &EntityType, pick: DVec3) -> Option<usize> {
    let (n, _) = polyline_shape(entity)?;
    let (vertices, _) = vertex_points(entity, n);
    vertices
        .iter()
        .enumerate()
        .min_by(|x, y| x.1.distance(pick).total_cmp(&y.1.distance(pick)))
        .map(|(i, _)| i)
}

fn preview(entity: &EntityType) -> Option<crate::scene::model::wire_model::WireModel> {
    let points: Vec<[f32; 3]> = match crate::entities::curve::entity_curve(entity) {
        Some(curve) => crate::entities::curve::curve_points(&curve)
            .into_iter()
            .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
            .collect(),
        None => {
            let (n, closed) = polyline_shape(entity)?;
            let (mut vertices, _) = vertex_points(entity, n);
            if closed {
                vertices.extend(vertices.first().copied());
            }
            vertices.iter().map(|p| p.as_vec3().to_array()).collect()
        }
    };
    Some(crate::scene::model::wire_model::WireModel::solid(
        "vertex_edit_preview".into(),
        points,
        crate::scene::model::wire_model::WireModel::CYAN,
        false,
    ))
}

// ── ADDVERTEX ───────────────────────────────────────────────────────────────

struct Placing {
    handle: Handle,
    entity: EntityType,
    seg: usize,
    pick: DVec3,
}

pub struct AddVertexCommand {
    picked: Option<EntityType>,
    placing: Option<Placing>,
    edits: usize,
}

impl Default for AddVertexCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl AddVertexCommand {
    pub fn new() -> Self {
        Self {
            picked: None,
            placing: None,
            edits: 0,
        }
    }

    fn commit(&mut self, at: Option<DVec3>) -> CmdResult {
        let Some(placing) = self.placing.take() else {
            return CmdResult::NeedPoint;
        };
        let at = at.or_else(|| point_on_segment(&placing.entity, placing.seg, placing.pick));
        match at.and_then(|at| with_vertex_at(&placing.entity, placing.seg, at)) {
            Some(edited) => CmdResult::ReplaceManyContinue(vec![(placing.handle, vec![edited])]),
            None => CmdResult::NeedPoint,
        }
    }
}

impl CadCommand for AddVertexCommand {
    fn name(&self) -> &'static str {
        "ADDVERTEX"
    }

    fn prompt(&self) -> String {
        if self.placing.is_some() {
            crate::t!("ADDVERTEX  Specify new vertex location:").into_owned()
        } else {
            crate::t!("ADDVERTEX  Select a polyline segment or [Undo]:").into_owned()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.placing.is_none() && self.edits > 0 {
            vec![CmdOption::new("Undo", "U")]
        } else {
            Vec::new()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.placing.is_none()
    }
    fn inject_before_entity_pick(&self) -> bool {
        true
    }
    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        let Some(entity) = self.picked.take() else {
            return CmdResult::NeedPoint;
        };
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        if let Some(seg) = nearest_segment(&entity, pt) {
            self.placing = Some(Placing {
                handle,
                entity,
                seg,
                pick: pt,
            });
        }
        CmdResult::NeedPoint
    }

    fn on_entity_replaced(&mut self, _old: Handle, _new_handles: &[Handle]) {
        self.edits += 1;
    }

    fn wants_text_input(&self) -> bool {
        self.placing.is_none()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let upper = text.trim().to_uppercase();
        if upper.is_empty() {
            return Some(self.on_enter());
        }
        Some(
            if matches!(upper.as_str(), "U" | "UNDO") && self.edits > 0 {
                self.edits -= 1;
                CmdResult::UndoDocument
            } else {
                CmdResult::NeedPoint
            },
        )
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        self.commit(Some(pt))
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<crate::scene::model::wire_model::WireModel> {
        let placing = self.placing.as_ref()?;
        preview(&with_vertex_at(&placing.entity, placing.seg, pt)?)
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.placing.is_some() {
            self.commit(None)
        } else {
            CmdResult::Cancel
        }
    }
}

// ── DELVERTEX ───────────────────────────────────────────────────────────────

pub struct DelVertexCommand {
    picked: Option<EntityType>,
    edits: usize,
}

impl Default for DelVertexCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl DelVertexCommand {
    pub fn new() -> Self {
        Self {
            picked: None,
            edits: 0,
        }
    }
}

impl CadCommand for DelVertexCommand {
    fn name(&self) -> &'static str {
        "DELVERTEX"
    }

    fn prompt(&self) -> String {
        crate::t!("DELVERTEX  Select a polyline vertex or [Undo]:").into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.edits > 0 {
            vec![CmdOption::new("Undo", "U")]
        } else {
            Vec::new()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }
    fn inject_before_entity_pick(&self) -> bool {
        true
    }
    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        let Some(entity) = self.picked.take() else {
            return CmdResult::NeedPoint;
        };
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let Some((n, _)) = polyline_shape(&entity) else {
            return CmdResult::NeedPoint;
        };
        let Some(vertex) = nearest_vertex(&entity, pt) else {
            return CmdResult::NeedPoint;
        };
        let mut edited = entity;
        edited.apply_grip_menu(vertex, GripMenuAction::RemoveVertex);
        if polyline_shape(&edited).is_some_and(|(len, _)| len + 1 == n) {
            CmdResult::ReplaceManyContinue(vec![(handle, vec![edited])])
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_entity_replaced(&mut self, _old: Handle, _new_handles: &[Handle]) {
        self.edits += 1;
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let upper = text.trim().to_uppercase();
        if upper.is_empty() {
            return Some(self.on_enter());
        }
        Some(
            if matches!(upper.as_str(), "U" | "UNDO") && self.edits > 0 {
                self.edits -= 1;
                CmdResult::UndoDocument
            } else {
                CmdResult::NeedPoint
            },
        )
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
