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
    /// The angle between two lines, or at a vertex between two points.
    Angular,
    /// A circle's or arc's radius.
    Radius,
    /// A circle's or arc's diameter.
    Diameter,
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
    TwoLinesSecond {
        line: LineTarget,
        /// Where the first line was picked.
        pick: DVec3,
    },
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
    /// Angular: `Select first line or arc or [3Point] <3Point>:`
    AngularFirst,
    /// `Select second line:`
    AngularSecond { first: LineTarget },
    /// 3Point: `Specify angle vertex:`
    AngularVertex,
    /// `Specify first angle constraint point:`
    AngularPoint1 { vertex: (ParametricRef, DVec3) },
    /// `Specify second angle constraint point:`
    AngularPoint2 {
        vertex: (ParametricRef, DVec3),
        first: (ParametricRef, DVec3),
    },
    /// `Specify dimension line location:` of an angle.
    AngularLocation { data: AngularData },
    /// `Enter value or name and value <ang1=27>:`
    AngularValue {
        data: AngularData,
        location: DVec3,
        sector: u8,
        measured: f64,
    },
    /// Radius/Diameter: `Select arc or circle:`
    RadialObject,
    /// `Specify dimension line location:` of a radius or diameter.
    RadialLocation { target: RadialTarget },
    /// `Enter value or name and value <dia1=50>:`
    RadialValue {
        target: RadialTarget,
        location: DVec3,
        measured: f64,
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

/// Angular's picks: two lines, or a vertex and two points (an arc gives
/// its center and its ends).
#[derive(Clone, Copy)]
enum AngularData {
    Lines {
        first: LineTarget,
        second: LineTarget,
    },
    Points {
        vertex: (ParametricRef, DVec3),
        first: (ParametricRef, DVec3),
        second: (ParametricRef, DVec3),
    },
}

/// The circle or arc a Radius/Diameter constraint measures.
#[derive(Clone, Copy)]
struct RadialTarget {
    circle: ParametricRef,
    center: DVec3,
    radius: f64,
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
    /// Angular precision (DIMADEC) for `Dimension text = 27`.
    angle_decimals: usize,
}

impl DimConstraintCommand {
    pub const NO_OBJECT: &'static str = "No object found.";
    pub const NO_POINT: &'static str = "No valid constraint point found.";
    pub const SAME_POINT: &'static str =
        "The object or point is already selected.  Select a different object or constraint point.";
    pub const INVALID_LINE: &'static str = "Invalid selection for Aligned. Select a line segment, polyline segment, text, MText, major or minor axis of ellipse or elliptical arc.";
    pub const PARALLEL_LINES: &'static str = "Lines are parallel.";

    pub fn new(axis: DimConstraintAxis, default_name: String) -> Self {
        Self {
            axis,
            step: match axis {
                DimConstraintAxis::Angular => Step::AngularFirst,
                DimConstraintAxis::Radius | DimConstraintAxis::Diameter => Step::RadialObject,
                _ => Step::First,
            },
            picked_entity: None,
            default_name,
            angle_decimals: 0,
        }
    }

    /// The angular precision the measured angle is reported with.
    pub fn with_angle_decimals(mut self, decimals: usize) -> Self {
        self.angle_decimals = decimals;
        self
    }

    fn angle_display(&self, degrees: f64) -> String {
        let decimals = self.angle_decimals;
        format!(
            "{:.decimals$}",
            crate::scene::parametric_constraints::normalize_angle_display(degrees)
        )
    }

    /// An Angular pick: a line or a polyline segment (text baselines and
    /// ellipse axes are not angle sides).
    fn angular_line(entity: &EntityType, handle: Handle, point: DVec3) -> Option<LineTarget> {
        matches!(
            entity,
            EntityType::Line(_) | EntityType::LwPolyline(_) | EntityType::Polyline2D(_)
        )
        .then(|| Self::line_target(entity, handle, point))
        .flatten()
    }

    /// The angle (degrees) the dimension line location picks between the
    /// directed sides `p1a→p1b` and `p2a→p2b`, and which of the four
    /// sectors it is in the solver's terms (which signed sides bound it).
    fn angle_frame(
        p1a: DVec3,
        p1b: DVec3,
        p2a: DVec3,
        p2b: DVec3,
        location: DVec3,
    ) -> Option<(f64, u8)> {
        use crate::scene::parametric_constraints::angle_sector;
        let tau = std::f64::consts::TAU;
        let (_, start, end) =
            crate::modules::annotate::angular_dim::two_line_frame(p1a, p1b, p2a, p2b, location)?;
        let sweep = (end - start).rem_euclid(tau);
        let same = |a: f64, b: f64| {
            let d = (a - b).rem_euclid(tau);
            d < 1.0e-6 || d > tau - 1.0e-6
        };
        let d1 = p1b - p1a;
        let d2 = p2b - p2a;
        let a1 = d1.y.atan2(d1.x);
        let a2 = d2.y.atan2(d2.x);
        let pi = std::f64::consts::PI;
        let sector = if same(start, a1) || same(start, a1 + pi) {
            // Swept from a side of the first line to a side of the second.
            if same(start, a1) == same(end, a2) {
                angle_sector::PARALLEL_COUNTERCLOCKWISE
            } else {
                angle_sector::ANTIPARALLEL_COUNTERCLOCKWISE
            }
        } else if same(start, a2) == same(end, a1) {
            angle_sector::PARALLEL_CLOCKWISE
        } else {
            angle_sector::ANTIPARALLEL_CLOCKWISE
        };
        Some((sweep.to_degrees(), sector))
    }

    /// The angle (degrees) the dimension line location picks at `vertex`
    /// between the rays to `first` and `second`: the counterclockwise sweep
    /// from the first ray when the location lies in it, else the other way
    /// round (a semicircle measures 180, a reflex angle is allowed).
    fn ray_frame(vertex: DVec3, first: DVec3, second: DVec3, location: DVec3) -> Option<(f64, u8)> {
        use crate::scene::parametric_constraints::angle_sector;
        let tau = std::f64::consts::TAU;
        let r1 = first - vertex;
        let r2 = second - vertex;
        let at = location - vertex;
        if r1.length_squared() < 1.0e-18 || r2.length_squared() < 1.0e-18 || at.length_squared() < 1.0e-18 {
            return None;
        }
        let a1 = r1.y.atan2(r1.x);
        let a2 = r2.y.atan2(r2.x);
        let t = at.y.atan2(at.x);
        let sweep = (a2 - a1).rem_euclid(tau);
        if sweep < 1.0e-9 {
            return None;
        }
        if (t - a1).rem_euclid(tau) <= sweep + 1.0e-9 {
            Some((sweep.to_degrees(), angle_sector::PARALLEL_COUNTERCLOCKWISE))
        } else {
            Some(((tau - sweep).to_degrees(), angle_sector::PARALLEL_CLOCKWISE))
        }
    }

    fn build_angular(&self, name: String, expression: String) -> Option<CmdResult> {
        let Step::AngularValue {
            data,
            location,
            sector,
            ..
        } = self.step
        else {
            return None;
        };
        if expression.trim().is_empty() {
            return None;
        }
        let (refs, points) = match data {
            AngularData::Lines { first, second } => (
                vec![first.line, second.line],
                vec![
                    first.ends[0].1,
                    first.ends[1].1,
                    second.ends[0].1,
                    second.ends[1].1,
                ],
            ),
            AngularData::Points {
                vertex,
                first,
                second,
            } => (
                vec![first.0, vertex.0, second.0],
                vec![vertex.1, first.1, second.1],
            ),
        };
        Some(CmdResult::AddAngularConstraint {
            refs,
            points,
            location,
            sector,
            renamed: name != self.default_name,
            name,
            expression,
        })
    }

    fn command_name(&self) -> &'static str {
        match self.axis {
            DimConstraintAxis::Linear => "DCLINEAR",
            DimConstraintAxis::Horizontal => "DCHORIZONTAL",
            DimConstraintAxis::Vertical => "DCVERTICAL",
            DimConstraintAxis::Aligned => "DCALIGNED",
            DimConstraintAxis::Angular => "DCANGULAR",
            DimConstraintAxis::Radius => "DCRADIUS",
            DimConstraintAxis::Diameter => "DCDIAMETER",
        }
    }

    fn noun(&self) -> &'static str {
        match self.axis {
            DimConstraintAxis::Linear => "Linear",
            DimConstraintAxis::Horizontal => "Horizontal",
            DimConstraintAxis::Vertical => "Vertical",
            DimConstraintAxis::Aligned => "Aligned",
            DimConstraintAxis::Angular => "Angular",
            DimConstraintAxis::Radius => "Radius",
            DimConstraintAxis::Diameter => "Diameter",
        }
    }

    /// The circle or arc a radial pick landed on, with its centre and radius.
    fn radial_target(entity: &EntityType, handle: Handle) -> Option<RadialTarget> {
        let (center, radius) = match entity {
            EntityType::Circle(circle) => (circle.center, circle.radius),
            EntityType::Arc(arc) => (arc.center, arc.radius),
            _ => return None,
        };
        (radius > 1.0e-12).then_some(RadialTarget {
            circle: ParametricRef::whole(handle),
            center: DVec3::new(center.x, center.y, center.z),
            radius,
        })
    }

    fn build_radial(&self, name: String, expression: String) -> Option<CmdResult> {
        let Step::RadialValue {
            target, location, ..
        } = self.step
        else {
            return None;
        };
        if expression.trim().is_empty() {
            return None;
        }
        Some(CmdResult::AddRadialConstraint {
            circle: target.circle,
            center: target.center,
            radius: target.radius,
            location,
            diameter: self.axis == DimConstraintAxis::Diameter,
            renamed: name != self.default_name,
            name,
            expression,
        })
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
            // Angular never measures here; its own steps decide the sector.
            DimConstraintAxis::Aligned
            | DimConstraintAxis::Angular
            | DimConstraintAxis::Radius
            | DimConstraintAxis::Diameter => (
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
            renamed: name != self.default_name,
            name,
            expression,
            label: match self.axis {
                DimConstraintAxis::Linear => "Linear constraint",
                DimConstraintAxis::Horizontal => "Horizontal distance constraint",
                DimConstraintAxis::Vertical => "Vertical distance constraint",
                DimConstraintAxis::Aligned
                | DimConstraintAxis::Angular
                | DimConstraintAxis::Radius
                | DimConstraintAxis::Diameter => "Aligned constraint",
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
            Step::AngularFirst => {
                format!("{name}  Select first line or arc or [3Point] <3Point>:")
            }
            Step::AngularSecond { .. } => format!("{name}  Select second line:"),
            Step::AngularVertex => format!("{name}  Specify angle vertex:"),
            Step::AngularPoint1 { .. } => {
                format!("{name}  Specify first angle constraint point:")
            }
            Step::AngularPoint2 { .. } => {
                format!("{name}  Specify second angle constraint point:")
            }
            Step::RadialObject => format!("{name}  Select arc or circle:"),
            Step::AngularLocation { .. }
            | Step::Location { .. }
            | Step::RadialLocation { .. } => {
                format!("{name}  Specify dimension line location:")
            }
            Step::RadialValue { measured, .. } => format!(
                "{name}  Enter value or name and value <{}={}>:",
                self.default_name,
                measured_expression(measured)
            ),
            Step::AngularValue { measured, .. } => format!(
                "{name}  Enter value or name and value <{}={}>:",
                self.default_name,
                measured_expression(measured)
            ),
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
            Step::AngularFirst => vec![CmdOption::new("3Point", "3P")],
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
            Step::First
                | Step::PointLinePoint
                | Step::Value { .. }
                | Step::AngularFirst
                | Step::AngularValue { .. }
                | Step::RadialValue { .. }
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
            Step::AngularFirst => {
                let keyword = text.trim().trim_start_matches('_').to_ascii_uppercase();
                if matches!(keyword.as_str(), "3P" | "3POINT") {
                    self.step = Step::AngularVertex;
                    return Some(CmdResult::NeedPoint);
                }
                None
            }
            Step::Value { .. } | Step::AngularValue { .. } | Step::RadialValue { .. } => {
                let text = text.trim();
                let (name, expression) = match text.split_once('=') {
                    Some((name, expression)) => (name.trim().to_string(), expression.trim().to_string()),
                    None => (self.default_name.clone(), text.to_string()),
                };
                match self.step {
                    Step::AngularValue { .. } => self.build_angular(name, expression),
                    Step::RadialValue { .. } => self.build_radial(name, expression),
                    _ => self.build(name, expression),
                }
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
                | Step::AngularFirst
                | Step::AngularSecond { .. }
                | Step::AngularVertex
                | Step::AngularPoint1 { .. }
                | Step::AngularPoint2 { .. }
                | Step::RadialObject
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
                    self.step = Step::TwoLinesSecond { line, pick: point };
                    CmdResult::NeedPoint
                }
                Err("") => CmdResult::NeedPoint,
                Err(message) => Self::report(message),
            },
            Step::TwoLinesSecond {
                line: first_line,
                pick: first_pick,
            } => match self.pick_line(handle, point) {
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
                        first_pick,
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
            Step::AngularFirst => {
                if handle.is_null() {
                    return Self::report(Self::NO_OBJECT);
                }
                let Some(entity) = self.picked_entity.take() else {
                    return CmdResult::NeedPoint;
                };
                if matches!(entity, EntityType::Arc(_)) {
                    // An arc's included angle: its center and its ends.
                    let center = ParametricRef::center(handle);
                    let start = ParametricRef::point(handle, 0);
                    let end = ParametricRef::point(handle, 1);
                    let (Some(c), Some(s), Some(e)) = (
                        Self::world(&entity, center),
                        Self::world(&entity, start),
                        Self::world(&entity, end),
                    ) else {
                        return Self::report(Self::NO_POINT);
                    };
                    self.step = Step::AngularLocation {
                        data: AngularData::Points {
                            vertex: (center, c),
                            first: (start, s),
                            second: (end, e),
                        },
                    };
                    return CmdResult::NeedPoint;
                }
                match Self::angular_line(&entity, handle, point) {
                    Some(line) => {
                        self.step = Step::AngularSecond { first: line };
                        CmdResult::NeedPoint
                    }
                    None => Self::report(&format!(
                        "Invalid selection for {}. Select a line, polyline segment or arc.",
                        self.noun()
                    )),
                }
            }
            Step::AngularSecond { first } => {
                if handle.is_null() {
                    return Self::report(Self::NO_OBJECT);
                }
                let Some(entity) = self.picked_entity.take() else {
                    return CmdResult::NeedPoint;
                };
                let Some(second) = Self::angular_line(&entity, handle, point) else {
                    return Self::report(&format!(
                        "Invalid selection for {}. Select a line, polyline segment or arc.",
                        self.noun()
                    ));
                };
                if second.line == first.line {
                    return Self::report(Self::SAME_POINT);
                }
                let cross = first.dir.x * second.dir.y - first.dir.y * second.dir.x;
                if cross.abs() < 1.0e-9 {
                    return CmdResult::CancelWithMessage(Self::PARALLEL_LINES.to_string());
                }
                self.step = Step::AngularLocation {
                    data: AngularData::Lines { first, second },
                };
                CmdResult::NeedPoint
            }
            Step::AngularVertex | Step::AngularPoint1 { .. } | Step::AngularPoint2 { .. } => {
                CmdResult::CheckConstraintPoint(CoincidentPick {
                    handle: (!handle.is_null()).then_some(handle),
                    point,
                    whole_curve: false,
                })
            }
            Step::RadialObject => {
                if handle.is_null() {
                    return Self::report(Self::NO_OBJECT);
                }
                let Some(entity) = self.picked_entity.take() else {
                    return CmdResult::NeedPoint;
                };
                let Some(target) = Self::radial_target(&entity, handle) else {
                    return Self::report(&format!(
                        "Invalid selection for {}. Select a circle or arc.",
                        self.noun()
                    ));
                };
                let measured = if self.axis == DimConstraintAxis::Diameter {
                    target.radius * 2.0
                } else {
                    target.radius
                };
                self.step = Step::RadialLocation { target };
                // The measured size is reported before the location, as the
                // reference prints it, with trailing zeros suppressed.
                CmdResult::ReportMeasurement(format!(
                    "Dimension text = {}",
                    measured_expression(measured)
                ))
            }
            Step::Location { .. }
            | Step::Value { .. }
            | Step::AngularLocation { .. }
            | Step::AngularValue { .. }
            | Step::RadialLocation { .. }
            | Step::RadialValue { .. } => self.on_point(point),
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
            Step::AngularVertex => {
                self.step = Step::AngularPoint1 {
                    vertex: (reference, point),
                };
                CmdResult::NeedPoint
            }
            Step::AngularPoint1 { vertex } => {
                if reference == vertex.0 {
                    return Self::report(Self::SAME_POINT);
                }
                self.step = Step::AngularPoint2 {
                    vertex,
                    first: (reference, point),
                };
                CmdResult::NeedPoint
            }
            Step::AngularPoint2 { vertex, first } => {
                if reference == vertex.0 || reference == first.0 {
                    return Self::report(Self::SAME_POINT);
                }
                self.step = Step::AngularLocation {
                    data: AngularData::Points {
                        vertex,
                        first,
                        second: (reference, point),
                    },
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
            Step::AngularVertex | Step::AngularPoint1 { .. } | Step::AngularPoint2 { .. } => {
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
            | Step::TwoLinesSecond { .. }
            | Step::AngularFirst
            | Step::AngularSecond { .. } => Self::report(Self::NO_OBJECT),
            Step::AngularLocation { data } => {
                let frame = match data {
                    AngularData::Lines { first, second } => Self::angle_frame(
                        first.ends[0].1,
                        first.ends[1].1,
                        second.ends[0].1,
                        second.ends[1].1,
                        point,
                    ),
                    AngularData::Points {
                        vertex,
                        first,
                        second,
                    } => Self::ray_frame(vertex.1, first.1, second.1, point),
                };
                let Some((measured, sector)) = frame else {
                    return Self::report(Self::NO_POINT);
                };
                self.step = Step::AngularValue {
                    data,
                    location: point,
                    sector,
                    measured,
                };
                CmdResult::ReportMeasurement(format!(
                    "Dimension text = {}",
                    self.angle_display(measured)
                ))
            }
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
            Step::RadialObject => Self::report(Self::NO_OBJECT),
            Step::RadialLocation { target } => {
                let measured = if self.axis == DimConstraintAxis::Diameter {
                    target.radius * 2.0
                } else {
                    target.radius
                };
                self.step = Step::RadialValue {
                    target,
                    location: point,
                    measured,
                };
                CmdResult::NeedPoint
            }
            Step::TwoLinesParallel { .. }
            | Step::Value { .. }
            | Step::AngularValue { .. }
            | Step::RadialValue { .. } => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::First => {
                self.step = Step::Object;
                CmdResult::NeedPoint
            }
            // Enter takes the 3Point default.
            Step::AngularFirst => {
                self.step = Step::AngularVertex;
                CmdResult::NeedPoint
            }
            Step::AngularValue { measured, .. } => self
                .build_angular(self.default_name.clone(), measured_expression(measured))
                .unwrap_or(CmdResult::Cancel),
            Step::PointLinePoint => {
                self.step = Step::LineFirst;
                CmdResult::NeedPoint
            }
            Step::Value { measured, .. } => self
                .build(self.default_name.clone(), measured_expression(measured))
                .unwrap_or(CmdResult::Cancel),
            Step::RadialValue { measured, .. } => self
                .build_radial(self.default_name.clone(), measured_expression(measured))
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
