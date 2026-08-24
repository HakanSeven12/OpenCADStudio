// DATALINK command — manages persistent links to external tabular data.

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;
use acadrust::entities::Table;
use acadrust::EntityType;
use glam::DVec3;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/data_link.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DATALINK",
        label: "Link\nData",
        icon: ICON,
        event: ModuleEvent::Command("DATALINK".to_string()),
    }
}

pub struct DataLinkPlaceCommand {
    table: Table,
    plane: WorkingPlane,
}

impl DataLinkPlaceCommand {
    pub fn new(mut table: Table, path: &str) -> Self {
        table.name = format!("__OPENCAD_LINK_PENDING__{path}");
        Self {
            table,
            plane: WorkingPlane::default(),
        }
    }

    fn preview(&self, point: DVec3) -> WireModel {
        let point = self.plane.to_local(point);
        let mut points = Vec::new();
        let mut x = 0.0;
        for column in 0..=self.table.column_count() {
            if column > 0 {
                x += self.table.columns[column - 1].width;
            }
            points.push(self.plane.to_world(point + DVec3::X * x).as_vec3().to_array());
            points.push(
                self.plane
                    .to_world(point + DVec3::new(x, -self.table.total_height(), 0.0))
                    .as_vec3()
                    .to_array(),
            );
        }
        let mut y = 0.0;
        for row in 0..=self.table.row_count() {
            if row > 0 {
                y -= self.table.rows[row - 1].height;
            }
            points.push(self.plane.to_world(point + DVec3::Y * y).as_vec3().to_array());
            points.push(
                self.plane
                    .to_world(point + DVec3::new(self.table.total_width(), y, 0.0))
                    .as_vec3()
                    .to_array(),
            );
        }
        WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
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
            name: "data_link_preview".into(),
            points,
            points_low: Vec::new(),
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: Vec::new(),
            tangent_geoms: Vec::new(),
            aci: 0,
            key_vertices: Vec::new(),
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris: Vec::new(),
            fill_tris_low: Vec::new(),
        }
    }
}

impl CadCommand for DataLinkPlaceCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DATALINK"
    }

    fn prompt(&self) -> String {
        t!("DATALINK  Specify insertion point:").into_owned()
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let point = self.plane.to_local(point);
        self.table.insertion_point = acadrust::types::Vector3::new(point.x, point.y, point.z);
        CmdResult::CommitAndExit(
            self.plane
                .place_entity(EntityType::Table(self.table.clone())),
        )
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        Some(self.preview(point))
    }
}
