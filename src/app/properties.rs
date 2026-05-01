use super::{H7CAD, VARIES_LABEL};
use super::helpers::{entity_type_label, entity_type_key, title_case_word};
use crate::scene::dispatch;
use crate::ui;
use crate::linetypes;
use acadrust::{EntityType, Handle};

impl H7CAD {
    /// Rebuild the PropertiesPanel from the current entity selection.
    /// Preserves UI state (open pickers, edit buffer) across refreshes.
    pub(super) fn refresh_properties(&mut self) {
        let i = self.active_tab;
        let color_picker_open = self.tabs[i].properties.color_picker_open;
        let color_palette_open = self.tabs[i].properties.color_palette_open;
        let edit_buf = std::mem::take(&mut self.tabs[i].properties.edit_buf);
        let selected_group = self.tabs[i].properties.selected_group.clone();

        let layer_names: Vec<String> = self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect();
        let linetype_items: Vec<ui::properties::LinetypeItem> = self.tabs[i]
            .scene
            .document
            .line_types
            .iter()
            .map(|lt| {
                let name = if lt.name.is_empty() {
                    "ByLayer".to_string()
                } else {
                    lt.name.clone()
                };
                let art = linetypes::extract_pattern(&lt.description);
                ui::properties::LinetypeItem { name, art }
            })
            .collect();
        let text_style_names: Vec<String> = self.tabs[i]
            .scene
            .document
            .text_styles
            .iter()
            .map(|style| style.name.trim().to_string())
            .filter(|name| !name.is_empty())
            .collect();

        let new_panel = {
            let selected = self.tabs[i].scene.selected_entities();
            let mut panel = match selected.len() {
                0 => ui::PropertiesPanel::empty(),
                1 => {
                    let (handle, entity) = selected[0];
                    let group_names = self.tabs[i].scene.group_names_for_entity(handle);
                    let mut sections =
                        dispatch::properties_sectioned(handle, entity, &text_style_names);

                    // Inject viewport-only properties that require doc access.
                    if let acadrust::EntityType::Viewport(vp) = entity {
                        let frozen_names: Vec<String> = vp
                            .frozen_layers
                            .iter()
                            .filter_map(|&h| {
                                self.tabs[i].scene.document.layers.iter()
                                    .find(|l| l.handle == h)
                                    .map(|l| l.name.clone())
                            })
                            .collect();

                        // Collect available UCS names for the name picker.
                        let ucs_names: Vec<String> = self.tabs[i].scene.document.ucss
                            .iter()
                            .map(|u| u.name.clone())
                            .filter(|n| !n.is_empty())
                            .collect();

                        // Current UCS name (resolved from vp.ucs_handle).
                        let current_ucs = self.tabs[i].scene.document.ucss
                            .iter()
                            .find(|u| u.handle == vp.ucs_handle)
                            .map(|u| u.name.clone())
                            .unwrap_or_default();

                        // Collect available named view names.
                        let view_names: Vec<String> = self.tabs[i].scene.document.views
                            .iter()
                            .map(|v| v.name.clone())
                            .filter(|n| !n.is_empty())
                            .collect();

                        if let Some(geom) = sections.last_mut() {
                            geom.props.push(crate::scene::object::Property {
                                label: "Frozen Layers".to_string(),
                                field: "frozen_layers",
                                value: crate::scene::object::PropValue::EditText(
                                    frozen_names.join(", "),
                                ),
                            });
                            if !ucs_names.is_empty() {
                                geom.props.push(crate::scene::object::Property {
                                    label: "UCS Name".to_string(),
                                    field: "vp_ucs_name",
                                    value: crate::scene::object::PropValue::Choice {
                                        selected: current_ucs,
                                        options: ucs_names,
                                    },
                                });
                            }
                            if !view_names.is_empty() {
                                geom.props.push(crate::scene::object::Property {
                                    label: "Named View".to_string(),
                                    field: "vp_named_view",
                                    value: crate::scene::object::PropValue::Choice {
                                        selected: String::new(),
                                        options: view_names,
                                    },
                                });
                            }
                        }
                    }

                    // Inject DimStyle picker for Dimension entities.
                    if let acadrust::EntityType::Dimension(_) = entity {
                        let dim_style_names: Vec<String> = self.tabs[i].scene.document.dim_styles
                            .iter()
                            .map(|s| s.name.clone())
                            .filter(|n| !n.is_empty())
                            .collect();
                        if !dim_style_names.is_empty() {
                            // Current style is already shown as EditText in the geom section;
                            // replace/upgrade it to a Choice if we have a list.
                            if let Some(geom) = sections.last_mut() {
                                // Find and replace the style_name EditText with a Choice.
                                if let Some(prop) = geom.props.iter_mut().find(|p| p.field == "style_name") {
                                    let current = match &prop.value {
                                        crate::scene::object::PropValue::EditText(s) => s.clone(),
                                        _ => String::new(),
                                    };
                                    prop.value = crate::scene::object::PropValue::Choice {
                                        selected: current,
                                        options: dim_style_names,
                                    };
                                }
                            }
                        }
                    }

                    if !group_names.is_empty() {
                        let label = group_names.join(", ");
                        if let Some(general) = sections.first_mut() {
                            general.props.push(crate::scene::object::Property {
                                label: "Group".to_string(),
                                field: "group",
                                value: crate::scene::object::PropValue::ReadOnly(label),
                            });
                        }
                    }
                    let title = entity_type_label(entity);
                    ui::PropertiesPanel {
                        choice_combos: sections
                            .iter()
                            .flat_map(|section| section.props.iter())
                            .filter_map(|prop| match &prop.value {
                                crate::scene::object::PropValue::Choice { options, .. } => Some((
                                    prop.field.to_string(),
                                    iced::widget::combo_box::State::new(options.clone()),
                                )),
                                _ => None,
                            })
                            .collect(),
                        sections,
                        title,
                        layer_combo: iced::widget::combo_box::State::new(layer_names.clone()),
                        linetype_combo: iced::widget::combo_box::State::new(linetype_items.clone()),
                        hatch_pattern_combo: iced::widget::combo_box::State::new(
                            crate::scene::hatch_patterns::names(),
                        ),
                        lineweight_combo: iced::widget::combo_box::State::new(
                            ui::properties::lw_options(),
                        ),
                        linetype_items,
                        ..Default::default()
                    }
                }
                _ => {
                    let groups = build_selection_groups(&selected);
                    let active_group = selected_group
                        .and_then(|group| groups.iter().find(|g| g.label == group.label).cloned())
                        .or_else(|| groups.first().cloned());

                    let filtered: Vec<(Handle, &EntityType)> = active_group
                        .as_ref()
                        .map(|group| {
                            selected
                                .iter()
                                .filter(|(handle, _)| group.handles.contains(handle))
                                .copied()
                                .collect()
                        })
                        .unwrap_or_default();

                    let sections = aggregate_sections(&filtered, &text_style_names);
                    ui::PropertiesPanel {
                        choice_combos: sections
                            .iter()
                            .flat_map(|section| section.props.iter())
                            .filter_map(|prop| match &prop.value {
                                crate::scene::object::PropValue::Choice { options, .. } => Some((
                                    prop.field.to_string(),
                                    iced::widget::combo_box::State::new(options.clone()),
                                )),
                                _ => None,
                            })
                            .collect(),
                        sections,
                        title: format!("{} objects selected", selected.len()),
                        selection_group_combo: iced::widget::combo_box::State::new(groups.clone()),
                        selection_groups: groups,
                        selected_group: active_group,
                        layer_combo: iced::widget::combo_box::State::new(layer_names.clone()),
                        linetype_combo: iced::widget::combo_box::State::new(linetype_items.clone()),
                        hatch_pattern_combo: iced::widget::combo_box::State::new(
                            crate::scene::hatch_patterns::names(),
                        ),
                        lineweight_combo: iced::widget::combo_box::State::new(
                            ui::properties::lw_options(),
                        ),
                        linetype_items,
                        ..Default::default()
                    }
                }
            };
            panel.color_picker_open = color_picker_open;
            panel.color_palette_open = color_palette_open;
            panel.edit_buf = edit_buf;
            panel
        };

        self.tabs[i].properties = new_panel;
        self.refresh_selected_grips();
    }

