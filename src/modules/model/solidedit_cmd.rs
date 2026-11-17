//! SOLIDEDIT, IMPRINT and XEDGES: editing a solid's faces, edges and body.

use document::{EntityType, Handle};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, FacePick, InputKind, SelectionEntity, SolidEditAction,
    SolidEditRequest,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Extrude,
    Move,
    Rotate,
    Offset,
    Taper,
    Delete,
    Copy,
    Color,
    EdgeCopy,
    EdgeColor,
    Imprint,
    Separate,
    Clean,
    Check,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Root,
    FaceMenu,
    EdgeMenu,
    BodyMenu,
    AddPicks,
    RemovePicks,
    Height,
    ExtrudeTaper,
    BasePoint,
    SecondPoint,
    AxisFirst,
    AxisSecond,
    AxisOrigin,
    Angle,
    Distance,
    TaperBase,
    TaperAxis,
    TaperAngle,
    Color,
    Solid,
    ImprintObject,
    DeleteSource,
}

pub struct SolidEditCommand {
    entry_name: &'static str,
    step: Step,
    op: Option<Op>,
    target: Option<Handle>,
    picks: Vec<FacePick>,
    base: Option<DVec3>,
    axis: Option<DVec3>,
    height: f64,
    tool: Option<Handle>,
}

impl SolidEditCommand {
    pub fn new() -> Self {
        Self {
            entry_name: "SOLIDEDIT",
            step: Step::Root,
            op: None,
            target: None,
            picks: Vec::new(),
            base: None,
            axis: None,
            height: 0.0,
            tool: None,
        }
    }

    /// IMPRINT: SOLIDEDIT's body imprint as a command of its own.
    pub fn imprint() -> Self {
        let mut command = Self::new();
        command.entry_name = "IMPRINT";
        command.op = Some(Op::Imprint);
        command.step = Step::Solid;
        command
    }

    /// Feeds the option keywords a ribbon button names (`FACE EXTRUDE`).
    pub fn with_options(mut self, options: &str) -> Self {
        for word in options.split_whitespace() {
            let _ = self.on_text_input(word);
        }
        self
    }

    fn edges(&self) -> bool {
        matches!(self.op, Some(Op::EdgeCopy | Op::EdgeColor))
    }

    fn choose(&mut self, op: Op) -> CmdResult {
        self.op = Some(op);
        self.step = match op {
            Op::Imprint | Op::Separate | Op::Clean | Op::Check => Step::Solid,
            _ => Step::AddPicks,
        };
        CmdResult::NeedPoint
    }

    fn finish(&mut self, action: SolidEditAction) -> CmdResult {
        let Some(handle) = self.target else {
            return CmdResult::Cancel;
        };
        CmdResult::SolidEdit(Box::new(SolidEditRequest {
            handle,
            picks: std::mem::take(&mut self.picks),
            action,
        }))
    }

    /// The step after the faces or edges are picked.
    fn after_picks(&mut self) -> CmdResult {
        self.step = match self.op {
            Some(Op::Extrude) => Step::Height,
            Some(Op::Move | Op::Copy | Op::EdgeCopy) => Step::BasePoint,
            Some(Op::Rotate) => Step::AxisFirst,
            Some(Op::Offset) => Step::Distance,
            Some(Op::Taper) => Step::TaperBase,
            Some(Op::Color | Op::EdgeColor) => Step::Color,
            Some(Op::Delete) => return self.finish(SolidEditAction::DeleteFaces),
            _ => return CmdResult::Cancel,
        };
        CmdResult::NeedPoint
    }

    fn after_second_point(&mut self, to: DVec3) -> CmdResult {
        let from = self.base.unwrap_or(DVec3::ZERO);
        self.finish(match self.op {
            Some(Op::Move) => SolidEditAction::MoveFaces { from, to },
            Some(Op::Copy) => SolidEditAction::CopyFaces { from, to },
            _ => SolidEditAction::CopyEdges { from, to },
        })
    }

