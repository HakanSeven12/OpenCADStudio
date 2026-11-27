//! Customize User Interface (CUI): the user's own ribbon panels, shown on
//! the My Tools tab, and the way to the keyboard shortcuts. Panels are made,
//! named, ordered and deleted on the left; the selected panel's commands are
//! added (by name or alias), ordered and removed on the right. Every change
//! shows on the ribbon at once.

use crate::app::Message;
use crate::ui::kit;
use crate::ui::ribbon::CustomPanel;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{dialog_button, hdivider};
use crate::ui::style::{chrome, kit_metrics as km};
use iced::widget::{Space, column, container, row, text, text_input};
use iced::{Background, Border, Element, Length, Theme};

#[derive(Debug, Clone, Default)]
pub struct State {
    pub selected: Option<usize>,
    /// The command being typed for the selected panel.
    pub draft: String,
    /// The suggestion list was closed (Esc, a click outside) for the text
    /// typed so far; typing opens it again.
    pub suggestions_hidden: bool,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Select(usize),
    NewPanel,
    DeletePanel,
    /// Move the selected panel up (`true`) or down.
    MovePanel(bool),
    Title(String),
    Draft(String),
    AddCommand,
    /// A suggestion row was clicked: its command goes through `AddCommand`.
    PickSuggestion(String),
    RemoveCommand(usize),
    MoveCommand(usize, bool),
    /// Close the suggestion list without picking.
    HideSuggestions,
    Shortcuts,
    /// Pick a file and write the user's panels to it.
    Export,
    /// Pick a file whose panels replace the user's.
    Import,
    /// The export finished: the file written, or why it failed. `None`
    /// when the picker was dismissed.
    Exported(Option<Result<String, String>>),
    /// The import picker finished: the file's text, or why it failed to
    /// read. `None` when the picker was dismissed.
    Imported(Option<Result<String, String>>),
}

/// Marker of an exported panels file, so another JSON file is not taken
/// for one.
const FILE_MARK: &str = "opencad-ribbon-panels";
const FILE_VERSION: u32 = 1;

#[derive(serde::Serialize, serde::Deserialize)]
struct PanelsFile {
    format: String,
    version: u32,
    panels: Vec<CustomPanel>,
}

/// The text of a panels file: the user's panels, in their order.
pub fn panels_to_string(panels: &[CustomPanel]) -> String {
    let file = PanelsFile {
        format: FILE_MARK.to_owned(),
        version: FILE_VERSION,
        panels: panels.to_vec(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// The panels in a panels file's text. A command this build does not know is
/// kept as written (like one typed in the dialog); empty commands are
/// dropped. Anything that is not a panels file of this or an older version
/// is an error and nothing is returned.
pub fn panels_from_str(text: &str) -> Result<Vec<CustomPanel>, String> {
    let file: PanelsFile = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if file.format != FILE_MARK {
        return Err(format!("not a panels file: format \"{}\"", file.format));
    }
    if file.version == 0 || file.version > FILE_VERSION {
        return Err(format!("unsupported panels file version {}", file.version));
    }
    Ok(file
        .panels
        .into_iter()
        .map(|panel| CustomPanel {
            title: panel.title,
            commands: panel
                .commands
                .into_iter()
                .map(|command| command.trim().to_owned())
                .filter(|command| !command.is_empty())
                .collect(),
        })
        .collect())
}

fn msg(m: Msg) -> Message {
    Message::RibbonCustomize(m)
}

/// Most suggestion rows shown under the add box.
const SUGGESTION_ROWS: usize = 6;

/// Width of the dialog: wide enough for the footer's four buttons in one row.
pub const DIALOG_WIDTH: f32 = 760.0;

/// Width of the panel list on the left.
const PANELS_WIDTH: f32 = 210.0;

/// Natural height of the two framed lists. The dialog is measured once at
/// this height and keeps its size: adding, removing or filtering rows never
/// moves the buttons under the lists. Resizing the window grows the lists.
const LIST_HEIGHT: f32 = 260.0;

/// The list height for this layout pass: fixed while the dialog measures
/// itself, the room left over once its frame is set.
fn list_height(sizing: &crate::ui::modal::ModalSizing) -> Length {
    if sizing.height == Length::Fill {
        Length::Fill
    } else {
        Length::Fixed(LIST_HEIGHT)
    }
}

/// A framed, inset area (the panel list, the command list).
fn well<'a>(content: impl Into<Element<'a, Message>>, height: Length) -> Element<'a, Message> {
    container(content)
        .width(Length::Fill)
        .height(height)
        .padding(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::surface(theme))),
            border: Border {
                color: chrome::border(theme),
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// A muted line centred in an empty list.
fn empty_note<'a>(note: String) -> Element<'a, Message> {
    container(
        text(note)
            .size(12)
            .style(muted_style)
            .align_x(iced::alignment::Horizontal::Center),
    )
    .padding(16)
    .center(Length::Fill)
    .into()
}

/// An icon button of a list toolbar; `None` greys it out.
fn tool<'a>(art: &'static [u8], tip: String, m: Option<Msg>) -> Element<'a, Message> {
    let icon = if m.is_some() {
        kit::tool_icon(art)
    } else {
        crate::ui::icons::themed_disabled(art, km::ROW_ICON)
    };
    kit::tool_button(icon, tip, m.map(msg), false)
}

