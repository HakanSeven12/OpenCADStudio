// Hatch, gradient, and boundary commands.

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::IconKind;
use crate::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};
use crate::scene::model::wire_model::WireModel;
use crate::t;
use codec::Handle;
use glam::DVec3;
use kernel::geom2d::{
    bounded_faces, contains, ring_nesting_depths, signed_area, Circle, Curve, Line, Tolerance,
};

// ── Icons ──────────────────────────────────────────────────────────────────

const ICON_HATCH: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_lines.svg"
));
const ICON_GRADIENT: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_gradient.svg"
));
const ICON_BOUNDARY: IconKind = IconKind::Svg(include_bytes!(
    "../../../../assets/icons/hatch/hatch_boundary.svg"
));

// ── Dropdown metadata ──────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "HATCH";
pub const ICON: IconKind = ICON_HATCH;

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    ("HATCH", "Hatch", ICON_HATCH),
    ("GRADIENT", "Gradient", ICON_GRADIENT),
    ("BOUNDARY", "Boundary", ICON_BOUNDARY),
];

// ── Shared mode ────────────────────────────────────────────────────────────

enum Mode {
    /// Primary: click inside a closed shape → boundary auto-detected.
    PickInside,
    /// Fallback: user manually picks polygon vertices (type "S" to enter).
    Manual,
}

/// The largest gap a boundary pick may bridge (HPGAPTOL range).
pub const MAX_GAP_TOLERANCE: f64 = 5000.0;

/// A typed gap tolerance, if it is a number in `0..=MAX_GAP_TOLERANCE`.
fn parse_gap_tolerance(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|value| (0.0..=MAX_GAP_TOLERANCE).contains(value))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HatchMode {
    PickInside,
    SelectObjects,
    Manual,
}

// ── CPU point-in-polygon (ray casting) ────────────────────────────────────

