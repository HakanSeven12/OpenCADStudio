//! The contextual PDF Underlay ribbon tab: its values come from the selected
//! underlays, its buttons edit them.

use super::*;
use codec::entities::{Underlay, UnderlayDisplayFlags, UnderlayType};
use crate::ui::ribbon::{UnderlayContext, UnderlayTabMsg};

impl OpenCADStudio {
    /// The selected PDF underlays, or nothing when anything else is selected.
    fn selected_pdf_underlays(&self, i: usize) -> Vec<(codec::Handle, Underlay)> {
        let mut out = Vec::new();
        for (handle, entity) in self.tabs[i].scene.selected_entities() {
            match entity {
                codec::EntityType::Underlay(u) if u.underlay_type == UnderlayType::Pdf => {
                    out.push((handle, u.clone()));
                }
                _ => return Vec::new(),
            }
        }
        out
    }

    /// Show the tab while only PDF underlays are selected.
    pub(in crate::app) fn sync_underlay_tab(&mut self) {
        let i = self.active_tab;
        let context = if self.tabs[i].is_start {
            None
        } else {
            self.selected_pdf_underlays(i).first().map(|(_, u)| {
                UnderlayContext::new(
                    u.contrast,
                    u.fade,
                    u.flags.contains(UnderlayDisplayFlags::MONOCHROME),
                    u.flags.contains(UnderlayDisplayFlags::ON),
                    crate::scene::model::pdf_vector::pdf_osnap(),
                )
            })
        };
        self.ribbon.set_underlay_context(context);
    }

    /// One undo step that edits every selected PDF underlay.
    fn edit_selected_underlays(&mut self, i: usize, label: &str, edit: impl Fn(&mut Underlay)) {
        let handles: Vec<_> = self
            .selected_pdf_underlays(i)
            .into_iter()
            .map(|(h, _)| h)
            .filter(|h| !self.tabs[i].scene.is_layer_locked(*h))
            .collect();
        if handles.is_empty() {
            return;
        }
        self.push_undo_snapshot(i, label);
        for handle in &handles {
            if let Some(codec::EntityType::Underlay(u)) =
                self.tabs[i].scene.document.get_entity_mut(*handle)
            {
                edit(u);
            }
            self.tabs[i].scene.reseed_derived_caches(*handle);
        }
        let changes: Vec<_> = handles
            .iter()
            .map(|h| (*h, crate::scene::ChangeKind::Modified))
            .collect();
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }

    pub(in crate::app) fn update_underlay_tab(&mut self, message: UnderlayTabMsg) -> Task<Message> {
        let i = self.active_tab;
        let Some(ctx) = self.ribbon.underlay_context_mut() else {
            return Task::none();
        };
        match message {
            UnderlayTabMsg::Contrast(v) => {
                ctx.contrast = v;
                ctx.contrast_text = v.to_string();
            }
            UnderlayTabMsg::Fade(v) => {
                ctx.fade = v;
                ctx.fade_text = v.to_string();
            }
            UnderlayTabMsg::ContrastText(s) => ctx.contrast_text = s,
            UnderlayTabMsg::FadeText(s) => ctx.fade_text = s,
            UnderlayTabMsg::Commit => {
                // A typed value outside 0–100 or not a number keeps the value.
                let parse = |s: &str, old: u8| {
                    s.trim().parse::<u8>().ok().filter(|v| *v <= 100).unwrap_or(old)
                };
                let (contrast, fade) = (
                    parse(&ctx.contrast_text, ctx.contrast),
                    parse(&ctx.fade_text, ctx.fade),
                );
                ctx.contrast_text = contrast.to_string();
                ctx.fade_text = fade.to_string();
                self.edit_selected_underlays(i, "PDFADJUST", |u| {
                    u.set_contrast(contrast);
                    u.set_fade(fade);
                });
            }
        }
        Task::none()
    }

    /// The tab's buttons (host-only command names).
    pub(super) fn dispatch_pdf_underlay(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            "_PDFULMONO" => {
                let on = !self
                    .selected_pdf_underlays(i)
                    .first()
                    .is_some_and(|(_, u)| u.flags.contains(UnderlayDisplayFlags::MONOCHROME));
                self.edit_selected_underlays(i, "PDFADJUST", |u| u.set_monochrome(on));
            }
            "_PDFULSHOW" => {
                let on = !self
                    .selected_pdf_underlays(i)
                    .first()
                    .is_some_and(|(_, u)| u.flags.contains(UnderlayDisplayFlags::ON));
                self.edit_selected_underlays(i, "PDFUNDERLAY", |u| u.set_on(on));
            }
            "_PDFULSNAP" => {
                let on = !crate::scene::model::pdf_vector::pdf_osnap();
                crate::scene::model::pdf_vector::set_pdf_osnap(on);
                self.tabs[i].scene.reseed_underlays();
                self.sync_underlay_tab();
            }
            "_PDFULUNCLIP" => {
                self.edit_selected_underlays(i, "PDFCLIP", |u| {
                    u.clip_boundary_vertices.clear();
                    u.clip_inverted = false;
                    u.flags -= UnderlayDisplayFlags::CLIPPING;
                });
            }
            "_PDFULCLIP" => {
                use crate::command::CadCommand;
                let Some((handle, underlay)) = self.selected_pdf_underlays(i).into_iter().next()
                else {
                    return Some(Task::none());
                };
                let (command, first) =
                    crate::modules::insert::pdf_clip::PdfClipCommand::new_boundary(handle, underlay);
                if let crate::command::CmdResult::ReportMeasurement(text) = first {
                    for line in text.lines() {
                        self.command_line.push_output(line);
                    }
                }
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "_PDFULIMPORT" => {
                use crate::command::CadCommand;
                let Some((handle, _)) = self.selected_pdf_underlays(i).into_iter().next() else {
                    return Some(Task::none());
                };
                let command = crate::modules::insert::pdf_import::PdfImportCommand::for_underlay(handle);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            _ => return None,
        }
        Some(self.finish_dispatch(cmd))
    }
}
