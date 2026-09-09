use acadrust::entities::{EntityCommon, ExtendedEntity, ExtendedEntityData, SectionObjectData};
use acadrust::types::{Color, Handle, Vector3};
use acadrust::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SectionKind {
    Plane,
    Slice,
    Boundary,
    Volume,
}

impl SectionKind {
    fn label(self) -> &'static str {
        match self {
            Self::Plane => "Plane",
            Self::Slice => "Slice",
            Self::Boundary => "Boundary",
            Self::Volume => "Volume",
        }
    }

    fn state(self) -> i32 {
        match self {
            Self::Plane | Self::Slice => 1,
            Self::Boundary => 2,
            Self::Volume => 4,
        }
    }
}

#[derive(Clone, Copy)]
enum Orthographic {
    Front,
    Back,
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Locate,
    Through,
    DrawStart,
    DrawNext,
    DrawDirection,
    Orthographic,
    Type,
}

/// Creates persistent SECTIONOBJECT entities without modifying source solids.
pub struct SectionPlaneCommand {
    step: Step,
    kind: SectionKind,
    first: Option<DVec3>,
    draw_points: Vec<DVec3>,
    picked_direction: Option<DVec3>,
    working: WorkingPlane,
    bounds: (DVec3, DVec3),
    empty_extents: bool,
    next_name: usize,
    notice: Option<&'static str>,
}

impl SectionPlaneCommand {
    pub fn new(bounds: Option<(DVec3, DVec3)>, next_name: usize) -> Self {
        let empty_extents = bounds.is_none();
        let bounds = bounds.unwrap_or((DVec3::ZERO, DVec3::splat(4000.0)));
        Self {
            step: Step::Locate,
            kind: SectionKind::Plane,
            first: None,
            draw_points: Vec::new(),
            picked_direction: None,
            working: WorkingPlane::default(),
            bounds,
            empty_extents,
            next_name: next_name.max(1),
            notice: None,
        }
    }

    fn corners(&self) -> [DVec3; 8] {
        let (lo, hi) = self.bounds;
        [
            DVec3::new(lo.x, lo.y, lo.z),
            DVec3::new(hi.x, lo.y, lo.z),
            DVec3::new(lo.x, hi.y, lo.z),
            DVec3::new(hi.x, hi.y, lo.z),
            DVec3::new(lo.x, lo.y, hi.z),
            DVec3::new(hi.x, lo.y, hi.z),
            DVec3::new(lo.x, hi.y, hi.z),
            DVec3::new(hi.x, hi.y, hi.z),
        ]
    }

    fn frame_extents(&self, centre: DVec3, tangent: DVec3, vertical: DVec3) -> (f64, f64, f64, f64) {
        let mut t_min = f64::INFINITY;
        let mut t_max = f64::NEG_INFINITY;
        let mut v_min = f64::INFINITY;
        let mut v_max = f64::NEG_INFINITY;
        for corner in self.corners() {
            let delta = corner - centre;
            let t = delta.dot(tangent);
            let v = delta.dot(vertical);
            t_min = t_min.min(t);
            t_max = t_max.max(t);
            v_min = v_min.min(v);
            v_max = v_max.max(v);
        }
        let t_span = t_max - t_min;
        let v_span = v_max - v_min;
        let t_margin = if t_span > 1e-12 { t_span * 0.1 } else { 1.0 };
        let v_margin = if v_span > 1e-12 {
            v_span * if self.empty_extents { 0.1 } else { 0.15 }
        } else {
            1.0
        };
        (t_min - t_margin, t_max + t_margin, v_min - v_margin, v_max + v_margin)
    }

    fn entity(&self, vertices: Vec<DVec3>, vertical: DVec3, positive_view: bool) -> EntityType {
        let first = vertices.first().copied().unwrap_or(DVec3::ZERO);
        let last = vertices.last().copied().unwrap_or(DVec3::X);
        let tangent = (last - first).normalize_or(DVec3::X);
        let vertical = (vertical - tangent * vertical.dot(tangent)).normalize_or(DVec3::Z);
        let viewing = vertical.cross(tangent).normalize_or(-DVec3::Z)
            * if positive_view { 1.0 } else { -1.0 };
        let centre = (first + last) * 0.5;
        let (_, _, v_min, v_max) = self.frame_extents(centre, tangent, vertical);
        let span = (last - first).length().max(1.0);
        let depth = match self.kind {
            SectionKind::Plane => 0.0,
            SectionKind::Slice => (span / 60.0).max(1e-4),
            SectionKind::Boundary | SectionKind::Volume => span,
        };
        let back_line_vertices = if matches!(self.kind, SectionKind::Boundary | SectionKind::Volume) {
            vertices.iter().map(|point| *point + viewing * depth).collect()
        } else {
            Vec::new()
        };
        let mut entity = ExtendedEntity {
            common: EntityCommon::new(),
            data: ExtendedEntityData::SectionObject(SectionObjectData {
                state: self.kind.state(),
                flags: 1 | if positive_view { 4 } else { 0 },
                name: format!("Section Plane ({})", self.next_name),
                vertical_direction: Vector3::new(vertical.x, vertical.y, vertical.z),
                top_height: v_max.max(0.0),
                bottom_height: (-v_min).max(0.0),
                indicator_alpha: 70,
                indicator_color: Color::from_index(9),
                vertices: vertices
                    .into_iter()
                    .map(|point| Vector3::new(point.x, point.y, point.z))
                    .collect(),
                back_line_vertices: back_line_vertices
                    .into_iter()
                    .map(|point| Vector3::new(point.x, point.y, point.z))
                    .collect(),
                settings_handle: Handle::NULL,
            }),
        };
        crate::entities::extended::set_section_slice_metadata(
            &mut entity,
            self.kind == SectionKind::Slice,
            depth,
        );
        EntityType::Extended(entity)
    }

