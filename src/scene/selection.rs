// Auto-split from scene/mod.rs. Pure text-move; behaviour unchanged.
use super::*;

impl Scene {
    // ── Selection ─────────────────────────────────────────────────────────
    /// Treat a classic LEADER and its attached annotation as one logical object.
    /// Clicking/copying/deleting either side expands to the complete pair.
    pub(crate) fn handles_expanded_for_leader_annotations(
        &self,
        handles: &[Handle],
    ) -> Vec<Handle> {
        let mut expanded = handles.to_vec();

        for &handle in handles {
            // LEADER -> annotation.
            if let Some(EntityType::Leader(leader)) = self.document.get_entity(handle) {
                if !leader.annotation_handle.is_null() {
                    expanded.push(leader.annotation_handle);
                }
            }

            // Annotation -> LEADER.
            expanded.extend(self.document.entities().filter_map(|entity| match entity {
                EntityType::Leader(leader)
                    if !leader.annotation_handle.is_null()
                        && leader.annotation_handle == handle =>
                {
                    Some(entity.common().handle)
                }
                _ => None,
            }));
        }

        expanded.sort_unstable_by_key(|handle| handle.value());
        expanded.dedup();
        expanded
    }
    pub fn select_entity(&mut self, handle: Handle, exclusive: bool) {
        let handles = self.handles_expanded_for_leader_annotations(&[handle]);

        if exclusive {
            self.selected.clear();
            self.selected_order.clear();
        }

        for handle in handles {
            if self.selected.insert(handle) {
                self.selected_order.push(handle);
            }
        }
        self.bump_selection();
    }

    pub fn deselect_all(&mut self) {
        self.selected.clear();
        self.selected_order.clear();
        self.bump_selection();
    }

    pub(crate) fn selected_handles_in_order(&self) -> Vec<Handle> {
        let mut seen = HashSet::default();
        let mut ordered: Vec<_> = self
            .selected_order
            .iter()
            .copied()
            .filter(|handle| self.selected.contains(handle) && seen.insert(*handle))
            .collect();
        let mut missing: Vec<_> = self
            .selected
            .iter()
            .copied()
            .filter(|handle| seen.insert(*handle))
            .collect();
        missing.sort_unstable_by_key(|handle| handle.value());
        ordered.extend(missing);
        ordered
    }

    /// Replace the complete selection and invalidate the GPU highlight overlay
    /// only when its contents actually changed. History/file/command paths must
    /// use this instead of assigning `selected` directly.
    pub(crate) fn replace_selection(&mut self, selected: HashSet<Handle>) {
        let handles: Vec<Handle> = selected.iter().copied().collect();
        let selected: HashSet<Handle> = self
            .handles_expanded_for_leader_annotations(&handles)
            .into_iter()
            .collect();

        if self.selected != selected {
            let mut seen = HashSet::default();
            let mut order: Vec<_> = self
                .selected_order
                .iter()
                .copied()
                .filter(|handle| selected.contains(handle) && seen.insert(*handle))
                .collect();
            let mut added: Vec<_> = selected
                .iter()
                .copied()
                .filter(|handle| seen.insert(*handle))
                .collect();
            added.sort_unstable_by_key(|handle| handle.value());
            order.extend(added);
            self.selected = selected;
            self.selected_order = order;
            self.bump_selection();
        }
    }

    /// Remove a single entity from the selection (Shift+click subtractive pick).
    pub fn deselect_entity(&mut self, handle: Handle) {
        let handles = self.handles_expanded_for_leader_annotations(&[handle]);
        let mut changed = false;

        for handle in handles {
            changed |= self.selected.remove(&handle);
            self.selected_order.retain(|selected| *selected != handle);
        }

        if changed {
            self.bump_selection();
        }
    }

    pub fn selected_entities(&self) -> Vec<(Handle, &EntityType)> {
        self.selected_handles_in_order()
            .into_iter()
            .filter_map(|h| self.document.get_entity(h).map(|e| (h, e)))
            .collect()
    }

