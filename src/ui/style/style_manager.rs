//! Shared scaffold for every style-manager window.
//!
//! All five managers (text / dimension / table / multileader / multiline)
//! share the same frame: a top toolbar (New / Copy / Delete on the left,
//! manager-specific actions such as Set Current / Apply on the right), a style
//! list on the left, and a property editor on the right. Only the editor
//! differs, so each manager builds just that and hands it to [`view`]; the
//! toolbar, list, inline-rename wiring and chrome live here once.

use crate::app::{Message, StyleKind};
use crate::t;
use iced::widget::button::{Status, Style};
use iced::widget::{button, checkbox, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Length, Theme};
use std::borrow::Cow;

/// Everything the shared frame needs. The per-manager `editor` element is the
/// only bespoke part.
///
/// The toolbar is uniform across every manager: New / Copy / Delete on the
/// left, then **Set Current** and **Apply** on the right. Each manager just
/// supplies the two messages, so the right side can never drift or go missing.
///
/// Two lifetimes: `'a` is what the returned element keeps alive (the editor and
/// the rename buffer the inline `text_input` borrows); `'b` is the transient
/// list data (`styles`, `selected`, …) that the frame only reads while building
/// rows, so callers may pass a locally-built `Vec`.
pub struct Scaffold<'a, 'b> {
    pub sizing: crate::ui::modal::ModalSizing,
    pub kind: StyleKind,
    pub styles: &'b [String],
    pub selected: &'b str,
    /// Current style for this manager, marked with a ◀ in the list. `None`
    /// when the manager has no "current" concept.
    pub current: Option<&'b str>,
    pub rename_active: Option<&'b str>,
    pub rename_buf: &'a str,
    pub on_new: Message,
    pub on_copy: Message,
    pub on_delete: Message,
    /// Present only for styles that can be imported from another drawing.
    pub on_import: Option<Message>,
    /// Tuple-variant constructor for the per-row select message
    /// (e.g. `Message::TextStyleDialogSelect`).
    pub on_select: fn(String) -> Message,
    /// "Set Current" action (right side). Every manager has one.
    pub on_set_current: Message,
    /// "Apply" action (right side, primary). Every manager has one.
    pub on_apply: Message,
    /// Referenced styles may be copied but not changed, renamed, deleted, or
    /// made current.
    pub read_only: bool,
    /// "Styles in use" list filter: whether it is on and the message that
    /// sets it. `None` for managers without one.
    pub in_use_filter: Option<(bool, fn(bool) -> Message)>,
    pub editor: Element<'a, Message>,
}

