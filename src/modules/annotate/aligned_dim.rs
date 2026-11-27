// DIMALIGNED command — aligned dimension (measures true distance between two points).

use document::types::Handle;
use document::EntityType;
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionAssociationInput, DimensionPreview, InputKind,
    WorkingPlane,
};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use crate::t;
pub(crate) use crate::entities::dimension::build::aligned::{
    aligned_dimension_entity, dim_line_endpoints,
};

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_aligned.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIMALIGNED",
        label: "Aligned",
        icon: ICON,
        event: ModuleEvent::Command("DIMALIGNED".to_string()),
    }
}

enum Step {
    First,
    Second(DVec3),
    DimLine { p1: DVec3, p2: DVec3 },
}

pub struct AlignedDimensionCommand {
    step: Step,
    plane: WorkingPlane,
    /// Optional text that replaces the measured value (None = measurement).
    text_override: Option<String>,
    /// True while the next typed line is captured as the text override.
    awaiting_text: bool,
    /// Explicit text rotation in radians (None = follow the UCS/style).
    text_angle: Option<f64>,
    /// True while the next typed value is captured as the text angle.
    awaiting_angle: bool,
    selecting_object: bool,
    picked_entity: Option<EntityType>,
    source_handle: Option<Handle>,
    mtext_override: bool,
}

impl Default for AlignedDimensionCommand {
    fn default() -> Self {
        Self::new()
    }
}

impl AlignedDimensionCommand {
    pub fn new() -> Self {
        Self {
            step: Step::First,
            plane: WorkingPlane::default(),
            text_override: None,
            awaiting_text: false,
            text_angle: None,
            awaiting_angle: false,
            selecting_object: false,
            picked_entity: None,
            source_handle: None,
            mtext_override: false,
        }
    }

    fn build_dimension(&self, first: DVec3, second: DVec3, point: DVec3) -> EntityType {
        let first = self.plane.to_local(first);
        let second = self.plane.to_local(second);
        let point = self.plane.to_local(point);
        let mut entity = aligned_dimension_entity(first, second, point, self.text_override.clone());
        // An explicit text angle overrides the default rotation.
        if let (Some(angle), EntityType::Dimension(dimension)) = (self.text_angle, &mut entity) {
            dimension.base_mut().text_rotation = angle;
        }
        self.plane.place_entity(entity)
    }
}

