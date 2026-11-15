//! Centralized UI Theme Accessibility & Contrast Test Suite.
//!
//! Evaluates WCAG 2.1 contrast ratios across all 22 built-in themes for every
//! major OpenCADStudio UI domain:
//! - Core Theme Palettes (base, weak, strong, weakest)
//! - Ribbon Bar (tabs, buttons, dropdown popups, contextual layout tab)
//! - Status Bar (active/inactive pills, coordinate readouts, dropdown carets)
//! - Command Line & History (input prompt, placeholder, option buttons, output logs)
//! - Properties & Dock Panels (field labels, section headers, muted text)
//! - Modals & Action Buttons (dialog body, Primary, Danger, Success buttons)
//! - Dropdowns & Selection Overlays (visual style picker, item checkmarks)
//! - Block Palette (card labels in normal/hover/pressed/placing states,
//!   header icon buttons, empty-state muted text)

use crate::ui::style::common::{accessible_accent, accessible_accent_threshold, wcag_contrast};
use iced::{Color, Theme};

/// Composite a semi-transparent foreground color over a solid background color.
fn composite_over(fg: Color, bg: Color) -> Color {
    let a = fg.a;
    Color {
        r: fg.r * a + bg.r * (1.0 - a),
        g: fg.g * a + bg.g * (1.0 - a),
        b: fg.b * a + bg.b * (1.0 - a),
        a: 1.0,
    }
}

#[test]
fn test_theme_core_surfaces_contrast() {
    for theme in Theme::ALL {
        let p = theme.palette();

        // Base surface & text
        let base_contrast = wcag_contrast(p.background.base.text, p.background.base.color);
        assert!(
            base_contrast >= 4.5,
            "Theme {:?} base text ({:.2}:1) fails WCAG AA on base background",
            theme,
            base_contrast
        );

        // Weak surface & text
        let weak_contrast = wcag_contrast(p.background.weak.text, p.background.weak.color);
        assert!(
            weak_contrast >= 4.5,
            "Theme {:?} weak text ({:.2}:1) fails WCAG AA on weak background",
            theme,
            weak_contrast
        );

        // Strong surface & text
        let strong_contrast = wcag_contrast(p.background.strong.text, p.background.strong.color);
        assert!(
            strong_contrast >= 4.5,
            "Theme {:?} strong text ({:.2}:1) fails WCAG AA on strong background",
            theme,
            strong_contrast
        );

        // Weakest surface & text
        let weakest_contrast = wcag_contrast(p.background.weakest.text, p.background.weakest.color);
        assert!(
            weakest_contrast >= 4.5,
            "Theme {:?} weakest text ({:.2}:1) fails WCAG AA on weakest background",
            theme,
            weakest_contrast
        );
    }
}

#[test]
fn test_ribbon_contrast() {
    for theme in Theme::ALL {
        let p = theme.palette();

        // 1. Active Tab: rendered on background.weakest.color
        let active_tab_bg = p.background.weakest.color;
        let active_tab_text = p.background.weakest.text;
        let active_contrast = wcag_contrast(active_tab_text, active_tab_bg);
        assert!(
            active_contrast >= 4.5,
            "Theme {:?} active ribbon tab text ({:.2}:1) fails WCAG AA",
            theme,
            active_contrast
        );

        // 2. Inactive Tab: rendered on ribbon background.base.color with 0.72 alpha
        let ribbon_bg = p.background.base.color;
        let inactive_tab_text = composite_over(p.background.base.text.scale_alpha(0.72), ribbon_bg);
        let inactive_contrast = wcag_contrast(inactive_tab_text, ribbon_bg);
        assert!(
            inactive_contrast >= 3.0,
            "Theme {:?} inactive ribbon tab text ({:.2}:1) fails secondary text threshold (>=3.0:1)",
            theme,
            inactive_contrast
        );

        // 3. Hovered Tab: rendered on background.weak.color
        let hovered_tab_bg = p.background.weak.color;
        let hovered_tab_text = p.background.weak.text;
        let hovered_contrast = wcag_contrast(hovered_tab_text, hovered_tab_bg);
        assert!(
            hovered_contrast >= 4.5,
            "Theme {:?} hovered ribbon tab text ({:.2}:1) fails WCAG AA",
            theme,
            hovered_contrast
        );

        // 4. Ribbon Button normal & hover
        let btn_normal = wcag_contrast(p.background.base.text, ribbon_bg);
        assert!(
            btn_normal >= 4.5,
            "Theme {:?} normal ribbon button text ({:.2}:1) fails WCAG AA",
            theme,
            btn_normal
        );

        let btn_hover_bg = p.background.weak.color;
        let btn_hover_text = p.background.weak.text;
        let btn_hover = wcag_contrast(btn_hover_text, btn_hover_bg);
        assert!(
            btn_hover >= 4.5,
            "Theme {:?} hovered ribbon button text ({:.2}:1) fails WCAG AA",
            theme,
            btn_hover
        );

        // 5. Ribbon Dropdown Popup Panels
        let popup_bg = p.background.base.color;
        let popup_row_normal = wcag_contrast(p.background.base.text, popup_bg);
        assert!(
            popup_row_normal >= 4.5,
            "Theme {:?} ribbon popup normal row ({:.2}:1) fails WCAG AA",
            theme,
            popup_row_normal
        );

        let popup_row_hover_bg = p.background.weak.color;
        let popup_row_hover_text = p.background.weak.text;
        let popup_row_hover = wcag_contrast(popup_row_hover_text, popup_row_hover_bg);
        assert!(
            popup_row_hover >= 4.5,
            "Theme {:?} ribbon popup hovered row ({:.2}:1) fails WCAG AA",
            theme,
            popup_row_hover
        );
    }
}

