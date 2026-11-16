//! The editor's right-click menu and the tag under text AutoStack or an
//! automatic list just changed. Submenus open in place under their row.
use super::*;
use crate::app::mtext_editor::context_menu::{ContextGroup as G, SmartTag};

macro_rules! m {
    ($id:literal) => {
        crate::tr!("text-editor-menu", $id)
    };
}

const BOLT: &[u8] = include_bytes!("../../../../../assets/icons/ui/bolt.svg");
const WIDTH: f32 = 300.0;

fn run(command: &str) -> Message {
    Message::MTextContextRun(command.to_string())
}

/// A row: its mark, label and shortcut hint; greyed when `command` is None.
fn line(
    mark: impl Into<Mark>,
    name: String,
    hint: &str,
    command: Option<&str>,
    indent: bool,
) -> Element<'static, Message> {
    button(
        row![
            Space::new().width(if indent { km::MENU_CHECK_W } else { 0.0 }),
            mark_cell(mark.into()),
            text(name)
                .size(ui_text::UI)
                .line_height(ui_text::line())
                .width(Fill)
                .wrapping(iced::widget::text::Wrapping::None),
            text(hint.to_string())
                .size(ui_text::UI)
                .line_height(ui_text::line())
                .style(|theme: &Theme| text::Style {
                    color: Some(dim(theme)),
                }),
        ]
        .align_y(iced::Alignment::Center)
        .height(Fill),
    )
    .on_press_maybe(command.map(run))
    .padding(iced::Padding::ZERO.right(km::MENU_PAD_X))
    .width(Fill)
    .height(ROW_H)
    .style(item_style)
    .into()
}

fn group_header(name: String, group: G, open: bool) -> Element<'static, Message> {
    button(
        row![
            mark_cell(Mark::Text("")),
            text(name).size(ui_text::UI).width(Fill),
            text(if open { "▾" } else { "▸" }).size(ui_text::UI),
        ]
        .align_y(iced::Alignment::Center)
        .height(Fill),
    )
    .on_press(Message::MTextContextGroup(Some(group)))
    .padding(iced::Padding::ZERO.right(km::MENU_PAD_X))
    .width(Fill)
    .height(ROW_H)
    .style(item_style)
    .into()
}

/// One submenu: its header, and its rows indented under it while open.
fn group(
    panel: Panel,
    open: Option<G>,
    name: String,
    group: G,
    rows: impl FnOnce() -> Vec<Element<'static, Message>>,
) -> Panel {
    let is_open = open == Some(group);
    let mut panel = panel.push(group_header(name, group, is_open));
    if is_open {
        for row in rows() {
            panel = panel.push(row);
        }
    }
    panel
}

fn separator() -> Element<'static, Message> {
    crate::ui::kit::menu::entry(crate::ui::kit::menu::Item::Separator)
}

