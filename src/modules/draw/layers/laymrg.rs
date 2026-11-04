use codec::Handle;
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::Scene;

#[derive(PartialEq)]
enum Step {
    Sources,
    Target,
    Confirm,
}

pub struct LayMrgCommand {
    layers: Vec<String>,
    objects: rustc_hash::FxHashMap<Handle, String>,
    current: String,
    sources: Vec<String>,
    target: String,
    previous: rustc_hash::FxHashSet<Handle>,
    step: Step,
    name_input: bool,
}

impl LayMrgCommand {
    pub fn new(scene: &Scene, current: &str, selected: Vec<Handle>) -> (Self, Option<String>) {
        let mut command = Self {
            layers: scene.document.layers.names().map(str::to_string).collect(),
            objects: scene
                .document
                .entities()
                .map(|e| (e.common().handle, e.common().layer.clone()))
                .collect(),
            current: current.to_string(),
            sources: vec![],
            target: String::new(),
            previous: selected.iter().copied().collect(),
            step: Step::Sources,
            name_input: false,
        };
        let mut messages = vec![];
        for handle in selected {
            if let Some(layer) = command.objects.get(&handle).cloned() {
                if let Some(message) = command.add_source(layer) {
                    messages.push(message);
                }
            }
        }
        if !command.sources.is_empty() {
            messages.push(command.selected_message());
            command.step = Step::Target;
        }
        (
            command,
            if messages.is_empty() {
                None
            } else {
                Some(messages.join("\n"))
            },
        )
    }

    fn add_source(&mut self, layer: String) -> Option<String> {
        if layer == "0" {
            return Some(crate::t!("LAYMRG: layer \"0\" cannot be merged away.").into_owned());
        }
        if layer.eq_ignore_ascii_case(&self.current) {
            return Some(
                crate::t!(
                    "LAYMRG: cannot merge the current layer. Make another layer current first."
                )
                .into_owned(),
            );
        }
        if !self.sources.contains(&layer) {
            self.sources.push(layer);
        }
        None
    }

    fn selected_message(&self) -> String {
        let layers = self.sources.join(", ");
        crate::tf!("Selected layers: {layers}.").into_owned()
    }

    fn accept_layer(&mut self, layer: String) -> CmdResult {
        if self.step == Step::Sources {
            let message = self
                .add_source(layer)
                .unwrap_or_else(|| self.selected_message());
            CmdResult::ReportMeasurement(message)
        } else {
            if self.sources.contains(&layer) {
                return CmdResult::ReportError(
                    crate::t!("LAYMRG: target layer is in the source list.").into_owned(),
                );
            }
            self.target = layer;
            self.step = Step::Confirm;
            let n = self.sources.len();
            let target = &self.target;
            CmdResult::ReportMeasurement(format!(
                "{}\n{}",
                crate::t!("******** WARNING ********"),
                crate::tf!("You are about to merge {n} layer(s) into layer \"{target}\".")
            ))
        }
    }
}

impl CadCommand for LayMrgCommand {
    fn name(&self) -> &'static str {
        "LAYMRG"
    }

    fn prompt(&self) -> String {
        if self.name_input {
            return crate::t!("Enter layer name:").into_owned();
        }
        match self.step {
            Step::Sources => crate::t!("Select object on layer to merge or [Name]:").into_owned(),
            Step::Target => crate::t!("Select object on target layer or [Name]:").into_owned(),
            Step::Confirm => crate::t!("Do you wish to continue? [Yes/No] <No>:").into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.name_input {
            return vec![];
        }
        if self.step == Step::Confirm {
            vec![CmdOption::new("Yes", "Yes"), CmdOption::new("No", "No")]
        } else {
            vec![CmdOption::new("Name", "Name")]
        }
    }

    fn input_kind(&self) -> InputKind {
        if self.name_input {
            InputKind::FreeText
        } else {
            InputKind::SingleToken
        }
    }

    fn is_selection_gathering(&self) -> bool {
        !self.name_input && self.step != Step::Confirm
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        if !self.is_selection_gathering() {
            return CmdResult::NeedPoint;
        }
        // The GUI sends the whole accumulated selection. At the target step,
        // prefer a newly picked handle, not a source from the previous step.
        let fresh = handles
            .iter()
            .rev()
            .find(|h| !self.previous.contains(*h))
            .copied();
        self.previous = handles.iter().copied().collect();
        if self.step == Step::Target {
            return fresh
                .or_else(|| handles.last().copied())
                .and_then(|h| self.objects.get(&h).cloned())
                .map_or(CmdResult::NeedPoint, |layer| self.accept_layer(layer));
        }
        let mut messages = vec![];
        for handle in handles {
            if let Some(layer) = self.objects.get(&handle).cloned() {
                if let Some(message) = self.add_source(layer) {
                    messages.push(message);
                }
            }
        }
        messages.push(self.selected_message());
        CmdResult::ReportMeasurement(messages.join("\n"))
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        if text.is_empty() {
            return Some(self.on_enter());
        }
        if self.step == Step::Confirm {
            return Some(
                if text.eq_ignore_ascii_case("yes") || text.eq_ignore_ascii_case("y") {
                    CmdResult::MergeLayers {
                        sources: self.sources.clone(),
                        target: self.target.clone(),
                    }
                } else if text.eq_ignore_ascii_case("no") || text.eq_ignore_ascii_case("n") {
                    CmdResult::Cancel
                } else {
                    CmdResult::NeedPoint
                },
            );
        }
        if !self.name_input && (text.eq_ignore_ascii_case("name") || text.eq_ignore_ascii_case("n"))
        {
            self.name_input = true;
            return Some(CmdResult::NeedPoint);
        }
        let layer = self
            .layers
            .iter()
            .find(|name| name.eq_ignore_ascii_case(text))
            .cloned();
        Some(if let Some(layer) = layer {
            self.name_input = false;
            self.accept_layer(layer)
        } else {
            let name = text;
            CmdResult::ReportError(crate::tf!("LAYMRG: no layer named \"{name}\".").into_owned())
        })
    }

    fn on_enter(&mut self) -> CmdResult {
        if self.step == Step::Sources && !self.sources.is_empty() {
            self.name_input = false;
            self.step = Step::Target;
            CmdResult::NeedPoint
        } else {
            CmdResult::Cancel
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }
}
