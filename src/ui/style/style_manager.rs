//! Shared scaffold for every style-manager window.
//!
//! All five managers (text / dimension / table / multileader / multiline)
//! share the same frame: a top toolbar (New / Copy / Delete on the left,
//! manager-specific actions such as Set Current / Apply on the right), a style
//! list on the left, and a property editor on the right. Only the editor
//! differs, so each manager builds just that and hands it to [`view`]; the
//! toolbar, list, inline-rename wiring and chrome live here once.

use crate::app::{Message, StyleKind};
use iced::widget::button::{Status, Style};
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Fill, Theme};

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
    /// Tuple-variant constructor for the per-row select message
    /// (e.g. `Message::TextStyleDialogSelect`).
    pub on_select: fn(String) -> Message,
    /// "Set Current" action (right side). Every manager has one.
    pub on_set_current: Message,
    /// "Apply" action (right side, primary). Every manager has one.
    pub on_apply: Message,
    pub editor: Element<'a, Message>,
}

pub fn view<'a, 'b>(s: Scaffold<'a, 'b>) -> Element<'a, Message> {
    // ── Toolbar: New / Copy / Delete | … | Set Current / Apply ────────────
    let bar = row![
        tb_button("New", s.on_new, false),
        tb_button("Copy", s.on_copy, false),
        tb_button("Delete", s.on_delete, false),
        Space::new().width(Fill),
        tb_button("Set Current", s.on_set_current, false),
        tb_button("Apply", s.on_apply, true),
    ]
    .spacing(4)
    .align_y(iced::Center);
    let toolbar = container(bar)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weak.color
            )),
            ..Default::default()
        })
        .width(Fill)
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
            )
        })
        .collect();

    let list_panel = container(
        column![
            text("Styles").size(10).style(muted_text_style),
            container(scrollable(column(rows).spacing(1)).height(Fill))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.weak.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into()
                    },
                    ..Default::default()
                    }
                })
                .width(Fill)
                .height(Fill)
                .padding(2),
        ]
        .spacing(4)
        .height(Fill),
    )
    .width(170)
    .height(Fill)
    .padding(iced::Padding {
        top: 12.0,
        right: 8.0,
        bottom: 12.0,
        left: 12.0,
    });

    let body = row![list_panel, vsep(), s.editor].height(Fill);

    container(column![toolbar, hdivider(), body])
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color
            )),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into()
}

// ── Shared chrome ──────────────────────────────────────────────────────────

pub(crate) fn tb_button<'a>(label: &'a str, msg: Message, accent: bool) -> Element<'a, Message> {
    let pad = if accent { [4, 14] } else { [4, 10] };
    button(text(label).size(11))
        .on_press(msg)
        .style(btn_s(accent))
        .padding(pad)
        .into()
}

fn btn_s(accent: bool) -> impl Fn(&Theme, Status) -> Style {
    move |theme: &Theme, st| {
        let palette = theme.extended_palette();
        let pair = match (accent, st) {
            (true, Status::Hovered | Status::Pressed) => palette.primary.strong,
            (false, Status::Hovered | Status::Pressed) => palette.background.strong,
            (true, _) => palette.primary.base,
            _ => palette.background.weak,
        };
        Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
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
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

pub(crate) fn hdivider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}

pub(crate) fn vsep<'a>() -> Element<'a, Message> {
    container(Space::new().width(1).height(Fill))
        .width(1)
        .height(Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.neutral.color
            )),
            ..Default::default()
        })
        .into()
}