/// A command row's own move / remove buttons, at its end.
fn row_tools<'a>(index: usize, count: usize) -> Element<'a, Message> {
    row![
        tool(
            crate::ui::icons::TRI_UP,
            crate::tr!("ribbon-customize", "move-up").to_string(),
            (index > 0).then_some(Msg::MoveCommand(index, true)),
        ),
        tool(
            crate::ui::icons::TRI_DOWN,
            crate::tr!("ribbon-customize", "move-down").to_string(),
            (index + 1 < count).then_some(Msg::MoveCommand(index, false)),
        ),
        tool(
            crate::ui::icons::TRASH,
            crate::tr!("ribbon-customize", "remove-command").to_string(),
            Some(Msg::RemoveCommand(index)),
        ),
    ]
    .spacing(2)
    .into()
}

fn panels_column<'a>(
    state: &State,
    panels: &[CustomPanel],
    height: Length,
) -> Element<'a, Message> {
    let selected = state.selected.filter(|index| *index < panels.len());
    let list: Element<'a, Message> = if panels.is_empty() {
        empty_note(crate::tr!("ribbon-customize", "no-panels").to_string())
    } else {
        let mut rows = column![];
        for (index, panel) in panels.iter().enumerate() {
            rows = rows.push(
                kit::list_row(vec![(
                    kit::text_cell(panel.title.clone(), false),
                    Length::Fill,
                )])
                .selected(selected == Some(index))
                .on_press(msg(Msg::Select(index)))
                .view(),
            );
        }
        kit::scroll_area(container(rows).padding(iced::Padding::ZERO.top(2.0)))
    };
    // New panel sits in the list's own header, next to the title of what
    // it adds to; ordering and deleting act on the picked row, under it.
    let header = row![
        text(crate::tr!("ribbon-customize", "panels"))
            .size(12)
            .style(muted_style),
        Space::new().width(Length::Fill),
        tool(
            crate::ui::icons::PLUS,
            crate::tr!("ribbon-customize", "new-panel").to_string(),
            Some(Msg::NewPanel),
        ),
    ]
    .align_y(iced::Center);
    let toolbar = row![
        Space::new().width(Length::Fill),
        tool(
            crate::ui::icons::TRI_UP,
            crate::tr!("ribbon-customize", "move-up").to_string(),
            selected.filter(|index| *index > 0).map(|_| Msg::MovePanel(true)),
        ),
        tool(
            crate::ui::icons::TRI_DOWN,
            crate::tr!("ribbon-customize", "move-down").to_string(),
            selected
                .filter(|index| index + 1 < panels.len())
                .map(|_| Msg::MovePanel(false)),
        ),
        tool(
            crate::ui::icons::TRASH,
            crate::tr!("ribbon-customize", "delete-panel").to_string(),
            selected.map(|_| Msg::DeletePanel),
        ),
    ]
    .spacing(2)
    .align_y(iced::Center);
    column![header, well(list, height), toolbar]
    .spacing(6)
    .width(Length::Fixed(PANELS_WIDTH))
    .into()
}

