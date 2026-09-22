//! DCONSTRAINT / ACONSTRAINT — constraints that need a typed target value
//! (a distance or an angle), unlike the plain select-and-click constraints
//! in `mod.rs`.
//!
//! These commands don't solve anything themselves. A `CadCommand`'s `on_text_input` only
//! gets `&mut self` — no document access — so it can't add a
//! `ParametricConstraint` to the scene directly; instead these commands hand the
//! typed value back as `CmdResult::AddParametricConstraint`, and the host (which
//! does have `&mut Scene`) adds the record and solves it via the same
//! `Scene::bump_entities` path any later edit to these entities will use too.
//! `default_value` (what the prompt shows in `<...>`, and what a bare Enter
//! submits) is still computed from a one-time read of current geometry —
//! that part doesn't change.

use acadrust::entities::EntityType;
use acadrust::types::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, InputKind};
use crate::modules::ToolDef;
use crate::scene::named_parameters::DrivingValue;
use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef};
use crate::scene::Scene;

/// Recognizes a typed prompt token as either a numeric literal or a
/// reference to an existing named parameter. `known_names` is a one-time snapshot taken at
/// command construction (`DistanceConstraintCommand`/`AngleConstraintCommand
/// ::new`, which already has `&Scene`) since `on_text_input` itself has no
/// document access — the same constraint `default_value` already works
/// around. Only *existence* is checked here; the actual value is resolved
/// fresh at solve time (`parametric_solve::build_constraint`), consistent with
/// this project's "no incremental/cached resolution" approach throughout.
fn parse_driving_value(text: &str, known_names: &[String]) -> Option<DrivingValue> {
    let text = text.trim();
    if let Ok(value) = text.parse::<f64>() {
        return Some(DrivingValue::Literal(value));
    }
    // Case-insensitive: the command line uppercases `SingleToken` input
    // (InputKind::SingleToken) before it reaches this parser, so a typed
    // lowercase parameter name like "hole_dia" arrives here as "HOLE_DIA".
    // Matching case-insensitively and returning the canonically-cased name
    // keeps the driving link pointed at the actual parameter.
    known_names
        .iter()
        .find(|n| n.eq_ignore_ascii_case(text))
        .map(|n| DrivingValue::Named(n.clone()))
}

pub mod distance_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("DCONSTRAINT")
    }
}

pub mod dimensional_tools {
    use super::*;

    fn command(id: &'static str) -> ToolDef {
        crate::modules::ribbon_command(id)
    }

    pub fn linear() -> ToolDef {
        command("DCLINEAR")
    }
    pub fn horizontal() -> ToolDef {
        command("DCHORIZONTAL")
    }
    pub fn vertical() -> ToolDef {
        command("DCVERTICAL")
    }
    pub fn aligned() -> ToolDef {
        command("DCALIGNED")
    }
    pub fn angular() -> ToolDef {
        command("DCANGULAR")
    }
    pub fn radius() -> ToolDef {
        command("DCRADIUS")
    }
    pub fn diameter() -> ToolDef {
        command("DCDIAMETER")
    }
    pub fn convert() -> ToolDef {
        command("DCCONVERT")
    }
}

pub mod angle_tool {
    use super::*;
    pub fn tool() -> ToolDef {
        crate::modules::ribbon_command("ACONSTRAINT")
    }
}

/// Which flavor of dimensional constraint the typed value drives.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DistanceMode {
    /// Line length (two-point distance) or a circle/arc's radius —
    /// whichever `is_circle` selects.
    Auto,
    /// Chooses the dominant world-axis component.
    Linear,
    Aligned,
    Radius,
    X,
    Y,
    Diameter,
}

/// Constrains a single line's length (or its X/Y-only component) between
/// its two endpoints, or a circle/arc's radius or diameter, to a typed target value.
pub struct DistanceConstraintCommand {
    command_name: &'static str,
    handle: Handle,
    /// Whether `handle` is a circle/arc (Radius/Diameter) or a line
    /// (Distance/DistanceX/DistanceY) — decided once at construction from
    /// the entity's type.
    is_circle: bool,
    mode: DistanceMode,
    default_value: f64,
    default_x: f64,
    default_y: f64,
    /// Snapshot of `Scene::named_parameters`' names at construction time —
    /// see `parse_driving_value`'s doc comment for why a snapshot.
    known_param_names: Vec<String>,
}

