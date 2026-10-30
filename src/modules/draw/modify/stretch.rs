// Stretch tool — ribbon definition + interactive command.
//
// Command:  STRETCH (SS)
//   Workflow:
//     1. Pick first corner of the crossing window (right-to-left = crossing).
//     2. Pick second corner.
//     3. Pick base point.
//     4. Pick new point → stretches only vertices inside the crossing window.
//
//   Entity behaviour:
//     Line        : move start if inside, move end if inside, move both if both inside.
//     LwPolyline  : move each vertex independently.
//     Polyline/P2D: move each vertex independently.
//     Arc         : ends inside move; a lone end keeps the arc's height.
//     Circle      : move the whole entity if its center is inside the window.
//     Insert      : move the whole entity if its insertion point is inside.
//     Spline      : control and fit points inside move.
//     All others  : left as they are.

use crate::t;
use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

// ── Stretching one entity ──────────────────────────────────────────────────

/// Stretch `entity` by `delta`: the points of it that lie in one of the
/// crossing `windows` (world XY) move, the rest stay. Arcs and polyline arc
/// segments keep their height (sagitta) as the chord under them changes.
/// Returns whether anything moved. The command's preview and its commit
/// both go through here.
pub fn stretch_entity(
    entity: &mut codec::EntityType,
    windows: &[(DVec3, DVec3)],
    delta: DVec3,
) -> bool {
    stretch_entity_except(entity, Handle::NULL, windows, &[], delta)
}

/// Whether `point` of `handle` was taken out of the stretch set with Shift.
pub(crate) fn is_excluded(excluded: &[(Handle, DVec3)], handle: Handle, point: DVec3) -> bool {
    excluded.iter().any(|(owner, removed)| {
        *owner == handle
            && (removed.x - point.x).abs() <= 1e-9
            && (removed.y - point.y).abs() <= 1e-9
    })
}

