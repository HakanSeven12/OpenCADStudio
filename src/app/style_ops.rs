//! Shared CRUD layer for every style manager (text / dimension / table /
//! multileader / multiline).
//!
//! The five managers all expose the same list operations — New, Copy, Delete,
//! Rename, Set-Current — over a named collection of styles. Only the *property
//! editor* and the *storage backend* differ, so those are the only parts kept
//! per-manager:
//!
//! * **Table-backed** (text, dim): live in `Table<T>`, keyed by upper-cased
//!   name. Renaming must re-key the entry and rewrite name-based entity
//!   references (TEXT/MTEXT `style`, DIMENSION `style_name`).
//! * **Object-backed** (table, multileader, multiline): live in
//!   `document.objects`, keyed by handle. Renaming only mutates the `name`
//!   field; entities reference these by handle, so nothing else moves.
//!
//! Centralising the flow here is what fixes the bug class that kept recurring
//! when each manager was hand-copied: a dead New, a missing ribbon refresh, a
//! style added without a handle (dropped on DWG save, issue #67).

use super::OpenCADStudio;
use acadrust::objects::{
    Dictionary, MLineStyle, MultiLeaderStyle, ObjectType, TableStyle,
};
use acadrust::tables::{DimStyle, TextStyle};
use acadrust::types::Handle;

const MLEADERSTYLE_DICT_NAME: &str = "ACAD_MLEADERSTYLE";

fn mleaderstyle_dict_handle(doc: &acadrust::CadDocument) -> Option<Handle> {
    let root_h = doc.header.named_objects_dict_handle;

    let root = match doc.objects.get(&root_h) {
        Some(ObjectType::Dictionary(root)) => root,
        _ => return None,
    };

    root.entries
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(MLEADERSTYLE_DICT_NAME))
        .map(|(_, handle)| *handle)
        .filter(|handle| {
            matches!(
                doc.objects.get(handle),
                Some(ObjectType::Dictionary(_))
            )
        })
}

fn import_mleaderstyle_names_from_dictionary(doc: &mut acadrust::CadDocument) {
    let Some(dict_h) = mleaderstyle_dict_handle(doc) else {
        return;
    };

    let entries = match doc.objects.get(&dict_h) {
        Some(ObjectType::Dictionary(dict)) => dict.entries.clone(),
        _ => return,
    };

    for (name, handle) in entries {
        if let Some(ObjectType::MultiLeaderStyle(style)) =
            doc.objects.get_mut(&handle)
        {
            style.name = name;
            style.owner_handle = dict_h;
        }
    }
}

fn sync_mleaderstyle_dictionary(doc: &mut acadrust::CadDocument) {
    let root_h = crate::scene::annotative::root_named_dict_handle(doc);

    let existing = match doc.objects.get(&root_h) {
        Some(ObjectType::Dictionary(root)) => root
            .entries
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(MLEADERSTYLE_DICT_NAME))
            .map(|(_, handle)| *handle),
        _ => None,
    };

    let dict_h = match existing.filter(|handle| {
        matches!(
            doc.objects.get(handle),
            Some(ObjectType::Dictionary(_))
        )
    }) {
        Some(handle) => handle,
        None => {
            let handle = doc.allocate_handle();

            let mut dict = Dictionary::new();
            dict.handle = handle;
            dict.owner = root_h;

            doc.objects
                .insert(handle, ObjectType::Dictionary(dict));

            handle
        }
    };

    if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_h) {
        root.entries
            .retain(|(name, _)| !name.eq_ignore_ascii_case(MLEADERSTYLE_DICT_NAME));

        root.add_entry(MLEADERSTYLE_DICT_NAME, dict_h);
    }

    let mut entries: Vec<(String, Handle)> = doc
        .objects
        .iter()
        .filter_map(|(&handle, object)| match object {
            ObjectType::MultiLeaderStyle(style) => {
                Some((style.name.clone(), handle))
            }
            _ => None,
        })
        .collect();

    entries.sort_by(|a, b| {
        a.0.to_lowercase().cmp(&b.0.to_lowercase())
    });

    for (_, handle) in &entries {
        if let Some(ObjectType::MultiLeaderStyle(style)) =
            doc.objects.get_mut(handle)
        {
            style.owner_handle = dict_h;
        }
    }

    if let Some(ObjectType::Dictionary(dict)) = doc.objects.get_mut(&dict_h) {
        dict.owner = root_h;
        dict.entries = entries;

        // AutoCAD stores MLEADERSTYLE entries as soft-owner references (350),
        // not hard-owner references (360).
        dict.hard_owner = false;
        dict.hard_owner_entries.clear();
    }
}

