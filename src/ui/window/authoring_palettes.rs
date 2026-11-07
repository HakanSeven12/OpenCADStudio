//! The Block Editor's authoring catalogs, using the shared dock chrome.

use crate::app::Message;
use crate::modules::{parametric, IconKind, ToolDef};
use crate::ui::dock::{self, Chrome, PanelId, VerticalLabel};
use crate::ui::icons;
use crate::ui::ribbon::{tooltip_content, tooltip_style};
use iced::widget::{
    button, canvas, column, container, row, rule, scrollable, stack, text, tooltip, Space,
};
use iced::{Background, Border, Element, Fill, Theme};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Parameters,
    Actions,
    ParameterSets,
    Constraints,
}

impl Tab {
    pub const ALL: [Self; 4] = [
        Self::Parameters,
        Self::Actions,
        Self::ParameterSets,
        Self::Constraints,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Parameters => "Parameters",
            Self::Actions => "Actions",
            Self::ParameterSets => "Parameter Sets",
            Self::Constraints => "Constraints",
        }
    }

    pub(crate) fn widget_id(self) -> &'static str {
        match self {
            Self::Parameters => "authoring-palette-parameters",
            Self::Actions => "authoring-palette-actions",
            Self::ParameterSets => "authoring-palette-parameter-sets",
            Self::Constraints => "authoring-palette-constraints",
        }
    }
}

const PARAMETERS: &[(&str, &[u8])] = &[
    ("Point", include_bytes!("../../../assets/icons/point.svg")),
    (
        "Linear",
        include_bytes!("../../../assets/icons/dim_linear.svg"),
    ),
    (
        "Polar",
        include_bytes!("../../../assets/icons/array_polar.svg"),
    ),
    ("XY", include_bytes!("../../../assets/icons/move.svg")),
    (
        "Rotation",
        include_bytes!("../../../assets/icons/rotate.svg"),
    ),
    (
        "Alignment",
        include_bytes!("../../../assets/icons/modify_align.svg"),
    ),
    ("Flip", include_bytes!("../../../assets/icons/mirror.svg")),
    (
        "Visibility",
        include_bytes!("../../../assets/icons/constrain/show.svg"),
    ),
    (
        "Lookup",
        include_bytes!("../../../assets/icons/data_extract.svg"),
    ),
    (
        "Basepoint",
        include_bytes!("../../../assets/icons/base_point.svg"),
    ),
];

const ACTIONS: &[(&str, &[u8])] = &[
    ("Move", include_bytes!("../../../assets/icons/move.svg")),
    ("Scale", include_bytes!("../../../assets/icons/scale.svg")),
    (
        "Stretch",
        include_bytes!("../../../assets/icons/stretch.svg"),
    ),
    (
        "Polar Stretch",
        include_bytes!("../../../assets/icons/array_polar.svg"),
    ),
    ("Rotate", include_bytes!("../../../assets/icons/rotate.svg")),
    ("Flip", include_bytes!("../../../assets/icons/mirror.svg")),
    (
        "Array",
        include_bytes!("../../../assets/icons/array_rect.svg"),
    ),
    (
        "Lookup",
        include_bytes!("../../../assets/icons/data_extract.svg"),
    ),
    (
        "Block Properties Table",
        include_bytes!("../../../assets/icons/table.svg"),
    ),
];

const PARAMETER_SETS: &[(&str, &[u8])] = &[
    (
        "Point Move",
        include_bytes!("../../../assets/icons/point.svg"),
    ),
    (
        "Linear Move",
        include_bytes!("../../../assets/icons/dim_linear.svg"),
    ),
    (
        "Linear Stretch",
        include_bytes!("../../../assets/icons/dim_linear.svg"),
    ),
    (
        "Linear Array",
        include_bytes!("../../../assets/icons/dim_linear.svg"),
    ),
    (
        "Linear Move Pair",
        include_bytes!("../../../assets/icons/dim_linear.svg"),
    ),
    (
        "Linear Stretch Pair",
        include_bytes!("../../../assets/icons/dim_linear.svg"),
    ),
    (
        "Polar Move",
        include_bytes!("../../../assets/icons/array_polar.svg"),
    ),
    (
        "Polar Stretch",
        include_bytes!("../../../assets/icons/array_polar.svg"),
    ),
    (
        "Polar Array",
        include_bytes!("../../../assets/icons/array_polar.svg"),
    ),
    (
        "Polar Move Pair",
        include_bytes!("../../../assets/icons/array_polar.svg"),
    ),
    (
        "Polar Stretch Pair",
        include_bytes!("../../../assets/icons/array_polar.svg"),
    ),
    ("XY Move", include_bytes!("../../../assets/icons/move.svg")),
    (
        "XY Move Pair",
        include_bytes!("../../../assets/icons/move.svg"),
    ),
    (
        "XY Move Box Set",
        include_bytes!("../../../assets/icons/move.svg"),
    ),
    (
        "XY Stretch Box Set",
        include_bytes!("../../../assets/icons/move.svg"),
    ),
    (
        "XY Array Box Set",
        include_bytes!("../../../assets/icons/move.svg"),
    ),
    (
        "Rotation Set",
        include_bytes!("../../../assets/icons/rotate.svg"),
    ),
    (
        "Flip Set",
        include_bytes!("../../../assets/icons/mirror.svg"),
    ),
    (
        "Visibility Set",
        include_bytes!("../../../assets/icons/constrain/show.svg"),
    ),
    (
        "Lookup Set",
        include_bytes!("../../../assets/icons/data_extract.svg"),
    ),
];

