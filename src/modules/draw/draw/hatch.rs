// Hatch, gradient, and boundary commands.

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::IconKind;
use crate::scene::model::hatch_model::{HatchModel, HatchPattern, PatFamily};
use crate::scene::model::wire_model::WireModel;
use acadrust::Handle;
use cadkernel::geom2d::{
    bounded_faces, contains, ring_nesting_depths, signed_area, Circle, Curve, Line, Tolerance,
};
use glam::DVec3;
use crate::t;

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
fn resolve_hatch_rings(
    outlines: &[Vec<[f64; 2]>],
    p: [f64; 2],
) -> Option<Vec<Vec<[f64; 2]>>> {
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

pub struct HatchCommand {
    outlines: Vec<Vec<[f64; 2]>>,
    boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    point_regions: Vec<Vec<Vec<[f64; 2]>>>,
    object_regions: Vec<Vec<Vec<[f64; 2]>>>,
    selected_objects: Vec<Handle>,
    mode: HatchMode,
    manual_pts: Vec<DVec3>,
    manual_bulges: Vec<f64>,
    manual_arc_mode: bool,
    manual_arc_midpoint: Option<DVec3>,
    missed: bool,
    retain_boundaries: bool,
    pattern_override: Option<(String, HatchPattern)>,
    angle_override: Option<f32>,
    scale_override: Option<f32>,
    associative: bool,
    separate_hatches: bool,
    island_style: acadrust::entities::HatchStyleType,
    inherited: Option<(
        HatchModel,
        acadrust::types::Color,
        acadrust::types::Transparency,
    )>,
    plane: WorkingPlane,
}

impl HatchCommand {
    pub fn new(
        outlines: Vec<Vec<[f64; 2]>>,
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
        selected_objects: Vec<Handle>,
        inherited: Option<(
            HatchModel,
            acadrust::types::Color,
            acadrust::types::Transparency,
        )>,
        plane: WorkingPlane,
    ) -> Self {
        let selected_objects: Vec<_> = selected_objects
            .into_iter()
            .filter(|handle| boundary_sources.contains_key(handle))
            .collect();
        let has_selection = !selected_objects.is_empty();
        let mut command = Self {
            outlines,
            boundary_sources,
            point_regions: Vec::new(),
            object_regions: Vec::new(),
            selected_objects: Vec::new(),
            mode: if has_selection {
                HatchMode::SelectObjects
            } else {
                HatchMode::PickInside
            },
            manual_pts: vec![],
            manual_bulges: vec![],
            manual_arc_mode: false,
            manual_arc_midpoint: None,
            missed: false,
            retain_boundaries: false,
            pattern_override: None,
            angle_override: None,
            scale_override: None,
            associative: true,
            separate_hatches: false,
            island_style: inherited
                .as_ref()
                .map(|(model, _, _)| model.style)
                .unwrap_or(acadrust::entities::HatchStyleType::Normal),
            inherited,
            plane,
        };
        command.set_object_selection(selected_objects);
        command
    }

    fn set_object_selection(&mut self, handles: Vec<Handle>) {
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
        self.missed = !handles.is_empty() && self.object_regions.is_empty();
        self.selected_objects = handles;
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
            acadrust::entities::HatchStyleType::Normal => "Normal",
            acadrust::entities::HatchStyleType::Outer => "Outer",
            acadrust::entities::HatchStyleType::Ignore => "Ignore",
        }
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
        let exterior: Vec<bool> = cadkernel::geom2d::ring_nesting_depths(&rings)
            .into_iter()
            .map(|depth| depth == 0)
            .collect();
        let mut boundary_sources: Vec<Vec<Handle>> = rings
            .iter()
            .map(|ring| crate::scene::ring_source_handles(ring, &self.boundary_sources))
            .collect();
        let mut boundary_paths = crate::scene::exact_hatch_paths(
            &rings,
            &exterior,
            &self.boundary_sources,
            1.0e-6,
        );
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
                        + (family.x0 as f64 * cos as f64
                            - family.y0 as f64 * sin as f64)
                            * scale as f64;
                    let base_y = source.world_origin[1]
                        + (family.x0 as f64 * sin as f64
                            + family.y0 as f64 * cos as f64)
                            * scale as f64;
                    let dx = base_x - origin[0];
                    let dy = base_y - origin[1];
                    family.x0 = ((dx * cos as f64 + dy * sin as f64) / scale as f64) as f32;
                    family.y0 = ((-dx * sin as f64 + dy * cos as f64) / scale as f64) as f32;
                }
            }
            return HatchModel {
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
        let (name, pattern) = self
            .pattern_override
            .clone()
            .unwrap_or_else(|| (pat_name.to_string(), default_pattern));
        HatchModel {
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

    fn manual_boundary_path(&self) -> Option<acadrust::entities::BoundaryPath> {
        use acadrust::entities::{BoundaryEdge, BoundaryPath, PolylineEdge};
        use acadrust::types::Vector3;
        if self.manual_pts.len() < 3 {
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
    let curvature = DVec3::from_array(cadkernel::space::curve::curvature_through(
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
        match &self.mode {
            HatchMode::PickInside => {
                let miss = if self.missed {
                    t!("  ⚠ No closed boundary found.").into_owned()
                } else {
                    String::new()
                };
                t!(
                    "HATCH  Pick internal point (%{count} regions selected; P <pattern> / A <angle> / L <scale>; Enter to apply):%{miss}",
                    count = self.region_count(),
                    miss = miss
                )
                .into_owned()
            }
            HatchMode::SelectObjects => {
                let miss = if self.missed {
                    t!("  ⚠ Selection has no closed boundary.").into_owned()
                } else {
                    String::new()
                };
                t!(
                    "HATCH  Select boundary objects (%{objects} objects, %{count} regions; P <pattern> / A <angle> / L <scale>; Enter to apply):%{miss}",
                    objects = self.selected_objects.len(),
                    count = self.region_count(),
                    miss = miss
                )
                .into_owned()
            }
            HatchMode::Manual => {
                if self.manual_pts.is_empty() {
                    t!("HATCH  Boundary point 1:").into_owned()
                } else {
                    let n = self.manual_pts.len() + 1;
                    t!("HATCH  Point %{n}:", n = n).into_owned()
                }
            }
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        match &self.mode {
            HatchMode::PickInside => {
                let mut options = vec![
                    CmdOption::new(t!("Select objects").as_ref(), "O"),
                    CmdOption::new(t!("Draw manually").as_ref(), "S"),
                    CmdOption::new(
                        if self.retain_boundaries {
                            "Keep boundaries: on"
                        } else {
                            "Keep boundaries: off"
                        },
                        "B",
                    ),
                    CmdOption::new(
                        if self.associative { "Associative: on" } else { "Associative: off" },
                        "N",
                    ),
                    CmdOption::new(
                        if self.separate_hatches { "Separate hatches: on" } else { "Separate hatches: off" },
                        "D",
                    ),
                    CmdOption::new(
                        &format!("Island style: {}", self.island_style_label()),
                        "Y",
                    ),
                ];
                if self.region_count() > 0 {
                    options.push(CmdOption::enter(t!("Accept").as_ref()));
                }
                options
            }
            HatchMode::SelectObjects => {
                let mut options = vec![
                    CmdOption::new(t!("Pick internal points").as_ref(), "I"),
                    CmdOption::new(t!("Draw manually").as_ref(), "S"),
                    CmdOption::new(
                        if self.retain_boundaries {
                            "Keep boundaries: on"
                        } else {
                            "Keep boundaries: off"
                        },
                        "B",
                    ),
                    CmdOption::new(
                        if self.associative { "Associative: on" } else { "Associative: off" },
                        "N",
                    ),
                    CmdOption::new(
                        if self.separate_hatches { "Separate hatches: on" } else { "Separate hatches: off" },
                        "D",
                    ),
                    CmdOption::new(
                        &format!("Island style: {}", self.island_style_label()),
                        "Y",
                    ),
                ];
                if self.region_count() > 0 {
                    options.push(CmdOption::enter(t!("Accept").as_ref()));
                }
                options
            }
            HatchMode::Manual => {
                let mut options = vec![
                    CmdOption::new(
                        if self.manual_arc_mode { "Line" } else { "Arc" },
                        if self.manual_arc_mode { "L" } else { "A" },
                    ),
                ];
                if self.manual_pts.len() >= 3 {
                    options.push(CmdOption::new("Close", "C"));
                    options.push(CmdOption::enter(t!("Accept").as_ref()));
                }
                options
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let pt = self.plane.to_local(pt);
        match &self.mode {
            HatchMode::PickInside => {
                let xy = [pt.x, pt.y];
                match resolve_hatch_rings(&self.outlines, xy) {
                    Some(rings) => {
                        self.missed = false;
                        self.add_point_region(rings);
                        CmdResult::NeedPoint
                    }
                    None => {
                        self.missed = true;
                        CmdResult::NeedPoint
                    }
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
        if matches!(self.mode, HatchMode::Manual) && self.manual_arc_midpoint.is_some() {
            return CmdResult::NeedPoint;
        }
        if matches!(self.mode, HatchMode::Manual) && self.manual_pts.len() >= 3 {
            let ring = self.manual_pts.iter().map(|p| [p.x, p.y]).collect();
            self.add_point_region(vec![ring]);
        }
        let rings = self.combined_rings();
        if rings.is_empty() {
            CmdResult::Cancel
        } else if matches!(self.mode, HatchMode::Manual) {
            let mut hatch = self.make_hatch(rings);
            if let Some(path) = self.manual_boundary_path() {
                hatch.boundary_paths = Some(std::sync::Arc::new(vec![path]));
            }
            if let Some((_, color, transparency)) = &self.inherited {
                CmdResult::CommitStyledHatch {
                    hatch,
                    color: color.clone(),
                    transparency: *transparency,
                }
            } else {
                CmdResult::CommitHatch(hatch)
            }
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

    fn is_selection_gathering(&self) -> bool {
        matches!(self.mode, HatchMode::SelectObjects)
    }

    fn selection_forces_add(&self) -> bool {
        matches!(self.mode, HatchMode::SelectObjects)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if matches!(self.mode, HatchMode::SelectObjects) {
            self.set_object_selection(handles);
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if matches!(self.mode, HatchMode::Manual) {
            if self.manual_arc_midpoint.take().is_some() {
                return Some(CmdResult::NeedPoint);
            }
            if self.manual_pts.pop().is_some() {
                self.manual_bulges.pop();
                return Some(CmdResult::NeedPoint);
            }
        }
        if matches!(self.mode, HatchMode::PickInside) && self.point_regions.pop().is_some() {
            Some(CmdResult::NeedPoint)
        } else {
            None
        }
    }

    fn hatch_preview_models(&self) -> Option<Vec<HatchModel>> {
        let mut rings = self.combined_rings();
        if matches!(self.mode, HatchMode::Manual) && self.manual_pts.len() >= 3 {
            rings.push(self.manual_pts.iter().map(|point| [point.x, point.y]).collect());
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
        if matches!(self.mode, HatchMode::Manual) {
            return match upper.as_str() {
                "A" | "ARC" => {
                    self.manual_arc_mode = true;
                    self.manual_arc_midpoint = None;
                    Some(CmdResult::NeedPoint)
                }
                "L" | "LINE" => {
                    self.manual_arc_mode = false;
                    self.manual_arc_midpoint = None;
                    Some(CmdResult::NeedPoint)
                }
                "C" | "CLOSE" if self.manual_pts.len() >= 3 => Some(self.on_enter()),
                _ => None,
            };
        }
        if upper == "ASSOCIATIVE" {
            self.associative = !self.associative;
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = upper.strip_prefix('P') {
            let name = rest.trim();
            if !name.is_empty() {
                if let Some(entry) = crate::scene::model::hatch_patterns::find(name) {
                    self.pattern_override = Some((entry.name.clone(), entry.gpu.clone()));
                }
            }
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = upper.strip_prefix('A') {
            if let Ok(value) = rest.trim().replace(',', ".").parse::<f32>() {
                self.angle_override = Some(value.to_radians());
            }
            return Some(CmdResult::NeedPoint);
        }
        if let Some(rest) = upper.strip_prefix('L') {
            if let Ok(value) = rest.trim().replace(',', ".").parse::<f32>() {
                if value > 0.0 {
                    self.scale_override = Some(value);
                }
            }
            return Some(CmdResult::NeedPoint);
        }
        match upper.as_str() {
            "O" | "OBJECT" | "OBJECTS" => {
                self.mode = HatchMode::SelectObjects;
                self.missed = false;
                Some(CmdResult::NeedPoint)
            }
            "I" | "INTERNAL" => {
                self.mode = HatchMode::PickInside;
                self.missed = false;
                Some(CmdResult::NeedPoint)
            }
            "S" => {
                self.mode = HatchMode::Manual;
                self.missed = false;
                Some(CmdResult::NeedPoint)
            }
            "B" | "BOUNDARY" | "BOUNDARIES" => {
                self.retain_boundaries = !self.retain_boundaries;
                if self.retain_boundaries {
                    self.separate_hatches = false;
                }
                Some(CmdResult::NeedPoint)
            }
            "N" | "ASSOCIATIVE" => {
                self.associative = !self.associative;
                Some(CmdResult::NeedPoint)
            }
            "D" | "SEPARATE" => {
                self.separate_hatches = !self.separate_hatches;
                if self.separate_hatches {
                    self.retain_boundaries = false;
                }
                Some(CmdResult::NeedPoint)
            }
            "Y" | "ISLAND" => {
                self.island_style = match self.island_style {
                    acadrust::entities::HatchStyleType::Normal => {
                        acadrust::entities::HatchStyleType::Outer
                    }
                    acadrust::entities::HatchStyleType::Outer => {
                        acadrust::entities::HatchStyleType::Ignore
                    }
                    acadrust::entities::HatchStyleType::Ignore => {
                        acadrust::entities::HatchStyleType::Normal
                    }
                };
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> { let pt = pt.as_vec3();
        if let HatchMode::Manual = &self.mode {
            if self.manual_pts.is_empty() {
                return None;
            }
            let mut pts: Vec<[f32; 3]> = self
                .manual_pts
                .iter()
                .map(|&p| self.plane.to_world(p).as_vec3().to_array())
                .collect();
            pts.push(pt.to_array());
            pts.push(self.plane.to_world(self.manual_pts[0]).as_vec3().to_array());
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

// ── GRADIENT command ───────────────────────────────────────────────────────

pub struct GradientCommand {
    outlines: Vec<Vec<[f64; 2]>>,
    boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    mode: Mode,
    manual_pts: Vec<DVec3>,
    missed: bool,
    /// Gradient shape, switchable via the prompt options (#415).
    kind: crate::scene::model::hatch_model::GradientKind,
    /// Swap the two colour stops.
    invert: bool,
}

impl GradientCommand {
    pub fn new(
        outlines: Vec<Vec<[f64; 2]>>,
        boundary_sources: rustc_hash::FxHashMap<Handle, crate::scene::BoundarySource>,
    ) -> Self {
        Self {
            outlines,
            boundary_sources,
            mode: Mode::PickInside,
            manual_pts: vec![],
            missed: false,
            kind: crate::scene::model::hatch_model::GradientKind::Linear,
            invert: false,
        }
    }

    fn make_hatch(&self, rings: Vec<Vec<[f64; 2]>>) -> HatchModel {
        let (rel, origin, wcs) = pack_rings(&rings);
        let exterior: Vec<bool> = cadkernel::geom2d::ring_nesting_depths(&rings)
            .into_iter()
            .map(|depth| depth == 0)
            .collect();
        let boundary_sources = rings
            .iter()
            .map(|ring| crate::scene::ring_source_handles(ring, &self.boundary_sources))
            .collect();
        let boundary_paths = crate::scene::exact_hatch_paths(
            &rings,
            &exterior,
            &self.boundary_sources,
            1.0e-6,
        );
        HatchModel {
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
            style: acadrust::entities::HatchStyleType::Normal,
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
                for (kind, inverted) in
                    crate::scene::model::hatch_model::GradientKind::CHOICES
                {
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
                match resolve_hatch_rings(&self.outlines, xy) {
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

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> { let pt = pt.as_vec3();
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
    outlines: Vec<Vec<[f64; 2]>>,
    point_regions: Vec<Vec<Vec<[f64; 2]>>>,
    selected_objects: Vec<Handle>,
    mode: BoundaryMode,
    island_style: BoundaryIslandStyle,
    gap_tolerance: f64,
    plane: WorkingPlane,
    missed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryMode {
    PickInside,
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
    ) -> Self {
        let outlines = crate::scene::boundary_faces(&sources, 1.0e-6);
        let mut command = Self {
            sources,
            active_sources: rustc_hash::FxHashMap::default(),
            restrict_sources: false,
            outlines,
            point_regions: Vec::new(),
            selected_objects: Vec::new(),
            mode: BoundaryMode::PickInside,
            island_style: BoundaryIslandStyle::Normal,
            gap_tolerance: 1.0e-6,
            plane,
            missed: false,
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
            .filter_map(|handle| self.sources.get(handle).cloned().map(|source| (*handle, source)))
            .collect();
        self.active_sources = selected_sources;
        self.restrict_sources = restrict_sources;
        let sources = if self.restrict_sources {
            &self.active_sources
        } else {
            &self.sources
        };
        self.outlines = crate::scene::boundary_faces(sources, self.gap_tolerance);
        self.missed = !handles.is_empty() && self.outlines.is_empty();
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
        let tolerance = Tolerance::new(self.gap_tolerance);
        let curves = |outline: &Vec<[f64; 2]>| {
            outline
                .iter()
                .copied()
                .zip(outline.iter().copied().cycle().skip(1))
                .take(outline.len())
                .map(|(start, end)| Curve::Line(Line { start, end }))
                .collect::<Vec<_>>()
        };
        let mut containing: Vec<(usize, f64)> = self
            .outlines
            .iter()
            .enumerate()
            .filter(|(_, outline)| contains(&curves(outline), point, tolerance))
            .map(|(index, outline)| (index, signed_area(outline).abs()))
            .collect();
        containing.sort_by(|left, right| left.1.total_cmp(&right.1));
        let outer_index = containing.first()?.0;
        let outer = &self.outlines[outer_index];
        let outer_curves = curves(outer);
        let depths = ring_nesting_depths(&self.outlines);
        let outer_depth = depths.get(outer_index).copied().unwrap_or(0);
        let mut rings = vec![outer.clone()];
        if self.island_style == BoundaryIslandStyle::Ignore {
            return Some(rings);
        }
        for (index, candidate) in self.outlines.iter().enumerate() {
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
            if self.island_style == BoundaryIslandStyle::Outer
                && candidate_depth != outer_depth + 1
            {
                continue;
            }
            rings.push(candidate.clone());
        }
        Some(rings)
    }

    fn add_point_region(&mut self, region: Vec<Vec<[f64; 2]>>) {
        if !self.point_regions.iter().any(|existing| existing == &region) {
            self.point_regions.push(region);
        }
    }

    fn make_entities(&self) -> Vec<acadrust::EntityType> {
        let sources = if self.restrict_sources {
            &self.active_sources
        } else {
            &self.sources
        };
        crate::scene::boundary_polyline_entities(
            &self.point_regions,
            self.plane,
            sources,
            self.gap_tolerance,
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
                t!("BOUNDARY  Pick internal point:%{miss}", miss = miss).into_owned()
            }
            BoundaryMode::SelectObjects => {
                t!("%{cmd}  Select objects:", cmd = self.name()).into_owned()
            }
            BoundaryMode::GapTolerance { .. } => format!(
                "BOUNDARY  {} <{}>:",
                t!("Tolerance"),
                self.gap_tolerance
            ),
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
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
            CmdOption::new(t!("Boundary").as_ref(), "O"),
            CmdOption::new(&island, "S"),
            CmdOption::new(t!("Tolerance").as_ref(), "G"),
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
        if let BoundaryMode::GapTolerance { return_to_selection } = self.mode {
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
        if let BoundaryMode::GapTolerance { return_to_selection } = self.mode {
            if let Ok(value) = text.trim().parse::<f64>() {
                if value.is_finite() && value > 0.0 {
                    self.gap_tolerance = value;
                    self.rebuild_faces();
                }
            }
            self.mode = if return_to_selection {
                BoundaryMode::SelectObjects
            } else {
                BoundaryMode::PickInside
            };
            return Some(CmdResult::NeedPoint);
        }
        match text.trim().to_ascii_uppercase().as_str() {
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
inventory::submit!(crate::command::CommandRegistration { names: &["BOUNDARY"] });  // BoundaryCommand
inventory::submit!(crate::command::CommandRegistration { names: &["GRADIENT"] });  // GradientCommand
inventory::submit!(crate::command::CommandRegistration { names: &["HATCH"] });  // HatchCommand

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!((rings[1][0][0] - (-15.0)).abs() < 1e-9, "hole is direct child b");
    }
}