/// [`stretch_entity`] for the entity `handle`, leaving its points listed in
/// `excluded` in place. Exclusions affect the moved vertices, not the
/// crossing selection itself.
pub(crate) fn stretch_entity_except(
    entity: &mut codec::EntityType,
    handle: Handle,
    windows: &[(DVec3, DVec3)],
    excluded: &[(Handle, DVec3)],
    delta: DVec3,
) -> bool {
    let in_win = |x: f64, y: f64| -> bool {
        windows.iter().any(|(win_min, win_max)| {
            x >= win_min.x && x <= win_max.x && y >= win_min.y && y <= win_max.y
        }) && !is_excluded(excluded, handle, DVec3::new(x, y, 0.0))
    };
    let (dx, dy, dz) = (delta.x, delta.y, delta.z);
    let mut stretched = false;
    match entity {
        codec::EntityType::Line(l) => {
            let s_in = in_win(l.start.x, l.start.y);
            let e_in = in_win(l.end.x, l.end.y);
            if s_in {
                l.start.x += dx;
                l.start.y += dy;
                l.start.z += dz;
                stretched = true;
            }
            if e_in {
                l.end.x += dx;
                l.end.y += dy;
                l.end.z += dz;
                stretched = true;
            }
        }
        codec::EntityType::LwPolyline(p) => {
            let Some(mut world) = crate::entities::curve::lwpolyline_world_xy(p) else {
                return false;
            };
            let before: Vec<[f64; 2]> =
                world.vertices.iter().map(|v| [v.location.x, v.location.y]).collect();
            for v in &mut world.vertices {
                if in_win(v.location.x, v.location.y) {
                    v.location.x += dx;
                    v.location.y += dy;
                    stretched = true;
                }
            }
            if stretched {
                let after: Vec<[f64; 2]> =
                    world.vertices.iter().map(|v| [v.location.x, v.location.y]).collect();
                let closed = world.is_closed;
                for (k, v) in world.vertices.iter_mut().enumerate() {
                    v.bulge = held_bulge(v.bulge, &before, &after, k, closed);
                }
                *p = world;
            }
        }
        codec::EntityType::Polyline2D(p) => {
            let before: Vec<[f64; 2]> =
                p.vertices.iter().map(|v| [v.location.x, v.location.y]).collect();
            for v in &mut p.vertices {
                if in_win(v.location.x, v.location.y) {
                    v.location.x += dx;
                    v.location.y += dy;
                    stretched = true;
                }
            }
            if stretched {
                let after: Vec<[f64; 2]> =
                    p.vertices.iter().map(|v| [v.location.x, v.location.y]).collect();
                let closed = p.is_closed();
                for (k, v) in p.vertices.iter_mut().enumerate() {
                    v.bulge = held_bulge(v.bulge, &before, &after, k, closed);
                }
            }
        }
        codec::EntityType::Polyline(p) => {
            for v in &mut p.vertices {
                if in_win(v.location.x, v.location.y) {
                    v.location.x += dx;
                    v.location.y += dy;
                    stretched = true;
                }
            }
        }
        codec::EntityType::Arc(a) => stretched = stretch_arc(a, &in_win, delta),
        codec::EntityType::Circle(c) => {
            if in_win(c.center.x, c.center.y) {
                c.center.x += dx;
                c.center.y += dy;
                c.center.z += dz;
                stretched = true;
            }
        }
        codec::EntityType::Ellipse(e) => {
            if in_win(e.center.x, e.center.y) {
                e.center.x += dx;
                e.center.y += dy;
                e.center.z += dz;
                stretched = true;
            }
        }
        codec::EntityType::Insert(ins) => {
            if in_win(ins.insert_point.x, ins.insert_point.y) {
                ins.insert_point.x += dx;
                ins.insert_point.y += dy;
                ins.insert_point.z += dz;
                stretched = true;
            }
        }
        codec::EntityType::Text(t) => {
            use codec::entities::{TextHorizontalAlignment as HA, TextVerticalAlignment as VA};
            let shift = |p: &mut codec::types::Vector3| {
                p.x += dx;
                p.y += dy;
                p.z += dz;
            };
            let justified =
                !matches!((t.horizontal_alignment, t.vertical_alignment), (HA::Left, VA::Baseline));
            match t.alignment_point.as_mut() {
                // Aligned and Fit text runs between its two points: each
                // stretches on its own.
                Some(a) if matches!(t.horizontal_alignment, HA::Aligned | HA::Fit) => {
                    if in_win(t.insertion_point.x, t.insertion_point.y) {
                        shift(&mut t.insertion_point);
                        stretched = true;
                    }
                    if in_win(a.x, a.y) {
                        shift(a);
                        stretched = true;
                    }
                }
                // Other justified text is placed by its alignment point; the
                // first point moves with it.
                Some(a) if justified => {
                    if in_win(a.x, a.y) {
                        shift(a);
                        shift(&mut t.insertion_point);
                        stretched = true;
                    }
                }
                _ => {
                    if in_win(t.insertion_point.x, t.insertion_point.y) {
                        shift(&mut t.insertion_point);
                        stretched = true;
                    }
                }
            }
        }
        codec::EntityType::MText(t) => {
            if in_win(t.insertion_point.x, t.insertion_point.y) {
                t.insertion_point.x += dx;
                t.insertion_point.y += dy;
                t.insertion_point.z += dz;
                stretched = true;
            }
        }
        codec::EntityType::Viewport(vp) => {
            stretched = windows.iter().any(|(win_min, win_max)| {
                crate::entities::viewport::stretch(vp, *win_min, *win_max, delta)
            });
        }
        codec::EntityType::Dimension(dim) => {
            use codec::entities::Dimension;
            // Move every definition point that falls inside the
            // window — the same points the grips expose — then
            // refresh the stored measurement so the value tracks
            // the stretched geometry.
            let mv = |p: &mut codec::types::Vector3| {
                if in_win(p.x, p.y) {
                    p.x += dx;
                    p.y += dy;
                    p.z += dz;
                    true
                } else {
                    false
                }
            };
            match &mut **dim {
                Dimension::Linear(d) => {
                    stretched |= mv(&mut d.first_point);
                    stretched |= mv(&mut d.second_point);
                    stretched |= mv(&mut d.definition_point);
                }
                Dimension::Aligned(d) => {
                    stretched |= mv(&mut d.first_point);
                    stretched |= mv(&mut d.second_point);
                    stretched |= mv(&mut d.definition_point);
                }
                Dimension::Radius(d) => {
                    stretched |= mv(&mut d.angle_vertex);
                    stretched |= mv(&mut d.definition_point);
                }
                Dimension::Diameter(d) => {
                    stretched |= mv(&mut d.angle_vertex);
                    stretched |= mv(&mut d.definition_point);
                }
                Dimension::Angular2Ln(d) => {
                    stretched |= mv(&mut d.angle_vertex);
                    stretched |= mv(&mut d.first_point);
                    stretched |= mv(&mut d.second_point);
                    stretched |= mv(&mut d.definition_point);
                }
                Dimension::Angular3Pt(d) => {
                    stretched |= mv(&mut d.angle_vertex);
                    stretched |= mv(&mut d.first_point);
                    stretched |= mv(&mut d.second_point);
                    stretched |= mv(&mut d.definition_point);
                }
                Dimension::Ordinate(d) => {
                    stretched |= mv(&mut d.feature_location);
                    stretched |= mv(&mut d.leader_endpoint);
                }
                Dimension::Arc(d) => {
                    stretched |= mv(&mut d.definition_point);
                    stretched |= mv(&mut d.first_extension_point);
                    stretched |= mv(&mut d.second_extension_point);
                    stretched |= mv(&mut d.center_point);
                    if d.has_leader {
                        stretched |= mv(&mut d.first_leader_point);
                        stretched |= mv(&mut d.second_leader_point);
                    }
                }
                Dimension::LargeRadial(d) => {
                    stretched |= mv(&mut d.definition_point);
                    stretched |= mv(&mut d.chord_point);
                    stretched |= mv(&mut d.override_center);
                    stretched |= mv(&mut d.jog_point);
                }
            }
            // Pinned text follows too; the zero sentinel means
            // "auto placement" and must not be captured by a
            // window that happens to cover the origin.
            let t = dim.base().text_middle_point;
            if t.x * t.x + t.y * t.y + t.z * t.z > 1e-16 {
                let mut t = t;
                if mv(&mut t) {
                    dim.base_mut().text_middle_point = t;
                    stretched = true;
                }
            }
            if stretched {
                dim.base_mut().actual_measurement = dim.stored_measurement();
            }
        }
        codec::EntityType::Polyline3D(p) => {
            for v in &mut p.vertices {
                if in_win(v.position.x, v.position.y) {
                    v.position.x += dx;
                    v.position.y += dy;
                    v.position.z += dz;
                    stretched = true;
                }
            }
        }
        // The control and fit points in the window move; the rest stay.
        codec::EntityType::Spline(s) => {
            for p in s.control_points.iter_mut().chain(s.fit_points.iter_mut()) {
                if in_win(p.x, p.y) {
                    p.x += dx;
                    p.y += dy;
                    p.z += dz;
                    stretched = true;
                }
            }
        }
        codec::EntityType::Point(p) => {
            if in_win(p.location.x, p.location.y) {
                p.location.x += dx;
                p.location.y += dy;
                p.location.z += dz;
                stretched = true;
            }
        }
        _ => {}
    }
    stretched
}

