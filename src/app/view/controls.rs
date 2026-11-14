use super::super::tab_document::{DynComponent, DynFieldEntry};
use super::super::Message;
use super::*;
use iced::widget::{button, column, container, mouse_area, row, text, tooltip};
use iced::{Background, Border, Element, Length, Theme};

fn viewport_tooltip<'a>(
    control: impl Into<Element<'a, Message>>,
    title: String,
    command: &'static str,
) -> Element<'a, Message> {
    let text = format!("{title}\n{} {command}", crate::t!("Command:"));
    crate::ui::kit::tooltip_gap(
        control,
        text,
        tooltip::Position::Bottom,
        crate::ui::style::ribbon_metrics::TIP_GAP,
    )
}

// ── Viewport control bar: the prototype's measurements ─────────────────────
//
// iced draws a border inside its container, so the 1 px border is part of
// the bar's padding: 1 px top and bottom, 1 + 3 px left and right.

/// Height of the bar: 22 px of content and the 1 px border top and bottom.
const VP_BAR_H: f32 = 24.0;
/// The bar's border width.
const VP_BORDER: f32 = 1.0;
/// Room between the border and the first / last item.
const VP_PAD_X: f32 = 3.0;
/// Space between items (no separators).
const VP_GAP: f32 = 2.0;
/// Corner radius of the bar.
const VP_BAR_RADIUS: f32 = 3.0;
/// Opacity of the bar's background over the drawing.
const VP_BAR_ALPHA: f32 = 0.88;
/// Side of an icon button.
const VP_BTN: f32 = 20.0;
/// Icon size inside a button.
const VP_ICON: f32 = 16.0;
/// Corner radius of a lit or hovered button.
const VP_BTN_RADIUS: f32 = 2.0;
/// Height of the visual-style picker head and its side padding.
const VP_PICKER_H: f32 = 20.0;
const VP_PICKER_PAD_X: f32 = 6.0;
/// Gap between the picker's text and its arrow.
const VP_PICKER_GAP: f32 = 6.0;
/// Text size of the picker and its flyout (whole pixels).
const VP_TEXT: f32 = crate::ui::style::text::UI;
/// Line height of that text, in whole pixels.
const VP_TEXT_LINE: f32 = crate::ui::style::text::UI_LINE;
/// The flyout: row height, the check-mark column, the right padding, the
/// vertical padding of the frame and how far below the head it opens.
const VP_MENU_ROW_H: f32 = 22.0;
const VP_MENU_CHECK_W: f32 = 26.0;
const VP_MENU_PAD_R: f32 = 16.0;
const VP_MENU_PAD_Y: f32 = 3.0;
const VP_MENU_OFFSET: f32 = 3.0;
/// Narrowest flyout.
const VP_MENU_MIN_W: f32 = 180.0;
/// How far the danger colour tints the close button's hover.
const VP_DANGER_TINT: f32 = 0.25;
/// Distance of the bar from the drawing area's top-left corner.
pub(super) const VP_INSET: f32 = 6.0;

/// Width of the visual-style flyout for `labels`: the longest name plus the
/// check column, the right padding and the frame's border.
fn vp_menu_width<'a>(labels: impl Iterator<Item = &'a str>) -> f32 {
    crate::ui::style::common::dropdown_popup_width(
        labels,
        VP_TEXT,
        VP_MENU_CHECK_W + VP_MENU_PAD_R + 2.0 * VP_BORDER,
        VP_MENU_MIN_W,
    )
}

/// Background of a highlighted flyout row: the palette's `row_selected`
/// (the accent at 38 % over the panel).
pub(crate) fn vp_row_selected(theme: &Theme) -> iced::Color {
    crate::ui::style::chrome::row_selected(theme)
}

/// Background of the bar: the theme background, faintly showing the
/// drawing through.
pub(crate) fn vp_bar_background(theme: &Theme) -> iced::Color {
    crate::ui::style::chrome::bar(theme).scale_alpha(VP_BAR_ALPHA)
}

