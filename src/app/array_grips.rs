//! Arrow grips of a selected associative array (counts and spacings).

use codec::Handle;

use crate::command::CadCommand;
use crate::scene::assoc_array;
use crate::scene::assoc_array_grips::{self as array_grips, Kind};
use crate::scene::model::object::{GripDef, GripShape};

use super::OpenCADStudio;

impl OpenCADStudio {
    /// The array under a lone selected array reference, when its arrows may
    /// be shown and dragged.
    fn editable_array(&self, handle: Handle) -> Option<assoc_array::AssocArray> {
        let tab = &self.tabs[self.active_tab];
        if tab.active_block_edit_session().is_some()
            || tab.scene.selected.len() != 1
            || tab.selected_handle != Some(handle)
            || tab.scene.is_layer_locked(handle)
        {
            return None;
        }
        assoc_array::array_of_insert(&tab.scene.document, handle)
    }

    pub(super) fn refresh_array_grips(&mut self, wo: [f64; 3]) {
        let i = self.active_tab;
        let Some(handle) = self.tabs[i].selected_handle else {
            return;
        };
        let Some(array) = self.editable_array(handle) else {
            return;
        };
        let origin = glam::DVec3::from_array(wo);
        let grips = array_grips::grips(&self.tabs[i].scene.document, &array);
        for grip in grips {
            self.tabs[i].selected_grips.push(GripDef {
                id: grip.kind.id(),
                world: grip.world - origin,
                is_midpoint: false,
                shape: GripShape::Triangle,
                dir: Some(grip.dir),
                axis: None,
            });
            self.tabs[i].selected_grip_handles.push(handle);
        }
    }

    /// A click on an array arrow starts its drag; false when `id` is not one.
    pub(super) fn start_array_grip(&mut self, handle: Handle, id: usize) -> bool {
        let Some(kind) = Kind::from_id(id) else {
            return false;
        };
        let Some(array) = self.editable_array(handle) else {
            return false;
        };
        let i = self.active_tab;
        let Some(drag) = array_grips::drag(&self.tabs[i].scene.document, &array, kind) else {
            return false;
        };
        let command =
            crate::modules::draw::modify::array_grip::ArrayGripCommand::new(array, kind, drag);
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.grip_hover = None;
        self.grip_popup = None;
        true
    }

    /// Tooltip of an array arrow.
    pub(in crate::app) fn array_grip_tip(&self, handle: Handle, id: usize) -> Option<&'static str> {
        let kind = Kind::from_id(id)?;
        let array =
            assoc_array::array_of_insert(&self.tabs[self.active_tab].scene.document, handle)?;
        Some(kind.label(array.kind))
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::scene::assoc_array::{self, AssocArraySpec};
    use crate::scene::assoc_array_grips::Kind;
    use codec::entities::Circle;
    use codec::EntityType;
    use glam::DVec3;

    #[test]
    fn dragging_the_column_arrow_adds_columns_in_one_undo_step() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let source = app.tabs[i]
            .scene
            .add_entity(EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)));
        let spec = AssocArraySpec::Rectangular {
            columns: 3,
            rows: 2,
            column_spacing: 20.0,
            row_spacing: 30.0,
        };
        let insert =
            assoc_array::create_from_spec(&mut app.tabs[i].scene, &[source], DVec3::ZERO, &spec)
                .unwrap();
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_selected_grips();
        let id = Kind::ColumnCount.id();
        let arrow = app.tabs[i]
            .selected_grips
            .iter()
            .find(|grip| grip.id == id)
            .expect("column count arrow")
            .world;
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();

        assert!(app.start_array_grip(insert, id));
        let command = app.tabs[i].active_cmd.as_mut().unwrap();
        assert_eq!(command.preview_hidden_handles(), &[insert]);
        let cursor = arrow + DVec3::new(60.0, 0.0, 0.0);
        // Preview: 6 columns x 2 rows, nothing written yet.
        assert_eq!(command.preview_entities(&before, cursor).len(), 12);
        let result = command.on_point(cursor);
        let _ = app.apply_cmd_result(result);

        assert!(app.tabs[i].active_cmd.is_none());
        let document = &app.tabs[i].scene.document;
        let array = assoc_array::array_of_insert(document, insert).unwrap();
        let params = assoc_array::value_params(document, &array);
        assert_eq!(assoc_array::param_value(params, "Items"), Some(6.0));
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth + 1);
        // The arrow follows the new last column.
        let moved = app.tabs[i]
            .selected_grips
            .iter()
            .find(|grip| grip.id == id)
            .unwrap()
            .world;
        assert!(moved.distance(cursor) < 1e-6, "{moved:?}");

        let _ = app.update(Message::Undo);
        let document = &app.tabs[i].scene.document;
        let array = assoc_array::array_of_insert(document, insert).unwrap();
        let params = assoc_array::value_params(document, &array);
        assert_eq!(assoc_array::param_value(params, "Items"), Some(3.0));
    }

    #[test]
    fn locked_arrays_show_no_arrows() {
        let mut app = OpenCADStudio::new_for_test();
        let i = app.active_tab;
        let source = app.tabs[i]
            .scene
            .add_entity(EntityType::Circle(Circle::from_coords(0.0, 0.0, 0.0, 2.0)));
        let spec = AssocArraySpec::Rectangular {
            columns: 3,
            rows: 2,
            column_spacing: 20.0,
            row_spacing: 30.0,
        };
        let insert =
            assoc_array::create_from_spec(&mut app.tabs[i].scene, &[source], DVec3::ZERO, &spec)
                .unwrap();
        let layer = match app.tabs[i].scene.document.get_entity(insert) {
            Some(EntityType::Insert(reference)) => reference.common.layer.clone(),
            _ => unreachable!(),
        };
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut(&layer)
            .unwrap()
            .lock();
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        app.refresh_selected_grips();
        assert!(!app.tabs[i]
            .selected_grips
            .iter()
            .any(|grip| crate::scene::assoc_array_grips::is_array_grip(grip.id)));
        assert!(!app.start_array_grip(insert, Kind::ColumnCount.id()));
    }
}
