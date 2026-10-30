//! An area that turns the platform input method (IME) on while it is shown,
//! for text typed into a canvas rather than a text input (#2231).
//!
//! iced enables the input method only for a widget that asks for it on each
//! redraw; the in-canvas text editors are not text inputs, so a Chinese,
//! Japanese or Korean input method never opened and its keys arrived as raw
//! Latin letters. Here the area asks for it, and the text the input method
//! commits is published as typed. A text input inside the area that took the
//! commit itself keeps it.

use iced::advanced::input_method::{self, InputMethod, Purpose};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::{window, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::Message;

/// Wrap `content`, enabling the input method over it and publishing each
/// committed text through `on_commit`.
pub fn ime_zone<'a>(
    content: impl Into<Element<'a, Message>>,
    on_commit: fn(String) -> Message,
) -> Element<'a, Message> {
    Element::new(ImeZone {
        content: content.into(),
        on_commit,
    })
}

struct ImeZone<'a> {
    content: Element<'a, Message>,
    on_commit: fn(String) -> Message,
}

impl Widget<Message, Theme, Renderer> for ImeZone<'_> {
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
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
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
        match event {
            Event::Window(window::Event::RedrawRequested(_)) => {
                let bounds = layout.bounds();
                shell.request_input_method(&InputMethod::<&str>::Enabled {
                    cursor: Rectangle::new(bounds.position(), Size::new(1.0, 16.0)),
                    purpose: Purpose::Normal,
                    preedit: None,
                });
            }
            Event::InputMethod(input_method::Event::Commit(text))
                if !text.is_empty() && !shell.is_event_captured() =>
            {
                shell.publish((self.on_commit)(text.clone()));
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
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
        self.content.as_widget().draw(
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
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
