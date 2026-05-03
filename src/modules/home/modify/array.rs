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
use glam::Vec3;

use crate::command::{CadCommand, CmdResult, EntityTransform};
use crate::modules::home::defaults;
use crate::modules::IconKind;
use crate::scene::wire_model::WireModel;

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

// ── Rectangular Array ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum RectStep {
    Rows,
    Cols { rows: u32 },
    RowSp { rows: u32, cols: u32 },
    ColSp { rows: u32, cols: u32, row_sp: f32 },
}

pub struct ArrayRectCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    step: RectStep,
    default_rows: u32,
    default_cols: u32,
    default_row_sp: f32,
    default_col_sp: f32,
}

impl ArrayRectCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            step: RectStep::Rows,
            default_rows: defaults::get_array_rows() as u32,
            default_cols: defaults::get_array_cols() as u32,
            default_row_sp: defaults::get_array_row_sp(),
            default_col_sp: defaults::get_array_col_sp(),
        }
    }

    fn build_transforms(rows: u32, cols: u32, row_sp: f32, col_sp: f32) -> Vec<EntityTransform> {
        let mut t = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                if r == 0 && c == 0 {
                    continue;
                }
                t.push(EntityTransform::Translate(Vec3::new(
                    col_sp * c as f32,
                    row_sp * r as f32,
                    0.0,
                )));
            }
        }
        t
    }
}

