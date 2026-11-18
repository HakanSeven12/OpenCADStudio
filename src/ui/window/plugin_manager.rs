//! Plugin Manager window — lists the add-ons compiled into this build and lets
//! the user enable/disable each one. A disabled plugin keeps its manifest
//! listed but drops its ribbon tab and command dispatch (persisted across
//! launches). Dynamic loading still comes with the phase-2 loader; see
//! `docs/plugin-architecture.md`.

#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use crate::app::Message;
use crate::plugin::external::{ExternalPlugin, RegistryEntry, ReleaseInfo};
use crate::t;
use crate::ui::style::common::muted_style;
use iced::widget::{
    button, canvas, column, container, markdown, row, rule, scrollable, text, text_input, Space,
};
use iced::{Background, Border, Element, Fill, Length, Theme};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;

/// Empty lane kept at the right edge of scroll content so the vertical
/// scrollbar never covers card controls.
const SCROLLBAR_GUTTER: f32 = 16.0;

/// Marketplace state passed to the Plugin Manager view.
pub struct MarketView<'a> {
    pub registry: &'a [RegistryEntry],
    pub registry_loading: bool,
    pub registry_error: Option<&'a str>,
    pub registry_error_details_open: bool,
    pub input: &'a str,
    pub search: &'a str,
    pub repos: &'a [String],
    pub release_tags: &'a FxHashMap<String, Vec<ReleaseInfo>>,
    pub selected_tag: &'a FxHashMap<String, String>,
    pub selected_repo: Option<&'a str>,
    pub readmes: &'a FxHashMap<String, Result<markdown::Content, String>>,
    pub readme_loading: &'a FxHashSet<String>,
    /// Repositories whose release list is being fetched.
    pub releases_loading: &'a FxHashSet<String>,
    /// Repositories whose install or update is downloading.
    pub installing: &'a FxHashSet<String>,
    /// Repositories installed this session, waiting for a restart: `true` for
    /// a fresh install, `false` for an update.
    pub installed_now: &'a FxHashMap<String, bool>,
    /// Names of the AI tools each loaded plugin offers.
    pub ai_tools: FxHashMap<String, Vec<String>>,
    pub status: &'a str,
}

/// Latest desktop builds, used by the browser-only plugin notice.
#[cfg(target_arch = "wasm32")]
pub const DESKTOP_DOWNLOAD_URL: &str =
    "https://github.com/HakanSeven12/OpenCADStudio/releases/latest";

// Register the command names for autocomplete. On the web they open a desktop
// download notice instead of the native marketplace.
inventory::submit!(crate::command::CommandRegistration {
    names: &["PLUGINS", "PLUGINMANAGER", "APPLOAD"]
});

fn primary_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary.base.color),
    }
}

fn badge<'a>(label: String) -> Element<'a, Message> {
    container(text(label).size(11))
        .padding([2, 8])
        .style(|theme: &Theme| {
            let pair = theme.palette().primary.weak;
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
        t!("Enable")
    } else {
        t!("Disable")
    };
    let want_enabled = disabled; // clicking flips the state
    let id_owned = id.to_string();
    button(text(label).size(12))
        .padding([3, 12])
        .on_press(Message::SetPluginEnabled(id_owned, want_enabled))
        .style(if disabled {
            button::success
        } else {
            button::danger
        })
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
fn status_badge<'a>(label: Cow<'static, str>, kind: StatusKind) -> Element<'a, Message> {
    container(text(label).size(11))
        .padding([2, 8])
        .style(move |theme: &Theme| {
            let palette = theme.palette();
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

fn card_style(theme: &Theme, selected: bool) -> container::Style {
    let palette = theme.palette();
    container::Style {
        background: selected
            .then(|| Background::Color(palette.primary.weak.color.scale_alpha(0.18))),
        border: Border {
            color: if selected {
                palette.primary.base.color
            } else {
                palette.background.strong.color
            },
            width: if selected { 1.5 } else { 1.0 },
            radius: 6.0.into(),
        },
        ..Default::default()
    }
}

fn repository_for_external(plugin: &ExternalPlugin, registry: &[RegistryEntry]) -> Option<String> {
    plugin.repository.clone().or_else(|| {
        registry
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(&plugin.name))
            .map(|entry| entry.repo.clone())
    })
}

fn matches_search(query: &str, values: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || values
            .iter()
            .any(|value| value.to_lowercase().contains(&query))
}

fn external_matches_search(plugin: &ExternalPlugin, repository: Option<&str>, query: &str) -> bool {
    matches_search(
        query,
        &[
            &plugin.name,
            &plugin.id,
            &plugin.description,
            repository.unwrap_or_default(),
        ],
    ) || plugin
        .command_prefixes
        .iter()
        .any(|command| matches_search(query, &[command]))
}

pub(crate) fn repository_is_installed(
    repository: &str,
    registry_name: Option<&str>,
    externals: &[ExternalPlugin],
) -> bool {
    externals.iter().any(|plugin| {
        plugin.repository.as_deref() == Some(repository)
            || registry_name.is_some_and(|name| plugin.name.eq_ignore_ascii_case(name))
    })
}

fn trim_version_prefix(value: &str) -> &str {
    value.trim_start_matches(['v', 'V'])
}

fn newest_update(installed: &str, releases: &[ReleaseInfo]) -> Option<String> {
    let installed = semver::Version::parse(trim_version_prefix(installed)).ok()?;
    releases
        .iter()
        .filter(|release| release_compatible(release))
        .filter_map(|release| {
            semver::Version::parse(trim_version_prefix(&release.tag))
                .ok()
                .map(|version| (version, release.tag.clone()))
        })
        .filter(|(version, _)| version > &installed)
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, tag)| tag)
}

