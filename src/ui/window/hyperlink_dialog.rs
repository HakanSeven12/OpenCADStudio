//! Insert / Edit Hyperlink window: where the link points (a file or web
//! address, or a view of this drawing), the text it shows, and how a file
//! address is stored. Used by HYPERLINK, the Properties Hyperlink row and
//! the block definition windows.

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Element, Fill, Length, Theme};

use crate::app::Message;
use crate::ui::style::common::muted_style;
use crate::ui::style::form::{button_style, dialog_button_styled_opt, field_style};
use crate::ui::window::pdf_dialogs::{card, chip, well_style};

#[derive(Debug, Clone)]
pub enum HyperlinkMsg {
    /// Link to a view of this drawing instead of a file or web page.
    ViewTarget(bool),
    View(String),
    Relative(bool),
    Dwf(bool),
    Browse,
    Help,
}

fn msg(m: HyperlinkMsg) -> Message {
    Message::HyperlinkDialog(m)
}

/// The window's state besides the address and text the older editor kept.
#[derive(Debug, Clone, Default)]
pub struct HyperlinkOptions {
    pub view_target: bool,
    /// The named location picked among `views`.
    pub view: String,
    /// Model, the layouts and the named views of this drawing.
    pub views: Vec<String>,
    pub relative: bool,
    /// A base the relative path is taken from exists (HYPERLINKBASE or the
    /// drawing's folder).
    pub relative_enabled: bool,
    /// The folder addresses resolve against, shown with the path.
    pub base: String,
    pub dwf: bool,
    /// Some target object already has a hyperlink.
    pub edit: bool,
    pub selected: usize,
    pub with_links: usize,
    /// A block definition's link: an address and text only (no view
    /// target, relative path or DWF switch, which its record doesn't keep).
    pub address_only: bool,
}

/// The full path an address resolves to, as the window shows it.
pub fn resolved_path(address: &str, base: &str) -> String {
    let (url, location) = crate::scene::hyperlink::Link::split_address(address);
    let looks_relative = !url.is_empty()
        && !url.contains("://")
        && !std::path::Path::new(&url).is_absolute()
        && !url.starts_with('\\')
        && !url.starts_with('/');
    let mut full = if looks_relative && !base.is_empty() {
        std::path::Path::new(base).join(&url).display().to_string()
    } else if url.is_empty() {
        base.to_string()
    } else {
        url
    };
    if !location.is_empty() {
        full.push('#');
        full.push_str(&location);
    }
    full
}

fn icon<'a>(bytes: &'static [u8], size: f32) -> Element<'a, Message> {
    crate::ui::icons::tinted(bytes, size, |theme: &Theme| {
        theme.palette().primary.base.color
    })
}

/// File or web page / view of this drawing, as joined buttons.
fn target_switch<'a>(view_target: bool) -> Element<'a, Message> {
    let part = |label: String, on: bool, value: bool| {
        button(text(label).size(11).width(Fill).align_x(iced::Center))
            .on_press(msg(HyperlinkMsg::ViewTarget(value)))
            .style(button_style(on))
            .padding([5, 6])
            .width(Fill)
    };
    container(
        row![
            part(
                crate::tr!("hyperlink-dialog", "target-file"),
                !view_target,
                false
            ),
            part(
                crate::tr!("hyperlink-dialog", "target-view"),
                view_target,
                true
            ),
        ]
        .spacing(2),
    )
    .padding(2)
    .width(Fill)
    .style(well_style)
    .into()
}

fn label<'a>(value: String) -> Element<'a, Message> {
    text(value)
        .size(11)
        .style(muted_style)
        .width(Length::Fixed(64.0))
        .into()
}