fn panel_editor<'a>(
    state: &State,
    suggestions: &[String],
    panel: &CustomPanel,
    height: Length,
) -> Element<'a, Message> {
    let count = panel.commands.len();
    let commands: Element<'a, Message> = if count == 0 {
        empty_note(crate::tr!("ribbon-customize", "no-commands").to_string())
    } else {
        let mut rows = column![];
        for (index, command) in panel.commands.iter().enumerate() {
            rows = rows.push(
                row![
                    container(text(command.to_string()).size(12))
                        .padding([0.0, 6.0])
                        .width(Length::Fill),
                    row_tools(index, count),
                ]
                .height(Length::Fixed(km::TOOL_H + 2.0))
                .align_y(iced::Center),
            );
        }
        kit::scroll_area(container(rows).padding(iced::Padding::ZERO.top(2.0)))
    };

    // The suggestions float under the add box: they move nothing and need
    // no room of their own.
    let offered: &[String] = if state.suggestions_hidden || state.draft.trim().is_empty() {
        &[]
    } else {
        suggestions
    };
    let options = offered
        .iter()
        .map(|name| {
            kit::dropdown_option(
                name.clone(),
                name.clone(),
                msg(Msg::PickSuggestion(name.clone())),
                false,
            )
        })
        .collect();
    let input = text_input(
        &crate::tr!("ribbon-customize", "command-placeholder"),
        &state.draft,
    )
    .on_input(|typed| msg(Msg::Draft(typed)))
    .on_submit(msg(Msg::AddCommand))
    .size(12)
    .padding([4, 8])
    .width(Length::Fill);
    let input = kit::dropdown(
        input.into(),
        crate::ui::dropdown_list::DropdownList::new(options)
            .query(state.draft.clone())
            .select_first_by_default(false)
            .on_escape(msg(Msg::HideSuggestions)),
        kit::dropdown_width(offered.iter().map(String::as_str), 240.0),
        msg(Msg::HideSuggestions),
        !offered.is_empty(),
    );
    let add = dialog_button(
        crate::tr!("ribbon-customize", "add"),
        msg(Msg::AddCommand),
        false,
    );

    column![
        container(
            text(crate::tr!("ribbon-customize", "title"))
                .size(12)
                .style(muted_style)
        )
        .height(Length::Fixed(km::TOOL_H))
        .align_y(iced::Center),
        text_input("", &panel.title)
            .on_input(|title| msg(Msg::Title(title)))
            .size(13)
            .padding([4, 8]),
        Space::new().height(4),
        text(crate::tr!("ribbon-customize", "commands"))
            .size(12)
            .style(muted_style),
        // The add box sits above the list: its suggestions float over the
        // list, never over the dialog's footer.
        row![input, add].spacing(6).align_y(iced::Center),
        well(commands, height),
    ]
    .spacing(6)
    .width(Length::Fill)
    .into()
}

pub fn view_window<'a>(
    state: &State,
    suggestions: &[String],
    panels: &[CustomPanel],
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let height = list_height(&sizing);
    let right: Element<'a, Message> = match state.selected.and_then(|index| panels.get(index)) {
        Some(panel) => panel_editor(state, suggestions, panel, height),
        // Without a panel the right side keeps a frame, so the dialog does
        // not change shape when one is picked.
        None => column![
            container(
                text(crate::tr!("ribbon-customize", "commands"))
                    .size(12)
                    .style(muted_style)
            )
            .height(Length::Fixed(km::TOOL_H))
            .align_y(iced::Center),
            well(
                empty_note(crate::tr!("ribbon-customize", "pick-panel").to_string()),
                height
            ),
        ]
        .spacing(6)
        .width(Length::Fill)
        .into(),
    };

    let footer = row![
        dialog_button(
            crate::tr!("ribbon-customize", "shortcuts"),
            msg(Msg::Shortcuts),
            false
        ),
        dialog_button(
            crate::tr!("ribbon-customize", "export"),
            msg(Msg::Export),
            false
        ),
        dialog_button(
            crate::tr!("ribbon-customize", "import"),
            msg(Msg::Import),
            false
        ),
        Space::new().width(Length::Fill),
        dialog_button(
            crate::tr!("ribbon-customize", "close"),
            Message::CloseModal,
            true
        ),
    ]
    .spacing(8)
    .align_y(iced::Center);

    let fill = sizing.height == Length::Fill;
    column![
        text(crate::tr!("ribbon-customize", "hint"))
            .size(12)
            .style(muted_style),
        row![panels_column(state, panels, height), right]
            .spacing(16)
            .height(if fill { Length::Fill } else { Length::Shrink }),
        hdivider(Length::Fill),
        footer,
    ]
    .spacing(12)
    .padding(16)
    .width(if sizing.width == Length::Fill {
        Length::Fill
    } else {
        Length::Fixed(DIALOG_WIDTH)
    })
    .into()
}

/// Command names to offer for the typed text: the command line's own
/// autocomplete ranking (prefix matches first, then substring matches; an
/// alias offers the command it stands for), cut to a few rows.
pub fn suggestions(
    typed: &str,
    dynamic: &[String],
    aliases: &rustc_hash::FxHashMap<String, String>,
) -> Vec<String> {
    let mut found = crate::ui::command_line::ranked_matches(typed, dynamic, aliases);
    found.truncate(SUGGESTION_ROWS);
    found
}

#[cfg(test)]
mod file_tests {
    use super::*;

    fn panel(title: &str, commands: &[&str]) -> CustomPanel {
        CustomPanel {
            title: title.to_owned(),
            commands: commands.iter().map(|c| c.to_string()).collect(),
        }
    }