/// Guarantee the built-in "Standard" style of every kind exists in `doc` —
/// a foreign or damaged file saved without them leaves the style dropdowns
/// broken with no way to recover, and new text/dimensions have nothing to
/// reference (#366). Missing entries are re-seeded with the app defaults.
/// Called on every file open; a no-op for healthy documents.
pub(crate) fn ensure_standard_styles(doc: &mut acadrust::CadDocument) {
    import_mleaderstyle_names_from_dictionary(doc);
    if !doc
        .text_styles
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case("Standard"))
    {
        let mut s = TextStyle::new("Standard");
        s.handle = doc.allocate_handle();
        let _ = doc.text_styles.add(s);
    }
    if !doc
        .dim_styles
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case("Standard"))
    {
        let mut s = DimStyle::new("Standard");
        s.handle = doc.allocate_handle();
        let _ = doc.dim_styles.add(s);
    }
    let has = |doc: &acadrust::CadDocument, pred: fn(&ObjectType) -> Option<&str>| {
        doc.objects
            .values()
            .filter_map(pred)
            .any(|n| n.eq_ignore_ascii_case("Standard"))
    };
    if !has(doc, |o| match o {
        ObjectType::TableStyle(s) => Some(&s.name),
        _ => None,
    }) {
        let mut s = TableStyle::standard();
        s.handle = doc.allocate_handle();
        doc.objects.insert(s.handle, ObjectType::TableStyle(s));
    }
    if !has(doc, |o| match o {
        ObjectType::MultiLeaderStyle(s) => Some(&s.name),
        _ => None,
    }) {
        let mut s = MultiLeaderStyle::standard();
        s.handle = doc.allocate_handle();
        doc.objects.insert(s.handle, ObjectType::MultiLeaderStyle(s));
    }
    sync_mleaderstyle_dictionary(doc);

    if !has(doc, |o| match o {
        ObjectType::MLineStyle(s) => Some(&s.name),
        _ => None,
    }) {
        let mut s = MLineStyle::standard();
        s.handle = doc.allocate_handle();
        doc.objects.insert(s.handle, ObjectType::MLineStyle(s));
    }
}

/// Which style manager an operation targets. Carried by the shared rename
/// messages so one handler can dispatch to the right storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleKind {
    Text,
    Dim,
    Table,
    MLeader,
    MLine,
}

impl StyleKind {
    /// True when this style feeds the ribbon's quick-set dropdown.
    fn in_ribbon(self) -> bool {
        !matches!(self, StyleKind::MLine)
    }
}

impl OpenCADStudio {
    // ── Queries ────────────────────────────────────────────────────────────

    /// All style names for `kind`, in display order (object-backed styles are
    /// sorted by name so the `HashMap` backing them renders stably).
    pub(super) fn style_names(&self, kind: StyleKind) -> Vec<String> {
        let doc = &self.tabs[self.active_tab].scene.document;
        let from_objects = |pick: fn(&ObjectType) -> Option<&str>| -> Vec<String> {
            let mut v: Vec<String> = doc
                .objects
                .values()
                .filter_map(pick)
                .map(str::to_string)
                .collect();
            v.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
            v
        };
        match kind {
            StyleKind::Text => doc.text_styles.iter().map(|s| s.name.clone()).collect(),
            StyleKind::Dim => doc.dim_styles.iter().map(|s| s.name.clone()).collect(),
            StyleKind::Table => from_objects(|o| match o {
                ObjectType::TableStyle(s) => Some(s.name.as_str()),
                _ => None,
            }),
            StyleKind::MLeader => from_objects(|o| match o {
                ObjectType::MultiLeaderStyle(s) => Some(s.name.as_str()),
                _ => None,
            }),
            StyleKind::MLine => from_objects(|o| match o {
                ObjectType::MLineStyle(s) => Some(s.name.as_str()),
                _ => None,
            }),
        }
    }

    pub(super) fn style_selected(&self, kind: StyleKind) -> String {
        match kind {
            StyleKind::Text => self.textstyle_selected.clone(),
            StyleKind::Dim => self.dimstyle_selected.clone(),
            StyleKind::Table => self.tablestyle_selected.clone(),
            StyleKind::MLeader => self.mleaderstyle_selected.clone(),
            StyleKind::MLine => self.mlstyle_selected.clone(),
        }
    }

    fn set_style_selected(&mut self, kind: StyleKind, name: String) {
        match kind {
            StyleKind::Text => self.textstyle_selected = name,
            StyleKind::Dim => self.dimstyle_selected = name,
            StyleKind::Table => self.tablestyle_selected = name,
            StyleKind::MLeader => self.mleaderstyle_selected = name,
            StyleKind::MLine => self.mlstyle_selected = name,
        }
    }

    fn style_exists(&self, kind: StyleKind, name: &str) -> bool {
        self.style_names(kind)
            .iter()
            .any(|n| n.eq_ignore_ascii_case(name))
    }

    /// First free `Style{n}` name for a fresh style.
    fn unique_new_name(&self, kind: StyleKind) -> String {
        (1u32..)
            .map(|n| format!("Style{n}"))
            .find(|c| !self.style_exists(kind, c))
            .unwrap()
    }

    /// First free `{base} ({n})` name for a copy / disambiguated entry.
    fn unique_suffixed_name(&self, kind: StyleKind, base: &str) -> String {
        (1u32..)
            .map(|n| format!("{base} ({n})"))
            .find(|c| !self.style_exists(kind, c))
            .unwrap()
    }

    // ── Per-manager glue (the only kind-specific list code) ────────────────

    /// Reload the property-editor buffers for the kinds that have them.
    fn load_style_bufs(&mut self, kind: StyleKind) {
        let i = self.active_tab;
        match kind {
            StyleKind::Text => self.load_textstyle_bufs(i),
            StyleKind::Dim => self.load_dimstyle_bufs(i),
            StyleKind::MLeader => self.load_mleaderstyle_bufs(i),
            StyleKind::Table | StyleKind::MLine => {}
        }
    }

    /// Refresh anything that mirrors the style list / current style after a
    /// mutation (ribbon dropdowns, geometry that depends on the style).
    fn after_style_change(&mut self, kind: StyleKind) {
        if kind.in_ribbon() {
            self.sync_ribbon_styles();
        }
    }