    /// Iterates every entity owned by the current layout's block-record.
    /// Returns an empty vec when the block-record is missing or holds no
    /// entity handles (legacy DXF without group-code 330 — we err on the
    /// side of "no candidates" instead of scanning the whole document, so
    /// model-block entities don't leak into a paper-layout selection).
    fn current_layout_entity_handles(&self) -> Vec<Handle> {
        let block = self.current_layout_block_handle();
        self.document
            .block_records
            .iter()
            .find(|br| br.handle == block)
            .map(|br| br.entity_handles.clone())
            .unwrap_or_default()
    }

    fn qselect_candidate_handles(&self, scope: crate::app::QSelectScope) -> Vec<Handle> {
        match scope {
            crate::app::QSelectScope::CurrentSpace => self.current_layout_entity_handles(),
            crate::app::QSelectScope::CurrentSelection => {
                self.selected_handles_in_order()
            }
        }
    }

    pub fn qselect_candidate_count(&self, scope: crate::app::QSelectScope) -> usize {
        self.qselect_candidate_handles(scope).len()
    }

    pub fn qselect_entity_type_names(&self, scope: crate::app::QSelectScope) -> Vec<String> {
        use crate::entities::traits::entity_type_name;
        let mut names = std::collections::BTreeSet::new();
        for h in self.qselect_candidate_handles(scope) {
            if let Some(entity) = self.document.get_entity(h) {
                names.insert(entity_type_name(entity).to_string());
            }
        }
        names.into_iter().collect()
    }

    /// Extends the current selection with every entity in the active
    /// layout that matches one of the selected entities by `(variant,
    /// layer)`. The seed selection stays selected. No-op when nothing is
    /// selected. Returns the number of newly-added entities.
    pub fn select_similar(&mut self) -> usize {
        use crate::entities::traits::entity_type_name;
        if self.selected.is_empty() {
            return 0;
        }
        let pairs: rustc_hash::FxHashSet<(&str, String)> = self
            .selected
            .iter()
            .filter_map(|h| self.document.get_entity(*h))
            .map(|e| (entity_type_name(e), e.as_entity().layer().to_string()))
            .collect();
        let handles = self.current_layout_entity_handles();
        let mut added = 0;
        for h in handles {
            if self.selected.contains(&h) {
                continue;
            }
            if let Some(e) = self.document.get_entity(h) {
                let key = (entity_type_name(e), e.as_entity().layer().to_string());
                if pairs.contains(&key) {
                    self.selected.insert(h);
                    self.selected_order.push(h);
                    added += 1;
                }
            }
        }
        if added > 0 {
            self.bump_selection();
        }
        added
    }

    /// Replace the selection with its complement: every selectable object
    /// in the active layout that isn't currently selected. The candidate
    /// set is the visible wire set, so objects on off/frozen layers (which
    /// can't be picked anyway) are excluded. Returns the new count.
    pub fn invert_selection(&mut self) -> usize {
        let prev: rustc_hash::FxHashSet<Handle> = self.selected.iter().copied().collect();
        let all: Vec<Handle> = self
            .entity_wires()
            .iter()
            .filter_map(|w| Self::handle_from_wire_name(&w.name))
            .collect();
        self.selected.clear();
        self.selected_order.clear();
        for h in all {
            if !prev.contains(&h) {
                self.selected.insert(h);
                self.selected_order.push(h);
            }
        }
        self.bump_selection();
        self.selected.len()
    }

