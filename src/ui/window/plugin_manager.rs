//! Plugin Manager window — lists the add-ons compiled into this build and lets
//! the user enable/disable each one. A disabled plugin keeps its manifest
//! listed but drops its ribbon tab and command dispatch (persisted across
//! launches). Dynamic loading still comes with the phase-2 loader; see
//! `docs/plugin-architecture.md`.

use crate::app::Message;
use crate::plugin::external::{ExternalPlugin, RegistryEntry};
use iced::widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Fill, Theme};
use rustc_hash::{FxHashMap, FxHashSet};

/// Marketplace state passed to the Plugin Manager view.
pub struct MarketView<'a> {
    pub registry: &'a [RegistryEntry],
    pub input: &'a str,
    pub repos: &'a [String],
    pub release_tags: &'a FxHashMap<String, Vec<String>>,
    pub selected_tag: &'a FxHashMap<String, String>,
    pub status: &'a str,
}

// Register the command names for autocomplete.
inventory::submit!(crate::command::CommandRegistration {
    names: &["PLUGINS", "PLUGINMANAGER"]
});

fn muted_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.68)),
    }
}

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().primary.base.color),
    }
}

fn badge<'a>(label: String) -> Element<'a, Message> {
    container(text(label).size(11))
        .padding([2, 8])
        .style(|theme: &Theme| {
            let pair = theme.extended_palette().primary.weak;
            container::Style {
            background: Some(Background::Color(pair.color)),
            text_color: Some(pair.text),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
            }
        })
        .into()
}

fn toggle_button<'a>(id: &str, disabled: bool) -> Element<'a, Message> {
    // Label shows the action the click performs.
    let label = if disabled {
        "Enable"
    } else {
        "Disable"
    };
    let want_enabled = disabled; // clicking flips the state
    let id_owned = id.to_string();
    button(text(label).size(12))
        .padding([3, 12])
        .on_press(Message::SetPluginEnabled(id_owned, want_enabled))
        .style(if disabled { button::success } else { button::danger })
        .into()
}

#[derive(Clone, Copy)]
enum StatusKind {
    Muted,
    Success,
    Danger,
    Warning,
}

/// Coloured status pill for a discovered external package.
fn status_badge<'a>(label: &str, kind: StatusKind) -> Element<'a, Message> {
    container(text(label.to_string()).size(11))
        .padding([2, 8])
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            let pair = match kind {
                StatusKind::Muted => palette.background.weak,
                StatusKind::Success => palette.success.weak,
                StatusKind::Danger => palette.danger.weak,
                StatusKind::Warning => palette.warning.weak,
            };
            container::Style {
            background: Some(Background::Color(pair.color)),
            text_color: Some(pair.text),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
            }
        })
        .into()
}

fn external_card<'a>(p: &ExternalPlugin, loaded: bool, disabled: bool) -> Element<'a, Message> {
    let (status, kind) = if loaded && disabled {
        ("Disabled", StatusKind::Muted)
    } else if loaded {
        ("Loaded", StatusKind::Success)
    } else if !p.api_compatible() {
        ("API incompatible", StatusKind::Danger)
    } else if !p.lib_present {
        ("No library", StatusKind::Warning)
    } else {
        ("Restart to load", StatusKind::Warning)
    };
    let mut header = row![
        text(p.name.clone()).size(15),
        Space::new().width(8),
        badge(format!("v{}", p.version)),
        Space::new().width(8),
        badge(format!("API {}", p.api_version)),
        Space::new().width(Fill),
        status_badge(status, kind),
    ]
    .align_y(iced::Center);
    // A loaded plugin can be turned off (drops its ribbon tab + dispatch).
    if loaded {
        header = header.push(Space::new().width(10));
        header = header.push(toggle_button(&p.id, disabled));
    }
    header = header.push(Space::new().width(6));
    header = header.push(pill_button(
        "Uninstall",
        Message::PluginUninstall(p.id.clone()),
        button::danger,
    ));

    let id_line = text(p.id.clone()).size(11).style(primary_style);
    let mut body = column![header, id_line].spacing(5);
    if !p.description.is_empty() {
        body = body.push(text(p.description.clone()).size(12).style(muted_style));
    }
    if !p.command_prefixes.is_empty() {
        body = body.push(
            text(format!("Commands: {}", p.command_prefixes.join(", ")))
                .size(11)
                .style(muted_style),
        );
    }
    container(body.padding([12, 14]))
        .width(Fill)
        .style(container::bordered_box)
        .into()
}

fn pill_button<'a>(
    label: &str,
    msg: Message,
    style: fn(&Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    button(text(label.to_string()).size(12))
        .padding([4, 12])
        .on_press(msg)
        .style(style)
        .into()
}

