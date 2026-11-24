//! The Visibility States palette (VISIBILITYSTATESPANELOPEN): the visibility
//! parameters of a BricsCAD parametric block in model space of the block's
//! own drawing, edited the way `VISIBILITYSTATES` edits them.
//!
//! The overview lists each parameter with its states; a click on a state
//! shows it, the arrow opens the state's page. A state's page lists every
//! object the parameter rules with whether it shows in that state, and adds
//! or removes objects. Every change goes through the command's own edit
//! (`scene::bricscad_parameters::author`), one undo step each.

use crate::app::Message;
use crate::scene::bricscad_parameters::ParameterView;
use crate::ui::dock::PanelId;
use crate::ui::kit;
use crate::ui::kit::field_style;
use crate::ui::style::kit_metrics as km;
use document::Handle;
use iced::widget::{column, container, row, text_input, Space};
use iced::{Element, Fill, Length, Padding};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

const ADD_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/plus.svg");
const OPEN_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/tri_right.svg");
const BACK_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/tri_left.svg");
const UP_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/tri_up.svg");
const DOWN_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/tri_down.svg");
const SHOWN_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/eye.svg");
const HIDDEN_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/eye_off.svg");
const REMOVE_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/close.svg");
const PICK_ICON: &[u8] = include_bytes!("../../../assets/icons/blocks/select_objects.svg");
const WARNING_ICON: &[u8] = include_bytes!("../../../assets/icons/ui/warning_triangle.svg");

/// The palette's own state; the parameters come from the drawing.
#[derive(Debug, Clone, Default)]
pub struct VisibilityPanel {
    pub show: bool,
    /// The state whose page is open: parameter and state.
    pub page: Option<(String, String)>,
    /// A parameter being added: its name and its first state.
    pub new_parameter: Option<(String, String)>,
    /// A state being added: the parameter and the name typed.
    pub new_state: Option<(String, String)>,
    /// Folded parameters (names in upper case).
    pub folded: Vec<String>,
    /// What the drawing holds, read at (document id, edit revision, language).
    pub cache: RefCell<Option<((u64, u64, u64), Rc<Model>)>>,
}

/// A drawing's visibility parameters as the palette shows them.
#[derive(Debug, Default)]
pub struct Model {
    /// `None` when they cannot be edited here.
    pub parameters: Option<Vec<ParameterView>>,
    /// Each ruled object's line: type, layer when shown, handle.
    pub labels: FxHashMap<Handle, String>,
}

#[derive(Debug, Clone)]
pub enum VisMsg {
    /// Show the state (parameter, state).
    Select(String, String),
    /// Open the state's page.
    Open(String, String),
    /// Move the state one place up (`true`) or down in the parameter's list.
    Move(String, String, bool),
    Back,
    Fold(String),
    NewParameter,
    NewParameterName(String),
    NewParameterState(String),
    /// Pick the objects the new parameter starts with.
    NewParameterPick,
    NewState(String),
    NewStateName(String),
    NewStateCommit,
    Cancel,
    /// Show or hide an object in the open state.
    Show(Handle, bool),
    /// The parameter no longer rules the object.
    Remove(Handle),
    /// Pick objects for the open state.
    AddObjects,
    /// Select the object in the drawing.
    Highlight(Handle),
}

fn msg(m: VisMsg) -> Message {
    Message::BricscadVisibility(m)
}

fn icon<'a>(bytes: &'static [u8]) -> Element<'a, Message> {
    crate::ui::icons::semantic(bytes, km::ROW_ICON)
}

/// Read what `document`'s model space holds for the palette.
pub fn read(document: &document::Document) -> Model {
    let owner = document.header.model_space_block_handle;
    let parameters = crate::scene::bricscad_parameters::parameter_views(document, owner);
    let layers = crate::scene::bricscad_parameters::shown_layers(document, owner);
    let mut labels = FxHashMap::default();
    for parameter in parameters.iter().flatten() {
        for (entity, _) in &parameter.objects {
            labels.entry(*entity).or_insert_with(|| {
                let kind = document
                    .get_entity(*entity)
                    .map(|e| crate::t!(crate::entities::names::ui_name(e)).into_owned())
                    .unwrap_or_default();
                let layer = layers.get(entity).cloned().unwrap_or_default();
                format!("{kind} · {layer} · {:X}", entity.value())
            });
        }
    }
    Model { parameters, labels }
}

