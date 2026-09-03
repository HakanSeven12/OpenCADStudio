//! Consolidated user configuration. Native builds use one grouped JSON file
//! (`<config>/OpenCADStudio/settings.json`); web builds keep the same JSON in
//! `localStorage`. It holds every app preference except the command aliases,
//! which use native `ocad.pgp` or a separate web storage key. Serialized via
//! serde so the data is structured and grouped, replacing the former scattered
//! flat stores (`settings.txt` / `recent.txt` / `recent_limit.txt` /
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
    /// General edge-stack dock layout (which panels are docked, side, order,
    /// width and auto-collapse) for the Properties panel and block palette.
    pub dock: crate::ui::dock::DockState,
    /// Add a newly selected annotation scale to existing annotative objects.
    pub annotation_auto_scale: i8,
    /// Ribbon collapse density.
    pub ribbon: RibbonConfig,
    /// Print dialog preferences (only the persisted fields; runtime state is
    /// skipped by `PlotDialogState`'s serde attributes).
    pub plot: PlotDialogState,
    /// Complete editable keyboard shortcut table.
    pub shortcuts: ShortcutConfig,
    /// Model space background, grid, and selection appearance.
    pub model_space: ModelSpaceThemeConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            settings: UserSettings::default(),
            theme: UiThemeConfig::default(),
            recent: RecentConfig::default(),
            start: StartConfig::default(),
            statusbar: StatusBarConfig::default(),
            dock: crate::ui::dock::DockState::default(),
            annotation_auto_scale: -4,
            ribbon: RibbonConfig::default(),
            plot: PlotDialogState::default(),
            shortcuts: ShortcutConfig::default(),
            model_space: ModelSpaceThemeConfig::default(),
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutConfig {
    pub bindings: std::collections::BTreeMap<String, String>,
}

impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            bindings: super::shortcuts::default_bindings(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockSide {
    Left,
    Right,
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
            palette: UiThemePalette::from_iced(theme.seed()),
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
        Self::from_iced(iced::Theme::Oxocarbon.seed())
    }
}

impl UiThemePalette {
    pub fn from_iced(palette: iced::theme::palette::Seed) -> Self {
        Self {
            background: color_to_rgb(palette.background),
            text: color_to_rgb(palette.text),
            primary: color_to_rgb(palette.primary),
            success: color_to_rgb(palette.success),
            warning: color_to_rgb(palette.warning),
            danger: color_to_rgb(palette.danger),
        }
    }

