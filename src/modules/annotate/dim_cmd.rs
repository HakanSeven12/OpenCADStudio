// DIM — one command for most dimension types. At its main prompt a picked
// line gives a linear (or aligned) dimension, a circle a diameter, an arc a
// radius; a point starts a dimension between two points. The options run
// the other kinds. Each dimension is made by the matching dimension command
// running inside DIM; after it DIM asks again until Enter.

use codec::{EntityType, Handle};
use glam::DVec3;

use crate::command::{
    CadCommand, CmdOption, CmdResult, DimensionPreview, DynSpec, InputKind, WorkingPlane,
};
use crate::modules::annotate::aligned_dim::AlignedDimensionCommand;
use crate::modules::annotate::angular_dim::AngularDimensionCommand;
use crate::modules::annotate::arc_length_dim::ArcLengthDimensionCommand;
use crate::modules::annotate::diameter_dim::DiameterDimensionCommand;
use crate::modules::annotate::linear_dim::LinearDimensionCommand;
use crate::modules::annotate::ordinate_dim::OrdinateDimCommand;
use crate::modules::annotate::radius_dim::RadiusDimensionCommand;
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/dim_linear.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DIM",
        label: "Dimension",
        icon: ICON,
        event: ModuleEvent::Command("DIM".to_string()),
    }
}

/// What a radial pick was, so its options can switch the kind.
#[derive(Clone)]
struct RadialPick {
    entity: EntityType,
    handle: Handle,
    point: DVec3,
    circle: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Linear,
    Aligned,
    Radius,
    Diameter,
    ArcLength,
    Angular,
    Ordinate,
    /// A baseline series run from DIM's options.
    Series,
    /// A continued series run from DIM's options.
    Continued,
}

pub struct DimCommand {
    plane: WorkingPlane,
    inner: Option<(Kind, Box<dyn CadCommand>)>,
    /// The first point of a dimension started from a point, while its second
    /// one is awaited.
    first_point: Option<DVec3>,
    radial: Option<RadialPick>,
    picked: Option<EntityType>,
    /// The Layer option is waiting for a name.
    layer: bool,
    /// The layer DIM was started with ("." = the current one).
    current_layer: String,
    /// The object picked at the Layer prompt, whose layer is taken.
    layer_pick: Option<EntityType>,
    /// The dimension just placed and the ones it lies over, while DIM asks
    /// what to do with them; `true` in a continued series, which offers only
    /// Break up and None.
    overlap: Option<(Handle, Vec<Handle>, bool)>,
}

/// The next default at DIM's own overlap question ('M', 'R' or 'N'; Break
/// up is not kept, Move away comes back after it). Kept for the session.
static OVERLAP_DEFAULT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(b'M');
/// The continued series' answer chosen last ('B' or 'N').
static CONTINUE_OVERLAP_DEFAULT: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(b'B');

