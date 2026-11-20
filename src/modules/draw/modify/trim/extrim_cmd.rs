use super::*;

// ══════════════════════════════════════════════════════════════════════════
// ExtrimCommand — EXTRIM (Express-Tools cookie-cutter trim). #253
//
// Pick one boundary, then a side: every object crossing the boundary is trimmed
// on the picked side, and objects lying wholly on that side are erased. The side
// test is a parity count — a segment from a candidate point to the pick point
// that crosses the boundary an even number of times lands on the pick side.
// ══════════════════════════════════════════════════════════════════════════

/// Extend `Geo::Line` boundary edges so a line boundary cuts across the whole
/// drawing (EXTRIM treats a line boundary as infinite).

pub(crate) fn extend_line_geos(geos: &mut [Geo]) {
    for g in geos.iter_mut() {
        if let Geo::Line { handle, p1, p2 } = g {
            let (dx, dy) = (p2[0] - p1[0], p2[1] - p1[1]);
            if dx.hypot(dy) > 1e-9 {
                // Same as `imply_edge_geos`: the boundary reaches as far as its
                // line does, so say so rather than picking a length.
                *g = Geo::InfLine {
                    handle: *handle,
                    bx: p1[0],
                    by: p1[1],
                    dx,
                    dy,
                };
            }
        }
    }
}

/// Kept parametric intervals — those whose midpoint is NOT on the pick side.

pub(crate) fn extrim_keep(
    ts: &[f64],
    point_at: &dyn Fn(f64) -> [f64; 2],
    on_pick_side: &dyn Fn([f64; 2]) -> bool,
) -> Vec<(f64, f64)> {
    let mut bounds = vec![0.0f64];
    bounds.extend_from_slice(ts);
    bounds.push(1.0);
    bounds.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
    bounds
        .windows(2)
        .filter_map(|w| {
            if w[1] - w[0] <= 1e-6 {
                return None;
            }
            let mid = point_at((w[0] + w[1]) * 0.5);
            if on_pick_side(mid) {
                None
            } else {
                Some((w[0], w[1]))
            }
        })
        .collect()
}

pub(crate) fn extrim_line(
    orig: &LineEnt,
    ts: &[f64],
    side: &dyn Fn([f64; 2]) -> bool,
) -> Vec<EntityType> {
    let p1 = [orig.start.x, orig.start.y];
    let p2 = [orig.end.x, orig.end.y];
    let z = orig.start.z;
    let pa = |t: f64| lerp2(p1, p2, t);
    extrim_keep(ts, &pa, side)
        .into_iter()
        .filter_map(|(ta, tb)| {
            let a = lerp2(p1, p2, ta);
            let b = lerp2(p1, p2, tb);
            if (b[0] - a[0]).hypot(b[1] - a[1]) < 1e-6 {
                return None;
            }
            let mut l = orig.clone();
            l.common.handle = Handle::NULL;
            l.start = Vector3::new(a[0], a[1], z);
            l.end = Vector3::new(b[0], b[1], z);
            Some(EntityType::Line(l))
        })
        .collect()
}

pub(crate) fn extrim_arc(
    orig: &ArcEnt,
    ts: &[f64],
    side: &dyn Fn([f64; 2]) -> bool,
) -> Vec<EntityType> {
    let a0 = orig.start_angle;
    let a1 = orig.end_angle;
    let span = {
        let s = norm(a1) - norm(a0);
        if s <= 0.0 {
            s + TAU
        } else {
            s
        }
    };
    let angle_at = |t: f64| norm(a0) + span * t;
    let (cx, cy, r) = (orig.center.x, orig.center.y, orig.radius);
    let pt = |t: f64| {
        let a = angle_at(t);
        [cx + r * a.cos(), cy + r * a.sin()]
    };
    extrim_keep(ts, &pt, side)
        .into_iter()
        .filter_map(|(ta, tb)| {
            if (tb - ta).abs() < 1e-6 {
                return None;
            }
            let mut a = orig.clone();
            a.common.handle = Handle::NULL;
            a.start_angle = angle_at(ta);
            a.end_angle = angle_at(tb);
            Some(EntityType::Arc(a))
        })
        .collect()
}

