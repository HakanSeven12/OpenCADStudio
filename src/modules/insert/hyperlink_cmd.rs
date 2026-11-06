// HYPERLINK object selection, -HYPERLINK and HYPERLINKOPTIONS.

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use document::Handle;
use glam::DVec3;

fn hex(handles: &[Handle]) -> String {
    handles
        .iter()
        .map(|h| format!("{:X}", h.value()))
        .collect::<Vec<_>>()
        .join(",")
}

fn keyword(text: &str) -> String {
    text.trim().trim_start_matches(['_', '.']).to_ascii_uppercase()
}

/// HYPERLINK without a pickfirst selection: `Select objects:`, then the
/// Insert / Edit Hyperlink window opens for them.
pub struct HyperlinkSelectCommand;

impl CadCommand for HyperlinkSelectCommand {
    fn name(&self) -> &'static str {
        "HYPERLINK"
    }

    fn prompt(&self) -> String {
        crate::t!("Select objects:").into_owned()
    }

    fn is_selection_gathering(&self) -> bool {
        true
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::Cancel;
        }
        CmdResult::Dispatch(format!("_HYPERLINKDIALOG {}", hex(&handles)))
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Option,
    InsertOption,
    Select,
    FirstCorner,
    OtherCorner,
    Url,
    Location,
    Description,
    RemoveSelect,
}

/// -HYPERLINK: Insert (on objects or on a new area rectangle) or Remove.
pub struct DashHyperlinkCommand {
    step: Step,
    handles: Vec<Handle>,
    corners: Vec<DVec3>,
    url: String,
    location: String,
}

impl DashHyperlinkCommand {
    pub fn new() -> Self {
        Self {
            step: Step::Option,
            handles: Vec::new(),
            corners: Vec::new(),
            url: String::new(),
            location: String::new(),
        }
    }

    fn invalid() -> CmdResult {
        CmdResult::ReportError(crate::tr!("hyperlink-cmd", "invalid-keyword"))
    }

    fn finish(&mut self, description: &str) -> CmdResult {
        let request = serde_json::json!({
            "handles": hex(&self.handles),
            "area": self.corners.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(),
            "url": self.url,
            "location": self.location,
            "text": description.trim(),
        });
        CmdResult::Dispatch(format!("_HYPERLINKAPPLY {request}"))
    }
}

impl CadCommand for DashHyperlinkCommand {
    fn name(&self) -> &'static str {
        "-HYPERLINK"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Option => crate::tr!("hyperlink-cmd", "option"),
            Step::InsertOption => crate::tr!("hyperlink-cmd", "insert-option"),
            Step::Select | Step::RemoveSelect => crate::t!("Select objects:").into_owned(),
            Step::FirstCorner => crate::tr!("hyperlink-cmd", "first-corner"),
            Step::OtherCorner => crate::tr!("hyperlink-cmd", "other-corner"),
            Step::Url => crate::tr!("hyperlink-cmd", "url"),
            Step::Location => crate::tr!("hyperlink-cmd", "location"),
            Step::Description => crate::tr!("hyperlink-cmd", "description"),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Option => vec![CmdOption::new("Remove", "R"), CmdOption::new("Insert", "I")],
            Step::InsertOption => vec![CmdOption::new("Area", "A"), CmdOption::new("Object", "O")],
            _ => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::Option | Step::InsertOption => InputKind::SingleToken,
            Step::Url | Step::Location | Step::Description => InputKind::FreeText,
            _ => InputKind::Point,
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, Step::Select | Step::RemoveSelect)
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, Step::Select | Step::RemoveSelect)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if handles.is_empty() {
            return CmdResult::CancelWithMessage(crate::tr!("hyperlink-cmd", "selection-error"));
        }
        if self.step == Step::RemoveSelect {
            return CmdResult::Dispatch(format!("_HYPERLINKREMOVELIST {}", hex(&handles)));
        }
        self.handles = handles;
        self.step = Step::Url;
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::FirstCorner => {
                self.corners = vec![pt];
                self.step = Step::OtherCorner;
            }
            Step::OtherCorner => {
                self.corners.push(pt);
                self.step = Step::Url;
            }
            _ => {}
        }
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match self.step {
            Step::Option => match keyword(text).as_str() {
                "" | "I" | "INSERT" => {
                    self.step = Step::InsertOption;
                    CmdResult::NeedPoint
                }
                "R" | "REMOVE" => {
                    self.step = Step::RemoveSelect;
                    CmdResult::NeedPoint
                }
                _ => Self::invalid(),
            },
            Step::InsertOption => match keyword(text).as_str() {
                "" | "O" | "OBJECT" => {
                    self.step = Step::Select;
                    CmdResult::NeedPoint
                }
                "A" | "AREA" => {
                    self.step = Step::FirstCorner;
                    CmdResult::NeedPoint
                }
                _ => Self::invalid(),
            },
            Step::Url => {
                self.url = text.trim().to_string();
                self.step = Step::Location;
                CmdResult::NeedPoint
            }
            Step::Location => {
                self.location = text.trim().to_string();
                self.step = Step::Description;
                CmdResult::NeedPoint
            }
            Step::Description => self.finish(text),
            _ => return None,
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Option
            | Step::InsertOption
            | Step::Url
            | Step::Location
            | Step::Description => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
            _ => CmdResult::NeedPoint,
        }
    }
}

