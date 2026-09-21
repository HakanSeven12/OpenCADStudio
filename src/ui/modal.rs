//! Shared in-canvas modal overlay.
//!
//! Former pop-up *windows* (layer manager, style editors, About, …) render as
//! centered overlays on top of the main view instead of separate OS windows.
//! The native build has one main window and the web build has only the canvas,
//! so both stack dialogs here — one code path for every platform.

use crate::app::Message;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::widget::{button, column, container, mouse_area, opaque, row, sensor, stack, Space};
use iced::{
    Background, Border, Element, Event, Length, Padding, Rectangle, Renderer, Size, Theme, Vector,
};

#[derive(Debug, Clone, Copy)]
pub struct ModalOptions {
    movable: bool,
    resizable: bool,
    neutral_close: bool,
}

/// Sizing used inside an intrinsically measured modal.
///
/// [`INTRINSIC`](Self::INTRINSIC) lets each widget report the most space it can
/// use, while [`FILL`](Self::FILL) distributes the resulting shared frame to
/// sibling toolbars, panes and scroll areas.
#[derive(Debug, Clone, Copy)]
pub struct ModalSizing {
    pub width: Length,
    pub height: Length,
}

impl ModalSizing {
    pub const INTRINSIC: Self = Self {
        width: Length::Fluid(iced_core::length::Constraint::Max),
        height: Length::Fluid(iced_core::length::Constraint::Max),
    };

    pub const FILL: Self = Self {
        width: Length::Fill,
        height: Length::Fill,
    };
}

/// Measure an intrinsic copy of `content`, then lay the fill copy out in the
/// resulting shared frame. `extra` grows that frame when the resize handle is
/// dragged; `max` only caps the initial intrinsic pass.
pub fn intrinsic<'a>(
    measurement: impl Into<Element<'a, Message>>,
    content: impl Into<Element<'a, Message>>,
    max: Size,
    extra: Vector,
) -> Element<'a, Message> {
    Element::new(Intrinsic {
        children: [measurement.into(), content.into()],
        max,
        extra,
    })
}

struct Intrinsic<'a> {
    children: [Element<'a, Message>; 2],
    max: Size,
    extra: Vector,
}

impl Widget<Message, Theme, Renderer> for Intrinsic<'_> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fit.max(self.max.width + self.extra.x),
            Length::Fit.max(self.max.height + self.extra.y),
        )
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let measure_max = Size::new(
            self.max.width.min(limits.max().width),
            self.max.height.min(limits.max().height),
        );
        let measure_limits = layout::Limits::new(Size::ZERO, measure_max);
        let measured = self.children[0]
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &measure_limits);
        let size = Size::new(
            (measured.size().width + self.extra.x).min(limits.max().width),
            (measured.size().height + self.extra.y).min(limits.max().height),
        );
        let final_limits = layout::Limits::new(size, size);
        let content = self.children[1]
            .as_widget_mut()
            .layout(&mut tree.children[1], renderer, &final_limits);

        layout::Node::with_children(size, vec![measured, content])
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.children[1]
            .as_widget_mut()
            .operate(&mut tree.children[1], layout.child(1), renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.children[1].as_widget_mut().update(
            &mut tree.children[1],
            event,
            layout.child(1),
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.children[1]
            .as_widget()
            .mouse_interaction(
                &tree.children[1],
                layout.child(1),
                cursor,
                viewport,
                renderer,
            )
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.children[1]
            .as_widget()
            .draw(
                &tree.children[1],
                renderer,
                theme,
                style,
                layout.child(1),
                cursor,
                viewport,
            );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.children[1]
            .as_widget_mut()
            .overlay(
                &mut tree.children[1],
                layout.child(1),
                renderer,
                viewport,
                translation,
            )
    }
}

impl ModalOptions {
    pub const STANDARD: Self = Self {
        movable: true,
        resizable: true,
        neutral_close: false,
    };

    pub const NOTICE: Self = Self {
        movable: true,
        resizable: false,
        neutral_close: true,
    };
}

/// Dim and block the application beneath an overlay-owned dialog.
///
/// Some third-party overlays ignore pointer movement outside their interactive
/// controls. The shield owns the cursor and pointer presses across the window,
/// making such an overlay behave modally.
pub fn backdrop<'a>(
    base: impl Into<Element<'a, Message>>,
    on_close: Message,
) -> Element<'a, Message> {
    let shield = mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme
                        .palette()
                        .background
                        .strongest
                        .color
                        .scale_alpha(0.55),
                )),
                ..Default::default()
            }),
    )
    .on_press(on_close)
    .interaction(iced::mouse::Interaction::Idle);

    stack![base.into(), opaque(shield)].into()
}