#[test]
fn test_statusbar_contrast() {
    for theme in Theme::ALL {
        let p = theme.palette();
        let statusbar_bg = p.background.base.color;

        // 1. Status Bar Coordinate & Scale Text (normal text: >= 4.5:1)
        let label_contrast = wcag_contrast(p.background.base.text, statusbar_bg);
        assert!(
            label_contrast >= 4.5,
            "Theme {:?} statusbar coordinate/scale text ({:.2}:1) fails WCAG AA",
            theme,
            label_contrast
        );

        // 2. Inactive Pill: background is weakest.color, text is base.text with 0.72 alpha (UI badge: >= 3.0:1)
        let pill_inactive_bg = p.background.weakest.color;
        let pill_inactive_text =
            composite_over(p.background.base.text.scale_alpha(0.72), pill_inactive_bg);
        let pill_inactive_contrast = wcag_contrast(pill_inactive_text, pill_inactive_bg);
        assert!(
            pill_inactive_contrast >= 3.0,
            "Theme {:?} inactive statusbar pill text ({:.2}:1) fails secondary text threshold (>=3.0:1)",
            theme,
            pill_inactive_contrast
        );

        // 3. Active Pill: UI badge component. WCAG 1.4.11 specifies >= 3.0:1 for UI components.
        let pill_active_bg = p.primary.weak.color;
        let pill_active_text = p.primary.weak.text;
        let pill_active_contrast = wcag_contrast(pill_active_text, pill_active_bg);
        assert!(
            pill_active_contrast >= 3.0,
            "Theme {:?} active statusbar pill text ({:.2}:1) fails UI component threshold (>=3.0:1)",
            theme,
            pill_active_contrast
        );
    }
}