    fn insert_default_style(&mut self, kind: StyleKind, name: &str, handle: Handle) {
        let doc = &mut self.tabs[self.active_tab].scene.document;
        match kind {
            StyleKind::Text => {
                let mut s = TextStyle::new(name);
                s.handle = handle;
                let _ = doc.text_styles.add(s);
            }
            StyleKind::Dim => {
                let mut s = DimStyle::new(name);
                s.handle = handle;
                let _ = doc.dim_styles.add(s);
            }
            StyleKind::Table => {
                let mut s = TableStyle::standard();
                s.name = name.to_string();
                s.handle = handle;
                doc.objects.insert(handle, ObjectType::TableStyle(s));
            }
            StyleKind::MLeader => {
                let mut s = MultiLeaderStyle::new(name);
                s.handle = handle;
                doc.objects.insert(handle, ObjectType::MultiLeaderStyle(s));
            }
            StyleKind::MLine => {
                let mut s = MLineStyle::standard();
                s.name = name.to_string();
                s.handle = handle;
                doc.objects.insert(handle, ObjectType::MLineStyle(s));
            }
        }
    }

    /// Clone the style named `src` under `name` with a fresh `handle`.
    /// Returns false if `src` no longer exists.
    fn clone_style_as(&mut self, kind: StyleKind, src: &str, name: &str, handle: Handle) -> bool {
        let doc = &mut self.tabs[self.active_tab].scene.document;
        match kind {
            StyleKind::Text => {
                if let Some(mut s) = doc.text_styles.get(src).cloned() {
                    s.name = name.to_string();
                    s.handle = handle;
                    let _ = doc.text_styles.add(s);
                    return true;
                }
            }
            StyleKind::Dim => {
                if let Some(mut s) = doc.dim_styles.get(src).cloned() {
                    s.name = name.to_string();
                    s.handle = handle;
                    let _ = doc.dim_styles.add(s);
                    return true;
                }
            }
            StyleKind::Table => {
                if let Some(mut s) = find_object_style(doc, src, |o| match o {
                    ObjectType::TableStyle(s) => Some((s.name.as_str(), s.clone())),
                    _ => None,
                }) {
                    s.name = name.to_string();
                    s.handle = handle;
                    doc.objects.insert(handle, ObjectType::TableStyle(s));
                    return true;
                }
            }
            StyleKind::MLeader => {
                if let Some(mut s) = find_object_style(doc, src, |o| match o {
                    ObjectType::MultiLeaderStyle(s) => Some((s.name.as_str(), s.clone())),
                    _ => None,
                }) {
                    s.name = name.to_string();
                    s.handle = handle;
                    doc.objects.insert(handle, ObjectType::MultiLeaderStyle(s));
                    return true;
                }
            }
            StyleKind::MLine => {
                if let Some(mut s) = find_object_style(doc, src, |o| match o {
                    ObjectType::MLineStyle(s) => Some((s.name.as_str(), s.clone())),
                    _ => None,
                }) {
                    s.name = name.to_string();
                    s.handle = handle;
                    doc.objects.insert(handle, ObjectType::MLineStyle(s));
                    return true;
                }
            }
        }
        false
    }

    fn remove_style_storage(&mut self, kind: StyleKind, name: &str) -> bool {
        let doc = &mut self.tabs[self.active_tab].scene.document;
        match kind {
            StyleKind::Text => doc.text_styles.remove(name).is_some(),
            StyleKind::Dim => doc.dim_styles.remove(name).is_some(),
            StyleKind::Table | StyleKind::MLeader | StyleKind::MLine => {
                let kind2 = kind;
                if let Some(h) = object_handle(doc, name, kind2) {
                    doc.objects.remove(&h).is_some()
                } else {
                    false
                }
            }
        }
    }

    pub(super) fn style_in_use(&self, kind: StyleKind, name: &str) -> bool {
        use acadrust::entities::EntityType;

        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        match kind {
            StyleKind::Text => {
                if doc.header.current_text_style_name.eq_ignore_ascii_case(name) {
                    return true;
                }
                let style_handle = doc.text_styles.get(name).map(|style| style.handle);
                let referenced_by_entity = doc.entities().any(|entity| match entity {
                    EntityType::Text(text) => text.style.eq_ignore_ascii_case(name),
                    EntityType::MText(text) => text.style.eq_ignore_ascii_case(name),
                    EntityType::AttributeEntity(attribute) => {
                        attribute.text_style.eq_ignore_ascii_case(name)
                    }
                    EntityType::AttributeDefinition(attribute) => {
                        attribute.text_style.eq_ignore_ascii_case(name)
                    }
                    EntityType::Insert(insert) => insert.attributes.iter().any(|attribute| {
                        attribute.text_style.eq_ignore_ascii_case(name)
                    }),
                    EntityType::MultiLeader(leader) => [
                        leader.text_style_handle,
                        leader.context.text_style_handle,
                    ]
                    .into_iter()
                    .flatten()
                    .any(|handle| Some(handle) == style_handle),
                    EntityType::Table(table) => table.rows.iter().any(|row| {
                        row.style
                            .as_ref()
                            .and_then(|style| style.text_style_handle)
                            .is_some_and(|handle| Some(handle) == style_handle)
                            || row.cells.iter().any(|cell| {
                                cell.style
                                    .as_ref()
                                    .and_then(|style| style.text_style_handle)
                                    .is_some_and(|handle| Some(handle) == style_handle)
                                    || cell.contents.iter().any(|content| {
                                        content
                                            .text_style_handle
                                            .is_some_and(|handle| Some(handle) == style_handle)
                                    })
                            })
                    }),
                    _ => false,
                });
                referenced_by_entity
                    || doc
                        .dim_styles
                        .iter()
                        .any(|style| style.dimtxsty.eq_ignore_ascii_case(name))
                    || doc.objects.values().any(|object| match object {
                        ObjectType::TableStyle(style) => [
                            &style.data_row_style,
                            &style.header_row_style,
                            &style.title_row_style,
                        ]
                        .into_iter()
                        .any(|row| row.text_style_name.eq_ignore_ascii_case(name)),
                        ObjectType::MultiLeaderStyle(style) => {
                            style.text_style_handle == style_handle
                        }
                        _ => false,
                    })
            }
            StyleKind::Dim => {
                doc.header.current_dimstyle_name.eq_ignore_ascii_case(name)
                    || doc.entities().any(|entity| match entity {
                        EntityType::Dimension(dimension) => {
                            dimension.base().style_name.eq_ignore_ascii_case(name)
                        }
                        EntityType::Leader(leader) => {
                            leader.dimension_style.eq_ignore_ascii_case(name)
                        }
                        EntityType::Tolerance(tolerance) => tolerance
                            .dimension_style_name
                            .eq_ignore_ascii_case(name),
                        _ => false,
                    })
            }
            StyleKind::Table | StyleKind::MLeader | StyleKind::MLine => {
                let Some(handle) = object_handle(doc, name, kind) else {
                    return false;
                };
                let is_current = match kind {
                    StyleKind::Table => {
                        doc.header.current_table_style_name.eq_ignore_ascii_case(name)
                    }
                    StyleKind::MLeader => {
                        doc.header.current_mleader_style_name.eq_ignore_ascii_case(name)
                            || self.tabs[i]
                                .active_mleader_style
                                .eq_ignore_ascii_case(name)
                    }
                    StyleKind::MLine => doc.header.multiline_style.eq_ignore_ascii_case(name),
                    StyleKind::Text | StyleKind::Dim => false,
                };
                is_current
                    || doc.entities().any(|entity| match (kind, entity) {
                        (StyleKind::Table, EntityType::Table(table)) => {
                            table.table_style_handle == Some(handle)
                        }
                        (StyleKind::MLeader, EntityType::MultiLeader(leader)) => {
                            leader.style_handle == Some(handle)
                        }
                        (StyleKind::MLine, EntityType::MLine(line)) => {
                            line.style_handle == Some(handle)
                                || line.style_name.eq_ignore_ascii_case(name)
                        }
                        _ => false,
                    })
            }
        }
    }

