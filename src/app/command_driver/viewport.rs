use super::*;

impl OpenCADStudio {
    pub(super) fn handle_mview_create(&mut self, viewport: codec::entities::Viewport, preserve_view: bool) {
        let i = self.active_tab;
        let saved_view = preserve_view.then(|| {
            (
                viewport.view_target.clone(),
                viewport.view_direction.clone(),
                viewport.view_center.clone(),
                viewport.view_height,
                viewport.custom_scale,
                viewport.lens_length,
                viewport.twist_angle,
                viewport.status.perspective,
            )
        });
        let label = self.history_label_from_active_cmd(i, "MVIEW");
        let pending = self.begin_undo(i, label, 1, true);
        let handle = self.commit_entity_handle(codec::EntityType::Viewport(viewport));
        if let (Some(handle), Some(saved)) = (handle, saved_view) {
            if let Some(codec::EntityType::Viewport(viewport)) =
                self.tabs[i].scene.document.get_entity_mut(handle)
            {
                viewport.view_target = saved.0;
                viewport.view_direction = saved.1;
                viewport.view_center = saved.2;
                viewport.view_height = saved.3;
                viewport.custom_scale = saved.4;
                viewport.lens_length = saved.5;
                viewport.twist_angle = saved.6;
                viewport.status.perspective = saved.7;
            }
            self.tabs[i].scene.camera_generation += 1;
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
    }

    pub(super) fn handle_mview_create_clipped(
        &mut self,
        boundary: Option<codec::EntityType>,
        boundary_handle: Handle,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        if boundary.is_none() {
            let scene = &self.tabs[i].scene;
            let valid = scene.entity_belongs_to_current_layout(boundary_handle)
                && scene
                    .document
                    .get_entity(boundary_handle)
                    .is_some_and(|entity| match entity {
                        codec::EntityType::Circle(_) => true,
                        codec::EntityType::Ellipse(ellipse) => ellipse.is_full(),
                        codec::EntityType::LwPolyline(polyline) => polyline.is_closed,
                        codec::EntityType::Polyline(polyline) => polyline.is_closed(),
                        codec::EntityType::Polyline2D(polyline) => polyline.is_closed(),
                        codec::EntityType::Polyline3D(polyline) => polyline.flags.closed,
                        _ => false,
                    });
            if !valid {
                self.command_line.push_error(
                    crate::t!(
                        "MVIEW Object: select a closed paper-space circle, ellipse, or polyline."
                    )
                    .as_ref(),
                );
                if let Some(prompt) = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.prompt())
                {
                    self.command_line.push_info(&prompt);
                }
                return Some(Task::none());
            }
        }

        let created_boundary = boundary.is_some();
        let touched = 2;
        let label = self.history_label_from_active_cmd(i, "MVIEW");
        let pending = self.begin_undo(i, label, touched, true);
        let clip_handle = match boundary {
            Some(mut boundary) => {
                // A non-rectangular viewport owns a helper boundary
                // entity through `clip_boundary_handle`. Keep that
                // helper in the document for DWG compatibility and
                // stencil clipping, but do not expose it as a separate
                // selectable polyline.
                boundary.common_mut().invisible = true;
                match self.commit_entity_handle(boundary) {
                    Some(handle) => handle,
                    None => {
                        self.tabs[i].active_cmd = None;
                        if let Some(pending) = pending {
                            self.commit_undo_delta(i, pending);
                        }
                        return Some(Task::none());
                    }
                }
            }
            None => boundary_handle,
        };
        let polygon = self.tabs[i].scene.clip_boundary_polygon(clip_handle, 0.0);
        let bounds: Option<(f64, f64, f64, f64)> = polygon.iter().fold(None, |bounds, point| {
            if !point[0].is_finite() || !point[1].is_finite() {
                return bounds;
            }
            Some(match bounds {
                Some((min_x, min_y, max_x, max_y)) => (
                    min_x.min(point[0] as f64),
                    min_y.min(point[1] as f64),
                    max_x.max(point[0] as f64),
                    max_y.max(point[1] as f64),
                ),
                None => (
                    point[0] as f64,
                    point[1] as f64,
                    point[0] as f64,
                    point[1] as f64,
                ),
            })
        });
        let Some((min_x, min_y, max_x, max_y)) = bounds else {
            self.command_line
                .push_error(crate::t!("MVIEW: the clipping boundary has no usable area.").as_ref());
            self.tabs[i].active_cmd = None;
            if let Some(pending) = pending {
                self.commit_undo_delta(i, pending);
            }
            return Some(Task::none());
        };
        if max_x - min_x < 1e-6 || max_y - min_y < 1e-6 {
            self.command_line
                .push_error(crate::t!("MVIEW: the clipping boundary has no usable area.").as_ref());
            self.tabs[i].active_cmd = None;
            if let Some(pending) = pending {
                self.commit_undo_delta(i, pending);
            }
            return Some(Task::none());
        }

        let mut viewport = codec::entities::Viewport::new();
        viewport.center =
            codec::types::Vector3::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0, 0.0);
        viewport.width = max_x - min_x;
        viewport.height = max_y - min_y;
        viewport.id = 2;
        viewport.clip_boundary_handle = clip_handle;
        let viewport_handle = self.commit_entity_handle(codec::EntityType::Viewport(viewport));
        if let Some(viewport_handle) = viewport_handle {
            if !created_boundary {
                let before = self.tabs[i]
                    .scene
                    .document
                    .get_entity(clip_handle)
                    .cloned()
                    .map(std::sync::Arc::new);
                self.tabs[i].scene.record_undo_before(clip_handle, before);
            }
            if let Some(boundary) = self.tabs[i].scene.document.get_entity_mut(clip_handle) {
                let common = boundary.common_mut();
                common.invisible = true;
                if !common.reactors.contains(&viewport_handle) {
                    common.reactors.push(viewport_handle);
                }
            }
            self.tabs[i]
                .scene
                .bump_entities(&[(clip_handle, crate::scene::ChangeKind::Modified)]);
        }
        self.tabs[i].dirty = true;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        None
    }

    pub(super) fn handle_wipeout_from_polyline(
        &mut self,
        handle: Handle,
        erase_source: bool,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        let wipeout = {
            let scene = &self.tabs[i].scene;
            scene
                .entity_belongs_to_active_space(handle)
                .then(|| scene.document.get_entity(handle))
                .flatten()
                .and_then(crate::modules::draw::draw::wipeout::wipeout_from_polyline)
        };
        if let Some(wipeout) = wipeout {
            if erase_source {
                return Some(self.apply_cmd_result(CmdResult::ReplaceMany(
                    vec![(handle, Vec::new())],
                    vec![wipeout],
                )));
            }
            return Some(self.apply_cmd_result(CmdResult::CommitAndExit(wipeout)));
        }
        self.command_line.push_error(
            crate::t!("WIPEOUT Polyline: select a straight, closed, planar 2D polyline with at least 3 non-intersecting vertices.").as_ref(),
        );
        let command = crate::modules::draw::draw::wipeout::WipeoutCommand::new_polyline();
        self.command_line
            .push_info(&crate::command::CadCommand::prompt(&command));
        self.tabs[i].active_cmd = Some(Box::new(command));
        None
    }

    pub(super) fn handle_mview_switch_layout(&mut self, layout: String) -> Task<Message> {
        let i = self.active_tab;
        let task = self.on_layout_switch_preserving_command(layout);
        if let Some(prompt) = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|command| command.prompt())
        {
            self.command_line.push_info(&prompt);
        }
        task
    }

    pub(super) fn handle_mview_cancel_to_layout(&mut self, layout: String) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.restore_pre_cmd_tangent();
        self.on_layout_switch(layout)
    }

    pub(super) fn handle_vp_layer_update(
        &mut self,
        vp_handle: Handle,
        freeze: Vec<String>,
        thaw: Vec<String>,
    ) {
        let i = self.active_tab;
        // Resolve layer names → handles, then update frozen_layers on the viewport(s).
        // vp_handle == Handle::NULL means "apply to all viewports in current layout".
        let freeze_handles: Vec<Handle> = freeze
            .iter()
            .filter_map(|name| {
                self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .find(|l| l.name.eq_ignore_ascii_case(name))
                    .map(|l| l.handle)
            })
            .collect();
        let thaw_handles: Vec<Handle> = thaw
            .iter()
            .filter_map(|name| {
                self.tabs[i]
                    .scene
                    .document
                    .layers
                    .iter()
                    .find(|l| l.name.eq_ignore_ascii_case(name))
                    .map(|l| l.handle)
            })
            .collect();

        let mut frozen_count = 0usize;
        let mut thawed_count = 0usize;

        // Collect target viewport handles
        let target_handles: Vec<Handle> = if vp_handle == codec::Handle::NULL {
            // All viewports in current layout block
            let block_handle = self.tabs[i].scene.current_layout_block_handle_pub();
            self.tabs[i]
                .scene
                .document
                .entities()
                .filter(|e| {
                    e.common().owner_handle == block_handle
                        && matches!(e, codec::EntityType::Viewport(_))
                })
                .map(|e| e.common().handle)
                .collect()
        } else {
            vec![vp_handle]
        };

        for &target_handle in &target_handles {
            if let Some(codec::EntityType::Viewport(vp)) =
                self.tabs[i].scene.document.get_entity_mut(target_handle)
            {
                for h in &freeze_handles {
                    if !vp.frozen_layers.contains(h) {
                        vp.frozen_layers.push(*h);
                        frozen_count += 1;
                    }
                }
                for h in &thaw_handles {
                    let before = vp.frozen_layers.len();
                    vp.frozen_layers.retain(|fh| fh != h);
                    if vp.frozen_layers.len() < before {
                        thawed_count += 1;
                    }
                }
            }
        }

        if frozen_count > 0 || thawed_count > 0 {
            self.push_undo_snapshot(i, "VPLAYER");
            self.tabs[i].dirty = true;
            if frozen_count > 0 {
                self.command_line.push_info(
                    crate::tf!("VPLAYER: {frozen_count} layer(s) frozen in viewport.").as_ref(),
                );
            }
            if thawed_count > 0 {
                self.command_line.push_info(
                    crate::tf!("VPLAYER: {thawed_count} layer(s) thawed in viewport.").as_ref(),
                );
            }
            // Sync layer panel so VP freeze columns update immediately.
            let doc_layers = self.tabs[i].scene.document.layers.clone();
            let vp_info = self.tabs[i].scene.viewport_list();
            self.tabs[i]
                .layers
                .sync_with_viewports(&doc_layers, vp_info);
        }

        // Show updated prompt (command stays active for more operations).
        let prompt = self.tabs[i].active_cmd.as_ref().map(|c| c.prompt());
        if let Some(p) = prompt {
            self.command_line.push_info(&p);
        }
    }

    pub(super) fn handle_zoom_to_window(&mut self, p1: glam::DVec3, p2: glam::DVec3) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.tabs[i].scene.remember_current_view();
        self.tabs[i]
            .scene
            .zoom_to_window(p1.as_vec3(), p2.as_vec3());
        self.command_line
            .push_output(crate::t!("Zoom Window").as_ref());
    }

    pub(super) fn handle_set_plot_window(&mut self, p1: glam::DVec3, p2: glam::DVec3) {
        let i = self.active_tab;
        let layout_name = self.tabs[i].scene.current_layout.clone();
        if layout_name == "Model" {
            // Model space: remember the window (world X/Y) for the plot dialog.
            let x0 = p1.x.min(p2.x);
            let y0 = p1.y.min(p2.y);
            let x1 = p1.x.max(p2.x);
            let y1 = p1.y.max(p2.y);
            self.plot_window = Some((x0, y0, x1, y1));
            self.plot_dialog.window = self.plot_window;
            self.command_line.push_output(
                crate::tf!("Plot window: {x0:.2},{y0:.2} to {x1:.2},{y1:.2}").as_ref(),
            );
            // Pick window closed the plot dialog so the viewport could
            // receive the two clicks — bring the dialog back with the
            // window now active.
            self.plot_dialog.area = "Window".to_string();
            // Remember the pick immediately, like the printer choice.
            self.save_config();
            self.active_modal = Some(super::super::ModalKind::Plot);
        } else {
            // PLOTWINDOW always describes the plotted layout. In MSPACE
            // the command points are model coordinates, so map them back
            // through the active floating viewport first.
            let p1 = self.tabs[i].scene.model_to_paper(p1);
            let p2 = self.tabs[i].scene.model_to_paper(p2);
            let x1 = p1.x.min(p2.x);
            let y1 = p1.y.min(p2.y);
            let x2 = p1.x.max(p2.x);
            let y2 = p1.y.max(p2.y);
            self.plot_window = Some((x1, y1, x2, y2));
            self.plot_dialog.window = self.plot_window;
            self.command_line.push_output(
                crate::tf!("Plot window: {x1:.2},{y1:.2} to {x2:.2},{y2:.2}").as_ref(),
            );
            self.plot_dialog.area = "Window".to_string();
            self.save_config();
            self.active_modal = Some(super::super::ModalKind::Plot);
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
    }

    pub(super) fn handle_quick_print(&mut self, handles: Vec<Handle>) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        self.on_quick_print_handles(handles)
    }
}
