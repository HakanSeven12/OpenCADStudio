//! The frame of a ribbon panel: its title strip drawn across the panel's
//! width, the title in it, and the dialog launcher in the strip's right
//! corner. A panel is as wide as its tools or its title; the strip and
//! launcher follow that width instead of filling, so measuring a panel stays
//! a plain layout. A press anywhere on the strip but the launcher works like
//! a button: it fires when released over the strip, so a drag that starts
//! there moves the panel. While the strip takes presses, it lights under the
//! pointer, background and title alike.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Renderer as _, Shell};
use iced::{
    Background, Border, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};

use crate::app::Message;
use crate::ui::style::chrome;

pub(super) struct PanelFrame<'a> {
    content: Element<'a, Message>,
    /// The title row, laid out in the strip under the content.
    title: Option<Element<'a, Message>>,
    corner: Option<Element<'a, Message>>,
    /// Height of the title strip at the bottom; 0 for none.
    strip: f32,
    /// The strip shows its panel's extra tools are open.
    lit: bool,
    on_press: Option<Message>,
}

/// Where the pointer is, and whether a press on the strip waits for its
/// release.
#[derive(Default)]
struct State {
    pressed: bool,
    hovered: bool,
}

impl<'a> PanelFrame<'a> {
    /// `content` ends with its title row, `strip` px high (0 for none);
    /// `corner` sits at the right end of that row.
    pub fn new(
        content: Element<'a, Message>,
        strip: f32,
        corner: Option<Element<'a, Message>>,
    ) -> Self {
        Self {
            content,
            title: None,
            corner: corner.filter(|_| strip > 0.0),
            strip,
            lit: false,
            on_press: None,
        }
    }

    /// The title row, drawn in the strip under `content` in the strip's
    /// text colour: `title_text`, or `strong` while the strip is lit.
    pub fn title(mut self, title: Option<Element<'a, Message>>) -> Self {
        self.title = title.filter(|_| self.strip > 0.0);
        self
    }

    /// Light the strip: its panel's extra tools are open.
    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    /// What a click on the strip sends.
    pub fn on_press(mut self, message: Option<Message>) -> Self {
        self.on_press = message;
        self
    }

    /// Whether `at` is on the strip and off the launcher.
    fn on_strip(&self, layout: Layout<'_>, at: Point) -> bool {
        let bounds = layout.bounds();
        let strip = Rectangle {
            y: bounds.y + bounds.height - self.strip,
            height: self.strip,
            ..bounds
        };
        strip.contains(at)
            && !self
                .corner
                .is_some()
                .then(|| layout.children().last())
                .flatten()
                .is_some_and(|corner| corner.bounds().contains(at))
    }

    fn parts(&self) -> impl Iterator<Item = &Element<'a, Message>> {
        std::iter::once(&self.content)
            .chain(self.title.as_ref())
            .chain(self.corner.as_ref())
    }

    fn parts_mut(&mut self) -> Vec<&mut Element<'a, Message>> {
        std::iter::once(&mut self.content)
            .chain(self.title.as_mut())
            .chain(self.corner.as_mut())
            .collect()
    }
}