    fn line_at(&self, point: DVec3, viewing: DVec3, vertical_hint: DVec3) -> (Vec<DVec3>, DVec3) {
        let viewing = viewing.normalize_or(-DVec3::Z);
        let vertical_candidate = vertical_hint - viewing * vertical_hint.dot(viewing);
        let vertical = if vertical_candidate.length_squared() > 1e-18 {
            vertical_candidate.normalize()
        } else {
            viewing.any_orthonormal_vector()
        };
        let tangent = viewing.cross(vertical).normalize_or(DVec3::X);
        let (t_min, t_max, _, _) = self.frame_extents(point, tangent, vertical);
        (vec![point + tangent * t_min, point + tangent * t_max], vertical)
    }

    fn orthographic(&self, kind: Orthographic) -> EntityType {
        let (lo, hi) = self.bounds;
        let centre = (lo + hi) * 0.5;
        let (viewing, vertical, positive) = match kind {
            Orthographic::Front => (DVec3::Y, DVec3::Z, true),
            Orthographic::Back => (DVec3::Y, DVec3::Z, false),
            Orthographic::Top => (-DVec3::Z, DVec3::Y, true),
            Orthographic::Bottom => (-DVec3::Z, DVec3::Y, false),
            Orthographic::Left => (-DVec3::X, DVec3::Z, false),
            Orthographic::Right => (-DVec3::X, DVec3::Z, true),
        };
        let (vertices, vertical) = self.line_at(centre, viewing, vertical);
        self.entity(vertices, vertical, positive)
    }

    fn finish_through(&mut self, through: DVec3) -> CmdResult {
        let Some(first) = self.first else {
            return CmdResult::NeedPoint;
        };
        let tangent = through - first;
        if tangent.length_squared() <= 1e-18 {
            self.notice = Some("SECTIONPLANE  The through point must differ from the first point:");
            return CmdResult::NeedPoint;
        }
        let tangent = tangent.normalize();
        let vertical_hint = if tangent.dot(self.working.z).abs() < 0.98 {
            self.working.z
        } else {
            self.working.y
        };
        let vertical = (vertical_hint - tangent * vertical_hint.dot(tangent)).normalize();
        CmdResult::CommitAndExit(self.entity(vec![first, through], vertical, true))
    }

    fn finish_draw(&mut self, direction: DVec3) -> CmdResult {
        if self.draw_points.len() < 2 {
            self.notice = Some("SECTIONPLANE  Specify at least two different section points:");
            self.step = Step::DrawNext;
            return CmdResult::NeedPoint;
        }
        let first = self.draw_points[0];
        let last = *self.draw_points.last().unwrap_or(&first);
        let tangent = (last - first).normalize_or(DVec3::X);
        let vertical_hint = if tangent.dot(self.working.z).abs() < 0.98 {
            self.working.z
        } else {
            self.working.y
        };
        let vertical = (vertical_hint - tangent * vertical_hint.dot(tangent)).normalize();
        let base_view = vertical.cross(tangent).normalize_or(-DVec3::Z);
        let centre = (first + last) * 0.5;
        let positive = (direction - centre).dot(base_view) >= 0.0;
        let vertices = std::mem::take(&mut self.draw_points);
        CmdResult::CommitAndExit(self.entity(vertices, vertical, positive))
    }

    fn choose_kind(&mut self, kind: SectionKind) -> CmdResult {
        self.kind = kind;
        self.step = Step::Locate;
        self.notice = None;
        CmdResult::NeedPoint
    }
}

