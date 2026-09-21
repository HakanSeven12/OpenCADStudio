use crate::app::Message;
use crate::io::recovery::{RecoveryReport, RecoveryStatus};
use crate::ui::style::common::muted_style;
use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Border, Element, Fill, Length, Shrink, Theme};

fn status_style(
    status: RecoveryStatus,
) -> impl Fn(&Theme) -> iced::widget::text::Style + Copy {
    move |theme: &Theme| iced::widget::text::Style {
        color: Some(match status {
            RecoveryStatus::Recovered => theme.palette().warning.base.color,
            RecoveryStatus::Failed => theme.palette().danger.base.color,
        }),
    }
}

fn metric<'a>(label: String, value: String) -> Element<'a, Message> {
    container(
        column![
            text(label).size(10).style(muted_style),
            text(value).size(18),
        ]
        .spacing(3),
    )
    .padding([10, 12])
    .width(Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.weak.color)),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 5.0.into(),
        },
        ..Default::default()
    })
    .into()
}

pub fn view_window<'a>(
    report: &'a RecoveryReport,
    allow_save_copy: bool,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let recovered = report.status == RecoveryStatus::Recovered;
    let heading = if recovered {
        crate::tr!("recovery", "opened-with-repairs")
    } else {
        crate::tr!("recovery", "open-failed")
    };
    let description = if recovered {
        crate::tr!("recovery", "repaired-description")
    } else {
        crate::tr!("recovery", "failed-description")
    };
    let status = report.status;

    let metrics = row![
        metric(
            crate::tr!("recovery", "entities-checked"),
            report.entities_scanned.to_string(),
        ),
        metric(
            crate::tr!("recovery", "issues-found"),
            report.issues_found().to_string(),
        ),
        metric(
            crate::tr!("recovery", "entities-removed"),
            report.removed_total().to_string(),
        ),
        metric(
            crate::tr!("recovery", "references-checked"),
            report.references_checked.to_string(),
        ),
    ]
    .spacing(8)
    .width(Fill);

    let mut details = column![].spacing(6);
    if report.referenced_entities_removed > 0 {
        details = details.push(
            text(format!(
                "{}: {}",
                crate::tr!("recovery", "referenced-entities-removed"),
                report.referenced_entities_removed
            ))
            .size(11),
        );
    }
    if report.references_missing > 0 || report.references_failed > 0 {
        details = details.push(
            text(format!(
                "{}: {}",
                crate::tr!("recovery", "references-unavailable"),
                report
                    .references_missing
                    .saturating_add(report.references_failed)
            ))
            .size(11),
        );
    }
    if let Some(error) = &report.error {
        details = details.push(text(error).size(11));
    }
    for (kind, message) in report.diagnostics.iter().take(100) {
        details = details.push(text(format!("[{kind}] {message}")).size(10));
    }
    if let Some(path) = &report.log_path {
        details = details.push(
            text(format!("{}: {}", crate::tr!("recovery", "log-path"), path.display()))
                .size(10)
                .style(muted_style),
        );
    } else if let Some(error) = &report.log_error {
        details = details.push(
            text(format!(
                "{}: {error}",
                crate::tr!("recovery", "log-write-failed")
            ))
            .size(10)
            .style(status_style(RecoveryStatus::Failed)),
        );
    } else if cfg!(target_arch = "wasm32") {
        details = details.push(
            text(crate::tr!("recovery", "log-download-ready"))
                .size(10)
                .style(muted_style),
        );
    }

    let detail_panel = container(scrollable(details).height(Fill))
        .padding([10, 12])
        .width(Fill)
        .height(Fill)
        .style(container::bordered_box);

    let mut actions = row![Space::new().width(Fill)].spacing(8);
    if recovered && report.save_as_required && allow_save_copy {
        actions = actions.push(
            button(text(crate::tr!("recovery", "save-copy")).size(12))
                .on_press(Message::RecoverySaveAs)
                .style(button::primary)
                .padding([6, 14]),
        );
    }
    if report.log_path.is_some() || cfg!(target_arch = "wasm32") {
        actions = actions.push(
            button(text(crate::tr!("recovery", "show-log")).size(12))
                .on_press(Message::RecoveryShowLog)
                .style(button::secondary)
                .padding([6, 14]),
        );
    }
    actions = actions.push(
        button(text(crate::tr!("action", "close")).size(12))
            .on_press(Message::RecoveryClose)
            .style(button::secondary)
            .padding([6, 14]),
    );

    container(
        column![
            text(heading).size(20).style(status_style(status)),
            text(&report.file_name).size(13),
            text(description).size(11).style(muted_style),
            Space::new().height(4),
            metrics,
            detail_panel,
            actions,
        ]
        .spacing(10)
        .width(sizing.width)
        .height(sizing.height),
    )
    .padding([12, 16])
    .width(sizing.width)
    .height(sizing.height)
    .into()
}

pub fn view_prompt<'a>(
    file_name: &'a str,
    error: &'a str,
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
    let actions = row![
        Space::new().width(content_width),
        button(text(crate::tr!("recovery", "decline")).size(12))
            .on_press(Message::RecoveryDecline)
            .style(button::secondary)
            .padding([6, 14]),
        button(text(crate::tr!("recovery", "attempt")).size(12))
            .on_press(Message::RecoveryAttempt)
            .style(button::primary)
            .padding([6, 14]),
    ]
    .spacing(8);

    container(
        column![
            text(crate::tr!("recovery", "prompt-heading")).size(20),
            text(file_name).size(13),
            text(crate::tr!("recovery", "prompt-description"))
                .size(11)
                .style(muted_style),
            container(scrollable(text(error).size(10)).height(content_height))
                .padding([10, 12])
                .width(content_width)
                .height(content_height)
                .style(container::bordered_box),
            actions,
        ]
        .spacing(10)
        .width(sizing.width)
        .height(sizing.height),
    )
    .padding([12, 16])
    .into()
}
