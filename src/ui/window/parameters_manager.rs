//! The Parameters Manager panel: the definition's parameters, one row per component.
use crate::app::Message;
use crate::scene::block_parameters_manager::{self as manager, Group, Row};
use crate::ui::dock::{self, Chrome, PanelId};
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Element, Fill, Length};

/// Every column stays visible in the panel's default width, so the fixed cells
/// plus their spacing must not outgrow it; `Description` takes what is left.
const COLUMNS: [(f32, &str); 7] = [
    (84.0, "Name"),
    (96.0, "Expression"),
    (52.0, "Value"),
    (44.0, "Type"),
    (34.0, "Show"),
    (34.0, "Order"),
    (0.0, "Description"),
];
const SPACING: f32 = 2.0;

/// The two numeric cells of a row. They print the same quantity at two
/// precisions, so typing into either writes the same number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Expression,
    Value,
}

/// The cell a manager edit is open on, and the parameter it writes. The panel is
/// rebuilt every frame and a tab can hold more than one Block Editor session, so
/// the edit carries the session and the parameter object resolved when the cell
/// was clicked: a number typed for one definition cannot land on a same-named
/// parameter of another, and a parameter deleted or renumbered in between does
/// not take it either.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub definition: codec::Handle,
    pub parameter: codec::Handle,
    pub component: crate::scene::model::object::BlockParameterComponent,
    pub name: String,
    pub cell: Cell,
}

/// Which cell the panel has open, what has been typed into it, and what the
/// list is filtered to. The filter carries the recorded `*`: `*` or empty lists
/// every row, a pattern with `*` matches the name, and the search field narrows
/// what is left by case-insensitive substring.
#[derive(Clone, Debug)]
pub struct Panel {
    pub editing: Option<Edit>,
    pub edit_buf: String,
    pub filter: String,
    pub search: String,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            editing: None,
            edit_buf: String::new(),
            filter: "*".to_owned(),
            search: String::new(),
        }
    }
}

impl Panel {
    fn is_open(&self, definition: codec::Handle, name: &str, cell: Cell) -> bool {
        self.editing.as_ref().is_some_and(|edit| {
            edit.definition == definition && edit.name == name && edit.cell == cell
        })
    }
}

/// The one field the panel can have open; a click that moves focus off it
/// commits what was typed, as Enter does.
pub fn edit_input_id() -> iced::widget::Id {
    iced::widget::Id::new("parameters-manager-edit")
}

fn cell(value: &str, width: f32) -> Element<'static, Message> {
    let body = text(if value.is_empty() { " " } else { value }.to_owned()).size(11);
    if width > 0.0 {
        container(body.width(Length::Fixed(width - 8.0)))
            .width(Length::Fixed(width))
            .into()
    } else {
        container(body).width(Fill).into()
    }
}

/// A numeric cell: its text is a button that opens the field, and the field is
/// what commits the row.
fn number_cell(
    definition: codec::Handle,
    name: &str,
    value: &str,
    which: Cell,
    panel: &Panel,
    width: f32,
) -> Element<'static, Message> {
    if panel.is_open(definition, name, which) {
        // The field starts empty — a typed number replaces the printed one
        // without first deleting it — but it shows what it will replace.
        return container(
            text_input(if value.is_empty() { " " } else { value }, &panel.edit_buf)
                .id(edit_input_id())
                .on_input(Message::ParametersManagerInput)
                .on_submit(Message::ParametersManagerCommit)
                .size(11),
        )
        .width(Length::Fixed(width))
        .into();
    }
    container(
        button(text(if value.is_empty() { " " } else { value }.to_owned()).size(11))
            .clip(true)
            .on_press(Message::ParametersManagerEdit(name.to_owned(), which)),
    )
    .width(Length::Fixed(width))
    .into()
}

fn show_cell(name: &str, show: bool, width: f32) -> Element<'static, Message> {
    container(
        button(text(crate::t!(if show { "Yes" } else { "No" }).into_owned()).size(11))
            .clip(true)
            .on_press(Message::ParametersManagerShow(name.to_owned())),
    )
    .width(Length::Fixed(width))
    .into()
}

fn header_line() -> Element<'static, Message> {
    row(COLUMNS.into_iter().map(|(width, name)| {
        let label = if name == "Order" {
            crate::tr!("parameters-manager", "order")
        } else {
            crate::t!(name).into_owned()
        };
        cell(&label, width)
    })).spacing(SPACING).into()
}

fn group_line(group: Group) -> Element<'static, Message> {
    let label = match group {
        Group::ActionParameters => crate::tr!("parameters-manager", "action-parameters"),
        Group::Attributes => crate::t!(group.name()).into_owned(),
    };
    container(text(label).size(11))
        .padding([6, 2])
        .into()
}