impl CadCommand for SectionPlaneCommand {
    fn name(&self) -> &'static str {
        "SECTIONPLANE"
    }

    fn prompt(&self) -> String {
        if let Some(notice) = self.notice {
            return crate::t!(notice).into_owned();
        }
        match self.step {
            Step::Locate => format!(
                "SECTIONPLANE  Type = {}\n{}",
                self.kind.label(),
                crate::t!("Select face or any point to locate section line or [Draw section/Orthographic/Type]:")
            ),
            Step::Through => crate::t!("SECTIONPLANE  Specify through point:").into_owned(),
            Step::DrawStart => crate::t!("SECTIONPLANE  Specify start point:").into_owned(),
            Step::DrawNext if self.draw_points.len() == 1 => {
                crate::t!("SECTIONPLANE  Specify next point:").into_owned()
            }
            Step::DrawNext => crate::t!("SECTIONPLANE  Specify next point or ENTER to complete:").into_owned(),
            Step::DrawDirection => crate::t!("SECTIONPLANE  Specify point in direction of section view:").into_owned(),
            Step::Orthographic => crate::t!("SECTIONPLANE  Align section to [Front/bAck/Top/Bottom/Left/Right] <Top>:").into_owned(),
            Step::Type => crate::t!("SECTIONPLANE  Enter section plane type [Plane/Slice/Boundary/Volume] <Plane>:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Locate => vec![
                CmdOption::new(crate::t!("Draw section").as_ref(), "D"),
                CmdOption::new(crate::t!("Orthographic").as_ref(), "O"),
                CmdOption::new(crate::t!("Type").as_ref(), "T"),
            ],
            Step::Orthographic => vec![
                CmdOption::new(crate::t!("Front").as_ref(), "F"),
                CmdOption::new(crate::t!("Back").as_ref(), "A"),
                CmdOption::new(crate::t!("Top").as_ref(), "T"),
                CmdOption::new(crate::t!("Bottom").as_ref(), "B"),
                CmdOption::new(crate::t!("Left").as_ref(), "L"),
                CmdOption::new(crate::t!("Right").as_ref(), "R"),
            ],
            Step::Type => vec![
                CmdOption::new(crate::t!("Plane").as_ref(), "P"),
                CmdOption::new(crate::t!("Slice").as_ref(), "S"),
                CmdOption::new(crate::t!("Boundary").as_ref(), "B"),
                CmdOption::new(crate::t!("Volume").as_ref(), "V"),
            ],
            _ => Vec::new(),
        }
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.working = plane;
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        self.notice = None;
        match self.step {
            Step::Locate => {
                self.first = Some(point);
                self.step = Step::Through;
                CmdResult::NeedPoint
            }
            Step::Through => self.finish_through(point),
            Step::DrawStart => {
                self.draw_points.clear();
                self.draw_points.push(point);
                self.step = Step::DrawNext;
                CmdResult::NeedPoint
            }
            Step::DrawNext => {
                if self.draw_points.last().is_some_and(|last| last.distance(point) <= 1e-9) {
                    self.notice = Some("SECTIONPLANE  Consecutive section points must differ:");
                } else {
                    self.draw_points.push(point);
                }
                CmdResult::NeedPoint
            }
            Step::DrawDirection => self.finish_draw(point),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        self.notice = None;
        match self.step {
            Step::DrawNext if self.draw_points.len() >= 2 => {
                self.step = Step::DrawDirection;
                CmdResult::NeedPoint
            }
            Step::Orthographic => CmdResult::CommitAndExit(self.orthographic(Orthographic::Top)),
            Step::Type => self.choose_kind(SectionKind::Plane),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let keyword = text.trim().to_ascii_uppercase();
        Some(match self.step {
            Step::Locate => match keyword.as_str() {
                "D" | "DRAW" | "DRAWSECTION" => {
                    self.step = Step::DrawStart;
                    CmdResult::NeedPoint
                }
                "O" | "ORTHO" | "ORTHOGRAPHIC" => {
                    self.step = Step::Orthographic;
                    CmdResult::NeedPoint
                }
                "T" | "TYPE" => {
                    self.step = Step::Type;
                    CmdResult::NeedPoint
                }
                _ => {
                    self.notice = Some("SECTIONPLANE  Unknown option. Choose Draw section, Orthographic, or Type:");
                    CmdResult::NeedPoint
                }
            },
            Step::Orthographic => {
                let kind = match keyword.as_str() {
                    "F" | "FRONT" => Some(Orthographic::Front),
                    "A" | "BACK" => Some(Orthographic::Back),
                    "T" | "TOP" => Some(Orthographic::Top),
                    "B" | "BOTTOM" => Some(Orthographic::Bottom),
                    "L" | "LEFT" => Some(Orthographic::Left),
                    "R" | "RIGHT" => Some(Orthographic::Right),
                    _ => None,
                };
                match kind {
                    Some(kind) => CmdResult::CommitAndExit(self.orthographic(kind)),
                    None => {
                        self.notice = Some("SECTIONPLANE  Unknown alignment. Choose Front, Back, Top, Bottom, Left, or Right:");
                        CmdResult::NeedPoint
                    }
                }
            }
            Step::Type => match keyword.as_str() {
                "P" | "PLANE" => self.choose_kind(SectionKind::Plane),
                "S" | "SLICE" => self.choose_kind(SectionKind::Slice),
                "B" | "BOUNDARY" => self.choose_kind(SectionKind::Boundary),
                "V" | "VOLUME" => self.choose_kind(SectionKind::Volume),
                _ => {
                    self.notice = Some("SECTIONPLANE  Unknown type. Choose Plane, Slice, Boundary, or Volume:");
                    CmdResult::NeedPoint
                }
            },
            _ => CmdResult::NeedPoint,
        })
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Locate | Step::Orthographic | Step::Type)
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::Locate
    }

    fn entity_pick_accepts_points(&self) -> bool {
        true
    }

    fn entity_pick_uses_surface_point(&self) -> bool {
        true
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn set_entity_pick_direction(&mut self, direction: Option<DVec3>) {
        self.picked_direction = direction;
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            self.first = Some(point);
            self.step = Step::Through;
            return CmdResult::NeedPoint;
        }
        let Some(face_normal) = self.picked_direction.take().filter(|normal| normal.length_squared() > 1e-18) else {
            self.notice = Some("SECTIONPLANE  Select a planar face or specify a point:");
            return CmdResult::NeedPoint;
        };
        let viewing = -face_normal.normalize();
        let vertical = if viewing.dot(self.working.z).abs() < 0.98 {
            self.working.z
        } else {
            self.working.y
        };
        let (vertices, vertical) = self.line_at(point, viewing, vertical);
        CmdResult::CommitAndExit(self.entity(vertices, vertical, true))
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.step != Step::DrawNext {
            return None;
        }
        self.draw_points.pop();
        self.step = if self.draw_points.is_empty() { Step::DrawStart } else { Step::DrawNext };
        Some(CmdResult::NeedPoint)
    }

    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        let mut points = self.draw_points.iter().map(|p| p.to_array()).collect::<Vec<_>>();
        match self.step {
            Step::Through => {
                if let Some(first) = self.first {
                    points = vec![first.to_array(), point.to_array()];
                }
            }
            Step::DrawNext => points.push(point.to_array()),
            Step::DrawDirection if self.draw_points.len() >= 2 => {
                let centre = (self.draw_points[0] + *self.draw_points.last().unwrap()) * 0.5;
                points.push([f64::NAN; 3]);
                points.push(centre.to_array());
                points.push(point.to_array());
            }
            _ => {}
        }
        if points.len() < 2 {
            Vec::new()
        } else {
            vec![WireModel::solid_f64(
                "sectionplane_preview".into(),
                points,
                WireModel::CYAN,
                false,
            )]
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["SECTIONPLANE"]
});

