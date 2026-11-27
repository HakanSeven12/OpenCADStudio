//! Linux UI-font discovery.
//!
//! The desktop's own configuration store is read first because it is the
//! source of truth outside a sandbox: gsettings for GNOME and its relatives,
//! `kdeglobals` for KDE Plasma, xfconf for Xfce. The XDG Settings portal
//! comes next — it serves the same values to Flatpak/Snap sandboxes that
//! cannot reach dconf or `~/.config` — then GTK's `settings.ini`. Only when
//! none of those name a font is Fontconfig's `sans-serif` match used, and it
//! is reported as [`Source::Fontconfig`] because it says nothing about the
//! desktop's choice.
//!
//! These sources are the same on Wayland and X11. What differs is scaling:
//! GNOME's `text-scaling-factor` reaches X11 clients through `Xft.dpi`, which
//! winit already turns into the window scale factor, so it is only applied
//! here for Wayland windows.

use super::{
    ini_value, is_generic_alias, parse_gvariant_string, parse_pango, parse_qfont, Source,
    SystemUiFont,
};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Upper bound for any single external read (subprocess or D-Bus call). A
/// hung settings daemon must not hold up startup.
const READ_TIMEOUT: Duration = Duration::from_millis(1500);

const GNOME_INTERFACE: &str = "org.gnome.desktop.interface";
const CINNAMON_INTERFACE: &str = "org.cinnamon.desktop.interface";
const MATE_INTERFACE: &str = "org.mate.interface";
const KDE_GENERAL: &str = "org.kde.kdeglobals.General";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Desktop {
    Gnome,
    Cinnamon,
    Mate,
    Kde,
    Xfce,
    Other,
}

impl Desktop {
    fn current() -> Self {
        let names = [
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
        ]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .collect::<Vec<_>>()
        .join(":");
        Self::from_names(&names)
    }

    fn from_names(names: &str) -> Self {
        let names = names.to_ascii_lowercase();
        let has = |n: &str| names.split([':', ';']).any(|d| d.trim() == n);
        if has("kde") || has("plasma") || has("plasmawayland") {
            Desktop::Kde
        } else if has("x-cinnamon") || has("cinnamon") {
            Desktop::Cinnamon
        } else if has("mate") {
            Desktop::Mate
        } else if has("xfce") {
            Desktop::Xfce
        } else if [
            "gnome",
            "gnome-classic",
            "unity",
            "budgie",
            "pantheon",
            "ubuntu",
            "pop",
        ]
        .iter()
        .any(|n| has(n))
        {
            Desktop::Gnome
        } else {
            Desktop::Other
        }
    }
}

pub(super) fn detect() -> Option<SystemUiFont> {
    let desktop = Desktop::current();
    let mut found = from_desktop_store(desktop)
        .or_else(|| from_portal(desktop))
        .or_else(from_gtk_settings_ini);
    if found.is_none() && desktop == Desktop::Kde {
        // Plasma writes no `font=` until the user changes it; its built-in
        // default is Noto Sans 10 pt.
        found = Some(SystemUiFont::from_parsed(
            parse_qfont("Noto Sans,10,-1,5,50,0,0,0,0,0")?,
            Source::KdeDefault,
        ));
    }
    let Some(mut font) = found else {
        return fontconfig_sans();
    };
    // "Sans 10" and friends name Fontconfig's default face; resolve it so
    // iced asks for the same family the desktop renders.
    if font.family.as_deref().is_none_or(is_generic_alias) {
        font.family = fc_match("sans-serif");
    }
    Some(font)
}

fn from_desktop_store(desktop: Desktop) -> Option<SystemUiFont> {
    match desktop {
        Desktop::Gnome => gsettings_font(GNOME_INTERFACE, Source::GnomeSettings),
        Desktop::Cinnamon => gsettings_font(CINNAMON_INTERFACE, Source::CinnamonSettings),
        Desktop::Mate => gsettings_font(MATE_INTERFACE, Source::MateSettings),
        Desktop::Kde => kdeglobals_font(),
        Desktop::Xfce => run("xfconf-query", &["-c", "xsettings", "-p", "/Gtk/FontName"])
            .and_then(|out| parse_pango(out.trim()))
            .map(|p| SystemUiFont::from_parsed(p, Source::XfceSettings)),
        Desktop::Other => None,
    }
}