impl DistanceConstraintCommand {
    /// `None` if `handle` isn't a Line, Circle, or Arc.
    pub fn new(scene: &Scene, handle: Handle) -> Option<Self> {
        Self::with_mode(scene, handle, "DCONSTRAINT", DistanceMode::Auto)
    }

    pub fn with_mode(
        scene: &Scene,
        handle: Handle,
        command_name: &'static str,
        requested_mode: DistanceMode,
    ) -> Option<Self> {
        let entity = scene.document.get_entity(handle)?;
        let (is_circle, default_value, default_x, default_y) = match entity {
            EntityType::Line(l) => {
                let dx = l.end.x - l.start.x;
                let dy = l.end.y - l.start.y;
                (false, (dx * dx + dy * dy).sqrt(), dx, dy)
            }
            EntityType::Circle(c) => (true, c.radius, 0.0, 0.0),
            EntityType::Arc(a) => (true, a.radius, 0.0, 0.0),
            _ => return None,
        };
        let known_param_names = scene
            .named_parameters()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mode = match requested_mode {
            DistanceMode::Linear if !is_circle => {
                if default_x.abs() >= default_y.abs() {
                    DistanceMode::X
                } else {
                    DistanceMode::Y
                }
            }
            DistanceMode::Aligned if !is_circle => DistanceMode::Aligned,
            DistanceMode::Radius if is_circle => DistanceMode::Radius,
            DistanceMode::Diameter if is_circle => DistanceMode::Diameter,
            DistanceMode::X | DistanceMode::Y if !is_circle => requested_mode,
            DistanceMode::Auto => DistanceMode::Auto,
            _ => return None,
        };
        Some(Self {
            command_name,
            handle,
            is_circle,
            mode,
            default_value,
            default_x,
            default_y,
            known_param_names,
        })
    }

    fn build(&self, target: DrivingValue) -> Option<CmdResult> {
        if let DrivingValue::Literal(v) = &target {
            let signed_component = matches!(self.mode, DistanceMode::X | DistanceMode::Y);
            if !v.is_finite() || (!signed_component && *v <= 0.0) {
                return None;
            }
        }
        let (kind, refs, label) = match (self.is_circle, self.mode) {
            (true, DistanceMode::Diameter) => (
                ConstraintKind::Diameter,
                vec![ParametricRef::whole(self.handle)],
                "Diameter constraint",
            ),
            (true, DistanceMode::Auto | DistanceMode::Radius) => (
                ConstraintKind::Radius,
                vec![ParametricRef::whole(self.handle)],
                "Radius constraint",
            ),
            (false, DistanceMode::X) => (
                ConstraintKind::DistanceX,
                vec![
                    ParametricRef::point(self.handle, 0),
                    ParametricRef::point(self.handle, 1),
                ],
                "DistanceX constraint",
            ),
            (false, DistanceMode::Y) => (
                ConstraintKind::DistanceY,
                vec![
                    ParametricRef::point(self.handle, 0),
                    ParametricRef::point(self.handle, 1),
                ],
                "DistanceY constraint",
            ),
            (false, DistanceMode::Auto | DistanceMode::Aligned | DistanceMode::Linear) => (
                ConstraintKind::Distance,
                vec![
                    ParametricRef::point(self.handle, 0),
                    ParametricRef::point(self.handle, 1),
                ],
                "Distance constraint",
            ),
            _ => return None,
        };
        Some(CmdResult::AddParametricConstraint {
            kind,
            refs,
            driving_param: Some(target),
            label,
        })
    }
}