#[cfg(test)]
mod tests {
    use super::*;

    fn data(entity: &EntityType) -> (&ExtendedEntity, &SectionObjectData) {
        let EntityType::Extended(entity) = entity else {
            panic!("expected extended entity");
        };
        let ExtendedEntityData::SectionObject(data) = &entity.data else {
            panic!("expected section object");
        };
        (entity, data)
    }

    #[test]
    fn slice_uses_persistent_thickness_without_boundary_geometry() {
        let mut command = SectionPlaneCommand::new(
            Some((DVec3::ZERO, DVec3::splat(10.0))),
            3,
        );
        command.kind = SectionKind::Slice;
        let entity = command.entity(vec![DVec3::ZERO, DVec3::Y * 6.0], DVec3::Z, true);
        let (entity, section) = data(&entity);

        assert_eq!(section.state, 1);
        assert!(section.back_line_vertices.is_empty());
        assert!(crate::entities::extended::section_is_slice(entity));
        assert!((crate::entities::extended::section_slice_depth(entity).unwrap() - 0.1).abs()
            < 1e-12);
    }

    #[test]
    fn boundary_keeps_a_real_back_line_and_no_slice_marker() {
        let mut command = SectionPlaneCommand::new(
            Some((DVec3::ZERO, DVec3::splat(10.0))),
            1,
        );
        command.kind = SectionKind::Boundary;
        let entity = command.entity(vec![DVec3::ZERO, DVec3::Y * 6.0], DVec3::Z, true);
        let (entity, section) = data(&entity);

        assert_eq!(section.state, 2);
        assert_eq!(section.back_line_vertices.len(), section.vertices.len());
        assert!(!crate::entities::extended::section_is_slice(entity));
    }
}
