// Missing-fonts prompt — shown after a drawing opens with `.shx` fonts this
// machine doesn't have. Offers to fetch them from the community repository
// (see `fonts/README.md` in the repo root) instead of rendering with a
// mismatched substitute.

use crate::app::Message;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill, Length, Shrink, Theme};

fn muted_style(theme: &Theme) -> text::Style {
    text::Style {
        color: Some(theme.palette().background.weak.text),
    }
}

/// The prompt: the missing-font list, a download action, and a skip.
pub fn view_window<'a>(
    fonts: &'a [String],
    font_source: &'a str,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let content_width = if matches!(sizing.width, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let content_height = if matches!(sizing.height, Length::Fill) {
        Fill
    } else {
        Shrink
    };
    let listed: Vec<Element<'a, Message>> = fonts
        .iter()
        .map(|name| text(format!("  {name}")).size(12).into())
        .collect();
    let actions = row![
        Space::new().width(content_width),
        button(text(crate::t!("Skip")).size(12))
            .on_press(Message::MissingFontsDismiss)
            .style(button::secondary)
            .padding([6, 14]),
        button(text(crate::t!("Download")).size(12))
            .on_press(Message::MissingFontsDownload)
            .style(button::primary)
            .padding([6, 14]),
    ]
    .spacing(8);

    container(
        column![
            text(crate::t!("Missing fonts")).size(20),
            text(crate::t!(
                "This drawing uses fonts that are not installed on this machine. Download them from the OpenCADStudio community font repository?"
            ))
            .size(11)
            .style(muted_style),
            container(scrollable(column(listed).spacing(2)).height(content_height))
                .padding([10, 12])
                .width(content_width)
                .height(content_height)
                .style(container::bordered_box),
            column![
                text(crate::t!("Font source — leave empty for the community repository, or point at any folder (e.g. your company's font server):"))
                    .size(10)
                    .style(muted_style),
                text_input(
                    crate::t!("https://server/fonts or a GitHub raw folder").as_ref(),
                    font_source,
                )
                .on_input(Message::MissingFontsSourceChanged)
                .size(11)
                .padding([4, 8])
                .width(content_width),
            ]
            .spacing(3),
            actions,
        ]
        .spacing(10)
        .width(sizing.width)
        .height(sizing.height),
    )
    .padding([12, 16])
    .into()
}
