use super::{Message, OpenCADStudio};
use crate::scene::block_action_bars::{self as bars, EntryKind, Group};
use document::Handle;
use iced::{Point, Rectangle, Task, Vector};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(super) struct ViewState {
    pub hidden: HashSet<Handle>,
    pub offsets: HashMap<Handle, Vector>,
}

impl OpenCADStudio {
    pub(in crate::app) fn action_bar_error(&self) -> Option<&'static str> {
        let tab = &self.tabs[self.active_tab];
        if tab.active_block_edit_session().is_none() {
            return Some("BACTIONBAR requires an active Block Editor session.");
        }
        if !self.block_editor_preferences.action_bars {
            return Some("Set BACTIONBARMODE to 1 to display action bars.");
        }
        None
    }

    pub(super) fn action_bar_layout(&self, bounds: Rectangle) -> Vec<(Group, Point, Vector, f32)> {
        let tab = &self.tabs[self.active_tab];
        if !self.block_editor_preferences.action_bars {
            return vec![];
        }
        let Some(session) = tab.active_block_edit_session() else {
            return vec![];
        };
        let camera = tab.scene.camera.borrow();
        let width = 360.0_f32.min(bounds.width.max(1.0));
        bars::groups(&tab.scene.document, session.br_handle)
            .into_iter()
            .filter_map(|group| {
                if tab.action_bars.hidden.contains(&group.parameter) {
                    return None;
                }
                let anchor = camera.project(group.anchor, bounds)?;
                if !anchor.x.is_finite()
                    || !anchor.y.is_finite()
                    || anchor.x < 0.0
                    || anchor.y < 0.0
                    || anchor.x > bounds.width
                    || anchor.y > bounds.height
                {
                    return None;
                }
                let offset = tab
                    .action_bars
                    .offsets
                    .get(&group.parameter)
                    .copied()
                    .unwrap_or(Vector::ZERO);
                let point = Point::new(
                    (bounds.x + anchor.x + 16.0 + offset.x)
                        .clamp(bounds.x, (bounds.x + bounds.width - width).max(bounds.x)),
                    (bounds.y + anchor.y + 20.0 + offset.y)
                        .clamp(bounds.y, (bounds.y + bounds.height - 90.0).max(bounds.y)),
                );
                Some((group, point, offset, width))
            })
            .collect()
    }

    pub(in crate::app) fn handle_action_bar(
        &mut self,
        handle: Handle,
        edit: bool,
    ) -> Task<Message> {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if !self.block_editor_preferences.action_bars || tab.active_cmd.is_some() {
            return Task::none();
        }
        let Some(session) = tab.active_block_edit_session() else {
            return Task::none();
        };
        let entry = bars::groups(&tab.scene.document, session.br_handle)
            .into_iter()
            .filter(|group| !tab.action_bars.hidden.contains(&group.parameter))
            .flat_map(|group| group.entries)
            .find(|entry| entry.handle == handle);
        let Some(entry) = entry else {
            return Task::none();
        };
        self.tabs[i].selected_authoring = Some(handle);
        self.tabs[i].scene.deselect_all();
        self.refresh_properties();
        if !edit {
            return Task::none();
        }
        match entry.kind {
            EntryKind::Action(_) => self.start_block_action_set(),
            EntryKind::Orphan(_) => self.start_block_action_association(),
            EntryKind::Lookup => {
                let task = self.open_lookup_table("");
                if let Some(state) = self.block_lookup.as_mut() {
                    if let Some(index) = state
                        .drafts
                        .iter()
                        .position(|draft| draft.parameter == handle)
                    {
                        state.selected = index;
                    }
                }
                task
            }
            EntryKind::PropertiesTable => self.open_block_properties_table(),
        }
    }

    pub(in crate::app) fn move_action_bar(&mut self, handle: Handle, offset: Vector) {
        let tab = &mut self.tabs[self.active_tab];
        if !self.block_editor_preferences.action_bars
            || tab.active_cmd.is_some()
            || !offset.x.is_finite()
            || !offset.y.is_finite()
        {
            return;
        }
        if tab.active_block_edit_session().is_some_and(|session| {
            bars::groups(&tab.scene.document, session.br_handle)
                .iter()
                .any(|group| group.parameter == handle)
        }) {
            tab.action_bars.offsets.insert(handle, offset);
        }
    }

    pub(in crate::app) fn set_action_bars(
        &mut self,
        document: u64,
        definition: Handle,
        mode: crate::modules::block_editor::action_bar::Mode,
        parameters: Vec<Handle>,
    ) -> Task<Message> {
        let tab = &mut self.tabs[self.active_tab];
        if tab.id != document
            || tab
                .active_block_edit_session()
                .map(|session| session.br_handle)
                != Some(definition)
            || !self.block_editor_preferences.action_bars
        {
            return Task::none();
        }
        let groups = bars::groups(&tab.scene.document, definition);
        if parameters
            .iter()
            .any(|handle| !groups.iter().any(|group| group.parameter == *handle))
        {
            self.command_line
                .push_error(crate::t!("The parameter list changed; restart BACTIONBAR.").as_ref());
            return Task::none();
        }
        use crate::modules::block_editor::action_bar::Mode;
        match mode {
            Mode::Show => {
                for handle in parameters {
                    tab.action_bars.hidden.remove(&handle);
                }
            }
            Mode::Hide => tab.action_bars.hidden.extend(parameters),
            Mode::Reset => tab.action_bars = ViewState::default(),
        }
        tab.active_cmd = None;
        self.command_line
            .push_output(crate::t!("Action bar display updated.").as_ref());
        Task::none()
    }

    pub(in crate::app) fn start_action_bar_command(&mut self, args: &str) -> Task<Message> {
        if let Some(error) = self.action_bar_error() {
            self.command_line.push_error(crate::t!(error).as_ref());
            return Task::none();
        }
        let i = self.active_tab;
        let tab = &self.tabs[i];
        let definition = tab.active_block_edit_session().unwrap().br_handle;
        let groups = bars::groups(&tab.scene.document, definition);
        let selected = tab.selected_authoring.and_then(|selected| {
            groups
                .iter()
                .find(|group| {
                    group.parameter == selected
                        || group.entries.iter().any(|entry| entry.handle == selected)
                })
                .map(|group| group.parameter)
        });
        let command = crate::modules::block_editor::action_bar::ActionBarCommand::new(
            tab.id, definition, groups, selected,
        );
        use crate::command::CadCommand;
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[i].active_cmd = Some(Box::new(command));
        let mut tasks = vec![];
        for part in args
            .trim()
            .splitn(2, char::is_whitespace)
            .filter(|part| !part.is_empty())
        {
            if self.tabs[i].active_cmd.is_some() {
                tasks.push(self.feed_command(crate::command::StepInput::Text(part.trim().into())));
            }
        }
        Task::batch(tasks)
    }
}
