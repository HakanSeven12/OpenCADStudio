//! Associative centre-line construction from lines and linear polyline segments.

use codec::entities::{CenterLineAssociation, CenterLineSource};
use codec::types::Vector3;
use codec::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::centerline::{construct_line, picked_source, CenterLineSettings};

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "CENTERLINE",
        label: "Centerline",
        icon: IconKind::Svg(include_bytes!("../../../../assets/icons/line.svg")),
        event: ModuleEvent::Command("CENTERLINE".to_owned()),
    }
}

/// Whether `text` is a typed point: at a pick prompt it selects the object
/// there rather than naming something.
pub(crate) fn is_typed_point(text: &str) -> bool {
    let text = text.trim().trim_start_matches(['@', '*', '#']);
    let parts: Vec<&str> = text.split(',').collect();
    (2..=3).contains(&parts.len())
        && parts.iter().all(|part| part.trim().parse::<f64>().is_ok())
}

/// The Layer option of CENTERMARK and CENTERLINE: the layer new centre
/// objects go on (CENTERLAYER), asked for until Enter or eXit. A name, `.`
/// (the current layer) or a picked object's layer is taken.
pub(crate) struct CenterLayerOption {
    command: &'static str,
    pub(crate) active: bool,
    value: String,
    changed: bool,
    names: Vec<String>,
    pick: Option<EntityType>,
}

impl CenterLayerOption {
    pub(crate) fn new(command: &'static str, value: String, names: Vec<String>) -> Self {
        Self {
            command,
            active: false,
            value,
            changed: false,
            names,
            pick: None,
        }
    }

    pub(crate) fn prompt(&self) -> String {
        let current = if self.value.trim().is_empty() || self.value.trim() == "." {
            crate::tr!("center-cmd", "use-current")
        } else {
            self.value.clone()
        };
        crate::tr!("center-cmd", "layer", command = self.command, current = current)
    }

    pub(crate) fn options(&self) -> Vec<CmdOption> {
        vec![CmdOption::new("?", "?"), CmdOption::new("eXit", "X")]
    }

    /// A typed answer; `None` for a typed point, which picks an object.
    pub(crate) fn on_text(&mut self, text: &str) -> Option<CmdResult> {
        if is_typed_point(text) {
            return None;
        }
        let value = text.trim().trim_matches('"');
        Some(match value.to_ascii_uppercase().trim_start_matches('_') {
            "" | "X" | "EXIT" => {
                self.active = false;
                CmdResult::NeedPoint
            }
            "?" => CmdResult::ReportMeasurement(self.names.join("\n")),
            _ => {
                self.value = value.to_string();
                self.changed = true;
                CmdResult::NeedPoint
            }
        })
    }

    pub(crate) fn inject(&mut self, entity: EntityType) {
        self.pick = Some(entity);
    }

    /// An object picked at the Layer prompt gives its layer.
    pub(crate) fn on_pick(&mut self, handle: Handle) -> CmdResult {
        if let Some(entity) = self.pick.take().filter(|_| !handle.is_null()) {
            self.value = entity.common().layer.clone();
            self.changed = true;
        }
        CmdResult::NeedPoint
    }

    pub(crate) fn update(&self) -> Option<(&'static str, String)> {
        self.changed
            .then(|| ("CENTERLAYER", self.value.clone()))
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
    layer: CenterLayerOption,
}

impl CenterLineCommand {
    pub(crate) fn new(settings: CenterLineSettings, layer_names: Vec<String>) -> Self {
        let layer = CenterLayerOption::new("CENTERLINE", settings.layer.clone(), layer_names);
        Self {
            first: None,
            picked: None,
            plane: WorkingPlane::default(),
            settings,
            layer,
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
        if self.layer.active {
            self.layer.prompt()
        } else if self.first.is_none() {
            crate::tr!("center-cmd", "line-first")
        } else {
            crate::tr!("center-cmd", "line-second")
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.layer.active {
            self.layer.options()
        } else if self.first.is_none() {
            vec![CmdOption::new("Layer", "L")]
        } else {
            Vec::new()
        }
    }

    fn needs_entity_pick(&self) -> bool {
        true
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn typed_point_picks_entity(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        if self.layer.active {
            self.layer.inject(entity);
        } else {
            self.picked = Some(entity);
        }
    }

    fn drawing_variable_update(&self) -> Option<(&'static str, String)> {
        self.layer.update()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.layer.active {
            return self.layer.on_text(text);
        }
        if self.first.is_none()
            && matches!(
                text.trim().trim_start_matches('_').to_ascii_uppercase().as_str(),
                "L" | "LAYER"
            )
        {
            self.layer.active = true;
            return Some(CmdResult::NeedPoint);
        }
        None
    }

    fn on_entity_pick(&mut self, handle: Handle, point: DVec3) -> CmdResult {
        if self.layer.active {
            return self.layer.on_pick(handle);
        }
        let entity = self.picked.take();
        let picked = entity
            .filter(|_| !handle.is_null())
            .and_then(|entity| picked_source(&entity, handle, point));
        let Some((source, start, end)) = picked else {
            return CmdResult::ReportError(crate::tr!("center-cmd", "line-invalid"));
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
            return CmdResult::ReportError(crate::tr!("center-cmd", "line-same"));
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
            return CmdResult::ReportError(crate::tr!("center-cmd", "line-same"));
        };
        association.write(&mut line.common.extended_data);
        CmdResult::CommitAndExit(EntityType::Line(line))
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.layer.active {
            self.layer.active = false;
            return CmdResult::NeedPoint;
        }
        CmdResult::Done
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
