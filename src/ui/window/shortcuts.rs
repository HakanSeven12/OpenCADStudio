//! Keyboard Shortcuts Reference window — fills the entire OS window.

use crate::app::Message;
use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Background, Color, Element, Fill, Theme};
use std::borrow::Cow;

/// Display name of the primary accelerator modifier on this platform.
/// Mirrors the runtime binding in `app::view`, which uses
/// `Modifiers::command()` — the Cmd key on macOS, Ctrl elsewhere — so the
/// reference window shows the key the user actually presses.
#[cfg(target_os = "macos")]
const MOD: &str = "Cmd";
#[cfg(not(target_os = "macos"))]
const MOD: &str = "Ctrl";

const TB: Color = Color {
    r: 0.13,
    g: 0.13,
    b: 0.13,
    a: 1.0,
};
const BG: Color = Color {
    r: 0.15,
    g: 0.15,
    b: 0.15,
    a: 1.0,
};
const BORDER: Color = Color {
    r: 0.35,
    g: 0.35,
    b: 0.35,
    a: 1.0,
};
const DIM: Color = Color {
    r: 0.55,
    g: 0.55,
    b: 0.55,
    a: 1.0,
};
const ACCENT: Color = Color {
    r: 0.25,
    g: 0.50,
    b: 0.85,
    a: 1.0,
};
const KEY: Color = Color {
    r: 0.40,
    g: 0.70,
    b: 1.00,
    a: 1.0,
};

fn hdivider<'a>() -> Element<'a, Message> {
    container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(BORDER)),
            ..Default::default()
        })
        .into()
}

fn shortcut_row<'a>(
    key: impl Into<Cow<'static, str>>,
    action: &'static str,
) -> Element<'a, Message> {
    row![
        text(key.into())
            .size(11)
            .color(KEY)
            .font(iced::Font::MONOSPACE)
            .width(160),
        text(action).size(11),
    ]
    .spacing(8)
    .align_y(iced::Center)
    .padding([2, 0])
    .into()
}

fn section<'a>(title: impl Into<Cow<'static, str>>) -> Element<'a, Message> {
    container(text(title.into()).size(11).color(DIM))
        .padding(iced::Padding {
            top: 6.0,
            right: 0.0,
            bottom: 2.0,
            left: 0.0,
        })
        .into()
}

pub fn view_window<'a>(
    overrides: &'a rustc_hash::FxHashMap<String, String>,
) -> Element<'a, Message> {
    // ── Toolbar ───────────────────────────────────────────────────────────
    let toolbar = container(
        row![
            text("Type  SHORTCUTS SET <key> <cmd>  to add custom shortcuts.")
                .size(10)
                .color(DIM),
        ]
        .align_y(iced::Center),
    )
    .style(|_: &Theme| container::Style {
        background: Some(Background::Color(TB)),
        ..Default::default()
    })
    .width(Fill)
    .padding([5, 10]);

    // ── Shortcut entries ──────────────────────────────────────────────────
    let mut rows: Vec<Element<'_, Message>> = vec![
        section("── Function Keys ──────────────────────────────────────"),
        shortcut_row("F3", "Toggle Object Snap"),
        shortcut_row("F7", "Toggle Grid"),
        shortcut_row("F8", "Toggle Ortho"),
        shortcut_row("F9", "Toggle Grid Snap"),
        shortcut_row("F10", "Toggle Polar Tracking"),
        shortcut_row("F11", "Toggle Object Snap Tracking"),
        shortcut_row("F12", "Toggle Dynamic Input"),
        section(format!(
            "── {MOD} Shortcuts ──────────────────────────────────────"
        )),
        shortcut_row(format!("{MOD}+N"), "New Drawing"),
        shortcut_row(format!("{MOD}+O"), "Open File"),
        shortcut_row(format!("{MOD}+S"), "Save"),
        shortcut_row(format!("{MOD}+Shift+S"), "Save As"),
        shortcut_row(format!("{MOD}+Z"), "Undo"),
        shortcut_row(format!("{MOD}+Shift+Z / {MOD}+Y"), "Redo"),
        shortcut_row(format!("{MOD}+C"), "Copy to Clipboard"),
        shortcut_row(format!("{MOD}+X"), "Cut to Clipboard"),
        shortcut_row(format!("{MOD}+V"), "Paste from Clipboard"),
        section("── Other Keys ──────────────────────────────────────────"),
        shortcut_row("Enter / Space", "Finalize command / Repeat last"),
        shortcut_row("Escape", "Cancel active command"),
        shortcut_row("Delete", "Delete selected entities"),
        shortcut_row("↑ / ↓", "Command history navigation"),
    ];

    // Custom overrides section
    rows.push(section(
        "── Custom Overrides (SHORTCUTS SET) ──────────────────",
    ));
    if overrides.is_empty() {
        rows.push(
            text("  (none — use: SHORTCUTS SET <key> <command>)")
                .size(11)
                .color(DIM)
                .into(),
        );
    } else {
        let mut sorted: Vec<_> = overrides.iter().collect();
        sorted.sort_by_key(|(k, _)| k.as_str());
        for (key, cmd) in sorted {
            rows.push(
                row![
                    text(key.as_str())
                        .size(11)
                        .color(KEY)
                        .font(iced::Font::MONOSPACE)
                        .width(160),
                    text(cmd.as_str()).size(11),
                ]
                .spacing(8)
                .align_y(iced::Center)
                .padding([2, 0])
                .into(),
            );
        }
    }

    // ── Section headers styled separately ────────────────────────────────
    let content = scrollable(column(rows).spacing(3).padding([12, 16]))
        .width(Fill)
        .height(Fill);

    // ── Header row with accent ────────────────────────────────────────────
    let header = container(
        row![
            text("Key").size(10).color(ACCENT).width(160),
            text("Action").size(10).color(ACCENT),
        ]
        .spacing(8)
        .padding([4, 16]),
    )
    .style(|_: &Theme| container::Style {
        background: Some(Background::Color(Color {
            r: 0.13,
            g: 0.13,
            b: 0.18,
            a: 1.0,
        })),
        ..Default::default()
    })
    .width(Fill);

    container(column![toolbar, hdivider(), header, hdivider(), content].spacing(0))
        .style(|_: &Theme| container::Style {
            background: Some(Background::Color(BG)),
            ..Default::default()
        })
        .width(Fill)
        .height(Fill)
        .into()
}
