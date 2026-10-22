use super::{Message, OpenCADStudio};
use crate::command::{CadCommand, CmdResult};
use crate::scene::{
    block_alignment::{Alignment, Targets},
    model::object::{GripDef, GripShape},
};
use codec::Handle;
use glam::DVec3;
use iced::Task;

impl OpenCADStudio {
    pub(super) fn alignment_grip_markers(&self, insert: Handle, offset: [f64; 3]) -> Vec<GripDef> {
        self.tabs[self.active_tab]
            .scene
            .alignment_grips(insert)
            .into_iter()
            .map(|alignment| GripDef {
                id: alignment.id,
                world: alignment.anchor - DVec3::from_array(offset),
                is_midpoint: false,
                shape: GripShape::Triangle,
                dir: Some(alignment.direction),
                axis: None,
            })
            .collect()
    }

    pub(super) fn start_alignment_grip(&mut self, insert: Handle, id: usize) -> bool {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if tab.active_cmd.is_some()
            || tab.scene.selected.len() != 1
            || tab.selected_handle != Some(insert)
        {
            return false;
        }
        let Some(alignment) = tab
            .scene
            .alignment_grips(insert)
            .into_iter()
            .find(|grip| grip.id == id)
        else {
            return false;
        };
        if tab.scene.is_layer_locked(insert) {
            return true;
        }
        let targets = Targets::from_scene(&tab.scene, insert, alignment.plane);
        let anchor = alignment.anchor;
        let command = crate::modules::block_editor::alignment::AlignmentCommand::new(
            tab.id, alignment, targets,
        );
        self.command_line.push_info(&command.prompt());
        self.tabs[i].active_cmd = Some(Box::new(command));
        self.tabs[i].last_cursor_world = anchor;
        self.last_point = Some(anchor);
        self.grip_hover = None;
        self.grip_popup = None;
        self.refresh_command_point_pick_context(i);
        self.refresh_active_cmd_preview(i);
        true
    }

    pub(super) fn handle_block_alignment(&mut self, result: CmdResult) -> Task<Message> {
        let CmdResult::AlignBlockReference {
            document,
            alignment,
            cursor,
            context,
            flipped,
        } = result
        else {
            unreachable!()
        };
        let i = self.active_tab;
        let insert = alignment.reference.common.handle;
        let result = self.aligned_reference(i, document, &alignment, cursor, context, flipped);
        match result {
            Ok(Some(entity)) => {
                self.push_undo_snapshot(i, "Align block reference");
                let mut staged = self.tabs[i].scene.document.clone();
                *staged.get_entity_mut(insert).unwrap() = entity;
                self.apply_action_document(i, staged);
                self.tabs[i].scene.bump_geometry();
                self.tabs[i].dirty = true;
                self.finish_pending_history(i);
            }
            Ok(None) => {}
            Err(error) => self.command_line.push_error(crate::t!(error).as_ref()),
        }
        if self.tabs[i].id == document {
            self.tabs[i].active_cmd = None;
            self.tabs[i].scene.set_command_preview_hidden(&[]);
            self.tabs[i].scene.clear_preview_wire();
            self.refresh_properties();
        }
        Task::none()
    }

    fn aligned_reference(
        &self,
        i: usize,
        document: u64,
        expected: &Alignment,
        cursor: DVec3,
        context: Option<crate::command::PointPickContext>,
        flipped: bool,
    ) -> Result<Option<codec::EntityType>, &'static str> {
        let tab = &self.tabs[i];
        let insert = expected.reference.common.handle;
        if tab.id != document || tab.selected_handle != Some(insert) {
            return Err("The alignment reference is no longer selected in this drawing.");
        }
        if tab.scene.is_layer_locked(insert) {
            return Err("The alignment reference is on a locked layer.");
        }
        let current = tab
            .scene
            .alignment_grips(insert)
            .into_iter()
            .find(|grip| grip.parameter == expected.parameter && grip.id == expected.id)
            .ok_or("The Alignment parameter is no longer available.")?;
        if current.reference != expected.reference
            || current.native != expected.native
            || current.anchor != expected.anchor
            || current.direction != expected.direction
        {
            return Err("The alignment reference changed. Pick its grip again.");
        }
        // Resolve targets again at commit, so changed or hidden geometry is never used from a stale preview.
        let targets = Targets::from_scene(&tab.scene, insert, current.plane);
        let (point, angle) = targets.pose(&current, cursor, context, flipped)?;
        if point.distance(current.anchor) < 1e-10 && angle.abs() < 1e-12 {
            return Ok(None);
        }
        current.transformed(point, angle).map(Some)
    }
}
