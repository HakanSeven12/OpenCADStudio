//! Dynamic-block flip-state list outside the block editor.
//!
//! A flip parameter with an attached action draws a chevron on the reference.
//! Clicking it lists the two state labels; picking one runs the flip action.
//! The block editor keeps toggling the same grip directly, so this list is
//! model-space only.

use codec::Handle;

use crate::scene::block_actions as actions;

use super::OpenCADStudio;

/// The open flip-state list for a model-space reference.
#[derive(Clone, Debug)]
pub struct FlipPopup {
    pub insert_handle: Handle,
    pub anchor: iced::Point,
    pub document: u64,
    pub definition: Handle,
    pub parameter: Handle,
    /// Base-state label first, flipped-state label second.
    pub labels: Vec<String>,
    /// `0` = base state, `1` = flipped state.
    pub current: usize,
}

impl OpenCADStudio {
    /// Open the flip list when `id` is the rendered grip of an actionable flip
    /// parameter on the selected model-space reference.
    pub(super) fn open_flip_popup_at(
        &mut self,
        handle: Handle,
        id: usize,
        anchor: iced::Point,
    ) -> bool {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_block_edit_session().is_some() || tab.block_test.is_some() {
            return false;
        }
        if tab.selected_handle != Some(handle) || tab.scene.selected.len() != 1 {
            return false;
        }
        let Some(def) = actions::instance::definition(&tab.scene.document, handle) else {
            return false;
        };
        let Some((node, _)) = crate::scene::block_grips::runtime::decode(id) else {
            return false;
        };
        let Some(p) = actions::all_parameters(&tab.scene.document, def)
            .into_iter()
            .find(|p| p.id == node)
        else {
            return false;
        };
        if !p.flip || !actions::has_action(&tab.scene.document, def, p.handle) {
            return false;
        }
        if tab.scene.is_layer_locked(handle) {
            return true;
        }
        let Ok(actions::Value::Flip(flipped)) =
            actions::instance::value(&tab.scene.document, handle, &p)
        else {
            return false;
        };
        let labels = [false, true]
            .into_iter()
            .map(|state| {
                crate::scene::block_parameter_properties::flip_state_label(
                    &tab.scene.document,
                    p.handle,
                    state,
                )
            })
            .collect::<Option<Vec<_>>>();
        let Some(labels) = labels else {
            return false;
        };
        let document = tab.id;
        self.flip_popup = Some(FlipPopup {
            insert_handle: handle,
            anchor,
            document,
            definition: def,
            parameter: p.handle,
            labels,
            current: usize::from(flipped),
        });
        self.visibility_popup = None;
        self.table_popup = None;
        true
    }

    pub(super) fn apply_flip_state(&mut self, popup: FlipPopup, flipped: bool) {
        let i = self.active_tab;
        if self.reject_locked_edit(i, popup.insert_handle) {
            return;
        }
        let _ = self.handle_block_action_result(
            crate::command::CmdResult::EvaluateBlockParameter {
                grip: None,
                document: popup.document,
                definition: popup.definition,
                parameter: popup.parameter,
                insert: Some(popup.insert_handle),
                value: actions::Value::Flip(flipped),
            },
        );
    }
}