fn point_in_polygon(p: [f64; 2], poly: &[[f64; 2]]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let vi = poly[i];
        let vj = poly[j];
        if (vi[1] > p[1]) != (vj[1] > p[1]) {
            let x_int = (vj[0] - vi[0]) * (p[1] - vi[1]) / (vj[1] - vi[1]) + vi[0];
            if p[0] < x_int {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Shoelace-area magnitude of a polygon. Used to pick the smallest enclosing
/// outline when a click falls inside several nested boundaries.
fn polygon_area(poly: &[[f64; 2]]) -> f64 {
    let n = poly.len();
    if n < 3 {
        return 0.0;
    }
    let origin = poly[0];
    let mut a = 0.0;
    for i in 1..n - 1 {
        let current = poly[i];
        let next = poly[i + 1];
        a += (current[0] - origin[0]) * (next[1] - origin[1])
            - (next[0] - origin[0]) * (current[1] - origin[1]);
    }
    (a * 0.5).abs()
}

/// True when every vertex of `inner` lies inside `outer`. Sufficient to
/// recognise a closed hatch outline as nested inside another for the common
/// rectangle / closed-polyline case.
fn polygon_contains_polygon(outer: &[[f64; 2]], inner: &[[f64; 2]]) -> bool {
    if inner.len() < 3 {
        return false;
    }
    inner.iter().all(|&v| point_in_polygon(v, outer))
}

/// Resolve the innermost clicked ring and its direct holes.
pub(crate) fn resolve_hatch_rings(outlines: &[Vec<[f64; 2]>], p: [f64; 2]) -> Option<Vec<Vec<[f64; 2]>>> {
    let mut containing: Vec<(usize, f64)> = outlines
        .iter()
        .enumerate()
        .filter(|(_, o)| point_in_polygon(p, o))
        .map(|(i, o)| (i, polygon_area(o)))
        .collect();
    if containing.is_empty() {
        return None;
    }
    // Innermost (smallest-area) outline containing the point is the fill.
    containing.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let outer_idx = containing[0].0;
    let outer = &outlines[outer_idx];

    let mut rings = vec![outer.clone()];
    for (i, o) in outlines.iter().enumerate() {
        if i == outer_idx {
            continue;
        }
        // Candidate hole: fully nested inside the fill outline. (An outline the
        // click sits in cannot qualify — it would have been the smaller fill.)
        if !polygon_contains_polygon(outer, o) || point_in_polygon(p, o) {
            continue;
        }
        // Only DIRECT children become holes. If another outline sits strictly
        // between `outer` and `o` (inside `outer`, and enclosing `o`), then `o`
        // belongs to that intermediate region's own fill; flagging it here would
        // re-fill it under even-odd once nesting reaches three levels.
        let has_intermediate = outlines.iter().enumerate().any(|(k, x)| {
            k != i
                && k != outer_idx
                && polygon_contains_polygon(outer, x)
                && polygon_contains_polygon(x, o)
        });
        if !has_intermediate {
            rings.push(o.clone());
        }
    }
    Some(rings)
}

/// Pack one or more rings (outer boundary + optional holes) into the Hatch
/// model storage: the `boundary` f32 ring list (NaN-separated) plus the exact
/// `boundary_wcs` (NaN-separated) used for persistence. The first vertex of the
/// first ring anchors the shared origin.
fn pack_rings(rings: &[Vec<[f64; 2]>]) -> (Vec<[f32; 2]>, [f64; 2], Vec<[f64; 2]>) {
    let mut wcs: Vec<[f64; 2]> = Vec::new();
    let mut first = true;
    for ring in rings {
        if !first {
            wcs.push([f64::NAN, f64::NAN]);
        }
        first = false;
        wcs.extend(ring.iter().copied());
    }
    let (rel, origin) = rte_boundary(wcs.iter().map(|&[x, y]| (x, y)));
    (rel, origin, wcs)
}

/// Store boundary points as precise-origin-relative offsets.
fn rte_boundary(pts: impl Iterator<Item = (f64, f64)>) -> (Vec<[f32; 2]>, [f64; 2]) {
    let pts: Vec<(f64, f64)> = pts.collect();
    let Some(&(ox, oy)) = pts.first() else {
        return (vec![], [0.0; 2]);
    };
    let rel = pts
        .iter()
        .map(|&(x, y)| [(x - ox) as f32, (y - oy) as f32])
        .collect();
    (rel, [ox, oy])
}

// ── HATCH command ──────────────────────────────────────────────────────────

/// The prompt HATCH is at besides its boundary method's own prompt.
#[derive(Clone, Copy, PartialEq)]
enum HatchStep {
    /// The method's prompt: pick, select, or the draw start / next point.
    Main,
    /// seTtings: the hatch toggles.
    Settings,
    /// Gap tolerance, asked from the settings.
    Gap,
    /// Draw: Mode's Area/Path question.
    DrawMode,
    /// Draw: Length's distance along the last segment.
    Length,
    RectFirst,
    RectOther(DVec3),
    CircleCenter,
    CircleRadius(DVec3),
    CircleDiameter(DVec3),
}

/// The method HATCH starts with, as the profile stores it.
fn method_code(mode: HatchMode) -> u8 {
    match mode {
        HatchMode::PickInside => 0,
        HatchMode::SelectObjects => 1,
        HatchMode::Manual => 2,
    }
}

fn yes_no(on: bool) -> String {
    if on {
        crate::tr!("hatch-cmd", "yes")
    } else {
        crate::tr!("hatch-cmd", "no")
    }
}

fn parse_number(text: &str) -> Option<f64> {
    text.trim()
        .replace(',', ".")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

pub struct HatchCommand {
    boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    point_regions: Vec<Vec<Vec<[f64; 2]>>>,
    object_regions: Vec<Vec<Vec<[f64; 2]>>>,
    selected_objects: Vec<Handle>,
    mode: HatchMode,
    step: HatchStep,
    /// The method typed at a prompt, kept as the next HATCH's start.
    method_chosen: Option<HatchMode>,
    /// A point was picked: later picks only analyse islands.
    analyzed: bool,
    manual_pts: Vec<DVec3>,
    manual_bulges: Vec<f64>,
    manual_arc_mode: bool,
    manual_arc_midpoint: Option<DVec3>,
    retain_boundaries: bool,
    pattern_override: Option<(String, HatchPattern)>,
    angle_override: Option<f32>,
    scale_override: Option<f32>,
    default_origin: [f64; 2],
    associative: bool,
    separate_hatches: bool,
    island_style: codec::entities::HatchStyleType,
    inherited: Option<(HatchModel, codec::types::Color, codec::types::Transparency)>,
    plane: WorkingPlane,
    /// HPGAPTOL: gaps up to this size count as closed for a picked point.
    gap_tolerance: f64,
}

impl HatchCommand {
    pub fn new(
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        selected_objects: Vec<Handle>,
        inherited: Option<(HatchModel, codec::types::Color, codec::types::Transparency)>,
        plane: WorkingPlane,
    ) -> Self {
        let selected_objects: Vec<_> = selected_objects
            .into_iter()
            .filter(|handle| boundary_sources.contains_key(handle))
            .collect();
        let has_selection = !selected_objects.is_empty();
        let mut command = Self {
            boundary_sources,
            point_regions: Vec::new(),
            object_regions: Vec::new(),
            selected_objects: Vec::new(),
            mode: if has_selection {
                HatchMode::SelectObjects
            } else {
                HatchMode::PickInside
            },
            step: HatchStep::Main,
            method_chosen: None,
            analyzed: false,
            manual_pts: vec![],
            manual_bulges: vec![],
            manual_arc_mode: false,
            manual_arc_midpoint: None,
            retain_boundaries: false,
            pattern_override: None,
            angle_override: None,
            scale_override: None,
            default_origin: [0.0, 0.0],
            associative: true,
            separate_hatches: false,
            island_style: inherited
                .as_ref()
                .map(|(model, _, _)| model.style)
                .unwrap_or(codec::entities::HatchStyleType::Normal),
            inherited,
            plane,
            gap_tolerance: 0.0,
        };
        command.set_object_selection(selected_objects);
        command
    }

    /// The boundary method HATCH starts with (the one used last: 0 pick
    /// internal point, 1 select objects, 2 draw). Preselected objects
    /// still start in select mode.
    pub fn with_start_method(mut self, method: u8) -> Self {
        if self.selected_objects.is_empty() {
            self.mode = match method {
                1 => HatchMode::SelectObjects,
                2 => HatchMode::Manual,
                _ => HatchMode::PickInside,
            };
        }
        self
    }

    /// The line printed before the first prompt (draw mode's current mode).
    pub fn start_message(&self) -> Option<String> {
        (self.mode == HatchMode::Manual).then(|| crate::tr!("hatch-cmd", "current-mode"))
    }

    /// HPASSOC: whether the hatch starts out associative.
    pub fn with_associative(mut self, associative: bool) -> Self {
        self.associative = associative;
        self
    }

    /// HPGAPTOL: the largest gap a picked boundary may have.
    pub fn with_gap_tolerance(mut self, gap_tolerance: f64) -> Self {
        self.gap_tolerance = gap_tolerance;
        self
    }

    pub fn with_origin(mut self, origin: [f64; 2]) -> Self {
        self.default_origin = origin;
        self
    }

    pub fn with_pattern(mut self, name: impl Into<String>, pattern: HatchPattern) -> Self {
        self.pattern_override = Some((name.into(), pattern));
        self
    }

    pub fn finish_selected(mut self) -> CmdResult {
        self.on_enter()
    }

    /// Whether `handles` gave no closed boundary.
    fn set_object_selection(&mut self, handles: Vec<Handle>) -> bool {
        let mut segments = Vec::new();
        for handle in &handles {
            if let Some(source) = self.boundary_sources.get(handle) {
                segments.extend(source.segments.iter().copied());
            }
        }
        self.object_regions = bounded_faces(&segments, Tolerance::new(1.0e-6))
            .into_iter()
            .map(|ring| vec![ring])
            .collect();
        let missed = !handles.is_empty() && self.object_regions.is_empty();
        self.selected_objects = handles;
        missed
    }

    fn add_point_region(&mut self, rings: Vec<Vec<[f64; 2]>>) {
        let duplicate = rings.first().is_some_and(|outer| {
            self.point_regions
                .iter()
                .any(|region| region.first() == Some(outer))
        });
        if !duplicate {
            self.point_regions.push(rings);
        }
    }

    fn region_count(&self) -> usize {
        self.point_regions.len() + self.object_regions.len()
    }

    fn island_style_label(&self) -> &'static str {
        match self.island_style {
            codec::entities::HatchStyleType::Normal => "Normal",
            codec::entities::HatchStyleType::Outer => "Outer",
            codec::entities::HatchStyleType::Ignore => "Ignore",
        }
    }

    /// Switch to a boundary method typed at a prompt.
    fn choose(&mut self, mode: HatchMode) -> CmdResult {
        self.mode = mode;
        self.method_chosen = Some(mode);
        self.step = HatchStep::Main;
        self.reset_drawing();
        if mode == HatchMode::Manual {
            Self::current_mode()
        } else {
            CmdResult::NeedPoint
        }
    }

    fn current_mode() -> CmdResult {
        CmdResult::ReportMeasurement(crate::tr!("hatch-cmd", "current-mode"))
    }

    fn settings(&mut self) -> CmdResult {
        self.step = HatchStep::Settings;
        let island = match self.island_style {
            codec::entities::HatchStyleType::Normal => crate::tr!("hatch-cmd", "island-normal"),
            codec::entities::HatchStyleType::Outer => crate::tr!("hatch-cmd", "island-outer"),
            codec::entities::HatchStyleType::Ignore => crate::tr!("hatch-cmd", "island-ignore"),
        };
        CmdResult::ReportMeasurement(crate::tr!(
            "hatch-cmd",
            "settings-state",
            keep = yes_no(self.retain_boundaries),
            associative = yes_no(self.associative),
            separate = yes_no(self.separate_hatches),
            island = island,
            gap = self.gap_tolerance.to_string()
        ))
    }

    fn reset_drawing(&mut self) {
        self.manual_pts.clear();
        self.manual_bulges.clear();
        self.manual_arc_mode = false;
        self.manual_arc_midpoint = None;
    }

    /// The drawn boundary with its arcs tessellated.
    fn drawn_ring(&self) -> Vec<[f64; 2]> {
        let count = self.manual_pts.len();
        let mut ring = Vec::new();
        for (index, point) in self.manual_pts.iter().enumerate() {
            let start = [point.x, point.y];
            let end = self.manual_pts[(index + 1) % count];
            let bulge = self.manual_bulges.get(index).copied().unwrap_or(0.0);
            match kernel::geom2d::BulgeArc::from_bulge(start, [end.x, end.y], bulge)
                .filter(|_| bulge.abs() > 1.0e-12)
            {
                Some(arc) => {
                    let mut points = arc.tessellate_angle(std::f64::consts::PI / 32.0);
                    points.pop();
                    ring.extend(points);
                }
                None => ring.push(start),
            }
        }
        ring
    }

    /// Hatch the drawn boundary and go back to the start prompt.
    fn finish_drawn(&mut self) -> CmdResult {
        let mut hatch = self.make_hatch(vec![self.drawn_ring()]);
        if let Some(path) = self.manual_boundary_path() {
            hatch.boundary_paths = Some(std::sync::Arc::new(vec![path]));
        }
        self.reset_drawing();
        self.step = HatchStep::Main;
        CmdResult::CommitHatchContinue {
            hatch,
            entity_style: self
                .inherited
                .as_ref()
                .map(|(_, color, transparency)| (color.clone(), *transparency)),
            message: crate::tr!("hatch-cmd", "current-mode"),
        }
    }

    fn finish_circle(&mut self, centre: DVec3, radius: f64) -> CmdResult {
        if radius <= 0.0 {
            return CmdResult::ReportError(crate::tr!("hatch-cmd", "positive"));
        }
        crate::modules::draw::defaults::set_circle_radius(radius);
        let offset = DVec3::new(radius, 0.0, 0.0);
        self.manual_pts = vec![centre + offset, centre - offset];
        self.manual_bulges = vec![1.0, 1.0];
        self.finish_drawn()
    }

    /// The next point `distance` along the last segment (+X after the start).
    fn add_length(&mut self, distance: f64) -> CmdResult {
        self.step = HatchStep::Main;
        let Some(&last) = self.manual_pts.last() else {
            return CmdResult::NeedPoint;
        };
        // ponytail: an arc's chord direction, not its end tangent.
        let direction = match self.manual_pts.len() {
            n if n >= 2 => (last - self.manual_pts[n - 2]).normalize_or(DVec3::X),
            _ => DVec3::X,
        };
        self.manual_bulges.push(0.0);
        self.manual_pts.push(last + direction * distance);
        CmdResult::NeedPoint
    }

    fn undo_drawn_point(&mut self) -> CmdResult {
        if self.manual_arc_midpoint.take().is_some() {
            return CmdResult::NeedPoint;
        }
        if self.manual_pts.len() > 1 {
            self.manual_pts.pop();
            self.manual_bulges.pop();
            return CmdResult::NeedPoint;
        }
        CmdResult::ReportMeasurement(crate::tr!("hatch-cmd", "segments-undone"))
    }

    fn combined_rings(&self) -> Vec<Vec<[f64; 2]>> {
        let mut rings = Vec::new();
        for ring in self
            .point_regions
            .iter()
            .chain(self.object_regions.iter())
            .flat_map(|region| region.iter())
        {
            if !rings.iter().any(|existing| existing == ring) {
                rings.push(ring.clone());
            }
        }
        rings
    }

    fn make_hatch(&self, rings: Vec<Vec<[f64; 2]>>) -> HatchModel {
        let perf_build = crate::perf::enabled();
        let build_started = perf_build.then(iced::time::Instant::now);
        let ring_count = rings.len();
        let input_vertices = if perf_build {
            rings.iter().map(Vec::len).sum::<usize>()
        } else {
            0
        };
        let world_rings: Vec<Vec<[f64; 2]>> = rings
            .iter()
            .map(|ring| {
                ring.iter()
                    .map(|&[x, y]| {
                        let point = self.plane.to_world(DVec3::new(x, y, 0.0));
                        [point.x, point.y]
                    })
                    .collect()
            })
            .collect();
        let (rel, origin, wcs) = pack_rings(&world_rings);
        let mut local_boundary = Vec::new();
        for (index, ring) in rings.iter().enumerate() {
            if index != 0 {
                local_boundary.push([f32::NAN, f32::NAN]);
            }
            local_boundary.extend(ring.iter().map(|&[x, y]| [x as f32, y as f32]));
        }
        let fill_plane = crate::scene::model::hatch_model::FillPlane {
            origin: self.plane.origin.to_array(),
            x_axis: self.plane.x.to_array(),
            y_axis: self.plane.y.to_array(),
        };
        let exterior: Vec<bool> = kernel::geom2d::ring_nesting_depths(&rings)
            .into_iter()
            .map(|depth| depth == 0)
            .collect();
        let near = crate::scene::sources_near_rings(&rings, &self.boundary_sources, 1.0e-6);
        let near_source_count = near.len();
        let mut boundary_sources: Vec<Vec<Handle>> = rings
            .iter()
            .map(|ring| crate::scene::ring_source_handles(ring, &near))
            .collect();
        let mut boundary_paths = crate::scene::exact_hatch_paths(&rings, &exterior, &near, 1.0e-6);

        if !self.associative {
            for handles in &mut boundary_sources {
                handles.clear();
            }
            for path in &mut boundary_paths {
                path.boundary_handles.clear();
                path.flags.set_external(false);
            }
        }
        if let Some((source, _, _)) = &self.inherited {
            let (name, mut pattern) = self
                .pattern_override
                .clone()
                .unwrap_or_else(|| (source.name.clone(), source.pattern.clone()));
            let angle = self.angle_override.unwrap_or(source.angle_offset);
            let scale = self.scale_override.unwrap_or(source.scale).max(1.0e-6);
            if let HatchPattern::Pattern(families) = &mut pattern {
                let (sin, cos) = angle.sin_cos();
                for family in families {
                    let base_x = source.world_origin[0]
                        + (family.x0 as f64 * cos as f64 - family.y0 as f64 * sin as f64)
                            * scale as f64;
                    let base_y = source.world_origin[1]
                        + (family.x0 as f64 * sin as f64 + family.y0 as f64 * cos as f64)
                            * scale as f64;
                    let dx = base_x - origin[0];
                    let dy = base_y - origin[1];
                    family.x0 = ((dx * cos as f64 + dy * sin as f64) / scale as f64) as f32;
                    family.y0 = ((-dx * sin as f64 + dy * cos as f64) / scale as f64) as f32;
                }
            }
            let build_ms = crate::perf::elapsed_ms(build_started);
            if perf_build && build_ms >= 1.0 {
                crate::perf_record!(
                    "[perf] hatch-build total={build_ms:.1}ms rings={ring_count} vertices={input_vertices} sources={} nearby_sources={near_source_count} inherited=true",
                    self.boundary_sources.len(),
                );
            }
            return HatchModel {
                pattern_origin: source.pattern_origin,
                render_instance: None,
                boundary: std::sync::Arc::new(rel),
                pattern,
                name,
                color: source.color,
                aci: source.aci,
                line_weight_px: source.line_weight_px,
                angle_offset: angle,
                scale,
                world_origin: origin,
                boundary_wcs: Some(std::sync::Arc::new(wcs)),
                fill_plane: Some(fill_plane),
                fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
                boundary_exterior: Some(std::sync::Arc::new(exterior)),
                boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
                boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
                style: self.island_style,
                draw_depth: source.draw_depth,
            };
        }
        // Default: ANSI31 from catalog; fallback to a single 45° family.
        let pat_name = "ANSI31";
        let default_pattern = crate::scene::model::hatch_patterns::find(pat_name)
            .and_then(|e| {
                if let HatchPattern::Pattern(f) = &e.gpu {
                    Some(HatchPattern::Pattern(f.clone()))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                // 45° lines, perpendicular spacing ≈ 5 world units.
                let dy = 5.0_f32 / (45.0_f32.to_radians().cos());
                HatchPattern::Pattern(vec![PatFamily {
                    angle_deg: 45.0,
                    x0: 0.0,
                    y0: 0.0,
                    dx: 0.0,
                    dy,
                    dashes: vec![],
                }])
            });
        let (name, mut pattern) = self
            .pattern_override
            .clone()
            .unwrap_or_else(|| (pat_name.to_string(), default_pattern));
        let angle = self.angle_override.unwrap_or(0.0);
        let scale = self.scale_override.unwrap_or(1.0).max(1.0e-6);
        if let (HatchPattern::Pattern(families), Some(anchor)) =
            (&mut pattern, local_boundary.first())
        {
            let (sin, cos) = (angle as f64).sin_cos();
            let dx = self.default_origin[0] - anchor[0] as f64;
            let dy = self.default_origin[1] - anchor[1] as f64;
            for family in families {
                family.x0 += ((dx * cos + dy * sin) / scale as f64) as f32;
                family.y0 += ((-dx * sin + dy * cos) / scale as f64) as f32;
            }
        }
        let build_ms = crate::perf::elapsed_ms(build_started);
        if perf_build && build_ms >= 1.0 {
            crate::perf_record!(
                "[perf] hatch-build total={build_ms:.1}ms rings={ring_count} vertices={input_vertices} sources={} nearby_sources={near_source_count} inherited=false",
                self.boundary_sources.len(),
            );
        }
        HatchModel {
            pattern_origin: Some(self.default_origin),
            render_instance: None,
            boundary: std::sync::Arc::new(rel),
            pattern,
            name,
            color: [0.75, 0.75, 0.75, 0.85],
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: self.angle_override.unwrap_or(0.0),
            scale: self.scale_override.unwrap_or(1.0).max(1.0e-6),
            world_origin: origin,
            boundary_wcs: Some(std::sync::Arc::new(wcs)),
            fill_plane: Some(fill_plane),
            fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
            boundary_exterior: Some(std::sync::Arc::new(exterior)),
            boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
            boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
            style: self.island_style,
            draw_depth: 0.0,
        }
    }

    fn manual_boundary_path(&self) -> Option<codec::entities::BoundaryPath> {
        use codec::entities::{BoundaryEdge, BoundaryPath, PolylineEdge};
        use codec::types::Vector3;
        if self.manual_pts.len() < 2 {
            return None;
        }
        let vertices = self
            .manual_pts
            .iter()
            .enumerate()
            .map(|(index, point)| {
                Vector3::new(
                    point.x,
                    point.y,
                    self.manual_bulges.get(index).copied().unwrap_or(0.0),
                )
            })
            .collect();
        let mut path = BoundaryPath::new();
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge {
            vertices,
            is_closed: true,
        }));
        Some(path)
    }
}

fn arc_bulge(start: DVec3, middle: DVec3, end: DVec3) -> Option<f64> {
    let curvature = DVec3::from_array(kernel::space::curve::curvature_through(
        start.to_array(),
        middle.to_array(),
        end.to_array(),
    ));
    let squared = curvature.length_squared();
    if squared <= f64::MIN_POSITIVE {
        return None;
    }
    let centre = start + curvature / squared;
    let circle = Curve::Circle(Circle {
        centre: [centre.x, centre.y],
        radius: squared.sqrt().recip(),
    });
    let first = circle.parameter_at([start.x, start.y]);
    let through = (circle.parameter_at([middle.x, middle.y]) - first).rem_euclid(1.0);
    let ccw = (circle.parameter_at([end.x, end.y]) - first).rem_euclid(1.0);
    let sweep = if through <= ccw + 1.0e-12 {
        ccw * std::f64::consts::TAU
    } else {
        (ccw - 1.0) * std::f64::consts::TAU
    };
    Some((sweep * 0.25).tan())
}

impl CadCommand for HatchCommand {
    fn name(&self) -> &'static str {
        "HATCH"
    }

    fn prompt(&self) -> String {
        match self.step {
            HatchStep::Settings => crate::tr!("hatch-cmd", "settings"),
            HatchStep::Gap => crate::tr!("hatch-cmd", "gap", gap = self.gap_tolerance.to_string()),
            HatchStep::DrawMode => crate::tr!("hatch-cmd", "mode"),
            HatchStep::Length => crate::tr!("hatch-cmd", "length"),
            HatchStep::RectFirst => crate::tr!("hatch-cmd", "rect-first"),
            HatchStep::RectOther(_) => crate::tr!("hatch-cmd", "rect-other"),
            HatchStep::CircleCenter => crate::tr!("hatch-cmd", "circle-center"),
            HatchStep::CircleRadius(_) => crate::tr!(
                "hatch-cmd",
                "circle-radius",
                radius = format!("{:.4}", crate::modules::draw::defaults::get_circle_radius())
            ),
            HatchStep::CircleDiameter(_) => crate::tr!(
                "hatch-cmd",
                "circle-diameter",
                diameter =
                    format!("{:.4}", crate::modules::draw::defaults::get_circle_radius() * 2.0)
            ),
            HatchStep::Main => match self.mode {
                HatchMode::PickInside => crate::tr!("hatch-cmd", "pick"),
                HatchMode::SelectObjects => crate::tr!("hatch-cmd", "select"),
                HatchMode::Manual if self.manual_pts.is_empty() => {
                    crate::tr!("hatch-cmd", "draw-start")
                }
                HatchMode::Manual if self.manual_arc_mode => {
                    if self.manual_arc_midpoint.is_none() {
                        crate::tr!("hatch-cmd", "arc-second")
                    } else {
                        crate::tr!("hatch-cmd", "arc-end")
                    }
                }
                HatchMode::Manual if self.manual_pts.len() >= 3 => {
                    crate::tr!("hatch-cmd", "next-close")
                }
                HatchMode::Manual => crate::tr!("hatch-cmd", "next"),
            },
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        let accept = |options: &mut Vec<CmdOption>| {
            if self.region_count() > 0 {
                options.push(CmdOption::enter(t!("Accept").as_ref()));
            }
        };
        match self.step {
            HatchStep::Settings => vec![
                CmdOption::new(
                    &format!("Keep boundaries: {}", if self.retain_boundaries { "Yes" } else { "No" }),
                    "K",
                ),
                CmdOption::new(
                    &format!("Associative: {}", if self.associative { "Yes" } else { "No" }),
                    "A",
                ),
                CmdOption::new(
                    &format!(
                        "Separate hatches: {}",
                        if self.separate_hatches { "Yes" } else { "No" }
                    ),
                    "S",
                ),
                CmdOption::new(&format!("Island style: {}", self.island_style_label()), "I"),
                CmdOption::new(&format!("Gap tolerance: {}", self.gap_tolerance), "G"),
                CmdOption::enter("Exit"),
            ],
            HatchStep::DrawMode => vec![CmdOption::new("Area", "A"), CmdOption::new("Path", "P")],
            HatchStep::CircleRadius(_) => vec![CmdOption::new("Diameter", "D")],
            HatchStep::Main => match self.mode {
                HatchMode::PickInside => {
                    let mut options = vec![
                        CmdOption::new("Select objects", "S"),
                        CmdOption::new("Draw", "D"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::new("seTtings", "T"),
                    ];
                    accept(&mut options);
                    options
                }
                HatchMode::SelectObjects => {
                    let mut options = vec![
                        CmdOption::new("picK internal point", "K"),
                        CmdOption::new("Draw", "D"),
                        CmdOption::new("Undo", "U"),
                        CmdOption::new("seTtings", "T"),
                    ];
                    accept(&mut options);
                    options
                }
                HatchMode::Manual if self.manual_pts.is_empty() => vec![
                    CmdOption::new("picK internal point", "K"),
                    CmdOption::new("Select objects", "S"),
                    CmdOption::new("Rectangle", "R"),
                    CmdOption::new("Circle", "C"),
                    CmdOption::new("Mode", "M"),
                    CmdOption::new("seTtings", "T"),
                ],
                HatchMode::Manual if self.manual_arc_mode => {
                    vec![CmdOption::new("Line", "L"), CmdOption::new("Undo", "U")]
                }
                HatchMode::Manual => {
                    let mut options = vec![CmdOption::new("Arc", "A")];
                    if self.manual_pts.len() >= 3 {
                        options.push(CmdOption::new("Close", "C"));
                    }
                    options.push(CmdOption::new("Length", "L"));
                    options.push(CmdOption::new("Undo", "U"));
                    options.push(CmdOption::enter("Exit"));
                    options
                }
            },
            _ => Vec::new(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let pt = self.plane.to_local(pt);
        match self.step {
            HatchStep::Settings | HatchStep::Gap | HatchStep::DrawMode => {
                return CmdResult::NeedPoint
            }
            HatchStep::Length => {
                let last = self.manual_pts.last().copied().unwrap_or(pt);
                return self.add_length(last.distance(pt));
            }
            HatchStep::RectFirst => {
                self.step = HatchStep::RectOther(pt);
                return CmdResult::NeedPoint;
            }
            HatchStep::RectOther(first) => {
                if (first.x - pt.x).abs() <= 1.0e-12 || (first.y - pt.y).abs() <= 1.0e-12 {
                    return CmdResult::NeedPoint;
                }
                self.manual_pts = vec![
                    first,
                    DVec3::new(pt.x, first.y, first.z),
                    pt,
                    DVec3::new(first.x, pt.y, first.z),
                ];
                self.manual_bulges.clear();
                return self.finish_drawn();
            }
            HatchStep::CircleCenter => {
                self.step = HatchStep::CircleRadius(pt);
                return CmdResult::NeedPoint;
            }
            HatchStep::CircleRadius(centre) => return self.finish_circle(centre, centre.distance(pt)),
            HatchStep::CircleDiameter(centre) => {
                return self.finish_circle(centre, centre.distance(pt) * 0.5)
            }
            HatchStep::Main => {}
        }
        match self.mode {
            HatchMode::PickInside => {
                let xy = [pt.x, pt.y];
                let faces = crate::scene::boundary_faces_around(
                    &self.boundary_sources,
                    xy,
                    self.gap_tolerance,
                );
                match resolve_hatch_rings(&faces, xy) {
                    Some(rings) => {
                        self.add_point_region(rings);
                        let islands = crate::tr!("hatch-cmd", "analyzing-islands");
                        let report = if std::mem::replace(&mut self.analyzed, true) {
                            islands
                        } else {
                            [
                                crate::tr!("hatch-cmd", "selecting"),
                                crate::tr!("hatch-cmd", "selecting-visible"),
                                crate::tr!("hatch-cmd", "analyzing-data"),
                                islands,
                            ]
                            .join("\n")
                        };
                        CmdResult::ReportMeasurement(report)
                    }
                    None => CmdResult::ReportError(crate::tr!("hatch-cmd", "no-boundary")),
                }
            }
            HatchMode::SelectObjects => CmdResult::NeedPoint,
            HatchMode::Manual => {
                if self.manual_pts.is_empty() || !self.manual_arc_mode {
                    if !self.manual_pts.is_empty() {
                        self.manual_bulges.push(0.0);
                    }
                    self.manual_pts.push(pt);
                } else if let Some(middle) = self.manual_arc_midpoint.take() {
                    let start = *self.manual_pts.last().unwrap();
                    self.manual_bulges
                        .push(arc_bulge(start, middle, pt).unwrap_or(0.0));
                    self.manual_pts.push(pt);
                } else {
                    self.manual_arc_midpoint = Some(pt);
                }
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            HatchStep::Settings | HatchStep::Length | HatchStep::RectFirst
            | HatchStep::RectOther(_) | HatchStep::CircleCenter => {
                self.step = HatchStep::Main;
                return CmdResult::NeedPoint;
            }
            HatchStep::Gap => {
                self.step = HatchStep::Settings;
                return CmdResult::NeedPoint;
            }
            HatchStep::DrawMode => {
                self.step = HatchStep::Main;
                return Self::current_mode();
            }
            HatchStep::CircleRadius(centre) | HatchStep::CircleDiameter(centre) => {
                return self
                    .finish_circle(centre, crate::modules::draw::defaults::get_circle_radius());
            }
            HatchStep::Main => {}
        }
        if self.mode == HatchMode::Manual {
            if self.manual_arc_midpoint.is_some() {
                return CmdResult::NeedPoint;
            }
            if self.manual_pts.len() >= 3 {
                return self.finish_drawn();
            }
            if !self.manual_pts.is_empty() {
                self.reset_drawing();
                return CmdResult::NeedPoint;
            }
        }
        let rings = self.combined_rings();
        if rings.is_empty() {
            CmdResult::Done
        } else if self.separate_hatches && !self.retain_boundaries {
            let hatches = self
                .point_regions
                .iter()
                .chain(self.object_regions.iter())
                .cloned()
                .map(|region| self.make_hatch(region))
                .collect();
            CmdResult::CommitHatches {
                hatches,
                entity_style: self
                    .inherited
                    .as_ref()
                    .map(|(_, color, transparency)| (color.clone(), *transparency)),
            }
        } else if self.retain_boundaries {
            CmdResult::CommitHatchWithBoundaries {
                hatch: self.make_hatch(rings.clone()),
                boundaries: crate::scene::boundary_entities_from_sources(
                    &rings,
                    self.plane,
                    &self.boundary_sources,
                    1.0e-6,
                ),
                entity_style: self
                    .inherited
                    .as_ref()
                    .map(|(_, color, transparency)| (color.clone(), *transparency)),
            }
        } else if let Some((_, color, transparency)) = &self.inherited {
            CmdResult::CommitStyledHatch {
                hatch: self.make_hatch(rings),
                color: color.clone(),
                transparency: *transparency,
            }
        } else {
            CmdResult::CommitHatch(self.make_hatch(rings))
        }
    }

    fn wants_raw_point(&self) -> bool {
        self.mode == HatchMode::PickInside && self.step == HatchStep::Main
    }

    fn hatch_gap_tolerance(&self) -> Option<f64> {
        Some(self.gap_tolerance)
    }

    fn hatch_start_method(&self) -> Option<u8> {
        self.method_chosen.map(method_code)
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if self.dyn_commit_as_text() {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(
            self.step,
            HatchStep::Gap
                | HatchStep::Length
                | HatchStep::CircleRadius(_)
                | HatchStep::CircleDiameter(_)
        )
    }

    fn is_selection_gathering(&self) -> bool {
        self.mode == HatchMode::SelectObjects && self.step == HatchStep::Main
    }

    fn selection_forces_add(&self) -> bool {
        self.is_selection_gathering()
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if !self.is_selection_gathering() {
            return CmdResult::Cancel;
        }
        if self.set_object_selection(handles) {
            CmdResult::ReportError(crate::tr!("hatch-cmd", "no-boundary"))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.step != HatchStep::Main {
            return None;
        }
        match self.mode {
            HatchMode::Manual if !self.manual_pts.is_empty() => Some(self.undo_drawn_point()),
            HatchMode::PickInside if self.point_regions.pop().is_some() => {
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn hatch_preview_models(&self) -> Option<Vec<HatchModel>> {
        let mut rings = self.combined_rings();
        if self.mode == HatchMode::Manual && self.manual_pts.len() >= 3 {
            rings.push(self.drawn_ring());
        }
        Some(if rings.is_empty() {
            Vec::new()
        } else {
            let mut preview = self.make_hatch(rings);
            preview.color = [0.15, 0.55, 1.0, 0.75];
            vec![preview]
        })
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let input = text.trim();
        let upper = input.to_ascii_uppercase();
        let point_or_keyword =
            || CmdResult::ReportError(t!("Point or option keyword required.").into_owned());
        // A typed coordinate is a point for whichever step asks for one.
        let typed_point = input.contains(',') || input.starts_with('@') || input.starts_with('<');
        match self.step {
            HatchStep::Gap => {
                if let Some(value) = parse_gap_tolerance(input) {
                    self.gap_tolerance = value;
                }
                return Some(self.settings());
            }
            HatchStep::Settings => {
                match upper.as_str() {
                    "K" | "KEEP" => {
                        self.retain_boundaries = !self.retain_boundaries;
                        if self.retain_boundaries {
                            self.separate_hatches = false;
                        }
                    }
                    "A" | "ASSOCIATIVE" => self.associative = !self.associative,
                    "S" | "SEPARATE" => {
                        self.separate_hatches = !self.separate_hatches;
                        if self.separate_hatches {
                            self.retain_boundaries = false;
                        }
                    }
                    "I" | "ISLAND" => {
                        use codec::entities::HatchStyleType::{Ignore, Normal, Outer};
                        self.island_style = match self.island_style {
                            Normal => Outer,
                            Outer => Ignore,
                            Ignore => Normal,
                        };
                    }
                    "G" | "GAP" => {
                        self.step = HatchStep::Gap;
                        return Some(CmdResult::NeedPoint);
                    }
                    "X" | "EXIT" => {
                        self.step = HatchStep::Main;
                        return Some(CmdResult::NeedPoint);
                    }
                    _ => {
                        return Some(CmdResult::ReportError(
                            t!("Invalid option keyword.").into_owned(),
                        ))
                    }
                }
                return Some(self.settings());
            }
            HatchStep::DrawMode => {
                return Some(match upper.as_str() {
                    "A" | "AREA" => {
                        self.step = HatchStep::Main;
                        Self::current_mode()
                    }
                    "P" | "PATH" => {
                        self.step = HatchStep::Main;
                        CmdResult::ReportError(crate::tr!("hatch-cmd", "path-unavailable"))
                    }
                    _ => CmdResult::ReportError(t!("Invalid option keyword.").into_owned()),
                });
            }
            HatchStep::Length => {
                return Some(match parse_number(input) {
                    Some(value) => self.add_length(value),
                    None => CmdResult::ReportError(crate::tr!("hatch-cmd", "numeric")),
                });
            }
            HatchStep::CircleRadius(_) | HatchStep::CircleDiameter(_) if typed_point => return None,
            HatchStep::CircleRadius(centre) | HatchStep::CircleDiameter(centre) => {
                if matches!(self.step, HatchStep::CircleRadius(_))
                    && matches!(upper.as_str(), "D" | "DIAMETER")
                {
                    self.step = HatchStep::CircleDiameter(centre);
                    return Some(CmdResult::NeedPoint);
                }
                let factor = if matches!(self.step, HatchStep::CircleDiameter(_)) { 0.5 } else { 1.0 };
                return Some(match parse_number(input) {
                    Some(value) => self.finish_circle(centre, value * factor),
                    None => CmdResult::ReportError(crate::tr!("hatch-cmd", "numeric")),
                });
            }
            HatchStep::RectFirst | HatchStep::RectOther(_) | HatchStep::CircleCenter => {
                return (!typed_point).then(point_or_keyword);
            }
            HatchStep::Main => {}
        }
        match self.mode {
            HatchMode::Manual if self.manual_pts.is_empty() => Some(match upper.as_str() {
                "K" | "PICK" => self.choose(HatchMode::PickInside),
                "S" | "SELECT" => self.choose(HatchMode::SelectObjects),
                "R" | "RECTANGLE" => {
                    self.step = HatchStep::RectFirst;
                    CmdResult::NeedPoint
                }
                "C" | "CIRCLE" => {
                    self.step = HatchStep::CircleCenter;
                    CmdResult::NeedPoint
                }
                "M" | "MODE" => {
                    self.step = HatchStep::DrawMode;
                    CmdResult::NeedPoint
                }
                "T" | "SETTINGS" => self.settings(),
                _ if typed_point => return None,
                _ => point_or_keyword(),
            }),
            HatchMode::Manual => match upper.as_str() {
                "A" | "ARC" if !self.manual_arc_mode => {
                    self.manual_arc_mode = true;
                    self.manual_arc_midpoint = None;
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LINE" if self.manual_arc_mode => {
                    self.manual_arc_mode = false;
                    self.manual_arc_midpoint = None;
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LENGTH" => {
                    self.step = HatchStep::Length;
                    Some(CmdResult::NeedPoint)
                }
                "C" | "CLOSE" if self.manual_pts.len() >= 3 && !self.manual_arc_mode => {
                    Some(self.finish_drawn())
                }
                "U" | "UNDO" => Some(self.undo_drawn_point()),
                "E" | "EXIT" if !self.manual_arc_mode => Some(self.on_enter()),
                _ => None,
            },
            HatchMode::PickInside | HatchMode::SelectObjects => {
                let picking = self.mode == HatchMode::PickInside;
                match upper.as_str() {
                    "S" | "SELECT" if picking => return Some(self.choose(HatchMode::SelectObjects)),
                    "K" | "PICK" if !picking => return Some(self.choose(HatchMode::PickInside)),
                    "D" | "DRAW" => return Some(self.choose(HatchMode::Manual)),
                    "T" | "SETTINGS" => return Some(self.settings()),
                    "U" | "UNDO" if picking => {
                        self.point_regions.pop();
                        return Some(CmdResult::NeedPoint);
                    }
                    // Select mode's Undo takes back the last selection step.
                    _ => {}
                }
                // Typed pattern / angle / scale: `P<name>`, `A<degrees>`, `L<scale>`.
                if let Some(name) = upper.strip_prefix('P').map(str::trim).filter(|s| !s.is_empty()) {
                    if let Some(entry) = crate::scene::model::hatch_patterns::find(name) {
                        self.pattern_override = Some((entry.name.clone(), entry.gpu.clone()));
                        return Some(CmdResult::NeedPoint);
                    }
                }
                if let Some(value) = upper.strip_prefix('A').and_then(|rest| parse_number(rest)) {
                    self.angle_override = Some((value as f32).to_radians());
                    return Some(CmdResult::NeedPoint);
                }
                if let Some(value) = upper
                    .strip_prefix('L')
                    .and_then(|rest| parse_number(rest))
                    .filter(|value| *value > 0.0)
                {
                    self.scale_override = Some(value as f32);
                    return Some(CmdResult::NeedPoint);
                }
                None
            }
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let local = self.plane.to_local(pt);
        let world = |p: DVec3| self.plane.to_world(p).as_vec3().to_array();
        let points: Vec<[f32; 3]> = match self.step {
            HatchStep::RectOther(first) => vec![
                world(first),
                world(DVec3::new(local.x, first.y, first.z)),
                world(DVec3::new(local.x, local.y, first.z)),
                world(DVec3::new(first.x, local.y, first.z)),
                world(first),
            ],
            HatchStep::CircleRadius(centre) | HatchStep::CircleDiameter(centre) => {
                let mut radius = DVec3::new(local.x, local.y, centre.z).distance(centre);
                if matches!(self.step, HatchStep::CircleDiameter(_)) {
                    radius *= 0.5;
                }
                (0..=64)
                    .map(|index| {
                        let angle = index as f64 / 64.0 * std::f64::consts::TAU;
                        world(centre + DVec3::new(angle.cos(), angle.sin(), 0.0) * radius)
                    })
                    .collect()
            }
            HatchStep::Main if self.mode == HatchMode::Manual && !self.manual_pts.is_empty() => {
                let mut points: Vec<[f32; 3]> =
                    self.manual_pts.iter().map(|&p| world(p)).collect();
                points.push(pt.as_vec3().to_array());
                points.push(world(self.manual_pts[0]));
                points
            }
            _ => return None,
        };
        Some(WireModel::solid(
            "rubber_band".into(),
            points,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── GRADIENT command ───────────────────────────────────────────────────────

pub struct GradientCommand {
    boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    mode: Mode,
    manual_pts: Vec<DVec3>,
    missed: bool,
    /// Gradient shape, switchable via the prompt options (#415).
    kind: crate::scene::model::hatch_model::GradientKind,
    /// Swap the two colour stops.
    invert: bool,
    /// HPGAPTOL: gaps up to this size count as closed for a picked point.
    gap_tolerance: f64,
}

impl GradientCommand {
    pub fn new(
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    ) -> Self {
        Self {
            boundary_sources,
            mode: Mode::PickInside,
            manual_pts: vec![],
            missed: false,
            kind: crate::scene::model::hatch_model::GradientKind::Linear,
            invert: false,
            gap_tolerance: 0.0,
        }
    }

    /// HPGAPTOL: the largest gap a picked boundary may have.
    pub fn with_gap_tolerance(mut self, gap_tolerance: f64) -> Self {
        self.gap_tolerance = gap_tolerance;
        self
    }

    fn make_hatch(&self, rings: Vec<Vec<[f64; 2]>>) -> HatchModel {
        let (rel, origin, wcs) = pack_rings(&rings);
        let exterior: Vec<bool> = kernel::geom2d::ring_nesting_depths(&rings)
            .into_iter()
            .map(|depth| depth == 0)
            .collect();
        let near = crate::scene::sources_near_rings(&rings, &self.boundary_sources, 1.0e-6);
        let boundary_sources = rings
            .iter()
            .map(|ring| crate::scene::ring_source_handles(ring, &near))
            .collect();
        let boundary_paths = crate::scene::exact_hatch_paths(&rings, &exterior, &near, 1.0e-6);
        HatchModel {
            pattern_origin: None,
            render_instance: None,
            boundary: std::sync::Arc::new(rel),
            pattern: HatchPattern::Gradient {
                angle_deg: 0.0,
                color2: [0.18, 0.18, 0.18, 0.0],
                kind: self.kind,
                invert: self.invert,
                shift: 0.0,
            },
            name: self.kind.dxf_name(self.invert).into(),
            color: [0.30, 0.60, 0.95, 0.80],
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            world_origin: origin,
            boundary_wcs: Some(std::sync::Arc::new(wcs)),
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: Some(std::sync::Arc::new(exterior)),
            boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
            boundary_paths: Some(std::sync::Arc::new(boundary_paths)),
            style: codec::entities::HatchStyleType::Normal,
            draw_depth: 0.0,
        }
    }
}

impl CadCommand for GradientCommand {
    fn name(&self) -> &'static str {
        "GRADIENT"
    }

    fn prompt(&self) -> String {
        match &self.mode {
            Mode::PickInside => {
                let miss = if self.missed {
                    t!("  ⚠ No closed boundary found.")
                } else {
                    std::borrow::Cow::Borrowed("")
                };
                t!(
                    "GRADIENT (%{kind}%{invert})  Pick internal point:%{miss}",
                    kind = t!(self.kind.choice_label(self.invert)),
                    invert = std::borrow::Cow::Borrowed(""),
                    miss = miss
                )
                .into_owned()
            }
            Mode::Manual => {
                if self.manual_pts.is_empty() {
                    t!("GRADIENT  Boundary point 1:").into_owned()
                } else {
                    t!("GRADIENT  Point %{n}:", n = self.manual_pts.len() + 1).into_owned()
                }
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match &self.mode {
            Mode::PickInside => {
                let mut opts = vec![CmdOption::new("Draw manually", "S")];
                for (kind, inverted) in crate::scene::model::hatch_model::GradientKind::CHOICES {
                    if kind != self.kind || inverted != self.invert {
                        let label = kind.choice_label(inverted);
                        opts.push(CmdOption::new(label, label));
                    }
                }
                opts
            }
            Mode::Manual => {
                // Enter accepts the boundary once at least 3 points are picked.
                if self.manual_pts.len() >= 3 {
                    vec![CmdOption::enter("Accept")]
                } else {
                    vec![]
                }
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.mode {
            Mode::PickInside => {
                let xy = [pt.x, pt.y];
                let faces = crate::scene::boundary_faces_around(
                    &self.boundary_sources,
                    xy,
                    self.gap_tolerance,
                );
                match resolve_hatch_rings(&faces, xy) {
                    Some(rings) => {
                        self.missed = false;
                        return CmdResult::CommitHatch(self.make_hatch(rings));
                    }
                    None => {
                        self.missed = true;
                        CmdResult::NeedPoint
                    }
                }
            }
            Mode::Manual => {
                // Keep the typed/snapped point exact (issue #311).
                self.manual_pts.push(pt);
                CmdResult::NeedPoint
            }
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.mode {
            Mode::PickInside => CmdResult::Cancel,
            Mode::Manual => {
                if self.manual_pts.len() < 3 {
                    return CmdResult::Cancel;
                }
                let wcs = self.manual_pts.iter().map(|p| [p.x, p.y]).collect();
                CmdResult::CommitHatch(self.make_hatch(vec![wcs]))
            }
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_raw_point(&self) -> bool {
        matches!(self.mode, Mode::PickInside)
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.mode, Mode::PickInside)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if t.eq_ignore_ascii_case("s") {
            self.mode = Mode::Manual;
            self.missed = false;
            return Some(CmdResult::NeedPoint);
        }
        if let Some((kind, inverted)) =
            crate::scene::model::hatch_model::GradientKind::from_choice_label(t)
        {
            self.kind = kind;
            self.invert = inverted;
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let pt = pt.as_vec3();
        if let Mode::Manual = &self.mode {
            if self.manual_pts.is_empty() {
                return None;
            }
            let mut pts: Vec<[f32; 3]> = self
                .manual_pts
                .iter()
                .map(|p| [p.x as f32, p.y as f32, p.z as f32])
                .collect();
            pts.push([pt.x, pt.y, pt.z]);
            pts.push([
                self.manual_pts[0].x as f32,
                self.manual_pts[0].y as f32,
                self.manual_pts[0].z as f32,
            ]);
            return Some(WireModel::solid(
                "rubber_band".into(),
                pts,
                WireModel::CYAN,
                false,
            ));
        }
        None
    }
}

// ── BOUNDARY command ───────────────────────────────────────────────────────

pub struct BoundaryCommand {
    sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    active_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    restrict_sources: bool,
    point_regions: Vec<Vec<Vec<[f64; 2]>>>,
    selected_objects: Vec<Handle>,
    mode: BoundaryMode,
    island_style: BoundaryIslandStyle,
    gap_tolerance: f64,
    plane: WorkingPlane,
    missed: bool,
    output_region: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryMode {
    PickInside,
    Advanced,
    ObjectType,
    SelectObjects,
    GapTolerance { return_to_selection: bool },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryIslandStyle {
    Normal,
    Outer,
    Ignore,
}

impl BoundaryCommand {
    pub fn new(
        sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        selected_objects: Vec<Handle>,
        plane: WorkingPlane,
        gap_tolerance: f64,
    ) -> Self {
        let mut command = Self {
            sources,
            active_sources: rustc_hash::FxHashMap::default(),
            restrict_sources: false,
            point_regions: Vec::new(),
            selected_objects: Vec::new(),
            mode: BoundaryMode::PickInside,
            island_style: BoundaryIslandStyle::Normal,
            gap_tolerance,
            plane,
            missed: false,
            output_region: false,
        };
        if !selected_objects.is_empty() {
            command.set_boundary_set(selected_objects);
        }
        command
    }

    fn set_boundary_set(&mut self, handles: Vec<Handle>) {
        let restrict_sources = !handles.is_empty();
        let selected_sources: rustc_hash::FxHashMap<_, _> = handles
            .iter()
            .filter_map(|handle| {
                self.sources
                    .get(handle)
                    .cloned()
                    .map(|source| (*handle, source))
            })
            .collect();
        self.active_sources = selected_sources;
        self.restrict_sources = restrict_sources;
        // Only a chosen boundary set is checked up front: it is small, while
        // the whole drawing is resolved around each pick instead (#1869).
        self.missed = restrict_sources
            && crate::scene::boundary_faces(&self.active_sources, self.gap_tolerance).is_empty();
        self.selected_objects = handles;
        self.point_regions.clear();
    }

    fn rebuild_faces(&mut self) {
        self.set_boundary_set(self.selected_objects.clone());
    }

    fn region_count(&self) -> usize {
        self.point_regions.len()
    }

    fn island_label(&self) -> &'static str {
        match self.island_style {
            BoundaryIslandStyle::Normal => "Normal",
            BoundaryIslandStyle::Outer => "Outer",
            BoundaryIslandStyle::Ignore => "Ignore",
        }
    }

    fn picked_region(&self, point: [f64; 2]) -> Option<Vec<Vec<[f64; 2]>>> {
        let sources = if self.restrict_sources {
            &self.active_sources
        } else {
            &self.sources
        };
        let outlines = crate::scene::boundary_faces_around(sources, point, self.gap_tolerance);
        let tolerance = Tolerance::new(1.0e-6);
        let curves = |outline: &Vec<[f64; 2]>| {
            outline
                .iter()
                .copied()
                .zip(outline.iter().copied().cycle().skip(1))
                .take(outline.len())
                .map(|(start, end)| Curve::Line(Line { start, end }))
                .collect::<Vec<_>>()
        };
        let mut containing: Vec<(usize, f64)> = outlines
            .iter()
            .enumerate()
            .filter(|(_, outline)| contains(&curves(outline), point, tolerance))
            .map(|(index, outline)| (index, signed_area(outline).abs()))
            .collect();
        containing.sort_by(|left, right| left.1.total_cmp(&right.1));
        let outer_index = containing.first()?.0;
        let outer = &outlines[outer_index];
        let outer_curves = curves(outer);
        let depths = ring_nesting_depths(&outlines);
        let outer_depth = depths.get(outer_index).copied().unwrap_or(0);
        let mut rings = vec![outer.clone()];
        if self.island_style == BoundaryIslandStyle::Ignore {
            return Some(rings);
        }
        for (index, candidate) in outlines.iter().enumerate() {
            let Some(seed) = candidate.first().copied() else {
                continue;
            };
            let candidate_curves = curves(candidate);
            let candidate_depth = depths.get(index).copied().unwrap_or(0);
            if index == outer_index
                || candidate_depth <= outer_depth
                || !contains(&outer_curves, seed, tolerance)
                || contains(&candidate_curves, point, tolerance)
            {
                continue;
            }
            if self.island_style == BoundaryIslandStyle::Outer && candidate_depth != outer_depth + 1
            {
                continue;
            }
            rings.push(candidate.clone());
        }
        Some(rings)
    }

    fn add_point_region(&mut self, region: Vec<Vec<[f64; 2]>>) {
        if !self
            .point_regions
            .iter()
            .any(|existing| existing == &region)
        {
            self.point_regions.push(region);
        }
    }

    fn make_entities(&self) -> Vec<codec::EntityType> {
        let sources = if self.restrict_sources {
            &self.active_sources
        } else {
            &self.sources
        };
        if self.output_region {
            return self
                .point_regions
                .iter()
                .filter_map(|rings| {
                    crate::scene::model::presspull_model::boundary_region(
                        sources, rings, self.plane,
                    )
                })
                .collect();
        }
        crate::scene::boundary_polyline_entities(
            &self.point_regions,
            self.plane,
            sources,
            1.0e-6,
        )
    }
}

impl CadCommand for BoundaryCommand {
    fn name(&self) -> &'static str {
        "BOUNDARY"
    }

    fn prompt(&self) -> String {
        let miss = if self.missed {
            t!("  ⚠ No closed boundary found.").into_owned()
        } else {
            String::new()
        };
        match self.mode {
            BoundaryMode::PickInside => {
                format!("BOUNDARY  Specify internal point or [Advanced options]:{miss}")
            }
            BoundaryMode::Advanced => "BOUNDARY  Enter an option [Object type]:".into(),
            BoundaryMode::ObjectType => format!(
                "BOUNDARY  Enter type of boundary object [Region/Polyline] <{}>:",
                if self.output_region {
                    "Region"
                } else {
                    "Polyline"
                }
            ),
            BoundaryMode::SelectObjects => {
                t!("%{cmd}  Select objects:", cmd = self.name()).into_owned()
            }
            BoundaryMode::GapTolerance { .. } => {
                format!("BOUNDARY  {} <{}>:", t!("Gap tolerance"), self.gap_tolerance)
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.mode == BoundaryMode::Advanced {
            return vec![CmdOption::new("Object type", "O")];
        }
        if self.mode == BoundaryMode::ObjectType {
            return vec![
                CmdOption::new("Region", "R"),
                CmdOption::new("Polyline", "P"),
            ];
        }
        if matches!(self.mode, BoundaryMode::GapTolerance { .. }) {
            return Vec::new();
        }
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            return vec![CmdOption::enter(t!("Accept").as_ref())];
        }
        let island = format!(
            "{}: {}",
            t!("Island detection style"),
            t!(self.island_label())
        );
        let mut options = vec![
            CmdOption::new("Advanced options", "A"),
            CmdOption::new(t!("Boundary").as_ref(), "O"),
            CmdOption::new(&island, "S"),
            CmdOption::new(
                &format!("{}: {}", t!("Gap tolerance"), self.gap_tolerance),
                "G",
            ),
        ];
        if self.region_count() > 0 {
            options.push(CmdOption::enter("Accept"));
        }
        options
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if matches!(self.mode, BoundaryMode::PickInside) {
            let local = self.plane.to_local(pt);
            match self.picked_region([local.x, local.y]) {
                Some(region) => {
                    self.missed = false;
                    self.add_point_region(region);
                }
                None => self.missed = true,
            }
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.mode == BoundaryMode::ObjectType {
            self.mode = BoundaryMode::Advanced;
            return CmdResult::NeedPoint;
        }
        if self.mode == BoundaryMode::Advanced {
            self.mode = BoundaryMode::PickInside;
            return CmdResult::NeedPoint;
        }
        if let BoundaryMode::GapTolerance {
            return_to_selection,
        } = self.mode
        {
            self.mode = if return_to_selection {
                BoundaryMode::SelectObjects
            } else {
                BoundaryMode::PickInside
            };
            return CmdResult::NeedPoint;
        }
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            self.mode = BoundaryMode::PickInside;
            return CmdResult::NeedPoint;
        }
        let entities = self.make_entities();
        if entities.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::CommitEntitiesAndExit(entities)
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.mode, BoundaryMode::GapTolerance { .. }) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn dyn_commit_as_text(&self) -> bool {
        matches!(self.mode, BoundaryMode::GapTolerance { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.mode == BoundaryMode::Advanced {
            if matches!(
                text.trim().to_ascii_uppercase().as_str(),
                "O" | "OBJECT" | "OBJECT TYPE"
            ) {
                self.mode = BoundaryMode::ObjectType;
            }
            return Some(CmdResult::NeedPoint);
        }
        if self.mode == BoundaryMode::ObjectType {
            match text.trim().to_ascii_uppercase().as_str() {
                "R" | "REGION" => self.output_region = true,
                "P" | "POLYLINE" => self.output_region = false,
                _ => return Some(CmdResult::NeedPoint),
            }
            self.mode = BoundaryMode::Advanced;
            return Some(CmdResult::NeedPoint);
        }
        if let BoundaryMode::GapTolerance {
            return_to_selection,
        } = self.mode
        {
            if let Some(value) = parse_gap_tolerance(text) {
                self.gap_tolerance = value;
                self.rebuild_faces();
            }
            self.mode = if return_to_selection {
                BoundaryMode::SelectObjects
            } else {
                BoundaryMode::PickInside
            };
            return Some(CmdResult::NeedPoint);
        }
        match text.trim().to_ascii_uppercase().as_str() {
            "A" | "ADVANCED" => self.mode = BoundaryMode::Advanced,
            "O" | "OBJECT" | "OBJECTS" => {
                self.mode = BoundaryMode::SelectObjects;
                self.missed = false;
            }
            "I" | "INTERNAL" | "POINTS" => {
                self.mode = BoundaryMode::PickInside;
                self.missed = false;
            }
            "S" | "ISLAND" | "ISLANDS" => {
                let style = match self.island_style {
                    BoundaryIslandStyle::Normal => BoundaryIslandStyle::Outer,
                    BoundaryIslandStyle::Outer => BoundaryIslandStyle::Ignore,
                    BoundaryIslandStyle::Ignore => BoundaryIslandStyle::Normal,
                };
                self.island_style = style;
                self.point_regions.clear();
            }
            "NORMAL" => {
                self.island_style = BoundaryIslandStyle::Normal;
                self.point_regions.clear();
            }
            "OUTER" => {
                self.island_style = BoundaryIslandStyle::Outer;
                self.point_regions.clear();
            }
            "IGNORE" => {
                self.island_style = BoundaryIslandStyle::Ignore;
                self.point_regions.clear();
            }
            "G" | "GAP" | "TOLERANCE" => {
                self.mode = BoundaryMode::GapTolerance {
                    return_to_selection: matches!(self.mode, BoundaryMode::SelectObjects),
                };
            }
            _ => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn wants_raw_point(&self) -> bool {
        self.mode == BoundaryMode::PickInside
    }

    fn hatch_gap_tolerance(&self) -> Option<f64> {
        Some(self.gap_tolerance)
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.mode, BoundaryMode::SelectObjects)
    }

    fn selection_forces_add(&self) -> bool {
        matches!(self.mode, BoundaryMode::SelectObjects)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if matches!(self.mode, BoundaryMode::SelectObjects) {
            self.set_boundary_set(handles);
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if matches!(self.mode, BoundaryMode::PickInside) {
            self.point_regions.pop().map(|_| CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if !matches!(self.mode, BoundaryMode::PickInside) {
            return None;
        }
        let local = self.plane.to_local(pt);
        let hovered = self.picked_region([local.x, local.y]);
        let mut rings: Vec<&Vec<[f64; 2]>> = Vec::new();
        for ring in self.point_regions.iter().flat_map(|region| region.iter()) {
            if !rings.iter().any(|existing| *existing == ring) {
                rings.push(ring);
            }
        }
        if let Some(region) = &hovered {
            for ring in region {
                if !rings.iter().any(|existing| *existing == ring) {
                    rings.push(ring);
                }
            }
        }
        if rings.is_empty() {
            return None;
        }
        let mut points = Vec::new();
        for ring in rings {
            if !points.is_empty() {
                points.push([f64::NAN; 3]);
            }
            for [x, y] in ring {
                points.push(self.plane.to_world(DVec3::new(*x, *y, 0.0)).to_array());
            }
            if let Some([x, y]) = ring.first() {
                points.push(self.plane.to_world(DVec3::new(*x, *y, 0.0)).to_array());
            }
        }
        Some(WireModel::solid_f64(
            "boundary_preview".into(),
            points,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["BOUNDARY"]
}); // BoundaryCommand
inventory::submit!(crate::command::CommandRegistration {
    names: &["GRADIENT"]
}); // GradientCommand
inventory::submit!(crate::command::CommandRegistration { names: &["HATCH"] }); // HatchCommand

#[cfg(test)]
mod tests {
    use super::*;
    use codec::EntityType;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    // Two nested rectangles, regardless of draw order, the resolution must be
    // deterministic and independent of which was drawn first.
    fn nested(draw_order: bool) -> Vec<Vec<[f64; 2]>> {
        let big = rect(-10.0, -10.0, 10.0, 10.0);
        let small = rect(-5.0, -5.0, 5.0, 5.0);
        if draw_order {
            vec![big, small]
        } else {
            vec![small, big]
        }
    }

    #[test]
    fn click_inside_small_hatches_only_small() {
        for order in [true, false] {
            let rings = resolve_hatch_rings(&nested(order), [0.0, 0.0]).unwrap();
            // Exactly one ring (no hole) and it is the small rectangle.
            assert_eq!(rings.len(), 1, "order {order}");
            assert_eq!(rings[0].len(), 4);
            assert!((rings[0][0][0] - (-5.0)).abs() < 1e-9, "order {order}");
        }
    }

    #[test]
    fn click_between_hatches_ring_with_hole() {
        for order in [true, false] {
            let rings = resolve_hatch_rings(&nested(order), [8.0, 0.0]).unwrap();
            // Outer ring + the small rectangle as a hole.
            assert_eq!(rings.len(), 2, "order {order}");
            // Outer is the big rectangle.
            assert!((rings[0][0][0] - (-10.0)).abs() < 1e-9, "order {order}");
            // Hole is the small rectangle.
            assert!((rings[1][0][0] - (-5.0)).abs() < 1e-9, "order {order}");
        }
    }

    #[test]
    fn click_outside_returns_none() {
        assert!(resolve_hatch_rings(&nested(true), [50.0, 50.0]).is_none());
    }

    #[test]
    fn hatch_pick_finds_region_and_island_among_far_clutter() {
        let mut sources = rustc_hash::FxHashMap::default();
        let mut add_ring = |base: u64, ring: &[[f64; 2]]| {
            for (index, (start, end)) in ring
                .iter()
                .copied()
                .zip(ring.iter().copied().cycle().skip(1))
                .take(ring.len())
                .enumerate()
            {
                let segment = Line { start, end };
                sources.insert(
                    Handle::new(base + index as u64),
                    crate::scene::BoundarySource {
                        segments: vec![segment],
                        curves: vec![Curve::Line(segment)],
                    },
                );
            }
        };
        add_ring(1, &rect(-10.0, -10.0, 10.0, 10.0));
        add_ring(10, &rect(-2.0, -2.0, 2.0, 2.0));
        for index in 0..64 {
            let x = 100.0 + index as f64 * 4.0;
            let segment = Line {
                start: [x, 100.0],
                end: [x + 1.0, 100.0],
            };
            sources.insert(
                Handle::new(100 + index),
                crate::scene::BoundarySource {
                    segments: vec![segment],
                    curves: Vec::new(),
                },
            );
        }

        let mut command = HatchCommand::new(sources, Vec::new(), None, WorkingPlane::default());
        command.on_point(DVec3::new(5.0, 5.0, 0.0));
        assert_eq!(command.point_regions.len(), 1);
        assert_eq!(command.point_regions[0].len(), 2, "outer ring plus island");
    }

    #[test]
    fn three_nested_levels() {
        let a = rect(-30.0, -30.0, 30.0, 30.0);
        let b = rect(-15.0, -15.0, 15.0, 15.0);
        let c = rect(-5.0, -5.0, 5.0, 5.0);
        // Click in the middle ring (between b and c).
        let rings = resolve_hatch_rings(&[a.clone(), b.clone(), c.clone()], [10.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 2, "middle ring fill with inner hole");
        // Click inside the innermost.
        let rings = resolve_hatch_rings(&[a, b, c], [0.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 1, "innermost fill has no hole");
    }

    #[test]
    fn click_outer_band_only_direct_child_is_hole() {
        let a = rect(-30.0, -30.0, 30.0, 30.0);
        let b = rect(-15.0, -15.0, 15.0, 15.0);
        let c = rect(-5.0, -5.0, 5.0, 5.0);
        // Click in the outermost band (between a and b): fill = a with only its
        // direct child b as a hole. The grandchild c must be excluded — adding
        // it would flip the innermost square back on under even-odd fill.
        let rings = resolve_hatch_rings(&[a, b, c], [20.0, 0.0]).unwrap();
        assert_eq!(rings.len(), 2, "outer band = a with b as its only hole");
        assert!((rings[0][0][0] - (-30.0)).abs() < 1e-9, "outer ring is a");
        assert!(
            (rings[1][0][0] - (-15.0)).abs() < 1e-9,
            "hole is direct child b"
        );
    }

    #[test]
    fn boundary_region_keeps_a_selected_hole_in_one_entity() {
        let rings = vec![rect(-10.0, -10.0, 10.0, 10.0), rect(-2.0, -2.0, 2.0, 2.0)];
        let segments: Vec<Line> = rings
            .iter()
            .flat_map(|ring| {
                ring.iter()
                    .copied()
                    .zip(ring.iter().copied().cycle().skip(1))
                    .take(ring.len())
                    .map(|(start, end)| Line { start, end })
            })
            .collect();
        let mut sources = rustc_hash::FxHashMap::default();
        sources.insert(
            Handle::new(1),
            crate::scene::BoundarySource {
                curves: segments.iter().cloned().map(Curve::Line).collect(),
                segments,
            },
        );
        let mut command = BoundaryCommand::new(sources, Vec::new(), WorkingPlane::default(), 0.0);
        command.point_regions = vec![rings];
        command.output_region = true;

        let entities = command.make_entities();
        let [EntityType::Region(region)] = entities.as_slice() else {
            panic!("expected one region");
        };
        assert_eq!(region.wires.len(), 2);
    }
}
