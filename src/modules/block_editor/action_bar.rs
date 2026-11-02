use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, PointPickContext};
use crate::scene::block_action_bars::Group;
use codec::Handle;
use glam::DVec3;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BACTIONBAR"]
});

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Show,
    Hide,
    Reset,
}

pub(crate) struct ActionBarCommand {
    document: u64,
    definition: Handle,
    groups: Vec<Group>,
    selected: Option<Handle>,
    mode: Option<Mode>,
    context: Option<PointPickContext>,
}
impl ActionBarCommand {
    pub(crate) fn new(
        document: u64,
        definition: Handle,
        groups: Vec<Group>,
        selected: Option<Handle>,
    ) -> Self {
        Self {
            document,
            definition,
            groups,
            selected,
            mode: None,
            context: None,
        }
    }
    fn finish(&self, parameters: Vec<Handle>) -> CmdResult {
        CmdResult::SetBlockActionBars {
            document: self.document,
            definition: self.definition,
            mode: self.mode.unwrap(),
            parameters,
        }
    }
    fn all(&self) -> Vec<Handle> {
        self.groups.iter().map(|group| group.parameter).collect()
    }
}
impl CadCommand for ActionBarCommand {
    fn name(&self) -> &'static str {
        "BACTIONBAR"
    }
    fn input_kind(&self) -> InputKind {
        if self.mode.is_none() {
            InputKind::SingleToken
        } else {
            InputKind::FreeText
        }
    }
    fn prompt(&self) -> String {
        if self.mode.is_none() {
            crate::t!("BACTIONBAR  [Show/Hide/Reset]:")
        } else if self.selected.is_some() {
            crate::t!("BACTIONBAR  Select a parameter, enter its name or [All] <Selected>:")
        } else {
            crate::t!("BACTIONBAR  Select a parameter, enter its name or [All] <All>:")
        }
        .into_owned()
    }
    fn options(&self) -> Vec<CmdOption> {
        if self.mode.is_none() {
            vec![
                CmdOption::new("Show", "Show"),
                CmdOption::new("Hide", "Hide"),
                CmdOption::new("Reset", "Reset"),
            ]
        } else {
            vec![CmdOption::new("All", "All")]
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if self.mode.is_none() {
            let mode = match text.to_ascii_uppercase().as_str() {
                "SHOW" | "S" => Mode::Show,
                "HIDE" | "H" => Mode::Hide,
                "RESET" | "R" => Mode::Reset,
                _ => {
                    return Some(CmdResult::ReportError(
                        crate::t!("Choose Show, Hide or Reset.").into_owned(),
                    ))
                }
            };
            self.mode = Some(mode);
            return Some(if matches!(mode, Mode::Reset) {
                self.finish(self.all())
            } else {
                CmdResult::NeedPoint
            });
        }
        if matches!(text.to_ascii_uppercase().as_str(), "ALL" | "A") {
            return Some(self.finish(self.all()));
        }
        let matches = self
            .groups
            .iter()
            .filter(|group| group.label.eq_ignore_ascii_case(text))
            .map(|group| group.parameter)
            .collect::<Vec<_>>();
        Some(if matches.len() == 1 {
            self.finish(matches)
        } else {
            CmdResult::ReportError(
                crate::t!("Enter a unique parameter name, pick its endpoint, or choose All.")
                    .into_owned(),
            )
        })
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.mode.is_none() {
            CmdResult::Cancel
        } else {
            self.finish(
                self.selected
                    .map_or_else(|| self.all(), |selected| vec![selected]),
            )
        }
    }
    fn wants_point_pick_context(&self) -> bool {
        self.mode.is_some()
    }
    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.mode.is_none() {
            return CmdResult::ReportError(crate::t!("Choose Show, Hide or Reset.").into_owned());
        }
        let Some(context) = self.context else {
            return CmdResult::ReportError(
                crate::t!("Enter a parameter name or choose All.").into_owned(),
            );
        };
        let screen = |point| {
            crate::scene::pick::hit_test::world_to_screen(
                point,
                context.view,
                context.eye,
                context.bounds,
            )
        };
        let click = screen(point);
        self.groups
            .iter()
            .map(|group| (screen(group.anchor).distance(click), group.parameter))
            .filter(|(distance, _)| {
                distance.is_finite() && *distance <= context.aperture_px.max(10.0)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, handle)| self.finish(vec![handle]))
            .unwrap_or_else(|| {
                CmdResult::ReportError(
                    crate::t!("Pick a parameter endpoint or choose All.").into_owned(),
                )
            })
    }
}
