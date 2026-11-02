// DATALINK command — manages persistent links to external tabular data.

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;
use codec::entities::Table;
use codec::EntityType;
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

/// Download from Source: refresh linked tables from their files.
pub fn download_tool() -> ToolDef {
    ToolDef {
        id: "DATALINKUPDATE",
        label: "Download from Source",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/sync.svg")),
        event: ModuleEvent::Command("DATALINKUPDATE;_U".to_string()),
    }
}

/// Upload to Source: write linked tables back to their files.
pub fn upload_tool() -> ToolDef {
    ToolDef {
        id: "DATALINKUPDATE_WRITE",
        label: "Upload to Source",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/ui/file_export.svg")),
        event: ModuleEvent::Command("DATALINKUPDATE;_W".to_string()),
    }
}

/// Marks a table waiting to be linked to the extraction settings named after
/// the marker.
pub const EXTRACTION_PENDING: &str = "__OPENCAD_EXTRACTION_PENDING__";

pub struct DataLinkPlaceCommand {
    table: Table,
    plane: WorkingPlane,
    command_name: &'static str,
}

impl DataLinkPlaceCommand {
    pub fn new(mut table: Table, path: &str) -> Self {
        table.name = format!("__OPENCAD_LINK_PENDING__{path}");
        Self {
            table,
            plane: WorkingPlane::default(),
            command_name: "DATALINK",
        }
    }

    /// Place a table that references a data-link object already stored in the
    /// drawing. Every populated cell is locked and points at the same link so
    /// update and save/reopen keep one stable relationship.
    pub fn existing(mut table: Table, link: codec::Handle) -> Self {
        link_cells(&mut table, link, 0);
        Self {
            table,
            plane: WorkingPlane::default(),
            command_name: "DATALINK",
        }
    }

    /// The same, placed from -TABLE's data Link option.
    pub fn existing_from_table(table: Table, link: codec::Handle) -> Self {
        Self {
            command_name: "-TABLE",
            ..Self::existing(table, link)
        }
    }

    /// Place a data extraction table; on placement it is linked to the
    /// extraction's settings file so DATALINKUPDATE can refresh it.
    pub fn extraction(mut table: Table, settings: String) -> Self {
        table.name = format!("{EXTRACTION_PENDING}{settings}");
        Self {
            table,
            plane: WorkingPlane::default(),
            command_name: "DATAEXTRACTION",
        }
    }

    /// Place a prepared, unlinked table.
    #[allow(dead_code)]
    pub fn unlinked(table: Table) -> Self {
        Self {
            table,
            plane: WorkingPlane::default(),
            command_name: "DATAEXTRACTION",
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
            points.push(
                self.plane
                    .to_world(point + DVec3::X * x)
                    .as_vec3()
                    .to_array(),
            );
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
            points.push(
                self.plane
                    .to_world(point + DVec3::Y * y)
                    .as_vec3()
                    .to_array(),
            );
            points.push(
                self.plane
                    .to_world(point + DVec3::new(self.table.total_width(), y, 0.0))
                    .as_vec3()
                    .to_array(),
            );
        }
        WireModel {
            bg_adapt: None,
            point_marker: None,
            taper_widths: Vec::new(),
            pattern_stations: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            snap_only: false,
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

            ..Default::default()
        }
    }
}

/// Links a table to a data link the way the drawing format stores it: the
/// first cell of the linked range (row `first_row`, column 0) carries the link
/// and the size of the range; every cell of the range is locked and marked as
/// linked. Rows above `first_row` (an extraction table's title) stay free.
pub fn link_cells(table: &mut Table, link: codec::Handle, first_row: usize) {
    use codec::entities::table::CellStateFlags;
    let rows = table.row_count().saturating_sub(first_row) as i32;
    let columns = table.column_count() as i32;
    for (row_index, row) in table.rows.iter_mut().enumerate() {
        for (column_index, cell) in row.cells.iter_mut().enumerate() {
            let anchor = row_index == first_row && column_index == 0;
            cell.has_linked_data = anchor;
            cell.data_link_handle = anchor.then_some(link);
            cell.data_link_rows = if anchor { rows } else { 0 };
            cell.data_link_columns = if anchor { columns } else { 0 };
            cell.data_link_unknown = 0;
            cell.state.remove(CellStateFlags::FORMAT_LOCKED);
            if row_index >= first_row {
                cell.state
                    .insert(CellStateFlags::LINKED | CellStateFlags::CONTENT_LOCKED);
            } else {
                cell.state
                    .remove(CellStateFlags::LINKED | CellStateFlags::CONTENT_LOCKED);
            }
        }
    }
}

impl CadCommand for DataLinkPlaceCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        self.command_name
    }

    fn prompt(&self) -> String {
        if self.command_name == "DATALINK" {
            t!("DATALINK  Specify insertion point:").into_owned()
        } else if self.command_name == "-TABLE" {
            t!("%{n}  Specify insertion point:", n = "-TABLE").into_owned()
        } else {
            t!("DATAEXTRACTION  Specify insertion point:").into_owned()
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let point = self.plane.to_local(point);
        self.table.insertion_point = codec::types::Vector3::new(point.x, point.y, point.z);
        CmdResult::CommitAndExit(
            self.plane
                .place_entity(EntityType::Table(Box::new(self.table.clone()))),
        )
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.command_name == "-TABLE" {
            return CmdResult::ReportError(t!("Invalid point.").into_owned());
        }
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, _text: &str) -> Option<CmdResult> {
        (self.command_name == "-TABLE")
            .then(|| CmdResult::ReportError(t!("Invalid point.").into_owned()))
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        Some(self.preview(point))
    }
}