fn external_card<'a>(
    p: &ExternalPlugin,
    repository: Option<String>,
    update_tag: Option<String>,
    loaded: bool,
    load_error: Option<&str>,
    disabled: bool,
    selected: bool,
    progress: Option<Element<'a, Message>>,
    ai_tools: Option<&[String]>,
) -> Element<'a, Message> {
    let failed_old_api = load_error.is_some() && !p.api_compatible();
    let installed = InstalledStatus::of(p, loaded, disabled, load_error);
    let (status, kind) = (installed.label(), installed.kind());
    let header = row![
        text(p.name.clone()).size(15),
        Space::new().width(8),
        badge(format!("v{}", p.version)),
        Space::new().width(8),
        badge(format!("API {}", p.api_version)),
        Space::new().width(Fill),
        status_badge(status, kind),
    ]
    .align_y(iced::Center);

    let id_line = text(p.id.clone()).size(11).style(muted_style);
    let mut info_body = column![header, id_line].spacing(5);
    if !p.description.is_empty() {
        info_body = info_body.push(text(p.description.clone()).size(12).style(muted_style));
    }
    if !p.command_prefixes.is_empty() {
        info_body = info_body.push(
            text(t!(
                "Commands:  %{cmds}",
                cmds = p.command_prefixes.join(", ")
            ))
            .size(11)
            .style(muted_style),
        );
    }
    if let Some(tools) = ai_tools {
        info_body = info_body.push(
            text(t!("AI tools:  %{tools}", tools = tools.join(", ")))
                .size(11)
                .style(muted_style),
        );
    }
    if let Some(error) = load_error {
        let detail = if failed_old_api {
            t!(
                "Plugin API  %{plugin}  is incompatible with this build's API  %{build}  ribbon ABI. Update or rebuild the plugin.",
                plugin = p.api_version,
                build = ocs_plugin_api::API_VERSION,
            )
        } else {
            t!("Load failed:  %{error}", error = error)
        };
        info_body = info_body.push(text(detail).size(11).width(Fill).style(|theme: &Theme| {
            iced::widget::text::Style {
                color: Some(theme.palette().danger.base.color),
            }
        }));
    }

    let info: Element<'a, Message> = if let Some(repo) = repository.clone() {
        button(info_body.width(Fill))
            .width(Fill)
            .padding(0)
            .style(button::text)
            .on_press(Message::PluginReadmeSelect(repo))
            .into()
    } else {
        info_body.width(Fill).into()
    };

    let mut actions = row![Space::new().width(Fill)].align_y(iced::Center);
    if let Some(progress) = progress {
        actions = actions.push(progress);
        actions = actions.push(Space::new().width(6));
    } else if let (Some(repo), Some(tag)) = (repository, update_tag) {
        actions = actions.push(pill_button(
            t!("Update to  %{tag}", tag = tag),
            Message::PluginUpdate(repo, tag),
            button::primary,
        ));
        actions = actions.push(Space::new().width(6));
    }
    // A loaded plugin can be turned off (drops its ribbon tab + dispatch).
    if loaded {
        actions = actions.push(toggle_button(&p.id, disabled));
        actions = actions.push(Space::new().width(6));
    }
    if p.bundled {
        actions = actions.push(
            text(t!("Bundled with Open CAD Studio"))
                .size(11)
                .style(muted_style),
        );
    } else {
        actions = actions.push(pill_button(
            t!("Uninstall"),
            Message::PluginUninstall(p.id.clone()),
            button::danger,
        ));
    }

    container(column![info, actions].spacing(8).padding([10, 12]))
        .width(Fill)
        .style(move |theme: &Theme| card_style(theme, selected))
        .into()
}

