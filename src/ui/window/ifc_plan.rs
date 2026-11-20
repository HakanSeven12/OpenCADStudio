use std::sync::Arc;

use iced::widget::{button, column, row, text, text_input, Space};
use iced::{Element, Fill};

use crate::app::Message;
use crate::ifc_plan::PlanModel;
use crate::ui::dropdown_list::pick_list;
use crate::ui::style::form::{dialog_button, dialog_button_styled_opt, field_style};
use crate::ui::window::import_dialogs::{ImportDialogMsg, ReadTicket};

#[derive(Debug, Clone)]
pub enum IfcPlanMsg {
    Browse,
    Picked(ReadTicket, Option<String>),
    Loaded(ReadTicket, Result<Arc<PlanModel>, String>),
    Storey(StoreyChoice),
    Height(String),
    Depth(String),
    Import,
    Generated(ReadTicket, Result<Arc<Vec<document::EntityType>>, String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreyChoice {
    pub index: usize,
    pub label: String,
}

/// The last successful import in this Studio session.
pub(crate) struct PreviousIfcPlan {
    filename: String,
    model: Arc<PlanModel>,
    selected: StoreyChoice,
    height: String,
    depth: String,
}

pub(crate) struct IfcPlanDialog {
    pub filename: String,
    pub model: Option<Arc<PlanModel>>,
    pub choices: Vec<StoreyChoice>,
    pub selected: Option<StoreyChoice>,
    pub height: String,
    pub depth: String,
    pub pending: bool,
    pub error: Option<String>,
}

impl IfcPlanDialog {
    pub fn new(filename: String) -> Self {
        Self {
            filename,
            model: None,
            choices: Vec::new(),
            selected: None,
            height: "1.2".into(),
            depth: "1.5".into(),
            pending: true,
            error: None,
        }
    }

    pub fn from_previous(previous: &PreviousIfcPlan) -> Self {
        let mut dialog = Self::new(previous.filename.clone());
        dialog.set_model(previous.model.clone());
        dialog.selected = Some(previous.selected.clone());
        dialog.height = previous.height.clone();
        dialog.depth = previous.depth.clone();
        dialog.pending = false;
        dialog
    }

    pub fn set_model(&mut self, model: Arc<PlanModel>) {
        self.choices = model
            .storeys
            .iter()
            .enumerate()
            .map(|(index, storey)| StoreyChoice {
                index,
                label: format!("{} ({:.2} m)", storey.name, storey.elevation),
            })
            .collect();
        if self.choices.is_empty() {
            self.choices.push(StoreyChoice {
                index: 0,
                label: crate::tr!("ifc2plan", "origin"),
            });
        }
        self.selected = match self.choices.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        };
        self.model = Some(model);
    }

    pub fn previous(&self) -> Option<PreviousIfcPlan> {
        Some(PreviousIfcPlan {
            filename: self.filename.clone(),
            model: self.model.clone()?,
            selected: self.selected.clone()?,
            height: self.height.clone(),
            depth: self.depth.clone(),
        })
    }

    pub fn elevations(&self) -> Option<(f64, f64)> {
        let model = self.model.as_ref()?;
        let offset: f64 = self.height.replace(',', ".").parse().ok()?;
        let depth: f64 = self.depth.replace(',', ".").parse().ok()?;
        let choice = self.selected.as_ref()?;
        let base = if model.storeys.is_empty() {
            0.0
        } else {
            model.storeys.get(choice.index)?.elevation
        };
        let elevation = base + offset;
        (elevation.is_finite() && depth.is_finite() && depth > 0.0).then_some((elevation, depth))
    }
}

fn msg(message: IfcPlanMsg) -> Message {
    Message::ImportDialog(ImportDialogMsg::IfcPlan(message))
}

pub(crate) fn view(
    state: &IfcPlanDialog,
    sizing: crate::ui::modal::ModalSizing,
) -> Element<'_, Message> {
    let label = |title| text(title).size(12);
    let mut controls = column![row![
        text(&state.filename).size(13).width(Fill),
        dialog_button_styled_opt(
            crate::tr!("ifc2plan", "choose-file"),
            (!state.pending).then(|| msg(IfcPlanMsg::Browse)),
            button::secondary,
        ),
    ]
    .align_y(iced::Center)
    .spacing(8)]
    .spacing(10);

    if state.model.is_some() {
        let storeys = pick_list(
            state.selected.as_ref(),
            state.choices.as_slice(),
            |choice: &StoreyChoice| choice.label.clone(),
        )
        .placeholder(crate::tr!("ifc2plan", "choose-storey"))
        .width(Fill);
        let storeys = if state.pending {
            storeys
        } else {
            storeys.on_select(|choice| msg(IfcPlanMsg::Storey(choice)))
        };
        let height = text_input("1.2", &state.height)
            .on_input_maybe((!state.pending).then_some(|s| msg(IfcPlanMsg::Height(s))))
            .style(field_style)
            .padding([6, 8]);
        let depth = text_input("1.5", &state.depth)
            .on_input_maybe((!state.pending).then_some(|s| msg(IfcPlanMsg::Depth(s))))
            .style(field_style)
            .padding([6, 8]);
        controls = controls
            .push(column![label(crate::tr!("ifc2plan", "storey")), storeys].spacing(4))
            .push(
                row![
                    column![label(crate::tr!("ifc2plan", "height")), height]
                        .width(Fill)
                        .spacing(4),
                    column![label(crate::tr!("ifc2plan", "depth")), depth]
                        .width(Fill)
                        .spacing(4)
                ]
                .spacing(12),
            );
    }
    if state.pending {
        controls = controls.push(label(if state.model.is_some() {
            crate::tr!("ifc2plan", "generating")
        } else {
            crate::tr!("ifc2plan", "reading")
        }));
    }
    if let Some(error) = &state.error {
        controls = controls.push(
            text(error)
                .size(12)
                .style(|theme: &iced::Theme| text::Style {
                    color: Some(theme.palette().danger.base.color),
                }),
        );
    }
    let ready = !state.pending && state.elevations().is_some();
    controls
        .push(
            row![
                Space::new().width(Fill),
                dialog_button(crate::t!("Cancel"), Message::CloseModal, false),
                dialog_button_styled_opt(
                    crate::tr!("ifc2plan", "create"),
                    ready.then(|| msg(IfcPlanMsg::Import)),
                    button::primary,
                ),
            ]
            .spacing(8),
        )
        .padding(16)
        .width(sizing.width)
        .into()
}