pub(crate) fn extrim_circle(
    orig: &CircleEnt,
    ts: &[f64],
    side: &dyn Fn([f64; 2]) -> bool,
) -> Vec<EntityType> {
    if ts.len() < 2 {
        return vec![];
    }
    let (cx, cy, r) = (orig.center.x, orig.center.y, orig.radius);
    let mut s = ts.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let n = s.len();
    let mut out = Vec::new();
    for i in 0..n {
        let ta = s[i];
        let tb = if i + 1 < n { s[i + 1] } else { s[0] + 1.0 };
        let mt = ((ta + tb) * 0.5).rem_euclid(1.0) * TAU;
        let mid = [cx + r * mt.cos(), cy + r * mt.sin()];
        if side(mid) {
            continue; // removed side
        }
        let mut arc = ArcEnt::new();
        arc.common = orig.common.clone();
        arc.common.handle = Handle::NULL;
        arc.center = orig.center;
        arc.radius = orig.radius;
        arc.thickness = orig.thickness;
        arc.normal = orig.normal;
        arc.start_angle = ta.rem_euclid(1.0) * TAU;
        arc.end_angle = tb.rem_euclid(1.0) * TAU;
        out.push(EntityType::Arc(arc));
    }
    out
}

/// Sample any entity to a dense XY polyline for the sampled trim path.
/// Plan-view sampling of any entity, at the density the editing previews
/// have always used.
///
/// One tessellation rather than a case per type. The per-type version this
/// replaced had drifted from the drawing's own: it sampled an arc at 16
/// segments per radian against the renderer's 20, sampled a spline at a
/// fixed 96 points however long it was, and read an arc's centre as world
/// coordinates when the entity stores it in its OCS.
///
/// Unbounded curves come back empty — a ray has no finite point list, and
/// how far to run it is the caller's decision. [`fence_sample_xy`] makes it.

pub(crate) fn sample_entity_xy(e: &EntityType) -> Vec<[f64; 2]> {
    // A polyline is sampled through its exploded segments so that a caller
    // walking the result sees the same seams the rest of TRIM does.
    if matches!(
        e,
        EntityType::LwPolyline(_)
            | EntityType::Polyline(_)
            | EntityType::Polyline2D(_)
            | EntityType::Polyline3D(_)
    ) {
        let mut pts: Vec<[f64; 2]> = Vec::new();
        for seg in crate::modules::draw::modify::explode::explode_polyline_segments(e) {
            let sp = sample_entity_xy(&seg);
            // A segment with no plan-view shape samples to nothing — an
            // edge-on plane has no XY curve, so `entity_curve_xy` declines it.
            // Skipping it here also keeps the seam test below from comparing
            // two `None`s and then slicing an empty sample (#1318).
            if sp.is_empty() {
                continue;
            }
            if pts.last() == sp.first() {
                pts.extend_from_slice(&sp[1..]);
            } else {
                pts.extend(sp);
            }
        }
        if pts.is_empty() {
            return projected_polyline_points_xy(e);
        }
        return pts;
    }
    let Some(curve) = entity_curve_xy(e) else {
        return vec![];
    };
    if curve.extent() != KernelExtent::Bounded {
        return vec![];
    }
    curve.tessellate(SAMPLE_SEGMENTS_PER_RADIAN)
}

/// Dense XY sampling of any entity for the removal preview (lines are
/// subdivided and circles closed so the preview cut follows the boundary).

