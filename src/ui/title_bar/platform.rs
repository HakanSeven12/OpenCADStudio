//! What the title bar looks like on each desktop: whether the app draws it
//! at all, and which window buttons it shows, in which style and on which
//! side. Read once from the environment and the desktop's own settings.

use std::sync::OnceLock;

/// The look of the window buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Windows 11: wide square buttons the height of the bar.
    Windows,
    /// GNOME and other GTK desktops (Adwaita): round buttons on a tint.
    Adwaita,
    /// KDE Plasma (Breeze): round, untinted until hovered.
    Breeze,
    /// macOS: the system's own traffic lights; the app draws none.
    System,
}

/// One window button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Minimize,
    Maximize,
    Close,
}

/// The window buttons: their style and the ones on each side of the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonLayout {
    pub style: ButtonStyle,
    pub left: Vec<Button>,
    pub right: Vec<Button>,
}

const ALL: [Button; 3] = [Button::Minimize, Button::Maximize, Button::Close];

/// The session's desktop names (`XDG_CURRENT_DESKTOP` and friends), joined
/// by `:` and in lower case.
fn desktop_names() -> String {
    [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]
    .iter()
    .filter_map(|v| std::env::var(v).ok())
    .collect::<Vec<_>>()
    .join(":")
    .to_ascii_lowercase()
}

fn has_desktop(names: &str, wanted: &[&str]) -> bool {
    names
        .split([':', ';'])
        .any(|name| wanted.contains(&name.trim()))
}

/// Whether the session runs a tiling window manager, which draws no title
/// bars and moves windows itself: the app then draws none either.
pub fn tiling_desktop() -> bool {
    static TILING: OnceLock<bool> = OnceLock::new();
    *TILING.get_or_init(|| cfg!(target_os = "linux") && is_tiling(&desktop_names()))
}

/// Whether desktop `names` (`:`/`;` separated) name a tiling window manager.
pub(super) fn is_tiling(names: &str) -> bool {
    has_desktop(
        &names.to_ascii_lowercase(),
        &[
            "i3",
            "sway",
            "hyprland",
            "bspwm",
            "awesome",
            "dwm",
            "xmonad",
            "qtile",
            "river",
            "niri",
            "herbstluftwm",
            "spectrwm",
            "leftwm",
        ],
    )
}

/// The window buttons for this session.
pub fn layout() -> &'static ButtonLayout {
    static LAYOUT: OnceLock<ButtonLayout> = OnceLock::new();
    LAYOUT.get_or_init(detect)
}

fn detect() -> ButtonLayout {
    if cfg!(target_os = "macos") {
        return ButtonLayout {
            style: ButtonStyle::System,
            left: Vec::new(),
            right: Vec::new(),
        };
    }
    if cfg!(target_os = "windows") {
        return windows_layout();
    }
    let names = desktop_names();
    if has_desktop(&names, &["kde", "plasma"]) {
        let (left, right) = parse_kwinrc(&kwinrc());
        return ButtonLayout {
            style: ButtonStyle::Breeze,
            left,
            right,
        };
    }
    // Other desktops (Xfce, Cinnamon, MATE, ...) keep their buttons in
    // settings of their own; GNOME's key would mostly hold its default,
    // close only. They get all three buttons on the right.
    if !has_desktop(&names, &GNOME_FAMILY) {
        return ButtonLayout {
            style: ButtonStyle::Adwaita,
            left: Vec::new(),
            right: ALL.to_vec(),
        };
    }
    // The GNOME family keeps its order in this key; without it (no
    // gsettings), GNOME's own default.
    let setting = gsettings("button-layout").unwrap_or_else(|| "appmenu:close".to_owned());
    let (left, right) = parse_gnome_layout(&setting);
    ButtonLayout {
        style: ButtonStyle::Adwaita,
        left,
        right,
    }
}

/// The width the window buttons and the inset before the logo take in
/// `layout`: a button's width, the gap between buttons of a group and a
/// margin at the bar's end for each group; macOS's traffic lights.
pub fn buttons_width_of(layout: &ButtonLayout) -> f32 {
    use crate::ui::style::ribbon_metrics as m;
    if layout.style == ButtonStyle::System {
        return m::TRAFFIC_LIGHTS_W;
    }
    m::TITLE_BAR_PAD
        + group_width(layout.style, layout.left.len())
        + group_width(layout.style, layout.right.len())
}

/// The width a group of `count` window buttons takes in `style`, with the
/// margin at its end of the bar; nothing without buttons.
pub fn group_width(style: ButtonStyle, count: usize) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let (w, gap, margin) = button_metrics(style);
    count as f32 * w + (count - 1) as f32 * gap + margin
}

