use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, CoincidentPick};
use crate::scene::parametric_constraints::{
    measured_expression, resolve_point, ConstraintKind, ParametricRef,
};

/// Which distance a dimensional constraint measures.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DimConstraintAxis {
    /// Horizontal or vertical, decided by where the dimension line goes.
    Linear,
    Horizontal,
    Vertical,
    Aligned,
}

/// A picked line (a line entity or one polyline segment) an Aligned
/// constraint measures perpendicular to.
#[derive(Clone, Copy)]
struct LineTarget {
    line: ParametricRef,
    /// Unit direction from its start to its end.
    dir: DVec3,
    /// Its two ends and where they are.
    ends: [(ParametricRef, DVec3); 2],
}

impl LineTarget {
    /// The end nearest to `point` — the end the reference measures from.
    fn nearest_end(&self, point: DVec3) -> (ParametricRef, DVec3) {
        let [a, b] = self.ends;
        if (b.1 - point).length_squared() < (a.1 - point).length_squared() {
            b
        } else {
            a
        }
    }
}

#[derive(Clone, Copy)]
enum Step {
    /// `Specify first constraint point or [Object] <Object>:`
    First,
    /// `Select object:`
    Object,
    /// `Specify second constraint point:`
    Second {
        first: ParametricRef,
        first_point: DVec3,
    },
    /// Aligned's Point & line: `Specify constraint point or [Line] <Line>:`
    PointLinePoint,
    /// `Select line:` after the constraint point.
    PointLineLine {
        point: ParametricRef,
        point_pos: DVec3,
    },
    /// `Select line:` first (the Line option).
    LineFirst,
    /// `Select constraint point:` after the line.
    LinePoint { line: LineTarget },
    /// Aligned's 2Lines: `Select first line:`
    TwoLinesFirst,
    /// `Select second line to make parallel:`
    TwoLinesSecond { line: LineTarget },
    /// The host is making the second line parallel; `accept_parallel_line`
    /// brings its solved ends.
    TwoLinesParallel {
        first: LineTarget,
        second: LineTarget,
        /// Where the second line was picked.
        pick: DVec3,
    },
    /// `Specify dimension line location:`
    Location {
        first: ParametricRef,
        second: ParametricRef,
        first_point: DVec3,
        second_point: DVec3,
        /// The line the distance is measured perpendicular to.
        direction: Option<LineTarget>,
    },
    /// `Enter value or name and value <d1=100>:`
    Value {
        first: ParametricRef,
        second: ParametricRef,
        first_point: DVec3,
        second_point: DVec3,
        location: DVec3,
        axis: DVec3,
        kind: ConstraintKind,
        measured: f64,
        direction: Option<LineTarget>,
    },
}

/// The reference's Linear/Horizontal/Vertical/Aligned dimensional constraint:
/// two constraint points (or one object's ends), a dimension line location,
/// then the parameter name and expression the dynamic dimension carries.
pub struct DimConstraintCommand {
    axis: DimConstraintAxis,
    step: Step,
    picked_entity: Option<EntityType>,
    /// The next free `dN` the host reserved for this constraint.
    default_name: String,
}

impl DimConstraintCommand {
    pub const NO_OBJECT: &'static str = "No object found.";
    pub const NO_POINT: &'static str = "No valid constraint point found.";
    pub const SAME_POINT: &'static str =
        "The object or point is already selected.  Select a different object or constraint point.";
    pub const INVALID_LINE: &'static str = "Invalid selection for Aligned. Select a line segment, polyline segment, text, MText, major or minor axis of ellipse or elliptical arc.";

    pub fn new(axis: DimConstraintAxis, default_name: String) -> Self {
        Self {
            axis,
            step: Step::First,
            picked_entity: None,
            default_name,
        }
    }

