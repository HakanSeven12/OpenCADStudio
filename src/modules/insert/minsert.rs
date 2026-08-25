//! MINSERT command — place a block reference as a rectangular array.
//!
//! Modeled on the plain INSERT command but simpler: there is no attribute
//! filling. The user picks a block name and an insertion point, then types the
//! array parameters (rows, columns, row spacing, column spacing). The committed
//! entity is a single [`Insert`] with its array fields set, which the renderer
//! replicates over `row_count × column_count` using the row/column spacing.

use acadrust::entities::Insert;
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "MINSERT",
        label: "Array Insert",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/blocks/insert.svg")),
        event: ModuleEvent::Command("MINSERT".to_string()),
    }
}

/// Which step the command is currently collecting.
enum Step {
    /// Pick / type the block name from the available list.
    Name,
    /// Specify the insertion point for `name`.
    Point { name: String },
    /// Type the numeric array parameters, one per `ParamIdx`.
    Params {
        name: String,
        point: Vector3,
        idx: ParamIdx,
    },
}

/// The numeric array parameter currently being typed.
#[derive(Clone, Copy)]
enum ParamIdx {
    Rows,
    Columns,
    RowSpacing,
    ColumnSpacing,
}

impl ParamIdx {
    fn next(self) -> Option<ParamIdx> {
        match self {
            ParamIdx::Rows => Some(ParamIdx::Columns),
            ParamIdx::Columns => Some(ParamIdx::RowSpacing),
            ParamIdx::RowSpacing => Some(ParamIdx::ColumnSpacing),
            ParamIdx::ColumnSpacing => None,
        }
    }
}

pub struct MinsertCommand {
    picker: crate::modules::insert::picker::BlockPicker,
    step: Step,
    /// Collected array parameters (defaults applied as the user Enters through).
    rows: u16,
    columns: u16,
    row_spacing: f64,
    column_spacing: f64,
    plane: WorkingPlane,
}

impl MinsertCommand {
    pub fn new_with_usage(
        available: Vec<String>,
        usage_rank: rustc_hash::FxHashMap<String, (u32, usize)>,
        cliprompt_lines: u8,
    ) -> Self {
        let limit = (cliprompt_lines as usize).clamp(0, crate::modules::insert::picker::MAX_SUGGESTIONS);
        let picker = crate::modules::insert::picker::BlockPicker::new(available, usage_rank, limit);
        Self {
            picker,
            step: Step::Name,
            rows: 1,
            columns: 1,
            row_spacing: 0.0,
            column_spacing: 0.0,
            plane: WorkingPlane::default(),
        }
    }

    /// Build the array Insert from the collected parameters and finish.
    fn build(&self, name: &str, point: Vector3) -> CmdResult {
        let mut ins = Insert::new(name.to_string(), point);
        ins.row_count = self.rows.max(1);
        ins.column_count = self.columns.max(1);
        ins.row_spacing = self.row_spacing;
        ins.column_spacing = self.column_spacing;
        CmdResult::CommitAndExit(self.plane.place_entity(EntityType::Insert(ins)))
    }

    /// Advance from the parameter currently at `idx` to the next, or build the
    /// entity when the last parameter has been accepted.
    fn advance_param(&mut self, name: String, point: Vector3, idx: ParamIdx) -> CmdResult {
        match idx.next() {
            Some(next) => {
                self.step = Step::Params {
                    name,
                    point,
                    idx: next,
                };
                CmdResult::NeedPoint
            }
            None => self.build(&name, point),
        }
    }
}

