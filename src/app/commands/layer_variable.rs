//! HATCHLAYER and XREFLAYER: the layer new hatches and new external
//! references go on. Kept in the drawing's variable dictionary; "." (or
//! empty) means the current layer.

use crate::app::{Message, OpenCADStudio};
use iced::Task;

const NAMES: &[&str] = &["HATCHLAYER", "XREFLAYER"];

impl OpenCADStudio {
    /// The layer variable `name` of drawing `i`, `None` for the current layer.
    pub(crate) fn layer_variable(&self, i: usize, name: &str) -> Option<String> {
        crate::io::drawing_variable(&self.tabs[i].scene.document, name)
            .map(|layer| layer.trim().to_string())
            .filter(|layer| !layer.is_empty() && layer != ".")
    }

    /// The layer a new object governed by `name` takes: that layer (created
    /// when it does not exist yet) or the current layer.
    pub(crate) fn new_variable_layer(&mut self, i: usize, name: &str) -> String {
        // A value that is no valid layer name (`bad/name`) would create a
        // layer no other program can name; it means the current layer.
        match self
            .layer_variable(i, name)
            .filter(|layer| crate::scene::valid_block_name(layer))
        {
            Some(layer) => {
                self.tabs[i].scene.ensure_layer(&layer);
                layer
            }
            None => self.tabs[i].active_layer.clone(),
        }
    }

    /// `HATCHLAYER [value]`, `XREFLAYER [value]` and their `SETVAR` forms.
    pub(super) fn dispatch_layer_variable(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let rest = cmd.strip_prefix("SETVAR ").map(str::trim).unwrap_or(cmd);
        let (typed, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let name = *NAMES
            .iter()
            .find(|name| typed.trim_start_matches(['_', '.']).eq_ignore_ascii_case(name))?;
        let value = value.trim().trim_matches('"').trim();
        if value.is_empty() {
            let current = self
                .layer_variable(i, name)
                .unwrap_or_else(|| crate::tr!("layer-variable", "use-current"));
            self.command_line.push_output(&crate::tr!(
                "layer-variable",
                "prompt",
                name = name,
                current = current
            ));
            self.pending_setvar = Some(name.to_string());
            return Some(Task::none());
        }
        let stored = if value == "." { "." } else { value };
        if crate::io::drawing_variable(&self.tabs[i].scene.document, name).as_deref() != Some(stored)
        {
            crate::io::set_drawing_variable(&mut self.tabs[i].scene.document, name, stored);
            self.tabs[i].dirty = true;
        }
        Some(Task::none())
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;

    #[test]
    fn hatchlayer_with_an_illegal_name_uses_the_current_layer() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let _ = app.dispatch_layer_variable("HATCHLAYER bad/name", i);
        let current = app.tabs[i].active_layer.clone();
        assert_eq!(app.new_variable_layer(i, "HATCHLAYER"), current);
        assert!(!app.tabs[i].scene.document.layers.contains("bad/name"));

        let _ = app.dispatch_layer_variable("HATCHLAYER Hatches", i);
        assert_eq!(app.new_variable_layer(i, "HATCHLAYER"), "Hatches");
        assert!(app.tabs[i].scene.document.layers.contains("Hatches"));
    }
}