/// The bulge of the segment from vertex `k` once its ends moved from
/// `before` to `after`: the arc keeps its height, bulge · chord / 2.
pub(crate) fn held_bulge(
    bulge: f64,
    before: &[[f64; 2]],
    after: &[[f64; 2]],
    k: usize,
    closed: bool,
) -> f64 {
    let next = if k + 1 < before.len() {
        k + 1
    } else if closed {
        0
    } else {
        return bulge;
    };
    let chord = |p: &[[f64; 2]]| (p[next][0] - p[k][0]).hypot(p[next][1] - p[k][1]);
    let (old, new) = (chord(before), chord(after));
    if bulge == 0.0 || old <= 1e-12 || new <= 1e-12 {
        bulge
    } else {
        bulge * old / new
    }
}

/// Stretch an arc by its ends: both in the window move it whole; one moves
/// that end while the arc keeps its height over the new chord.
fn stretch_arc(arc: &mut codec::entities::Arc, in_win: &dyn Fn(f64, f64) -> bool, delta: DVec3) -> bool {
    let (start, finish) = (arc.start_point_wcs(), arc.end_point_wcs());
    stretch_arc_endpoints(arc, in_win(start.x, start.y), in_win(finish.x, finish.y), delta)
}

/// Apply fixed endpoint membership, shared by ordinary and dynamic Stretch.
pub(crate) fn stretch_arc_endpoints(
    arc: &mut codec::entities::Arc, start_in: bool, finish_in: bool, delta: DVec3,
) -> bool {
    use codec::types::{Matrix3, Vector3};
    let ocs = Matrix3::arbitrary_axis(arc.normal);
    let end = |angle: f64| {
        Vector3::new(
            arc.center.x + arc.radius * angle.cos(),
            arc.center.y + arc.radius * angle.sin(),
            arc.center.z,
        )
    };
    let (start, finish) = (end(arc.start_angle), end(arc.end_angle));
    if !start_in && !finish_in {
        return false;
    }
    let local = ocs.transpose() * Vector3::new(delta.x, delta.y, delta.z);
    if start_in && finish_in {
        arc.center = arc.center + local;
        return true;
    }
    let sweep = (arc.end_angle - arc.start_angle).rem_euclid(std::f64::consts::TAU);
    let shift = |p: Vector3, moved: bool| if moved { p + local } else { p };
    let (a, b) = (shift(start, start_in), shift(finish, finish_in));
    let old_chord = (finish.x - start.x).hypot(finish.y - start.y);
    let new_chord = (b.x - a.x).hypot(b.y - a.y);
    if old_chord <= 1e-12 || new_chord <= 1e-12 {
        return false;
    }
    let bulge = (sweep / 4.0).tan() * old_chord / new_chord;
    // The arc through `a` and `b` with that bulge, counter-clockwise from
    // `a`: its centre sits off the chord's midpoint by the apothem.
    let sweep = 4.0 * bulge.atan();
    let radius = new_chord / (2.0 * (sweep / 2.0).sin());
    let apothem = radius * (sweep / 2.0).cos();
    let (ux, uy) = ((b.x - a.x) / new_chord, (b.y - a.y) / new_chord);
    let centre = Vector3::new(
        (a.x + b.x) / 2.0 - uy * apothem,
        (a.y + b.y) / 2.0 + ux * apothem,
        arc.center.z,
    );
    arc.center = centre;
    arc.radius = radius.abs();
    arc.start_angle = (a.y - centre.y).atan2(a.x - centre.x);
    arc.end_angle = (b.y - centre.y).atan2(b.x - centre.x);
    true
}

