// Array dropdown — ribbon definition + interactive commands.
//
// ARRAYRECT (AR):
//   Rectangular array: row/column counts and spacing collected via text input.
//   1. Row count → 2. Column count → 3. Row spacing → 4. Column spacing
//   → Returns BatchCopy with a grid of Translate transforms.
//
// ARRAYPATH:
//   Path array: copies placed at equal intervals along a curve/line.
//   (pending geometry engine support — stub)
//
// ARRAYPOLAR:
//   Polar array: copies rotated around a center point by a total angle.
//   1. Center point → 2. Item count (text) → 3. Total angle in degrees (text)

use acadrust::Handle;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, EntityTransform, WorkingPlane};
use crate::modules::draw::defaults;
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;

// ── Dropdown constants ─────────────────────────────────────────────────────

pub const DROPDOWN_ID: &str = "array_type";
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../../assets/icons/array_rect.svg"));

pub const DROPDOWN_ITEMS: &[(&str, &str, IconKind)] = &[
    (
        "ARRAYRECT",
        "Rectangular Array",
        IconKind::Svg(include_bytes!("../../../../assets/icons/array_rect.svg")),
    ),
    (
        "ARRAYPATH",
        "Path Array",
        IconKind::Svg(include_bytes!("../../../../assets/icons/array_path.svg")),
    ),
    (
        "ARRAYPOLAR",
        "Polar Array",
        IconKind::Svg(include_bytes!("../../../../assets/icons/array_polar.svg")),
    ),
];

// ── Item limit ─────────────────────────────────────────────────────────────

/// Most items one array may create, including the source. Every item becomes
/// a transform and then a copy of each selected object, so an unbounded typed
/// count exhausts memory before anything is drawn.
pub(crate) const MAX_ARRAY_ITEMS: u64 = 100_000;

/// Whether an array of `counts` (rows, columns, levels, ...) stays in bounds.
pub(crate) fn array_items_within_limit(counts: &[u64]) -> bool {
    counts
        .iter()
        .try_fold(1u64, |total, &count| total.checked_mul(count))
        .is_some_and(|total| total <= MAX_ARRAY_ITEMS)
}

pub(crate) fn array_limit_error() -> CmdResult {
    CmdResult::ReportError(
        crate::tf!("Array size exceeds the limit of {} items.", MAX_ARRAY_ITEMS).into_owned(),
    )
}

// ── Rectangular Array ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum RectStep {
    Rows,
    Cols { rows: u32 },
    RowSp { rows: u32, cols: u32 },
    ColSp { rows: u32, cols: u32, row_sp: f64 },
}

pub struct ArrayRectCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    step: RectStep,
    default_rows: u32,
    default_cols: u32,
    default_row_sp: f64,
    default_col_sp: f64,
    plane: WorkingPlane,
}

impl ArrayRectCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            step: RectStep::Rows,
            default_rows: (defaults::get_array_rows() as u32).clamp(1, MAX_ARRAY_ITEMS as u32),
            default_cols: (defaults::get_array_cols() as u32).clamp(1, MAX_ARRAY_ITEMS as u32),
            default_row_sp: defaults::get_array_row_sp(),
            default_col_sp: defaults::get_array_col_sp(),
            plane: WorkingPlane::default(),
        }
    }

    /// Row/column offsets run along the active coordinate axes.
    fn build_transforms(
        rows: u32,
        cols: u32,
        row_sp: f64,
        col_sp: f64,
        plane: WorkingPlane,
    ) -> Vec<EntityTransform> {
        let mut t = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                if r == 0 && c == 0 {
                    continue;
                }
                t.push(EntityTransform::Translate(plane.vector_to_world(DVec3::new(
                    col_sp * c as f64,
                    row_sp * r as f64,
                    0.0,
                ))));
            }
        }
        t
    }
}

