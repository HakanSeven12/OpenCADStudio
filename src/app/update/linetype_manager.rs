//! The Linetype Manager: a style manager (see `style_ops`) over the drawing's
//! linetype table. Validation and every change go through
//! `scene::linetype_service`.

use crate::app::{Message, ModalKind, OpenCADStudio, StyleKind};
use crate::scene::linetype_service as service;
use crate::ui::style::linetype::{
    ElementKind, LinetypeDraft, LinetypeMsg, PatternRow, PreviewElement,
};
use crate::ui::style::style_manager::StyleRow;
use document::tables::linetype::LineTypeComplexContent;
use document::tables::LineType;
use iced::Task;
use std::borrow::Cow;

/// A new linetype's pattern: visible at normal zoom.
pub(in crate::app) const NEW_PATTERN: [f64; 2] = [10.0, -5.0];

fn is_locked(linetype: &LineType) -> bool {
    service::is_builtin(&linetype.name) || linetype.xref_dependent || linetype.name.contains('|')
}

impl OpenCADStudio {
    fn selected_linetype(&self) -> Option<&LineType> {
        self.tabs[self.active_tab]
            .scene
            .document
            .line_types
            .get(&self.linetype_selected)
    }

    fn linetype_editable(&self) -> bool {
        self.selected_linetype()
            .is_some_and(|linetype| !is_locked(linetype) && !linetype.is_complex())
    }

    pub(in crate::app) fn linetype_is_locked(&self, name: &str) -> bool {
        self.tabs[self.active_tab]
            .scene
            .document
            .line_types
            .get(name)
            .is_some_and(is_locked)
    }

