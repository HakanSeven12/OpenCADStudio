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
        let kwinrc = config_home()
            .map(|dir| dir.join("kwinrc"))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default();
        let (left, right) = parse_kwinrc(&kwinrc);
        return ButtonLayout {
            style: ButtonStyle::Breeze,
            left,
            right,
        };
    }
    // Other desktops (Xfce, Cinnamon, MATE, ...) keep their buttons in
    // settings of their own; GNOME's key would mostly hold its default,
    // close only. They get all three buttons on the right.
    if !has_desktop(&names, &["gnome", "budgie", "pantheon", "unity", "ubuntu"]) {
        return ButtonLayout {
            style: ButtonStyle::Adwaita,
            left: Vec::new(),
            right: ALL.to_vec(),
        };
    }
    // The GNOME family keeps its order in this key; without it (no
    // gsettings), GNOME's own default.
    let setting = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.wm.preferences", "button-layout"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_else(|| "appmenu:close".to_owned());
    let (left, right) = parse_gnome_layout(&setting);
    ButtonLayout {
        style: ButtonStyle::Adwaita,
        left,
        right,
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

/// What a double click on the title bar does here: on macOS as set in
/// System Settings (Desktop & Dock), elsewhere it maximizes.
pub fn double_click() -> DoubleClick {
    static ACTION: OnceLock<DoubleClick> = OnceLock::new();
    *ACTION.get_or_init(|| {
        if !cfg!(target_os = "macos") {
            return DoubleClick::Maximize;
        }
        let read = |key: &str| {
            std::process::Command::new("defaults")
                .args(["read", "-g", key])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        };
        parse_double_click(
            read("AppleActionOnDoubleClick").as_deref(),
            read("AppleMiniaturizeOnDoubleClick").as_deref(),
        )
    })
}

/// macOS's `AppleActionOnDoubleClick` (`Maximize`, `Minimize`, `None`),
/// or before it the older `AppleMiniaturizeOnDoubleClick` (`1`); unset, the
/// window zooms.
pub(super) fn parse_double_click(action: Option<&str>, miniaturize: Option<&str>) -> DoubleClick {
    match action.map(str::trim) {
        Some("Minimize") => DoubleClick::Minimize,
        Some("None") => DoubleClick::Nothing,
        Some(_) => DoubleClick::Maximize,
        None if miniaturize.map(str::trim) == Some("1") => DoubleClick::Minimize,
        None => DoubleClick::Maximize,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_tiling, parse_double_click, parse_gnome_layout, parse_kwinrc, Button::*, DoubleClick,
    };

    #[test]
    fn the_mac_double_click_setting_is_read() {
        assert_eq!(parse_double_click(None, None), DoubleClick::Maximize);
        assert_eq!(
            parse_double_click(Some("Maximize\n"), None),
            DoubleClick::Maximize
        );
        assert_eq!(
            parse_double_click(Some("Minimize\n"), None),
            DoubleClick::Minimize
        );
        assert_eq!(parse_double_click(Some("None"), None), DoubleClick::Nothing);
        assert_eq!(parse_double_click(None, Some("1")), DoubleClick::Minimize);
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