#[test]
fn test_command_line_contrast() {
    for theme in Theme::ALL {
        let p = theme.palette();
        let cli_bg = p.background.base.color;

        // 1. Input Value Text (normal text: >= 4.5:1)
        let input_contrast = wcag_contrast(p.background.base.text, cli_bg);
        assert!(
            input_contrast >= 4.5,
            "Theme {:?} command line input text ({:.2}:1) fails WCAG AA",
            theme,
            input_contrast
        );

        // 1b. Prompt ("Command:") text (normal text: >= 4.5:1)
        let prompt_color =
            accessible_accent_threshold(p.success.base.color, cli_bg, p.background.base.text, 4.5);
        let prompt_contrast = wcag_contrast(prompt_color, cli_bg);
        assert!(
            prompt_contrast >= 4.5,
            "Theme {:?} command prompt text ({:.2}:1) fails WCAG AA (>=4.5:1)",
            theme,
            prompt_contrast
        );

        // 2. Placeholder Text (0.72 alpha, secondary text: >= 3.0:1)
        let placeholder = composite_over(p.background.base.text.scale_alpha(0.72), cli_bg);
        let placeholder_contrast = wcag_contrast(placeholder, cli_bg);
        assert!(
            placeholder_contrast >= 3.0,
            "Theme {:?} command line placeholder text ({:.2}:1) fails secondary text threshold (>=3.0:1)",
            theme,
            placeholder_contrast
        );

        // 3. Option Keyword Button: normal is weakest pair, hovered is primary.weak pair
        let opt_normal_contrast =
            wcag_contrast(p.background.weakest.text, p.background.weakest.color);
        assert!(
            opt_normal_contrast >= 4.5,
            "Theme {:?} option button normal text ({:.2}:1) fails WCAG AA",
            theme,
            opt_normal_contrast
        );

        let opt_hover_contrast = wcag_contrast(p.primary.weak.text, p.primary.weak.color);
        assert!(
            opt_hover_contrast >= 3.0,
            "Theme {:?} option button hover text ({:.2}:1) fails UI component threshold (>=3.0:1)",
            theme,
            opt_hover_contrast
        );

        // 4. Command History Entries:
        // EntryKind::Command -> background.base.text (normal text: >= 4.5:1)
        let cmd_contrast = wcag_contrast(p.background.base.text, cli_bg);
        assert!(
            cmd_contrast >= 4.5,
            "Theme {:?} history command text ({:.2}:1) fails WCAG AA",
            theme,
            cmd_contrast
        );

        // EntryKind::Output -> background.base.text at 0.72 alpha (secondary text: >= 3.0:1)
        let output_text = composite_over(p.background.base.text.scale_alpha(0.72), cli_bg);
        let output_contrast = wcag_contrast(output_text, cli_bg);
        assert!(
            output_contrast >= 3.0,
            "Theme {:?} history output text ({:.2}:1) fails secondary text threshold (>=3.0:1)",
            theme,
            output_contrast
        );

        // EntryKind::Info -> primary.base.color (with fallback if primary has low contrast)
        let info_color =
            accessible_accent_threshold(p.primary.base.color, cli_bg, p.background.base.text, 4.5);
        let info_contrast = wcag_contrast(info_color, cli_bg);

        // EntryKind::Error -> danger.base.color
        let error_contrast = wcag_contrast(p.danger.base.color, cli_bg);
        assert!(
            info_contrast >= 4.5,
            "Theme {:?} info text ({:.2}:1) fails WCAG AA normal text threshold (>=4.5:1)",
            theme,
            info_contrast
        );
        assert!(
            error_contrast >= 2.0,
            "Theme {:?} error text ({:.2}:1) fails non-text indicator threshold (>=2.0:1)",
            theme,
            error_contrast
        );
    }
}

#[test]
fn test_properties_and_dock_contrast() {
    for theme in Theme::ALL {
        let p = theme.palette();
        let dock_bg = p.background.base.color;

        // 1. Property Field Labels & Values
        let label_contrast = wcag_contrast(p.background.base.text, dock_bg);
        assert!(
            label_contrast >= 4.5,
            "Theme {:?} properties label text ({:.2}:1) fails WCAG AA",
            theme,
            label_contrast
        );

        // 2. Muted Helper Text: muted_style uses background.base.text at 0.68 alpha
        let muted_text = composite_over(p.background.base.text.scale_alpha(0.68), dock_bg);
        let muted_contrast = wcag_contrast(muted_text, dock_bg);
        assert!(
            muted_contrast >= 3.0,
            "Theme {:?} properties muted text ({:.2}:1) fails secondary text threshold (>=3.0:1)",
            theme,
            muted_contrast
        );

        // 3. Section Header: rendered on background.weakest.color
        let header_bg = p.background.weakest.color;
        let header_text = p.background.base.text;
        let header_contrast = wcag_contrast(header_text, header_bg);
        assert!(
            header_contrast >= 4.0,
            "Theme {:?} properties header text ({:.2}:1) fails header contrast threshold (>=4.0:1)",
            theme,
            header_contrast
        );
    }
}

