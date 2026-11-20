//! Visual Styles Manager (VISUALSTYLES): the styles a viewport draws in and
//! the edge settings they share (crease angle, occluded and silhouette edges).

use crate::app::Message;
use crate::modules::view::visual_style::VISUAL_STYLES;
use document::entities::ViewportRenderMode;
use iced::{
    widget::{button, checkbox, column, container, row, text, text_input, Space},
    Element, Fill,
};

/// What the window shows: the active viewport's style and the live edge
/// settings, plus the crease angle being typed.
pub(crate) struct View<'a> {
    pub current: ViewportRenderMode,
    pub crease: &'a str,
    pub occluded: bool,
    pub silhouette: bool,
}

pub(crate) fn view_window(v: View<'_>) -> Element<'_, Message> {
    let mut styles = column![text(crate::t!("Available Visual Styles")).size(13)].spacing(4);
    for style in VISUAL_STYLES {
        let label = text(crate::t!(style.label)).size(12);
        let b = button(label)
            .width(Fill)
            .on_press(Message::SetRenderMode(style.mode));
        styles = styles.push(if style.mode == v.current {
            b.style(button::primary)
        } else {
            b.style(button::secondary)
        });
    }
    let edges = column![
        text(crate::t!("Edge Settings")).size(13),
        row![
            text(crate::t!("Crease angle (VSEDGESMOOTH)")).size(12).width(220),
            text_input("1", v.crease)
                .on_input(Message::VisualStylesCreaseInput)
                .on_submit(Message::VisualStylesCreaseApply)
                .size(12)
                .width(80),
        ]
        .spacing(8)
        .align_y(iced::Center),
        text(crate::t!(
            "Mesh edges between faces that meet at less than this angle are not drawn (0–180)."
        ))
        .size(11),
        checkbox(v.occluded)
            .label(crate::t!("Show occluded edges dashed in Hidden Line (VSOCCLUDEDEDGES)"))
            .text_size(12)
            .on_toggle(Message::VisualStylesOccludedEdges),
        checkbox(v.silhouette)
            .label(crate::t!("Show silhouette edges on solids (DISPSILH)"))
            .text_size(12)
            .on_toggle(Message::DispSilhChanged),
    ]
    .spacing(8);
    let body = column![
        styles,
        edges,
        row![
            Space::new().width(Fill),
            button(text(crate::t!("Close"))).on_press(Message::CloseModal),
        ],
    ]
    .spacing(16);
    container(body).padding(18).width(460).into()
}