impl CadCommand for DistanceConstraintCommand {
    fn name(&self) -> &'static str {
        self.command_name
    }

    fn prompt(&self) -> String {
        match (self.is_circle, self.mode) {
            (true, DistanceMode::Diameter) => format!(
                "Specify diameter <{:.4}> or [Radius]: ",
                self.default_value * 2.0
            ),
            (true, DistanceMode::Auto | DistanceMode::Radius) => format!(
                "Specify distance <{:.4}> or [Diameter]: ",
                self.default_value
            ),
            (false, DistanceMode::X) => format!("Specify X distance <{:.4}>: ", self.default_x),
            (false, DistanceMode::Y) => format!("Specify Y distance <{:.4}>: ", self.default_y),
            (false, DistanceMode::Auto | DistanceMode::Aligned | DistanceMode::Linear) => format!(
                "Specify distance <{:.4}> or [Xdistance/Ydistance]: ",
                self.default_value
            ),
            _ => "Unsupported distance constraint mode.".to_string(),
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        let default = match self.mode {
            DistanceMode::Diameter => self.default_value * 2.0,
            DistanceMode::X => self.default_x,
            DistanceMode::Y => self.default_y,
            DistanceMode::Auto
            | DistanceMode::Aligned
            | DistanceMode::Linear
            | DistanceMode::Radius => self.default_value,
        };
        self.build(DrivingValue::Literal(default))
            .unwrap_or(CmdResult::Cancel)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        // An existing parameter name wins over a mode keyword with the same spelling.
        if let Some(value @ DrivingValue::Named(_)) =
            parse_driving_value(text, &self.known_param_names)
        {
            return self.build(value);
        }
        // Mode-switch keywords re-prompt instead of building — consumed
        // inputs return `Some(NeedPoint)` so the driver doesn't re-offer
        // the same token a second time (matches e.g. `trim.rs`'s
        // `T`/`B`/`F` keyword handling). Defensively uppercased same as
        // `trim.rs` even though `SingleToken` input already arrives
        // uppercased from the command line.
        let keyword = text.trim().to_uppercase();
        match (self.is_circle, keyword.as_str()) {
            (true, "D" | "DIAMETER") => {
                self.mode = DistanceMode::Diameter;
                return Some(CmdResult::NeedPoint);
            }
            (true, "R" | "RADIUS") => {
                self.mode = DistanceMode::Radius;
                return Some(CmdResult::NeedPoint);
            }
            (false, "X" | "XDISTANCE") => {
                self.mode = DistanceMode::X;
                return Some(CmdResult::NeedPoint);
            }
            (false, "Y" | "YDISTANCE") => {
                self.mode = DistanceMode::Y;
                return Some(CmdResult::NeedPoint);
            }
            _ => {}
        }
        let value = parse_driving_value(text, &self.known_param_names)?;
        self.build(value)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Constrains the angle (in degrees) from `fixed`'s direction to `moving`'s.
pub struct AngleConstraintCommand {
    command_name: &'static str,
    fixed_handle: Handle,
    moving_handle: Handle,
    default_value: f64,
    /// Snapshot of `Scene::named_parameters`' names at construction time —
    /// see `parse_driving_value`'s doc comment for why a snapshot.
    known_param_names: Vec<String>,
}

impl AngleConstraintCommand {
    /// `None` unless both `fixed` and `moving` are lines.
    pub fn new(scene: &Scene, fixed: Handle, moving: Handle) -> Option<Self> {
        Self::with_name(scene, fixed, moving, "ACONSTRAINT")
    }

    pub fn with_name(
        scene: &Scene,
        fixed: Handle,
        moving: Handle,
        command_name: &'static str,
    ) -> Option<Self> {
        let fixed_entity = scene.document.get_entity(fixed)?;
        let moving_entity = scene.document.get_entity(moving)?;
        let (EntityType::Line(f), EntityType::Line(m)) = (fixed_entity, moving_entity) else {
            return None;
        };
        let a1 = (f.end.y - f.start.y).atan2(f.end.x - f.start.x);
        let a2 = (m.end.y - m.start.y).atan2(m.end.x - m.start.x);
        let default_value = (a2 - a1).to_degrees();
        let known_param_names = scene
            .named_parameters()
            .iter()
            .map(|p| p.name.clone())
            .collect();
        Some(Self {
            command_name,
            fixed_handle: fixed,
            moving_handle: moving,
            default_value,
            known_param_names,
        })
    }

    fn build(&self, target: DrivingValue) -> Option<CmdResult> {
        if matches!(&target, DrivingValue::Literal(value) if !value.is_finite()) {
            return None;
        }
        Some(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Angle,
            refs: vec![
                ParametricRef::whole(self.fixed_handle),
                ParametricRef::whole(self.moving_handle),
            ],
            driving_param: Some(target),
            label: "Angle constraint",
        })
    }
}

impl CadCommand for AngleConstraintCommand {
    fn name(&self) -> &'static str {
        self.command_name
    }

    fn prompt(&self) -> String {
        format!("Specify angle in degrees <{:.4}>: ", self.default_value)
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.build(DrivingValue::Literal(self.default_value))
            .unwrap_or(CmdResult::Cancel)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = parse_driving_value(text, &self.known_param_names)?;
        self.build(value)
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "DCONSTRAINT",
        "ACONSTRAINT",
        "DCLINEAR",
        "DCHORIZONTAL",
        "DCVERTICAL",
        "DCALIGNED",
        "DCANGULAR",
        "DCRADIUS",
        "DCDIAMETER",
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_driving_value_prefers_a_numeric_literal() {
        let known = vec!["hole_dia".to_string()];
        assert_eq!(
            parse_driving_value("12.5", &known),
            Some(DrivingValue::Literal(12.5))
        );
        // Distance's own `build` rejects a non-positive target later; the
        // parse step itself accepts any number, negative included.
        assert_eq!(
            parse_driving_value("-3", &known),
            Some(DrivingValue::Literal(-3.0))
        );
    }

    #[test]
    fn parse_driving_value_recognizes_a_known_parameter_name() {
        let known = vec!["hole_dia".to_string(), "plate_len".to_string()];
        assert_eq!(
            parse_driving_value("hole_dia", &known),
            Some(DrivingValue::Named("hole_dia".to_string()))
        );
    }

    #[test]
    fn parse_driving_value_rejects_an_unknown_token() {
        let known = vec!["hole_dia".to_string()];
        assert_eq!(parse_driving_value("bogus", &known), None);
        assert_eq!(parse_driving_value("", &known), None);
    }

    #[test]
    fn parse_driving_value_matches_a_known_parameter_regardless_of_typed_case() {
        // The command line uppercases `SingleToken` input before it reaches
        // here, so a lowercase-named parameter must still resolve — and
        // resolve to its canonically-cased name, not the uppercased token.
        let known = vec!["hole_dia".to_string()];
        assert_eq!(
            parse_driving_value("HOLE_DIA", &known),
            Some(DrivingValue::Named("hole_dia".to_string()))
        );
        assert_eq!(
            parse_driving_value("Hole_Dia", &known),
            Some(DrivingValue::Named("hole_dia".to_string()))
        );
    }

    fn add_line(scene: &mut Scene) -> Handle {
        scene.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            acadrust::types::Vector3::new(0.0, 0.0, 0.0),
            acadrust::types::Vector3::new(6.0, 8.0, 0.0),
        )))
    }

    fn add_circle(scene: &mut Scene) -> Handle {
        scene.add_entity(EntityType::Circle(
            acadrust::entities::Circle::from_center_radius(
                acadrust::types::Vector3::new(0.0, 0.0, 0.0),
                3.0,
            ),
        ))
    }

    #[test]
    fn x_keyword_switches_mode_and_then_builds_a_distance_x_constraint() {
        let mut scene = Scene::new();
        let line = add_line(&mut scene);
        let mut cmd =
            DistanceConstraintCommand::new(&scene, line).expect("Line supports DCONSTRAINT");

        // The keyword itself only switches mode and re-prompts — it must
        // not build a constraint yet.
        assert!(matches!(cmd.on_text_input("X"), Some(CmdResult::NeedPoint)));
        assert!(
            cmd.prompt().contains("X distance"),
            "prompt should reflect the new mode: {}",
            cmd.prompt()
        );

        match cmd.on_text_input("10") {
            Some(CmdResult::AddParametricConstraint {
                kind,
                driving_param: Some(DrivingValue::Literal(v)),
                ..
            }) => {
                assert_eq!(kind, ConstraintKind::DistanceX);
                assert_eq!(v, 10.0);
            }
            _ => panic!("expected an AddParametricConstraint{{DistanceX}}, got a different result"),
        }
    }

    #[test]
    fn component_distances_accept_signed_values() {
        let mut scene = Scene::new();
        let line = add_line(&mut scene);
        let mut cmd =
            DistanceConstraintCommand::new(&scene, line).expect("Line supports DCONSTRAINT");
        assert!(matches!(cmd.on_text_input("X"), Some(CmdResult::NeedPoint)));
        assert!(matches!(
            cmd.on_text_input("-3"),
            Some(CmdResult::AddParametricConstraint {
                kind: ConstraintKind::DistanceX,
                driving_param: Some(DrivingValue::Literal(-3.0)),
                ..
            })
        ));
    }

    #[test]
    fn component_distance_defaults_use_the_selected_axis() {
        let mut scene = Scene::new();
        let line = add_line(&mut scene);
        let mut cmd =
            DistanceConstraintCommand::new(&scene, line).expect("Line supports DCONSTRAINT");
        assert!(matches!(cmd.on_text_input("Y"), Some(CmdResult::NeedPoint)));
        assert!(cmd.prompt().contains("<8.0000>"));
        assert!(matches!(
            cmd.on_enter(),
            CmdResult::AddParametricConstraint {
                kind: ConstraintKind::DistanceY,
                driving_param: Some(DrivingValue::Literal(8.0)),
                ..
            }
        ));
    }

    #[test]
    fn a_parameter_named_like_a_mode_keyword_remains_usable() {
        let mut scene = Scene::new();
        let circle = add_circle(&mut scene);
        scene.named_parameters_mut().set("r", "5").unwrap();
        let mut cmd =
            DistanceConstraintCommand::new(&scene, circle).expect("Circle supports DCONSTRAINT");

        assert!(matches!(
            cmd.on_text_input("R"),
            Some(CmdResult::AddParametricConstraint {
                kind: ConstraintKind::Radius,
                driving_param: Some(DrivingValue::Named(name)),
                ..
            }) if name == "r"
        ));
    }

    #[test]
    fn ordinary_distances_reject_nonpositive_and_nonfinite_values() {
        let mut scene = Scene::new();
        let line = add_line(&mut scene);
        let mut cmd =
            DistanceConstraintCommand::new(&scene, line).expect("Line supports DCONSTRAINT");
        assert!(cmd.on_text_input("-3").is_none());
        assert!(cmd.build(DrivingValue::Literal(f64::INFINITY)).is_none());
    }

    #[test]
    fn d_keyword_switches_a_circles_command_to_diameter_mode() {
        let mut scene = Scene::new();
        let circle = add_circle(&mut scene);
        let mut cmd =
            DistanceConstraintCommand::new(&scene, circle).expect("Circle supports DCONSTRAINT");

        assert!(matches!(cmd.on_text_input("D"), Some(CmdResult::NeedPoint)));
        assert!(
            cmd.prompt().contains("diameter"),
            "prompt should reflect Diameter mode: {}",
            cmd.prompt()
        );

        match cmd.on_text_input("16") {
            Some(CmdResult::AddParametricConstraint {
                kind,
                driving_param: Some(DrivingValue::Literal(v)),
                ..
            }) => {
                assert_eq!(kind, ConstraintKind::Diameter);
                assert_eq!(v, 16.0);
            }
            _ => panic!("expected an AddParametricConstraint{{Diameter}}"),
        }
    }

    #[test]
    fn default_mode_still_builds_a_plain_radius_constraint() {
        let mut scene = Scene::new();
        let circle = add_circle(&mut scene);
        let mut cmd =
            DistanceConstraintCommand::new(&scene, circle).expect("Circle supports DCONSTRAINT");

        match cmd.on_text_input("5") {
            Some(CmdResult::AddParametricConstraint { kind, .. }) => {
                assert_eq!(kind, ConstraintKind::Radius)
            }
            _ => panic!("expected an AddParametricConstraint{{Radius}}"),
        }
    }
}