/// A bar button's look: idle flat; hovered `hover` with a `hover_border`;
/// an `active` toggle lit like a ribbon button, its border lighter while
/// hovered. `danger` tints the hover with the danger colour (close).
fn vp_button_style(active: bool, hovered: bool, danger: bool, theme: &Theme) -> container::Style {
    use crate::ui::style::chrome;
    let (background, border) = match (active, hovered) {
        (true, false) => (Some(chrome::lit(theme)), chrome::lit_border(theme)),
        (true, true) => (Some(chrome::lit(theme)), chrome::hover_border(theme)),
        (false, true) if danger => (
            Some(chrome::mix(
                chrome::panel(theme),
                chrome::danger(theme),
                VP_DANGER_TINT,
            )),
            chrome::hover_border(theme),
        ),
        (false, true) => (Some(chrome::hover(theme)), chrome::hover_border(theme)),
        (false, false) => (None, iced::Color::TRANSPARENT),
    };
    container::Style {
        background: background.map(Background::Color),
        border: Border {
            color: border,
            width: if background.is_some() { VP_BORDER } else { 0.0 },
            radius: VP_BTN_RADIUS.into(),
        },
        ..Default::default()
    }
}

/// The icon of a bar button in `color`.
fn vp_icon<'a>(bytes: &'static [u8], color: fn(&Theme) -> iced::Color) -> Element<'a, Message> {
    crate::ui::icons::tinted(bytes, VP_ICON, color)
}

/// A 20 px square cell holding `icon`, styled for `active` / `hovered`.
fn vp_cell<'a>(
    icon: Element<'a, Message>,
    active: bool,
    hovered: bool,
    danger: bool,
) -> Element<'a, Message> {
    container(icon)
        .center_x(Length::Fixed(VP_BTN))
        .center_y(Length::Fixed(VP_BTN))
        .style(move |theme: &Theme| vp_button_style(active, hovered, danger, theme))
        .into()
}

/// A bar button drawn by hover state: `idle` normally, `hovered` while the
/// pointer is over it. Lets a `mouse_area` (which has no status) look like
/// the buttons.
fn vp_hover_cell<'a>(bytes: &'static [u8], active: bool, danger: bool) -> Element<'a, Message> {
    use crate::ui::style::chrome;
    let idle: fn(&Theme) -> iced::Color = if active { chrome::strong } else { chrome::text };
    let hover: fn(&Theme) -> iced::Color = if danger {
        chrome::danger
    } else if active {
        chrome::strong
    } else {
        chrome::text
    };
    iced::widget::hover(
        vp_cell(vp_icon(bytes, idle), active, false, danger),
        vp_cell(vp_icon(bytes, hover), active, true, danger),
    )
}