/// `None` outside model space.
pub fn view<'a>(
    panel: &'a VisibilityPanel,
    model: Option<Rc<Model>>,
    width: f32,
    chrome: crate::ui::dock::Chrome,
) -> Element<'a, Message> {
    let title_bar = crate::ui::dock::title_bar(
        PanelId::BricscadVisibility,
        crate::t!("Visibility States").into_owned(),
        chrome,
    );
    let body = match model.as_deref() {
        None => kit::empty_state(crate::t!(
            "Visibility states are edited in model space of the block's own drawing."
        )),
        Some(Model {
            parameters: None, ..
        }) => kit::empty_state(crate::t!(
            "The visibility states of this drawing cannot be edited here."
        )),
        Some(Model {
            parameters: Some(parameters),
            labels,
        }) => {
            let page = panel.page.as_ref().and_then(|(parameter, state)| {
                let p = parameters
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(parameter))?;
                let s = p
                    .states
                    .iter()
                    .position(|x| x.eq_ignore_ascii_case(state))?;
                Some((p, s))
            });
            match page {
                Some((parameter, s)) => state_page(parameter, s, labels),
                None => overview(panel, parameters),
            }
        }
    };
    kit::palette(title_bar, body, width)
}

fn name_field<'a>(
    placeholder: String,
    value: &str,
    on_input: impl Fn(String) -> VisMsg + 'a,
    on_submit: Option<VisMsg>,
) -> Element<'a, Message> {
    let mut field = text_input(&placeholder, value)
        .size(crate::ui::style::text::UI)
        .padding([0.0, km::CELL_PAD_X])
        .style(field_style)
        .on_input(move |v| msg(on_input(v)));
    if let Some(m) = on_submit {
        field = field.on_submit(msg(m));
    }
    field.into()
}

fn overview<'a>(panel: &'a VisibilityPanel, parameters: &[ParameterView]) -> Element<'a, Message> {
    let top = kit::band(
        row![
            kit::tool_button(
                icon(ADD_ICON),
                crate::t!("Add parameter").into_owned(),
                Some(msg(VisMsg::NewParameter)),
                panel.new_parameter.is_some(),
            ),
            Space::new().width(Fill),
        ]
        .spacing(km::TOOL_GAP)
        .align_y(iced::Center),
    );
    let mut list = column![];
    if let Some((name, state)) = &panel.new_parameter {
        list = list.push(
            container(
                column![
                    name_field(
                        crate::t!("Parameter name").into_owned(),
                        name,
                        VisMsg::NewParameterName,
                        None,
                    ),
                    name_field(
                        crate::t!("First state").into_owned(),
                        state,
                        VisMsg::NewParameterState,
                        Some(VisMsg::NewParameterPick),
                    ),
                    row![
                        Space::new().width(Fill),
                        kit::text_button(
                            crate::t!("Cancel").into_owned(),
                            Some(msg(VisMsg::Cancel)),
                            false,
                        ),
                        kit::text_button(
                            crate::t!("Select objects").into_owned(),
                            (!name.trim().is_empty() && !state.trim().is_empty())
                                .then(|| msg(VisMsg::NewParameterPick)),
                            true,
                        ),
                    ]
                    .spacing(km::TOOL_GAP)
                    .align_y(iced::Center),
                ]
                .spacing(km::TOOL_GAP),
            )
            .padding(Padding {
                top: km::TOOL_GAP,
                bottom: km::TOOL_GAP,
                left: km::GRID_INSET_L,
                right: km::GRID_INSET_R,
            }),
        );
    }
    if parameters.is_empty() && panel.new_parameter.is_none() {
        list = list.push(kit::empty_state(crate::t!(
            "There are no visibility states in this drawing. Click + to add a parameter."
        )));
    }
    for parameter in parameters {
        let key = parameter.name.to_uppercase();
        let folded = panel.folded.contains(&key);
        list = list.push(kit::category_bar(
            crate::tf!("{} ({} objects)", parameter.name, parameter.objects.len()).into_owned(),
            folded,
            msg(VisMsg::Fold(parameter.name.clone())),
        ));
        if folded {
            continue;
        }
        for (s, state) in parameter.states.iter().enumerate() {
            let current = s == parameter.current;
            let step = |bytes: &'static [u8], up: bool, possible: bool, tip: String| {
                kit::tooltip(
                    kit::toggle_cell(
                        icon(bytes),
                        possible
                            .then(|| msg(VisMsg::Move(parameter.name.clone(), state.clone(), up))),
                    ),
                    tip,
                    kit::TipPosition::Bottom,
                )
            };
            let open = row![
                step(UP_ICON, true, s > 0, crate::t!("Move up").into_owned()),
                step(
                    DOWN_ICON,
                    false,
                    s + 1 < parameter.states.len(),
                    crate::t!("Move down").into_owned()
                ),
                kit::tooltip(
                    kit::toggle_cell(
                        icon(OPEN_ICON),
                        Some(msg(VisMsg::Open(parameter.name.clone(), state.clone()))),
                    ),
                    crate::t!("Objects of this state").into_owned(),
                    kit::TipPosition::Bottom,
                ),
            ]
            .align_y(iced::Center);
            list = list.push(
                kit::tree_row(state.clone())
                    .icon(kit::menu::dot(
                        km::CHECK / 2.0,
                        if current {
                            crate::ui::style::chrome::accent
                        } else {
                            crate::ui::style::chrome::surface_raised
                        },
                    ))
                    .selected(current)
                    .trailing(open)
                    .on_press(msg(VisMsg::Select(parameter.name.clone(), state.clone())))
                    .on_double_click(msg(VisMsg::Open(parameter.name.clone(), state.clone())))
                    .view(),
            );
        }
        match &panel.new_state {
            Some((p, name)) if p.eq_ignore_ascii_case(&parameter.name) => {
                list = list.push(
                    container(
                        row![
                            name_field(
                                crate::t!("New state").into_owned(),
                                name,
                                VisMsg::NewStateName,
                                Some(VisMsg::NewStateCommit),
                            ),
                            kit::text_button(
                                crate::t!("Cancel").into_owned(),
                                Some(msg(VisMsg::Cancel)),
                                false,
                            ),
                            kit::text_button(
                                crate::t!("Add").into_owned(),
                                (!name.trim().is_empty()).then(|| msg(VisMsg::NewStateCommit)),
                                true,
                            ),
                        ]
                        .spacing(km::TOOL_GAP)
                        .align_y(iced::Center),
                    )
                    .padding(Padding::ZERO.left(km::GRID_INSET_L).right(km::GRID_INSET_R)),
                );
            }
            _ => {
                list = list.push(
                    kit::tree_row(crate::t!("Add state").into_owned())
                        .icon(icon(ADD_ICON))
                        .muted(true)
                        .on_press(msg(VisMsg::NewState(parameter.name.clone())))
                        .view(),
                );
            }
        }
    }
    column![top, container(kit::scroll_area(list)).height(Fill)].into()
}

