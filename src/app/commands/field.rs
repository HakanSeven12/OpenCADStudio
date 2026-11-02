//! FIELD and the Field dialog behind the ƒ buttons of the attribute
//! definition dialogs.

use crate::app::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::modules::annotate::field_cmd::{
    FieldObjectPickCommand, FieldPlaceCommand, FieldTablePickCommand,
};
use crate::ui::window::field_dialog::{
    fields_of, nav_props, object_properties, FieldDialogMsg, FieldDialogState, FieldTarget,
    DATE_FORMATS, NAMED_TYPES,
};
use iced::Task;

impl OpenCADStudio {
    pub(super) fn dispatch_field(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        match cmd {
            "FIELD" => {
                self.open_field_dialog(FieldTarget::NewText);
                Some(Task::none())
            }
            // FIELDEVAL — the events that update fields, kept in the drawing.
            cmd if cmd == "FIELDEVAL"
                || cmd.starts_with("FIELDEVAL ")
                || cmd.starts_with("SETVAR FIELDEVAL") =>
            {
                let value = cmd
                    .trim_start_matches("SETVAR ")
                    .trim_start_matches("FIELDEVAL")
                    .trim();
                let current = crate::entities::field::fieldeval(&self.tabs[i].scene.document);
                let ask = |app: &mut Self| {
                    app.command_line
                        .push_output(&format!("Enter new value for FIELDEVAL <{current}>:"));
                    app.pending_setvar = Some("FIELDEVAL".into());
                };
                match value.parse::<i32>() {
                    _ if value.is_empty() => ask(self),
                    Ok(n) if (0..=31).contains(&n) => {
                        if n != current {
                            self.push_undo_snapshot(i, "FIELDEVAL");
                            crate::io::set_drawing_variable(
                                &mut self.tabs[i].scene.document,
                                "FIELDEVAL",
                                &n.to_string(),
                            );
                            self.tabs[i].dirty = true;
                        }
                    }
                    _ => {
                        let (min, max) = (0, 31);
                        self.command_line.push_error(&crate::tf!(
                            "Requires an integer between {min} and {max}."
                        ));
                        ask(self);
                    }
                }
                Some(Task::none())
            }
            cmd if cmd == "_FIELD_OBJECT" || cmd.starts_with("_FIELD_OBJECT ") => {
                let handle = cmd
                    .split_whitespace()
                    .nth(1)
                    .and_then(|h| u64::from_str_radix(h, 16).ok())
                    .map(codec::Handle::new);
                if let Some(state) = self.field_dialog.as_mut() {
                    if let Some(entity) =
                        handle.and_then(|h| self.tabs[i].scene.document.get_entity(h))
                    {
                        let props = object_properties(entity);
                        state.object = Some((
                            entity.common().handle,
                            crate::t!(crate::entities::names::ui_name(entity)).into_owned(),
                        ));
                        state.object_prop = props.first().copied();
                        state.object_props = props;
                        state.object_versioned = None;
                    }
                    self.active_modal = Some(crate::app::ModalKind::Field);
                    self.refresh_field_preview();
                }
                Some(Task::none())
            }
            cmd if cmd == "_FIELD_CELL" || cmd.starts_with("_FIELD_CELL ") => {
                let parts: Vec<&str> = cmd.split_whitespace().skip(1).collect();
                if let Some(expression) = self.table_cell_expression(i, &parts) {
                    if let Some(state) = self.field_dialog.as_mut() {
                        // The reference appends at the end, with no operator.
                        state.formula.push_str(&expression);
                    }
                    self.refresh_field_preview();
                    // A real result gets the Decimal format, as the reference picks it.
                    if let Some(state) = self.field_dialog.as_mut() {
                        if state.formula_format == 0 && state.preview.contains('.') {
                            state.formula_format = 2;
                        }
                    }
                }
                if self.field_dialog.is_some() {
                    self.active_modal = Some(crate::app::ModalKind::Field);
                    self.refresh_field_preview();
                }
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// The formula text for a picked table cell or cell range:
    /// `Table(%<\_ObjId H>%).B4` or `Table(%<\_ObjId H>%).Evaluate(Sum(A3:B5))`.
    fn table_cell_expression(&self, i: usize, parts: &[&str]) -> Option<String> {
        let function = *parts.first()?;
        let cell = |at: &[&str]| -> Option<(codec::Handle, usize, usize)> {
            let handle = codec::Handle::new(u64::from_str_radix(at.first()?, 16).ok()?);
            let point = glam::DVec3::new(
                at.get(1)?.parse().ok()?,
                at.get(2)?.parse().ok()?,
                at.get(3)?.parse().ok()?,
            );
            let document = &self.tabs[i].scene.document;
            let codec::EntityType::Table(table) = document.get_entity(handle)? else {
                return None;
            };
            let hit = crate::modules::annotate::table_cmd::table_cell_at(table, None, point)?;
            Some((handle, hit.row, hit.column))
        };
        let name = |row: usize, column: usize| format!("{}{}", column_letters(column), row + 1);
        let (table, row, column) = cell(parts.get(1..5)?)?;
        if function == "Cell" {
            return Some(format!(
                "Table(%<\\_ObjId {:X}>%).{}",
                table.value(),
                name(row, column)
            ));
        }
        let (second_table, row2, column2) = cell(parts.get(5..9)?)?;
        if second_table != table {
            return None;
        }
        Some(format!(
            "Table(%<\\_ObjId {:X}>%).Evaluate({function}({}:{}))",
            table.value(),
            name(row.min(row2), column.min(column2)),
            name(row.max(row2), column.max(column2)),
        ))
    }

    /// Open the Field dialog for `target`.
    pub(in crate::app) fn open_field_dialog(&mut self, target: FieldTarget) {
        let mut state = FieldDialogState::new(target);
        if matches!(target, FieldTarget::AttdefDefault | FieldTarget::AttdefEdit) {
            state.placeholder_block = self.tabs[self.active_tab]
                .active_block_edit_session()
                .map(|session| session.block_name.clone());
        }
        state.examples = DATE_FORMATS
            .iter()
            .map(|f| self.field_value(&format!("\\AcVar Date \\f \"{f}\""), &[]))
            .collect();
        if let Some(db) = self.sheet_set.db() {
            let custom = codec::sheet_set::custom_properties(db.sheet_set());
            let names = |flag: i32| {
                custom
                    .iter()
                    .filter(|(_, _, f)| f & flag != 0)
                    .map(|(n, _, _)| n.clone())
                    .collect()
            };
            state.ss_sheet_custom = names(codec::sheet_set::CUSTOM_SHEET_PROP);
            state.ss_set_custom = names(codec::sheet_set::CUSTOM_SHEET_SET_PROP);
            state.ss_set_name = db.name().to_string();
        }
        state.custom_properties = self.tabs[self.active_tab]
            .scene
            .document
            .summary_info
            .custom_properties
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        state.ss_sets = self.sheet_set.sets.clone();
        state.ss_set = self.sheet_set.current.unwrap_or(0);
        self.field_dialog = Some(state);
        self.fill_named_objects();
        self.refresh_field_preview();
        self.active_modal = Some(crate::app::ModalKind::Field);
    }

    /// Open the Field dialog on the field text `host` is made of, so OK
    /// changes that field in place. False when `host` is not such a text.
    pub(in crate::app) fn open_field_edit(&mut self, host: codec::Handle) -> bool {
        let i = self.active_tab;
        let Some((code, objects)) =
            crate::entities::field::whole_text_field(&self.tabs[i].scene.document, host)
        else {
            return false;
        };
        self.open_field_dialog(FieldTarget::EditText(host));
        let document = &self.tabs[i].scene.document;
        if let Some(state) = self.field_dialog.as_mut() {
            let property = state.seed_from_code(&code, &objects);
            let object = objects.first().and_then(|handle| document.get_entity(*handle));
            if let (Some(property), Some(entity)) = (property, object) {
                let props = object_properties(entity);
                state.object = Some((
                    entity.common().handle,
                    crate::t!(crate::entities::names::ui_name(entity)).into_owned(),
                ));
                state.object_prop = props
                    .iter()
                    .copied()
                    .find(|known| known.eq_ignore_ascii_case(&property))
                    .or_else(|| props.first().copied());
                state.object_props = props;
            }
        }
        self.fill_named_objects();
        self.refresh_field_preview();
        true
    }

    fn field_value(&self, code: &str, objects: &[codec::Handle]) -> String {
        crate::entities::field::evaluate(
            &self.tabs[self.active_tab].scene.document,
            code,
            objects,
            None,
        )
        .unwrap_or_else(|| "----".to_string())
    }

    fn refresh_field_preview(&mut self) {
        let Some(state) = self.field_dialog.as_ref() else {
            return;
        };
        let (code, objects) = state.code();
        let preview = if state.complete() {
            if state.name == "BlockPlaceholder" {
                // A placeholder shows its property name until a block reference resolves it.
                let (label, _, _) =
                    codec::fields::BLOCK_PLACEHOLDER_PROPERTIES[state.placeholder_property];
                text_case(label, state.text_case)
            } else {
                self.field_value(&code, &objects)
            }
        } else {
            String::new()
        };
        if let Some(state) = self.field_dialog.as_mut() {
            state.preview = preview;
        }
    }

    /// The names of the chosen named-object type, sorted.
    fn fill_named_objects(&mut self) {
        let Some(state) = self.field_dialog.as_mut() else {
            return;
        };
        let document = &self.tabs[self.active_tab].scene.document;
        let mut names: Vec<(String, codec::Handle)> = match NAMED_TYPES[state.named_type] {
            "Block" => document
                .block_records
                .iter()
                .filter(|b| !b.name.starts_with('*'))
                .map(|b| (b.name.clone(), b.handle))
                .collect(),
            "Dimstyle" => document
                .dim_styles
                .iter()
                .map(|s| (s.name.clone(), s.handle))
                .collect(),
            "Layer" => document
                .layers
                .iter()
                .map(|l| (l.name.clone(), l.handle))
                .collect(),
            "Linetype" => document
                .line_types
                .iter()
                .map(|l| (l.name.clone(), l.handle))
                .collect(),
            "Textstyle" => document
                .text_styles
                .iter()
                .filter(|s| !s.name.is_empty())
                .map(|s| (s.name.clone(), s.handle))
                .collect(),
            "View" => document
                .views
                .iter()
                .map(|v| (v.name.clone(), v.handle))
                .collect(),
            _ => Vec::new(),
        };
        names.sort_by_key(|(n, _)| n.to_lowercase());
        state.named_names = names;
        state.named = None;
    }

    pub(in crate::app) fn on_field_dialog(&mut self, m: FieldDialogMsg) -> Task<Message> {
        let i = self.active_tab;
        let Some(state) = self.field_dialog.as_mut() else {
            return Task::none();
        };
        match m {
            FieldDialogMsg::Category(c) => {
                state.category = c;
                let names = fields_of(c);
                let custom = state.name == crate::ui::window::field_dialog::CUSTOM_PROPERTY
                    && state.entries().iter().any(|row| row.1);
                if !names.contains(&state.name) && !custom {
                    state.name = names.first().copied().unwrap_or("Date");
                }
            }
            FieldDialogMsg::Name(n) => {
                state.name = n;
                // Sheet set fields start in Title case.
                state.text_case = if crate::ui::window::field_dialog::SHEET_SET_FIELDS.contains(&n) { 4 } else { 0 };
                state.ss_custom.clear();
                state.ss_node.clear();
                state.ss_prop.clear();
                // SheetSet starts on the set and its first property; SheetView on nothing.
                if n == "SheetSet" {
                    if let Some(db) = state.ss_sets.get(state.ss_set) {
                        state.ss_node = db.sheet_set().id().to_string();
                        state.ss_prop = nav_props(db, &state.ss_node).first().map(|p| p.0.clone()).unwrap_or_default();
                    }
                }
            }
            FieldDialogMsg::CustomProperty(property) => {
                state.name = crate::ui::window::field_dialog::CUSTOM_PROPERTY;
                state.custom_property = property;
                state.text_case = 0;
            }
            FieldDialogMsg::SsCustom(name) => state.ss_custom = name,
            FieldDialogMsg::SsSet(k) => {
                state.ss_set = k;
                state.ss_node.clear();
                state.ss_prop.clear();
            }
            FieldDialogMsg::SsNode(id) => {
                state.ss_prop = state
                    .ss_sets
                    .get(state.ss_set)
                    .and_then(|db| nav_props(db, &id).first().map(|p| p.0.clone()))
                    .unwrap_or_default();
                state.ss_node = id;
            }
            FieldDialogMsg::SsProp(p) => state.ss_prop = p,
            FieldDialogMsg::SsHref(v) => state.ss_href = v,
            FieldDialogMsg::SsBrowse => {
                return Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title(crate::t!("Open Sheet Set").as_ref())
                            .add_filter(crate::t!("Sheet Set (*.dst)").as_ref(), &["dst", "DST"])
                            .pick_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    |p| Message::FieldDialog(FieldDialogMsg::SsPicked(p)),
                );
            }
            FieldDialogMsg::SsPicked(None) => {}
            FieldDialogMsg::SsPicked(Some(path)) => match crate::app::commands::sheet_set::read_set(&path.to_string_lossy()) {
                Ok(db) => {
                    let key = codec::sheet_set::path_key(&path.to_string_lossy());
                    let open = state
                        .ss_sets
                        .iter()
                        .position(|d| d.path.as_deref().is_some_and(|p| codec::sheet_set::path_key(p) == key));
                    state.ss_set = match open {
                        Some(k) => k,
                        None => {
                            state.ss_sets.push(db);
                            state.ss_sets.len() - 1
                        }
                    };
                    state.ss_node.clear();
                    state.ss_prop.clear();
                }
                Err(e) => self.command_line.push_error(&format!("{}: {e}", path.display())),
            },
            FieldDialogMsg::SsPlaceholder(k) => {
                state.ss_placeholder = k;
                state.ss_custom.clear();
            }
            FieldDialogMsg::SsScaleFormat(k) => state.ss_scale_format = k,
            FieldDialogMsg::DateFormat(f) => state.date_format = f,
            FieldDialogMsg::DateExample(k) => {
                if let Some(f) = DATE_FORMATS.get(k) {
                    state.date_format = f.to_string();
                }
            }
            FieldDialogMsg::TextCase(k) => state.text_case = k,
            FieldDialogMsg::FileParts(b) => state.file_parts = b,
            FieldDialogMsg::FileExtension(v) => state.file_extension = v,
            FieldDialogMsg::SizeUnit(k) => state.size_unit = k,
            FieldDialogMsg::SysVar(v) => state.sysvar = v,
            FieldDialogMsg::Diesel(v) => state.diesel = v,
            FieldDialogMsg::NamedType(k) => {
                state.named_type = k;
                self.fill_named_objects();
            }
            FieldDialogMsg::Named(k) => state.named = Some(k),
            FieldDialogMsg::ObjectProp(p) => state.object_prop = Some(p),
            FieldDialogMsg::Formula(v) => state.formula = v,
            FieldDialogMsg::FormulaFormat(k) => state.formula_format = k,
            FieldDialogMsg::FormulaPrecision(k) => state.formula_precision = k,
            FieldDialogMsg::Conversion(v) => state.conversion = v,
            FieldDialogMsg::Prefix(v) => state.prefix = v,
            FieldDialogMsg::Suffix(v) => state.suffix = v,
            FieldDialogMsg::DecimalSeparator(k) => state.decimal_separator = k,
            FieldDialogMsg::ThousandsSeparator(k) => state.thousands_separator = k,
            FieldDialogMsg::Coordinates(k) => state.coordinates = k,
            FieldDialogMsg::Evaluate => {}
            FieldDialogMsg::HyperlinkText(v) => state.hyperlink_text = v,
            FieldDialogMsg::HyperlinkUrl(v) => state.hyperlink_url = v,
            FieldDialogMsg::BrowseHyperlink => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title(crate::t!("Select a file to link").as_ref())
                            .pick_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    |path| match path {
                        Some(path) => Message::FieldDialog(FieldDialogMsg::HyperlinkUrl(path.to_string_lossy().into_owned())),
                        None => Message::Noop,
                    },
                );
            }
            FieldDialogMsg::PlotScale(k) => state.plot_scale = k,
            FieldDialogMsg::CountExpression(v) => state.count_expression = v,
            FieldDialogMsg::ShowCountInstances => {
                // The dialog closes and count mode shows what the expression counts.
                let json = state.count_expression.clone();
                self.field_dialog = None;
                self.close_active_modal();
                self.show_count_instances(i, &json);
                return Task::none();
            }
            FieldDialogMsg::PlaceholderProperty(k) => {
                state.placeholder_property = k;
                state.text_case = 0;
            }
            FieldDialogMsg::TableFunction(function) => {
                // The dialog waits while the cells are picked.
                self.active_modal = None;
                let command = FieldTablePickCommand::new(function);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                return Task::none();
            }
            FieldDialogMsg::SelectObject => {
                // The dialog waits while one object is picked.
                self.active_modal = None;
                let command = FieldObjectPickCommand;
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                return Task::none();
            }
            FieldDialogMsg::Help => self.command_line.push_info(
                crate::t!("Inserts a field: text that updates from the drawing, the date or another source.").as_ref(),
            ),
            FieldDialogMsg::Ok => return self.field_dialog_ok(i),
        }
        self.refresh_field_preview();
        Task::none()
    }

