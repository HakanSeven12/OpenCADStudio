//! Bottom status bar — Model/Layout tabs + OSNAP toggle + status info

pub mod statusbar_config;
pub mod statusbar_menu;
pub mod status_menu;

use iced::widget::tooltip::Position as TipPos;
use iced::widget::{
    button, column, container, mouse_area, row, text, text_input, tooltip,
};
use iced::{Background, Border, Color, Element, Length, Theme};
use iced_aw::ContextMenu;
use std::sync::Arc;

/// Scrollable id of the status-bar layout-tab strip (retained so the existing
/// `Message::ScrollLayoutTabs` handler still resolves; the strip now flex-wraps
/// instead of scrolling).
pub const LAYOUT_TABS_SCROLL_ID: &str = "statusbar-layout-tabs";

/// Widget id of the inline layout-rename text input, so the rename can grab
/// keyboard focus the moment it opens (issue #86).
pub const LAYOUT_RENAME_INPUT_ID: &str = "layout_rename_input";

use crate::app::Message;
use crate::snap::Snapper;
use crate::ui::statusbar::statusbar_config::{StatusBarConfig, StatusPill};
use crate::ui::statusbar::status_menu::Entry as StatusMenuEntry;
use crate::ui::wrap_bar::{WrapBar, WrapFlow};

pub struct StatusMenuData<'a> {
    pub layout_names: Vec<String>,
    pub polar_custom_input: &'a str,
    pub scale_is_model: bool,
    pub scale_list: Vec<(String, f32, f64)>,
    pub has_selection: bool,
    pub selection_types: Vec<String>,
    pub selection_filter: &'a rustc_hash::FxHashSet<String>,
    pub tooltip_hidden: bool,
}

