use super::*;
use super::super::util::*;

/// Return `(new vertex grip id, old vertex count)` for polyline Add Vertex.
/// LWPolyline segment grips live after the vertex grips, so translate their id
/// back to the segment before selecting the inserted vertex.
fn polyline_add_vertex_target(entity: &AcadEntityType, grip_id: usize) -> Option<(usize, usize)> {
    match entity {
        AcadEntityType::LwPolyline(polyline) => {
            let n = polyline.vertices.len();
            if n == 0 {
                None
            } else if grip_id < n {
                Some((grip_id + 1, n))
            } else {
                let segment = grip_id - n;
                let segment_count = if polyline.is_closed {
                    n
                } else {
                    n.saturating_sub(1)
                };
                (segment < segment_count).then_some((segment + 1, n))
            }
        }
        AcadEntityType::Polyline(polyline) if grip_id < polyline.vertices.len() => {
            Some((grip_id + 1, polyline.vertices.len()))
        }
        AcadEntityType::Polyline2D(polyline) if grip_id < polyline.vertices.len() => {
            Some((grip_id + 1, polyline.vertices.len()))
        }
        AcadEntityType::Polyline3D(polyline) if grip_id < polyline.vertices.len() => {
            Some((grip_id + 1, polyline.vertices.len()))
        }
        // A hatch boundary vertex: the new vertex is the next grip.
        AcadEntityType::Hatch(hatch) => {
            use crate::entities::traits::Grippable;
            use crate::scene::model::object::GripMenuAction;
            hatch
                .grip_menu(grip_id)
                .iter()
                .any(|item| matches!(item.action, GripMenuAction::AddVertex))
                .then(|| (grip_id + 1, hatch.grips().len()))
        }
        _ => None,
    }
}

/// The modes Ctrl steps through after the one a polyline grip was picked up in.
fn grip_ctrl_stages(
    items: &[crate::scene::model::object::GripMenuItem],
) -> Vec<crate::scene::model::object::GripMenuAction> {
    use crate::scene::model::object::GripMenuAction as Action;
    if !items.iter().any(|item| item.action == Action::AddVertex) {
        return Vec::new();
    }
    [
        Action::AddVertex,
        Action::RemoveVertex,
        Action::ConvertToArc,
        Action::ConvertToLine,
    ]
    .into_iter()
    .filter(|action| items.iter().any(|item| item.action == *action))
    .collect()
}

fn polyline_vertex_count(entity: &AcadEntityType) -> Option<usize> {
    match entity {
        AcadEntityType::LwPolyline(polyline) => Some(polyline.vertices.len()),
        AcadEntityType::Polyline(polyline) => Some(polyline.vertices.len()),
        AcadEntityType::Polyline2D(polyline) => Some(polyline.vertices.len()),
        AcadEntityType::Polyline3D(polyline) => Some(polyline.vertices.len()),
        AcadEntityType::Hatch(hatch) => {
            Some(crate::entities::traits::Grippable::grips(hatch).len())
        }
        _ => None,
    }
}

impl OpenCADStudio {
    pub(in crate::app) fn grip_ctrl_one_shot(
        &self,
        i: usize,
    ) -> Option<crate::scene::model::object::GripMenuAction> {
        let cycle = self.grip_ctrl_cycle.as_ref()?;
        let active = self.tabs[i].active_grip.as_ref()?;
        (active.handle == cycle.handle && active.grip_id == cycle.active_grip_id)
            .then_some(cycle.one_shot)
            .flatten()
    }