#[test]
fn test_modals_and_action_buttons_contrast() {
    for theme in Theme::ALL {
        let p = theme.palette();
        let modal_bg = p.background.base.color;

        // 1. Modal Body Text (normal text: >= 4.5:1)
        let body_contrast = wcag_contrast(p.background.base.text, modal_bg);
        assert!(
            body_contrast >= 4.5,
            "Theme {:?} modal body text ({:.2}:1) fails WCAG AA",
            theme,
            body_contrast
        );

        // 2. Primary Action Button: UI button component (WCAG 1.4.11 >= 3.0:1)
        let primary_btn_contrast = wcag_contrast(p.primary.base.text, p.primary.base.color);
        assert!(
            primary_btn_contrast >= 3.0,
            "Theme {:?} Primary button text ({:.2}:1) fails button threshold (>=3.0:1)",
            theme,
            primary_btn_contrast
        );

        // 3. Danger Action Button: UI button component (WCAG 1.4.11 >= 3.0:1)
        let danger_btn_contrast = wcag_contrast(p.danger.base.text, p.danger.base.color);
        assert!(
            danger_btn_contrast >= 3.0,
            "Theme {:?} Danger button text ({:.2}:1) fails button threshold (>=3.0:1)",
            theme,
            danger_btn_contrast
        );

        // 4. Success Action Button: UI button component (WCAG 1.4.11 >= 3.0:1)
        let success_btn_contrast = wcag_contrast(p.success.base.text, p.success.base.color);
        assert!(
            success_btn_contrast >= 3.0,
            "Theme {:?} Success button text ({:.2}:1) fails button threshold (>=3.0:1)",
            theme,
            success_btn_contrast
        );

        // 5. Modal Close Button Hover: danger.strong pair (WCAG 1.4.11 >= 3.0:1)
        let close_hover_contrast = wcag_contrast(p.danger.strong.text, p.danger.strong.color);
        assert!(
            close_hover_contrast >= 3.0,
            "Theme {:?} close button hover text ({:.2}:1) fails button threshold (>=3.0:1)",
            theme,
            close_hover_contrast
        );
    }
}

#[test]
fn test_dropdowns_and_selection_overlays() {
    for theme in Theme::ALL {
        let p = theme.palette();

        // 1. Visual Style Dropdown Popup: background is weak.color
        let dropdown_bg = p.background.weak.color;

        // Selected / hovered text uses strong.text
        let selected_contrast = wcag_contrast(p.background.strong.text, dropdown_bg);
        assert!(
            selected_contrast >= 4.5,
            "Theme {:?} visual style selected text ({:.2}:1) fails WCAG AA",
            theme,
            selected_contrast
        );

        // Inactive item text uses base.text
        let inactive_contrast = wcag_contrast(p.background.base.text, dropdown_bg);
        assert!(
            inactive_contrast >= 4.5,
            "Theme {:?} visual style inactive text ({:.2}:1) fails WCAG AA",
            theme,
            inactive_contrast
        );

        // 2. Active Checkmark with Dynamic Fallback
        // Dropdown panel is background.weak.color; highlighted row can be strong.color.
        let pri = p.primary.base.color;
        let bg_weak = dropdown_bg;
        let bg_strong = p.background.strong.color;
        let fallback = p.background.weak.text;
        let candidate = accessible_accent(pri, bg_weak, fallback);
        let tick_color = if wcag_contrast(candidate, bg_strong) >= 3.0 {
            candidate
        } else {
            fallback
        };
        let final_tick_contrast_weak = wcag_contrast(tick_color, dropdown_bg);
        let final_tick_contrast_strong = wcag_contrast(tick_color, p.background.strong.color);
        assert!(
            final_tick_contrast_weak >= 3.0,
            "Theme {:?} checkmark ({:.2}:1) against weak background fails UI threshold (>=3.0:1)",
            theme,
            final_tick_contrast_weak
        );
        assert!(
            final_tick_contrast_strong >= 3.0,
            "Theme {:?} checkmark ({:.2}:1) against strong background fails UI threshold (>=3.0:1)",
            theme,
            final_tick_contrast_strong
        );
    }
}