    pub fn to_iced(self) -> iced::theme::palette::Seed {
        iced::theme::palette::Seed {
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

pub(crate) fn rgb_to_hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

pub(crate) fn parse_hex(value: &str) -> Option<[u8; 3]> {
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

/// Canvas mode for Model space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ModelSpaceMode {
    /// Canvas background dynamically follows the active UI theme.
    #[default]
    MatchTheme,
    /// Classic CAD dark charcoal canvas (#212830) regardless of UI theme.
    ClassicDark,
    /// Custom background colors specified by the user.
    Custom,
}

impl ModelSpaceMode {
    pub const ALL: [Self; 3] = [Self::MatchTheme, Self::ClassicDark, Self::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Self::MatchTheme => "Match Theme",
            Self::ClassicDark => "Classic CAD Dark",
            Self::Custom => "Custom",
        }
    }
}

/// Standard classic CAD dark charcoal background [33, 40, 48].
pub const CLASSIC_CAD_DARK_BG: [u8; 3] = [33, 40, 48];

/// Standard paper space sheet background [255, 255, 255].
pub const DEFAULT_PAPER_BG: [u8; 3] = [255, 255, 255];

/// Standard paper space desk surround [138, 138, 138].
pub const DEFAULT_DESK_BG: [u8; 3] = [138, 138, 138];

/// Persistent configuration for Model Space appearance and selection visual effects.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelSpaceThemeConfig {
    /// How the canvas background is determined.
    pub mode: ModelSpaceMode,
    /// Custom model space canvas background [R, G, B] (0–255).
    pub custom_bg: Option<[u8; 3]>,
    /// Custom paper space sheet background [R, G, B] (0–255).
    pub custom_paper_bg: Option<[u8; 3]>,
    /// Custom paper space desk/surround background [R, G, B] (0–255).
    pub custom_desk_bg: Option<[u8; 3]>,
    /// Grid opacity percentage (5–100, default 18%).
    pub grid_opacity: u8,
    /// SELECTIONAREA: whether selection marquees have a translucent shaded fill.
    pub selection_area: bool,
    /// SELECTIONAREAOPACITY: transparency percentage of selection areas (0–100, default 12%).
    pub selection_opacity: u8,
    /// WINDOWSAREACOLOR: ACI index (0 = Theme Primary, 1..=255 = ACI).
    pub selection_window_color: u8,
    /// CROSSINGAREACOLOR: ACI index (0 = Theme Success, 1..=255 = ACI).
    pub selection_crossing_color: u8,
    /// SELECTIONEFFECTCOLOR: ACI index for selected entities highlight (0 = Theme Primary, 1..=255 = ACI).
    pub selection_highlight_color: u8,
    /// SELECTIONEFFECT: whether selected objects glow with solid highlight (true) or dash (false).
    pub selection_effect: bool,
    /// SELECTIONPREVIEW: rollover/hover highlight mode (0 = off, 1 = in cmd, 2 = idle, 3 = both).
    pub selection_preview: u8,
    /// GRIPSIZE: grip marker half-size in pixels (1–25, default 5).
    pub grip_size: u8,
    /// GRIPCOLOR: unselected grip ACI color (0 = Theme Primary outline, 1..=255 = ACI).
    pub grip_color: u8,
    /// GRIPHOT: selected/hot grip ACI color (0 = Theme Danger, 1..=255 = ACI).
    pub grip_hot: u8,
    /// GRIPHOVER: hovered/warm grip ACI color (0 = Theme Primary Strong, 1..=255 = ACI).
    pub grip_hover: u8,
}

impl Default for ModelSpaceThemeConfig {
    fn default() -> Self {
        Self {
            mode: ModelSpaceMode::MatchTheme,
            custom_bg: None,
            custom_paper_bg: None,
            custom_desk_bg: None,
            grid_opacity: 18,
            selection_area: true,
            selection_opacity: 12,
            selection_window_color: 0,
            selection_crossing_color: 0,
            selection_highlight_color: 0,
            selection_effect: true,
            selection_preview: 3,
            grip_size: 5,
            grip_color: 0,
            grip_hot: 0,
            grip_hover: 0,
        }
    }
}

impl ModelSpaceThemeConfig {
    /// Resolve active model space canvas background as [f32; 4] based on current theme.
    pub fn resolve_model_bg(&self, theme: &iced::Theme) -> [f32; 4] {
        let rgb = match self.mode {
            ModelSpaceMode::MatchTheme => theme_canvas_background(theme),
            ModelSpaceMode::ClassicDark => CLASSIC_CAD_DARK_BG,
            ModelSpaceMode::Custom => self.custom_bg.unwrap_or(CLASSIC_CAD_DARK_BG),
        };
        [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            1.0,
        ]
    }

    /// Resolve active paper space sheet background as [f32; 4].
    pub fn resolve_paper_bg(&self) -> [f32; 4] {
        let rgb = self.custom_paper_bg.unwrap_or(DEFAULT_PAPER_BG);
        [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            1.0,
        ]
    }

    /// Resolve active paper space desk surround background as [f32; 4].
    pub fn resolve_desk_bg(&self) -> [f32; 4] {
        let rgb = self.custom_desk_bg.unwrap_or(DEFAULT_DESK_BG);
        [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
            1.0,
        ]
    }