fn pill_button<'a>(
    label: impl Into<Cow<'static, str>>,
    msg: Message,
    style: fn(&Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    button(text(label.into()).size(12))
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

/// A turning arc in its colour, drawn afresh each frame from the wall clock;
/// the view is redrawn while an install or a release check runs.
struct Spinner(fn(&Theme) -> iced::Color);

impl canvas::Program<Message> for Spinner {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: iced::Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis() % 1000);
        let start = millis as f32 / 1000.0 * std::f32::consts::TAU;
        let arc = canvas::Path::new(|path| {
            path.arc(canvas::path::Arc {
                center: frame.center(),
                radius: bounds.width.min(bounds.height) / 2.0 - 1.5,
                start_angle: iced::Radians(start),
                end_angle: iced::Radians(start + 4.5),
            });
        });
        frame.stroke(
            &arc,
            canvas::Stroke::default()
                .with_width(2.0)
                .with_color((self.0)(theme))
                .with_line_cap(canvas::LineCap::Round),
        );
        vec![frame.into_geometry()]
    }
}

/// What an install or update button turns into once clicked: a disabled
/// button with a spinner while it downloads, then Restart to load the plugin.
fn progress_action<'a>(m: &MarketView, repo: &str) -> Option<Element<'a, Message>> {
    if m.installing.contains(repo) {
        let content = row![
            canvas(Spinner(|theme| theme.palette().success.base.text))
                .width(12)
                .height(12),
            text(t!("Installing…")).size(12),
        ]
        .spacing(6)
        .align_y(iced::Center);
        // No press while it runs, but drawn as the live button it replaced.
        return Some(
            button(content)
                .padding([4, 12])
                .style(|theme: &Theme, _| button::success(theme, button::Status::Active))
                .into(),
        );
    }
    m.installed_now.contains_key(repo).then(|| {
        pill_button(t!("Restart"), Message::PluginRestart, button::primary)
    })
}

/// Whether this build can install `release`.
pub(crate) fn release_compatible(release: &ReleaseInfo) -> bool {
    ocs_plugin_api::manifest::host_accepts_plugin_version(release.api_version)
        && release.acadrust_compatible
        && release.rustc_compatible
}

/// A repository whose releases are fetched and none of them installs here
/// (none for this platform, or none this build accepts).
fn market_incompatible(m: &MarketView, repo: &str) -> bool {
    !m.installed_now.contains_key(repo)
        && m.release_tags
            .get(repo)
            .is_some_and(|releases| !releases.iter().any(release_compatible))
}

/// Where an installed package stands; the card's pill and the automation
/// `plugins` read both come from here.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstalledStatus {
    Disabled,
    Loaded,
    ApiIncompatible,
    Incompatible,
    LoadFailed,
    NoLibrary,
    RestartToLoad,
}

impl InstalledStatus {
    pub(crate) fn of(
        p: &ExternalPlugin,
        loaded: bool,
        disabled: bool,
        load_error: Option<&str>,
    ) -> Self {
        if loaded && disabled {
            Self::Disabled
        } else if loaded {
            Self::Loaded
        } else if !p.api_compatible() {
            Self::ApiIncompatible
        } else if (p.acadrust_declared && !p.acadrust_compatible())
            || (ocs_plugin_api::version_info::uses_acadrust_gate(p.api_version)
                && !p.rustc_compatible())
        {
            Self::Incompatible
        } else if load_error.is_some() {
            Self::LoadFailed
        } else if !p.lib_present {
            Self::NoLibrary
        } else {
            Self::RestartToLoad
        }
    }