    fn number(text: &str) -> Option<f64> {
        crate::entities::common::parse_typed_length(text).filter(|value| value.is_finite())
    }

    fn numeric_error() -> CmdResult {
        CmdResult::ReportError(crate::t!("Requires a numeric value.").into_owned())
    }
}

fn parse_color(text: &str) -> Option<document::types::Color> {
    let text = text.trim();
    let parts = text
        .split(',')
        .map(|part| part.trim().parse::<u8>().ok())
        .collect::<Option<Vec<_>>>();
    if let Some([r, g, b]) = parts.as_deref() {
        return Some(document::types::Color::Rgb { r: *r, g: *g, b: *b });
    }
    crate::ui::window::attman_dialog::parse_color(text)
}

impl CadCommand for SolidEditCommand {
    fn name(&self) -> &'static str {
        self.entry_name
    }

    fn prompt(&self) -> String {
        let text = match self.step {
            Step::Root => "Enter a solid editing option [Face/Edge/Body/eXit] <eXit>:",
            Step::FaceMenu => {
                "Enter a face editing option [Extrude/Move/Rotate/Offset/Taper/Delete/Copy/coLor/eXit] <eXit>:"
            }
            Step::EdgeMenu => "Enter an edge editing option [Copy/coLor/eXit] <eXit>:",
            Step::BodyMenu => {
                "Enter a body editing option [Imprint/seParate solids/Shell/cLean/Check/eXit] <eXit>:"
            }
            Step::AddPicks if self.edges() => "Select edges or [Undo/Remove]:",
            Step::RemovePicks if self.edges() => "Remove edges or [Undo/Add]:",
            Step::AddPicks => "Select faces or [Undo/Remove/ALL]:",
            Step::RemovePicks => "Remove faces or [Undo/Add/ALL]:",
            Step::Height => "Specify height of extrusion:",
            Step::ExtrudeTaper => "Specify angle of taper for extrusion <0>:",
            Step::BasePoint => "Specify a base point or displacement:",
            Step::SecondPoint => "Specify a second point of displacement:",
            Step::AxisFirst => "Specify an axis point or [Xaxis/Yaxis/Zaxis] <2points>:",
            Step::AxisSecond => "Specify the second point on the rotation axis:",
            Step::AxisOrigin => "Specify the origin of the rotation <0,0,0>:",
            Step::Angle => "Specify a rotation angle:",
            Step::Distance => "Specify the offset distance:",
            Step::TaperBase => "Specify the base point:",
            Step::TaperAxis => "Specify another point along the axis of tapering:",
            Step::TaperAngle => "Specify the taper angle:",
            Step::Color => "Enter new color [ByLayer/color number/R,G,B] <ByLayer>:",
            Step::Solid => "Select a 3D solid:",
            Step::ImprintObject => "Select an object to imprint:",
            Step::DeleteSource => "Delete the source object [Yes/No] <N>:",
        };
        crate::t!(text).into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        let keys: &[(&'static str, &'static str)] = match self.step {
            Step::Root => &[("Face", "FACE"), ("Edge", "EDGE"), ("Body", "BODY"), ("eXit", "EXIT")],
            Step::FaceMenu => &[
                ("Extrude", "EXTRUDE"),
                ("Move", "MOVE"),
                ("Rotate", "ROTATE"),
                ("Offset", "OFFSET"),
                ("Taper", "TAPER"),
                ("Delete", "DELETE"),
                ("Copy", "COPY"),
                ("coLor", "COLOR"),
                ("eXit", "EXIT"),
            ],
            Step::EdgeMenu => &[("Copy", "COPY"), ("coLor", "COLOR"), ("eXit", "EXIT")],
            Step::BodyMenu => &[
                ("Imprint", "IMPRINT"),
                ("seParate solids", "SEPARATE"),
                ("Shell", "SHELL"),
                ("cLean", "CLEAN"),
                ("Check", "CHECK"),
                ("eXit", "EXIT"),
            ],
            Step::AddPicks if self.edges() => &[("Undo", "UNDO"), ("Remove", "REMOVE")],
            Step::RemovePicks if self.edges() => &[("Undo", "UNDO"), ("Add", "ADD")],
            Step::AddPicks => &[("Undo", "UNDO"), ("Remove", "REMOVE"), ("ALL", "ALL")],
            Step::RemovePicks => &[("Undo", "UNDO"), ("Add", "ADD"), ("ALL", "ALL")],
            Step::AxisFirst => &[("Xaxis", "XAXIS"), ("Yaxis", "YAXIS"), ("Zaxis", "ZAXIS")],
            Step::Color => &[("ByLayer", "BYLAYER")],
            Step::DeleteSource => &[("Yes", "YES"), ("No", "NO")],
            _ => &[],
        };
        keys.iter()
            .map(|(label, keyword)| CmdOption::new(label, keyword))
            .collect()
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::BasePoint
            | Step::SecondPoint
            | Step::AxisFirst
            | Step::AxisSecond
            | Step::AxisOrigin
            | Step::TaperBase
            | Step::TaperAxis
            | Step::Solid
            | Step::ImprintObject => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(
            self.step,
            Step::AddPicks | Step::RemovePicks | Step::Solid | Step::ImprintObject
        )
    }

    fn entity_pick_includes_fills(&self) -> bool {
        true
    }

    fn entity_pick_uses_surface_point(&self) -> bool {
        matches!(self.step, Step::AddPicks | Step::RemovePicks)
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            Step::AddPicks | Step::RemovePicks => {
                if self.target.is_none_or(|target| target == handle) {
                    self.target = Some(handle);
                    self.picks.push(if self.step == Step::AddPicks {
                        FacePick::Add(point)
                    } else {
                        FacePick::Remove(point)
                    });
                }
                CmdResult::NeedPoint
            }
            Step::Solid => {
                self.target = Some(handle);
                match self.op {
                    Some(Op::Imprint) => {
                        self.step = Step::ImprintObject;
                        CmdResult::NeedPoint
                    }
                    Some(Op::Separate) => self.finish(SolidEditAction::Separate),
                    Some(Op::Clean) => self.finish(SolidEditAction::Clean),
                    Some(Op::Check) => self.finish(SolidEditAction::Check),
                    _ => CmdResult::Cancel,
                }
            }
            Step::ImprintObject if self.target != Some(handle) => {
                self.tool = Some(handle);
                self.step = Step::DeleteSource;
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::BasePoint => {
                self.base = Some(point);
                self.step = Step::SecondPoint;
            }
            Step::SecondPoint => return self.after_second_point(point),
            Step::AxisFirst => {
                self.base = Some(point);
                self.step = Step::AxisSecond;
            }
            Step::AxisSecond => {
                let from = self.base.unwrap_or(DVec3::ZERO);
                if point.distance(from) <= 1e-12 {
                    return CmdResult::NeedPoint;
                }
                self.axis = Some(point - from);
                self.step = Step::Angle;
            }
            Step::AxisOrigin => {
                self.base = Some(point);
                self.step = Step::Angle;
            }
            Step::TaperBase => {
                self.base = Some(point);
                self.step = Step::TaperAxis;
            }
            Step::TaperAxis => {
                let from = self.base.unwrap_or(DVec3::ZERO);
                if point.distance(from) <= 1e-12 {
                    return CmdResult::NeedPoint;
                }
                self.axis = Some(point);
                self.step = Step::TaperAngle;
            }
            _ => {}
        }
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        let keyword = value.to_ascii_uppercase();
        let keyword = keyword.as_str();
        let result = match self.step {
            Step::Root => match keyword {
                "F" | "FACE" => {
                    self.step = Step::FaceMenu;
                    CmdResult::NeedPoint
                }
                "E" | "EDGE" => {
                    self.step = Step::EdgeMenu;
                    CmdResult::NeedPoint
                }
                "B" | "BODY" => {
                    self.step = Step::BodyMenu;
                    CmdResult::NeedPoint
                }
                "X" | "EXIT" => CmdResult::Cancel,
                _ => CmdResult::NeedPoint,
            },
            Step::FaceMenu => match keyword {
                "E" | "EXTRUDE" => self.choose(Op::Extrude),
                "M" | "MOVE" => self.choose(Op::Move),
                "R" | "ROTATE" => self.choose(Op::Rotate),
                "O" | "OFFSET" => self.choose(Op::Offset),
                "T" | "TAPER" => self.choose(Op::Taper),
                "D" | "DELETE" => self.choose(Op::Delete),
                "C" | "COPY" => self.choose(Op::Copy),
                "L" | "COLOR" => self.choose(Op::Color),
                "X" | "EXIT" => CmdResult::Cancel,
                _ => CmdResult::NeedPoint,
            },
            Step::EdgeMenu => match keyword {
                "C" | "COPY" => self.choose(Op::EdgeCopy),
                "L" | "COLOR" => self.choose(Op::EdgeColor),
                "X" | "EXIT" => CmdResult::Cancel,
                _ => CmdResult::NeedPoint,
            },
            Step::BodyMenu => match keyword {
                "I" | "IMPRINT" => self.choose(Op::Imprint),
                "P" | "SEPARATE" => self.choose(Op::Separate),
                "S" | "SHELL" => CmdResult::Dispatch("SHELL".into()),
                "L" | "CLEAN" => self.choose(Op::Clean),
                "C" | "CHECK" => self.choose(Op::Check),
                "X" | "EXIT" => CmdResult::Cancel,
                _ => CmdResult::NeedPoint,
            },
            Step::AddPicks | Step::RemovePicks => match keyword {
                "U" | "UNDO" => {
                    self.picks.pop();
                    CmdResult::NeedPoint
                }
                "R" | "REMOVE" => {
                    self.step = Step::RemovePicks;
                    CmdResult::NeedPoint
                }
                "A" | "ADD" => {
                    self.step = Step::AddPicks;
                    CmdResult::NeedPoint
                }
                "ALL" if !self.edges() && self.target.is_some() => {
                    self.picks.push(FacePick::All);
                    CmdResult::NeedPoint
                }
                _ => CmdResult::NeedPoint,
            },
            Step::Height => match Self::number(value) {
                Some(height) if height != 0.0 => {
                    self.height = height;
                    self.step = Step::ExtrudeTaper;
                    CmdResult::NeedPoint
                }
                _ => Self::numeric_error(),
            },
            Step::ExtrudeTaper => match value.parse::<f64>() {
                Ok(angle) if angle.is_finite() && angle.abs() < 90.0 => {
                    self.finish(SolidEditAction::ExtrudeFaces {
                        height: self.height,
                        taper: angle.to_radians(),
                    })
                }
                _ => Self::numeric_error(),
            },
            Step::AxisFirst => {
                let axis = match keyword {
                    "X" | "XAXIS" => DVec3::X,
                    "Y" | "YAXIS" => DVec3::Y,
                    "Z" | "ZAXIS" => DVec3::Z,
                    _ => return Some(CmdResult::NeedPoint),
                };
                self.axis = Some(axis);
                self.step = Step::AxisOrigin;
                CmdResult::NeedPoint
            }
            Step::Angle => match value.parse::<f64>() {
                Ok(angle) if angle.is_finite() => self.finish(SolidEditAction::RotateFaces {
                    origin: self.base.unwrap_or(DVec3::ZERO),
                    axis: self.axis.unwrap_or(DVec3::Z),
                    angle: angle.to_radians(),
                }),
                _ => Self::numeric_error(),
            },
            Step::Distance => match Self::number(value) {
                Some(distance) if distance != 0.0 => {
                    self.finish(SolidEditAction::OffsetFaces { distance })
                }
                _ => Self::numeric_error(),
            },
            Step::TaperAngle => match value.parse::<f64>() {
                Ok(angle) if angle.is_finite() && angle.abs() < 90.0 => {
                    self.finish(SolidEditAction::TaperFaces {
                        base: self.base.unwrap_or(DVec3::ZERO),
                        toward: self.axis.unwrap_or(DVec3::Z),
                        angle: angle.to_radians(),
                    })
                }
                _ => Self::numeric_error(),
            },
            Step::Color => match parse_color(value) {
                Some(color) if self.op == Some(Op::EdgeColor) => {
                    self.finish(SolidEditAction::ColorEdges(color))
                }
                Some(color) => self.finish(SolidEditAction::ColorFaces(color)),
                None => CmdResult::ReportError(crate::t!("Invalid color.").into_owned()),
            },
            Step::DeleteSource => match keyword {
                "Y" | "YES" => self.imprint_with(true),
                "N" | "NO" => self.imprint_with(false),
                _ => CmdResult::NeedPoint,
            },
            _ => return None,
        };
        Some(result)
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Root | Step::FaceMenu | Step::EdgeMenu | Step::BodyMenu => CmdResult::Cancel,
            Step::AddPicks | Step::RemovePicks if self.picks.is_empty() => CmdResult::NeedPoint,
            Step::AddPicks | Step::RemovePicks => self.after_picks(),
            Step::ExtrudeTaper => self.finish(SolidEditAction::ExtrudeFaces {
                height: self.height,
                taper: 0.0,
            }),
            // A base point alone is a displacement from the origin.
            Step::SecondPoint => {
                let displacement = self.base.unwrap_or(DVec3::ZERO);
                self.base = Some(DVec3::ZERO);
                self.after_second_point(displacement)
            }
            Step::AxisFirst => CmdResult::NeedPoint,
            Step::AxisOrigin => {
                self.base = Some(DVec3::ZERO);
                self.step = Step::Angle;
                CmdResult::NeedPoint
            }
            Step::Color => match self.op {
                Some(Op::EdgeColor) => {
                    self.finish(SolidEditAction::ColorEdges(document::types::Color::ByLayer))
                }
                _ => self.finish(SolidEditAction::ColorFaces(document::types::Color::ByLayer)),
            },
            Step::DeleteSource => self.imprint_with(false),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        matches!(self.step, Step::AddPicks | Step::RemovePicks).then(|| {
            self.picks.pop();
            CmdResult::NeedPoint
        })
    }
}