pub(crate) fn preview_sample_xy(e: &EntityType) -> Vec<[f64; 2]> {
    match e {
        EntityType::Line(l) => {
            let p1 = [l.start.x, l.start.y];
            let p2 = [l.end.x, l.end.y];
            (0..=24).map(|i| lerp2(p1, p2, i as f64 / 24.0)).collect()
        }
        EntityType::Circle(c) => {
            let steps = 64usize;
            (0..=steps)
                .map(|i| {
                    let a = TAU * (i as f64 / steps as f64);
                    [
                        c.center.x + c.radius * a.cos(),
                        c.center.y + c.radius * a.sin(),
                    ]
                })
                .collect()
        }
        // Polylines sample only their vertices — subdivide the straight
        // segments so the preview cut follows the boundary instead of
        // jumping at the nearest vertex (#340).
        EntityType::LwPolyline(_)
        | EntityType::Polyline(_)
        | EntityType::Polyline2D(_)
        | EntityType::Polyline3D(_) => {
            let mut pts: Vec<[f64; 2]> = Vec::new();
            for seg in crate::modules::draw::modify::explode::explode_polyline_segments(e) {
                let sp: Vec<[f64; 2]> = match &seg {
                    EntityType::Line(l) => {
                        let p1 = [l.start.x, l.start.y];
                        let p2 = [l.end.x, l.end.y];
                        (0..=24).map(|i| lerp2(p1, p2, i as f64 / 24.0)).collect()
                    }
                    _ => sample_entity_xy(&seg),
                };
                if sp.is_empty() {
                    continue;
                }
                if pts.last() == sp.first() {
                    pts.extend_from_slice(&sp[1..]);
                } else {
                    pts.extend(sp);
                }
            }
            if pts.is_empty() {
                return projected_polyline_points_xy(e);
            }
            pts
        }
        _ => sample_entity_xy(e),
    }
}

/// Project a planar polyline into the XY working plane and sample its curve.
/// This covers imported OCS polylines whose extrusion normal prevents the
/// older entity-explosion path from producing world-XY line/arc pieces.

pub(crate) fn projected_polyline_points_xy(e: &EntityType) -> Vec<[f64; 2]> {
    entity_curve_xy(e)
        .filter(|curve| curve.extent() == KernelExtent::Bounded)
        .map(|curve| curve.tessellate(SAMPLE_SEGMENTS_PER_RADIAN))
        .unwrap_or_default()
}

/// Append the sub-runs of `pts` for which `take` holds to `out` as one wire's
/// point list, separated from earlier content by a NaN pen-up.

pub(crate) fn collect_runs(
    pts: &[[f64; 2]],
    take: &dyn Fn([f64; 2]) -> bool,
    out: &mut Vec<[f32; 3]>,
) {
    let mut run: Vec<[f64; 2]> = Vec::new();
    let flush = |run: &mut Vec<[f64; 2]>, out: &mut Vec<[f32; 3]>| {
        if run.len() >= 2 {
            if !out.is_empty() {
                out.push([f32::NAN, f32::NAN, f32::NAN]);
            }
            out.extend(run.iter().map(|p| [p[0] as f32, p[1] as f32, 0.0]));
        }
        run.clear();
    };
    for &p in pts {
        if take(p) {
            run.push(p);
        } else {
            flush(&mut run, out);
        }
    }
    flush(&mut run, out);
}

/// Build a preview wire from a NaN-break point list.

pub(crate) fn preview_wire(points: Vec<[f32; 3]>, color: [f32; 4], name: &str) -> WireModel {
    WireModel {
        bg_adapt: None,
        point_marker: None,
        world_width: 0.0,
        depth_override: None,
        display_visible: true,
        snap_only: false,
        plot_visible: true,
        fill_is_3d: false,
        fill_is_2d_solid: false,
        solid_normal: None,
        render_instance: None,
        dash_from_start: false,
        name: name.into(),
        color,
        selected: false,
        pattern_length: 0.0,
        pattern: [0.0; 8],
        line_weight_px: 1.0,
        aci: 0,
        aabb: WireModel::UNBOUNDED_AABB,
        plinegen: true,
        geom: std::sync::Arc::new(crate::scene::WireGeom {
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            text_verts: Vec::new(),
            points,
            points_low: Vec::new(),
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
            ..crate::scene::WireGeom::default()
        }),
        ..Default::default()
    }
}

/// Insert the exact boundary crossings between consecutive samples so a
/// sampled cut lands ON the boundary instead of at the nearest sample. A
/// polyline samples only its vertices, so without this a straight segment
/// crossing the boundary was cut at the wrong place — or not at all when
/// both endpoints were on the kept side (#340).

