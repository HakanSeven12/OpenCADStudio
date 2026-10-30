//! DIMINSPECT (Inspection Dimension window) and -DIMINSPECT.

use crate::app::{Message, OpenCADStudio};
use crate::entities::dim_inspect::{self as inspect, Inspection, Shape};
use crate::modules::annotate::dim_inspect::{DimInspectCommand, SEP};
use crate::ui::window::dim_inspect_dialog::{DimInspectMsg, DimInspectState};
use codec::Handle;
use iced::Task;

fn is_dimension(entity: &codec::EntityType) -> bool {
    matches!(entity, codec::EntityType::Dimension(_))
}

impl OpenCADStudio {
    pub(super) fn dispatch_dim_inspect(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (name, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
        match name.trim_start_matches(['_', '.']).to_ascii_uppercase().as_str() {
            "DIMINSPECT" => {
                let handles = self.selected_dimension_handles(i);
                self.open_dim_inspect(i, handles);
            }
            // The window's Select dimensions: back with what was picked.
            "DIMINSPECT_DIALOG" => {
                let handles = self.selected_dimension_handles(i);
                self.open_dim_inspect(i, handles);
            }
            "-DIMINSPECT" => {
                let command = DimInspectCommand::new();
                self.command_line.push_info(&crate::command::CadCommand::prompt(&command));
                self.command_line
                    .set_step_options(crate::command::CadCommand::options(&command));
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "DIMINSPECT_SELECT" => {
                use crate::modules::draw::select::SelectObjectsCommand;
                self.tabs[i].scene.deselect_all();
                let picked = format!("_DIMINSPECT_PICKED {}", rest.trim());
                let select = SelectObjectsCommand::filtered(
                    "-DIMINSPECT",
                    &picked,
                    "Select dimensions:",
                    is_dimension,
                );
                self.command_line
                    .push_info(&crate::command::CadCommand::prompt(&select));
                self.tabs[i].active_cmd = Some(Box::new(select));
            }
            "DIMINSPECT_PICKED" => {
                let handles = self.selected_dimension_handles(i);
                if handles.is_empty() {
                    return Some(Task::none());
                }
                if rest.trim().eq_ignore_ascii_case("REMOVE") {
                    let count = self.remove_dim_inspection(i, &handles);
                    self.tabs[i].scene.deselect_all();
                    self.command_line.push_output(&crate::tr!(
                        "dim-inspect",
                        "removed",
                        count = count
                    ));
                } else {
                    let command = DimInspectCommand::adding(handles);
                    self.command_line
                        .push_info(&crate::command::CadCommand::prompt(&command));
                    self.command_line
                        .set_step_options(crate::command::CadCommand::options(&command));
                    self.tabs[i].active_cmd = Some(Box::new(command));
                }
            }
            "DIMINSPECT_APPLY" => {
                let fields: Vec<&str> = rest.splitn(4, SEP).collect();
                let [shape, label, rate, handles] = fields.as_slice() else {
                    return Some(Task::none());
                };
                let inspection = Inspection {
                    shape: match *shape {
                        "A" => Shape::Angular,
                        "N" => Shape::None,
                        _ => Shape::Round,
                    },
                    label: (!label.is_empty()).then(|| label.to_string()),
                    rate: Some(rate.to_string()),
                };
                let handles: Vec<Handle> = handles
                    .split(',')
                    .filter_map(|h| u64::from_str_radix(h, 16).ok())
                    .map(Handle::new)
                    .collect();
                self.apply_dim_inspection(i, &handles, &inspection);
                self.tabs[i].scene.deselect_all();
            }
            _ => return None,
        }
        Some(Task::none())
    }

    fn selected_dimension_handles(&self, i: usize) -> Vec<Handle> {
        let scene = &self.tabs[i].scene;
        scene
            .selected_entities()
            .into_iter()
            .filter(|(handle, entity)| is_dimension(entity) && !scene.is_layer_locked(*handle))
            .map(|(handle, _)| handle)
            .collect()
    }

    fn open_dim_inspect(&mut self, i: usize, handles: Vec<Handle>) {
        let doc = &self.tabs[i].scene.document;
        // The first selected dimension that has an inspection shows its
        // values; otherwise the window's last values.
        let current = handles.iter().find_map(|&h| {
            doc.get_entity(h)
                .and_then(|e| inspect::read(&e.common().extended_data))
        });
        let s = &self.dim_inspect_settings;
        let mut state = DimInspectState {
            handles,
            any_inspected: current.is_some(),
            shape: match s.shape {
                1 => Shape::Angular,
                2 => Shape::None,
                _ => Shape::Round,
            },
            label_on: s.label_on,
            label: s.label.clone(),
            rate_on: s.rate_on,
            rate: s.rate.clone(),
        };
        if let Some(current) = current {
            state.shape = current.shape;
            state.label_on = current.label.is_some();
            if let Some(label) = current.label {
                state.label = label;
            }
            state.rate_on = current.rate.is_some();
            if let Some(rate) = current.rate {
                state.rate = rate;
            }
        }
        self.dim_inspect_dialog = Some(state);
        self.active_modal = Some(crate::app::ModalKind::DimInspect);
    }

    /// Show `inspection` on the dimensions, as one undo step.
    fn apply_dim_inspection(&mut self, i: usize, handles: &[Handle], inspection: &Inspection) {
        if handles.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, "DIMINSPECT");
        self.tabs[i].scene.ensure_app_id(inspect::APP);
        let app = self.tabs[i]
            .scene
            .document
            .app_ids
            .get(inspect::APP)
            .map(|a| a.handle.value());
        for &handle in handles {
            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                let common = entity.common_mut();
                inspect::set(common, inspection);
                common
                    .extended_data
                    .raw_dwg_eed
                    .retain(|(h, _)| Some(*h) != app);
            }
        }
        self.invalidate_property_targets(i, handles);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    /// Take the inspection off the dimensions; returns how many had one.
    fn remove_dim_inspection(&mut self, i: usize, handles: &[Handle]) -> usize {
        self.push_undo_snapshot(i, "DIMINSPECT");
        let app = self.tabs[i]
            .scene
            .document
            .app_ids
            .get(inspect::APP)
            .map(|a| a.handle.value());
        let mut count = 0;
        for &handle in handles {
            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                let common = entity.common_mut();
                if inspect::remove(common) {
                    common
                        .extended_data
                        .raw_dwg_eed
                        .retain(|(h, _)| Some(*h) != app);
                    count += 1;
                }
            }
        }
        if count > 0 {
            self.invalidate_property_targets(i, handles);
            self.tabs[i].dirty = true;
            self.refresh_properties();
        }
        count
    }