    /// Rename `old`→`new` in the backing store, re-keying table entries and
    /// rewriting name-based references + current-style pointers.
    fn rename_style_storage(&mut self, kind: StyleKind, old: &str, new: &str) {
        let i = self.active_tab;
        match kind {
            StyleKind::Text => {
                let doc = &mut self.tabs[i].scene.document;
                if let Some(mut s) = doc.text_styles.get(old).cloned() {
                    s.name = new.to_string();
                    if !s.handle.is_valid() {
                        s.handle = doc.allocate_handle();
                    }
                    let _ = doc.text_styles.add(s);
                }
                doc.text_styles.remove(old);
                if doc.header.current_text_style_name.eq_ignore_ascii_case(old) {
                    doc.header.current_text_style_name = new.to_string();
                }
                for e in doc.entities_mut() {
                    match e {
                        acadrust::entities::EntityType::Text(t)
                            if t.style.eq_ignore_ascii_case(old) =>
                        {
                            t.style = new.to_string();
                        }
                        acadrust::entities::EntityType::MText(t)
                            if t.style.eq_ignore_ascii_case(old) =>
                        {
                            t.style = new.to_string();
                        }
                        acadrust::entities::EntityType::AttributeEntity(a)
                            if a.text_style.eq_ignore_ascii_case(old) =>
                        {
                            a.text_style = new.to_string();
                        }
                        acadrust::entities::EntityType::AttributeDefinition(a)
                            if a.text_style.eq_ignore_ascii_case(old) =>
                        {
                            a.text_style = new.to_string();
                        }
                        acadrust::entities::EntityType::Insert(insert) => {
                            for attribute in &mut insert.attributes {
                                if attribute.text_style.eq_ignore_ascii_case(old) {
                                    attribute.text_style = new.to_string();
                                }
                            }
                        }
                        _ => {}
                    }
                }
                for object in doc.objects.values_mut() {
                    let ObjectType::TableStyle(style) = object else {
                        continue;
                    };
                    for row in [
                        &mut style.data_row_style,
                        &mut style.header_row_style,
                        &mut style.title_row_style,
                    ] {
                        if row.text_style_name.eq_ignore_ascii_case(old) {
                            row.text_style_name = new.to_string();
                        }
                    }
                }
            }
            StyleKind::Dim => {
                let doc = &mut self.tabs[i].scene.document;
                if let Some(mut s) = doc.dim_styles.get(old).cloned() {
                    s.name = new.to_string();
                    if !s.handle.is_valid() {
                        s.handle = doc.allocate_handle();
                    }
                    let _ = doc.dim_styles.add(s);
                }
                doc.dim_styles.remove(old);
                if doc.header.current_dimstyle_name.eq_ignore_ascii_case(old) {
                    doc.header.current_dimstyle_name = new.to_string();
                }
                for e in doc.entities_mut() {
                    match e {
                        acadrust::entities::EntityType::Dimension(d)
                            if d.base().style_name.eq_ignore_ascii_case(old) =>
                        {
                            d.base_mut().style_name = new.to_string();
                        }
                        acadrust::entities::EntityType::Leader(l)
                            if l.dimension_style.eq_ignore_ascii_case(old) =>
                        {
                            l.dimension_style = new.to_string();
                        }
                        acadrust::entities::EntityType::Tolerance(t)
                            if t.dimension_style_name.eq_ignore_ascii_case(old) =>
                        {
                            t.dimension_style_name = new.to_string();
                        }
                        _ => {}
                    }
                }
            }
            StyleKind::Table => {
                let doc = &mut self.tabs[i].scene.document;
                if let Some(h) = object_handle(doc, old, kind) {
                    if let Some(ObjectType::TableStyle(s)) = doc.objects.get_mut(&h) {
                        s.name = new.to_string();
                    }
                }
                if self.ribbon.active_table_style.eq_ignore_ascii_case(old) {
                    self.ribbon.active_table_style = new.to_string();
                }
                if doc.header.current_table_style_name.eq_ignore_ascii_case(old) {
                    doc.header.current_table_style_name = new.to_string();
                }
            }
            StyleKind::MLeader => {
                {
                    let doc = &mut self.tabs[i].scene.document;
                    if let Some(h) = object_handle(doc, old, kind) {
                        if let Some(ObjectType::MultiLeaderStyle(s)) = doc.objects.get_mut(&h) {
                            s.name = new.to_string();
                        }
                    }
                    if doc
                        .header
                        .current_mleader_style_name
                        .eq_ignore_ascii_case(old)
                    {
                        doc.header.current_mleader_style_name = new.to_string();
                    }
                }
                if self.tabs[i].active_mleader_style.eq_ignore_ascii_case(old) {
                    self.tabs[i].active_mleader_style = new.to_string();
                }
                if self.ribbon.active_mleader_style.eq_ignore_ascii_case(old) {
                    self.ribbon.active_mleader_style = new.to_string();
                }
            }
            StyleKind::MLine => {
                let doc = &mut self.tabs[i].scene.document;
                if let Some(h) = object_handle(doc, old, kind) {
                    if let Some(ObjectType::MLineStyle(s)) = doc.objects.get_mut(&h) {
                        s.name = new.to_string();
                    }
                }
                if doc.header.multiline_style.eq_ignore_ascii_case(old) {
                    doc.header.multiline_style = new.to_string();
                }
                for entity in doc.entities_mut() {
                    if let acadrust::entities::EntityType::MLine(line) = entity {
                        if line.style_name.eq_ignore_ascii_case(old) {
                            line.style_name = new.to_string();
                        }
                    }
                }
            }
        }
    }

