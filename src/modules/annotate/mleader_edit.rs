// MLEADEREDIT / AIMLEADEREDITADD / AIMLEADEREDITREMOVE / MLEADERALIGN /
// MLEADERCOLLECT commands.
//
// MLEADEREDIT:   pick a multileader → add leaders at arrowhead points, or
//                [Remove leaders] picked leaders; Enter applies the edit.
// MLEADERALIGN:  select multileaders → pick base alignment direction
// MLEADERCOLLECT: select block-content multileaders → pick collection point

use codec::entities::{LeaderLine, MultiLeader};
use codec::types::Vector3;
use codec::{EntityType, Handle};
use glam::{DVec3, Vec3};
use kernel::geom2d::{closest_point, Curve, Line};

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;

// ── MLEADEREDIT ───────────────────────────────────────────────────────────────

pub const ICON_ADD: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/mleader_add.svg"));

pub fn tool_add() -> ToolDef {
    ToolDef {
        id: "AIMLEADEREDITADD",
        label: "Add Leader",
        icon: ICON_ADD,
        event: ModuleEvent::Command("AIMLEADEREDITADD".to_string()),
    }
}

pub const ICON_REMOVE: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/mleader_remove.svg"));

pub fn tool_remove() -> ToolDef {
    ToolDef {
        id: "AIMLEADEREDITREMOVE",
        label: "Remove Leader",
        icon: ICON_REMOVE,
        event: ModuleEvent::Command("AIMLEADEREDITREMOVE".to_string()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditMode {
    Add,
    Remove,
}

/// One command behind MLEADEREDIT and its add / remove entry points: the
/// picked multileader is edited in a copy and written back in one step.
pub struct MLeaderEditCommand {
    name: &'static str,
    mode: EditMode,
    picked: Option<EntityType>,
    target: Option<(Handle, MultiLeader)>,
    /// Leader lines picked for removal, as (root, line) indices.
    removals: Vec<(usize, usize)>,
    changed: bool,
}

impl MLeaderEditCommand {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            mode: if name == "AIMLEADEREDITREMOVE" {
                EditMode::Remove
            } else {
                EditMode::Add
            },
            picked: None,
            target: None,
            removals: Vec::new(),
            changed: false,
        }
    }

    /// Starts on `ml` (the preselected multileader) instead of asking for one.
    pub fn with_target(mut self, handle: Handle, ml: MultiLeader) -> Self {
        self.target = Some((handle, ml));
        self
    }

    /// The leader line nearest `pick`, when the pick lands on it.
    fn leader_at(ml: &MultiLeader, pick: Vector3) -> Option<(usize, usize)> {
        let tolerance = (ml.context.arrowhead_size.max(ml.arrowhead_size) * 1.5).max(1.0e-6);
        ml.context
            .leader_roots
            .iter()
            .enumerate()
            .flat_map(|(root_index, root)| {
                root.lines.iter().enumerate().map(move |(line_index, line)| {
                    let mut distance = line
                        .points
                        .windows(2)
                        .map(|segment| point_segment_distance_xy(pick, segment[0], segment[1]))
                        .fold(f64::INFINITY, f64::min);
                    if let Some(last) = line.points.last().copied() {
                        distance = distance.min(point_segment_distance_xy(
                            pick,
                            last,
                            root.connection_point,
                        ));
                    }
                    (root_index, line_index, distance)
                })
            })
            .filter(|(_, _, distance)| *distance <= tolerance)
            .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(root, line, _)| (root, line))
    }

    /// The edited multileader with the picked leaders taken out.
    fn applied(&self) -> Option<(Handle, MultiLeader)> {
        let (handle, ml) = self.target.as_ref()?;
        let mut ml = ml.clone();
        let mut removals = self.removals.clone();
        removals.sort_unstable_by(|a, b| b.cmp(a));
        for (root, line) in removals {
            if let Some(r) = ml.context.leader_roots.get_mut(root) {
                if line < r.lines.len() {
                    r.lines.remove(line);
                }
            }
        }
        ml.context.leader_roots.retain(|root| !root.lines.is_empty());
        Some((*handle, ml))
    }
}

