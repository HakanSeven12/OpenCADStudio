//! The frame of a ribbon panel: its title strip drawn across the panel's
//! width, and the dialog launcher in the strip's right corner. A panel is as
//! wide as its tools or its title; the strip and launcher follow that width
//! instead of filling, so measuring a panel stays a plain layout. A press
//! anywhere on the strip but the launcher works like a button: it fires when
//! released over the strip, so a drag that starts there moves the panel.

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
    corner: Option<Element<'a, Message>>,
    /// Height of the title strip at the bottom; 0 for none.
    strip: f32,
    /// The strip shows its panel's extra tools are open.
    lit: bool,
    on_press: Option<Message>,
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
            corner: corner.filter(|_| strip > 0.0),
            strip,
            lit: false,
            on_press: None,
        }
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
            && !layout
                .children()
                .nth(1)
                .is_some_and(|corner| corner.bounds().contains(at))
    }

    fn parts_mut(&mut self) -> Vec<&mut Element<'a, Message>> {
        std::iter::once(&mut self.content)
            .chain(self.corner.as_mut())
            .collect()
    }
}

impl<'a> Widget<Message, Theme, Renderer> for PanelFrame<'a> {
    // Whether a press on the strip waits for its release.
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<bool>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(false)
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
        let content = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits);
        let size = content.size();
        let mut nodes = vec![content];
        if let Some(corner) = &mut self.corner {
            let node = corner.as_widget_mut().layout(
                &mut tree.children[1],
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
        let pressed = tree.state.downcast_mut::<bool>();
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                *pressed = over;
                if over {
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if *pressed => {
                *pressed = false;
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
        std::iter::once(&self.content)
            .chain(self.corner.as_ref())
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
                Background::Color(if self.lit {
                    chrome::hover(theme)
                } else {
                    chrome::title_bar(theme)
                }),
            );
            // A 1 px line over the strip.
            renderer.fill_quad(quad(top, 1.0), Background::Color(chrome::page(theme)));
        }
        for ((child, tree), layout) in std::iter::once(&self.content)
            .chain(self.corner.as_ref())
            .zip(&tree.children)
            .zip(layout.children())
        {
            child
                .as_widget()
                .draw(tree, renderer, theme, style, layout, cursor, viewport);
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
