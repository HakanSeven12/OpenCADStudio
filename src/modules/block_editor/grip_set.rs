//! Configure native grip counts using the command host's parameter picking.
use crate::command::{CadCommand, CmdOption, CmdResult, InputKind, PointPickContext};
use crate::scene::{block_authoring::edit, block_grips::config};
use codec::{objects::ObjectType, Document, Handle};
use glam::DVec3;

inventory::submit!(crate::command::CommandRegistration {
    names: &["BGRIPSET"]
});

pub(crate) struct GripSetCommand {
    document: u64,
    definition: Handle,
    candidates: Vec<(edit::Item, Vec<usize>)>,
    selected: Option<usize>,
    expected: Vec<(Handle, ObjectType)>,
    context: Option<PointPickContext>,
}
impl GripSetCommand {
    pub(crate) fn new(
        document: u64,
        definition: Handle,
        doc: &Document,
        selected: Option<Handle>,
    ) -> Result<Self, &'static str> {
        let candidates = edit::items(doc, definition)
            .into_iter()
            .filter_map(|mut item| {
                if let Ok(grips) = config::locations(doc, definition, item.handle) {
                    item.grips.extend(
                        grips
                            .into_iter()
                            .map(|grip| (edit::Part::Point, grip.point)),
                    );
                }
                let choices = config::choices(doc, definition, item.handle);
                (!choices.is_empty()).then_some((item, choices))
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err("Add a parameter before configuring grips.");
        }
        let selected = candidates
            .iter()
            .position(|(item, _)| Some(item.handle) == selected);
        // Capture native structures so delayed input cannot overwrite a newer edit.
        let expected = config::snapshot(doc, definition)?;
        Ok(Self {
            document,
            definition,
            candidates,
            selected,
            expected,
            context: None,
        })
    }
    fn finish(&self, count: Option<usize>) -> CmdResult {
        CmdResult::SetBlockParameterGrips {
            document: self.document,
            definition: self.definition,
            parameter: self.candidates[self.selected.unwrap()].0.handle,
            expected: self.expected.clone(),
            count,
        }
    }
}
impl CadCommand for GripSetCommand {
    fn name(&self) -> &'static str {
        "BGRIPSET"
    }
    fn input_kind(&self) -> InputKind {
        if self.selected.is_some() {
            InputKind::SingleToken
        } else {
            InputKind::FreeText
        }
    }
    fn prompt(&self) -> String {
        if let Some(index) = self.selected {
            let options = self.candidates[index]
                .1
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join("/");
            crate::tf!(
                "BGRIPSET  Number of grips [{}/Reposition] (Escape cancels):",
                options
            )
            .into_owned()
        } else {
            crate::t!("BGRIPSET  Select a parameter or enter its name:").into_owned()
        }
    }
    fn options(&self) -> Vec<CmdOption> {
        let Some(index) = self.selected else {
            return vec![];
        };
        let mut options = self.candidates[index]
            .1
            .iter()
            .map(|count| {
                let label = match count {
                    0 => "0",
                    1 => "1",
                    2 => "2",
                    _ => "4",
                };
                CmdOption::new(label, label)
            })
            .collect::<Vec<_>>();
        options.push(CmdOption::new("Reposition", "Reposition"));
        options
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if let Some(index) = self.selected {
            return Some(
                if text.eq_ignore_ascii_case("Reposition") || text.eq_ignore_ascii_case("R") {
                    self.finish(None)
                } else if let Ok(count) = text.parse::<usize>() {
                    if self.candidates[index].1.contains(&count) {
                        self.finish(Some(count))
                    } else {
                        CmdResult::ReportError(
                            crate::t!("Choose an available grip count.").into_owned(),
                        )
                    }
                } else {
                    CmdResult::ReportError(
                        crate::t!("Choose a grip count or Reposition.").into_owned(),
                    )
                },
            );
        }
        let matches = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, (item, _))| item.name.eq_ignore_ascii_case(text))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        Some(if matches.len() == 1 {
            self.selected = Some(matches[0]);
            CmdResult::NeedPoint
        } else {
            CmdResult::ReportError(
                crate::t!("Enter a unique parameter name or pick a parameter.").into_owned(),
            )
        })
    }
    fn wants_point_pick_context(&self) -> bool {
        self.selected.is_none()
    }
    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.selected.is_some() {
            return CmdResult::ReportError(
                crate::t!("Choose a grip count or Reposition.").into_owned(),
            );
        }
        let Some(context) = self.context else {
            return CmdResult::ReportError(crate::t!("Enter a parameter name.").into_owned());
        };
        let screen = |p| {
            crate::scene::pick::hit_test::world_to_screen(
                p,
                context.view,
                context.eye,
                context.bounds,
            )
        };
        let click = screen(point);
        let closest = self
            .candidates
            .iter()
            .enumerate()
            .flat_map(|(index, (item, _))| {
                item.grips
                    .iter()
                    .map(move |(_, p)| (screen(DVec3::new(p.x, p.y, p.z)).distance(click), index))
            })
            .filter(|(distance, _)| {
                distance.is_finite() && *distance <= context.aperture_px.max(10.0)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, index)) = closest {
            self.selected = Some(index);
            CmdResult::NeedPoint
        } else {
            CmdResult::ReportError(
                crate::t!("Pick a parameter definition point or label.").into_owned(),
            )
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}
