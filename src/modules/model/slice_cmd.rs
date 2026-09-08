use acadrust::{EntityType, Handle};
use cadkernel::space::Plane;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, SelectionEntity, WorkingPlane};
use crate::scene::model::wire_model::WireModel;

#[derive(Clone, Copy)]
enum PlaneKind {
    Xy,
    Yz,
    Zx,
    View,
}

enum Step {
    Targets,
    FirstPoint,
    ThreeFirst,
    ThreeSecond(DVec3),
    ThirdPoint(DVec3, DVec3),
    AxisFirst,
    AxisSecond(DVec3),
    PlanePoint(PlaneKind),
    PickObject,
    PickSurface,
    Side(Plane),
    SurfaceSide,
}

/// Interactive front-end for plane-based SLICE.
pub struct SliceCommand {
    step: Step,
    targets: Vec<Handle>,
    selected: Vec<Handle>,
    working: WorkingPlane,
    view_normal: DVec3,
    preview_center: DVec3,
    preview_radius: f64,
    picked: Option<EntityType>,
    surface_tool: Option<cadkernel::brep::Body>,
    notice: Option<&'static str>,
}

impl SliceCommand {
    pub fn new(
        targets: Vec<Handle>,
        view_normal: DVec3,
        preview_center: DVec3,
        preview_radius: f64,
    ) -> Self {
        let step = if targets.is_empty() { Step::Targets } else { Step::FirstPoint };
        Self {
            step,
            targets,
            selected: Vec::new(),
            working: WorkingPlane::default(),
            view_normal: view_normal.normalize_or(DVec3::Z),
            preview_center,
            preview_radius: preview_radius.max(1.0),
            picked: None,
            surface_tool: None,
            notice: None,
        }
    }

    fn plane(origin: DVec3, x: DVec3, normal: DVec3) -> Option<Plane> {
        Plane::orthonormal(origin.to_array(), x.to_array(), normal.to_array())
    }

    fn enter_side(&mut self, plane: Plane) -> CmdResult {
        self.notice = None;
        self.step = Step::Side(plane);
        CmdResult::NeedPoint
    }

    fn finish(&mut self, keep_point: Option<DVec3>) -> CmdResult {
        let Step::Side(plane) = self.step else {
            return CmdResult::NeedPoint;
        };
        CmdResult::SliceEntities {
            targets: std::mem::take(&mut self.targets),
            plane,
            keep_point,
        }
    }

    fn picked_plane(&self) -> Option<Plane> {
        let entity = self.picked.as_ref()?;
        if !matches!(
            entity,
            EntityType::Arc(_)
                | EntityType::Circle(_)
                | EntityType::Ellipse(_)
                | EntityType::Spline(_)
                | EntityType::LwPolyline(_)
                | EntityType::Polyline(_)
        ) {
            return None;
        }
        crate::scene::model::presspull_model::profile_geometry(entity)
            .map(|(plane, _, _)| plane)
    }

    fn finish_surface(&mut self, keep_point: Option<DVec3>) -> CmdResult {
        let Some(cutter) = self.surface_tool.take() else {
            return CmdResult::NeedPoint;
        };
        CmdResult::SliceSurfaceEntities {
            targets: std::mem::take(&mut self.targets),
            cutter: Box::new(cutter),
            keep_point,
        }
    }

    fn plane_at(&self, point: DVec3, kind: PlaneKind) -> Option<Plane> {
        let (x, normal) = match kind {
            PlaneKind::Xy => (self.working.x, self.working.z),
            PlaneKind::Yz => (self.working.y, self.working.x),
            PlaneKind::Zx => (self.working.z, self.working.y),
            PlaneKind::View => {
                let candidate = if self.working.x.dot(self.view_normal).abs() < 0.98 {
                    self.working.x
                } else {
                    self.working.y
                };
                (candidate, self.view_normal)
            }
        };
        Self::plane(point, x, normal)
    }
}