/// The two fields the list is narrowed with. Their effect is on the rows below
/// and on the count in the footer, not on any parameter. What has been read off
/// the pane is that the filter field carries `*` and that the search field says
/// what it searches for; this filter placeholder word and the width it is given
/// are layout, not measurement.
fn toolbar(panel: &Panel) -> Element<'static, Message> {
    row(vec![
        container(
            text_input(&crate::tr!("parameters-manager", "filter"), &panel.filter)
                .on_input(Message::ParametersManagerFilter)
                .size(11),
        )
        .width(Length::Fixed(96.0))
        .into(),
        container(
            text_input(&crate::tr!("parameters-manager", "search"), &panel.search)
                .on_input(Message::ParametersManagerSearch)
                .size(11),
        )
        .width(Fill)
        .into(),
    ])
    .spacing(SPACING)
    .into()
}

fn row_line(entry: &Row, panel: &Panel, definition: codec::Handle) -> Element<'static, Message> {
    let name = entry.name.as_str();
    let (expression, value) = if entry.numeric {
        (
            number_cell(
                definition,
                name,
                &entry.expression,
                Cell::Expression,
                panel,
                COLUMNS[1].0,
            ),
            number_cell(
                definition,
                name,
                &entry.value,
                Cell::Value,
                panel,
                COLUMNS[2].0,
            ),
        )
    } else {
        (
            cell(&entry.expression, COLUMNS[1].0),
            cell(&entry.value, COLUMNS[2].0),
        )
    };
    row(vec![
        cell(name, COLUMNS[0].0),
        expression,
        value,
        cell(&match entry.type_name {
            "Polar" => crate::tr!("parameters-manager", "polar"),
            "Visibility" => crate::tr!("parameters-manager", "visibility"),
            "Variable" => crate::tr!("parameters-manager", "variable"),
            name => crate::t!(name).into_owned(),
        }, COLUMNS[3].0),
        show_cell(name, entry.show, COLUMNS[4].0),
        cell(&entry.order, COLUMNS[5].0),
        cell(&entry.description, COLUMNS[6].0),
    ])
    .spacing(SPACING)
    .into()
}