#[test]
fn every_parameter_set_has_an_authoring_workflow() {
    for (label, _) in PARAMETER_SETS {
        assert!(
            crate::modules::block_editor::parameter_set::get(label).is_some(),
            "{label}"
        );
    }
}

/// Catalog rows remain inert until their authoring commands exist.
#[allow(clippy::too_many_arguments)]
fn catalog(
    tab: Tab,
    visibility_error: Option<&'static str>,
    point_error: Option<&'static str>,
    flip_error: Option<&'static str>,
    lookup_error: Option<&'static str>,
    properties_table_error: Option<&'static str>,
    move_error: Option<&'static str>,
    scale_error: Option<&'static str>,
    rotate_error: Option<&'static str>,
    flip_action_error: Option<&'static str>,
    stretch_action_error: Option<&'static str>,
    polar_stretch_action_error: Option<&'static str>,
    array_action_error: Option<&'static str>,
    parameter_set_error: Option<&'static str>,
) -> Element<'static, Message> {
    let (entries, unavailable) = match tab {
        Tab::Parameters => (
            PARAMETERS,
            crate::t!("Parameter authoring is not available yet."),
        ),
        Tab::Actions => (ACTIONS, crate::t!("Action authoring is not available yet.")),
        Tab::ParameterSets => (
            PARAMETER_SETS,
            crate::t!("Parameter set authoring is not available yet."),
        ),
        Tab::Constraints => return constraints(),
    };
    column(entries.iter().map(|(label, icon)| {
        let visibility = tab == Tab::Parameters && *label == "Visibility";
        let point = tab == Tab::Parameters && *label == "Point";
        let distance = tab == Tab::Parameters && matches!(*label, "Linear" | "Polar" | "XY");
        let orientation = tab == Tab::Parameters && matches!(*label,"Rotation"|"Alignment"|"Flip");
        let orientation_error = if *label=="Flip" {flip_error} else {point_error};
        let single=tab==Tab::Parameters&&matches!(*label,"Lookup"|"Basepoint");
        let lookup_action=tab==Tab::Actions&&*label=="Lookup";
        let properties_table_action=tab==Tab::Actions&&*label=="Block Properties Table";
        let geometry_action=tab==Tab::Actions&&matches!(*label,"Move"|"Scale"|"Stretch"|"Polar Stretch"|"Rotate"|"Flip"|"Array");
        let action_error=match *label {"Scale"=>scale_error,"Stretch"=>stretch_action_error,"Polar Stretch"=>polar_stretch_action_error,"Rotate"=>rotate_error,"Flip"=>flip_action_error,"Array"=>array_action_error,_=>move_error};
        let parameter_set = (tab == Tab::ParameterSets)
            .then(|| crate::modules::block_editor::parameter_set::get(label))
            .flatten();
        let enabled = (geometry_action&&action_error.is_none())||(single&&point_error.is_none())||(lookup_action&&lookup_error.is_none())||(properties_table_action&&properties_table_error.is_none())||(orientation && orientation_error.is_none()) || (visibility && visibility_error.is_none()) || ((point || distance) && point_error.is_none()) || (parameter_set.is_some() && parameter_set_error.is_none());
        let base = if enabled {
            icons::semantic(icon, 28.0)
        } else {
            icons::semantic_disabled(icon, 28.0)
        };
        let icon = if tab != Tab::Parameters {
            stack![
                base,
                container(if enabled {
                    icons::semantic(include_bytes!("../../../assets/icons/ui/bolt.svg"), 14.0)
                } else {
                    icons::semantic_disabled(include_bytes!("../../../assets/icons/ui/bolt.svg"), 14.0)
                })
                .width(28)
                .height(28)
                .align_right(Fill)
                .align_top(Fill),
            ]
            .into()
        } else {
            base
        };
        let message = enabled.then(|| Message::RibbonToolClick {
            tool_id: if parameter_set.is_some() { format!("BPARAMETERSET_{}", label.to_ascii_uppercase().replace(' ', "_")) } else if geometry_action || lookup_action {format!("BACTION_{}",label.to_ascii_uppercase())}else if properties_table_action {"BPROPERTIESTABLE".into()}else{format!("BPARAMETER_{}", label.to_ascii_uppercase())},
            event: crate::modules::ModuleEvent::Command(if parameter_set.is_some() { format!("BPARAMETERSET {label}") } else if geometry_action || lookup_action {format!("BACTION {}",label.to_ascii_uppercase())}else if properties_table_action {"BPROPERTIESTABLE".into()}else{format!("BPARAMETER {}", label.to_ascii_uppercase())}),
        });
        let description = if visibility {
            crate::t!(visibility_error
                .unwrap_or("Place a visibility parameter, then use BVSTATE to manage its states."))
            .into_owned()
        } else if point {
            crate::t!(point_error.unwrap_or("Place a point parameter and its position label."))
                .into_owned()
        } else if distance {
            crate::t!(point_error.unwrap_or("Place the base point, endpoint, and dimension label. Creates one end grip; associate an action to change geometry.")).into_owned()
        } else if orientation {
            crate::t!(orientation_error.unwrap_or(match *label {
                "Rotation" => "Place the center, specify a radius, then the default rotation angle.",
                "Alignment" => "Place the alignment base point and tangent direction.",
                _ => "Place a reflection line and its flip-state label. Associate a Flip action to mirror geometry.",
            })).into_owned()
        } else if single {
            crate::t!(point_error.unwrap_or(if *label=="Basepoint" {"Place or relocate the dynamic block insertion anchor."}else{"Place a Lookup parameter, then use the Lookup action to edit its property table."})).into_owned()
        } else if geometry_action {
            crate::t!(action_error.unwrap_or(if *label=="Stretch" {"Select a parameter endpoint, define the stretch frame, select its objects, then place the action marker."}else if *label=="Array" {"Select a parameter endpoint, select the objects to array, specify the column offset, then place the action marker."}else{"Select a parameter endpoint, select the objects it controls, then place the action marker. Use the reference parameter grip in the drawing or Test Block."})).into_owned()
        } else if lookup_action {
            crate::t!(lookup_error.unwrap_or("Choose a Lookup parameter, then create or edit its property table.")).into_owned()
        } else if properties_table_action {
            crate::t!(properties_table_error.unwrap_or("Create or edit this block's properties table; rows bind Linear and Polar parameters.")).into_owned()
        } else if parameter_set.is_some() {
            crate::t!(parameter_set_error.unwrap_or("Place a parameter, then continue into its matching action or editor.")).into_owned()
        } else {
            unavailable.to_string()
        };
        let entry = tool_row(label, icon, message, &description);
        if visibility {
            container(entry).id("authoring-tool-visibility").into()
        } else if point {
            container(entry).id("authoring-tool-point").into()
        } else if distance {
            container(entry).id(match *label { "Linear" => "authoring-tool-linear", "Polar" => "authoring-tool-polar", _ => "authoring-tool-xy" }).into()
        } else if orientation {
            container(entry).id(match *label {"Rotation"=>"authoring-tool-rotation","Alignment"=>"authoring-tool-alignment",_=>"authoring-tool-flip"}).into()
        } else if single {
            container(entry).id(if *label=="Lookup" {"authoring-tool-lookup"}else{"authoring-tool-basepoint"}).into()
        } else if geometry_action {
            container(entry).id(match *label {"Move"=>"authoring-tool-move-action","Scale"=>"authoring-tool-scale-action","Rotate"=>"authoring-tool-rotate-action","Stretch"=>"authoring-tool-stretch-action","Polar Stretch"=>"authoring-tool-polar-stretch-action","Array"=>"authoring-tool-array-action",_=>"authoring-tool-flip-action"}).into()
        } else if lookup_action {
            container(entry).id("authoring-tool-lookup-table").into()
        } else if properties_table_action {
            container(entry).id("authoring-tool-properties-table").into()
        } else {
            entry
        }
    }))
    .spacing(2)
    .into()
}

