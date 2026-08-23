// DIMBASELINE command — stacked baseline dimensions all measured from the same origin.
//
// Each new point becomes the second extension line origin of a new dimension.
// The first extension line is always the same base origin point.
// Each new dimension line is placed further from the baseline by DIMDLI (increment).
//
// Constructed from commands.rs after finding the last placed linear/aligned dimension.

use acadrust::entities::{Dimension, DimensionLinear};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::{DVec3, Vec3};

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_baseline.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMBASELINE",
        label: "Baseline",
        icon: ICON,
        event: ModuleEvent::Command("DIMBASELINE".to_string()),
    }
}

/// Fallback stacking increment (world units) when no DimStyle is available.
const DEFAULT_DIMDLI: f64 = 1.5;

pub struct DimBaselineCommand {
    /// Fixed first-extension-line origin (never changes).
    base_p1: DVec3,
    /// Direction along the measurement direction (0.0 = horizontal, PI/2 = vertical).
    rotation: f64,
    /// Text reading rotation inherited from the base dim (keeps a UCS-aligned
    /// chain's text consistent with the originating DIMLINEAR). 0 = natural.
    text_rotation: f64,
    /// Unit vector perpendicular to the dimension axis, pointing toward the dim line side.
    perp: DVec3,
    /// Perpendicular distance of the NEXT dimension line from the extension-line axis.
    next_offset: f64,
    /// Stacking increment from the active DimStyle (DIMDLI).
    dimdli: f64,
    /// True once we have a base dimension loaded.
    ready: bool,
}

impl DimBaselineCommand {
    /// No base dim found — cancel immediately.
    pub fn new() -> Self {
        Self {
            base_p1: DVec3::ZERO,
            rotation: 0.0,
            text_rotation: 0.0,
            perp: DVec3::Y,
            next_offset: DEFAULT_DIMDLI,
            dimdli: DEFAULT_DIMDLI,
            ready: false,
        }
    }

    /// Build from the last placed dimension.
    ///
    /// `p1` — first extension line origin (fixed baseline).
    /// `p2` — second extension line origin of the base dim (unused for placement, kept for context).
    /// `definition_point` — dim-line position of the base dim (defines perpendicular side).
    /// `rotation` — 0.0 = horizontal, PI/2 = vertical.
    /// `dimdli` — DimStyle stacking increment (use [`DEFAULT_DIMDLI`] when no style is active).
    pub fn from_base(
        p1: Vec3,
        _p2: Vec3,
        definition_point: Vec3,
        rotation: f64,
        text_rotation: f64,
        dimdli: f32,
    ) -> Self {
        // The base dim's points enter as f32 from the caller; promote to f64 so
        // all committed-coordinate math downstream stays exact.
        let p1 = p1.as_dvec3();
        let definition_point = definition_point.as_dvec3();
        // Measurement axis = the base dim's rotation angle (any angle, incl. a
        // UCS-aligned one), not a world horizontal/vertical.
        let axis = DVec3::new(rotation.cos(), rotation.sin(), 0.0);
        let perp = DVec3::new(-axis.y, axis.x, 0.0);
        let base_offset = (definition_point - p1).dot(perp);
        let dimdli = dimdli as f64;
        let dimdli = if dimdli.abs() < 1e-6 {
            DEFAULT_DIMDLI
        } else {
            dimdli.abs()
        };
        // Stack each baseline outward — in the SIGN direction of the base dim's
        // own offset. When the base dim sits on the -perp side (below / left of
        // the points) a positive increment would march the stack back toward and
        // across the points; the signed increment keeps it moving away. (#181)
        let dir = if base_offset >= 0.0 { 1.0 } else { -1.0 };
        let dimdli = dimdli * dir;
        let next_offset = base_offset + dimdli;
        Self {
            base_p1: p1,
            rotation,
            text_rotation,
            perp,
            next_offset,
            dimdli,
            ready: true,
        }
    }
}

impl DimBaselineCommand {
    /// The two dimension-line endpoints for the new baseline at `p2`: the fixed
    /// base origin and `p2` each projected INDEPENDENTLY onto the dim line (at
    /// the current stacking offset). Projecting both keeps the line straight
    /// even when the two origins aren't level — a shared offset tilts it. (#181)
    fn dim_line_pts(&self, p2: DVec3) -> (DVec3, DVec3) {
        let target = self.base_p1.dot(self.perp) + self.next_offset;
        let d1 = self.base_p1 + self.perp * (target - self.base_p1.dot(self.perp));
        let d2 = p2 + self.perp * (target - p2.dot(self.perp));
        (d1, d2)
    }
}

impl CadCommand for DimBaselineCommand {
    fn name(&self) -> &'static str {
        "DIMBASELINE"
    }

    fn prompt(&self) -> String {
        if !self.ready {
            t!("DIMBASELINE  No base dimension found. Place a dimension first.").into_owned()
        } else {
            t!("DIMBASELINE  Specify a second extension line origin (Enter to exit):").into_owned()
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if !self.ready {
            return CmdResult::Cancel;
        }
        let p1 = self.base_p1;
        let p2 = pt;

        // Build a new linear dimension.
        let mut dim = DimensionLinear::new(v3(p1), v3(p2));
        dim.rotation = self.rotation;
        if self.text_rotation.abs() > 1e-9 {
            dim.base.text_rotation = self.text_rotation;
        }

        let (dim_line_pt, dim_line_pt2) = self.dim_line_pts(p2);
        dim.definition_point = v3(dim_line_pt);
        dim.base.definition_point = v3(dim_line_pt);
        dim.base.text_middle_point = v3((dim_line_pt + dim_line_pt2) * 0.5);
        dim.base.insertion_point = dim.base.text_middle_point;
        dim.base.actual_measurement = dim.measurement();

        // Stack the next dim line further out.
        self.next_offset += self.dimdli;

        CmdResult::CommitEntity(EntityType::Dimension(Dimension::Linear(dim)))
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if !self.ready {
            return None;
        }
        // dim_line_pts stays in f64; downcast only here, into the preview's
        // [f32;3] GPU buffer — correct pixel-space, never touches committed geometry.
        let (dim_line_pt, dim_line_pt2) = self.dim_line_pts(pt);
        let p1 = self.base_p1.as_vec3();
        let dim_line_pt = dim_line_pt.as_vec3();
        let dim_line_pt2 = dim_line_pt2.as_vec3();
        let pt = pt.as_vec3();
        Some(WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: "dimbase_preview".into(),
            points: vec![
                [p1.x, p1.y, p1.z],
                [dim_line_pt.x, dim_line_pt.y, dim_line_pt.z],
                [f32::NAN, 0.0, 0.0],
                [pt.x, pt.y, pt.z],
                [dim_line_pt2.x, dim_line_pt2.y, dim_line_pt2.z],
                [f32::NAN, 0.0, 0.0],
                [dim_line_pt.x, dim_line_pt.y, dim_line_pt.z],
                [dim_line_pt2.x, dim_line_pt2.y, dim_line_pt2.z],
            ],
            points_low: Vec::new(),
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![],
            tangent_geoms: vec![],
            aci: 0,
            key_vertices: vec![],
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        })
    }
}

fn v3(p: DVec3) -> Vector3 {
    Vector3::new(p.x, p.y, p.z)
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["DIMBASELINE"] });  // DimBaselineCommand