pub(super) fn viewport_controls<'a>(
    render_mode: document::entities::ViewportRenderMode,
    show_grid: bool,
    snap_on: bool,
    include_split: bool,
    tile_count: usize,
    render_mode_menu_open: bool,
    render_mode_preview: Option<document::entities::ViewportRenderMode>,
    style_name: Option<String>,
    hover_outline: (bool, u8),
) -> Element<'a, Message> {
    use crate::ui::style::chrome;
    let render_modes: Vec<RenderModeChoice> = crate::modules::view::visual_style::VISUAL_STYLES
        .iter()
        .map(|style| RenderModeChoice(style.mode))
        .collect();

    // A bar button: flat until hovered, an `active` toggle lit; `danger`
    // (close) turns its icon and hover the danger colour.
    let bar_btn = move |bytes: &'static [u8],
                        active: bool,
                        danger: bool,
                        msg: Message,
                        title: String,
                        command: &'static str| {
        let button = button(vp_hover_cell(bytes, active, danger))
            .on_press(msg)
            .padding(0)
            .style(|_: &Theme, _| button::Style::default());
        viewport_tooltip(button, title, command)
    };
    let icon_btn =
        move |bytes: &'static [u8],
              active: bool,
              msg: Message,
              title: String,
              command: &'static str| { bar_btn(bytes, active, false, msg, title, command) };
    let danger_btn =
        move |bytes: &'static [u8], msg: Message, title: String, command: &'static str| {
            bar_btn(bytes, false, true, msg, title, command)
        };

    // The standard picker cannot show a live sample beside its rows. This
    // flyout keeps the current style unchanged while hovering; a click commits.
    let arrow = if render_mode_menu_open {
        crate::ui::icons::dropup_arrow()
    } else {
        crate::ui::icons::dropdown_arrow()
    };
    let picker_head = button(
        row![
            text(style_name.unwrap_or_else(|| RenderModeChoice(render_mode).to_string()))
                .size(VP_TEXT)
                .line_height(iced::Pixels(VP_TEXT_LINE))
                .wrapping(iced::advanced::text::Wrapping::None),
            arrow,
        ]
        .spacing(VP_PICKER_GAP)
        .height(Length::Fill)
        .align_y(iced::Center),
    )
    .on_press(Message::ToggleRenderModeMenu(render_mode))
    .height(Length::Fixed(VP_PICKER_H))
    .padding([0.0, VP_PICKER_PAD_X])
    .style(move |theme: &Theme, status| {
        let lit = render_mode_menu_open
            || matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: lit.then(|| Background::Color(chrome::hover(theme))),
            text_color: if lit {
                chrome::strong(theme)
            } else {
                chrome::text(theme)
            },
            border: Border {
                color: if lit {
                    chrome::hover_border(theme)
                } else {
                    iced::Color::TRANSPARENT
                },
                width: if lit { VP_BORDER } else { 0.0 },
                radius: VP_BTN_RADIUS.into(),
            },
            ..Default::default()
        }
    });
    let picker_head = viewport_tooltip(
        picker_head,
        crate::t!("Visual Style").into_owned(),
        "VISUALSTYLES",
    );

    let preview_mode = render_mode_preview.unwrap_or(render_mode);
    let mode_names: Vec<String> = render_modes.iter().map(|c| c.to_string()).collect();
    let dropdown_w = vp_menu_width(mode_names.iter().map(|s| s.as_str()));

    let mut choices = column![].width(Length::Fill);
    for choice in render_modes {
        let highlighted = choice.0 == preview_mode;
        let selected = choice.0 == render_mode;
        let checkmark = container(crate::ui::icons::themed_check_cell(selected))
            .center_x(Length::Fixed(VP_MENU_CHECK_W));
        let label = text(choice.to_string())
            .size(VP_TEXT)
            .line_height(iced::Pixels(VP_TEXT_LINE))
            .wrapping(iced::advanced::text::Wrapping::None);
        let row_content = row![checkmark, label]
            .height(Length::Fill)
            .align_y(iced::alignment::Vertical::Center);
        let option = container(row_content)
            .padding(iced::Padding::ZERO.right(VP_MENU_PAD_R))
            .width(Length::Fill)
            .height(Length::Fixed(VP_MENU_ROW_H))
            .style(move |theme: &Theme| container::Style {
                background: highlighted.then(|| Background::Color(vp_row_selected(theme))),
                text_color: Some(if highlighted {
                    chrome::strong(theme)
                } else {
                    chrome::text(theme)
                }),
                ..Default::default()
            });
        choices = choices.push(
            mouse_area(option)
                .interaction(iced::mouse::Interaction::Pointer)
                .on_enter(Message::PreviewRenderMode(choice.0))
                .on_press(Message::SetRenderMode(choice.0)),
        );
    }
    let popup = container(choices)
        .padding([VP_MENU_PAD_Y + VP_BORDER, VP_BORDER])
        .width(Length::Fixed(dropdown_w))
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::panel(theme))),
            border: Border {
                color: chrome::border(theme),
                width: VP_BORDER,
                radius: 0.0.into(),
            },
            text_color: Some(chrome::text(theme)),
            ..Default::default()
        });
    let picker: Element<'a, Message> =
        iced_aw::DropDown::new(picker_head, popup, render_mode_menu_open)
            .width(Length::Fixed(dropdown_w))
            .alignment(iced_aw::drop_down::Alignment::Bottom)
            .offset(VP_MENU_OFFSET)
            .on_dismiss(Message::DismissRenderModeMenu)
            .into();

    // No separators: the gap alone parts the items.
    let mut bar = row![]
        .spacing(VP_GAP)
        .align_y(iced::alignment::Vertical::Center);
    bar = bar
        .push(icon_btn(
            crate::ui::icons::GRID,
            show_grid,
            Message::ToggleGrid,
            crate::t!("Toggle Grid").into_owned(),
            "GRID",
        ))
        .push(icon_btn(
            crate::ui::icons::SNAP,
            snap_on,
            Message::ToggleGridSnap,
            crate::t!("Toggle Grid Snap").into_owned(),
            "SNAP",
        ))
        .push(picker);
    if include_split {
        bar = bar
            .push(icon_btn(
                crate::ui::icons::SPLIT_V,
                false,
                Message::SplitModelViewport(false),
                crate::tr!("viewport", "split-vertical"),
                "VPORTS 2V",
            ))
            .push(icon_btn(
                crate::ui::icons::SPLIT_H,
                false,
                Message::SplitModelViewport(true),
                crate::tr!("viewport", "split-horizontal"),
                "VPORTS 2H",
            ));
        // Drag handle + close: only meaningful with more than one model tile.
        // The handle is a `mouse_area` (not a button) so it fires on press-DOWN,
        // letting the drag continue onto the target pane to swap them (a button
        // would only fire on release). Placed just left of Close.
        if tile_count > 1 {
            let drag = mouse_area(vp_hover_cell(crate::ui::icons::MOVE, false, false))
                .interaction(iced::mouse::Interaction::Grab)
                .on_press(Message::PaneMoveStart);
            let drag = viewport_tooltip(drag, crate::tr!("viewport", "move"), "VPORTS");
            // Hover outline switch: one small icon, lit while the outline is
            // on. A click switches it; the wheel over it sets the strength.
            let (outline_on, outline_opacity) = hover_outline;
            let outline = mouse_area(vp_hover_cell(VPORTS_HIGHLIGHT_ICON, outline_on, false))
                .interaction(iced::mouse::Interaction::Pointer)
                .on_press(Message::ViewportHighlightToggle)
                .on_enter(Message::ViewportOutlineSwitchHover(true))
                .on_exit(Message::ViewportOutlineSwitchHover(false))
                .on_scroll(move |delta| {
                    Message::ViewportHighlightOpacity(outline_opacity_step(outline_opacity, delta))
                });
            let outline = viewport_tooltip(
                outline,
                outline_tooltip(outline_on, outline_opacity),
                "VPHIGHLIGHT",
            );
            bar = bar.push(outline).push(drag).push(danger_btn(
                crate::ui::icons::CLOSE,
                Message::CloseModelViewport,
                crate::tr!("viewport", "close"),
                "VPORTS SINGLE",
            ));
        }
    }

    container(bar)
        .height(Length::Fixed(VP_BAR_H))
        .padding([VP_BORDER, VP_BORDER + VP_PAD_X])
        .align_y(iced::alignment::Vertical::Center)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(vp_bar_background(theme))),
            border: Border {
                color: chrome::strip_divider(theme),
                width: VP_BORDER,
                radius: VP_BAR_RADIUS.into(),
            },
            ..Default::default()
        })
        .into()
}