fn tool_row(
    label: &str,
    icon: Element<'static, Message>,
    message: Option<Message>,
    description: &str,
) -> Element<'static, Message> {
    let enabled = message.is_some();
    let entry = button(
        row![icon, text(crate::t!(label).into_owned()).size(12)]
            .spacing(8)
            .align_y(iced::Center),
    )
    .on_press_maybe(message)
    .padding([4, 6])
    .height(40)
    .width(Fill)
    .style(move |theme: &Theme, status| {
        if enabled {
            button::text(theme, status)
        } else {
            button::Style {
                text_color: theme.palette().background.base.text.scale_alpha(0.55),
                ..Default::default()
            }
        }
    });
    tooltip(
        entry,
        tooltip_content(format!("{}\n{}", crate::t!(label), description)),
        tooltip::Position::Right,
    )
    .gap(6.0)
    .delay(std::time::Duration::from_millis(400))
    .style(tooltip_style)
    .into()
}

fn constraint_row(tool: ToolDef) -> Element<'static, Message> {
    let icon = match tool.icon {
        IconKind::Svg(bytes) => icons::semantic(bytes, 28.0),
        IconKind::Glyph(glyph) => text(glyph).size(28).into(),
    };
    let label = if tool.id == "LCONSTRAINT" {
        "Collinear"
    } else {
        tool.label
    };
    let description = match &tool.event {
        crate::modules::ModuleEvent::Command(command) => command.clone(),
        _ => tool.id.to_owned(),
    };
    container(tool_row(
        label,
        icon,
        Some(Message::RibbonToolClick {
            tool_id: tool.id.into(),
            event: tool.event,
        }),
        &description,
    ))
    .id(format!("authoring-tool-{}", tool.id))
    .into()
}

