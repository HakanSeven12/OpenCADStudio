//! A wrapper that sends a message when its content is pressed, before the
//! content sees the press. A `mouse_area` hears a press only when nothing
//! inside captured it; a text field or a context menu always captures, so
//! a grid row's value cell (select the row) or a list row under a context
//! menu (select it before the menu opens) uses this instead.

use crate::app::Message;
use iced::advanced::widget::{tree, Operation, Tree};
use iced::advanced::{layout, mouse, overlay, renderer, Layout, Shell, Widget};
use iced::{Element, Event, Length, Rectangle, Size, Theme, Vector};

struct PressFirst<'a> {
    content: Element<'a, Message>,
    button: mouse::Button,
    message: Message,
}

/// `content`, sending `message` when `button` is pressed over it, before
/// `content` handles the press.
pub fn press_first<'a>(
    content: impl Into<Element<'a, Message>>,
    button: mouse::Button,
    message: Message,
) -> Element<'a, Message> {
    Element::new(PressFirst {
        content: content.into(),
        button,
        message,
    })
}

impl Widget<Message, Theme, iced::Renderer> for PressFirst<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(button)) = event {
            if *button == self.button && cursor.is_over(layout.bounds()) {
                shell.publish(self.message.clone());
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
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
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Message;
    use crate::ui::style::kit_metrics as km;

    /// A press in a grid row's value field selects the row, although the
    /// field captures the press (a `mouse_area` would never hear it).
    #[test]
    fn a_press_in_the_value_field_selects_the_row() {
        let row = crate::ui::kit::row_view(
            crate::ui::kit::name_text("Hyperlink"),
            crate::ui::kit::read_only_field("https://example.org"),
            false,
            Some(Message::PropRowSelect("Misc/Hyperlink".into())),
        );
        let mut ui = iced_test::simulator(row);
        let name = ui.find("Hyperlink").expect("name cell present");
        let bounds = name.visible_bounds().expect("name visible");
        ui.point_at(iced::Point::new(
            bounds.x + km::NAME_COL_W + 2.0 * km::INDENT,
            bounds.center_y(),
        ));
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Left,
        ))]);
        let selected = ui
            .into_messages()
            .any(|m| matches!(m, Message::PropRowSelect(ref key) if key == "Misc/Hyperlink"));
        assert!(selected, "the row is selected");
    }
}
