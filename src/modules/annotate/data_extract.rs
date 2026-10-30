// DATAEXTRACTION command — launches the Data Extraction wizard.

use crate::modules::{IconKind, ModuleEvent, ToolDef};

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/data_extract.svg"));

pub fn tool() -> ToolDef {
    ToolDef {
        id: "DATAEXTRACTION",
        label: "Extract\nData",
        icon: ICON,
        event: ModuleEvent::Command("DATAEXTRACTION".to_string()),
    }
}

/// -DATAEXTRACTION: asks for a saved extraction's settings file and runs it.
/// A relative path is looked up next to the drawing first.
pub struct TemplateCommand {
    pub drawing_dir: Option<std::path::PathBuf>,
}

impl crate::command::CadCommand for TemplateCommand {
    fn name(&self) -> &'static str {
        "-DATAEXTRACTION"
    }

    fn prompt(&self) -> String {
        crate::t!("-DATAEXTRACTION  Enter the template file path for the extraction:").into_owned()
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<crate::command::CmdResult> {
        use crate::command::CmdResult;
        let path = std::path::Path::new(text.trim().trim_matches('"'));
        if path.as_os_str().is_empty() {
            return Some(CmdResult::NeedPoint);
        }
        let path = match &self.drawing_dir {
            Some(dir) if path.is_relative() && dir.join(path).is_file() => dir.join(path),
            _ => path.to_path_buf(),
        };
        if !path.is_file() {
            return Some(CmdResult::ReportMeasurement(
                crate::t!("Invalid name.").into_owned(),
            ));
        }
        Some(CmdResult::Dispatch(format!(
            "-DATAEXTRACTION _RUN {}",
            path.display()
        )))
    }

    fn on_point(&mut self, _point: glam::DVec3) -> crate::command::CmdResult {
        crate::command::CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> crate::command::CmdResult {
        crate::command::CmdResult::NeedPoint
    }
}
