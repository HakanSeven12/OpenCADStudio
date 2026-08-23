//! Associative centre-line construction from lines and linear polyline segments.

use acadrust::entities::{CenterLineAssociation, CenterLineSource};
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::centerline::{construct_line, picked_source, CenterLineSettings};
use crate::t;

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "CENTERLINE",
        label: "Center Line",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("CENTERLINE".to_owned()),
    }
}

#[derive(Clone)]
struct PickedSource {
    source: CenterLineSource,
    start: DVec3,
    end: DVec3,
}

fn vector(point: DVec3) -> Vector3 {
    Vector3::new(point.x, point.y, point.z)
}

pub struct CenterLineCommand {
    first: Option<PickedSource>,
    picked: Option<EntityType>,
    plane: WorkingPlane,
    settings: CenterLineSettings,
}

impl CenterLineCommand {
    pub(crate) fn new(settings: CenterLineSettings) -> Self {
        Self {
            first: None,
            picked: None,
            plane: WorkingPlane::default(),
            settings,
        }
    }
}

impl CadCommand for CenterLineCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "CENTERLINE"
    }

    fn prompt(&self) -> String {
        if self.first.is_none() {
            t!("CENTERLINE  Select first line:").into_owned()
        } else {
            t!("CENTERLINE  Select second line:").into_owned()
        }
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

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if handle.is_null() {
            return CmdResult::NeedPoint;
        }
        let Some(entity) = self.picked.take() else {
            return CmdResult::NeedPoint;
        };
        let Some((source, start, end)) = picked_source(&entity, handle, point) else {
            return CmdResult::NeedPoint;
        };
        let picked = PickedSource { source, start, end };
        let Some(first) = self.first.take() else {
            self.first = Some(picked);
            return CmdResult::NeedPoint;
        };
        if first.source.handle == picked.source.handle
            && first.source.segment_index == picked.source.segment_index
        {
            self.first = Some(first);
            return CmdResult::NeedPoint;
        }

        let association = CenterLineAssociation {
            first: first.source.clone(),
            second: picked.source.clone(),
            plane_origin: vector(self.plane.origin),
            plane_x: vector(self.plane.x),
            plane_y: vector(self.plane.y),
            start_extension: self.settings.extension,
            end_extension: self.settings.extension,
            start_length_adjustment: 0.0,
            end_length_adjustment: 0.0,
            associated: true,
        };
        let Some(mut line) = construct_line(
            (first.start, first.end),
            (picked.start, picked.end),
            &association,
        ) else {
            self.first = Some(first);
            return CmdResult::NeedPoint;
        };
        association.write(&mut line.common.extended_data);
        CmdResult::CommitAndExit(EntityType::Line(line))
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

inventory::submit!(crate::command::CommandRegistration {
    names: &[
        "CENTERLINE",
        "CENTERRESET",
        "CENTERREASSOCIATE",
        "CENTERDISASSOCIATE",
    ]
});