    /// This build cannot run the package as installed.
    pub(crate) fn incompatible(self) -> bool {
        matches!(self, Self::ApiIncompatible | Self::Incompatible)
    }

    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Loaded => "loaded",
            Self::ApiIncompatible => "api_incompatible",
            Self::Incompatible => "incompatible",
            Self::LoadFailed => "load_failed",
            Self::NoLibrary => "no_library",
            Self::RestartToLoad => "restart_to_load",
        }
    }

    fn label(self) -> Cow<'static, str> {
        match self {
            Self::Disabled => t!("Disabled"),
            Self::Loaded => t!("Loaded"),
            Self::ApiIncompatible => t!("API incompatible"),
            Self::Incompatible => t!("Incompatible"),
            Self::LoadFailed => t!("Load failed"),
            Self::NoLibrary => t!("No library"),
            Self::RestartToLoad => t!("Restart to load"),
        }
    }

    fn kind(self) -> StatusKind {
        match self {
            Self::Disabled => StatusKind::Muted,
            Self::Loaded => StatusKind::Success,
            Self::ApiIncompatible | Self::Incompatible | Self::LoadFailed => StatusKind::Danger,
            Self::NoLibrary | Self::RestartToLoad => StatusKind::Warning,
        }
    }
}

/// Release dropdown + Install (+ optional unlink) for one repo.
fn install_controls<'a>(
    m: &MarketView,
    repo: &str,
    releases: Vec<ReleaseInfo>,
    selected: Option<String>,
    removable: bool,
    progress: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    if let Some(progress) = progress {
        return progress;
    }
    let repo_s = repo.to_string();
    let tags = releases
        .iter()
        .map(|release| release.tag.clone())
        .collect::<Vec<_>>();
    let selected_release = selected
        .as_ref()
        .and_then(|selected| releases.iter().find(|release| release.tag == *selected));
    // Releases not fetched yet are still being checked, not missing.
    if tags.is_empty() && m.releases_loading.contains(repo) {
        return row![
            canvas(Spinner(|theme| theme.palette().background.strong.text))
                .width(12)
                .height(12),
            text(t!("Checking releases…")).size(11).style(muted_style),
        ]
        .spacing(6)
        .align_y(iced::Center)
        .into();
    }
    let picker: Element<'_, Message> = if tags.is_empty() {
        text(t!("no releases")).size(11).style(muted_style).into()
    } else {
        let r = repo_s.clone();
        crate::ui::dropdown_list::pick_list(selected, tags, |value| value.to_string())
            .on_select(move |tag| Message::PluginReleaseSelect(r.clone(), tag))
            .text_size(12)
            .into()
    };
    let action = match selected_release {
        Some(release) if release_compatible(release) => {
            pill_button(
                t!("Install"),
                Message::PluginInstall(repo_s.clone()),
                button::success,
            )
        }
        Some(_) => status_badge(t!("Incompatible"), StatusKind::Danger),
        None => status_badge(t!("Unavailable"), StatusKind::Muted),
    };
    let mut controls = row![picker, Space::new().width(8), action]
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

fn market_card<'a>(
    body: iced::widget::Column<'a, Message>,
    selected: bool,
) -> Element<'a, Message> {
    container(body.spacing(4).padding([10, 12]))
        .width(Fill)
        .style(move |theme: &Theme| card_style(theme, selected))
        .into()
}