impl CadCommand for ArrayRectCommand {
    fn name(&self) -> &'static str {
        "ARRAYRECT"
    }

    fn prompt(&self) -> String {
        match self.step {
            RectStep::Rows => format!("ARRAYRECT  Enter row count <{}>:", self.default_rows),
            RectStep::Cols { rows } => format!(
                "ARRAYRECT  Enter column count <{}>  [{rows} rows]:",
                self.default_cols
            ),
            RectStep::RowSp { rows, cols } => format!(
                "ARRAYRECT  Row spacing <{:.0}>  [{rows}×{cols}]:",
                self.default_row_sp
            ),
            RectStep::ColSp { rows, cols, row_sp } => format!(
                "ARRAYRECT  Column spacing <{:.0}>  [{rows}×{cols}, row={row_sp:.0}]:",
                self.default_col_sp
            ),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
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
                    defaults::set_array_rows(v as f32);
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
                    let v = t.parse::<u32>().unwrap_or(self.default_cols).max(1);
                    defaults::set_array_cols(v as f32);
                    self.default_cols = v;
                    v
                };
                self.step = RectStep::RowSp { rows, cols };
                None
            }
            RectStep::RowSp { rows, cols } => {
                let row_sp = if t.is_empty() {
                    self.default_row_sp
                } else {
                    let v = t.parse::<f32>().unwrap_or(self.default_row_sp);
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
                    let v = t.parse::<f32>().unwrap_or(self.default_col_sp);
                    defaults::set_array_col_sp(v);
                    v
                };
                Some(CmdResult::BatchCopy(
                    self.handles.clone(),
                    Self::build_transforms(rows, cols, row_sp, col_sp),
                ))
            }
        }
    }

    fn on_preview_wires(&mut self, _pt: Vec3) -> Vec<WireModel> {
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
        Self::build_transforms(rows, cols, row_sp, col_sp)
            .iter()
            .flat_map(|t| {
                if let EntityTransform::Translate(delta) = t {
                    self.wire_models
                        .iter()
                        .map(|w| w.translated(*delta))
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                }
            })
            .collect()
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult {
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
    Count { center: Vec3 },
    Angle { center: Vec3, count: u32 },
}

pub struct ArrayPolarCommand {
    handles: Vec<Handle>,
    wire_models: Vec<WireModel>,
    step: PolarStep,
    default_count: u32,
    default_angle: f32,
}

impl ArrayPolarCommand {
    pub fn new(handles: Vec<Handle>, wire_models: Vec<WireModel>) -> Self {
        Self {
            handles,
            wire_models,
            step: PolarStep::Center,
            default_count: defaults::get_array_p_count() as u32,
            default_angle: defaults::get_array_p_angle(),
        }
    }
}

impl CadCommand for ArrayPolarCommand {
    fn name(&self) -> &'static str {
        "ARRAYPOLAR"
    }

    fn prompt(&self) -> String {
        match &self.step {
            PolarStep::Center => format!(
                "ARRAYPOLAR  Specify center point  [{} objects]:",
                self.handles.len()
            ),
            PolarStep::Count { .. } => {
                format!("ARRAYPOLAR  Enter item count <{}>:", self.default_count)
            }
            PolarStep::Angle { count, .. } => format!(
                "ARRAYPOLAR  Enter total angle in degrees <{:.0}>  [{count} items]:",
                self.default_angle
            ),
        }
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, PolarStep::Count { .. } | PolarStep::Angle { .. })
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
                    defaults::set_array_p_count(v as f32);
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
                    let v = t.parse::<f32>().unwrap_or(self.default_angle);
                    defaults::set_array_p_angle(v);
                    v
                };
                let step_rad = total_deg.to_radians() / count as f32;
                let transforms = (1..count)
                    .map(|n| EntityTransform::Rotate {
                        center,
                        angle_rad: step_rad * n as f32,
                    })
                    .collect();
                Some(CmdResult::BatchCopy(self.handles.clone(), transforms))
            }
            _ => None,
        }
    }

    fn on_preview_wires(&mut self, pt: Vec3) -> Vec<WireModel> {
        let (center, count, total_deg) = match &self.step {
            PolarStep::Center => (pt, self.default_count, self.default_angle),
            PolarStep::Count { center } => (*center, self.default_count, self.default_angle),
            PolarStep::Angle { center, count } => (*center, *count, self.default_angle),
        };
        let step_rad = total_deg.to_radians() / count as f32;
        let mut out: Vec<WireModel> = (1..count)
            .flat_map(|n| {
                let angle_rad = step_rad * n as f32;
                self.wire_models
                    .iter()
                    .map(move |w| w.rotated(center, angle_rad))
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

    fn on_point(&mut self, pt: Vec3) -> CmdResult {
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
use std::f32::consts::TAU as FTAU;

// ── Path geometry helpers ──────────────────────────────────────────────────

/// Tessellate an LwPolyline.
fn lw_dense_pts(p: &acadrust::entities::LwPolyline) -> Vec<Vec3> {
    let z = p.elevation as f32;
    let verts = &p.vertices;
    if verts.is_empty() {
        return vec![];
    }
    let n = verts.len();
    let segs = if p.is_closed { n } else { n.saturating_sub(1) };
    let mut out: Vec<Vec3> = vec![];

    for i in 0..segs {
        let v0 = &verts[i];
        let v1 = &verts[(i + 1) % n];
        let x0 = v0.location.x as f32;
        let y0 = v0.location.y as f32;
        let x1 = v1.location.x as f32;
        let y1 = v1.location.y as f32;

        if out.is_empty() {
            out.push(Vec3::new(x0, y0, z));
        }

        let bulge = v0.bulge as f32;
        if bulge.abs() < 1e-9 {
            out.push(Vec3::new(x1, y1, z));
        } else {
            let dx = x1 - x0;
            let dy = y1 - y0;
            let d = (dx * dx + dy * dy).sqrt();
            if d < 1e-9 {
                out.push(Vec3::new(x1, y1, z));
                continue;
            }
            let angle = 4.0 * bulge.atan();
            let r = (d * 0.5) / (angle * 0.5).sin().abs();
            let mx = (x0 + x1) * 0.5;
            let my = (y0 + y1) * 0.5;
            let inv = 1.0 / d;
            let (px, py) = (-dy * inv, dx * inv);
            let sign = if bulge > 0.0 { 1.0f32 } else { -1.0 };
            let h_val = r - (r * r - d * d * 0.25).max(0.0).sqrt();
            let (cx, cy) = (mx - sign * px * (r - h_val), my - sign * py * (r - h_val));
            let a0 = (y0 - cy).atan2(x0 - cx);
            let a1 = (y1 - cy).atan2(x1 - cx);
            // Signed span: CCW for bulge > 0, CW (negative) for bulge < 0.
            let span = if bulge > 0.0 {
                let s = a1 - a0;
                if s <= 0.0 {
                    s + FTAU
                } else {
                    s
                }
            } else {
                let s = a1 - a0;
                if s >= 0.0 {
                    s - FTAU
                } else {
                    s
                }
            };
            let steps = ((r * span.abs() / 2.0).ceil() as usize).clamp(4, 64);
            for j in 1..=steps {
                let ang = a0 + span * (j as f32 / steps as f32);
                out.push(Vec3::new(cx + r * ang.cos(), cy + r * ang.sin(), z));
            }
        }
    }
    out
}

/// Walk `pts` (ordered) and return `count` points at equal arc-length spacing.
fn subsample_equidistant(pts: &[Vec3], count: usize) -> Vec<Vec3> {
    if count == 0 {
        return vec![];
    }
    if pts.len() < 2 {
        return vec![pts.first().copied().unwrap_or(Vec3::ZERO); count];
    }

    let mut cum = vec![0.0f32; pts.len()];
    for i in 1..pts.len() {
        cum[i] = cum[i - 1] + pts[i].distance(pts[i - 1]);
    }
    let total = *cum.last().unwrap();
    if total < 1e-9 {
        return vec![pts[0]; count];
    }

    let mut out = Vec::with_capacity(count);
    let mut seg = 0usize;
    for i in 0..count {
        let target = if count > 1 {
            total * i as f32 / (count - 1) as f32
        } else {
            0.0
        };
        while seg + 2 < pts.len() && cum[seg + 1] < target - 1e-9 {
            seg += 1;
        }
        let seg_len = cum[seg + 1] - cum[seg];
        let t = if seg_len > 1e-9 {
            (target - cum[seg]) / seg_len
        } else {
            0.0
        };
        out.push(pts[seg].lerp(pts[seg + 1], t.clamp(0.0, 1.0)));
    }
    out
}

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
            default_count: defaults::get_array_path_count() as u32,
        }
    }

    /// Sample `count` evenly-spaced points along `entity`.
    fn sample_path(entity: &EntityType, count: usize) -> Vec<Vec3> {
        if count == 0 {
            return vec![];
        }
        let fn_norm = |x: f32| -> f32 { ((x % FTAU) + FTAU) % FTAU };
        match entity {
            EntityType::Line(l) => {
                let p0 = Vec3::new(l.start.x as f32, l.start.y as f32, 0.0);
                let p1 = Vec3::new(l.end.x as f32, l.end.y as f32, 0.0);
                let d = (count - 1).max(1) as f32;
                (0..count).map(|i| p0.lerp(p1, i as f32 / d)).collect()
            }
            EntityType::Arc(a) => {
                let (cx, cy, r) = (a.center.x as f32, a.center.y as f32, a.radius as f32);
                let a0 = a.start_angle as f32;
                let a1 = a.end_angle as f32;
                let span = {
                    let s = fn_norm(a1) - fn_norm(a0);
                    if s <= 0.0 {
                        s + FTAU
                    } else {
                        s
                    }
                };
                let d = (count - 1).max(1) as f32;
                (0..count)
                    .map(|i| {
                        let ang = fn_norm(a0) + span * (i as f32 / d);
                        Vec3::new(cx + r * ang.cos(), cy + r * ang.sin(), 0.0)
                    })
                    .collect()
            }
            EntityType::Circle(c) => {
                let (cx, cy, r) = (c.center.x as f32, c.center.y as f32, c.radius as f32);
                (0..count)
                    .map(|i| {
                        let ang = i as f32 / count as f32 * FTAU;
                        Vec3::new(cx + r * ang.cos(), cy + r * ang.sin(), 0.0)
                    })
                    .collect()
            }
            EntityType::LwPolyline(p) => subsample_equidistant(&lw_dense_pts(p), count),
            _ => vec![Vec3::ZERO; count],
        }
    }

    /// Build Translate transforms: delta from pts[0] to each subsequent point.
    fn build_transforms(pts: &[Vec3]) -> Vec<EntityTransform> {
        if pts.len() < 2 {
            return vec![];
        }
        let origin = pts[0];
        pts[1..]
            .iter()
            .map(|&p| EntityTransform::Translate(p - origin))
            .collect()
    }
}

impl CadCommand for ArrayPathCommand {
    fn name(&self) -> &'static str {
        "ARRAYPATH"
    }

    fn prompt(&self) -> String {
        match &self.step {
            PathStep::SelectPath => format!(
                "ARRAYPATH  Select path entity  [{} objects]:",
                self.handles.len()
            ),
            PathStep::Count { .. } => {
                format!("ARRAYPATH  Enter item count <{}>:", self.default_count)
            }
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, PathStep::SelectPath)
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, PathStep::Count { .. })
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: Vec3) -> CmdResult {
        if handle.is_null() || self.handles.contains(&handle) {
            return CmdResult::NeedPoint;
        }
        if let Some(entity) = self
            .all_entities
            .iter()
            .find(|e| e.common().handle == handle)
            .cloned()
        {
            self.step = PathStep::Count {
                path_entity: entity,
            };
        }
        CmdResult::NeedPoint
    }

    fn on_hover_entity(&mut self, handle: Handle, _pt: Vec3) -> Vec<WireModel> {
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
                    pts.iter().map(|p| [p.x, p.y, p.z]).collect(),
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
            defaults::set_array_path_count(v as f32);
            self.default_count = v;
            v
        };
        let pts = Self::sample_path(path_entity, count as usize);
        let transforms = Self::build_transforms(&pts);
        if transforms.is_empty() {
            return Some(CmdResult::Cancel);
        }
        Some(CmdResult::BatchCopy(self.handles.clone(), transforms))
    }

    fn on_preview_wires(&mut self, _pt: Vec3) -> Vec<WireModel> {
        let PathStep::Count { path_entity } = &self.step else {
            return vec![];
        };
        let pts = Self::sample_path(path_entity, self.default_count as usize);
        let transforms = Self::build_transforms(&pts);
        transforms
            .iter()
            .flat_map(|t| {
                if let EntityTransform::Translate(delta) = t {
                    self.wire_models
                        .iter()
                        .map(|w| w.translated(*delta))
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                }
            })
            .collect()
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult {
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
    Cols { rows: u32 },
    Levels { rows: u32, cols: u32 },
    RowSp { rows: u32, cols: u32, levels: u32 },
    ColSp { rows: u32, cols: u32, levels: u32, row_sp: f32 },
    LvlSp { rows: u32, cols: u32, levels: u32, row_sp: f32, col_sp: f32 },
}

pub struct Array3DCommand {
    handles: Vec<Handle>,
    step: Array3DStep,
}

impl Array3DCommand {
    pub fn new(handles: Vec<Handle>) -> Self {
        Self { handles, step: Array3DStep::Rows }
    }

    fn build_transforms(
        rows: u32, cols: u32, levels: u32,
        row_sp: f32, col_sp: f32, lvl_sp: f32,
    ) -> Vec<EntityTransform> {
        let mut t = Vec::new();
        for l in 0..levels {
            for r in 0..rows {
                for c in 0..cols {
                    if l == 0 && r == 0 && c == 0 { continue; }
                    // World: X=col dir, Z=row dir (DXF Y), Y=level (up)
                    t.push(EntityTransform::Translate(Vec3::new(
                        col_sp * c as f32,
                        lvl_sp * l as f32,
                        row_sp * r as f32,
                    )));
                }
            }
        }
        t
    }
}

impl CadCommand for Array3DCommand {
    fn name(&self) -> &'static str { "ARRAY3D" }

    fn prompt(&self) -> String {
        match self.step {
            Array3DStep::Rows => "ARRAY3D  Enter row count:".into(),
            Array3DStep::Cols { rows } => format!("ARRAY3D  Enter column count  [{rows} rows]:"),
            Array3DStep::Levels { rows, cols } => format!("ARRAY3D  Enter level count  [{rows}×{cols}]:"),
            Array3DStep::RowSp { rows, cols, levels } => format!("ARRAY3D  Row spacing  [{rows}×{cols}×{levels}]:"),
            Array3DStep::ColSp { rows, cols, levels, row_sp } => format!("ARRAY3D  Column spacing  [{rows}×{cols}×{levels}, row={row_sp:.0}]:"),
            Array3DStep::LvlSp { rows, cols, levels, row_sp, col_sp } => format!("ARRAY3D  Level spacing  [{rows}×{cols}×{levels}, r={row_sp:.0} c={col_sp:.0}]:"),
        }
    }

    fn wants_text_input(&self) -> bool { true }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim().replace(',', ".");
        let t = t.as_str();
        match self.step {
            Array3DStep::Rows => {
                let v = if t.is_empty() { 2 } else { t.parse::<u32>().unwrap_or(2).max(1) };
                self.step = Array3DStep::Cols { rows: v };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::Cols { rows } => {
                let v = if t.is_empty() { 2 } else { t.parse::<u32>().unwrap_or(2).max(1) };
                self.step = Array3DStep::Levels { rows, cols: v };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::Levels { rows, cols } => {
                let v = if t.is_empty() { 2 } else { t.parse::<u32>().unwrap_or(2).max(1) };
                self.step = Array3DStep::RowSp { rows, cols, levels: v };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::RowSp { rows, cols, levels } => {
                let v: f32 = if t.is_empty() { 1.0 } else { t.parse().unwrap_or(1.0) };
                self.step = Array3DStep::ColSp { rows, cols, levels, row_sp: v };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::ColSp { rows, cols, levels, row_sp } => {
                let v: f32 = if t.is_empty() { 1.0 } else { t.parse().unwrap_or(1.0) };
                self.step = Array3DStep::LvlSp { rows, cols, levels, row_sp, col_sp: v };
                Some(CmdResult::NeedPoint)
            }
            Array3DStep::LvlSp { rows, cols, levels, row_sp, col_sp } => {
                let v: f32 = if t.is_empty() { 1.0 } else { t.parse().unwrap_or(1.0) };
                let transforms = Self::build_transforms(rows, cols, levels, row_sp, col_sp, v);
                Some(CmdResult::BatchCopy(self.handles.clone(), transforms))
            }
        }
    }

    fn on_point(&mut self, _pt: Vec3) -> CmdResult { CmdResult::NeedPoint }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").map_or(CmdResult::NeedPoint, |r| r)
    }

    fn on_escape(&mut self) -> CmdResult { CmdResult::Cancel }

    fn on_preview_wires(&mut self, _pt: Vec3) -> Vec<WireModel> { vec![] }
}
