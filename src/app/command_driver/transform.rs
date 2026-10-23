use super::*;

impl OpenCADStudio {
    pub(super) fn handle_transform_selected(
        &mut self,
        mut handles: Vec<Handle>,
        transform: crate::command::EntityTransform,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if handles.is_empty() {
            self.tabs[i].active_cmd = None;
            return Some(Task::none());
        }
        if matches!(&transform, crate::command::EntityTransform::Translate(_)) {
            let scope = self.tabs[i].current_parametric_scope();
            handles = self.tabs[i]
                .scene
                .parametric_connected_handles(scope, &handles, false);
            handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        }
        let label = self.history_label_from_active_cmd(i, "MOVE");
        // A translation carries its complete connected constraint
        // component so each member keeps its own dimensions. Other
        // transforms retain their selected-object behavior.
        let pending = self.begin_undo(i, label, handles.len(), true);
        let handles = self.tabs[i]
            .scene
            .transform_picked_segments(&handles, &transform);
        self.tabs[i].scene.transform_entities(&handles, &transform);
        self.apply_inferred_constraints(i, &handles);
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.refresh_properties();
        if let Some(p) = pending {
            self.commit_undo_delta(i, p);
        }
        None
    }

    pub(super) fn handle_copy_selected(
        &mut self,
        mut handles: Vec<Handle>,
        transform: crate::command::EntityTransform,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if handles.is_empty() {
            return Some(Task::none());
        }
        let label = self.history_label_from_active_cmd(i, "COPY");
        // Copying a dimension clones a *D block record, so gate delta on
        // the selection being dimension-free.
        let delta_safe = self.delta_copy_safe(i, &handles);
        let pending = self.begin_undo(i, label, handles.len(), delta_safe);
        let new_handles = self.tabs[i].scene.copy_entities(&handles, &transform);
        self.tabs[i].dirty = true;
        // One selection transaction for all newly copied entities.
        // Per-handle select_entity invalidated selection and GPU highlight
        // thousands of times when COPY committed large block selections.
        self.tabs[i].scene.select_entities_exclusive(&new_handles);
        self.tabs[i].scene.clear_preview_wire();
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
        self.refresh_properties();
        if let Some(pd) = pending {
            self.commit_undo_delta(i, pd);
        }
        None
    }

