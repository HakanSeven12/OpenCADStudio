use crate::app::settings::IsoPlane;
use crate::app::Message;
use crate::snap::{SnapType, ALL_3D_SNAP_MODES, ALL_SNAP_MODES};
use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Border, Element, Fill, Theme};
use std::borrow::Cow;

/// Active tab in the Drafting Settings dialog.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum DraftingSettingsTab {
    #[default]
    SnapAndGrid,
    PolarTracking,
    ObjectSnap,
    ObjectSnap3D,
    DynamicInput,
    QuickProperties,
    SelectionCycling,
}

/// Working buffer of drafting settings edited in the dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftingSettingsState {
    pub active_tab: DraftingSettingsTab,
    // Snap and Grid
    pub snap_on: bool,
    pub grid_on: bool,
    pub snap_x_input: String,
    pub snap_y_input: String,
    pub snap_equal: bool,
    pub grid_x_input: String,
    pub grid_y_input: String,
    pub grid_major_input: String,
    pub grid_adaptive: bool,
    pub grid_beyond_limits: bool,
    pub isometric: bool,
    pub iso_plane: IsoPlane,
    pub snap_angle_deg: f32,
    // Polar Tracking
    pub polar_on: bool,
    pub ortho_on: bool,
    pub polar_increment_deg: f32,
    // Object Snap
    pub osnap_on: bool,
    pub otrack_on: bool,
    pub snap_modes: rustc_hash::FxHashSet<SnapType>,
    // 3D Object Snap
    pub osnap3d_on: bool,
    pub snap3d_modes: rustc_hash::FxHashSet<SnapType>,
    // Dynamic Input
    pub dyn_input_on: bool,
    // Quick Properties
    pub quick_props_on: bool,
    // Selection Cycling
    pub selection_cycling_on: bool,
}

impl DraftingSettingsState {
    /// Check whether settings were modified compared to the saved snapshot.
    /// Excludes `active_tab` so tab navigation does not mark the dialog dirty.
    pub fn is_dirty(&self, saved: &Self) -> bool {
        self.snap_on != saved.snap_on
            || self.grid_on != saved.grid_on
            || self.snap_x_input != saved.snap_x_input
            || self.snap_y_input != saved.snap_y_input
            || self.snap_equal != saved.snap_equal
            || self.grid_x_input != saved.grid_x_input
            || self.grid_y_input != saved.grid_y_input
            || self.grid_major_input != saved.grid_major_input
            || self.grid_adaptive != saved.grid_adaptive
            || self.grid_beyond_limits != saved.grid_beyond_limits
            || self.isometric != saved.isometric
            || self.iso_plane != saved.iso_plane
            || (self.snap_angle_deg - saved.snap_angle_deg).abs() > 0.001
            || self.polar_on != saved.polar_on
            || self.ortho_on != saved.ortho_on
            || (self.polar_increment_deg - saved.polar_increment_deg).abs() > 0.001
            || self.osnap_on != saved.osnap_on
            || self.otrack_on != saved.otrack_on
            || self.snap_modes != saved.snap_modes
            || self.osnap3d_on != saved.osnap3d_on
            || self.snap3d_modes != saved.snap3d_modes
            || self.dyn_input_on != saved.dyn_input_on
            || self.quick_props_on != saved.quick_props_on
            || self.selection_cycling_on != saved.selection_cycling_on
    }
}

/// Format a snap spacing value for the dialog's text inputs.
pub fn format_snap_spacing(v: f32) -> String {
    if (v - v.round()).abs() < 1e-4 {
        format!("{}", v.round() as i32)
    } else {
        format!("{v}")
    }
}

/// Parse a snap spacing input. Spacings must be positive and finite.
pub fn parse_snap_spacing(s: &str) -> Option<f32> {
    let v: f32 = s.trim().parse().ok()?;
    if v.is_finite() && v > 0.0 && v <= 1e9 {
        Some(v)
    } else {
        None
    }
}

/// Parse a major-line interval. Must be an integer in 2..=100.
pub fn parse_grid_major(s: &str) -> Option<u32> {
    let v: u32 = s.trim().parse().ok()?;
    (2..=100).contains(&v).then_some(v)
}