impl CadCommand for AlignedDimensionCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "DIMALIGNED"
    }

    /// A dimension's point prompts refuse unusable input this way (measured).
    fn point_rejection(&self) -> Option<String> {
        Some(t!("Invalid point input.").into_owned())
    }

    fn prompt(&self) -> String {
        if self.awaiting_text {
            return if self.mtext_override {
                t!("DIMALIGNED  Enter formatted dimension text (blank = measured value):")
                    .into_owned()
            } else {
                t!("DIMALIGNED  Enter dimension text (blank = measured value):").into_owned()
            };
        }
        if self.awaiting_angle {
            return t!("DIMALIGNED  Specify text angle (degrees):").into_owned();
        }
        if self.selecting_object {
            return t!("DIMALIGNED  Select object to dimension:").into_owned();
        }
        match self.step {
            Step::First => t!(
                "DIMALIGNED  Specify first extension line origin or <select object>:"
            )
            .into_owned(),
            Step::Second(_) => t!("DIMALIGNED  Specify second extension line origin:").into_owned(),
            Step::DimLine { .. } => {
                t!("DIMALIGNED  Specify dimension line location  [Mtext/Text/Angle]:").into_owned()
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::First => {
                self.step = Step::Second(pt);
                CmdResult::NeedPoint
            }
            Step::Second(p1) => {
                if pt.distance_squared(p1) <= 1e-24 {
                    return CmdResult::NeedPoint;
                }
                self.step = Step::DimLine { p1, p2: pt };
                CmdResult::NeedPoint
            }
            Step::DimLine { p1, p2 } => CmdResult::CommitDimension {
                entity: self.build_dimension(p1, p2, pt),
                association: DimensionAssociationInput::Infer(self.source_handle),
                preserve_base_style: false,
                continue_command: false,
            },
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        // A bare Enter while entering override text/angle accepts the default.
        if self.awaiting_text {
            self.awaiting_text = false;
            return CmdResult::NeedPoint;
        }
        if self.awaiting_angle {
            self.awaiting_angle = false;
            return CmdResult::NeedPoint;
        }
        if matches!(self.step, Step::First) {
            self.selecting_object = true;
            return CmdResult::NeedPoint;
        }
        CmdResult::Cancel
    }

    /// Points and object picks may come through a paper-space viewport;
    /// the committed dimension then reports the model measurement.
    fn measures_through_viewports(&self) -> bool {
        true
    }

    fn dimension_acquired_points(&self) -> Vec<DVec3> {
        match self.step {
            Step::First => vec![],
            Step::Second(p) => vec![p],
            Step::DimLine { p1, p2 } => vec![p1, p2],
        }
    }

    fn dimension_placement_pending(&self) -> bool {
        matches!(self.step, Step::DimLine { .. })
    }

    fn input_kind(&self) -> InputKind {
        if self.awaiting_text {
            InputKind::FreeText
        } else if self.awaiting_angle {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if !matches!(self.step, Step::DimLine { .. }) || self.awaiting_text || self.awaiting_angle {
            return Vec::new();
        }
        vec![
            CmdOption::new("MText", "MTEXT"),
            CmdOption::new("Text", "TEXT"),
            CmdOption::new("Angle", "ANGLE"),
        ]
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // While entering the override text or angle it is a value, not a point
        // step.
        !self.awaiting_text && !self.awaiting_angle
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.awaiting_text {
            let t = text.trim();
            // Blank (or the "<>" placeholder) keeps the measured value.
            self.text_override = if t.is_empty() || t == "<>" {
                None
            } else {
                Some(t.to_string())
            };
            self.awaiting_text = false;
            return Some(CmdResult::NeedPoint);
        }
        if self.awaiting_angle {
            let t = text.trim();
            // Blank clears any explicit angle (follow the default again).
            self.text_angle = if t.is_empty() {
                None
            } else {
                crate::entities::common::parse_typed_angle(t)
            };
            self.awaiting_angle = false;
            return Some(CmdResult::NeedPoint);
        }
        if !matches!(self.step, Step::DimLine { .. }) {
            return None;
        }
        match text.trim().to_uppercase().as_str() {
            "T" | "TEXT" => {
                self.mtext_override = false;
                self.awaiting_text = true;
                Some(CmdResult::NeedPoint)
            }
            "M" | "MTEXT" => {
                self.mtext_override = true;
                self.awaiting_text = true;
                Some(CmdResult::NeedPoint)
            }
            "A" | "ANGLE" => {
                self.awaiting_angle = true;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        self.selecting_object
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_entity = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        let Some(entity) = self.picked_entity.take() else {
            return CmdResult::NeedPoint;
        };
        let Some((p1, p2)) = super::linear_dim::dimension_source_points(&entity, point) else {
            return CmdResult::NeedPoint;
        };
        if p1.distance_squared(p2) <= 1e-24 {
            return CmdResult::NeedPoint;
        }
        self.source_handle = Some(handle);
        self.selecting_object = false;
        self.step = Step::DimLine { p1, p2 };
        CmdResult::NeedPoint
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        let (p1, p2) = match self.step {
            Step::First => return None,
            Step::Second(p1) => (p1, pt),
            Step::DimLine { p1, p2 } => {
                let p1 = self.plane.to_local(p1);
                let p2 = self.plane.to_local(p2);
                let pt = self.plane.to_local(pt);
                let points = preview_aligned(p1, p2, pt)
                    .into_iter()
                    .map(|point| {
                        if point.x.is_nan() {
                            point
                        } else {
                            self.plane.to_world(point)
                        }
                    })
                    .collect();
                return Some(preview_wire(points));
            }
        };
        Some(preview_wire(vec![p1, p2]))
    }

    fn dyn_spec(&self) -> Option<crate::command::DynSpec> {
        if !matches!(self.step, Step::DimLine { .. }) || self.awaiting_text || self.awaiting_angle {
            return None;
        }
        Some(crate::command::DynSpec {
            anchor: crate::command::DynAnchor::LastPoint,
            fields: Vec::new(),
            guide: crate::command::DynGuide::None,
            ref_point: None,
        })
    }

    fn dimension_preview(&self, cursor: DVec3) -> Option<Vec<DimensionPreview>> {
        if self.selecting_object {
            return None;
        }
        let (first, second) = match self.step {
            Step::First => return None,
            Step::Second(first) => (first, cursor),
            Step::DimLine { p1, p2 } => (p1, p2),
        };
        if first.distance_squared(second) <= 1e-24 {
            return Some(Vec::new());
        }
        Some(vec![DimensionPreview::current_style(
            self.build_dimension(first, second, cursor),
        )])
    }
}

fn preview_aligned(p1: DVec3, p2: DVec3, dim_pt: DVec3) -> Vec<DVec3> {
    let (d1, d2) = dim_line_endpoints(p1, p2, dim_pt);
    let axis = (d2 - d1).normalize_or_zero();
    let perp = DVec3::new(-axis.y, axis.x, 0.0);
    let nan = DVec3::splat(f64::NAN);
    let arrow = 0.22;
    let text = (d1 + d2) * 0.5 + perp * 0.15;
    let half_width = ((p2 - p1).length().log10().max(0.0) + 1.0) * 0.18;
    let half_height = 0.16;
    vec![
        p1,
        d1,
        nan,
        p2,
        d2,
        nan,
        d1,
        d2,
        nan,
        d1 + axis * arrow + perp * arrow * 0.45,
        d1,
        d1 + axis * arrow - perp * arrow * 0.45,
        nan,
        d2 - axis * arrow + perp * arrow * 0.45,
        d2,
        d2 - axis * arrow - perp * arrow * 0.45,
        nan,
        text - axis * half_width - perp * half_height,
        text + axis * half_width - perp * half_height,
        text + axis * half_width + perp * half_height,
        text - axis * half_width + perp * half_height,
        text - axis * half_width - perp * half_height,
    ]
}

fn preview_wire(points: Vec<DVec3>) -> WireModel {
    WireModel::solid_f64(
        "dimaligned_preview".into(),
        points.into_iter().map(|point| point.to_array()).collect(),
        WireModel::CYAN,
        false,
    )
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["DIMALIGNED"]
}); // AlignedDimensionCommand
