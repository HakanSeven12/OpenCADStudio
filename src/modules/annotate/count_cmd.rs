// COUNT / COUNTAREA, COUNTTABLE and UPDATEFIELD.
//
//   Specify first corner point of the count area or
//     [Current area/Entire model space/Object/Polygonal] <Current area>:
//   Specify opposite corner:                (Polygonal: Specify start point: /
//                                            Specify next point; Object:
//                                            Select object as count area boundary:)
//   Select target objects or [List all blocks] <List all blocks>:   (COUNT)
//
// The command gathers its answers and hands them to the application as
// `_COUNTRUN <area> L|T <handles>` (COUNTAREA: `_COUNTAREASET <area>`), the
// area being `C` (current area, which counts all of model space), `E`
// (entire model space), `R x,y x,y`, `P x,y x,y …` or `O <handle>`.

use codec::types::Handle;
use codec::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::model::wire_model::WireModel;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Step {
    Area,
    Corner,
    PolyStart,
    PolyNext,
    Object,
    Targets,
}

pub struct CountCommand {
    /// COUNTAREA: only the area is asked.
    area_only: bool,
    step: Step,
    area: String,
    points: Vec<DVec3>,
    targets: Vec<Handle>,
    picked: Option<EntityType>,
}

fn xy(p: DVec3) -> String {
    format!("{:?},{:?}", p.x, p.y)
}

impl CountCommand {
    pub fn new(area_only: bool) -> Self {
        Self {
            area_only,
            step: Step::Area,
            area: String::new(),
            points: Vec::new(),
            targets: Vec::new(),
            picked: None,
        }
    }

    /// COUNT whose area is already answered (`COUNT _C`).
    pub fn with_area(area: &str) -> Self {
        Self { area: area.to_string(), step: Step::Targets, ..Self::new(false) }
    }

    fn area_done(&mut self, area: String) -> CmdResult {
        self.area = area;
        if self.area_only {
            return CmdResult::Dispatch(format!("_COUNTAREASET {}", self.area));
        }
        self.step = Step::Targets;
        CmdResult::NeedPoint
    }

    fn area_keyword(&mut self, text: &str) -> CmdResult {
        match text.trim().trim_start_matches('_').to_ascii_uppercase().as_str() {
            "" | "C" | "CURRENT" | "CURRENT AREA" => self.area_done("C".into()),
            "E" | "ENTIRE" | "ENTIRE MODEL SPACE" => self.area_done("E".into()),
            "O" | "OBJECT" => {
                self.step = Step::Object;
                CmdResult::NeedPoint
            }
            "P" | "POLYGONAL" => {
                self.points.clear();
                self.step = Step::PolyStart;
                CmdResult::NeedPoint
            }
            _ => CmdResult::ReportError(crate::t!("Invalid option keyword.").into_owned()),
        }
    }

    fn finish_targets(&self) -> CmdResult {
        if self.targets.is_empty() {
            return CmdResult::Dispatch(format!("_COUNTRUN {} L", self.area));
        }
        let handles: Vec<String> = self.targets.iter().map(|h| format!("{:X}", h.value())).collect();
        CmdResult::Dispatch(format!("_COUNTRUN {} T {}", self.area, handles.join(",")))
    }
}

