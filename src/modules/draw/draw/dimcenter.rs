// Legacy centre cross and associative centre mark commands.

use acadrust::types::Vector3;
use acadrust::entities::{CenterMarkAssociation, CenterMarkSource};
use acadrust::{EntityType, Handle, Line};
use glam::DVec3;
use crate::t;

use crate::command::{CadCommand, CmdResult};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::centerline::{
    center_measure_is_relative, resolve_center_measure, CenterLineSettings,
};

// ── Ribbon definition ─────────────────────────────────────────────────────

#[allow(dead_code)] // ribbon definition ready for wiring; command works via the command line
pub fn tool() -> ToolDef {
    ToolDef {
        id: "CENTERMARK",
        label: "Center Mark",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("CENTERMARK".to_string()),
    }
}

// ── Command implementation ────────────────────────────────────────────────

pub struct DimCenterCommand {
    /// The picked entity, injected by the host before `on_entity_pick` runs.
    /// `None` until the host injects it.
    picked: Option<EntityType>,
}

impl DimCenterCommand {
    pub fn new() -> Self {
        Self { picked: None }
    }

    /// Extract (center, radius) from a Circle or Arc; `None` for anything else.
    fn center_radius(entity: &EntityType) -> Option<(DVec3, f64, DVec3, DVec3)> {
        match entity {
            EntityType::Circle(circle) => {
                let normal = (circle.normal.x, circle.normal.y, circle.normal.z);
                let center = crate::scene::view::transform::ocs_point_to_wcs(
                    (circle.center.x, circle.center.y, circle.center.z),
                    normal,
                );
                let (x, y) = crate::scene::view::transform::ocs_axes(normal);
                Some((
                    DVec3::new(center.0, center.1, center.2),
                    circle.radius,
                    DVec3::new(x.0, x.1, x.2),
                    DVec3::new(y.0, y.1, y.2),
                ))
            }
            EntityType::Arc(arc) => {
                let normal = (arc.normal.x, arc.normal.y, arc.normal.z);
                let center = crate::scene::view::transform::ocs_point_to_wcs(
                    (arc.center.x, arc.center.y, arc.center.z),
                    normal,
                );
                let (x, y) = crate::scene::view::transform::ocs_axes(normal);
                Some((
                    DVec3::new(center.0, center.1, center.2),
                    arc.radius,
                    DVec3::new(x.0, x.1, x.2),
                    DVec3::new(y.0, y.1, y.2),
                ))
            }
            _ => None,
        }
    }

    /// Build the two cross lines from a center and radius.
    fn build_cross(center: DVec3, radius: f64, x: DVec3, y: DVec3) -> Vec<EntityType> {
        let m = radius * 0.2;
        let to_vector = |point: DVec3| Vector3::new(point.x, point.y, point.z);
        let horizontal = Line::from_points(to_vector(center - x * m), to_vector(center + x * m));
        let vertical = Line::from_points(to_vector(center - y * m), to_vector(center + y * m));
        vec![EntityType::Line(horizontal), EntityType::Line(vertical)]
    }
}

impl CadCommand for DimCenterCommand {
    fn name(&self) -> &'static str {
        "DIMCENTER"
    }

    fn prompt(&self) -> String {
        t!("DIMCENTER  Select arc or circle:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        match self.picked.as_ref().and_then(Self::center_radius) {
            Some((center, radius, x, y)) => {
                let lines = Self::build_cross(center, radius, x, y);
                CmdResult::ReplaceMany(vec![], lines)
            }
            // Picked something that is not a circle or arc — keep prompting.
            None => CmdResult::NeedPoint,
        }
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Associative centre-mark command. Each accepted pick commits one smart mark
/// and keeps the command active so further circular objects can be selected.
pub struct CenterMarkCommand {
    picked: Option<EntityType>,
    settings: CenterLineSettings,
}

impl CenterMarkCommand {
    pub(crate) fn new(settings: CenterLineSettings) -> Self {
        Self { picked: None, settings }
    }

    fn build_mark(
        &self,
        source: CenterMarkSource,
        center: DVec3,
        radius: f64,
        x: DVec3,
        y: DVec3,
    ) -> EntityType {
        let diameter = radius * 2.0;
        let association = CenterMarkAssociation {
            source,
            plane_origin: Vector3::new(center.x, center.y, center.z),
            plane_x: Vector3::new(x.x, x.y, x.z),
            plane_y: Vector3::new(y.x, y.y, y.z),
            center: Vector3::new(center.x, center.y, center.z),
            radius,
            cross_size: resolve_center_measure(&self.settings.cross_size, diameter, 0.1),
            cross_gap: resolve_center_measure(&self.settings.cross_gap, diameter, 0.05),
            cross_size_relative: center_measure_is_relative(&self.settings.cross_size),
            cross_gap_relative: center_measure_is_relative(&self.settings.cross_gap),
            extension_length: self.settings.extension,
            length_adjustments: [0.0; 4],
            overshoots: [0.0; 4],
            show_extensions: self.settings.mark_extensions,
            associated: true,
        };
        let mut line = Line::from_points(
            Vector3::new(center.x, center.y, center.z),
            Vector3::new(center.x, center.y, center.z),
        );
        crate::scene::centermark::update_carrier(&mut line, &association);
        EntityType::Line(line)
    }
}

impl CadCommand for CenterMarkCommand {
    fn name(&self) -> &'static str { "CENTERMARK" }

    fn prompt(&self) -> String {
        t!("CENTERMARK  Select arc or circle <finish>:").into_owned()
    }

    fn needs_entity_pick(&self) -> bool { true }
    fn inject_before_entity_pick(&self) -> bool { true }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked = Some(entity);
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let Some((source, center, radius, x, y)) = self
            .picked
            .as_ref()
            .and_then(|entity| crate::scene::centermark::picked_mark_source(entity, handle, point))
        else {
            return CmdResult::NeedPoint;
        };
        CmdResult::CommitEntity(self.build_mark(source, center, radius, x, y))
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult { CmdResult::Cancel }
    fn on_escape(&mut self) -> CmdResult { CmdResult::Cancel }
}

pub struct CenterMarkReassociateCommand {
    target: Handle,
    picked: Option<EntityType>,
}

impl CenterMarkReassociateCommand {
    pub fn new(target: Handle) -> Self {
        Self { target, picked: None }
    }
}

impl CadCommand for CenterMarkReassociateCommand {
    fn name(&self) -> &'static str { "CENTERREASSOCIATE" }
    fn prompt(&self) -> String {
        t!("CENTERREASSOCIATE  Select new arc or circle:").into_owned()
    }
    fn needs_entity_pick(&self) -> bool { true }
    fn inject_before_entity_pick(&self) -> bool { true }
    fn inject_picked_entity(&mut self, entity: EntityType) { self.picked = Some(entity); }
    fn on_entity_pick(&mut self, source: Handle, point: DVec3) -> CmdResult {
        let valid = self.picked.as_ref().and_then(|entity| {
            crate::scene::centermark::picked_mark_source(entity, source, point)
        }).is_some();
        if !valid {
            return CmdResult::NeedPoint;
        }
        CmdResult::ReassociateCenterMark { target: self.target, source, point }
    }
    fn on_point(&mut self, _point: DVec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult { CmdResult::Cancel }
    fn on_escape(&mut self) -> CmdResult { CmdResult::Cancel }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration {
    names: &["DIMCENTER", "DCE", "CENTERMARK"]
}); // DimCenterCommand
