use crate::app::Message;
use crate::scene::block_lookup::{self, Property};
use codec::{
    objects::{BlockLookupAction, BlockLookupParameter},
    Handle,
};
use iced::{
    widget::{
        button, checkbox, column, container, pick_list, row, scrollable, text, text_input, Space,
    },
    Element, Fill,
};

#[derive(Debug, Clone)]
pub enum Action {
    Select(usize),
    Property(usize),
    AddProperty,
    RemoveProperty(usize),
    Cell(usize, usize, String),
    AddRow,
    DeleteRow(usize),
    MoveRow(usize, bool),
    Reverse(usize, bool),
    Audit,
    Apply,
}

pub(crate) struct Draft {
    pub parameter: Handle,
    pub original_parameter: BlockLookupParameter,
    pub original: Option<(Handle, BlockLookupAction)>,
    pub edited: BlockLookupAction,
}
impl Draft {
    pub(crate) fn changed(&self) -> bool {
        match &self.original {
            Some((_, original)) => original != &self.edited,
            None => self.edited != block_lookup::empty_table(&self.original_parameter),
        }
    }
}
pub(crate) struct State {
    pub document: u64,
    pub definition: Handle,
    pub epoch: u64,
    pub drafts: Vec<Draft>,
    pub selected: usize,
    pub properties: Vec<Property>,
    pub selected_property: Option<usize>,
    pub error: Option<String>,
    pub notice: Option<String>,
}