fn gsettings_font(schema: &str, source: Source) -> Option<SystemUiFont> {
    let desc = parse_gvariant_string(&run("gsettings", &["get", schema, "font-name"])?)?;
    let mut font = SystemUiFont::from_parsed(parse_pango(&desc)?, source);
    if schema == GNOME_INTERFACE {
        let factor = run("gsettings", &["get", schema, "text-scaling-factor"])
            .and_then(|out| out.trim().parse::<f64>().ok());
        apply_text_scaling(&mut font, factor);
    }
    Some(font)
}

/// GNOME's accessibility "Large Text" / text-scaling-factor multiplies every
/// font size. X11 clients already receive it through `Xft.dpi` (winit's
/// scale factor), so only Wayland windows need it here.
fn apply_text_scaling(font: &mut SystemUiFont, factor: Option<f64>) {
    let Some(factor) = factor.filter(|f| f.is_finite() && *f > 0.0) else {
        return;
    };
    if winit_uses_wayland() {
        font.size_px = font.size_px.map(|px| px * factor as f32);
    }
}

/// winit prefers Wayland whenever a compositor socket is advertised.
fn winit_uses_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty())
        || std::env::var_os("WAYLAND_SOCKET").is_some_and(|v| !v.is_empty())
}

fn config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

fn config_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = config_home().into_iter().collect();
    let system = std::env::var("XDG_CONFIG_DIRS").unwrap_or_else(|_| "/etc/xdg".into());
    dirs.extend(
        system
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    dirs
}

/// KConfig cascades from the user's file to the system ones; the first file
/// that sets the key wins.
fn kdeglobals_font() -> Option<SystemUiFont> {
    config_dirs().into_iter().find_map(|dir| {
        let text = std::fs::read_to_string(dir.join("kdeglobals")).ok()?;
        let value = ini_value(&text, "General", "font")?;
        Some(SystemUiFont::from_parsed(
            parse_qfont(&value)?,
            Source::KdeGlobals,
        ))
    })
}

fn from_gtk_settings_ini() -> Option<SystemUiFont> {
    let home = config_home()?;
    ["gtk-4.0", "gtk-3.0"].iter().find_map(|dir| {
        let text = std::fs::read_to_string(home.join(dir).join("settings.ini")).ok()?;
        let desc = ini_value(&text, "Settings", "gtk-font-name")?;
        Some(SystemUiFont::from_parsed(
            parse_pango(&desc)?,
            Source::GtkSettingsIni,
        ))
    })
}

/// Read the UI font through `org.freedesktop.portal.Settings`. The GNOME and
/// GTK portal backends expose `org.gnome.desktop.interface`; the KDE backend
/// exposes `kdeglobals` groups as `org.kde.kdeglobals.<Group>`.
fn from_portal(desktop: Desktop) -> Option<SystemUiFont> {
    let (tx, rx) = std::sync::mpsc::channel();
    // A detached thread: if the bus never answers, startup proceeds after the
    // timeout and the thread exits whenever the call finally fails.
    std::thread::Builder::new()
        .name("ocs-ui-font-portal".into())
        .spawn(move || {
            let result = iced::futures::executor::block_on(read_portal(desktop));
            let _ = tx.send(result);
        })
        .ok()?;
    rx.recv_timeout(READ_TIMEOUT).ok().flatten()
}