fn repository_display_name(repository: &str) -> String {
    repository
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(repository)
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            if part.eq_ignore_ascii_case("opencad") {
                "OpenCAD".to_string()
            } else {
                let mut chars = part.chars();
                chars
                    .next()
                    .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn add_repository_card<'a>(m: &MarketView) -> Element<'a, Message> {
    let form = row![
        text_input(t!("GitHub URL or owner/repository").as_ref(), m.input)
            .on_input(Message::PluginRepoInput)
            .on_submit(Message::PluginRepoAdd)
            .size(13)
            .width(Fill),
        Space::new().width(8),
        pill_button(
            t!("Add repository"),
            Message::PluginRepoAdd,
            button::primary,
        ),
    ]
    .align_y(iced::Center);

    market_card(
        column![
            text(t!("Add from GitHub")).size(14),
            text(t!(
                "Paste a public repository URL. Compatible releases and the README are detected automatically."
            ))
            .size(11)
            .style(muted_style),
            Space::new().height(3),
            form,
        ]
        .spacing(5),
        false,
    )
}

fn registry_error_message(error: &str) -> (Cow<'static, str>, Cow<'static, str>) {
    let error = error.to_ascii_lowercase();
    if error.contains("certificate")
        || error.contains("unknownissuer")
        || error.contains("unknown issuer")
    {
        (
            t!("Unable to verify the server certificate"),
            t!(
                "Open CAD Studio could not trust the certificate presented for the plugin registry. Check your system certificate and proxy settings, then retry."
            ),
        )
    } else if error.contains("timed out") || error.contains("timeout") {
        (
            t!("Plugin registry request timed out"),
            t!(
                "Check your internet or proxy connection, then retry. Manually added repositories remain available."
            ),
        )
    } else {
        (
            t!("Unable to load the plugin registry"),
            t!(
                "Check your internet or proxy connection, then retry. Manually added repositories remain available."
            ),
        )
    }
}

fn registry_notice<'a>(m: &MarketView) -> Option<Element<'a, Message>> {
    if let Some(error) = m.registry_error {
        let (title, message) = registry_error_message(error);
        let actions = row![
            pill_button(t!("Retry"), Message::PluginRegistryRetry, button::primary,),
            Space::new().width(6),
            pill_button(
                if m.registry_error_details_open {
                    t!("Hide details")
                } else {
                    t!("Show details")
                },
                Message::PluginRegistryErrorDetailsToggle,
                button::secondary,
            ),
            Space::new().width(6),
            pill_button(
                t!("Copy details"),
                Message::PluginRegistryCopyDiagnostics,
                button::secondary,
            ),
        ]
        .align_y(iced::Center);
        let mut body = column![
            text(title).size(13),
            text(message).size(11).style(muted_style),
            Space::new().height(3),
            actions,
        ]
        .spacing(5);
        if m.registry_error_details_open {
            body = body.push(
                container(text(error.to_string()).size(10).width(Fill))
                    .padding(8)
                    .width(Fill)
                    .style(container::bordered_box),
            );
        }
        return Some(
            container(body.padding([10, 12]))
                .width(Fill)
                .style(|theme: &Theme| {
                    let pair = theme.palette().warning.weak;
                    container::Style {
                        background: Some(Background::Color(pair.color.scale_alpha(0.16))),
                        border: Border {
                            color: pair.color,
                            width: 1.0,
                            radius: 6.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .into(),
        );
    }

    (m.registry_loading && m.registry.is_empty()).then(|| {
        container(
            column![
                text(t!("Loading plugin catalog…")).size(13),
                text(t!(
                    "Connecting securely using your system certificate settings."
                ))
                .size(11)
                .style(muted_style),
            ]
            .spacing(5)
            .padding([10, 12]),
        )
        .width(Fill)
        .style(container::bordered_box)
        .into()
    })
}

/// The store: compatible entries here, entries no release of which fits this
/// build pushed onto `incompatible` for the list's last section.
fn marketplace_section<'a>(
    m: &MarketView,
    externals: &[ExternalPlugin],
    incompatible: &mut Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut col = column![text(t!("Available plugins")).size(13).style(primary_style)].spacing(6);
    let mut visible = 0usize;
    if let Some(notice) = registry_notice(m) {
        col = col.push(notice);
    }

    // Curated registry entries (from the OpenCADStudio repo).
    for e in m.registry {
        // A plugin installed just now keeps its card, its button a Restart.
        if (repository_is_installed(&e.repo, Some(&e.name), externals)
            && m.installed_now.get(&e.repo) != Some(&true))
            || !matches_search(m.search, &[&e.name, &e.description, &e.repo])
        {
            continue;
        }
        let tags = m.release_tags.get(&e.repo).cloned().unwrap_or_default();
        let selected = m.selected_tag.get(&e.repo).cloned();
        let mut info = column![text(e.name.clone()).size(14)].spacing(4);
        if !e.description.is_empty() {
            info = info.push(text(e.description.clone()).size(12).style(muted_style));
        }
        let info = button(info.width(Fill))
            .width(Fill)
            .padding(0)
            .style(button::text)
            .on_press(Message::PluginReadmeSelect(e.repo.clone()));
        let actions = row![
            Space::new().width(Fill),
            install_controls(m, &e.repo, tags, selected, false, progress_action(m, &e.repo)),
        ]
        .align_y(iced::Center);
        let card = market_card(
            column![info, actions].spacing(8),
            m.selected_repo == Some(e.repo.as_str()),
        );
        if market_incompatible(m, &e.repo) {
            incompatible.push(card);
        } else {
            visible += 1;
            col = col.push(card);
        }
    }

    // User-linked repositories join the same catalog, but curated and installed
    // repositories are suppressed so every plugin appears only once.
    for repo in m.repos {
        if m.registry.iter().any(|entry| entry.repo == *repo)
            || (repository_is_installed(repo, None, externals)
                && m.installed_now.get(repo) != Some(&true))
        {
            continue;
        }
        let display_name = repository_display_name(repo);
        if !matches_search(m.search, &[&display_name, repo]) {
            continue;
        }
        let tags = m.release_tags.get(repo).cloned().unwrap_or_default();
        let selected = m.selected_tag.get(repo).cloned();
        let info = button(
            column![
                text(display_name).size(14),
                text(t!("Added from GitHub")).size(11).style(muted_style),
            ]
            .spacing(4)
            .width(Fill),
        )
        .width(Fill)
        .padding(0)
        .style(button::text)
        .on_press(Message::PluginReadmeSelect(repo.clone()));
        let actions = row![
            Space::new().width(Fill),
            install_controls(m, repo, tags, selected, true, progress_action(m, repo)),
        ]
        .align_y(iced::Center);
        let card = market_card(
            column![info, actions].spacing(8),
            m.selected_repo == Some(repo.as_str()),
        );
        if market_incompatible(m, repo) {
            incompatible.push(card);
        } else {
            visible += 1;
            col = col.push(card);
        }
    }

    if visible == 0 && m.registry_error.is_none() && !(m.registry_loading && m.registry.is_empty())
    {
        let message = if m.search.trim().is_empty() {
            t!("No additional plugins are available.")
        } else {
            t!("No available plugins match your search.")
        };
        col = col.push(text(message).size(12).style(muted_style));
    }

    col = col.push(Space::new().height(8));
    col = col.push(add_repository_card(m));
    if !m.status.is_empty() {
        col = col.push(text(m.status.to_string()).size(11).style(muted_style));
    }
    col.into()
}

fn resolve_readme_link(repo: &str, uri: &str) -> String {
    if uri.starts_with("https://") || uri.starts_with("http://") || uri.starts_with("mailto:") {
        uri.to_string()
    } else if uri.starts_with('/') {
        format!("https://github.com{uri}")
    } else if uri.starts_with('#') {
        format!("https://github.com/{repo}{uri}")
    } else {
        format!(
            "https://github.com/{repo}/blob/HEAD/{}",
            uri.trim_start_matches("./")
        )
    }
}

fn readme_panel<'a>(
    market: &MarketView<'a>,
    theme: &Theme,
    width: Length,
    height: Length,
) -> Element<'a, Message> {
    let Some(repo) = market.selected_repo else {
        return container(
            column![
                text(t!("Plugin details")).size(16),
                text(t!("Select a plugin to read its GitHub README."))
                    .size(12)
                    .style(muted_style),
            ]
            .spacing(8),
        )
        .center_x(width)
        .center_y(height)
        .width(width)
        .height(height)
        .style(container::bordered_box)
        .into();
    };

    let name = repo
        .rsplit_once('/')
        .map(|(_, name)| name)
        .unwrap_or(repo)
        .to_string();
    let header = row![
        column![
            text(name).size(16),
            text(repo.to_string()).size(11).style(primary_style),
        ]
        .spacing(3)
        .width(width),
        pill_button(
            t!("View on GitHub"),
            Message::OpenUrl(format!("https://github.com/{repo}")),
            button::secondary,
        ),
    ]
    .align_y(iced::Center);

    let content: Element<'a, Message> = if market.readme_loading.contains(repo) {
        container(
            column![
                text(t!("Loading README…")).size(13),
                text(t!("Fetching the default branch from GitHub."))
                    .size(11)
                    .style(muted_style),
            ]
            .spacing(6),
        )
        .center_x(width)
        .center_y(height)
        .width(width)
        .height(height)
        .into()
    } else {
        match market.readmes.get(repo) {
            Some(Ok(readme)) => {
                let source_repo = repo.to_string();
                markdown::view(
                    readme.items(),
                    markdown::Settings::with_text_size(13, theme),
                )
                .map(move |uri| Message::OpenUrl(resolve_readme_link(&source_repo, &uri)))
            }
            Some(Err(error)) => container(
                column![
                    text(t!("README could not be loaded")).size(14),
                    text(error.clone()).size(11).style(muted_style),
                    Space::new().height(4),
                    pill_button(
                        t!("Retry"),
                        Message::PluginReadmeSelect(repo.to_string()),
                        button::secondary,
                    ),
                ]
                .spacing(6),
            )
            .center_x(width)
            .center_y(height)
            .width(width)
            .height(height)
            .into(),
            None => container(
                text(t!("Select the plugin again to load its README."))
                    .size(12)
                    .style(muted_style),
            )
            .center_x(width)
            .center_y(height)
            .width(width)
            .height(height)
            .into(),
        }
    };

    let gutter = iced::Padding {
        top: 8.0,
        right: SCROLLBAR_GUTTER,
        bottom: 8.0,
        left: 0.0,
    };
    let readme = scrollable(container(content).padding(gutter))
        .height(height)
        .width(width);

    container(
        column![header, rule::horizontal(1), readme]
            .spacing(10)
            .padding([12, 14])
            .width(width)
            .height(height),
    )
    .width(width)
    .height(height)
    .style(container::bordered_box)
    .into()
}

pub fn view_window<'a>(
    disabled: &FxHashSet<String>,
    externals: &[ExternalPlugin],
    loaded: &FxHashSet<String>,
    load_errors: &FxHashMap<String, String>,
    market: MarketView<'a>,
    theme: &'a Theme,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let width = sizing.width;
    let height = sizing.height;
    let title = text(t!("Plugins")).size(20);
    let subtitle = text(t!(
        "Browse, install, and manage add-ons. Select one to view its README."
    ))
    .size(12)
    .style(muted_style);

    let mut list = column![].spacing(10);
    // Installed first, then the store, then whatever this build cannot run.
    let mut incompatible: Vec<Element<'a, Message>> = Vec::new();
    // Installed external packages (from the plugins folder).
    if externals.is_empty() {
        list = list.push(
            text(t!("No plugins installed yet."))
                .size(13)
                .style(muted_style),
        );
    } else {
        let mut visible_installed = 0usize;
        for p in externals {
            let repository = repository_for_external(p, market.registry);
            if !external_matches_search(p, repository.as_deref(), market.search) {
                continue;
            }
            // Installed just now: its store card carries the Restart button.
            if repository
                .as_ref()
                .is_some_and(|repo| market.installed_now.get(repo) == Some(&true))
            {
                continue;
            }
            let selected = repository.as_deref() == market.selected_repo;
            let update_tag = repository
                .as_ref()
                .and_then(|repo| market.release_tags.get(repo))
                .and_then(|releases| newest_update(&p.version, releases));
            let progress = repository
                .as_deref()
                .and_then(|repo| progress_action(&market, repo));
            let load_error = load_errors.get(&p.id).map(String::as_str);
            let card = external_card(
                p,
                repository,
                update_tag,
                loaded.contains(&p.id),
                load_error,
                disabled.contains(&p.id),
                selected,
                progress,
                market.ai_tools.get(&p.id).map(Vec::as_slice),
            );
            if InstalledStatus::of(
                p,
                loaded.contains(&p.id),
                disabled.contains(&p.id),
                load_error,
            )
            .incompatible()
            {
                incompatible.push(card);
                continue;
            }
            if visible_installed == 0 {
                list = list.push(text(t!("Installed")).size(13).style(primary_style));
            }
            visible_installed += 1;
            list = list.push(card);
        }
        if visible_installed == 0 && !market.search.trim().is_empty() {
            list = list.push(
                text(t!("No installed plugins match your search."))
                    .size(12)
                    .style(muted_style),
            );
        }
    }
    // Marketplace: install from a linked repository's releases.
    list = list.push(Space::new().height(14));
    list = list.push(marketplace_section(&market, externals, &mut incompatible));
    if !incompatible.is_empty() {
        list = list.push(Space::new().height(14));
        list = list.push(text(t!("Incompatible")).size(13).style(primary_style));
        for card in incompatible {
            list = list.push(card);
        }
    }
    let gutter = iced::Padding {
        top: 0.0,
        right: SCROLLBAR_GUTTER,
        bottom: 0.0,
        left: 0.0,
    };
    let catalog = scrollable(container(list.width(Fill)).padding(gutter))
        .height(height)
        .width(Fill);
    let search = text_input(t!("Search plugins…").as_ref(), market.search)
        .on_input(Message::PluginSearchInput)
        .size(13)
        .padding([7, 10])
        .width(Fill);
    let catalog_pane = column![search, catalog]
        .spacing(10)
        .width(Fill)
        .height(height);
    let details = readme_panel(&market, theme, width, height);
    let body = row![
        container(catalog_pane)
            .width(Length::Fixed(410.0))
            .height(height),
        details,
    ]
    .spacing(14)
    .height(height)
    .width(width);

    container(
        column![title, subtitle, Space::new().height(12), body]
            .spacing(4)
            .padding(18)
            .width(width)
            .height(height),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        ..Default::default()
    })
    .width(width)
    .height(height)
    .into()
}