pub(crate) fn insert_boundary_crossings(pts: &[[f64; 2]], geos: &[Geo]) -> Vec<[f64; 2]> {
    let Some(last) = pts.last() else {
        return Vec::new();
    };
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(pts.len());
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        out.push(a);
        let mut ts = line_seg_ts(a[0], a[1], b[0], b[1], Handle::NULL, geos);
        ts.sort_by(|x, y| x.total_cmp(y));
        for t in ts {
            if t > 1e-6 && t < 1.0 - 1e-6 {
                out.push(lerp2(a, b, t));
            }
        }
    }
    out.push(*last);
    out
}

/// Sampled trim: `None` leaves the entity unchanged, `Some(vec![])` erases it,
/// `Some(runs)` replaces it with the surviving pieces as LwPolylines.
/// Classification is per SEGMENT midpoint (samples + exact crossings), so a
/// vertex sitting exactly on the boundary can't flip the parity test.

pub(crate) fn extrim_sampled(
    pts: &[[f64; 2]],
    geos: &[Geo],
    side: &dyn Fn([f64; 2]) -> bool,
) -> Option<Vec<EntityType>> {
    if pts.len() < 2 {
        return None;
    }
    let dense = insert_boundary_crossings(pts, geos);
    let seg_kept: Vec<bool> = dense
        .windows(2)
        .map(|w| !side([(w[0][0] + w[1][0]) * 0.5, (w[0][1] + w[1][1]) * 0.5]))
        .collect();
    if seg_kept.iter().all(|&k| k) {
        return None; // wholly on the kept side — untouched
    }
    if seg_kept.iter().all(|&k| !k) {
        return Some(vec![]); // wholly on the pick side — erased
    }
    let mut runs: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    for (i, &kept) in seg_kept.iter().enumerate() {
        if kept {
            if cur.is_empty() {
                cur.push(dense[i]);
            }
            cur.push(dense[i + 1]);
        } else if cur.len() >= 2 {
            runs.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        runs.push(cur);
    }
    Some(
        runs.into_iter()
            .map(|run| {
                let mut pl = LwPolyline::new();
                pl.common.handle = Handle::NULL;
                pl.is_closed = false;
                pl.vertices = run
                    .into_iter()
                    .map(|p| LwVertex::from_coords(p[0], p[1]))
                    .collect();
                EntityType::LwPolyline(pl)
            })
            .collect(),
    )
}

pub struct ExtrimCommand {
    pub(crate) all: Vec<(Handle, EntityType)>,
    pub(crate) boundary: Option<Handle>,
    pub(crate) geos: Vec<Geo>,
}

impl ExtrimCommand {
    pub fn new(all: Vec<(Handle, EntityType)>) -> Self {
        let all = all
            .into_iter()
            .map(|(handle, entity)| (handle, entity_with_lwpolyline_world_xy(&entity)))
            .collect();
        Self {
            all,
            boundary: None,
            geos: Vec::new(),
        }
    }
}

impl CadCommand for ExtrimCommand {
    fn name(&self) -> &'static str {
        "EXTRIM"
    }

    fn prompt(&self) -> String {
        if self.boundary.is_none() {
            crate::t!("EXTRIM  Select cutting boundary:").into_owned()
        } else {
            crate::t!("EXTRIM  Click the side to trim away:").into_owned()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.boundary.is_none()
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let Some((_, e)) = self.all.iter().find(|(h, _)| *h == handle) else {
            return CmdResult::NeedPoint;
        };
        let mut geos = build_geos(std::slice::from_ref(e));
        // Only a picked LINE boundary is treated as infinite. A polyline
        // explodes into per-segment Line geos — extending those turns a
        // closed boundary into a grid of infinite lines and the parity
        // side-test breaks (#340).
        if matches!(e, EntityType::Line(_)) {
            extend_line_geos(&mut geos);
        }
        if geos.is_empty() {
            return CmdResult::NeedPoint; // not a usable boundary; keep asking
        }
        self.boundary = Some(handle);
        self.geos = geos;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(bh) = self.boundary else {
            return CmdResult::NeedPoint;
        };
        let q = [pt.x, pt.y];
        let geos = self.geos.clone();
        let side = |m: [f64; 2]| {
            line_seg_ts(m[0], m[1], q[0], q[1], Handle::NULL, &geos)
                .len()
                .is_multiple_of(2)
        };
        let mut repl: Vec<(Handle, Vec<EntityType>)> = Vec::new();
        for (h, e) in &self.all {
            if *h == bh {
                continue;
            }
            match e {
                EntityType::Line(l) => {
                    let ts = line_seg_ts(l.start.x, l.start.y, l.end.x, l.end.y, *h, &geos);
                    if ts.is_empty() {
                        let mid = [(l.start.x + l.end.x) * 0.5, (l.start.y + l.end.y) * 0.5];
                        if side(mid) {
                            repl.push((*h, vec![]));
                        }
                    } else {
                        repl.push((*h, extrim_line(l, &ts, &side)));
                    }
                }
                EntityType::Arc(a) => {
                    let ts = arc_seg_ts(
                        a.center.x,
                        a.center.y,
                        a.radius,
                        a.start_angle,
                        a.end_angle,
                        *h,
                        &geos,
                    );
                    if ts.is_empty() {
                        let am = norm(a.start_angle);
                        let mid = [
                            a.center.x + a.radius * am.cos(),
                            a.center.y + a.radius * am.sin(),
                        ];
                        if side(mid) {
                            repl.push((*h, vec![]));
                        }
                    } else {
                        repl.push((*h, extrim_arc(a, &ts, &side)));
                    }
                }
                EntityType::Circle(c) => {
                    let ts = arc_seg_ts(c.center.x, c.center.y, c.radius, 0.0, TAU, *h, &geos);
                    if ts.len() < 2 {
                        if side([c.center.x, c.center.y]) {
                            repl.push((*h, vec![]));
                        }
                    } else {
                        repl.push((*h, extrim_circle(c, &ts, &side)));
                    }
                }
                EntityType::LwPolyline(_)
                | EntityType::Polyline(_)
                | EntityType::Polyline2D(_)
                | EntityType::Polyline3D(_)
                | EntityType::Ellipse(_)
                | EntityType::Spline(_) => {
                    let pts = sample_entity_xy(e);
                    if let Some(res) = extrim_sampled(&pts, &geos, &side) {
                        repl.push((*h, res));
                    }
                }
                _ => {}
            }
        }
        if repl.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::ReplaceMany(repl, Vec::new())
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let Some(bh) = self.boundary else {
            return Vec::new();
        };
        if self.geos.is_empty() {
            return Vec::new();
        }
        let q = [pt.x, pt.y];
        let geos = &self.geos;
        let side = |m: [f64; 2]| {
            line_seg_ts(m[0], m[1], q[0], q[1], Handle::NULL, geos)
                .len()
                .is_multiple_of(2)
        };
        // Removed (pick side) → red, surviving → blue; the boundary → yellow.
        let mut removed: Vec<[f32; 3]> = Vec::new();
        let mut kept: Vec<[f32; 3]> = Vec::new();
        for (h, e) in &self.all {
            if *h == bh {
                continue;
            }
            let pts = preview_sample_xy(e);
            if pts.len() < 2 {
                continue;
            }
            collect_runs(&pts, &side, &mut removed);
            collect_runs(&pts, &|p| !side(p), &mut kept);
        }
        let mut boundary_pts: Vec<[f32; 3]> = Vec::new();
        if let Some((_, be)) = self.all.iter().find(|(h, _)| *h == bh) {
            boundary_pts = preview_sample_xy(be)
                .into_iter()
                .map(|p| [p[0] as f32, p[1] as f32, 0.0])
                .collect();
        }

        const YELLOW: [f32; 4] = [1.0, 0.90, 0.15, 1.0];
        const REMOVE_RED: [f32; 4] = [0.95, 0.30, 0.30, 1.0];
        let mut out = Vec::new();
        if boundary_pts.len() >= 2 {
            out.push(preview_wire(boundary_pts, YELLOW, "extrim_boundary"));
        }
        if kept.len() >= 2 {
            out.push(preview_wire(kept, WireModel::SELECTED, "extrim_keep"));
        }
        if removed.len() >= 2 {
            out.push(preview_wire(removed, REMOVE_RED, "extrim_remove"));
        }
        out
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["EXTRIM"] }); // ExtrimCommand
