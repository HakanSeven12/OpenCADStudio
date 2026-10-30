//! INSERTOBJ, OLELINKS and the OLE variables (OLEHIDE, OLEQUALITY,
//! OLESTARTUP, OLEFRAME, MSOLESCALE); the Properties edits of an OLE
//! object's size, scales and aspect lock.

use crate::app::{Message, OpenCADStudio};
use crate::entities::ole2frame::{self, FrameState};
use codec::{EntityType, Handle};
use iced::Task;

/// OLEQUALITY: the plot quality new OLE objects take (0 monochrome, 1 low
/// graphics, 2 high graphics, 3 automatic). A profile setting.
static OLE_QUALITY: std::sync::atomic::AtomicI16 = std::sync::atomic::AtomicI16::new(3);

pub(crate) fn ole_quality() -> i16 {
    OLE_QUALITY.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn set_ole_quality(value: i16) {
    OLE_QUALITY.store(value.clamp(0, 3), std::sync::atomic::Ordering::Relaxed);
}

/// An integer OLE variable: its range and the message a value outside it
/// gets (the prompt then asks again).
fn integer_range(name: &str) -> Option<(i16, i16, &'static str)> {
    Some(match name {
        "OLEHIDE" | "OLEQUALITY" => (0, 3, "Requires an integer between 0 and 3."),
        "OLESTARTUP" => (0, 1, "Requires 0 or 1 only."),
        "OLEFRAME" => (0, 2, "Requires an integer between 0 and 2."),
        _ => return None,
    })
}

impl OpenCADStudio {
    pub(super) fn dispatch_ole(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let verb = verb.trim_start_matches(['_', '.']).to_ascii_uppercase();
        let rest = rest.trim();
        if verb == "SETVAR" {
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let name = name.to_ascii_uppercase();
            if integer_range(&name).is_some() || name == "MSOLESCALE" {
                if value.trim().is_empty() {
                    return Some(Task::none());
                }
                return Some(self.set_ole_variable(i, &name, Some(value.trim())));
            }
            return None;
        }
        match verb.as_str() {
            "INSERTOBJ" => Some(Task::perform(
                crate::io::pick_insert_object(),
                Message::InsertObjectResult,
            )),
            // Linked OLE objects and their sources; nothing when there are none.
            "OLELINKS" => {
                let links: Vec<String> = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter_map(|entity| match entity {
                        EntityType::Ole2Frame(ole)
                            if ole.ole_object_type == codec::entities::OleObjectType::Link =>
                        {
                            Some(ole.source_application.clone())
                        }
                        _ => None,
                    })
                    .collect();
                for (n, source) in links.iter().enumerate() {
                    self.command_line.push_output(&format!("{}.  {source}", n + 1));
                }
                Some(Task::none())
            }
            name if integer_range(name).is_some() || name == "MSOLESCALE" => {
                let value = (!rest.is_empty()).then_some(rest);
                Some(self.set_ole_variable(i, name, value))
            }
            _ => None,
        }
    }

    fn ole_variable(&self, i: usize, name: &str) -> String {
        let document = &self.tabs[i].scene.document;
        match name {
            "OLEHIDE" => ole2frame::hide_mode().to_string(),
            "OLEQUALITY" => ole_quality().to_string(),
            "OLESTARTUP" => i16::from(document.header.ole_startup).to_string(),
            "OLEFRAME" => ole2frame::frame_mode(document).to_string(),
            _ => format!("{:.4}", ms_ole_scale(document)),
        }
    }

    /// Shows the prompt, or takes `value`: a refused value says why and asks
    /// again, except a negative MSOLESCALE, which ends the command.
    fn set_ole_variable(&mut self, i: usize, name: &str, value: Option<&str>) -> Task<Message> {
        let ask = |app: &mut Self| {
            let current = app.ole_variable(i, name);
            app.command_line
                .push_output(&format!("Enter new value for {name} <{current}>:"));
            app.pending_setvar = Some(name.to_string());
        };
        let Some(value) = value else {
            ask(self);
            return Task::none();
        };
        if name == "MSOLESCALE" {
            match value.parse::<f64>() {
                Ok(scale) if scale >= 0.0 && scale.is_finite() => {
                    let document = &mut self.tabs[i].scene.document;
                    if (ms_ole_scale(document) - scale).abs() > 0.0 {
                        crate::io::set_drawing_variable(document, "MSOLESCALE", &scale.to_string());
                        self.tabs[i].dirty = true;
                    }
                }
                Ok(_) => {
                    self.command_line
                        .push_error("Cannot set MSOLESCALE to that value.");
                    self.command_line.push_output("*Invalid*");
                }
                Err(_) => {
                    self.command_line
                        .push_error("Requires numeric distance or two points.");
                    ask(self);
                }
            }
            return Task::none();
        }
        let Some((min, max, message)) = integer_range(name) else {
            return Task::none();
        };
        let Some(v) = value.parse::<i16>().ok().filter(|v| (min..=max).contains(v)) else {
            self.command_line.push_error(message);
            ask(self);
            return Task::none();
        };
        match name {
            "OLEHIDE" => {
                if v != ole2frame::hide_mode() {
                    ole2frame::set_hide_mode(v);
                    for tab in &mut self.tabs {
                        tab.scene.bump_geometry();
                    }
                    self.save_config();
                }
            }
            "OLEQUALITY" => {
                if v != ole_quality() {
                    set_ole_quality(v);
                    self.save_config();
                }
            }
            "OLESTARTUP" => {
                self.tabs[i].scene.document.header.ole_startup = v == 1;
                self.tabs[i].dirty = true;
            }
            _ => {
                let document = &mut self.tabs[i].scene.document;
                if ole2frame::frame_mode(document) != v {
                    crate::io::set_drawing_variable(document, "OLEFRAME", &v.to_string());
                    self.tabs[i].scene.bump_geometry();
                    self.tabs[i].dirty = true;
                }
            }
        }
        Task::none()
    }

    /// Width, Height and the scale rows of OLE objects. The upper-left corner
    /// stays; with the aspect locked the other side follows. An object read
    /// from a file takes the size it is first given as its 100 %.
    pub(in crate::app) fn resize_ole_frames(
        &mut self,
        i: usize,
        handles: &[Handle],
        field: &str,
        value: &str,
    ) {
        let Some(v) = crate::entities::common::parse_f64(value) else {
            return;
        };
        let handles: Vec<Handle> = handles
            .iter()
            .copied()
            .filter(|h| matches!(self.tabs[i].scene.document.get_entity(*h), Some(EntityType::Ole2Frame(_))))
            .collect();
        self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
            let state = app.tabs[i]
                .ole_frames
                .get(&handle.value())
                .copied()
                .unwrap_or_default();
            let Some(EntityType::Ole2Frame(ole)) = app.tabs[i].scene.document.get_entity_mut(handle)
            else {
                return;
            };
            let (_, w, h) = ole2frame::frame_box(ole);
            if w <= 0.0 || h <= 0.0 {
                return;
            }
            let base = state.base.unwrap_or((w, h));
            let follow = |side: f64, from: f64, to: f64| {
                if state.lock_aspect {
                    side * to / from
                } else {
                    side
                }
            };
            let (nw, nh) = match field {
                "ole_width" => (v, follow(h, w, v)),
                "ole_height" => (follow(w, h, v), v),
                "ole_scale_width" => {
                    let nw = base.0 * v / 100.0;
                    (nw, follow(h, w, nw))
                }
                _ => {
                    let nh = base.1 * v / 100.0;
                    (follow(w, h, nh), nh)
                }
            };
            ole2frame::set_size(ole, nw, nh);
            let base = state
                .base
                .unwrap_or(if field.starts_with("ole_scale") { (w, h) } else { (nw, nh) });
            app.tabs[i].ole_frames.insert(
                handle.value(),
                FrameState {
                    base: Some(base),
                    ..state
                },
            );
        });
    }

    /// The Lock aspect row: kept with the open drawing, as the reference keeps it.
    pub(in crate::app) fn set_ole_lock_aspect(&mut self, i: usize, value: &str) {
        let on = value == crate::t!("Yes").as_ref() || value.eq_ignore_ascii_case("yes");
        for handle in self.property_target_handles(i) {
            if matches!(self.tabs[i].scene.document.get_entity(handle), Some(EntityType::Ole2Frame(_))) {
                self.tabs[i]
                    .ole_frames
                    .entry(handle.value())
                    .or_default()
                    .lock_aspect = on;
            }
        }
        self.refresh_properties();
    }

    /// The scale and lock rows the open drawing keeps for an OLE object.
    pub(in crate::app) fn fill_ole_rows(
        &self,
        i: usize,
        handle: Handle,
        ole: &codec::entities::Ole2Frame,
        sections: &mut [crate::scene::model::object::PropSection],
    ) {
        use crate::scene::model::object::PropValue;
        let state = self.tabs[i]
            .ole_frames
            .get(&handle.value())
            .copied()
            .unwrap_or_default();
        let (_, w, h) = ole2frame::frame_box(ole);
        let scale = |size: f64, base: Option<f64>| match base {
            Some(base) if base > 0.0 => crate::entities::underlay::plain_number(size / base * 100.0),
            _ => "0".to_string(),
        };
        for prop in sections.iter_mut().flat_map(|s| s.props.iter_mut()) {
            match prop.field {
                "ole_scale_width" => prop.value = PropValue::EditText(scale(w, state.base.map(|b| b.0))),
                "ole_scale_height" => prop.value = PropValue::EditText(scale(h, state.base.map(|b| b.1))),
                "ole_lock_aspect" => {
                    if let PropValue::Choice { selected, .. } = &mut prop.value {
                        *selected = if state.lock_aspect { crate::t!("Yes") } else { crate::t!("No") }
                            .into_owned();
                    }
                }
                _ => {}
            }
        }
    }
}

/// MSOLESCALE of the drawing (1 when not set).
fn ms_ole_scale(document: &codec::Document) -> f64 {
    crate::io::drawing_variable(document, "MSOLESCALE")
        .and_then(|value| value.trim().parse::<f64>().ok())
        .unwrap_or(1.0)
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["INSERTOBJ", "OLELINKS", "OLEHIDE", "OLEQUALITY", "OLESTARTUP", "OLEFRAME", "MSOLESCALE"]
});
