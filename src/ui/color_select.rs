//! Shared colour selector: a dropdown-style button that opens a list of named
//! colours (each shown with its swatch) plus the full ACI palette. Used by the
//! properties panel and every style editor so colour selection looks and
//! behaves the same everywhere.

use crate::app::Message;
use crate::ui::properties::acad_color_display;
use acadrust::types::Color as AcadColor;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Clipboard, Shell};
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{Background, Border, Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

/// Which "logical" entries the colour list offers besides the standard ACI
/// colours.
#[derive(Clone, Copy, Default)]
pub struct ColorExtras {
    pub by_layer: bool,
    pub by_block: bool,
}

/// Encode a colour as the ACI integer string the style editors store
/// (ByBlock=0, ByLayer=256, indexed 1-255; RGB has no ACI slot → ByLayer).
pub fn color_to_aci_string(c: AcadColor) -> String {
    match c {
        AcadColor::ByBlock => "0".to_string(),
        AcadColor::ByLayer => "256".to_string(),
        AcadColor::None => "257".to_string(),
        AcadColor::Index(i) => i.to_string(),
        AcadColor::Rgb { .. } => "256".to_string(),
    }
}

/// Decode an ACI integer string back into an `AcadColor`.
pub fn aci_string_to_color(s: &str) -> AcadColor {
    match s.trim().parse::<i16>().unwrap_or(256) {
        0 => AcadColor::ByBlock,
        256 => AcadColor::ByLayer,
        257 => AcadColor::None,
        n if (1..=255).contains(&n) => AcadColor::Index(n as u8),
        _ => AcadColor::ByLayer,
    }
}

/// Display name for a colour: the standard name for ACI 1-9 / ByLayer /
/// ByBlock, otherwise the "R,G,B" values (0-255) so unnamed palette colours
/// read meaningfully.
pub fn color_display_name(c: AcadColor) -> String {
    let (_, label) = acad_color_display(c);
    if label == "Index" || label == "Custom" {
        match c {
            AcadColor::Index(i) => {
                let (r, g, b) =
                    acadrust::types::aci_table::aci_to_rgb(i).unwrap_or((128, 128, 128));
                format!("{r},{g},{b}")
            }
            AcadColor::Rgb { r, g, b } => format!("{r},{g},{b}"),
            _ => label.to_string(),
        }
    } else {
        label.to_string()
    }
}

/// A small colour square.
fn swatch<'a>(bg: Color) -> Element<'a, Message> {
    container(text("").width(13).height(13))
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                color: theme.extended_palette().background.neutral.color,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        })
        .width(13)
        .height(13)
        .into()
}

/// Build a colour selector.
///
/// * `current` — the currently selected colour (shown on the button).
/// * `open` — whether the colour list / palette is expanded.
/// * `extras` — whether ByLayer / ByBlock appear in the list.
/// * `on_select` — called with the chosen colour.
/// * `on_toggle` — opens / closes the list.
pub fn color_selector<'a>(
    current: AcadColor,
    open: bool,
    extras: ColorExtras,
    on_select: impl Fn(AcadColor) -> Message + 'a,
    on_toggle: Message,
    on_more: Message,
) -> Element<'a, Message> {
    let (cur_bg, _) = acad_color_display(current);
    let cur_name = color_display_name(current);

    // Closed button: current swatch + name + caret.
    let head = button(
        row![
            swatch(cur_bg),
            text(cur_name).size(11),
            crate::ui::icons::themed_arrow_toggle(open, 9.0),
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .on_press(on_toggle)
    .padding([3, 6])
    .width(Length::Fill);

    if !open {
        return head.into();
    }

    let popup = container(color_list(extras, on_select, on_more))
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: Some(Background::Color(palette.background.weak.color)),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
            }
        })
        .padding(5)
        .width(220);

    // The popup is shown as a floating overlay (anchored below the button) so
    // it doesn't push the surrounding form down.
    Element::new(Floating {
        base: head.into(),
        popup: popup.into(),
    })
}

fn list_row_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let hovered = matches!(status, button::Status::Hovered);
    let text_color = if hovered {
        palette.background.strong.text
    } else {
        palette.background.base.text
    };
    button::Style {
        background: hovered.then_some(Background::Color(palette.background.strong.color)),
        text_color,
        ..Default::default()
    }
}

/// The colour list shown inside a picker popup: named ACI colours (with
/// swatches) plus a "More…" entry that opens the full palette window. Shared by
/// `color_selector` and the ribbon's colour overlay.
pub fn color_list<'a>(
    extras: ColorExtras,
    on_select: impl Fn(AcadColor) -> Message + 'a,
    on_more: Message,
) -> Element<'a, Message> {
    let named_row = |color: AcadColor| -> Element<'a, Message> {
        let (bg, name) = acad_color_display(color);
        button(
            row![swatch(bg), text(name).size(11)]
                .spacing(5)
                .align_y(iced::Center),
        )
        .on_press(on_select(color))
        .style(list_row_style)
        .padding([2, 4])
        .width(Length::Fill)
        .into()
    };

    let mut list = column![].spacing(1);
    if extras.by_layer {
        list = list.push(named_row(AcadColor::ByLayer));
    }
    if extras.by_block {
        list = list.push(named_row(AcadColor::ByBlock));
    }
    for i in 1u8..=9 {
        list = list.push(named_row(AcadColor::Index(i)));
    }
    list = list.push(
        button(text("More…").size(11))
            .on_press(on_more)
            .style(list_row_style)
            .padding([2, 4])
            .width(Length::Fill),
    );
    list.into()
}

