// TABLE command — create an empty table entity.
//
// Workflow:
//   1. Text: Enter number of columns  (default 3)
//   2. Text: Enter number of rows     (default 4, includes header row)
//   3. Point: Click insertion point
//
// Creates a Table entity with uniform row height (0.5) and column width (2.0).

use acadrust::entities::TableBuilder;
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/table.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "TABLE",
        label: "Table",
        icon: ICON,
        event: ModuleEvent::Command("TABLE".to_string()),
    }
}

const DEFAULT_COLS: usize = 3;
const DEFAULT_ROWS: usize = 4;
const COL_WIDTH: f64 = 2.0;
const ROW_HEIGHT: f64 = 0.5;

enum Step {
    Columns,
    Rows { cols: usize },
    Insertion { cols: usize, rows: usize },
}

pub struct TableCommand {
    step: Step,
    style_handle: Option<acadrust::Handle>,
    column_width: f64,
    row_height: f64,
    preview_scale: f64,
    plane: WorkingPlane,
}

impl TableCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Columns,
            style_handle: None,
            column_width: COL_WIDTH,
            row_height: ROW_HEIGHT,
            preview_scale: 1.0,
            plane: WorkingPlane::default(),
        }
    }

    pub fn with_style(
        style_handle: acadrust::Handle,
        style: &acadrust::objects::TableStyle,
        annotation_multiplier: f64,
    ) -> Self {
        let text_height = style
            .data_row_style
            .text_height
            .max(style.header_row_style.text_height)
            .max(style.title_row_style.text_height)
            .max(1.0e-6);
        let row_height = text_height * 1.5 + style.vertical_margin * 2.0;
        let column_width = text_height * 8.0 + style.horizontal_margin * 2.0;
        Self {
            step: Step::Columns,
            style_handle: Some(style_handle),
            column_width,
            row_height,
            preview_scale: if style.annotative {
                annotation_multiplier
            } else {
                1.0
            },
            plane: WorkingPlane::default(),
        }
    }
}

impl CadCommand for TableCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "TABLE"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Columns => t!(
                "TABLE  Enter number of columns [%{cols}]:",
                cols = DEFAULT_COLS
            )
            .into_owned(),
            Step::Rows { cols } => t!(
                "TABLE  Enter number of rows (incl. header) [%{rows}]  (%{cols} cols):",
                rows = DEFAULT_ROWS,
                cols = cols
            )
            .into_owned(),
            Step::Insertion { cols, rows } => t!(
                "TABLE  Specify insertion point  [%{cols}×%{rows}]:",
                cols = cols,
                rows = rows
            )
            .into_owned(),
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, Step::Insertion { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        match &self.step {
            Step::Columns => {
                let cols = if t.is_empty() {
                    DEFAULT_COLS
                } else {
                    t.parse::<usize>().ok().filter(|&n| n >= 1)?
                };
                self.step = Step::Rows { cols };
                Some(CmdResult::NeedPoint)
            }
            Step::Rows { cols } => {
                let cols = *cols;
                let rows = if t.is_empty() {
                    DEFAULT_ROWS
                } else {
                    t.parse::<usize>().ok().filter(|&n| n >= 1)?
                };
                self.step = Step::Insertion { cols, rows };
                Some(CmdResult::NeedPoint)
            }
            Step::Insertion { .. } => None,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.step {
            Step::Columns => {
                // Accept default.
                self.step = Step::Rows { cols: DEFAULT_COLS };
                CmdResult::NeedPoint
            }
            Step::Rows { cols } => {
                let cols = *cols;
                self.step = Step::Insertion {
                    cols,
                    rows: DEFAULT_ROWS,
                };
                CmdResult::NeedPoint
            }
            Step::Insertion { .. } => CmdResult::Cancel,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if let Step::Insertion { cols, rows } = self.step {
            let point = self.plane.to_local(pt);
            let ins = Vector3::new(point.x, point.y, point.z);
            let mut table = TableBuilder::new(rows, cols)
                .at(ins)
                .row_height(self.row_height)
                .column_width(self.column_width)
                .build();
            table.table_style_handle = self.style_handle;
            CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Table(table)))
        } else {
            CmdResult::NeedPoint
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if let Step::Insertion { cols, rows } = self.step {
            // Preview: outline of the table bounding box.
            let w = (cols as f32) * (self.column_width * self.preview_scale) as f32;
            let h = (rows as f32) * (self.row_height * self.preview_scale) as f32;
            let point = self.plane.to_local(pt);
            let corners = [
                point,
                point + DVec3::X * w as f64,
                point + DVec3::new(w as f64, -(h as f64), 0.0),
                point - DVec3::Y * h as f64,
            ]
            .map(|corner| self.plane.to_world(corner).as_vec3().to_array());
            let [p0, p1, p2, p3] = corners;
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
                name: "table_preview".into(),
                points: vec![
                    p0, p1, p1, p2, p2, p3, p3, p0,
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
        } else {
            None
        }
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["TABLE"] });  // TableCommand