impl CadCommand for MinsertCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "MINSERT"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Name => {
                if self.picker.is_empty() {
                    return t!("MINSERT  Enter block name:").into_owned();
                }
                let needle = self.picker.needle();
                let filtered = self.picker.filtered();
                if !needle.is_empty() && filtered.is_empty() {
                    return t!("MINSERT  No matching blocks for \"%{needle}\"", needle = needle).into_owned();
                }
                if needle.is_empty() {
                    let total = self.picker.total();
                    let shown = filtered.len();
                    if total <= shown {
                        t!("MINSERT  Enter block name:").into_owned()
                    } else {
                        t!("MINSERT  Enter block name:  [%{shown} of %{total} — type to search]", shown = shown, total = total).into_owned()
                    }
                } else {
                    t!("MINSERT  Enter block name:  \"%{needle}\"  [%{shown} matches]", needle = needle, shown = filtered.len()).into_owned()
                }
            }
            Step::Point { name } => {
                t!(
                    "MINSERT  Specify insertion point for \"%{name}\":",
                    name = name
                )
                .into_owned()
            }
            Step::Params { idx, .. } => match idx {
                ParamIdx::Rows => t!(
                    "MINSERT  Enter number of rows <%{rows}>:",
                    rows = self.rows
                )
                .into_owned(),
                ParamIdx::Columns => t!(
                    "MINSERT  Enter number of columns <%{cols}>:",
                    cols = self.columns
                )
                .into_owned(),
                ParamIdx::RowSpacing => t!(
                    "MINSERT  Enter row spacing <%{val}>:",
                    val = self.row_spacing
                )
                .into_owned(),
                ParamIdx::ColumnSpacing => t!(
                    "MINSERT  Enter column spacing <%{val}>:",
                    val = self.column_spacing
                )
                .into_owned(),
            },
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match &self.step {
            Step::Name => CmdResult::NeedPoint,
            Step::Point { name } => {
                let name = name.clone();
                let point = self.plane.to_local(pt);
                let point = Vector3::new(point.x, point.y, point.z);
                self.step = Step::Params {
                    name,
                    point,
                    idx: ParamIdx::Rows,
                };
                CmdResult::NeedPoint
            }
            // Numeric-parameter steps ignore stray clicks and keep prompting.
            Step::Params { .. } => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.step {
            // Bare Enter at a parameter step accepts the current default and
            // advances (or commits after the last one).
            Step::Params { name, point, idx } => {
                let (name, point, idx) = (name.clone(), *point, *idx);
                self.advance_param(name, point, idx)
            }
            // Enter before a block / point is supplied cancels the command.
            Step::Name | Step::Point { .. } => CmdResult::Cancel,
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        match &self.step {
            Step::Name => self.picker.filtered().iter().map(|n| crate::command::CmdOption::new(n, n)).collect(),
            _ => Vec::new(),
        }
    }

    fn on_live_input(&mut self, input: &str) -> bool {
        if !matches!(self.step, Step::Name) {
            return false;
        }
        let needle = input.trim();
        if needle == self.picker.needle() {
            return false;
        }
        self.picker.set_needle(needle.to_string());
        true
    }

    fn wants_text_input(&self) -> bool {
        matches!(self.step, Step::Name | Step::Params { .. })
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match &self.step {
            Step::Name => {
                let typed = text.trim();
                if typed.is_empty() {
                    self.picker.set_needle(String::new());
                    return Some(CmdResult::NeedPoint);
                }
                if let Some(matched) = self.picker.contains_name(typed) {
                    self.step = Step::Point { name: matched };
                    return Some(CmdResult::NeedPoint);
                }
                self.picker.set_needle(typed.to_string());
                Some(CmdResult::NeedPoint)
            }
            Step::Point { .. } => None,
            Step::Params { name, point, idx } => {
                let (name, point, idx) = (name.clone(), *point, *idx);
                let typed = text.trim();
                // Empty input keeps the default; otherwise parse and store.
                if !typed.is_empty() {
                    match idx {
                        ParamIdx::Rows => {
                            if let Ok(v) = typed.parse::<u16>() {
                                self.rows = v.max(1);
                            }
                        }
                        ParamIdx::Columns => {
                            if let Ok(v) = typed.parse::<u16>() {
                                self.columns = v.max(1);
                            }
                        }
                        ParamIdx::RowSpacing => {
                            if let Some(v) = crate::entities::common::parse_typed_length(typed) {
                                self.row_spacing = v;
                            }
                        }
                        ParamIdx::ColumnSpacing => {
                            if let Some(v) = crate::entities::common::parse_typed_length(typed) {
                                self.column_spacing = v;
                            }
                        }
                    }
                }
                Some(self.advance_param(name, point, idx))
            }
        }
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["MINSERT"]
});
