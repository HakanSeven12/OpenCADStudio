use super::*;
use crate::command::CmdResult;
use crate::scene::{model::hatch_model::HatchModel, WireModel};
use glam::DVec3;

#[derive(Default)]
pub(in crate::app) struct ParameterPreview {
    pub wires: Vec<WireModel>,
    pub hatches: Vec<HatchModel>,
    pub hidden: Vec<Handle>,
}

impl OpenCADStudio {
    pub(in crate::app) fn block_parameter_geometry_preview(
        &self,
        i: usize,
        cursor: DVec3,
    ) -> Option<ParameterPreview> {
        let tab = &self.tabs[i];
        let CmdResult::EvaluateBlockParameter {
            grip,
            insert,
            document,
            definition,
            parameter,
            value,
        } = tab.active_cmd.as_ref()?.block_parameter_preview(cursor)?
        else {
            return None;
        };
        let empty = || Some(ParameterPreview::default());
        if tab.id != document || !cursor.is_finite() || tab.active_block_edit_session().is_some() {
            return empty();
        }
        let (handle, source) = if let Some(insert) = insert {
            if tab.block_test.is_some()
                || actions::instance::definition(&tab.scene.document, insert) != Some(definition)
            {
                return empty();
            }
            (insert, &tab.scene.document)
        } else {
            let Some(test) = tab
                .block_test
                .as_ref()
                .filter(|test| test.definition == definition)
            else {
                return empty();
            };
            (test.insert, test.baseline.as_ref())
        };
        if tab.selected_handle != Some(handle)
            || !tab.scene.selected.contains(&handle)
            || tab.scene.is_layer_locked(handle)
            || !actions::has_action(source, definition, parameter)
            || grip.is_some_and(|id| {
                !crate::scene::block_grips::runtime::action_grip_available(
                    source, definition, parameter, id,
                )
            })
        {
            return empty();
        }
        let Some(EntityType::Insert(reference)) = tab.scene.document.get_entity(handle) else {
            return empty();
        };
        if !tab.scene.entity_belongs_to_active_space(handle) {
            return empty();
        }
        if insert.is_none() && tab.scene.document.entities().any(|entity| matches!(entity, EntityType::Insert(other)
            if other.common.handle != handle && (other.block_name == reference.block_name
                || tab.scene.document.dynamic_definition_for_insert(other.common.handle) == Some(definition)))) { return empty(); }
        let Some(inverse) = crate::scene::inverse_affine(
            &crate::scene::render_graph::insert_transform(&tab.scene.document, reference),
        ) else {
            return empty();
        };
        let local = value.map_points(|point| actions::v(inverse.apply(actions::native(point))));
        let evaluated = if insert.is_some() {
            actions::instance::evaluate_value(&tab.scene.document, handle, parameter, local)
        } else {
            actions::instance::evaluate_test_value(
                &tab.scene.document,
                source,
                definition,
                handle,
                parameter,
                local,
            )
        };
        let Ok(staged) = evaluated else {
            return empty();
        };
        let Some((wires, hatches)) = tab
            .scene
            .reference_document_preview(staged.as_ref().unwrap_or(&tab.scene.document), handle)
        else {
            return empty();
        };
        Some(ParameterPreview {
            wires,
            hatches,
            hidden: vec![handle],
        })
    }
}
