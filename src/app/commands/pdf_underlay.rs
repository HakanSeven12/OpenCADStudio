//! The right-edge PDF underlay and xref tools: their state comes from the
//! selection, their buttons edit it.

use super::*;
use codec::entities::{Underlay, UnderlayDisplayFlags, UnderlayType};
use crate::ui::ribbon::UnderlayContext;

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

    /// The selected xrefs, or nothing when anything else is selected.
    pub(in crate::app) fn selected_xrefs(&self, i: usize) -> Vec<codec::Handle> {
        let doc = &self.tabs[i].scene.document;
        let mut out = Vec::new();
        for (handle, entity) in self.tabs[i].scene.selected_entities() {
            let xref = match entity {
                codec::EntityType::Insert(ins) => doc
                    .block_records
                    .get(&ins.block_name)
                    .is_some_and(|br| br.flags.is_xref || br.flags.is_xref_overlay),
                _ => false,
            };
            if !xref {
                return Vec::new();
            }
            out.push(handle);
        }
        out
    }

    /// The selection context for the right-edge tools: the selected PDF
    /// underlay's switches while only underlays are selected, or that only
    /// xrefs are.
    pub(in crate::app) fn sync_underlay_tab(&mut self) {
        let i = self.active_tab;
        let xref = !self.tabs[i].is_start && !self.selected_xrefs(i).is_empty();
        let context = if self.tabs[i].is_start {
            None
        } else {
            self.selected_pdf_underlays(i).first().map(|(_, u)| {
                UnderlayContext {
                    monochrome: u.flags.contains(UnderlayDisplayFlags::MONOCHROME),
                    shown: u.flags.contains(UnderlayDisplayFlags::ON),
                    snap: crate::scene::model::pdf_vector::pdf_osnap(),
                }
            })
        };
        self.ribbon.set_underlay_context(context, xref);
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

    /// The right-edge tools (host-only command names).
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
            // External Reference tab: the selected xrefs.
            "_XREFEDIT" | "_XREFOPEN" => {
                let Some(&handle) = self.selected_xrefs(i).first() else {
                    return Some(Task::none());
                };
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entity(handle, true);
                let command = if cmd == "_XREFEDIT" { "REFEDIT" } else { "XOPEN" };
                return Some(self.dispatch_command(command));
            }
            "_XREFCLIP" => {
                use crate::command::CadCommand;
                let inserts = self.selected_xrefs(i);
                if inserts.is_empty() {
                    return Some(Task::none());
                }
                let clipped = inserts.iter().any(|h| {
                    crate::scene::pick::xclip::filter_handle(&self.tabs[i].scene.document, *h).is_some()
                });
                let (command, first) =
                    crate::modules::insert::xclip::XclipCommand::start_new_boundary(inserts, clipped);
                if let crate::command::CmdResult::ReportMeasurement(text) = first {
                    for line in text.lines() {
                        self.command_line.push_output(line);
                    }
                }
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
            }
            "_XREFUNCLIP" => {
                let inserts = self.selected_xrefs(i);
                if !inserts.is_empty() {
                    self.apply_xclip(i, inserts, crate::modules::insert::xclip::XclipAction::Delete);
                }
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