impl CadCommand for MLeaderEditCommand {
    fn name(&self) -> &'static str {
        self.name
    }

    fn prompt(&self) -> String {
        if self.target.is_none() {
            return crate::tr!("mleader-edit", "select");
        }
        match self.mode {
            EditMode::Add => crate::tr!("mleader-edit", "add"),
            EditMode::Remove => crate::tr!("mleader-edit", "remove"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match (self.target.is_some(), self.mode) {
            (false, _) => Vec::new(),
            (true, EditMode::Add) => vec![CmdOption::new("Remove leaders", "R")],
            (true, EditMode::Remove) => vec![CmdOption::new("Add leaders", "A")],
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.target.is_none()
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn typed_point_picks_entity(&self) -> bool {
        self.target.is_none()
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(EntityType::MultiLeader(ml)) if !handle.is_null() => {
                self.target = Some((handle, *ml));
                CmdResult::NeedPoint
            }
            Some(_) => CmdResult::ReportError(crate::tr!("mleader-edit", "not-multileader")),
            None => CmdResult::NeedPoint,
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::Point
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.target.is_some()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        self.target.as_ref()?;
        let key = text.trim().trim_start_matches('_').to_ascii_uppercase();
        match (self.mode, key.as_str()) {
            (EditMode::Add, "R" | "REMOVE" | "REMOVE LEADERS") => self.mode = EditMode::Remove,
            (EditMode::Remove, "A" | "ADD" | "ADD LEADERS") => self.mode = EditMode::Add,
            _ => {
                return Some(CmdResult::ReportError(
                    t!("Invalid option keyword.").into_owned(),
                ))
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let pick = Vector3::new(pt.x, pt.y, pt.z);
        let mode = self.mode;
        let Some((_, ml)) = self.target.as_mut() else {
            return CmdResult::NeedPoint;
        };
        match mode {
            EditMode::Add => {
                crate::entities::multileader::add_leader_line(ml, pick);
                self.changed = true;
            }
            EditMode::Remove => {
                if let Some(found) = Self::leader_at(ml, pick) {
                    if !self.removals.contains(&found) {
                        self.removals.push(found);
                        self.changed = true;
                    }
                }
            }
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if !self.changed {
            return CmdResult::Cancel;
        }
        let Some((handle, ml)) = self.applied() else {
            return CmdResult::Cancel;
        };
        // Taking out every leader leaves the content on its own.
        let message = (!self.removals.is_empty() && ml.total_leader_line_count() == 0)
            .then(|| crate::tr!("mleader-edit", "none-left"));
        let entity = EntityType::MultiLeader(Box::new(ml));
        match message {
            Some(message) => CmdResult::UpdateEntityAndReport {
                handle,
                entity,
                message,
            },
            None => CmdResult::UpdateEntityAndFinish { handle, entity },
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let (_, ml) = self.applied()?;
        // Rubber band from the cursor to the landing it would join.
        if self.mode != EditMode::Add {
            return None;
        }
        let mut probe = ml;
        crate::entities::multileader::add_leader_line(
            &mut probe,
            Vector3::new(pt.x, pt.y, pt.z),
        );
        let root = probe
            .context
            .leader_roots
            .iter()
            .find(|root| {
                root.lines
                    .last()
                    .and_then(|line| line.points.first())
                    .is_some_and(|p| (p.x - pt.x).abs() < 1e-9 && (p.y - pt.y).abs() < 1e-9)
            })?;
        let cp = root.connection_point;
        Some(preview_wire(&[
            pt.as_vec3(),
            Vec3::new(cp.x as f32, cp.y as f32, cp.z as f32),
        ]))
    }

    fn persistent_preview_wires(&self) -> Vec<WireModel> {
        // The edit so far: the added and the remaining leaders.
        let Some((_, ml)) = self.applied() else {
            return Vec::new();
        };
        ml.context
            .leader_roots
            .iter()
            .flat_map(|root| {
                root.lines.iter().map(move |line| {
                    let mut pts: Vec<Vec3> = line
                        .points
                        .iter()
                        .map(|p| Vec3::new(p.x as f32, p.y as f32, p.z as f32))
                        .collect();
                    let cp = root.connection_point;
                    pts.push(Vec3::new(cp.x as f32, cp.y as f32, cp.z as f32));
                    preview_wire(&pts)
                })
            })
            .collect()
    }
}

// ── MLEADERALIGN ─────────────────────────────────────────────────────────────

pub const ICON_ALIGN: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/mleader_align.svg"));

pub fn tool_align() -> ToolDef {
    ToolDef {
        id: "MLEADERALIGN",
        label: "Align",
        icon: ICON_ALIGN,
        event: ModuleEvent::Command("MLEADERALIGN".to_string()),
    }
}

/// How MLEADERALIGN spaces the multileaders it lines up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MLeaderAlignMode {
    /// Keep each content where it falls along the line (its projection).
    UseCurrent,
    /// Spread the contents evenly between two points.
    Distribute,
    /// Turn each leader's last segment parallel to the one aligned to.
    Parallel,
    /// Stack the contents along the line this far apart.
    Spacing,
}

/// The align mode and spacing carry over to the next MLEADERALIGN.
static ALIGN_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static ALIGN_SPACING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn align_mode() -> MLeaderAlignMode {
    match ALIGN_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => MLeaderAlignMode::Distribute,
        2 => MLeaderAlignMode::Parallel,
        3 => MLeaderAlignMode::Spacing,
        _ => MLeaderAlignMode::UseCurrent,
    }
}

fn align_spacing() -> f64 {
    f64::from_bits(ALIGN_SPACING.load(std::sync::atomic::Ordering::Relaxed))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AlignStep {
    Gathering,
    AlignTo,
    Direction,
    Options,
    Spacing,
    SecondPoint,
}

pub struct MLeaderAlignCommand {
    step: AlignStep,
    handles: Vec<Handle>,
    reference: Option<Handle>,
    picked: Option<EntityType>,
    first: Option<DVec3>,
    mode: MLeaderAlignMode,
    spacing: f64,
}

impl MLeaderAlignCommand {
    pub fn new() -> Self {
        Self {
            step: AlignStep::Gathering,
            handles: Vec::new(),
            reference: None,
            picked: None,
            first: None,
            mode: align_mode(),
            spacing: align_spacing(),
        }
    }

    fn mode_line(&self) -> String {
        let mode = match self.mode {
            MLeaderAlignMode::UseCurrent => crate::tr!("mleader-align", "mode-current"),
            MLeaderAlignMode::Distribute => crate::tr!("mleader-align", "mode-distribute"),
            MLeaderAlignMode::Parallel => crate::tr!("mleader-align", "mode-parallel"),
            MLeaderAlignMode::Spacing => {
                crate::tr!("mleader-align", "mode-spacing", value = format!("{:.6}", self.spacing))
            }
        };
        crate::tr!("mleader-align", "current-mode", mode = mode)
    }

    /// The step after the selection or an option: where the mode starts.
    fn mode_step(&self) -> AlignStep {
        AlignStep::AlignTo
    }

    fn set_mode(&mut self, mode: MLeaderAlignMode) {
        self.mode = mode;
        ALIGN_MODE.store(mode as u8, std::sync::atomic::Ordering::Relaxed);
    }

    fn result(&self, from: DVec3, to: DVec3) -> CmdResult {
        CmdResult::AlignMLeaders {
            handles: self.handles.clone(),
            reference: self.reference,
            mode: self.mode,
            spacing: self.spacing,
            from,
            to,
        }
    }
}

impl CadCommand for MLeaderAlignCommand {
    fn name(&self) -> &'static str {
        "MLEADERALIGN"
    }

    fn prompt(&self) -> String {
        match self.step {
            AlignStep::Gathering => crate::tr!("mleader-align", "select"),
            AlignStep::AlignTo if self.mode == MLeaderAlignMode::Distribute => {
                crate::tr!("mleader-align", "first-point")
            }
            AlignStep::AlignTo => crate::tr!("mleader-align", "align-to"),
            AlignStep::Direction => crate::tr!("mleader-align", "direction"),
            AlignStep::SecondPoint => crate::tr!("mleader-align", "second-point"),
            AlignStep::Spacing => crate::tr!(
                "mleader-align",
                "spacing",
                value = format!("{:.6}", self.spacing)
            ),
            AlignStep::Options => {
                let current = match self.mode {
                    MLeaderAlignMode::UseCurrent => crate::tr!("mleader-align", "opt-current"),
                    MLeaderAlignMode::Distribute => crate::tr!("mleader-align", "opt-distribute"),
                    MLeaderAlignMode::Parallel => crate::tr!("mleader-align", "opt-parallel"),
                    MLeaderAlignMode::Spacing => crate::tr!("mleader-align", "opt-spacing"),
                };
                crate::tr!("mleader-align", "options", current = current)
            }
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            AlignStep::AlignTo => vec![CmdOption::new("Options", "O")],
            AlignStep::Options => vec![
                CmdOption::new("Distribute", "D"),
                CmdOption::new("make leader segments Parallel", "P"),
                CmdOption::new("specify Spacing", "S"),
                CmdOption::new("Use current spacing", "U"),
            ],
            _ => Vec::new(),
        }
    }

    fn is_selection_gathering(&self) -> bool {
        self.step == AlignStep::Gathering
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        Some(|entity| matches!(entity, EntityType::MultiLeader(_)))
    }

    fn selection_entity_filter_reports(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        // Each pick adds to the set; Enter ends the selection.
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == AlignStep::AlignTo && self.mode != MLeaderAlignMode::Distribute
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn typed_point_picks_entity(&self) -> bool {
        self.needs_entity_pick()
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        match self.picked.take() {
            Some(EntityType::MultiLeader(_)) if !handle.is_null() => {
                self.reference = Some(handle);
                if self.mode == MLeaderAlignMode::Parallel {
                    return self.result(pt, pt);
                }
                self.step = AlignStep::Direction;
                CmdResult::NeedPoint
            }
            Some(_) => CmdResult::ReportError(crate::tr!("mleader-edit", "not-multileader")),
            None => CmdResult::NeedPoint,
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            AlignStep::Options | AlignStep::Spacing => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == AlignStep::AlignTo
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let key = text.trim().trim_start_matches('_').to_ascii_uppercase();
        match self.step {
            AlignStep::AlignTo => match key.as_str() {
                "O" | "OPTIONS" => {
                    self.step = AlignStep::Options;
                    Some(CmdResult::NeedPoint)
                }
                _ => None,
            },
            AlignStep::Options => {
                match key.as_str() {
                    "D" | "DISTRIBUTE" => self.set_mode(MLeaderAlignMode::Distribute),
                    "P" | "PARALLEL" => self.set_mode(MLeaderAlignMode::Parallel),
                    "S" | "SPACING" => {
                        self.step = AlignStep::Spacing;
                        return Some(CmdResult::NeedPoint);
                    }
                    "U" | "USE" | "USE CURRENT SPACING" => {
                        self.set_mode(MLeaderAlignMode::UseCurrent)
                    }
                    _ => {
                        return Some(CmdResult::ReportError(
                            t!("Invalid option keyword.").into_owned(),
                        ))
                    }
                }
                self.step = self.mode_step();
                Some(CmdResult::NeedPoint)
            }
            AlignStep::Spacing => {
                let value = crate::entities::common::parse_typed_length(text.trim())
                    .filter(|v| *v >= 0.0)?;
                self.spacing = value;
                ALIGN_SPACING.store(value.to_bits(), std::sync::atomic::Ordering::Relaxed);
                self.set_mode(MLeaderAlignMode::Spacing);
                self.step = self.mode_step();
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            AlignStep::AlignTo if self.mode == MLeaderAlignMode::Distribute => {
                self.first = Some(pt);
                self.step = AlignStep::SecondPoint;
                CmdResult::NeedPoint
            }
            AlignStep::SecondPoint => {
                let first = self.first.unwrap_or(pt);
                self.result(first, pt)
            }
            AlignStep::Direction => self.result(pt, pt),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            AlignStep::Gathering if self.handles.is_empty() => CmdResult::Cancel,
            AlignStep::Gathering => {
                self.step = self.mode_step();
                CmdResult::ReportMeasurement(self.mode_line())
            }
            AlignStep::Options => {
                self.step = self.mode_step();
                CmdResult::NeedPoint
            }
            AlignStep::Spacing => {
                self.set_mode(MLeaderAlignMode::Spacing);
                self.step = self.mode_step();
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        match self.step {
            AlignStep::SecondPoint => {
                let first = self.first?;
                Some(preview_wire(&[first.as_vec3(), pt.as_vec3()]))
            }
            _ => None,
        }
    }
}

// ── MLEADERCOLLECT ────────────────────────────────────────────────────────────

pub const ICON_COLLECT: IconKind =
    IconKind::Svg(include_bytes!("../../../assets/icons/mleader_collect.svg"));

pub fn tool_collect() -> ToolDef {
    ToolDef {
        id: "MLEADERCOLLECT",
        label: "Collect",
        icon: ICON_COLLECT,
        event: ModuleEvent::Command("MLEADERCOLLECT".to_string()),
    }
}

/// How MLEADERCOLLECT lays the collected blocks out.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MLeaderCollectLayout {
    Horizontal,
    Vertical,
    /// Rows no wider than the width (0: one row).
    Wrap(f64),
    /// Rows of this many blocks.
    WrapNumber(u32),
}

/// The layout carries over to the next MLEADERCOLLECT.
static COLLECT_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static COLLECT_WRAP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static COLLECT_NUMBER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

fn collect_layout() -> MLeaderCollectLayout {
    match COLLECT_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => MLeaderCollectLayout::Vertical,
        2 => MLeaderCollectLayout::Wrap(f64::from_bits(
            COLLECT_WRAP.load(std::sync::atomic::Ordering::Relaxed),
        )),
        3 => MLeaderCollectLayout::WrapNumber(
            COLLECT_NUMBER.load(std::sync::atomic::Ordering::Relaxed),
        ),
        _ => MLeaderCollectLayout::Horizontal,
    }
}

fn store_collect_layout(layout: MLeaderCollectLayout) {
    use std::sync::atomic::Ordering;
    let mode = match layout {
        MLeaderCollectLayout::Horizontal => 0,
        MLeaderCollectLayout::Vertical => 1,
        MLeaderCollectLayout::Wrap(width) => {
            COLLECT_WRAP.store(width.to_bits(), Ordering::Relaxed);
            2
        }
        MLeaderCollectLayout::WrapNumber(count) => {
            COLLECT_NUMBER.store(count, Ordering::Relaxed);
            3
        }
    };
    COLLECT_MODE.store(mode, Ordering::Relaxed);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CollectStep {
    Gathering,
    Location,
    WrapWidth,
    WrapNumber,
}

pub struct MLeaderCollectCommand {
    step: CollectStep,
    handles: Vec<Handle>,
    layout: MLeaderCollectLayout,
}

impl MLeaderCollectCommand {
    pub fn new() -> Self {
        Self {
            step: CollectStep::Gathering,
            handles: Vec::new(),
            layout: collect_layout(),
        }
    }
}

impl CadCommand for MLeaderCollectCommand {
    fn name(&self) -> &'static str {
        "MLEADERCOLLECT"
    }

    fn prompt(&self) -> String {
        match self.step {
            CollectStep::Gathering => crate::tr!("mleader-align", "select"),
            CollectStep::Location => {
                let current = match self.layout {
                    MLeaderCollectLayout::Horizontal => {
                        crate::tr!("mleader-collect", "horizontal")
                    }
                    MLeaderCollectLayout::Vertical => crate::tr!("mleader-collect", "vertical"),
                    MLeaderCollectLayout::Wrap(_) | MLeaderCollectLayout::WrapNumber(_) => {
                        crate::tr!("mleader-collect", "wrap")
                    }
                };
                crate::tr!("mleader-collect", "location", current = current)
            }
            CollectStep::WrapWidth => {
                let current = match self.layout {
                    MLeaderCollectLayout::WrapNumber(_) => crate::tr!("mleader-collect", "number"),
                    MLeaderCollectLayout::Wrap(width) => format!("{width:.6}"),
                    _ => format!(
                        "{:.6}",
                        f64::from_bits(COLLECT_WRAP.load(std::sync::atomic::Ordering::Relaxed))
                    ),
                };
                crate::tr!("mleader-collect", "wrap-width", value = current)
            }
            CollectStep::WrapNumber => crate::tr!(
                "mleader-collect",
                "number-per-row",
                value = COLLECT_NUMBER.load(std::sync::atomic::Ordering::Relaxed).to_string()
            ),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            CollectStep::Location => vec![
                CmdOption::new("Vertical", "V"),
                CmdOption::new("Horizontal", "H"),
                CmdOption::new("Wrap", "W"),
            ],
            CollectStep::WrapWidth => vec![CmdOption::new("Number", "N")],
            _ => Vec::new(),
        }
    }

    fn is_selection_gathering(&self) -> bool {
        self.step == CollectStep::Gathering
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        Some(|entity| {
            matches!(entity, EntityType::MultiLeader(ml)
                if ml.content_type == codec::entities::LeaderContentType::Block)
        })
    }

    fn selection_entity_filter_reports(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        // Each pick adds to the set; Enter ends the selection.
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            CollectStep::WrapWidth | CollectStep::WrapNumber => InputKind::SingleToken,
            _ => InputKind::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.step == CollectStep::Location
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let key = text.trim().trim_start_matches('_').to_ascii_uppercase();
        match self.step {
            CollectStep::Location => {
                self.layout = match key.as_str() {
                    "V" | "VERTICAL" => MLeaderCollectLayout::Vertical,
                    "H" | "HORIZONTAL" => MLeaderCollectLayout::Horizontal,
                    "W" | "WRAP" => {
                        self.step = CollectStep::WrapWidth;
                        return Some(CmdResult::NeedPoint);
                    }
                    _ => return None,
                };
                store_collect_layout(self.layout);
                Some(CmdResult::NeedPoint)
            }
            CollectStep::WrapWidth if matches!(key.as_str(), "N" | "NUMBER") => {
                self.step = CollectStep::WrapNumber;
                Some(CmdResult::NeedPoint)
            }
            CollectStep::WrapNumber => {
                let count = text.trim().parse::<u32>().ok().filter(|n| *n >= 1)?;
                self.layout = MLeaderCollectLayout::WrapNumber(count);
                store_collect_layout(self.layout);
                self.step = CollectStep::Location;
                Some(CmdResult::NeedPoint)
            }
            CollectStep::WrapWidth => {
                let width = crate::entities::common::parse_typed_length(text.trim())
                    .filter(|v| *v >= 0.0)?;
                self.layout = MLeaderCollectLayout::Wrap(width);
                store_collect_layout(self.layout);
                self.step = CollectStep::Location;
                Some(CmdResult::NeedPoint)
            }
            CollectStep::Gathering => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step != CollectStep::Location {
            return CmdResult::NeedPoint;
        }
        CmdResult::CollectMLeaders {
            handles: self.handles.clone(),
            point: pt,
            layout: self.layout,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            // Enter with nothing picked ends the command; otherwise it moves
            // on to the location.
            CollectStep::Gathering => {
                if self.handles.is_empty() {
                    return CmdResult::Cancel;
                }
                self.step = CollectStep::Location;
                return CmdResult::NeedPoint;
            }
            // Enter keeps the wrap rule the prompt offered.
            CollectStep::WrapWidth => {
                if !matches!(
                    self.layout,
                    MLeaderCollectLayout::Wrap(_) | MLeaderCollectLayout::WrapNumber(_)
                ) {
                    self.layout = MLeaderCollectLayout::Wrap(f64::from_bits(
                        COLLECT_WRAP.load(std::sync::atomic::Ordering::Relaxed),
                    ));
                }
                store_collect_layout(self.layout);
                self.step = CollectStep::Location;
                return CmdResult::NeedPoint;
            }
            CollectStep::WrapNumber => {
                self.layout = MLeaderCollectLayout::WrapNumber(
                    COLLECT_NUMBER.load(std::sync::atomic::Ordering::Relaxed),
                );
                store_collect_layout(self.layout);
                self.step = CollectStep::Location;
                return CmdResult::NeedPoint;
            }
            _ => {}
        }
        CmdResult::Cancel
    }

    fn on_mouse_move(&mut self, _pt: DVec3) -> Option<WireModel> {
        None
    }
}

fn preview_wire(pts: &[Vec3]) -> WireModel {
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
        name: "mleader_edit_preview".into(),
        points: pts.iter().map(|p| [p.x, p.y, p.z]).collect(),
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

        ..Default::default()
    }
}

fn point_segment_distance_xy(point: Vector3, start: Vector3, end: Vector3) -> f64 {
    closest_point(
        &Curve::Line(Line {
            start: [start.x, start.y],
            end: [end.x, end.y],
        }),
        [point.x, point.y],
    )
    .distance
}

/// How far `point` is from a multileader's leader lines and landings, the
/// parts a pick box catches.
pub(crate) fn mleader_pick_distance(ml: &MultiLeader, point: Vector3) -> f64 {
    let mut distance = f64::INFINITY;
    for root in &ml.context.leader_roots {
        let d = &root.direction;
        let len = (d.x * d.x + d.y * d.y).sqrt();
        if len > 1.0e-12 && ml.enable_landing && ml.enable_dogleg {
            let reach = root.landing_distance.max(0.0) / len;
            let end = Vector3::new(
                root.connection_point.x + d.x * reach,
                root.connection_point.y + d.y * reach,
                root.connection_point.z,
            );
            distance = distance.min(point_segment_distance_xy(point, root.connection_point, end));
        }
        for line in &root.lines {
            for segment in line.points.windows(2) {
                distance = distance.min(point_segment_distance_xy(point, segment[0], segment[1]));
            }
            if let Some(last) = line.points.last().copied() {
                distance =
                    distance.min(point_segment_distance_xy(point, last, root.connection_point));
            }
        }
    }
    distance
}

// Silence unused-import warning for MultiLeader and LeaderLine if not used in all paths
fn _uses_ml_types(_ml: &MultiLeader, _ll: &LeaderLine) {}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["MLEADEREDIT", "AIMLEADEREDITADD", "AIMLEADEREDITREMOVE"]
}); // MLeaderEditCommand
inventory::submit!(crate::command::CommandRegistration {
    names: &["MLEADERALIGN"]
}); // MLeaderAlignCommand
inventory::submit!(crate::command::CommandRegistration {
    names: &["MLEADERCOLLECT"]
}); // MLeaderCollectCommand
