use crate::app::Message;
use crate::t;
use crate::ui::style::common::muted_style as muted;
use iced::widget::{button, checkbox, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Length, Theme};

fn button_style(primary: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        if primary {
            button::primary(theme, status)
        } else {
            button::secondary(theme, status)
        }
    }
}

pub fn view_window<'a>(
    layouts: &'a [(String, bool)],
    printer: Option<&'a str>,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let selected = layouts.iter().filter(|(_, checked)| *checked).count();
    let rows: Vec<Element<'a, Message>> = layouts
        .iter()
        .map(|(name, checked)| {
            let layout = name.clone();
            container(
                checkbox(*checked)
                    .label(name.clone())
                    .on_toggle(move |_| Message::PrintAllToggle(layout.clone()))
                    .size(16)
                    .text_size(12),
            )
            .padding([6, 8])
            .width(Length::Fill)
            .into()
        })
        .collect();
    let list: Element<'a, Message> = if rows.is_empty() {
        container(text(t!("No paper layouts are available.")).size(12).style(muted))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    } else {
        scrollable(column(rows).spacing(2)).height(sizing.height).into()
    };
    let printer = printer
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| t!("Default printer").into_owned());
    let summary = if selected == 1 {
        t!("1 layout selected").into_owned()
    } else {
        format!("{selected} {}", t!("layouts selected"))
    };

    let mut pdf = button(text(t!("PDF")).size(11))
        .style(button_style(false))
        .padding([5, 16]);
    let mut print = button(text(t!("Print")).size(11))
        .style(button_style(true))
        .padding([5, 18]);
    if selected > 0 {
        pdf = pdf.on_press(Message::PrintAllPdf);
        print = print.on_press(Message::PrintAllPrint);
    }

    let list_panel = container(list)
        .width(sizing.width)
        .height(sizing.height)
        .padding(4)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            border: Border {
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

    container(
        column![
            text(t!("Select the layouts to output. Each page is printed in Layout mode."))
                .size(12),
            row![
                button(text(t!("Select all")).size(11))
                    .on_press(Message::PrintAllSelectAll)
                    .style(button_style(false))
                    .padding([4, 10]),
                button(text(t!("Select none")).size(11))
                    .on_press(Message::PrintAllSelectNone)
                    .style(button_style(false))
                    .padding([4, 10]),
                Space::new().width(Length::Fill),
                text(summary).size(11).style(muted),
            ]
            .spacing(6)
            .align_y(iced::Center),
            list_panel,
            row![
                text(format!("{}: {printer}", t!("Printer")))
                    .size(11)
                    .style(muted),
                Space::new().width(Length::Fill),
                button(text(t!("Options…")).size(11))
                    .on_press(Message::PrintAllOptions)
                    .style(button_style(false))
                    .padding([5, 12]),
                pdf,
                print,
            ]
            .spacing(7)
            .align_y(iced::Center),
        ]
        .spacing(10),
    )
    .padding(14)
    .width(sizing.width)
    .height(sizing.height)
    .into()
}