    pub(in crate::app) fn update_dim_inspect(&mut self, message: DimInspectMsg) -> Task<Message> {
        let i = self.active_tab;
        let Some(state) = self.dim_inspect_dialog.as_mut() else {
            return Task::none();
        };
        match message {
            DimInspectMsg::Shape(shape) => state.shape = shape,
            DimInspectMsg::LabelOn(on) => state.label_on = on,
            DimInspectMsg::Label(value) => state.label = value,
            DimInspectMsg::RateOn(on) => state.rate_on = on,
            DimInspectMsg::Rate(value) => state.rate = value,
            DimInspectMsg::Help => {
                self.command_line
                    .push_info(&crate::tr!("dim-inspect", "help"));
            }
            DimInspectMsg::Select => {
                self.remember_dim_inspect();
                self.dim_inspect_dialog = None;
                self.active_modal = None;
                use crate::modules::draw::select::SelectObjectsCommand;
                self.tabs[i].scene.deselect_all();
                let select = SelectObjectsCommand::filtered(
                    "DIMINSPECT",
                    "_DIMINSPECT_DIALOG",
                    "Select dimensions:",
                    is_dimension,
                );
                self.command_line
                    .push_info(&crate::command::CadCommand::prompt(&select));
                self.tabs[i].active_cmd = Some(Box::new(select));
            }
            DimInspectMsg::Remove => {
                let handles = state.handles.clone();
                self.dim_inspect_dialog = None;
                self.active_modal = None;
                self.remove_dim_inspection(i, &handles);
                self.tabs[i].scene.deselect_all();
            }
            DimInspectMsg::Ok => {
                let inspection = Inspection {
                    shape: state.shape,
                    label: state.label_on.then(|| state.label.clone()),
                    rate: state.rate_on.then(|| state.rate.clone()),
                };
                let handles = state.handles.clone();
                self.remember_dim_inspect();
                self.dim_inspect_dialog = None;
                self.active_modal = None;
                self.apply_dim_inspection(i, &handles, &inspection);
                self.tabs[i].scene.deselect_all();
            }
        }
        Task::none()
    }

    /// The window's values become the next time's.
    fn remember_dim_inspect(&mut self) {
        let Some(state) = self.dim_inspect_dialog.as_ref() else {
            return;
        };
        self.dim_inspect_settings = crate::app::settings::DimInspectSettings {
            shape: match state.shape {
                Shape::Round => 0,
                Shape::Angular => 1,
                Shape::None => 2,
            },
            label_on: state.label_on,
            label: state.label.clone(),
            rate_on: state.rate_on,
            rate: state.rate.clone(),
        };
        self.persist_settings_if_changed();
    }
}