pub fn view<'a, 'b>(s: Scaffold<'a, 'b>) -> Element<'a, Message> {
    let width = s.sizing.width;
    let height = s.sizing.height;
    // ── Toolbar: New / Copy / Delete | … | Set Current / Apply ────────────
    let mut bar = row![
        tb_button(t!("New"), s.on_new, false),
        tb_button(t!("Copy"), s.on_copy, false),
        tb_button_enabled(t!("Delete"), s.on_delete, false, !s.read_only),
    ];
    if let Some(message) = s.on_import {
        bar = bar.push(tb_button(t!("Import"), message, false));
    }
    let bar = bar
        .push(Space::new().width(width))
        .push(tb_button_enabled(
            t!("Set Current"), s.on_set_current, false, !s.read_only,
        ))
        .push(tb_button_enabled(t!("Apply"), s.on_apply, true, !s.read_only))
        .spacing(4)
        .align_y(iced::Center);
    let toolbar = container(bar)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .width(width)
        .padding([5, 8]);

    // ── Left: style list (single click selects, double click renames) ─────
    let rows: Vec<Element<'_, Message>> = s
        .styles
        .iter()
        .map(|name| {
            let is_sel = name.as_str() == s.selected;
            let is_current = s.current == Some(name.as_str());
            crate::ui::style::style_list::item(
                name,
                is_current,
                is_sel,
                s.kind,
                (s.on_select)(name.clone()),
                s.rename_active,
                s.rename_buf,
                !(s.read_only && is_sel),
            )
        })
        .collect();

    let filter: Element<'_, Message> = match s.in_use_filter {
        Some((on, message)) => checkbox(on)
            .label(t!("Styles in use"))
            .on_toggle(message)
            .size(12)
            .text_size(11)
            .into(),
        None => Space::new().height(0).into(),
    };
    let list_panel = container(
        column![
            text(t!("Styles")).size(10).style(muted_text_style),
            filter,
            container(scrollable(column(rows).spacing(1)).height(height))
                .style(|theme: &Theme| {
                    let palette = theme.palette();
                    container::Style {
                        background: Some(Background::Color(palette.background.weak.color)),
                        border: Border {
                            color: palette.background.neutral.color,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .width(width)
                .height(height)
                .padding(2),
        ]
        .spacing(4)
        .height(height),
    )
    .width(170)
    .height(height)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    let body = row![list_panel, vsep(height), s.editor].height(height);

    container(column![toolbar, hdivider(width), body])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            ..Default::default()
        })
        .width(width)
        .height(height)
        .into()
}

/// Choose source styles, then confirm each name collision individually.
pub(crate) fn import_view<'a>(
    draft: &'a crate::app::StyleImportDraft,
    target: &codec::Document,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let mut content = column![
        text(draft.file.clone()).size(12),
        text(t!("Select the styles to import")).size(11),
    ]
    .spacing(8);
    if let Some(name) = &draft.conflict {
        content = content
            .push(text(name.clone()).size(14))
            .push(text(t!("A style with this name already exists. Overwrite it?")))
            .push(row![
                button(text(t!("Overwrite"))).on_press(Message::StyleImportConfirm(true)),
                button(text(t!("Skip"))).on_press(Message::StyleImportConfirm(false)),
                button(text(t!("Cancel"))).on_press(Message::StyleImportCancel),
            ]
            .spacing(8));
    } else {
        let mut rows = column![].spacing(4);
        for (name, on) in &draft.selected {
            let label = if crate::app::style_import_exists(target, draft.kind, name) {
                format!("{} ({})", name, t!("Existing"))
            } else {
                name.clone()
            };
            let key = name.clone();
            rows = rows.push(
                checkbox(*on)
                    .label(label)
                    .on_toggle(move |_| Message::StyleImportToggle(key.clone())),
            );
        }
        if draft.selected.is_empty() {
            rows = rows.push(text(t!("No styles found in this drawing.")));
        }
        content = content
            .push(scrollable(rows).height(sizing.height))
            .push(row![
                button(text(t!("Import"))).on_press_maybe(
                    draft.selected.iter().any(|(_, on)| *on)
                        .then_some(Message::StyleImportApply)
                ),
                button(text(t!("Cancel"))).on_press(Message::StyleImportCancel),
            ]
            .spacing(8));
    }
    container(content)
        .padding(16)
        .width(sizing.width)
        .height(sizing.height)
        .into()
}

pub struct EditorTab {
    pub label: Cow<'static, str>,
    pub active: bool,
    pub on_press: Message,
}

pub struct EditorComparison {
    pub selected: String,
    pub options: Vec<String>,
    pub summary: String,
    pub on_select: fn(String) -> Message,
}

/// Shared right-hand editor composition used by every named style manager.
/// The preview stays visible while the active property page scrolls.
pub struct EditorShell<'a> {
    pub sizing: crate::ui::modal::ModalSizing,
    pub selected: String,
    pub status: Cow<'static, str>,
    pub preview: Element<'a, Message>,
    pub comparison: Option<EditorComparison>,
    pub tabs: Vec<EditorTab>,
    pub content: Element<'a, Message>,
}