impl CadCommand for CountCommand {
    fn name(&self) -> &'static str {
        if self.area_only {
            "COUNTAREA"
        } else {
            "COUNT"
        }
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Area => "Specify first corner point of the count area or [Current area/Entire model space/Object/Polygonal] <Current area>:".into(),
            Step::Corner => "Specify opposite corner:".into(),
            Step::PolyStart => "Specify start point:".into(),
            // The reference shows this one without a colon.
            Step::PolyNext => "Specify next point".into(),
            Step::Object => "Select object as count area boundary:".into(),
            Step::Targets => "Select target objects or [List all blocks] <List all blocks>:".into(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Area => vec![
                CmdOption::new("Current area", "C"),
                CmdOption::new("Entire model space", "E"),
                CmdOption::new("Object", "O"),
                CmdOption::new("Polygonal", "P"),
            ],
            Step::Targets => vec![CmdOption::new("List all blocks", "L")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Targets => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == Step::Area
    }

    fn is_selection_gathering(&self) -> bool {
        self.step == Step::Targets
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        // The selection arrives whole after every pick: `1 found`, then
        // `1 found, 2 total`.
        let found = handles.iter().filter(|h| !self.targets.contains(h)).count();
        let first = self.targets.is_empty();
        self.targets = handles;
        let total = self.targets.len();
        CmdResult::ReportMeasurement(if first {
            crate::tf!("{found} found").into_owned()
        } else {
            crate::tf!("{found} found, {total} total").into_owned()
        })
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::Object
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(EntityType::LwPolyline(pl)) if codec::count::valid_boundary(&pl) => {
                self.area_done(format!("O {:X}", handle.value()))
            }
            _ => CmdResult::CancelWithMessage(
                crate::t!(
                    "Invalid count area boundary object. Select a closed polyline consisting of line segments and does not intersect itself."
                )
                .into_owned(),
            ),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Area => {
                self.points = vec![pt];
                self.step = Step::Corner;
                CmdResult::NeedPoint
            }
            Step::Corner => {
                let area = format!("R {} {}", xy(self.points[0]), xy(pt));
                self.area_done(area)
            }
            Step::PolyStart | Step::PolyNext => {
                self.points.push(pt);
                self.step = Step::PolyNext;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            Step::Area => Some(self.area_keyword(text)),
            Step::Targets => match text.trim().trim_start_matches('_').to_ascii_uppercase().as_str() {
                "L" | "LIST" | "LIST ALL BLOCKS" => {
                    self.targets.clear();
                    Some(self.finish_targets())
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Area => self.area_keyword(""),
            Step::PolyNext if self.points.len() >= 3 => {
                let ring: Vec<String> = self.points.iter().map(|p| xy(*p)).collect();
                let area = format!("P {}", ring.join(" "));
                self.area_done(area)
            }
            Step::Targets => self.finish_targets(),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let ring: Vec<DVec3> = match self.step {
            Step::Corner => {
                let a = self.points[0];
                vec![a, DVec3::new(pt.x, a.y, a.z), pt, DVec3::new(a.x, pt.y, a.z)]
            }
            Step::PolyNext => {
                let mut ring = self.points.clone();
                ring.push(pt);
                ring
            }
            _ => return None,
        };
        let mut points: Vec<[f64; 3]> = ring.iter().map(|p| p.to_array()).collect();
        points.push(points[0]);
        Some(WireModel::solid_f64("count_area".into(), points, WireModel::CYAN, false))
    }
}

/// COUNTTABLE: the blocks to list, then where the table goes.
pub struct CountTableCommand {
    step: TableStep,
    /// Defined user blocks (sorted), and the listing's other counts:
    /// external references, dependent and unnamed blocks.
    blocks: Vec<String>,
    others: [usize; 3],
    names: Vec<String>,
    /// Listing lines still to show after `Press ENTER to continue:`.
    pending: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TableStep {
    Names,
    List,
    Point,
    Page,
}

impl CountTableCommand {
    pub fn new(blocks: Vec<String>, others: [usize; 3]) -> Self {
        Self { step: TableStep::Names, blocks, others, names: Vec::new(), pending: Vec::new() }
    }

    /// The palette's Create Table: the blocks are chosen, only the point is asked.
    pub fn placing(names: Vec<String>) -> Self {
        Self { step: TableStep::Point, blocks: Vec::new(), others: [0; 3], names, pending: Vec::new() }
    }

    /// `?`: the defined blocks matching the pattern, paged by 22 lines
    /// (`Press ENTER to continue:` in between), then the summary.
    fn listing(&mut self, pattern: &str) -> CmdResult {
        let pattern = if pattern.trim().is_empty() { "*" } else { pattern.trim() };
        let mut names: Vec<&String> = self
            .blocks
            .iter()
            .filter(|name| pattern.split(',').any(|p| crate::io::xref_model::wildcard_match(name, p.trim())))
            .collect();
        names.sort_by_key(|name| list_order(name));
        self.pending = std::iter::once(String::new())
            .chain(std::iter::once("Defined blocks.".to_string()))
            .chain(names.iter().map(|name| format!("  {:<31}", format!("\"{name}\""))))
            .collect();
        self.next_page()
    }

    fn next_page(&mut self) -> CmdResult {
        let page: Vec<String> = self.pending.drain(..self.pending.len().min(22)).collect();
        if !self.pending.is_empty() {
            self.step = TableStep::Page;
            return CmdResult::ReportMeasurement(page.join("\n"));
        }
        CmdResult::Measurement(format!(
            "{}\n\nUser     External     Dependent   Unnamed\nBlocks   References   Blocks      Blocks\n{:>5}{:>10}{:>12}{:>12}\n",
            page.join("\n"),
            self.blocks.len(),
            self.others[0],
            self.others[1],
            self.others[2]
        ))
    }
}

/// Block listing order: `_` first, then `-`, space and other punctuation,
/// digits, and letters without case.
fn list_order(name: &str) -> Vec<(u32, u32)> {
    name.chars()
        .map(|c| match c {
            '_' => (0, 0),
            '-' => (1, 0),
            ' ' => (2, 0),
            '0'..='9' => (4, c as u32),
            c if c.is_alphabetic() => (5, c.to_lowercase().next().unwrap_or(c) as u32),
            c => (3, c as u32),
        })
        .collect()
}

impl CadCommand for CountTableCommand {
    fn name(&self) -> &'static str {
        "COUNTTABLE"
    }

    fn prompt(&self) -> String {
        match self.step {
            TableStep::Names => "Enter block name(s) to include or [?] <all blocks>:".into(),
            TableStep::List => "Enter block(s) to list <*>:".into(),
            TableStep::Point => "Specify insertion point:".into(),
            TableStep::Page => "Press ENTER to continue:".into(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            TableStep::Point => InputKind::Point,
            _ => InputKind::FreeText,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        Some(match self.step {
            TableStep::Names if text == "?" => {
                self.step = TableStep::List;
                CmdResult::NeedPoint
            }
            TableStep::Names if text.is_empty() => {
                self.names.clear();
                self.step = TableStep::Point;
                CmdResult::NeedPoint
            }
            TableStep::Names => {
                let mut names = Vec::new();
                for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    match self.blocks.iter().find(|b| b.eq_ignore_ascii_case(part)) {
                        Some(b) => names.push(b.clone()),
                        None => {
                            return Some(CmdResult::CancelWithMessage(
                                crate::tf!("Block \"{part}\" cannot be found.").into_owned(),
                            ))
                        }
                    }
                }
                self.names = names;
                self.step = TableStep::Point;
                CmdResult::NeedPoint
            }
            TableStep::List => self.listing(text),
            TableStep::Page => self.next_page(),
            TableStep::Point => return None,
        })
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step != TableStep::Point {
            return CmdResult::NeedPoint;
        }
        CmdResult::Dispatch(format!("_COUNTTABLEPLACE {:?},{:?},{:?} {}", pt.x, pt.y, pt.z, self.names.join("|")))
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            TableStep::Point => CmdResult::ReportError(crate::t!("Invalid point.").into_owned()),
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }
}

/// UPDATEFIELD: `Select objects:`, then the fields of the selection are
/// evaluated again.
pub struct UpdateFieldCommand {
    selected: Vec<Handle>,
}

impl UpdateFieldCommand {
    pub fn new() -> Self {
        Self { selected: Vec::new() }
    }
}

impl CadCommand for UpdateFieldCommand {
    fn name(&self) -> &'static str {
        "UPDATEFIELD"
    }

    fn prompt(&self) -> String {
        "Select objects:".into()
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.selected = handles;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        // An empty selection still runs (the reference keeps an undo step).
        let handles: Vec<String> = self.selected.iter().map(|h| format!("{:X}", h.value())).collect();
        CmdResult::Dispatch(format!("_UPDATEFIELDRUN {}", handles.join(",")))
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["COUNT", "COUNTAREA", "COUNTCLOSE", "COUNTFIELD", "COUNTLIST", "COUNTLISTCLOSE", "COUNTTABLE", "UPDATEFIELD"]
});