fn overlap_default(continued: bool) -> u8 {
    if continued {
        CONTINUE_OVERLAP_DEFAULT.load(std::sync::atomic::Ordering::Relaxed)
    } else {
        OVERLAP_DEFAULT.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl DimCommand {
    pub fn new(current_layer: String) -> Self {
        Self {
            plane: WorkingPlane::default(),
            inner: None,
            first_point: None,
            radial: None,
            picked: None,
            layer: false,
            current_layer,
            layer_pick: None,
            overlap: None,
        }
    }

    fn answer_overlap(&mut self, choice: u8) -> CmdResult {
        let Some((new, others, continued)) = self.overlap.take() else {
            return CmdResult::NeedPoint;
        };
        if continued {
            CONTINUE_OVERLAP_DEFAULT.store(choice, std::sync::atomic::Ordering::Relaxed);
        } else {
            let next = if choice == b'B' { b'M' } else { choice };
            OVERLAP_DEFAULT.store(next, std::sync::atomic::Ordering::Relaxed);
        }
        CmdResult::Dispatch(format!(
            "_DIM_OVERLAP {} {} {}",
            choice as char,
            new.value(),
            others
                .iter()
                .map(|handle| handle.value().to_string())
                .collect::<Vec<_>>()
                .join(",")
        ))
    }

    /// DIM at its Layer prompt, which repeats until Enter or eXit.
    pub fn at_layer(current_layer: String) -> Self {
        Self {
            layer: true,
            ..Self::new(current_layer)
        }
    }

    /// DIM running a command it handed off (the Baseline and Continue
    /// series); Enter at that command's first prompt comes back to DIM.
    pub fn adopt(&mut self, command: Box<dyn CadCommand>, continued: bool) {
        let kind = if continued { Kind::Continued } else { Kind::Series };
        self.inner = Some((kind, command));
    }

    fn start(&mut self, kind: Kind) -> &mut Box<dyn CadCommand> {
        let mut command: Box<dyn CadCommand> = match kind {
            Kind::Linear => Box::new(LinearDimensionCommand::new()),
            Kind::Aligned => Box::new(AlignedDimensionCommand::new()),
            Kind::Radius => Box::new(RadiusDimensionCommand::new()),
            Kind::Diameter => Box::new(DiameterDimensionCommand::new()),
            Kind::ArcLength => Box::new(ArcLengthDimensionCommand::new()),
            Kind::Angular => Box::new(AngularDimensionCommand::new()),
            Kind::Ordinate => Box::new(OrdinateDimCommand::new()),
            Kind::Series | Kind::Continued => unreachable!("a series is adopted, not started"),
        };
        command.set_working_plane(self.plane);
        &mut self.inner.insert((kind, command)).1
    }

    /// A pick replayed into a dimension command whose first prompt picks an
    /// object.
    fn start_picked(&mut self, kind: Kind, entity: EntityType, handle: Handle, point: DVec3) -> CmdResult {
        let command = self.start(kind);
        if !command.needs_entity_pick() {
            // DIMLINEAR / DIMALIGNED pick objects after an Enter.
            let _ = command.on_enter();
        }
        command.inject_picked_entity(entity);
        let result = command.on_entity_pick(handle, point);
        self.wrap(result)
    }

    /// The inner command's result: a finished dimension keeps DIM running.
    fn wrap(&mut self, result: CmdResult) -> CmdResult {
        match result {
            CmdResult::CommitDimension {
                entity,
                association,
                preserve_base_style,
                continue_command,
            } => {
                if !continue_command {
                    self.reset();
                }
                CmdResult::CommitDimension {
                    entity,
                    association,
                    preserve_base_style,
                    continue_command: true,
                }
            }
            other => other,
        }
    }

    fn reset(&mut self) {
        self.inner = None;
        self.first_point = None;
        self.radial = None;
        self.picked = None;
    }

    fn inner(&self) -> Option<&dyn CadCommand> {
        self.inner.as_ref().map(|(_, command)| command.as_ref())
    }

    /// The kind a radial option switches to, with the same pick.
    fn switch_radial(&mut self, kind: Kind) -> Option<CmdResult> {
        let pick = self.radial.clone()?;
        if kind == Kind::ArcLength && pick.circle {
            return None;
        }
        Some(self.start_picked(kind, pick.entity, pick.handle, pick.point))
    }
}

/// A line-like source has two ends; circles and arcs are radial.
enum Source {
    Line,
    Circle,
    Arc,
    Other,
}

fn source_of(entity: &EntityType) -> Source {
    match entity {
        EntityType::Line(_) | EntityType::LwPolyline(_) | EntityType::Polyline(_) => Source::Line,
        EntityType::Circle(_) => Source::Circle,
        EntityType::Arc(_) => Source::Arc,
        _ => Source::Other,
    }
}

impl CadCommand for DimCommand {
    fn name(&self) -> &'static str {
        "DIM"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
        if let Some((_, command)) = self.inner.as_mut() {
            command.set_working_plane(plane);
        }
    }

    fn ask_dim_overlap(&mut self, new: Handle, others: Vec<Handle>) -> bool {
        let continued = match &self.inner {
            None if !self.layer && self.overlap.is_none() => false,
            Some((Kind::Continued, _)) => true,
            _ => return false,
        };
        self.overlap = Some((new, others, continued));
        true
    }

    fn prompt(&self) -> String {
        if let Some((_, _, continued)) = &self.overlap {
            let default = match overlap_default(*continued) {
                b'B' => "Break up",
                b'R' => "Replace",
                b'N' => "None",
                _ => "Move away",
            };
            return if *continued {
                crate::tr!("dim-cmd", "overlap-continue", default = default)
            } else {
                crate::tr!("dim-cmd", "overlap", default = default)
            };
        }
        if self.layer {
            return crate::tr!("dim-cmd", "layer", current = self.current_layer.clone());
        }
        if let Some(pick) = &self.radial {
            if self
                .inner()
                .is_some_and(|command| command.dimension_placement_pending())
            {
                return if pick.circle {
                    crate::tr!("dim-cmd", "diameter-location")
                } else {
                    crate::tr!("dim-cmd", "radius-location")
                };
            }
        }
        match self.inner() {
            // The inner command's prompt, under DIM's name.
            Some(command) => {
                let prompt = command.prompt();
                match prompt.split_once("  ") {
                    Some((_, rest)) => format!("DIM  {rest}"),
                    None => prompt,
                }
            }
            None => crate::tr!("dim-cmd", "main"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if let Some((_, _, continued)) = &self.overlap {
            if *continued {
                return vec![CmdOption::new("Break up", "B"), CmdOption::new("None", "N")];
            }
            return vec![
                CmdOption::new("Move away", "M"),
                CmdOption::new("Break up", "B"),
                CmdOption::new("Replace", "R"),
                CmdOption::new("None", "N"),
            ];
        }
        if self.layer {
            return vec![CmdOption::new("?", "?"), CmdOption::new("eXit", "X")];
        }
        if let Some(pick) = &self.radial {
            if self
                .inner()
                .is_some_and(|command| command.dimension_placement_pending())
            {
                let mut options = Vec::new();
                if pick.circle {
                    options.push(CmdOption::new("Radius", "RADIUS"));
                } else {
                    options.push(CmdOption::new("Diameter", "DIAMETER"));
                }
                options.push(CmdOption::new("Jogged", "JOGGED"));
                options.push(CmdOption::new("Angular", "ANGULAR"));
                if !pick.circle {
                    options.push(CmdOption::new("Arc length", "ARCLENGTH"));
                }
                if let Some(command) = self.inner() {
                    options.extend(command.options());
                }
                return options;
            }
        }
        match self.inner() {
            Some(command) => command.options(),
            None => vec![
                CmdOption::new("Angular", "ANGULAR"),
                CmdOption::new("Baseline", "BASELINE"),
                CmdOption::new("Continue", "CONTINUE"),
                CmdOption::new("Ordinate", "ORDINATE"),
                CmdOption::new("aliGn", "ALIGN"),
                CmdOption::new("Distribute", "DISTRIBUTE"),
                CmdOption::new("Layer", "LAYER"),
                CmdOption::new("Undo", "UNDO"),
            ],
        }
    }

    fn input_kind(&self) -> InputKind {
        self.inner()
            .map_or(InputKind::Point, |command| command.input_kind())
    }

    #[allow(deprecated)]
    fn wants_text_input(&self) -> bool {
        self.layer || self.inner().is_some_and(|command| command.wants_text_input())
    }

    fn point_step_accepts_keywords(&self) -> bool {
        self.inner()
            .is_none_or(|command| command.point_step_accepts_keywords())
    }

    fn needs_entity_pick(&self) -> bool {
        if self.overlap.is_some() {
            return false;
        }
        match self.inner() {
            Some(command) => command.needs_entity_pick(),
            // The main prompt picks objects and takes points; the Layer
            // prompt takes a picked object's layer.
            None => true,
        }
    }

    fn entity_pick_accepts_points(&self) -> bool {
        match self.inner() {
            Some(command) => command.entity_pick_accepts_points(),
            None => true,
        }
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.inner()
            .is_none_or(|command| command.entity_pick_highlights_hover())
    }

    fn inject_before_entity_pick(&self) -> bool {
        self.inner()
            .is_none_or(|command| command.inject_before_entity_pick())
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        match self.inner.as_mut() {
            Some((_, command)) => command.inject_picked_entity(entity),
            None if self.layer => self.layer_pick = Some(entity),
            None => self.picked = Some(entity),
        }
    }

    fn on_entity_pick(&mut self, handle: Handle, pt: DVec3) -> CmdResult {
        if let Some((_, command)) = self.inner.as_mut() {
            let result = command.on_entity_pick(handle, pt);
            return self.wrap(result);
        }
        if self.layer {
            return match self.layer_pick.take() {
                Some(entity) if !handle.is_null() => {
                    let layer = entity.common().layer.clone();
                    self.current_layer = layer.clone();
                    CmdResult::Dispatch(format!("_DIM_LAYER {layer}"))
                }
                _ => CmdResult::NeedPoint,
            };
        }
        let Some(entity) = self.picked.take() else {
            return CmdResult::NeedPoint;
        };
        match source_of(&entity) {
            Source::Line => {
                // A slanted line is measured along itself.
                let kind = match &entity {
                    EntityType::Line(line) => {
                        let dx = line.end.x - line.start.x;
                        let dy = line.end.y - line.start.y;
                        if dx.abs() > 1e-9 && dy.abs() > 1e-9 {
                            Kind::Aligned
                        } else {
                            Kind::Linear
                        }
                    }
                    _ => Kind::Linear,
                };
                self.start_picked(kind, entity, handle, pt)
            }
            Source::Circle | Source::Arc => {
                let circle = matches!(entity, EntityType::Circle(_));
                self.radial = Some(RadialPick {
                    entity: entity.clone(),
                    handle,
                    point: pt,
                    circle,
                });
                let kind = if circle { Kind::Diameter } else { Kind::Radius };
                self.start_picked(kind, entity, handle, pt)
            }
            Source::Other => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.overlap.is_some() || (self.layer && self.inner.is_none()) {
            return CmdResult::NeedPoint;
        }
        if let Some((kind, command)) = self.inner.as_mut() {
            // A slanted pair of points is dimensioned along itself.
            if *kind == Kind::Linear && command.dimension_acquired_points().len() == 1 {
                if let Some(first) = self.first_point {
                    let d = self.plane.to_local(pt) - self.plane.to_local(first);
                    if d.x.abs() > 1e-9 && d.y.abs() > 1e-9 {
                        let aligned = self.start(Kind::Aligned);
                        let _ = aligned.on_point(first);
                        let result = aligned.on_point(pt);
                        return self.wrap(result);
                    }
                }
            }
            let result = command.on_point(pt);
            return self.wrap(result);
        }
        self.first_point = Some(pt);
        let command = self.start(Kind::Linear);
        let result = command.on_point(pt);
        self.wrap(result)
    }

    fn on_enter(&mut self) -> CmdResult {
        if let Some((_, _, continued)) = &self.overlap {
            let choice = overlap_default(*continued);
            return self.answer_overlap(choice);
        }
        if self.layer {
            self.layer = false;
            return CmdResult::NeedPoint;
        }
        match self.inner.as_mut() {
            Some((_, command)) => {
                let result = command.on_enter();
                match result {
                    // Enter that ends the inner command returns to DIM.
                    CmdResult::Cancel => {
                        self.reset();
                        CmdResult::NeedPoint
                    }
                    other => self.wrap(other),
                }
            }
            // Enter at DIM's own prompt ends it quietly.
            None => CmdResult::Measurement(String::new()),
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if let Some((_, _, continued)) = &self.overlap {
            let word = text.trim().trim_start_matches('_').to_ascii_uppercase();
            let choice = match word.as_str() {
                "M" | "MOVE" | "MOVE AWAY" if !*continued => b'M',
                "B" | "BREAK" | "BREAK UP" => b'B',
                "R" | "REPLACE" if !*continued => b'R',
                "N" | "NONE" => b'N',
                _ => return None,
            };
            return Some(self.answer_overlap(choice));
        }
        if self.layer {
            let value = text.trim().trim_matches('"');
            return Some(match value.to_ascii_uppercase().as_str() {
                "" => CmdResult::NeedPoint,
                "X" | "EXIT" | "_X" | "_EXIT" => {
                    self.layer = false;
                    CmdResult::NeedPoint
                }
                "?" => CmdResult::Dispatch("_DIM_LAYER_LIST".into()),
                _ => {
                    self.current_layer = value.to_string();
                    CmdResult::Dispatch(format!("_DIM_LAYER {value}"))
                }
            });
        }
        let keyword = text.trim().trim_start_matches(['_', '.']).to_ascii_uppercase();
        if self.radial.is_some()
            && self
                .inner()
                .is_some_and(|command| command.dimension_placement_pending())
        {
            let switched = match keyword.as_str() {
                "R" | "RADIUS" => self.switch_radial(Kind::Radius),
                "D" | "DIAMETER" => self.switch_radial(Kind::Diameter),
                "L" | "ARCLENGTH" => self.switch_radial(Kind::ArcLength),
                "A" | "ANGULAR" => {
                    let pick = self.radial.clone()?;
                    Some(self.start_picked(Kind::Angular, pick.entity, pick.handle, pick.point))
                }
                // Jogged needs the drawing's dimension defaults.
                "J" | "JOGGED" => Some(CmdResult::Dispatch("DIMJOGGED".into())),
                _ => None,
            };
            if switched.is_some() {
                return switched;
            }
        }
        if let Some((_, command)) = self.inner.as_mut() {
            // DIM names the text angle "text aNgle".
            let text = if command.dimension_placement_pending()
                && matches!(keyword.as_str(), "N" | "ANGLE")
            {
                "A"
            } else {
                text
            };
            let result = command.on_text_input(text)?;
            return Some(self.wrap(result));
        }
        match keyword.as_str() {
            "A" | "AN" | "ANG" | "ANGULAR" => {
                self.start(Kind::Angular);
                Some(CmdResult::NeedPoint)
            }
            "O" | "ORD" | "ORDINATE" => {
                self.start(Kind::Ordinate);
                Some(CmdResult::NeedPoint)
            }
            // The continued and baseline series need the last dimension.
            // Both series pick their first dimension and come back to DIM.
            "B" | "BASELINE" => Some(CmdResult::Dispatch("_DIM_BASELINE".into())),
            "C" | "CONTINUE" => Some(CmdResult::Dispatch("_DIM_CONTINUE".into())),
            "G" | "ALIGN" => Some(CmdResult::Dispatch("_DIM_ALIGN".into())),
            "D" | "DISTRIBUTE" => Some(CmdResult::Dispatch("_DIM_DISTRIBUTE".into())),
            "L" | "LAYER" => {
                self.layer = true;
                Some(CmdResult::NeedPoint)
            }
            "U" | "UNDO" => Some(CmdResult::Dispatch("_DIM_UNDO".into())),
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        self.inner
            .as_mut()
            .and_then(|(_, command)| command.on_mouse_move(pt))
    }

    fn dimension_preview(&self, cursor: DVec3) -> Option<Vec<DimensionPreview>> {
        self.inner()?.dimension_preview(cursor)
    }

    fn measures_through_viewports(&self) -> bool {
        true
    }

    fn dimension_placement_pending(&self) -> bool {
        self.inner()
            .is_some_and(|command| command.dimension_placement_pending())
    }

    fn dimension_acquired_points(&self) -> Vec<DVec3> {
        self.inner()
            .map(|command| command.dimension_acquired_points())
            .unwrap_or_default()
    }

    fn on_editor_text(&mut self, value: String) {
        if let Some((_, command)) = self.inner.as_mut() {
            command.on_editor_text(value);
        }
    }

    fn on_editor_closed(&mut self, committed: bool) -> CmdResult {
        match self.inner.as_mut() {
            Some((_, command)) => {
                let result = command.on_editor_closed(committed);
                self.wrap(result)
            }
            None => CmdResult::NeedPoint,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        let (_, command) = self.inner.as_mut()?;
        let result = command.on_undo_step()?;
        Some(self.wrap(result))
    }

    fn dyn_spec(&self) -> Option<DynSpec> {
        self.inner()?.dyn_spec()
    }

    fn text_field(&self) -> Option<(String, Vec<Handle>)> {
        self.inner()?.text_field()
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIM"] });