    fn command_name(&self) -> &'static str {
        match self.axis {
            DimConstraintAxis::Linear => "DCLINEAR",
            DimConstraintAxis::Horizontal => "DCHORIZONTAL",
            DimConstraintAxis::Vertical => "DCVERTICAL",
            DimConstraintAxis::Aligned => "DCALIGNED",
        }
    }

    fn noun(&self) -> &'static str {
        match self.axis {
            DimConstraintAxis::Linear => "Linear",
            DimConstraintAxis::Horizontal => "Horizontal",
            DimConstraintAxis::Vertical => "Vertical",
            DimConstraintAxis::Aligned => "Aligned",
        }
    }

    fn report(message: &str) -> CmdResult {
        CmdResult::ReportError(message.to_string())
    }

    /// The two end points an object pick constrains: a line's or an arc's
    /// ends, the picked polyline segment's vertices.
    fn object_ends(
        entity: &EntityType,
        handle: Handle,
        point: DVec3,
    ) -> Option<(ParametricRef, ParametricRef)> {
        match entity {
            EntityType::Line(_) | EntityType::Arc(_) => {
                Some((ParametricRef::point(handle, 0), ParametricRef::point(handle, 1)))
            }
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let (source, _, _) =
                    crate::scene::centerline::picked_source(entity, handle, point)?;
                let index = source.segment_index;
                // A closed polyline's last segment ends at the first vertex.
                let end = if resolve_point(entity, index + 1).is_some() {
                    index + 1
                } else {
                    0
                };
                Some((ParametricRef::point(handle, index), ParametricRef::point(handle, end)))
            }
            _ => None,
        }
    }

    fn world(entity: &EntityType, reference: ParametricRef) -> Option<DVec3> {
        let point = resolve_point(entity, reference.marker?)?;
        Some(DVec3::new(point.x, point.y, point.z))
    }

    /// The line a Point & line or 2Lines pick measures against: a line
    /// entity as a whole, or the picked polyline segment.
    fn line_target(entity: &EntityType, handle: Handle, point: DVec3) -> Option<LineTarget> {
        let (line, first, second) = match entity {
            EntityType::Line(_) => (
                ParametricRef::whole(handle),
                ParametricRef::point(handle, 0),
                ParametricRef::point(handle, 1),
            ),
            EntityType::LwPolyline(_) | EntityType::Polyline2D(_) => {
                let (source, _, _) =
                    crate::scene::centerline::picked_source(entity, handle, point)?;
                let index = source.segment_index;
                if index < 0 {
                    return None;
                }
                let end = if resolve_point(entity, index + 1).is_some() {
                    index + 1
                } else {
                    0
                };
                (
                    ParametricRef::segment(handle, index as usize),
                    ParametricRef::point(handle, index),
                    ParametricRef::point(handle, end),
                )
            }
            // A text's baseline or an ellipse's axis is a line whose one
            // constraint point is the insertion point / the center.
            EntityType::Text(_) | EntityType::MText(_) => {
                return Self::axis_target(entity, ParametricRef::text_baseline(handle), ParametricRef::point(handle, 0));
            }
            EntityType::Ellipse(_) => {
                let center = ParametricRef::point(handle, -3);
                let major = Self::axis_target(entity, ParametricRef::ellipse_major_axis(handle), center);
                let minor = Self::axis_target(entity, ParametricRef::ellipse_minor_axis(handle), center);
                let gap = |target: &LineTarget| {
                    let offset = point - target.ends[0].1;
                    (offset.x * target.dir.y - offset.y * target.dir.x).abs()
                };
                return match (major, minor) {
                    (Some(major), Some(minor)) => Some(if gap(&minor) < gap(&major) { minor } else { major }),
                    (major, minor) => major.or(minor),
                };
            }
            _ => return None,
        };
        let start = Self::world(entity, first)?;
        let finish = Self::world(entity, second)?;
        let dir = (finish - start).try_normalize()?;
        Some(LineTarget {
            line,
            dir,
            ends: [(first, start), (second, finish)],
        })
    }

    /// A directional axis (text baseline, ellipse axis) as a line target
    /// whose only constraint point is `anchor`.
    fn axis_target(entity: &EntityType, axis: ParametricRef, anchor: ParametricRef) -> Option<LineTarget> {
        let [start, end] =
            crate::scene::parametric_constraints::directional_axis_endpoints(entity, axis)?;
        let (start, end) = (
            DVec3::new(start.x, start.y, start.z),
            DVec3::new(end.x, end.y, end.z),
        );
        let dir = (end - start).try_normalize()?;
        let anchor_point = Self::world(entity, anchor)?;
        Some(LineTarget {
            line: axis,
            dir,
            ends: [(anchor, anchor_point), (anchor, anchor_point)],
        })
    }

    /// A picked line for the current line step, or the message to show.
    fn pick_line(&mut self, handle: Handle, point: DVec3) -> Result<LineTarget, &'static str> {
        if handle.is_null() {
            return Err(Self::NO_OBJECT);
        }
        let Some(entity) = self.picked_entity.take() else {
            return Err("");
        };
        Self::line_target(&entity, handle, point).ok_or(Self::INVALID_LINE)
    }

    /// The measured kind and axis: Linear reads the dimension line location
    /// the way a linear dimension does, the others are fixed; a Point & line
    /// or 2Lines distance runs perpendicular to its line.
    fn decide(
        &self,
        first: DVec3,
        second: DVec3,
        location: DVec3,
        direction: Option<LineTarget>,
    ) -> (ConstraintKind, DVec3) {
        if let Some(line) = direction {
            return (
                ConstraintKind::DistanceDirected,
                DVec3::new(line.dir.y, -line.dir.x, 0.0),
            );
        }
        match self.axis {
            DimConstraintAxis::Linear => {
                let axis =
                    crate::modules::annotate::linear_dim::measure_axis(first, second, location);
                if axis.x.abs() > 0.5 {
                    (ConstraintKind::DistanceX, DVec3::X)
                } else {
                    (ConstraintKind::DistanceY, DVec3::Y)
                }
            }
            DimConstraintAxis::Horizontal => (ConstraintKind::DistanceX, DVec3::X),
            DimConstraintAxis::Vertical => (ConstraintKind::DistanceY, DVec3::Y),
            DimConstraintAxis::Aligned => (
                ConstraintKind::Distance,
                (second - first).normalize_or(DVec3::X),
            ),
        }
    }

    fn measured(kind: ConstraintKind, first: DVec3, second: DVec3, axis: DVec3) -> f64 {
        match kind {
            ConstraintKind::DistanceX => (second.x - first.x).abs(),
            ConstraintKind::DistanceY => (second.y - first.y).abs(),
            ConstraintKind::DistanceDirected => (second - first).dot(axis).abs(),
            _ => (second - first).length(),
        }
    }

    fn build(&self, name: String, expression: String) -> Option<CmdResult> {
        let Step::Value {
            first,
            second,
            first_point,
            second_point,
            location,
            axis,
            kind,
            direction,
            ..
        } = self.step
        else {
            return None;
        };
        if expression.trim().is_empty() {
            return None;
        }
        Some(CmdResult::AddDimensionalConstraint {
            kind,
            first,
            second,
            first_point,
            second_point,
            location,
            axis,
            direction: direction.map(|line| line.line),
            name,
            expression,
            label: match self.axis {
                DimConstraintAxis::Linear => "Linear constraint",
                DimConstraintAxis::Horizontal => "Horizontal distance constraint",
                DimConstraintAxis::Vertical => "Vertical distance constraint",
                DimConstraintAxis::Aligned => "Aligned constraint",
            },
        })
    }
}

