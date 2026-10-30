//! TEXTLAYER: the layer new TEXT and MTEXT go on. Kept in the drawing's
//! variable dictionary; empty means the current layer.

use crate::app::{Message, OpenCADStudio};
use codec::Handle;
use iced::Task;

const NAME: &str = "TEXTLAYER";

impl OpenCADStudio {
    /// TEXTLAYER of drawing `i`, `None` for the current layer.
    pub(crate) fn text_layer(&self, i: usize) -> Option<String> {
        crate::io::drawing_variable(&self.tabs[i].scene.document, NAME)
            .filter(|layer| !layer.trim().is_empty())
    }

    pub(crate) fn set_text_layer(&mut self, i: usize, layer: &str) {
        let value = if layer.trim() == "." { "" } else { layer.trim() };
        if self.text_layer(i).as_deref().unwrap_or("") == value {
            return;
        }
        crate::io::set_drawing_variable(&mut self.tabs[i].scene.document, NAME, value);
        self.tabs[i].dirty = true;
    }

    /// `TEXTLAYER [value]` and `SETVAR TEXTLAYER value`.
    pub(super) fn dispatch_text_layer(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !name.trim_start_matches(['_', '.']).eq_ignore_ascii_case(NAME) {
            return None;
        }
        let value = value.trim();
        if value.is_empty() {
            let current = self
                .text_layer(i)
                .unwrap_or_else(|| crate::tr!("text-layer", "use-current"));
            self.command_line
                .push_output(&crate::tr!("text-layer", "prompt", current = current));
            self.pending_setvar = Some(NAME.to_string());
        } else {
            let value = value.trim_matches('"').trim();
            // A layer name never has these characters; nothing at all is
            // refused too ("." is the current layer).
            if value.is_empty()
                || value.contains([
                    '<', '>', '/', '\\', '"', ':', ';', '?', '*', '|', ',', '=', '`',
                ])
            {
                self.command_line
                    .push_error(crate::t!("Cannot set TEXTLAYER to that value.").as_ref());
                let current = self
                    .text_layer(i)
                    .unwrap_or_else(|| crate::tr!("text-layer", "use-current"));
                self.command_line
                    .push_output(&crate::tr!("text-layer", "prompt", current = current));
                self.pending_setvar = Some(NAME.to_string());
            } else {
                self.set_text_layer(i, value);
            }
        }
        Some(Task::none())
    }

    /// Commit a TEXT or MTEXT the TEXT / MTEXT commands made: on TEXTLAYER
    /// (created when it does not exist yet) when one is set.
    pub(crate) fn commit_new_text(&mut self, mut entity: codec::EntityType) -> Option<Handle> {
        let i = self.active_tab;
        match self.text_layer(i) {
            Some(layer) => {
                self.tabs[i].scene.ensure_layer(&layer);
                entity.as_entity_mut().set_layer(layer);
                self.commit_entity_handle_preserve_layer(entity)
            }
            None => self.commit_entity_handle(entity),
        }
    }
}