/// Whether a table takes any of its cells from a data link.
pub fn is_linked_table(entity: &EntityType) -> bool {
    matches!(entity, EntityType::Table(table) if table
        .rows
        .iter()
        .flat_map(|row| row.cells.iter())
        .any(|cell| cell.data_link_handle.is_some()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UpdateStep {
    Option,
    Select { write: bool },
    LinkName,
}

/// DATALINKUPDATE: choose to update tables from their sources or write them
/// back, pick the linked tables (or, when updating, name a link or take them
/// all), then hand the choice to the inline `DATALINKUPDATE _RUN …` handler.
pub struct DataLinkUpdateCommand {
    links: Vec<String>,
    step: UpdateStep,
    selected: Vec<codec::Handle>,
}

impl DataLinkUpdateCommand {
    /// `links` are the drawing's data link names.
    pub fn new(mut links: Vec<String>) -> Self {
        links.sort_by_key(|name| name.to_lowercase());
        Self {
            links,
            step: UpdateStep::Option,
            selected: Vec::new(),
        }
    }

    fn link_list(&self) -> String {
        let mut lines = vec![
            t!("Data links in Drawing:").into_owned(),
            "----------------------".to_string(),
        ];
        lines.extend(self.links.iter().map(|name| format!(" {name}")));
        lines.join("\n")
    }

    fn choose(&mut self, write: bool) -> CmdResult {
        if self.links.is_empty() {
            return CmdResult::Dispatch("DATALINKUPDATE _RUN NONE".into());
        }
        self.step = UpdateStep::Select { write };
        CmdResult::NeedPoint
    }
}

impl CadCommand for DataLinkUpdateCommand {
    fn name(&self) -> &'static str {
        "DATALINKUPDATE"
    }

    fn prompt(&self) -> String {
        let text = match self.step {
            UpdateStep::Option => {
                t!("Select an option [Update data link/Write data link] <Update data link>:")
            }
            UpdateStep::Select { write: false } => {
                t!("Select objects or [Data link/all data linKs]:")
            }
            UpdateStep::Select { write: true } => t!("Select objects:"),
            UpdateStep::LinkName => t!("Enter data link name or [?]:"),
        };
        format!("DATALINKUPDATE  {text}")
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            UpdateStep::Option => vec![
                CmdOption::new("Update data link", "U"),
                CmdOption::new("Write data link", "W"),
            ],
            UpdateStep::Select { write: false } => vec![
                CmdOption::new("Data link", "D"),
                CmdOption::new("all data linKs", "K"),
            ],
            UpdateStep::Select { write: true } => Vec::new(),
            UpdateStep::LinkName => vec![CmdOption::new("?", "?")],
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, UpdateStep::Select { .. })
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        matches!(self.step, UpdateStep::Select { .. }).then_some(is_linked_table as fn(&EntityType) -> bool)
    }

    fn selection_entity_filter_reports(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<codec::Handle>) -> CmdResult {
        self.selected = handles;
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let raw = text.trim();
        let word = raw.trim_start_matches('_').to_uppercase();
        match self.step {
            UpdateStep::Option => {
                if word.is_empty() {
                    return None;
                }
                if "UPDATE".starts_with(&word) {
                    Some(self.choose(false))
                } else if "WRITE".starts_with(&word) {
                    Some(self.choose(true))
                } else {
                    Some(CmdResult::ReportMeasurement(
                        t!("Invalid option keyword.").into_owned(),
                    ))
                }
            }
            UpdateStep::Select { write: false } => match word.as_str() {
                "D" | "DATA" | "DATALINK" => {
                    self.step = UpdateStep::LinkName;
                    Some(CmdResult::NeedPoint)
                }
                "K" | "ALLDATALINKS" | "LINKS" => {
                    Some(CmdResult::Dispatch("DATALINKUPDATE _RUN U ALL".into()))
                }
                _ => None,
            },
            UpdateStep::Select { write: true } => None,
            UpdateStep::LinkName => {
                if raw.is_empty() || raw == "?" {
                    return Some(CmdResult::ReportMeasurement(self.link_list()));
                }
                match self
                    .links
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case(raw))
                {
                    Some(name) => Some(CmdResult::Dispatch(format!(
                        "DATALINKUPDATE _RUN U NAME {name}"
                    ))),
                    None => Some(CmdResult::ReportMeasurement(
                        crate::tf!("Data link {raw} does not exist.").into_owned(),
                    )),
                }
            }
        }
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            UpdateStep::Option => self.choose(false),
            UpdateStep::Select { write } => {
                if self.selected.is_empty() {
                    return CmdResult::Cancel;
                }
                CmdResult::Relaunch(
                    format!("DATALINKUPDATE _RUN {} SEL", if write { "W" } else { "U" }),
                    std::mem::take(&mut self.selected),
                )
            }
            UpdateStep::LinkName => CmdResult::ReportMeasurement(self.link_list()),
        }
    }
}
