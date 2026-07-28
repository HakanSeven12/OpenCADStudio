use super::super::Message;
use crate::scene::{VIEWCUBE_PX, VIEWCUBE_REGION_PX};
use iced::widget::{
    button, container, mouse_area, pick_list, stack, Space,
};
use iced::{Background, Border, Element, Theme};

// ── Render-mode picker ──────────────────────────────────────────────────────

/// Top-left viewport control bar: a single dark chip holding (optionally) the
/// horizontal/vertical split buttons, the render-mode picker, and the grid /
/// grid-snap toggles. `include_split` is off for paper-space viewports, which
/// have no model-tile splitting. Grid / snap reflect the active viewport's
/// state and emit `ToggleGrid` / `ToggleGridSnap`.
// ── ViewCube navigation controls (home / roll / nudge / UCS) ───────────────

/// Place `el` at pixel offset (x, y) inside a fill layer (top-left origin).
fn vc_place<'a>(x: f32, y: f32, el: Element<'a, Message>) -> Element<'a, Message> {
    crate::ui::pin_at(iced::Point::new(x, y), el)
}

/// Borderless square icon button used by the ViewCube nav controls.
fn vc_btn<'a>(content: Element<'a, Message>, size: f32, msg: Message) -> Element<'a, Message> {
    button(
        container(content)
            .width(iced::Length::Fixed(size))
            .height(iced::Length::Fixed(size))
            .center_x(iced::Length::Fixed(size))
            .center_y(iced::Length::Fixed(size)),
    )
    .padding(0)
    .on_press(msg)
    .style(|theme: &Theme, status| iced::widget::button::Style {
        background: matches!(
            status,
            iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed
        )
        .then_some(Background::Color(
            theme.extended_palette().primary.weak.color
        )),
        border: Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        text_color: theme.extended_palette().background.base.text,
        ..Default::default()
    })
    .into()
}

/// Overlay of home / roll / nudge controls sized to the whole nav region, so
/// the caller can position it exactly like the cube hit area.
pub(super) fn viewcube_nav_controls<'a>() -> Element<'a, Message> {
    use crate::scene::NudgeDir;
    use crate::ui::icons;
    let r = VIEWCUBE_REGION_PX;
    let c = r * 0.5;
    let cube_half = VIEWCUBE_PX as f32 * 0.36; // VIEWCUBE_PX * VIEWCUBE_SCALE
    let nr = cube_half + 6.0; // nudge triangle distance from centre
    const BTN: f32 = 16.0;
    const TRI: f32 = 9.0;
    let ctr = |cx: f32, cy: f32, s: f32| (cx - s * 0.5, cy - s * 0.5);

    // Home top-left, roll arrows top-right.
    let (rax, ray) = (r - 2.0 * BTN - 4.0, 2.0);
    let (rbx, rby) = (r - BTN - 2.0, 2.0);
    // Nudge triangles pointing inward at the four cube faces.
    let (tux, tuy) = ctr(c, c - nr, TRI);
    let (tdx, tdy) = ctr(c, c + nr, TRI);
    let (tlx, tly) = ctr(c - nr, c, TRI);
    let (trx, try_) = ctr(c + nr, c, TRI);

    // Bottom layer: the cube/cardinal hit area covering the whole region. The
    // control buttons sit ABOVE it in the same stack, so a click on a button is
    // caught by the button while a click on the cube (or empty space) falls
    // through to this mouse_area → ViewportClick. Moves keep cursor_pos current.
    let cube_hit = mouse_area(
        Space::new()
            .width(iced::Length::Fixed(r))
            .height(iced::Length::Fixed(r)),
    )
    .on_move(Message::CursorMoved)
    .on_press(Message::ViewportClick);

    let controls = stack![
        cube_hit,
        vc_place(
            3.0,
            3.0,
            vc_btn(icons::themed_home(13.0), BTN, Message::ViewCubeHome)
        ),
        vc_place(
            rax,
            ray,
            vc_btn(icons::themed_undo(12.0, true), BTN, Message::ViewCubeRoll(false))
        ),
        vc_place(
            rbx,
            rby,
            vc_btn(icons::themed_redo(12.0, true), BTN, Message::ViewCubeRoll(true))
        ),
        vc_place(
            tux,
            tuy,
            vc_btn(
                icons::themed_arrow_down(8.0),
                TRI,
                Message::ViewCubeNudge(NudgeDir::Up)
            )
        ),
        vc_place(
            tdx,
            tdy,
            vc_btn(
                icons::themed_arrow_up(8.0),
                TRI,
                Message::ViewCubeNudge(NudgeDir::Down)
            )
        ),
        vc_place(
            tlx,
            tly,
            vc_btn(
                icons::themed_arrow_right(8.0),
                TRI,
                Message::ViewCubeNudge(NudgeDir::Left)
            )
        ),
        vc_place(
            trx,
            try_,
            vc_btn(
                icons::themed_arrow_left(8.0),
                TRI,
                Message::ViewCubeNudge(NudgeDir::Right)
            )
        ),
    ];

    container(controls)
        .width(iced::Length::Fixed(r))
        .height(iced::Length::Fixed(r))
        .into()
}

/// Fixed width of the UCS picker, so it can be centred under the cube.
pub(super) const UCS_PICKER_W: f32 = 84.0;

/// The WCS / named-UCS selector shown under the cube.
pub(super) fn viewcube_ucs_picker<'a>(current: String, names: Vec<String>) -> Element<'a, Message> {
    let mut options = vec!["WCS".to_string()];
    options.extend(names);
    let selected = if current.is_empty() {
        "WCS".to_string()
    } else {
        current
    };
    pick_list(options, Some(selected), Message::SetViewcubeUcs)
        .text_size(11)
        .padding([2, 6])
        // Fixed width so the caller can centre it under the cube centre with a
        // simple half-width offset (content-sized width would drift off-centre).
        .width(iced::Length::Fixed(UCS_PICKER_W))
        .style(move |theme: &Theme, _| {
            let palette = theme.extended_palette();
            iced::widget::pick_list::Style {
            background: Background::Color(palette.background.weak.color),
            border: Border {
                radius: 3.0.into(),
                color: palette.background.neutral.color,
                width: 1.0,
                ..Default::default()
            },
            text_color: palette.background.base.text,
            placeholder_color: palette.background.base.text.scale_alpha(0.68),
            handle_color: palette.background.base.text,
            }
        })
        .into()
}