/// Browser replacement for the native Plugin Manager.
#[cfg(target_arch = "wasm32")]
pub fn view_web_notice<'a>() -> Element<'a, Message> {
    let download = button(text(t!("Download desktop app")).size(13))
        .on_press(Message::OpenUrl(DESKTOP_DOWNLOAD_URL.to_string()))
        .padding([9, 18])
        .style(|theme: &Theme, status| {
            let mut style = button::primary(theme, status);
            style.text_color = iced::Color::WHITE;
            style
        });

    let icon = container(crate::ui::icons::themed_primary(
        crate::ui::icons::GEAR,
        26.0,
    ))
    .center(Length::Fixed(44.0))
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().primary.weak.color)),
        border: Border {
            radius: 12.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    let notice = container(
        column![
            icon,
            text(t!("Plugins are available in the desktop app"))
                .size(20)
                .width(Length::Fit)
                .align_x(iced::alignment::Horizontal::Center),
            text(t!(
                "Open CAD Studio plugins are native packages and cannot run inside a browser. Download the desktop app to browse, install, and use plugins."
            ))
            .size(13)
            .width(Length::Fit)
            .align_x(iced::alignment::Horizontal::Center)
            .style(muted_style),
            Space::new().height(4),
            download,
        ]
        .spacing(10)
        .align_x(iced::alignment::Horizontal::Center)
        .width(Length::Fit),
    )
    .width(Length::Fit.max(380.0));

    container(notice)
        .center_x(Length::Fit)
        .padding([16, 20])
        .width(Length::Fit)
        .height(Length::Fit)
        .into()
}