fn file_card<'a>(address: &'a str, options: &'a HyperlinkOptions) -> Element<'a, Message> {
    let input = text_input(&crate::tr!("hyperlink-dialog", "address-hint"), address)
        .on_input(Message::HyperlinkUrlChanged)
        .size(12)
        .padding([4, 8])
        .width(Fill)
        .style(field_style);
    let browse = button(
        row![
            icon(crate::ui::icons::FOLDER_OPEN, 13.0),
            text(crate::tr!("hyperlink-dialog", "browse")).size(11)
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .on_press(msg(HyperlinkMsg::Browse))
    .style(button_style(false))
    .padding([4, 9]);
    let path = resolved_path(address, &options.base);
    // Without a base folder (HYPERLINKBASE, or a saved drawing) the switch
    // keeps its state but cannot be changed.
    let relative: Element<'a, Message> = if options.relative_enabled {
        chip(
            crate::tr!("hyperlink-dialog", "relative"),
            options.relative,
            msg(HyperlinkMsg::Relative(!options.relative)),
        )
    } else {
        button(text(crate::tr!("hyperlink-dialog", "relative")).size(11))
            .style(button_style(options.relative))
            .padding([4, 10])
            .into()
    };
    let mut rows = column![
        row![
            label(crate::tr!("hyperlink-dialog", "address")),
            input,
            browse
        ]
        .spacing(6)
        .align_y(iced::Center),
        row![
            label(crate::tr!("hyperlink-dialog", "path")),
            text(path).size(11).style(muted_style).width(Fill)
        ]
        .spacing(6),
    ]
    .spacing(8);
    if !options.address_only {
        rows = rows.push(
            row![
                relative,
                chip(
                    crate::tr!("hyperlink-dialog", "dwf"),
                    options.dwf,
                    msg(HyperlinkMsg::Dwf(!options.dwf)),
                ),
            ]
            .spacing(6),
        );
    }
    card(crate::tr!("hyperlink-dialog", "link"), rows)
}

fn view_card<'a>(options: &'a HyperlinkOptions) -> Element<'a, Message> {
    let rows = options.views.iter().map(|name| {
        let on = name.eq_ignore_ascii_case(&options.view);
        button(text(name.as_str()).size(11).width(Fill))
            .on_press(msg(HyperlinkMsg::View(name.clone())))
            .style(button_style(on))
            .padding([4, 10])
            .width(Fill)
            .into()
    });
    card(
        crate::tr!("hyperlink-dialog", "views"),
        container(
            scrollable(column(rows.collect::<Vec<_>>()).spacing(2).width(Fill))
                .height(Length::Fixed(128.0)),
        )
        .padding(4)
        .width(Fill)
        .style(well_style),
    )
}

/// "1 object selected." and "0 objects with hyperlinks.", as the counts go.
fn counts<'a>(options: &HyperlinkOptions) -> Element<'a, Message> {
    let selected = if options.selected == 1 {
        crate::tr!("hyperlink-dialog", "selected-one")
    } else {
        format!(
            "{} {}",
            options.selected,
            crate::tr!("hyperlink-dialog", "selected-many")
        )
    };
    let linked = if options.with_links == 1 {
        crate::tr!("hyperlink-dialog", "linked-one")
    } else {
        format!(
            "{} {}",
            options.with_links,
            crate::tr!("hyperlink-dialog", "linked-many")
        )
    };
    row![
        text(selected).size(11).style(muted_style),
        Space::new().width(Fill),
        text(linked).size(11).style(muted_style)
    ]
    .into()
}

pub fn view<'a>(
    address: &'a str,
    description: &'a str,
    mixed: bool,
    options: &'a HyperlinkOptions,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let target: Element<'a, Message> = if options.view_target {
        view_card(options)
    } else {
        file_card(address, options)
    };
    let shown = card(
        crate::tr!("hyperlink-dialog", "text"),
        text_input("", description)
            .on_input(Message::HyperlinkDescriptionChanged)
            .size(12)
            .padding([4, 8])
            .width(Fill)
            .style(field_style),
    );
    let mut body = if options.address_only {
        column![target, shown]
    } else {
        column![target_switch(options.view_target), target, shown]
    }
    .spacing(10);
    if mixed {
        body = body.push(
            text(crate::t!(
                "Selected objects have different hyperlink values."
            ))
            .size(11)
            .style(muted_style),
        );
    }
    if options.selected > 0 {
        body = body.push(counts(options));
    }
    let ready = if options.view_target {
        !options.view.is_empty()
    } else {
        !address.trim().is_empty()
    };
    let mut footer = row![
        button(text("?").size(12))
            .on_press(msg(HyperlinkMsg::Help))
            .style(button_style(false))
            .padding([5, 11]),
        Space::new().width(Fill),
    ]
    .spacing(6)
    .align_y(iced::Center);
    if options.edit {
        footer = footer.push(dialog_button_styled_opt(
            crate::tr!("hyperlink-dialog", "remove"),
            Some(Message::HyperlinkRemove),
            button::danger,
        ));
    }
    footer = footer
        .push(dialog_button_styled_opt(
            crate::t!("Cancel"),
            Some(Message::HyperlinkCancel),
            button::secondary,
        ))
        .push(dialog_button_styled_opt(
            crate::t!("OK"),
            ready.then_some(Message::HyperlinkApply),
            button::primary,
        ));
    column![body, Space::new().height(Fill), footer]
        .spacing(10)
        .padding([10, 12])
        .width(sizing.width)
        .height(sizing.height)
        .into()
}
