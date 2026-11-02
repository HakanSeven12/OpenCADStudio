use crate::app::Message;
use crate::scene::block_properties_table::{Column, TableView};
use codec::Handle;
use iced::{
    widget::{button, column, container, pick_list, row, scrollable, text, text_input, Space},
    Element, Fill,
};

#[derive(Debug, Clone)]
pub enum Action {
    Property(usize),
    AddProperty,
    RemoveProperty(usize),
    Cell(usize, usize, String),
    AddRow,
    DeleteRow(usize),
    Apply,
    MustMatch(bool),
    /// `OK` refused the table; leave without applying anything.
    DiscardClose,
    /// Stay in the dialog and keep the work.
    KeepEditing,
}

pub(crate) struct State {
    pub document: u64,
    pub definition: Handle,
    pub epoch: u64,
    pub original: Option<TableView>,
    pub must_match: bool,
    pub columns: Vec<Column>,
    pub properties: Vec<Column>,
    pub selected_property: Option<usize>,
    pub rows: Vec<Vec<String>>,
    pub error: Option<String>,
    pub notice: Option<String>,
    /// `OK` refused the table and the dialog now offers to discard and close.
    pub confirm_close: bool,
}

impl State {
    pub(crate) fn edit(&mut self, action: Action) {
        self.error = None;
        self.notice = None;
        self.confirm_close = false;
        match action {
            Action::Property(index) if index < self.properties.len() => {
                self.selected_property = Some(index);
                return;
            }
            _ => {}
        }
        match action {
            Action::AddProperty => {
                let Some(property) = self.selected_property.and_then(|i| self.properties.get(i))
                else {
                    return;
                };
                if self.columns.len() >= 64
                    || self.columns.iter().any(|c| {
                        c.node_id == property.node_id
                            && c.connection_name == property.connection_name
                    })
                {
                    return;
                }
                for row in &mut self.rows {
                    row.push("0".into());
                }
                self.columns.push(property.clone());
                self.selected_property = None;
            }
            Action::RemoveProperty(index) if index < self.columns.len() => {
                self.columns.remove(index);
                for row in &mut self.rows {
                    if index < row.len() {
                        row.remove(index);
                    }
                }
            }
            Action::Cell(row, col, value)
                if row < self.rows.len()
                    && col < self.columns.len()
                    && col < self.rows[row].len() =>
            {
                self.rows[row][col] = value;
            }
            Action::AddRow if !self.columns.is_empty() && self.rows.len() < 1000 => {
                self.rows.push(vec!["0".into(); self.columns.len()]);
            }
            Action::MustMatch(value) => self.must_match = value,
            Action::DeleteRow(row) if row < self.rows.len() => {
                self.rows.remove(row);
            }
            _ => {}
        }
    }
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let send = |a| Message::BlockPropertiesTable(a);
    let cols = state.columns.len();
    let rows = state.rows.len();
    let mut headings = row![text(" ").width(28)].spacing(6);
    for (index, column_binding) in state.columns.iter().enumerate() {
        let mut header = column![text(column_binding.display_name.clone()).size(13)];
        header = header.push(
            button(text(crate::t!("Remove").into_owned()).size(11))
                .on_press(send(Action::RemoveProperty(index))),
        );
        headings = headings.push(container(header.spacing(4)).width(160));
    }
    let mut grid = column![headings].spacing(7);
    for (row_index, row_values) in state.rows.iter().enumerate() {
        let mut cells = row![text((row_index + 1).to_string()).width(28)].spacing(6);
        for col_index in 0..cols {
            cells = cells.push(
                text_input("", row_values.get(col_index).map_or("", String::as_str))
                    .on_input(move |v| send(Action::Cell(row_index, col_index, v)))
                    .width(160)
                    .style(crate::ui::style::form::field_style),
            );
        }
        cells = cells.push(
            button(text(crate::t!("Delete").into_owned()))
                .on_press(send(Action::DeleteRow(row_index))),
        );
        grid = grid.push(cells);
    }
    let available = state
        .properties
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            !state
                .columns
                .iter()
                .any(|c| c.node_id == p.node_id && c.connection_name == p.connection_name)
        })
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    let main = container(
        column![
            row![
                pick_list(state.selected_property, available, |i: &usize| state
                    .properties[*i]
                    .display_name
                    .clone())
                .on_select(move |i| send(Action::Property(i)))
                .placeholder(crate::t!("Choose input property").into_owned())
                .width(Fill),
                button(text(crate::t!("Add Property").into_owned()))
                    .on_press_maybe(state.selected_property.map(|_| send(Action::AddProperty)))
            ]
            .spacing(8),
            text(
                crate::t!("Bind Linear or Polar parameters. Existing table columns are retained.")
                    .into_owned()
            )
            .size(12),
            scrollable(grid)
                .direction(scrollable::Direction::Both {
                    vertical: Default::default(),
                    horizontal: Default::default()
                })
                .height(Fill)
                .width(Fill),
            row![
                button(text(crate::t!("Add Row").into_owned()))
                    .on_press_maybe((cols > 0 && rows < 1000).then_some(send(Action::AddRow))),
                Space::new().width(Fill)
            ]
            .spacing(8),
            iced::widget::checkbox(state.must_match)
                .label(crate::t!("Block properties must match a row in the table").into_owned())
                .on_toggle(move |value| send(Action::MustMatch(value))),
            text(state.error.clone().unwrap_or_default())
                .size(12)
                .style(text::danger),
            text(state.notice.clone().unwrap_or_default()).size(12),
            row![
                Space::new().width(Fill),
                button(text(crate::t!("OK").into_owned()))
                    .on_press(send(Action::Apply))
                    .style(button::primary),
                button(text(crate::t!("Cancel").into_owned())).on_press(Message::CloseModal)
            ]
            .spacing(8),
        ]
        .spacing(12),
    )
    .padding(16)
    .width(sizing.width)
    .height(sizing.height);
    if !state.confirm_close {
        return main.into();
    }
    crate::ui::modal::discard_guard(
        main,
        Message::BlockPropertiesTable(Action::DiscardClose),
        Message::BlockPropertiesTable(Action::KeepEditing),
    )
}