#[cfg(test)]
mod tests {
    use super::{newest_update, registry_error_message, repository_display_name};
    use crate::plugin::external::ReleaseInfo;

    #[test]
    fn newest_update_uses_semver_not_release_order() {
        let releases = vec![
            ReleaseInfo {
                tag: "v1.4.0".to_string(),
                api_version: 4,
                acadrust_source: None,
                acadrust_declared: false,
                acadrust_compatible: true,
                rustc_version: None,
                rustc_declared: false,
                rustc_compatible: true,
            },
            ReleaseInfo {
                tag: "v2.0.0".to_string(),
                api_version: 4,
                acadrust_source: None,
                acadrust_declared: false,
                acadrust_compatible: true,
                rustc_version: None,
                rustc_declared: false,
                rustc_compatible: true,
            },
            ReleaseInfo {
                tag: "v1.9.9".to_string(),
                api_version: 4,
                acadrust_source: None,
                acadrust_declared: false,
                acadrust_compatible: true,
                rustc_version: None,
                rustc_declared: false,
                rustc_compatible: true,
            },
        ];
        assert_eq!(newest_update("1.3.0", &releases).as_deref(), Some("v2.0.0"));
        assert_eq!(newest_update("2.0.0", &releases), None);
    }

    #[test]
    fn newest_update_ignores_incompatible_releases() {
        let releases = vec![
            ReleaseInfo {
                tag: "v1.4.0".to_string(),
                api_version: 4,
                acadrust_source: Some(
                    "git+https://github.com/HakanSeven12/opencadcodec.git?rev=94df2c3#94df2c3f87fa051b16ffc3923f80e9247c85c5fd".to_string(),
                ),
                acadrust_declared: true,
                acadrust_compatible: true,
                rustc_version: None,
                rustc_declared: false,
                rustc_compatible: true,
            },
            ReleaseInfo {
                tag: "v2.0.0".to_string(),
                api_version: 4,
                acadrust_source: Some(
                    "git+https://github.com/HakanSeven12/opencadcodec.git?rev=0908da7#0908da7b6e4f702a6c78359a57f53e2b79cf39eb".to_string(),
                ),
                acadrust_declared: true,
                acadrust_compatible: false,
                rustc_version: None,
                rustc_declared: false,
                rustc_compatible: true,
            },
        ];
        assert_eq!(newest_update("1.3.0", &releases).as_deref(), Some("v1.4.0"));
    }

    #[test]
    fn repository_name_is_human_readable() {
        assert_eq!(
            repository_display_name("owner/opencad-storm_sewer-plugin"),
            "OpenCAD Storm Sewer Plugin",
        );
    }

    #[test]
    fn certificate_errors_get_user_friendly_copy() {
        // The registry message is localised; pin English so the assertion
        // does not depend on the ambient process locale.
        crate::i18n::set_language(crate::i18n::Language::EnUs)
            .expect("English locale must be available");
        let (title, message) =
            registry_error_message("io: invalid peer certificate: UnknownIssuer");

        assert_eq!(title, "Unable to verify the server certificate");
        assert!(message.contains("system certificate"));
        assert!(!message.contains("UnknownIssuer"));
    }
}
