//! The table style window's cell style cards (New Cell Style, Manage Cell
//! Styles), its Borders page (line properties applied to edge sets with
//! the border buttons) and the Format / Text style "…" windows.

use crate::app::{Message, ModalKind, OpenCADStudio};
use crate::ui::style::mleaderstyle::{LoadDraft, ReloadAsk};
use crate::ui::style::tablestyle::{
    cell_style_label, sorted_cell_styles, CreateCell, ManageCells, TsCellMsg,
    BORDER_BUTTONS, OTHER_LINETYPE,
};
use crate::ui::style::textstyle::{name_notice, NameNotice};
use document::objects::{TableBorderType, TableStyle, TABLE_GRID_EDGES};
use iced::Task;

/// Why `name` cannot name a cell style of `style` (`except` is the one
/// being renamed). A shown name (Title, Header, Data) counts as taken.
fn cell_name_notice(style: &TableStyle, name: &str, except: Option<&str>) -> Option<NameNotice> {
    if name.trim().is_empty() {
        return Some(NameNotice::Empty);
    }
    name_notice(name, |typed| {
        style.cell_styles().any(|cell| {
            except.is_none_or(|except| !cell.name.eq_ignore_ascii_case(except))
                && (cell.name.eq_ignore_ascii_case(typed)
                    || cell_style_label(&cell.name).eq_ignore_ascii_case(typed)
                    || row_english_name(&cell.name).is_some_and(|n| n.eq_ignore_ascii_case(typed)))
        })
    })
}

fn row_english_name(name: &str) -> Option<&'static str> {
    match name {
        document::objects::TITLE_CELL_STYLE => Some("Title"),
        document::objects::HEADER_CELL_STYLE => Some("Header"),
        document::objects::DATA_CELL_STYLE => Some("Data"),
        _ => None,
    }
}

/// "Copy of <source>", then "Copy(2) of <source>"… until it is free.
fn copy_name(style: &TableStyle, source: &str) -> String {
    let shown = cell_style_label(source).into_owned();
    let mut name = crate::i18n::plain(crate::tr!("table-style-cells", "copy-of", name = shown.clone()));
    let mut n = 2;
    while cell_name_notice(style, &name, None).is_some() {
        name = crate::i18n::plain(crate::tr!(
            "table-style-cells",
            "copy-n-of",
            n = n,
            name = shown.clone()
        ));
        n += 1;
    }
    name
}

