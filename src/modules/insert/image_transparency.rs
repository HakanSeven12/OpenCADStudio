// TRANSPARENCY — whether raster images show their transparent pixels as
// transparent or draw every pixel in its stored colour.
//
//   Select image(s):
//   Enter transparency mode [ON/OFF] <OFF>:     (the default is the first
//                                                image's current mode)

use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult};

pub struct TransparencyCommand {
    handles: Vec<Handle>,
    /// `None` while images are being selected; then the default mode.
    default_on: Option<bool>,
}

impl TransparencyCommand {
    pub fn new() -> Self {
        Self { handles: Vec::new(), default_on: None }
    }

    /// The mode prompt for images already chosen.
    pub fn mode(handles: Vec<Handle>, default_on: bool) -> Self {
        Self { handles, default_on: Some(default_on) }
    }

    fn apply(&mut self, on: bool) -> CmdResult {
        let command = if on { "TRANSPARENCY ON" } else { "TRANSPARENCY OFF" };
        CmdResult::Relaunch(command.to_string(), std::mem::take(&mut self.handles))
    }
}

impl CadCommand for TransparencyCommand {
    fn name(&self) -> &'static str {
        "TRANSPARENCY"
    }

    fn prompt(&self) -> String {
        match self.default_on {
            None => crate::t!("Select image(s):").into_owned(),
            Some(true) => crate::t!("Enter transparency mode [ON/OFF] <ON>:").into_owned(),
            Some(false) => crate::t!("Enter transparency mode [ON/OFF] <OFF>:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.default_on {
            None => Vec::new(),
            Some(_) => vec![CmdOption::new("ON", "ON"), CmdOption::new("OFF", "OFF")],
        }
    }

    fn is_selection_gathering(&self) -> bool {
        self.default_on.is_none()
    }

    fn wants_text_input(&self) -> bool {
        self.default_on.is_some()
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let default_on = self.default_on?;
        Some(match text.trim().to_uppercase().as_str() {
            "" => self.apply(default_on),
            "ON" => self.apply(true),
            "OFF" => self.apply(false),
            _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
        })
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.default_on {
            None if self.handles.is_empty() => CmdResult::Cancel,
            // The app keeps only the images and asks for the mode.
            None => CmdResult::Relaunch(
                "TRANSPARENCY MODE".to_string(),
                std::mem::take(&mut self.handles),
            ),
            Some(on) => self.apply(on),
        }
    }
}