impl State {
    pub(crate) fn edit(&mut self, action: Action) {
        self.error = None;
        self.notice = None;
        match action {
            Action::Select(index) if index < self.drafts.len() => {
                self.selected = index;
                self.selected_property = None;
                return;
            }
            Action::Property(index) if index < self.properties.len() => {
                self.selected_property = Some(index);
                return;
            }
            _ => {}
        }
        let Some(draft) = self.drafts.get_mut(self.selected) else {
            return;
        };
        let primary_id = draft
            .original_parameter
            .parameter
            .parameter
            .element
            .eval
            .node_id;
        let table = &mut draft.edited;
        let cols = table.columns.len();
        let rows = table.row_count.max(0) as usize;
        match action {
            Action::AddProperty => {
                let Some(property) = self.selected_property.and_then(|i| self.properties.get(i))
                else {
                    return;
                };
                if cols >= 64
                    || table.columns.iter().any(|c| {
                        c.node_id == property.column.node_id
                            && c.connection_name == property.column.connection_name
                    })
                {
                    return;
                }
                let index = table
                    .columns
                    .iter()
                    .position(|c| c.lookup_property)
                    .unwrap_or(cols);
                for row in (0..rows).rev() {
                    table.expressions.insert(
                        row * cols + index,
                        if property.column.lookup_property {
                            format!("Value{}", row + 1)
                        } else {
                            property.value.clone()
                        },
                    );
                }
                table.columns.insert(index, property.column.clone());
                table.column_count += 1;
                self.selected_property = None;
            }
            Action::RemoveProperty(index)
                if table
                    .columns
                    .get(index)
                    .is_some_and(|c| !c.lookup_property || c.node_id != primary_id) =>
            {
                for row in (0..rows).rev() {
                    table.expressions.remove(row * cols + index);
                }
                table.columns.remove(index);
                table.column_count -= 1;
            }
            Action::Cell(row, col, value) if row < rows && col < cols => {
                table.expressions[row * cols + col] = value;
            }
            Action::AddRow if rows < 1000 => {
                for column in &table.columns {
                    let value = if column.lookup_property {
                        format!("Value{}", rows + 1)
                    } else {
                        self.properties
                            .iter()
                            .find(|p| {
                                p.column.node_id == column.node_id
                                    && p.column.connection_name == column.connection_name
                            })
                            .map(|p| p.value.clone())
                            .unwrap_or_default()
                    };
                    table.expressions.push(value);
                }
                table.row_count += 1;
            }
            Action::DeleteRow(row) if row < rows => {
                table.expressions.drain(row * cols..(row + 1) * cols);
                table.row_count -= 1;
            }
            Action::MoveRow(row, up) if row < rows => {
                let next = if up {
                    row.checked_sub(1)
                } else {
                    (row + 1 < rows).then_some(row + 1)
                };
                if let Some(next) = next {
                    for col in 0..cols {
                        table.expressions.swap(row * cols + col, next * cols + col);
                    }
                }
            }
            Action::Reverse(index, value)
                if table.columns.get(index).is_some_and(|c| c.lookup_property) =>
            {
                table.columns[index].writable = value;
            }
            Action::Audit => {
                match block_lookup::validate(table, draft.original.as_ref().map(|(_, t)| t))
                    .and_then(|_| {
                        block_lookup::validate_with_properties(
                            table,
                            draft.original.as_ref().map(|(_, t)| t),
                            &self.properties,
                        )
                    })
                {
                    Ok(()) => self.notice = Some(crate::t!("Table audit passed.").into_owned()),
                    Err(error) => self.error = Some(crate::t!(error.as_str()).into_owned()),
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn view(state: &State, sizing: crate::ui::modal::ModalSizing) -> Element<'_, Message> {
    let send = |a| Message::BlockLookup(a);
    let draft = &state.drafts[state.selected];
    let table = &draft.edited;
    let cols = table.columns.len();
    let rows = table.row_count.max(0) as usize;
    let mut headings = row![text(" ").width(28)].spacing(6);
    for (index, column) in table.columns.iter().enumerate() {
        let name = if column.lookup_property {
            state
                .properties
                .iter()
                .find(|p| p.column.lookup_property && p.column.node_id == column.node_id)
                .map(|p| p.label.clone())
                .unwrap_or_else(|| crate::t!("Lookup value").into_owned())
        } else {
            state
                .properties
                .iter()
                .find(|p| {
                    p.column.node_id == column.node_id
                        && p.column.connection_name == column.connection_name
                })
                .map(|p| p.label.clone())
                .unwrap_or_else(|| crate::t!("Input property").into_owned())
        };
        let mut header = column![text(name).size(13)];
        if column.lookup_property {
            header = header.push(
                row![
                    checkbox(column.writable).on_toggle(move |v| send(Action::Reverse(index, v))),
                    text(crate::t!("Reverse lookup").into_owned()).size(11)
                ]
                .spacing(4),
            );
            if column.node_id
                != draft
                    .original_parameter
                    .parameter
                    .parameter
                    .element
                    .eval
                    .node_id
            {
                header = header.push(
                    button(text(crate::t!("Remove").into_owned()).size(11))
                        .on_press(send(Action::RemoveProperty(index))),
                );
            }
        } else {
            header = header.push(
                button(text(crate::t!("Remove").into_owned()).size(11))
                    .on_press(send(Action::RemoveProperty(index))),
            );
        }
        headings = headings.push(container(header.spacing(4)).width(160));
    }
    let mut grid = column![headings].spacing(7);
    for row_index in 0..rows {
        let mut cells = row![text((row_index + 1).to_string()).width(28)].spacing(6);
        for col_index in 0..cols {
            cells = cells.push(
                text_input("", &table.expressions[row_index * cols + col_index])
                    .on_input(move |v| send(Action::Cell(row_index, col_index, v)))
                    .width(160)
                    .style(crate::ui::style::form::field_style),
            );
        }
        cells =
            cells
                .push(button(text("↑")).on_press_maybe(
                    (row_index > 0).then_some(send(Action::MoveRow(row_index, true))),
                ))
                .push(button(text("↓")).on_press_maybe(
                    (row_index + 1 < rows).then_some(send(Action::MoveRow(row_index, false))),
                ))
                .push(
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
            !table.columns.iter().any(|c| {
                c.node_id == p.column.node_id && c.connection_name == p.column.connection_name
            })
        })
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    container(
        column![
            row![
                text(crate::t!("Lookup parameter").into_owned()),
                pick_list(
                    Some(state.selected),
                    (0..state.drafts.len()).collect::<Vec<_>>(),
                    |i: &usize| state.drafts[*i].original_parameter.lookup_name.clone()
                )
                .on_select(move |i| send(Action::Select(i)))
                .width(Fill)
            ]
            .spacing(10),
            row![
                pick_list(state.selected_property, available, |i: &usize| state
                    .properties[*i]
                    .label
                    .clone())
                .on_select(move |i| send(Action::Property(i)))
                .placeholder(crate::t!("Choose input or Lookup output property").into_owned())
                .width(Fill),
                button(text(crate::t!("Add Property").into_owned()))
                    .on_press_maybe(state.selected_property.map(|_| send(Action::AddProperty)))
            ]
            .spacing(8),
            text(
                crate::t!(
                    "Add Linear or Polar distance inputs. Existing table columns are retained."
                )
                .into_owned()
            )
            .size(12),
            text(crate::tr!("block-lookup", "input-help", closed = "[2,6]", open = "(2,6]")).size(12),
            text(crate::tr!("block-lookup", "quoted-help", name = "`a b`")).size(12),
            scrollable(grid)
                .direction(scrollable::Direction::Both {
                    vertical: Default::default(),
                    horizontal: Default::default()
                })
                .height(Fill)
                .width(Fill),
            row![
                button(text(crate::t!("Add Row").into_owned()))
                    .on_press_maybe((rows < 1000).then_some(send(Action::AddRow))),
                button(text(crate::t!("Audit").into_owned())).on_press(send(Action::Audit)),
                Space::new().width(Fill)
            ]
            .spacing(8),
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
    .height(sizing.height)
    .into()
}