// ── Ribbon definition ──────────────────────────────────────────────────────

pub fn tool() -> ToolDef {
    ToolDef {
        id: "STRETCH",
        label: "Stretch",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/stretch.svg")),
        event: ModuleEvent::Command("STRETCH".to_string()),
    }
}

// ── Command implementation ─────────────────────────────────────────────────

enum Step {
    /// Waiting for the first corner of another crossing window.
    WindowCorner1,
    /// Waiting for the opposite corner.
    WindowCorner2(DVec3, bool),
    /// Selection is complete; waiting for the displacement base point.
    Base,
    /// Waiting for the displacement target.
    Target { base: DVec3 },
}

pub struct StretchCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    /// Independent crossing windows accumulated during the selection stage.
    windows: Vec<(DVec3, DVec3)>,
    excluded_points: Vec<(Handle, DVec3)>,
    /// The objects the drag really changes. A crossing window also catches
    /// objects that only pass through it; those stay drawn and snappable
    /// instead of being hidden behind the preview (#1989). Resolved on the
    /// first preview, once the windows are final.
    moving: Option<Vec<Handle>>,
    remove_release_pending: bool,
    shift_down: bool,
    step: Step,
}

impl StretchCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            windows: Vec::new(),
            excluded_points: Vec::new(),
            moving: None,
            remove_release_pending: false,
            shift_down: false,
            step: Step::WindowCorner1,
        }
    }

    /// A selection made by a crossing window before the command: that window
    /// marks what moves, so the command goes straight to the base point.
    pub fn preselected(
        handles: Vec<Handle>,
        wire_models: Vec<WireModel>,
        window: (DVec3, DVec3),
    ) -> Self {
        Self {
            handles,
            wire_models,
            windows: vec![window],
            excluded_points: Vec::new(),
            moving: None,
            remove_release_pending: false,
            shift_down: false,
            step: Step::Base,
        }
    }

    /// Continue gathering crossing windows after the host has resolved the
    /// entities touched by the latest window.
    pub fn with_windows(
        handles: Vec<Handle>,
        wire_models: Vec<WireModel>,
        windows: Vec<(DVec3, DVec3)>,
        excluded_points: Vec<(Handle, DVec3)>,
    ) -> Self {
        Self {
            handles,
            wire_models,
            windows,
            excluded_points,
            moving: None,
            remove_release_pending: false,
            shift_down: false,
            step: Step::WindowCorner1,
        }
    }
    /// A window enclosing every vertex of the current selection, or `None`
    /// when nothing is selected. Padded so a vertex exactly on the boundary
    /// counts as inside rather than depending on the comparison's edge.
    fn selection_bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        let mut any = false;
        for wire in &self.wire_models {
            for index in 0..wire.points.len() {
                let point = wire_point(wire, index);
                if !point.is_finite() {
                    continue;
                }
                min = min.min(point);
                max = max.max(point);
                any = true;
            }
        }
        if !any {
            return None;
        }
        let pad = (max - min).length().max(1.0) * 1e-6;
        Some((min - DVec3::splat(pad), max + DVec3::splat(pad)))
    }
}

