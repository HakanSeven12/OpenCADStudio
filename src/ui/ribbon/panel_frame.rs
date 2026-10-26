//! The frame of a ribbon panel: its title strip drawn across the panel's
//! width, and the dialog launcher in the strip's right corner. A panel is as
//! wide as its tools or its title; the strip and launcher follow that width
//! instead of filling, so measuring a panel stays a plain layout.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Renderer as _, Shell};
use iced::{
    Background, Border, Element, Event, Length, Point, Rectangle, Renderer, Shadow, Size, Theme,
    Vector,
};

use super::widgets::PANEL_TITLE_H;
use crate::app::Message;
use crate::ui::style::chrome;

pub(super) struct PanelFrame<'a> {
    content: Element<'a, Message>,
    corner: Option<Element<'a, Message>>,
    strip: bool,
}

impl<'a> PanelFrame<'a> {
    /// `content` ends with its title row when `strip` is set; `corner` sits at
    /// the right end of that row.
    pub fn new(
        content: Element<'a, Message>,
        strip: bool,
        corner: Option<Element<'a, Message>>,
    ) -> Self {
        Self {
            content,
            corner: corner.filter(|_| strip),
            strip,
        }
    }

    fn parts_mut(&mut self) -> Vec<&mut Element<'a, Message>> {
        std::iter::once(&mut self.content)
            .chain(self.corner.as_mut())
            .collect()
    }
}

impl<'a> Widget<Message, Theme, Renderer> for PanelFrame<'a> {
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
                &layout::Limits::new(Size::ZERO, Size::new(size.width, PANEL_TITLE_H)),
            );
            let corner_size = node.size();
            nodes.push(node.move_to(Point::new(
                (size.width - corner_size.width).max(0.0),
                size.height - (PANEL_TITLE_H + corner_size.height) / 2.0,
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
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
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
        if self.strip {
            let top = bounds.y + bounds.height - PANEL_TITLE_H;
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
                quad(top, PANEL_TITLE_H),
                Background::Color(chrome::title_bar(theme)),
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