    /// Resolve active selection highlight overlay color as [f32; 4].
    pub fn resolve_selection_color(&self) -> [f32; 4] {
        if self.selection_highlight_color == 0 {
            crate::scene::model::wire_model::WireModel::SELECTED
        } else if let Some((r, g, b)) = acadrust::types::aci_table::aci_to_rgb(self.selection_highlight_color) {
            [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
        } else {
            crate::scene::model::wire_model::WireModel::SELECTED
        }
    }
}

/// Default canvas background for a given theme variant.
pub fn theme_canvas_background(theme: &iced::Theme) -> [u8; 3] {
    match theme {
        iced::Theme::Light => [255, 255, 255],
        iced::Theme::SolarizedLight => [253, 246, 227],
        iced::Theme::GruvboxLight => [251, 241, 199],
        iced::Theme::TokyoNightLight => [225, 226, 231],
        iced::Theme::KanagawaLotus => [242, 236, 222],
        iced::Theme::Dark => [32, 34, 37],
        iced::Theme::Dracula => [40, 42, 54],
        iced::Theme::Nord => [46, 52, 64],
        iced::Theme::SolarizedDark => [0, 43, 54],
        iced::Theme::GruvboxDark => [40, 40, 40],
        iced::Theme::TokyoNight => [26, 27, 38],
        iced::Theme::TokyoNightStorm => [36, 40, 59],
        iced::Theme::KanagawaWave => [31, 31, 40],
        iced::Theme::KanagawaDragon => [24, 26, 28],
        iced::Theme::Moonfly => [8, 18, 24],
        iced::Theme::Nightfly => [1, 22, 39],
        iced::Theme::Oxocarbon => [22, 22, 22],
        iced::Theme::Ferra => [43, 41, 46],
        _ => color_to_rgb(theme.palette().background.base.color),
    }
}

/// Parse a flexible theme name string into an `iced::Theme` variant.
/// Normalizes by stripping spaces, underscores, and dashes, case-insensitive.
pub fn parse_theme_name(s: &str) -> Option<iced::Theme> {
    let clean: String = s
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
        .flat_map(|c| c.to_uppercase())
        .collect();
    match clean.as_str() {
        "DARK" => Some(iced::Theme::Dark),
        "LIGHT" => Some(iced::Theme::Light),
        "DRACULA" => Some(iced::Theme::Dracula),
        "NORD" => Some(iced::Theme::Nord),
        "SOLARIZEDLIGHT" => Some(iced::Theme::SolarizedLight),
        "SOLARIZEDDARK" => Some(iced::Theme::SolarizedDark),
        "GRUVBOXLIGHT" => Some(iced::Theme::GruvboxLight),
        "GRUVBOXDARK" => Some(iced::Theme::GruvboxDark),
        "TOKYONIGHT" => Some(iced::Theme::TokyoNight),
        "TOKYONIGHTSTORM" => Some(iced::Theme::TokyoNightStorm),
        "TOKYONIGHTLIGHT" => Some(iced::Theme::TokyoNightLight),
        "KANAGAWAWAVE" => Some(iced::Theme::KanagawaWave),
        "KANAGAWADRAGON" => Some(iced::Theme::KanagawaDragon),
        "KANAGAWALOTUS" => Some(iced::Theme::KanagawaLotus),
        "MOONFLY" => Some(iced::Theme::Moonfly),
        "NIGHTFLY" => Some(iced::Theme::Nightfly),
        "OXOCARBON" => Some(iced::Theme::Oxocarbon),
        "FERRA" => Some(iced::Theme::Ferra),
        _ => None,
    }
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
    /// unreadable. Unknown or missing fields fall back to their section defaults
    /// via `#[serde(default)]`.
    pub fn load() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let body = config_path().and_then(|p| std::fs::read_to_string(p).ok());

        #[cfg(target_arch = "wasm32")]
        let body = web_sys::window()
            .and_then(|window| window.local_storage().ok().flatten())
            .and_then(|storage| storage.get_item(WEB_CONFIG_KEY).ok().flatten());

        body.and_then(|body| serde_json::from_str(&body).ok())
            .unwrap_or_default()
    }

    /// Persist the config as JSON. Best-effort; silent on unavailable or
    /// read-only storage.
    pub fn save(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(path) = config_path() else { return };
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, json);
            }
        }

        #[cfg(target_arch = "wasm32")]
        if let (Some(storage), Ok(json)) = (
            web_sys::window().and_then(|window| window.local_storage().ok().flatten()),
            serde_json::to_string(self),
        ) {
            let _ = storage.set_item(WEB_CONFIG_KEY, &json);
        }
    }
}

#[cfg(target_arch = "wasm32")]
const WEB_CONFIG_KEY: &str = "opencadstudio.settings";