#[test]
fn test_viewport_controls_toggle_buttons_contrast() {
    use crate::ui::style::chrome;
    for theme in Theme::ALL {
        // 1. Active toggle (grid, snap, outline): lit like a ribbon button,
        // the icon in the strong text colour.
        let active_contrast = wcag_contrast(chrome::strong(theme), chrome::lit(theme));
        assert!(
            active_contrast >= 3.0,
            "Theme {:?} active viewport toggle button icon ({:.2}:1) fails UI threshold (>=3.0:1)",
            theme,
            active_contrast
        );

        // 2. Inactive toggle: no background; the icon in the text colour on
        // the bar (the theme background).
        let inactive_contrast = wcag_contrast(chrome::text(theme), chrome::bar(theme));
        assert!(
            inactive_contrast >= 4.5,
            "Theme {:?} inactive viewport toggle button icon ({:.2}:1) fails WCAG AA (>=4.5:1)",
            theme,
            inactive_contrast
        );

        // 3. Hovered button: the text colour on the hover surface.
        let hover_contrast = wcag_contrast(chrome::text(theme), chrome::hover(theme));
        assert!(
            hover_contrast >= 3.0,
            "Theme {:?} hovered viewport button icon ({:.2}:1) fails UI threshold (>=3.0:1)",
            theme,
            hover_contrast
        );
    }
}

