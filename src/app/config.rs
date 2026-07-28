//! Consolidated user configuration — one grouped JSON file
//! (`<config>/OpenCADStudio/settings.json`) holding every app preference except
//! the command aliases (which stay in the hand-editable `ocad.pgp`). Serialized
//! via serde so the file is structured and grouped, replacing the former
//! scattered flat stores (`settings.txt` / `recent.txt` / `recent_limit.txt` /
//! `statusbar.txt` / `ribbon.txt` / `plot.txt`).

use serde::{Deserialize, Serialize};

use super::settings::UserSettings;
use crate::ui::ribbon::CollapseMode;
use crate::ui::statusbar::statusbar_config::StatusBarConfig;
use crate::ui::window::plot::PlotDialogState;

/// The whole persisted config, grouped into top-level sections.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Input modes, backup, plugin lists, viewport background colours, …
    pub settings: UserSettings,
    /// Iced theme selection and the six base colours used by a custom theme.
    pub theme: UiThemeConfig,
    /// Recent-files list + retained count.
    pub recent: RecentConfig,
    /// Last selected section on the tabbed Start page.
    pub start: StartConfig,
    /// Which status-bar pills the user has hidden.
    pub statusbar: StatusBarConfig,
    /// Ribbon collapse density.
    pub ribbon: RibbonConfig,
    /// Print dialog preferences (only the persisted fields; runtime state is
    /// skipped by `PlotDialogState`'s serde attributes).
    pub plot: PlotDialogState,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            settings: UserSettings::default(),
            theme: UiThemeConfig::default(),
            recent: RecentConfig::default(),
            start: StartConfig::default(),
            statusbar: StatusBarConfig::default(),
            ribbon: RibbonConfig::default(),
            plot: PlotDialogState::default(),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiThemeConfig {
    pub name: String,
    pub palette: UiThemePalette,
}

impl Default for UiThemeConfig {
    fn default() -> Self {
        let theme = iced::Theme::Oxocarbon;
        Self {
            name: theme.to_string(),
            palette: UiThemePalette::from_iced(theme.palette()),
        }
    }
}

impl UiThemeConfig {
    pub fn to_iced(&self) -> iced::Theme {
        if self.name == "Custom" {
            iced::Theme::custom("Custom", self.palette.to_iced())
        } else {
            builtin_theme(&self.name).unwrap_or(iced::Theme::Oxocarbon)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiThemePalette {
    pub background: [u8; 3],
    pub text: [u8; 3],
    pub primary: [u8; 3],
    pub success: [u8; 3],
    pub warning: [u8; 3],
    pub danger: [u8; 3],
}

impl Default for UiThemePalette {
    fn default() -> Self {
        Self::from_iced(iced::Theme::Oxocarbon.palette())
    }
}

impl UiThemePalette {
    pub fn from_iced(palette: iced::theme::Palette) -> Self {
        Self {
            background: color_to_rgb(palette.background),
            text: color_to_rgb(palette.text),
            primary: color_to_rgb(palette.primary),
            success: color_to_rgb(palette.success),
            warning: color_to_rgb(palette.warning),
            danger: color_to_rgb(palette.danger),
        }
    }

    pub fn to_iced(self) -> iced::theme::Palette {
        iced::theme::Palette {
            background: rgb_to_color(self.background),
            text: rgb_to_color(self.text),
            primary: rgb_to_color(self.primary),
            success: rgb_to_color(self.success),
            warning: rgb_to_color(self.warning),
            danger: rgb_to_color(self.danger),
        }
    }

    pub fn hex_values(self) -> [String; 6] {
        [
            rgb_to_hex(self.background),
            rgb_to_hex(self.text),
            rgb_to_hex(self.primary),
            rgb_to_hex(self.success),
            rgb_to_hex(self.warning),
            rgb_to_hex(self.danger),
        ]
    }

    pub fn set_hex(&mut self, index: usize, value: &str) -> bool {
        let Some(rgb) = parse_hex(value) else {
            return false;
        };
        match index {
            0 => self.background = rgb,
            1 => self.text = rgb,
            2 => self.primary = rgb,
            3 => self.success = rgb,
            4 => self.warning = rgb,
            5 => self.danger = rgb,
            _ => return false,
        }
        true
    }
}

pub fn builtin_theme(name: &str) -> Option<iced::Theme> {
    iced::Theme::ALL
        .iter()
        .find(|theme| theme.to_string() == name)
        .cloned()
}

fn color_to_rgb(color: iced::Color) -> [u8; 3] {
    [
        (color.r * 255.0).round() as u8,
        (color.g * 255.0).round() as u8,
        (color.b * 255.0).round() as u8,
    ]
}

fn rgb_to_color(rgb: [u8; 3]) -> iced::Color {
    iced::Color::from_rgb8(rgb[0], rgb[1], rgb[2])
}

fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

fn parse_hex(value: &str) -> Option<[u8; 3]> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if value.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&value[0..2], 16).ok()?,
        u8::from_str_radix(&value[2..4], 16).ok()?,
        u8::from_str_radix(&value[4..6], 16).ok()?,
    ])
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentConfig {
    /// Recently opened file paths, newest first.
    pub files: Vec<String>,
    /// How many recent files to keep.
    pub limit: usize,
}

impl Default for RecentConfig {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            limit: super::recent::RECENT_DEFAULT,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StartConfig {
    pub section: super::StartSection,
}

#[derive(Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct RibbonConfig {
    pub collapse: CollapseMode,
}

impl AppConfig {
    /// Read the saved config, or all-defaults when the file is missing or
    /// unreadable (fresh install / wasm). Unknown or missing fields fall back to
    /// their section defaults via `#[serde(default)]`.
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_default()
    }

    /// Persist the config as pretty JSON. Best-effort; silent on failure
    /// (read-only home, full disk, wasm — where `config_dir` is `None`).
    pub fn save(&self) {
        let Some(path) = config_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }
}

fn config_path() -> Option<std::path::PathBuf> {
    Some(crate::config::config_dir()?.join("settings.json"))
}