    /// Rebuild the cached selected_grips from the current entity selection.
    pub(super) fn refresh_selected_grips(&mut self) {
        let i = self.active_tab;
        let is_paper = self.tabs[i].scene.current_layout != "Model";
        // Paper-space entity coordinates are NOT offset by world_offset (same rule
        // as wire tessellation in wires_for_block). Only subtract in model space.
        let wo = if is_paper { [0.0f64; 3] } else { self.tabs[i].scene.world_offset };
        let (new_handle, new_grips) = {
            let selected = self.tabs[i].scene.selected_entities();
            if selected.len() == 1 {
                let (handle, entity) = selected[0];
                let grips = dispatch::grips(entity)
                    .into_iter()
                    .map(|mut g| {
                        g.world.x -= wo[0] as f32;
                        g.world.y -= wo[1] as f32;
                        g.world.z -= wo[2] as f32;
                        g
                    })
                    .collect();
                (Some(handle), grips)
            } else {
                (None, vec![])
            }
        };
        self.tabs[i].selected_handle = new_handle;
        self.tabs[i].selected_grips = new_grips;
    }

    pub(super) fn property_target_handles(&self, i: usize) -> Vec<Handle> {
        let handles = self.tabs[i].properties.selected_handles();
        if !handles.is_empty() {
            handles
        } else {
            self.tabs[i].selected_handle.into_iter().collect()
        }
    }

