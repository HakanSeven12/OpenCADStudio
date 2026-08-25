use crate::app::Message;
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

pub fn wide_menu<'a>(
    content: impl Into<Element<'a, Message>>,
    menu_width: f32,
) -> Element<'a, Message> {
    Element::new(WideMenu {
        content: content.into(),
        menu_width,
    })
}

struct WideMenu<'a> {
    content: Element<'a, Message>,
    menu_width: f32,
}

#[derive(Default)]
struct State {
    menu_layout: layout::Node,
}

impl Widget<Message, Theme, Renderer> for WideMenu<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
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
        let content = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            limits,
        );

        layout::Node::with_children(content.size(), vec![content])
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            layout.child(0),
            renderer,
            operation,
        );
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout.child(0),
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout.child(0),
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout.child(0),
            cursor,
            viewport,
        );
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut widget::Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        let bounds = layout.child(0).bounds();
        let (state, children) = (&mut tree.state, &mut tree.children);
        let state = state.downcast_mut::<State>();

        state.menu_layout = layout::Node::new(Size::new(
            self.menu_width.max(bounds.width),
            bounds.height,
        ))
        .move_to(Point::new(bounds.x, bounds.y));

        self.content.as_widget_mut().overlay(
            &mut children[0],
            Layout::new(&state.menu_layout),
            renderer,
            viewport,
            translation,
        )
    }
}