    // ── Public operations (called by the message handlers) ─────────────────

    // Note: the structural ops below mutate the document live (so the dialog
    // shows a preview) but do NOT mark the tab dirty, push undo, or rebuild the
    // drawing. Those side effects are deferred to `style_stage_commit` (Apply);
    // closing the window without Apply calls `style_stage_discard`, which
    // restores the snapshot taken when the manager opened.

    pub(super) fn style_new(&mut self, kind: StyleKind) {
        let i = self.active_tab;
        let name = self.unique_new_name(kind);
        let h = self.tabs[i].scene.document.allocate_handle();
        self.insert_default_style(kind, &name, h);
        self.set_style_selected(kind, name.clone());
        self.load_style_bufs(kind);
        self.after_style_change(kind);
        self.command_line
            .push_output(crate::tf!("Style '{name}' created.").as_ref());
    }

    pub(super) fn style_copy(&mut self, kind: StyleKind) {
        let i = self.active_tab;
        let src = self.style_selected(kind);
        let name = self.unique_suffixed_name(kind, &src);
        let h = self.tabs[i].scene.document.allocate_handle();
        if !self.clone_style_as(kind, &src, &name, h) {
            return;
        }
        self.set_style_selected(kind, name.clone());
        self.load_style_bufs(kind);
        self.after_style_change(kind);
        self.command_line
            .push_output(crate::tf!("Style '{name}' created.").as_ref());
    }

    pub(super) fn style_delete(&mut self, kind: StyleKind) {
        let name = self.style_selected(kind);        
        if name.eq_ignore_ascii_case("Standard") {
            self.command_line
                .push_error(crate::t!("Cannot delete the Standard style.").as_ref());
            return;
        }
        if self.style_in_use(kind, &name) {
            self.command_line
                .push_error(crate::t!("Cannot delete a style that is current or in use.").as_ref());
            return;
        }
        if !self.remove_style_storage(kind, &name) {
            return;
        }
        let first = self
            .style_names(kind)
            .into_iter()
            .next()
            .unwrap_or_else(|| "Standard".to_string());
        self.set_style_selected(kind, first);
        self.load_style_bufs(kind);
        self.after_style_change(kind);
        self.command_line
            .push_output(crate::tf!("Style '{name}' deleted.").as_ref());
    }

    /// Begin inline rename of the double-clicked style.
    pub(super) fn style_rename_start(&mut self, kind: StyleKind, name: String) {
        self.set_style_selected(kind, name.clone());
        self.load_style_bufs(kind);
        self.style_rename_buf = name.clone();
        self.style_rename = Some(name);
    }

    /// Commit the inline rename. No-op (with feedback) on empty / unchanged /
    /// colliding names, and the Standard style cannot be renamed.
    pub(super) fn style_rename_commit(&mut self, kind: StyleKind) {
        let Some(old) = self.style_rename.take() else {
            return;
        };
        let new = self.style_rename_buf.trim().to_string();
        self.style_rename_buf.clear();
        if new.is_empty() || new.eq_ignore_ascii_case(&old) {
            return;
        }

        if old.eq_ignore_ascii_case("Standard") {
            self.command_line
                .push_error(crate::t!("Cannot rename the Standard style.").as_ref());
            return;
        }
        if self.style_exists(kind, &new) {
            self.command_line
                .push_error(crate::tf!("Style '{new}' already exists.").as_ref());
            return;
        }
        self.rename_style_storage(kind, &old, &new);
        if self.style_selected(kind).eq_ignore_ascii_case(&old) {
            self.set_style_selected(kind, new.clone());
        }
        self.load_style_bufs(kind);
        self.after_style_change(kind);
        self.command_line
            .push_output(crate::tf!("Renamed '{old}' → '{new}'.").as_ref());
    }

