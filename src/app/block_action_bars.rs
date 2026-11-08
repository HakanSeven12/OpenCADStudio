use super::{Message, OpenCADStudio};
use crate::scene::block_action_bars::{self as bars, EntryKind, Group};
use document::Handle;
use iced::{Point, Rectangle, Size, Task, Vector};
use std::collections::{HashMap, HashSet};

/// The height a bar is assumed to occupy when it is laid out, used both to keep
/// the card inside the viewport and to keep two default cards from covering
/// each other. The rendered card can be shorter; the reserved strip is the
/// space a card with a selected entry and its edit row needs.
pub(super) const BAR_HEIGHT: f32 = 90.0;

/// The clear space kept between two stacked cards.
const BAR_GAP: f32 = 8.0;

#[derive(Default)]
pub(super) struct ViewState {
    pub hidden: HashSet<Handle>,
    pub offsets: HashMap<Handle, Vector>,
}

fn overlaps(a: Rectangle, b: Rectangle) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

/// Place a default card in the in-viewport slot nearest its natural position
/// that no card already placed covers. When the viewport has no free slot the
/// natural position is kept, so a short tile degrades to the old placement
/// rather than pushing a card outside the view.
fn stack_clear(natural: Point, width: f32, bounds: Rectangle, placed: &[Rectangle]) -> Point {
    let lowest = (bounds.y + bounds.height - BAR_HEIGHT).max(bounds.y);
    let mut slots = vec![natural.y.clamp(bounds.y, lowest)];
    for rect in placed {
        slots.push(rect.y + rect.height + BAR_GAP);
        slots.push(rect.y - BAR_HEIGHT - BAR_GAP);
    }
    slots.push(bounds.y);
    slots.push(lowest);
    slots.retain(|y| (bounds.y..=lowest).contains(y));
    slots.sort_by(|a, b| (a - natural.y).abs().total_cmp(&(b - natural.y).abs()));
    slots.dedup();
    for y in slots {
        let rect = Rectangle::new(Point::new(natural.x, y), Size::new(width, BAR_HEIGHT));
        if !placed.iter().any(|other| overlaps(*other, rect)) {
            return Point::new(natural.x, y);
        }
    }
    Point::new(natural.x, natural.y.clamp(bounds.y, lowest))
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
        let mut placed: Vec<Rectangle> = vec![];
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
                let natural = Point::new(
                    (bounds.x + anchor.x + 16.0 + offset.x)
                        .clamp(bounds.x, (bounds.x + bounds.width - width).max(bounds.x)),
                    (bounds.y + anchor.y + 20.0 + offset.y).clamp(
                        bounds.y,
                        (bounds.y + bounds.height - BAR_HEIGHT).max(bounds.y),
                    ),
                );
                let point = if tab.action_bars.offsets.contains_key(&group.parameter) {
                    natural
                } else {
                    stack_clear(natural, width, bounds, &placed)
                };
                placed.push(Rectangle::new(point, Size::new(width, BAR_HEIGHT)));
                let effective = Vector::new(point.x - natural.x, point.y - natural.y) + offset;
                Some((group, point, effective, width))
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