    /// Add an entity to the correct space (model or paper space layout).
    pub(super) fn commit_entity(&mut self, mut entity: acadrust::EntityType) {
        let i = self.active_tab;
        let layer = &self.tabs[i].active_layer;
        if layer != "0" || entity.as_entity().layer().is_empty() {
            entity.as_entity_mut().set_layer(layer.clone());
        }

        crate::scene::dispatch::apply_color(&mut entity, self.ribbon.active_color);
        crate::scene::dispatch::apply_common_prop(
            &mut entity,
            "linetype",
            &self.ribbon.active_linetype.clone(),
        );
        crate::scene::dispatch::apply_line_weight(&mut entity, self.ribbon.active_lineweight);

        if matches!(&entity, acadrust::EntityType::Viewport(_))
            && self.tabs[i].scene.current_layout != "Model"
        {
            // Assign a unique viewport ID (max existing id + 1, min 2).
            if let acadrust::EntityType::Viewport(ref mut vp) = entity {
                let layout_block = self.tabs[i].scene.current_layout_block_handle_pub();
                let max_id = self.tabs[i]
                    .scene
                    .document
                    .entities()
                    .filter_map(|e| {
                        if let acadrust::EntityType::Viewport(v) = e {
                            if v.common.owner_handle == layout_block { Some(v.id) } else { None }
                        } else {
                            None
                        }
                    })
                    .max()
                    .unwrap_or(1);
                vp.id = (max_id + 1).max(2);
            }

            let layout = self.tabs[i].scene.current_layout.clone();
            match self.tabs[i]
                .scene
                .document
                .add_entity_to_layout(entity, &layout)
            {
                Ok(new_handle) => {
                    self.tabs[i].scene.auto_fit_viewport(new_handle);
                }
                Err(e) => self
                    .command_line
                    .push_error(&format!("Viewport could not be added: {e}")),
            }
        } else {
            self.tabs[i].scene.add_entity(entity);
        }
    }
}

// ── Multi-selection property aggregation ───────────────────────────────────

