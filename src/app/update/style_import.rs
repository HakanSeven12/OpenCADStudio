//! Import named text and dimension styles into the open style manager.

use crate::app::{Message, ModalKind, OpenCADStudio, StyleKind};
use codec::{Document, Handle};
use iced::Task;
use std::collections::VecDeque;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct StyleImportDraft {
    pub kind: StyleKind,
    pub file: String,
    pub source: Document,
    pub selected: Vec<(String, bool)>,
    pub conflict: Option<String>,
    remaining: VecDeque<String>,
    approved: Vec<(String, bool)>,
}

fn style_names(kind: StyleKind, doc: &Document) -> Vec<(String, bool)> {
    match kind {
        StyleKind::Text => doc
            .text_styles
            .iter()
            .filter(|s| !s.is_shape_file && !s.name.contains('|'))
            .map(|s| (s.name.clone(), false))
            .collect(),
        StyleKind::Dim => doc
            .dim_styles
            .iter()
            .filter(|s| !s.name.contains('|'))
            .map(|s| (s.name.clone(), false))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn style_exists(doc: &Document, kind: StyleKind, name: &str) -> bool {
    match kind {
        StyleKind::Text => doc.text_styles.get(name).is_some(),
        StyleKind::Dim => doc.dim_styles.get(name).is_some(),
        _ => false,
    }
}

impl StyleImportDraft {
    /// Stop at the next collision, recording non-conflicting styles on the way.
    fn next_conflict(&mut self, target: &Document) -> bool {
        while let Some(name) = self.remaining.pop_front() {
            if style_exists(target, self.kind, &name) {
                self.conflict = Some(name);
                return true;
            }
            self.approved.push((name, false));
        }
        false
    }
}

fn import_one(
    doc: &mut Document,
    source: &Document,
    kind: StyleKind,
    name: &str,
    overwrite: bool,
    missing_refs: &mut bool,
) -> bool {
    if style_exists(doc, kind, name) && !overwrite {
        return false;
    }
    match kind {
        StyleKind::Text => {
            let Some(mut style) = source.text_styles.get(name).cloned() else {
                return false;
            };
            if style.is_shape_file {
                return false;
            }
            if let Some(old) = doc.text_styles.get(name) {
                if old.xref_dependent {
                    return false;
                }
                style.handle = old.handle;
                style.name = old.name.clone();
            } else {
                style.handle = doc.allocate_handle();
            }
            style.xref_dependent = false;
            style.xref_block_record_handle = Handle::NULL;
            doc.text_styles.add_or_replace(style);
        }
        StyleKind::Dim => {
            let Some(mut style) = source.dim_styles.get(name).cloned() else {
                return false;
            };
            if let Some(old) = doc.dim_styles.get(name) {
                if old.xref_dependent {
                    return false;
                }
                style.handle = old.handle;
                style.name = old.name.clone();
            } else {
                style.handle = doc.allocate_handle();
            }
            if doc.text_styles.get(&style.dimtxsty).is_none() {
                let dependency = style.dimtxsty.clone();
                import_one(
                    doc,
                    source,
                    StyleKind::Text,
                    &dependency,
                    false,
                    missing_refs,
                );
            }
            if doc.text_styles.get(&style.dimtxsty).is_none() {
                *missing_refs = true;
            }
            // Source handles name the dependencies; point them at this
            // drawing's records of the same names, as a paste does.
            let mut refs = rustc_hash::FxHashMap::default();
            for text in source.text_styles.iter() {
                refs.insert(text.handle, text.name.clone());
            }
            for linetype in source.line_types.iter() {
                refs.insert(linetype.handle, linetype.name.clone());
            }
            for block in source.block_records.iter() {
                refs.insert(block.handle, block.name.clone());
            }
            let before = crate::app::command_driver::clipboard::dim_style_blocks(&style)
                .into_iter()
                .chain([
                    style.dimltex_handle,
                    style.dimltex1_handle,
                    style.dimltex2_handle,
                ])
                .filter(|handle| !handle.is_null())
                .count();
            let text_name = style.dimtxsty.clone();
            style.dimtxsty_handle = source
                .text_styles
                .get(&text_name)
                .map_or(style.dimtxsty_handle, |text| text.handle);
            crate::app::command_driver::clipboard::repoint_dim_style(doc, &mut style, &refs);
            let after = crate::app::command_driver::clipboard::dim_style_blocks(&style)
                .into_iter()
                .chain([
                    style.dimltex_handle,
                    style.dimltex1_handle,
                    style.dimltex2_handle,
                ])
                .filter(|handle| !handle.is_null())
                .count();
            *missing_refs |= after < before;
            match doc
                .text_styles
                .iter()
                .find(|text| !style.dimtxsty_handle.is_null() && text.handle == style.dimtxsty_handle)
                .or_else(|| doc.text_styles.get("Standard"))
            {
                Some(text) => {
                    style.dimtxsty = text.name.clone();
                    style.dimtxsty_handle = text.handle;
                }
                None => {
                    style.dimtxsty_handle = Handle::NULL;
                    *missing_refs = true;
                }
            }
            style.xref_reference = false;
            style.xref_resolved = false;
            style.xref_dependent = false;
            style.xref_handle = Handle::NULL;
            doc.dim_styles.add_or_replace(style);
        }
        _ => return false,
    }
    true
}

impl OpenCADStudio {
    pub(super) fn style_import_pick(&mut self, kind: StyleKind) -> Task<Message> {
        if !matches!(kind, StyleKind::Text | StyleKind::Dim) {
            return Task::none();
        }
        Task::perform(
            async move {
                rfd::AsyncFileDialog::new()
                    .set_title(crate::t!("Select Drawing to Import Styles From").as_ref())
                    .add_filter(
                        crate::t!("Drawings and templates").as_ref(),
                        &["dwg", "dxf", "dwt"],
                    )
                    .pick_file()
                    .await
                    .map(|handle| crate::sys::handle_path(&handle))
            },
            move |path| Message::StyleImportFile(kind, path),
        )
    }

    pub(super) fn style_import_file(
        &mut self,
        kind: StyleKind,
        path: Option<PathBuf>,
    ) -> Task<Message> {
        let Some(path) = path else {
            return Task::none();
        };
        super::file::background_task(
            crate::t!("Import Styles"),
            move || {
                let file = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                crate::io::load_file(&path).map(|doc| (file, doc))
            },
            move |result| Message::StyleImportLoaded(kind, result),
        )
    }

    pub(super) fn style_import_loaded(
        &mut self,
        kind: StyleKind,
        result: Result<(String, Document), String>,
    ) -> Task<Message> {
        let parent = match kind {
            StyleKind::Text => ModalKind::TextStyle,
            StyleKind::Dim => ModalKind::DimStyle,
            _ => return Task::none(),
        };
        if self.active_modal != Some(parent) {
            return Task::none();
        }
        match result {
            Ok((file, source)) => {
                self.style_import = Some(StyleImportDraft {
                    kind,
                    file,
                    selected: style_names(kind, &source),
                    source,
                    conflict: None,
                    remaining: VecDeque::new(),
                    approved: Vec::new(),
                });
                self.active_modal = Some(ModalKind::StyleImport);
                self.reset_modal_geometry();
            }
            Err(error) => self.command_line.push_error(&error),
        }
        Task::none()
    }

    pub(super) fn style_import_toggle(&mut self, name: &str) {
        if let Some(draft) = self.style_import.as_mut() {
            if draft.conflict.is_some() {
                return;
            }
            if let Some((_, selected)) = draft.selected.iter_mut().find(|(n, _)| n == name) {
                *selected = !*selected;
            }
        }
    }

    pub(super) fn style_import_apply(&mut self) {
        let Some(draft) = self.style_import.as_mut() else {
            return;
        };
        draft.remaining = draft
            .selected
            .iter()
            .filter_map(|(name, selected)| selected.then_some(name.clone()))
            .collect();
        draft.approved.clear();
        self.style_import_next();
    }

    /// Advance through the chosen styles. Every collision pauses for its own
    /// yes/no decision; no document changes occur until all choices are made.
    fn style_import_next(&mut self) {
        let Some(draft) = self.style_import.as_mut() else {
            return;
        };
        let doc = &self.tabs[self.active_tab].scene.document;
        if draft.next_conflict(doc) {
            return;
        }
        self.style_import_finish();
    }

    pub(super) fn style_import_confirm(&mut self, overwrite: bool) {
        let Some(draft) = self.style_import.as_mut() else {
            return;
        };
        let Some(name) = draft.conflict.take() else {
            return;
        };
        if overwrite {
            draft.approved.push((name, true));
        }
        self.style_import_next();
    }

    fn style_import_finish(&mut self) {
        let Some(draft) = self.style_import.take() else {
            return;
        };
        let kind = draft.kind;
        match kind {
            StyleKind::Text => self.stage_textstyle_bufs(),
            StyleKind::Dim => self.apply_dimstyle_bufs(self.active_tab),
            _ => return,
        }
        let doc = &mut self.tabs[self.active_tab].scene.document;
        let mut imported = Vec::new();
        let mut missing_refs = false;
        for (name, overwrite) in draft.approved {
            if import_one(
                doc,
                &draft.source,
                kind,
                &name,
                overwrite,
                &mut missing_refs,
            ) {
                imported.push(name);
            }
        }
        if missing_refs {
            self.command_line.push_error(crate::t!(
                "Some imported styles reference arrow blocks, linetypes, or text styles unavailable in this drawing; defaults will be used."
            ).as_ref());
        }
        if let Some(first) = imported.first() {
            match kind {
                StyleKind::Text => {
                    self.textstyle_selected = first.clone();
                    self.load_textstyle_bufs(self.active_tab);
                }
                StyleKind::Dim => {
                    self.dimstyle_selected = first.clone();
                    self.load_dimstyle_bufs(self.active_tab);
                }
                _ => {}
            }
        }
        self.command_line.push_output(
            crate::tf!(
                "Imported {} style(s). Apply to keep the changes.",
                imported.len()
            )
            .as_ref(),
        );
        self.active_modal = Some(match kind {
            StyleKind::Text => ModalKind::TextStyle,
            _ => ModalKind::DimStyle,
        });
        self.reset_modal_geometry();
    }

    pub(super) fn style_import_cancel(&mut self) {
        if let Some(draft) = self.style_import.take() {
            self.active_modal = Some(match draft.kind {
                StyleKind::Text => ModalKind::TextStyle,
                _ => ModalKind::DimStyle,
            });
            self.reset_modal_geometry();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::tables::{DimStyle, LineType, TextStyle};

    #[test]
    fn every_name_collision_gets_a_separate_decision() {
        let mut target = Document::new();
        target.text_styles.add_or_replace(TextStyle::new("First"));
        target.text_styles.add_or_replace(TextStyle::new("Second"));
        let mut draft = StyleImportDraft {
            kind: StyleKind::Text,
            file: String::new(),
            source: Document::new(),
            selected: Vec::new(),
            conflict: None,
            remaining: VecDeque::from([
                "First".to_string(), "Fresh".to_string(), "Second".to_string(),
            ]),
            approved: Vec::new(),
        };

        assert!(draft.next_conflict(&target));
        assert_eq!(draft.conflict.take().as_deref(), Some("First"));
        draft.approved.push(("First".to_string(), true));
        assert!(draft.next_conflict(&target));
        assert_eq!(draft.conflict.take().as_deref(), Some("Second"));
        // Declining the second conflict leaves its name out of the import.
        assert!(!draft.next_conflict(&target));
        assert_eq!(draft.approved, vec![
            ("First".to_string(), true), ("Fresh".to_string(), false),
        ]);
    }

    #[test]
    fn text_collision_requires_approval_and_keeps_target_handle() {
        let mut source = Document::new();
        let mut imported = TextStyle::new("Labels");
        imported.width_factor = 0.7;
        imported.handle = source.allocate_handle();
        source.text_styles.add_or_replace(imported);

        let mut target = Document::new();
        let mut existing = TextStyle::new("LABELS");
        existing.width_factor = 1.2;
        existing.handle = target.allocate_handle();
        let target_handle = existing.handle;
        target.text_styles.add_or_replace(existing);

        let mut missing = false;
        assert!(!import_one(
            &mut target,
            &source,
            StyleKind::Text,
            "Labels",
            false,
            &mut missing
        ));
        assert_eq!(target.text_styles.get("Labels").unwrap().width_factor, 1.2);
        assert!(import_one(
            &mut target,
            &source,
            StyleKind::Text,
            "Labels",
            true,
            &mut missing
        ));
        let result = target.text_styles.get("Labels").unwrap();
        assert_eq!(result.width_factor, 0.7);
        assert_eq!(result.handle, target_handle);
        assert_eq!(result.name, "LABELS");
    }

    #[test]
    fn dimension_import_maps_named_dependencies_not_source_handles() {
        let mut source = Document::new();
        let mut text = TextStyle::new("Labels");
        text.handle = source.allocate_handle();
        source.text_styles.add_or_replace(text.clone());
        let mut line = LineType::new("Dashed");
        line.handle = source.allocate_handle();
        source.line_types.add_or_replace(line.clone());
        let mut dimension = DimStyle::new("Detail");
        dimension.handle = source.allocate_handle();
        dimension.dimtxsty = text.name.clone();
        dimension.dimtxsty_handle = text.handle;
        dimension.dimltex_handle = line.handle;
        source.dim_styles.add_or_replace(dimension);

        let mut target = Document::new();
        let mut target_line = LineType::new("DASHED");
        target_line.handle = target.allocate_handle();
        let target_line_handle = target_line.handle;
        target.line_types.add_or_replace(target_line);

        let mut missing = false;
        assert!(import_one(
            &mut target,
            &source,
            StyleKind::Dim,
            "Detail",
            false,
            &mut missing
        ));
        let result = target.dim_styles.get("Detail").unwrap();
        assert_eq!(result.dimltex_handle, target_line_handle);
        assert_eq!(
            result.dimtxsty_handle,
            target.text_styles.get("Labels").unwrap().handle
        );
        assert!(!missing);
    }

    #[test]
    fn missing_dimension_reference_uses_default_and_reports_it() {
        let mut source = Document::new();
        let mut line = LineType::new("Missing");
        line.handle = source.allocate_handle();
        source.line_types.add_or_replace(line.clone());
        let mut dimension = DimStyle::new("Detail");
        dimension.dimltex_handle = line.handle;
        source.dim_styles.add_or_replace(dimension);

        let mut target = Document::new();
        let mut missing = false;
        assert!(import_one(
            &mut target,
            &source,
            StyleKind::Dim,
            "Detail",
            false,
            &mut missing
        ));
        assert!(target
            .dim_styles
            .get("Detail")
            .unwrap()
            .dimltex_handle
            .is_null());
        assert!(missing);
    }
}