/// Helper for grouped sub-panels with a light border and header title.
fn group<'a>(title: impl Into<String>, body: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    container(
        column![
            text(title.into()).size(11).style(|theme: &Theme| text::Style {
                color: Some(theme.palette().background.strongest.text.scale_alpha(0.75)),
            }),
            body.into(),
        ]
        .spacing(6),
    )
    .padding([8, 10])
    .width(Fill)
    .style(|theme: &Theme| container::Style {
        border: Border {
            width: 1.0,
            radius: 4.0.into(),
            color: theme.palette().background.strong.color,
        },
        ..Default::default()
    })
    .into()
}

pub fn view_window<'a>(
    state: &'a DraftingSettingsState,
    dirty: bool,
    close_confirm: bool,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'a, Message> {
    let toggle = |value: bool, label: Cow<'a, str>, message: Message| {
        row![
            checkbox(value).on_toggle(move |_| message.clone()).size(15),
            text(label).size(12),
        ]
        .spacing(7)
        .align_y(iced::Center)
    };

    // ── Tab Bar ──────────────────────────────────────────────────────────
    // Real tab strip: a single non-wrapping row of tab-shaped buttons with a
    // divider underneath, matching the style-manager editor shell. The row
    // never wraps — it clips on overflow so "Selection Cycling" can't spill
    // onto a second row.
    let tab_button = |tab: DraftingSettingsTab, label: Cow<'a, str>| {
        let is_active = state.active_tab == tab;
        button(text(label).size(11))
            .on_press(Message::DraftingSettingsTabChanged(tab))
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                let pair = match (is_active, status) {
                    (true, _) => palette.primary.strong,
                    (
                        false,
                        button::Status::Hovered | button::Status::Pressed,
                    ) => palette.background.strong,
                    _ => palette.background.weak,
                };
                button::Style {
                    background: Some(Background::Color(pair.color)),
                    text_color: pair.text,
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: iced::border::Radius {
                            top_left: 4.0,
                            top_right: 4.0,
                            bottom_right: 0.0,
                            bottom_left: 0.0,
                        },
                    },
                    ..Default::default()
                }
            })
            .padding([4, 8])
    };

    let tab_row = row![
        tab_button(DraftingSettingsTab::SnapAndGrid, crate::t!("Snap and Grid")),
        tab_button(DraftingSettingsTab::PolarTracking, crate::t!("Polar Tracking")),
        tab_button(DraftingSettingsTab::ObjectSnap, crate::t!("Object Snap")),
        tab_button(DraftingSettingsTab::ObjectSnap3D, crate::t!("3D Object Snap")),
        tab_button(DraftingSettingsTab::DynamicInput, crate::t!("Dynamic Input")),
        tab_button(DraftingSettingsTab::QuickProperties, crate::t!("Quick Properties")),
        tab_button(DraftingSettingsTab::SelectionCycling, crate::t!("Selection Cycling")),
    ]
    .spacing(2)
    .width(Fill)
    .clip(true);

    let tab_divider = container(Space::new().width(Fill).height(1))
        .width(Fill)
        .height(1)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.neutral.color,
            )),
            ..Default::default()
        });

    let tab_bar = column![tab_row, tab_divider].spacing(0).width(Fill);

    // ── Tab 1: Snap and Grid ─────────────────────────────────────────────
    let snap_and_grid_view = {
        // Left Column: Snap settings
        let snap_on_toggle = toggle(
            state.snap_on,
            crate::t!("Snap On (F9)"),
            Message::DraftingSettingsToggleSnap,
        );

        let snap_spacing_group = group(
            crate::t!("Snap spacing"),
            column![
                row![
                    text(crate::t!("Snap X spacing:")).size(11).width(120),
                    text_input("10", &state.snap_x_input)
                        .on_input(Message::DraftingSettingsSnapXChanged)
                        .size(11)
                        .padding([4, 7])
                        .width(100),
                ]
                .spacing(8)
                .align_y(iced::Center),
                row![
                    text(crate::t!("Snap Y spacing:")).size(11).width(120),
                    text_input("10", &state.snap_y_input)
                        .on_input(Message::DraftingSettingsSnapYChanged)
                        .size(11)
                        .padding([4, 7])
                        .width(100),
                ]
                .spacing(8)
                .align_y(iced::Center),
                row![
                    checkbox(state.snap_equal)
                        .on_toggle(|_| Message::DraftingSettingsToggleEqualSnap)
                        .size(14),
                    text(crate::t!("Equal X and Y spacing")).size(11),
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(6),
        );

        let mut iso_planes = row![].spacing(4);
        for plane in IsoPlane::ALL {
            iso_planes = iso_planes.push(
                button(text(crate::t!(plane.label())).size(11))
                    .on_press(Message::DraftingSettingsSetIsoPlane(plane))
                    .style(if state.isometric && state.iso_plane == plane {
                        button::primary
                    } else {
                        button::secondary
                    })
                    .padding([4, 10]),
            );
        }

        let snap_type_group = group(
            crate::t!("Snap type"),
            column![
                toggle(
                    state.isometric,
                    crate::t!("Enable isometric drafting"),
                    Message::DraftingSettingsToggleIsometric,
                ),
                iso_planes,
                text(crate::t!("F5 cycles Left, Top, and Right.")).size(10.5),
                Space::new().height(4),
                row![
                    text(crate::t!("Rotation: %{angle}°", angle = state.snap_angle_deg)).size(11),
                    button(text(crate::t!("Reset rotation")).size(10))
                        .on_press(Message::DraftingSettingsResetRotation)
                        .style(button::secondary)
                        .padding([3, 8]),
                ]
                .spacing(8)
                .align_y(iced::Center),
            ]
            .spacing(6),
        );

        let left_col = column![snap_on_toggle, snap_spacing_group, snap_type_group]
            .spacing(10)
            .width(Fill);

        // Right Column: Grid settings
        let grid_on_toggle = toggle(
            state.grid_on,
            crate::t!("Grid On (F7)"),
            Message::DraftingSettingsToggleGrid,
        );

        let grid_spacing_group = group(
            crate::t!("Grid spacing"),
            column![
                row![
                    text(crate::t!("Grid X spacing:")).size(11).width(120),
                    text_input("10", &state.grid_x_input)
                        .on_input(Message::DraftingSettingsGridXChanged)
                        .size(11)
                        .padding([4, 7])
                        .width(100),
                ]
                .spacing(8)
                .align_y(iced::Center),
                row![
                    text(crate::t!("Grid Y spacing:")).size(11).width(120),
                    text_input("10", &state.grid_y_input)
                        .on_input(Message::DraftingSettingsGridYChanged)
                        .size(11)
                        .padding([4, 7])
                        .width(100),
                ]
                .spacing(8)
                .align_y(iced::Center),
                row![
                    text(crate::t!("Major line every:")).size(11).width(120),
                    text_input("5", &state.grid_major_input)
                        .on_input(Message::DraftingSettingsGridMajorChanged)
                        .size(11)
                        .padding([4, 7])
                        .width(100),
                ]
                .spacing(8)
                .align_y(iced::Center),
            ]
            .spacing(6),
        );

        let grid_behavior_group = group(
            crate::t!("Grid behavior"),
            column![
                row![
                    checkbox(state.grid_adaptive)
                        .on_toggle(|_| Message::DraftingSettingsToggleAdaptiveGrid)
                        .size(14),
                    text(crate::t!("Adaptive grid")).size(11),
                ]
                .spacing(6)
                .align_y(iced::Center),
                row![
                    checkbox(state.grid_beyond_limits)
                        .on_toggle(|_| Message::DraftingSettingsToggleBeyondLimits)
                        .size(14),
                    text(crate::t!("Display grid beyond Limits")).size(11),
                ]
                .spacing(6)
                .align_y(iced::Center),
            ]
            .spacing(6),
        );

        let right_col = column![grid_on_toggle, grid_spacing_group, grid_behavior_group]
            .spacing(10)
            .width(Fill);

        row![left_col, right_col].spacing(16).width(Fill)
    };

    // ── Tab 2: Polar Tracking ────────────────────────────────────────────
    let polar_tracking_view = {
        let polar_on_toggle = toggle(
            state.polar_on,
            crate::t!("Polar Tracking On (F10)"),
            Message::DraftingSettingsTogglePolar,
        );

        let ortho_on_toggle = toggle(
            state.ortho_on,
            crate::t!("Ortho mode (F8)"),
            Message::DraftingSettingsToggleOrtho,
        );

        let polar_angle_group = group(
            crate::t!("Polar Angle Settings"),
            column![
                row![
                    text(crate::t!("Increment angle:")).size(11).width(130),
                    text(format!("{:.1}°", state.polar_increment_deg)).size(11),
                ]
                .spacing(8)
                .align_y(iced::Center),
                text(crate::t!(
                    "Polar Tracking guides cursor movement along specified angles. Ortho mode constrains movement to orthogonal axes."
                ))
                .size(10.5)
                .style(|theme: &Theme| text::Style {
                    color: Some(theme.palette().background.strongest.text.scale_alpha(0.7)),
                }),
            ]
            .spacing(6),
        );

        column![
            polar_on_toggle,
            ortho_on_toggle,
            Space::new().height(4),
            polar_angle_group,
        ]
        .spacing(10)
        .width(Fill)
    };

    // ── Tab 3: Object Snap ───────────────────────────────────────────────
    let object_snap_view = {
        let osnap_on_toggle = toggle(
            state.osnap_on,
            crate::t!("Object Snap On (F3)"),
            Message::DraftingSettingsToggleOsnap,
        );

        let otrack_on_toggle = toggle(
            state.otrack_on,
            crate::t!("Object Snap Tracking On (F11)"),
            Message::DraftingSettingsToggleOtrack,
        );

        let action_buttons = row![
            button(text(crate::t!("Select All")).size(10.5))
                .on_press(Message::DraftingSettingsSnapSelectAll)
                .style(button::secondary)
                .padding([4, 10]),
            button(text(crate::t!("Clear All")).size(10.5))
                .on_press(Message::DraftingSettingsSnapClearAll)
                .style(button::secondary)
                .padding([4, 10]),
        ]
        .spacing(8);

        // Two columns of snap modes:
        let total = ALL_SNAP_MODES.len();
        let mid = (total + 1) / 2;
        let (first_half, second_half) = ALL_SNAP_MODES.split_at(mid);

        let mut col1 = column![].spacing(6).width(Fill);
        for &(snap_type, _, label) in first_half {
            let is_checked = state.snap_modes.contains(&snap_type);
            col1 = col1.push(
                row![
                    checkbox(is_checked)
                        .on_toggle(move |_| Message::DraftingSettingsToggleSnapMode(snap_type))
                        .size(14),
                    text(crate::t!(label)).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
            );
        }

        let mut col2 = column![].spacing(6).width(Fill);
        for &(snap_type, _, label) in second_half {
            let is_checked = state.snap_modes.contains(&snap_type);
            col2 = col2.push(
                row![
                    checkbox(is_checked)
                        .on_toggle(move |_| Message::DraftingSettingsToggleSnapMode(snap_type))
                        .size(14),
                    text(crate::t!(label)).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
            );
        }

        let snap_modes_group = group(
            crate::t!("Object Snap modes"),
            row![col1, col2].spacing(20).width(Fill),
        );

        column![
            row![osnap_on_toggle, Space::new().width(24), otrack_on_toggle]
                .align_y(iced::Center),
            action_buttons,
            Space::new().height(4),
            snap_modes_group,
        ]
        .spacing(10)
        .width(Fill)
    };

    // ── Tab 4: 3D Object Snap ────────────────────────────────────────────
    let object_snap_3d_view = {
        let osnap3d_toggle = toggle(
            state.osnap3d_on,
            crate::t!("3D Object Snap On (F4)"),
            Message::DraftingSettingsToggle3dOsnap,
        );

        // Implemented 3D modes follow the same checkbox pattern as the
        // Object Snap tab, driven by ALL_3D_SNAP_MODES.
        let mut modes_col = column![].spacing(6);
        for &(snap_type, _, label) in ALL_3D_SNAP_MODES {
            let is_checked = state.snap3d_modes.contains(&snap_type);
            modes_col = modes_col.push(
                row![
                    checkbox(is_checked)
                        .on_toggle(move |_| Message::DraftingSettingsToggleSnapMode3d(snap_type))
                        .size(14),
                    text(crate::t!(label)).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
            );
        }

        let modes_group = group(crate::t!("3D Object Snap modes"), modes_col);

        column![osnap3d_toggle, Space::new().height(4), modes_group]
            .spacing(10)
            .width(Fill)
    };

    // ── Tab 5: Dynamic Input ─────────────────────────────────────────────
    let dynamic_input_view = {
        let dyn_input_toggle = toggle(
            state.dyn_input_on,
            crate::t!("Enable Pointer Input (F12)"),
            Message::DraftingSettingsToggleDynInput,
        );

        let pointer_group = group(
            crate::t!("Pointer Input"),
            column![
                row![
                    checkbox(true).size(14),
                    text(crate::t!("Display coordinate input near crosshairs")).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
                row![
                    checkbox(true).size(14),
                    text(crate::t!("Enable dimension input fields")).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
            ]
            .spacing(6),
        );

        let prompt_group = group(
            crate::t!("Dynamic Prompts"),
            column![
                row![
                    checkbox(true).size(14),
                    text(crate::t!("Show command prompting and command input near crosshairs"))
                        .size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
            ]
            .spacing(6),
        );

        column![
            dyn_input_toggle,
            Space::new().height(4),
            pointer_group,
            prompt_group,
        ]
        .spacing(10)
        .width(Fill)
    };

    // ── Tab 6: Quick Properties ──────────────────────────────────────────
    let quick_properties_view = {
        let quick_props_toggle = toggle(
            state.quick_props_on,
            crate::t!("Display Quick Properties palette on selection"),
            Message::DraftingSettingsToggleQuickProps,
        );

        let palette_group = group(
            crate::t!("Palette Location"),
            column![
                row![
                    checkbox(true).size(14),
                    text(crate::t!("Cursor-dependent position")).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
                row![
                    checkbox(false).size(14),
                    text(crate::t!("Static quadrant location")).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
            ]
            .spacing(6),
        );

        column![
            quick_props_toggle,
            Space::new().height(4),
            palette_group,
        ]
        .spacing(10)
        .width(Fill)
    };

    // ── Tab 7: Selection Cycling ─────────────────────────────────────────
    let selection_cycling_view = {
        let sel_cycling_toggle = toggle(
            state.selection_cycling_on,
            crate::t!("Allow selection cycling"),
            Message::DraftingSettingsToggleSelCycling,
        );

        let cycling_group = group(
            crate::t!("Display Selection Cycling List Box"),
            column![
                row![
                    checkbox(true).size(14),
                    text(crate::t!("Show cycling badge when objects overlap")).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
                row![
                    checkbox(true).size(14),
                    text(crate::t!("Display selection candidate list box on click")).size(11),
                ]
                .spacing(7)
                .align_y(iced::Center),
            ]
            .spacing(6),
        );

        column![
            sel_cycling_toggle,
            Space::new().height(4),
            cycling_group,
        ]
        .spacing(10)
        .width(Fill)
    };

    // ── Active Tab Content ───────────────────────────────────────────────
    let tab_content: Element<'a, Message> = match state.active_tab {
        DraftingSettingsTab::SnapAndGrid => snap_and_grid_view.into(),
        DraftingSettingsTab::PolarTracking => polar_tracking_view.into(),
        DraftingSettingsTab::ObjectSnap => object_snap_view.into(),
        DraftingSettingsTab::ObjectSnap3D => object_snap_3d_view.into(),
        DraftingSettingsTab::DynamicInput => dynamic_input_view.into(),
        DraftingSettingsTab::QuickProperties => quick_properties_view.into(),
        DraftingSettingsTab::SelectionCycling => selection_cycling_view.into(),
    };

    // ── Bottom Action Buttons ────────────────────────────────────────────
    let ok_button = button(text(crate::t!("OK")).size(12))
        .on_press(Message::DraftingSettingsOk)
        .style(button::primary)
        .padding([6, 18]);

    let apply_button = button(text(crate::t!("Apply")).size(12))
        .on_press_maybe(if dirty {
            Some(Message::DraftingSettingsApply)
        } else {
            None
        })
        .style(if dirty {
            button::secondary
        } else {
            button::text
        })
        .padding([6, 18]);

    let close_button = button(text(crate::t!("Close")).size(12))
        .on_press(Message::DraftingSettingsClose)
        .style(button::secondary)
        .padding([6, 18]);

    let action_row = row![
        Space::new().width(Fill),
        ok_button,
        apply_button,
        close_button,
    ]
    .spacing(8)
    .align_y(iced::Center);

    // ── Body Layout ──────────────────────────────────────────────────────
    let body = column![
        tab_bar,
        Space::new().height(10),
        scrollable(tab_content).spacing(6).height(Fill),
        Space::new().height(12),
        action_row,
    ]
    .width(sizing.width)
    .height(sizing.height);

    let main_content = container(body)
        .style(container::rounded_box)
        .padding([14, 16])
        .width(sizing.width)
        .height(sizing.height);

    // ── Unsaved Changes Guard ────────────────────────────────────────────
    if !close_confirm {
        return main_content.into();
    }
    crate::ui::modal::discard_guard(
        main_content,
        Message::DraftingSettingsCloseDiscard,
        Message::DraftingSettingsCloseKeep,
    )
}