    /// Builds a selection from the chosen scope and filter. Exclude mode
    /// keeps candidates that do not match. Append mode unions the result
    /// with the selection that existed before the query.
    ///
    /// `type_name` of `None` means "any type". `property_field` of
    /// `None` skips the property test (only the type filter applies).
    /// The operator's `Any` variant also skips the property test.
    /// Numeric operators (`Gt` / `Lt`) parse both sides as `f64` and
    /// reject anything non-numeric.
    pub fn qselect(
        &mut self,
        scope: crate::app::QSelectScope,
        type_name: Option<&str>,
        property_field: Option<&str>,
        op: crate::app::QSelectOp,
        value: &str,
        mode: crate::app::QSelectMode,
        append: bool,
    ) -> usize {
        use crate::app::{QSelectMode, QSelectOp};
        use crate::entities::traits::entity_type_name;
        let previous = self.selected.clone();
        let handles = self.qselect_candidate_handles(scope);
        let mut result = HashSet::default();
        for h in handles {
            let Some(e) = self.document.get_entity(h) else {
                continue;
            };
            let type_ok = type_name.is_none_or(|t| entity_type_name(e) == t);
            let prop_ok = if !type_ok {
                true
            } else {
                match (property_field, op) {
                    (None, _) | (_, QSelectOp::Any) => true,
                    (Some(field), op) => {
                        match self.entity_property_value(e, field) {
                            Some(actual) => match op {
                                QSelectOp::Eq => actual.eq_ignore_ascii_case(value),
                                QSelectOp::Neq => !actual.eq_ignore_ascii_case(value),
                                QSelectOp::Gt | QSelectOp::Lt => {
                                    match (
                                        crate::entities::common::parse_f64(&actual),
                                        crate::entities::common::parse_f64(value),
                                    ) {
                                        (Some(a), Some(b)) if matches!(op, QSelectOp::Gt) => a > b,
                                        (Some(a), Some(b)) => a < b,
                                        _ => false,
                                    }
                                }
                                QSelectOp::Any => true,
                            },
                            None => false,
                        }
                    }
                }
            };
            let matches_filter = type_ok && prop_ok;
            let keep = match mode {
                QSelectMode::Include => matches_filter,
                QSelectMode::Exclude => !matches_filter,
            };
            if keep {
                result.insert(h);
            }
        }
        if append {
            result.extend(previous);
        }
        self.replace_selection(result);
        self.selected.len()
    }

    /// Returns the sorted set of entity-type names present in the active
    /// layout. Used to populate the Quick Select "Object type" dropdown
    /// with only the types that actually exist in the drawing.
    pub fn entity_type_names_in_layout(&self) -> Vec<String> {
        use crate::entities::traits::entity_type_name;
        let mut names: std::collections::BTreeSet<String> =
            std::collections::BTreeSet::new();
        for h in self.current_layout_entity_handles() {
            if let Some(e) = self.document.get_entity(h) {
                names.insert(entity_type_name(e).to_string());
            }
        }
        names.into_iter().collect()
    }

    /// True when `handle`'s entity type is allowed by the selection filter.
    /// The filter stores excluded type names; empty = everything allowed.
    pub fn passes_selection_filter(&self, handle: Handle) -> bool {
        if self.selection_filter.is_empty() {
            return true;
        }
        match self.document.get_entity(handle) {
            Some(e) => !self
                .selection_filter
                .contains(crate::entities::traits::entity_type_name(e)),
            None => true,
        }
    }

    /// True when the selection filter is excluding at least one type.
    pub fn selection_filter_active(&self) -> bool {
        !self.selection_filter.is_empty()
    }