fn constraints() -> Element<'static, Message> {
    // Reuse the ribbon definitions, including their command events and icons.
    let geometric = [
        parametric::coincident_tool::tool(),
        parametric::perpendicular::tool(),
        parametric::parallel::tool(),
        parametric::tangent::tool(),
        parametric::horizontal::tool(),
        parametric::vertical::tool(),
        parametric::colinear::tool(),
        parametric::concentric_tool::tool(),
        parametric::smooth_tool(),
        parametric::symmetric::tool(),
        parametric::equal::tool(),
        parametric::fixed::tool(),
    ];
    let dimensional = [
        parametric::dimensional_tools::aligned(),
        parametric::dimensional_tools::horizontal(),
        parametric::dimensional_tools::vertical(),
        parametric::dimensional_tools::angular(),
        parametric::dimensional_tools::radius(),
        parametric::dimensional_tools::diameter(),
    ];
    column![
        container(text(crate::t!("Geometric Constraints")).size(11)).padding([10, 6]),
        column(geometric.into_iter().map(constraint_row)).spacing(2),
        container(rule::horizontal(1)).padding([8, 6]),
        container(text(crate::t!("Constraint Parameters")).size(11)).padding([10, 6]),
        column(dimensional.into_iter().map(constraint_row)).spacing(2),
    ]
    .into()
}

#[allow(clippy::too_many_arguments)]
pub fn view(
    selected_tab: Tab,
    width: f32,
    chrome: Chrome,
    visibility_error: Option<&'static str>,
    point_error: Option<&'static str>,
    flip_error: Option<&'static str>,
    lookup_error: Option<&'static str>,
    properties_table_error: Option<&'static str>,
    move_error: Option<&'static str>,
    scale_error: Option<&'static str>,
    rotate_error: Option<&'static str>,
    flip_action_error: Option<&'static str>,
    stretch_action_error: Option<&'static str>,
    polar_stretch_action_error: Option<&'static str>,
    array_action_error: Option<&'static str>,
    parameter_set_error: Option<&'static str>,
) -> Element<'static, Message> {
    let tabs = column(Tab::ALL.into_iter().map(|category| {
        let label = category.label();
        let selected = category == selected_tab;
        container(
            button(
                canvas(VerticalLabel {
                    text: crate::t!(label).into_owned(),
                    centered: false,
                })
                .width(16)
                .height((label.len() as f32 * 7.0) + 8.0),
            )
            .on_press(Message::AuthoringPaletteTabSelected(category))
            .padding([6, 4])
            .style(move |theme: &Theme, status| {
                let palette = theme.palette();
                button::Style {
                    background: Some(Background::Color(
                        if selected || status == button::Status::Hovered {
                            palette.background.base.color
                        } else {
                            palette.background.weak.color
                        },
                    )),
                    border: Border {
                        color: if selected {
                            palette.primary.base.color
                        } else {
                            palette.background.neutral.color
                        },
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                }
            }),
        )
        .id(category.widget_id())
        .into()
    }))
    .spacing(2)
    .push(Space::new().height(Fill));
    dock::frame(
        dock::title_bar(
            PanelId::AuthoringPalettes,
            crate::t!("Authoring Palettes").into_owned(),
            chrome,
        ),
        row![
            scrollable(catalog(
                selected_tab,
                visibility_error,
                point_error,
                flip_error,
                lookup_error,
                properties_table_error,
                move_error,
                scale_error,
                rotate_error,
                flip_action_error,
                stretch_action_error,
                polar_stretch_action_error,
                array_action_error,
                parameter_set_error,
            ))
            .width(Fill)
            .height(Fill),
            tabs
        ]
        .spacing(4)
        .height(Fill),
        width,
    )
}
