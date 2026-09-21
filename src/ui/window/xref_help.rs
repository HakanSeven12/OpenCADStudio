//! Reference Manager help window: how the manager works on the web build
//! (read-only list, disabled mutations, no previews) and where to report
//! issues. Rendered as a modal like About/DSettings so the text gets a
//! proper window width — an anchored toolbar dropdown squeezed it into an
//! unreadable sliver.

use crate::app::Message;
use crate::ui::style::common::muted_style;
use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Element, Fill, Length, Theme};

/// GitHub issues page linked from the help window, so web users can report
/// missing or broken behavior where it will be seen.
pub const XREF_ISSUES_URL: &str = "https://github.com/HakanSeven12/OpenCADStudio/issues";

pub fn view_window(sizing: crate::ui::modal::ModalSizing) -> Element<'static, Message> {
    // Fixed window width: paragraphs wrap against this instead of the
    // toolbar, which is what made the dropdown unreadable.
    let body_w = Length::Fixed(460.0);
    let para = |label: String| -> Element<'static, Message> {
        text(label).size(12).width(Fill).into()
    };

    let intro = para(crate::t!("How the Reference Manager works on web").into_owned());
    let list = para(
        crate::t!(
            "This panel lists the drawing's references read-only: status, type, size, date and saved path, plus the Details pane. Select rows to inspect them; List and Tree switch the presentation."
        )
        .into_owned(),
    );
    let limits = para(
        crate::t!(
            "Attach, Refresh and Change Path stay disabled here: the web build has no filesystem access and no native file pickers, so references can be neither added, reloaded nor repathed — and previews stay unavailable. Nested references are read-only on every platform, and relative paths need the drawing saved first."
        )
        .into_owned(),
    );
    let report_hint = para(
        crate::t!(
            "Hit something broken or missing? Report it on the GitHub issues page so it can be fixed."
        )
        .into_owned(),
    );

    let report_button = button(text(crate::t!("Report an issue")).size(12))
        .on_press(Message::OpenUrl(XREF_ISSUES_URL.to_string()))
        .style(button::primary)
        .padding([6, 18]);
    let close_button = button(text(crate::t!("Close")).size(12))
        .on_press(Message::CloseModal)
        .style(button::secondary)
        .padding([6, 18]);
    let action_row: Element<'static, Message> = row![
        Space::new().width(Fill),
        report_button,
        close_button,
    ]
    .spacing(8)
    .align_y(iced::Center)
    .into();

    let content = column![
        text(crate::t!("Reference Manager — Web").into_owned())
            .size(14),
        text(crate::t!("EXTERNALREFERENCES on the web build").into_owned())
            .size(11)
            .style(muted_style),
        intro,
        list,
        limits,
        report_hint,
        Space::new().height(4),
        action_row,
    ]
    .spacing(10)
    .padding(16)
    .width(body_w);

    container(content)
        .width(sizing.width)
        .height(sizing.height)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.base.color,
            )),
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn issues_url_points_at_github_issues() {
        assert!(super::XREF_ISSUES_URL.starts_with("https://github.com/"));
        assert!(super::XREF_ISSUES_URL.contains("/issues"));
    }
}