    /// Returns common and type-specific filter properties with the editor
    /// each value needs. Choice editors are enriched from both document
    /// tables and values present in the selected candidate scope.
    pub fn qselect_properties(
        &self,
        type_name: Option<&str>,
        scope: crate::app::QSelectScope,
    ) -> Vec<crate::app::QSelectPropertyChoice> {
        use crate::entities::traits::{entity_type_name, EntityTypeOps};
        use crate::scene::model::object::PropValue;
        use crate::app::{QSelectPropertyChoice, QSelectValueEditor};

        let candidate_handles: Vec<Handle> = self
            .qselect_candidate_handles(scope)
            .into_iter()
            .filter(|h| {
                self.document.get_entity(*h).is_some_and(|entity| {
                    type_name.is_none_or(|t| entity_type_name(entity) == t)
                })
            })
            .collect();

        let mut layer_options: Vec<String> = self
            .document
            .layers
            .iter()
            .map(|layer| layer.name.clone())
            .collect();
        layer_options.sort_by_key(|value| value.to_lowercase());
        let mut linetype_options = vec!["ByLayer".to_string(), "ByBlock".to_string()];
        linetype_options.extend(
            self.document
                .line_types
                .iter()
                .map(|linetype| linetype.name.clone()),
        );

        let choice = |field: &str, label: String, editor: QSelectValueEditor| {
            QSelectPropertyChoice {
                field: field.to_string(),
                label,
                editor,
            }
        };
        let mut out = vec![
            choice("handle", crate::t!("Handle").into_owned(), QSelectValueEditor::Text),
            choice(
                "color",
                crate::t!("Color").into_owned(),
                QSelectValueEditor::Choice(vec!["ByLayer".into(), "ByBlock".into()]),
            ),
            choice(
                "layer",
                crate::t!("Layer").into_owned(),
                QSelectValueEditor::Choice(layer_options.clone()),
            ),
            choice(
                "linetype",
                crate::t!("Linetype").into_owned(),
                QSelectValueEditor::Choice(linetype_options.clone()),
            ),
            choice(
                "linetype_scale",
                crate::t!("Linetype scale").into_owned(),
                QSelectValueEditor::Number,
            ),
            choice(
                "plot_style",
                crate::t!("Plot style").into_owned(),
                QSelectValueEditor::Choice(vec![
                    "ByLayer".into(),
                    "ByBlock".into(),
                    "ByColor".into(),
                ]),
            ),
            choice(
                "lineweight",
                crate::t!("Lineweight").into_owned(),
                QSelectValueEditor::Choice(vec![
                    "ByLayer".into(),
                    "ByBlock".into(),
                    "Default".into(),
                ]),
            ),
            choice(
                "transparency",
                crate::t!("Transparency").into_owned(),
                QSelectValueEditor::Choice(vec!["ByLayer".into()]),
            ),
            choice(
                "hyperlink",
                crate::t!("Hyperlink").into_owned(),
                QSelectValueEditor::Text,
            ),
        ];
        if let Some(t) = type_name {
            let text_style_names: Vec<String> = self
                .document
                .text_styles
                .iter()
                .map(|s| s.name.clone())
                .collect();
            let sample = candidate_handles
                .iter()
                .copied()
                .filter_map(|h| self.document.get_entity(h))
                .find(|e| entity_type_name(e) == t);
            if let Some(sample) = sample {
                let mut sections = vec![crate::scene::cache::properties::general_section(sample)];
                if let Some(section) =
                    crate::scene::cache::properties::visualization_section(sample)
                {
                    sections.push(section);
                }
                sections.extend(sample.geometry_properties(&text_style_names));
                for section in sections {
                    for prop in section.props {
                        if out.iter().any(|item| item.field == prop.field) {
                            continue;
                        }
                        let editor = if prop.field == "material" {
                            QSelectValueEditor::Choice(vec![
                                "ByLayer".into(),
                                "ByBlock".into(),
                                "Custom".into(),
                            ])
                        } else {
                            match prop.value {
                                PropValue::PlainText(_) => QSelectValueEditor::Text,
                                PropValue::ReadOnly(ref value)
                                | PropValue::EditText(ref value) => {
                                    let field = prop.field.to_ascii_lowercase();
                                    let textual = [
                                        "name",
                                        "text",
                                        "content",
                                        "style",
                                        "tag",
                                        "prompt",
                                        "value",
                                        "description",
                                        "format",
                                        "font",
                                        "path",
                                        "file",
                                        "url",
                                    ]
                                    .iter()
                                    .any(|part| field.contains(part));
                                    if !textual
                                        && crate::entities::common::parse_f64(value).is_some()
                                    {
                                        QSelectValueEditor::Number
                                    } else {
                                        QSelectValueEditor::Text
                                    }
                                }
                                PropValue::LayerChoice(_) => {
                                    QSelectValueEditor::Choice(layer_options.clone())
                                }
                                PropValue::Choice { ref options, .. } => {
                                    QSelectValueEditor::Choice(options.clone())
                                }
                                PropValue::EditChoice {
                                    ref value,
                                    ref options,
                                } => {
                                    let mut values = options.clone();
                                    if !values.iter().any(|item| item == value) {
                                        values.push(value.clone());
                                    }
                                    QSelectValueEditor::Choice(values)
                                }
                                PropValue::ColorChoice(_) => QSelectValueEditor::Choice(vec![
                                    "ByLayer".into(),
                                    "ByBlock".into(),
                                ]),
                                PropValue::LwChoice(_) => QSelectValueEditor::Choice(vec![
                                    "ByLayer".into(),
                                    "ByBlock".into(),
                                    "Default".into(),
                                ]),
                                PropValue::LinetypeChoice(_) => {
                                    QSelectValueEditor::Choice(linetype_options.clone())
                                }
                                PropValue::HatchPatternChoice(value) => {
                                    let mut patterns: Vec<String> =
                                        crate::scene::model::hatch_patterns::catalog()
                                            .iter()
                                            .map(|entry| entry.name.clone())
                                            .collect();
                                    if !patterns.iter().any(|item| item == &value) {
                                        patterns.push(value);
                                    }
                                    QSelectValueEditor::Choice(patterns)
                                }
                                PropValue::BoolToggle { .. } => {
                                    QSelectValueEditor::Choice(vec![
                                        "false".into(),
                                        "true".into(),
                                    ])
                                }
                                PropValue::AttrText { .. } => QSelectValueEditor::Text,
                                PropValue::Stepper { .. }
                                | PropValue::ColorVaries
                                | PropValue::LwVaries => continue,
                            }
                        };
                        out.push(choice(prop.field, prop.label, editor));
                    }
                }
            }
        }

        for property in &mut out {
            let QSelectValueEditor::Choice(options) = &mut property.editor else {
                continue;
            };
            let common_choice = matches!(
                property.field.as_str(),
                "color"
                    | "layer"
                    | "linetype"
                    | "plot_style"
                    | "lineweight"
                    | "transparency"
                    | "material"
            );
            if !common_choice && !options.is_empty() {
                continue;
            }
            for handle in &candidate_handles {
                let Some(entity) = self.document.get_entity(*handle) else {
                    continue;
                };
                if let Some(value) = self.entity_property_value(entity, &property.field) {
                    if !options.iter().any(|option| option.eq_ignore_ascii_case(&value)) {
                        options.push(value);
                    }
                }
            }
            options.sort_by_key(|value| value.to_lowercase());
        }
        out
    }