impl CadCommand for SliceCommand {
    fn name(&self) -> &'static str {
        "SLICE"
    }

    fn prompt(&self) -> String {
        if let Some(notice) = self.notice {
            return crate::t!(notice).into_owned();
        }
        match self.step {
            Step::Targets => crate::t!(
                "SLICE  Select solids or surfaces to slice, then press Enter:"
            )
            .into_owned(),
            Step::FirstPoint => crate::t!(
                "SLICE  Specify first point on slicing plane or [planar Object/Surface/Zaxis/View/XY/YZ/ZX/3points] <3points>:"
            )
            .into_owned(),
            Step::ThreeFirst => {
                crate::t!("SLICE  Specify first point on slicing plane:").into_owned()
            }
            Step::ThreeSecond(_) => {
                crate::t!("SLICE  Specify second point on slicing plane:").into_owned()
            }
            Step::ThirdPoint(_, _) => {
                crate::t!("SLICE  Specify third point on slicing plane:").into_owned()
            }
            Step::AxisFirst => {
                crate::t!("SLICE  Specify a point on the slicing plane:").into_owned()
            }
            Step::AxisSecond(_) => {
                crate::t!("SLICE  Specify a point on the plane's normal axis:").into_owned()
            }
            Step::PlanePoint(PlaneKind::View) => {
                crate::t!("SLICE  Specify a point on the current view plane:").into_owned()
            }
            Step::PlanePoint(_) => {
                crate::t!("SLICE  Specify a point on the current UCS plane:").into_owned()
            }
            Step::PickObject => crate::t!("SLICE  Select a planar curve object:").into_owned(),
            Step::PickSurface => crate::t!("SLICE  Select a cutting surface:").into_owned(),
            Step::Side(_) => crate::t!(
                "SLICE  Specify a point on the desired side or [Both] <Both>:"
            )
            .into_owned(),
            Step::SurfaceSide => crate::t!(
                "SLICE  Select a sliced object to keep or [Both] <Both>:"
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::FirstPoint => vec![
                CmdOption::new(crate::t!("planar Object").as_ref(), "O"),
                CmdOption::new(crate::t!("Surface").as_ref(), "S"),
                CmdOption::new(crate::t!("Zaxis").as_ref(), "Z"),
                CmdOption::new(crate::t!("View").as_ref(), "V"),
                CmdOption::new("XY", "XY"),
                CmdOption::new("YZ", "YZ"),
                CmdOption::new("ZX", "ZX"),
                CmdOption::new(crate::t!("3points").as_ref(), "3"),
            ],
            Step::Side(_) | Step::SurfaceSide => {
                vec![CmdOption::new(crate::t!("Both").as_ref(), "B")]
            }
            _ => Vec::new(),
        }
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.working = plane;
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        self.notice = None;
        match self.step {
            Step::FirstPoint => {
                self.step = Step::ThreeSecond(point);
                CmdResult::NeedPoint
            }
            Step::ThreeFirst => {
                self.step = Step::ThreeSecond(point);
                CmdResult::NeedPoint
            }
            Step::ThreeSecond(first) => {
                self.step = Step::ThirdPoint(first, point);
                CmdResult::NeedPoint
            }
            Step::ThirdPoint(first, second) => {
                let x = second - first;
                let normal = x.cross(point - first);
                Self::plane(first, x, normal)
                    .map_or(CmdResult::NeedPoint, |plane| self.enter_side(plane))
            }
            Step::AxisFirst => {
                self.step = Step::AxisSecond(point);
                CmdResult::NeedPoint
            }
            Step::AxisSecond(origin) => {
                let normal = point - origin;
                let x = if self.working.x.dot(normal.normalize_or(DVec3::Z)).abs() < 0.98 {
                    self.working.x
                } else {
                    self.working.y
                };
                Self::plane(origin, x, normal)
                    .map_or(CmdResult::NeedPoint, |plane| self.enter_side(plane))
            }
            Step::PlanePoint(kind) => self
                .plane_at(point, kind)
                .map_or(CmdResult::NeedPoint, |plane| self.enter_side(plane)),
            Step::Side(_) => self.finish(Some(point)),
            Step::SurfaceSide => self.finish_surface(Some(point)),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Targets if !self.selected.is_empty() => {
                self.targets = std::mem::take(&mut self.selected);
                self.step = Step::FirstPoint;
                CmdResult::DeselectAndContinue
            }
            Step::FirstPoint => {
                self.step = Step::ThreeFirst;
                CmdResult::NeedPoint
            }
            Step::Side(_) => self.finish(None),
            Step::SurfaceSide => self.finish_surface(None),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().to_ascii_uppercase();
        Some(match self.step {
            Step::FirstPoint => match keyword.as_str() {
                "O" | "OBJECT" => {
                    self.step = Step::PickObject;
                    CmdResult::NeedPoint
                }
                "S" | "SURFACE" => {
                    self.step = Step::PickSurface;
                    CmdResult::NeedPoint
                }
                "Z" | "ZAXIS" => {
                    self.step = Step::AxisFirst;
                    CmdResult::NeedPoint
                }
                "V" | "VIEW" => {
                    self.step = Step::PlanePoint(PlaneKind::View);
                    CmdResult::NeedPoint
                }
                "XY" => {
                    self.step = Step::PlanePoint(PlaneKind::Xy);
                    CmdResult::NeedPoint
                }
                "YZ" => {
                    self.step = Step::PlanePoint(PlaneKind::Yz);
                    CmdResult::NeedPoint
                }
                "ZX" | "XZ" => {
                    self.step = Step::PlanePoint(PlaneKind::Zx);
                    CmdResult::NeedPoint
                }
                "3" | "3P" | "3POINTS" => {
                    self.step = Step::ThreeFirst;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::NeedPoint,
            },
            Step::Side(_) if matches!(keyword.as_str(), "" | "B" | "BOTH") => self.finish(None),
            Step::SurfaceSide if matches!(keyword.as_str(), "" | "B" | "BOTH") => {
                self.finish_surface(None)
            }
            _ => CmdResult::NeedPoint,
        })
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::FirstPoint | Step::Side(_) | Step::SurfaceSide)
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::Targets)
    }

    fn selection_forces_add(&self) -> bool {
        true
    }

    fn inject_selection_entities(&mut self, entities: Vec<SelectionEntity>) {
        self.selected = entities
            .into_iter()
            .filter_map(|item| {
                matches!(item.entity, EntityType::Solid3D(_) | EntityType::Surface(_))
                    .then_some(item.handle)
            })
            .collect();
    }

    fn on_selection_complete(&mut self, _handles: Vec<Handle>) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(
            self.step,
            Step::PickObject | Step::PickSurface | Step::SurfaceSide
        )
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        self.needs_entity_pick()
    }

    fn inject_before_entity_pick(&self) -> bool {
        matches!(self.step, Step::PickObject | Step::PickSurface)
    }

    fn entity_pick_uses_surface_point(&self) -> bool {
        matches!(self.step, Step::SurfaceSide)
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, _handle: Handle, _point: DVec3) -> CmdResult {
        if matches!(self.step, Step::SurfaceSide) {
            return self.finish_surface(Some(_point));
        }
        let surface_only = matches!(self.step, Step::PickSurface);
        if surface_only {
            let body = self.picked.as_ref().and_then(|entity| match entity {
                EntityType::Surface(surface) => {
                    crate::scene::convert::solid3d_tess::kernel_surface_body(surface)
                }
                _ => None,
            });
            if let Some(body) = body {
                if let Some(plane) = planar_body_plane(&body) {
                    return self.enter_side(plane);
                }
                if body.face_keys().count() == 1 {
                    self.surface_tool = Some(body);
                    self.notice = None;
                    self.step = Step::SurfaceSide;
                    return CmdResult::NeedPoint;
                }
            }
            self.notice = Some(
                "SLICE  The selected surface does not define one supported cutting sheet. Select another surface:",
            );
            return CmdResult::NeedPoint;
        }
        match self.picked_plane() {
            Some(plane) => self.enter_side(plane),
            None => {
                self.notice = Some(
                    "SLICE  The selected object does not define a plane. Select a planar curve object:",
                );
                CmdResult::NeedPoint
            }
        }
    }

    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        let mut wires = Vec::new();
        let plane = match self.step {
            Step::ThirdPoint(first, second) => {
                wires.push(WireModel::solid_f64(
                    "slice_triangle".into(),
                    vec![
                        first.to_array(),
                        second.to_array(),
                        point.to_array(),
                        first.to_array(),
                    ],
                    WireModel::CYAN,
                    false,
                ));
                Self::plane(first, second - first, (second - first).cross(point - first))
            }
            Step::AxisSecond(origin) => {
                wires.push(WireModel::solid_f64(
                    "slice_normal".into(),
                    vec![origin.to_array(), point.to_array()],
                    WireModel::CYAN,
                    false,
                ));
                let normal = point - origin;
                let x = if self.working.x.dot(normal.normalize_or(DVec3::Z)).abs() < 0.98 {
                    self.working.x
                } else {
                    self.working.y
                };
                Self::plane(origin, x, normal)
            }
            Step::PlanePoint(kind) => self.plane_at(point, kind),
            Step::Side(plane) => Some(plane),
            _ => None,
        };
        if let Some(plane) = plane {
            wires.extend(plane_grid(
                plane,
                self.preview_center,
                self.preview_radius,
            ));
        }
        wires
    }
}