    pub(super) fn style_rename_cancel(&mut self) {
        self.style_rename = None;
        self.style_rename_buf.clear();
    }

    // ── Staging: nothing persists until Apply ──────────────────────────────
    //
    // A style manager is a transaction. When it opens we snapshot the style
    // tables / objects / current pointers; every New / Copy / Delete / Rename /
    // Set Current / property edit mutates the live document for an in-dialog
    // preview, but the tab stays clean and the drawing is not rebuilt. Apply
    // commits (marks dirty, pushes one undo entry, rebuilds); closing the
    // window without Apply discards by restoring the snapshot.

    /// Snapshot the style-related document state into a transferable record.
    fn capture_style_state(&self) -> StyleStateSnapshot {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let style_objects = doc
            .objects
            .iter()
            .filter(|(_, o)| {
                matches!(
                    o,
                    ObjectType::TableStyle(_)
                        | ObjectType::MLineStyle(_)
                        | ObjectType::MultiLeaderStyle(_)
                )
            })
            .map(|(&h, o)| (h, o.clone()))
            .collect();
        StyleStateSnapshot {
            text_styles: doc.text_styles.clone(),
            dim_styles: doc.dim_styles.clone(),
            style_objects,
            current_text: doc.header.current_text_style_name.clone(),
            current_dim: doc.header.current_dimstyle_name.clone(),
            multiline_style: doc.header.multiline_style.clone(),
            current_table: doc.header.current_table_style_name.clone(),
            current_mleader: doc.header.current_mleader_style_name.clone(),
            active_table: self.ribbon.active_table_style.clone(),
            active_mleader: self.ribbon.active_mleader_style.clone(),
            tab_active_mleader: self.tabs[i].active_mleader_style.clone(),
        }
    }

    /// Overwrite the live style state with a snapshot (used by commit's undo
    /// dance and by discard).
    pub(super) fn restore_style_state(&mut self, i: usize, snap: &StyleStateSnapshot) {
        let doc = &mut self.tabs[i].scene.document;
        doc.text_styles = snap.text_styles.clone();
        doc.dim_styles = snap.dim_styles.clone();
        doc.objects.retain(|_, o| {
            !matches!(
                o,
                ObjectType::TableStyle(_)
                    | ObjectType::MLineStyle(_)
                    | ObjectType::MultiLeaderStyle(_)
            )
        });
        for (h, o) in &snap.style_objects {
            doc.objects.insert(*h, o.clone());
        }
        sync_mleaderstyle_dictionary(doc);
        doc.header.current_text_style_name = snap.current_text.clone();
        doc.header.current_dimstyle_name = snap.current_dim.clone();
        doc.header.multiline_style = snap.multiline_style.clone();
        doc.header.current_table_style_name = snap.current_table.clone();
        doc.header.current_mleader_style_name = snap.current_mleader.clone();
        if i == self.active_tab {
            self.ribbon.active_table_style = snap.active_table.clone();
            self.ribbon.active_mleader_style = snap.active_mleader.clone();
        }
        self.tabs[i].active_mleader_style = snap.tab_active_mleader.clone();
    }

    /// Begin a staging transaction for a freshly-opened style manager.
    pub(super) fn style_stage_begin(&mut self) {
        let dirty_at_open = self.tabs[self.active_tab].dirty;
        let baseline = self.capture_style_state();
        self.style_stage = Some(StyleStage {
            dirty_at_open,
            baseline,
        });
    }

    /// Commit the staged changes (Apply): make them permanent with a single
    /// undo entry, mark the tab dirty, and rebuild the drawing.
    pub(super) fn style_stage_commit(&mut self) {
        let i = self.active_tab;
        let Some(stage) = self.style_stage.take() else {
            // No staged edits: a bare Apply has no geometry work to publish.
            self.sync_ribbon_styles();
            return;
        };
        sync_mleaderstyle_dictionary(
            &mut self.tabs[i].scene.document,
        );

        let edited = self.capture_style_state();
        let changed = edited != stage.baseline;
        if changed {
            self.tabs[i].dirty = true;
            let (text_names, dim_names, object_handles) = edited.changed_keys(&stage.baseline);
            let changed_mleader_styles:
                Vec<acadrust::objects::MultiLeaderStyle> =
                object_handles
                    .iter()
                    .filter_map(|handle| {
                        match self.tabs[i]
                            .scene
                            .document
                            .objects
                            .get(handle)
                        {
                            Some(
                                acadrust::objects::ObjectType::
                                    MultiLeaderStyle(style),
                            ) => Some(style.clone()),
                            _ => None,
                        }
                    })
                    .collect();

            let mut changed_mleaders = Vec::new();

            for style in changed_mleader_styles {
                let entity_handles: Vec<acadrust::Handle> = {
                    let doc = &self.tabs[i].scene.document;

                    doc.entities()
                        .filter_map(|entity| {
                            match entity {
                                acadrust::EntityType::MultiLeader(ml)
                                    if ml.style_handle
                                        == Some(style.handle) =>
                                {
                                    Some(ml.common.handle)
                                }
                                _ => None,
                            }
                        })
                        .collect()
                };

                for handle in entity_handles {
                    if crate::scene::annotative::
                        apply_mleader_style_to_object(
                            &mut self.tabs[i].scene.document,
                            handle,
                            &style,
                        )
                    {
                        changed_mleaders.push((
                            handle,
                            crate::scene::ChangeKind::Modified,
                        ));
                    }
                }
            }

            if !changed_mleaders.is_empty() {
                self.tabs[i]
                    .scene
                    .bump_entities(&changed_mleaders);
            }
            self.tabs[i].scene.invalidate_text_style_dependencies_many(&text_names);
            self.tabs[i]
                .scene
                .invalidate_dim_style_dependencies_many(&dim_names);
            self.tabs[i]
                .scene
                .invalidate_object_style_dependencies(&object_handles);
            self.commit_style_undo(i, stage.baseline, edited.clone(), stage.dirty_at_open);
        } else {
            self.tabs[i].dirty = stage.dirty_at_open;
        }
        self.sync_ribbon_styles();
        // Re-baseline so further edits in the still-open window stage afresh.
        self.style_stage = Some(StyleStage {
            dirty_at_open: self.tabs[i].dirty,
            baseline: edited,
        });
    }

