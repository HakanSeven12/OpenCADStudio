use super::{Message, ModalKind, OpenCADStudio};
use crate::scene::{block_basepoint, block_lookup as lookup};
use crate::ui::window::block_lookup::{Action, Draft, State};
use iced::Task;

impl OpenCADStudio {
    pub(in crate::app) fn handle_place_block_single_parameter(
        &mut self,
        result: crate::command::CmdResult,
    ) -> Task<Message> {
        use crate::modules::block_editor::single_parameter::Kind;
        let crate::command::CmdResult::PlaceBlockSingleParameter {
            document_id,
            definition,
            kind,
            location,
            grips,
        } = result
        else {
            unreachable!()
        };
        let i = self.active_tab;
        if self.tabs[i].id != document_id
            || self.tabs[i]
                .active_block_edit_session()
                .map(|s| s.br_handle)
                != Some(definition)
        {
            self.command_line.push_error(
                crate::t!("The parameter's Block Editor session is no longer active.").as_ref(),
            );
            return Task::none();
        }
        if let Some(error) = self.parameter_placement_error(kind.name()) {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        if !location.is_finite() {
            self.command_line
                .push_error(crate::t!("Specify a finite parameter location.").as_ref());
            return Task::none();
        }
        let location = codec::types::Vector3::new(location.x, location.y, location.z);
        if kind == Kind::Basepoint
            && block_basepoint::parameter(&self.tabs[i].scene.document, definition).is_some_and(
                |(_, p)| {
                    p.point == location
                        && p.base_point == location
                        && p.parameter.definition_point == location
                },
            )
        {
            self.tabs[i].active_cmd = None;
            self.tabs[i].scene.clear_preview_wire();
            return Task::none();
        }
        self.push_undo_snapshot(i, &format!("{} parameter", kind.name()));
        let parameter = match kind {
            Kind::Basepoint => {
                block_basepoint::create(&mut self.tabs[i].scene.document, definition, location)
            }
            Kind::Lookup => lookup::create(&mut self.tabs[i].scene.document, definition, location),
        }
        .expect("validated parameter placement");
        if let Some(count) = grips
            .filter(|_| kind == Kind::Lookup)
            .filter(|count| *count != 1)
        {
            crate::scene::block_grips::config::set(
                &mut self.tabs[i].scene.document,
                definition,
                parameter,
                Some(count),
            )
            .expect("validated Lookup grip count");
        }
        self.tabs[i].scene.bump_geometry();
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].scene.clear_preview_wire();
        self.finish_pending_history(i);
        self.refresh_properties();
        self.command_line.push_output(
            crate::t!(if kind == Kind::Lookup {
                "Lookup parameter placed."
            } else {
                "Basepoint parameter placed."
            })
            .as_ref(),
        );
        self.finish_parameter_set_placement(i, parameter)
    }
    pub(in crate::app) fn lookup_table_error(&self) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        let Some(session) = tab.active_block_edit_session() else {
            return Some("Open a block in the Block Editor before editing a Lookup table.");
        };
        if lookup::parameters(&tab.scene.document, session.br_handle).is_empty() {
            Some("Place a Lookup parameter before editing its property table.")
        } else {
            lookup::placement_error(&tab.scene.document, session.br_handle)
        }
    }
    /// Open the property table the chosen Lookup parameter owns; the action
    /// command that picked the parameter stays active, as the reference does.
    pub(in crate::app) fn open_lookup_table_from_action(
        &mut self,
        document: u64,
        definition: codec::Handle,
        name: String,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].id != document
            || self.tabs[i].active_block_edit_session().map(|s| s.br_handle) != Some(definition)
        {
            self.command_line.push_error(
                crate::t!("The action's Block Editor session is no longer active.").as_ref(),
            );
            return Task::none();
        }
        self.open_lookup_table(&name)
    }
    pub(in crate::app) fn open_lookup_table(&mut self, name: &str) -> Task<Message> {
        if let Some(error) = self.lookup_table_error() {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        let tab = &self.tabs[self.active_tab];
        let definition = tab.active_block_edit_session().unwrap().br_handle;
        let mut drafts = vec![];
        for (handle, parameter) in lookup::parameters(&tab.scene.document, definition) {
            let original = match lookup::table(&tab.scene.document, definition, handle) {
                Ok(value) => value,
                Err(error) => {
                    self.command_line.push_error(crate::t!(error).as_ref());
                    return Task::none();
                }
            };
            let edited = original
                .as_ref()
                .map(|(_, t)| t.clone())
                .unwrap_or_else(|| lookup::empty_table(parameter));
            if original.as_ref().is_some_and(|(handle, _)| {
                drafts.iter().any(|draft: &Draft| {
                    draft
                        .original
                        .as_ref()
                        .is_some_and(|(existing, _)| existing == handle)
                })
            }) {
                continue;
            }
            if edited.row_count < 0
                || edited.row_count > 1000
                || edited.columns.is_empty()
                || edited.columns.len() > 64
                || edited.column_count != edited.columns.len() as i32
                || edited.expressions.len() != edited.columns.len() * edited.row_count as usize
            {
                self.command_line.push_error(
                    crate::t!("This Lookup table has inconsistent or unsupported dimensions.")
                        .as_ref(),
                );
                return Task::none();
            }
            drafts.push(Draft {
                parameter: handle,
                original_parameter: parameter.clone(),
                original,
                edited,
            });
        }
        let selected = if name.is_empty() {
            0
        } else {
            let Some(index) = drafts.iter().position(|d| {
                d.original_parameter.lookup_name.eq_ignore_ascii_case(name)
                    || lookup::parameters(&tab.scene.document, definition)
                        .into_iter()
                        .any(|(_, p)| {
                            p.lookup_name.eq_ignore_ascii_case(name)
                                && d.edited.columns.iter().any(|column| {
                                    column.lookup_property
                                        && column.node_id
                                            == p.parameter.parameter.element.eval.node_id
                                })
                        })
            }) else {
                self.command_line
                    .push_error(crate::t!("No Lookup parameter has that name.").as_ref());
                return Task::none();
            };
            index
        };
        self.block_lookup = Some(State {
            document: tab.id,
            definition,
            epoch: tab.scene.geometry_epoch,
            drafts,
            selected,
            properties: lookup::authoring_properties(&tab.scene.document, definition),
            selected_property: None,
            error: None,
            notice: None,
            confirm_close: false,
        });
        self.active_modal = Some(ModalKind::BlockLookup);
        Task::none()
    }
    pub(super) fn handle_block_lookup(&mut self, action: Action) -> Task<Message> {
        if !matches!(action, Action::Apply) {
            if let Some(state) = self.block_lookup.as_mut() {
                match action {
                    Action::DiscardClose => {
                        // Leaving this way is the same as pressing `Cancel`.
                        self.close_active_modal();
                        return Task::none();
                    }
                    Action::KeepEditing => {
                        // The diagnosis stays on screen; only the offer goes.
                        state.confirm_close = false;
                        return Task::none();
                    }
                    _ => state.edit(action),
                }
            }
            return Task::none();
        }
        let Some(state) = self.block_lookup.as_ref() else {
            return Task::none();
        };
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let stale = tab.id != state.document
            || tab.active_block_edit_session().map(|s| s.br_handle) != Some(state.definition)
            || tab.scene.geometry_epoch != state.epoch
            || state.drafts.iter().any(|d| {
                lookup::parameters(&tab.scene.document, state.definition)
                    .into_iter()
                    .find(|(h, _)| *h == d.parameter)
                    .map(|(_, p)| p)
                    != Some(&d.original_parameter)
                    || lookup::table(&tab.scene.document, state.definition, d.parameter)
                        .ok()
                        .as_ref()
                        != Some(&d.original)
            });
        if stale {
            self.block_lookup.as_mut().unwrap().error = Some(
                crate::t!("The block changed. Cancel and reopen the Lookup table.").into_owned(),
            );
            return Task::none();
        }
        let mut claimed = std::collections::BTreeSet::new();
        if state
            .drafts
            .iter()
            .filter(|draft| draft.changed())
            .any(|draft| {
                draft
                    .edited
                    .columns
                    .iter()
                    .filter(|column| column.lookup_property)
                    .any(|column| !claimed.insert(column.node_id))
            })
        {
            self.block_lookup.as_mut().unwrap().error = Some(
                crate::t!("A Lookup output cannot belong to multiple edited tables.").into_owned(),
            );
            return Task::none();
        }
        let mut working = tab.scene.document.clone();
        let mut changed = false;
        let mut refusal = None;
        for draft in &state.drafts {
            if draft.changed() {
                if let Err(error) = lookup::apply_table(
                    &mut working,
                    state.definition,
                    draft.parameter,
                    draft.edited.clone(),
                ) {
                    refusal = Some(crate::t!(error.as_str()).into_owned());
                    break;
                }
                changed = true;
            }
        }
        if let Some(error) = refusal {
            let state = self.block_lookup.as_mut().unwrap();
            state.error = Some(error);
            // Refusing the table is not a reason to hold the operator inside
            // the dialog: offer the way out along with the diagnosis.
            state.confirm_close = true;
            return Task::none();
        }
        if changed {
            self.push_undo_snapshot(i, "Lookup tables");
            self.tabs[i].scene.document = working;
            self.tabs[i].scene.bump_geometry();
            self.tabs[i].dirty = true;
            self.finish_pending_history(i);
            self.refresh_properties();
        }
        self.block_lookup = None;
        self.active_modal = None;
        Task::none()
    }
}