pub fn editor_shell<'a>(shell: EditorShell<'a>) -> Element<'a, Message> {
    let tabs: Vec<Element<'a, Message>> = shell
        .tabs
        .into_iter()
        .map(|tab| {
            button(text(tab.label).size(11))
                .on_press(tab.on_press)
                .style(tab_button_style(tab.active))
                .padding([4, 10])
                .into()
        })
        .collect();
    let comparison: Element<'a, Message> = match shell.comparison {
        Some(comparison) if !comparison.options.is_empty() => row![
            text(t!("Compare with")).size(10).style(muted_text_style),
            iced::widget::pick_list(Some(comparison.selected), comparison.options, |value| value
                .to_string(),)
            .on_select(comparison.on_select)
            .text_size(11)
            .width(150),
            text(comparison.summary).size(10).style(muted_text_style),
        ]
        .spacing(8)
        .align_y(iced::Center)
        .into(),
        _ => Space::new().height(0).into(),
    };
    let preview = container(shell.preview)
        .width(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        });
    container(
        column![
            row![
                text(shell.selected).size(13).style(primary_text_style),
                Space::new().width(Length::Fill),
                text(shell.status).size(10).style(muted_text_style),
            ]
            .align_y(iced::Center),
            preview,
            comparison,
            row(tabs).spacing(2),
            hdivider(Length::Fill),
            scrollable(
                container(shell.content)
                    .padding([12, 12])
                    .width(Length::Fill)
            )
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .spacing(6)
        .height(shell.sizing.height),
    )
    .height(shell.sizing.height)
    .width(Length::Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 0.0,
        bottom: 12.0,
        left: 0.0,
    })
    .into()
}

// ── Shared chrome ──────────────────────────────────────────────────────────

pub(crate) fn tb_button(
    label: impl Into<Cow<'static, str>>,
    msg: Message,
    accent: bool,
) -> Element<'static, Message> {
    let pad = if accent { [4, 14] } else { [4, 10] };
    button(text(label.into()).size(11))
        .on_press(msg)
        .style(btn_s(accent))
        .padding(pad)
        .into()
}

pub(crate) fn tb_button_enabled(
    label: impl Into<Cow<'static, str>>,
    msg: Message,
    accent: bool,
    enabled: bool,
) -> Element<'static, Message> {
    let pad = if accent { [4, 14] } else { [4, 10] };
    let button = button(text(label.into()).size(11))
        .style(btn_s(accent))
        .padding(pad);
    if enabled {
        button.on_press(msg).into()
    } else {
        button.into()
    }
}

fn tab_button_style(active: bool) -> impl Fn(&Theme, Status) -> Style {
    move |theme: &Theme, status| {
        let palette = theme.palette();
        let pair = match (active, status) {
            (true, _) => palette.primary.strong,
            (false, Status::Hovered | Status::Pressed) => palette.background.strong,
            _ => palette.background.weak,
        };
        Style {
            background: Some(Background::Color(pair.color)),
            text_color: pair.text,
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    }
}

fn primary_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn btn_s(accent: bool) -> impl Fn(&Theme, Status) -> Style {
    move |theme: &Theme, st| {
        let palette = theme.palette();
        let pair = match (accent, st) {
            (_, Status::Disabled) => palette.background.weak,
            (true, Status::Hovered | Status::Pressed) => palette.primary.strong,
            (false, Status::Hovered | Status::Pressed) => palette.background.strong,
            (true, _) => palette.primary.base,
            _ => palette.background.weak,
        };
        Style {
            background: Some(Background::Color(pair.color)),
            text_color: if st == Status::Disabled {
                pair.text.scale_alpha(0.45)
            } else {
                pair.text
            },
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    }
}

pub(crate) fn muted_text_style(theme: &Theme) -> iced::widget::text::Style {
    crate::ui::style::common::muted_style(theme)
}

pub(crate) use crate::ui::style::form::{hdivider, vsep};