    /// Discard staged changes (window closed without Apply): restore the
    /// snapshot taken when the manager opened.
    pub(super) fn style_stage_discard(&mut self) {
        let Some(stage) = self.style_stage.take() else {
            return;
        };
        self.restore_style_state(self.active_tab, &stage.baseline);
        self.tabs[self.active_tab].dirty = stage.dirty_at_open;
        self.sync_ribbon_styles();
    }
}

/// Snapshot of every document field a style manager can touch.
#[derive(Clone, PartialEq)]
pub(super) struct StyleStateSnapshot {
    text_styles: acadrust::tables::Table<TextStyle>,
    dim_styles: acadrust::tables::Table<DimStyle>,
    style_objects: Vec<(Handle, ObjectType)>,
    current_text: String,
    current_dim: String,
    multiline_style: String,
    current_table: String,
    current_mleader: String,
    active_table: String,
    active_mleader: String,
    tab_active_mleader: String,
}

impl StyleStateSnapshot {
    pub(super) fn estimated_bytes(&self) -> usize {
        self.text_styles
            .iter()
            .count()
            .saturating_mul(320)
            .saturating_add(self.dim_styles.iter().count().saturating_mul(1024))
            .saturating_add(self.style_objects.len().saturating_mul(512))
    }

    pub(super) fn changed_keys(&self, other: &Self) -> (Vec<String>, Vec<String>, Vec<Handle>) {
        let text_names = self
            .text_styles
            .iter()
            .filter(|style| other.text_styles.get(&style.name) != Some(*style))
            .map(|style| style.name.clone())
            .chain(
                other
                    .text_styles
                    .iter()
                    .filter(|style| self.text_styles.get(&style.name) != Some(*style))
                    .map(|style| style.name.clone()),
            )
            .collect::<rustc_hash::FxHashSet<_>>()
            .into_iter()
            .collect();
        let dim_names = self
            .dim_styles
            .iter()
            .filter(|style| other.dim_styles.get(&style.name) != Some(*style))
            .map(|style| style.name.clone())
            .chain(
                other
                    .dim_styles
                    .iter()
                    .filter(|style| self.dim_styles.get(&style.name) != Some(*style))
                    .map(|style| style.name.clone()),
            )
            .collect::<rustc_hash::FxHashSet<_>>()
            .into_iter()
            .collect();
        let self_objects: rustc_hash::FxHashMap<_, _> =
            self.style_objects.iter().map(|(h, o)| (*h, o)).collect();
        let other_objects: rustc_hash::FxHashMap<_, _> =
            other.style_objects.iter().map(|(h, o)| (*h, o)).collect();
        let object_handles = self
            .style_objects
            .iter()
            .chain(other.style_objects.iter())
            .map(|(handle, _)| *handle)
            .collect::<rustc_hash::FxHashSet<_>>()
            .into_iter()
            .filter(|handle| self_objects.get(handle) != other_objects.get(handle))
            .collect();
        (text_names, dim_names, object_handles)
    }
}

/// An in-progress style-manager transaction.
pub(super) struct StyleStage {
    dirty_at_open: bool,
    baseline: StyleStateSnapshot,
}

// ── Object-store helpers ───────────────────────────────────────────────────

/// Find the object-backed style named `name` and return a clone. `pick` maps a
/// matching variant to `(its name, a clone of the inner style)`.
fn find_object_style<T>(
    doc: &acadrust::CadDocument,
    name: &str,
    pick: impl Fn(&ObjectType) -> Option<(&str, T)>,
) -> Option<T> {
    doc.objects.values().find_map(|o| {
        let (n, val) = pick(o)?;
        n.eq_ignore_ascii_case(name).then_some(val)
    })
}

fn object_handle(doc: &acadrust::CadDocument, name: &str, kind: StyleKind) -> Option<Handle> {
    doc.objects.iter().find_map(|(&h, o)| {
        let matches = match (kind, o) {
            (StyleKind::Table, ObjectType::TableStyle(s)) => s.name.eq_ignore_ascii_case(name),
            (StyleKind::MLeader, ObjectType::MultiLeaderStyle(s)) => {
                s.name.eq_ignore_ascii_case(name)
            }
            (StyleKind::MLine, ObjectType::MLineStyle(s)) => s.name.eq_ignore_ascii_case(name),
            _ => false,
        };
        matches.then_some(h)
    })
}

// ── Annotation-scale manager staging (mirrors the style-manager stage) ──────

/// Snapshot of the drawing's annotation-scale *list* a scale manager can stage:
/// the `Scale` objects and the `ACAD_SCALELIST` dictionary. Set Current takes
/// effect immediately (like the pill) and is *not* reverted; `current` is kept
/// only to repair the header pointer if a staged rename/delete of the current
/// scale is rolled back, leaving it dangling.
pub(super) struct ScaleSnapshot {
    scales: Vec<(Handle, ObjectType)>,
    scalelist: Option<(Handle, ObjectType)>,
    current: String,
}