#[derive(Clone, Default)]
pub struct StatusBar {
    #[allow(dead_code)]
    pub coord_display: String,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            coord_display: "MODEL".into(),
        }
    }

    pub fn view<'a>(
        &'a self,
        snapper: &'a Snapper,
        ortho_mode: bool,
        polar_mode: bool,
        polar_increment_deg: f32,
        dyn_input: bool,
        otrack: bool,
        layouts: Vec<String>,
        reorderable_layouts: Vec<String>,
        current_layout: String,
        // Start/welcome view has no drawing to own layouts.
        is_start: bool,
        // If `Some((original, edit_value))`, the named tab shows a text input.
        rename_state: Option<&'a (String, String)>,
        // Scale of the first user viewport in the active paper layout.
        viewport_scale: Option<f64>,
        // Number of user viewports in the current paper layout (0 = model space).
        viewport_count: usize,
        // True when the user is editing inside a paper-space viewport (MSPACE).
        in_mspace: bool,
        // Whether the layout tabs (Model/Paper) are visible (LAYOUTTAB).
        show_layout_tabs: bool,
        // Current annotation scale for model space (1.0 = 1:1, 50.0 = 1:50, etc.).
        annotation_scale: f32,
        // True when the scale pill is interactive (always model space; paper space only when a viewport is active/selected).
        scale_pill_enabled: bool,
        // LWDISPLAY header flag — controls lineweight visibility in the viewport.
        lineweight_display: bool,
        // Live cursor position in model coordinates, for the coordinate readout.
        cursor_world: glam::DVec3,
        // $COORDS readout mode: 0 = static (updates only on a pick), 1 = live
        // absolute, 2 = polar (distance<angle from the last point while picking).
        coords_mode: i16,
        // The last committed point, for the static (0) and polar (2) readouts.
        last_point: Option<glam::DVec3>,
        // True while a command is prompting for a point (enables the polar readout).
        picking: bool,
        // True while clean-screen mode hides the ribbon and side panels.
        clean_screen: bool,
        // Drawing units (INSUNITS) for the units pill.
        insertion_units: i16,
        // True when objects are hidden by Isolate / Hide.
        isolation_active: bool,
        // Whether entity transparency is shown (Transparency pill state).
        transparency_display: bool,
        // Whether the Quick Properties floating panel is enabled.
        quick_properties: bool,
        // True when the selection filter is excluding at least one type.
        selection_filter_active: bool,
        // Whether selection cycling is enabled.
        selection_cycling: bool,
        // Which pills the user has chosen to show on the bar.
        config: &'a StatusBarConfig,
        menu_data: StatusMenuData<'a>,
    ) -> Element<'a, Message> {
        let StatusMenuData {
            layout_names,
            polar_custom_input,
            scale_is_model,
            scale_list,
            has_selection,
            selection_types,
            selection_filter,
            tooltip_hidden,
        } = menu_data;

        // Leftmost hamburger: opens a dropdown listing Model + every layout, so
        // a layout can be picked directly even when the tab strip is scrolled.
        let menu_icon = if is_start {
            crate::ui::icons::themed_disabled(crate::ui::icons::MENU, 16.0)
        } else {
            crate::ui::icons::themed_secondary(crate::ui::icons::MENU, 16.0)
        };
        let menu_button = button(menu_icon)
            .style(button::subtle)
            .padding([4, 8]);
        let menu_btn = if is_start {
            tip(
                menu_button.into(),
                "Open or create a drawing to manage layouts.",
            )
        } else {
            status_menu::menu_bar(
                menu_tip(
                    menu_button
                        .on_press(Message::StatusMenuTooltipHidden(true))
                        .into(),
                    "Model and layout list",
                    tooltip_hidden,
                ),
                statusbar_menu::layout_entries(&layout_names, &current_layout),
                200.0,
            )
        };

        let add_button = button(text("+").size(12))
            .style(button::subtle)
            .padding([4, 8]);
        let add_btn = if is_start {
            tip(
                add_button.into(),
                "Open or create a drawing to add a layout.",
            )
        } else {
            add_button.on_press(Message::LayoutCreate).into()
        };

        // ── Right side ────────────────────────────────────────────────────
        let osnap_active = snapper.is_active();

        let vp_label = if viewport_count > 0 {
            format!("{} VP", viewport_count)
        } else {
            String::new()
        };
        // Scale pill: opens the scale picker popup.
        // Model space: always interactive, shows annotation scale.
        // Paper space: interactive only when a viewport is active/selected.
        let scale_label = if current_layout == "Model" {
            format_scale(Some(1.0 / annotation_scale as f64))
        } else {
            format_scale(viewport_scale)
        };
        let scale_element: Element<'_, Message> = if scale_pill_enabled {
            status_menu::menu_bar(
                menu_tip(
                    popup_pill(&scale_label),
                    "Annotation / Viewport Scale\nClick to change",
                    tooltip_hidden,
                ),
                crate::ui::popup::scale_popup::menu_entries(
                    scale_is_model,
                    annotation_scale,
                    viewport_scale,
                    scale_list,
                ),
                if scale_is_model { 150.0 } else { 120.0 },
            )
        } else {
            status_pill(scale_label).into()
        };
        // Build the right-side pills, honouring the user's per-pill visibility.
        // They live in a flex-wrap flow (WrapFlow) so they spill onto extra rows
        // when the width can't hold them all on one line.
        let vis = |p: StatusPill| config.is_visible(p);
        let mut pills: Vec<Element<'_, Message>> = Vec::new();
        if vis(StatusPill::Coords) {
            let coords_label = format_coords(cursor_world, last_point, coords_mode, picking);
            pills.push(
                tip(
                    action_pill(&coords_label, Message::CycleCoordsMode),
                    "Cursor coordinates ($COORDS)\nClick to cycle: static / live / polar",
                )
                .into(),
            );
        }
        if vis(StatusPill::Ortho) {
            pills.push(
                tip(
                    toggle_pill(crate::ui::icons::ST_ORTHO, ortho_mode, Message::ToggleOrtho),
                    "Orthogonal Mode\nF8",
                )
                .into(),
            );
        }
        if vis(StatusPill::Lwt) {
            pills.push(
                tip(
                    toggle_pill(crate::ui::icons::ST_LWT, lineweight_display, Message::ToggleLineweightDisplay),
                    "Show Lineweight\nLWDISPLAY",
                )
                .into(),
            );
        }
        if vis(StatusPill::Polar) {
            pills.push(
                polar_pill(
                    polar_mode,
                    polar_increment_deg,
                    tooltip_hidden,
                    crate::ui::popup::polar_popup::menu_entries(
                        polar_increment_deg,
                        polar_custom_input,
                    ),
                )
                .into(),
            );
        }
        if vis(StatusPill::Dyn) {
            pills.push(
                tip(
                    toggle_pill(crate::ui::icons::ST_DYN, dyn_input, Message::ToggleDynInput),
                    "Dynamic Input\nF12",
                )
                .into(),
            );
        }
        if vis(StatusPill::Otrack) {
            pills.push(
                tip(
                    toggle_pill(crate::ui::icons::ST_OTRACK, otrack, Message::ToggleOTrack),
                    "Object Snap Tracking\nF11",
                )
                .into(),
            );
        }
        if vis(StatusPill::Osnap) {
            pills.push(
                osnap_btn(
                    osnap_active,
                    snapper.snap_enabled,
                    tooltip_hidden,
                    crate::ui::popup::snap_popup::menu_entries(snapper),
                )
                .into(),
            );
        }
        if vis(StatusPill::Space) {
            pills.push(
                tip(
                    space_mode_btn(&current_layout, in_mspace),
                    "PAPER: double-click viewport to enter MSPACE\nMODEL: click to switch to Model Space",
                )
                .into(),
            );
        }
        if vis(StatusPill::Scale) {
            pills.push(scale_element);
        }
        if vis(StatusPill::Units) {
            pills.push(
                status_menu::menu_bar(
                    menu_tip(
                        popup_pill(crate::ui::popup::units_popup::unit_short(insertion_units)),
                        "Drawing Units (INSUNITS)\nClick to change",
                        tooltip_hidden,
                    ),
                    crate::ui::popup::units_popup::menu_entries(insertion_units),
                    140.0,
                )
                .into(),
            );
        }
        if vis(StatusPill::Transparency) {
            pills.push(
                tip(
                    toggle_pill(
                        crate::ui::icons::ST_TRANSPARENCY,
                        transparency_display,
                        Message::ToggleTransparencyDisplay,
                    ),
                    "Show Transparency\nForce opaque when off",
                )
                .into(),
            );
        }
        if vis(StatusPill::Isolate) {
            pills.push(
                status_menu::menu_bar(
                    menu_tip(
                        toggle_pill(
                            crate::ui::icons::ST_ISOLATE,
                            isolation_active,
                            Message::StatusMenuTooltipHidden(true),
                        ),
                        "Isolate Objects\nClick for Isolate / Hide / End",
                        tooltip_hidden,
                    ),
                    crate::ui::popup::isolate_popup::menu_entries(
                        has_selection,
                        isolation_active,
                    ),
                    160.0,
                )
                .into(),
            );
        }
        if vis(StatusPill::QuickProps) {
            pills.push(
                tip(
                    toggle_pill(crate::ui::icons::ST_QUICKPROPS, quick_properties, Message::ToggleQuickProperties),
                    "Quick Properties\nFloating panel on selection",
                )
                .into(),
            );
        }
        if vis(StatusPill::SelFilter) {
            pills.push(
                status_menu::menu_bar(
                    menu_tip(
                        toggle_pill(
                            crate::ui::icons::ST_FILTER,
                            selection_filter_active,
                            Message::StatusMenuTooltipHidden(true),
                        ),
                        "Selection Filtering\nLimit which object types can be picked",
                        tooltip_hidden,
                    ),
                    crate::ui::popup::selection_filter_popup::menu_entries(
                        selection_types,
                        selection_filter,
                    ),
                    180.0,
                )
                .into(),
            );
        }
        if vis(StatusPill::SelCycle) {
            pills.push(
                tip(
                    toggle_pill(crate::ui::icons::ST_SELCYCLE, selection_cycling, Message::ToggleSelectionCycling),
                    "Selection Cycling\nRepeat-click to step through overlapping objects",
                )
                .into(),
            );
        }
        if vis(StatusPill::Vp) && !vp_label.is_empty() {
            pills.push(
                tip(
                    status_pill(vp_label).into(),
                    "Viewport count in active layout",
                )
                .into(),
            );
        }
        if vis(StatusPill::CleanScreen) {
            pills.push(
                tip(
                    toggle_pill(crate::ui::icons::ST_CLEANSCREEN, clean_screen, Message::ToggleCleanScreen),
                    "Clean Screen\nHide ribbon and panels",
                )
                .into(),
            );
        }
        // Customization handle: opens the pill show/hide menu.
        pills.push(
            status_menu::menu_bar(
                menu_tip(
                    customize_btn(),
                    "Customization\nShow or hide status-bar items",
                    tooltip_hidden,
                ),
                statusbar_menu::customization_entries(config),
                200.0,
            )
            .into(),
        );
        let right_status = WrapFlow::new(pills)
            .spacing_x(2.0)
            .row_h(30.0)
            .justify_end(true);

        // Left area: hamburger menu + Model/layout tabs in a flex-wrap flow, so
        // they spill onto lower rows when narrow (no scroll arrows). The pills
        // use the remaining space on the final tab row when they fit; otherwise
        // WrapBar adds another right-aligned row.
        let mut left: Vec<Element<'_, Message>> = Vec::new();
        left.push(menu_btn);
        if show_layout_tabs {
            let reorderable_layouts: Arc<[String]> = reorderable_layouts.into();
            for name in layouts {
                let is_active = name == current_layout;
                let renaming = rename_state
                    .filter(|(orig, _)| *orig == name)
                    .map(|(_, edit)| edit.as_str());
                left.push(
                    space_tab(
                        name,
                        is_active,
                        renaming,
                        !is_start,
                        reorderable_layouts.clone(),
                    )
                    .into(),
                );
            }
            left.push(add_btn.into());
        }
        let left_area = WrapFlow::new(left).spacing_x(2.0).row_h(30.0);

        let wrap = WrapBar::new(left_area.into(), right_status.into())
            .min_row_h(30.0)
            .justify_end(true);

        container(wrap)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                background: Some(Background::Color(palette.background.base.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
                }
            })
            .width(Length::Fill)
            // One row matches the drawing (document) tab bar height so the three
            // horizontal strips — tabs, status bar, command line — line up.
            // WrapBar vertically centres every pill (issue #216) and grows to a
            // second row when the width can't hold both blocks.
            .padding([0, 4])
            .into()
    }
}