/// A wire vertex rebuilt from its double-single halves.
fn wire_point(wire: &WireModel, index: usize) -> DVec3 {
    let high = wire.points[index];
    let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
    DVec3::new(
        high[0] as f64 + low[0] as f64,
        high[1] as f64 + low[1] as f64,
        high[2] as f64 + low[2] as f64,
    )
}

impl CadCommand for StretchCommand {
    fn name(&self) -> &'static str {
        "STRETCH"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::WindowCorner1 => {
                if self.windows.is_empty() && self.handles.is_empty() {
                    t!("STRETCH  Specify first corner of crossing window:").into_owned()
                } else {
                    t!(
                        "STRETCH  Specify first corner of another crossing window, or press Enter to continue:"
                    )
                    .into_owned()
                }
            }
            Step::WindowCorner2(..) => t!("STRETCH  Specify opposite corner:").into_owned(),
            Step::Base => t!("STRETCH  Specify base point:").into_owned(),
            Step::Target { base } => {
                let bx = format!("{:.3}", base.x);
                let bz = format!("{:.3}", base.z);
                t!(
                    "STRETCH  Specify new point  [base %{bx},%{bz}]:",
                    bx = bx,
                    bz = bz
                )
                .into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::WindowCorner1 => {
                // Remember Shift at the FIRST corner. The user can release it
                // while choosing the second corner without turning subtraction
                // into an ordinary additive crossing window.
                self.step = Step::WindowCorner2(pt, self.shift_down && !self.windows.is_empty());
                CmdResult::NeedPoint
            }

            Step::WindowCorner2(c1, subtract) => {
                let win_min = c1.min(pt);
                let win_max = c1.max(pt);

                // A Shift+window subtracts only currently active vertices.
                // The host uses its filtered stretch grips, rather than
                // reselecting whole entities or guessing tessellation points.
                let mut windows = self.windows.clone();
                let subtract_window = if *subtract {
                    Some((win_min, win_max))
                } else {
                    windows.push((win_min, win_max));
                    None
                };

                CmdResult::StretchWindow {
                    handles: self.handles.clone(),
                    windows,
                    excluded_points: self.excluded_points.clone(),
                    subtract_window,
                }
            }

            Step::Base => {
                self.step = Step::Target { base: pt };
                CmdResult::NeedPoint
            }

            Step::Target { base } => {
                let delta = pt - *base;

                CmdResult::StretchEntities {
                    handles: self.handles.clone(),
                    windows: self.windows.clone(),
                    excluded_points: self.excluded_points.clone(),
                    delta,
                }
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        if let Step::WindowCorner1 = self.step {
            // Preserve the existing preselection behaviour: if STRETCH started
            // from a selected set and no explicit crossing window was drawn,
            // treat its complete bounds as the stretch window.
            if self.windows.is_empty() {
                if let Some((win_min, win_max)) = self.selection_bounds() {
                    self.windows.push((win_min, win_max));
                }
            }

            if !self.handles.is_empty() && !self.windows.is_empty() {
                self.step = Step::Base;
                return CmdResult::NeedPoint;
            }
        }

        CmdResult::Cancel
    }
    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn set_shift(&mut self, shift: bool) {
        self.shift_down = shift;
    }

    fn remove_stretch_point(&mut self, handle: Handle, world: DVec3) -> bool {
        if !matches!(self.step, Step::WindowCorner1)
            || !self.handles.contains(&handle)
            || !self.windows.iter().any(|(min, max)| {
                world.x >= min.x && world.x <= max.x && world.y >= min.y && world.y <= max.y
            })
        {
            return false;
        }
        if !is_excluded(&self.excluded_points, handle, world) {
            self.excluded_points.push((handle, world));
        }
        self.remove_release_pending = true;
        true
    }

    fn consume_stretch_remove_release(&mut self) -> bool {
        std::mem::take(&mut self.remove_release_pending)
    }

    fn window_corner_pick(&self) -> bool {
        // The two crossing-window corners are free points; Ortho/Polar must not
        // pin the opposite corner to an axis or the window becomes a line (#291).
        matches!(self.step, Step::WindowCorner1 | Step::WindowCorner2(_, _))
    }

    fn selects_by_window(&self) -> bool {
        // Crossing windows select what stretches; dynamic input has nothing
        // to enter there (#657).
        self.window_corner_pick()
    }

    fn window_first_corner(&self) -> Option<DVec3> {
        // Expose the first corner so the host draws a filled crossing marquee to
        // the cursor, matching a normal box selection instead of a bare outline.
        match &self.step {
            Step::WindowCorner2(c1, _) => Some(*c1),
            _ => None,
        }
    }

    fn preview_entities(&mut self, document: &codec::Document, pt: DVec3) -> Vec<codec::EntityType> {
        // The base-point step already has the final windows: resolve what
        // moves there, so the drag starts with only those objects hidden.
        if matches!(self.step, Step::Base | Step::Target { .. }) && self.moving.is_none() {
            let moving: Vec<Handle> = self
                .handles
                .iter()
                .copied()
                .filter(|handle| {
                    document.get_entity(*handle).is_some_and(|entity| {
                        let mut probe = entity.clone();
                        let (windows, excluded) = (&self.windows, &self.excluded_points);
                        stretch_entity_except(&mut probe, *handle, windows, excluded, DVec3::X)
                    })
                })
                .collect();
            self.wire_models.retain(|wire| {
                crate::scene::Scene::handle_from_wire_name(&wire.name)
                    .is_some_and(|handle| moving.contains(&handle))
            });
            self.moving = Some(moving);
        }
        let Step::Target { base } = &self.step else {
            return Vec::new();
        };
        let delta = pt - *base;
        self.moving
            .iter()
            .flatten()
            .filter_map(|handle| document.get_entity(*handle).map(|entity| (*handle, entity)))
            .map(|(handle, entity)| {
                let mut entity = entity.clone();
                let excluded = &self.excluded_points;
                stretch_entity_except(&mut entity, handle, &self.windows, excluded, delta);
                entity
            })
            .collect()
    }

    fn preview_hidden_handles(&self) -> &[Handle] {
        match self.step {
            Step::Target { .. } => self.moving.as_deref().unwrap_or(&self.handles),
            _ => &[],
        }
    }

    fn preview_original_snap_wires(&self) -> &[WireModel] {
        if matches!(self.step, Step::Target { .. }) {
            &self.wire_models
        } else {
            &[]
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        match &self.step {
            // The crossing-window rectangle is drawn as a filled selection
            // marquee by the host (via window_first_corner) so it matches a
            // normal box selection — nothing to draw here. (#291)
            Step::WindowCorner2(_, _) => vec![],
            // STRETCH hides the original resident geometry. Draw a pale,
            // stationary ghost from its snapshot *behind* the moving geometry.
            // Do not replace the snapshot itself: it remains the snap source.
            Step::Target { base } => {
                let mut wires: Vec<WireModel> = self
                    .wire_models
                    .iter()
                    .filter(|wire| wire.display_visible)
                    .cloned()
                    .map(|mut wire| {
                        wire.selected = false;
                        wire.color = [0.67, 0.79, 0.84, wire.color[3] * 0.38];
                        wire.aci = 0;
                        wire.reference_only = true;
                        wire.plot_visible = false;
                        wire
                    })
                    .collect();
                wires.push(WireModel::solid_f64(
                    "rubber_band".into(),
                    vec![[base.x, base.y, base.z], [pt.x, pt.y, pt.z]],
                    WireModel::CYAN,
                    false,
                ));
                wires
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod shift_stretch_tests {
    use super::*;
    use codec::entities::Line;
    use codec::types::Vector3;

    #[test]
    fn shift_window_subtracts_selected_vertices_without_adding_a_crossing_window() {
        let original = vec![(DVec3::ZERO, DVec3::new(20.0, 20.0, 0.0))];
        let mut cmd = StretchCommand::with_windows(
            vec![Handle::NULL], vec![], original.clone(), vec![],
        );
        // The first corner latches subtraction: releasing Shift before the
        // second click must not turn it back into an additive selection.
        cmd.set_shift(true);
        assert!(matches!(cmd.on_point(DVec3::new(5.0, 7.0, 0.0)), CmdResult::NeedPoint));
        cmd.set_shift(false);
        match cmd.on_point(DVec3::new(12.0, 15.0, 0.0)) {
            CmdResult::StretchWindow {
                windows,
                subtract_window: Some((min, max)),
                ..
            } => {
                assert_eq!(windows, original);
                assert_eq!(min, DVec3::new(5.0, 7.0, 0.0));
                assert_eq!(max, DVec3::new(12.0, 15.0, 0.0));
            }
            _ => panic!("Shift selection must subtract existing STRETCH vertices"),
        }
    }

    #[test]
    fn normal_window_still_adds_to_the_stretch_selection() {
        let original = vec![(DVec3::ZERO, DVec3::new(20.0, 20.0, 0.0))];
        let mut cmd = StretchCommand::with_windows(vec![], vec![], original.clone(), vec![]);
        assert!(matches!(cmd.on_point(DVec3::new(24.0, 4.0, 0.0)), CmdResult::NeedPoint));
        match cmd.on_point(DVec3::new(30.0, 10.0, 0.0)) {
            CmdResult::StretchWindow {
                windows,
                subtract_window: None,
                ..
            } => {
                assert_eq!(windows.len(), 2);
                assert_eq!(windows[0], original[0]);
            }
            _ => panic!("Ordinary STRETCH crossing window must still add"),
        }
    }

    #[test]
    fn excluded_endpoint_stays_fixed_while_other_endpoint_stretches() {
        let mut entity = codec::EntityType::Line(Line::from_points(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        ));
        let window = [(DVec3::new(-1.0, -1.0, -1.0), DVec3::new(11.0, 1.0, 1.0))];
        let handle = Handle::new(7);
        let excluded = [(handle, DVec3::ZERO)];
        assert!(stretch_entity_except(
            &mut entity,
            handle,
            &window,
            &excluded,
            DVec3::new(5.0, 0.0, 0.0),
        ));
        let codec::EntityType::Line(line) = entity else { panic!("line expected") };
        assert_eq!(line.start, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(line.end, Vector3::new(15.0, 0.0, 0.0));
    }

    #[test]
    fn shift_removed_point_survives_command_to_commit() {
        let handle = Handle::new(42);
        let window = (DVec3::new(-1.0, -1.0, -1.0), DVec3::new(11.0, 1.0, 1.0));
        let mut cmd = StretchCommand::with_windows(
            vec![handle], vec![], vec![window], Vec::new(),
        );
        assert!(cmd.remove_stretch_point(handle, DVec3::ZERO));
        assert!(cmd.consume_stretch_remove_release());
        assert!(!cmd.consume_stretch_remove_release());
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::ZERO), CmdResult::NeedPoint));
        let CmdResult::StretchEntities { excluded_points, .. } =
            cmd.on_point(DVec3::new(5.0, 0.0, 0.0))
        else { panic!("expected STRETCH result") };
        assert_eq!(excluded_points, vec![(handle, DVec3::ZERO)]);
    }

    #[test]
    fn only_objects_the_drag_moves_are_hidden() {
        let mut document = codec::Document::new();
        let mut add = |a: Vector3, b: Vector3| {
            let line = codec::EntityType::Line(Line::from_points(a, b));
            document.add_entity(line).unwrap()
        };
        // One end inside the window, and a line merely passing through it.
        let moved = add(Vector3::new(5.0, 5.0, 0.0), Vector3::new(50.0, 5.0, 0.0));
        let crossing = add(Vector3::new(-20.0, 2.0, 0.0), Vector3::new(30.0, 2.0, 0.0));
        let window = (DVec3::new(0.0, 0.0, -1.0), DVec3::new(10.0, 10.0, 1.0));
        let mut cmd =
            StretchCommand::with_windows(vec![moved, crossing], vec![], vec![window], vec![]);
        assert!(matches!(cmd.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(cmd.on_point(DVec3::ZERO), CmdResult::NeedPoint));
        let preview = cmd.preview_entities(&document, DVec3::new(0.0, 3.0, 0.0));
        assert_eq!(preview.len(), 1);
        assert_eq!(cmd.preview_hidden_handles(), &[moved]);
    }
}
