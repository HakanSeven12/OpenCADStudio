use crate::app::Message;
use codec::{Document, Handle};
use iced::{
    widget::{button, checkbox, column, container, row, scrollable, text, Space},
    Element, Fill,
};

pub(crate) struct Row {
    pub handle: Handle,
    pub name: String,
    pub parameter_type: String,
    pub location: String,
    pub enabled: bool,
}

pub(crate) struct State {
    pub document: u64,
    pub definition: Handle,
    pub expected: Vec<(Handle, codec::objects::ObjectType)>,
    pub rows: Vec<Row>,
    pub error: Option<String>,
    pub selected: Option<usize>,
}

impl State {
    pub(crate) fn new(
        document: u64,
        definition: Handle,
        doc: &Document,
    ) -> Result<Self, &'static str> {
        let grips = crate::scene::block_grips::grips(doc, definition)?;
        let expected = grips
            .iter()
            .map(|g| (g.handle, doc.objects[&g.handle].clone()))
            .collect();
        let rows = grips
            .into_iter()
            .map(|g| Row {
                handle: g.handle,
                enabled: g.enabled,
                name: g.name,
                parameter_type: crate::t!(g.parameter_type).into_owned(),
                location: g.location,
            })
            .collect();
        Ok(Self {
            document,
            definition,
            expected,
            rows,
            error: None,
            selected: None,
        })
    }
}

pub(crate) fn view_window(state: &State) -> Element<'_, Message> {
    let header = row![
        text(crate::t!("Cycling")).width(70),
        text(crate::t!("Parameter name")).width(180),
        text(crate::t!("Type")).width(145),
        text(crate::t!("Location")).width(180),
    ]
    .spacing(8)
    .padding(6);
    let mut rows = column![header].spacing(2);
    for (index, grip) in state.rows.iter().enumerate() {
        let cells = row![
            checkbox(grip.enabled)
                .on_toggle(move |enabled| Message::BlockCycleEnable(index, enabled))
                .width(70),
            text(&grip.name).width(180),
            text(&grip.parameter_type).width(145),
            text(&grip.location).width(180),
        ]
        .spacing(8)
        .align_y(iced::Center);
        rows = rows.push(
            button(cells)
                .on_press(Message::BlockCycleSelect(index))
                .padding(6)
                .style(if state.selected == Some(index) {
                    button::primary
                } else {
                    button::text
                }),
        );
    }
    let mut up = button(text(crate::t!("Move Up"))).width(Fill);
    let mut down = button(text(crate::t!("Move Down"))).width(Fill);
    let mut cycling = button(text(crate::t!("Cycling"))).width(Fill);
    if let Some(index) = state.selected.filter(|index| *index < state.rows.len()) {
        if index > 0 {
            up = up.on_press(Message::BlockCycleMove(index, true));
        }
        if index + 1 < state.rows.len() {
            down = down.on_press(Message::BlockCycleMove(index, false));
        }
        cycling = cycling.on_press(Message::BlockCycleEnable(index, !state.rows[index].enabled));
    }
    let list = scrollable(rows)
        .direction(scrollable::Direction::Both {
            vertical: scrollable::Scrollbar::default(),
            horizontal: scrollable::Scrollbar::default(),
        })
        .height(300)
        .width(Fill);
    let mut content = column![
        text(crate::t!("Grip cycling list")),
        row![
            container(list).style(container::bordered_box).width(Fill),
            column![up, down, cycling].spacing(6).width(115),
        ]
        .spacing(12),
    ]
    .spacing(8);
    if let Some(error) = &state.error {
        content = content.push(text(error).style(iced::widget::text::danger));
    }
    content = content.push(
        row![
            Space::new().width(Fill),
            button(text(crate::t!("Cancel"))).on_press(Message::CloseModal),
            button(text(crate::t!("Apply"))).on_press(Message::BlockCycleApply),
        ]
        .spacing(8),
    );
    container(content).padding(18).width(820).into()
}