/// A dialog's "unsaved changes" guard: `main` dimmed behind a small panel
/// that offers to discard the changes and close, or to keep editing. Clicking
/// the dim area keeps editing. Callers show it in place of the plain dialog
/// once the user asked to close with something to lose.
pub fn discard_guard<'a>(
    main: impl Into<Element<'a, Message>>,
    on_discard: Message,
    on_keep: Message,
) -> Element<'a, Message> {
    let shield = mouse_area(
        container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme
                        .palette()
                        .background
                        .strongest
                        .color
                        .scale_alpha(0.55),
                )),
                ..Default::default()
            }),
    )
    .on_press(on_keep.clone());
    let panel = container(
        column![
            iced::widget::text(crate::t!("Unsaved changes will be discarded.")).size(13.5),
            Space::new().height(12),
            row![
                button(iced::widget::text(crate::t!("Discard && close")).size(12))
                    .on_press(on_discard)
                    .padding([5, 14])
                    .style(button::danger),
                button(iced::widget::text(crate::t!("Keep editing")).size(12))
                    .on_press(on_keep)
                    .padding([5, 14])
                    .style(button::secondary),
            ]
            .spacing(10),
        ]
        .spacing(0),
    )
    .padding([18, 22])
    .style(container::rounded_box);
    stack![
        main.into(),
        shield,
        container(panel).center_x(Length::Fill).center_y(Length::Fill)
    ]
    .into()
}

/// Stack `content` over `base` behind a dimmed backdrop, framed with a title bar
/// (the ✕ close button at its right end). The backdrop only dims and blocks
/// clicks from reaching the view beneath — it does **not** dismiss the dialog;
/// closing is the ✕ button alone (`on_close`).
///
/// `offset` shifts the dialog from screen-centre so it can be dragged by its
/// title bar; pass `Vector::ZERO` to keep it centred.
pub fn modal<'a>(
    base: impl Into<Element<'a, Message>>,
    title: impl iced::widget::text::IntoFragment<'a>,
    content: impl Into<Element<'a, Message>>,
    on_close: Message,
    offset: Vector,
    options: ModalOptions,
) -> Element<'a, Message> {
    let close = if options.neutral_close {
        button(crate::ui::icons::themed_secondary(
            crate::ui::icons::CLOSE,
            13.0,
        ))
        .on_press(on_close)
        .padding([1, 7])
        .style(neutral_close_style)
    } else {
        button(crate::ui::icons::themed_danger(
            crate::ui::icons::CLOSE,
            13.0,
        ))
        .on_press(on_close)
        .padding([1, 7])
        .style(close_style)
    };

    // The dialog name is centred across the content width with the ✕ overlaid
    // at the right edge. The title bar is itself an overlay: the content layer
    // below determines the intrinsic modal width before `Fill` is resolved.
    let title_text = iced::widget::text(title).size(15);
    let title_surface: Element<'a, Message> = container(title_text)
        .width(Length::Fill)
        .height(Length::Fixed(24.0))
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into();
    let title_surface: Element<'a, Message> = if options.movable {
        mouse_area(title_surface)
            .on_press(Message::ModalGrab)
            .interaction(iced::mouse::Interaction::Grab)
            .into()
    } else {
        title_surface
    };
    let controls: Element<'a, Message> = row![close].align_y(iced::Center).into();
    let title_bar = stack![
        title_surface,
        container(controls)
            .width(Length::Fill)
            .height(Length::Fixed(24.0))
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Center),
    ]
    .width(Length::Fill)
    .height(Length::Fixed(24.0));

    let panel_style = |theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.palette().background.base.color,
        )),
        border: Border {
            color: theme.palette().background.neutral.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        ..Default::default()
    };

    // The first stack layer dictates its intrinsic size. Its top spacer reserves
    // room for the title, while the actual title bar and resize grip overlay it
    // without influencing the modal dimensions.
    let measured_content = sensor(content).on_resize(Message::ModalContentResized);
    let body_base = column![
        Space::new().height(Length::Fixed(24.0)),
        measured_content,
    ]
    .spacing(6);
    let mut body = stack![body_base, title_bar];
    if options.resizable {
        let resize = mouse_area(
            container(crate::ui::icons::themed_primary(crate::ui::icons::RESIZE, 15.0))
                .padding([0, 2]),
        )
        .on_press(Message::ModalResizeGrab)
        .interaction(iced::mouse::Interaction::Grab);
        body = body.push(
            container(resize)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Bottom),
        );
    }
    let framed: Element<'a, Message> =
        container(body).padding(10).style(panel_style).into();

    // Position via asymmetric padding (padding is non-negative): shifting a
    // centred box by `d` on an axis needs (near − far) padding = 2·d there.
    let pad = Padding {
        top: offset.y.max(0.0) * 2.0,
        right: (-offset.x).max(0.0) * 2.0,
        bottom: (-offset.y).max(0.0) * 2.0,
        left: offset.x.max(0.0) * 2.0,
    };

    // The backdrop fills the screen so dragging keeps tracking the cursor even
    // when it leaves the title bar; release anywhere ends the drag.
    let backdrop = mouse_area(
        container(framed)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .padding(pad)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme
                        .palette()
                        .background
                        .strongest
                        .color
                        .scale_alpha(0.55),
                )),
                ..Default::default()
            }),
    )
    .on_move(Message::ModalDragMove)
    .on_release(Message::ModalDragRelease);

    stack![
        base.into(),
        // `opaque` blocks pointer events from passing through, so the dimmed
        // backdrop swallows clicks instead of closing or hitting the view.
        opaque(backdrop),
    ]
    .into()
}

fn close_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => palette.danger.strong,
        _ => palette.background.weakest,
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn neutral_close_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.palette();
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => palette.background.weak,
        _ => palette.background.weakest,
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        text_color: pair.text,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