// ── Coordinate readout ────────────────────────────────────────────────────

fn format_coords(cursor: glam::DVec3, last: Option<glam::DVec3>, mode: i16, picking: bool) -> String {
    let abs = |p: glam::DVec3| format!("{:.4}, {:.4}, {:.4}", p.x, p.y, p.z);
    match mode {
        // Static: show the last picked point; the readout freezes between picks.
        0 => abs(last.unwrap_or(cursor)),
        // Polar: distance < angle relative to the last point while a command is
        // prompting for a point; absolute otherwise.
        2 => match (picking, last) {
            (true, Some(l)) => {
                let d = cursor - l;
                let dist = (d.x * d.x + d.y * d.y).sqrt();
                let mut ang = d.y.atan2(d.x).to_degrees();
                if ang < 0.0 {
                    ang += 360.0;
                }
                format!("{dist:.4} < {ang:.2}\u{b0}")
            }
            _ => abs(cursor),
        },
        // 1 (default) and anything else: live absolute.
        _ => abs(cursor),
    }
}

// ── Customization handle ──────────────────────────────────────────────────

fn customize_btn() -> Element<'static, Message> {
    button(crate::ui::icons::themed_secondary(crate::ui::icons::MENU, 16.0))
        .on_press(Message::StatusMenuTooltipHidden(true))
        .style(button::subtle)
        .padding([4, 8])
        .into()
}