/// Square icon variant of [`pill_button`] for glyph-free actions (e.g. remove).
fn pill_icon_button<'a>(
    icon: &'static [u8],
    msg: Message,
    style: fn(&Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    button(crate::ui::icons::themed(icon, 11.0))
        .padding([5, 9])
        .on_press(msg)
        .style(style)
        .into()
}

/// Release dropdown + Install (+ optional unlink) for one repo.
fn install_controls<'a>(
    repo: &str,
    tags: Vec<String>,
    selected: Option<String>,
    removable: bool,
) -> Element<'a, Message> {
    let repo_s = repo.to_string();
    let picker: Element<'_, Message> = if tags.is_empty() {
        text("no releases").size(11).style(muted_style).into()
    } else {
        let r = repo_s.clone();
        pick_list(tags, selected, move |tag| {
            Message::PluginReleaseSelect(r.clone(), tag)
        })
        .text_size(12)
        .into()
    };
    let mut controls = row![
        picker,
        Space::new().width(8),
        pill_button(
            "Install",
            Message::PluginInstall(repo_s.clone()),
            button::success,
        ),
    ]
    .align_y(iced::Center)
    .spacing(4);
    if removable {
        controls = controls.push(Space::new().width(6));
        controls = controls.push(pill_icon_button(
            crate::ui::icons::CLOSE,
            Message::PluginRepoRemove(repo_s),
            button::danger,
        ));
    }
    controls.into()
}

fn market_card<'a>(body: iced::widget::Column<'a, Message>) -> Element<'a, Message> {
    container(body.spacing(4).padding([10, 12]))
        .width(Fill)
        .style(container::bordered_box)
        .into()
}

fn marketplace_section<'a>(m: &MarketView) -> Element<'a, Message> {
    let mut col = column![text("Available plugins").size(13).style(primary_style)].spacing(6);

    // Curated registry entries (from the OpenCADStudio repo).
    for e in m.registry {
        let tags = m.release_tags.get(&e.repo).cloned().unwrap_or_default();
        let selected = m.selected_tag.get(&e.repo).cloned();
        let header = row![
            text(e.name.clone()).size(14),
            Space::new().width(Fill),
            install_controls(&e.repo, tags, selected, false),
        ]
        .align_y(iced::Center);
        let mut body = column![header, text(e.repo.clone()).size(11).style(primary_style)];
        if !e.description.is_empty() {
            body = body.push(text(e.description.clone()).size(12).style(muted_style));
        }
        col = col.push(market_card(body));
    }

    // Manual: link any repo by owner/repo.
    col = col.push(Space::new().height(6));
    col = col.push(text("Add a repository").size(12).style(muted_style));
    col = col.push(
        row![
            text_input("owner/repo", m.input)
                .on_input(Message::PluginRepoInput)
                .on_submit(Message::PluginRepoAdd)
                .size(13)
                .width(Fill),
            Space::new().width(8),
            pill_button("Add", Message::PluginRepoAdd, button::primary),
        ]
        .align_y(iced::Center),
    );
    for repo in m.repos {
        let tags = m.release_tags.get(repo).cloned().unwrap_or_default();
        let selected = m.selected_tag.get(repo).cloned();
        let header = row![
            text(repo.clone()).size(13),
            Space::new().width(Fill),
            install_controls(repo, tags, selected, true),
        ]
        .align_y(iced::Center);
        col = col.push(market_card(column![header]));
    }

    if !m.status.is_empty() {
        col = col.push(text(m.status.to_string()).size(11).style(muted_style));
    }
    col.into()
}

pub fn view_window<'a>(
    disabled: &FxHashSet<String>,
    externals: &[ExternalPlugin],
    loaded: &FxHashSet<String>,
    market: MarketView,
) -> Element<'a, Message> {
    let title = text("Plugins").size(20);
    let subtitle = text("Add-ons load from the plugins folder. Install from a repository below.")
        .size(12)
        .style(muted_style);

    let mut list = column![].spacing(10);
    // Installed external packages (from the plugins folder).
    if externals.is_empty() {
        list = list.push(text("No plugins installed yet.").size(13).style(muted_style));
    } else {
        list = list.push(text("Installed").size(13).style(primary_style));
        for p in externals {
            list = list.push(external_card(
                p,
                loaded.contains(&p.id),
                disabled.contains(&p.id),
            ));
        }
    }
    // Marketplace: install from a linked repository's releases.
    list = list.push(Space::new().height(14));
    list = list.push(marketplace_section(&market));
    let body: Element<'_, Message> = scrollable(list.width(Fill)).height(Fill).into();

    container(
        column![title, subtitle, Space::new().height(12), body]
            .spacing(4)
            .padding(20)
            .width(Fill)
            .height(Fill),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.base.color,
        )),
        ..Default::default()
    })
    .width(Fill)
    .height(Fill)
    .into()
}
