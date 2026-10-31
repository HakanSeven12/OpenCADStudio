//! A popup anchored under its trigger and kept inside the window. Whether it
//! is open is editor state, so typing, focus changes or a redraw never close
//! it. A press outside closes it (and is consumed, so the caret and the
//! selection stay); Escape is left to the editor's single Escape owner.
use super::super::*;

const GAP: f32 = 2.0;
const MARGIN: f32 = 4.0;

pub(in crate::app::view::overlay) fn anchored<'a>(
    base: impl Into<Element<'a, Message>>,
    popup: Option<Element<'a, Message>>,
    close: Message,
) -> Element<'a, Message> {
    Element::new(Anchored {
        base: base.into(),
        popup,
        close,
    })
}

struct Anchored<'a> {
    base: Element<'a, Message>,
    popup: Option<Element<'a, Message>>,
    close: Message,
}

impl Widget<Message, Theme, iced::Renderer> for Anchored<'_> {
    fn diff(&mut self, tree: &mut widget::Tree) {
        // The base keeps child 0 whether or not the popup exists.
        let mut elements: Vec<&mut Element<'_, Message>> = vec![&mut self.base];
        elements.extend(self.popup.as_mut());
        tree.diff_children_custom(
            &mut elements,
            |tree, element| tree.diff(element.as_widget_mut()),
            |element| widget::Tree::new(element.as_widget()),
        );
    }

    fn size(&self) -> Size<Length> {
        self.base.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.base
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.base
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.base.as_widget_mut().update(
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
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.base.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut iced::Renderer,
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
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        let anchor = layout.bounds() + translation;
        let (base_tree, popup_tree) = tree.children.split_at_mut(1);
        match (self.popup.as_mut(), popup_tree.first_mut()) {
            (Some(content), Some(tree)) => Some(overlay::Element::new(Box::new(Popup {
                content,
                tree,
                anchor,
                close: self.close.clone(),
            }))),
            _ => self.base.as_widget_mut().overlay(
                &mut base_tree[0],
                layout,
                renderer,
                viewport,
                translation,
            ),
        }
    }
}

struct Popup<'a, 'b> {
    content: &'b mut Element<'a, Message>,
    tree: &'b mut widget::Tree,
    anchor: Rectangle,
    close: Message,
}

impl overlay::Overlay<Message, Theme, iced::Renderer> for Popup<'_, '_> {
    fn layout(&mut self, renderer: &iced::Renderer, bounds: Size) -> layout::Node {
        let safe = Size::new(
            (bounds.width - 2.0 * MARGIN).max(0.0),
            (bounds.height - 2.0 * MARGIN).max(0.0),
        );
        let node = self.content.as_widget_mut().layout(
            self.tree,
            renderer,
            &layout::Limits::new(Size::ZERO, safe),
        );
        let size = node.size();
        let max_x = (bounds.width - MARGIN - size.width).max(MARGIN);
        let x = self.anchor.x.clamp(MARGIN, max_x);
        let below = self.anchor.y + self.anchor.height + GAP;
        let above = self.anchor.y - GAP - size.height;
        let y = if below + size.height <= bounds.height - MARGIN {
            below
        } else if above >= MARGIN {
            above
        } else {
            (bounds.height - MARGIN - size.height).max(MARGIN)
        };
        node.move_to(iced::Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        self.content.as_widget().draw(
            self.tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            &layout.bounds(),
        );
    }

    fn operate(
        &mut self,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
    ) {
        if let Event::Mouse(mouse::Event::ButtonPressed(_)) = event {
            // The trigger toggles itself, so a press on it is left to it.
            if !cursor.is_over(layout.bounds()) && !cursor.is_over(self.anchor) {
                shell.publish(self.close.clone());
                shell.capture_event();
                return;
            }
        }
        self.content.as_widget_mut().update(
            self.tree,
            event,
            layout,
            cursor,
            renderer,
            shell,
            &layout.bounds(),
        );
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            self.tree,
            layout,
            cursor,
            &layout.bounds(),
            renderer,
        )
    }

    fn overlay<'c>(
        &'c mut self,
        layout: Layout<'c>,
        renderer: &iced::Renderer,
    ) -> Option<overlay::Element<'c, Message, Theme, iced::Renderer>> {
        // Tooltips of the popup's own items.
        let bounds = layout.bounds();
        self.content
            .as_widget_mut()
            .overlay(self.tree, layout, renderer, &bounds, Vector::ZERO)
    }
}