// ── Tooltip helper ────────────────────────────────────────────────────────

fn tip<'a>(content: Element<'a, Message>, label: &'static str) -> Element<'a, Message> {
    tip_node(content, text(label).size(11).into())
}

/// A menu root shows its tooltip until clicked. Moving from the root into the
/// opened menu resets suppression for the next hover without covering the menu.
fn menu_tip<'a>(
    content: Element<'a, Message>,
    label: &'static str,
    hidden: bool,
) -> Element<'a, Message> {
    let content = if hidden {
        content
    } else {
        tip(content, label)
    };

    mouse_area(content)
        .on_exit(Message::StatusMenuTooltipHidden(false))
        .into()
}

/// Like [`tip`] but the tooltip body is any element — used to embed an SVG
/// glyph (e.g. the dropdown caret) instead of a Unicode character that renders
/// as tofu on the web. (#138)
fn tip_node<'a>(content: Element<'a, Message>, body: Element<'a, Message>) -> Element<'a, Message> {
    tooltip(
        content,
        container(body)
            .style(container::bordered_box)
            .padding([4, 8]),
        TipPos::Top,
    )
    .into()
}

// ── Simple toggle pill ────────────────────────────────────────────────────

/// A status-bar toggle, drawn as a tinted icon (issue #216: the old size-10
/// text labels were too small to read). The name lives in the tooltip each
/// call site already wraps it with.
fn toggle_pill(icon: &'static [u8], active: bool, msg: Message) -> Element<'static, Message> {
    let icon = if active {
        crate::ui::icons::themed_primary(icon, 17.0)
    } else {
        crate::ui::icons::themed_secondary(icon, 17.0)
    };
    button(icon)
        .on_press(msg)
        .style(move |theme: &Theme, status| {
            let mut style = button::subtle(theme, status);
            if active {
                let palette = theme.extended_palette();
                style.background = Some(Background::Color(match status {
                    button::Status::Hovered => palette.primary.base.color,
                    _ => palette.primary.weak.color,
                }));
                style.text_color = palette.primary.weak.text;
                style.border.color = palette.primary.base.color;
                style.border.width = 1.0;
            }
            style
        })
        .padding([4, 7])
        .into()
}

// ── Split pill (toggle + dropdown caret) ──────────────────────────────────
//
// Shared chrome for every status-bar pill that pairs a toggle with a picker
// popup (OSNAP, POLAR, …): one outer border wraps the caller's `main` region
// and a dropdown caret, so both halves read as a single integrated control and
// the border / background / caret / sizing live in exactly ONE place.

/// Wrap a caller-built, already-click-wired `main` element together with its
/// menu-bearing dropdown caret.
fn split_pill<'a>(
    main: Element<'static, Message>,
    caret: Element<'a, Message>,
    active: bool,
) -> Element<'a, Message> {
    container(row![main, caret].spacing(3).align_y(iced::Center))
        .style(move |theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: Some(Background::Color(if active {
                palette.primary.weak.color
            } else {
                palette.background.weakest.color
            })),
            border: Border {
                color: if active {
                    palette.primary.base.color
                } else {
                    palette.background.neutral.color
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
            }
        })
        .padding([4, 6])
        .into()
}

