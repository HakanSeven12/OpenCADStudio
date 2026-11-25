//! Drawing names at the command line. While no command runs, the layers,
//! blocks, hatch patterns, named views and text, dimension and visual styles
//! whose names contain the typed text show under the command names. A picked
//! layer or style becomes current, a block is inserted, HATCH starts with a
//! pattern and a view is restored.

use crate::app::{Message, OpenCADStudio};
use crate::modules::view::visual_style::{mode_for_style_name, VISUAL_STYLES};
use crate::modules::StyleKey;
use crate::ui::command_line::{ranked_content, ContentKind, ContentMatch};
use document::entities::ViewportRenderMode as Mode;
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
            // As the ribbon's text style list: no shape files.
            let text_styles = document
                .text_styles
                .iter()
                .filter(|style| !style.is_shape_file)
                .map(|style| (ContentKind::TextStyle, style.name.as_str(), None));
            let dim_styles = document
                .dim_styles
                .iter()
                .map(|style| (ContentKind::DimStyle, style.name.as_str(), None));
            // The visual styles by the names the picker shows, then by
            // the names of the drawing's own styles (Realistic, …).
            let visual_names: Vec<String> = VISUAL_STYLES
                .iter()
                .map(|style| crate::t!(style.label).into_owned())
                .chain(
                    crate::io::visual_styles::offered(document)
                        .into_iter()
                        .filter(|(_, handle)| named_style_mode(document, *handle).is_some())
                        .map(|(name, _)| name),
                )
                .collect();
            let visual_styles = visual_names
                .iter()
                .map(|name| (ContentKind::VisualStyle, name.as_str(), None));
            let names = layers
                .chain(blocks)
                .chain(patterns)
                .chain(views)
                .chain(text_styles)
                .chain(dim_styles)
                .chain(visual_styles);
            ranked_content(&input, names)
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
            // As a pick in the ribbon's text style list, in one undo step.
            ContentKind::TextStyle => {
                let i = self.active_tab;
                self.push_undo_snapshot(i, "TEXTSTYLE");
                let _ = self.on_ribbon_style_changed(StyleKey::TextStyle, name.clone());
                self.tabs[i].dirty = true;
                let line = crate::tr!("text-style", "now-current", name = name);
                self.command_line.push_output(&crate::i18n::plain(line));
                Task::none()
            }
            ContentKind::DimStyle => {
                self.push_undo_snapshot(self.active_tab, "CDIMSTY");
                self.dispatch_then_answer("CDIMSTY", &[&name])
            }
            // The command the ribbon's visual style list runs. A drawing style
            // name runs the closest of the modes, as the menu bar's Realistic
            // does. A visual style is view state: it takes no undo step.
            ContentKind::VisualStyle => match self.visual_style_command(&name) {
                Some(command) => self.dispatch_command(command),
                None => Task::none(),
            },
        }
    }

    /// The command that draws the visual style `name`: a name the picker
    /// shows, or the name of one of the drawing's own styles.
    fn visual_style_command(&self, name: &str) -> Option<&'static str> {
        let mode = match VISUAL_STYLES
            .iter()
            .find(|style| crate::t!(style.label) == name)
        {
            Some(style) => style.mode,
            None => {
                let document = &self.tabs[self.active_tab].scene.document;
                crate::io::visual_styles::offered(document)
                    .into_iter()
                    .find(|(offered, _)| offered == name)
                    .and_then(|(_, handle)| named_style_mode(document, handle))?
            }
        };
        VISUAL_STYLES
            .iter()
            .find(|style| style.mode == mode)
            .map(|style| style.command)
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

/// The mode that the drawing's visual style `handle` draws as, when it is a
/// style that OCS draws.
fn named_style_mode(
    document: &document::Document,
    handle: document::types::Handle,
) -> Option<Mode> {
    match document.objects.get(&handle) {
        Some(document::objects::ObjectType::VisualStyle(style)) => {
            mode_for_style_name(&style.description).map(|(mode, _)| mode)
        }
        _ => None,
    }
}