/// Full ACI palette as a standalone window body: ByLayer / ByBlock plus the
/// 256-colour grid. `on_pick` is called with the chosen colour.
pub fn color_grid_window(on_pick: impl Fn(AcadColor) -> Message) -> Element<'static, Message> {
    let chip = |color: AcadColor, label: &'static str| -> Element<'static, Message> {
        let (bg, _) = acad_color_display(color);
        button(
            row![swatch(bg), text(label).size(11)]
                .spacing(5)
                .align_y(iced::Center),
        )
        .on_press(on_pick(color))
        .padding([3, 6])
        .into()
    };

    const COLS: u16 = 16;
    let mut grid = column![].spacing(2);
    let mut idx: u16 = 1;
    while idx <= 255 {
        let mut r = row![].spacing(2);
        for _ in 0..COLS {
            if idx > 255 {
                break;
            }
            let ci = idx as u8;
            let (bg, _) = acad_color_display(AcadColor::Index(ci));
            r = r.push(
                button(text("").width(18).height(18))
                    .on_press(on_pick(AcadColor::Index(ci)))
                    .style(move |theme: &Theme, status| button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            color: if matches!(status, button::Status::Hovered) {
                                theme.extended_palette().primary.base.color
                            } else {
                                theme.extended_palette().background.neutral.color
                            },
                            width: if matches!(status, button::Status::Hovered) {
                                1.5
                            } else {
                                1.0
                            },
                            radius: 1.0.into(),
                        },
                        text_color: theme.extended_palette().background.base.text,
                        ..Default::default()
                    })
                    .padding(0),
            );
            idx += 1;
        }
        grid = grid.push(r);
    }

    container(
        column![
            text("Select Color").size(13),
            row![chip(AcadColor::ByLayer, "ByLayer"), chip(AcadColor::ByBlock, "ByBlock")].spacing(6),
            scrollable(grid).height(Length::Fill),
        ]
        .spacing(8),
    )
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(
            theme.extended_palette().background.weak.color
        )),
        ..Default::default()
    })
    .padding(10)
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Render `base` inline with `popup` floating just below it — the shared
/// dropdown mechanic for the panel's custom dropdowns (colour picker, block
/// Name). Unlike iced's menu overlay it always opens downward.
pub fn floating_below<'a>(
    base: Element<'a, Message>,
    popup: Element<'a, Message>,
) -> Element<'a, Message> {
    Element::new(Floating { base, popup })
}

/// A widget that renders `base` inline and `popup` as a floating overlay
/// anchored just below it.
struct Floating<'a> {
    base: Element<'a, Message>,
    popup: Element<'a, Message>,
}

impl<'a> Widget<Message, Theme, Renderer> for Floating<'a> {
    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.base), widget::Tree::new(&self.popup)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
        tree.diff_children(&[self.base.as_widget(), self.popup.as_widget()]);
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.base.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.base.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
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
        self.base.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.base
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        self.base.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let bounds = layout.bounds();
        let anchor = Point::new(
            bounds.x + translation.x,
            bounds.y + bounds.height + translation.y + 2.0,
        );
        Some(overlay::Element::new(Box::new(FloatingOverlay {
            popup: &mut self.popup,
            tree: &mut tree.children[1],
            anchor,
        })))
    }
}

impl<'a> From<Floating<'a>> for Element<'a, Message> {
    fn from(f: Floating<'a>) -> Self {
        Element::new(f)
    }
}

struct FloatingOverlay<'a, 'b> {
    popup: &'b mut Element<'a, Message>,
    tree: &'b mut widget::Tree,
    anchor: Point,
}

impl overlay::Overlay<Message, Theme, Renderer> for FloatingOverlay<'_, '_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let viewport = Rectangle::with_size(bounds);
        let limits = layout::Limits::new(Size::ZERO, viewport.size());
        let node = self
            .popup
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let size = node.size();
        let mut x = self.anchor.x;
        let mut y = self.anchor.y;
        if x + size.width > viewport.width {
            x = (viewport.width - size.width).max(0.0);
        }
        if y + size.height > viewport.height {
            // Not enough room below — flip above the anchor.
            y = (self.anchor.y - bounds.height.min(0.0) - size.height).max(0.0);
        }
        layout::Node::with_children(size, vec![node]).translate(Vector::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let child = layout.children().next().unwrap();
        self.popup.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            child,
            cursor,
            &child.bounds(),
        );
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let child = layout.children().next().unwrap();
        let vp = child.bounds();
        self.popup.as_widget_mut().update(
            self.tree, event, child, cursor, renderer, clipboard, shell, &vp,
        );
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        let child = layout.children().next().unwrap();
        self.popup
            .as_widget_mut()
            .operate(self.tree, child, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let child = layout.children().next().unwrap();
        self.popup
            .as_widget()
            .mouse_interaction(self.tree, child, cursor, &child.bounds(), renderer)
    }
}