// ── Polar tracking pill ───────────────────────────────────────────────────
//
// Main: left-click toggles polar on/off; right-click cycles the increment.
// Caret: opens the angle picker.

fn polar_pill<'a>(
    active: bool,
    increment_deg: f32,
    tooltip_hidden: bool,
    entries: Vec<StatusMenuEntry<'a>>,
) -> Element<'a, Message> {
    let angle = crate::ui::popup::polar_popup::angle_label(increment_deg);
    let tooltip_text = format!(
        "Polar Tracking ({angle})\nF10 — left-click on/off\nRight-click cycles · ▾ picks angle",
    );

    // Right-click quick-cycles through the same increments the picker lists.
    const CYCLE: &[f32] = &[90.0, 45.0, 30.0, 22.5, 18.0, 15.0, 10.0, 5.0, 1.0];
    let next_angle = match CYCLE.iter().position(|&a| (a - increment_deg).abs() < 1e-3) {
        Some(i) => CYCLE[(i + 1) % CYCLE.len()],
        None => 45.0,
    };

    let polar_icon = if active {
        crate::ui::icons::themed_primary(crate::ui::icons::ST_POLAR, 17.0)
    } else {
        crate::ui::icons::themed_secondary(crate::ui::icons::ST_POLAR, 17.0)
    };
    let main = mouse_area(
        row![
            polar_icon,
            text(angle).size(11).style(move |theme: &Theme| {
                let palette = theme.extended_palette();
                text::Style {
                    color: Some(if active {
                        palette.primary.base.color
                    } else {
                        palette.background.base.text.scale_alpha(0.72)
                    }),
                }
            }),
        ]
        .spacing(2)
        .align_y(iced::Center),
    )
    .on_press(Message::TogglePolar)
    .on_right_press(Message::SetPolarAngle(next_angle));
    let main = tooltip(
        main,
        container(text(tooltip_text).size(11))
            .style(container::bordered_box)
            .padding([4, 8]),
        TipPos::Top,
    );

    let caret = status_menu::menu_bar(
        menu_tip(
            mouse_area(
                container(if active {
                    crate::ui::icons::themed_primary_arrow_down(9.0)
                } else {
                    crate::ui::icons::themed_secondary_arrow_down(9.0)
                })
                .padding([4, 7]),
            )
            .on_press(Message::StatusMenuTooltipHidden(true))
            .into(),
            "Polar angle\nClick to choose",
            tooltip_hidden,
        ),
        entries,
        120.0,
    );

    split_pill(main.into(), caret, active)
}