    /// Why Apply is off, or `None` when the editor can be written.
    pub(in crate::app) fn linetype_draft_error(&self) -> Option<Cow<'static, str>> {
        if self.linetype_draft.scales().is_none() {
            return Some(crate::t!("Scales must be positive numbers."));
        }
        if !self.linetype_editable() {
            return None;
        }
        let Some(pattern) = self.linetype_draft.pattern() else {
            return Some(crate::t!("Lengths must be positive numbers."));
        };
        let document = &self.tabs[self.active_tab].scene.document;
        service::plan_modify(
            document,
            &self.linetype_selected,
            Some(&self.linetype_draft.description),
            Some(&pattern),
        )
        .err()
        .map(|error| Cow::Owned(error.to_string()))
    }

    pub(in crate::app) fn load_linetype_bufs(&mut self, i: usize) {
        let scene = &self.tabs[i].scene;
        let document = &scene.document;
        let usage = service::LinetypeUsage::of(document);
        self.linetype_counts = document
            .line_types
            .iter()
            .map(|linetype| (linetype.name.to_ascii_lowercase(), usage.count(linetype)))
            .collect();
        let mut draft = LinetypeDraft {
            ltscale: document.header.linetype_scale.to_string(),
            celtscale: document.header.current_entity_linetype_scale.to_string(),
            msltscale: scene.model_linetype_scaling(),
            psltscale: document.header.paper_space_linetype_scaling,
            ..Default::default()
        };
        if let Some(linetype) = document.line_types.get(&self.linetype_selected) {
            draft.description = linetype.description.clone();
            draft.set_pattern(linetype.elements.iter().map(|element| element.length));
        }
        self.linetype_draft = draft;
    }

    /// Write what the editor holds into the staged document. A value that
    /// cannot be written stays in the editor (Apply says why).
    pub(in crate::app) fn flush_linetype_draft(&mut self) {
        let i = self.active_tab;
        if let Some((ltscale, celtscale)) = self.linetype_draft.scales() {
            let (msltscale, psltscale) =
                (self.linetype_draft.msltscale, self.linetype_draft.psltscale);
            let scene = &mut self.tabs[i].scene;
            scene.document.header.linetype_scale = ltscale;
            scene.document.header.current_entity_linetype_scale = celtscale;
            scene.document.header.paper_space_linetype_scaling = psltscale;
            if scene.model_linetype_scaling() != msltscale {
                scene.set_model_linetype_scaling(msltscale);
            }
        }
        if !self.linetype_editable() {
            return;
        }
        let Some(pattern) = self.linetype_draft.pattern() else {
            return;
        };
        let document = &self.tabs[i].scene.document;
        let Ok(updated) = service::plan_modify(
            document,
            &self.linetype_selected,
            Some(&self.linetype_draft.description),
            Some(&pattern),
        ) else {
            return;
        };
        if document.line_types.get(&updated.name) != Some(&updated) {
            service::apply_modify(&mut self.tabs[i].scene.document, updated);
        }
    }

    /// The list: each linetype's use count and lock. "Styles in use" keeps
    /// the used ones, the current and the selected.
    pub(in crate::app) fn linetype_list(&self) -> (Vec<String>, Vec<StyleRow>) {
        let document = &self.tabs[self.active_tab].scene.document;
        let current = &document.header.current_linetype_name;
        self.style_names(StyleKind::Linetype)
            .into_iter()
            .filter_map(|name| {
                let linetype = document.line_types.get(&name)?;
                let count = self
                    .linetype_counts
                    .get(&name.to_ascii_lowercase())
                    .copied()
                    .unwrap_or(0);
                Some((
                    name,
                    StyleRow {
                        count,
                        locked: is_locked(linetype),
                        annotative: false,
                    },
                ))
            })
            .filter(|(name, row)| {
                !self.linetype_in_use_only
                    || row.count > 0
                    || name.eq_ignore_ascii_case(current)
                    || name.eq_ignore_ascii_case(&self.linetype_selected)
            })
            .unzip()
    }

    /// What the preview draws: the editor's pattern, or the stored one when
    /// the editor cannot be read or holds complex elements.
    pub(in crate::app) fn linetype_preview(&self) -> Vec<PreviewElement> {
        if self.linetype_editable() {
            if let Some(pattern) = self.linetype_draft.pattern() {
                return pattern
                    .into_iter()
                    .map(|length| PreviewElement {
                        length,
                        label: None,
                        shape: false,
                    })
                    .collect();
            }
        }
        self.selected_linetype()
            .map(|linetype| {
                linetype
                    .elements
                    .iter()
                    .map(|element| PreviewElement {
                        length: element.length,
                        label: element
                            .complex
                            .as_ref()
                            .and_then(|data| match &data.content {
                                LineTypeComplexContent::Text { text } => {
                                    (!text.trim().is_empty()).then(|| text.clone())
                                }
                                LineTypeComplexContent::Shape { .. } => None,
                            }),
                        shape: element.complex.as_ref().is_some_and(|data| {
                            matches!(data.content, LineTypeComplexContent::Shape { .. })
                                && !data.style_handle.is_null()
                        }),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(in crate::app) fn linetype_is_complex(&self) -> bool {
        self.selected_linetype().is_some_and(LineType::is_complex)
    }

    fn on_linetype_open(&mut self) -> Task<Message> {
        let kind = ModalKind::LinetypeManager;
        // Already open, perhaps behind the main window: bring it up and keep
        // what it holds. Beginning again would lose the unapplied changes and
        // what Close restores.
        if self.dialog_open(kind) {
            let raised = self.raise_tool(kind);
            return self.raise_tool_window(kind).unwrap_or(raised);
        }
        let i = self.active_tab;
        let names = self.style_names(StyleKind::Linetype);
        let current = self.tabs[i]
            .scene
            .document
            .header
            .current_linetype_name
            .clone();
        self.linetype_selected = names
            .iter()
            .find(|name| name.eq_ignore_ascii_case(&current))
            .or_else(|| {
                names
                    .iter()
                    .find(|name| name.eq_ignore_ascii_case("ByLayer"))
            })
            .or(names.first())
            .cloned()
            .unwrap_or_default();
        self.linetype_tab = 0;
        self.linetype_keys_active = true;
        self.linetype_list_view = None;
        self.linetype_library_status.clear();
        self.active_modal = Some(ModalKind::LinetypeManager);
        self.load_linetype_bufs(i);
        self.style_stage_begin_linetypes();
        Task::none()
    }

    fn linetype_set_current(&mut self) {
        let i = self.active_tab;
        let name = self.linetype_selected.clone();
        let document = &mut self.tabs[i].scene.document;
        let handle = if service::is_builtin(&name) && !name.eq_ignore_ascii_case("Continuous") {
            document::types::Handle::NULL
        } else {
            document
                .line_types
                .get(&name)
                .map_or(document::types::Handle::NULL, |linetype| linetype.handle)
        };
        document.header.current_linetype_name = name.clone();
        document.header.current_linetype_handle = handle;
        self.command_line
            .push_output(crate::tf!("Current linetype set to {name}.").as_ref());
    }

    fn remember_library_status(&mut self) {
        self.linetype_library_status = self
            .command_line
            .history
            .last()
            .map(|entry| entry.text.clone())
            .unwrap_or_default();
    }

    fn edit_linetype_row(&mut self, index: usize, edit: impl FnOnce(&mut PatternRow)) {
        if self.linetype_editable() {
            if let Some(row) = self.linetype_draft.rows.get_mut(index) {
                edit(row);
            }
        }
    }

    /// Up or Down in the list: the row above or below, kept in view. It
    /// stops at the ends and shows what the list shows, filter included.
    pub(in crate::app) fn linetype_list_step(&mut self, down: bool) -> Task<Message> {
        if !self.linetype_keys_active
            || self.style_rename.is_some()
            || self.style_editor_blocked(StyleKind::Linetype)
        {
            return Task::none();
        }
        let (names, _) = self.linetype_list();
        let Some(index) = names
            .iter()
            .position(|name| *name == self.linetype_selected)
        else {
            return Task::none();
        };
        let target = if down {
            index + 1
        } else if index > 0 {
            index - 1
        } else {
            return Task::none();
        };
        let Some(name) = names.get(target).cloned() else {
            return Task::none();
        };
        let selected = self.on_linetype_message(LinetypeMsg::Select(name));
        let Some(view) = self.linetype_list_view.as_mut() else {
            return selected;
        };
        let Some(offset) = view.offset_for_row(target, names.len()) else {
            return selected;
        };
        // Known at once, for the next key before the list reports back.
        view.offset = offset;
        Task::batch([
            selected,
            iced::widget::operation::scroll_to(
                crate::ui::style::style_manager::linetype_list_id(),
                iced::widget::scrollable::AbsoluteOffset { x: 0.0, y: offset },
            ),
        ])
    }

    pub(in crate::app) fn on_linetype_message(&mut self, message: LinetypeMsg) -> Task<Message> {
        let i = self.active_tab;
        match message {
            LinetypeMsg::Open => return self.on_linetype_open(),
            LinetypeMsg::Select(name) => {
                self.linetype_keys_active = true;
                if !self.style_editor_blocked(StyleKind::Linetype) {
                    self.flush_linetype_draft();
                    self.linetype_selected = name;
                    self.load_linetype_bufs(i);
                }
            }
            LinetypeMsg::Tab(tab) => self.linetype_tab = tab,
            LinetypeMsg::ListScrolled(view) => self.linetype_list_view = Some(view),
            LinetypeMsg::InUseFilter(on) => self.linetype_in_use_only = on,
            LinetypeMsg::New => return self.style_new(StyleKind::Linetype),
            LinetypeMsg::Copy => return self.style_copy(StyleKind::Linetype),
            LinetypeMsg::Delete => self.style_delete(StyleKind::Linetype),
            LinetypeMsg::Import => {
                return Task::perform(crate::io::pick_linetype_file(), Message::LinetypeLoaded);
            }
            LinetypeMsg::SetCurrent => {
                if !self.style_editor_blocked(StyleKind::Linetype) {
                    self.flush_linetype_draft();
                    self.linetype_set_current();
                }
            }
            LinetypeMsg::Apply => {
                if !self.style_editor_blocked(StyleKind::Linetype) {
                    self.flush_linetype_draft();
                    self.style_stage_commit();
                    self.load_linetype_bufs(i);
                }
            }
            LinetypeMsg::Description(value) => {
                self.linetype_keys_active = false;
                if self.linetype_editable() {
                    self.linetype_draft.description = value;
                }
            }
            LinetypeMsg::ElementKind(index, kind) => self.edit_linetype_row(index, |row| {
                row.kind = kind;
                if kind == ElementKind::Dot {
                    row.length.clear();
                } else if row.length.is_empty() {
                    row.length = "1".to_owned();
                }
            }),
            LinetypeMsg::ElementLength(index, value) => {
                self.linetype_keys_active = false;
                self.edit_linetype_row(index, |row| row.length = value)
            }
            LinetypeMsg::ElementAdd => {
                if self.linetype_editable() && self.linetype_draft.rows.len() < 12 {
                    // Dashes and gaps usually alternate.
                    let kind = match self.linetype_draft.rows.last() {
                        Some(row) if row.kind != ElementKind::Gap => ElementKind::Gap,
                        _ => ElementKind::Dash,
                    };
                    self.linetype_draft.rows.push(PatternRow {
                        kind,
                        length: "5".to_owned(),
                    });
                }
            }
            LinetypeMsg::ElementDelete(index) => {
                if self.linetype_editable() && index < self.linetype_draft.rows.len() {
                    self.linetype_draft.rows.remove(index);
                }
            }
            LinetypeMsg::ElementMove(index, up) if self.linetype_editable() => {
                let rows = &mut self.linetype_draft.rows;
                let other = if up {
                    index.checked_sub(1)
                } else {
                    Some(index + 1).filter(|next| *next < rows.len())
                };
                if let (true, Some(other)) = (index < rows.len(), other) {
                    rows.swap(index, other);
                }
            }
            LinetypeMsg::ElementMove(..) => {}
            LinetypeMsg::Scale(field, value) => {
                self.linetype_keys_active = false;
                match field {
                    "ltscale" => self.linetype_draft.ltscale = value,
                    "celtscale" => self.linetype_draft.celtscale = value,
                    _ => {}
                }
            }
            LinetypeMsg::ScaleToggle(field, on) => match field {
                "msltscale" => self.linetype_draft.msltscale = on,
                "psltscale" => self.linetype_draft.psltscale = on,
                _ => {}
            },
            LinetypeMsg::Export { all } => {
                let suggested = if all {
                    "linetypes.lin".to_owned()
                } else {
                    format!("{}.lin", self.linetype_selected)
                };
                return Task::perform(
                    crate::io::pick_linetype_export_path(suggested),
                    move |path| Message::Linetype(LinetypeMsg::ExportPicked { all, path }),
                );
            }
            LinetypeMsg::ExportPicked { all, path } => {
                if let Some(path) = path {
                    self.flush_linetype_draft();
                    let names = if all {
                        "*".to_owned()
                    } else {
                        self.linetype_selected.clone()
                    };
                    self.dash_linetype_export(i, &names, &path, true);
                    self.remember_library_status();
                }
            }
            LinetypeMsg::SaveToLibrary => {
                self.flush_linetype_draft();
                let name = self.linetype_selected.clone();
                self.dash_linetype_save(i, &name);
                self.remember_library_status();
            }
            LinetypeMsg::ReloadLibrary => {
                self.dash_linetype_reload();
                self.remember_library_status();
            }
        }
        Task::none()
    }
}