/// An in-progress scale-manager transaction (baseline restored on discard).
/// `changed` guards against a no-op Apply dirtying the drawing / pushing undo.
pub(super) struct ScaleStage {
    dirty_at_open: bool,
    baseline: ScaleSnapshot,
    changed: bool,
    /// The manager materialised the fallback scale set into real objects on
    /// open. Even with no user edit these must be reverted on close, so the
    /// discard restores the baseline when this is set (not only on `changed`).
    materialized: bool,
}

impl OpenCADStudio {
    fn capture_scale_state(&self) -> ScaleSnapshot {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let scales: Vec<(Handle, ObjectType)> = doc
            .objects
            .iter()
            .filter(|(_, o)| matches!(o, ObjectType::Scale(_)))
            .map(|(h, o)| (*h, o.clone()))
            .collect();
        let scalelist_h = self.tabs[i].scene.scalelist_dict_handle();
        let scalelist = scalelist_h.and_then(|h| doc.objects.get(&h).map(|o| (h, o.clone())));
        let current = doc.header.current_annotation_scale.clone();
        ScaleSnapshot {
            scales,
            scalelist,
            current,
        }
    }

    fn restore_scale_state(&mut self, snap: &ScaleSnapshot) {
        let i = self.active_tab;
        let root_h = self.tabs[i].scene.document.header.named_objects_dict_handle;
        let cur_scalelist_h = self.tabs[i].scene.scalelist_dict_handle();
        let doc = &mut self.tabs[i].scene.document;
        doc.objects.retain(|_, o| !matches!(o, ObjectType::Scale(_)));
        if let Some(h) = cur_scalelist_h {
            doc.objects.remove(&h);
        }
        for (h, o) in &snap.scales {
            doc.objects.insert(*h, o.clone());
        }
        if let Some((h, o)) = &snap.scalelist {
            doc.objects.insert(*h, o.clone());
        }
        // Restore ONLY the ACAD_SCALELIST root entry — leave every other root
        // entry alone (a modal blocks concurrent edits, but be surgical anyway).
        let baseline_scalelist_h = snap.scalelist.as_ref().map(|(h, _)| *h);
        if let Some(ObjectType::Dictionary(root)) = doc.objects.get_mut(&root_h) {
            root.entries.retain(|(k, _)| k != "ACAD_SCALELIST");
            if let Some(h) = baseline_scalelist_h {
                root.entries.push(("ACAD_SCALELIST".to_string(), h));
            }
        }
        // Repair a dangling current-scale pointer. A staged rename/delete of the
        // current scale rewrote the header outside this snapshot; if the live
        // name no longer resolves against the restored list, fall back to the
        // snapshot's name. A Set Current to a still-valid scale is preserved.
        let cur = self.tabs[i]
            .scene
            .document
            .header
            .current_annotation_scale
            .clone();
        let resolves = self.tabs[i]
            .scene
            .scale_list()
            .iter()
            .any(|(n, _, _)| n.eq_ignore_ascii_case(&cur));
        if !resolves {
            self.tabs[i].scene.document.header.current_annotation_scale = snap.current.clone();
        }
        self.tabs[i].scene.invalidate_annotation_dependencies();
    }

    /// Begin a staging transaction for a freshly-opened scale manager.
    pub(super) fn scale_stage_begin(&mut self) {
        let dirty_at_open = self.tabs[self.active_tab].dirty;
        let baseline = self.capture_scale_state();
        self.scale_stage = Some(ScaleStage {
            dirty_at_open,
            baseline,
            changed: false,
            materialized: false,
        });
    }

    /// Flag that the staged scale list has actually been mutated, so a later
    /// Apply commits (and a no-op Apply doesn't dirty the drawing).
    pub(super) fn scale_stage_mark(&mut self) {
        if let Some(s) = self.scale_stage.as_mut() {
            s.changed = true;
        }
    }

    /// Flag that the manager materialised the fallback scale set on open, so
    /// the discard reverts it even when the user made no edit.
    pub(super) fn scale_stage_materialized(&mut self) {
        if let Some(s) = self.scale_stage.as_mut() {
            s.materialized = true;
        }
    }

    /// Commit staged scale changes (Apply): one undo entry, mark dirty, and
    /// re-baseline so the still-open window keeps staging afresh. A no-op Apply
    /// (nothing staged) does nothing.
    pub(super) fn scale_stage_commit(&mut self) {
        let i = self.active_tab;
        let Some(stage) = self.scale_stage.take() else {
            return;
        };
        if !stage.changed {
            self.scale_stage = Some(stage);
            return;
        }
        let edited = self.capture_scale_state();
        self.restore_scale_state(&stage.baseline);
        self.push_undo_snapshot(i, "SCALELISTEDIT");
        self.restore_scale_state(&edited);
        self.tabs[i].dirty = true;
        self.scale_stage = Some(ScaleStage {
            dirty_at_open: true,
            baseline: edited,
            changed: false,
            // The materialised set is now committed as the new baseline, so it
            // no longer needs reverting on close.
            materialized: false,
        });
    }

    /// Discard staged scale changes (window closed without Apply): restore the
    /// snapshot taken when the manager opened. No-op when nothing was staged.
    pub(super) fn scale_stage_discard(&mut self) {
        let Some(stage) = self.scale_stage.take() else {
            return;
        };
        // Revert on a real edit *or* when only the fallback set was materialised
        // (opened but not applied) — otherwise those 12 scales would leak in.
        if stage.changed || stage.materialized {
            self.restore_scale_state(&stage.baseline);
            self.tabs[self.active_tab].dirty = stage.dirty_at_open;
        }
    }
}