#[cfg(not(target_arch = "wasm32"))]
fn config_path() -> Option<std::path::PathBuf> {
    Some(crate::config::config_dir()?.join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_space_defaults() {
        let cfg = ModelSpaceThemeConfig::default();
        assert_eq!(cfg.mode, ModelSpaceMode::MatchTheme);
        assert_eq!(cfg.custom_bg, None);
        assert_eq!(cfg.custom_paper_bg, None);
        assert_eq!(cfg.grid_opacity, 18);
        assert!(cfg.selection_area);
        assert_eq!(cfg.selection_opacity, 12);
        assert_eq!(cfg.selection_window_color, 0);
        assert_eq!(cfg.selection_crossing_color, 0);
        assert_eq!(cfg.selection_highlight_color, 0);
        assert!(cfg.selection_effect);
        assert_eq!(cfg.selection_preview, 3);
    }

    #[test]
    fn test_resolve_model_bg_match_theme() {
        let cfg = ModelSpaceThemeConfig {
            mode: ModelSpaceMode::MatchTheme,
            ..Default::default()
        };
        // Light theme should produce pure white background [1.0, 1.0, 1.0, 1.0]
        let light_bg = cfg.resolve_model_bg(&iced::Theme::Light);
        assert_eq!(light_bg, [1.0, 1.0, 1.0, 1.0]);

        // Oxocarbon theme should produce dark background
        let oxo_bg = cfg.resolve_model_bg(&iced::Theme::Oxocarbon);
        assert!((oxo_bg[0] - 22.0 / 255.0).abs() < 1e-4);
        assert!((oxo_bg[1] - 22.0 / 255.0).abs() < 1e-4);
        assert!((oxo_bg[2] - 22.0 / 255.0).abs() < 1e-4);
        assert_eq!(oxo_bg[3], 1.0);
    }

    #[test]
    fn test_resolve_model_bg_classic_dark() {
        let cfg = ModelSpaceThemeConfig {
            mode: ModelSpaceMode::ClassicDark,
            ..Default::default()
        };
        // Even with Light theme active, ClassicDark must stay locked to [33, 40, 48]
        let bg = cfg.resolve_model_bg(&iced::Theme::Light);
        assert!((bg[0] - 33.0 / 255.0).abs() < 1e-4);
        assert!((bg[1] - 40.0 / 255.0).abs() < 1e-4);
        assert!((bg[2] - 48.0 / 255.0).abs() < 1e-4);
        assert_eq!(bg[3], 1.0);
    }

    #[test]
    fn test_resolve_model_bg_custom() {
        let cfg = ModelSpaceThemeConfig {
            mode: ModelSpaceMode::Custom,
            custom_bg: Some([10, 20, 30]),
            ..Default::default()
        };
        let bg = cfg.resolve_model_bg(&iced::Theme::Light);
        assert!((bg[0] - 10.0 / 255.0).abs() < 1e-4);
        assert!((bg[1] - 20.0 / 255.0).abs() < 1e-4);
        assert!((bg[2] - 30.0 / 255.0).abs() < 1e-4);
    }

    #[test]
    fn test_model_space_config_serde_roundtrip() {
        let mut original = AppConfig::default();
        original.model_space.mode = ModelSpaceMode::Custom;
        original.model_space.custom_bg = Some([12, 34, 56]);
        original.model_space.grid_opacity = 45;
        original.model_space.selection_opacity = 35;
        original.model_space.selection_window_color = 5;

        let serialized = serde_json::to_string(&original).expect("serialize config");
        let deserialized: AppConfig = serde_json::from_str(&serialized).expect("deserialize config");

        assert_eq!(deserialized.model_space.mode, ModelSpaceMode::Custom);
        assert_eq!(deserialized.model_space.custom_bg, Some([12, 34, 56]));
        assert_eq!(deserialized.model_space.grid_opacity, 45);
        assert_eq!(deserialized.model_space.selection_opacity, 35);
        assert_eq!(deserialized.model_space.selection_window_color, 5);
    }

    #[test]
    fn test_parse_theme_name() {
        assert_eq!(parse_theme_name("light"), Some(iced::Theme::Light));
        assert_eq!(parse_theme_name("DARK"), Some(iced::Theme::Dark));
        assert_eq!(parse_theme_name("solarized light"), Some(iced::Theme::SolarizedLight));
        assert_eq!(parse_theme_name("tokyo-night-storm"), Some(iced::Theme::TokyoNightStorm));
        assert_eq!(parse_theme_name("Kanagawa_Lotus"), Some(iced::Theme::KanagawaLotus));
        assert_eq!(parse_theme_name("1"), None);
        assert_eq!(parse_theme_name("0"), None);
        assert_eq!(parse_theme_name("nonexistent_theme"), None);
    }
}
