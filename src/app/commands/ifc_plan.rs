use super::*;
use crate::ifc_plan;
use crate::ui::window::ifc_plan::{IfcPlanDialog, IfcPlanMsg, IfcXrefTarget};
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
        use super::dgn_import::pick_from;
        use crate::app::ModalKind::IfcPlan;

        let ticket = self.start_window_read(IfcPlan);
        let picked = move |path| message(IfcPlanMsg::Picked(ticket, path));
        let dialog = crate::sys::file_dialog()
            .set_title(crate::tr!("ifc2plan", "title"))
            .add_filter("IFC (*.ifc)", &["ifc"]);

        #[cfg(target_os = "windows")]
        if let Some(owner) = self
            .tool_windows
            .iter()
            .find(|tool| tool.kind == IfcPlan)
            .and_then(|tool| tool.window)
        {
            return iced::window::run(owner, move |window| dialog.set_parent(window))
                .then(move |dialog| Task::perform(pick_from(dialog), picked));
        }
        Task::perform(pick_from(dialog), picked)
    }

    pub(super) fn read_ifc_plan(&mut self, path: &str) -> Task<Message> {
        self.read_ifc_plan_with_target(path, None)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn read_ifc_slice(&mut self, path: &std::path::Path) -> Task<Message> {
        let tab = &self.tabs[self.active_tab];
        self.read_ifc_slice_into(path, tab.id, tab.document_generation)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn read_ifc_slice_into(
        &mut self,
        path: &std::path::Path,
        tab_id: u64,
        document_generation: u64,
    ) -> Task<Message> {
        let Some(i) = self.import_target(tab_id, document_generation) else {
            return Task::none();
        };
        if self.tabs[i].ifccad_session.is_some() {
            self.command_line
                .push_error("Attach an IFC slice to a DWG or DXF drawing.");
            return Task::none();
        }
        let base = self.tabs[i]
            .current_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .unwrap_or(std::path::Path::new("."));
        let resolved = crate::io::xref::resolve_path(&path.to_string_lossy(), base)
            .unwrap_or_else(|| path.to_path_buf());
        let target = IfcXrefTarget {
            path: resolved.clone(),
            tab_id,
            document_generation,
        };
        self.read_ifc_plan_with_target(&resolved.to_string_lossy(), Some(target))
    }

    fn read_ifc_plan_with_target(
        &mut self,
        path: &str,
        xref: Option<IfcXrefTarget>,
    ) -> Task<Message> {
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or(path);
        self.close_active_modal();
        let mut state = IfcPlanDialog::new(filename.to_owned());
        state.xref = xref;
        self.import_dialogs.ifc_plan = Some(state);
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
                #[cfg(not(target_arch = "wasm32"))]
                if let Some((tab_id, generation)) = self
                    .import_dialogs
                    .ifc_plan
                    .as_ref()
                    .and_then(|state| state.xref.as_ref())
                    .map(|target| (target.tab_id, target.document_generation))
                {
                    return self.read_ifc_slice_into(
                        std::path::Path::new(&path),
                        tab_id,
                        generation,
                    );
                }
                return self.read_ifc_plan(&path);
            }
            IfcPlanMsg::Picked(_, _) => return Task::none(),
            IfcPlanMsg::Loaded(ticket, _) | IfcPlanMsg::Generated(ticket, _)
                if !self.window_read_current(ticket, IfcPlan) =>
            {
                return Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            IfcPlanMsg::XrefGenerated(ticket, _) if !self.window_read_current(ticket, IfcPlan) => {
                return Task::none();
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
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(path) = state.xref.as_ref().map(|target| target.path.clone()) {
                    use crate::io::xref::ifc_slice;
                    let (Some(choice), Some(cut)) = (state.selected.as_ref(), state.cut_offset())
                    else {
                        return Task::none();
                    };
                    let view = match ifc_slice::View::new(&model, choice.index, cut, depth) {
                        Ok(view) => view,
                        Err(error) => {
                            state.error = Some(error);
                            return Task::none();
                        }
                    };
                    state.pending = true;
                    state.error = None;
                    let ticket = self.start_window_read(IfcPlan);
                    return crate::app::update::background_task(
                        "IFC XREF",
                        move || {
                            let read = ifc_slice::from_model(&model, &path, &view)?;
                            Ok(std::sync::Arc::new(ifc_slice::Ready { path, view, read }))
                        },
                        move |result| message(IfcPlanMsg::XrefGenerated(ticket, result)),
                    );
                }
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
            #[cfg(not(target_arch = "wasm32"))]
            IfcPlanMsg::XrefGenerated(_, result) => {
                state.pending = false;
                match result {
                    Ok(ready) => {
                        let Some((tab_id, generation)) = state
                            .xref
                            .as_ref()
                            .map(|target| (target.tab_id, target.document_generation))
                        else {
                            return Task::none();
                        };
                        let Some(i) = self.import_target(tab_id, generation) else {
                            self.close_active_modal();
                            return Task::none();
                        };
                        if i != self.active_tab {
                            self.close_active_modal();
                            self.command_line.push_error(
                                "Attach cancelled: a different drawing became current while the reference was read.",
                            );
                            return Task::none();
                        }
                        let ready = std::sync::Arc::unwrap_or_clone(ready);
                        self.close_active_modal();
                        self.place_ifc_slice(i, &ready.path, &ready.view, ready.read);
                    }
                    Err(error) => state.error = Some(error),
                }
            }
            IfcPlanMsg::Picked(_, _) => unreachable!(),
        }
        Task::none()
    }
}
