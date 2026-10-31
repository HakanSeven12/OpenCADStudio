//! Select a Lookup parameter, then open its property table.
use crate::command::{CadCommand, CmdResult, InputKind, PointPickContext};
use codec::Handle;
use glam::DVec3;

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub name: String,
    pub point: DVec3,
}

pub(crate) struct LookupActionCommand {
    document: u64,
    definition: Handle,
    candidates: Vec<Candidate>,
    context: Option<PointPickContext>,
}
impl LookupActionCommand {
    pub(crate) fn new(document: u64, definition: Handle, candidates: Vec<Candidate>) -> Self {
        Self {
            document,
            definition,
            candidates,
            context: None,
        }
    }
    fn choose(&self, candidate: &Candidate) -> CmdResult {
        CmdResult::OpenBlockLookupTable {
            document: self.document,
            definition: self.definition,
            name: candidate.name.clone(),
        }
    }
}
impl CadCommand for LookupActionCommand {
    fn name(&self) -> &'static str {
        "BACTION"
    }
    fn input_kind(&self) -> InputKind {
        InputKind::FreeText
    }
    fn prompt(&self) -> String {
        crate::t!("BACTION Lookup  Select a parameter endpoint or enter its name:").into_owned()
    }
    fn wants_point_pick_context(&self) -> bool {
        true
    }
    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }
    fn on_text_input(&mut self, s: &str) -> Option<CmdResult> {
        let s = s.trim();
        let matches = self
            .candidates
            .iter()
            .filter(|candidate| candidate.name.eq_ignore_ascii_case(s))
            .collect::<Vec<_>>();
        Some(if matches.len() == 1 {
            self.choose(matches[0])
        } else {
            CmdResult::ReportError(
                crate::t!("Enter a unique Lookup parameter name or pick its location.")
                    .into_owned(),
            )
        })
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let Some(c) = self.context else {
            return CmdResult::ReportError(
                crate::t!("Enter the Lookup parameter name.").into_owned(),
            );
        };
        let screen =
            |p: DVec3| crate::scene::pick::hit_test::world_to_screen(p, c.view, c.eye, c.bounds);
        let click = screen(point);
        let candidate = self
            .candidates
            .iter()
            .map(|p| (screen(p.point).distance(click), p))
            .filter(|(d, _)| d.is_finite() && *d <= c.aperture_px.max(10.))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p);
        match candidate {
            Some(candidate) => self.choose(candidate),
            None => CmdResult::ReportError(crate::t!("Pick a Lookup parameter grip.").into_owned()),
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
