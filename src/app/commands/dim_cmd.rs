//! DIM and the steps it hands to the app (Layer, Baseline, Continue,
//! aliGn, Distribute, Undo).

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::annotate::dim_baseline::DimBaselineCommand;
use crate::modules::annotate::dim_cmd::DimCommand;
use crate::modules::annotate::dim_continue::DimContinueCommand;
use iced::Task;

impl OpenCADStudio {
    /// The layer DIM shows at its Layer prompt.
    fn dim_layer_label(&self, i: usize) -> String {
        crate::io::drawing_variable(&self.tabs[i].scene.document, "DIMLAYER")
            .map(|layer| layer.trim().to_string())
            .filter(|layer| !layer.is_empty() && layer != ".")
            .unwrap_or_else(|| crate::tr!("dim-cmd", "use-current"))
    }

    fn install_dim(&mut self, i: usize, command: DimCommand) {
        self.command_line.push_info(&command.prompt());
        self.command_line.set_step_options(command.options());
        self.tabs[i].active_cmd = Some(Box::new(command));
    }

    pub(crate) fn start_dim(&mut self, i: usize) {
        let command = DimCommand::new(self.dim_layer_label(i));
        self.install_dim(i, command);
    }

    /// The offset between baseline and distributed dimensions: the
    /// drawing's DIMDLI (the current style's unless DIM's Offset changed it).
    fn current_dimdli(&self, i: usize) -> f64 {
        let document = &self.tabs[i].scene.document;
        if !document.dim_styles.is_empty() {
            return crate::scene::creation_style::drawing_dimdli(document);
        }
        document
            .dim_styles
            .iter()
            .find(|style| {
                style
                    .name
                    .eq_ignore_ascii_case(&document.header.current_dimstyle_name)
            })
            .map(|style| style.dimdli)
            .unwrap_or(if document.header.measurement == 1 { 3.75 } else { 0.38 })
    }

    fn recent_dimension(&self, i: usize) -> Option<codec::EntityType> {
        let scene = &self.tabs[i].scene;
        scene
            .last_created_dimension
            .filter(|handle| scene.entity_belongs_to_active_space(*handle))
            .and_then(|handle| scene.document.get_entity(handle))
            .cloned()
    }

    /// DIMBASELINE; from DIM (`in_dim`) the base is picked rather than taken
    /// from the last dimension.
    pub(crate) fn dim_baseline_command(&self, i: usize, in_dim: bool) -> DimBaselineCommand {
        let recent = if in_dim { None } else { self.recent_dimension(i) };
        // Baseline dimensions are spaced by the drawing's DIMDLI, whatever
        // the base dimension's own value is (DIMCONTINUEMODE included).
        let mut command = DimBaselineCommand::new(
            recent,
            self.current_dimdli(i),
            self.dimension_continue_mode == 1,
        );
        if in_dim {
            command.run_from_dim(self.current_dimdli(i));
        }
        command
    }

    pub(crate) fn dim_continue_command(&self, i: usize, in_dim: bool) -> DimContinueCommand {
        let recent = if in_dim { None } else { self.recent_dimension(i) };
        let style_text_move = self.tabs[i]
            .scene
            .document
            .dim_styles
            .iter()
            .map(|style| (style.name.clone(), style.dimtmove))
            .collect();
        let mut command = DimContinueCommand::new(recent, self.dimension_continue_mode == 1)
            .with_style_text_move(style_text_move);
        if in_dim {
            command.run_from_dim();
        }
        command
    }

    /// The dimensions of the current space a typed extension line origin
    /// picks, within the distance a typed point picks an object by
    /// (0.002 of the space's extent).
    fn dimension_origin_picks(
        &self,
        i: usize,
    ) -> crate::modules::annotate::dim_baseline::OriginPicks {
        let document = &self.tabs[i].scene.document;
        let owner = self.tabs[i]
            .current_parametric_scope()
            .owner_handle(document);
        let mut extent = 0.0f64;
        let mut dimensions = Vec::new();
        for entity in document
            .entities()
            .filter(|entity| entity.common().owner_handle == owner)
        {
            let bounds = entity.as_entity().bounding_box();
            extent = extent
                .max((bounds.max.x - bounds.min.x).abs())
                .max((bounds.max.y - bounds.min.y).abs());
            if matches!(entity, codec::EntityType::Dimension(_)) {
                dimensions.push(entity.clone());
            }
        }
        crate::modules::annotate::dim_baseline::OriginPicks::new(
            dimensions,
            (extent * 0.002).max(1.0e-9),
        )
    }

    fn dim_offset_settings(&mut self, i: usize) {
        let offset = self.current_dimdli(i);
        self.command_line.push_output(
            crate::tr!("dim-cmd", "offset-settings", offset = format!("{offset:.6}")).as_ref(),
        );
    }

    pub(super) fn dispatch_dim_cmd(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (name, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
        match name.trim_start_matches(['_', '.']).to_ascii_uppercase().as_str() {
            "DIM" => self.start_dim(i),
            // Layer: DIMLAYER for the dimensions DIM makes next; the prompt
            // repeats until Enter or eXit.
            "DIM_LAYER" => {
                let task = self.dispatch_command(&format!("DIMLAYER {}", rest.trim()));
                let command = DimCommand::at_layer(self.dim_layer_label(i));
                self.install_dim(i, command);
                return Some(task);
            }
            "DIM_LAYER_LIST" => {
                let names: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .map(|layer| layer.name.clone())
                    .collect();
                for name in names {
                    self.command_line.push_output(&name);
                }
                let command = DimCommand::at_layer(self.dim_layer_label(i));
                self.install_dim(i, command);
            }
            "DIM_BASELINE" => {
                self.dim_offset_settings(i);
                let series = self
                    .dim_baseline_command(i, true)
                    .with_origin_picks(self.dimension_origin_picks(i));
                let mut command = DimCommand::new(self.dim_layer_label(i));
                command.adopt(Box::new(series), false);
                self.install_dim(i, command);
            }
            "DIM_CONTINUE" => {
                let series = self
                    .dim_continue_command(i, true)
                    .with_origin_picks(self.dimension_origin_picks(i));
                let mut command = DimCommand::new(self.dim_layer_label(i));
                command.adopt(Box::new(series), true);
                self.install_dim(i, command);
            }
            "DIM_ALIGN" => {
                use crate::modules::annotate::dimspace::DimSpaceCommand;
                let command = DimSpaceCommand::aligning();
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "DIM_DISTRIBUTE" => {
                use crate::modules::annotate::dim_distribute::DimDistributeCommand;
                self.dim_offset_settings(i);
                let command = DimDistributeCommand::new(self.current_dimdli(i));
                self.command_line.push_info(&command.prompt());
                self.command_line.set_step_options(command.options());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            // Undo: the last dimension goes, DIM goes on.
            "DIM_UNDO" => {
                let task = self.dispatch_command("U");
                self.start_dim(i);
                return Some(task);
            }
            _ => return None,
        }
        Some(Task::none())
    }
}