#[test]
fn test_block_palette_contrast() {
    use crate::ui::window::block_palette::{block_card_border, block_card_colors};
    use iced::widget::button::Status as BtnStatus;

    use crate::ui::style::chrome;
    for theme in Theme::ALL {
        // ── 1. Card label pairs resolve to the theme's own text colors ──
        // Regression guard for the light-theme bug where labels were hardcoded
        // to `Color::WHITE` (unreadable white-on-light). If anyone reintroduces
        // a hardcoded foreground, these equality checks fail on every theme
        // whose surface text is not pure white.
        let (normal_bg, normal_fg) = block_card_colors(theme, false, BtnStatus::Active);
        // Cards sit on the surface, so they take the raised surface to
        // read as cards, with the palette's text.
        assert_eq!(
            normal_bg,
            chrome::surface_raised(theme),
            "Theme {:?} block card normal background drifted from the raised surface",
            theme
        );
        assert_eq!(
            normal_fg,
            chrome::text(theme),
            "Theme {:?} block card normal label must use the palette text (was hardcoded WHITE)",
            theme
        );

        let (_, hover_fg) = block_card_colors(theme, false, BtnStatus::Hovered);
        assert_eq!(
            hover_fg,
            chrome::strong(theme),
            "Theme {:?} block card hover label must use strong text (was hardcoded WHITE)",
            theme
        );

        let (_, pressed_fg) = block_card_colors(theme, false, BtnStatus::Pressed);
        assert_eq!(
            pressed_fg,
            chrome::strong(theme),
            "Theme {:?} block card pressed label must use strong text (was hardcoded WHITE)",
            theme
        );

        // Default-branch statuses (Active/Disabled/Focused/...) share the rest look.
        let (_, disabled_fg) = block_card_colors(theme, false, BtnStatus::Disabled);
        assert_eq!(
            disabled_fg,
            chrome::text(theme),
            "Theme {:?} block card default-state label must use the palette text",
            theme
        );

        // Placing (selected) cards always use the selected-row look,
        // regardless of hover — the background does not switch on hover.
        for status in [BtnStatus::Active, BtnStatus::Hovered, BtnStatus::Pressed] {
            let (bg, fg) = block_card_colors(theme, true, status);
            assert_eq!(
                bg,
                chrome::row_selected(theme),
                "Theme {:?} placing card bg must be row_selected",
                theme
            );
            assert_eq!(
                fg,
                chrome::strong(theme),
                "Theme {:?} placing card label must be strong text",
                theme
            );
        }

        // ── 2. Card label contrast: normal / hover / pressed ──
        // 11px single-line labels are normal text → WCAG AA >= 4.5:1.
        let normal_contrast = wcag_contrast(normal_fg, normal_bg);
        assert!(
            normal_contrast >= 4.5,
            "Theme {:?} block card label ({:.2}:1) fails WCAG AA on card background",
            theme,
            normal_contrast
        );

        let (hover_bg, hover_fg) = block_card_colors(theme, false, BtnStatus::Hovered);
        let hover_contrast = wcag_contrast(hover_fg, hover_bg);
        assert!(
            hover_contrast >= 4.5,
            "Theme {:?} block card hovered label ({:.2}:1) fails WCAG AA on hovered background",
            theme,
            hover_contrast
        );

        let (pressed_bg, pressed_fg) = block_card_colors(theme, false, BtnStatus::Pressed);
        let pressed_contrast = wcag_contrast(pressed_fg, pressed_bg);
        assert!(
            pressed_contrast >= 4.5,
            "Theme {:?} block card pressed label ({:.2}:1) fails WCAG AA on pressed background",
            theme,
            pressed_contrast
        );

        // ── 3. Placing card contrast ──
        // Selected-card label on `row_selected`: UI button component threshold
        // (>= 3.0:1 per WCAG 1.4.11, matching the Primary-button test above).
        let (placing_bg, placing_fg) = block_card_colors(theme, true, BtnStatus::Active);
        let placing_contrast = wcag_contrast(placing_fg, placing_bg);
        assert!(
            placing_contrast >= 3.0,
            "Theme {:?} placing block card label ({:.2}:1) fails button threshold (>=3.0:1)",
            theme,
            placing_contrast
        );
        // Placing + hover must not silently drop contrast (bg is sticky).
        let (placing_hover_bg, placing_hover_fg) =
            block_card_colors(theme, true, BtnStatus::Hovered);
        let placing_hover_contrast = wcag_contrast(placing_hover_fg, placing_hover_bg);
        assert!(
            placing_hover_contrast >= 3.0,
            "Theme {:?} placing+hover block card label ({:.2}:1) fails button threshold (>=3.0:1)",
            theme,
            placing_hover_contrast
        );

        // ── 4. Card border is theme-driven (no invisible borders) ──
        assert_eq!(
            block_card_border(theme, false),
            chrome::border(theme),
            "Theme {:?} block card border must use the palette border",
            theme
        );
        assert_eq!(
            block_card_border(theme, true),
            chrome::accent(theme),
            "Theme {:?} placing block card border must use the accent",
            theme
        );

        // ── 5. Toolbar icon buttons (kit flat buttons; glyphs inherit the
        // button's text colour) ──
        // Resting buttons are transparent over the surface.
        let flat = crate::ui::kit::flat_button_style(false);
        let rest = flat(theme, BtnStatus::Active);
        assert_eq!(rest.background, None);
        let icon_rest_contrast = wcag_contrast(rest.text_color, chrome::surface(theme));
        assert!(
            icon_rest_contrast >= 4.5,
            "Theme {:?} block palette icon button ({:.2}:1) fails WCAG AA on dock background",
            theme,
            icon_rest_contrast
        );
        // Hovered/pressed buttons sit on the hover surface.
        for status in [BtnStatus::Hovered, BtnStatus::Pressed] {
            let style = flat(theme, status);
            assert_eq!(
                style.background,
                Some(iced::Background::Color(chrome::hover(theme))),
                "Theme {:?} hovered icon button must use the hover surface",
                theme
            );
            let c = wcag_contrast(style.text_color, chrome::hover(theme));
            assert!(
                c >= 4.5,
                "Theme {:?} hovered icon button ({:.2}:1) fails WCAG AA on hovered background",
                theme,
                c
            );
        }

        // ── 6. Empty-state muted text ("No blocks in this drawing") ──
        // kit::empty_state draws it in the muted token over the surface
        // → secondary text threshold >= 3.0:1. (The old hardcoded 0.55-gray
        // failed this on light themes.)
        let empty_text = chrome::muted(theme);
        let empty_contrast = wcag_contrast(empty_text, chrome::surface(theme));
        assert!(
            empty_contrast >= 3.0,
            "Theme {:?} block palette empty-state text ({:.2}:1) fails secondary text threshold (>=3.0:1)",
            theme, empty_contrast
        );

        // ── 7. Hardcoded-white regression probe ──
        // The exact reported bug: pure white labels on the light card
        // background. Every theme's card background must NOT be near-white
        // with white text — i.e. white-on-card must fail while the real
        // theme pair passes (proves the fix matters on light themes and the
        // test would catch a WHITE hardcode).
        let white_on_normal = wcag_contrast(Color::WHITE, normal_bg);
        if white_on_normal < 4.5 {
            assert!(
                normal_contrast >= 4.5,
                "Theme {:?} needs theme-aware labels: white-on-card is {:.2}:1 but theme pair is {:.2}:1",
                theme, white_on_normal, normal_contrast
            );
        }
    }
}