    fn field_dialog_ok(&mut self, i: usize) -> Task<Message> {
        let Some(state) = self.field_dialog.take() else {
            return Task::none();
        };
        let (code, objects) = state.code();
        let value = state.preview.clone();
        let field = Some((code, objects));
        match state.target {
            FieldTarget::NewText => {
                self.close_active_modal();
                let defaults = crate::scene::creation_style::current_text_defaults(
                    &self.tabs[i].scene.document,
                );
                let command = FieldPlaceCommand::new(
                    value,
                    field.expect("set above"),
                    defaults.style_name,
                    defaults.height,
                );
                self.reset_command_start_state(i);
                self.command_line.push_info(&command.prompt());
                self.tabs[i].active_cmd = Some(Box::new(command));
                self.push_ucs_to_cmd(i);
                return self.focus_cmd_input();
            }
            FieldTarget::AttdefDefault => {
                if let Some(dialog) = self.attdef_dialog.as_mut() {
                    dialog.default = value;
                    dialog.field = field;
                }
                self.active_modal = Some(crate::app::ModalKind::AttDef);
            }
            FieldTarget::EditText(host) => {
                self.close_active_modal();
                self.push_undo_snapshot(i, "FIELD");
                crate::entities::field::set_text_field(
                    &mut self.tabs[i].scene.document,
                    host,
                    field,
                    &value,
                );
                self.tabs[i]
                    .scene
                    .bump_entities(&[(host, crate::scene::ChangeKind::Modified)]);
                self.tabs[i].dirty = true;
                self.refresh_properties();
                return Task::none();
            }
            FieldTarget::AttdefEdit => {
                if let Some(edit) = self.attdef_edit.as_mut() {
                    edit.default = value;
                    edit.field = field;
                    edit.field_changed = true;
                }
                self.active_modal = Some(crate::app::ModalKind::AttDefEdit);
            }
        }
        Task::none()
    }

