//! A column of small ribbon buttons that all take the width of the widest,
//! so their hover and pressed outlines line up on both sides. Each button
//! is laid out at its natural width first, then again at the column's.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::Message;

pub(super) struct StretchColumn<'a> {
    children: Vec<Element<'a, Message>>,
    spacing: f32,
}

impl<'a> StretchColumn<'a> {
    pub fn new(children: Vec<Element<'a, Message>>, spacing: f32) -> Self {
        Self { children, spacing }
    }
}

impl<'a> Widget<Message, Theme, Renderer> for StretchColumn<'a> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Shrink, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let max = limits.max();
        // Natural widths: a fluid width shrinks to its content.
        let natural = layout::Limits::with_compression(
            Size::ZERO,
            max,
            Size::new(true, limits.compression().height),
        );
        let width = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .map(|(child, tree)| {
                child
                    .as_widget_mut()
                    .layout(tree, renderer, &natural)
                    .size()
                    .width
            })
            .fold(0.0, f32::max)
            .min(max.width);
        // Every button again at the column's width.
        let stretched = layout::Limits::new(Size::new(width, 0.0), Size::new(width, max.height));
        let mut y = 0.0;
        let nodes = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .map(|(child, tree)| {
                let node = child
                    .as_widget_mut()
                    .layout(tree, renderer, &stretched)
                    .move_to(Point::new(0.0, y));
                y += node.size().height + self.spacing;
                node
            })
            .collect::<Vec<_>>();
        let height = (y - self.spacing).max(0.0);
        layout::Node::with_children(Size::new(width, height), nodes)
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
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child
                .as_widget_mut()
                .update(tree, event, layout, cursor, renderer, shell, viewport);
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
        self.children
            .iter()
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
        for ((child, tree), layout) in self
            .children
            .iter()
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
            .children
            .iter_mut()
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
        let overlays = self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .filter_map(|((child, tree), layout)| {
                child
                    .as_widget_mut()
                    .overlay(tree, layout, renderer, viewport, translation)
            })
            .collect::<Vec<_>>();
        (!overlays.is_empty()).then(|| overlay::Group::with_children(overlays).overlay())
    }
}

impl<'a> From<StretchColumn<'a>> for Element<'a, Message> {
    fn from(column: StretchColumn<'a>) -> Self {
        Element::new(column)
    }
}
