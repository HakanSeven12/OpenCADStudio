// Wide-line tool — interactive command.
//
// Command: TRACE — first type a band width (a number; Enter alone keeps the
// default 1.0), then pick a sequence of points. Each picked point extends a
// connected, solid-filled band of the fixed width along the path. Undo (U)
// drops the last picked vertex; Enter / Esc finishes the band.
//
// The result is committed as one lightweight polyline whose constant width is
// set, so the renderer fills it as a continuous solid band. Picking a single
// entity here would not apply — TRACE builds new geometry from picked points,
// so the planar point-pick path is used (every vertex shares the polyline
// elevation, taken from the first point's Z).

use acadrust::entities::{LwPolyline, LwVertex};
use acadrust::types::{Vector2, Vector3};
use acadrust::EntityType;
use crate::t;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

// ── Ribbon definition ─────────────────────────────────────────────────────

#[allow(dead_code)] // ribbon definition ready for wiring; command works via the command line
pub fn tool() -> ToolDef {
    ToolDef {
        id: "TRACE",
        label: "Trace",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("TRACE".to_string()),
    }
}

// ── Command implementation ────────────────────────────────────────────────

pub struct TraceCommand {
    /// Band width. Collected first (typed), defaults to 1.0 until a width step
    /// is completed.
    width: f64,
    /// True until the width has been entered (typed value or bare Enter for the
    /// default). While false the command is in its width-prompt step.
    width_set: bool,
    /// Every point picked so far, in world space. `points[0]` fixes the band's
    /// elevation (its Z).
    points: Vec<DVec3>,
    plane: WorkingPlane,
}

impl TraceCommand {
    pub fn new() -> Self {
        Self {
            width: 1.0,
            width_set: false,
            points: Vec::new(),
            plane: WorkingPlane::default(),
        }
    }

    /// Build the wide lightweight polyline from the collected points, or `None`
    /// when there are too few points to form a segment.
    fn build(&self) -> Option<EntityType> {
        if self.points.len() < 2 {
            return None;
        }
        let local: Vec<DVec3> = self
            .points
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect();
        let elevation = local[0].z;
        let mut pl = LwPolyline::new();
        pl.is_closed = false;
        pl.constant_width = self.width;
        pl.elevation = elevation;
        pl.vertices = local
            .iter()
            .map(|p| {
                let mut v = LwVertex::new(Vector2::new(p.x, p.y));
                v.start_width = self.width;
                v.end_width = self.width;
                v
            })
            .collect();
        pl.normal = Vector3::UNIT_Z;
        Some(self.plane.place_entity(EntityType::LwPolyline(pl)))
    }
}

impl CadCommand for TraceCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "TRACE"
    }

    fn prompt(&self) -> String {
        if !self.width_set {
            let w = format!("{:.4}", self.width);
            return t!("TRACE  Specify trace width <%{w}>:", w = w).into_owned();
        }
        match self.points.len() {
            0 => t!("TRACE  Specify start point:").into_owned(),
            1 => t!("TRACE  Specify next point  [Undo]:").into_owned(),
            _ => t!("TRACE  Specify next point  [Undo]:").into_owned(),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        // A point click before the width is fixed accepts the current default.
        if !self.width_set {
            self.width_set = true;
        }
        self.points.push(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if !self.width_set {
            // Bare Enter at the width step keeps the default and advances.
            self.width_set = true;
            return CmdResult::NeedPoint;
        }
        match self.build() {
            Some(e) => CmdResult::CommitAndExit(e),
            None => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn wants_text_input(&self) -> bool {
        // Width step reads a typed number; the point steps accept the Undo
        // keyword.
        !self.width_set || !self.points.is_empty()
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.width_set && !self.points.is_empty()
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.width_set && !self.points.is_empty() {
            vec![CmdOption::new("Undo", "U")]
        } else {
            Vec::new()
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if !self.width_set {
            if t.is_empty() {
                // Treated as bare Enter: keep the default width.
                self.width_set = true;
                return Some(CmdResult::NeedPoint);
            }
            if let Some(w) = crate::entities::common::parse_typed_length(t) {
                if w > 0.0 {
                    self.width = w;
                    self.width_set = true;
                    return Some(CmdResult::NeedPoint);
                }
            }
            // Unparsable / non-positive: re-prompt without changing state.
            return Some(CmdResult::NeedPoint);
        }
        match t.to_uppercase().as_str() {
            "U" | "UNDO" => {
                self.points.pop();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if !self.width_set || self.points.is_empty() {
            return None;
        }
        // Preview the path centre-line committed so far plus the pending segment
        // to the cursor.
        let mut pts: Vec<[f64; 3]> = self.points.iter().map(|p| [p.x, p.y, p.z]).collect();
        pts.push([pt.x, pt.y, pt.z]);
        Some(WireModel::solid_f64(
            "rubber_band".to_string(),
            pts,
            WireModel::CYAN,
            false,
        ))
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["TRACE"] });  // TraceCommand