/// The right-click menu, when open.
pub(in crate::app::view::overlay) fn context_menu(
    ed: &MTextEditorState,
) -> Option<(iced::Point, Element<'static, Message>)> {
    let menu = ed.context_menu.as_ref()?;
    let p = crate::app::mtext_prefs::prefs();
    let open = menu.open;
    let has_selection = ed.sel.is_some_and(|(a, b)| a != b);
    let lists = p.allow_lists;
    let mut panel = Panel::new(m!("title"), WIDTH);
    if menu.word.is_some() {
        if menu.suggestions.is_empty() {
            panel = panel.push(line("", m!("no-suggestions"), "", None, false));
        }
        for word in menu.suggestions.iter().take(3) {
            panel = panel.push(line("", word.clone(), "", Some(&format!("_MTED SPELLREPLACE {word}")), false));
        }
        if menu.suggestions.len() > 3 {
            let more: Vec<String> = menu.suggestions.iter().skip(3).take(10).cloned().collect();
            panel = group(panel, open, m!("more-suggestions"), G::Suggestions, || {
                more.iter()
                    .map(|w| line("", w.clone(), "", Some(&format!("_MTED SPELLREPLACE {w}")), true))
                    .collect()
            });
        }
        panel = panel
            .separator()
            .push(line("", m!("add-to-dictionary"), "", Some("_MTED SPELLADD"), false))
            .push(line("", m!("ignore-all"), "", Some("_MTED SPELLIGNORE"), false))
            .separator();
    }
    panel = panel
        .push(line("", m!("select-all"), "Ctrl+A", Some("_MTED SELECTALL"), false))
        .push(line("", m!("cut"), "Ctrl+X", has_selection.then_some("_MTED CUT"), false))
        .push(line("", m!("copy"), "Ctrl+C", has_selection.then_some("_MTED COPY"), false))
        .push(line("", m!("paste"), "Ctrl+V", Some("_MTED PASTE"), false));
    panel = group(panel, open, m!("paste-special"), G::PasteSpecial, || {
        vec![
            line("", m!("paste-no-character"), "", Some("_MTED PASTE NOCHAR"), true),
            line("", m!("paste-no-paragraph"), "", Some("_MTED PASTE NOPARA"), true),
            line("", m!("paste-plain"), "", Some("_MTED PASTE PLAIN"), true),
        ]
    })
    .separator()
    .push(line("", m!("insert-field"), "Ctrl+F", Some("_MTED FIELD"), false));
    panel = group(panel, open, m!("symbol"), G::Symbol, || {
        let mut rows: Vec<Element<'static, Message>> = symbols()
            .into_iter()
            .map(|(symbol, name)| {
                let command = format!("_MTED SYMBOL \\U+{:04X}", symbol.chars().next().unwrap_or(' ') as u32);
                line(symbol, name, "", Some(&command), true)
            })
            .collect();
        rows.push(separator());
        rows.push(line("", m!("other-symbol"), "", Some("_MTED CHARMAP"), true));
        rows
    })
    .push(line("", m!("import-text"), "", Some("_MTED IMPORT"), false))
    .separator();
    panel = group(panel, open, m!("paragraph-alignment"), G::Alignment, || {
        [
            (m!("align-left"), "LEFT"),
            (m!("align-center"), "CENTER"),
            (m!("align-right"), "RIGHT"),
            (m!("align-justify"), "JUSTIFY"),
            (m!("align-distribute"), "DISTRIBUTE"),
        ]
        .into_iter()
        .map(|(name, arg)| line("", name, "", Some(&format!("_MTED ALIGN {arg}")), true))
        .collect()
    })
    .push(line("", m!("paragraph"), "", Some("_MTED PARAGRAPH"), false));
    let kind = ed.list_kind();
    panel = group(panel, open, m!("bullets-and-lists"), G::Lists, || {
        let item = |on: bool, key: String, arg: &str| {
            line(check(on), key, "", lists.then(|| format!("_MTED LIST {arg}")).as_deref(), true)
        };
        vec![
            item(kind.is_none(), m!("list-off"), "OFF"),
            item(kind == Some(ListKind::LowerLetter), m!("list-lowercase"), "LOWER"),
            item(kind == Some(ListKind::UpperLetter), m!("list-uppercase"), "UPPER"),
            item(kind == Some(ListKind::Number), m!("list-numbered"), "NUMBER"),
            item(kind == Some(ListKind::Bullet), m!("list-bulleted"), "BULLET"),
            separator(),
            item(false, m!("list-start"), "START"),
            item(false, m!("list-continue"), "CONTINUE"),
            separator(),
            line(
                check(p.allow_auto_lists),
                m!("allow-auto-lists"),
                "",
                lists.then_some("_MTED LIST AUTO"),
                true,
            ),
            line(check(lists), m!("allow-lists"), "", Some("_MTED LIST ALLOW"), true),
        ]
    })
    .separator();
    panel = group(panel, open, m!("columns"), G::Columns, || {
        let mut rows = vec![
            line("", m!("no-columns"), "", Some("_MTED COLUMNS NONE"), true),
            line("", m!("dynamic-auto"), "", Some("_MTED COLUMNS DYNAMIC AUTO"), true),
            line("", m!("dynamic-manual"), "", Some("_MTED COLUMNS DYNAMIC MANUAL"), true),
        ];
        for n in 2..=6 {
            rows.push(line(
                "",
                format!("{}: {n}", m!("static-columns")),
                "",
                Some(&format!("_MTED COLUMNS STATIC {n}")),
                true,
            ));
        }
        rows.push(separator());
        rows.push(line("", m!("insert-column-break"), "Alt+Enter", Some("_MTED COLUMNBREAK"), true));
        rows.push(line("", m!("column-settings"), "", Some("_MTED COLUMNSETTINGS"), true));
        rows
    })
    .separator()
    .push(line("", m!("find-and-replace"), "Ctrl+R", Some("_MTED FIND"), false));
    panel = group(panel, open, m!("change-case"), G::Case, || {
        vec![
            line("", m!("uppercase"), "Ctrl+Shift+U", has_selection.then_some("_MTED CASE UPPER"), true),
            line("", m!("lowercase"), "Ctrl+Shift+L", has_selection.then_some("_MTED CASE LOWER"), true),
        ]
    })
    .push(line(check(ed.auto_caps), m!("all-caps"), "", Some("_MTED AUTOCAPS"), false));
    panel = group(panel, open, m!("character-set"), G::Charset, || {
        crate::ui::window::mtext_tool_dialogs::CHARSETS
            .iter()
            .map(|(code, name)| {
                line("", crate::t!(*name).into_owned(), "", Some(&format!("_MTED CHARSET {code}")), true)
            })
            .collect()
    })
    .separator()
    .push(line("", m!("combine-paragraphs"), "", has_selection.then_some("_MTED COMBINE"), false));
    panel = group(panel, open, m!("remove-formatting"), G::RemoveFormatting, || {
        vec![
            line("", m!("remove-character"), "Ctrl+Space", Some("_MTED CLEAR"), true),
            line("", m!("remove-paragraph"), "", Some("_MTED CLEAR PARA"), true),
            line("", m!("remove-all"), "", Some("_MTED CLEAR ALL"), true),
        ]
    })
    .push(line("", m!("background-mask"), "", Some("_MTED MASK"), false))
    .separator();
    let toolbar = crate::app::commands::mtext_toolbar() == 1;
    panel = group(panel, open, m!("editor-settings"), G::Settings, || {
        vec![
            line(check(p.wysiwyg), m!("wysiwyg"), "", Some("_MTED WYSIWYG"), true),
            line(check(toolbar), m!("show-toolbar"), "", Some("_MTED TOOLBAR"), true),
            line(check(ed.ruler), m!("show-ruler"), "", Some("_MTED RULER"), true),
            line(check(p.show_background), m!("show-background"), "", Some("_MTED BACKGROUND"), true),
            separator(),
            line(check(p.spell_check), m!("check-spelling"), "", Some("_MTED SPELL"), true),
            line("", m!("spelling-settings"), "", Some("_MTED SPELLSETTINGS"), true),
            line("", m!("dictionaries"), "", Some("_MTED DICTIONARIES"), true),
            separator(),
            line("", m!("highlight-color"), "", Some("_MTED HIGHLIGHT"), true),
        ]
    })
    .push(line("", m!("help"), "F1", Some("_MTED HELP"), false))
    .separator()
    .push(line("", m!("cancel"), "", Some("_MTED CANCEL"), false));
    Some((menu.at, panel.build()))
}

