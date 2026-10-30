//! JUSTIFYTEXT, SCALETEXT and TEXTALIGN (Annotate > Text).

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::annotate::text_edit::{
    align_anchor, align_positions, frame_size, text_align_setting, is_text, justified, moved,
    parse_handles, scaled, text_height, Justify, JustifyTextCommand, ScaleTextCommand,
    TextAlignCommand, TextFrame,
};
use crate::modules::draw::select::SelectObjectsCommand;
use codec::{EntityType, Handle};
use iced::Task;

impl OpenCADStudio {
    pub(super) fn dispatch_text_edit(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let rest = rest.trim();
        match verb.trim_start_matches('.').to_ascii_uppercase().as_str() {
            name @ ("JUSTIFYTEXT" | "SCALETEXT") => {
                let preselected = self.selected_text(i);
                if !preselected.is_empty() {
                    return Some(self.start_text_prompt(i, name, preselected));
                }
                let pending = if name == "JUSTIFYTEXT" {
                    "_JUSTIFYTEXTSEL"
                } else {
                    "_SCALETEXTSEL"
                };
                let command = SelectObjectsCommand::filtered(name, pending, "Select objects:", is_text);
                Some(self.start_count_command(i, Box::new(command)))
            }
            name @ ("_JUSTIFYTEXTSEL" | "_SCALETEXTSEL") => {
                let handles = self.selected_text(i);
                // Nothing selected ends the command quietly.
                if handles.is_empty() {
                    return Some(Task::none());
                }
                let name = if name == "_JUSTIFYTEXTSEL" { "JUSTIFYTEXT" } else { "SCALETEXT" };
                Some(self.start_text_prompt(i, name, handles))
            }
            "_JUSTIFYTEXTAPPLY" => {
                let (option, list) = rest.split_once(' ').unwrap_or((rest, ""));
                if let Some(j) = Justify::parse(option) {
                    let handles = parse_handles(list);
                    self.replace_texts(i, "JUSTIFYTEXT", &handles, |doc, e| justified(e, doc, j));
                }
                self.tabs[i].scene.deselect_all();
                Some(Task::none())
            }
            "_SCALETEXTAPPLY" => {
                self.apply_scaletext(i, rest);
                Some(Task::none())
            }
            "TEXTALIGN" => {
                self.tabs[i].scene.deselect_all();
                self.command_line.push_output(&TextAlignCommand::current_settings());
                Some(self.start_count_command(i, Box::new(TextAlignCommand::new())))
            }
            "_TEXTALIGNAPPLY" => {
                self.apply_textalign(i, rest);
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// The selected TEXT and MTEXT, in selection order.
    fn selected_text(&self, i: usize) -> Vec<Handle> {
        let scene = &self.tabs[i].scene;
        scene
            .selected_handles_in_order()
            .into_iter()
            .filter(|h| scene.document.get_entity(*h).is_some_and(is_text))
            .collect()
    }

    fn start_text_prompt(&mut self, i: usize, name: &str, handles: Vec<Handle>) -> Task<Message> {
        let command: Box<dyn CadCommand> = if name == "JUSTIFYTEXT" {
            Box::new(JustifyTextCommand::new(handles))
        } else {
            let text_size = self.tabs[i].scene.document.header.text_height;
            Box::new(ScaleTextCommand::new(handles, text_size))
        };
        self.start_count_command(i, command)
    }

    /// Replace each of `handles` by what `change` makes of it, as one undo
    /// step; returns how many changed.
    fn replace_texts(
        &mut self,
        i: usize,
        label: &str,
        handles: &[Handle],
        mut change: impl FnMut(&codec::Document, &EntityType) -> Option<EntityType>,
    ) -> usize {
        let doc = &self.tabs[i].scene.document;
        let updates: Vec<(Handle, EntityType)> = handles
            .iter()
            .filter(|h| !self.tabs[i].scene.is_layer_locked(**h))
            .filter_map(|h| {
                let entity = doc.get_entity(*h)?;
                change(doc, entity).map(|new| (*h, new))
            })
            .collect();
        let changed: Vec<Handle> = updates.iter().map(|(h, _)| *h).collect();
        let mut updates = updates.into_iter();
        self.apply_property_op(i, label, &changed, |app, handle| {
            let Some((_, new)) = updates.next() else {
                return;
            };
            if let Some(entity) = app.tabs[i]
                .scene
                .document
                .entities_mut()
                .find(|e| e.common().handle == handle)
            {
                *entity = new;
            }
        });
        changed.len()
    }

    /// `_SCALETEXTAPPLY <base> <H|S|P|M> <value or text> <handles>`.
    fn apply_scaletext(&mut self, i: usize, rest: &str) {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        let [base, mode, value, list] = parts[..] else {
            return;
        };
        let base = match base {
            "Existing" => None,
            other => Justify::parse(other),
        };
        let handles = parse_handles(list);
        self.tabs[i].scene.deselect_all();
        let doc = &self.tabs[i].scene.document;
        // The height each object gets; `None` leaves it out.
        let target: Box<dyn Fn(&codec::Document, &EntityType) -> Option<f64>> = match mode {
            "H" => {
                let h: f64 = value.parse().unwrap_or(0.0);
                Box::new(move |_, _| Some(h))
            }
            "S" => {
                let k: f64 = value.parse().unwrap_or(1.0);
                Box::new(move |_, e| text_height(e).map(|h| h * k))
            }
            "M" => {
                let source = parse_handles(value)
                    .first()
                    .and_then(|h| doc.get_entity(*h))
                    .and_then(text_height);
                let Some(h) = source else {
                    return;
                };
                Box::new(move |_, _| Some(h))
            }
            // Paper height: annotative objects only, at the current
            // annotation scale.
            "P" => {
                let h: f64 = value.parse().unwrap_or(0.0);
                let scale = self.tabs[i].scene.creation_annotation_multiplier();
                let ignored = handles
                    .iter()
                    .filter_map(|h| doc.get_entity(*h))
                    .filter(|e| !crate::scene::annotative::is_annotative(doc, e))
                    .count();
                if ignored > 0 {
                    self.command_line.push_output(&crate::tr!(
                        "text-edit",
                        "non-annotative",
                        count = ignored.to_string()
                    ));
                }
                Box::new(move |doc, e| {
                    crate::scene::annotative::is_annotative(doc, e).then_some(h * scale)
                })
            }
            _ => return,
        };
        let n = self.replace_texts(i, "SCALETEXT", &handles, |doc, e| {
            let new_height = target(doc, e)?;
            let old = text_height(e).filter(|h| *h > 0.0)?;
            scaled(e, doc, base, new_height / old)
        });
        if n > 0 || mode != "P" {
            self.command_line
                .push_output(&crate::tr!("text-edit", "changed", count = n.to_string()));
        }
    }

    /// `_TEXTALIGNAPPLY {"handles", "target", "first", "second"}`.
    fn apply_textalign(&mut self, i: usize, rest: &str) {
        let Ok(request) = serde_json::from_str::<serde_json::Value>(rest) else {
            return;
        };
        let handles = parse_handles(request["handles"].as_str().unwrap_or(""));
        let target = request["target"].as_str().map(parse_handles).and_then(|h| h.first().copied());
        let point = |v: &serde_json::Value| Some([v.get(0)?.as_f64()?, v.get(1)?.as_f64()?]);
        let Some(second) = point(&request["second"]) else {
            return;
        };
        let doc = &self.tabs[i].scene.document;
        let anchor_of = |h: Handle| {
            let e = doc.get_entity(h)?;
            let frame = TextFrame::of(e, doc)?;
            let j = align_anchor(text_align_setting());
            let (w, hgt) = frame_size(&frame);
            Some((frame.anchor(j), w, hgt))
        };
        // Aligning to a text starts the line at its anchor; that text stays.
        let first = match target {
            Some(t) => anchor_of(t).map(|(p, _, _)| p),
            None => point(&request["first"]),
        };
        let Some(first) = first else {
            return;
        };
        let movers: Vec<Handle> = handles.into_iter().filter(|h| Some(*h) != target).collect();
        let anchors: Vec<([f64; 2], f64, f64)> = movers.iter().filter_map(|h| anchor_of(*h)).collect();
        if anchors.len() != movers.len() {
            return;
        }
        let mut list = anchors.clone();
        let mut handles_in_order = movers.clone();
        if let Some(t) = target {
            if let Some(a) = anchor_of(t) {
                list.insert(0, a);
                handles_in_order.insert(0, t);
            }
        }
        let goals = align_positions(&list, first, second);
        let deltas: std::collections::HashMap<Handle, [f64; 2]> = handles_in_order
            .iter()
            .zip(list.iter().zip(goals))
            .map(|(h, ((from, _, _), to))| (*h, [to[0] - from[0], to[1] - from[1]]))
            .collect();
        self.replace_texts(i, "TEXTALIGN", &movers, |_, e| {
            let delta = deltas.get(&e.common().handle)?;
            moved(e, *delta)
        });
        self.tabs[i].scene.deselect_all();
    }
}