    /// Ctrl on a hot polyline grip moves to its next mode, as the grip menu
    /// offers them. Each stage starts from the geometry the grip was picked up
    /// with, so a stage left behind leaves nothing in the drawing or in undo.
    pub(in crate::app) fn cycle_grip_ctrl_mode(&mut self) -> Task<Message> {
        use crate::entities::traits::EntityTypeOps;
        use crate::scene::model::object::GripMenuAction as Action;
        let i = self.active_tab;
        if self.tabs[i].grip_copy || self.grip_pending.is_some() {
            return Task::none();
        }
        let Some(active) = self.tabs[i].active_grip.as_ref() else {
            return Task::none();
        };
        let [target] = active.targets.as_slice() else {
            return Task::none();
        };
        if active.gizmo {
            return Task::none();
        }
        let (handle, active_grip_id) = (active.handle, active.grip_id);
        let mut cycle = self
            .grip_ctrl_cycle
            .take()
            .filter(|cycle| cycle.handle == handle && cycle.active_grip_id == active_grip_id)
            .unwrap_or(crate::app::GripCtrlCycle {
                handle,
                grip_id: active_grip_id,
                is_translate: target.is_translate,
                world: active.origin_world,
                stage: 0,
                active_grip_id,
                one_shot: None,
            });
        let original = self
            .grip_originals
            .iter()
            .find(|(owner, _)| *owner == handle)
            .map(|(_, entity)| entity.clone())
            .or_else(|| self.tabs[i].scene.document.get_entity(handle).cloned());
        let Some(original) = original else {
            return Task::none();
        };
        // Other grips (and entities whose Add Vertex is not a placement) keep
        // their single behaviour.
        if polyline_add_vertex_target(&original, cycle.grip_id).is_none() {
            return Task::none();
        }
        let items = original.grip_menu(cycle.grip_id);
        let stages = grip_ctrl_stages(&items);
        if stages.is_empty() || self.reject_locked_edit(i, handle) {
            return Task::none();
        }

        self.cancel_active_grip_edit();
        cycle.stage = (cycle.stage + 1) % (stages.len() + 1);
        cycle.one_shot = None;
        let mut task = Task::none();
        let label = match cycle.stage.checked_sub(1).map(|stage| stages[stage]) {
            None => {
                let edit = self.grip_edit_for_hit(
                    i,
                    handle,
                    cycle.grip_id,
                    cycle.is_translate,
                    cycle.world,
                );
                // A rectangle's grips start in their own Resize / Move Parallel.
                let stretches =
                    matches!(edit.mode, crate::scene::pick::grip::GripEditMode::Stretch);
                self.tabs[i].active_grip = Some(edit);
                self.sync_dyn_fields();
                stretches.then_some("Stretch")
            }
            Some(Action::AddVertex) => {
                // The menu's own placement: it prompts for the new vertex.
                let index = items
                    .iter()
                    .position(|item| item.action == Action::AddVertex);
                self.grip_popup = Some(crate::app::GripPopup {
                    handle,
                    grip_id: cycle.grip_id,
                    anchor: self.tabs[i].last_cursor_screen,
                    items: items.clone(),
                    selected: 0,
                    pinned: true,
                });
                task = self.on_grip_menu_pick(index.unwrap_or(0));
                None
            }
            Some(Action::ConvertToArc) => {
                let dirty_before = self.tabs[i].dirty;
                if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle) {
                    entity.apply_grip_menu(cycle.grip_id, Action::ConvertToArc);
                }
                self.tabs[i]
                    .scene
                    .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                self.tabs[i].dirty = true;
                self.refresh_selected_grips();
                self.refresh_properties();
                let world = self.tabs[i]
                    .selected_grip_handles
                    .iter()
                    .zip(self.tabs[i].selected_grips.iter())
                    .find(|(owner, grip)| **owner == handle && grip.id == cycle.grip_id)
                    .map_or(cycle.world, |(_, grip)| grip.world);
                self.grip_originals = vec![(handle, original)];
                self.grip_dirty_before = Some(dirty_before);
                self.tabs[i].active_grip =
                    Some(GripEdit::single(handle, cycle.grip_id, false, world));
                Some("Convert to Arc")
            }
            Some(action) => {
                self.tabs[i].active_grip = Some(GripEdit::single(
                    handle,
                    cycle.grip_id,
                    cycle.is_translate,
                    cycle.world,
                ));
                cycle.one_shot = Some(action);
                items
                    .iter()
                    .find(|item| item.action == action)
                    .map(|item| item.label)
            }
        };
        if let Some(label) = label {
            self.command_line
                .push_info(&format!("{}:", crate::i18n::translate(label)));
        }
        let Some(active_grip_id) = self.tabs[i].active_grip.as_ref().map(|grip| grip.grip_id)
        else {
            return task;
        };
        cycle.active_grip_id = active_grip_id;
        self.grip_ctrl_cycle = Some(cycle);
        // Show the new mode at the cursor without waiting for the next move.
        let at = self.tabs[i].last_cursor_screen;
        Task::batch([task, Task::done(Message::ViewportMove(at))])
    }

    /// The click that confirms Remove Vertex or Convert to Line: the menu's
    /// own action, recorded as one undo step.
    pub(in crate::app) fn commit_grip_ctrl_one_shot(&mut self, i: usize) -> Task<Message> {
        use crate::entities::traits::EntityTypeOps;
        let Some(action) = self.grip_ctrl_one_shot(i) else {
            return Task::none();
        };
        let Some(cycle) = self.grip_ctrl_cycle.take() else {
            return Task::none();
        };
        self.cancel_active_grip_edit();
        let items = self.tabs[i]
            .scene
            .document
            .get_entity(cycle.handle)
            .map(|entity| entity.grip_menu(cycle.grip_id))
            .unwrap_or_default();
        let Some(index) = items.iter().position(|item| item.action == action) else {
            return Task::none();
        };
        self.grip_popup = Some(crate::app::GripPopup {
            handle: cycle.handle,
            grip_id: cycle.grip_id,
            anchor: self.tabs[i].last_cursor_screen,
            items,
            selected: index,
            pinned: true,
        });
        self.on_grip_menu_pick(index)
    }

    pub(in crate::app) fn on_grip_menu_pick(&mut self, idx: usize) -> Task<Message> {
        let i = self.active_tab;
        let Some(popup) = self.grip_popup.take() else {
            return Task::none();
        };
        if self.reject_locked_edit(i, popup.handle) {
            return Task::none();
        }
        self.grip_hover = None;
        let Some(item) = popup.items.get(idx).cloned() else {
            return Task::none();
        };
        use crate::entities::traits::EntityTypeOps;
        use crate::scene::model::object::GripMenuAction;
        if matches!(
            item.action,
            GripMenuAction::Stretch
                | GripMenuAction::RectangleResize
                | GripMenuAction::MoveWithText
                | GripMenuAction::MoveWithDimLine
                | GripMenuAction::MoveWithLeader
                | GripMenuAction::MoveIndependent
        ) {
            let is_dimension = matches!(
                self.tabs[i].scene.document.get_entity(popup.handle),
                Some(document::EntityType::Dimension(_))
            );
            if is_dimension {
                let movement = match item.action {
                    GripMenuAction::MoveWithDimLine => Some("Keep dim line with text"),
                    GripMenuAction::MoveWithLeader => Some("Move text, add leader"),
                    GripMenuAction::MoveIndependent => Some("Move text, no leader"),
                    _ => None,
                };
                if let Some(movement) = movement {
                    crate::entities::dim_override::set_property(
                        &mut self.tabs[i].scene.document,
                        popup.handle,
                        "dim_text_movement",
                        movement,
                    );
                    self.tabs[i].dirty = true;
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(popup.handle, crate::scene::ChangeKind::Modified)]);
                }
            }
            // Stretch / Move = grab this grip. Engage it so the next
            // click places it (click-move-click) — same as picking the
            // grip directly in the viewport. Without this the menu just
            // closed and the grip never became hot (issue #48).
            if let Some((_, g)) = self.tabs[i]
                .selected_grip_handles
                .iter()
                .zip(self.tabs[i].selected_grips.iter())
                .find(|(owner, g)| **owner == popup.handle && g.id == popup.grip_id)
            {
                // Only multileaders use the whole-object grip.
                let is_multileader = matches!(
                    self.tabs[i].scene.document.get_entity(popup.handle),
                    Some(document::EntityType::MultiLeader(_))
                );
                let (grip_id, is_translate) =
                    if matches!(item.action, GripMenuAction::MoveWithLeader) && is_multileader {
                        (crate::entities::multileader::MOVE_ALL_GRIP, true)
                    } else {
                        (
                            popup.grip_id,
                            matches!(item.action, GripMenuAction::MoveWithText)
                                || (!is_dimension && g.is_midpoint),
                        )
                    };
                if matches!(item.action, GripMenuAction::RectangleResize) {
                    let rectangle = self.tabs[i]
                        .scene
                        .document
                        .get_entity(popup.handle)
                        .and_then(|entity| {
                            let AcadEntityType::LwPolyline(polyline) = entity else {
                                return None;
                            };
                            let (frame, plane) =
                                crate::entities::lwpolyline::rectangle_frame(polyline)?;
                            let opposite = polyline.vertices.get((popup.grip_id + 2) % 4)?.location;
                            Some((
                                glam::DVec3::from_array(plane.point_at([opposite.x, opposite.y])),
                                glam::DVec3::from_array(plane.vector_at(frame.width_axis))
                                    .try_normalize()?,
                                glam::DVec3::from_array(plane.vector_at(frame.height_axis))
                                    .try_normalize()?,
                            ))
                        });
                    if let Some((opposite, width_axis, height_axis)) = rectangle {
                        if self.grip_originals.is_empty() {
                            self.grip_originals = self.tabs[i]
                                .scene
                                .document
                                .get_entity(popup.handle)
                                .cloned()
                                .map(|entity| vec![(popup.handle, entity)])
                                .unwrap_or_default();
                        }
                        self.tabs[i].active_grip = Some(GripEdit::rectangle_resize(
                            popup.handle,
                            popup.grip_id,
                            g.world,
                            opposite,
                            width_axis,
                            height_axis,
                        ));
                        self.sync_dyn_fields();
                    }
                } else {
                    self.tabs[i].active_grip = Some(GripEdit::single(
                        popup.handle,
                        grip_id,
                        is_translate,
                        g.world,
                    ));
                }
            }
            return Task::none();
        }
        // Actions that need a follow-up number stash a pending
        // state + prompt; the next typed value drives
        // `apply_grip_menu_value`.
        let prompt = self.tabs[i]
            .scene
            .document
            .get_entity(popup.handle)
            .and_then(|e| e.grip_menu_value_prompt(popup.grip_id, item.action));
        if let Some(label) = prompt {
            self.grip_pending = Some(crate::app::GripPendingValue {
                handle: popup.handle,
                grip_id: popup.grip_id,
                action: item.action,
                label,
            });
            if matches!(
                item.action,
                GripMenuAction::Lengthen
                    | GripMenuAction::Radius
                    | GripMenuAction::ArcLength
                    | GripMenuAction::RectangleWidth
                    | GripMenuAction::RectangleHeight
                    | GripMenuAction::MoveParallel
            ) {
                if let Some((_, grip)) = self.tabs[i]
                    .selected_grip_handles
                    .iter()
                    .zip(self.tabs[i].selected_grips.iter())
                    .find(|(owner, grip)| **owner == popup.handle && grip.id == popup.grip_id)
                {
                    if self.grip_originals.is_empty() {
                        self.grip_originals = self.tabs[i]
                            .scene
                            .document
                            .get_entity(popup.handle)
                            .cloned()
                            .map(|entity| vec![(popup.handle, entity)])
                            .unwrap_or_default();
                    }
                    self.tabs[i].active_grip = Some(match item.action {
                        GripMenuAction::Radius => {
                            GripEdit::radius(popup.handle, popup.grip_id, grip.world)
                        }
                        GripMenuAction::ArcLength => {
                            GripEdit::arc_length(popup.handle, popup.grip_id, grip.world)
                        }
                        GripMenuAction::RectangleWidth => {
                            GripEdit::rectangle_width(popup.handle, popup.grip_id, grip.world)
                        }
                        GripMenuAction::RectangleHeight => {
                            GripEdit::rectangle_height(popup.handle, popup.grip_id, grip.world)
                        }
                        GripMenuAction::MoveParallel => {
                            GripEdit::move_parallel(popup.handle, popup.grip_id, grip.world)
                        }
                        _ => GripEdit::lengthen(popup.handle, popup.grip_id, grip.world),
                    });
                }
                // Popup actions do not pass through the normal grip
                // press handler, which is where dynamic fields are
                // usually seeded. Build the value field
                // immediately so it is visible before the next mouse
                // move (and so keyboard input has a field to target).
                self.sync_dyn_fields();
                if matches!(item.action, GripMenuAction::Radius) {
                    self.command_line
                        .push_info(crate::t!("Specify point or enter radius:").as_ref());
                } else if matches!(item.action, GripMenuAction::ArcLength) {
                    self.command_line
                        .push_info(crate::t!("Specify point or enter arc length:").as_ref());
                } else if matches!(item.action, GripMenuAction::RectangleWidth) {
                    self.command_line.push_info("Specify point or enter width:");
                } else if matches!(item.action, GripMenuAction::RectangleHeight) {
                    self.command_line
                        .push_info("Specify point or enter height:");
                } else if matches!(item.action, GripMenuAction::MoveParallel) {
                    self.command_line
                        .push_info("Specify point or enter parallel offset:");
                } else {
                    self.command_line
                        .push_info(crate::t!("Specify point or enter distance:").as_ref());
                }
            } else {
                self.command_line.push_info(crate::tf!("{label}:").as_ref());
            }
            return self.focus_cmd_input();
        }
        // Break at vertex replaces the entity with the split pieces —
        // structural, so it can't ride the in-place apply below.
        if matches!(item.action, GripMenuAction::BreakVertex) {
            let pieces = match self.tabs[i].scene.document.get_entity(popup.handle) {
                Some(document::EntityType::LwPolyline(p)) => {
                    crate::entities::lwpolyline::break_at_vertex(p, popup.grip_id)
                }
                _ => None,
            };
            match pieces {
                Some(pieces) => {
                    self.push_undo_snapshot(i, "BREAK");
                    self.tabs[i].scene.erase_entities(&[popup.handle]);
                    for e in pieces {
                        self.tabs[i].scene.add_entity(e);
                    }
                    self.tabs[i].dirty = true;
                    self.refresh_selected_grips();
                    self.refresh_properties();
                    self.command_line
                        .push_output(crate::t!("Polyline broken at vertex.").as_ref());
                }
                None => self
                    .command_line
                    .push_error(crate::t!("Cannot break at this vertex.").as_ref()),
            }
            return Task::none();
        }
        // Polyline Add Vertex is an interactive placement, not an
        // immediate midpoint edit. Seed the undo snapshot before the
        // provisional vertex exists, then engage its new grip so the
        // regular snap/ortho/polar preview follows the cursor. One
        // click commits the whole append+move; Escape restores the
        // original entity.
        if matches!(item.action, GripMenuAction::AddVertex) {
            let placement = self.tabs[i]
                .scene
                .document
                .get_entity(popup.handle)
                .cloned()
                .and_then(|original| {
                    polyline_add_vertex_target(&original, popup.grip_id)
                        .map(|(new_gid, old_len)| (original, new_gid, old_len))
                });
            if let Some((original, new_gid, old_len)) = placement {
                let dirty_before = self.tabs[i].dirty;
                if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(popup.handle) {
                    entity.apply_grip_menu(popup.grip_id, item.action);
                }
                let inserted = self.tabs[i]
                    .scene
                    .document
                    .get_entity(popup.handle)
                    .and_then(polyline_vertex_count)
                    .is_some_and(|len| len == old_len + 1);
                if !inserted {
                    if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(popup.handle) {
                        *entity = original;
                    }
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(popup.handle, crate::scene::ChangeKind::Modified)]);
                    self.tabs[i].dirty = dirty_before;
                    self.refresh_selected_grips();
                    self.refresh_properties();
                    self.command_line
                        .push_error(crate::t!("Cannot add a vertex here.").as_ref());
                    return Task::none();
                }
                self.tabs[i]
                    .scene
                    .bump_entities(&[(popup.handle, crate::scene::ChangeKind::Modified)]);
                self.tabs[i].dirty = true;
                self.refresh_selected_grips();
                self.refresh_properties();
                let grip_world = self.tabs[i]
                    .selected_grip_handles
                    .iter()
                    .zip(self.tabs[i].selected_grips.iter())
                    .find(|(owner, grip)| **owner == popup.handle && grip.id == new_gid)
                    .map(|(_, grip)| grip.world);
                if let Some(grip_world) = grip_world {
                    self.grip_originals = vec![(popup.handle, original)];
                    self.grip_dirty_before = Some(dirty_before);
                    self.tabs[i].active_grip =
                        Some(GripEdit::single(popup.handle, new_gid, false, grip_world));
                    self.command_line
                        .push_info(crate::t!("Specify new vertex location:").as_ref());
                } else {
                    if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(popup.handle) {
                        *entity = original;
                    }
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(popup.handle, crate::scene::ChangeKind::Modified)]);
                    self.tabs[i].dirty = dirty_before;
                    self.refresh_selected_grips();
                    self.refresh_properties();
                    self.command_line
                        .push_error(crate::t!("Cannot place the new vertex.").as_ref());
                }
                return Task::none();
            }
        }
        // One-shot action — apply immediately.
        let unchanged = item.label.starts_with('✓')
            || self.tabs[i]
                .scene
                .document
                .get_entity(popup.handle)
                .is_some_and(|entity| match (item.action, entity) {
                    (GripMenuAction::ShowFit, document::EntityType::Spline(spline)) => {
                        crate::entities::spline::shows_fit_points(spline)
                    }
                    (GripMenuAction::ShowControlVertices, document::EntityType::Spline(spline)) => {
                        crate::entities::spline::shows_control_vertices(spline)
                    }
                    _ => false,
                });
        if unchanged {
            return Task::none();
        }
        self.push_undo_snapshot(i, item.label);
        // For Add Leader, the new arrow becomes the last grip; remember
        // its id so we can grab it for placement right after.
        let add_leader_gid = if matches!(item.action, GripMenuAction::AddLeader) {
            self.tabs[i]
                .scene
                .document
                .get_entity(popup.handle)
                .and_then(|e| match e {
                    document::EntityType::MultiLeader(ml) => Some(
                        ml.context
                            .leader_roots
                            .iter()
                            .flat_map(|r| r.lines.iter())
                            .map(|l| l.points.len())
                            .sum::<usize>(),
                    ),
                    _ => None,
                })
        } else {
            None
        };
        if matches!(item.action, GripMenuAction::AboveDimLine) {
            crate::entities::dim_override::set_property(
                &mut self.tabs[i].scene.document,
                popup.handle,
                "dim_text_pos_vert",
                "Above",
            );
        }
        if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(popup.handle) {
            entity.apply_grip_menu(popup.grip_id, item.action);
        }
        // Menu actions reshape dimensions too — drop a stale baked *D
        // block so the edit is visible (no-op for non-dims). (#398)
        self.tabs[i]
            .scene
            .invalidate_dim_block_recorded(popup.handle);
        self.tabs[i]
            .scene
            .bump_entities(&[(popup.handle, crate::scene::ChangeKind::Modified)]);
        self.tabs[i].dirty = true;
        self.refresh_selected_grips();
        self.refresh_properties();
        // Grab the new arrow so it follows the cursor (click places it,
        // Esc removes it).
        if let Some(new_gid) = add_leader_gid {
            if let Some((_, g)) = self.tabs[i]
                .selected_grip_handles
                .iter()
                .zip(self.tabs[i].selected_grips.iter())
                .find(|(owner, g)| **owner == popup.handle && g.id == new_gid)
            {
                self.tabs[i].active_grip =
                    Some(GripEdit::single(popup.handle, new_gid, false, g.world));
                self.grip_add_provisional = Some((popup.handle, new_gid));
            }
        }
        // Convert to Arc enters placement: the segment grip goes hot
        // in Absolute mode, so the arc re-fits through the cursor as
        // it moves and the next click seats it (#339).
        if matches!(item.action, GripMenuAction::ConvertToArc) {
            if let Some((_, g)) = self.tabs[i]
                .selected_grip_handles
                .iter()
                .zip(self.tabs[i].selected_grips.iter())
                .find(|(owner, g)| **owner == popup.handle && g.id == popup.grip_id)
            {
                self.tabs[i].active_grip = Some(GripEdit::single(
                    popup.handle,
                    popup.grip_id,
                    false,
                    g.world,
                ));
            }
        }
        Task::none()
    }

}