/// A button's width, the gap between buttons of a group and the margin at
/// the bar's end, in `style`.
pub fn button_metrics(style: ButtonStyle) -> (f32, f32, f32) {
    use crate::ui::style::ribbon_metrics as m;
    match style {
        ButtonStyle::Windows => (m::CAPTION_W, 0.0, 0.0),
        ButtonStyle::Adwaita => (m::ADWAITA_BUTTON, m::CAPTION_GAP, m::CAPTION_GAP),
        ButtonStyle::Breeze => (m::BREEZE_BUTTON, m::CAPTION_GAP, m::CAPTION_GAP),
        ButtonStyle::System => (0.0, 0.0, 0.0),
    }
}

/// Windows 11's buttons: minimize, maximize and close on the right.
pub fn windows_layout() -> ButtonLayout {
    ButtonLayout {
        style: ButtonStyle::Windows,
        left: Vec::new(),
        right: ALL.to_vec(),
    }
}

/// Desktops that keep GNOME's window-manager settings.
const GNOME_FAMILY: [&str; 5] = ["gnome", "budgie", "pantheon", "unity", "ubuntu"];

/// A key of `org.gnome.desktop.wm.preferences`, as `gsettings` prints it.
fn gsettings(key: &str) -> Option<String> {
    std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.wm.preferences", key])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// KDE's `kwinrc`: the user's, else the system-wide default a
/// distribution ships.
fn kwinrc() -> String {
    config_home()
        .map(|dir| dir.join("kwinrc"))
        .into_iter()
        .chain(std::iter::once(std::path::PathBuf::from("/etc/xdg/kwinrc")))
        .find_map(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default()
}

fn config_home() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config"))
        })
}

