// PASTESPEC — paste the clipboard in a chosen format: the copied objects, the
// copied objects as a block, or the system clipboard's text as multi-line text.

use crate::t;
use codec::entities::mtext::AttachmentPoint;
use codec::entities::MText;
use codec::types::Vector3;
use codec::EntityType;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};

pub struct PasteSpecCommand {
    /// Objects copied inside the application are waiting on the clipboard.
    objects: bool,
    /// The system clipboard's text, already escaped for MTEXT.
    text: Option<String>,
    style: String,
    height: f64,
    placing_text: bool,
    plane: WorkingPlane,
}

impl PasteSpecCommand {
    pub fn new(objects: bool, text: Option<&str>, style: String, height: f64) -> Self {
        Self {
            objects,
            text: text.map(mtext_value),
            style,
            height,
            placing_text: false,
            plane: WorkingPlane::default(),
        }
    }
}

/// Plain text as an MTEXT value: control characters escaped, lines as
/// paragraphs.
fn mtext_value(text: &str) -> String {
    text.trim_end_matches(['\r', '\n'])
        .replace('\\', "\\\\")
        .replace('{', "\\{")
        .replace('}', "\\}")
        .replace("\r\n", "\\P")
        .replace(['\r', '\n'], "\\P")
}

impl CadCommand for PasteSpecCommand {
    fn name(&self) -> &'static str {
        "PASTESPEC"
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn prompt(&self) -> String {
        if self.placing_text {
            t!("PASTESPEC  Specify insertion point:").into_owned()
        } else {
            t!("PASTESPEC  Paste as [Objects/Block/Text]:").into_owned()
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.placing_text {
            return Vec::new();
        }
        let mut options = Vec::new();
        if self.objects {
            options.push(CmdOption::new("Objects", "O"));
            options.push(CmdOption::new("Block", "B"));
        }
        if self.text.is_some() {
            options.push(CmdOption::new("Text", "T"));
        }
        options
    }

    fn wants_text_input(&self) -> bool {
        !self.placing_text
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.placing_text {
            return None;
        }
        match text.trim().trim_start_matches('_').to_ascii_uppercase().as_str() {
            "O" | "OBJECTS" if self.objects => Some(CmdResult::Dispatch("PASTECLIP".into())),
            "B" | "BLOCK" if self.objects => Some(CmdResult::Dispatch("PASTEBLOCK".into())),
            "T" | "TEXT" if self.text.is_some() => {
                self.placing_text = true;
                Some(CmdResult::NeedPoint)
            }
            _ => None,
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        let Some(value) = self.text.clone().filter(|_| self.placing_text) else {
            return CmdResult::NeedPoint;
        };
        let p = self.plane.to_local(pt);
        let mut mtext = MText::new();
        mtext.value = value;
        mtext.insertion_point = Vector3::new(p.x, p.y, p.z);
        mtext.height = self.height;
        mtext.style = self.style.clone();
        mtext.attachment_point = AttachmentPoint::TopLeft;
        mtext.rectangle_width = 0.0;
        CmdResult::CommitAndExit(self.plane.place_entity(EntityType::MText(mtext)))
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.placing_text {
            return CmdResult::NeedPoint;
        }
        // Enter takes the first format offered.
        let first = self.options().first().map(|o| o.keyword.clone());
        first
            .and_then(|keyword| self.on_text_input(&keyword))
            .unwrap_or(CmdResult::Cancel)
    }
}

inventory::submit!(crate::command::CommandRegistration { names: &["PASTESPEC"] });

#[cfg(test)]
mod tests {
    use super::mtext_value;

    #[test]
    fn plain_text_becomes_escaped_paragraphs() {
        assert_eq!(mtext_value("a{b}\\c\r\nline 2\n"), "a\\{b\\}\\\\c\\Pline 2");
    }
}