impl CadCommand for DimConstraintCommand {
    fn name(&self) -> &'static str {
        self.command_name()
    }

    fn prompt(&self) -> String {
        let name = self.command_name();
        match self.step {
            Step::First if self.axis == DimConstraintAxis::Aligned => format!(
                "{name}  Specify first constraint point or [Object/Point & line/2Lines] <Object>:"
            ),
            Step::First => {
                format!("{name}  Specify first constraint point or [Object] <Object>:")
            }
            Step::Object => format!("{name}  Select object:"),
            Step::Second { .. } => format!("{name}  Specify second constraint point:"),
            Step::PointLinePoint => {
                format!("{name}  Specify constraint point or [Line] <Line>:")
            }
            Step::PointLineLine { .. } | Step::LineFirst => format!("{name}  Select line:"),
            Step::LinePoint { .. } => format!("{name}  Select constraint point:"),
            Step::TwoLinesFirst => format!("{name}  Select first line:"),
            Step::TwoLinesSecond { .. } | Step::TwoLinesParallel { .. } => {
                format!("{name}  Select second line to make parallel:")
            }
            Step::Location { .. } => format!("{name}  Specify dimension line location:"),
            Step::Value { measured, .. } => format!(
                "{name}  Enter value or name and value <{}={}>:",
                self.default_name,
                measured_expression(measured)
            ),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::First if self.axis == DimConstraintAxis::Aligned => vec![
                CmdOption::new("Object", "O"),
                CmdOption::new("Point & line", "P"),
                CmdOption::new("2Lines", "2L"),
            ],
            Step::First => vec![CmdOption::new("Object", "O")],
            Step::PointLinePoint => vec![CmdOption::new("Line", "L")],
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(
            self.step,
            Step::First | Step::PointLinePoint | Step::Value { .. }
        )
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match self.step {
            Step::First => {
                let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
                let aligned = self.axis == DimConstraintAxis::Aligned;
                match keyword.as_str() {
                    "O" | "OBJECT" => self.step = Step::Object,
                    "P" | "POINT" | "POINT & LINE" | "POINT&LINE" if aligned => {
                        self.step = Step::PointLinePoint
                    }
                    "2L" | "2LINES" if aligned => self.step = Step::TwoLinesFirst,
                    _ => return None,
                }
                Some(CmdResult::NeedPoint)
            }
            Step::PointLinePoint => {
                let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
                if matches!(keyword.as_str(), "L" | "LINE") {
                    self.step = Step::LineFirst;
                    return Some(CmdResult::NeedPoint);
                }
                None
            }
            Step::Value { .. } => {
                let text = text.trim();
                let (name, expression) = match text.split_once('=') {
                    Some((name, expression)) => (name.trim().to_string(), expression.trim().to_string()),
                    None => (self.default_name.clone(), text.to_string()),
                };
                self.build(name, expression)
            }
            _ => None,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(
            self.step,
            Step::First
                | Step::Object
                | Step::Second { .. }
                | Step::PointLinePoint
                | Step::PointLineLine { .. }
                | Step::LineFirst
                | Step::LinePoint { .. }
                | Step::TwoLinesFirst
                | Step::TwoLinesSecond { .. }
        )
    }

    fn entity_pick_accepts_points(&self) -> bool {
        true
    }

    fn typed_point_picks_entity(&self) -> bool {
        true
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
        match self.step {
            // The host resolves the constraint point and hands it back
            // through `accept_constraint_point`.
            Step::First | Step::Second { .. } | Step::PointLinePoint | Step::LinePoint { .. } => {
                CmdResult::CheckConstraintPoint(CoincidentPick {
                    handle: (!handle.is_null()).then_some(handle),
                    point,
                    whole_curve: false,
                })
            }
            Step::PointLineLine { point: first, point_pos } => {
                match self.pick_line(handle, point) {
                    Ok(line) => {
                        if line.line.entity == first.entity {
                            return Self::report(Self::SAME_POINT);
                        }
                        let (second, second_point) = line.nearest_end(point_pos);
                        self.step = Step::Location {
                            first,
                            second,
                            first_point: point_pos,
                            second_point,
                            direction: Some(line),
                        };
                        CmdResult::NeedPoint
                    }
                    Err("") => CmdResult::NeedPoint,
                    Err(message) => Self::report(message),
                }
            }
            Step::LineFirst => match self.pick_line(handle, point) {
                Ok(line) => {
                    self.step = Step::LinePoint { line };
                    CmdResult::NeedPoint
                }
                Err("") => CmdResult::NeedPoint,
                Err(message) => Self::report(message),
            },
            Step::TwoLinesFirst => match self.pick_line(handle, point) {
                Ok(line) => {
                    self.step = Step::TwoLinesSecond { line };
                    CmdResult::NeedPoint
                }
                Err("") => CmdResult::NeedPoint,
                Err(message) => Self::report(message),
            },
            Step::TwoLinesSecond { line: first_line } => match self.pick_line(handle, point) {
                Ok(second_line) => {
                    if second_line.line == first_line.line {
                        return Self::report(Self::SAME_POINT);
                    }
                    // The host makes the second line parallel first; the
                    // distance is measured on the solved geometry.
                    self.step = Step::TwoLinesParallel {
                        first: first_line,
                        second: second_line,
                        pick: point,
                    };
                    CmdResult::MakeParallel {
                        first_line: first_line.line,
                        first_ends: [first_line.ends[0].0, first_line.ends[1].0],
                        second_line: second_line.line,
                        second_ends: [second_line.ends[0].0, second_line.ends[1].0],
                    }
                }
                Err("") => CmdResult::NeedPoint,
                Err(message) => Self::report(message),
            },
            Step::TwoLinesParallel { .. } => CmdResult::NeedPoint,
            Step::Object => {
                if handle.is_null() {
                    return Self::report(Self::NO_OBJECT);
                }
                let Some(entity) = self.picked_entity.take() else {
                    return CmdResult::NeedPoint;
                };
                let Some((first, second)) = Self::object_ends(&entity, handle, point) else {
                    return Self::report(&format!(
                        "Invalid selection for {}. Select a line, polyline segment or arc.",
                        self.noun()
                    ));
                };
                let (Some(first_point), Some(second_point)) =
                    (Self::world(&entity, first), Self::world(&entity, second))
                else {
                    return Self::report(Self::NO_POINT);
                };
                self.step = Step::Location {
                    first,
                    second,
                    first_point,
                    second_point,
                    direction: None,
                };
                CmdResult::NeedPoint
            }
            Step::Location { .. } | Step::Value { .. } => self.on_point(point),
        }
    }

    fn accept_parallel_line(&mut self, ends: [(ParametricRef, DVec3); 2]) -> CmdResult {
        let Step::TwoLinesParallel { first, second, pick } = self.step else {
            return CmdResult::NeedPoint;
        };
        let Some(dir) = (ends[1].1 - ends[0].1).try_normalize() else {
            return Self::report(Self::NO_POINT);
        };
        let second = LineTarget {
            line: second.line,
            dir,
            ends,
        };
        // The first line's end nearest the second pick, then the second
        // line's end nearest that end.
        let (first_ref, first_point) = first.nearest_end(pick);
        let (second_ref, second_point) = second.nearest_end(first_point);
        self.step = Step::Location {
            first: first_ref,
            second: second_ref,
            first_point,
            second_point,
            direction: Some(first),
        };
        CmdResult::NeedPoint
    }

    fn accept_constraint_point(&mut self, reference: ParametricRef, point: DVec3) -> CmdResult {
        match self.step {
            Step::First => {
                self.step = Step::Second {
                    first: reference,
                    first_point: point,
                };
                CmdResult::NeedPoint
            }
            Step::Second { first, first_point } => {
                if first == reference {
                    return Self::report(Self::SAME_POINT);
                }
                self.step = Step::Location {
                    first,
                    second: reference,
                    first_point,
                    second_point: point,
                    direction: None,
                };
                CmdResult::NeedPoint
            }
            Step::PointLinePoint => {
                self.step = Step::PointLineLine {
                    point: reference,
                    point_pos: point,
                };
                CmdResult::NeedPoint
            }
            Step::LinePoint { line } => {
                if line.line.entity == reference.entity {
                    return Self::report(Self::SAME_POINT);
                }
                let (first, first_point) = line.nearest_end(point);
                self.step = Step::Location {
                    first,
                    second: reference,
                    first_point,
                    second_point: point,
                    direction: Some(line),
                };
                CmdResult::NeedPoint
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        match self.step {
            Step::First | Step::Second { .. } | Step::PointLinePoint | Step::LinePoint { .. } => {
                CmdResult::CheckConstraintPoint(CoincidentPick {
                    handle: None,
                    point,
                    whole_curve: false,
                })
            }
            Step::Object
            | Step::PointLineLine { .. }
            | Step::LineFirst
            | Step::TwoLinesFirst
            | Step::TwoLinesSecond { .. } => Self::report(Self::NO_OBJECT),
            Step::Location {
                first,
                second,
                first_point,
                second_point,
                direction,
            } => {
                let (kind, axis) = self.decide(first_point, second_point, point, direction);
                let measured = Self::measured(kind, first_point, second_point, axis);
                self.step = Step::Value {
                    first,
                    second,
                    first_point,
                    second_point,
                    location: point,
                    axis,
                    kind,
                    measured,
                    direction,
                };
                CmdResult::ReportMeasurement(format!("Dimension text = {measured:.4}"))
            }
            Step::TwoLinesParallel { .. } | Step::Value { .. } => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::First => {
                self.step = Step::Object;
                CmdResult::NeedPoint
            }
            Step::PointLinePoint => {
                self.step = Step::LineFirst;
                CmdResult::NeedPoint
            }
            Step::Value { measured, .. } => self
                .build(self.default_name.clone(), measured_expression(measured))
                .unwrap_or(CmdResult::Cancel),
            _ => CmdResult::Cancel,
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `DIMCONSTRAINT`: the reference's option front end for the dimensional
/// constraint family; each choice runs the focused command.
pub struct DimConstraintMenuCommand {
    /// The option Enter takes: the dimensional constraint used last.
    default: &'static str,
}

impl DimConstraintMenuCommand {
    pub fn new(default: &'static str) -> Self {
        Self { default }
    }

    fn dispatch(keyword: &str) -> Option<&'static str> {
        Some(match keyword {
            "L" | "LINEAR" => "DCLINEAR",
            "H" | "HORIZONTAL" => "DCHORIZONTAL",
            "V" | "VERTICAL" => "DCVERTICAL",
            "A" | "ALIGNED" => "DCALIGNED",
            "AN" | "ANGULAR" => "DCANGULAR",
            "R" | "RADIAL" | "RADIUS" => "DCRADIUS",
            "D" | "DIAMETER" => "DCDIAMETER",
            "F" | "FORM" => "DCFORM",
            "C" | "CONVERT" => "DCCONVERT",
            _ => return None,
        })
    }

    fn command_for_label(label: &str) -> &'static str {
        match label {
            "Linear" => "DCLINEAR",
            "Horizontal" => "DCHORIZONTAL",
            "Vertical" => "DCVERTICAL",
            "ANgular" => "DCANGULAR",
            "Radius" => "DCRADIUS",
            "Diameter" => "DCDIAMETER",
            "Convert" => "DCCONVERT",
            _ => "DCALIGNED",
        }
    }
}

impl CadCommand for DimConstraintMenuCommand {
    fn name(&self) -> &'static str {
        "DIMCONSTRAINT"
    }

    fn prompt(&self) -> String {
        format!(
            "DIMCONSTRAINT  Enter a dimensional constraint option [Linear/Horizontal/Vertical/Aligned/ANgular/Radius/Diameter/Form/Convert] <{}>:",
            self.default
        )
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![
            CmdOption::new("Linear", "L"),
            CmdOption::new("Horizontal", "H"),
            CmdOption::new("Vertical", "V"),
            CmdOption::new("Aligned", "A"),
            CmdOption::new("ANgular", "AN"),
            CmdOption::new("Radius", "R"),
            CmdOption::new("Diameter", "D"),
            CmdOption::new("Form", "F"),
            CmdOption::new("Convert", "C"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
        Self::dispatch(&keyword).map(|command| CmdResult::Dispatch(command.to_string()))
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch(Self::command_for_label(self.default).to_string())
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// `DCFORM`: `Enter constraint form [Annotational/Dynamic] <current>:`;
/// the host records the answer and continues into DIMCONSTRAINT's option
/// prompt, as the reference does.
pub struct ConstraintFormCommand {
    annotational: bool,
}

impl ConstraintFormCommand {
    pub fn new(annotational: bool) -> Self {
        Self { annotational }
    }

    fn current(&self) -> &'static str {
        if self.annotational {
            "Annotational"
        } else {
            "Dynamic"
        }
    }
}

impl CadCommand for ConstraintFormCommand {
    fn name(&self) -> &'static str {
        "DCFORM"
    }

    fn prompt(&self) -> String {
        format!(
            "DCFORM  Enter constraint form [Annotational/Dynamic] <{}>:",
            self.current()
        )
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![
            CmdOption::new("Annotational", "A"),
            CmdOption::new("Dynamic", "D"),
        ]
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
        let form = match keyword.as_str() {
            "A" | "ANNOTATIONAL" => "Annotational",
            "D" | "DYNAMIC" => "Dynamic",
            _ => return None,
        };
        Some(CmdResult::Dispatch(format!("DCFORM_SET {form}")))
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Dispatch(format!("DCFORM_SET {}", self.current()))
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// The value prompt a double-clicked dynamic dimension opens, standing in
/// for the reference's in-place editor; `name=expression` also renames.
pub struct DimensionValueCommand {
    name: String,
    current: String,
}

impl DimensionValueCommand {
    pub fn new(name: String, current: String) -> Self {
        Self { name, current }
    }
}

impl CadCommand for DimensionValueCommand {
    fn name(&self) -> &'static str {
        "DCVALUE"
    }

    fn prompt(&self) -> String {
        format!(
            "DCVALUE  Enter value or name and value <{}={}>:",
            self.name, self.current
        )
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        (!text.is_empty()).then(|| CmdResult::Dispatch(format!("DCVALUE {} {text}", self.name)))
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::Line;
    use acadrust::types::Vector3;

    #[test]
    fn object_pick_takes_the_line_ends_then_asks_for_the_location() {
        let mut command = DimConstraintCommand::new(DimConstraintAxis::Linear, "d1".into());
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        command.inject_picked_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(100.0, 50.0, 0.0),
        )));
        assert!(matches!(
            command.on_entity_pick(Handle::new(7), DVec3::new(50.0, 25.0, 0.0)),
            CmdResult::NeedPoint
        ));
        // Below the pair: horizontal, so the measured value is the X span.
        let CmdResult::ReportMeasurement(text) = command.on_point(DVec3::new(50.0, -30.0, 0.0))
        else {
            panic!("the location must report the dimension text");
        };
        assert_eq!(text, "Dimension text = 100.0000");
        assert_eq!(
            command.prompt(),
            "DCLINEAR  Enter value or name and value <d1=100>:"
        );
        let CmdResult::AddDimensionalConstraint {
            kind, name, expression, ..
        } = command.on_text_input("w=d1*2").expect("a value builds the constraint")
        else {
            panic!("the value must build the constraint");
        };
        assert_eq!(kind, ConstraintKind::DistanceX);
        assert_eq!(name, "w");
        assert_eq!(expression, "d1*2");
    }

    #[test]
    fn a_location_beside_the_pair_measures_vertically() {
        let mut command = DimConstraintCommand::new(DimConstraintAxis::Linear, "d1".into());
        let first = ParametricRef::point(Handle::new(7), 0);
        let second = ParametricRef::point(Handle::new(7), 1);
        command.accept_constraint_point(first, DVec3::ZERO);
        command.accept_constraint_point(second, DVec3::new(100.0, 50.0, 0.0));
        let CmdResult::ReportMeasurement(text) = command.on_point(DVec3::new(140.0, 25.0, 0.0))
        else {
            panic!("the location must report the dimension text");
        };
        assert_eq!(text, "Dimension text = 50.0000");
    }
}
