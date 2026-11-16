//! Before the first annotative object of a session, the annotation scale is
//! asked for: the scale picked becomes the current annotation scale
//! (CANNOSCALE). "Don't ask again" turns the question off for good.
//!
//! TEXT and MTEXT with an annotative text style, the dimension commands with
//! an annotative dimension style and MLEADER with an annotative multileader
//! style ask when they start; an annotative block reference asks once it is
//! placed. Hatches and attribute definitions do not ask.

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
    /// The command just started makes an annotative object with the current
    /// style: ask for the annotation scale, once per session.
    pub(super) fn ask_annotation_scale_if_first(&mut self) {
        let i = self.active_tab;
        let Some(name) = self.tabs[i].active_cmd.as_ref().map(|command| command.name()) else {
            return;
        };
        let doc = &self.tabs[i].scene.document;
        let annotative = match name {
            "TEXT" | "MTEXT" => doc
                .text_styles
                .iter()
                .find(|style| style.name.eq_ignore_ascii_case(&doc.header.current_text_style_name))
                .is_some_and(|style| style.annotative),
            "DIM" | "DIMLINEAR" | "DIMALIGNED" | "DIMANGULAR" | "DIMARC" | "DIMRADIUS"
            | "DIMDIAMETER" | "DIMJOGGED" | "DIMORDINATE" | "DIMBASELINE" | "DIMCONTINUE"
            | "QDIM" => crate::scene::annotative::dim_style_is_annotative(
                doc,
                &doc.header.current_dimstyle_name,
            ),
            "MLEADER" => doc.objects.values().any(|object| {
                matches!(object, document::objects::ObjectType::MultiLeaderStyle(style)
                    if style.is_annotative
                        && style.name.eq_ignore_ascii_case(&doc.header.current_mleader_style_name))
            }),
            _ => false,
        };
        if annotative {
            self.ask_annotation_scale();
        }
    }

    /// A block reference was just placed: an annotative block asks for the
    /// annotation scale, once per session.
    pub(in crate::app) fn ask_annotation_scale_after_insert(&mut self, block: &str) {
        let doc = &self.tabs[self.active_tab].scene.document;
        if crate::scene::annotative::block_is_annotative(doc, block) {
            self.ask_annotation_scale();
        }
    }

    /// Ask for the annotation scale unless it was asked in this session, is
    /// turned off, or nobody can answer.
    fn ask_annotation_scale(&mut self) {
        let i = self.active_tab;
        if !annotation_scale_prompt()
            || ASKED.load(Ordering::Relaxed)
            || self.active_modal.is_some()
            // A running script has nobody to answer it.
            || self.script_running
        {
            return;
        }
        let doc = &self.tabs[i].scene.document;
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