// ── OSNAP pill ─────────────────────────────────────────────────────────────
//
// Main: toggles the global snap on/off. Caret: opens the snap-type dropdown.

fn osnap_btn<'a>(
    active: bool,
    snap_enabled: bool,
    tooltip_hidden: bool,
    entries: Vec<StatusMenuEntry<'a>>,
) -> Element<'a, Message> {
    let on = active || snap_enabled;
    let snap_icon = if on {
        crate::ui::icons::themed_primary(crate::ui::icons::ST_OSNAP, 17.0)
    } else {
        crate::ui::icons::themed_secondary(crate::ui::icons::ST_OSNAP, 17.0)
    };
    let main = tip(
        mouse_area(snap_icon)
        .on_press(Message::ToggleSnapEnabled)
        .into(),
        "Object Snap: toggle on/off\nF3",
    );

    let caret = status_menu::menu_bar(
        menu_tip(
            mouse_area(
                container(if on {
                    crate::ui::icons::themed_primary_arrow_down(9.0)
                } else {
                    crate::ui::icons::themed_secondary_arrow_down(9.0)
                })
                .padding([4, 7]),
            )
            .on_press(Message::StatusMenuTooltipHidden(true))
            .into(),
            "Object Snap list\nClick to choose snap types",
            tooltip_hidden,
        ),
        entries,
        210.0,
    );

    split_pill(main, caret, on)
}

// ── Helpers ───────────────────────────────────────────────────────────────

fn layout_tab_context_menu(name: String) -> Element<'static, Message> {
    let item = |label: &'static str, msg: Message| {
        button(text(label).size(12))
            .on_press(msg)
            .style(button::subtle)
            .padding([4, 12])
            .width(Length::Fill)
    };

    container(
        column![
            item("Rename", Message::LayoutRenameStart(name.clone())),
            item("Delete", Message::LayoutDelete(name)),
        ]
        .spacing(0)
        .width(160),
    )
    .style(container::bordered_box)
    .padding([4, 0])
    .into()
}

/// A layout tab button.
///
/// When `rename_edit` is `Some(value)` the tab shows an inline text input
/// instead of the normal button.  The tab is not renameable when it is the
/// "Model" tab (callers simply never pass `Some` for that name).
fn space_tab<'a>(
    label: String,
    is_active: bool,
    rename_edit: Option<&'a str>,
    enabled: bool,
    reorderable_layouts: Arc<[String]>,
) -> Element<'a, Message> {
    let tab_style = move |theme: &Theme| {
        let palette = theme.extended_palette();
        let text_color = if !enabled {
            palette.background.base.text.scale_alpha(0.42)
        } else if is_active {
            palette.primary.weak.text
        } else {
            palette.background.base.text.scale_alpha(0.72)
        };
        container::Style {
            background: is_active.then_some(Background::Color(palette.primary.weak.color)),
            border: Border {
                color: if is_active {
                    palette.primary.base.color
                } else {
                    Color::TRANSPARENT
                },
                width: if is_active { 1.0 } else { 0.0 },
                radius: 2.0.into(),
            },
            text_color: Some(text_color),
            ..Default::default()
        }
    };

    if !enabled {
        let display = container(text(label.clone()).size(12))
            .style(tab_style)
            .padding([4, 10]);
        crate::ui::wrap_bar::PosReport::owned(
            format!("SB_LAYOUT_TAB:{label}"),
            tip(
                display.into(),
                "Open or create a drawing to switch layouts.",
            ),
        )
        .into()
    } else if let Some(edit_val) = rename_edit {
        // Inline rename text input with a cancel (✕) button.
        let input = text_input("", edit_val)
            .id(iced::widget::Id::new(LAYOUT_RENAME_INPUT_ID))
            .on_input(Message::LayoutRenameEdit)
            .on_submit(Message::LayoutRenameCommit)
            .size(12)
            .padding([3, 6])
            .width(Length::Fixed(90.0));

        let cancel_btn = button(crate::ui::icons::themed_secondary(
            crate::ui::icons::CLOSE,
            10.0,
        ))
        .on_press(Message::LayoutRenameCancel)
        .style(button::subtle)
        .padding([4, 4]);

        row![input, cancel_btn]
            .spacing(0)
            .align_y(iced::Center)
            .into()
    } else {
        // Normal clickable tab — left click switches. Paper-layout tabs are
        // wrapped in `ContextMenu`, which owns right-click handling.
        let display = container(text(label.clone()).size(12))
            .style(tab_style)
            .padding([4, 10]);

        let switch_msg = Message::LayoutSwitch(label.clone());
        let has_context_menu = reorderable_layouts.contains(&label);
        let report_key = format!("SB_LAYOUT_TAB:{label}");
        let tab = mouse_area(display).on_press(switch_msg);
        let tab: Element<'a, Message> = if has_context_menu {
            crate::ui::wrap_bar::ReorderTab::layout(
                label.clone(),
                reorderable_layouts,
                tab,
            )
            .into()
        } else {
            tab.into()
        };

        let tab = if has_context_menu {
            ContextMenu::new(tab, move || layout_tab_context_menu(label.clone())).into()
        } else {
            tab
        };
        crate::ui::wrap_bar::PosReport::owned(
            report_key,
            tab,
        )
        .into()
    }
}