impl OpenCADStudio {
    fn ts_style(&self) -> Option<&TableStyle> {
        use document::objects::ObjectType;
        self.tabs[self.active_tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::TableStyle(style) if style.name == self.tablestyle_selected => {
                    Some(style)
                }
                _ => None,
            })
    }

    pub(super) fn on_table_style_cells(&mut self, message: TsCellMsg) -> Task<Message> {
        let i = self.active_tab;
        self.ts_color_open = None;
        match message {
            TsCellMsg::CreateOpen => return self.ts_create_open(false),
            TsCellMsg::ManageNew => return self.ts_create_open(true),
            TsCellMsg::CreateName(name) => {
                if let Some(create) = self.ts_cards.create.as_mut() {
                    create.name = name;
                }
            }
            TsCellMsg::CreateStart(start) => {
                if let Some(create) = self.ts_cards.create.as_mut() {
                    create.start = start;
                }
            }
            TsCellMsg::CreateCancel => self.ts_cards.create = None,
            TsCellMsg::CreateOk => return self.ts_create_ok(),
            TsCellMsg::NoticeClose => {
                if let Some(create) = self.ts_cards.create.as_mut() {
                    create.notice = None;
                } else if let Some(manage) = self.ts_cards.manage.as_mut() {
                    manage.notice = None;
                }
            }
            TsCellMsg::ManageOpen => {
                let Some(style) = self.ts_style() else {
                    return Task::none();
                };
                self.ts_cards.manage = Some(ManageCells {
                    before: style.modern_overrides.clone(),
                    selected: self.ts_cell.clone(),
                    rename: None,
                    notice: None,
                    delete_ask: false,
                    in_use: false,
                });
            }
            TsCellMsg::ManageSelect(name) => {
                if let Some(manage) = self.ts_cards.manage.as_mut() {
                    manage.selected = name;
                    manage.rename = None;
                }
            }
            TsCellMsg::RenameStart => {
                if let Some(manage) = self.ts_cards.manage.as_mut() {
                    if !document::objects::is_row_cell_style(&manage.selected) {
                        manage.rename = Some(manage.selected.clone());
                        return iced::widget::operation::focus(iced::widget::Id::new(
                            crate::ui::style::tablestyle::RENAME_ID,
                        ));
                    }
                }
            }
            TsCellMsg::RenameText(value) => {
                // Typing in the name field renames in place.
                if let Some(manage) = self.ts_cards.manage.as_mut() {
                    if !document::objects::is_row_cell_style(&manage.selected) {
                        manage.rename = Some(value);
                    }
                }
            }
            TsCellMsg::RenameCommit => self.ts_rename_commit(),
            TsCellMsg::DeleteAsk => {
                if let Some(manage) = self.ts_cards.manage.as_mut() {
                    if !document::objects::is_row_cell_style(&manage.selected) {
                        manage.delete_ask = true;
                    }
                }
            }
            TsCellMsg::DeleteAnswer(yes) => self.ts_delete_answer(yes),
            TsCellMsg::ManageOk => {
                // A name being typed is committed first; a refused one
                // keeps the card open with its notice.
                self.ts_rename_commit();
                if self
                    .ts_cards
                    .manage
                    .as_ref()
                    .is_some_and(|manage| manage.notice.is_some())
                {
                    return Task::none();
                }
                self.ts_cards.manage = None;
                let exists = self
                    .ts_style()
                    .is_some_and(|style| style.cell_style(&self.ts_cell).is_some());
                if !exists {
                    self.ts_cell = document::objects::DATA_CELL_STYLE.to_string();
                }
                self.load_tablestyle_bufs(i);
            }
            TsCellMsg::ManageCancel => {
                if let Some(manage) = self.ts_cards.manage.take() {
                    if let Some(style) = self.tablestyle_mut(i) {
                        style.modern_overrides = manage.before;
                        style.sync_rows_from_cell_styles();
                    }
                    self.load_tablestyle_bufs(i);
                }
            }
            TsCellMsg::ManageHelp => self
                .command_line
                .push_info(crate::tr!("table-style-cells", "manage-help").as_str()),
            TsCellMsg::QuickRename | TsCellMsg::QuickDelete => {
                if document::objects::is_row_cell_style(&self.ts_cell) {
                    return Task::none();
                }
                let rename = matches!(message, TsCellMsg::QuickRename);
                let _ = self.on_table_style_cells(TsCellMsg::ManageOpen);
                return self.on_table_style_cells(if rename {
                    TsCellMsg::RenameStart
                } else {
                    TsCellMsg::DeleteAsk
                });
            }
            TsCellMsg::FormatOpen => self.ts_format_open(),
            TsCellMsg::TextStyles => return self.ts_open_text_styles(),
            TsCellMsg::BorderLineweight(value) => {
                if let Ok(weight) = value.parse() {
                    self.ts_border.line_weight = weight;
                }
            }
            TsCellMsg::BorderLinetype(code) => {
                if code == OTHER_LINETYPE {
                    let name = self.ts_border_linetype_name();
                    self.ts_cards.linetype = Some(name);
                } else {
                    self.ts_border.linetype = code;
                }
            }
            TsCellMsg::BorderColor(color) => self.ts_border.color = color,
            TsCellMsg::BorderDouble(on) => self.ts_border.double = on,
            TsCellMsg::BorderSpacing(value) => self.ts_border.spacing = value,
            TsCellMsg::BorderApply(index) => self.ts_border_apply(index as usize),
            TsCellMsg::LinetypePick(name) => {
                if let Some(selected) = self.ts_cards.linetype.as_mut() {
                    *selected = name;
                }
            }
            TsCellMsg::LinetypeCancel => self.ts_cards.linetype = None,
            TsCellMsg::LinetypeOk => {
                if let Some(name) = self.ts_cards.linetype.take() {
                    if let Some(code) = self.ts_linetype_code(&name) {
                        self.ts_border.linetype = code;
                    }
                }
            }
            // Load… lists the program's linetypes first; File… picks a
            // `.lin` file instead.
            TsCellMsg::LinetypeLoad => {
                let source = crate::io::linetypes::catalog_source();
                self.ts_cards.load = Some(LoadDraft {
                    file: String::new(),
                    dir: None,
                    entries: crate::io::linetypes::source_entries(&source),
                    source,
                    picked: Vec::new(),
                });
            }
            TsCellMsg::LoadFile => {
                return Task::perform(crate::io::pick_linetype_file_named(), |picked| {
                    Message::TableStyleCells(TsCellMsg::LoadFileChosen(picked))
                });
            }
            TsCellMsg::LoadFileChosen(Some(file)) => {
                if let Some(load) = self.ts_cards.load.as_mut() {
                    load.entries = crate::io::linetypes::source_entries(&file.source);
                    load.file = file.name;
                    load.source = file.source;
                    load.dir = file.dir;
                    load.picked.clear();
                }
            }
            TsCellMsg::LoadFileChosen(None) => {}
            TsCellMsg::LoadToggle(name) => {
                if let Some(load) = self.ts_cards.load.as_mut() {
                    if let Some(at) = load.picked.iter().position(|p| p.eq_ignore_ascii_case(&name))
                    {
                        load.picked.remove(at);
                    } else {
                        load.picked.push(name);
                    }
                }
            }
            TsCellMsg::LoadCancel => self.ts_cards.load = None,
            // OK asks about the ticked linetypes the drawing already has,
            // one by one, before loading anything.
            TsCellMsg::LoadOk => {
                if let Some(load) = self.ts_cards.load.as_ref() {
                    match ReloadAsk::for_load(load, &self.tabs[i].scene.document) {
                        Some(ask) => self.ts_cards.reload_ask = Some(ask),
                        None => self.ts_finish_load(Vec::new()),
                    }
                }
            }
            TsCellMsg::ReloadAnswer(answer) => {
                if let Some(mut ask) = self.ts_cards.reload_ask.take() {
                    match ask.answer(answer) {
                        Some(Ok(reload)) => self.ts_finish_load(reload),
                        Some(Err(())) => self.ts_cards.load = None,
                        None => self.ts_cards.reload_ask = Some(ask),
                    }
                }
            }
        }
        Task::none()
    }

    /// New Cell Style: named a copy of the selected cell style (the edited
    /// one, or Manage's selection), which it starts with.
    fn ts_create_open(&mut self, from_manage: bool) -> Task<Message> {
        let source = match (&self.ts_cards.manage, from_manage) {
            (Some(manage), true) => manage.selected.clone(),
            _ => self.ts_cell.clone(),
        };
        let Some(style) = self.ts_style() else {
            return Task::none();
        };
        let name = copy_name(style, &source);
        self.ts_cards.create = Some(CreateCell {
            name,
            start: source,
            notice: None,
            from_manage,
        });
        iced::widget::operation::focus(iced::widget::Id::new(
            crate::ui::style::tablestyle::CREATE_NAME_ID,
        ))
    }

    /// Continue: the cell style is made, or the name is refused.
    fn ts_create_ok(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let Some(create) = self.ts_cards.create.clone() else {
            return Task::none();
        };
        let Some(style) = self.ts_style() else {
            return Task::none();
        };
        if let Some(notice) = cell_name_notice(style, &create.name, None) {
            if let Some(card) = self.ts_cards.create.as_mut() {
                card.notice = Some(notice);
            }
            return Task::none();
        }
        let name = create.name.trim().to_string();
        if let Some(style) = self.tablestyle_mut(i) {
            style.create_cell_style(&name, Some(&create.start));
            style.sync_rows_from_cell_styles();
        }
        self.ts_cards.create = None;
        match self.ts_cards.manage.as_mut().filter(|_| create.from_manage) {
            // Manage selects it; the edited cell style stays until OK.
            Some(manage) => manage.selected = name,
            None => {
                self.ts_cell = name;
                self.load_tablestyle_bufs(i);
            }
        }
        Task::none()
    }

    /// The inline rename ends: an empty name keeps the old one, a taken or
    /// unusable one is refused.
    fn ts_rename_commit(&mut self) {
        let i = self.active_tab;
        let Some((old, typed)) = self
            .ts_cards
            .manage
            .as_mut()
            .and_then(|m| m.rename.take().map(|typed| (m.selected.clone(), typed)))
        else {
            return;
        };
        if typed.trim().is_empty() || typed.trim() == old {
            return;
        }
        let notice = self
            .ts_style()
            .and_then(|style| cell_name_notice(style, &typed, Some(&old)));
        if let Some(notice) = notice {
            if let Some(manage) = self.ts_cards.manage.as_mut() {
                manage.notice = Some(notice);
            }
            return;
        }
        let new_name = typed.trim().to_string();
        let renamed = self
            .tablestyle_mut(i)
            .is_some_and(|style| style.rename_cell_style(&old, &new_name));
        if renamed {
            if self.ts_cell.eq_ignore_ascii_case(&old) {
                self.ts_cell = new_name.clone();
            }
            if let Some(manage) = self.ts_cards.manage.as_mut() {
                manage.selected = new_name;
            }
        }
    }

    /// Delete's question answered: a cell style a table of the style uses
    /// stays; otherwise it goes and the one above it is selected.
    fn ts_delete_answer(&mut self, yes: bool) {
        let i = self.active_tab;
        let Some(manage) = self.ts_cards.manage.as_mut() else {
            return;
        };
        let was_asked = std::mem::take(&mut manage.delete_ask);
        manage.in_use = false;
        if !yes || !was_asked {
            return;
        }
        let name = manage.selected.clone();
        if self.table_cell_style_in_use(i, &name) {
            if let Some(manage) = self.ts_cards.manage.as_mut() {
                manage.in_use = true;
            }
            return;
        }
        let above = self.ts_style().and_then(|style| {
            let list = sorted_cell_styles(style);
            let at = list
                .iter()
                .position(|cell| cell.name.eq_ignore_ascii_case(&name))?;
            list.get(at.saturating_sub(1))
                .filter(|_| at > 0)
                .or_else(|| list.get(at + 1))
                .map(|cell| cell.name.clone())
        });
        if let Some(style) = self.tablestyle_mut(i) {
            if style.delete_cell_style(&name) {
                style.sync_rows_from_cell_styles();
            }
        }
        if let (Some(manage), Some(above)) = (self.ts_cards.manage.as_mut(), above) {
            manage.selected = above;
        }
    }

    /// A border button: the line properties go on its edges, which become
    /// visible; No Border hides every edge and keeps their properties.
    fn ts_border_apply(&mut self, index: usize) {
        let Some(&(_, mask)) = BORDER_BUTTONS.get(index) else {
            return;
        };
        let hide = index + 1 == BORDER_BUTTONS.len();
        self.ts_border.last = Some(index as u8);
        let border = self.ts_border.clone();
        let linetype = u64::from_str_radix(border.linetype.trim(), 16)
            .map(document::Handle::new)
            .unwrap_or(document::Handle::NULL);
        let spacing = crate::entities::common::parse_f64(&border.spacing).filter(|v| *v > 0.0);
        self.edit_table_cell_style(|cell| {
            for edge in TABLE_GRID_EDGES.iter().copied().filter(|edge| mask & edge != 0) {
                let data = &mut cell.cell_style;
                if hide {
                    data.set_edge_hidden(edge, true);
                    continue;
                }
                let grid = data.grid_mut(edge);
                grid.line_type = linetype;
                grid.border.line_weight = document::types::LineWeight::from_value(border.line_weight);
                grid.border.color = border.color;
                grid.border.border_type = if border.double {
                    TableBorderType::Double
                } else {
                    TableBorderType::Single
                };
                if let Some(spacing) = spacing {
                    grid.border.double_line_spacing = spacing;
                }
                data.set_edge_hidden(edge, false);
            }
        });
    }

    /// The border linetype's name, for Select Linetype.
    fn ts_border_linetype_name(&self) -> String {
        let code = &self.ts_border.linetype;
        if code == "0" {
            return "ByBlock".into();
        }
        let handle = u64::from_str_radix(code, 16).unwrap_or(0);
        self.tabs[self.active_tab]
            .scene
            .document
            .line_types
            .iter()
            .find(|lt| lt.handle.value() == handle)
            .map_or_else(|| "ByBlock".into(), |lt| lt.name.clone())
    }

    /// A linetype name as the border linetype code.
    fn ts_linetype_code(&self, name: &str) -> Option<String> {
        if name.eq_ignore_ascii_case("ByBlock") {
            return Some("0".into());
        }
        self.tabs[self.active_tab]
            .scene
            .document
            .line_types
            .iter()
            .find(|lt| lt.name.eq_ignore_ascii_case(name))
            .map(|lt| format!("{:X}", lt.handle.value()))
    }

    /// Load Linetypes ends: the ticked linetypes the drawing lacks are
    /// added, `reload` are read again; Select Linetype keeps its choice.
    fn ts_finish_load(&mut self, reload: Vec<String>) {
        let Some(load) = self.ts_cards.load.take() else {
            return;
        };
        let doc = &mut self.tabs[self.active_tab].scene.document;
        crate::io::linetypes::populate_document_named(
            doc,
            &load.source,
            load.dir.as_deref(),
            &load.picked,
        );
        crate::io::linetypes::reload_document_named(doc, &load.source, load.dir.as_deref(), &reload);
    }

    /// Format "…": the Cell Data Format window on the cell style's data
    /// type and format; it returns here.
    fn ts_format_open(&mut self) {
        let Some(cell) = self
            .ts_style()
            .and_then(|style| style.cell_style(&self.ts_cell))
            .cloned()
        else {
            return;
        };
        let format = &cell.cell_style.content_format;
        let mut state = cell_format_state(format);
        state.preview = crate::app::commands::table_cells::format_preview(
            &self.tabs[self.active_tab].scene.document,
            &state,
        );
        self.table_cell_format = Some(state);
        self.ts_format_return = true;
        self.active_modal = Some(ModalKind::TableCellFormat);
        self.reset_modal_geometry();
    }

    /// The Cell Data Format window closes: OK (`state`) writes the data
    /// type and format into the cell style; the table style window comes
    /// back either way.
    pub(in crate::app) fn table_style_format_back(
        &mut self,
        state: Option<crate::ui::window::table_cell_dialog::CellFormatState>,
    ) {
        use crate::ui::window::table_cell_dialog::FORMAT_KINDS;
        self.ts_format_return = false;
        self.table_cell_format = None;
        if let Some(state) = state {
            let picture = state.picture();
            let category = FORMAT_KINDS[state.kind.min(8)];
            if let Some(&(_, data_type, unit_type)) = crate::entities::table::CELL_DATA_CATEGORIES
                .iter()
                .find(|(name, _, _)| *name == category)
            {
                self.edit_table_cell_style(|cell| {
                    let format = &mut cell.cell_style.content_format;
                    format.value_data_type = data_type;
                    format.value_unit_type = unit_type;
                    format.value_format_string = picture;
                });
            }
        }
        self.active_modal = Some(ModalKind::TableStyle);
        self.reset_modal_geometry();
    }

    /// Text style "…": the Text Style manager opens on the cell style's
    /// text style, made current, as a child of this window's transaction.
    fn ts_open_text_styles(&mut self) -> Task<Message> {
        let i = self.active_tab;
        let doc = &self.tabs[i].scene.document;
        let name = self
            .ts_style()
            .and_then(|style| style.cell_style(&self.ts_cell))
            .map(|cell| cell.cell_style.content_format.text_style)
            .and_then(|handle| doc.text_styles.iter().find(|t| t.handle == handle))
            .map(|t| t.name.clone());
        if let Some(name) = name {
            self.tabs[i].scene.document.header.current_text_style_name = name;
            self.sync_ribbon_styles();
        }
        let parent = self.style_stage.take();
        let task = self.on_text_style_dialog_open();
        self.style_parent_stage = parent;
        self.ts_text_return = true;
        task
    }

    /// Back from the Text Style manager: the cell style takes the text
    /// style current when it closed.
    pub(in crate::app) fn table_style_back_from_text_styles(&mut self) {
        let i = self.active_tab;
        self.ts_text_return = false;
        self.style_stage_child_close();
        let doc = &self.tabs[i].scene.document;
        let current = &doc.header.current_text_style_name;
        let handle = doc
            .text_styles
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(current))
            .map(|t| t.handle);
        if let Some(handle) = handle {
            self.edit_table_cell_style(|cell| {
                cell.cell_style.content_format.text_style = handle;
            });
        }
        self.active_modal = Some(ModalKind::TableStyle);
        self.reset_modal_geometry();
    }
}

/// The Cell Data Format settings a cell style's data type and format read
/// as.
pub(in crate::app) fn cell_format_state(
    format: &document::objects::TableContentFormat,
) -> crate::ui::window::table_cell_dialog::CellFormatState {
    use crate::ui::window::table_cell_dialog::{CellFormatState, FORMAT_KINDS};
    let category =
        crate::entities::table::cell_data_category(format.value_data_type, format.value_unit_type);
    let kind = FORMAT_KINDS
        .iter()
        .position(|name| *name == category)
        .unwrap_or(4);
    let mut state = CellFormatState {
        kind,
        unit: 0,
        precision: if matches!(kind, 1 | 5) { 3 } else { 0 },
        symbol: crate::entities::field::os_currency_symbol(),
        append: kind == 5,
        negative_parens: false,
        separator: 0,
        axes: [true; 3],
        date: String::new(),
        sample: None,
        preview: String::new(),
        conversion: "1".into(),
        prefix: String::new(),
        suffix: String::new(),
        decimal: 0,
        thousands: 0,
        zeros: 0,
    };
    state.read_picture(&format.value_format_string);
    state
}