async fn read_portal(desktop: Desktop) -> Option<SystemUiFont> {
    use ashpd::desktop::settings::Settings;
    let settings = Settings::new().await.ok()?;
    let kde = async {
        let value = settings.read::<String>(KDE_GENERAL, "font").await.ok()?;
        Some(SystemUiFont::from_parsed(
            parse_qfont(&value)?,
            Source::Portal,
        ))
    };
    let gnome = async {
        let desc = settings
            .read::<String>(GNOME_INTERFACE, "font-name")
            .await
            .ok()?;
        let mut font = SystemUiFont::from_parsed(parse_pango(&desc)?, Source::Portal);
        let factor = settings
            .read::<f64>(GNOME_INTERFACE, "text-scaling-factor")
            .await
            .ok();
        apply_text_scaling(&mut font, factor);
        Some(font)
    };
    if desktop == Desktop::Kde {
        match kde.await {
            Some(font) => Some(font),
            None => gnome.await,
        }
    } else {
        match gnome.await {
            Some(font) => Some(font),
            None => kde.await,
        }
    }
}

/// Whether a portal `SettingChanged` signal concerns the UI font.
fn is_font_setting(namespace: &str, key: &str) -> bool {
    match namespace {
        GNOME_INTERFACE => matches!(key, "font-name" | "text-scaling-factor"),
        CINNAMON_INTERFACE | MATE_INTERFACE => key == "font-name",
        KDE_GENERAL => key == "font",
        _ => false,
    }
}

pub(super) fn change_signals() -> impl iced::futures::Stream<Item = ()> + Send + 'static {
    use iced::futures::{SinkExt, StreamExt};
    // The signal stream borrows the proxy, so both live in the producer.
    iced::stream::channel(1, async |mut output| {
        let Ok(settings) = ashpd::desktop::settings::Settings::new().await else {
            log::info!("UI font: no Settings portal; font changes apply after restart");
            return;
        };
        let Ok(mut changes) = settings.receive_setting_changed().await else {
            log::info!("UI font: portal change signal unavailable; changes apply after restart");
            return;
        };
        while let Some(setting) = changes.next().await {
            if is_font_setting(setting.namespace(), setting.key()) && output.send(()).await.is_err()
            {
                return;
            }
        }
    })
}

/// Fontconfig's resolution of a pattern to a concrete family name.
fn fc_match(pattern: &str) -> Option<String> {
    let out = run("fc-match", &["--format=%{family[0]}", pattern])?;
    Some(out.trim().to_string()).filter(|f| !f.is_empty())
}

fn fontconfig_sans() -> Option<SystemUiFont> {
    Some(SystemUiFont {
        family: Some(fc_match("sans-serif")?),
        alternates: Vec::new(),
        weight: 400,
        italic: false,
        size_px: None,
        source: Source::Fontconfig,
    })
}

/// Run a settings helper with a bounded wait; `None` if it is missing, fails
/// or does not finish in time.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + READ_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
    let mut out = String::new();
    use std::io::Read;
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_names() {
        assert_eq!(Desktop::from_names("KDE"), Desktop::Kde);
        assert_eq!(Desktop::from_names("ubuntu:GNOME"), Desktop::Gnome);
        assert_eq!(Desktop::from_names("X-Cinnamon"), Desktop::Cinnamon);
        assert_eq!(Desktop::from_names("MATE"), Desktop::Mate);
        assert_eq!(Desktop::from_names("XFCE"), Desktop::Xfce);
        assert_eq!(Desktop::from_names("sway"), Desktop::Other);
        assert_eq!(Desktop::from_names(""), Desktop::Other);
    }

    #[test]
    fn font_change_filter() {
        assert!(is_font_setting(GNOME_INTERFACE, "font-name"));
        assert!(is_font_setting(GNOME_INTERFACE, "text-scaling-factor"));
        assert!(is_font_setting(KDE_GENERAL, "font"));
        assert!(!is_font_setting(GNOME_INTERFACE, "color-scheme"));
        assert!(!is_font_setting(
            "org.freedesktop.appearance",
            "color-scheme"
        ));
    }

    #[test]
    fn missing_helper_is_none() {
        assert_eq!(run("ocs-definitely-not-a-program", &[]), None);
    }
}