/// The tag's button under the text, with its menu while open; `place`
/// turns a point of the text's own frame into the canvas.
pub(in crate::app::view::overlay) fn smart_tag(
    ed: &MTextEditorState,
    place: impl Fn(f32, f32) -> iced::Point,
) -> Option<(iced::Point, Element<'static, Message>)> {
    let tag = ed.smart_tag?;
    let at = match tag {
        SmartTag::Stack(at) | SmartTag::List(at) => at,
    };
    let glyph = ed.glyph_boxes.iter().find(|b| b.vis == at)?;
    let anchor = place(glyph.local[0], glyph.local[1]);
    let base = button(crate::ui::icons::semantic(BOLT, 14.0))
        .on_press(Message::MTextSmartTag(!ed.smart_tag_open))
        .padding(3)
        .style(trigger_style(ed.smart_tag_open));
    let panel = ed.smart_tag_open.then(|| match tag {
        SmartTag::Stack(_) => {
            let kind = ed.stack_kind_at(at);
            Panel::new(m!("stack-tag"), 220.0)
                .push(line(
                    check(kind == Some(StackingType::Diagonal)),
                    m!("diagonal"),
                    "",
                    Some("_MTED STACKTAG DIAGONAL"),
                    false,
                ))
                .push(line(
                    check(kind == Some(StackingType::Horizontal)),
                    m!("horizontal"),
                    "",
                    Some("_MTED STACKTAG HORIZONTAL"),
                    false,
                ))
                .separator()
                .push(line("", m!("unstack"), "", Some("_MTED STACKTAG UNSTACK"), false))
                .push(line("", m!("stack-properties"), "", Some("_MTED STACKTAG PROPERTIES"), false))
                .build()
        }
        SmartTag::List(_) => {
            let mut panel = Panel::new(m!("list-tag"), 260.0)
                .push(line("", m!("list-start"), "", Some("_MTED LISTTAG START"), false))
                .push(line("", m!("list-continue"), "", Some("_MTED LISTTAG CONTINUE"), false));
            panel = if ed.list_tag_removed() {
                panel.push(line("", m!("redo-list"), "", Some("_MTED LISTTAG REDO"), false))
            } else {
                panel.push(line("", m!("remove-list"), "", Some("_MTED LISTTAG REMOVE"), false))
            };
            panel
                .push(line("", m!("stop-auto-lists"), "", Some("_MTED LISTTAG STOP"), false))
                .build()
        }
    });
    Some((
        iced::Point::new(anchor.x, anchor.y + 2.0),
        popup::anchored(base, panel, Message::MTextSmartTag(false)),
    ))
}