const VPORTS_HIGHLIGHT_ICON: &[u8] = include_bytes!("../../../assets/icons/vports_highlight.svg");

/// Tooltip of the hover-outline switch: its state and how to change it.
fn outline_tooltip(on: bool, opacity: u8) -> String {
    let state = if on {
        format!("{} {opacity}%", crate::t!("on,"))
    } else {
        crate::t!("off").into_owned()
    };
    format!(
        "{} {state}\n{}",
        crate::t!("Hover outline:"),
        crate::t!("Click: on/off. Mouse wheel: intensity.")
    )
}

/// One wheel notch over the hover-outline switch: 5 % up or down.
fn outline_opacity_step(opacity: u8, delta: iced::mouse::ScrollDelta) -> u8 {
    let y = match delta {
        iced::mouse::ScrollDelta::Lines { y, .. } => y,
        iced::mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
    };
    let step = if y > 0.0 {
        5
    } else if y < 0.0 {
        -5
    } else {
        0
    };
    (i32::from(opacity) + step).clamp(5, 100) as u8
}

// ── Dynamic-input field formatting ─────────────────────────────────────────

/// Short prefix shown before a dynamic-input box's value.
/// The string shown inside a dynamic-input box: the typed buffer when the
/// field is locked, otherwise the live value derived from the cursor
/// world position (and the base point for polar quantities).
pub(super) fn dyn_component_value(
    f: &DynFieldEntry,
    w: glam::DVec3,
    base: Option<glam::DVec3>,
    xf: &super::super::helpers::UcsXform,
    comma_cartesian: bool,
    absolute: bool,
) -> String {
    if let Some(b) = &f.buffer {
        return b.clone();
    }
    let b = base.unwrap_or(glam::DVec3::ZERO);
    let p = xf.to_ucs(w);
    // Relative deltas and the polar angle read in the active UCS plane. The
    // delta is offset-invariant, so only the axis rotation matters (identity
    // xf reproduces the world-frame deltas).
    let d = xf.vec_to_ucs(w - b);
    let dx = d.x;
    let dy = d.y;
    // When a base point exists (DYN-on after the first pick) the cartesian
    // fields show relative deltas — matching the typed-value convention
    // in `dyn_resolve_point` so the live preview and the committed
    // coordinate use the same frame. See #35.
    let relative = base.is_some() && !absolute;
    // Width / Height read as unsigned magnitudes (the sign is taken from the
    // cursor side on commit), matching the rectangle's two-edge entry. But once
    // the user separates the values with `,` the entry is a cartesian
    // coordinate pair, so the fields read as signed X/Y deltas to match the
    // committed point (#269).
    let wh = matches!(
        f.role,
        crate::command::DynRole::Width | crate::command::DynRole::Height
    ) && relative
        && !comma_cartesian;
    match f.component {
        DynComponent::X if relative => {
            crate::entities::common::format_length(if wh { dx.abs() } else { dx })
        }
        DynComponent::Y if relative => {
            crate::entities::common::format_length(if wh { dy.abs() } else { dy })
        }
        DynComponent::Z if relative => crate::entities::common::format_length(0.0),
        DynComponent::X => crate::entities::common::format_length(p.x),
        DynComponent::Y => crate::entities::common::format_length(p.y),
        DynComponent::Z => crate::entities::common::format_length(p.z),
        // Scaled by the role so a diameter box reads twice the radius.
        DynComponent::Distance => crate::entities::common::format_length(
            (dx * dx + dy * dy).sqrt() * f.role.value_scale() as f64,
        ),
        // Shared rule: unsigned magnitude of the short angle, so CW (below the
        // reference axis) reads positive (e.g. 30°, not -30°/330°). The
        // committed value stays signed (see dyn_resolve_point).
        DynComponent::Angle => crate::entities::common::format_angle(
            (crate::command::dyn_display_angle_deg(dy.atan2(dx) as f32) as f64).to_radians(),
        ),
        // Typed-only scalar — no geometric value to track when empty.
        DynComponent::Scalar => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::ViewportRenderMode as M;

    #[test]
    fn the_outline_switch_wheel_steps_the_strength_within_range() {
        use iced::mouse::ScrollDelta;
        let up = ScrollDelta::Lines { x: 0.0, y: 1.0 };
        let down = ScrollDelta::Lines { x: 0.0, y: -1.0 };
        assert_eq!(outline_opacity_step(50, up), 55);
        assert_eq!(outline_opacity_step(50, down), 45);
        assert_eq!(outline_opacity_step(100, up), 100);
        assert_eq!(
            outline_opacity_step(5, down),
            5,
            "never wheeled out of sight"
        );
        assert!(outline_tooltip(true, 50).contains("50%"));
    }

    #[test]
    fn the_bar_has_the_board_sizes_on_whole_pixels() {
        // 22 px of content inside a 1 px border; 20 px buttons with 16 px
        // icons; 22 px flyout rows; 11 px text.
        assert_eq!(VP_BAR_H - 2.0 * VP_BORDER, 22.0);
        const { assert!(VP_BTN <= VP_BAR_H - 2.0 * VP_BORDER) };
        assert_eq!(
            (VP_BTN, VP_ICON, VP_MENU_ROW_H, VP_TEXT),
            (20.0, 16.0, 22.0, 11.0)
        );
        for v in [
            VP_BAR_H,
            VP_PAD_X,
            VP_GAP,
            VP_BTN,
            VP_ICON,
            VP_PICKER_H,
            VP_TEXT,
            VP_TEXT_LINE,
            VP_MENU_ROW_H,
            VP_MENU_CHECK_W,
            VP_MENU_PAD_R,
            VP_INSET,
        ] {
            assert_eq!(v.fract(), 0.0, "{v}");
        }
    }

    #[test]
    fn test_viewport_controls_construction() {
        for style in crate::modules::view::visual_style::VISUAL_STYLES {
            let _el = viewport_controls(
                style.mode,
                true,
                true,
                true,
                1,
                false,
                None,
                None,
                (true, 50),
            );
            let _el_open = viewport_controls(
                style.mode,
                false,
                false,
                false,
                2,
                true,
                Some(M::Wireframe3D),
                None,
                (false, 15),
            );
        }
    }

    use crate::ui::style::common::wcag_contrast;

    #[test]
    fn test_visual_style_dropdown_contrast_across_all_themes() {
        use crate::ui::style::chrome;
        for theme in iced::Theme::ALL {
            // The flyout sits on the panel; the highlighted row on the
            // accent mix, in the strong text colour.
            let panel = chrome::panel(theme);
            let text_contrast = wcag_contrast(chrome::text(theme), panel);
            assert!(
                text_contrast >= 4.5,
                "Theme {:?} row text contrast {:.2} on the panel is too low",
                theme,
                text_contrast
            );
            let row = vp_row_selected(theme);
            let selected_contrast = wcag_contrast(chrome::strong(theme), row);
            assert!(
                selected_contrast >= 3.0,
                "Theme {:?} highlighted row text contrast {:.2} is too low",
                theme,
                selected_contrast
            );
        }
    }

    #[test]
    fn test_visual_style_dropdown_width_accommodates_all_items() {
        let max_label_len = crate::modules::view::visual_style::VISUAL_STYLES
            .iter()
            .map(|c| c.label.chars().count())
            .max()
            .unwrap_or(0);
        let dropdown_w = vp_menu_width(
            crate::modules::view::visual_style::VISUAL_STYLES
                .iter()
                .map(|c| c.label),
        );

        // Longest default label is "Gouraud Shaded + Edges" (22 chars)
        assert_eq!(max_label_len, 22);
        // The width fits the 22-char label (as estimated at 11 px) plus the
        // check column, the right padding and the frame.
        let text_w = max_label_len as f32 * VP_TEXT * 0.68;
        assert!(dropdown_w >= text_w + VP_MENU_CHECK_W + VP_MENU_PAD_R);

        // CJK labels count wide characters double
        let cjk_labels = ["二维线框", "着色+边"];
        let cjk_w = vp_menu_width(cjk_labels.iter().copied());
        assert_eq!(cjk_w, VP_MENU_MIN_W);

        for style in crate::modules::view::visual_style::VISUAL_STYLES {
            assert!(!style.label.contains('\n'), "Label should be a single line");
        }
    }
}