impl<'a> Widget<Message, Theme, Renderer> for PanelFrame<'a> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        let mut parts = self.parts_mut();
        tree.diff_children_custom(
            &mut parts,
            |tree, part| tree.diff(part.as_widget_mut()),
            |part| widget::Tree::new(part.as_widget()),
        );
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let mut trees = tree.children.iter_mut();
        let content = self.content.as_widget_mut().layout(
            trees.next().expect("content tree"),
            renderer,
            limits,
        );
        let mut size = content.size();
        let mut nodes = vec![content];
        if let Some(title) = &mut self.title {
            // The strip under the content; the panel is as wide as the wider
            // of the two, and the narrower one is centred.
            let node = title.as_widget_mut().layout(
                trees.next().expect("title tree"),
                renderer,
                &layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, self.strip)),
            );
            let title_size = node.size();
            let width = size.width.max(title_size.width);
            nodes[0] = nodes[0]
                .clone()
                .move_to(Point::new(((width - size.width) / 2.0).round(), 0.0));
            nodes.push(node.move_to(Point::new(
                ((width - title_size.width) / 2.0).round(),
                size.height + (self.strip - title_size.height) / 2.0,
            )));
            size = Size::new(width, size.height + self.strip);
        }
        if let Some(corner) = &mut self.corner {
            let node = corner.as_widget_mut().layout(
                trees.next().expect("corner tree"),
                renderer,
                &layout::Limits::new(Size::ZERO, Size::new(size.width, self.strip)),
            );
            let corner_size = node.size();
            nodes.push(node.move_to(Point::new(
                (size.width - corner_size.width).max(0.0),
                size.height - (self.strip + corner_size.height) / 2.0,
            )));
        }
        layout::Node::with_children(size, nodes)
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
        for ((child, tree), layout) in self
            .parts_mut()
            .into_iter()
            .zip(&mut tree.children)
            .zip(layout.children())
            .rev()
        {
            child
                .as_widget_mut()
                .update(tree, event, layout, cursor, renderer, shell, viewport);
            if shell.is_event_captured() {
                return;
            }
        }
        let Some(message) = self.on_press.clone() else {
            return;
        };
        let over = cursor
            .position_over(layout.bounds())
            .is_some_and(|at| self.on_strip(layout, at));
        let state = tree.state.downcast_mut::<State>();
        if state.hovered != over {
            state.hovered = over;
            shell.request_redraw();
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.pressed = over;
                if over {
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.pressed => {
                state.pressed = false;
                if over {
                    shell.publish(message);
                    shell.capture_event();
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.on_press.is_some()
            && cursor
                .position_over(layout.bounds())
                .is_some_and(|at| self.on_strip(layout, at))
        {
            return mouse::Interaction::Pointer;
        }
        self.parts()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, tree), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
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
        let bounds = layout.bounds();
        // Lit while the extra tools are open, or under the pointer while a
        // click opens them.
        let hot = self.lit
            || (self.on_press.is_some()
                && cursor
                    .position_over(bounds)
                    .is_some_and(|at| self.on_strip(layout, at)));
        if self.strip > 0.0 {
            let top = bounds.y + bounds.height - self.strip;
            let quad = |y: f32, height: f32| renderer::Quad {
                bounds: Rectangle {
                    x: bounds.x,
                    y,
                    width: bounds.width,
                    height,
                },
                border: Border::default(),
                shadow: Shadow::default(),
                snap: true,
            };
            renderer.fill_quad(
                quad(top, self.strip),
                Background::Color(if hot {
                    chrome::hover(theme)
                } else {
                    chrome::title_bar(theme)
                }),
            );
            // A 1 px line over the strip.
            renderer.fill_quad(quad(top, 1.0), Background::Color(chrome::page(theme)));
        }
        let title_style = renderer::Style {
            text_color: if hot {
                chrome::strong(theme)
            } else {
                chrome::title_text(theme)
            },
        };
        let mut children = tree.children.iter().zip(layout.children());
        if let Some((tree, layout)) = children.next() {
            self.content
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
        }
        if let Some(title) = &self.title {
            if let Some((tree, layout)) = children.next() {
                title.as_widget().draw(
                    tree,
                    renderer,
                    theme,
                    &title_style,
                    layout,
                    cursor,
                    viewport,
                );
            }
        }
        if let Some(corner) = &self.corner {
            if let Some((tree, layout)) = children.next() {
                corner
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, viewport);
            }
        }
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        for ((child, tree), layout) in self
            .parts_mut()
            .into_iter()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .operate(tree, layout, renderer, operation);
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let children = std::iter::once(&mut self.content)
            .chain(self.title.as_mut())
            .chain(self.corner.as_mut())
            .zip(&mut tree.children)
            .zip(layout.children())
            .filter_map(|((child, tree), layout)| {
                child
                    .as_widget_mut()
                    .overlay(tree, layout, renderer, viewport, translation)
            })
            .collect::<Vec<_>>();
        (!children.is_empty()).then(|| overlay::Group::with_children(children).overlay())
    }
}

impl<'a> From<PanelFrame<'a>> for Element<'a, Message> {
    fn from(frame: PanelFrame<'a>) -> Self {
        Element::new(frame)
    }
}