pub fn view(
    rows: &[Row],
    panel: &Panel,
    definition: codec::Handle,
    width: f32,
    chrome: Chrome,
) -> Element<'static, Message> {
    let listed = manager::displayed(rows, &panel.filter, &panel.search);
    let mut lines: Vec<Element<'static, Message>> = vec![header_line()];
    for group in Group::ALL {
        let group_rows: Vec<&Row> = listed
            .iter()
            .copied()
            .filter(|row| row.group == group)
            .collect();
        if group_rows.is_empty() {
            continue;
        }
        lines.push(group_line(group));
        for row in group_rows {
            lines.push(row_line(row, panel, definition));
        }
    }
    let footer = crate::tr!("parameters-manager", "footer", shown = listed.len().to_string(), total = rows.len().to_string());
    dock::frame(
        dock::title_bar(
            PanelId::ParametersManager,
            crate::t!("Parameters Manager").into_owned(),
            chrome,
        ),
        column![
            container(toolbar(panel)).width(Fill).padding([2, 2]),
            container(
                scrollable(column(lines.into_iter()).spacing(2))
                    .width(Fill)
                    .height(Fill)
            )
            .width(Fill)
            .height(Fill),
            container(text(footer).size(11))
                .width(Fill)
                .padding([2, 2]),
        ]
        .spacing(2)
        .height(Fill),
        width,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::block_parameters_manager::Group;

    fn chrome() -> Chrome {
        Chrome {
            auto_collapse: false,
            floating: false,
            title_hovered: false,
        }
    }

    fn sample() -> Vec<Row> {
        vec![
            Row {
                group: Group::ActionParameters,
                name: "Angle1".into(),
                expression: "359.635713238".into(),
                value: "0".into(),
                type_name: "Polar",
                numeric: true,
                show: true,
                order: String::new(),
                description: String::new(),
            },
            Row {
                group: Group::ActionParameters,
                name: "Point1".into(),
                expression: "0".into(),
                value: "0".into(),
                type_name: "Point",
                numeric: false,
                show: true,
                order: String::new(),
                description: String::new(),
            },
            Row {
                group: Group::Attributes,
                name: "TAG1".into(),
                expression: String::new(),
                value: String::new(),
                type_name: "Variable",
                numeric: false,
                show: true,
                order: String::new(),
                description: String::new(),
            },
        ]
    }

    #[test]
    fn the_panel_prints_the_recorded_columns_groups_and_count() {
        let default_width = PanelId::ParametersManager.default_width();
        let _ = view(
            &sample(),
            &Panel::default(),
            codec::Handle::new(1),
            default_width,
            chrome(),
        );

        let headers: [&str; 7] = COLUMNS.map(|(_, name)| name);
        assert_eq!(
            headers,
            [
                "Name",
                "Expression",
                "Value",
                "Type",
                "Show",
                "Order",
                "Description"
            ]
        );
        // Every column, including the flexible last one, has to be reachable at
        // the width the panel opens at; a wider row set silently clips.
        let fixed: f32 = COLUMNS
            .iter()
            .map(|(width, _)| width)
            .sum::<f32>()
            + SPACING * (COLUMNS.len() as f32 - 1.0);
        assert!(
            fixed < default_width,
            "the six fixed columns take {fixed} of the {default_width} default width"
        );
        assert_eq!(
            [Group::ActionParameters.name(), Group::Attributes.name()],
            ["Action Parameters", "Attributes"]
        );
        assert_eq!(manager::footer(2, 2), "All: 2 of 2 parameters displayed");
    }

    #[test]
    fn the_filter_starts_at_the_recorded_default() {
        let rows = sample();
        let panel = Panel::default();
        assert_eq!(
            panel.filter, "*",
            "the filter field reads * when the pane opens"
        );
        assert_eq!(panel.search, "", "and the search field is empty");
        assert_eq!(
            manager::displayed(&rows, &panel.filter, &panel.search).len(),
            rows.len(),
            "which lists every row"
        );
        let filtered = Panel {
            filter: "*1".to_owned(),
            search: "point".to_owned(),
            ..Panel::default()
        };
        let shown = manager::displayed(&rows, &filtered.filter, &filtered.search);
        assert_eq!(
            shown
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            ["Point1"],
            "the filter matches the name and the search narrows what is left"
        );
        let _ = view(
            &rows,
            &filtered,
            codec::Handle::new(1),
            PanelId::ParametersManager.default_width(),
            chrome(),
        );
        assert_eq!(
            manager::footer(rows.len(), shown.len()),
            "All: 1 of 3 parameters displayed",
            "which is what the pane's footer prints"
        );
    }

    /// Where `label` is drawn in the pane built from `panel`, if anywhere.
    /// The pane is laid out very tall so no row is scrolled out of view.
    fn locate(panel: &Panel, label: &str) -> Option<iced::Point> {
        use iced_test::core::widget;
        use iced_test::runtime::user_interface::{Cache, UserInterface};
        use iced_test::Selector;
        let _one = crate::headless_ui_lock();
        let mut renderer = iced_test::futures::futures::executor::block_on(
            <iced::Renderer as iced_test::core::renderer::Headless>::new(
                iced_test::core::renderer::Settings::default(),
                None,
            ),
        )
        .expect("headless renderer");
        let width = PanelId::ParametersManager.default_width();
        let mut ui = UserInterface::build(
            view(&sample(), panel, codec::Handle::new(1), width, chrome()),
            iced::Size::new(width, 100_000.0),
            Cache::default(),
            &mut renderer,
        );
        let mut find = Selector::find(label);
        ui.operate(&renderer, &mut widget::operation::black_box(&mut find));
        match widget::Operation::finish(&find) {
            widget::operation::Outcome::Some(Some(target)) => {
                target.visible_bounds().map(|b| b.center())
            }
            _ => None,
        }
    }

    #[test]
    fn the_pane_draws_the_two_fields_and_only_the_rows_they_leave() {
        let both_empty = Panel {
            filter: String::new(),
            search: String::new(),
            ..Panel::default()
        };
        assert!(
            locate(&both_empty, "Name*").is_some(),
            "the filter field is in the pane"
        );
        assert!(
            locate(&both_empty, "Search for parameter").is_some(),
            "beside the search field"
        );
        for name in ["Angle1", "Point1", "TAG1"] {
            assert!(
                locate(&both_empty, name).is_some(),
                "with both fields empty, {name} is listed"
            );
        }
        assert!(
            locate(&both_empty, &crate::tr!("parameters-manager", "footer", shown = "3", total = "3")).is_some(),
            "and the footer says so"
        );

        let angle_only = Panel {
            filter: "Angle*".to_owned(),
            ..Panel::default()
        };
        assert!(
            locate(&angle_only, "Angle1").is_some(),
            "the named row stays"
        );
        for name in ["Point1", "TAG1"] {
            assert!(
                locate(&angle_only, name).is_none(),
                "{name} is filtered out, so the pane does not draw it at all"
            );
        }
        assert!(
            locate(&angle_only, "Attributes").is_none(),
            "and a group with no row left has no header either"
        );
        assert!(
            locate(&angle_only, &crate::tr!("parameters-manager", "footer", shown = "1", total = "3")).is_some(),
            "the footer counts what is left out of what there is"
        );
    }

    #[test]
    fn the_open_cell_is_the_one_clicked() {
        let definition = codec::Handle::new(7);
        let other = codec::Handle::new(8);
        let panel = Panel {
            editing: Some(Edit {
                definition,
                parameter: codec::Handle::new(9),
                component: crate::scene::model::object::BlockParameterComponent::Angle,
                name: "Angle1".to_owned(),
                cell: Cell::Value,
            }),
            edit_buf: "90".to_owned(),
            ..Panel::default()
        };
        assert!(panel.is_open(definition, "Angle1", Cell::Value));
        assert!(
            !panel.is_open(definition, "Angle1", Cell::Expression),
            "the other cell of the same row stays closed"
        );
        assert!(
            !panel.is_open(definition, "Distance1", Cell::Value),
            "and the same cell of another row does not open"
        );
        assert!(
            !panel.is_open(other, "Angle1", Cell::Value),
            "a session's open cell does not show in another definition's panel"
        );
        let _ = view(
            &sample(),
            &panel,
            definition,
            PanelId::ParametersManager.default_width(),
            chrome(),
        );
    }
}
