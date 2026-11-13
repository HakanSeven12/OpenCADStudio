//! Before the first annotative text of a session, the annotation scale is
//! asked for: the scale picked becomes the current annotation scale
//! (CANNOSCALE). "Don't ask again" turns the question off for good.

use std::sync::atomic::{AtomicBool, Ordering};

use iced::Task;

use crate::app::{Message, OpenCADStudio};
use crate::ui::window::anno_scale_prompt::{AnnoScaleMsg, AnnoScalePrompt};

/// The question is asked (the user's setting).
static ON: AtomicBool = AtomicBool::new(true);
/// It was asked in this session.
static ASKED: AtomicBool = AtomicBool::new(false);

pub(crate) fn annotation_scale_prompt() -> bool {
    ON.load(Ordering::Relaxed)
}

pub(crate) fn set_annotation_scale_prompt(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

impl OpenCADStudio {
    /// The command just started makes a text with an annotative current
    /// style: ask for the annotation scale, once per session.
    pub(super) fn ask_annotation_scale_if_first(&mut self) {
        let i = self.active_tab;
        if !annotation_scale_prompt()
            || ASKED.load(Ordering::Relaxed)
            || self.active_modal.is_some()
            // A running script has nobody to answer it.
            || self.script_running
        {
            return;
        }
        let makes_text = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| matches!(command.name(), "TEXT" | "MTEXT"));
        if !makes_text {
            return;
        }
        let doc = &self.tabs[i].scene.document;
        let annotative = doc
            .text_styles
            .iter()
            .find(|style| style.name.eq_ignore_ascii_case(&doc.header.current_text_style_name))
            .is_some_and(|style| style.annotative);
        if !annotative {
            return;
        }
        ASKED.store(true, Ordering::Relaxed);
        let scales: Vec<String> = self.tabs[i]
            .scene
            .scale_list()
            .into_iter()
            .map(|(name, _, _)| name)
            .collect();
        let current = doc.header.current_annotation_scale.clone();
        let picked = scales
            .iter()
            .find(|name| name.eq_ignore_ascii_case(&current))
            .cloned()
            .or_else(|| scales.first().cloned())
            .unwrap_or(current);
        self.anno_scale_prompt = Some(AnnoScalePrompt {
            scales,
            picked,
            dont_ask: false,
        });
        self.active_modal = Some(crate::app::ModalKind::AnnotationScalePrompt);
    }

    pub(in crate::app) fn on_anno_scale_prompt(&mut self, message: AnnoScaleMsg) -> Task<Message> {
        let Some(state) = self.anno_scale_prompt.as_mut() else {
            return Task::none();
        };
        match message {
            AnnoScaleMsg::Pick(name) => state.picked = name,
            AnnoScaleMsg::DontAsk(on) => state.dont_ask = on,
            AnnoScaleMsg::Ok => {
                let i = self.active_tab;
                let (picked, dont_ask) = (state.picked.clone(), state.dont_ask);
                let previous = self.tabs[i].scene.displayed_annotation_scale_handle();
                if let Some(handle) = self.tabs[i].scene.set_annotation_scale_named(&picked) {
                    if self.annotation_auto_scale > 0 {
                        self.tabs[i].scene.add_annotation_scale_to_objects(
                            handle,
                            previous,
                            self.annotation_auto_scale as u8,
                        );
                    }
                    self.tabs[i].dirty = true;
                }
                if dont_ask {
                    set_annotation_scale_prompt(false);
                    self.save_config();
                }
                self.close_active_modal();
            }
        }
        Task::none()
    }
}
