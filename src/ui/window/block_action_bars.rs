use crate::{
    app::Message,
    scene::block_action_bars::{EntryKind, Group},
};
use document::Handle;
use iced::{
    widget::{button, canvas, column, container, row, text, tooltip, Space},
    Color, Element, Fill, Point, Rectangle, Vector,
};

#[derive(Default)]
struct DragState {
    start: Option<Point>,
    initial: Vector,
    last: Option<Point>,
}
struct DragHandle {
    parameter: Handle,
    offset: Vector,
    enabled: bool,
}
impl canvas::Program<Message> for DragHandle {
    type State = DragState;
    fn update(
        &self,
        state: &mut DragState,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if !self.enabled {
            state.start = None;
            return None;
        }
        use iced::mouse::{Button, Event};
        match event {
            iced::Event::Mouse(Event::CursorMoved { position }) => {
                state.last = Some(*position);
                state.start.map(|start| {
                    canvas::Action::publish(Message::BlockActionBarMove(
                        self.parameter,
                        state.initial + (*position - start),
                    ))
                    .and_capture()
                })
            }
            iced::Event::Mouse(Event::ButtonPressed(Button::Left)) => {
                let point = state
                    .last
                    .or_else(|| cursor.position())
                    .filter(|point| bounds.contains(*point))?;
                state.start = Some(point);
                state.initial = self.offset;
                Some(
                    canvas::Action::publish(Message::BlockActionBarMove(
                        self.parameter,
                        self.offset,
                    ))
                    .and_capture(),
                )
            }
            iced::Event::Mouse(Event::ButtonReleased(Button::Left))
                if state.start.take().is_some() =>
            {
                Some(
                    canvas::Action::publish(Message::BlockActionBarMove(
                        self.parameter,
                        self.offset,
                    ))
                    .and_capture(),
                )
            }
            _ => None,
        }
    }
    fn mouse_interaction(
        &self,
        state: &DragState,
        bounds: Rectangle,
        cursor: iced::mouse::Cursor,
    ) -> iced::mouse::Interaction {
        if state.start.is_some() {
            iced::mouse::Interaction::Grabbing
        } else if self.enabled && cursor.is_over(bounds) {
            iced::mouse::Interaction::Grab
        } else {
            Default::default()
        }
    }
    fn draw(
        &self,
        _state: &DragState,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        for x in [3.0, 8.0] {
            for y in [4.0, 9.0, 14.0] {
                frame.fill(
                    &canvas::Path::circle(Point::new(x, y), 1.0),
                    Color::from_rgb(0.25, 0.25, 0.25),
                );
            }
        }
        vec![frame.into_geometry()]
    }
}

pub(crate) fn view(
    group: Group,
    selected: Option<Handle>,
    offset: Vector,
    width: f32,
    enabled: bool,
    action_color: Color,
) -> Element<'static, Message> {
    let mut close = button(text("×").size(12)).padding([0, 5]);
    if enabled {
        close = close.on_press(Message::BlockActionBarHide(group.parameter));
    }
    let header = row![
        canvas(DragHandle {
            parameter: group.parameter,
            offset,
            enabled
        })
        .width(12)
        .height(18),
        text(group.label).size(12).width(Fill),
        close
    ]
    .spacing(5)
    .align_y(iced::Center);
    let mut controls = row![].spacing(4).width(Fill);
    let mut selected_entry = None;
    for entry in group.entries {
        let label = match entry.kind {
            EntryKind::Action(kind) | EntryKind::Orphan(kind) => {
                crate::t!(kind.name()).into_owned()
            }
            EntryKind::Lookup => crate::t!("Lookup").into_owned(),
            EntryKind::PropertiesTable => crate::t!("Table").into_owned(),
        };
        let is_selected = selected == Some(entry.handle);
        let mut control =
            button(text(label).size(12))
                .padding([3, 6])
                .style(move |theme, status| {
                    let mut style = if is_selected {
                        button::primary(theme, status)
                    } else {
                        button::secondary(theme, status)
                    };
                    if !is_selected {
                        style.background = Some(Color::from_rgb(0.88, 0.88, 0.88).into());
                        style.text_color = action_color;
                    }
                    style
                });
        if enabled {
            control = control.on_press(Message::BlockActionBarSelect(entry.handle));
        }
        controls = controls.push(tooltip(
            control,
            container(text(entry.label.clone()).size(12))
                .padding(5)
                .style(container::bordered_box),
            tooltip::Position::Top,
        ));
        if is_selected {
            selected_entry = Some(entry);
        }
    }
    let mut content = column![header, controls.wrap()].spacing(4);
    if let Some(entry) = selected_entry {
        let label = match entry.kind {
            EntryKind::Action(_) => "Edit selection / frame",
            EntryKind::Orphan(_) => "Associate parameter",
            _ => "Edit table",
        };
        let mut edit = button(text(crate::t!(label)).size(12)).padding([3, 6]);
        if enabled {
            edit = edit.on_press(Message::BlockActionBarEdit(entry.handle));
        }
        content = content.push(row![Space::new().width(Fill), edit]);
    }
    iced::widget::opaque(
        container(content)
            .padding(5)
            .width(width)
            .style(|_| container::Style {
                background: Some(Color::from_rgb(0.82, 0.82, 0.82).into()),
                text_color: Some(Color::BLACK),
                border: iced::Border {
                    color: Color::from_rgb(0.35, 0.35, 0.35),
                    width: 1.0,
                    radius: 3.0.into(),
                },
                ..Default::default()
            }),
    )
    .into()
}