    /// Reads a property value from an entity for QSELECT comparison.
    /// Returns the canonical string used as the left-hand side of the
    /// operator test. Common properties have hand-rolled formatting so
    /// `"ByLayer"` / `"7"` / `"0.30mm"` are stable; everything else
    /// goes through `geometry_properties()` and pulls the matching
    /// row's value out.
    pub fn entity_property_value(
        &self,
        entity: &acadrust::EntityType,
        field: &str,
    ) -> Option<String> {
        use crate::entities::traits::EntityTypeOps;
        use crate::scene::model::object::PropValue;
        match field {
            "handle" => Some(entity.common().handle.value().to_string()),
            "layer" => Some(entity.common().layer.clone()),
            "color" => Some(Self::format_color(entity.common().color)),
            "linetype" => Some(if entity.common().linetype.is_empty() {
                "ByLayer".to_string()
            } else {
                entity.common().linetype.clone()
            }),
            "linetype_scale" => Some(format!("{:.4}", entity.common().linetype_scale)),
            "plot_style" => Some(
                match entity.common().plotstyle_flags {
                    0 => "ByLayer",
                    1 => "ByBlock",
                    _ => "ByColor",
                }
                .to_string(),
            ),
            "lineweight" => Some(Self::format_lineweight(entity.common().line_weight)),
            "transparency" => Some(if entity.common().transparency.alpha() == 0 {
                "ByLayer".to_string()
            } else {
                ((entity.common().transparency.alpha() as f64 / 255.0 * 100.0).round() as u32)
                    .to_string()
            }),
            "hyperlink" => Some(
                entity
                    .common()
                    .extended_data
                    .get_record("PE_URL")
                    .and_then(|record| {
                        record.values.iter().find_map(|value| match value {
                            acadrust::xdata::XDataValue::String(text) if !text.is_empty() => {
                                Some(text.clone())
                            }
                            _ => None,
                        })
                    })
                    .unwrap_or_default(),
            ),
            "material" => Some(
                match entity.common().material_flags {
                    0 => "ByLayer",
                    1 => "ByBlock",
                    _ => "Custom",
                }
                .to_string(),
            ),
            _ => {
                let text_style_names: Vec<String> = self
                    .document
                    .text_styles
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                let mut sections = vec![crate::scene::cache::properties::general_section(entity)];
                if let Some(section) =
                    crate::scene::cache::properties::visualization_section(entity)
                {
                    sections.push(section);
                }
                sections.extend(entity.geometry_properties(&text_style_names));
                let prop = sections
                    .into_iter()
                    .flat_map(|s| s.props)
                    .find(|p| p.field == field)?;
                Some(match prop.value {
                    PropValue::ReadOnly(s)
                    | PropValue::EditText(s)
                    | PropValue::PlainText(s) => s,
                    PropValue::LayerChoice(s) => s,
                    PropValue::Choice { selected, .. } => selected,
                    PropValue::EditChoice { value, .. } => value,
                    PropValue::ColorChoice(c) => Self::format_color(c),
                    PropValue::LwChoice(lw) => Self::format_lineweight(lw),
                    PropValue::LinetypeChoice(s) => s,
                    PropValue::HatchPatternChoice(s) => s,
                    PropValue::BoolToggle { value, .. } => value.to_string(),
                    PropValue::AttrText { value, .. } => value,
                    PropValue::Stepper { display, .. } => display,
                    PropValue::ColorVaries | PropValue::LwVaries => return None,
                })
            }
        }
    }