impl SolidEditCommand {
    fn imprint_with(&mut self, delete_tool: bool) -> CmdResult {
        let Some(tool) = self.tool else {
            return CmdResult::Cancel;
        };
        self.finish(SolidEditAction::Imprint { tool, delete_tool })
    }
}

/// XEDGES: every edge of the selected solids, regions and surfaces drawn as
/// lines, arcs, circles, ellipses and splines.
pub struct ExtractEdgesCommand {
    handles: Vec<Handle>,
}

fn has_edges(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Solid3D(_) | EntityType::Region(_) | EntityType::Surface(_) | EntityType::Body(_)
    )
}

impl ExtractEdgesCommand {
    pub fn new(preselection: Vec<(Handle, EntityType)>) -> Self {
        Self {
            handles: preselection
                .into_iter()
                .filter_map(|(handle, entity)| has_edges(&entity).then_some(handle))
                .collect(),
        }
    }

    /// The preselection is the whole answer when there is one.
    pub fn ready(&self) -> Option<CmdResult> {
        (!self.handles.is_empty()).then(|| CmdResult::ExtractEdges(self.handles.clone()))
    }
}

impl CadCommand for ExtractEdgesCommand {
    fn name(&self) -> &'static str {
        "XEDGES"
    }

    fn prompt(&self) -> String {
        crate::t!("Select objects:").into_owned()
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.handles.is_empty() {
            CmdResult::Cancel
        } else {
            CmdResult::ExtractEdges(std::mem::take(&mut self.handles))
        }
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn selection_forces_add(&self) -> bool {
        true
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        self.handles = entities
            .into_iter()
            .filter_map(|entry| has_edges(&entry.entity).then_some(entry.handle))
            .collect();
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        self.on_enter()
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["SOLIDEDIT", "IMPRINT", "XEDGES"]
});
