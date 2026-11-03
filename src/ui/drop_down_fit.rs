//! Keeps an `iced_aw::DropDown` list under its field inside a scrollable.
//!
//! The drop-down keeps its list on screen by testing it against the viewport
//! it is handed, but reads only that viewport's width and height, as if it
//! started at the window's top-left corner. Inside a scrollable the viewport
//! is the scrollable's visible area in window coordinates, so a field in a
//! dock pallet (which does not start at the corner) sees its list "overflow",
//! and the list jumps left by its own width or up over the field. Wrapped
//! here, the drop-down gets a viewport reaching from the window corner to the
//! visible area's far edges: the test then trips only where the list really
//! leaves the visible area.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::Message;

/// Wrap a drop-down so its list is placed against the visible area's real
/// edges.
pub fn drop_down_fit<'a>(drop_down: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(DropDownFit {
        content: drop_down.into(),
    })
}

struct DropDownFit<'a> {
    content: Element<'a, Message>,
}

impl Widget<Message, Theme, Renderer> for DropDownFit<'_> {
    fn tag(&self) -> tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> tree::State {
        self.content.as_widget().state()
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.content.as_widget_mut().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content.as_widget_mut().layout(tree, renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget_mut()
            .update(tree, event, layout, cursor, renderer, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let from_corner = Rectangle {
            x: 0.0,
            y: 0.0,
            width: viewport.x + viewport.width,
            height: viewport.y + viewport.height,
        };
        let inner = self
            .content
            .as_widget_mut()
            .overlay(tree, layout, renderer, &from_corner, translation)?;
        Some(overlay::Element::new(Box::new(FitOverlay { inner })))
    }
}

struct FitOverlay<'a> {
    inner: overlay::Element<'a, Message, Theme, Renderer>,
}

impl overlay::Overlay<Message, Theme, Renderer> for FitOverlay<'_> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        self.inner.as_overlay_mut().layout(renderer, bounds)
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.inner
            .as_overlay()
            .draw(renderer, theme, style, layout, cursor);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        self.inner
            .as_overlay_mut()
            .update(event, layout, cursor, renderer, shell);
        if matches!(
            event,
            Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            })
        ) {
            crate::ui::dropdown_list::mark_dropdown_escape_captured();
            shell.capture_event();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.inner
            .as_overlay()
            .mouse_interaction(layout, cursor, renderer)
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.inner
            .as_overlay_mut()
            .operate(layout, renderer, operation);
    }
}
