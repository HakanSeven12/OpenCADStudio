use super::*;
use crate::ifc_plan;
use crate::ui::window::ifc_plan::{IfcPlanDialog, IfcPlanMsg};
use crate::ui::window::import_dialogs::ImportDialogMsg;

fn message(event: IfcPlanMsg) -> Message {
    Message::ImportDialog(ImportDialogMsg::IfcPlan(event))
}

impl OpenCADStudio {
    pub(super) fn begin_ifc_plan(&mut self) -> Task<Message> {
        use crate::app::ModalKind::IfcPlan;
        if self.focus_dialog(IfcPlan) {
            return Task::none();
        }
        if let Some(previous) = &self.import_dialogs.previous_ifc_plan {
            self.import_dialogs.ifc_plan = Some(IfcPlanDialog::from_previous(previous));
            self.active_modal = Some(IfcPlan);
            return Task::none();
        }
        self.pick_ifc_plan()
    }

    fn pick_ifc_plan(&mut self) -> Task<Message> {
        let ticket = self.start_window_read(crate::app::ModalKind::IfcPlan);
        let filters = vec![("IFC (*.ifc)".into(), vec!["ifc".into()])];
        Task::perform(
            super::dgn_import::pick(crate::tr!("ifc2plan", "title"), filters),
            move |path| message(IfcPlanMsg::Picked(ticket, path)),
        )
    }

    pub(super) fn read_ifc_plan(&mut self, path: &str) -> Task<Message> {
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or(path);
        self.close_active_modal();
        self.import_dialogs.ifc_plan = Some(IfcPlanDialog::new(filename.to_owned()));
        self.active_modal = Some(crate::app::ModalKind::IfcPlan);
        let ticket = self.start_window_read(crate::app::ModalKind::IfcPlan);
        let path = path.to_owned();
        crate::app::update::background_task(
            "IFC2PLAN",
            move || {
                let bytes = crate::scene::model::pdf_raster::source_bytes(&path)
                    .ok_or_else(|| format!("Could not read IFC file: {path}"))?;
                ifc_plan::load_model(&bytes).map(std::sync::Arc::new)
            },
            move |result| {
                Message::ToolWindowTask(
                    crate::app::ModalKind::IfcPlan,
                    Box::new(message(IfcPlanMsg::Loaded(ticket, result))),
                )
            },
        )
    }

    pub(super) fn update_ifc_plan(&mut self, event: IfcPlanMsg) -> Task<Message> {
        use crate::app::ModalKind::IfcPlan;
        match event {
            IfcPlanMsg::Picked(ticket, Some(path)) if self.window_read_current(ticket, IfcPlan) => {
                return self.read_ifc_plan(&path);
            }
            IfcPlanMsg::Picked(_, _) => return Task::none(),
            IfcPlanMsg::Loaded(ticket, _) | IfcPlanMsg::Generated(ticket, _)
                if !self.window_read_current(ticket, IfcPlan) =>
            {
                return Task::none()
            }
            _ => {}
        }
        let Some(state) = self
            .import_dialogs
            .ifc_plan
            .as_mut()
            .filter(|_| self.active_modal == Some(IfcPlan))
        else {
            return Task::none();
        };
        match event {
            IfcPlanMsg::Browse if !state.pending => return self.pick_ifc_plan(),
            IfcPlanMsg::Browse => {}
            IfcPlanMsg::Loaded(_, result) => {
                state.pending = false;
                match result {
                    Ok(model) => state.set_model(model),
                    Err(error) => state.error = Some(error),
                }
            }
            IfcPlanMsg::Storey(choice) if !state.pending => state.selected = Some(choice),
            IfcPlanMsg::Height(text) if !state.pending => state.height = text,
            IfcPlanMsg::Depth(text) if !state.pending => state.depth = text,
            IfcPlanMsg::Storey(_) | IfcPlanMsg::Height(_) | IfcPlanMsg::Depth(_) => {}
            IfcPlanMsg::Import => {
                if state.pending {
                    return Task::none();
                }
                let (Some((elevation, depth)), Some(model)) = (state.elevations(), &state.model)
                else {
                    return Task::none();
                };
                let model = model.clone();
                let filename = state.filename.clone();
                state.pending = true;
                state.error = None;
                let ticket = self.start_window_read(IfcPlan);
                return crate::app::update::background_task(
                    "IFC2PLAN",
                    move || {
                        ifc_plan::entities(&model, &filename, elevation, depth)
                            .map(std::sync::Arc::new)
                    },
                    move |result| message(IfcPlanMsg::Generated(ticket, result)),
                );
            }
            IfcPlanMsg::Generated(_, result) => {
                state.pending = false;
                match result {
                    Ok(entities) => {
                        self.import_dialogs.previous_ifc_plan = state.previous();
                        self.close_active_modal();
                        let task = self.update(Message::TabNew);
                        let i = self.active_tab;
                        self.push_undo_snapshot(i, "IFC2PLAN");
                        let scene = &mut self.tabs[i].scene;
                        scene.document.header.insertion_units = 4; // millimetres
                        let mut app_id =
                            document::tables::AppId::new(ifc_plan::source::APPLICATION);
                        app_id.handle = scene.document.allocate_handle();
                        let _ = scene.document.app_ids.add(app_id);
                        for (name, weight) in ifc_plan::LAYERS {
                            let mut layer = document::tables::Layer::new(name);
                            layer.handle = scene.document.allocate_handle();
                            layer.line_weight = document::types::LineWeight::from_value(weight);
                            let _ = scene.document.layers.add(layer);
                        }
                        let count = entities.len();
                        scene.add_entities(std::sync::Arc::unwrap_or_clone(entities));
                        scene.fit_all();
                        self.tabs[i].dirty = true;
                        self.refresh_layer_panel();
                        self.refresh_properties();
                        self.command_line
                            .push_output(&format!("IFC2PLAN: {count} lines."));
                        return task;
                    }
                    Err(error) => state.error = Some(error),
                }
            }
            IfcPlanMsg::Picked(_, _) => unreachable!(),
        }
        Task::none()
    }
}
