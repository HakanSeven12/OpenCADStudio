use super::*;
use super::super::document::{DynComponent, DynFieldEntry};
use super::super::Message;
use iced::widget::{
    button, container, mouse_area, row,
};
use iced::{Background, Border, Element, Theme};

pub(super) fn viewport_controls<'a>(
    render_mode: acadrust::entities::ViewportRenderMode,
    show_grid: bool,
    snap_on: bool,
    include_split: bool,
    tile_count: usize,
) -> Element<'a, Message> {
    use acadrust::entities::ViewportRenderMode as M;
    let render_modes: Vec<RenderModeChoice> = vec![
        RenderModeChoice(M::Wireframe2D),
        RenderModeChoice(M::Wireframe3D),
        RenderModeChoice(M::HiddenLine),
        RenderModeChoice(M::FlatShaded),
        RenderModeChoice(M::GouraudShaded),
        RenderModeChoice(M::FlatShadedWithEdges),
        RenderModeChoice(M::GouraudShadedWithEdges),
    ];
    let danger_btn = move |bytes: &'static [u8], msg: Message| {
        button(crate::ui::icons::themed_danger(bytes, 15.0))
            .on_press(msg)
            .padding([4, 6])
            .style(move |theme: &Theme, status| iced::widget::button::Style {
                background: matches!(
                    status,
                    iced::widget::button::Status::Hovered
                        | iced::widget::button::Status::Pressed
                )
                .then_some(Background::Color(
                    theme.extended_palette().danger.weak.color
                )),
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                text_color: theme.extended_palette().danger.base.color,
                ..Default::default()
            })
    };

    // Borderless icon button; an `active` toggle gets an accent tint + fill.
    let icon_btn = move |bytes: &'static [u8], active: bool, msg: Message| {
        let icon = if active {
            crate::ui::icons::themed_primary(bytes, 15.0)
        } else {
            crate::ui::icons::themed(bytes, 15.0)
        };
        button(icon)
            .on_press(msg)
            .padding([4, 6])
            .style(move |theme: &Theme, status| {
                let palette = theme.extended_palette();
                let pair = match (active, status) {
                    (_, iced::widget::button::Status::Hovered) => {
                        Some(palette.background.strong)
                    }
                    (true, _) => Some(palette.primary.weak),
                    (false, _) => None,
                };
                iced::widget::button::Style {
                background: pair.map(|p| Background::Color(p.color)),
                border: Border {
                    radius: 3.0.into(),
                    ..Default::default()
                },
                text_color: pair
                    .map(|p| p.text)
                    .unwrap_or(palette.background.base.text),
                ..Default::default()
                }
            })
    };

    // Render-mode picker, restyled borderless so the outer chip frames it.
    let picker = iced::widget::pick_list(
        render_modes,
        Some(RenderModeChoice(render_mode)),
        |c| Message::SetRenderMode(c.0),
    )
    .text_size(11)
    .padding([4, 6])
    .style(move |theme: &Theme, _| {
        let text = theme.extended_palette().background.base.text;
        iced::widget::pick_list::Style {
        background: Background::Color(iced::Color::TRANSPARENT),
        border: Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        text_color: text,
        placeholder_color: text.scale_alpha(0.68),
        handle_color: text,
        }
    });

    // Thin vertical divider between control groups.
    let sep = || {
        container(iced::widget::Space::new().width(1.0).height(16.0)).style(|theme: &Theme| {
            iced::widget::container::Style {
                background: Some(Background::Color(
                    theme.extended_palette().background.neutral.color.scale_alpha(0.7)
                )),
                ..Default::default()
            }
        })
    };

    let mut bar = row![]
        .spacing(3)
        .align_y(iced::alignment::Vertical::Center);
    bar = bar
        .push(icon_btn(crate::ui::icons::GRID, show_grid, Message::ToggleGrid))
        .push(sep())
        .push(icon_btn(crate::ui::icons::SNAP, snap_on, Message::ToggleGridSnap))
        .push(sep())
        .push(picker);
    if include_split {
        bar = bar
            .push(sep())
            .push(icon_btn(crate::ui::icons::SPLIT_V, false, Message::SplitModelViewport(false)))
            .push(sep())
            .push(icon_btn(crate::ui::icons::SPLIT_H, false, Message::SplitModelViewport(true)));
        // Drag handle + close: only meaningful with more than one model tile.
        // The handle is a `mouse_area` (not a button) so it fires on press-DOWN,
        // letting the drag continue onto the target pane to swap them (a button
        // would only fire on release). Placed just left of Close.
        if tile_count > 1 {
            let drag = mouse_area(
                container(crate::ui::icons::themed_success(crate::ui::icons::MOVE, 15.0))
                    .padding([4, 6])
                    .style(|_: &Theme| iced::widget::container::Style {
                        border: Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
            )
            .interaction(iced::mouse::Interaction::Grab)
            .on_press(Message::PaneMoveStart);
            bar = bar
                .push(sep())
                .push(drag)
                .push(sep())
                .push(danger_btn(crate::ui::icons::CLOSE, Message::CloseModelViewport));
        }
    }

    container(bar)
        .padding(2)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            iced::widget::container::Style {
            background: Some(Background::Color(
                palette.background.weak.color.scale_alpha(0.92)
            )),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
            }
        })
        .into()
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
) -> String {
    if let Some(b) = &f.buffer {
        return b.clone();
    }
    let b = base.unwrap_or(glam::DVec3::ZERO);
    // Relative deltas and the polar angle read in the active UCS plane. The
    // delta is offset-invariant, so only the axis rotation matters (identity
    // xf reproduces the world-frame deltas).
    let d = xf.vec_to_ucs(w - b);
    let dx = d.x as f64;
    let dy = d.y as f64;
    // When a base point exists (DYN-on after the first pick) the cartesian
    // fields show relative deltas — matching the typed-value convention
    // in `dyn_resolve_point` so the live preview and the committed
    // coordinate use the same frame. See #35.
    let has_base = base.is_some();
    // Width / Height read as unsigned magnitudes (the sign is taken from the
    // cursor side on commit), matching the rectangle's two-edge entry. But once
    // the user separates the values with `,` the entry is a cartesian
    // coordinate pair, so the fields read as signed X/Y deltas to match the
    // committed point (#269).
    let wh = matches!(f.role, crate::command::DynRole::Width | crate::command::DynRole::Height)
        && !comma_cartesian;
    match f.component {
        DynComponent::X if has_base => format!("{:.4}", if wh { dx.abs() } else { dx }),
        DynComponent::Y if has_base => format!("{:.4}", if wh { dy.abs() } else { dy }),
        DynComponent::Z if has_base => "0.0000".to_string(),
        DynComponent::X => format!("{:.4}", w.x),
        DynComponent::Y => format!("{:.4}", w.y),
        DynComponent::Z => format!("{:.4}", b.z),
        // Scaled by the role so a diameter box reads twice the radius.
        DynComponent::Distance => {
            format!("{:.4}", (dx * dx + dy * dy).sqrt() * f.role.value_scale() as f64)
        }
        // Shared rule: unsigned magnitude of the short angle, so CW (below the
        // reference axis) reads positive (e.g. 30°, not -30°/330°). The
        // committed value stays signed (see dyn_resolve_point).
        DynComponent::Angle => {
            format!("{:.1}", crate::command::dyn_display_angle_deg(dy.atan2(dx) as f32))
        }
        // Typed-only scalar — no geometric value to track when empty.
        DynComponent::Scalar => String::new(),
    }
}