/// Space-mode toggle button in the status bar.
///
/// - Model tab            → "MODEL"  (non-clickable, informational)
/// - Layout, PSPACE       → "PAPER"  (click → MspaceCommand: enter MSPACE)
/// - Layout, MSPACE       → "MODEL"  (click → ExitViewport: return to PSPACE)
fn space_mode_btn(current_layout: &str, in_mspace: bool) -> Element<'static, Message> {
    let is_model_tab = current_layout == "Model";

    // Labels and styling follow AutoCAD convention:
    //   PAPER = currently in paper-space editing
    //   MODEL = currently in model-space editing (either the Model tab or MSPACE)
    let (label, active, on_press) = if is_model_tab {
        ("MODEL", false, None::<Message>)
    } else if in_mspace {
        ("MODEL", true, Some(Message::ExitViewport))
    } else {
        ("PAPER", false, Some(Message::MspaceCommand))
    };

    let clickable = on_press.is_some();
    let mut btn = button(text(label).size(12))
        .style(move |theme: &Theme, status| {
            let mut style = button::subtle(theme, status);
            if active {
                let palette = theme.extended_palette();
                style.background = Some(Background::Color(match status {
                    button::Status::Hovered if clickable => palette.primary.base.color,
                    _ => palette.primary.weak.color,
                }));
                style.text_color = palette.primary.weak.text;
                style.border.color = palette.primary.base.color;
                style.border.width = 1.0;
            }
            style
        })
        .padding([4, 7]);

    if let Some(msg) = on_press {
        btn = btn.on_press(msg);
    }

    btn.into()
}

fn status_pill(label: impl Into<String>) -> Element<'static, Message> {
    container(text(label.into()).size(12))
    .style(container::bordered_box)
    .padding([4, 8])
    .into()
}

// ── Scale popup button ────────────────────────────────────────────────────

/// Shared visual root for labelled status-bar menus (units, scale, …).
fn popup_pill(label: &str) -> Element<'static, Message> {
    action_pill(label, Message::StatusMenuTooltipHidden(true))
}

fn action_pill(label: &str, msg: Message) -> Element<'static, Message> {
    let label = label.to_string();
    button(text(label).size(12))
    .on_press(msg)
    .style(button::subtle)
    .padding([4, 7])
    .into()
}

// ── Scale display ─────────────────────────────────────────────────────────

/// Formats a viewport scale factor as a human-readable ratio string.
///
/// - `None`  → "1:1"  (model space or no viewport yet)
/// - `1.0`   → "1:1"
/// - `0.02`  → "1:50"
/// - `2.0`   → "2:1"
fn format_scale(scale: Option<f64>) -> String {
    let s = match scale {
        None => return "1:1".to_string(),
        Some(v) if v <= 0.0 => return "1:1".to_string(),
        Some(v) => v,
    };

    // Try to express as a clean integer ratio.
    if s >= 1.0 {
        let n = s.round() as u32;
        if (s - n as f64).abs() < 0.01 * s {
            return if n == 1 {
                "1:1".to_string()
            } else {
                format!("{}:1", n)
            };
        }
    } else {
        let inv = (1.0 / s).round() as u32;
        if (s - 1.0 / inv as f64).abs() < 0.01 * s {
            return format!("1:{}", inv);
        }
    }

    // Fall back to a decimal string.
    format!("{:.4}", s)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
