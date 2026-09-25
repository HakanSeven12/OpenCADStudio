use super::*;

impl OpenCADStudio {
    pub(super) fn handle_lengthen_entity(&mut self, handle: Handle, pick_pt: glam::DVec3, mode: crate::modules::draw::modify::lengthen::LenMode) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Some(Task::none());
        }
        use crate::modules::draw::modify::lengthen::lengthen_entity;
        let result = self.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .and_then(|e| lengthen_entity(e, pick_pt.as_vec3(), &mode));
        match result {
            Some(new_entity) => {
                let label = self.history_label_from_active_cmd(i, "LENGTHEN");
                self.push_undo_snapshot(i, label);
                self.tabs[i].scene.erase_entities(&[handle]);
                self.tabs[i].scene.add_entity(new_entity);
                self.tabs[i].dirty = true;
                self.command_line
                    .push_output(crate::t!("LENGTHEN: applied.").as_ref());
                self.refresh_properties();
            }
            None => {
                self.command_line
                    .push_error(crate::t!("LENGTHEN: entity type not supported.").as_ref());
            }
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        None
    }

    pub(super) fn handle_divide_entity(&mut self, handle: Handle, n: usize, marker: Option<crate::command::CurveMarker>) {
        let i = self.active_tab;
        use crate::modules::draw::inquiry::divide::divide_entity;
        let pts = self.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .map(|e| divide_entity(e, n, marker.as_ref()))
            .unwrap_or_default();
        let count = pts.len();
        if count > 0 {
            self.push_undo_snapshot(i, "DIVIDE");
            let layer = self.tabs[i].active_layer.clone();
            for mut p in pts {
                p.as_entity_mut().set_layer(layer.clone());
                self.tabs[i].scene.add_entity(p);
            }
            self.tabs[i].dirty = true;
            self.command_line
                .push_output(crate::tf!("DIVIDE: {count} marker(s) placed.").as_ref());
        } else {
            self.command_line.push_error(
                crate::t!("DIVIDE: entity type not supported or N < 2.").as_ref(),
            );
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
    }

    pub(super) fn handle_measure_entity(&mut self, result: CmdResult) {
        let i = self.active_tab;
        let CmdResult::MeasureEntity {
            handle,
            segment_length,
            pick_point,
            marker,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        use crate::modules::draw::inquiry::divide::measure_entity;
        let pts = self.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .map(|e| measure_entity(e, segment_length, pick_point, marker.as_ref()))
            .unwrap_or_default();
        let count = pts.len();
        if count > 0 {
            self.push_undo_snapshot(i, "MEASURE");
            let layer = self.tabs[i].active_layer.clone();
            for mut p in pts {
                p.as_entity_mut().set_layer(layer.clone());
                self.tabs[i].scene.add_entity(p);
            }
            self.tabs[i].dirty = true;
            self.command_line
                .push_output(crate::tf!("MEASURE: {count} marker(s) placed.").as_ref());
        } else {
            self.command_line
                .push_output(crate::t!("MEASURE: 0 markers placed.").as_ref());
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
    }

    pub(super) fn handle_pedit_op(&mut self, handle: Handle, op: crate::modules::draw::modify::pedit::PeditOp) -> Task<Message> {
        let i = self.active_tab;
        self.apply_pedit_result(i, handle, op)
    }

    pub(super) fn handle_join_to_source(&mut self, source: Handle, handles: Vec<Handle>) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, source) {
            return Some(Task::none());
        }
        let joined = self.tabs[i]
            .scene
            .document
            .get_entity(source)
            .and_then(|entity| {
                let candidates: Vec<_> = handles
                    .iter()
                    .filter(|handle| {
                        **handle != source && !self.tabs[i].scene.is_layer_locked(**handle)
                    })
                    .filter_map(|handle| {
                        self.tabs[i]
                            .scene
                            .document
                            .get_entity(*handle)
                            .map(|entity| (*handle, entity))
                    })
                    .collect();
                crate::modules::draw::modify::join::join_to_source(entity, &candidates)
            });
        if let Some((replacement, consumed)) = joined {
            self.push_undo_snapshot(i, "JOIN");
            if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(source) {
                *entity = replacement;
            }
            self.tabs[i].scene.erase_entities(&consumed);
            self.tabs[i].scene.refresh_fill_model(source);
            self.tabs[i]
                .scene
                .bump_entities(&[(source, crate::scene::ChangeKind::Modified)]);
            self.tabs[i].dirty = true;
            self.command_line.push_output(&format!(
                "JOIN: {} objects joined to source.",
                consumed.len()
            ));
            self.refresh_properties();
        } else {
            self.command_line
                .push_info("JOIN: no compatible objects joined to source.");
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        None
    }

    pub(super) fn handle_join_entities(&mut self, handles: Vec<Handle>) -> Option<Task<Message>> {
        let i = self.active_tab;
        if let Some(handle) = handles
            .iter()
            .find(|handle| self.tabs[i].scene.is_layer_locked(**handle))
        {
            self.reject_locked_edit(i, *handle);
            return Some(Task::none());
        }
        use crate::modules::draw::modify::join::join_entities;
        let pairs: Vec<_> = handles
            .iter()
            .filter_map(|&h| self.tabs[i].scene.document.get_entity(h).map(|e| (h, e)))
            .collect();
        match join_entities(&pairs) {
            Some((to_remove, merged)) => {
                let label = self.history_label_from_active_cmd(i, "JOIN");
                self.push_undo_snapshot(i, label);
                self.tabs[i].scene.erase_entities(&to_remove);
                let count_in = to_remove.len();
                let count_out = merged.len();
                for e in merged {
                    self.tabs[i].scene.add_entity(e);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.command_line.push_output(
                    crate::tf!("JOIN: {count_in} object(s) joined into {count_out}.")
                        .as_ref(),
                );
                self.refresh_properties();
            }
            None => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                self.command_line.push_error(
                    crate::t!("JOIN: objects don't form a single connected chain, or contain an unsupported type / tilted arc.").as_ref(),
                );
            }
        }
        None
    }

    pub(super) fn handle_break_entity(&mut self, handle: Handle, p1: glam::DVec3, p2: glam::DVec3) -> Option<Task<Message>> {
        let i = self.active_tab;
        if self.reject_locked_edit(i, handle) {
            return Some(Task::none());
        }
        use crate::modules::draw::modify::break_cmd::break_entity;
        let replacement = self.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .and_then(|e| break_entity(e, p1, p2));
        match replacement {
            Some(frags) => {
                let unchanged = frags.len() == 1
                    && self.tabs[i].scene.document.get_entity(handle).is_some_and(
                        |original| {
                            let mut fragment = frags[0].clone();
                            fragment.common_mut().handle = handle;
                            &fragment == original
                        },
                    );
                if unchanged {
                    self.tabs[i].active_cmd = None;
                    self.tabs[i].snap_result = None;
                    self.tabs[i].scene.clear_preview_wire();
                    self.restore_pre_cmd_tangent();
                    self.command_line.push_output("BREAK: no geometry changed.");
                    return Some(Task::none());
                }
                let label = self.history_label_from_active_cmd(i, "BREAK");
                self.push_undo_snapshot(i, label);
                let count = frags.len();
                let owner = self.tabs[i]
                    .scene
                    .document
                    .get_entity(handle)
                    .map(|entity| entity.common().owner_handle);
                let mut fragments = frags.into_iter();
                if let Some(mut first) = fragments.next() {
                    first.common_mut().handle = handle;
                    self.tabs[i].scene.update_entity(first);
                    for mut fragment in fragments {
                        fragment.common_mut().handle = Handle::NULL;
                        if let Some(owner) = owner {
                            fragment.common_mut().owner_handle = owner;
                        }
                        self.tabs[i].scene.add_entity(fragment);
                    }
                } else {
                    self.tabs[i].scene.erase_entities(&[handle]);
                }
                self.tabs[i].dirty = true;
                self.tabs[i].scene.clear_preview_wire();
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.restore_pre_cmd_tangent();
                self.command_line
                    .push_output(crate::tf!("BREAK: {} fragment(s).", count).as_ref());
                self.refresh_properties();
            }
            None => {
                self.tabs[i].active_cmd = None;
                self.tabs[i].snap_result = None;
                self.tabs[i].scene.clear_preview_wire();
                self.restore_pre_cmd_tangent();
                self.command_line
                    .push_error(crate::t!("BREAK: entity type not supported.").as_ref());
            }
        }
        None
    }


    pub(super) fn handle_hatchedit_apply(&mut self, result: CmdResult) -> Option<Task<Message>> {
        let i = self.active_tab;
        let CmdResult::HatcheditApply {
            handle,
            name,
            scale,
            angle,
            operation,
        } = result
        else {
            unreachable!("router only routes the matching variant");
        };
        if self.reject_locked_edit(i, handle) {
            return Some(Task::none());
        }
        if !matches!(
            self.tabs[i].scene.document.get_entity(handle),
            Some(codec::EntityType::Hatch(_))
        ) {
            self.command_line
                .push_error(crate::t!("HATCHEDIT: hatch entity not found.").as_ref());
        } else {
            use crate::command::{CadCommand, HatchEditOperation};
            if matches!(&operation, HatchEditOperation::BeginAssociate) {
                let associative = matches!(self.tabs[i].scene.document.get_entity(handle),Some(codec::EntityType::Hatch(h)) if h.is_associative);
                if associative {
                    self.command_line
                        .push_info("HATCHEDIT: hatch is already associative.");
                    self.tabs[i].active_cmd = None;
                } else {
                    let plane = match self.tabs[i].scene.document.get_entity(handle) {
                        Some(codec::EntityType::Hatch(h)) => {
                            let storage =
                                crate::entities::curve::ocs_plane(h.normal, h.elevation);
                            crate::command::WorkingPlane::new(
                                glam::DVec3::from_array(storage.origin),
                                glam::DVec3::from_array(storage.x_axis),
                                glam::DVec3::from_array(storage.y_axis),
                            )
                        }
                        _ => self.tabs[i].ucs_xform().working_plane(),
                    };
                    let mut sources =
                        self.tabs[i].scene.boundary_sources_on_plane(plane, 1e-6);
                    sources.remove(&handle);
                    let command=crate::modules::draw::draw::hatchedit::HatcheditCommand::for_association(handle,name,scale,angle,plane,sources);
                    self.tabs[i].scene.deselect_all();
                    self.command_line.push_info(&command.prompt());
                    self.tabs[i].active_cmd = Some(Box::new(command));
                }
                return Some(Task::none());
            }
            if let HatchEditOperation::DrawOrderBoundary { above } = &operation {
                let references: Vec<_> =
                    match self.tabs[i].scene.document.get_entity(handle) {
                        Some(codec::EntityType::Hatch(h)) => h
                            .paths
                            .iter()
                            .flat_map(|p| p.boundary_handles.iter())
                            .filter(|h| {
                                self.tabs[i].scene.document.get_entity(**h).is_some()
                            })
                            .map(|h| format!("{:X}", h.value()))
                            .collect(),
                        _ => Vec::new(),
                    };
                self.tabs[i].active_cmd = None;
                if references.is_empty() {
                    self.command_line
                        .push_info("HATCHEDIT: no associated boundary objects.");
                    return Some(Task::none());
                }
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entity(handle, false);
                let command = format!(
                    "DRAWORDER {} {}",
                    if *above { "ABOVE" } else { "UNDER" },
                    references.join(" ")
                );
                return Some(self.dispatch_view(&command, i).unwrap_or_else(Task::none));
            }
            if matches!(
                &operation,
                HatchEditOperation::DrawOrderFront | HatchEditOperation::DrawOrderBack
            ) {
                let command = if matches!(&operation, HatchEditOperation::DrawOrderFront) {
                    "DRAWORDER FRONT"
                } else {
                    "DRAWORDER BACK"
                };
                self.tabs[i].scene.deselect_all();
                self.tabs[i].scene.select_entity(handle, false);
                self.tabs[i].active_cmd = None;
                return Some(self.dispatch_view(command, i).unwrap_or_else(Task::none));
            }
            self.push_undo_snapshot(i, "HATCHEDIT");
            match operation {
                HatchEditOperation::Appearance {
                    color,
                    layer,
                    transparency,
                } => {
                    let layer = layer.map(|name| {
                        if name == "." {
                            self.tabs[i].active_layer.clone()
                        } else {
                            name
                        }
                    });
                    if layer.as_ref().is_some_and(|name| {
                        self.tabs[i].scene.document.layers.get(name).is_none()
                    }) {
                        self.discard_last_undo_entry(i);
                        self.command_line.push_error("HATCHEDIT: layer not found.");
                        return Some(Task::none());
                    }
                    if let Some(entity) = self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        let common = entity.common_mut();
                        if let Some(value) = color {
                            common.color = value;
                            common.color_name = None;
                            common.color_book_handle = None;
                        }
                        if let Some(value) = layer {
                            common.layer = value;
                        }
                        if let Some(value) = transparency {
                            common.transparency = value;
                        }
                    }
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    self.refresh_properties();
                }
                HatchEditOperation::Update {
                    origin,
                    store_origin,
                    disassociate,
                    style,
                    annotative,
                } => {
                    if store_origin {
                        if let Some((x, y)) = origin {
                            if !self.tabs[i].scene.document.set_hatch_origin([x, y]) {
                                self.discard_last_undo_entry(i);
                                self.command_line
                                    .push_error("HATCHEDIT: cannot store default origin.");
                                return Some(Task::none());
                            }
                        }
                    }
                    if let Some(codec::EntityType::Hatch(hatch)) =
                        self.tabs[i].scene.document.get_entity_mut(handle)
                    {
                        if !name.is_empty() && name != hatch.pattern.name {
                            if let Some(entry) =
                                crate::scene::model::hatch_patterns::find(&name)
                            {
                                let mut pattern =
                                    crate::scene::model::hatch_patterns::build_dxf_pattern(
                                        entry,
                                    );
                                crate::entities::hatch::scale_pattern_geometry(
                                    &mut pattern,
                                    scale.max(1.0e-6) as f64,
                                );
                                crate::entities::hatch::rotate_pattern_geometry(
                                    &mut pattern,
                                    (angle as f64).to_radians(),
                                );
                                let origin = hatch.pattern_origin();
                                crate::entities::hatch::translate_pattern_geometry(
                                    &mut pattern,
                                    origin.x,
                                    origin.y,
                                );
                                hatch.pattern = pattern;
                                hatch.is_solid = matches!(
                                    entry.gpu,
                                    crate::scene::model::hatch_model::HatchPattern::Solid
                                );
                                hatch.pattern_type =
                                    codec::entities::HatchPatternType::Predefined;
                                hatch.gradient_color.enabled = false;
                            }
                        } else {
                            let requested_scale = scale.max(1.0e-6) as f64;
                            if hatch.pattern_scale > 1.0e-12 {
                                let factor = requested_scale / hatch.pattern_scale;
                                hatch.scale_pattern_about_origin(factor);
                            }
                            let requested_angle = (angle as f64).to_radians();
                            let delta = requested_angle - hatch.pattern_angle;
                            hatch.rotate_pattern_about_origin(delta);
                        }
                        hatch.pattern_scale = scale.max(1.0e-6) as f64;
                        hatch.pattern_angle = (angle as f64).to_radians();
                        if let Some((x, y)) = origin {
                            hatch.set_pattern_origin(codec::types::Vector2::new(x, y));
                        }
                        if disassociate {
                            for path in &mut hatch.paths {
                                path.boundary_handles.clear();
                                path.flags.set_external(false);
                            }
                            hatch.is_associative = false;
                        }
                        if let Some(style) = style {
                            hatch.style = style;
                        }
                    }
                    if let Some(value) = annotative {
                        crate::scene::annotative::set_entity_annotative(
                            &mut self.tabs[i].scene.document,
                            handle,
                            value,
                        );
                        if value {
                            if let Some(scale_handle) =
                                self.tabs[i].scene.creation_annotation_scale_handle()
                            {
                                crate::scene::annotative::create_annotation_context(
                                    &mut self.tabs[i].scene.document,
                                    handle,
                                    scale_handle,
                                );
                            }
                        }
                    }
                    self.tabs[i]
                        .scene
                        .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                }
                HatchEditOperation::AddBoundaries(handles) => {
                    self.tabs[i]
                        .scene
                        .edit_hatch_boundary_handles(handle, &handles, true);
                }
                HatchEditOperation::RemoveBoundaries(handles) => {
                    self.tabs[i]
                        .scene
                        .edit_hatch_boundary_handles(handle, &handles, false);
                }
                HatchEditOperation::AssociatePaths(paths) => {
                    if paths.is_empty()
                        || paths.iter().any(|path| {
                            path.boundary_handles.is_empty()
                                || path.boundary_handles.iter().any(|source| {
                                    self.tabs[i]
                                        .scene
                                        .document
                                        .get_entity(*source)
                                        .is_none()
                                })
                        })
                    {
                        self.discard_last_undo_entry(i);
                        self.command_line.push_error(
                            "HATCHEDIT: associated boundary is no longer available.",
                        );
                        return Some(Task::none());
                    }
                    self.tabs[i].scene.replace_hatch_association(handle, paths);
                }
                HatchEditOperation::RecreateBoundary { associate, region } => {
                    let source = self.tabs[i].scene.document.get_entity(handle).cloned();
                    if let Some(codec::EntityType::Hatch(source)) = source {
                        let storage = crate::entities::curve::ocs_plane(
                            source.normal,
                            source.elevation,
                        );
                        let plane = crate::command::WorkingPlane::new(
                            glam::DVec3::from_array(storage.origin),
                            glam::DVec3::from_array(storage.x_axis),
                            glam::DVec3::from_array(storage.y_axis),
                        );
                        let (entities, path_groups) = if region {
                            let loops = source
                                .paths
                                .iter()
                                .map(|path| {
                                    path.edges
                                        .iter()
                                        .map(crate::entities::hatch::edge_curve)
                                        .collect::<Option<Vec<_>>>()
                                })
                                .collect::<Option<Vec<_>>>();
                            let Some(loops) = loops.filter(|loops| {
                                !loops.is_empty()
                                    && loops.iter().all(|ring| !ring.is_empty())
                            }) else {
                                self.discard_last_undo_entry(i);
                                self.command_line.push_error(
                                    "HATCHEDIT: boundary cannot form a region.",
                                );
                                return Some(Task::none());
                            };
                            let contained = loops
                                .iter()
                                .enumerate()
                                .map(|(inner, ring)| {
                                    let seed = ring[0].point_at(0.0);
                                    loops
                                        .iter()
                                        .enumerate()
                                        .map(|(outer, boundary)| {
                                            inner != outer
                                                && kernel::geom2d::contains(
                                                    boundary,
                                                    seed,
                                                    kernel::geom2d::Tolerance::new(1e-6),
                                                )
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .collect::<Vec<_>>();
                            let depths = contained
                                .iter()
                                .map(|row| row.iter().filter(|inside| **inside).count())
                                .collect::<Vec<_>>();
                            let groups = depths
                                .iter()
                                .enumerate()
                                .filter(|(_, depth)| **depth % 2 == 0)
                                .map(|(outer, depth)| {
                                    let mut indices = vec![outer];
                                    indices.extend((0..loops.len()).filter(|inner| {
                                        depths[*inner] == *depth + 1
                                            && contained[*inner][outer]
                                    }));
                                    indices
                                })
                                .collect::<Vec<_>>();
                            let entities = groups
                                .iter()
                                .map(|indices| {
                                    let profiles = indices
                                        .iter()
                                        .map(|index| loops[*index].clone())
                                        .collect::<Vec<_>>();
                                    crate::scene::model::presspull_model::region_from_loops(
                                        &profiles, plane,
                                    )
                                })
                                .collect::<Option<Vec<_>>>();
                            let Some(entities) = entities else {
                                self.discard_last_undo_entry(i);
                                self.command_line.push_error(
                                    "HATCHEDIT: boundary cannot form a region.",
                                );
                                return Some(Task::none());
                            };
                            (entities, groups)
                        } else {
                            let rings = crate::scene::hatch_boundary_rings(&source);
                            if rings.len() != source.paths.len() {
                                self.discard_last_undo_entry(i);
                                self.command_line.push_error(
                                    "HATCHEDIT: boundary cannot form a polyline.",
                                );
                                return Some(Task::none());
                            }
                            let entities = crate::scene::boundary_entities(&rings, plane);
                            if entities.len() != source.paths.len() {
                                self.discard_last_undo_entry(i);
                                self.command_line.push_error(
                                    "HATCHEDIT: boundary cannot form a polyline.",
                                );
                                return Some(Task::none());
                            }
                            let groups =
                                (0..entities.len()).map(|index| vec![index]).collect();
                            (entities, groups)
                        };
                        let mut path_handles = vec![None; source.paths.len()];
                        for (entity, paths) in entities.into_iter().zip(path_groups) {
                            if let Some(boundary) = self.commit_entity_handle(entity) {
                                for path in paths {
                                    if let Some(slot) = path_handles.get_mut(path) {
                                        *slot = Some(boundary);
                                    }
                                }
                            }
                        }
                        if associate {
                            if let Some(codec::EntityType::Hatch(hatch)) =
                                self.tabs[i].scene.document.get_entity_mut(handle)
                            {
                                for (path, boundary) in
                                    hatch.paths.iter_mut().zip(path_handles.iter().copied())
                                {
                                    if let Some(boundary) = boundary {
                                        path.boundary_handles = vec![boundary];
                                        path.flags.set_external(true);
                                    }
                                }
                                hatch.is_associative =
                                    path_handles.iter().all(Option::is_some);
                            }
                        }
                        self.tabs[i]
                            .scene
                            .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    }
                }
                HatchEditOperation::Separate => {
                    let source = self.tabs[i].scene.document.get_entity(handle).cloned();
                    if let Some(codec::EntityType::Hatch(hatch)) = source {
                        let groups = crate::scene::separated_hatch_path_groups(&hatch);
                        if groups.len() > 1 {
                            for paths in groups {
                                let mut separated = hatch.clone();
                                separated.common.handle = codec::Handle::NULL;
                                separated.paths = paths;
                                separated.is_associative = separated
                                    .paths
                                    .iter()
                                    .any(|path| !path.boundary_handles.is_empty());
                                self.tabs[i]
                                    .scene
                                    .add_entity(codec::EntityType::Hatch(separated));
                            }
                            self.tabs[i].scene.erase_entities(&[handle]);
                        }
                    }
                }
                HatchEditOperation::DrawOrderFront
                | HatchEditOperation::DrawOrderBack
                | HatchEditOperation::DrawOrderBoundary { .. }
                | HatchEditOperation::BeginAssociate => unreachable!(),
            }
            self.tabs[i].dirty = true;
            self.command_line
                .push_output(crate::t!("HATCHEDIT: hatch updated.").as_ref());
        }
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        self.restore_pre_cmd_tangent();
        None
    }

    fn apply_pedit_result(
        &mut self,
        tab: usize,
        handle: Handle,
        op: crate::modules::draw::modify::pedit::PeditOp,
    ) -> Task<Message> {
        use crate::modules::draw::modify::pedit::{apply_pedit, convert_to_polyline, PeditOp};

        if !matches!(
            &op,
            PeditOp::Multiple(_, _) | PeditOp::JoinSelection(_, _, _)
        ) && self.reject_locked_edit(tab, handle)
        {
            return Task::none();
        }
        match &op {
            PeditOp::JoinSelection(handles, fuzz, kind) => {
                let mut available = handles
                    .iter()
                    .filter_map(|handle| {
                        if self.tabs[tab].scene.is_layer_locked(*handle) {
                            return None;
                        }
                        self.tabs[tab]
                            .scene
                            .document
                            .get_entity(*handle)
                            .cloned()
                            .map(|entity| (*handle, entity))
                    })
                    .collect::<Vec<_>>();
                let connector_distance =
                    crate::modules::draw::modify::pedit::selection_connector_distance(
                        &available, *fuzz,
                    );
                let mut changes = Vec::new();
                while !available.is_empty() {
                    let (source_handle, source) = available.remove(0);
                    let candidates = available
                        .iter()
                        .map(|(handle, entity)| (*handle, entity))
                        .collect::<Vec<_>>();
                    if let Some((mut result, consumed)) =
                        crate::modules::draw::modify::pedit::join_selection_extend(
                            &source,
                            &candidates,
                            *fuzz,
                            *kind,
                            connector_distance,
                        )
                    {
                        *result.common_mut() = source.common().clone();
                        available.retain(|(handle, _)| !consumed.contains(handle));
                        changes.push((source_handle, result, consumed));
                    }
                }
                if !changes.is_empty() {
                    self.push_undo_snapshot(tab, "PEDIT");
                    for (source, replacement, consumed) in changes {
                        self.tabs[tab].scene.erase_entities(&consumed);
                        self.tabs[tab].scene.update_entity(replacement.clone());
                        if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                            for old in consumed {
                                command.on_entity_replaced(old, &[source]);
                            }
                            command.inject_picked_entity(replacement);
                        }
                    }
                    if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                        command.on_pedit_applied();
                    }
                    self.tabs[tab].dirty = true;
                    self.refresh_properties();
                }
            }
            PeditOp::Multiple(handles, operation) => {
                let replacements: Vec<_> = handles
                    .iter()
                    .filter(|handle| !self.tabs[tab].scene.is_layer_locked(**handle))
                    .filter_map(|handle| {
                        let original = self.tabs[tab].scene.document.get_entity(*handle)?;
                        if matches!(operation.as_ref(), PeditOp::ConvertToPolyline) {
                            convert_to_polyline(original)
                                .map(|replacement| (*handle, replacement, true))
                        } else {
                            let mut replacement = original.clone();
                            apply_pedit(&mut replacement, operation).then_some((
                                *handle,
                                replacement,
                                false,
                            ))
                        }
                    })
                    .collect();
                if replacements.is_empty() {
                    return Task::none();
                }
                self.push_undo_snapshot(tab, "PEDIT");
                for (handle, replacement, converted) in replacements {
                    let updated_handle = if converted {
                        self.tabs[tab].scene.erase_entities(&[handle]);
                        let new_handle = self.tabs[tab].scene.add_entity(replacement);
                        if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                            command.on_entity_replaced(handle, &[new_handle]);
                        }
                        new_handle
                    } else {
                        if let Some(entity) = self.tabs[tab].scene.document.get_entity_mut(handle) {
                            *entity = replacement;
                        }
                        self.tabs[tab].scene.refresh_fill_model(handle);
                        self.tabs[tab]
                            .scene
                            .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                        handle
                    };
                    let updated = self.tabs[tab]
                        .scene
                        .document
                        .get_entity(updated_handle)
                        .cloned();
                    if let (Some(command), Some(entity)) =
                        (self.tabs[tab].active_cmd.as_mut(), updated)
                    {
                        command.inject_picked_entity(entity);
                    }
                }
                if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                    command.on_pedit_applied();
                }
                self.tabs[tab].dirty = true;
                self.refresh_properties();
            }
            PeditOp::VertexRange { first, last, split } => {
                let pieces = self.tabs[tab]
                    .scene
                    .document
                    .get_entity(handle)
                    .and_then(|entity| {
                        crate::modules::draw::modify::pedit::edit_vertex_range(
                            entity, *first, *last, *split,
                        )
                    });
                if let Some(mut pieces) = pieces {
                    self.push_undo_snapshot(tab, "PEDIT");
                    let source = pieces.remove(0);
                    self.tabs[tab].scene.update_entity(source);
                    for mut piece in pieces {
                        piece.common_mut().handle = Handle::NULL;
                        self.tabs[tab].scene.add_entity(piece);
                    }
                    let updated = self.tabs[tab].scene.document.get_entity(handle).cloned();
                    if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                        if let Some(entity) = updated {
                            command.inject_picked_entity(entity);
                        }
                        command.on_pedit_applied();
                    }
                    self.tabs[tab].dirty = true;
                    self.refresh_properties();
                } else {
                    self.command_line.push_error("PEDIT: invalid vertex range.");
                }
            }
            PeditOp::ConvertToPolyline => {
                let converted = self.tabs[tab]
                    .scene
                    .document
                    .get_entity(handle)
                    .and_then(convert_to_polyline);
                match converted {
                    Some(polyline) => {
                        return self
                            .apply_cmd_result(CmdResult::ReplaceEntity(handle, vec![polyline]));
                    }
                    None => self
                        .command_line
                        .push_error(crate::t!("PEDIT: cannot convert this entity.").as_ref()),
                }
            }
            _ => {
                self.push_undo_snapshot(tab, "PEDIT");
                let changed = self.tabs[tab]
                    .scene
                    .document
                    .get_entity_mut(handle)
                    .map(|entity| apply_pedit(entity, &op))
                    .unwrap_or(false);
                if changed {
                    let updated = self.tabs[tab].scene.document.get_entity(handle).cloned();
                    if let Some(command) = self.tabs[tab].active_cmd.as_mut() {
                        if let Some(entity) = updated {
                            command.inject_picked_entity(entity);
                        }
                        command.on_pedit_applied();
                    }
                    self.tabs[tab].dirty = true;
                    self.tabs[tab].scene.refresh_fill_model(handle);
                    self.tabs[tab]
                        .scene
                        .bump_entities(&[(handle, crate::scene::ChangeKind::Modified)]);
                    self.command_line
                        .push_output(crate::t!("PEDIT: applied.").as_ref());
                    self.refresh_properties();
                } else {
                    self.discard_last_undo_entry(tab);
                    self.command_line.push_error(
                        crate::t!("PEDIT: operation not applicable to this entity.").as_ref(),
                    );
                }
            }
        }
        if let Some(prompt) = self.tabs[tab]
            .active_cmd
            .as_ref()
            .map(|command| command.prompt())
        {
            self.command_line.push_info(&prompt);
        }
        Task::none()
    }
}
