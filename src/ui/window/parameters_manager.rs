//! The Parameters Manager panel: the definition's parameters, one row per component.
use crate::app::Message;
use crate::scene::block_parameters_manager::{self as manager, Group, Row};
use crate::ui::dock::{self, Chrome, PanelId};
use iced::widget::{column, container, row, scrollable, text};
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

fn line(cells: [&str; 7]) -> Element<'static, Message> {
    let mut cells = cells.into_iter();
    row(COLUMNS
        .into_iter()
        .map(|(width, _)| cell(cells.next().unwrap_or_default(), width)))
    .spacing(SPACING)
    .into()
}

fn header_line() -> Element<'static, Message> {
    line(COLUMNS.map(|(_, name)| name))
}

fn group_line(group: Group) -> Element<'static, Message> {
    container(text(crate::t!(group.name()).into_owned()).size(11))
        .padding([6, 2])
        .into()
}

pub fn view(rows: &[Row], width: f32, chrome: Chrome) -> Element<'static, Message> {
    let listed = manager::displayed(rows, "*", "");
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
            lines.push(line([
                row.name.as_str(),
                row.expression.as_str(),
                row.value.as_str(),
                row.type_name,
                if row.show { "Yes" } else { "No" },
                row.order.as_str(),
                row.description.as_str(),
            ]));
        }
    }
    let footer = manager::footer(rows.len(), listed.len());
    dock::frame(
        dock::title_bar(
            PanelId::ParametersManager,
            crate::t!("Parameters Manager").into_owned(),
            chrome,
        ),
        column![
            container(
                scrollable(column(lines.into_iter()).spacing(2))
                    .width(Fill)
                    .height(Fill)
            )
            .width(Fill)
            .height(Fill),
            container(text(crate::t!(footer.as_str()).into_owned()).size(11))
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

    fn sample() -> Vec<Row> {
        vec![
            Row {
                group: Group::ActionParameters,
                name: "Angle1".into(),
                expression: "359.635713238".into(),
                value: "0".into(),
                type_name: "Polar",
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
                show: true,
                order: String::new(),
                description: String::new(),
            },
        ]
    }

    #[test]
    fn the_panel_prints_the_recorded_columns_groups_and_count() {
        let chrome = Chrome {
            auto_collapse: false,
            floating: false,
            title_hovered: false,
        };
        let default_width = PanelId::ParametersManager.default_width();
        let _ = view(&sample(), default_width, chrome);

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
}