    #[test]
    fn a_panels_file_round_trips() {
        let panels = vec![panel("Mine", &["LINE", "NOSUCHCOMMAND"]), panel("Empty", &[])];
        assert_eq!(panels_from_str(&panels_to_string(&panels)), Ok(panels));
        assert_eq!(panels_from_str(&panels_to_string(&[])), Ok(Vec::new()));
    }

    #[test]
    fn a_malformed_file_is_an_error() {
        for text in [
            "",
            "not json",
            "[]",
            r#"{"panels": []}"#,
            r#"{"format":"other","version":1,"panels":[]}"#,
            r#"{"format":"opencad-ribbon-panels","version":99,"panels":[]}"#,
            r#"{"format":"opencad-ribbon-panels","version":1,"panels":[{"title":5}]}"#,
        ] {
            assert!(panels_from_str(text).is_err(), "{text:?} was accepted");
        }
    }

    #[test]
    fn empty_commands_are_dropped_and_unknown_ones_kept() {
        let text = r#"{"format":"opencad-ribbon-panels","version":1,
            "panels":[{"title":"A","commands":[" LINE ","","FUTURE_CMD"]}]}"#;
        assert_eq!(
            panels_from_str(text),
            Ok(vec![panel("A", &["LINE", "FUTURE_CMD"])])
        );
    }

    /// An import that fails leaves the user's panels as they were; one that
    /// works replaces them.
    #[test]
    fn an_import_replaces_the_panels_and_a_bad_one_changes_nothing() {
        use crate::app::{Message, OpenCADStudio};
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.update(Message::RibbonCustomizeOpen);
        let _ = app.update(Message::RibbonCustomize(Msg::NewPanel));
        let before = app.ribbon.custom_panels().to_vec();
        assert_eq!(before.len(), 1);

        let _ = app.update(Message::RibbonCustomize(Msg::Imported(Some(Ok(
            "garbage".to_owned()
        )))));
        assert_eq!(app.ribbon.custom_panels(), before);
        let _ = app.update(Message::RibbonCustomize(Msg::Imported(Some(Err(
            "unreadable".to_owned()
        )))));
        assert_eq!(app.ribbon.custom_panels(), before);

        let wanted = vec![panel("A", &["LINE"]), panel("B", &["CIRCLE"])];
        let _ = app.update(Message::RibbonCustomize(Msg::Imported(Some(Ok(
            panels_to_string(&wanted),
        )))));
        assert_eq!(app.ribbon.custom_panels(), wanted);
    }
}

/// The footer buttons are one line high at every window size the dialog
/// can have (finding 173: "Apply & Exit" wrapped onto two lines).
#[cfg(test)]
mod footer_tests {
    use crate::app::{Message, OpenCADStudio};

    fn text_bounds(app: &OpenCADStudio, size: iced::Size, label: &str) -> iced::Rectangle {
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            size,
            app.view_main(),
        );
        ui.find(label.to_string())
            .unwrap_or_else(|_| panic!("{label} not found"))
            .visible_bounds()
            .unwrap_or_else(|| panic!("{label} not visible"))
    }

    /// Every label has the height of `one_line` and sits on its row.
    fn assert_one_row(app: &OpenCADStudio, size: iced::Size, one_line: &str, labels: &[&str]) {
        let single = text_bounds(app, size, one_line);
        for label in labels {
            let at = text_bounds(app, size, label);
            assert!(
                (at.height - single.height).abs() < 0.5,
                "{label} wraps at {size:?}: {at:?} vs {one_line} {single:?}"
            );
            assert!(
                (at.y - single.y).abs() < 0.5,
                "{label} is not on the row of {one_line} at {size:?}: {at:?} vs {single:?}"
            );
        }
    }

    const SIZES: [iced::Size; 3] = [
        iced::Size::new(1280.0, 900.0),
        // The smallest window the app is used at.
        iced::Size::new(900.0, 600.0),
        // A small window or a high display scale: the dialog is capped by the
        // window, so only the labels themselves keep the footer on one row.
        iced::Size::new(640.0, 480.0),
    ];

    #[test]
    fn the_shortcuts_footer_is_one_row_at_the_default_and_the_smallest_size() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.update(Message::ShortcutsPanelOpen);
        for size in SIZES {
            assert_one_row(
                &app,
                size,
                "Apply",
                &["Apply & Exit", "Reset to default", "Export...", "Import..."],
            );
        }
    }

    #[test]
    fn the_customize_footer_is_one_row_at_the_default_and_the_smallest_size() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.update(Message::RibbonCustomizeOpen);
        for size in SIZES {
            assert_one_row(&app, size, "Close", &["Keyboard Shortcuts…", "Export…", "Import…"]);
        }
    }
}