fn planar_body_plane(body: &cadkernel::brep::Body) -> Option<Plane> {
    let mut faces = body.face_keys();
    let first = cadkernel::brep::planar_face_profile(body, faces.next()?)?.plane;
    let normal = DVec3::from_array(first.normal()?);
    let tolerance = cadkernel::brep::operation_tolerance(&[body]);
    faces
        .all(|face| {
            let Some(profile) = cadkernel::brep::planar_face_profile(body, face) else {
                return false;
            };
            profile.plane.normal().is_some_and(|other| {
                normal.dot(DVec3::from_array(other)).abs() >= 1.0 - 1e-9
                    && first
                        .distance_to(profile.plane.origin)
                        .is_some_and(|gap| gap.abs() <= tolerance * 4.0)
            })
        })
        .then_some(first)
}

fn plane_grid(plane: Plane, centre: DVec3, radius: f64) -> Vec<WireModel> {
    let normal = DVec3::from_array(plane.normal().unwrap_or([0.0, 0.0, 1.0]));
    let projected = centre
        - normal
            * plane
                .distance_to(centre.to_array())
                .unwrap_or_default();
    let x = DVec3::from_array(plane.x_axis);
    let y = DVec3::from_array(plane.y_axis);
    let color = [0.2, 0.55, 1.0, 1.0];
    (-5..=5)
        .flat_map(|index| {
            let offset = radius * index as f64 / 5.0;
            [
                WireModel::solid_f64(
                    format!("slice_grid_x_{index}"),
                    vec![
                        (projected + x * offset - y * radius).to_array(),
                        (projected + x * offset + y * radius).to_array(),
                    ],
                    color,
                    false,
                ),
                WireModel::solid_f64(
                    format!("slice_grid_y_{index}"),
                    vec![
                        (projected + y * offset - x * radius).to_array(),
                        (projected + y * offset + x * radius).to_array(),
                    ],
                    color,
                    false,
                ),
            ]
        })
        .collect()
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["SLICE", "SL"]
});