pub(super) fn build_selection_groups(
    selected: &[(Handle, &EntityType)],
) -> Vec<ui::properties::SelectionGroup> {
    let mut groups = vec![ui::properties::SelectionGroup {
        label: format!("All({})", selected.len()),
        handles: selected.iter().map(|(handle, _)| *handle).collect(),
    }];

    let mut by_type: std::collections::BTreeMap<String, Vec<Handle>> =
        std::collections::BTreeMap::new();
    for (handle, entity) in selected {
        by_type
            .entry(entity_type_key(entity))
            .or_default()
            .push(*handle);
    }

    for (kind, handles) in by_type {
        groups.push(ui::properties::SelectionGroup {
            label: format!("{}({})", title_case_word(&kind), handles.len()),
            handles,
        });
    }

    groups
}

pub(super) fn aggregate_sections(
    selected: &[(Handle, &EntityType)],
    text_style_names: &[String],
) -> Vec<crate::scene::object::PropSection> {
    if selected.is_empty() {
        return vec![];
    }

    let mut all_sections: Vec<Vec<crate::scene::object::PropSection>> = selected
        .iter()
        .map(|(handle, entity)| dispatch::properties_sectioned(*handle, entity, text_style_names))
        .collect();

    let mut result = all_sections.remove(0);
    for sections in all_sections {
        result = merge_sections(&result, &sections);
    }
    result
}

fn merge_sections(
    left: &[crate::scene::object::PropSection],
    right: &[crate::scene::object::PropSection],
) -> Vec<crate::scene::object::PropSection> {
    left.iter()
        .filter_map(|section| {
            let rhs = right.iter().find(|candidate| candidate.title == section.title)?;
            let props: Vec<crate::scene::object::Property> = section
                .props
                .iter()
                .filter_map(|prop| {
                    let other =
                        rhs.props.iter().find(|candidate| candidate.field == prop.field)?;
                    Some(crate::scene::object::Property {
                        label: prop.label.clone(),
                        field: prop.field,
                        value: merge_prop_value(&prop.value, &other.value),
                    })
                })
                .collect();
            if props.is_empty() {
                None
            } else {
                Some(crate::scene::object::PropSection {
                    title: section.title.clone(),
                    props,
                })
            }
        })
        .collect()
}

fn merge_prop_value(
    left: &crate::scene::object::PropValue,
    right: &crate::scene::object::PropValue,
) -> crate::scene::object::PropValue {
    use crate::scene::object::PropValue;

    if left == right {
        return left.clone();
    }

    match (left, right) {
        (PropValue::LayerChoice(_), PropValue::LayerChoice(_)) => {
            PropValue::LayerChoice(VARIES_LABEL.into())
        }
        (PropValue::ColorChoice(_), PropValue::ColorChoice(_))
        | (PropValue::ColorVaries, _)
        | (_, PropValue::ColorVaries) => PropValue::ColorVaries,
        (PropValue::LwChoice(_), PropValue::LwChoice(_))
        | (PropValue::LwVaries, _)
        | (_, PropValue::LwVaries) => PropValue::LwVaries,
        (PropValue::LinetypeChoice(_), PropValue::LinetypeChoice(_)) => {
            PropValue::LinetypeChoice(VARIES_LABEL.into())
        }
        (
            PropValue::Choice { options, .. },
            PropValue::Choice {
                options: other_options,
                ..
            },
        ) if options == other_options => PropValue::Choice {
            selected: VARIES_LABEL.into(),
            options: options.clone(),
        },
        (PropValue::EditText(_), PropValue::EditText(_)) => {
            PropValue::EditText(VARIES_LABEL.into())
        }
        (PropValue::ReadOnly(_), PropValue::ReadOnly(_)) => {
            PropValue::ReadOnly(VARIES_LABEL.into())
        }
        (PropValue::HatchPatternChoice(_), PropValue::HatchPatternChoice(_)) => {
            PropValue::HatchPatternChoice(VARIES_LABEL.into())
        }
        (
            PropValue::BoolToggle { field, .. },
            PropValue::BoolToggle {
                field: other_field,
                ..
            },
        ) if field == other_field => PropValue::ReadOnly(VARIES_LABEL.into()),
        _ => left.clone(),
    }
}