impl CadCommand for ArrayRectCommand {
    fn name(&self) -> &'static str {
        "ARRAYRECT"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        match self.step {
            RectStep::Rows => {
                crate::tf!("ARRAYRECT  Enter row count <{}>:", self.default_rows).into_owned()
            }
            RectStep::Cols { rows } => crate::tf!(
                "ARRAYRECT  Enter column count <{}>  [{rows} rows]:",
                self.default_cols
            )
            .into_owned(),
            RectStep::RowSp { rows, cols } => crate::tf!(
                "ARRAYRECT  Row spacing <{:.0}>  [{rows}×{cols}]:",
                self.default_row_sp
            )
            .into_owned(),
            RectStep::ColSp { rows, cols, row_sp } => crate::tf!(
                "ARRAYRECT  Column spacing <{:.0}>  [{rows}×{cols}, row={row_sp:.0}]:",
                self.default_col_sp
            )
            .into_owned(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn dyn_field(&self) -> crate::command::DynField {
        crate::command::DynField::Scalar
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim().replace(',', ".");
        let t = t.as_str();
        match self.step {
            RectStep::Rows => {
                let rows = if t.is_empty() {
                    self.default_rows
                } else {
                    let v = t.parse::<u32>().unwrap_or(self.default_rows).max(1);
                    if !array_items_within_limit(&[v as u64]) {
                        return Some(array_limit_error());
                    }
                    defaults::set_array_rows(v as f64);
                    self.default_rows = v;
                    v
                };
                self.step = RectStep::Cols { rows };
                None
            }
            RectStep::Cols { rows } => {
                let cols = if t.is_empty() {
                    self.default_cols
                } else {
                    t.parse::<u32>().unwrap_or(self.default_cols).max(1)
                };
                if !array_items_within_limit(&[rows as u64, cols as u64]) {
                    return Some(array_limit_error());
                }
                if !t.is_empty() {
                    defaults::set_array_cols(cols as f64);
                    self.default_cols = cols;
                }
                self.step = RectStep::RowSp { rows, cols };
                None
            }
            RectStep::RowSp { rows, cols } => {
                let row_sp = if t.is_empty() {
                    self.default_row_sp
                } else {
                    let v = crate::entities::common::parse_typed_length(t)
                        .unwrap_or(self.default_row_sp);
                    defaults::set_array_row_sp(v);
                    self.default_row_sp = v;
                    v
                };
                self.step = RectStep::ColSp { rows, cols, row_sp };
                None
            }
            RectStep::ColSp { rows, cols, row_sp } => {
                let col_sp = if t.is_empty() {
                    self.default_col_sp
                } else {
                    let v = crate::entities::common::parse_typed_length(t)
                        .unwrap_or(self.default_col_sp);
                    defaults::set_array_col_sp(v);
                    v
                };
                Some(CmdResult::BatchCopy(
                    self.handles.clone(),
                    Self::build_transforms(rows, cols, row_sp, col_sp, self.plane),
                ))
            }
        }
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        let (rows, cols, row_sp, col_sp) = match self.step {
            RectStep::Rows => (
                self.default_rows,
                self.default_cols,
                self.default_row_sp,
                self.default_col_sp,
            ),
            RectStep::Cols { rows } => (
                rows,
                self.default_cols,
                self.default_row_sp,
                self.default_col_sp,
            ),
            RectStep::RowSp { rows, cols } => {
                (rows, cols, self.default_row_sp, self.default_col_sp)
            }
            RectStep::ColSp { rows, cols, row_sp } => (rows, cols, row_sp, self.default_col_sp),
        };
        // Defaults are only bounded one count at a time.
        if !array_items_within_limit(&[rows as u64, cols as u64]) {
            return vec![];
        }
        Self::build_transforms(rows, cols, row_sp, col_sp, self.plane)
            .iter()
            .flat_map(|t| {
                if let EntityTransform::Translate(delta) = t {
                    self.wire_models
                        .iter()
                        .map(|w| w.translated(delta.as_vec3()))
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                }
            })
            .collect()
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        // Enter with empty input = use default for current step
        self.on_text_input("").map_or(CmdResult::NeedPoint, |r| r)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Polar Array ────────────────────────────────────────────────────────────

enum PolarStep {
    Center,
    Count { center: DVec3 },
    Angle { center: DVec3, count: u32 },
}

pub struct ArrayPolarCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    step: PolarStep,
    default_count: u32,
    default_angle: f64,
    plane: WorkingPlane,
}

impl ArrayPolarCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            step: PolarStep::Center,
            default_count: (defaults::get_array_p_count() as u32).clamp(2, MAX_ARRAY_ITEMS as u32),
            default_angle: defaults::get_array_p_angle(),
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for ArrayPolarCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "ARRAYPOLAR"
    }

    fn prompt(&self) -> String {
        match &self.step {
            PolarStep::Center => crate::tf!(
                "ARRAYPOLAR  Specify center point  [{} objects]:",
                self.handles.len()
            )
            .into_owned(),
            PolarStep::Count { .. } => {
                crate::tf!("ARRAYPOLAR  Enter item count <{}>:", self.default_count).into_owned()
            }
            PolarStep::Angle { count, .. } => crate::tf!(
                "ARRAYPOLAR  Enter total angle in degrees <{:.0}>  [{count} items]:",
                self.default_angle
            )
            .into_owned(),
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, PolarStep::Count { .. } | PolarStep::Angle { .. })
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.step, PolarStep::Count { .. } | PolarStep::Angle { .. }) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim().replace(',', ".");
        let t = t.as_str();
        match &self.step {
            PolarStep::Count { center } => {
                let center = *center;
                let count = if t.is_empty() {
                    self.default_count
                } else {
                    let v = t.parse::<u32>().unwrap_or(self.default_count).max(2);
                    if !array_items_within_limit(&[v as u64]) {
                        return Some(array_limit_error());
                    }
                    defaults::set_array_p_count(v as f64);
                    self.default_count = v;
                    v
                };
                self.step = PolarStep::Angle { center, count };
                None
            }
            PolarStep::Angle { center, count } => {
                let center = *center;
                let count = *count;
                let total_deg = if t.is_empty() {
                    self.default_angle
                } else {
                    // Held in degrees, which is what the stored default is.
                    let v = crate::entities::common::parse_typed_angle(t)
                        .map(f64::to_degrees)
                        .unwrap_or(self.default_angle);
                    defaults::set_array_p_angle(v);
                    v
                };
                let step_rad = total_deg.to_radians() / count as f64;
                let transforms = (1..count)
                    .map(|n| EntityTransform::Rotate {
                        center,
                        axis: self.plane.z,
                        angle_rad: step_rad * n as f64,
                    })
                    .collect();
                Some(CmdResult::BatchCopy(self.handles.clone(), transforms))
            }
            _ => None,
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        let pt = pt.as_vec3();
        let (center, count, total_deg) = match &self.step {
            PolarStep::Center => (pt, self.default_count, self.default_angle),
            PolarStep::Count { center } => {
                (center.as_vec3(), self.default_count, self.default_angle)
            }
            PolarStep::Angle { center, count } => {
                (center.as_vec3(), *count, self.default_angle)
            }
        };
        let step_rad = total_deg.to_radians() / count as f64;
        let axis = self.plane.z.as_vec3();
        let mut out: Vec<WireModel> = (1..count)
            .flat_map(|n| {
                let angle_rad = (step_rad * n as f64) as f32;
                self.wire_models
                    .iter()
                    .map(move |wire| wire.rotated_about_axis(center, axis, angle_rad))
            })
            .collect();
        // Rubber-band from center to cursor while picking the center point.
        if matches!(self.step, PolarStep::Center) {
            out.push(WireModel::solid(
                "rubber_band".into(),
                vec![[center.x, center.y, center.z], [pt.x, pt.y, pt.z]],
                WireModel::CYAN,
                false,
            ));
        }
        out
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let PolarStep::Center = self.step {
            self.step = PolarStep::Count { center: pt };
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").map_or(CmdResult::NeedPoint, |r| r)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Path Array ─────────────────────────────────────────────────────────────
//
// ARRAYPATH:
//   Copies selected objects at equal arc-length intervals along a path entity.
//   1. Select path entity (Line, Arc, Circle, LwPolyline)
//   2. Enter item count (total, including the original at the path start)
//   → Returns BatchCopy with Translate transforms derived from path samples.

use acadrust::EntityType;
use crate::entities::curve::entity_curve;
use std::f64::consts::PI as FPI;
use std::f64::consts::TAU as FTAU;

// ── Path geometry helpers ──────────────────────────────────────────────────



// ── State machine ──────────────────────────────────────────────────────────

enum PathStep {
    SelectPath,
    Count { path_entity: EntityType },
}

pub struct ArrayPathCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    all_entities: Vec<EntityType>,
    step: PathStep,
    default_count: u32,
    /// Where the user clicked to select the path. The array starts from the
    /// path end nearest this point, so clicking near either end picks the
    /// travel direction (a stored arc is always CCW regardless of how it was
    /// drawn, so without this the array could run opposite to expectation).
    pick_pt: DVec3,
}

impl ArrayPathCommand {
    pub fn new(
        handles: Vec<Handle>,
        wire_models: Vec<WireModel>,
        all_entities: Vec<EntityType>,
    ) -> Self {
        Self {
            handles,
            wire_models,
            all_entities,
            step: PathStep::SelectPath,
            default_count: (defaults::get_array_path_count() as u32).clamp(2, MAX_ARRAY_ITEMS as u32),
            pick_pt: DVec3::ZERO,
        }
    }

    /// Sample the path and orient the points so index 0 is the end nearest the
    /// pick point, giving the user control over the array's travel direction.
    fn oriented_samples(&self, entity: &EntityType, count: usize) -> Vec<DVec3> {
        let mut pts = Self::sample_path(entity, count);
        if pts.len() >= 2 {
            let d_first = self.pick_pt.distance_squared(pts[0]);
            let d_last = self.pick_pt.distance_squared(pts[pts.len() - 1]);
            if d_last < d_first {
                pts.reverse();
            }
        }
        pts
    }

    /// Sample `count` evenly-spaced points along `entity`.
    /// `count` points spaced evenly **by distance** along the path.
    ///
    /// By distance, not by parameter. The two agree on a line, a circle and a
    /// circular arc, which is how a per-type version got away with confusing
    /// them; on an ellipse or a spline they do not, and copies laid out at
    /// even parameters bunch up wherever the parameter runs slow. The path's
    /// own OCS is honoured too, so an arc carrying a mirrored normal is
    /// walked along the side it is drawn on.
    fn sample_path(entity: &EntityType, count: usize) -> Vec<DVec3> {
        if count == 0 {
            return vec![];
        }
        let Some(curve) = entity_curve(entity) else {
            return vec![DVec3::ZERO; count];
        };
        let total = curve.length();
        if !total.is_finite() || total <= 0.0 {
            return vec![DVec3::ZERO; count];
        }
        // A closed path has no far end to stop short of, so the last copy
        // must not land on top of the first.
        let steps = if curve.is_closed() {
            count as f64
        } else {
            (count - 1).max(1) as f64
        };
        (0..count)
            .map(|i| {
                let p = curve.point_at_distance(total * (i as f64 / steps));
                DVec3::new(p[0], p[1], p[2])
            })
            .collect()
    }

    /// Continuous path-tangent angle (XY) at each sample, via central
    /// differences and unwrapped so the sequence stays smooth across an arc
    /// that turns more than ±π in total.
    fn tangents(pts: &[DVec3]) -> Vec<f64> {
        let n = pts.len();
        let mut a = vec![0.0f64; n];
        for i in 0..n {
            let prev = if i > 0 { pts[i - 1] } else { pts[i] };
            let next = if i + 1 < n { pts[i + 1] } else { pts[i] };
            let d = next - prev;
            a[i] = if d.x.abs() < 1e-12 && d.y.abs() < 1e-12 {
                if i > 0 {
                    a[i - 1]
                } else {
                    0.0
                }
            } else {
                d.y.atan2(d.x)
            };
        }
        // Unwrap to remove ±2π jumps.
        for i in 1..n {
            while a[i] - a[i - 1] > FPI {
                a[i] -= FTAU;
            }
            while a[i] - a[i - 1] <= -FPI {
                a[i] += FTAU;
            }
        }
        a
    }

    /// Build per-copy transforms that array the selection along the path with
    /// the items aligned to the path tangent (the first item keeps its drawn
    /// orientation; each copy rotates by the tangent change at its sample).
    /// On a straight path the tangent never changes, so every copy is a pure
    /// `Translate` — identical to a non-aligned array.
    fn build_transforms(pts: &[DVec3]) -> Vec<EntityTransform> {
        if pts.len() < 2 {
            return vec![];
        }
        let p0 = pts[0];
        let tans = Self::tangents(pts);
        let t0 = tans[0];
        pts.iter()
            .enumerate()
            .skip(1)
            .map(|(i, &p)| {
                let dth = tans[i] - t0;
                if dth.abs() < 1e-5 {
                    EntityTransform::Translate(p - p0)
                } else {
                    // The aligned copy is the rigid motion x' = p + R(x - p0),
                    // which is a pure rotation by `dth` about its fixed point.
                    let center = Self::rigid_center(p0, p, dth);
                    EntityTransform::Rotate {
                        center,
                        axis: DVec3::Z,
                        angle_rad: dth,
                    }
                }
            })
            .collect()
    }

    /// Fixed point of the rigid motion `x' = pi + R(dth)·(x - p0)`; rotating an
    /// entity by `dth` about this point reproduces that motion exactly.
    fn rigid_center(p0: DVec3, pi: DVec3, dth: f64) -> DVec3 {
        let (s, c) = dth.sin_cos();
        // b = pi - R·p0
        let bx = pi.x - (c * p0.x - s * p0.y);
        let by = pi.y - (s * p0.x + c * p0.y);
        // c = (I - R)^{-1} · b,  with (I-R)^{-1} = 1/(2(1-c)) [[1-c, -s],[s, 1-c]]
        let one_c = 1.0 - c;
        let det = 2.0 * one_c;
        DVec3::new(
            (one_c * bx - s * by) / det,
            (s * bx + one_c * by) / det,
            p0.z,
        )
    }
}

impl CadCommand for ArrayPathCommand {
    fn name(&self) -> &'static str {
        "ARRAYPATH"
    }

    fn prompt(&self) -> String {
        match &self.step {
            PathStep::SelectPath => crate::tf!(
                "ARRAYPATH  Select path entity  [{} objects]:",
                self.handles.len()
            )
            .into_owned(),
            PathStep::Count { .. } => {
                crate::tf!("ARRAYPATH  Enter item count <{}>:", self.default_count).into_owned()
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, PathStep::SelectPath)
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, PathStep::Count { .. })
    }

    fn dyn_field(&self) -> crate::command::DynField {
        if matches!(self.step, PathStep::Count { .. }) {
            crate::command::DynField::Scalar
        } else {
            crate::command::DynField::Point
        }
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if handle.is_null() || self.handles.contains(&handle) {
            return CmdResult::NeedPoint;
        }
        if let Some(entity) = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle)
            .cloned()
        {
            self.pick_pt = pt;
            self.step = PathStep::Count {
                path_entity: entity,
            };
        }
        CmdResult::NeedPoint
    }

    fn on_hover_entity(&mut self, handle: Handle, _pt: DVec3) -> Vec<WireModel> {
        if handle.is_null() || self.handles.contains(&handle) {
            return vec![];
        }
        if !matches!(self.step, PathStep::SelectPath) {
            return vec![];
        }
        if let Some(entity) = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle)
        {
            let pts = Self::sample_path(entity, 64);
            if pts.len() >= 2 {
                return vec![WireModel::solid(
                    "arraypath_hover".into(),
                    pts.iter()
                        .map(|p| [p.x as f32, p.y as f32, p.z as f32])
                        .collect(),
                    WireModel::CYAN,
                    false,
                )];
            }
        }
        vec![]
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let PathStep::Count { path_entity } = &self.step else {
            return None;
        };
        let t = text.trim().replace(',', ".");
        let count = if t.is_empty() {
            self.default_count
        } else {
            let v = t.parse::<u32>().unwrap_or(self.default_count).max(2);
            if !array_items_within_limit(&[v as u64]) {
                return Some(array_limit_error());
            }
            defaults::set_array_path_count(v as f64);
            self.default_count = v;
            v
        };
        let pts = self.oriented_samples(path_entity, count as usize);
        let transforms = Self::build_transforms(&pts);
        if transforms.is_empty() {
            return Some(CmdResult::Cancel);
        }
        Some(CmdResult::BatchCopy(self.handles.clone(), transforms))
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        let PathStep::Count { path_entity } = &self.step else {
            return vec![];
        };
        let pts = self.oriented_samples(path_entity, self.default_count as usize);
        let transforms = Self::build_transforms(&pts);
        transforms
            .iter()
            .flat_map(|t| match t {
                EntityTransform::Translate(delta) => self
                    .wire_models
                    .iter()
                    .map(|w| w.translated(delta.as_vec3()))
                    .collect::<Vec<_>>(),
                EntityTransform::Rotate { center, angle_rad, .. } => self
                    .wire_models
                    .iter()
                    .map(|w| w.rotated(center.as_vec3(), *angle_rad as f32))
                    .collect::<Vec<_>>(),
                _ => vec![],
            })
            .collect()
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").map_or(CmdResult::NeedPoint, |r| r)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── 3D Rectangular Array ──────────────────────────────────────────────────

/// ARRAY3D — rectangular array in X (columns), Z (rows in drawing plane), Y (levels up).
/// Prompts: rows → cols → levels → row spacing → col spacing → level spacing
#[derive(Debug, Clone, Copy)]
enum Array3DStep {
    Rows,
    Cols {
        rows: u32,
    },
    Levels {
        rows: u32,
        cols: u32,
    },
    RowSp {
        rows: u32,
        cols: u32,
        levels: u32,
    },
    ColSp {
        rows: u32,
        cols: u32,
        levels: u32,
        row_sp: f64,
    },
    LvlSp {
        rows: u32,
        cols: u32,
        levels: u32,
        row_sp: f64,
        col_sp: f64,
    },
}

pub struct Array3DCommand {
    handles: Vec<Handle>,
    step: Array3DStep,
}

impl Array3DCommand {
    pub fn new(handles: Vec<Handle>) -> Self {
        Self {
            handles,
            step: Array3DStep::Rows,
        }
    }

    fn build_transforms(
        rows: u32,
        cols: u32,
        levels: u32,
        row_sp: f64,
        col_sp: f64,
        lvl_sp: f64,
    ) -> Vec<EntityTransform> {
        let mut t = Vec::new();
        for l in 0..levels {
            for r in 0..rows {
                for c in 0..cols {
                    if l == 0 && r == 0 && c == 0 {
                        continue;
                    }
                    // Drawing plane is world XY: X = col dir, Y = row dir,
                    // Z = level (elevation).
                    t.push(EntityTransform::Translate(DVec3::new(
                        col_sp * c as f64,
                        row_sp * r as f64,
                        lvl_sp * l as f64,
                    )));
                }
            }
        }
        t
    }
}

impl CadCommand for Array3DCommand {
    fn name(&self) -> &'static str {
        "ARRAY3D"
    }

    fn prompt(&self) -> String {
        match self.step {
            Array3DStep::Rows => t!("ARRAY3D  Enter row count:").into_owned(),
            Array3DStep::Cols { rows } => {
                t!("ARRAY3D  Enter column count  [%{rows} rows]:", rows = rows).into_owned()
            }
            Array3DStep::Levels { rows, cols } => t!(
                "ARRAY3D  Enter level count  [%{rows}×%{cols}]:",
                rows = rows,
                cols = cols
            )
            .into_owned(),
            Array3DStep::RowSp { rows, cols, levels } => t!(
                "ARRAY3D  Row spacing  [%{rows}×%{cols}×%{levels}]:",
                rows = rows,
                cols = cols,
                levels = levels
            )
            .into_owned(),
            Array3DStep::ColSp {
                rows,
                cols,
                levels,
                row_sp,
            } => {
                let rs = format!("{:.0}", row_sp);
                t!(
                    "ARRAY3D  Column spacing  [%{rows}×%{cols}×%{levels}, row=%{rs}]:",
                    rows = rows,
                    cols = cols,
                    levels = levels,
                    rs = rs
                )
                .into_owned()
            }
            Array3DStep::LvlSp {
                rows,
                cols,
                levels,
                row_sp,
                col_sp,
            } => {
                let rs = format!("{:.0}", row_sp);
                let cs = format!("{:.0}", col_sp);
                t!(
                    "ARRAY3D  Level spacing  [%{rows}×%{cols}×%{levels}, r=%{rs} c=%{cs}]:",
                    rows = rows,
                    cols = cols,
                    levels = levels,
                    rs = rs,
                    cs = cs
                )
                .into_owned()
            }
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn dyn_field(&self) -> crate::command::DynField {
        crate::command::DynField::Scalar
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim().replace(',', ".");
        let t = t.as_str();
        match self.step {
            Array3DStep::Rows => {
                let v = if t.is_empty() {
                    2
                } else {
                    t.parse::<u32>().unwrap_or(2).max(1)
                };
                if !array_items_within_limit(&[v as u64]) {
                    return Some(array_limit_error());
                }
                self.step = Array3DStep::Cols { rows: v };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::Cols { rows } => {
                let v = if t.is_empty() {
                    2
                } else {
                    t.parse::<u32>().unwrap_or(2).max(1)
                };
                if !array_items_within_limit(&[rows as u64, v as u64]) {
                    return Some(array_limit_error());
                }
                self.step = Array3DStep::Levels { rows, cols: v };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::Levels { rows, cols } => {
                let v = if t.is_empty() {
                    2
                } else {
                    t.parse::<u32>().unwrap_or(2).max(1)
                };
                if !array_items_within_limit(&[rows as u64, cols as u64, v as u64]) {
                    return Some(array_limit_error());
                }
                self.step = Array3DStep::RowSp {
                    rows,
                    cols,
                    levels: v,
                };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::RowSp { rows, cols, levels } => {
                let v: f64 = if t.is_empty() {
                    1.0
                } else {
                    t.parse().unwrap_or(1.0)
                };
                self.step = Array3DStep::ColSp {
                    rows,
                    cols,
                    levels,
                    row_sp: v,
                };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::ColSp {
                rows,
                cols,
                levels,
                row_sp,
            } => {
                let v: f64 = if t.is_empty() {
                    1.0
                } else {
                    t.parse().unwrap_or(1.0)
                };
                self.step = Array3DStep::LvlSp {
                    rows,
                    cols,
                    levels,
                    row_sp,
                    col_sp: v,
                };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::LvlSp {
                rows,
                cols,
                levels,
                row_sp,
                col_sp,
            } => {
                let v: f64 = if t.is_empty() {
                    1.0
                } else {
                    t.parse().unwrap_or(1.0)
                };
                let transforms = Self::build_transforms(rows, cols, levels, row_sp, col_sp, v);
                Some(CmdResult::BatchCopy(self.handles.clone(), transforms))
            }
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").map_or(CmdResult::NeedPoint, |r| r)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_preview_wires(&mut self, _pt: DVec3) -> Vec<WireModel> {
        vec![]
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["ARRAY3D", "3DARRAY"] });

#[cfg(test)]
mod item_limit_tests {
    use super::*;

    #[test]
    fn item_limit_counts_every_dimension_without_overflow() {
        assert!(array_items_within_limit(&[MAX_ARRAY_ITEMS]));
        assert!(!array_items_within_limit(&[MAX_ARRAY_ITEMS + 1]));
        assert!(!array_items_within_limit(&[1_000, 1_000]));
        assert!(!array_items_within_limit(&[u32::MAX as u64, u32::MAX as u64, u32::MAX as u64]));
    }

    #[test]
    fn oversized_counts_are_rejected_at_the_prompt() {
        let mut rect = ArrayRectCommand::new(vec![Handle::new(1)], Vec::new());
        assert!(rect.on_text_input("100000").is_none());
        assert!(matches!(rect.on_text_input("100000"), Some(CmdResult::ReportError(_))));
        assert!(matches!(rect.step, RectStep::Cols { rows: 100_000 }));

        let mut polar = ArrayPolarCommand::new(vec![Handle::new(1)], Vec::new());
        polar.step = PolarStep::Count { center: DVec3::ZERO };
        assert!(matches!(polar.on_text_input("4294967295"), Some(CmdResult::ReportError(_))));

        let mut cube = Array3DCommand::new(vec![Handle::new(1)]);
        assert!(matches!(cube.on_text_input("300"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cube.on_text_input("300"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cube.on_text_input("2"), Some(CmdResult::ReportError(_))));
    }
}