    pub(super) fn handle_align_selected(&mut self, result: CmdResult) {
        let i = self.active_tab;
        let CmdResult::AlignSelected {
            mut handles,
            src1,
            dst1,
            angle_rad,
            scale,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if handles.is_empty() {
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.tabs[i].scene.clear_preview_wire();
            self.restore_pre_cmd_tangent();
        } else {
            let label = self.history_label_from_active_cmd(i, "ALIGN");
            // A chain of transform_entities on the same handles — the
            // recording keeps each entity's first (pre-align) image, so
            // it's delta-safe like a single transform.
            let pending = self.begin_undo(i, label, handles.len(), true);
            // Step 1: translate so src1 is at origin
            self.tabs[i]
                .scene
                .transform_entities(&handles, &crate::command::EntityTransform::Translate(-src1));
            // Step 2: uniform scale (only when != 1)
            if (scale - 1.0).abs() > 1e-4 {
                self.tabs[i].scene.transform_entities(
                    &handles,
                    &crate::command::EntityTransform::Scale {
                        center: glam::DVec3::ZERO,
                        factor: scale,
                    },
                );
            }
            // Step 3: rotate in the XY plane by angle_rad
            if angle_rad.abs() > 1e-4 {
                self.tabs[i].scene.transform_entities(
                    &handles,
                    &crate::command::EntityTransform::Rotate {
                        center: glam::DVec3::ZERO,
                        axis: glam::DVec3::Z,
                        angle_rad,
                    },
                );
            }
            // Step 4: translate to dst1
            self.tabs[i]
                .scene
                .transform_entities(&handles, &crate::command::EntityTransform::Translate(dst1));
            self.tabs[i].dirty = true;
            self.tabs[i].scene.deselect_all();
            for h in &handles {
                self.tabs[i].scene.select_entity(*h, false);
            }
            self.tabs[i].scene.clear_preview_wire();
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.restore_pre_cmd_tangent();
            self.command_line
                .push_output(crate::t!("ALIGN: applied.").as_ref());
            self.refresh_properties();
            if let Some(pd) = pending {
                self.commit_undo_delta(i, pd);
            }
        }
    }

    pub(super) fn handle_stretch_window(
        &mut self,
        mut handles: Vec<Handle>,
        windows: Vec<(glam::DVec3, glam::DVec3)>,
        mut excluded_points: Vec<(Handle, glam::DVec3)>,
        subtract_window: Option<(glam::DVec3, glam::DVec3)>,
    ) {
        use crate::modules::draw::modify::stretch::is_excluded;
        let i = self.active_tab;
        if let Some((min, max)) = subtract_window {
            // Subtract only the vertex grips that STRETCH actually selected.
            // Testing whole entities would remove all of their vertices,
            // including points outside the subtraction rectangle.
            for (&handle, grip) in self.tabs[i]
                .selected_grip_handles
                .iter()
                .zip(self.tabs[i].selected_grips.iter())
            {
                let world = grip.world;
                let inside = world.x >= min.x - 1e-9
                    && world.x <= max.x + 1e-9
                    && world.y >= min.y - 1e-9
                    && world.y <= max.y + 1e-9;
                if inside && !is_excluded(&excluded_points, handle, world) {
                    excluded_points.push((handle, world));
                }
            }
        }
        // Accumulate every entity touched by any crossing window. Keep STRETCH
        // in its selection stage; Enter is what advances to the base point.
        {
            let scene = &self.tabs[i].scene;

            // A subtraction window leaves the current crossing windows
            // unchanged; do not rerun a large drawing's selection query.
            for (win_min, win_max) in windows.iter().filter(|_| subtract_window.is_none()) {
                handles.extend(
                    scene
                        .interaction_handles_in_world_aabb([
                            win_min.x, win_min.y, win_max.x, win_max.y,
                        ])
                        .into_iter()
                        .filter(|&handle| !scene.is_layer_locked(handle)),
                );
            }
        }

        handles.sort_unstable_by_key(|handle| handle.value());
        handles.dedup();

        if handles.is_empty() {
            self.command_line
                .push_output(crate::t!("STRETCH: nothing crosses the window.").as_ref());
        } else {
            // Visual feedback must only highlight entities that actually have a
            // stretchable point inside one of the crossing windows. `handles` can
            // contain the whole candidate/preselection set, while `windows` decides
            // which vertices will really move.
            let point_inside = |point: glam::DVec3| {
                const EPS: f64 = 1.0e-9;

                windows.iter().any(|(win_min, win_max)| {
                    point.x >= win_min.x - EPS
                        && point.x <= win_max.x + EPS
                        && point.y >= win_min.y - EPS
                        && point.y <= win_max.y + EPS
                        && point.z >= win_min.z - EPS
                        && point.z <= win_max.z + EPS
                })
            };

            let mut visual_handles = Vec::new();

            for &handle in &handles {
                let wires = self.tabs[i].scene.wire_models_for(&[handle]);
                let active_point = |point: glam::DVec3| {
                    point_inside(point) && !is_excluded(&excluded_points, handle, point)
                };

                let affected = wires.iter().any(|wire| {
                    // LINE / LWPOLYLINE / other vertex-based entities.
                    let vertex_hit = wire
                        .key_vertices
                        .iter()
                        .any(|point| active_point(glam::DVec3::from_array(*point)));

                    if vertex_hit {
                        return true;
                    }

                    // Whole-object STRETCH cases such as circles/arcs/inserts/points:
                    // use only their meaningful anchor snap, not quadrants or
                    // tessellated display points.
                    wire.snap_pts.iter().any(|(point, hint)| {
                        matches!(
                            hint,
                            crate::scene::model::wire_model::SnapHint::Center
                                | crate::scene::model::wire_model::SnapHint::Insertion
                                | crate::scene::model::wire_model::SnapHint::Node
                        ) && active_point(*point)
                    })
                });

                if affected {
                    visual_handles.push(handle);
                }
            }

            self.tabs[i].scene.deselect_all();
            self.tabs[i].scene.select_entities(&visual_handles);
            self.refresh_selected_grips();
            // STRETCH temporary grips: keep only the actual vertices/endpoints
            // that lie inside one of the accumulated crossing windows.
            //
            // Mid-segment grips are deliberately excluded: STRETCH moves vertices,
            // not the midpoint affordances used by normal grip editing.
            let mut stretch_grips = Vec::new();
            let mut stretch_grip_handles = Vec::new();

            for (handle, grip) in std::mem::take(&mut self.tabs[i].selected_grip_handles)
                .into_iter()
                .zip(std::mem::take(&mut self.tabs[i].selected_grips))
            {
                if !grip.is_midpoint
                    && point_inside(grip.world)
                    && !is_excluded(&excluded_points, handle, grip.world)
                {
                    stretch_grip_handles.push(handle);
                    stretch_grips.push(grip);
                }
            }

            self.tabs[i].selected_grip_handles = stretch_grip_handles;
            self.tabs[i].selected_grips = stretch_grips;
        }

        use crate::command::CadCommand;
        use crate::modules::draw::modify::stretch::StretchCommand;

        let wires = self.tabs[i].scene.wire_models_for(&handles);

        let cmd = StretchCommand::with_windows(handles, wires, windows, excluded_points);

        self.command_line.push_info(&CadCommand::prompt(&cmd));
        self.tabs[i].active_cmd = Some(Box::new(cmd));
    }

    pub(super) fn handle_stretch_entities(
        &mut self,
        mut handles: Vec<Handle>,
        windows: Vec<(glam::DVec3, glam::DVec3)>,
        excluded_points: Vec<(Handle, glam::DVec3)>,
        delta: glam::DVec3,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if handles.is_empty() {
            self.tabs[i].active_cmd = None;
            return Some(Task::none());
        }
        let structural = handles.iter().any(|handle| {
            matches!(
                self.tabs[i].scene.document.get_entity(*handle),
                Some(codec::EntityType::Dimension(_))
            )
        });
        let pending = self.begin_undo(i, "STRETCH", handles.len(), !structural);
        let mut count = 0usize;
        let mut changed_handles = Vec::new();
        let mut driven_refs = Vec::new();

        // Dimensions whose points moved — their baked *D block is
        // stale afterwards and must be dropped (see #398 / #372).
        let mut stretched_dims: Vec<codec::Handle> = Vec::new();
        for handle in &handles {
            // Subtract only this entity's explicitly removed vertices. The same
            // predicate gates preview, geometry mutation and inferred constraints.
            let in_win = |x: f64, y: f64| -> bool {
                windows.iter().any(|(min, max)| {
                    x >= min.x && x <= max.x && y >= min.y && y <= max.y
                }) && !crate::modules::draw::modify::stretch::is_excluded(
                    &excluded_points,
                    *handle,
                    glam::DVec3::new(x, y, 0.0),
                )
            };
            let before = self.tabs[i].scene.document.get_entity_arc(*handle);
            if let Some(entity) = before.as_deref() {
                use crate::scene::parametric_constraints::ParametricRef;
                match entity {
                    codec::EntityType::Line(line) => {
                        if in_win(line.start.x, line.start.y) {
                            driven_refs.push(ParametricRef::point(*handle, 0));
                        }
                        if in_win(line.end.x, line.end.y) {
                            driven_refs.push(ParametricRef::point(*handle, 1));
                        }
                    }
                    codec::EntityType::LwPolyline(polyline) => {
                        if let Some(world) = crate::entities::curve::lwpolyline_world_xy(polyline) {
                            for (index, vertex) in world.vertices.iter().enumerate() {
                                if in_win(vertex.location.x, vertex.location.y) {
                                    driven_refs.push(ParametricRef::point(*handle, index as i32));
                                }
                            }
                        }
                    }
                    codec::EntityType::Polyline2D(polyline) => {
                        for (index, vertex) in polyline.vertices.iter().enumerate() {
                            if in_win(vertex.location.x, vertex.location.y) {
                                driven_refs.push(ParametricRef::point(*handle, index as i32));
                            }
                        }
                    }
                    codec::EntityType::Circle(circle)
                        if in_win(circle.center.x, circle.center.y) =>
                    {
                        driven_refs.push(ParametricRef::center(*handle));
                    }
                    codec::EntityType::Arc(arc) if in_win(arc.center.x, arc.center.y) => {
                        driven_refs.push(ParametricRef::center(*handle));
                    }
                    _ => {}
                }
            }
            let Some(entity) = self.tabs[i].scene.document.get_entity_mut(*handle) else {
                continue;
            };
            let stretched = crate::modules::draw::modify::stretch::stretch_entity_except(
                entity,
                *handle,
                &windows,
                &excluded_points,
                delta,
            );
            if stretched && matches!(entity, codec::EntityType::Dimension(_)) {
                stretched_dims.push(*handle);
            }
            if stretched {
                if let Some(before) = before {
                    self.tabs[i].scene.record_undo_before(*handle, Some(before));
                }
                self.tabs[i].scene.mark_entity_dirty(*handle);
                changed_handles.push(*handle);
                count += 1;
            }
        }
        // A stretched dimension renders through its baked *D block when
        // one exists (file roundtrip) — drop it so tessellation falls
        // back to the live points and the next save re-bakes. (#372)
        for h in stretched_dims {
            self.tabs[i].scene.invalidate_dim_block_recorded(h);
        }

        // Geometry was edited in place via get_entity_mut, which the
        // scene's tessellation cache doesn't observe. Re-tessellate the
        // moved entities so the viewport reflects the stretch right away
        // instead of only on the next unrelated redraw. See #95.
        if count > 0 {
            let changes: Vec<_> = changed_handles
                .iter()
                .map(|&handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities_with_parametric_policy(
                &changes,
                &driven_refs,
                self.constraint_solve_mode && !driven_refs.is_empty(),
            );
            self.apply_inferred_constraints(i, &changed_handles);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.command_line
            .push_output(crate::tf!("STRETCH: {count} entity(ies) stretched.").as_ref());
        self.refresh_properties();
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        None
    }
}