/// -HYPERLINK Remove, after the listing: which of the listed links go.
pub struct HyperlinkRemovePick {
    /// Each listed object with its address, in listing order.
    entries: Vec<(Handle, String)>,
}

impl HyperlinkRemovePick {
    pub fn new(entries: Vec<(Handle, String)>) -> Self {
        Self { entries }
    }
}

impl CadCommand for HyperlinkRemovePick {
    fn name(&self) -> &'static str {
        "-HYPERLINK"
    }

    fn prompt(&self) -> String {
        crate::tr!("hyperlink-cmd", "remove-pick")
    }

    fn input_kind(&self) -> InputKind {
        InputKind::FreeText
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let answer = text.trim();
        if answer.is_empty() {
            return Some(CmdResult::Cancel);
        }
        let chosen: Vec<Handle> = if answer == "*" {
            self.entries.iter().map(|(h, _)| *h).collect()
        } else if let Ok(n) = answer.parse::<usize>() {
            self.entries
                .get(n.wrapping_sub(1))
                .map(|(h, _)| *h)
                .into_iter()
                .collect()
        } else {
            self.entries
                .iter()
                .filter(|(_, url)| url.eq_ignore_ascii_case(answer))
                .map(|(h, _)| *h)
                .collect()
        };
        Some(if chosen.is_empty() {
            CmdResult::ReportError(crate::tr!("hyperlink-cmd", "not-valid"))
        } else {
            CmdResult::Dispatch(format!("_HYPERLINKREMOVE {}", hex(&chosen)))
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// HYPERLINKOPTIONS: the hyperlink cursor, tooltip and shortcut menu switch.
pub struct HyperlinkOptionsCommand {
    current: bool,
}

impl HyperlinkOptionsCommand {
    pub fn new(current: bool) -> Self {
        Self { current }
    }
}

impl CadCommand for HyperlinkOptionsCommand {
    fn name(&self) -> &'static str {
        "HYPERLINKOPTIONS"
    }

    fn prompt(&self) -> String {
        if self.current {
            crate::tr!("hyperlink-cmd", "options-yes")
        } else {
            crate::tr!("hyperlink-cmd", "options-no")
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")]
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match keyword(text).as_str() {
            "" => CmdResult::Cancel,
            "Y" | "YES" => CmdResult::Dispatch("_HYPERLINKOPTIONSSET 1".into()),
            "N" | "NO" => CmdResult::Dispatch("_HYPERLINKOPTIONSSET 0".into()),
            _ => CmdResult::ReportError(crate::tr!("hyperlink-cmd", "invalid-keyword")),
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["HYPERLINK", "-HYPERLINK", "HYPERLINKOPTIONS", "HYPERLINKBASE"]
});
