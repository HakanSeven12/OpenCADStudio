//! DIMLAYER: the layer new dimensions go on. Kept in the drawing's variable
//! dictionary; "." (or empty) means the current layer.

use crate::app::{Message, OpenCADStudio};
use iced::Task;

const NAME: &str = "DIMLAYER";

impl OpenCADStudio {
    /// DIMLAYER of drawing `i`, `None` for the current layer.
    pub(crate) fn dim_layer(&self, i: usize) -> Option<String> {
        crate::io::drawing_variable(&self.tabs[i].scene.document, NAME)
            .map(|layer| layer.trim().to_string())
            .filter(|layer| !layer.is_empty() && layer != ".")
    }

    pub(crate) fn set_dim_layer(&mut self, i: usize, layer: &str) {
        let value = match layer.trim() {
            "" | "." => ".",
            other => other,
        };
        let current = crate::io::drawing_variable(&self.tabs[i].scene.document, NAME);
        if current.as_deref().unwrap_or(".") == value {
            return;
        }
        crate::io::set_drawing_variable(&mut self.tabs[i].scene.document, NAME, value);
        self.tabs[i].dirty = true;
    }

    /// The layer a new dimension takes: DIMLAYER (created when it does not
    /// exist yet) or the current layer.
    pub(crate) fn new_dimension_layer(&mut self, i: usize) -> String {
        match self.dim_layer(i) {
            Some(layer) => {
                self.tabs[i].scene.ensure_layer(&layer);
                layer
            }
            None => self.tabs[i].active_layer.clone(),
        }
    }

    /// `DIMLAYER [value]` and `SETVAR DIMLAYER value`.
    pub(super) fn dispatch_dim_layer(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !name.trim_start_matches(['_', '.']).eq_ignore_ascii_case(NAME) {
            return None;
        }
        let value = value.trim();
        if value.is_empty() {
            let current = self
                .dim_layer(i)
                .unwrap_or_else(|| crate::tr!("dim-layer", "use-current"));
            self.command_line
                .push_output(&crate::tr!("dim-layer", "prompt", current = current));
            self.pending_setvar = Some(NAME.to_string());
        } else {
            self.set_dim_layer(i, value.trim_matches('"'));
        }
        Some(Task::none())
    }
}