    /// The dialogs a Field dialog was opened from come back when it closes.
    pub(in crate::app) fn close_field_dialog(&mut self) {
        let target = self.field_dialog.take().map(|s| s.target);
        self.close_active_modal();
        self.active_modal = match target {
            Some(FieldTarget::AttdefDefault) if self.attdef_dialog.is_some() => {
                Some(crate::app::ModalKind::AttDef)
            }
            Some(FieldTarget::AttdefEdit) if self.attdef_edit.is_some() => {
                Some(crate::app::ModalKind::AttDefEdit)
            }
            _ => None,
        };
    }
}

/// Spreadsheet column letters: A … Z, AA, AB …
fn column_letters(mut column: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push(b'A' + (column % 26) as u8);
        if column < 26 {
            break;
        }
        column = column / 26 - 1;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// The Field dialog's text cases: (none), Uppercase, Lowercase, First capital, Title case.
fn text_case(value: &str, case: usize) -> String {
    match case {
        1 => value.to_uppercase(),
        2 => value.to_lowercase(),
        3 => {
            let lower = value.to_lowercase();
            let mut chars = lower.chars();
            chars
                .next()
                .map(|c| c.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        }
        4 => value
            .split(' ')
            .map(|word| {
                let lower = word.to_lowercase();
                let mut chars = lower.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().chain(chars).collect::<String>())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::app::OpenCADStudio;
    use crate::ui::window::field_dialog::{FieldDialogMsg, FieldDialogState, FieldTarget};
    use codec::entities::{EntityType, LwPolyline, MText};

    fn app_with_custom_properties() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.summary_info.custom_properties = vec![
            ("Zeta".into(), "last?".into()),
            ("Client".into(), "Acme".into()),
            ("Job No".into(), "42-7".into()),
        ];
        app
    }

    fn document_category() -> usize {
        crate::ui::window::field_dialog::CATEGORIES
            .iter()
            .position(|(name, _)| *name == "Document")
            .unwrap()
    }

    /// S6: the Document category lists the custom properties, in stored
    /// order, after its own fields.
    #[test]
    fn the_document_category_lists_the_custom_properties_in_order() {
        let mut app = app_with_custom_properties();
        app.open_field_dialog(FieldTarget::NewText);
        let _ = app.on_field_dialog(FieldDialogMsg::Category(document_category()));
        let state = app.field_dialog.as_ref().unwrap();
        let labels: Vec<String> = state.entries().into_iter().map(|row| row.0).collect();
        let own = crate::ui::window::field_dialog::fields_of(document_category()).len();
        assert_eq!(&labels[own..], ["Zeta", "Client", "Job No"]);
        assert_eq!(labels.len(), own + 3);
        // Other categories do not list them.
        let _ = app.on_field_dialog(FieldDialogMsg::Category(0));
        let state = app.field_dialog.as_ref().unwrap();
        assert!(state.entries().iter().all(|row| row.0 != "Client"));
    }

    /// S6: choosing a property writes exactly the stored field code.
    #[test]
    fn choosing_a_custom_property_inserts_its_field_code() {
        let mut app = app_with_custom_properties();
        app.open_field_dialog(FieldTarget::NewText);
        let _ = app.on_field_dialog(FieldDialogMsg::Category(document_category()));
        let pick = app
            .field_dialog
            .as_ref()
            .unwrap()
            .entries()
            .into_iter()
            .find(|row| row.0 == "Job No")
            .expect("the property is listed");
        let _ = app.on_field_dialog(pick.2);
        let state = app.field_dialog.as_ref().unwrap();
        assert!(state.complete());
        let (code, objects) = state.code();
        assert_eq!(format!("%<{code}>%"), r"%<\AcVar CustomDP.Job No>%");
        assert!(objects.is_empty());
        assert_eq!(state.preview, "42-7");
    }

    /// S6: a field opened from the drawing selects its property again.
    #[test]
    fn a_custom_property_field_reopens_on_its_property() {
        let mut state = FieldDialogState::new(FieldTarget::NewText);
        state.seed_from_code(r"\AcVar CustomDP.Job No", &[]);
        assert_eq!(state.name, "CustomDP");
        assert_eq!(state.custom_property, "Job No");
        assert_eq!(state.code().0, r"\AcVar CustomDP.Job No");
    }

    #[test]
    fn a_field_text_opens_its_field_and_ok_changes_it_in_place() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut square = LwPolyline::new();
        for (x, y) in [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)] {
            square.add_point(codec::types::Vector2::new(x, y));
        }
        square.is_closed = true;
        let document = &mut app.tabs[i].scene.document;
        let polyline = document.add_entity(EntityType::LwPolyline(square)).unwrap();
        let text = document.add_entity(EntityType::MText(Box::new(MText::new()))).unwrap();
        crate::entities::field::set_text_field(
            document,
            text,
            Some((
                "\\AcObjProp Object(%<\\_ObjIdx 0>%).Area \\f \"%lu2%pr1\"".to_string(),
                vec![polyline],
            )),
            "4.0",
        );

        let _ = app.begin_text_edit(text);
        let state = app.field_dialog.as_ref().expect("the Field dialog opens");
        assert_eq!(state.target, FieldTarget::EditText(text));
        assert_eq!((state.name, state.object_prop), ("Object", Some("Area")));
        assert_eq!((state.formula_format, state.formula_precision), (2, 2));

        let _ = app.on_field_dialog(FieldDialogMsg::FormulaPrecision(4));
        let _ = app.on_field_dialog(FieldDialogMsg::Ok);
        let (code, objects) =
            crate::entities::field::whole_text_field(&app.tabs[i].scene.document, text).unwrap();
        assert!(code.ends_with("\\f \"%lu2%pr3\""), "{code}");
        assert_eq!(objects, vec![polyline]);
        let Some(EntityType::MText(shown)) = app.tabs[i].scene.document.get_entity(text) else {
            panic!("the text stays");
        };
        assert_eq!(shown.value, "4.000");
    }

    #[test]
    fn ok_on_an_unchanged_level_mark_keeps_its_format() {
        use codec::entities::Line;
        use codec::types::Vector3;
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let document = &mut app.tabs[i].scene.document;
        let line = document
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 3.5, 0.0),
            )))
            .unwrap();
        let text = document
            .add_entity(EntityType::MText(Box::new(MText::new())))
            .unwrap();
        let code = "\\AcObjProp.16.2 Object(%<\\_ObjIdx 0>%).EndPoint \\f \"%lu2%pt2%pr2%ps[+,]\"";
        crate::entities::field::set_text_field(
            document,
            text,
            Some((code.to_string(), vec![line])),
            "+3.50",
        );

        let _ = app.begin_text_edit(text);
        assert!(app.field_dialog.is_some(), "the Field dialog opens");
        let _ = app.on_field_dialog(FieldDialogMsg::Ok);
        let (written, objects) =
            crate::entities::field::whole_text_field(&app.tabs[i].scene.document, text).unwrap();
        assert_eq!(written, code);
        assert_eq!(objects, vec![line]);
        let Some(EntityType::MText(shown)) = app.tabs[i].scene.document.get_entity(text) else {
            panic!("the text stays");
        };
        assert_eq!(shown.value, "+3.50");
    }

    /// A text showing `code` for `object`, opened in the Field dialog.
    fn editing(code: &str, object: EntityType) -> (OpenCADStudio, codec::Handle, codec::Handle) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let document = &mut app.tabs[i].scene.document;
        let object = document.add_entity(object).unwrap();
        let text = document
            .add_entity(EntityType::MText(Box::new(MText::new())))
            .unwrap();
        crate::entities::field::set_text_field(
            document,
            text,
            Some((code.to_string(), vec![object])),
            "",
        );
        let _ = app.begin_text_edit(text);
        assert!(app.field_dialog.is_some(), "the Field dialog opens");
        (app, text, object)
    }

    fn written(app: &OpenCADStudio, text: codec::Handle) -> (String, String) {
        let document = &app.tabs[app.active_tab].scene.document;
        let (code, _) = crate::entities::field::whole_text_field(document, text).unwrap();
        let Some(EntityType::MText(shown)) = document.get_entity(text) else {
            panic!("the text stays");
        };
        (code, shown.value.clone())
    }

    #[test]
    fn an_area_in_square_centimetres_shows_in_square_metres_with_a_comma() {
        let mut room = LwPolyline::new();
        for (x, y) in [(0.0, 0.0), (500.0, 0.0), (500.0, 300.0), (0.0, 300.0)] {
            room.add_point(codec::types::Vector2::new(x, y));
        }
        room.is_closed = true;
        let code = "\\AcObjProp Object(%<\\_ObjIdx 0>%).Area \\f \"%lu2%pr2\"";
        let (mut app, text, _) = editing(code, EntityType::LwPolyline(room));

        let _ = app.on_field_dialog(FieldDialogMsg::Conversion("0.0001".into()));
        let _ = app.on_field_dialog(FieldDialogMsg::Suffix(" m²".into()));
        let _ = app.on_field_dialog(FieldDialogMsg::DecimalSeparator(1));
        assert_eq!(app.field_dialog.as_ref().unwrap().preview, "15,00 m²");
        let _ = app.on_field_dialog(FieldDialogMsg::Ok);

        let (code, shown) = written(&app, text);
        assert_eq!(
            code,
            "\\AcObjProp Object(%<\\_ObjIdx 0>%).Area \\f \"%lu2%pr2%ds44%ct8[0.0001]%ps[, m²]\""
        );
        assert_eq!(shown, "15,00 m²");
    }

    #[test]
    fn a_level_mark_shows_the_height_with_a_plus_sign() {
        use codec::entities::Line;
        use codec::types::Vector3;
        let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 3.5, 0.0));
        let code = "\\AcObjProp.16.2 Object(%<\\_ObjIdx 0>%).EndPoint \\f \"%lu2%pr2\"";
        let (mut app, text, _) = editing(code, EntityType::Line(line));
        assert!(app.field_dialog.as_ref().unwrap().preview.contains(", "));

        let _ = app.on_field_dialog(FieldDialogMsg::Coordinates(3));
        let _ = app.on_field_dialog(FieldDialogMsg::Prefix("+".into()));
        let _ = app.on_field_dialog(FieldDialogMsg::Ok);

        let (code, shown) = written(&app, text);
        assert_eq!(
            code,
            "\\AcObjProp.16.2 Object(%<\\_ObjIdx 0>%).EndPoint \\f \"%lu2%pt2%pr2%ps[+,]\""
        );
        assert_eq!(shown, "+3.50");
    }

    #[test]
    fn a_block_insertion_point_field_reopens_unchanged() {
        use codec::entities::Insert;
        use codec::types::Vector3;
        let door = Insert::new("DOOR", Vector3::new(4.0, 7.5, 0.0));
        // The height of a block's insertion point, in the WCS.
        let code = "\\AcObjProp Object(%<\\_ObjIdx 0>%).InsertionPoint \\f \"%lu2%pt2%pr2\"";
        let (mut app, text, _) = editing(code, EntityType::Insert(door));
        let state = app.field_dialog.as_ref().unwrap();
        assert_eq!(state.object_prop, Some("InsertionPoint"));

        let _ = app.on_field_dialog(FieldDialogMsg::Ok);
        assert_eq!(written(&app, text), (code.to_string(), "7.50".to_string()));
    }

    #[test]
    fn a_text_contents_field_reopens_unchanged() {
        let mut room = MText::new();
        room.value = "Salon".into();
        // Another text's contents, in upper case.
        let code = "\\AcObjProp Object(%<\\_ObjIdx 0>%).TextString \\f \"%tc1\"";
        let (mut app, text, _) = editing(code, EntityType::MText(Box::new(room)));
        let state = app.field_dialog.as_ref().unwrap();
        assert_eq!(state.object_prop, Some("TextString"));

        let _ = app.on_field_dialog(FieldDialogMsg::Ok);
        assert_eq!(written(&app, text), (code.to_string(), "SALON".to_string()));
    }

    #[test]
    fn a_reopened_wcs_point_field_stays_in_the_wcs() {
        use codec::entities::Line;
        use codec::types::Vector3;
        let line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 3.5, 0.0));
        let code = "\\AcObjProp Object(%<\\_ObjIdx 0>%).EndPoint \\f \"%lu2%pt2%pr2\"";
        let (mut app, text, _) = editing(code, EntityType::Line(line));

        let _ = app.on_field_dialog(FieldDialogMsg::Ok);
        assert_eq!(written(&app, text).0, code);
    }
}
