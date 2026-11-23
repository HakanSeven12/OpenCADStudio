use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn on_viewport_left_press(&mut self) -> Task<Message> {
        let i = self.active_tab;
        self.tabs[i].measurement.dismiss_result();
        self.block_palette.pressed = None;
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
        }
        // A left-click during a command resets the right-click cycle, so
        // the next right-click acts as Enter again rather than opening
        // the context menu.
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
            .input
            .right_click_entered = false;
        // A click in the viewport dismisses any open ribbon dropdown
        // (e.g. the annotation style combo), which has no backdrop of
        // its own to catch outside clicks.
        self.ribbon.close_dropdown();
        // Likewise dismiss the Properties color dropdowns — a viewport
        // press starts a box selection, so without this they'd only
        // close on the second click (issue #104).
        self.tabs[i].properties.color_picker_open = false;
        // Click anywhere outside the popup dismisses it. A hover popup must
        // not swallow a grip click beneath it; a pinned popup was opened by an
        // explicit click, so its outside click only dismisses the menu.
        if let Some(popup) = self.grip_popup.take() {
            self.grip_hover = None;
            if popup.pinned {
                return Task::none();
            }
        }
        // Outside-click dismiss for the visibility-state dropdown
        // (its buttons sit above this mouse_area).
        if self.visibility_popup.take().is_some() {
            return Task::none();
        }
        if self.table_popup.take().is_some() {
            return Task::none();
        }
        // Same dismiss-on-outside-click for the right-click
        // context menu: its panel is opaque, so a press that
        // reaches here is outside the menu.
        {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            if sel.menu.open_at.take().is_some() {
                return Task::none();
            }
        }
        // A pending client `getpoint` request turns this click into the
        // answer: the snapped world point under the cursor. The pick must
        // not change the selection while the person is answering, and only
        // the pinned document's viewport may answer.
        if self.tabs[i].active_cmd.is_none() {
            let answers = self
                .control
                .get_point
                .as_ref()
                .is_some_and(|session| session.document_id == self.tabs[i].id);
            if answers {
                let world = self.tabs[i].last_cursor_world;
                self.resolve_get_point(Some([world.x, world.y, world.z]));
                return Task::none();
            }
        }
        let (p, vp_size) = {
            let sel = self.tabs[i].scene.selection.borrow();
            let p = match sel.input.last_move_pos {
                Some(p) => p,
                None => return Task::none(),
            };
            (p, sel.view.vp_size)
        };
        let (vw, vh) = vp_size;

        if self.tabs[i].active_cmd.is_none() {
            if let Some((kind, references)) = self.constraint_glyph_under(i, p) {
                let mut handles: Vec<_> = references
                    .iter()
                    .map(|reference| reference.entity)
                    .collect();
                handles.sort_unstable_by_key(|handle| handle.value());
                handles.dedup();
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entities(&handles);
                self.refresh_selected_grips();
                self.refresh_properties();
                self.command_line
                    .push_info(&format!("{kind:?} constraint selected."));
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                return Task::none();
            }
        }

        // An engaged grip owns the next left press (click-move-click placement
        // or the release of a press-drag). Do not let the same press arm the
        // normal box/lasso state or trigger another viewport control; the
        // release path below will commit the grip.
        #[cfg(not(target_arch = "wasm32"))]
        let plugin_grip_active = self.tabs[i].active_plugin_grip.is_some();
        #[cfg(target_arch = "wasm32")]
        let plugin_grip_active = false;
        // While MTP / M2P runs from a hot grip, its point picks are clicks
        // for that modifier, not the grip placement.
        let grip_owns_press =
            self.tabs[i].active_grip.is_some() && self.tabs[i].active_cmd.is_none();
        if grip_owns_press || plugin_grip_active {
            // The press position can be newer than the last ViewportMove.
            // Refresh the custom drag through the native snap/constraint path
            // before consuming the gesture; its release then settles at the
            // exact event-time point. ViewportLeftPressAt's plain-selection
            // path has not already run a full move, so this emits one Move.
            let task = if plugin_grip_active {
                self.on_viewport_move(p)
            } else {
                Task::none()
            };
            self.tabs[i]
                .scene
                .selection
                .borrow_mut()
                .clear_left_selection_gesture();
            return task;
        }

        // Interactive navigation tools reuse the middle-button movement path,
        // so no selection/pick logic runs while the left button drives them.
        if self.tabs[i].orbit_mode || self.tabs[i].pan_mode || self.tabs[i].zoom_dynamic_mode {
            self.clear_navigation_hover(i);
            self.tabs[i].scene.remember_current_view();
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.middle_down = true;
            sel.input.middle_last_pos = Some(p);
            sel.input.middle_pan_from = None;
            if self.tabs[i].zoom_dynamic_mode {
                sel.gesture.box_anchor = Some(p);
                sel.gesture.box_current = Some(p);
                sel.gesture.box_crossing = false;
                sel.gesture.box_crossing_locked = true;
                // A fresh drag starts neutral: the magnifier shows "+" until
                // the pointer has actually moved far enough to mean zoom out.
                sel.input.zoom_dir_out = false;
            }
            return Task::none();
        }

        if vw > 1.0 && vh > 1.0 {
            let rot = self.tabs[i].scene.active_view_rotation_mat();
            // Map the cursor into whichever area owns the cube (active
            // viewport rect in paper, active tile in model) so the
            // consume-check lines up with the gizmo — same framing as
            // ViewportClick's snap hit-test.
            let (cx, cy, w, h) = match self.tabs[i]
                .scene
                .active_viewport
                .and_then(|hndl| self.tabs[i].scene.viewport_screen_rect(hndl, (vw, vh)))
            {
                Some(rect) => (p.x - rect.x, p.y - rect.y, rect.width, rect.height),
                None => {
                    let tb = self.tabs[i].scene.active_model_tile_bounds(vw, vh);
                    (p.x - tb.x, p.y - tb.y, tb.width, tb.height)
                }
            };
            if scene::hit_test(cx, cy, w, h, rot, VIEWCUBE_PX).is_some() {
                return Task::none();
            }
        }

        let custom_grip_at_cursor =
            self.tabs[i].active_cmd.is_none() && self.custom_block_grip_at_cursor(i, p, vw, vh);
        if custom_grip_at_cursor {
            self.ucs_icon_selected = false;
            self.ucs_icon_hover = false;
        }

        // UCS icon: click to select (grips appear), then drag a grip to
        // move/rotate. A click off the icon clears the selection. Works
        // in the corner too (parked icon), not just at the origin.
        if self.show_ucs_icon
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].active_block_edit_session().is_none()
            && !custom_grip_at_cursor
        {
            if let Some(hit) = self.ucs_icon_hit_info(i, vw, vh) {
                // Already selected → a press on a grip starts the drag.
                if self.ucs_icon_selected {
                    if let Some(kind) = ucs_grip_under(p, &hit) {
                        self.ucs_grip_drag = Some(kind);
                        return Task::none();
                    }
                }
                // Press on the icon body selects it (and shows grips).
                if over_ucs_icon(p, &hit) {
                    self.ucs_icon_selected = true;
                    self.ucs_icon_hover = true;
                    return Task::none();
                }
            }
            // Press elsewhere drops the selection, then falls through to
            // the normal pick / box-select so the click still lands.
            self.ucs_icon_selected = false;
        }

        // Divider resize is handled natively by the input pane_grid
        // (`on_resize`); the active pane already follows the cursor via
        // `focus_model_pane`. So a press here goes straight to picking.

        // From here the click targets the active tile: map the
        // cursor into it and use the tile's size for picking, so
        // grip / selection hit-tests land in the right pane.
        let p_full = p;
        // Inside a floating viewport the pane is the viewport's own rect
        // + camera (matches the GPU); otherwise the active model tile.
        let edit_frame = self.tabs[i].scene.viewport_edit_frame((vw, vh));
        let tile_b = match &edit_frame {
            Some((_, full)) => *full,
            None => self.tabs[i].scene.active_model_tile_bounds(vw, vh),
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = iced::Point {
            x: p_full.x - tile_b.x,
            y: p_full.y - tile_b.y,
        };
        let (vw, vh) = (tile_b.width, tile_b.height);
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: vw,
            height: vh,
        };

        // STRETCH: Shift+click an already selected vertex to subtract only
        // that vertex (not its entire entity). Use the same grip hit testing
        // as a normal click, including paper-space and viewport transforms.
        if self.shift_down
            && self.tabs[i].active_cmd.is_some()
            && !self.tabs[i].selected_grips.is_empty()
        {
            let grip_hit = if let Some(cam) = &edit_cam {
                find_hit_grip_rte(
                    p,
                    &self.tabs[i].selected_grips,
                    cam.view_proj_rte(bounds),
                    cam.eye(),
                    bounds,
                )
            } else if self.tabs[i].scene.current_layout != "Model" {
                let cam = self.tabs[i].scene.camera.borrow();
                let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
                let half_h = cam.ortho_size();
                let half_w = half_h * aspect;
                find_hit_grip_paper(
                    p,
                    &self.tabs[i].selected_grips,
                    cam.target.x as f32,
                    cam.target.y as f32,
                    half_w,
                    half_h,
                    bounds,
                )
            } else {
                let cam = self.tabs[i].scene.camera.borrow();
                find_hit_grip(p, &self.tabs[i].selected_grips, &cam, bounds)
            };
            if let Some((index, grip_id, _, world)) = grip_hit {
                if let Some(&handle) = self.tabs[i].selected_grip_handles.get(index) {
                    let removed = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .is_some_and(|command| command.remove_stretch_point(handle, world));
                    if removed {
                        self.tabs[i].selected_grips.remove(index);
                        self.tabs[i].selected_grip_handles.remove(index);
                        self.tabs[i].hot_grips.remove(&(handle, grip_id));
                        self.grip_hover = None;
                        self.tabs[i]
                            .scene
                            .selection
                            .borrow_mut()
                            .clear_left_selection_gesture();
                        return Task::none();
                    }
                }
            }
        }

        // STRETCH: Shift+click on an object (away from its grips) takes the
        // whole object out, which also works where vertices of several
        // objects coincide and a Shift+window would catch them all.
        if self.shift_down
            && self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.name() == "STRETCH")
        {
            let (view_rot, eye) = match &edit_cam {
                Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                None => {
                    let cam = self.tabs[i].scene.camera.borrow();
                    (cam.view_proj_rte(bounds), cam.eye())
                }
            };
            let wires = self.tabs[i].scene.hit_test_wires();
            let hit = scene::pick::hit_test::click_hit(
                p,
                &*wires,
                view_rot,
                eye,
                bounds,
                self.tabs[i].scene.document.header.lineweight_display,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
            )
            .and_then(Scene::handle_from_wire_name);
            drop(wires);
            if let Some(handle) = hit {
                let removed = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .is_some_and(|command| command.remove_stretch_entity(handle));
                if removed {
                    self.remove_stretch_object_feedback(i, handle);
                    return Task::none();
                }
            }
        }

        if self.tabs[i].active_cmd.is_none()
            && self.tabs[i].active_grip.is_none()
            && !self.tabs[i].selected_grips.is_empty()
        {
            {
                let is_paper = self.tabs[i].scene.current_layout != "Model";
                // In-viewport grips are model-space; project them with the
                // viewport camera so they hit-test where the GPU draws
                // them. Paper-space entities use the 2-D paper transform;
                // the model tab uses the model camera.
                let grip_hit = if self.tabs[i].active_block_edit_session().is_some()
                    && self.tabs[i].selected_authoring.is_some()
                {
                    self.authoring_grip_hit(p, bounds)
                } else if let Some(cam) = &edit_cam {
                    find_hit_grip_rte(
                        p,
                        &self.tabs[i].selected_grips,
                        cam.view_proj_rte(bounds),
                        cam.eye(),
                        bounds,
                    )
                } else if is_paper {
                    let cam = self.tabs[i].scene.camera.borrow();
                    let aspect = if vh > 0.0 { vw / vh } else { 1.0 };
                    let half_h = cam.ortho_size();
                    let half_w = half_h * aspect;
                    let tx = cam.target.x as f32;
                    let ty = cam.target.y as f32;
                    drop(cam);
                    find_hit_grip_paper(
                        p,
                        &self.tabs[i].selected_grips,
                        tx,
                        ty,
                        half_w,
                        half_h,
                        bounds,
                    )
                } else {
                    let cam = self.tabs[i].scene.camera.borrow();
                    find_hit_grip(p, &self.tabs[i].selected_grips, &cam, bounds)
                };
                if let Some((grip_index, grip_id, is_translate, world)) = grip_hit {
                    let Some(&handle) = self.tabs[i].selected_grip_handles.get(grip_index) else {
                        return Task::none();
                    };
                    let grip_shape = self.tabs[i].selected_grips[grip_index].shape;
                    if matches!(
                        grip_shape,
                        crate::scene::model::object::GripShape::Dropdown
                            | crate::scene::model::object::GripShape::DropdownAdjacent
                    ) {
                        use crate::entities::traits::EntityTypeOps;
                        let items = self.tabs[i]
                            .scene
                            .document
                            .get_entity(handle)
                            .map(|entity| entity.grip_menu(grip_id))
                            .unwrap_or_default();
                        if !items.is_empty() {
                            let selected = items
                                .iter()
                                .position(|item| item.label.starts_with('✓'))
                                .unwrap_or(0);
                            self.grip_popup = Some(crate::app::GripPopup {
                                handle,
                                grip_id,
                                anchor: p_full,
                                items,
                                selected,
                                pinned: true,
                            });
                        }
                        self.grip_hover = None;
                        return Task::none();
                    }
                    // The visibility (lookup) grip opens a state
                    // dropdown instead of starting a stretch drag.
                    if grip_id == crate::app::visibility::VIS_GRIP_ID {
                        self.open_visibility_popup(p);
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    if grip_id == crate::app::visibility::TABLE_GRIP_ID {
                        self.open_table_popup();
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    if self.start_test_action_grip(handle, grip_id) {
                        return Task::none();
                    }
                    if self.start_authoring_grip(handle, grip_id) {
                        return Task::none();
                    }
                    if self.start_array_grip(handle, grip_id) {
                        return Task::none();
                    }
                    if self.shift_down {
                        let key = (handle, grip_id);
                        if !self.tabs[i].hot_grips.remove(&key) {
                            self.tabs[i].hot_grips.insert(key);
                        }
                        self.grip_hover = None;
                        self.grip_popup = None;
                        return Task::none();
                    }
                    let mut edit = self.grip_edit_for_hit(i, handle, grip_id, is_translate, world);
                    if edit.gizmo {
                        // Where the gizmo part was grabbed: the drag keeps
                        // that offset from the cloud's insertion point.
                        let (view_rot, eye) = match &edit_cam {
                            Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                            None => {
                                let cam = self.tabs[i].scene.camera.borrow();
                                (cam.view_proj_rte(bounds), cam.eye())
                            }
                        };
                        let grabbed = match (edit.axis, edit.plane) {
                            (Some(axis), _) => {
                                cursor_on_projected_axis(p, bounds, view_rot, eye, world, axis)
                            }
                            (_, Some(normal)) => {
                                cursor_on_plane(p, bounds, view_rot, eye, world, normal)
                            }
                            _ => None,
                        };
                        edit.grab = grabbed.map(|point| point - world);
                    }
                    if edit.mode == GripEditMode::Radius {
                        self.grip_pending = Some(crate::app::GripPendingValue {
                            handle,
                            grip_id,
                            action: crate::scene::model::object::GripMenuAction::Radius,
                            label: "New radius",
                        });
                        self.command_line
                            .push_info(crate::t!("Specify point or enter radius:").as_ref());
                    }
                    self.tabs[i].active_grip = Some(edit);
                    self.grip_ctrl_cycle = None;
                    self.grip_engage_press = Some(p);
                    self.grip_hover = None;
                    self.grip_popup = None;
                    self.tabs[i]
                        .scene
                        .selection
                        .borrow_mut()
                        .clear_left_selection_gesture();

                    // A grip edit is not a CAD command, so explicitly seed the shared
                    // dynamic-input fields for the newly engaged grip.
                    self.sync_dyn_fields();

                    return Task::none();
                }
            }
        }

        // API v11: plugin-declared custom grips engage on press regardless of
        // the selection (always-visible display state). Grips are tested
        // before any wire pick, so a custom grip visually coincident with its
        // own reference-only marker — or any click-through geometry — stays
        // fully hittable and wins the hit.
        #[cfg(not(target_arch = "wasm32"))]
        if self.tabs[i].active_cmd.is_none() && self.tabs[i].active_grip.is_none() {
            if let Some(candidate) = self.hit_plugin_grip(i, p) {
                self.grip_hover = None;
                self.grip_popup = None;
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                let task = self.begin_plugin_grip_drag(i, &candidate);
                self.grip_engage_press = Some(p_full);
                self.sync_dyn_fields();
                return task;
            }
        }

        // Navigation, overlays and grips retain priority over document hyperlinks.
        if self.ctrl_down
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].scene.current_layout == "Model"
        {
            let (view_rot, eye) = {
                let cam = self.tabs[i].scene.camera.borrow();
                (cam.view_proj_rte(bounds), cam.eye())
            };
            let wires = self.tabs[i].scene.hit_test_wires();
            let url = scene::pick::hit_test::click_hit(
                p,
                &*wires,
                view_rot,
                eye,
                bounds,
                self.tabs[i].scene.document.header.lineweight_display,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
            )
            .and_then(Scene::handle_from_wire_name)
            .and_then(|handle| self.tabs[i].scene.document.get_entity(handle))
            .and_then(scene::pe_url_of)
            .and_then(crate::sys::web_hyperlink);
            if let Some(url) = url {
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                self.command_line
                    .push_info(&format!("{}: {url}", crate::t!("Hyperlink")));
                return crate::sys::open_url(&url, self.main_window);
            }
        }

        // Ctrl+click on a polyline picks the segment under the cursor; the
        // edit commands then move just that part.
        // SUbobject at a selection prompt picks segments the same way.
        let subobject = self.subobject_mode();
        if self.ctrl_down && self.tabs[i].active_cmd.is_none() || subobject {
            let (view_rot, eye) = {
                let cam = self.tabs[i].scene.camera.borrow();
                (cam.view_proj_rte(bounds), cam.eye())
            };
            let wires = self.tabs[i].scene.hit_test_wires();
            let hit = scene::pick::hit_test::click_hit(
                p,
                &*wires,
                view_rot,
                eye,
                bounds,
                self.tabs[i].scene.document.header.lineweight_display,
                crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                &self.tabs[i].scene.draw_depth_map(),
            )
            .and_then(Scene::handle_from_wire_name)
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
            drop(wires);
            let point = self.tabs[i].last_cursor_world;
            if let Some(handle) =
                hit.filter(|handle| self.tabs[i].scene.toggle_sub_segment(*handle, point))
            {
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                if subobject {
                    return self.feed_subobject_pick(Some(handle));
                }
                self.refresh_properties();
                return Task::none();
            }
            let solid = |entity: Option<&document::EntityType>| {
                matches!(
                    entity,
                    Some(
                        document::EntityType::Solid3D(_)
                            | document::EntityType::Region(_)
                            | document::EntityType::Body(_)
                            | document::EntityType::Surface(_)
                            | document::EntityType::Mesh(_)
                    )
                )
            };
            if subobject
                && hit.is_some_and(|handle| !solid(self.tabs[i].scene.document.get_entity(handle)))
            {
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                return self.feed_subobject_pick(None);
            }
        }

        let authoring_label = self.authoring_label_at(p, bounds);
        if self.pick_authoring_object(p, bounds) {
            if let Some((handle, name)) = authoring_label
                .filter(|(handle, _)| self.tabs[i].selected_authoring == Some(*handle))
            {
                let now = Instant::now();
                let is_double = self.last_vp_click_time.is_some_and(|last| {
                    now.duration_since(last).as_millis() < 400
                        && self.last_vp_click_pos.is_some_and(|previous| {
                            (p.x - previous.x).hypot(p.y - previous.y) < 8.0
                        })
                });
                self.last_vp_click_time = Some(now);
                self.last_vp_click_pos = Some(p);
                if is_double && self.authoring_name_is_editable(handle) {
                    use crate::scene::block_authoring::metadata::{self, Field};
                    let (field, name) = self.tabs[i]
                        .active_block_edit_session()
                        .and_then(|session| {
                            metadata::read(&self.tabs[i].scene.document, session.br_handle, handle)
                                .ok()
                        })
                        .and_then(|metadata| {
                            let text = metadata
                                .text
                                .iter()
                                .find(|text| {
                                    matches!(text.field, Field::Name | Field::SecondaryName)
                                        && text.value == name
                                })
                                .or_else(|| {
                                    metadata.text.iter().find(|text| text.field == Field::Name)
                                })?;
                            Some((text.field.key(), text.value.to_owned()))
                        })
                        .unwrap_or(("authoring_name", name));
                    let key = crate::ui::properties::FieldKey::Geom(field);
                    self.tabs[i].properties.edit_buf.insert(key, name);
                    self.tabs[i].properties.active_field = None;
                    let id = crate::ui::properties::prop_geom_field_id(field);
                    return Task::batch([
                        iced::widget::operation::focus(id.clone()),
                        iced::widget::operation::select_all(id),
                    ]);
                }
            }
            return Task::none();
        }

        // Ctrl+click on a composite solid picks the solid of it under the
        // cursor — the one the Properties palette and grips then edit.
        if self.ctrl_down
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].scene.current_layout == "Model"
        {
            let (view_rot, eye) = {
                let cam = self.tabs[i].scene.camera.borrow();
                (cam.view_proj_rte(bounds), cam.eye())
            };
            let hit = self.tabs[i]
                .scene
                .solid_click_hit_point(p, view_rot, eye, bounds);
            if let Some((handle, _)) = hit.filter(|(handle, point)| {
                crate::scene::model::solid_history::select_operand_at(
                    &self.tabs[i].scene.document,
                    *handle,
                    *point,
                )
            }) {
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entity(handle, false);
                self.refresh_properties();
                return Task::none();
            }
        }

        // A command step that takes a point or a pick commits on the press,
        // so the cursor moving before the button comes up cannot move it.
        // Selection steps and drag-fence picks still wait for the release.
        let picks_on_press = self.command_picks_on_press();
        // A press on a selected object may become a drag of the selection.
        self.drag_move = (self.tabs[i].active_cmd.is_none()
            && self.pick_first
            && (self.pick_drag_mode == 0 || self.pick_auto & 2 == 0)
            && self.tabs[i].scene.selection.borrow().gesture.box_anchor.is_none()
            && !self.shift_down
            && !self.tabs[i].scene.selected.is_empty()
            // A press inside a table cell drags out a cell range instead.
            && self.table_cell_under(i, p, bounds, &edit_cam, 0.0).is_none()
            && self.selected_object_under(i, p, bounds, &edit_cam))
        .then_some((self.tabs[i].last_cursor_world, false));
        let mut sel = self.tabs[i].scene.selection.borrow_mut();
        sel.input.left_down = true;
        if crate::perf::enabled() {
            crate::perf_record_diagnostic!(
                "[perf] viewport-press  press_pos={:?} cmd_active={}",
                sel.input.left_press_pos,
                self.tabs[i].active_cmd.is_some(),
            );
        }
        // Stored in full-canvas space (like ViewportMove's cursor and
        // the overlay box / lasso drawing); release maps it into the
        // active tile. Tile-local here would double-offset the anchor.
        sel.input.left_press_pos = Some(p_full);
        sel.input.left_press_time = Some(Instant::now());
        sel.input.left_dragging = false;
        sel.input.area_drag_blocked = false;
        drop(sel);
        if picks_on_press {
            // The command path clears `left_down`, so the real release
            // that follows finds nothing pressed and does nothing.
            return self.on_viewport_left_release();
        }
        Task::none()
    }

    pub(in crate::app) fn on_viewport_left_release(&mut self) -> Task<Message> {
        let i = self.active_tab;
        if crate::perf::enabled() {
            let sel = self.tabs[i].scene.selection.borrow();
            crate::perf_record_diagnostic!(
                "[perf] viewport-release left_down={} anchor={:?} dragging={}",
                sel.input.left_down,
                sel.gesture.box_anchor,
                sel.input.left_dragging,
            );
        }
        // A click fires after the cursor has settled — clear any pending
        // deferred snap so the release's own snap runs at full fidelity.
        self.snap_dwell = None;
        self.box_preview_pending = None;
        self.box_preview_due = None;
        self.box_preview_cache = None;
        // The release commits (or restarts) any box; its preview is done. A
        // Fence / polygon being picked keeps its own: the click only adds a
        // vertex where the cursor already stands.
        let path = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|command| command.selection_path().is_some());
        if !path {
            self.tabs[i].scene.set_box_hover_highlights(&[]);
        }
        // A block dragged from the Blocks palette lands here.
        if let Some(item) = self.take_block_drop() {
            return self.drop_block(item);
        }
        // A dragged selection drops where the button comes up: the release
        // is the MOVE / COPY destination click, and a COPY places one copy.
        if let Some((_, true)) = self.drag_move.take() {
            {
                // Starting the command reset the press; the drop reads as a
                // click at the cursor.
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.input.left_down = true;
                sel.input.left_dragging = false;
            }
            let placed = self.on_viewport_left_release();
            let copying = self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.name() == "COPY");
            if !copying {
                return placed;
            }
            return Task::batch([placed, self.feed_command(crate::command::StepInput::Enter)]);
        }
        if let Some(command) = self.tabs[i].active_cmd.as_mut() {
            command.set_ctrl(self.ctrl_down);
            command.set_shift(self.shift_down);
            if command.consume_stretch_remove_release() {
                self.tabs[i]
                    .scene
                    .selection
                    .borrow_mut()
                    .clear_left_selection_gesture();
                return Task::none();
            }
        }

        // Navigation mode: end this drag but keep the tool armed for the next
        // left drag (exit is Esc / another command). Mirror of the press.
        if self.tabs[i].orbit_mode || self.tabs[i].pan_mode || self.tabs[i].zoom_dynamic_mode {
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.middle_down = false;
            sel.input.middle_last_pos = None;
            sel.orbit.pivot = None;
            sel.gesture.box_anchor = None;
            sel.gesture.box_anchor_world = None;
            sel.gesture.box_current = None;
            sel.gesture.box_crossing_locked = false;
            sel.input.zoom_dir_out = false;
            drop(sel);
            self.arm_hover_after_navigation(i);
            return self.refresh_pointer_after_navigation(i);
        }

        // Commit a UCS icon grip drag: persist the new UCS so it
        // round-trips, and clear the lingering press state. In BEDIT this
        // rebases only the block; elsewhere it writes the model/viewport UCS.
        if self.ucs_grip_drag.take().is_some() {
            self.commit_active_ucs_change(i, "UCS");
            self.tabs[i].snap_result = None;
            self.snapper.from_point = None;
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.left_down = false;
            sel.input.left_press_pos = None;
            sel.input.left_dragging = false;
            return Task::none();
        }

        let (p, is_click, is_down) = {
            let sel = self.tabs[i].scene.selection.borrow();
            let last = match sel.input.last_move_pos {
                Some(p) => p,
                None => return Task::none(),
            };
            let is_click = !sel.input.left_dragging;
            let p = selection_click_position(
                last,
                sel.input.left_press_pos,
                is_click,
                self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_none_or(|command| command.is_selection_gathering()),
            );
            (p, is_click, sel.input.left_down)
        };

        // Custom grips mirror native click-move-click: the engaging bare
        // click leaves the grip hot; a press-drag release or the next click
        // settles. The second press first refreshes the event-time point via
        // the active-grip guard, then this release performs the settle.
        #[cfg(not(target_arch = "wasm32"))]
        if self.tabs[i].active_plugin_grip.is_some() {
            {
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.clear_left_selection_gesture();
            }
            let engaging_click = self.grip_engage_press.take().is_some_and(|press| {
                let (dx, dy) = (p.x - press.x, p.y - press.y);
                dx * dx + dy * dy <= 12.0 * 12.0
            });
            let moved = self.tabs[i]
                .active_plugin_grip
                .as_ref()
                .is_some_and(|active| active.moved);
            if engaging_click && !moved {
                return Task::none();
            }
            let task = self.settle_plugin_grip_drag();
            self.sync_dyn_fields();
            return task;
        }

        // Grip editing: click-move-click (plus legacy press-drag).
        // The grip engages on press (active_grip set). This release
        // commits only if the grip has actually moved, or if it was a
        // press-drag. A bare engaging click (no movement yet) keeps the
        // grip hot so the user can move the cursor and click again to
        // place it. Escape cancels (handled elsewhere).
        if let Some(grip) = self.tabs[i]
            .active_grip
            .clone()
            .filter(|_| self.tabs[i].active_cmd.is_none())
        {
            // Reset mouse state so the lingering press from the engaging
            // click doesn't read as an in-progress drag on later moves.
            {
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.clear_left_selection_gesture();
            }
            // Grip-menu Base Point: this click re-bases the gesture at the
            // cursor instead of placing anything.
            if self.tabs[i].grip_base_pending {
                self.tabs[i].grip_base_pending = false;
                if let Some(active) = self.tabs[i].active_grip.as_mut() {
                    active.origin_world = active.last_world;
                }
                return Task::none();
            }
            // The press that picked the grip up, released within the drag
            // threshold, was a click however the hand trembled: stay hot
            // and wait for the placement click.
            let engaging_click = self.grip_engage_press.take().is_some_and(|press| {
                let (dx, dy) = (p.x - press.x, p.y - press.y);
                dx * dx + dy * dy <= 12.0 * 12.0
            });
            if !engaging_click && self.grip_ctrl_one_shot(i).is_some() {
                return self.commit_grip_ctrl_one_shot(i);
            }
            let moved = grip.last_world != grip.origin_world;
            if engaging_click || (is_click && !moved) {
                return Task::none();
            }
            return self.commit_active_grip_edit();
        }

        // Map the release point into the active Model tile so the
        // click's pick / on_point / selection use the active pane's
        // camera + bounds. `p_full` keeps the canvas point for the
        // box/poly selection rectangle (drawn in canvas space).
        let p_full = p;
        // Inside a floating viewport the pane is the viewport's own rect
        // and camera (see the MoveCursor path); pick / snap then run in
        // model space exactly where the GPU draws the content.
        let canvas_sz = self.tabs[i].scene.selection.borrow().view.vp_size;
        let edit_frame = self.tabs[i].scene.viewport_edit_frame(canvas_sz);
        let (tile_vw, tile_vh, tile_off) = match &edit_frame {
            Some((_, full)) => (full.width, full.height, iced::Point::new(full.x, full.y)),
            None => {
                let tb = self.tabs[i]
                    .scene
                    .active_model_tile_bounds(canvas_sz.0, canvas_sz.1);
                (tb.width, tb.height, iced::Point::new(tb.x, tb.y))
            }
        };
        let edit_cam = edit_frame.map(|(cam, _)| cam);
        let p = iced::Point {
            x: p_full.x - tile_off.x,
            y: p_full.y - tile_off.y,
        };

        let is_gathering = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.is_selection_gathering())
            .unwrap_or(false);
        let selection_box_active = self.tabs[i]
            .scene
            .selection
            .borrow()
            .gesture
            .box_anchor
            .is_some();
        let selection_pick_add = self.pick_add
            || self.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.selection_forces_add());
        // A committed window corner must stay free of the Ortho/Polar
        // lock too, so the picked rectangle isn't flattened (#291).
        let is_window_corner = self.tabs[i]
            .active_cmd
            .as_ref()
            .map(|c| c.window_corner_pick())
            .unwrap_or(false);
        // An internal-point pick commits the bare cursor position (#2009).
        let raw_pick = self.tabs[i]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.wants_raw_point());
        let free_point = is_window_corner || raw_pick;

        if is_down
            && is_click
            && self.tabs[i].active_cmd.is_some()
            && !is_gathering
            && !selection_box_active
        {
            let (vw, vh) = (tile_vw, tile_vh);
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vw,
                height: vh,
            };

            // Keep the visible discrete candidate only as a ranking hint for
            // this click. The snap engine MUST rediscover it from the actual
            // clicked geometry; we never commit a stale cached coordinate.
            // Results on the tab are canvas-space, while the snap engine
            // receives tile-local pixels.
            let visible_before_click = self.tabs[i].snap_result.filter(|hit| {
                let dx = hit.screen.x - p_full.x;
                let dy = hit.screen.y - p_full.y;
                dx * dx + dy * dy <= self.snapper.osnap_radius_px.powi(2)
            });
            self.tabs[i].snap_result = None;
            let snapped;
            let tangent_obj_at_click;

            let world_pt = {
                // Cursor → model point (viewport camera inside a viewport,
                // else paper sheet → model). Model space throughout.
                let raw = self.cursor_model_point(i, &edit_cam, p, bounds);
                let (view_rot, eye) = match &edit_cam {
                    Some(cam) => (cam.view_proj_rte(bounds), cam.eye()),
                    None => {
                        let c = self.tabs[i].scene.camera.borrow();
                        (c.view_proj_rte(bounds), c.eye())
                    }
                };
                let snap_cursor = raw;
                let entity_pick = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .filter(|c| c.needs_entity_pick());
                let needs_entity_click = entity_pick.is_some();
                let entity_click_takes_points =
                    entity_pick.is_some_and(|command| command.entity_pick_accepts_points());
                let is_gathering = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|c| c.is_selection_gathering());
                // An entity pick or a selection gather never snaps, so it
                // skips the snap candidate query too.
                let skips_snap = (needs_entity_click && !entity_click_takes_points) || is_gathering;
                let click_source_started = crate::perf::enabled().then(Instant::now);
                let all_wires =
                    if let (Some(_), Some(h)) = (&edit_cam, self.tabs[i].scene.active_viewport) {
                        self.tabs[i]
                            .scene
                            .model_wires_for_viewport_arc(h, bounds.height)
                    } else {
                        self.tabs[i].scene.hit_test_wires()
                    };
                let click_source_ms = crate::perf::elapsed_ms(click_source_started);
                let click_wire_count = all_wires.len();
                let click_query_started = crate::perf::enabled().then(Instant::now);
                let snap_candidates = if skips_snap {
                    scene::pick::interaction_index::InteractionCandidates::pending(all_wires)
                } else {
                    self.tabs[i].scene.interaction_candidates_near(
                        all_wires,
                        snap_cursor,
                        view_rot,
                        eye,
                        bounds,
                        self.snapper.osnap_radius_px,
                    )
                };
                let click_query_ms = crate::perf::elapsed_ms(click_query_started);
                let click_evaluate_started = crate::perf::enabled().then(Instant::now);
                let needs_tan = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.needs_tangent_pick())
                    .unwrap_or(false);
                // Probe the active OTRACK ray before snapping so its geometry
                // intersection can be tested in the same candidate pass.
                let tracking_probe = if !needs_entity_click && !needs_tan && !free_point {
                    self.active_otrack_hit(
                        i,
                        raw,
                        None,
                        self.last_point,
                        true,
                        view_rot,
                        eye,
                        bounds,
                    )
                } else {
                    None
                };
                let mut snap_hit = if skips_snap {
                    None
                } else if needs_tan {
                    self.snapper.snap_tangent_only(
                        snap_cursor.as_vec3(),
                        p,
                        &snap_candidates,
                        view_rot,
                        eye,
                        bounds,
                    )
                } else {
                    let (go, gr) = self.drafting_grid_basis(i);
                    self.snapper.from_point = self.last_point;

                    let construction_ray = if free_point {
                        None
                    } else {
                        self.last_point.and_then(|base| {
                            self.active_construction_ray(
                                i,
                                p,
                                snap_cursor,
                                base,
                                view_rot,
                                eye,
                                bounds,
                            )
                        })
                    };
                    let additional_rays =
                        tracking_probe.map(|track| [(track.base, track.base + track.dir)]);

                    self.snapper.snap_with_additional_construction_rays_stable(
                        snap_cursor,
                        p,
                        &snap_candidates,
                        view_rot,
                        eye,
                        bounds,
                        go,
                        gr,
                        construction_ray,
                        additional_rays.as_ref().map_or(&[], |rays| rays.as_slice()),
                        visible_before_click,
                    )
                };
                // Paper-space snapping through layout viewports. Mirrors
                // the cursor-move pass: the click recomputes snapping from
                // scratch, so the viewport query has to run here too. The hit
                // comes back already projected onto the sheet.
                let mut click_frame: Option<crate::scene::viewport_ref::ViewportFrame> = None;
                if edit_cam.is_none() && !needs_entity_click && !needs_tan {
                    if let Some((vp_hit, frame)) =
                        self.paper_viewport_snap(i, p_full, (vw, vh), raw)
                    {
                        let merged = crate::snap::merge_snap(snap_hit, Some(vp_hit), p_full);
                        if merged.is_some_and(|hit| hit.viewport.is_some()) {
                            click_frame = Some(frame);
                        }
                        snap_hit = merged;
                    }
                }
                // Solid-face snaps (Nearest/Perpendicular to face), mirroring
                // the cursor-move pass so click and preview agree. Merged
                // before the accepted-snap record so face sources associate.
                if !needs_entity_click
                    && !needs_tan
                    && !free_point
                    && (self
                        .snapper
                        .is_on_3d(crate::snap::SnapType::FacePerpendicular)
                        || self.snapper.is_on_3d(crate::snap::SnapType::NearestFace))
                {
                    let face_hit = self.tabs[i].scene.solid_face_snaps(
                        p,
                        view_rot,
                        eye,
                        bounds,
                        self.snapper.osnap_radius_px,
                        self.last_point,
                        self.snapper
                            .is_on_3d(crate::snap::SnapType::FacePerpendicular),
                        self.snapper.is_on_3d(crate::snap::SnapType::NearestFace),
                    );
                    if face_hit.is_some() {
                        snap_hit = crate::snap::merge_snap(snap_hit, face_hit, p);
                    }
                }
                // Mouse-up must resolve the same original snap as mouse-move;
                // otherwise the highlighted reference point and committed
                // STRETCH destination could disagree.
                snap_hit = crate::snap::merge_snap(
                    snap_hit,
                    self.command_original_snap_hit(i, p, view_rot, eye, bounds),
                    p,
                );
                let click_evaluate_ms = crate::perf::elapsed_ms(click_evaluate_started);
                if crate::perf::enabled()
                    && click_source_ms + click_query_ms + click_evaluate_ms >= 1.0
                {
                    crate::perf_record!(
                        "[perf] snap-click source={click_source_ms:.2}ms query={click_query_ms:.2}ms evaluate={click_evaluate_ms:.2}ms input_wires={click_wire_count} candidates={} snap_on={} snap3d_on={} tangent={needs_tan}",
                        snap_candidates.len(),
                        self.snapper.snap_enabled,
                        self.snapper.snap3d_enabled,
                    );
                }
                if raw_pick {
                    snap_hit = None;
                }
                self.pending_click_snap = snap_hit.map(|hit| (hit, click_frame));
                snapped = snap_hit.is_some();
                tangent_obj_at_click = snap_hit.and_then(|hit| hit.tangent_obj);
                // Snap runs in model space; the result is already model.
                let mut pt = snap_hit.map(|s| s.world).unwrap_or(raw);
                // When no UCS is active clamp to world XY; with a UCS the point is
                // already constrained to that plane by the ray–plane intersection.
                // A genuine 3D object snap keeps its elevation — only grid snaps
                // and misses are flattened.
                let uses_command_cursor_plane = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.cursor_plane())
                    .is_some();
                let clamp_world_xy = self.tabs[i].active_ucs.is_none()
                    && !uses_command_cursor_plane
                    && !snap_keeps_elevation(snap_hit.map(|s| s.snap_type));
                if clamp_world_xy {
                    pt.z = 0.0;
                }
                let axis_lock = if let Some(base) = self.last_point {
                    self.active_axis_lock(
                        i,
                        raw,
                        base,
                        !needs_entity_click
                            && !needs_tan
                            && !free_point
                            && !uses_command_cursor_plane,
                    )
                } else {
                    self.axis_lock_dir = None;
                    None
                };
                if let (Some(dir), Some(base)) = (axis_lock, self.last_point) {
                    pt = axis_lock_apply(pt, base, dir);
                    if clamp_world_xy {
                        pt.z = 0.0;
                    }
                }
                let otrack = if axis_lock.is_none() {
                    self.active_otrack_hit(
                        i,
                        raw,
                        snap_hit,
                        self.last_point,
                        !free_point,
                        view_rot,
                        eye,
                        bounds,
                    )
                } else {
                    None
                };
                if let Some(h) = otrack.filter(|_| {
                    // Match the hover path: preserve exact object snaps, but
                    // allow Extension to resolve a crossing with OTRACK.
                    !snap_hit.is_some_and(|s| {
                        !matches!(
                            s.snap_type,
                            crate::snap::SnapType::Grid | crate::snap::SnapType::Extension
                        )
                    })
                }) {
                    // Commit the actual OTRACK intersection, not the cursor
                    // projection onto Extension.
                    pt = h.aligned;
                    if clamp_world_xy {
                        pt.z = 0.0;
                    }
                } else if axis_lock.is_none()
                    && !free_point
                    && !uses_command_cursor_plane
                    && !snap_hit.is_some_and(|s| s.snap_type != crate::snap::SnapType::Grid)
                {
                    // Object snap wins over ortho/polar — a snapped point
                    // commits as-is. Grid snap still combines. (#132)
                    if let Some((point, _)) = self
                        .last_point
                        .and_then(|base| self.drafting_point(i, p, pt, base, view_rot, eye, bounds))
                    {
                        pt = point;
                    }
                }
                pt = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .and_then(|command| command.cursor_axis())
                    .and_then(|(origin, direction)| {
                        command_axis_point(snap_hit, p, bounds, view_rot, eye, origin, direction)
                    })
                    .unwrap_or(pt);
                // A click while dynamic-input fields hold typed values
                // commits the CONSTRAINED point — the same resolution
                // the preview shows and Enter would commit (#356).
                if !needs_entity_click
                    && self.tabs[i].active_cmd.is_some()
                    && self.tabs[i].dyn_fields.iter().any(|f| f.buffer.is_some())
                {
                    self.tabs[i].last_cursor_world = pt;
                    if let Some(r) = self.dyn_resolve_point() {
                        pt = r;
                    }
                }
                if entity_click_takes_points {
                    snap_hit.map_or(raw, |hit| hit.world)
                } else {
                    pt
                }
            };

            // `world_pt` is in offset-relative (local) space, matching
            // the camera and the point-creation commands. Entity-pick /
            // tangent / structure-pick commands instead compare the
            // click against WCS document entities, so they need the
            // world_offset added back (model space only — paper-space
            // entities are already in sheet coordinates). Without this,
            // TRIM/EXTEND/FILLET pick the wrong side on UTM-scale files.
            let pick_wcs = {
                let wo = if self.tabs[i].scene.current_layout == "Model" {
                    [0.0_f64; 3]
                } else {
                    [0.0; 3]
                };
                world_pt + glam::DVec3::new(wo[0], wo[1], wo[2])
            };

            if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                command.set_ctrl(self.ctrl_down);
                command.set_shift(self.shift_down);
            }
            let result = if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_structure_point_pick())
                .unwrap_or(false)
            {
                let pick = self.tabs[i].active_cmd.as_ref().and_then(|c| {
                    c.resolve_object_pick(&self.tabs[i].scene, pick_wcs.x as f64, pick_wcs.y as f64)
                });
                if let Some(pick) = pick {
                    let center = glam::DVec3::new(pick.x, pick.y, pick_wcs.z);
                    let result = self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_structure_pick(pick.handle, center));
                    self.command_line
                        .push_info(crate::tf!("{} acquired.", pick.label).as_ref());
                    result
                } else {
                    let msg = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.object_pick_miss_message())
                        .unwrap_or("No object near click.");
                    if msg.is_empty() {
                        self.tabs[i]
                            .active_cmd
                            .as_mut()
                            .map(|c| c.on_point(pick_wcs))
                    } else {
                        self.command_line.push_error(msg);
                        None
                    }
                }
            } else if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_entity_pick())
                .unwrap_or(false)
            {
                let pick_aperture = crate::ui::overlay::pick_box_aperture_px(self.pick_box);
                let (view_rot2, eye2, all_wires2) = self.pick_view(i, &edit_cam, bounds);
                let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                    all_wires2,
                    world_pt,
                    view_rot2,
                    eye2,
                    bounds,
                    pick_aperture * 2.0,
                );
                let include_fills = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.entity_pick_includes_fills())
                    .unwrap_or(false);
                let candidate_handles = if include_fills {
                    self.tabs[i]
                        .scene
                        .interaction_candidate_handles(&click_candidates)
                } else {
                    None
                };
                let hit = scene::pick::hit_test::click_hit(
                    p,
                    &click_candidates,
                    view_rot2,
                    eye2,
                    bounds,
                    self.tabs[i].scene.document.header.lineweight_display,
                    pick_aperture,
                    &self.tabs[i].scene.draw_depth_map(),
                )
                .and_then(Scene::handle_from_wire_name)
                .or_else(|| {
                    if !include_fills {
                        return None;
                    }
                    scene::pick::hit_test::click_hit_hatch(
                        p,
                        &self.tabs[i]
                            .scene
                            .visible_hatches_for_click(candidate_handles.as_ref()),
                        view_rot2,
                        eye2,
                        bounds,
                        crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                        &self.tabs[i].scene.draw_depth_map(),
                        candidate_handles.as_ref(),
                    )
                })
                .or_else(|| {
                    include_fills.then(|| {
                        scene::pick::hit_test::click_hit_insert_hatch(
                            p,
                            self.tabs[i].scene.insert_hatches_for_click().as_ref(),
                            view_rot2,
                            eye2,
                            bounds,
                            crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                            &self.tabs[i].scene.draw_depth_map(),
                            candidate_handles.as_ref(),
                        )
                    })?
                })
                .or_else(|| {
                    include_fills.then(|| {
                        self.tabs[i].scene.solid_click_hit(
                            p,
                            view_rot2,
                            eye2,
                            bounds,
                            candidate_handles.as_ref(),
                        )
                    })?
                })
                .or_else(|| {
                    // A solid's edge: a face seen edge-on draws only there.
                    include_fills.then(|| {
                        self.tabs[i].scene.solid_edge_hover_hit(
                            p,
                            view_rot2,
                            eye2,
                            bounds,
                            candidate_handles.as_ref(),
                            pick_aperture,
                        )
                    })?
                });
                // A dimension pick inside a viewport measures the model
                // geometry shown there, never the viewport entity itself: with
                // nothing under the cursor the click is a sheet point (#3203).
                let hit = hit.filter(|&handle| {
                    let measures = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.measures_through_viewports());
                    !measures
                        || !matches!(
                            self.tabs[i].scene.document.get_entity(handle),
                            Some(document::EntityType::Viewport(_))
                        )
                });
                if let Some(handle) = hit {
                    let uses_surface_point = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|command| command.entity_pick_uses_surface_point());
                    let entity_pick_point = if uses_surface_point {
                        self.solid_pick_point(i, handle, &edit_cam, p, bounds, view_rot2, eye2)
                            .or_else(|| {
                                self.tabs[i]
                                    .active_cmd
                                    .as_ref()
                                    .is_some_and(|command| command.entity_pick_deferred_hover())
                                    .then(|| {
                                        self.profile_pick_point(i, handle, &edit_cam, p, bounds)
                                    })
                                    .flatten()
                            })
                            .unwrap_or_else(|| {
                                let bounded_pick = self.tabs[i]
                                    .active_cmd
                                    .as_ref()
                                    .is_some_and(|command| command.entity_pick_deferred_hover());
                                if bounded_pick
                                    && matches!(
                                        self.tabs[i].scene.document.get_entity(handle),
                                        Some(document::EntityType::Solid3D(_)),
                                    )
                                {
                                    // The aperture caught an edge outside its face.
                                    // Do not reinterpret the working-plane point as
                                    // a hit on another cap of the same body.
                                    glam::DVec3::NAN
                                } else {
                                    pick_wcs
                                }
                            })
                    } else {
                        pick_wcs
                    };
                    let entity_pick_direction =
                        if uses_surface_point && entity_pick_point.is_finite() {
                            if matches!(
                                self.tabs[i].scene.document.get_entity(handle),
                                Some(document::EntityType::Solid3D(_))
                            ) {
                                self.tabs[i]
                                    .scene
                                    .solid_planar_face_normal_at(handle, entity_pick_point)
                            } else {
                                self.tabs[i]
                                    .scene
                                    .document
                                    .get_entity(handle)
                                    .and_then(|entity| {
                                        crate::scene::model::sweep_model::extrusion_axis(
                                            entity,
                                            self.tabs[i].ucs_xform().axes().3.to_array(),
                                        )
                                    })
                                    .map(glam::DVec3::from_array)
                            }
                        } else {
                            None
                        };
                    // A dimension pick on a block reference measures the
                    // nested entity under the click, as a pick through a
                    // viewport does.
                    let nested = if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.measures_through_viewports())
                        && matches!(
                            self.tabs[i].scene.document.get_entity(handle),
                            Some(document::EntityType::Insert(_))
                        ) {
                        let scene = &self.tabs[i].scene;
                        scene.resolve_measurable_entity(handle, entity_pick_point, None)
                    } else {
                        None
                    };
                    let (handle, block_path) = match &nested {
                        Some((entity, path)) => {
                            let nested_handle = entity.as_entity().handle();
                            (
                                if nested_handle.is_valid() {
                                    nested_handle
                                } else {
                                    handle
                                },
                                path.clone(),
                            )
                        }
                        None => (handle, Vec::new()),
                    };
                    // Some commands (e.g. SS_CATCHMENT) need the entity
                    // body before `on_entity_pick` can advance.
                    let inject_first = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.inject_before_entity_pick())
                        .unwrap_or(false);
                    if inject_first {
                        let surface_area = self.tabs[i]
                            .scene
                            .meshes
                            .get(&handle)
                            .or_else(|| self.tabs[i].scene.block_meshes.get(&handle))
                            .map(|mesh| mesh.metrics.surface_area);
                        if let Some(entity) = nested
                            .map(|(entity, _)| entity)
                            .or_else(|| self.tabs[i].scene.document.get_entity(handle).cloned())
                        {
                            if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                                cmd.inject_picked_entity(entity);
                                if let Some(area) = surface_area {
                                    cmd.inject_picked_surface_area(area);
                                }
                            }
                        }
                    }

                    if !self.dimension_acquisition_allowed(i, None) {
                        self.finish_command_click(i);
                        return Task::none();
                    }
                    let shift = self.shift_down;
                    let result = self.tabs[i].active_cmd.as_mut().map(|c| {
                        // Shift-swap state for TRIM/EXTEND (#336).
                        c.set_shift(shift);
                        c.set_entity_pick_direction(entity_pick_direction);
                        c.on_entity_pick(handle, entity_pick_point)
                    });
                    self.record_dimension_entity_points(i, None, handle, block_path);
                    // HATCHEDIT: after pick, inject hatch model data into the command.
                    if self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .map(|c| c.name() == "HATCHEDIT")
                        .unwrap_or(false)
                    {
                        if let Some(model) = self.tabs[i].scene.hatches.get(&handle).cloned() {
                            let entity = self.tabs[i].scene.document.get_entity(handle);
                            let annotative = entity.is_some_and(|entity| {
                                crate::scene::annotative::is_annotative(
                                    &self.tabs[i].scene.document,
                                    entity,
                                )
                            });
                            let (scale, angle) = match entity {
                                Some(document::EntityType::Hatch(hatch)) => (
                                    hatch.pattern_scale as f32,
                                    hatch.pattern_angle.to_degrees() as f32,
                                ),
                                _ => (model.scale, model.angle_offset.to_degrees()),
                            };
                            use crate::command::CadCommand;
                            use crate::modules::draw::draw::hatchedit::HatcheditCommand;
                            let current_color =
                                self.tabs[i].scene.document.header.current_entity_color;
                            let current_transparency =
                                self.tabs[i].scene.document.current_entity_transparency();
                            let current_origin = self.tabs[i].scene.document.hatch_origin();
                            let cmd: Box<dyn CadCommand> = Box::new(
                                HatcheditCommand::with_handle(
                                    handle,
                                    model.name.clone(),
                                    scale,
                                    angle,
                                    annotative,
                                )
                                .with_appearance(entity, current_color, current_transparency)
                                .with_origin(current_origin),
                            );
                            self.command_line.push_info(&cmd.prompt());
                            self.tabs[i].active_cmd = Some(cmd);
                        } else {
                            self.command_line
                                .push_error(crate::t!("HATCHEDIT: not a hatch entity.").as_ref());
                            self.tabs[i].active_cmd = None;
                        }
                    }
                    // DIMTEDIT: inject cloned entity via trait.
                    {
                        let needs_inject = self.tabs[i]
                            .active_cmd
                            .as_ref()
                            .map(|c| matches!(c.name(), "DIMTEDIT"))
                            .unwrap_or(false);
                        if needs_inject {
                            if let Some(entity) =
                                self.tabs[i].scene.document.get_entity(handle).cloned()
                            {
                                if let Some(cmd) = self.tabs[i].active_cmd.as_mut() {
                                    cmd.inject_picked_entity(entity);
                                    let prompt = cmd.prompt();
                                    self.command_line.push_info(&prompt);
                                }
                            }
                        }
                    }
                    result
                } else if let Some(result) = {
                    let dimension = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .is_some_and(|c| c.measures_through_viewports());
                    if dimension {
                        let per_px = self.tabs[i].scene.camera.borrow().ortho_size() as f64 * 2.0
                            / (bounds.height as f64).max(1.0);
                        self.try_dimension_viewport_entity_pick(
                            i,
                            pick_wcs,
                            crate::ui::overlay::pick_box_aperture_px(self.pick_box) as f64 * per_px,
                        )
                    } else {
                        None
                    }
                } {
                    Some(result)
                } else if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.entity_pick_accepts_points())
                {
                    let pending = self.pending_click_snap.take();
                    if !self.record_accepted_snap(
                        i,
                        pending.map(|(hit, _)| hit),
                        pending.and_then(|(_, frame)| frame),
                        pick_wcs,
                    ) {
                        self.finish_command_click(i);
                        return Task::none();
                    }
                    self.refresh_command_point_pick_context(i);
                    self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|command| command.on_point(pick_wcs))
                } else if let Some(result) = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .and_then(|command| command.on_empty_pick(pick_wcs))
                {
                    Some(result)
                } else if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|command| command.accepts_drag_selection())
                {
                    let anchor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                    let mut selection = self.tabs[i].scene.selection.borrow_mut();
                    selection.gesture.box_anchor = Some(p_full);
                    selection.gesture.box_current = Some(p_full);
                    selection.gesture.box_anchor_world = Some(anchor_world);
                    if !selection.gesture.box_crossing_locked {
                        selection.gesture.box_crossing = false;
                    }
                    None
                } else {
                    self.command_line
                        .push_info(crate::t!("Nothing found at that point.").as_ref());
                    None
                }
            } else if self.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| c.needs_tangent_pick())
                .unwrap_or(false)
            {
                if let Some(obj) = tangent_obj_at_click {
                    self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_tangent_point(obj, pick_wcs))
                } else {
                    self.command_line
                        .push_info(crate::t!("Select a tangent object.").as_ref());
                    None
                }
            } else if !self.command_point_allowed(i, world_pt) {
                None
            } else {
                // A scalar typed into the dynamic-input box but not
                // yet confirmed with Enter is applied before the
                // point pick — e.g. an OFFSET distance typed and then
                // clicked takes effect rather than being discarded.
                let wants_text = self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.input_kind().wants_text())
                    .unwrap_or(false);
                if wants_text {
                    if let Some(text) = self.tabs[i]
                        .dyn_fields
                        .iter()
                        .find_map(|f| f.buffer.clone())
                    {
                        let text = crate::app::expr_eval::eval_to_string(text.trim());
                        if let Some(c) = self.tabs[i].active_cmd.as_mut() {
                            c.on_text_input(&text);
                        }
                        for f in self.tabs[i].dyn_fields.iter_mut() {
                            f.buffer = None;
                        }
                        self.tabs[i].dyn_active = 0;
                    }
                }
                // Record what this point step accepted, alongside the point
                // itself: paper point, model point, viewport, frame and
                // geometry identity. Commands that don't care keep consuming
                // `world_pt` exactly as before.
                let pending = self.pending_click_snap.take();
                if !self.record_accepted_snap(
                    i,
                    pending.map(|(hit, _)| hit),
                    pending.and_then(|(_, frame)| frame),
                    world_pt,
                ) {
                    self.finish_command_click(i);
                    return Task::none();
                }
                // An unsnapped first point on a solid face lands on the face.
                let world_pt = match self.apply_dynamic_ucs(snapped.then_some(world_pt)) {
                    Some(on_face) if !snapped => on_face,
                    _ => world_pt,
                };
                self.last_point = Some(world_pt);
                // The one-shot snap override is spent by this pick —
                // restore the running osnap configuration (#337).
                self.snapper.clear_override();
                self.dyn_user_reshaped = false;
                self.dyn_coord_absolute = self.dyn_pointer_absolute;
                self.sync_dyn_fields();
                self.reset_tracking_after_point();
                // A running Tangent snap may carry a tangent object; a
                // command can consume it to resolve a deferred tangent
                // (LINE tangent to two circles, which needs both). When
                // it does, sync last_point to the command's resolved
                // anchor since it replaced the picked coordinate.
                self.refresh_command_point_pick_context(i);
                let handled = self.tabs[i]
                    .active_cmd
                    .as_mut()
                    .and_then(|c| c.on_point_with_tangent(world_pt, tangent_obj_at_click));
                if handled.is_some() {
                    if let Some(a) = self.tabs[i]
                        .active_cmd
                        .as_ref()
                        .and_then(|c| c.resolved_anchor())
                    {
                        self.last_point = Some(a);
                    }
                    handled
                } else {
                    self.tabs[i]
                        .active_cmd
                        .as_mut()
                        .map(|c| c.on_point(world_pt))
                }
            };

            let panel_updates = self.tabs[i]
                .active_cmd
                .as_mut()
                .map(|c| c.take_pending_panel_updates())
                .unwrap_or_default();
            self.apply_panel_or_dialog_updates(panel_updates);

            if !matches!(
                result,
                Some(crate::command::CmdResult::CommitDimension { .. })
            ) {
                self.sync_dimension_snaps(i);
            }
            if let Some(r) = result {
                let task = self.apply_cmd_result(r);
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.input.left_down = false;
                sel.input.left_press_pos = None;
                sel.input.left_press_time = None;
                sel.input.left_dragging = false;
                drop(sel);
                self.arm_resolved_command_snap_dwell(i, p_full);
                return task;
            }
            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.left_down = false;
            sel.input.left_press_pos = None;
            sel.input.left_press_time = None;
            sel.input.left_dragging = false;
            drop(sel);
            self.arm_resolved_command_snap_dwell(i, p_full);
            return Task::none();
        }

        let (is_down2, is_dragging, box_anchor, box_crossing, poly_drag, poly_crossing, poly_fence) = {
            let sel = self.tabs[i].scene.selection.borrow();
            (
                sel.input.left_down,
                sel.input.left_dragging,
                sel.gesture.box_anchor,
                if sel.gesture.box_crossing_locked {
                    sel.gesture.box_crossing
                } else {
                    sel.gesture
                        .box_anchor
                        .is_some_and(|anchor| crate::scene::selection_box_crossing(anchor, p_full))
                },
                sel.gesture.poly_active,
                sel.gesture.poly_crossing,
                sel.gesture.poly_fence,
            )
        };

        let mut selection_just_completed = false;

        // Active-tile-local selection: tile-sized bounds and the box
        // anchor mapped into the tile, so box / crossing selection
        // matches the active pane (p is already tile-local).
        let vp_size = (tile_vw, tile_vh);
        let box_anchor = box_anchor.map(|a| iced::Point {
            x: a.x - tile_off.x,
            y: a.y - tile_off.y,
        });

        if is_down2 && !self.pick_first && self.tabs[i].active_cmd.is_none() {
            self.tabs[i]
                .scene
                .selection
                .borrow_mut()
                .clear_left_selection_gesture();
        } else if is_down2 {
            let bounds = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: vp_size.0,
                height: vp_size.1,
            };

            if is_dragging {
                let drag_geometry = if poly_drag {
                    let mut points = self.tabs[i]
                        .scene
                        .selection
                        .borrow()
                        .gesture
                        .lasso_path(p_full);
                    if !poly_fence {
                        points.push(points[0]);
                    }
                    let fence = points
                        .into_iter()
                        .map(|point| {
                            let local = Point::new(point.x - tile_off.x, point.y - tile_off.y);
                            let world = self.cursor_model_point(i, &edit_cam, local, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    let (min, max) = fence.iter().fold(
                        (
                            [f64::INFINITY, f64::INFINITY],
                            [f64::NEG_INFINITY, f64::NEG_INFINITY],
                        ),
                        |(mut min, mut max), point| {
                            min[0] = min[0].min(point[0]);
                            min[1] = min[1].min(point[1]);
                            max[0] = max[0].max(point[0]);
                            max[1] = max[1].max(point[1]);
                            (min, max)
                        },
                    );
                    Some((fence, (!poly_fence).then_some((min, max))))
                } else if let Some(anchor) = box_anchor {
                    let points = [
                        anchor,
                        iced::Point::new(anchor.x, p.y),
                        p,
                        iced::Point::new(p.x, anchor.y),
                        anchor,
                    ];
                    let fence = points
                        .into_iter()
                        .map(|point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    let (min, max) = fence.iter().fold(
                        (
                            [f64::INFINITY, f64::INFINITY],
                            [f64::NEG_INFINITY, f64::NEG_INFINITY],
                        ),
                        |(mut min, mut max), point| {
                            min[0] = min[0].min(point[0]);
                            min[1] = min[1].min(point[1]);
                            max[0] = max[0].max(point[0]);
                            max[1] = max[1].max(point[1]);
                            (min, max)
                        },
                    );
                    Some((fence, Some((min, max))))
                } else {
                    None
                };

                if let Some((fence, window)) = drag_geometry {
                    if self.tabs[i].active_cmd.is_none() {
                        self.tabs[i].last_crossing_window = window.filter(|_| {
                            if poly_drag {
                                poly_crossing
                            } else {
                                box_crossing
                            }
                        });
                    }
                    let result = self.tabs[i].active_cmd.as_mut().and_then(|command| {
                        command.set_shift(self.shift_down);
                        command.on_drag_selection(
                            &fence,
                            window,
                            if poly_drag {
                                poly_crossing
                            } else {
                                box_crossing
                            },
                        )
                    });
                    if let Some(result) = result {
                        let mut selection = self.tabs[i].scene.selection.borrow_mut();
                        selection.input.left_down = false;
                        selection.input.left_press_pos = None;
                        selection.input.left_press_time = None;
                        selection.input.left_dragging = false;
                        selection.gesture.poly_active = false;
                        selection.gesture.poly_points.clear();
                        selection.gesture.poly_crossing = false;
                        selection.gesture.box_anchor = None;
                        selection.gesture.box_anchor_world = None;
                        selection.gesture.box_current = None;
                        selection.gesture.box_crossing = false;
                        selection.gesture.box_crossing_locked = false;
                        drop(selection);
                        return self.apply_cmd_result(result);
                    }
                }

                // A drag that starts inside a table cell picks the cells
                // from there to the one under the release.
                let cell_drag = box_anchor
                    .filter(|_| self.tabs[i].active_cmd.is_none())
                    .and_then(|a| {
                        let end = self.cursor_model_point(i, &edit_cam, p, bounds);
                        let (table, anchor) =
                            self.table_cell_under(i, a, bounds, &edit_cam, 0.0)?;
                        let (end_table, end) = self.tabs[i].scene.table_cell_pick(end, 0.0)?;
                        (end_table == table).then_some(crate::scene::TableCellSelection {
                            table,
                            anchor,
                            end,
                        })
                    });
                if let Some(cells) = cell_drag {
                    self.tabs[i].scene.select_table_cells(cells);
                    self.refresh_properties();
                    selection_just_completed = true;
                } else if !poly_drag {
                    // Complete the press-and-drag rectangle on release.
                    if let Some(a) = box_anchor {
                        let crossing = box_crossing;
                        let (mut handles, timing) =
                            self.box_selection_handles(i, &edit_cam, bounds, a, p, crossing);
                        // The drag gesture applies the selection filter as the click one does.
                        handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
                        self.filter_handles_reject_locked(i, &mut handles);
                        let polygon = [a, Point::new(p.x, a.y), p, Point::new(a.x, p.y)];
                        let marker_hit = is_gathering
                            && self.select_command_marker_region(
                                i,
                                &edit_cam,
                                bounds,
                                &polygon,
                                crossing,
                                !selection_pick_add,
                                !handles.is_empty(),
                            );
                        let cand_ms = timing.candidates;
                        let hit_ms = timing.hit;
                        let t_apply = crate::perf::enabled().then(Instant::now);
                        self.commit_area_selection(
                            i,
                            &handles,
                            marker_hit,
                            selection_pick_add,
                            is_gathering,
                        );
                        self.pick_authoring_box(a, p, crossing, !handles.is_empty());
                        let apply_ms = t_apply.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
                        let t_props = crate::perf::enabled().then(Instant::now);
                        self.queue_properties_refresh(i);
                        if crate::perf::enabled() {
                            crate::perf_record!(
                                "[perf] select-commit kind=drag-box crossing={crossing} \
candidates={cand_ms:.1}ms hit={hit_ms:.1}ms apply={apply_ms:.1}ms \
properties={:.1}ms picked={}",
                                t_props.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0),
                                handles.len(),
                            );
                        }
                        selection_just_completed = true;
                    }
                } else {
                    let (mut handles, poly_pts) =
                        self.lasso_selection_handles(i, &edit_cam, bounds, p_full, tile_off);
                    handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
                    self.filter_handles_reject_locked(i, &mut handles);
                    let marker_hit = is_gathering
                        && !poly_fence
                        && self.select_command_marker_region(
                            i,
                            &edit_cam,
                            bounds,
                            &poly_pts,
                            poly_crossing,
                            !selection_pick_add,
                            !handles.is_empty(),
                        );
                    self.commit_area_selection(
                        i,
                        &handles,
                        marker_hit,
                        selection_pick_add,
                        is_gathering,
                    );
                    self.queue_properties_refresh(i);
                    selection_just_completed = true;
                }
                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                sel.gesture.poly_active = false;
                sel.gesture.poly_points.clear();
                sel.gesture.poly_crossing = false;
                sel.gesture.box_anchor = None;
                sel.gesture.box_anchor_world = None;
                sel.gesture.box_current = None;
            } else {
                if box_anchor.is_none() {
                    if is_gathering && is_click {
                        let (view, eye) = match &edit_cam {
                            Some(camera) => (camera.view_proj_rte(bounds), camera.eye()),
                            None => {
                                let camera = self.tabs[i].scene.camera.borrow();
                                (camera.view_proj_rte(bounds), camera.eye())
                            }
                        };
                        let context = crate::command::PointPickContext {
                            view,
                            eye,
                            bounds,
                            aperture_px: crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                        };
                        let remove = self.shift_down || self.select_remove_mode;
                        let tab = &mut self.tabs[i];
                        let hit = tab.active_cmd.as_mut().and_then(|command| {
                            command.on_selection_marker_click(
                                &tab.scene,
                                p,
                                context,
                                remove,
                                !selection_pick_add,
                            )
                        });
                        if let Some(name) = hit {
                            tab.scene
                                .selection
                                .borrow_mut()
                                .clear_left_selection_gesture();
                            if !remove && !selection_pick_add {
                                tab.scene.deselect_all();
                            }
                            self.command_line.push_info(&format!(
                                "{} {}",
                                if remove { "−" } else { "+" },
                                name
                            ));
                            if !remove && !selection_pick_add {
                                return self.feed_command(
                                    crate::command::StepInput::SelectionComplete(Vec::new()),
                                );
                            }
                            return Task::none();
                        }
                    }
                    // A parametric-constraint glyph draws above its geometry
                    // and takes click priority over the entity beneath it.
                    let glyph_hit = (self.tabs[i].scene.current_layout == "Model")
                        .then(|| {
                            let scope = self.tabs[i].current_parametric_scope();
                            self.tabs[i].scene.constraint_glyph_hit(
                                scope,
                                canvas_sz,
                                self.show_constraint_values,
                                self.constraint_bar_display,
                                self.constraint_bar_mode,
                                p_full,
                            )
                        })
                        .flatten();
                    if let Some(id) = glyph_hit {
                        self.tabs[i].scene.deselect_all();
                        self.tabs[i].scene.selected_constraint = Some(id);
                        self.refresh_properties();
                        selection_just_completed = true;
                    }
                    // A click inside a table cell (clear of its grid lines)
                    // picks the cell; Shift+click stretches the picked range.
                    let mut cell_hit = false;
                    if glyph_hit.is_none() && self.tabs[i].active_cmd.is_none() {
                        let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                        let aperture = crate::ui::overlay::pick_box_aperture_px(self.pick_box);
                        let edge = self.cursor_model_point(
                            i,
                            &edit_cam,
                            iced::Point::new(p.x + aperture, p.y),
                            bounds,
                        );
                        let margin = (edge - click_world).length();
                        if let Some((table, cell)) =
                            self.table_cell_under(i, p, bounds, &edit_cam, margin)
                        {
                            // A quick press-move-release across cells picks the
                            // range between them.
                            let released = self.tabs[i]
                                .scene
                                .selection
                                .borrow()
                                .input
                                .last_move_pos
                                .map(|last| {
                                    iced::Point::new(last.x - tile_off.x, last.y - tile_off.y)
                                });
                            let end = released
                                .map(|last| self.cursor_model_point(i, &edit_cam, last, bounds))
                                .and_then(|world| self.tabs[i].scene.table_cell_pick(world, 0.0))
                                .filter(|(end_table, end)| *end_table == table && *end != cell)
                                .map(|(_, end)| end);
                            let cells = match self.tabs[i]
                                .scene
                                .table_cells
                                .filter(|c| self.shift_down && c.table == table)
                            {
                                _ if end.is_some() => crate::scene::TableCellSelection {
                                    table,
                                    anchor: cell,
                                    end: end.unwrap_or(cell),
                                },
                                Some(c) => crate::scene::TableCellSelection { end: cell, ..c },
                                None => crate::scene::TableCellSelection {
                                    table,
                                    anchor: cell,
                                    end: cell,
                                },
                            };
                            self.tabs[i].scene.select_table_cells(cells);
                            self.refresh_properties();
                            selection_just_completed = true;
                            cell_hit = true;
                        }
                    }
                    if glyph_hit.is_none() && !cell_hit {
                        let (view_rot, eye, all_wires) = self.pick_view(i, &edit_cam, bounds);
                        let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                        let t_arm = crate::perf::enabled().then(Instant::now);
                        let prior_selection = self.tabs[i].scene.selected.len();
                        let pick_aperture = crate::ui::overlay::pick_box_aperture_px(self.pick_box);
                        let query_started = crate::perf::enabled().then(Instant::now);
                        let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                            all_wires,
                            click_world,
                            view_rot,
                            eye,
                            bounds,
                            pick_aperture * 2.0,
                        );
                        let candidate_ms = crate::perf::elapsed_ms(query_started);
                        let handles_started = crate::perf::enabled().then(Instant::now);
                        let candidate_handles = self.tabs[i]
                            .scene
                            .interaction_candidate_handles(&click_candidates);
                        let handles_ms = crate::perf::elapsed_ms(handles_started);

                        // Selection cycling: where two or more objects
                        // overlap, open a list box to pick which one; a
                        // single object falls through to the normal click.
                        // Gated behind the toggle, so default picking is
                        // unchanged when off.
                        let mut handled_by_cycling = false;
                        if self.selection_cycling == 2 {
                            let cands: Vec<Handle> = scene::pick::hit_test::click_hits_all(
                                p,
                                &click_candidates,
                                view_rot,
                                eye,
                                bounds,
                                self.tabs[i].scene.document.header.lineweight_display,
                                pick_aperture,
                            )
                            .into_iter()
                            .filter_map(Scene::handle_from_wire_name)
                            .filter(|&h| self.tabs[i].scene.passes_selection_filter(h))
                            .filter(|&h| !self.tabs[i].scene.is_reference_only(h))
                            .collect();
                            // An object drawn by several wires (a dimension's
                            // lines, arrows and text) is one candidate.
                            let mut seen = std::collections::HashSet::new();
                            let cands: Vec<Handle> =
                                cands.into_iter().filter(|h| seen.insert(*h)).collect();
                            if cands.len() >= 2 {
                                // Overlap: open the list box at the cursor.
                                self.cycle_candidates = Some(crate::app::SelectionCycle {
                                    tab_id: self.tabs[i].id,
                                    point: p_full,
                                    handles: cands,
                                    modifiers: Some((self.shift_down, self.select_remove_mode)),
                                });
                                handled_by_cycling = true;
                            }
                        }

                        if !handled_by_cycling {
                            let context = crate::command::PointPickContext {
                                view: view_rot,
                                eye,
                                bounds,
                                aperture_px: pick_aperture,
                            };
                            let hit = self
                                .selection_click_hit(
                                    i,
                                    p,
                                    context,
                                    &click_candidates,
                                    candidate_handles.as_ref(),
                                    t_arm,
                                    candidate_ms,
                                    handles_ms,
                                )
                                .filter(|&h| self.tabs[i].scene.passes_selection_filter(h));
                            if let Some(handle) = hit {
                                selection_just_completed = self.commit_point_selection(
                                    i,
                                    handle,
                                    self.shift_down,
                                    self.select_remove_mode,
                                );
                            } else {
                                // Empty-space click only ARMS a box here; it
                                // no longer clears the selection, so a box can
                                // add to it (issue #83). The box completion
                                // (or Esc) decides what happens to the
                                // selection.
                                // PICKADD 0 (#226): OS convention — the
                                // empty click also drops the selection.
                                if !selection_pick_add && !self.shift_down {
                                    if let Some(command) = self.tabs[i].active_cmd.as_mut() {
                                        command.clear_selection_markers();
                                    }
                                    self.tabs[i].scene.deselect_all();
                                    self.tabs[i].last_crossing_window = None;
                                    self.queue_properties_refresh(i);
                                }
                                if self.pick_auto & 1 == 0 || self.pick_drag_mode == 1 {
                                    self.tabs[i]
                                        .scene
                                        .selection
                                        .borrow_mut()
                                        .input
                                        .clear_left_buttons();
                                    return Task::none();
                                }
                                // Pin the anchor to the world point under it
                                // so a zoom/pan mid-drag re-projects it
                                // instead of leaving it frozen in pixels
                                // (#234). Computed before the selection
                                // borrow so the &self projection can't clash.
                                let anchor_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                                if let Some(t) = t_arm {
                                    let arm_ms = t.elapsed().as_secs_f64() * 1000.0;
                                    if arm_ms >= 5.0 {
                                        crate::perf_record!(
                                            "[perf] select-arm {arm_ms:>7.1}ms pick+clear, \
was_selected={}",
                                            prior_selection,
                                        );
                                    }
                                }
                                let mut sel = self.tabs[i].scene.selection.borrow_mut();
                                // Full-canvas space: ViewportMove updates
                                // box_current in canvas coords and the overlay
                                // draws there; release maps back into the tile.
                                sel.gesture.box_anchor = Some(p_full);
                                sel.gesture.box_current = Some(p_full);
                                sel.gesture.box_anchor_world = Some(anchor_world);
                                if !sel.gesture.box_crossing_locked {
                                    sel.gesture.box_crossing = false;
                                }
                                drop(sel);
                                self.push_box_corner_prompt();
                            }
                        }
                    }
                } else {
                    let a = box_anchor.unwrap();
                    let box_points = [
                        a,
                        iced::Point::new(a.x, p.y),
                        p,
                        iced::Point::new(p.x, a.y),
                        a,
                    ];
                    let command_fence = box_points
                        .into_iter()
                        .map(|point| {
                            let world = self.cursor_model_point(i, &edit_cam, point, bounds);
                            [world.x, world.y]
                        })
                        .collect::<Vec<_>>();
                    let (command_min, command_max) = command_fence.iter().fold(
                        (
                            [f64::INFINITY, f64::INFINITY],
                            [f64::NEG_INFINITY, f64::NEG_INFINITY],
                        ),
                        |(mut min, mut max), point| {
                            min[0] = min[0].min(point[0]);
                            min[1] = min[1].min(point[1]);
                            max[0] = max[0].max(point[0]);
                            max[1] = max[1].max(point[1]);
                            (min, max)
                        },
                    );
                    let command_result = self.tabs[i].active_cmd.as_mut().and_then(|command| {
                        command.set_shift(self.shift_down);
                        command.on_drag_selection(
                            &command_fence,
                            Some((command_min, command_max)),
                            box_crossing,
                        )
                    });
                    if let Some(result) = command_result {
                        let mut selection = self.tabs[i].scene.selection.borrow_mut();
                        selection.input.left_down = false;
                        selection.input.left_press_pos = None;
                        selection.input.left_press_time = None;
                        selection.input.left_dragging = false;
                        selection.gesture.box_anchor = None;
                        selection.gesture.box_anchor_world = None;
                        selection.gesture.box_current = None;
                        selection.gesture.box_crossing = false;
                        selection.gesture.box_crossing_locked = false;
                        drop(selection);
                        return self.apply_cmd_result(result);
                    }

                    let crossing = box_crossing;
                    if self.tabs[i].active_cmd.is_none()
                        && !self.shift_down
                        && !self.select_remove_mode
                    {
                        self.tabs[i].last_crossing_window = if crossing {
                            Some((command_min, command_max))
                        } else {
                            None
                        };
                    }
                    let (mut handles, timing) =
                        self.box_selection_handles(i, &edit_cam, bounds, a, p, crossing);
                    let (cand_ms, hit_ms) = (timing.candidates, timing.hit);
                    let (m_handles, m_wires, m_hatch) =
                        (Some(timing.handles), Some(timing.wires), Some(timing.hatch));
                    let t_filter = crate::perf::enabled().then(Instant::now);
                    // Selection filter: keep only allowed types.
                    handles.retain(|&h| self.tabs[i].scene.passes_selection_filter(h));
                    self.filter_handles_reject_locked(i, &mut handles);
                    let polygon = [a, Point::new(p.x, a.y), p, Point::new(a.x, p.y)];
                    let marker_hit = is_gathering
                        && self.select_command_marker_region(
                            i,
                            &edit_cam,
                            bounds,
                            &polygon,
                            crossing,
                            !selection_pick_add,
                            !handles.is_empty(),
                        );
                    let filter_ms = t_filter.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
                    let t_apply = crate::perf::enabled().then(Instant::now);
                    self.commit_area_selection(
                        i,
                        &handles,
                        marker_hit,
                        selection_pick_add,
                        is_gathering,
                    );
                    let apply_ms = t_apply.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
                    let t_props = crate::perf::enabled().then(Instant::now);
                    self.pick_authoring_box(a, p, crossing, !handles.is_empty());
                    self.queue_properties_refresh(i);
                    if crate::perf::enabled() {
                        crate::perf_record!(
                            "[perf] select-commit kind=window crossing={crossing} \
candidates={cand_ms:.1}ms hit={hit_ms:.1}ms [handles={:.1} wires={:.1} \
hatch={:.1} mesh={:.1}] filter={filter_ms:.1}ms apply={apply_ms:.1}ms \
properties={:.1}ms picked={}",
                            m_handles.unwrap_or(0.0),
                            m_wires.unwrap_or(0.0) - m_handles.unwrap_or(0.0),
                            m_hatch.unwrap_or(0.0) - m_wires.unwrap_or(0.0),
                            hit_ms - m_hatch.unwrap_or(0.0),
                            t_props.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0),
                            handles.len(),
                        );
                    }
                    let mut sel = self.tabs[i].scene.selection.borrow_mut();
                    sel.gesture.box_anchor = None;
                    sel.gesture.box_anchor_world = None;
                    sel.gesture.box_current = None;
                    sel.gesture.box_crossing = false;
                    sel.gesture.box_crossing_locked = false;
                    selection_just_completed = true;
                }
            }

            let mut sel = self.tabs[i].scene.selection.borrow_mut();
            sel.input.left_down = false;
            sel.input.left_press_pos = None;
            sel.input.left_press_time = None;
            sel.input.left_dragging = false;
        }

        if selection_just_completed {
            self.quick_properties_anchor = p_full;
        }

        if is_gathering && selection_just_completed {
            let handles: Vec<Handle> = self.tabs[i]
                .scene
                .selected_entities()
                .into_iter()
                .map(|(h, _)| h)
                .collect();
            return self.feed_command(crate::command::StepInput::SelectionComplete(handles));
        }

        // ── Double-click in Model Space: DDEDIT for Text/MText ────
        if is_click
            && is_down
            && box_anchor.is_none()
            && self.pick_first
            && self.tabs[i].active_cmd.is_none()
            && self.tabs[i].scene.current_layout == "Model"
        {
            let now = Instant::now();
            let is_double_model = self
                .last_vp_click_time
                .map(|t| {
                    let dt = now.duration_since(t).as_millis();
                    let last = self.last_vp_click_pos.unwrap_or(p);
                    let d = (p.x - last.x).hypot(p.y - last.y);
                    dt < 400 && d < 8.0
                })
                .unwrap_or(false);

            self.last_vp_click_time = Some(now);
            self.last_vp_click_pos = Some(p);

            if is_double_model {
                let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };
                if let Some(task) = self.double_click_edit(i, p, &edit_cam, bounds) {
                    return task;
                }
            }
        }

        // ── Double-click: enter/exit MSPACE ───────────────────────
        // Only when no command is running, no drag, and we're in paper space.
        if is_click
                    && is_down   // ensures there was a matching left-press
                    && self.tabs[i].active_cmd.is_none()
                    && self.tabs[i].scene.current_layout != "Model"
        {
            let now = Instant::now();
            let is_double = self
                .last_vp_click_time
                .map(|t| {
                    let dt = now.duration_since(t).as_millis();
                    let last = self.last_vp_click_pos.unwrap_or(p);
                    let d = (p.x - last.x).hypot(p.y - last.y);
                    dt < 400 && d < 8.0
                })
                .unwrap_or(false);

            self.last_vp_click_time = Some(now);
            self.last_vp_click_pos = Some(p);

            if is_double {
                let (vw, vh) = self.tabs[i].scene.selection.borrow().view.vp_size;
                let bounds = iced::Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                };

                // An object with an editor takes the double-click before the
                // viewport it lies in, as in model space: a paper-space
                // object, or in an active viewport a model object, picked
                // through that viewport's view.
                let edit_bounds = match edit_cam {
                    Some(_) => iced::Rectangle {
                        x: 0.0,
                        y: 0.0,
                        width: tile_vw,
                        height: tile_vh,
                    },
                    None => bounds,
                };
                if let Some(task) = self.double_click_edit(i, p, &edit_cam, edit_bounds) {
                    return task;
                }

                let (view_rot, eye) = {
                    let c = self.tabs[i].scene.camera.borrow();
                    (c.view_proj_rte(bounds), c.eye())
                };

                let all_wires = self.tabs[i].scene.hit_test_wires();
                let click_world = self.cursor_model_point(i, &edit_cam, p, bounds);
                let click_candidates = self.tabs[i].scene.interaction_pick_candidates_near(
                    all_wires,
                    click_world,
                    view_rot,
                    eye,
                    bounds,
                    crate::ui::overlay::pick_box_aperture_px(self.pick_box) * 2.0,
                );

                let raw_wire_hit = scene::pick::hit_test::click_hit(
                    p,
                    &click_candidates,
                    view_rot,
                    eye,
                    bounds,
                    self.tabs[i].scene.document.header.lineweight_display,
                    crate::ui::overlay::pick_box_aperture_px(self.pick_box),
                    &self.tabs[i].scene.draw_depth_map(),
                )
                .and_then(Scene::handle_from_wire_name);

                // Nothing with an editor was double-clicked: enter the
                // clicked viewport.
                let wire_hit: Option<document::Handle> = raw_wire_hit.and_then(|h| {
                    if let Some(AcadEntityType::Viewport(vp)) =
                        self.tabs[i].scene.document.get_entity(h)
                    {
                        if !Scene::is_sheet_viewport(&self.tabs[i].scene.document, vp) {
                            Some(h)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                // 2) Decide which viewport (if any) the click enters,
                //    keyed on the *visible* on-screen rectangle. Screen-space
                //    (not the full paper rect) so a click on the empty area
                //    beside an off-screen viewport doesn't match its
                //    partly-off-canvas rect and switch to it by mistake.
                //    The wire hit only refines *which* visible viewport
                //    (border precision / overlap), and only when the click
                //    actually lands inside THAT viewport's visible rect —
                //    otherwise its border-wire pick tolerance could match a
                //    far viewport whose edge merely passes near the cursor
                //    (e.g. clicking the overlap of two viewports while a
                //    third's border runs nearby) and enter the wrong one.
                let screen_hit = self.tabs[i]
                    .scene
                    .viewport_at_screen_point(p.x, p.y, (vw, vh));
                let wire_in_visible = wire_hit.filter(|&h| {
                    self.tabs[i]
                        .scene
                        .viewport_screen_rect(h, (vw, vh))
                        .is_some_and(|r| {
                            let x0 = r.x.max(0.0);
                            let y0 = r.y.max(0.0);
                            let x1 = (r.x + r.width).min(vw);
                            let y1 = (r.y + r.height).min(vh);
                            p.x >= x0 && p.x <= x1 && p.y >= y0 && p.y <= y1
                        })
                });
                let hit_vp = wire_in_visible.or(screen_hit);

                if let Some(handle) = hit_vp {
                    return Task::done(Message::EnterViewport(handle));
                } else if self.tabs[i].scene.active_viewport.is_some() {
                    // Double-clicked outside all viewports while in MSPACE → exit.
                    return Task::done(Message::ExitViewport);
                }
            }
        }

        Task::none()
    }

}
