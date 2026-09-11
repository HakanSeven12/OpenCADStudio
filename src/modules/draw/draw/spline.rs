// Spline tool — ribbon definition + interactive command.
//
// Command:  SPLINE (SPL)
//   Click to add fit points. Enter (≥2 pts) → commits EntityType::Spline.

use crate::t;
use acadrust::types::Vector3;
use acadrust::{CadDocument, EntityType, Handle, Spline};
use std::collections::HashMap;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;
use glam::DVec3;

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "SPLINE",
        label: "Spline",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/spline.svg")),
        event: ModuleEvent::Command("SPLINE".to_string()),
    }
}

pub struct SplineCommand {
    pts: Vec<DVec3>,
    control_vertices: bool,
    choosing_method: bool,
    choosing_knots: bool,
    choosing_tangent: bool,
    knot_parameterization: i32,
    begin_tangent: Vector3,
    end_tangent: Vector3,
    choosing_objects: bool,
    convertible: HashMap<Handle, Spline>,
    selected_objects: Vec<Handle>,
    choosing_degree: bool,
    degree: usize,
}

impl SplineCommand {
    pub fn new() -> Self {
        Self {
            pts: Vec::new(),
            control_vertices: false,
            choosing_method: false,
            choosing_knots: false,
            choosing_tangent: false,
            knot_parameterization: 0,
            begin_tangent: Vector3::ZERO,
            end_tangent: Vector3::ZERO,
            choosing_objects: false,
            convertible: HashMap::new(),
            selected_objects: Vec::new(),
            choosing_degree: false,
            degree: 3,
        }
    }

    pub fn control_vertices() -> Self {
        Self {
            control_vertices: true,
            ..Self::new()
        }
    }

    pub fn with_document(mut self, document: &CadDocument) -> Self {
        self.convertible = document.entities().filter_map(|entity| {
            spline_from_polyline(entity).map(|spline| (entity.common().handle, spline))
        }).collect();
        self
    }

    fn build(&self, closed: bool) -> Option<EntityType> {
        if self.pts.len() < 2 {
            return None;
        }
        let mut spline = if self.control_vertices {
            let points: Vec<_> = self.pts.iter().map(|point| point.to_array()).collect();
            control_spline(&points, self.degree.min(points.len() - 1), closed)?
        } else {
            make_spline(&self.pts, closed)
        };
        spline.knot_parameterization = self.knot_parameterization;
        spline.begin_tangent = self.begin_tangent;
        spline.end_tangent = self.end_tangent;
        Some(EntityType::Spline(spline))
    }
}

fn control_spline(points: &[[f64; 3]], degree: usize, closed: bool) -> Option<Spline> {
    let curve = cadkernel::space::NurbsCurve3::from_control_polygon(degree, points, closed)?;
    let mut spline = Spline {
        degree: curve.degree() as i32,
        control_points: curve.control_points().iter().map(|p| Vector3::new(p[0], p[1], p[2])).collect(),
        knots: curve.knots().to_vec(),
        weights: curve.weights().to_vec(),
        ..Default::default()
    };
    spline.flags.closed = closed;
    spline.flags.periodic = closed;
    spline.flags.planar = crate::entities::curve::spline_is_planar(&spline);
    Some(spline)
}

fn spline_from_polyline(entity: &EntityType) -> Option<Spline> {
    let (degree, points, closed, normal) = match entity {
        EntityType::Polyline2D(poly) if poly.flags.is_spline_fit() => {
            let degree = match poly.smooth_surface as i16 { 5 => 2, 6 => 3, _ => return None };
            let plane = crate::entities::curve::ocs_plane(poly.normal, poly.elevation);
            let controls: Vec<_> = poly.vertices.iter().filter(|vertex| vertex.flags.bits() & 16 != 0)
                .map(|vertex| plane.point_at([vertex.location.x, vertex.location.y])).collect();
            (degree, controls, poly.is_closed(), poly.normal)
        }
        EntityType::Polyline3D(poly) if poly.flags.spline_fit => {
            let degree = match poly.smooth_type as i16 { 5 => 2, 6 => 3, _ => return None };
            let controls: Vec<_> = poly.vertices.iter().filter(|vertex| vertex.flags & 16 != 0)
                .map(|vertex| [vertex.position.x, vertex.position.y, vertex.position.z]).collect();
            (degree, controls, poly.is_closed(), poly.normal)
        }
        _ => return None,
    };
    let mut spline = control_spline(&points, degree, closed)?;
    spline.normal = normal;
    spline.common = entity.common().clone();
    spline.common.handle = Handle::NULL;
    Some(spline)
}