fn state_page<'a>(
    parameter: &ParameterView,
    s: usize,
    labels: &FxHashMap<Handle, String>,
) -> Element<'a, Message> {
    let state = &parameter.states[s];
    let current = s == parameter.current;
    let top = kit::band(
        row![
            kit::tool_button(
                icon(BACK_ICON),
                crate::t!("Back to the parameters").into_owned(),
                Some(msg(VisMsg::Back)),
                false,
            ),
            container(kit::value_text(
                format!("{} › {}", parameter.name, state),
                false
            ))
            .width(Fill),
            kit::text_button(
                crate::t!("Show").into_owned(),
                (!current).then(|| msg(VisMsg::Select(parameter.name.clone(), state.clone()))),
                false,
            ),
            kit::tool_button(
                icon(PICK_ICON),
                crate::t!("Add objects to this state").into_owned(),
                Some(msg(VisMsg::AddObjects)),
                false,
            ),
        ]
        .spacing(km::TOOL_GAP)
        .align_y(iced::Center),
    );
    let mut list = column![];
    if parameter.objects.is_empty() {
        list = list.push(kit::empty_state(crate::t!(
            "The parameter rules no objects yet. Add objects to this state."
        )));
    }
    let mut constant = false;
    for (entity, pattern) in &parameter.objects {
        let shown = pattern.get(s).copied().unwrap_or(false);
        let same_everywhere = pattern.iter().all(|v| *v == pattern[0]);
        constant |= same_everywhere;
        let label = labels.get(entity).cloned().unwrap_or_default();
        let toggle = kit::toggle_cell(
            icon(if shown { SHOWN_ICON } else { HIDDEN_ICON }),
            Some(msg(VisMsg::Show(*entity, !shown))),
        );
        let remove = kit::tooltip(
            kit::toggle_cell(icon(REMOVE_ICON), Some(msg(VisMsg::Remove(*entity)))),
            crate::t!("Remove from parameter").into_owned(),
            kit::TipPosition::Bottom,
        );
        let mut cells: Vec<(Element<'a, Message>, Length)> = vec![
            (toggle, Length::Shrink),
            (kit::text_cell(label, !shown), Length::Fill),
        ];
        if same_everywhere {
            cells.push((
                kit::tooltip(
                    icon(WARNING_ICON),
                    crate::t!("Shown the same in every state of the parameter").into_owned(),
                    kit::TipPosition::Bottom,
                ),
                Length::Shrink,
            ));
        }
        cells.push((remove, Length::Shrink));
        list = list.push(
            kit::list_row(cells)
                .on_press(msg(VisMsg::Highlight(*entity)))
                .view(),
        );
    }
    let footer = constant.then(|| {
        kit::footer_text(crate::t!(
            "One or more objects show the same in every state of this parameter."
        ))
    });
    let mut page = column![top, container(kit::scroll_area(list)).height(Fill)];
    if let Some(footer) = footer {
        page = page.push(footer);
    }
    page.into()
}