/// GNOME's `button-layout` (`'appmenu:minimize,maximize,close'`): the
/// buttons before the colon go left, the rest right. Unknown names (the
/// application menu, spacers) are skipped.
pub(super) fn parse_gnome_layout(setting: &str) -> (Vec<Button>, Vec<Button>) {
    let setting = setting.trim().trim_matches(|c| c == '\'' || c == '"');
    let side = |part: &str| {
        part.split(',')
            .filter_map(|name| match name.trim() {
                "minimize" => Some(Button::Minimize),
                "maximize" => Some(Button::Maximize),
                "close" => Some(Button::Close),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    match setting.split_once(':') {
        Some((left, right)) => (side(left), side(right)),
        None => (Vec::new(), side(setting)),
    }
}

/// KDE's `kwinrc`: `ButtonsOnLeft` and `ButtonsOnRight` of
/// `[org.kde.kdecoration2]`, one letter a button (I minimize, A maximize,
/// X close; others are skipped). Without them, Plasma's defaults.
pub(super) fn parse_kwinrc(kwinrc: &str) -> (Vec<Button>, Vec<Button>) {
    let mut section = "";
    let mut left = None;
    let mut right = None;
    for line in kwinrc.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name;
            continue;
        }
        if section != "org.kde.kdecoration2" {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "ButtonsOnLeft" => left = Some(value.trim().to_owned()),
                "ButtonsOnRight" => right = Some(value.trim().to_owned()),
                _ => {}
            }
        }
    }
    let side = |letters: &str| {
        letters
            .chars()
            .filter_map(|c| match c {
                'I' => Some(Button::Minimize),
                'A' => Some(Button::Maximize),
                'X' => Some(Button::Close),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    (
        side(left.as_deref().unwrap_or("MS")),
        side(right.as_deref().unwrap_or("HIAX")),
    )
}

/// What a double click on the title bar does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleClick {
    Maximize,
    Minimize,
    Nothing,
}

/// What a double click on the title bar does here, as the desktop is set:
/// GNOME's `action-double-click-titlebar`, KDE's `TitlebarDoubleClickCommand`,
/// elsewhere it maximizes. On macOS the system's own title area handles the
/// double click (System Settings › Desktop & Dock), so the app does nothing.
pub fn double_click() -> DoubleClick {
    static ACTION: OnceLock<DoubleClick> = OnceLock::new();
    *ACTION.get_or_init(|| {
        if cfg!(target_os = "macos") {
            return DoubleClick::Nothing;
        }
        if !cfg!(target_os = "linux") {
            return DoubleClick::Maximize;
        }
        let names = desktop_names();
        if has_desktop(&names, &["kde", "plasma"]) {
            return parse_kde_double_click(&kwinrc());
        }
        if has_desktop(&names, &GNOME_FAMILY) {
            let setting = gsettings("action-double-click-titlebar");
            return parse_gnome_double_click(setting.as_deref().unwrap_or("toggle-maximize"));
        }
        DoubleClick::Maximize
    })
}

/// GNOME's `action-double-click-titlebar`; the vertical-only and menu
/// actions, which the app has no counterpart for, maximize.
pub(super) fn parse_gnome_double_click(setting: &str) -> DoubleClick {
    match setting.trim().trim_matches(|c| c == '\'' || c == '"') {
        "minimize" => DoubleClick::Minimize,
        "none" | "lower" => DoubleClick::Nothing,
        _ => DoubleClick::Maximize,
    }
}

/// KDE's `TitlebarDoubleClickCommand` in `[Windows]` of `kwinrc`; unset,
/// Plasma's default (maximize). Actions without a counterpart do nothing.
pub(super) fn parse_kde_double_click(kwinrc: &str) -> DoubleClick {
    match ini_value(kwinrc, "Windows", "TitlebarDoubleClickCommand").as_deref() {
        None | Some("Maximize") => DoubleClick::Maximize,
        Some("Minimize") => DoubleClick::Minimize,
        Some(_) => DoubleClick::Nothing,
    }
}

/// The value of `key` in `[section]` of an ini-style file.
fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = "";
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name;
        } else if current == section {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.trim().to_owned());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        buttons_width_of, is_tiling, parse_gnome_double_click, parse_gnome_layout,
        parse_kde_double_click, parse_kwinrc, Button::*, ButtonLayout, ButtonStyle, DoubleClick,
    };

    #[test]
    fn the_desktops_double_click_setting_is_read() {
        assert_eq!(
            parse_gnome_double_click("'toggle-maximize'"),
            DoubleClick::Maximize
        );
        assert_eq!(
            parse_gnome_double_click("'minimize'\n"),
            DoubleClick::Minimize
        );
        assert_eq!(parse_gnome_double_click("'none'"), DoubleClick::Nothing);
        assert_eq!(parse_kde_double_click(""), DoubleClick::Maximize);
        let kwinrc = "[Windows]\nTitlebarDoubleClickCommand=Minimize\n";
        assert_eq!(parse_kde_double_click(kwinrc), DoubleClick::Minimize);
        let kwinrc = "[Windows]\nTitlebarDoubleClickCommand=Shade\n";
        assert_eq!(parse_kde_double_click(kwinrc), DoubleClick::Nothing);
    }

    #[test]
    fn the_buttons_width_follows_the_style() {
        let layout = |style, left: Vec<_>, right: Vec<_>| ButtonLayout { style, left, right };
        let windows = layout(
            ButtonStyle::Windows,
            vec![],
            vec![Minimize, Maximize, Close],
        );
        assert_eq!(buttons_width_of(&windows), 6.0 + 3.0 * 46.0);
        let gnome = layout(ButtonStyle::Adwaita, vec![], vec![Close]);
        assert_eq!(buttons_width_of(&gnome), 6.0 + 24.0 + 6.0);
        let kde = layout(ButtonStyle::Breeze, vec![Close], vec![Minimize, Maximize]);
        assert_eq!(buttons_width_of(&kde), 6.0 + 3.0 * 20.0 + 6.0 + 2.0 * 6.0);
        let mac = layout(ButtonStyle::System, vec![], vec![]);
        assert_eq!(buttons_width_of(&mac), 78.0);
    }

    #[test]
    fn tiling_window_managers_are_recognised() {
        assert!(is_tiling("sway"));
        assert!(is_tiling("Hyprland"));
        assert!(is_tiling("i3:ubuntu"));
        assert!(!is_tiling("GNOME"));
        assert!(!is_tiling("KDE"));
        assert!(!is_tiling("ubuntu:GNOME"));
        assert!(!is_tiling(""));
    }

    #[test]
    fn gnome_button_layouts_are_read() {
        assert_eq!(
            parse_gnome_layout("'appmenu:close'\n"),
            (vec![], vec![Close])
        );
        assert_eq!(
            parse_gnome_layout("'appmenu:minimize,maximize,close'"),
            (vec![], vec![Minimize, Maximize, Close])
        );
        assert_eq!(
            parse_gnome_layout("'close,minimize,maximize:'"),
            (vec![Close, Minimize, Maximize], vec![])
        );
        assert_eq!(parse_gnome_layout("close"), (vec![], vec![Close]));
    }

    #[test]
    fn kde_button_layouts_are_read() {
        assert_eq!(parse_kwinrc(""), (vec![], vec![Minimize, Maximize, Close]));
        let custom =
            "[Desktops]\nNumber=2\n[org.kde.kdecoration2]\nButtonsOnLeft=XIA\nButtonsOnRight=\n";
        assert_eq!(
            parse_kwinrc(custom),
            (vec![Close, Minimize, Maximize], vec![])
        );
    }
}