/// Store the chosen construction method directly in the persistent spline.
fn make_spline(pts: &[DVec3], closed: bool) -> Spline {
    let mut spline = Spline {
        degree: 3,
        fit_points: pts
            .iter()
            .map(|point| Vector3::new(point.x, point.y, point.z))
            .collect(),
        ..Default::default()
    };
    spline.flags.closed = closed;
    // Closed fit curves use the kernel's periodic interpolation rather than
    // appending a closing straight segment to an open interpolant.
    spline.flags.periodic = closed;
    spline
}

impl CadCommand for SplineCommand {
    fn name(&self) -> &'static str {
        if self.control_vertices {
            "SPLINECV"
        } else {
            "SPLINE"
        }
    }

    fn prompt(&self) -> String {
        if self.choosing_objects { return t!("Select spline-fit polylines:").to_string(); }
        if self.choosing_degree {
            format!("SPLINE  Enter degree of spline <{}>:", self.degree)
        } else if self.choosing_method {
            "SPLINE  Choose creation method [Fit/Control vertices]:".into()
        } else if self.choosing_knots {
            "SPLINE  Enter knot parameterization [Chord/Square root/Uniform]:".into()
        } else if self.choosing_tangent {
            "SPLINE  Specify tangent direction:".into()
        } else if self.pts.is_empty() && self.control_vertices {
            t!("SPLINE  Specify first control point:").into_owned()
        } else if self.pts.is_empty() {
            t!("SPLINE  Specify first point:").into_owned()
        } else {
            let n = self.pts.len();
            t!("SPLINE  Specify next point  [%{n} pts]:", n = n).into_owned()
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        use crate::command::CmdOption;
        if self.choosing_objects { return Vec::new(); }
        if self.choosing_method {
            return vec![
                CmdOption::new("Fit", "FIT"),
                CmdOption::new("Control vertices", "CV"),
            ];
        }
        if self.choosing_degree { return vec![]; }
        if self.choosing_tangent { return vec![]; }
        if self.choosing_knots {
            return vec![CmdOption::new("Chord", "CH"), CmdOption::new("Square root", "S"), CmdOption::new("Uniform", "U")];
        }
        if self.pts.is_empty() {
            let mut options = vec![CmdOption::new(t!("Method").as_ref(), "M"), CmdOption::new("Object", "O")];
            if self.control_vertices {
                options.push(CmdOption::new("Degree", "D"));
            } else {
                options.push(CmdOption::new("Knots", "K"));
            }
            return options;
        }
        let mut opts = vec![];
        if self.pts.len() >= 3 { opts.push(CmdOption::new(t!("Close").as_ref(), "C")); }
        if !self.control_vertices { opts.push(CmdOption::new("Tangency", "T")); }
        // Undo only makes sense once a control point exists.
        opts.push(CmdOption::new(t!("Undo").as_ref(), "U"));
        opts.push(CmdOption::enter(t!("Done").as_ref()));
        opts
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.choosing_objects || self.choosing_method || self.choosing_knots
            || self.choosing_degree || !pt.is_finite() {
            return CmdResult::NeedPoint;
        }
        if self.choosing_tangent {
            let Some(anchor) = self.pts.last() else { return CmdResult::NeedPoint; };
            let Some(dir) = (pt - *anchor).try_normalize() else { return CmdResult::NeedPoint; };
            let tangent = Vector3::new(dir.x, dir.y, dir.z);
            if self.pts.len() == 1 { self.begin_tangent = tangent; }
            else { self.end_tangent = tangent; }
            self.choosing_tangent = false;
            if self.pts.len() > 1 {
                return self.build(false).map_or(CmdResult::NeedPoint, CmdResult::CommitAndExit);
            }
            return CmdResult::NeedPoint;
        }
        if self.pts.last().is_some_and(|last| last.distance_squared(pt) < 1e-20) {
            return CmdResult::NeedPoint;
        }
        self.pts.push(pt);
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.choosing_objects {
            let replacements = self.selected_objects.iter().filter_map(|handle| {
                self.convertible.get(handle).map(|spline| (*handle, vec![EntityType::Spline(spline.clone())]))
            }).collect::<Vec<_>>();
            return if replacements.is_empty() { CmdResult::Cancel }
                else { CmdResult::ReplaceMany(replacements, Vec::new()) };
        }

        if self.choosing_method || self.choosing_knots || self.choosing_degree {
            self.choosing_degree = false;
            self.choosing_method = false;
            self.choosing_knots = false;
            return CmdResult::NeedPoint;
        }
        if self.choosing_tangent { return CmdResult::NeedPoint; }
        match self.build(false) {
            Some(e) => CmdResult::CommitAndExit(e),
            None => CmdResult::Cancel,
        }
    }

    fn enter_accepts_default_start(&self) -> bool {
        self.pts.is_empty() && !self.choosing_method && !self.choosing_objects
            && !self.choosing_knots && !self.choosing_degree && !self.choosing_tangent
    }

    fn is_selection_gathering(&self) -> bool { self.choosing_objects }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.selected_objects = handles.into_iter().filter(|handle| self.convertible.contains_key(handle)).collect();
        CmdResult::NeedPoint
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.pts.is_empty() { return None; }
        self.pts.pop();
        self.end_tangent = Vector3::ZERO;
        self.choosing_tangent = false;
        if self.pts.is_empty() { self.begin_tangent = Vector3::ZERO; }
        Some(CmdResult::NeedPoint)
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.choosing_tangent { return None; }
        if self.choosing_degree {
            self.degree = text.trim().parse::<usize>().ok().filter(|d| (1..=10).contains(d))?;
            self.choosing_degree = false;
            return Some(CmdResult::NeedPoint);
        }
        if self.choosing_knots {
            self.knot_parameterization = match text.trim().to_ascii_uppercase().as_str() {
                "CH" | "CHORD" => 0,
                "S" | "SQUARE" | "SQUAREROOT" => 1,
                "U" | "UNIFORM" => 2,
                _ => return None,
            };
            self.choosing_knots = false;
            return Some(CmdResult::NeedPoint);
        }
        match text.trim().to_uppercase().as_str() {
            "D" | "DEGREE" if self.pts.is_empty() && self.control_vertices => {
                self.choosing_degree = true;
                Some(CmdResult::NeedPoint)
            }
            "K" | "KNOTS" if self.pts.is_empty() && !self.control_vertices => {
                self.choosing_knots = true;
                Some(CmdResult::NeedPoint)
            }
            "T" | "TANGENCY" if !self.pts.is_empty() && !self.control_vertices => {
                self.choosing_tangent = true;
                Some(CmdResult::NeedPoint)
            }
            "O" | "OBJECT" if self.pts.is_empty() => {
                self.choosing_objects = true;
                Some(CmdResult::NeedPoint)
            }
            "M" | "METHOD" if self.pts.is_empty() => {
                self.choosing_method = true;
                Some(CmdResult::NeedPoint)
            }
            "F" | "FIT" if self.pts.is_empty() => {
                self.control_vertices = false;
                self.choosing_method = false;
                Some(CmdResult::NeedPoint)
            }
            "CV" | "CONTROL" | "CONTROLVERTICES" if self.pts.is_empty() => {
                self.control_vertices = true;
                self.choosing_method = false;
                Some(CmdResult::NeedPoint)
            }
            "C" | "CLOSE" if self.pts.len() >= 3 => match self.build(true) {
                Some(e) => Some(CmdResult::CommitAndExit(e)),
                None => Some(CmdResult::NeedPoint),
            },
            "U" | "UNDO" => {
                self.on_undo_step().or(Some(CmdResult::NeedPoint))
            }
            _ => None,
        }
    }

    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if self.pts.is_empty() || self.choosing_objects || self.choosing_method
            || self.choosing_knots || self.choosing_degree || self.choosing_tangent {
            return None;
        }
        // Preview the committed construction method.
        self.pts.push(pt);
        let entity = self.build(false);
        self.pts.pop();
        let Some(EntityType::Spline(spline)) = entity else { return None; };
        let points = crate::entities::curve::spline_curve(&spline)
            .map(|curve| crate::entities::curve::curve_points(&curve))
            .unwrap_or_else(|| crate::entities::spline::measurement_polyline(&spline));
        Some(WireModel::solid(
            "rubber_band".into(),
            points.into_iter().map(|p| [p[0] as f32, p[1] as f32, p[2] as f32]).collect(),
            WireModel::CYAN,
            false,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_vertices_make_a_clamped_finite_spline() {
        let points = [
            DVec3::new(0.0, 0.0, 0.0),
            DVec3::new(1.0, 2.0, 0.0),
            DVec3::new(2.0, 2.0, 0.0),
            DVec3::new(3.0, 0.0, 0.0),
        ];
        let controls: Vec<_> = points.iter().map(|point| point.to_array()).collect();
        let spline = control_spline(&controls, 3, false).expect("valid control polygon");

        assert_eq!(spline.degree, 3);
        assert_eq!(spline.control_points.len(), 4);
        assert_eq!(spline.knots, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
        assert_eq!(spline.weights, vec![1.0; 4]);
        assert!(crate::entities::curve::spline_curve(&spline).is_some());
    }

    #[test]
    fn fit_options_persist_parameterization_and_endpoint_tangents() {
        let mut command = SplineCommand::new();
        assert!(matches!(command.on_text_input("K"), Some(CmdResult::NeedPoint)));
        assert!(matches!(command.on_text_input("S"), Some(CmdResult::NeedPoint)));
        assert!(matches!(command.on_point(DVec3::ZERO), CmdResult::NeedPoint));
        assert!(matches!(command.on_text_input("T"), Some(CmdResult::NeedPoint)));
        assert!(matches!(command.on_point(DVec3::X), CmdResult::NeedPoint));
        assert!(matches!(command.on_point(DVec3::new(1.0, 1.0, 0.0)), CmdResult::NeedPoint));
        assert!(matches!(command.on_point(DVec3::new(2.0, 0.0, 0.0)), CmdResult::NeedPoint));
        assert!(matches!(command.on_text_input("T"), Some(CmdResult::NeedPoint)));

        let CmdResult::CommitAndExit(EntityType::Spline(spline)) =
            command.on_point(DVec3::new(2.0, 1.0, 0.0))
        else {
            panic!("end tangency must complete the fit spline");
        };
        assert_eq!(spline.knot_parameterization, 1);
        assert_eq!(spline.begin_tangent, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(spline.end_tangent, Vector3::new(0.0, 1.0, 0.0));
        assert!(crate::entities::curve::spline_curve(&spline).is_some());
    }

    #[test]
    fn control_degree_is_clamped_to_the_available_points() {
        let mut command = SplineCommand::control_vertices();
        assert!(matches!(command.on_text_input("D"), Some(CmdResult::NeedPoint)));
        assert!(matches!(command.on_text_input("10"), Some(CmdResult::NeedPoint)));
        for point in [DVec3::ZERO, DVec3::X, DVec3::Y] {
            assert!(matches!(command.on_point(point), CmdResult::NeedPoint));
        }
        let CmdResult::CommitAndExit(EntityType::Spline(spline)) = command.on_enter() else {
            panic!("valid control polygon must commit");
        };
        assert_eq!(spline.degree, 2);
        assert!(!spline.flags.closed);
        assert!(!spline.flags.periodic);
    }

    #[test]
    fn object_conversion_waits_for_enter_and_preserves_the_replacement() {
        let handle = Handle::new(17);
        let converted = control_spline(
            &[[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [3.0, 0.0, 0.0]],
            2,
            false,
        )
        .expect("valid converted spline");
        let mut command = SplineCommand::new();
        command.convertible.insert(handle, converted.clone());
        assert!(matches!(command.on_text_input("O"), Some(CmdResult::NeedPoint)));
        assert!(command.is_selection_gathering());
        assert!(matches!(
            command.on_selection_complete(vec![handle]),
            CmdResult::NeedPoint
        ));
        assert!(matches!(
            command.on_enter(),
            CmdResult::ReplaceMany(replacements, erased)
                if erased.is_empty()
                    && replacements == vec![(handle, vec![EntityType::Spline(converted)])]
        ));
    }

    #[test]
    fn closed_fitted_polyline_conversion_preserves_a_periodic_spline() {
        use acadrust::entities::{
            Polyline2D, PolylineFlags, SmoothSurfaceType, Vertex2D, VertexFlags,
        };

        let mut polyline = Polyline2D::new();
        polyline.flags = PolylineFlags::SPLINE_FIT | PolylineFlags::CLOSED;
        polyline.smooth_surface = SmoothSurfaceType::CubicBSpline;
        polyline.common.layer = "DETAIL".into();
        polyline.vertices = [
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(2.0, 0.0, 0.0),
            Vector3::new(2.0, 2.0, 0.0),
            Vector3::new(0.0, 2.0, 0.0),
        ]
        .into_iter()
        .map(|point| {
            let mut vertex = Vertex2D::new(point);
            vertex.flags = VertexFlags::SPLINE_CONTROL;
            vertex
        })
        .collect();

        let spline = spline_from_polyline(&EntityType::Polyline2D(polyline))
            .expect("fitted polyline must convert");
        assert_eq!(spline.degree, 3);
        assert!(spline.flags.closed);
        assert!(spline.flags.periodic);
        assert!(spline.flags.planar);
        assert_eq!(spline.common.layer, "DETAIL");
        assert!(crate::entities::curve::spline_curve(&spline).is_some());
    }

    #[test]
    fn duplicate_points_and_point_undo_keep_tangent_state_consistent() {
        let mut command = SplineCommand::new();
        assert!(matches!(command.on_point(DVec3::ZERO), CmdResult::NeedPoint));
        assert!(matches!(command.on_point(DVec3::ZERO), CmdResult::NeedPoint));
        assert_eq!(command.pts.len(), 1);
        assert!(matches!(command.on_text_input("T"), Some(CmdResult::NeedPoint)));
        assert!(matches!(command.on_point(DVec3::X), CmdResult::NeedPoint));
        assert_ne!(command.begin_tangent, Vector3::ZERO);
        assert!(command.on_undo_step().is_some());
        assert!(command.pts.is_empty());
        assert_eq!(command.begin_tangent, Vector3::ZERO);
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["SPLINE", "SPLINECV"]
}); // SplineCommand