    fn format_color(c: acadrust::types::Color) -> String {
        use acadrust::types::Color;
        match c {
            Color::ByLayer => "ByLayer".to_string(),
            Color::None => "None".to_string(),
            Color::ByBlock => "ByBlock".to_string(),
            Color::Index(i) => i.to_string(),
            Color::Rgb { r, g, b } => format!("{},{},{}", r, g, b),
        }
    }

    fn format_lineweight(lw: acadrust::types::LineWeight) -> String {
        use acadrust::types::LineWeight;
        match lw {
            LineWeight::ByLayer => "ByLayer".to_string(),
            LineWeight::ByBlock => "ByBlock".to_string(),
            LineWeight::Default => "Default".to_string(),
            LineWeight::Value(v) => format!("{:.2}mm", v as f64 / 100.0),
        }
    }

    // ── Erase ─────────────────────────────────────────────────────────────

    pub fn erase_entities(&mut self, handles: &[Handle]) {

        let erase_handles = self.handles_expanded_for_leader_annotations(handles);

        let mut handle_set: HashSet<Handle> = HashSet::default();
        let mut erased: Vec<(Handle, ChangeKind)> = Vec::new();
        let mut highlight_changed = false;

        for &h in &erase_handles {
            // Objects on a locked layer can't be erased.
            if self.is_layer_locked(h) {
                continue;
            }
            // Delta-undo: capture the removed entity so an undo can re-insert it.
            if self.is_recording_undo() {
                let before = self.document.get_entity_arc(h);
                self.record_undo_before(h, before);
            }
            self.delete_solid_history(h);
            self.remember_removed_cache_categories(h);
            self.document.remove_entity_arc(h);
            highlight_changed |= self.selected.remove(&h);
            self.selected_order.retain(|selected| *selected != h);
            if self.hover_highlight == Some(h) {
                self.hover_highlight = None;
                highlight_changed = true;
            }
            self.hatches.remove(&h);
            self.images.remove(&h);
            self.meshes.remove(&h);
            self.block_meshes.remove(&h);
            self.solid_models.remove(&h);
            handle_set.insert(h);
            erased.push((h, ChangeKind::Removed));
        }
        if highlight_changed {
            self.bump_selection();
        }
        // Capture exactly the group objects that this erase will rewrite, plus
        // the group dictionary when an emptied group will be removed. Do this
        // before taking mutable object-map borrows below.
        if self.is_recording_undo() && !handle_set.is_empty() {
            let affected_groups: Vec<(Handle, ObjectType)> = self
                .document
                .objects
                .iter()
                .filter_map(|(&handle, object)| match object {
                    ObjectType::Group(group)
                        if group.entities.iter().any(|h| handle_set.contains(h)) =>
                    {
                        Some((handle, object.clone()))
                    }
                    _ => None,
                })
                .collect();
            if !affected_groups.is_empty() {
                let removes_group = affected_groups.iter().any(|(_, object)| {
                    matches!(
                        object,
                        ObjectType::Group(group)
                            if group.entities.iter().all(|h| handle_set.contains(h))
                    )
                });
                if removes_group {
                    let dictionary = self.document.header.acad_group_dict_handle;
                    let dictionary_before = self.document.objects.get(&dictionary).cloned();
                    self.record_undo_object_before(dictionary, dictionary_before);
                }
                for (handle, object) in affected_groups {
                    self.record_undo_object_before(handle, Some(object));
                }
            }
        }
        // Remove erased handles from all groups; delete groups that become empty.
        let group_dict_handle = self.document.header.acad_group_dict_handle;
        let to_remove: Vec<Handle> = self
            .document
            .objects
            .values_mut()
            .filter_map(|obj| match obj {
                ObjectType::Group(g) => {
                    g.entities.retain(|h| !handle_set.contains(h));
                    if g.entities.is_empty() {
                        Some(g.handle)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();
        for gh in &to_remove {
            if let Some(ObjectType::Dictionary(dict)) =
                self.document.objects.get_mut(&group_dict_handle)
            {
                dict.entries.retain(|(_, h)| h != gh);
            }
            self.document.objects.remove(gh);
        }
        // Deleting top-level entities/inserts leaves block definitions intact.
        // Report the exact erased handles so derived caches drop just those and
        // the resident set removes only their wires (bump_entities drops them
        // from the tessellation memos too).
        if !erased.is_empty() {
            self.invalidate_dependency_index();
            self.bump_entities(&erased);
        }
    }

    /// Restore erased Arc-backed entities without re-linking their still-present
    /// block-record handles. Used by OOPS and history replay.
    pub fn restore_erased_entities(&mut self, entities: Vec<Arc<EntityType>>) -> Vec<Handle> {
        let mut restored = Vec::with_capacity(entities.len());
        let mut changes = Vec::with_capacity(entities.len());
        for entity in entities {
            let handle = entity.common().handle;
            if handle.is_null() || self.document.get_entity(handle).is_some() {
                continue;
            }
            if self.is_recording_undo() {
                self.record_undo_before(handle, None);
            }
            if self.document.restore_entity_arc(entity).is_some() {
                self.reseed_derived_caches(handle);
                restored.push(handle);
                changes.push((handle, ChangeKind::Added));
            }
        }
        if !changes.is_empty() {
            self.invalidate_dependency_index();
            self.bump_entities(&changes);
        }
        restored
    }
}
