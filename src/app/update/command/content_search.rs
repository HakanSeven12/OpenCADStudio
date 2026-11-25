//! Drawing names at the command line. While no command runs, the layers,
//! blocks, hatch patterns and named views whose names contain the typed text
//! show under the command names. A picked layer becomes current, a block is
//! inserted, HATCH starts with a pattern and a view is restored.

use crate::app::{Message, OpenCADStudio};
use crate::ui::command_line::{ranked_content, ContentKind, ContentMatch};
use iced::Task;

impl OpenCADStudio {
    /// Find the drawing names for the typed text. While a command runs,
    /// there are none.
    pub(in crate::app) fn refresh_content_suggestions(&mut self) {
        let i = self.active_tab;
        let input = self.command_line.input.clone();
        let matches = if self.tabs[i].active_cmd.is_some() {
            Vec::new()
        } else {
            let scene = &self.tabs[i].scene;
            let document = &scene.document;
            let layers = document
                .layers
                .iter()
                .filter(|layer| !layer.name.starts_with('*'))
                .map(|layer| (ContentKind::Layer, layer.name.as_str(), Some(layer.color)));
            let blocks = scene
                .placeable_block_names()
                .map(|name| (ContentKind::Block, name, None));
            let patterns = crate::scene::model::hatch_patterns::catalog()
                .iter()
                .map(|entry| (ContentKind::HatchPattern, entry.name.as_str(), None));
            let views = document
                .views
                .iter()
                .map(|view| (ContentKind::View, view.name.as_str(), None));
            ranked_content(&input, layers.chain(blocks).chain(patterns).chain(views))
        };
        self.command_line.set_content_matches(&input, matches);
    }

    /// Use a drawing name picked in the suggestions, through the command
    /// that a user would type for it.
    pub(in crate::app) fn run_content_suggestion(&mut self, item: ContentMatch) -> Task<Message> {
        self.command_line.input.clear();
        self.command_line.autocomplete_cursor = None;
        let name = item.name;
        match item.kind {
            // One undo step, as the ribbon's layer list takes.
            ContentKind::Layer => {
                let i = self.active_tab;
                self.push_undo_snapshot(i, "CLAYER");
                match self.set_current_layer_name(i, &name) {
                    Ok(()) => {
                        let name_arg = &name;
                        self.command_line
                            .push_output(crate::tf!("CLAYER set to \"{name_arg}\"").as_ref());
                    }
                    Err(error) => self.command_line.push_error(&error),
                }
                Task::none()
            }
            ContentKind::Block => self.dispatch_then_answer("-INSERT", &[&name]),
            // As a pick in the Hatch panel, so that the pattern also wins
            // over a selected hatch's, which HATCH starts with.
            ContentKind::HatchPattern => {
                let task = self.dispatch_command("HATCH");
                let pattern = crate::ui::window::hatch_panel::HatchPanelMsg::Pattern(name);
                Task::batch([task, self.on_hatch_panel(pattern)])
            }
            // Restore first, so that a view named as a preset (`TOP`) is
            // restored and not the preset.
            ContentKind::View => self.dispatch_then_answer("-VIEW", &["_R", &name]),
        }
    }

    /// Start `command` and answer its prompts with `answers`, each as one
    /// input: a name with spaces stays one name.
    fn dispatch_then_answer(&mut self, command: &str, answers: &[&str]) -> Task<Message> {
        let mut tasks = vec![self.dispatch_command(command)];
        for answer in answers {
            if self.tabs[self.active_tab].active_cmd.is_none() {
                break;
            }
            tasks.push(self.feed_active_cmd(answer));
        }
        Task::batch(tasks)
    }
}
