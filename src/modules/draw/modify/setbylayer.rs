use acadrust::Handle;
use glam::DVec3;
use crate::command::{CadCommand, CmdOption, CmdResult};

static MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);
pub fn mode() -> u8 { MODE.load(std::sync::atomic::Ordering::Relaxed) }
pub fn set_mode(value: u8) { MODE.store(value, std::sync::atomic::Ordering::Relaxed); }

pub fn apply_mask(common: &mut acadrust::entities::EntityCommon, mask: u8, change_byblock: bool) -> bool {
    let before = common.clone();
    if mask & 1 != 0 && (change_byblock || common.color != acadrust::types::Color::ByBlock) {
        common.color = acadrust::types::Color::ByLayer;
        common.color_name = None;
        common.color_book_handle = None;
    }
    if mask & 2 != 0 && (change_byblock || !common.linetype.eq_ignore_ascii_case("ByBlock")) {
        common.linetype = "ByLayer".to_string();
        common.linetype_handle = None;
    }
    if mask & 4 != 0 && (change_byblock || common.line_weight != acadrust::types::LineWeight::ByBlock) {
        common.line_weight = acadrust::types::LineWeight::ByLayer;
    }
    if mask & 8 != 0 && (change_byblock || common.material_flags != 1) {
        common.material_flags = 0;
        common.material_handle = None;
    }
    if mask & 16 != 0 && (change_byblock || common.plotstyle_flags != 1) {
        common.plotstyle_flags = 0;
        common.plotstyle_handle = None;
    }
    if mask & 128 != 0 && (change_byblock || common.transparency != acadrust::types::Transparency::BY_BLOCK) {
        common.transparency = acadrust::types::Transparency::BY_LAYER;
    }
    *common != before
}

enum Step { Selection, ByBlock, Blocks }

pub struct SetByLayerCommand {
    selected: Vec<Handle>,
    step: Step,
    change_byblock: bool,
    include_blocks: bool,
}

impl SetByLayerCommand {
    pub fn new(selected: Vec<Handle>) -> Self {
        let step = if selected.is_empty() { Step::Selection } else { Step::ByBlock };
        Self { selected, step, change_byblock: mode() & 32 != 0, include_blocks: mode() & 64 != 0 }
    }

    fn answer(&mut self, yes: bool) -> CmdResult {
        match self.step {
            Step::ByBlock => {
                self.change_byblock = yes;
                self.step = Step::Blocks;
                CmdResult::NeedPoint
            }
            Step::Blocks => {
                let flags = (mode() & !(32 | 64)) | if self.change_byblock { 32 } else { 0 } | if yes { 64 } else { 0 };
                set_mode(flags);
                CmdResult::Relaunch(
                format!("SETBYLAYER_APPLY {} {}", u8::from(self.change_byblock), u8::from(yes)),
                self.selected.clone(),
            ) },
            Step::Selection => CmdResult::NeedPoint,
        }
    }
}

impl CadCommand for SetByLayerCommand {
    fn name(&self) -> &'static str { "SETBYLAYER" }
    fn prompt(&self) -> String {
        match self.step {
            Step::Selection => "Select objects:".into(),
            Step::ByBlock => format!("Change ByBlock to ByLayer? [Yes/No] <{}>:", if self.change_byblock { "Yes" } else { "No" }),
            Step::Blocks => format!("Include blocks? [Yes/No] <{}>:", if self.include_blocks { "Yes" } else { "No" }),
        }
    }
    fn is_selection_gathering(&self) -> bool { matches!(self.step, Step::Selection) }
    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.selected = handles;
        CmdResult::NeedPoint
    }
    fn options(&self) -> Vec<CmdOption> {
        if self.is_selection_gathering() { Vec::new() }
        else { vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")] }
    }
    fn wants_text_input(&self) -> bool { !self.is_selection_gathering() }
    fn on_point(&mut self, _: DVec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult {
        if self.is_selection_gathering() {
            if self.selected.is_empty() { CmdResult::Cancel }
            else { self.step = Step::ByBlock; CmdResult::NeedPoint }
        } else { self.answer(if matches!(self.step, Step::ByBlock) { self.change_byblock } else { self.include_blocks }) }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(match text.trim().to_uppercase().as_str() {
            "Y" | "YES" => self.answer(true),
            "N" | "NO" => self.answer(false),
            _ => CmdResult::NeedPoint,
        })
    }
}

pub struct ModeCommand;
impl CadCommand for ModeCommand {
    fn name(&self) -> &'static str { "SETBYLAYERMODE" }
    fn prompt(&self) -> String { format!("Enter new value for SETBYLAYERMODE <{}>:", mode()) }
    fn wants_text_input(&self) -> bool { true }
    fn on_point(&mut self, _: DVec3) -> CmdResult { CmdResult::NeedPoint }
    fn on_enter(&mut self) -> CmdResult { CmdResult::Cancel }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        Some(if let Ok(value) = text.trim().parse::<u8>() {
            set_mode(value); CmdResult::Cancel
        } else { CmdResult::NeedPoint })
    }
}
inventory::submit!(crate::command::CommandRegistration { names: &["SETBYLAYERMODE"] });

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::EntityCommon;
    use acadrust::types::{Color, Transparency};

    #[test]
    fn mask_changes_only_requested_properties() {
        let mut common = EntityCommon::default();
        common.color = Color::from_index(3);
        common.linetype = "DASHED".into();
        common.transparency = Transparency::from_percent(40.0);

        assert!(apply_mask(&mut common, 1 | 128, false));
        assert_eq!(common.color, Color::ByLayer);
        assert_eq!(common.linetype, "DASHED");
        assert_eq!(common.transparency, Transparency::BY_LAYER);
    }

    #[test]
    fn byblock_values_are_kept_unless_requested() {
        let mut common = EntityCommon::default();
        common.color = Color::ByBlock;
        common.linetype = "ByBlock".into();
        common.transparency = Transparency::BY_BLOCK;

        assert!(!apply_mask(&mut common, 1 | 2 | 128, false));
        assert!(apply_mask(&mut common, 1 | 2 | 128, true));
        assert_eq!(common.color, Color::ByLayer);
        assert_eq!(common.linetype, "ByLayer");
        assert_eq!(common.transparency, Transparency::BY_LAYER);
    }
}
