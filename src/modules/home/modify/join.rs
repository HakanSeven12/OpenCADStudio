// JOIN command — merge collinear Lines or co-circular Arcs into one entity.
//
// Supports:
//   Line + Line  → merged Line (if collinear and end-to-end)
//   Arc  + Arc   → merged Arc  (if same center/radius and contiguous)
//   Arc  + Arc   → Circle      (if the merged arc spans 360°)
//
// Workflow: select objects then press Enter to join.

use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::Vec3;

use crate::command::{CadCommand, CmdResult};

// ── Command ────────────────────────────────────────────────────────────────

pub struct JoinCommand {
    handles: Vec<Handle>,
    gathering: bool,
}

impl JoinCommand {
    pub fn new() -> Self {
        Self { handles: vec![], gathering: true }
    }
}

impl CadCommand for JoinCommand {
    fn name(&self) -> &'static str { "JOIN" }

    fn prompt(&self) -> String {
        format!("JOIN  Select objects to join ({} selected, Enter to apply):", self.handles.len())
    }

    fn is_selection_gathering(&self) -> bool { self.gathering }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.handles.len() < 2 {
            return CmdResult::Cancel;
        }
        self.gathering = false;
        CmdResult::JoinEntities(self.handles.clone())
    }
}

// ── Geometry ───────────────────────────────────────────────────────────────

/// Try to join all `entities` in the slice into a minimal set.
/// Returns `(kept, removed)` handle lists and the merged entity vec.
pub fn join_entities(entities: &[(Handle, &EntityType)]) -> Option<(Vec<Handle>, Vec<EntityType>)> {
    if entities.len() < 2 {
        return None;
    }

    // Split into lines and arcs
    let lines: Vec<_> = entities.iter()
        .filter(|(_, e)| matches!(e, EntityType::Line(_)))
        .collect();
    let arcs: Vec<_> = entities.iter()
        .filter(|(_, e)| matches!(e, EntityType::Arc(_)))
        .collect();

    if !lines.is_empty() && arcs.is_empty() {
        return try_join_lines(&lines);
    }
    if !arcs.is_empty() && lines.is_empty() {
        return try_join_arcs(&arcs);
    }
    None
}

fn try_join_lines(lines: &[&(Handle, &EntityType)]) -> Option<(Vec<Handle>, Vec<EntityType>)> {
    // Collect endpoints in XZ plane
    let segs: Vec<_> = lines.iter().map(|(h, e)| {
        if let EntityType::Line(l) = e {
            let s = Vec3::new(l.start.x as f32, l.start.z as f32, 0.0);
            let e2 = Vec3::new(l.end.x as f32, l.end.z as f32, 0.0);
            (*h, l, s, e2)
        } else { unreachable!() }
    }).collect();

    // Check collinearity: all lines must be parallel and on the same infinite line.
    let (_, first_line, s0, e0) = segs[0];
    let dir0 = (e0 - s0).normalize_or_zero();
    if dir0.length_squared() < 1e-12 { return None; }

    for (_, _, si, ei) in &segs[1..] {
        // Check parallel
        let diri = (*ei - *si).normalize_or_zero();
        let cross = (dir0.x * diri.y - dir0.y * diri.x).abs();
        if cross > 1e-4 { return None; }
        // Check co-linear (point on same line)
        let off = *si - s0;
        let perp = (off - dir0 * off.dot(dir0)).length();
        if perp > 1e-3 { return None; }
    }

    // Project all endpoints onto the direction axis
    let params: Vec<f32> = segs.iter().flat_map(|(_, _, si, ei)| {
        [si.dot(dir0), ei.dot(dir0)]
    }).collect();
    let t_min = params.iter().cloned().fold(f32::INFINITY, f32::min);
    let t_max = params.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

    // Check connectivity: no gaps larger than tolerance
    let mut sorted: Vec<[f32; 2]> = segs.iter().map(|(_, _, si, ei)| {
        let ta = si.dot(dir0);
        let tb = ei.dot(dir0);
        [ta.min(tb), ta.max(tb)]
    }).collect();
    sorted.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());
    for w in sorted.windows(2) {
        if w[1][0] > w[0][1] + 1e-3 { return None; } // gap
    }

    let new_start = s0 + dir0 * t_min;
    let new_end   = s0 + dir0 * t_max;

    let mut merged = first_line.clone();
    merged.common.handle = Handle::NULL;
    merged.start = vec3_xz_to_v3(new_start, first_line.start.z);
    merged.end   = vec3_xz_to_v3(new_end,   first_line.start.z);

    let handles: Vec<Handle> = lines.iter().map(|(h, _)| *h).collect();
    Some((handles, vec![EntityType::Line(merged)]))
}

fn try_join_arcs(arcs: &[&(Handle, &EntityType)]) -> Option<(Vec<Handle>, Vec<EntityType>)> {
    let segs: Vec<_> = arcs.iter().map(|(h, e)| {
        if let EntityType::Arc(a) = e { (*h, a) }
        else { unreachable!() }
    }).collect();

    // All arcs must share the same center and radius
    let (_, first) = segs[0];
    let cx = first.center.x;
    let cy = first.center.y;
    let r  = first.radius;

    for (_, a) in &segs[1..] {
        if (a.center.x - cx).abs() > 1e-3 || (a.center.y - cy).abs() > 1e-3 { return None; }
        if (a.radius - r).abs() > 1e-3 { return None; }
    }

    // Collect (start_angle, end_angle) in radians.
    let mut intervals: Vec<[f32; 2]> = segs.iter().map(|(_, a)| {
        [a.start_angle as f32, a.end_angle as f32]
    }).collect();

    // Sort by start angle
    intervals.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap());

    // Try to merge into one contiguous arc
    let merged_start = intervals[0][0];
    let mut merged_end   = intervals[0][1];
    for &[s, e] in &intervals[1..] {
        let span = (e - merged_end).rem_euclid(std::f32::consts::TAU);
        let gap  = (s - merged_end).rem_euclid(std::f32::consts::TAU);
        let _ = span;
        if gap > 1e-3 { return None; } // discontinuous
        merged_end = e;
    }

    let span = merged_end - merged_start;
    let handles: Vec<Handle> = arcs.iter().map(|(h, _)| *h).collect();

    if (span - std::f32::consts::TAU).abs() < 0.01 {
        // Full circle
        let mut circle = acadrust::entities::Circle::new();
        circle.common = first.common.clone();
        circle.common.handle = Handle::NULL;
        circle.center = first.center.clone();
        circle.radius = r;
        circle.normal = first.normal.clone();
        return Some((handles, vec![EntityType::Circle(circle)]));
    }

    let mut merged_arc = first.clone();
    merged_arc.common.handle = Handle::NULL;
    merged_arc.start_angle = merged_start as f64;
    merged_arc.end_angle   = merged_end   as f64;
    Some((handles, vec![EntityType::Arc(merged_arc)]))
}

fn vec3_xz_to_v3(v: Vec3, z: f64) -> Vector3 {
    Vector3::new(v.x as f64, v.y as f64, z)
}
