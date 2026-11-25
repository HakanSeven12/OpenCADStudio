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
    #[cfg(not(target_arch = "wasm32"))]
    XrefGenerated(
        ReadTicket,
        Result<Arc<crate::io::xref::ifc_slice::Ready>, String>,
    ),
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

#[derive(Clone)]
pub(crate) struct IfcXrefTarget {
    pub path: std::path::PathBuf,
    pub tab_id: u64,
    pub document_generation: u64,
}

pub(crate) struct IfcPlanDialog {
    pub filename: String,
    pub xref: Option<IfcXrefTarget>,
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
            xref: None,
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
        let offset = self.cut_offset()?;
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

    pub fn cut_offset(&self) -> Option<f64> {
        self.height.replace(',', ".").parse().ok()
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
    let editable = !state.pending && state.model.is_some();
    let storeys = pick_list(
        state.selected.as_ref(),
        state.choices.as_slice(),
        |choice: &StoreyChoice| choice.label.clone(),
    )
    .placeholder(crate::tr!("ifc2plan", "choose-storey"))
    .width(Fill);
    let storeys = if editable {
        storeys.on_select(|choice| msg(IfcPlanMsg::Storey(choice)))
    } else {
        storeys
    };
    let height = text_input("1.2", &state.height)
        .on_input_maybe(editable.then_some(|s| msg(IfcPlanMsg::Height(s))))
        .style(field_style)
        .padding([6, 8]);
    let depth = text_input("1.5", &state.depth)
        .on_input_maybe(editable.then_some(|s| msg(IfcPlanMsg::Depth(s))))
        .style(field_style)
        .padding([6, 8]);

    // A detached window is sized before the IFC load completes.
    let mut controls = column![
        row![
            text(&state.filename).size(13).width(Fill),
            dialog_button_styled_opt(
                crate::tr!("ifc2plan", "choose-file"),
                (!state.pending).then(|| msg(IfcPlanMsg::Browse)),
                button::secondary,
            ),
        ]
        .align_y(iced::Center)
        .spacing(8),
        column![label(crate::tr!("ifc2plan", "storey")), storeys].spacing(4),
        row![
            column![label(crate::tr!("ifc2plan", "height")), height]
                .width(Fill)
                .spacing(4),
            column![label(crate::tr!("ifc2plan", "depth")), depth]
                .width(Fill)
                .spacing(4)
        ]
        .spacing(12),
    ]
    .spacing(10);

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
                    if state.xref.is_some() {
                        crate::t!("Attach").into_owned()
                    } else {
                        crate::tr!("ifc2plan", "create")
                    },
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::ui::modal::ModalSizing;
    use iced_test::simulator::Simulator;

    #[test]
    fn ifc_plan_loaded_controls_fit_at_the_loading_window_height() {
        let model =
            Arc::new(crate::ifc_plan::load_model(crate::ifc_plan::TWO_WALLS_FIXTURE).unwrap());
        let cancel = crate::t!("Cancel").into_owned();

        for xref in [false, true] {
            let mut state = IfcPlanDialog::new("two-walls.ifc".into());
            state.xref = xref.then(|| IfcXrefTarget {
                path: state.filename.clone().into(),
                tab_id: 0,
                document_generation: 0,
            });

            let loading_height = {
                let mut loading = Simulator::with_size(
                    iced::Settings::default(),
                    (430.0, 285.0),
                    view(&state, ModalSizing::INTRINSIC),
                );
                let footer = loading.find(cancel.as_str()).unwrap().bounds();
                // The detached window includes the area below the footer label.
                footer.y + footer.height + 16.0
            };

            state.set_model(model.clone());
            state.selected = state.choices.first().cloned();
            state.pending = false;
            let mut loaded = Simulator::with_size(
                iced::Settings::default(),
                (430.0, loading_height),
                view(&state, ModalSizing::FILL),
            );
            for label in [
                crate::tr!("ifc2plan", "storey"),
                crate::tr!("ifc2plan", "height"),
                crate::tr!("ifc2plan", "depth"),
            ] {
                let target = loaded.find(label.as_str()).unwrap();
                let visible = target.visible_bounds().expect("field must be visible");
                let bounds = target.bounds();
                assert!(
                    visible.height + 0.01 >= bounds.height && visible.width + 0.01 >= bounds.width,
                    "{label} is clipped: {visible:?} vs {bounds:?}"
                );
            }
            let primary = if xref {
                crate::t!("Attach").into_owned()
            } else {
                crate::tr!("ifc2plan", "create")
            };
            loaded.click(primary.as_str()).unwrap();
            assert!(loaded.into_messages().any(|message| matches!(
                message,
                Message::ImportDialog(ImportDialogMsg::IfcPlan(IfcPlanMsg::Import))
            )));
        }
    }
}
