use super::super::Message;
use crate::scene::{VIEWCUBE_PX, VIEWCUBE_REGION_PX};
use iced::widget::{button, container, mouse_area, stack, Space};
use iced::{Background, Border, Element, Theme};

// ── Render-mode picker ──────────────────────────────────────────────────────

// Top-left viewport control bar: a single dark chip holding (optionally) the
// horizontal/vertical split buttons, the render-mode picker, and the grid /
// grid-snap toggles. `include_split` is off for paper-space viewports, which
// have no model-tile splitting. Grid / snap reflect the active viewport's
// state and emit `ToggleGrid` / `ToggleGridSnap`.
// ── ViewCube navigation controls (home / roll / nudge / UCS) ───────────────

/// Place `el` at pixel offset (x, y) inside a fill layer (top-left origin).
fn vc_place<'a>(x: f32, y: f32, el: Element<'a, Message>) -> Element<'a, Message> {
    iced::widget::pin(el)
        .position(iced::Point::new(x.max(0.0), y.max(0.0)))
        .into()
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
        .then_some(Background::Color(theme.palette().primary.weak.color)),
        border: Border {
            radius: 3.0.into(),
            ..Default::default()
        },
        text_color: theme.palette().background.base.text,
        ..Default::default()
    })
    .into()
}

/// Overlay of home / roll / nudge controls sized to the whole nav region, so
/// the caller can position it exactly like the cube hit area.
pub(super) fn viewcube_nav_controls<'a>(
    viewport: Option<document::Handle>,
) -> Element<'a, Message> {
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
    .on_move(move |point| Message::CursorMoved(point, viewport))
    .on_press(Message::ViewportClick(viewport));

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
            vc_btn(
                icons::themed_undo(12.0, true),
                BTN,
                Message::ViewCubeRoll(false)
            )
        ),
        vc_place(
            rbx,
            rby,
            vc_btn(
                icons::themed_redo(12.0, true),
                BTN,
                Message::ViewCubeRoll(true)
            )
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

/// The WCS / named-UCS selector shown under the cube. `names` is the
/// epoch-memoised `Scene::cached_ucs_names` Arc (#34) — a refcount bump per
/// frame instead of N String clones.
pub(super) fn viewcube_ucs_picker<'a>(
    current: String,
    names: std::sync::Arc<[String]>,
) -> Element<'a, Message> {
    let mut options = vec!["WCS".to_string()];
    options.extend(names.iter().cloned());
    let selected = if current.is_empty() {
        "WCS".to_string()
    } else {
        current
    };
    crate::ui::dropdown_list::pick_list(Some(selected), options, |value| value.to_string())
        .on_select(Message::SetViewcubeUcs)
        .text_size(11)
        .padding([2, 6])
        // Fixed width so the caller can centre it under the cube centre with a
        // simple half-width offset (content-sized width would drift off-centre).
        .width(iced::Length::Fixed(UCS_PICKER_W))
        .style(move |theme: &Theme, _| {
            let palette = theme.palette();
            iced::widget::pick_list::Style {
                background: Background::Color(palette.background.weak.color),
                border: Border {
                    radius: 3.0.into(),
                    color: palette.background.neutral.color,
                    width: 1.0,
                    ..Default::default()
                },
                text_color: palette.background.base.text,
                placeholder_color: crate::ui::style::readable::hint_text(theme),
                handle_color: palette.background.base.text,
            }
        })
        .into()
}

/// True when the active Model tile is the whole canvas (a single tile), as
/// laid out by the tile rectangles themselves. That holds whatever size the
/// canvas was measured at, so the corner widgets can then follow the layout
/// (aligned to the top-right) instead of a pixel position computed from a
/// viewport size that is only measured after the view is built.
pub(super) fn tile_is_whole_canvas(rect: iced::Rectangle, canvas: (f32, f32)) -> bool {
    rect.x.abs() <= 0.5
        && rect.y.abs() <= 0.5
        && (rect.width - canvas.0).abs() <= 0.5
        && (rect.height - canvas.1).abs() <= 0.5
}

/// Padding from the canvas top-right corner, as `(top, right)`, to the nav
/// controls and to the UCS picker centred under them.
pub(super) fn corner_paddings() -> ((f32, f32), (f32, f32)) {
    let pad = crate::scene::VIEWCUBE_PAD;
    let controls = (pad, pad);
    let picker = (
        pad + VIEWCUBE_REGION_PX + 10.0,
        (pad + VIEWCUBE_REGION_PX * 0.5 - UCS_PICKER_W * 0.5).max(0.0),
    );
    (controls, picker)
}

/// `el` in a layer that fills the canvas, `top` / `right` px from its
/// top-right corner. The layout places it, so it needs no measured size.
pub(super) fn top_right_layer<'a>(
    el: Element<'a, Message>,
    (top, right): (f32, f32),
) -> Element<'a, Message> {
    container(el)
        .width(iced::Length::Fill)
        .height(iced::Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Top)
        .padding(iced::Padding {
            top,
            right,
            bottom: 0.0,
            left: 0.0,
        })
        .into()
}

#[cfg(test)]
mod corner_tests {
    use super::*;

    /// The tile rectangles are fractions of the canvas, so a single tile is the
    /// whole canvas at any measured size, including a transient one from the
    /// frame a dialog closes; a half tile is not.
    #[test]
    fn a_single_tile_is_the_whole_canvas_at_any_measured_size() {
        for canvas in [(1600.0, 860.0), (1590.0, 90.0), (300.0, 40.0)] {
            let whole = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: canvas.0,
                height: canvas.1,
            };
            assert!(tile_is_whole_canvas(whole, canvas));
        }
        let half = iced::Rectangle {
            x: 800.0,
            y: 0.0,
            width: 800.0,
            height: 860.0,
        };
        assert!(!tile_is_whole_canvas(half, (1600.0, 860.0)));
    }

    /// The picker's centre sits under the cube centre, measured from the
    /// top-right corner, so no viewport size is involved.
    #[test]
    fn the_picker_is_centred_under_the_cube_from_the_top_right_corner() {
        let ((c_top, c_right), (p_top, p_right)) = corner_paddings();
        assert_eq!(
            (c_top, c_right),
            (crate::scene::VIEWCUBE_PAD, crate::scene::VIEWCUBE_PAD)
        );
        let cube_centre = c_right + VIEWCUBE_REGION_PX * 0.5;
        let picker_centre = p_right + UCS_PICKER_W * 0.5;
        assert!((cube_centre - picker_centre).abs() < 1e-3);
        assert!(p_top > c_top + VIEWCUBE_REGION_PX);
    }
}
