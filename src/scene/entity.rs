use super::*;

#[derive(Debug, Clone)]
pub struct CreateBlockOptions {
    pub name: String,
    pub handles: Vec<Handle>,
    pub base_point: glam::DVec3,
    pub world_to_block: codec::types::Transform,
    pub block_to_world: codec::types::Transform,
    pub mode: crate::ui::window::block_definition::BlockObjectMode,
    pub annotative: bool,
    pub match_orientation: bool,
    pub scale_uniformly: bool,
    pub allow_exploding: bool,
    pub unit: i16,
    pub description: String,
    pub hyperlink_url: String,
    pub hyperlink_desc: String,
    pub redefine: bool,
}

/// Order sampled boundary edges into one tip-to-tail loop.
pub(super) fn chain_path_edges(polys: Vec<Vec<[f64; 2]>>) -> Vec<[f64; 2]> {
    chain_path_edges_with_directions(polys).0
}

pub(super) fn chain_path_edges_with_directions(
    polys: Vec<Vec<[f64; 2]>>,
) -> (Vec<[f64; 2]>, Vec<f64>) {
    let d2 = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2);
    let mut directions = vec![0.0; polys.len()];
    let mut polys: Vec<_> = polys
        .into_iter()
        .enumerate()
        .filter(|(_, points)| !points.is_empty())
        .collect();
    if polys.is_empty() {
        return (Vec::new(), directions);
    }
    let (first_index, first) = polys.swap_remove(0);
    directions[first_index] = 1.0;
    let mut chain: std::collections::VecDeque<[f64; 2]> = first.into();
    while !polys.is_empty() {
        let head = *chain.front().unwrap();
        let tail = *chain.back().unwrap();
        // (distance, index, reverse-points, attach-at-front)
        let mut best = (f64::MAX, 0usize, false, false);
        for (i, (_, p)) in polys.iter().enumerate() {
            let s = p[0];
            let e = *p.last().unwrap();
            for c in [
                (d2(tail, s), i, false, false),
                (d2(tail, e), i, true, false),
                (d2(head, e), i, false, true),
                (d2(head, s), i, true, true),
            ] {
                if c.0 < best.0 {
                    best = c;
                }
            }
        }
        let (_, idx, rev, at_front) = best;
        let (original_index, mut p) = polys.swap_remove(idx);
        directions[original_index] = if rev { -1.0 } else { 1.0 };
        if rev {
            p.reverse();
        }
        if at_front {
            if d2(p[p.len() - 1], head) < 1e-18 {
                p.pop();
            }
            for q in p.into_iter().rev() {
                chain.push_front(q);
            }
        } else {
            let mut it = p.into_iter();
            if let Some(first) = it.next() {
                if d2(first, tail) >= 1e-18 {
                    chain.push_back(first);
                }
            }
            chain.extend(it);
        }
    }
    (chain.into(), directions)
}

fn family_from_stored_line(
    ln: &codec::entities::hatch::HatchPatternLine,
) -> crate::scene::model::hatch_model::PatFamily {
    let (ca, sa) = (ln.angle.cos(), ln.angle.sin());
    let dx = ln.offset.x * ca + ln.offset.y * sa;
    let dy = -ln.offset.x * sa + ln.offset.y * ca;
    crate::scene::model::hatch_model::PatFamily {
        angle_deg: ln.angle.to_degrees() as f32,
        x0: 0.0,
        y0: 0.0,
        dx: dx as f32,
        dy: dy as f32,
        dashes: ln.dash_lengths.iter().map(|&d| d as f32).collect(),
    }
}

/// Preserve the selected hue while moving its HSL lightness towards the
/// persisted one-colour tint/shade target (0 = black, 1 = white).
pub(crate) fn gradient_tint_color(base: [f32; 4], target: f32) -> [f32; 4] {
    let max = base[0].max(base[1]).max(base[2]);
    let min = base[0].min(base[1]).min(base[2]);
    let lightness = (max + min) * 0.5;
    let target = target.clamp(0.0, 1.0);
    let mut result = base;
    if target <= lightness {
        let factor = if lightness > 1.0e-6 {
            target / lightness
        } else {
            0.0
        };
        for channel in &mut result[..3] {
            *channel *= factor;
        }
    } else {
        let factor = if lightness < 1.0 - 1.0e-6 {
            (target - lightness) / (1.0 - lightness)
        } else {
            1.0
        };
        for channel in &mut result[..3] {
            *channel += (1.0 - *channel) * factor;
        }
    }
    result
}

impl Scene {
    // ── Entity management ─────────────────────────────────────────────────

    /// Register `name` in the layer table if it isn't already there, giving the
    /// new layer a real handle so it survives a DWG save (handle-based format;
    /// issue #67). Called whenever an entity is added or edited: an entity that
    /// names a layer no explicit LAYER command ever created — e.g. one supplied
    /// by a plugin through `add_entity` — otherwise has no table entry, so the
    /// DWG writer resolves its layer name to a NULL handle and it reopens on
    /// layer 0. Auto-registering keeps it on its own layer (#252). Names are
    /// registered verbatim so the writer's (case-insensitive) lookup matches;
    /// the always-present default layer "0" and empty names are no-ops.
    /// Flags `layer_table_dirty` so the app refreshes its layer lists.
    pub fn ensure_layer(&mut self, name: &str) {
        if name.trim().is_empty() || self.document.layers.contains(name) {
            return;
        }
        let mut layer = codec::tables::Layer::new(name);
        layer.handle = self.document.allocate_handle();
        let _ = self.document.layers.add(layer);
        self.invalidate_layer_dependencies(&[name.to_string()]);
        self.layer_table_dirty = true;
    }

    pub(crate) fn ensure_app_id(&mut self, name: &str) {
        if name.trim().is_empty() || self.document.app_ids.contains(name) {
            return;
        }
        let mut app_id = codec::tables::AppId::new(name);
        app_id.handle = self.document.allocate_handle();
        let _ = self.document.app_ids.add(app_id);
    }

    fn prepare_section_objects(&mut self, entity: &mut EntityType) -> bool {
        use codec::entities::ExtendedEntityData;
        use codec::objects::{
            ClassObject, ClassObjectData, SectionManager, SectionSettings, SectionTypeSettings,
        };

        let EntityType::Extended(extended) = entity else {
            return false;
        };
        let ExtendedEntityData::SectionObject(data) = &mut extended.data else {
            return false;
        };

        if extended.common.handle.is_null() {
            extended.common.handle = self.document.allocate_handle();
        }
        let entity_handle = extended.common.handle;

        let valid_settings = matches!(
            self.document.objects.get(&data.settings_handle),
            Some(ObjectType::ClassObject(object))
                if matches!(&object.data, ClassObjectData::SectionSettings(_))
        );
        if !valid_settings {
            let settings_handle = self.document.allocate_handle();
            let mut settings =
                ClassObject::new(ClassObjectData::SectionSettings(SectionSettings {
                    current_type: 4,
                    types: vec![SectionTypeSettings {
                        section_type: 4,
                        generation: 17,
                        ..SectionTypeSettings::default()
                    }],
                }));
            settings.handle = settings_handle;
            settings.owner = entity_handle;
            self.document
                .objects
                .insert(settings_handle, ObjectType::ClassObject(settings));
            data.settings_handle = settings_handle;
        } else if let Some(ObjectType::ClassObject(settings)) =
            self.document.objects.get_mut(&data.settings_handle)
        {
            settings.owner = entity_handle;
        }

        let root = crate::scene::annotative::root_named_dict_handle(&mut self.document);
        let manager_handle = self
            .document
            .objects
            .get(&root)
            .and_then(|object| match object {
                ObjectType::Dictionary(dictionary) => dictionary
                    .entries
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case("ACAD_SECTION_MANAGER"))
                    .map(|(_, handle)| *handle),
                _ => None,
            })
            .filter(|handle| {
                matches!(
                    self.document.objects.get(handle),
                    Some(ObjectType::ClassObject(object))
                        if matches!(&object.data, ClassObjectData::SectionManager(_))
                )
            })
            .unwrap_or_else(|| {
                let handle = self.document.allocate_handle();
                let mut manager =
                    ClassObject::new(ClassObjectData::SectionManager(SectionManager::default()));
                manager.handle = handle;
                manager.owner = root;
                self.document
                    .objects
                    .insert(handle, ObjectType::ClassObject(manager));
                if let Some(ObjectType::Dictionary(dictionary)) =
                    self.document.objects.get_mut(&root)
                {
                    dictionary
                        .entries
                        .retain(|(name, _)| !name.eq_ignore_ascii_case("ACAD_SECTION_MANAGER"));
                    dictionary.add_entry("ACAD_SECTION_MANAGER", handle);
                }
                handle
            });
        if let Some(ObjectType::ClassObject(manager)) =
            self.document.objects.get_mut(&manager_handle)
        {
            manager.owner = root;
            if let ClassObjectData::SectionManager(manager) = &mut manager.data {
                if !manager.sections.contains(&entity_handle) {
                    manager.sections.push(entity_handle);
                }
            }
        }
        true
    }

    pub fn add_entity(&mut self, entity: EntityType) -> Handle {
        self.add_entity_internal(entity, true)
    }

    /// Batch-add several entities, publishing geometry changes once at the end.
    /// This is the fast path used by plugin `add_entities` requests.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn add_entities(&mut self, entities: Vec<EntityType>) -> Vec<Handle> {
        let mut handles = Vec::with_capacity(entities.len());
        let mut changes = Vec::with_capacity(entities.len());
        let mut needs_geometry_bump = false;

        for entity in entities {
            let affects_blocks = matches!(&entity, EntityType::Block(_) | EntityType::BlockEnd(_));
            let handle = self.add_entity_internal(entity, false);
            handles.push(handle);
            if handle.is_null() {
                continue;
            }
            if affects_blocks {
                needs_geometry_bump = true;
            } else {
                changes.push((handle, ChangeKind::Added));
            }
        }

        if needs_geometry_bump {
            self.bump_geometry();
        } else if !changes.is_empty() {
            self.bump_entities(&changes);
        }

        handles
    }

    fn add_entity_internal(&mut self, mut entity: EntityType, bump: bool) -> Handle {
        // Only block sentinels mutate a block definition and require rebuilding
        // the block cache. A top-level INSERT merely references an existing
        // definition, so adding it can patch just that new render handle.
        let affects_blocks = matches!(&entity, EntityType::Block(_) | EntityType::BlockEnd(_));
        // INSERT invalidates rendered block instances, but it does not mutate
        // the referenced block definition. Only block sentinels require a
        // structure image for undo; ordinary owner membership is intrinsic add
        // bookkeeping and remains in place while an entity delta is undone.
        let mutates_block_structure =
            matches!(&entity, EntityType::Block(_) | EntityType::BlockEnd(_));
        let creates_section_structure = self.prepare_section_objects(&mut entity);
        let hatch_seed = if let EntityType::Hatch(dxf) = &entity {
            let color = self.render_style(&entity).0;
            Self::hatch_model_from_dxf(dxf, color)
        } else if let EntityType::Solid(solid) = &entity {
            let color = self.render_style(&entity).0;
            Some(Self::solid_hatch_model(solid, color))
        } else {
            None
        };
        let image_seed = self.image_seed_for(&entity);
        let facet_res = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_res);
        let isolines =
            crate::entities::solid3d::clamp_header_isolines(self.document.header.isolines);
        let mesh_seed = if matches!(
            &entity,
            EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_)
        ) {
            let color = self.render_style(&entity).0;
            crate::entities::solid3d::tessellate_volume(
                &entity,
                color,
                facet_res,
                chordal_deflection,
                isolines,
            )
            .map(offset_mesh_lod_set)
        } else {
            None
        };

        self.ensure_image_definition(&mut entity);

        // Register the entity's layer if it names one no LAYER command created
        // (e.g. a plugin-supplied layer) so it survives a DWG save instead of
        // collapsing to layer 0 in the reopened file (#252).
        let layer = entity.common().layer.clone();
        // Delta-undo poison inputs (captured before the mutations below): an
        // add that also creates a new layer, adds a block, or inserts an image
        // definition mutates non-entity state a pure-entity delta can't undo.
        let creates_layer = self.is_recording_undo()
            && !layer.trim().is_empty()
            && !self.document.layers.contains(&layer);
        self.ensure_layer(&layer);
        let app_ids: Vec<String> = entity
            .common()
            .extended_data
            .records()
            .iter()
            .map(|record| record.application_name.clone())
            .collect();
        let creates_app_id = self.is_recording_undo()
            && app_ids
                .iter()
                .any(|name| !name.trim().is_empty() && !self.document.app_ids.contains(name));
        for name in &app_ids {
            self.ensure_app_id(name);
        }

        // Route to the correct block based on current editing mode:
        //   - BEDIT block editor: geometry belongs to the edited block record,
        //     so it becomes part of the block definition (issue #261).
        //   - PSPACE (paper layout, no active viewport): paper-space layout block.
        //   - MSPACE or model layout: model space (document default).
        let handle = if let Some(br) = self.block_edit_block {
            entity.common_mut().owner_handle = br;
            self.document.add_entity(entity).unwrap_or(Handle::NULL)
        } else if self.current_layout != "Model" && self.active_viewport.is_none() {
            let layout_name = self.current_layout.clone();
            self.document
                .add_entity_to_layout(entity, &layout_name)
                .unwrap_or(Handle::NULL)
        } else {
            self.document.add_entity(entity).unwrap_or(Handle::NULL)
        };

        if !handle.is_null() {
            if let Some(keep) = self.refedit_keep.as_mut() {
                keep.insert(handle);
            }
            self.invalidate_dependency_index();
            if let Some(model) = hatch_seed {
                self.hatches.insert(handle, model);
            }
            if let Some(model) = image_seed {
                self.images.insert(handle, model);
            }
            if let Some(mut model) = mesh_seed {
                // Tessellated before the document assigned the handle, so the
                // mesh still carries handle 0 as its name — the entity key that
                // picking and plotting read back through `entity_handle`.
                for mesh in &mut model.lods {
                    mesh.name = handle.value().to_string();
                }
                if let Some(entity) = self.document.get_entity(handle) {
                    let color = self.render_style(entity).0;
                    let material = crate::scene::model::material_model::resolve_material_with_base(
                        &self.document,
                        entity,
                        color,
                        None,
                        self.material_base_dir.as_deref(),
                    );
                    material.apply_to_with_face_overrides(
                        &mut model,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        &mut model,
                        &self.document,
                        entity,
                    );
                }
                self.meshes.insert(handle, model);
            }
            // Delta-undo: the new handle's before-image is "nothing" (it did not
            // exist). Poison the recording if this add also mutated non-entity
            // state (a new layer, application ID, or block).
            if self.is_recording_undo() {
                self.record_undo_before(handle, None);
                if creates_layer
                    || creates_app_id
                    || mutates_block_structure
                    || creates_section_structure
                {
                    self.poison_undo_recording();
                }
            }
            if bump {
                if affects_blocks {
                    self.bump_geometry();
                } else {
                    // Plain top-level add: name the new handle so every derived cache
                    // patches in just this one entity instead of rebuilding.
                    self.bump_entities(&[(handle, ChangeKind::Added)]);
                }
            }
        }
        handle
    }

    pub fn rename_layer(&mut self, old: &str, new: &str) -> bool {
        if self.document.rename_layer(old, new).is_err() {
            return false;
        }
        self.invalidate_dependency_index();
        true
    }

    /// Rename a block definition: re-key its record, update the Block marker's
    /// name, and repoint every INSERT that referenced the old name so all
    /// instances keep resolving. Returns false if `old` is missing or
    /// anonymous/xref, `new` is invalid or already taken, or the names are
    /// equal (case-insensitive). (#261)
    pub fn rename_block(&mut self, old: &str, new: &str) -> bool {
        if !crate::scene::valid_block_name(new) {
            return false;
        }
        if old.eq_ignore_ascii_case(new) {
            return false;
        }
        if self.document.block_records.get(new).is_some() {
            return false;
        }
        // Anonymous (*) names are program-owned and re-numbered on save; an
        // xref('|') symbol name is bound to the referenced file.
        if self
            .document
            .block_records
            .get(old)
            .map(|br| br.is_anonymous() || br.flags.is_xref || br.name.contains('|'))
            .unwrap_or(true)
        {
            return false;
        }
        let Some(mut br) = self.document.block_records.remove(old) else {
            return false;
        };
        let block_marker = br.block_entity_handle;
        br.name = new.to_string();
        if self.document.block_records.add(br).is_err() {
            return false;
        }
        // Keep the Block marker entity's name in sync (used on DXF/DWG save).
        if let Some(EntityType::Block(b)) = self.document.get_entity_mut(block_marker) {
            b.name = new.to_string();
        }
        // Repoint every INSERT reference so all instances keep resolving.
        for e in self.document.entities_mut() {
            if let EntityType::Insert(ins) = e {
                if ins.block_name.eq_ignore_ascii_case(old) {
                    ins.block_name = new.to_string();
                }
            }
        }
        self.invalidate_dependency_index();
        self.bump_geometry();
        true
    }

    /// Replace the entity stored under `entity`'s handle with `entity`, keeping
    /// its identity (handle + owning block), and refresh the derived
    /// hatch/image/mesh caches so the edit is visible. Returns `false` when no
    /// entity has that handle. This is the in-place counterpart to
    /// [`add_entity`](Self::add_entity) used to commit a plugin's edit of an
    /// existing entity.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn update_entity(&mut self, entity: EntityType) -> bool {
        self.update_entities_policy(vec![entity], false) == 1
    }

    /// Batched variant of [`update_entity`]: writes every replacement, then
    /// issues a single geometry bump for the whole set instead of one bump per
    /// entity.
    pub fn update_entities(&mut self, updates: Vec<EntityType>) -> usize {
        self.update_entities_policy(updates, false)
    }

    /// Like [`update_entity`](Self::update_entity) but without the
    /// reference-only refusal. The plugin owns reference-only entities and
    /// keeps full write access through the plugin host API, so a plugin's own
    /// refresh of derived display geometry must not be refused. Locked-layer
    /// policy stays enforced either way.
    pub fn update_entity_plugin(&mut self, entity: EntityType) -> bool {
        self.update_entities_policy(vec![entity], true) == 1
    }

    /// Like [`update_entities`](Self::update_entities) but without the
    /// reference-only refusal (see [`update_entity_plugin`](Self::update_entity_plugin)).
    pub fn update_entities_plugin(&mut self, updates: Vec<EntityType>) -> usize {
        self.update_entities_policy(updates, true)
    }

    /// Shared write path of the four `update_entity*` entry points. Locked
    /// layers always refuse (inside `update_entity_impl`); reference-only
    /// entities refuse every caller except the plugin-owned `_plugin`
    /// variants, mirroring the `remove_entities` edit-policy split.
    fn update_entities_policy(
        &mut self,
        updates: Vec<EntityType>,
        allow_reference_only: bool,
    ) -> usize {
        let mut moved = 0usize;
        let mut changed = Vec::with_capacity(updates.len());
        let mut block_change = false;
        for entity in updates {
            if !allow_reference_only && self.is_reference_only(entity.common().handle) {
                continue;
            }
            if self.update_entity_impl(entity, &mut changed, &mut block_change) {
                moved += 1;
            }
        }
        if !changed.is_empty() || block_change {
            self.publish_entity_updates(changed, block_change);
        }
        moved
    }

    /// One geometry bump for a set of slot writes already reflected in
    /// `changed` / `block_change`.
    fn publish_entity_updates(&mut self, changed: Vec<(Handle, ChangeKind)>, block_change: bool) {
        if block_change {
            self.bump_geometry();
        } else {
            self.bump_entities(&changed);
        }
    }

    /// Shared write path for `update_entity` / `update_entities`: validates,
    /// records undo, writes the slot, and reseeds derived caches, recording
    /// the change instead of bumping geometry.
    fn update_entity_impl(
        &mut self,
        mut entity: EntityType,
        changed: &mut Vec<(Handle, ChangeKind)>,
        block_change: &mut bool,
    ) -> bool {
        let handle = entity.common().handle;
        if self.is_layer_locked(handle) {
            return false;
        }
        let Some(existing) = self.document.get_entity(handle) else {
            return false;
        };
        // IPC snapshots omit storage-only payloads (notably Unknown/Extended
        // entities). Preserve them when replacing a clone from a plugin.
        entity.preserve_storage_data_from(existing);
        // The caller edited a snapshot copy; keep the live entity in its block.
        entity.common_mut().owner_handle = existing.common().owner_handle;

        // Replacing (or becoming) a block sentinel forces a full block-cache
        // rebuild. INSERT edits (including retargeting to another existing
        // definition) only change that top-level render handle.
        let affects_blocks = matches!(existing, EntityType::Block(_) | EntityType::BlockEnd(_))
            || matches!(&entity, EntityType::Block(_) | EntityType::BlockEnd(_));

        // A plugin edit may retarget the entity to a novel layer; register it
        // so the edited entity keeps that layer on save instead of collapsing
        // to layer 0 in the reopened file (#252).
        let new_layer = entity.common().layer.clone();
        let creates_layer = self.is_recording_undo()
            && !new_layer.trim().is_empty()
            && !self.document.layers.contains(&new_layer);
        self.ensure_layer(&new_layer);

        // Rebuild the derived-model seeds from the new entity (as add_entity).
        let hatch_seed = if let EntityType::Hatch(dxf) = &entity {
            let color = self.render_style(&entity).0;
            Self::hatch_model_from_dxf(dxf, color)
        } else if let EntityType::Solid(solid) = &entity {
            let color = self.render_style(&entity).0;
            Some(Self::solid_hatch_model(solid, color))
        } else {
            None
        };
        let image_seed = self.image_seed_for(&entity);
        let facet_res = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_res);
        let isolines =
            crate::entities::solid3d::clamp_header_isolines(self.document.header.isolines);
        let mesh_seed = if matches!(
            &entity,
            EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_)
        ) {
            let color = self.render_style(&entity).0;
            crate::entities::solid3d::tessellate_volume(
                &entity,
                color,
                facet_res,
                chordal_deflection,
                isolines,
            )
            .map(offset_mesh_lod_set)
        } else {
            None
        };

        // Delta-undo: capture the entity's pre-edit image (before the slot is
        // overwritten) so an undo can restore it, and poison if this replace
        // also created a layer or crossed a block boundary.
        if self.is_recording_undo() {
            let before = self.document.get_entity_arc(handle);
            self.record_undo_before(handle, before);
            if creates_layer || affects_blocks {
                self.poison_undo_recording();
            }
        }

        // Write the new entity into the live slot.
        let Some(slot) = self.document.get_entity_mut(handle) else {
            return false;
        };
        *slot = entity;
        self.invalidate_dependency_index();

        // Drop stale derived caches for this handle, then reseed for the new
        // entity's type (which may differ from the old one).
        self.hatches.remove(&handle);
        self.images.remove(&handle);
        self.meshes.remove(&handle);
        self.solid_models.remove(&handle);
        if let Some(model) = hatch_seed {
            self.hatches.insert(handle, model);
        }
        if let Some(model) = image_seed {
            self.images.insert(handle, model);
        }
        if let Some(mut model) = mesh_seed {
            if let Some(entity) = self.document.get_entity(handle) {
                let color = self.render_style(entity).0;
                let material = crate::scene::model::material_model::resolve_material_with_base(
                    &self.document,
                    entity,
                    color,
                    None,
                    self.material_base_dir.as_deref(),
                );
                material.apply_to_with_face_overrides(
                    &mut model,
                    &self.document,
                    self.material_base_dir.as_deref(),
                );
                crate::scene::model::visual_style_model::apply_mesh_visual_style(
                    &mut model,
                    &self.document,
                    entity,
                );
            }
            self.meshes.insert(handle, model);
        }

        if affects_blocks {
            self.mark_entity_dirty(handle);
            *block_change = true;
        } else {
            // One entity changed in place: report just this handle so every
            // derived cache patches it instead of rebuilding (bump_entities also
            // drops it from the tessellation memos).
            changed.push((handle, ChangeKind::Modified));
        }
        true
    }

    /// Rebuild the per-entity derived caches (hatch fill / raster image / solid
    /// mesh) for a single handle from whatever entity currently lives at it —
    /// or drop them all if the handle is now absent. Mirrors the reseed block in
    /// [`Scene::update_entity`]; used by delta-undo when it re-applies an
    /// entity's before / after image so the fills and meshes follow.
    /// Rebuild every underlay's raster and wire, after a setting they draw
    /// or snap by (PDFOSNAP / UOSNAP) changed.
    pub(crate) fn reseed_underlays(&mut self) {
        let handles: Vec<Handle> = self
            .document
            .entities()
            .filter(|entity| matches!(entity, EntityType::Underlay(_)))
            .map(|entity| entity.common().handle)
            .collect();
        for handle in &handles {
            self.reseed_derived_caches(*handle);
        }
        let changes: Vec<_> = handles
            .iter()
            .map(|handle| (*handle, crate::scene::ChangeKind::Modified))
            .collect();
        self.bump_entities(&changes);
    }

    pub(crate) fn reseed_derived_caches(&mut self, handle: Handle) {
        let (hatch_seed, image_seed) = match self.document.get_entity(handle) {
            None => (None, None),
            Some(entity) => {
                let hatch_seed = if let EntityType::Hatch(dxf) = entity {
                    let color = self.render_style(entity).0;
                    Self::hatch_model_from_dxf(dxf, color)
                } else if let EntityType::Solid(solid) = entity {
                    let color = self.render_style(entity).0;
                    Some(Self::solid_hatch_model(solid, color))
                } else {
                    None
                };
                let image_seed = self.image_seed_for(entity);
                (hatch_seed, image_seed)
            }
        };
        self.hatches.remove(&handle);
        self.images.remove(&handle);
        self.meshes.remove(&handle);
        self.solid_models.remove(&handle);
        if let Some(model) = hatch_seed {
            self.hatches.insert(handle, model);
        }
        if let Some(model) = image_seed {
            self.images.insert(handle, model);
        }
        self.refresh_meshes_for_handles(&[handle]);
        self.restore_solid_models(&[handle]);
    }

    pub fn restore_solid_models(&mut self, handles: &[Handle]) {
        let bodies: Vec<(Handle, kernel::brep::Body)> = handles
            .iter()
            .filter(|handle| !self.solid_models.contains_key(handle))
            .filter_map(|&handle| {
                let tree = self.document.solid_history_tree(handle);
                let rebuilt = || {
                    tree.as_ref()
                        .and_then(|tree| kernel::acis::rebuild_history_tree(tree).ok())
                };
                let stored = || match self.document.get_entity(handle) {
                    Some(EntityType::Solid3D(solid)) => {
                        crate::scene::convert::solid3d_tess::kernel_body(solid)
                    }
                    Some(EntityType::Region(region)) => {
                        crate::scene::convert::solid3d_tess::kernel_region_body(region)
                    }
                    Some(EntityType::Surface(surface)) => {
                        crate::scene::convert::solid3d_tess::kernel_surface_body(surface)
                    }
                    _ => None,
                };
                // A composite's saved body is what its booleans produced;
                // replaying them is for edits, not for reading the file.
                let body = if tree.as_ref().is_some_and(|tree| tree.has_boolean()) {
                    stored().or_else(rebuilt)
                } else {
                    rebuilt().or_else(stored)
                }?;
                Some((handle, body))
            })
            .collect();
        self.solid_models.extend(bodies);
    }

    /// Re-tessellate only the named ACIS entities. The former edit path
    /// cleared and rebuilt every solid in the drawing when one selected solid
    /// moved or was copied.
    pub fn refresh_meshes_for_handles(&mut self, handles: &[Handle]) {
        if handles.is_empty() {
            return;
        }
        let mesh_entities: Vec<(Handle, std::sync::Arc<EntityType>)> = handles
            .iter()
            .filter_map(|&handle| {
                let entity = self.document.get_entity_arc(handle)?;
                matches!(
                    entity.as_ref(),
                    EntityType::Solid3D(_)
                        | EntityType::Region(_)
                        | EntityType::Body(_)
                        | EntityType::Surface(_)
                        | EntityType::Mesh(_)
                        | EntityType::PolygonMesh(_)
                        | EntityType::PolyfaceMesh(_)
                )
                .then_some((handle, entity))
            })
            .collect();
        for handle in handles {
            self.meshes.remove(handle);
            self.block_meshes.remove(handle);
            self.solid_models.remove(handle);
        }
        // The overwhelmingly common Undo/Redo target is 2-D geometry. Return
        // before scanning every Layout object just to discover there is no mesh
        // to rebuild.
        if mesh_entities.is_empty() {
            return;
        }
        let layout_blocks: std::collections::HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                codec::objects::ObjectType::Layout(l) if !l.block_record.is_null() => {
                    Some(l.block_record)
                }
                _ => None,
            })
            .collect();
        let entries: Vec<(Handle, std::sync::Arc<EntityType>, [f32; 4], bool)> = mesh_entities
            .into_iter()
            .map(|(handle, entity)| {
                let color = self.render_style(entity.as_ref()).0;
                let top_level = layout_blocks.contains(&entity.common().owner_handle);
                (handle, entity, color, top_level)
            })
            .collect();
        let facet_res = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_res);
        let isolines =
            crate::entities::solid3d::clamp_header_isolines(self.document.header.isolines);
        use crate::par::prelude::*;
        let built: Vec<(Handle, MeshLodSet, bool)> = entries
            .into_par_iter()
            .filter_map(|(handle, entity, color, top_level)| {
                crate::entities::solid3d::tessellate_volume(
                    entity.as_ref(),
                    color,
                    facet_res,
                    chordal_deflection,
                    isolines,
                )
                .map(|mut mesh| {
                    crate::scene::model::material_model::resolve_material_with_base(
                        &self.document,
                        entity.as_ref(),
                        color,
                        None,
                        self.material_base_dir.as_deref(),
                    )
                    .apply_to_with_face_overrides(
                        &mut mesh,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        &mut mesh,
                        &self.document,
                        entity.as_ref(),
                    );
                    let mesh = if top_level {
                        offset_mesh_lod_set(mesh)
                    } else {
                        mesh
                    };
                    (handle, mesh, top_level)
                })
            })
            .collect();
        for (handle, mut mesh, top_level) in built {
            if top_level {
                self.meshes.insert(handle, mesh);
            } else {
                mesh.prepare_instance_source(handle);
                self.block_meshes.insert(handle, mesh);
            }
        }
    }

    /// Returns the RGBA color for the given layer name.
    pub fn layer_color(&self, layer: &str) -> [f32; 4] {
        let layer_entry = self.document.layers.get(layer);
        let color = layer_entry
            .map(|l| &l.color)
            .unwrap_or(&codec::types::Color::WHITE);
        let [r, g, b, _] = crate::scene::convert::tess_util::aci_to_rgba(color);
        let alpha = layer_entry
            .map(|layer| 1.0 - layer.transparency.as_percent() as f32)
            .unwrap_or(1.0);
        [r, g, b, alpha]
    }

    pub fn custom_block_names(&self) -> Vec<String> {
        self.document
            .block_records
            .iter()
            .filter(|br| !br.is_standard() && !br.is_layout())
            .map(|br| br.name.clone())
            .collect()
    }

    pub fn create_block_from_entities(
        &mut self,
        handles: &[Handle],
        name: &str,
        world_to_block: &codec::types::Transform,
        block_to_world: &codec::types::Transform,
    ) -> Result<Handle, String> {
        self.create_block_with_options(CreateBlockOptions {
            name: name.to_string(),
            handles: handles.to_vec(),
            base_point: glam::DVec3::ZERO,
            world_to_block: *world_to_block,
            block_to_world: *block_to_world,
            mode: crate::ui::window::block_definition::BlockObjectMode::Convert,
            annotative: false,
            match_orientation: false,
            scale_uniformly: false,
            allow_exploding: true,
            unit: 0,
            description: String::new(),
            hyperlink_url: String::new(),
            hyperlink_desc: String::new(),
            redefine: false,
        })
    }

    pub fn create_block_with_options(
        &mut self,
        options: CreateBlockOptions,
    ) -> Result<Handle, String> {
        // Base-point contract: the contents are stored *relative to the picked
        // base* because `world_to_block` already subtracts it, so the
        // definition's own `Block.base_point` stays ZERO and the picked base
        // becomes the block origin (inserting at a point puts that point where
        // the base was). Callers must not also store the picked base in the
        // definition — that would subtract it twice. Imported definitions,
        // whose geometry arrives relative to their own base point, keep that
        // base point and are never rewritten here.
        let name = options.name.trim();
        if name.is_empty() {
            return Err("Block name cannot be empty.".into());
        }
        if name.starts_with('*') {
            return Err("Block name cannot start with '*'.".into());
        }

        let existing = self
            .document
            .block_records
            .get(name)
            .map(|br| (br.handle, br.block_entity_handle));

        let (br_handle, block_handle) = if let Some((existing_br_h, existing_blk_h)) = existing {
            if !options.redefine {
                return Err(format!("Block \"{name}\" already exists."));
            }
            // Redefining: remove all existing entities owned by this block record (except Block/BlockEnd)
            let owned: Vec<Handle> = self
                .document
                .entities()
                .filter(|e| {
                    e.common().owner_handle == existing_br_h
                        && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
                })
                .map(|e| e.common().handle)
                .collect();
            for h in owned {
                self.remove_image_definition_reactor(h);
                self.document.remove_entity(h);
            }
            // Update existing BlockRecord fields
            if let Some(br) = self.document.block_records.get_mut(name) {
                br.units = options.unit;
                br.description = options.description.clone();
                br.explodable = options.allow_exploding;
                br.scale_uniformly = options.scale_uniformly || options.annotative;
            }
            // Update existing Block entity fields
            if let Some(EntityType::Block(b)) = self.document.get_entity_mut(existing_blk_h) {
                b.description = options.description.clone();
                // The new contents are baked relative to the picked base (see
                // the contract above), so the definition's base point has to
                // go back to ZERO. Leaving an imported definition's non-zero
                // base point here would subtract the picked base twice.
                b.base_point = codec::types::Vector3::ZERO;
            }
            (existing_br_h, existing_blk_h)
        } else {
            let next = self.document.next_handle();
            let br_handle = Handle::new(next);
            let block_handle = Handle::new(next + 1);
            let end_handle = Handle::new(next + 2);

            let mut block_record = codec::tables::BlockRecord::new(name);
            block_record.handle = br_handle;
            block_record.block_entity_handle = block_handle;
            block_record.block_end_handle = end_handle;
            block_record.units = options.unit;
            block_record.description = options.description.clone();
            block_record.explodable = options.allow_exploding;
            block_record.scale_uniformly = options.scale_uniformly || options.annotative;
            self.document
                .block_records
                .add(block_record)
                .map_err(|e| e.to_string())?;

            let mut block = Block::new(name, codec::types::Vector3::ZERO);
            block.common.handle = block_handle;
            block.common.owner_handle = br_handle;
            block.description = options.description.clone();
            self.document
                .add_entity(EntityType::Block(block))
                .map_err(|e| e.to_string())?;

            let mut block_end = BlockEnd::new();
            block_end.common.handle = end_handle;
            block_end.common.owner_handle = br_handle;
            self.document
                .add_entity(EntityType::BlockEnd(block_end))
                .map_err(|e| e.to_string())?;
            (br_handle, block_handle)
        };
        self.write_block_definition_xdata(br_handle, block_handle, &options);

        let source_entities: Vec<_> = options
            .handles
            .iter()
            .filter_map(|&h| self.document.get_entity(h).cloned().map(|e| (h, e)))
            .collect();
        // No objects at all defines an empty block (the reference's
        // "Continue" / an empty selection at -BLOCK); objects that all vanished
        // are an error.
        if source_entities.is_empty() && !options.handles.is_empty() {
            return Err("No valid entities selected for block creation.".into());
        }

        let local = EntityTransform::Affine(options.world_to_block);
        for (old_handle, mut entity) in source_entities {
            view::dispatch::apply_transform(&mut entity, &local);
            entity = crate::modules::draw::modify::explode::normalize_entity_for_block(entity);
            if matches!(&entity, EntityType::Table(_)) {
                Self::reset_clone_subhandles(&mut self.document, &mut entity);
            }
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            self.ensure_image_definition(&mut entity);
            let handle = self
                .document
                .add_entity(entity)
                .map_err(|e| e.to_string())?;
            // An image draws from its cached model, not the entity (#2149).
            self.reseed_derived_caches(handle);
            if options.mode != crate::ui::window::block_definition::BlockObjectMode::Retain {
                self.erase_entities(&[old_handle]);
            }
        }

        let mut insert_handle = Handle::NULL;
        // An empty definition has nothing to convert into a reference.
        if options.mode == crate::ui::window::block_definition::BlockObjectMode::Convert
            && !options.handles.is_empty()
        {
            let mut insert = DxfInsert::new(name, codec::types::Vector3::ZERO);
            codec::Entity::apply_transform(&mut insert, &options.block_to_world);
            insert_handle = self.add_entity(EntityType::Insert(insert));
        }

        // The definition's thumbnail (BLOCK_RECORD 310), as the format keeps it.
        let thumbnail = self.block_thumbnail(name);
        if let Some(br) = self.document.block_records.get_mut(name) {
            br.preview_data = thumbnail;
        }

        // Advance the block epoch so all consumers and viewport renderers re-tessellate
        self.bump_geometry();
        Ok(insert_handle)
    }

    /// A 40 × 40, 32-bit bottom-up DIB of the block's geometry: white strokes
    /// on the dark preview background, fitted with a one-pixel margin. Empty
    /// for a block without drawable geometry.
    pub fn block_thumbnail(&self, name: &str) -> Vec<u8> {
        const SIZE: usize = 40;
        let wires = self.block_preview_wires(name);
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for wire in &wires {
            for p in &wire.points {
                if p[0].is_finite() && p[1].is_finite() {
                    lo = [lo[0].min(p[0] as f64), lo[1].min(p[1] as f64)];
                    hi = [hi[0].max(p[0] as f64), hi[1].max(p[1] as f64)];
                }
            }
        }
        if !lo[0].is_finite() {
            return Vec::new();
        }
        let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-9);
        let scale = (SIZE as f64 - 3.0) / span;
        let offset = [
            (SIZE as f64 - (hi[0] - lo[0]) * scale) / 2.0,
            (SIZE as f64 - (hi[1] - lo[1]) * scale) / 2.0,
        ];
        let to_px = |p: &[f32; 3]| {
            (
                ((p[0] as f64 - lo[0]) * scale + offset[0]).floor() as i64,
                ((p[1] as f64 - lo[1]) * scale + offset[1]).floor() as i64,
            )
        };
        // Bottom-up rows of BGRA pixels.
        let mut pixels = vec![[0x30u8, 0x28, 0x21, 0xFF]; SIZE * SIZE];
        let mut plot = |x: i64, y: i64| {
            if (0..SIZE as i64).contains(&x) && (0..SIZE as i64).contains(&y) {
                pixels[y as usize * SIZE + x as usize] = [0xFF; 4];
            }
        };
        for wire in &wires {
            let mut prev: Option<(i64, i64)> = None;
            for p in &wire.points {
                if !(p[0].is_finite() && p[1].is_finite()) {
                    prev = None;
                    continue;
                }
                let (x1, y1) = to_px(p);
                let (mut x0, mut y0) = prev.unwrap_or((x1, y1));
                let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
                let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
                let mut err = dx + dy;
                loop {
                    plot(x0, y0);
                    if x0 == x1 && y0 == y1 {
                        break;
                    }
                    let e2 = 2 * err;
                    if e2 >= dy {
                        err += dy;
                        x0 += sx;
                    }
                    if e2 <= dx {
                        err += dx;
                        y0 += sy;
                    }
                }
                prev = Some((x1, y1));
            }
        }
        let mut dib = Vec::with_capacity(40 + SIZE * SIZE * 4);
        for value in [40u32, SIZE as u32, SIZE as u32] {
            dib.extend_from_slice(&value.to_le_bytes());
        }
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        for value in [0u32, (SIZE * SIZE * 4) as u32, 1, 1, 0, 0] {
            dib.extend_from_slice(&value.to_le_bytes());
        }
        for pixel in pixels {
            dib.extend_from_slice(&pixel);
        }
        dib
    }

    /// The definition-level xdata of a block, kept on its block record the
    /// way the format stores it: `AcadAnnotative` (`AnnotativeData { 1 1 }`)
    /// for an annotative block, `AcadAnnoPO` when its orientation matches the
    /// layout, and the `PE_URL` hyperlink. Markers an older definition kept
    /// on its BLOCK entity are dropped.
    pub fn write_block_definition_xdata(
        &mut self,
        br_handle: Handle,
        block_handle: Handle,
        options: &CreateBlockOptions,
    ) {
        use codec::xdata::{ExtendedDataRecord, XDataValue as V};
        for app in ["AcadAnnotative", "AcadAnnoPO", "PE_URL"] {
            view::dispatch::set_entity_xdata(&mut self.document, block_handle, app, None);
            self.document.remove_object_xdata_record(br_handle, app);
        }
        let mut records = Vec::new();
        if options.annotative {
            let mut anno = ExtendedDataRecord::new("AcadAnnotative");
            for value in [
                V::String("AnnotativeData".into()),
                V::ControlString("{".into()),
                V::Integer16(1),
                V::Integer16(1),
                V::ControlString("}".into()),
            ] {
                anno.add_value(value);
            }
            records.push(anno);
            if options.match_orientation {
                let mut po = ExtendedDataRecord::new("AcadAnnoPO");
                po.add_value(V::Integer16(1));
                records.push(po);
            }
        }
        let url = options.hyperlink_url.trim();
        if !url.is_empty() {
            let mut link = ExtendedDataRecord::new("PE_URL");
            link.add_value(V::String(url.to_string()));
            link.add_value(V::ControlString("{".into()));
            let description = options.hyperlink_desc.trim();
            if !description.is_empty() {
                link.add_value(V::String(description.to_string()));
            }
            link.add_value(V::ControlString("{".into()));
            link.add_value(V::Integer32(0));
            link.add_value(V::ControlString("}".into()));
            link.add_value(V::ControlString("}".into()));
            records.push(link);
        }
        for record in records {
            if !self.document.app_ids.contains(&record.application_name) {
                let mut app = codec::tables::AppId::new(&record.application_name);
                app.handle = self.document.allocate_handle();
                let _ = self.document.app_ids.add(app);
            }
            self.document.set_object_xdata_record(br_handle, record);
        }
    }

    /// Define a new block named `name` from `entities` (owned, not yet in the
    /// document), with `base` as its insertion origin. Unlike
    /// [`create_block_from_entities`] this does NOT place an insert — the
    /// caller starts an interactive insert so paste-as-block can prompt for the
    /// drop point. The geometry comes from the clipboard rather than live
    /// entities, so there is nothing to stage or erase. (#129)
    /// Register a new image's shared definition and exclusively owned reactor.
    pub(crate) fn ensure_image_definition(&mut self, entity: &mut EntityType) {
        let EntityType::RasterImage(img) = entity else {
            return;
        };
        use codec::objects::{Dictionary, ImageDefinition, ImageDefinitionReactor, ObjectType};
        let existing = img
            .definition_handle
            .filter(|h| {
                matches!(
                    self.document.objects.get(h),
                    Some(ObjectType::ImageDefinition(_))
                )
            })
            .or_else(|| {
                self.document
                    .objects
                    .iter()
                    .find_map(|(h, object)| match object {
                        ObjectType::ImageDefinition(def)
                            if def.file_name.eq_ignore_ascii_case(&img.file_path) =>
                        {
                            Some(*h)
                        }
                        _ => None,
                    })
            });
        let def_handle = existing.unwrap_or_else(|| self.document.allocate_handle());
        let root = self.document.header.named_objects_dict_handle;
        let dictionary = match self.document.objects.get(&root) {
            Some(ObjectType::Dictionary(entries)) => entries.get("ACAD_IMAGE_DICT").filter(|h| {
                matches!(
                    self.document.objects.get(h),
                    Some(ObjectType::Dictionary(_))
                )
            }),
            _ => None,
        };
        // First-touch object images cover both new objects and shared mutations.
        if self.is_recording_undo() {
            for h in [Some(root), dictionary, Some(def_handle)]
                .into_iter()
                .flatten()
            {
                let before = self.document.objects.get(&h).cloned();
                self.record_undo_object_before(h, before);
            }
        }
        let owner = dictionary.unwrap_or_else(|| {
            let h = self.document.allocate_handle();
            self.record_undo_object_before(h, None);
            let mut entries = Dictionary::new();
            entries.handle = h;
            entries.owner = root;
            self.document
                .objects
                .insert(h, ObjectType::Dictionary(entries));
            if let Some(ObjectType::Dictionary(entries)) = self.document.objects.get_mut(&root) {
                entries.add_entry("ACAD_IMAGE_DICT", h);
            }
            h
        });
        if let Some(ObjectType::Dictionary(entries)) = self.document.objects.get_mut(&owner) {
            if !entries.entries.iter().any(|(_, h)| *h == def_handle) {
                let stem = std::path::Path::new(&img.file_path)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("IMAGE");
                let mut key = stem.to_owned();
                let mut suffix = 1;
                while entries.get(&key).is_some() {
                    suffix += 1;
                    key = format!("{stem}_{suffix}");
                }
                entries.add_entry(key, def_handle);
            }
        }
        if existing.is_none() {
            let mut def = ImageDefinition::with_dimensions(
                &img.file_path,
                img.size.x as u32,
                img.size.y as u32,
            );
            def.handle = def_handle;
            def.is_loaded = true;
            self.document
                .objects
                .insert(def_handle, ObjectType::ImageDefinition(def));
        }
        // An image that already owns a live reactor (re-added or moved into a
        // block under its own handle) keeps it.
        if let Some(own) = img.definition_reactor_handle.filter(|h| {
            !img.common.handle.is_null()
                && matches!(self.document.objects.get(h),
                    Some(ObjectType::ImageDefinitionReactor(r)) if r.owner == img.common.handle)
        }) {
            if let Some(ObjectType::ImageDefinition(def)) =
                self.document.objects.get_mut(&def_handle)
            {
                def.owner = owner;
                for h in [owner, own] {
                    if !def.reactors.contains(&h) {
                        def.reactors.push(h);
                    }
                }
            }
            img.definition_handle = Some(def_handle);
            return;
        }
        // Reserve the image's real handle before making its reactor. Copies
        // arrive with a NULL handle and must never reuse the source's reactor.
        if img.common.handle.is_null() {
            img.common.handle = self.document.allocate_handle();
        }
        let reactor_handle = self.document.allocate_handle();
        self.record_undo_object_before(reactor_handle, None);
        let mut reactor = ImageDefinitionReactor::new(img.common.handle);
        reactor.handle = reactor_handle;
        reactor.owner = img.common.handle;
        self.document
            .objects
            .insert(reactor_handle, ObjectType::ImageDefinitionReactor(reactor));
        if let Some(ObjectType::ImageDefinition(def)) = self.document.objects.get_mut(&def_handle) {
            def.owner = owner;
            if !def.reactors.contains(&owner) {
                def.reactors.push(owner);
            }
            def.reactors.push(reactor_handle);
        }
        img.definition_handle = Some(def_handle);
        img.definition_reactor_handle = Some(reactor_handle);
    }

    /// Remove an erased image's owned reactor, retaining its shared definition.
    pub(super) fn remove_image_definition_reactor(&mut self, handle: Handle) {
        let Some(EntityType::RasterImage(img)) = self.document.get_entity(handle) else {
            return;
        };
        let Some(reactor) = img.definition_reactor_handle else {
            return;
        };
        if !matches!(self.document.objects.get(&reactor),
            Some(ObjectType::ImageDefinitionReactor(r)) if r.owner == handle)
        {
            return;
        }
        let definition = img.definition_handle;
        if self.is_recording_undo() {
            for h in [Some(reactor), definition].into_iter().flatten() {
                let before = self.document.objects.get(&h).cloned();
                self.record_undo_object_before(h, before);
            }
        }
        self.document.objects.remove(&reactor);
        if let Some(ObjectType::ImageDefinition(def)) =
            definition.and_then(|h| self.document.objects.get_mut(&h))
        {
            def.reactors.retain(|h| *h != reactor);
        }
    }

    pub fn define_block_from_owned_entities(
        &mut self,
        entities: Vec<EntityType>,
        name: &str,
        base: glam::DVec3,
    ) -> Result<Vec<Handle>, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Block name cannot be empty.".into());
        }
        if name.starts_with('*') {
            return Err("Block name cannot start with '*'.".into());
        }
        if self.document.block_records.get(name).is_some() {
            return Err(format!("Block \"{name}\" already exists."));
        }
        if entities.is_empty() {
            return Err("Nothing to make into a block.".into());
        }

        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut block_record = codec::tables::BlockRecord::new(name);
        block_record.handle = br_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = end_handle;
        self.document
            .block_records
            .add(block_record)
            .map_err(|e| e.to_string())?;

        let mut block = Block::new(name, codec::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::Block(block))
            .map_err(|e| e.to_string())?;

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        self.document
            .add_entity(EntityType::BlockEnd(block_end))
            .map_err(|e| e.to_string())?;

        let local = EntityTransform::Translate(-base);
        let mut entity_handles = Vec::with_capacity(entities.len());
        for mut entity in entities {
            view::dispatch::apply_transform(&mut entity, &local);
            entity = crate::modules::draw::modify::explode::normalize_entity_for_block(entity);
            Self::reset_clone_subhandles(&mut self.document, &mut entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            self.ensure_image_definition(&mut entity);
            let handle = self
                .document
                .add_entity(entity)
                .map_err(|e| e.to_string())?;
            entity_handles.push(handle);
        }
        // Solids, surfaces and meshes of a definition draw through their
        // instanced block meshes, which only a tessellation pass creates.
        self.refresh_meshes_for_handles(&entity_handles);
        // Block defns don't render on their own, but the geometry cache must
        // pick up the new definition so the interactive insert can preview it.
        self.bump_geometry();
        Ok(entity_handles)
    }

    /// Recreate a block definition verbatim — the entities are already in
    /// block-local coordinates (unlike `define_block_from_owned_entities`,
    /// which folds in a base offset). No-op if the block already exists.
    /// Used when pasting an INSERT whose block this drawing lacks. (#135)
    pub fn define_block_raw(
        &mut self,
        name: &str,
        base_point: codec::types::Vector3,
        entities: Vec<EntityType>,
    ) {
        if name.is_empty() || self.document.block_records.get(name).is_some() {
            return;
        }
        let next = self.document.next_handle();
        let br_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);

        let mut block_record = codec::tables::BlockRecord::new(name);
        block_record.flags.anonymous = name.starts_with('*')
            && !(block_record.is_model_space() || block_record.is_paper_space());
        block_record.handle = br_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = end_handle;
        if self.document.block_records.add(block_record).is_err() {
            return;
        }

        let mut block = Block::new(name, base_point);
        block.common.handle = block_handle;
        block.common.owner_handle = br_handle;
        let _ = self.document.add_entity(EntityType::Block(block));

        let mut block_end = BlockEnd::new();
        block_end.common.handle = end_handle;
        block_end.common.owner_handle = br_handle;
        let _ = self.document.add_entity(EntityType::BlockEnd(block_end));

        let mut added = Vec::new();
        for mut entity in entities {
            Self::reset_clone_subhandles(&mut self.document, &mut entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            self.ensure_image_definition(&mut entity);
            added.extend(self.document.add_entity(entity).ok());
        }
        self.refresh_meshes_for_handles(&added);
        self.bump_geometry();
    }

    /// Replace the contents and base point of block `name`, defining it when
    /// absent. References keep pointing at it and show the new contents.
    /// Refuses and returns Err if the new contents lead back to `name` directly or transitively.
    pub fn redefine_block_raw(
        &mut self,
        name: &str,
        base_point: codec::types::Vector3,
        entities: Vec<EntityType>,
    ) -> Result<(), String> {
        for entity in &entities {
            if let EntityType::Insert(ins) = entity {
                if crate::scene::block_service::would_create_cycle(
                    &self.document,
                    name,
                    &ins.block_name,
                ) {
                    return Err(crate::scene::block_service::circular_dependency_error(
                        name,
                        &ins.block_name,
                    ));
                }
            }
        }
        let Some((br_handle, block_handle)) = self
            .document
            .block_records
            .get(name)
            .map(|br| (br.handle, br.block_entity_handle))
        else {
            self.define_block_raw(name, base_point, entities);
            return Ok(());
        };
        if self.block_edit_block == Some(br_handle) {
            return Err(format!(
                "block \"{name}\" is open in the Block Editor; close it before redefining"
            ));
        }
        let owned: Vec<Handle> = self
            .document
            .entities()
            .filter(|e| {
                e.common().owner_handle == br_handle
                    && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))
            })
            .map(|e| e.common().handle)
            .collect();
        for h in &owned {
            self.remove_image_definition_reactor(*h);
            self.document.remove_entity(*h);
        }
        if let Some(EntityType::Block(block)) = self.document.get_entity_mut(block_handle) {
            block.base_point = base_point;
        }
        // The old contents' meshes go with them; the new ones are built.
        let mut changed = owned;
        for mut entity in entities {
            Self::reset_clone_subhandles(&mut self.document, &mut entity);
            entity.common_mut().handle = Handle::NULL;
            entity.common_mut().owner_handle = br_handle;
            self.ensure_image_definition(&mut entity);
            changed.extend(self.document.add_entity(entity).ok());
        }
        self.refresh_meshes_for_handles(&changed);
        self.bump_geometry();
        Ok(())
    }

    /// Whether the resident hatch set draws `handle`'s own fill.
    pub(super) fn hatch_fill_shown(
        &self,
        handle: Handle,
        target_block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
    ) -> bool {
        let Some(entity) = self.document.get_entity(handle) else {
            return true;
        };
        // SOLID already renders through its WCS-aware fill triangles.
        // Its cached HatchModel is an XY-only plot fallback; sending
        // that to the screen too adds a flattened, grip-less copy at
        // z=0 for elevated or angled geometry (#617).
        if matches!(entity, EntityType::Solid(_)) {
            return false;
        }
        let c = entity.common();
        if self.display_entity_invisible(c)
            || self.entity_temporarily_hidden(handle)
            || self.layer_hidden(&c.layer)
            || crate::scene::annotative::annotative_offscale_for(
                &self.document,
                c,
                annotation_scale_handle,
                all_visible,
            )
        {
            return false;
        }
        // Per-viewport layer freeze: a content viewport that freezes
        // this layer hides its fills too, not just its wires.
        if self.layer_frozen_in(&c.layer, frozen) {
            return false;
        }
        // Reject block-defn-only hatches (entities owned by a
        // BLOCK record that's neither model nor a paper layout
        // block) — the scene graph emits only their laid-out copies.
        self.belongs_to_visible_block(handle, c.owner_handle, target_block)
    }

    /// Solid backdrop for a hatch's HATCHBACKGROUNDCOLOR, drawn behind `fill`;
    /// `style` is the fill's resolved (colour, ACI), which ByBlock takes.
    pub(super) fn hatch_backdrop(
        &self,
        dxf: &DxfHatch,
        fill: &HatchModel,
        style: ([f32; 4], u8),
        viewport: Option<Handle>,
    ) -> Option<HatchModel> {
        let bg = crate::entities::hatch::background_color(dxf)?;
        let mut b = fill.clone();
        b.pattern = model::hatch_model::HatchPattern::Solid;
        // ByLayer / ByBlock backgrounds resolve through the normal style chain
        // instead of the raw ACI table (#415).
        let (bg_color, bg_aci) = match bg {
            codec::types::Color::ByLayer => {
                let layer = self.document.layers.get(&dxf.common.layer);
                let aci = layer
                    .and_then(|layer| match &layer.color {
                        codec::types::Color::Index(index) => Some(*index),
                        _ => None,
                    })
                    .unwrap_or(0);
                (
                    crate::scene::view::render::layer_render_style_viewport(
                        &self.document,
                        &dxf.common.layer,
                        viewport,
                    )
                    .color,
                    aci,
                )
            }
            codec::types::Color::ByBlock => style,
            codec::types::Color::Index(index) => {
                let color = codec::types::Color::Index(index);
                (crate::scene::convert::tess_util::aci_to_rgba(&color), index)
            }
            other => (crate::scene::convert::tess_util::aci_to_rgba(&other), 0),
        };
        b.color = bg_color;
        b.aci = bg_aci;
        b.name = "SOLID".into();
        Some(b)
    }

    /// `model` synced to its entity's current display style: the fill and
    /// its optional background (HATCHBACKGROUNDCOLOR) backdrop.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn synced_hatch_model(
        &self,
        handle: Handle,
        model: &HatchModel,
        annotation_scale_handle: Option<Handle>,
        viewport: Option<Handle>,
        hatch_bg: [f32; 4],
        depth_map: &HashMap<u64, [f32; 2]>,
        tint_selected: bool,
    ) -> (Option<HatchModel>, HatchModel) {
        let contextual = self.document.get_entity(handle).map(|entity| {
            crate::scene::annotative::entity_for_annotation_context(
                &self.document,
                entity,
                annotation_scale_handle,
            )
        });
        let entity = contextual.as_deref();
        let mut m = match entity {
            Some(EntityType::Hatch(dxf))
                if crate::scene::annotative::active_object_context_for_scale(
                    &self.document,
                    handle,
                    annotation_scale_handle,
                )
                .is_some() =>
            {
                Self::hatch_model_from_dxf(dxf, model.color).unwrap_or_else(|| model.clone())
            }
            _ => model.clone(),
        };
        // Optional solid backdrop drawn behind the pattern/gradient when
        // the hatch carries a HATCHBACKGROUNDCOLOR. Same draw_depth +
        // emitted first so LessEqual layering keeps it underneath.
        let mut backdrop: Option<HatchModel> = None;
        if let Some(e) = entity {
            let mut style = crate::scene::view::render::render_style_for_viewport(
                &self.document,
                e,
                viewport,
            );
            style.0 = crate::scene::view::render::adapt_to_bg(style.0, hatch_bg);
            m.aci = style.4;
            m.line_weight_px = style.3;
            // Gradient RGB comes from its own colour stops, while
            // transparency remains an entity property shared by both
            // stops. Solid/pattern fills take the complete style.
            if matches!(m.pattern, model::hatch_model::HatchPattern::Gradient { .. }) {
                m.set_alpha(style.0[3]);
            } else {
                m.color = style.0;
            }
            if let EntityType::Hatch(dxf) = e {
                backdrop = self.hatch_backdrop(dxf, &m, (style.0, style.4), viewport);
                match &mut m.pattern {
                    // Pattern built from the hatch's own stored lines is
                    // already final (scale 1 / angle 0) — don't re-apply
                    // pattern_scale/angle. Only the catalog-derived path
                    // (empty stored lines) needs the override.
                    model::hatch_model::HatchPattern::Pattern(_)
                        if dxf.pattern.lines.is_empty() =>
                    {
                        m.angle_offset = dxf.pattern_angle as f32;
                        m.scale = dxf.pattern_scale as f32;
                    }
                    model::hatch_model::HatchPattern::Gradient { angle_deg, shift, .. } => {
                        *angle_deg = dxf.gradient_color.angle.to_degrees() as f32;
                        *shift = dxf.gradient_color.shift as f32;
                    }
                    model::hatch_model::HatchPattern::Pattern(_)
                    | model::hatch_model::HatchPattern::Solid => {}
                }
            }
        }
        m.color = self.count_color(handle, m.color, self.current_bg());
        if let Some(b) = &mut backdrop {
            b.color = self.count_color(handle, b.color, self.current_bg());
        }
        if tint_selected && self.selected.contains(&handle) {
            m.color = [0.15, 0.55, 1.00, m.color[3]];
        }
        let d = depth_map.get(&handle.value()).map_or(0.0, |d| d[0]);
        m.draw_depth = d;
        if let Some(b) = &mut backdrop {
            b.draw_depth = d;
        }
        (backdrop, m)
    }

    pub(super) fn synced_hatch_models(
        &self,
        target_block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        tint_selected: bool,
    ) -> Vec<HatchModel> {
        // synced_hatch_models is cached on geometry_epoch and the GPU
        // upload is keyed on geometry_epoch only (see render.rs — hatch
        // buffers are "static"). Don't view-cull here; the per-frame
        // skip flag in compute_hatch_lod handles frustum + sub-pixel
        // culling at draw time, which keeps the GPU upload set stable
        // across pan/zoom.
        //
        // Every content viewport supplies the block it renders. Do not depend
        // on camera/frustum culling to separate paper and model coordinates:
        // overlapping coordinates otherwise make foreign fills visible.
        let hatch_bg = if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        };
        let depth_map = self.draw_depth_map();
        let constraint_display = (self.constraint_shading_active() && self.block_edit_block == Some(target_block))
            .then(|| (self.block_constraint_report(target_block), self.constraint_palette()));
        let mut models: Vec<HatchModel> = self
            .hatches
            .iter()
            .filter(|(&handle, _)| {
                self.hatch_fill_shown(
                    handle,
                    target_block,
                    frozen,
                    annotation_scale_handle,
                    all_visible,
                )
            })
            .flat_map(|(&handle, model)| {
                let (backdrop, mut model) = self.synced_hatch_model(
                    handle,
                    model,
                    annotation_scale_handle,
                    viewport,
                    hatch_bg,
                    &depth_map,
                    tint_selected,
                );
                if let Some((report, palette)) = &constraint_display {
                    if let Some(mut color) = self.constraint_display_color(report, handle, palette) {
                        color[3] = model.color[3];
                        model.color = color;
                        if let crate::scene::model::hatch_model::HatchPattern::Gradient { color2, .. } = &mut model.pattern { *color2 = color; }
                    }
                }
                backdrop.into_iter().chain(std::iter::once(model))
            })
            .collect();

        // Background for adapting block-child hatch colours at the leaf (#221).
        // Instanced/owned hatch leaves are produced by the shared scene graph.
        models.extend(self.instanced_hatch_models(
            target_block,
            hatch_bg,
            tint_selected,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
        ));

        // Wide polyline bands remain on the wire path, including inside block
        // instances; the graph does not reclassify them as hatch fills.

        models
    }

    /// Materialize hatch leaves reached through visible scene containers in
    /// `layout_block`, with transforms and inherited styles already resolved.
    ///
    /// Shared by the on-screen hatch set (`synced_hatch_models`) and the
    /// paper/export hatch set (`paper_canvas_hatches`) so a plot draws
    /// block-internal hatches identically to the viewport.
    ///
    /// `hatch_bg` adapts pure black/white leaf colours to the target
    /// background; `tint_selected` re-colours fills of a selected INSERT
    /// (screen highlight) and should be `false` for export.
    pub(super) fn instanced_hatch_models(
        &self,
        layout_block: Handle,
        hatch_bg: [f32; 4],
        tint_selected: bool,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<HatchModel> {
        self.instanced_hatch_models_filtered(
            layout_block,
            hatch_bg,
            tint_selected,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            None,
            false,
            false,
        )
    }

    fn layer_plottable_in_context(
        &self,
        entity: &EntityType,
        context: &crate::scene::render_graph::InstanceContext,
    ) -> bool {
        let common = entity.common();
        let layer = if crate::scene::view::render::is_effective_layer_zero(&common.layer) {
            context
                .insert_path
                .iter()
                .rev()
                .find(|insert| {
                    !crate::scene::view::render::is_effective_layer_zero(&insert.common.layer)
                })
                .map(|insert| insert.common.layer.as_str())
                .unwrap_or(common.layer.as_str())
        } else {
            common.layer.as_str()
        };
        self.document
            .layers
            .get(layer)
            .map(|layer| layer.is_plottable)
            .unwrap_or(true)
    }

    pub(super) fn instanced_plot_hatch_models(
        &self,
        layout_block: Handle,
        hatch_bg: [f32; 4],
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<HatchModel> {
        self.instanced_hatch_models_filtered(
            layout_block,
            hatch_bg,
            false,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            None,
            false,
            true,
        )
    }
    /// Build live hatch overlays for INSERT grip previews. The edited INSERT
    /// is intentionally hidden from the resident scene while its current
    /// document entity moves, so include that hidden target and reuse the full
    /// block-expansion path for nested inserts, inherited styles and XCLIP.
    pub(super) fn preview_insert_hatch_models(&self, handles: &[Handle]) -> Vec<HatchModel> {
        let targets: rustc_hash::FxHashSet<Handle> = handles
            .iter()
            .copied()
            .filter(|&handle| {
                matches!(
                    self.document.get_entity(handle),
                    Some(EntityType::Insert(_))
                )
            })
            .collect();
        if targets.is_empty() {
            return Vec::new();
        }
        let hatch_bg = if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        };
        let frozen: rustc_hash::FxHashSet<Handle> = self
            .interaction_viewport_frozen_layers()
            .into_iter()
            .flatten()
            .copied()
            .collect();
        self.instanced_hatch_models_filtered(
            self.interaction_block_handle(),
            hatch_bg,
            true,
            (!frozen.is_empty()).then_some(&frozen),
            self.displayed_annotation_scale_handle(),
            self.annotation_all_visible(),
            self.active_viewport,
            Some(&targets),
            true,
            false,
        )
    }

    fn instanced_hatch_models_filtered(
        &self,
        layout_block: Handle,
        hatch_bg: [f32; 4],
        tint_selected: bool,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        targets: Option<&rustc_hash::FxHashSet<Handle>>,
        include_preview_hidden: bool,
        plot_only: bool,
    ) -> Vec<HatchModel> {
        self.instanced_hatch_models_for_document(
            &self.document,
            layout_block,
            hatch_bg,
            tint_selected,
            frozen,
            annotation_scale_handle,
            all_visible,
            viewport,
            targets,
            include_preview_hidden,
            plot_only,
        )
    }

    pub(super) fn instanced_hatch_models_for_document(
        &self,
        document: &Document,
        layout_block: Handle,
        hatch_bg: [f32; 4],
        tint_selected: bool,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
        targets: Option<&rustc_hash::FxHashSet<Handle>>,
        include_preview_hidden: bool,
        plot_only: bool,
    ) -> Vec<HatchModel> {
        let constraint_display = (!plot_only && self.constraint_shading_active() && self.block_edit_block == Some(layout_block))
            .then(|| (self.block_constraint_report(layout_block), self.constraint_palette()));
        let depth_map = self.draw_depth_map();
        let visibility_reveal = if plot_only {
            rustc_hash::FxHashSet::default()
        } else {
            self.visibility_reveal_handles()
        };
        // Plot passes draw the stored drawing; live renders of the walked
        // space hide layers outside its visible set (foreign viewports
        // resolve to `None` inside `render_space_layer_walk`).
        let layer_walk = (!plot_only)
            .then(|| self.render_space_layer_walk(layout_block, viewport))
            .flatten();
        let graph = crate::scene::render_graph::RenderSceneGraph::new(
            document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_visibility_reveal(&visibility_reveal)
        .with_layer_walk(layer_walk)
        .with_annotation_scale(self.annotation_scale)
        .with_viewport(viewport);
        let mut hatch_block_memo = std::collections::HashMap::new();
        let mut models = Vec::new();
        let mut hatch_sources = rustc_hash::FxHashMap::default();
        graph.walk_root(
            self.render_scene_root(layout_block),
            |entity, context| {
                let common = entity.common();

                if plot_only && !self.layer_plottable_in_context(entity, context) {
                    return false;
                }

                if context.is_instanced() {
                    return true;
                }
                if self.object_isolation.hides(common.handle)
                    || (!include_preview_hidden && self.preview_hidden.contains(&common.handle))
                    || self.hidden_dynamic_dimensions.contains(&common.handle)
                    // A dynamic dimension never plots.
                    || (plot_only && self.is_dynamic_dimension(common.handle))
                {
                    return false;
                }
                if let Some(targets) = targets {
                    return matches!(entity, EntityType::Insert(_))
                        && targets.contains(&common.handle);
                }
                let block_uses: Vec<_> = crate::scene::render_graph::entity_render_block_uses(
                    document,
                    entity,
                    1.0,
                )
                .into_iter()
                .filter(|block_use| block_use.active)
                .collect();
                if block_uses.is_empty() {
                    true
                } else {
                    block_uses.into_iter().any(|block_use| {
                        crate::scene::render_graph::block_contains_hatch(
                            document,
                            &block_use.insert.block_name,
                            &mut hatch_block_memo,
                        )
                    })
                }
            },
            |entity, context| {
                if !context.is_instanced() {
                    return;
                }
                let EntityType::Hatch(source_hatch) = entity else {
                    return;
                };
                if plot_only && !self.layer_plottable_in_context(entity, context) {
                    return;
                }
                let style = context.style_for(document, entity);
                let preserve_white_mask = source_hatch.is_solid
                    && matches!(source_hatch.common.color, codec::types::Color::Index(7));
                let color = if preserve_white_mask {
                    style.0
                } else {
                    crate::scene::view::render::adapt_to_bg(style.0, hatch_bg)
                };

                let mut placed = EntityType::Hatch(source_hatch.clone());
                placed.apply_transform(&context.transform);
                let EntityType::Hatch(hatch) = placed else {
                    return;
                };
                let Some(mut model) = Self::hatch_model_from_dxf(&hatch, color) else {
                    return;
                };
                if let Some((report, palette)) = &constraint_display {
                    if let Some(mut color) = self.constraint_display_color(report, context.root_handle, palette) {
                        color[3] = model.color[3];
                        model.color = color;
                        if let crate::scene::model::hatch_model::HatchPattern::Gradient { color2, .. } = &mut model.pattern { *color2 = color; }
                    }
                }
                model.aci = style.4;
                model.line_weight_px = style.3;
                model.draw_depth =
                    context.draw_depth(source_hatch.common.handle, depth_map.as_ref());
                for clip in &context.clips {
                    let clip: Vec<[f32; 2]> = clip
                        .iter()
                        .map(|point| [point[0] as f32, point[1] as f32])
                        .collect();
                    let clipped = pick::xclip::clip_hatch_boundary(
                        &model.boundary,
                        model.world_origin,
                        &clip,
                    );
                    if clipped.is_empty() {
                        return;
                    }
                    model.boundary = std::sync::Arc::new(clipped);
                }
                if !context.clips.is_empty() {
                    // The plane-local outline is a second copy of the boundary
                    // that viewport plotting prefers; rebuild it from the
                    // clipped outline or the XCLIP is lost there. (#1508)
                    model.fill_plane_boundary = model.fill_plane.map(|plane| {
                        let origin = glam::DVec3::from_array(plane.origin);
                        let (x_axis, y_axis) = (
                            glam::DVec3::from_array(plane.x_axis),
                            glam::DVec3::from_array(plane.y_axis),
                        );
                        std::sync::Arc::new(
                            model
                                .boundary
                                .iter()
                                .map(|&[x, y]| {
                                    if x.is_nan() || y.is_nan() {
                                        return [f32::NAN, f32::NAN];
                                    }
                                    let world = glam::DVec3::new(
                                        model.world_origin[0] + x as f64,
                                        model.world_origin[1] + y as f64,
                                        origin.z,
                                    );
                                    let local = world - origin;
                                    [local.dot(x_axis) as f32, local.dot(y_axis) as f32]
                                })
                                .collect(),
                        )
                    });
                }
                model.color = self.count_color(context.root_handle, model.color, hatch_bg);
                if tint_selected && self.selected.contains(&context.root_handle) {
                    model.color = [0.15, 0.55, 1.00, model.color[3]];
                }
                let matrix = &context.transform.matrix.m;
                let linear = [
                    matrix[0][0].to_bits(),
                    matrix[0][1].to_bits(),
                    matrix[0][2].to_bits(),
                    matrix[1][0].to_bits(),
                    matrix[1][1].to_bits(),
                    matrix[1][2].to_bits(),
                    matrix[2][0].to_bits(),
                    matrix[2][1].to_bits(),
                    matrix[2][2].to_bits(),
                ];
                let key = (
                    source_hatch.common.handle.value(),
                    linear,
                    model
                        .boundary
                        .iter()
                        .flat_map(|point| point.map(f32::to_bits))
                        .collect::<Vec<_>>(),
                    model.color.map(f32::to_bits),
                    model.angle_offset.to_bits(),
                    model.scale.to_bits(),
                    model.line_weight_px.to_bits(),
                    model.aci,
                );
                let source_id = *hatch_sources
                    .entry(key)
                    .or_insert_with(crate::scene::model::instance_model::next_source_id);
                model.render_instance = Some(crate::scene::model::instance_model::RenderInstance {
                    source_id,
                    translation: [matrix[0][3], matrix[1][3], matrix[2][3]],
                });
                models.push(model);
            },
        );
        models
    }

    /// Wipeout fill models — rendered in a separate pass AFTER wires so that
    /// wipeouts correctly mask everything below them in the draw order.
    pub(crate) fn wipeout_models(
        &self,
        target_block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        viewport: Option<Handle>,
    ) -> Vec<HatchModel> {
        let bg_color = if self.current_layout != "Model" {
            self.paper_bg_color
        } else {
            self.bg_color
        };
        self.wipeout_models_for_block_graph(
            target_block,
            frozen,
            annotation_scale_handle,
            all_visible,
            bg_color,
            false,
            false,
            viewport,
        )
    }

    pub(super) fn wipeout_models_for_block_graph(
        &self,
        target_block: Handle,
        frozen: Option<&rustc_hash::FxHashSet<Handle>>,
        annotation_scale_handle: Option<Handle>,
        all_visible: bool,
        bg_color: [f32; 4],
        tint_insert_selection: bool,
        plot_only: bool,
        viewport: Option<Handle>,
    ) -> Vec<HatchModel> {
        let depth_map = self.draw_depth_map();
        let visibility_reveal = if plot_only {
            rustc_hash::FxHashSet::default()
        } else {
            self.visibility_reveal_handles()
        };
        // LAYWALK hides block-tree wipeouts only in the walked space itself.
        let layer_walk = (!plot_only)
            .then(|| self.render_space_layer_walk(target_block, viewport))
            .flatten();
        let graph = crate::scene::render_graph::RenderSceneGraph::new(
            &self.document,
            frozen,
            annotation_scale_handle,
            all_visible,
            depth_map.as_ref(),
        )
        .with_visibility_reveal(&visibility_reveal)
        .with_layer_walk(layer_walk)
        .with_annotation_scale(self.annotation_scale);
        let mut models = Vec::new();
        let mut wipeout_sources = rustc_hash::FxHashMap::default();
        graph.walk_root(
            self.render_scene_root(target_block),
            |entity, context| {
                let common = entity.common();
                if plot_only && !self.layer_plottable_in_context(entity, context) {
                    return false;
                }
                context.is_instanced() || !self.entity_temporarily_hidden(common.handle)
            },
            |entity, context| {
                let EntityType::Wipeout(source) = entity else {
                    return;
                };
                let mut wipeout = source.clone();
                if context.is_instanced() {
                    wipeout.insertion_point = context.transform.apply(source.insertion_point);
                    wipeout.u_vector = context.transform.apply_rotation(source.u_vector);
                    wipeout.v_vector = context.transform.apply_rotation(source.v_vector);
                }
                let Some(mut fill_plane) = Self::wipeout_fill_plane(&wipeout) else {
                    return;
                };
                let (world_origin, mut boundary) = Self::wipeout_boundary_2d(&wipeout);
                for clip in &context.clips {
                    let clip: Vec<[f32; 2]> = clip
                        .iter()
                        .map(|point| [point[0] as f32, point[1] as f32])
                        .collect();
                    boundary = pick::xclip::clip_hatch_boundary(&boundary, world_origin, &clip);
                }
                if boundary.len() < 3 {
                    return;
                }
                if !context.clips.is_empty() {
                    let Some(local) =
                        Self::wipeout_boundary_at_xy(fill_plane.0, world_origin, &boundary)
                    else {
                        return;
                    };
                    fill_plane.1 = local;
                }
                let selection_handle = if tint_insert_selection && context.is_instanced() {
                    context.root_handle
                } else {
                    source.common.handle
                };
                let color = if self.selected.contains(&selection_handle) {
                    [0.15, 0.55, 1.00, 0.35]
                } else {
                    bg_color
                };
                let render_instance = if context.is_instanced() {
                    let matrix = &context.transform.matrix.m;
                    let linear = [
                        matrix[0][0].to_bits(),
                        matrix[0][1].to_bits(),
                        matrix[0][2].to_bits(),
                        matrix[1][0].to_bits(),
                        matrix[1][1].to_bits(),
                        matrix[1][2].to_bits(),
                        matrix[2][0].to_bits(),
                        matrix[2][1].to_bits(),
                        matrix[2][2].to_bits(),
                    ];
                    let key = (
                        source.common.handle.value(),
                        linear,
                        boundary
                            .iter()
                            .flat_map(|point| point.map(f32::to_bits))
                            .collect::<Vec<_>>(),
                        color.map(f32::to_bits),
                    );
                    let source_id = *wipeout_sources
                        .entry(key)
                        .or_insert_with(crate::scene::model::instance_model::next_source_id);
                    Some(crate::scene::model::instance_model::RenderInstance {
                        source_id,
                        translation: [matrix[0][3], matrix[1][3], matrix[2][3]],
                    })
                } else {
                    None
                };
                let fill_plane_boundary = Some(Arc::new(fill_plane.1));
                let fill_plane = Some(fill_plane.0);
                models.push(HatchModel {
                    pattern_origin: None,
                    render_instance,
                    boundary: Arc::new(boundary),
                    boundary_wcs: None,
                    fill_plane,
                    fill_plane_boundary,
                    boundary_exterior: None,
                    boundary_sources: None,
                    boundary_paths: None,
                    style: codec::entities::HatchStyleType::Normal,
                    pattern: model::hatch_model::HatchPattern::Solid,
                    name: "WIPEOUT_FILL".into(),
                    color,
                    aci: 0,
                    line_weight_px: 1.0,
                    angle_offset: 0.0,
                    scale: 1.0,
                    world_origin,
                    draw_depth: context.draw_depth(source.common.handle, depth_map.as_ref()),
                });
            },
        );
        models
    }

    /// Compute the 2D (XY) boundary polygon for a Wipeout entity.
    /// Wipeout fill boundary as small f32 offsets from the returned world_origin
    /// (the insertion point, kept in f64).
    pub(super) fn wipeout_boundary_2d(wo: &codec::entities::Wipeout) -> ([f64; 2], Vec<[f32; 2]>) {
        let origin = [wo.insertion_point.x, wo.insertion_point.y];
        let plane = kernel::space::Plane::from_axes(
            [
                wo.insertion_point.x,
                wo.insertion_point.y,
                wo.insertion_point.z,
            ],
            [wo.u_vector.x, wo.u_vector.y, wo.u_vector.z],
            [wo.v_vector.x, wo.v_vector.y, wo.v_vector.z],
        );
        let boundary = Self::wipeout_local_boundary(wo)
            .into_iter()
            .map(|point| {
                if point[0].is_finite() && point[1].is_finite() {
                    let offset = plane.vector_at(point);
                    [offset[0] as f32, offset[1] as f32]
                } else {
                    [f32::NAN; 2]
                }
            })
            .collect();
        (origin, boundary)
    }

    fn wipeout_local_boundary(wo: &codec::entities::Wipeout) -> Vec<[f64; 2]> {
        use codec::entities::{WipeoutClipMode, WipeoutClipType};
        let mut outer = vec![
            [0.0, 0.0],
            [wo.size.x, 0.0],
            [wo.size.x, wo.size.y],
            [0.0, wo.size.y],
            [0.0, 0.0],
        ];
        if !wo.clipping_enabled
            || wo.clip_boundary_vertices.len() < 3
            || !matches!(wo.clip_type, WipeoutClipType::Polygonal)
        {
            return outer;
        }
        let mut clip: Vec<[f64; 2]> = wo
            .clip_boundary_vertices
            .iter()
            .map(|point| [point.x + wo.size.x * 0.5, wo.size.y * 0.5 - point.y])
            .collect();
        if clip.last() != clip.first() {
            clip.push(clip[0]);
        }
        if matches!(wo.clip_mode, WipeoutClipMode::Inside) {
            outer.push([f64::NAN; 2]);
            outer.extend(clip);
            outer
        } else {
            clip
        }
    }

    fn wipeout_fill_plane(
        wipeout: &codec::entities::Wipeout,
    ) -> Option<(model::hatch_model::FillPlane, Vec<[f32; 2]>)> {
        let origin = [
            wipeout.insertion_point.x,
            wipeout.insertion_point.y,
            wipeout.insertion_point.z,
        ];
        let x_axis = [wipeout.u_vector.x, wipeout.u_vector.y, wipeout.u_vector.z];
        let y_axis = [wipeout.v_vector.x, wipeout.v_vector.y, wipeout.v_vector.z];
        kernel::space::Plane::from_axes(origin, x_axis, y_axis).normal()?;
        let boundary = Self::wipeout_local_boundary(wipeout)
            .into_iter()
            .map(|point| [point[0] as f32, point[1] as f32])
            .collect();
        Some((
            model::hatch_model::FillPlane {
                origin,
                x_axis,
                y_axis,
            },
            boundary,
        ))
    }

    fn wipeout_boundary_at_xy(
        plane: model::hatch_model::FillPlane,
        world_origin: [f64; 2],
        boundary: &[[f32; 2]],
    ) -> Option<Vec<[f32; 2]>> {
        let plane = kernel::space::Plane::from_axes(plane.origin, plane.x_axis, plane.y_axis);
        boundary
            .iter()
            .map(|point| {
                if point[0].is_finite() && point[1].is_finite() {
                    plane
                        .coordinates_at_xy([
                            world_origin[0] + point[0] as f64,
                            world_origin[1] + point[1] as f64,
                        ])
                        .map(|point| [point[0] as f32, point[1] as f32])
                } else {
                    Some([f32::NAN; 2])
                }
            })
            .collect()
    }

    pub(crate) fn hatch_model_from_dxf(dxf: &DxfHatch, color: [f32; 4]) -> Option<HatchModel> {
        let normal = (dxf.normal.x, dxf.normal.y, dxf.normal.z);
        // Build the boundary in f64 first so the precision-preserving
        // origin computation below sees full WCS precision. We only cast
        // to f32 once at the end, after subtracting the AABB centre, so
        // the stored offsets are small-magnitude with high f32 precision
        // even on large UTM-scale drawings.
        let to_xy = |x: f64, y: f64| -> [f64; 2] {
            let (wx, wy, _) =
                crate::scene::view::transform::ocs_point_to_wcs((x, y, dxf.elevation), normal);
            [wx, wy]
        };
        if dxf.paths.is_empty() {
            return None;
        }

        let mut rings = Vec::new();
        let mut local_rings = Vec::new();
        let mut ring_sources = Vec::new();

        for path in &dxf.paths {
            // Skip TEXTBOX boundary paths (flag bit 3). These are text
            // derived bounding boxes used for island detection; they are
            // never drawn or filled. Treating one as a fill boundary paints its
            // rectangle solid and creates a phantom bar.
            if path.flags.bits() & 8 != 0 {
                continue;
            }
            // Skip loops flagged not closed (flag bit 5). AutoCAD leaves such a
            // loop out of the hatch even when its edges meet: it neither fills
            // it nor cuts it as an island. Filling it painted areas AutoCAD
            // never shows.
            if path.flags.is_not_closed() {
                continue;
            }
            let mut edge_polys: Vec<Vec<[f64; 2]>> = Vec::new();
            for edge in &path.edges {
                if let Some(curve) = crate::entities::hatch::edge_curve(edge) {
                    edge_polys.push(curve.tessellate_angle(kernel::tessellation::DEFAULT_ANGLE));
                }
            }
            let mut local_ring = chain_path_edges(edge_polys);
            if local_ring.is_empty() {
                continue;
            }
            if local_ring.len() >= 3 {
                let first = local_ring[0];
                let last = *local_ring.last().unwrap();
                if (first[0] - last[0]).abs() > 1e-5 || (first[1] - last[1]).abs() > 1e-5 {
                    local_ring.push(first);
                }
            }
            let ring = local_ring
                .iter()
                .map(|point| to_xy(point[0], point[1]))
                .collect();
            rings.push(ring);
            local_rings.push((local_ring, path));
            ring_sources.push(path.boundary_handles.clone());
        }

        if rings.is_empty() {
            return None;
        }

        let depths = kernel::geom2d::ring_nesting_depths(&rings);
        let mut boundary = Vec::new();
        let mut local_boundary = Vec::new();
        let mut boundary_exterior = Vec::new();
        let mut boundary_sources = Vec::new();
        let mut spline_paths = Vec::new();
        for (((ring, (local_ring, path)), sources), depth) in rings
            .into_iter()
            .zip(local_rings)
            .zip(ring_sources)
            .zip(depths)
        {
            let keep = match dxf.style {
                codec::entities::HatchStyleType::Normal => true,
                codec::entities::HatchStyleType::Outer => depth <= 1,
                codec::entities::HatchStyleType::Ignore => depth == 0,
            };
            if !keep {
                continue;
            }
            if !boundary.is_empty() {
                boundary.push([f64::NAN, f64::NAN]);
                local_boundary.push([f32::NAN, f32::NAN]);
            }
            let range = boundary.len()..boundary.len() + ring.len();
            boundary.extend(ring);
            local_boundary.extend(local_ring.iter().map(|&[x, y]| [x as f32, y as f32]));
            if path
                .edges
                .iter()
                .any(|edge| matches!(edge, codec::entities::BoundaryEdge::Spline(_)))
            {
                spline_paths.push((path, local_ring, range));
            }
            boundary_exterior.push(depth == 0);
            boundary_sources.push(sources);
        }
        // The batched hatch renderer keeps boundaries in a GPU storage
        // buffer (no fixed length), so a hatch with many island loops must
        // retain *every* loop or even-odd island detection breaks. The old
        // flat `truncate(1024)` cut complex multi-loop hatches mid-boundary:
        // trailing islands were dropped and the final partial loop was left
        // open, flipping the even-odd parity so the fill bled across the
        // rest of the shape. Only guard against pathological vertex counts,
        // and when trimming, cut at a whole-loop (NaN sentinel) boundary so
        // no sub-loop is ever left open. (#148)
        const MAX_HATCH_MODEL_VERTS: usize = 16_384;
        if boundary.len() > MAX_HATCH_MODEL_VERTS {
            // Drop only whole trailing loops: cut at the last NaN sentinel
            // at/before the cap. If the first loop alone exceeds the cap,
            // keep it whole rather than leaving it open.
            let cut = boundary[..MAX_HATCH_MODEL_VERTS]
                .iter()
                .rposition(|&[x, y]| x.is_nan() || y.is_nan())
                .unwrap_or(boundary.len());
            boundary.truncate(cut);
        }

        // When the HATCH carries its own resolved pattern-line geometry
        // (angle + world-unit offset, exactly as the DWG stores it), use THAT
        // instead of re-deriving spacing from the name-matched catalog entry
        // × pattern_scale. The catalog's base spacing (e.g. metric acadiso
        // ANSI31 = 3.175) rarely matches what the drawing was authored against
        // (imperial 0.125), so the catalog path rendered lines up to ~25×
        // (= inch→mm) too coarse — a dense fill collapsed to a few stray
        // lines. The stored offset is the authoritative world-unit spacing, so
        // the resulting families are already final: no pattern_scale / angle
        // is re-applied (see `prebaked` below and the HatchModel fields).
        let prebaked =
            !dxf.is_solid && !dxf.gradient_color.is_enabled() && !dxf.pattern.lines.is_empty();

        // The gradient's first stop is the fill's start colour (not the
        // entity colour); capture it so the HatchModel draws stop-0 → stop-1.
        let mut gradient_color1: Option<[f32; 4]> = None;
        let mut pattern = if dxf.gradient_color.is_enabled() {
            let stop = |i: usize| {
                dxf.gradient_color
                    .colors
                    .get(i)
                    .and_then(|e| e.color.rgb())
                    .map(|(r, g, b)| {
                        [
                            r as f32 / 255.0,
                            g as f32 / 255.0,
                            b as f32 / 255.0,
                            color[3],
                        ]
                    })
            };
            gradient_color1 = stop(0);
            let color1 = gradient_color1.unwrap_or(color);
            let color2 = if dxf.gradient_color.is_single_color {
                gradient_tint_color(color1, dxf.gradient_color.color_tint as f32)
            } else {
                stop(1).unwrap_or(color)
            };
            let angle_deg = dxf.gradient_color.angle.to_degrees() as f32;
            let (kind, invert) =
                model::hatch_model::GradientKind::from_name(&dxf.gradient_color.name);
            model::hatch_model::HatchPattern::Gradient {
                angle_deg,
                color2,
                kind,
                invert,
                shift: dxf.gradient_color.shift as f32,
            }
        } else if dxf.is_solid {
            model::hatch_model::HatchPattern::Solid
        } else if prebaked {
            model::hatch_model::HatchPattern::Pattern(
                dxf.pattern
                    .lines
                    .iter()
                    .map(family_from_stored_line)
                    .collect(),
            )
        } else {
            let pat_name = &dxf.pattern.name;
            if let Some(entry) = crate::scene::model::hatch_patterns::find(pat_name) {
                entry.gpu.clone()
            } else if matches!(
                dxf.pattern_type,
                codec::entities::hatch::HatchPatternType::UserDefined
            ) {
                // User-defined hatch: parallel lines at `pattern_angle`, spaced
                // `pattern_scale` apart, plus a perpendicular set when
                // `is_double`. Its name ("_USER") is not a catalog pattern.
                // Build BASE families (angle 0, and 90 for the cross set) with
                // unit perpendicular spacing; the HatchModel's angle_offset
                // (= pattern_angle) and scale (= pattern_scale) below rotate and
                // space them — exactly as a predefined .PAT pattern is applied —
                // so the angle/scale is applied once, not doubled. Replaces the
                // old fallback that forced every user-defined hatch to flat
                // horizontal lines at the wrong spacing (#278).
                let fam = |angle_deg: f32| model::hatch_model::PatFamily {
                    angle_deg,
                    x0: 0.0,
                    y0: 0.0,
                    dx: 0.0,
                    dy: 1.0,
                    dashes: vec![],
                };
                let mut fams = vec![fam(0.0)];
                if dxf.is_double {
                    fams.push(fam(90.0));
                }
                model::hatch_model::HatchPattern::Pattern(fams)
            } else {
                model::hatch_model::HatchPattern::Pattern(vec![model::hatch_model::PatFamily {
                    angle_deg: 0.0,
                    x0: 0.0,
                    y0: 0.0,
                    dx: 0.0,
                    dy: 5.0 * dxf.pattern_scale as f32,
                    dashes: vec![],
                }])
            }
        };

        let name = if dxf.gradient_color.is_enabled() {
            dxf.gradient_color.name.clone()
        } else if dxf.is_solid {
            "SOLID".into()
        } else {
            dxf.pattern.name.clone()
        };

        // Precision-preserving cast f64 → f32: pick an `world_origin`
        // anchor (boundary AABB centre in f64) and store every vertex
        // as a small f32 offset from it. NaN separators are preserved
        // so the in_polygon ray-cast still sees the path breaks.
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for &[x, y] in &boundary {
            if x.is_finite() && y.is_finite() {
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
        let world_origin = if min_x.is_finite() && min_y.is_finite() {
            [(min_x + max_x) * 0.5, (min_y + max_y) * 0.5]
        } else {
            [0.0, 0.0]
        };

        // Anchor the prebaked families near the geometry, tracking the pattern
        // origin. The DWG base points sit at the pattern's *authored* origin,
        // which on a UTM drawing can be ~1e6 units from the hatch. Handing the
        // shader `base - world_origin` (~1e6) is wrong two ways: it consumes
        // `x0/y0` as f32 (so ~0.5 m quantization shreds the phase — boundaries
        // are unaffected, they ride the double-single relative-to-eye path), AND
        // it evaluates the pattern that far from its origin. Multi-family
        // aggregate patterns (AR-CONC, AR-SAND, GRAVEL, …) are effectively
        // quasi-periodic: their families only cohere into the intended
        // stones/grains near their shared origin and dissolve into scattered
        // dashes at large offsets.
        //
        // So fold the origin's offset from `world_origin` down to a small,
        // coherence-safe remainder on a grid of `spacing * 64` (many multiples
        // of the pattern spacing — well inside the coherence range yet far
        // larger than any realistic grip drag). Applying ONE common fold (from
        // the reference line) preserves each family's relative phase, which is
        // what forms the stones. Because the origin grip / Origin X/Y edit
        // shifts every base point by the same delta, the sub-grid remainder
        // tracks that delta 1:1, so the grip still moves the fill; a grid
        // crossing (a whole `spacing * 64` drag) is never hit in practice.
        if prebaked {
            if let model::hatch_model::HatchPattern::Pattern(fams) = &mut pattern {
                if let Some(rb) = dxf.pattern.lines.first().map(|l| l.base_point) {
                    let spacing = dxf
                        .pattern
                        .lines
                        .iter()
                        .map(|l| l.offset.length())
                        .fold(0.0_f64, f64::max)
                        .max(1e-6);
                    let cell = spacing * 64.0;
                    let fold = |v: f64, o: f64| -> f64 {
                        let d = v - o;
                        d - (d / cell).round() * cell
                    };
                    let ox = fold(rb.x, world_origin[0]);
                    let oy = fold(rb.y, world_origin[1]);
                    for (fam, ln) in fams.iter_mut().zip(dxf.pattern.lines.iter()) {
                        fam.x0 = (ln.base_point.x - rb.x + ox) as f32;
                        fam.y0 = (ln.base_point.y - rb.y + oy) as f32;
                    }
                }
            }
        }
        let project = |point: [f64; 2]| {
            let [x, y] = to_xy(point[0], point[1]);
            [
                (x - world_origin[0]) as f32 as f64,
                (y - world_origin[1]) as f32 as f64,
            ]
        };
        for (path, ring, range) in spline_paths.into_iter().rev() {
            if range.end > boundary.len() {
                continue;
            }
            let curves = || {
                path.edges
                    .iter()
                    .filter_map(crate::entities::hatch::edge_curve)
                    .collect()
            };
            if let Some(refined) = kernel::geom2d::refine_spline_boundary(&ring, curves, &project) {
                if boundary.len() - range.len() + refined.len() > MAX_HATCH_MODEL_VERTS {
                    continue;
                }
                boundary.splice(range.clone(), refined.iter().map(|p| to_xy(p[0], p[1])));
                local_boundary.splice(
                    range,
                    refined.into_iter().map(|[x, y]| [x as f32, y as f32]),
                );
            }
        }

        let boundary_f32: Vec<[f32; 2]> = boundary
            .iter()
            .map(|&[x, y]| {
                if x.is_finite() && y.is_finite() {
                    [(x - world_origin[0]) as f32, (y - world_origin[1]) as f32]
                } else {
                    [f32::NAN, f32::NAN]
                }
            })
            .collect();

        let storage = crate::entities::curve::ocs_plane(dxf.normal, dxf.elevation);
        Some(HatchModel {
            pattern_origin: Some([dxf.pattern_origin().x, dxf.pattern_origin().y]),
            render_instance: None,
            boundary: std::sync::Arc::new(boundary_f32),
            boundary_wcs: None,
            fill_plane: Some(model::hatch_model::FillPlane {
                origin: storage.origin,
                x_axis: storage.x_axis,
                y_axis: storage.y_axis,
            }),
            fill_plane_boundary: Some(std::sync::Arc::new(local_boundary)),
            boundary_exterior: Some(std::sync::Arc::new(boundary_exterior)),
            boundary_sources: Some(std::sync::Arc::new(boundary_sources)),
            boundary_paths: Some(std::sync::Arc::new(dxf.paths.clone())),
            style: dxf.style,
            pattern,
            name,
            // A gradient starts from its first stop; other fills use the
            // entity colour.
            color: gradient_color1.unwrap_or(color),
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: if prebaked {
                0.0
            } else {
                dxf.pattern_angle as f32
            },
            scale: if prebaked {
                1.0
            } else {
                dxf.pattern_scale as f32
            },
            world_origin,
            draw_depth: 0.0,
        })
    }

    /// Decode and cache all RasterImage entities from the current document.
    /// Silently skips images whose files cannot be read.
    pub fn populate_images_from_document(&mut self) {
        self.populate_images_from_document_unbumped();
        self.bump_geometry();
    }

    /// Web drop flow: `names` are raster files whose bytes were just
    /// registered (name-keyed). When this drawing references one of them by
    /// name but carries no model for it — the reference pointed at the
    /// author's machine and was unresolvable at open time — rebuild the image
    /// models so the entity finally renders. Returns whether a rebuild ran.
    pub fn resolve_registered_image_names(&mut self, names: &[String]) -> bool {
        let matches_name = |img: &codec::entities::RasterImage| {
            names
                .iter()
                .any(|name| crate::scene::model::image_model::same_file_name(&img.file_path, name))
        };
        let unresolved = self.document.entities().any(|entity| match entity {
            EntityType::RasterImage(img) => {
                matches_name(img) && !self.images.contains_key(&img.common.handle)
            }
            _ => false,
        });
        if unresolved {
            self.populate_images_from_document();
        }
        unresolved
    }

    fn populate_images_from_document_unbumped(&mut self) {
        self.images.clear();
        let entries: Vec<(Handle, codec::entities::EntityType)> = self
            .document
            .entities()
            .filter(|e| {
                matches!(
                    e,
                    EntityType::RasterImage(_) | EntityType::Ole2Frame(_) | EntityType::Underlay(_)
                )
            })
            .map(|e| (e.common().handle, e.clone()))
            .collect();
        for (handle, entity) in entries {
            if let Some(model) = self.image_seed_for(&entity) {
                self.images.insert(handle, model);
            }
        }
    }

    /// Rebuild the cached fill model (hatch / DXF SOLID) for `handle` after
    /// its document entity was edited in place. The fill models are prebuilt
    /// at load, so a pattern-scale / background / boundary edit stays
    /// invisible until the cached model is refreshed (#415).
    pub fn refresh_fill_model(&mut self, handle: Handle) {
        let contextual = self.document.get_entity(handle).map(|entity| {
            crate::scene::annotative::entity_for_annotation_context(
                &self.document,
                entity,
                self.displayed_annotation_scale_handle(),
            )
        });
        let new_model = match contextual.as_deref() {
            Some(EntityType::Hatch(dxf)) => {
                let color = convert::tess_util::aci_to_rgba(&dxf.common.color);
                Self::hatch_model_from_dxf(dxf, color)
            }
            Some(EntityType::Solid(s)) => {
                let color = convert::tess_util::aci_to_rgba(&s.common.color);
                Some(Self::solid_hatch_model(s, color))
            }
            _ => None,
        };
        if let Some(model) = new_model {
            self.hatches.insert(handle, model);
        }
    }

    pub fn populate_hatches_from_document(&mut self) {
        self.populate_hatches_from_document_unbumped();
        self.bump_geometry();
    }

    fn populate_hatches_from_document_unbumped(&mut self) {
        self.hatches.clear();

        let entries: Vec<(Handle, EntityType)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some((
                    h.common.handle,
                    crate::scene::annotative::entity_for_annotation_context(
                        &self.document,
                        e,
                        self.displayed_annotation_scale_handle(),
                    )
                    .into_owned(),
                )),
                EntityType::Solid(s) => Some((s.common.handle, e.clone())),
                _ => None,
            })
            .collect();

        use crate::par::prelude::*;
        self.hatches = entries
            .into_par_iter()
            .filter_map(|(handle, kind)| {
                // Paper-space entities live in sheet coordinates — world_offset must not
                let model = match &kind {
                    EntityType::Hatch(dxf) => {
                        let color = convert::tess_util::aci_to_rgba(&dxf.common.color);
                        Self::hatch_model_from_dxf(dxf, color)
                    }
                    EntityType::Solid(solid) => {
                        let color = convert::tess_util::aci_to_rgba(&solid.common.color);
                        Some(Self::solid_hatch_model(solid, color))
                    }
                    _ => None,
                };
                model.map(|m| (handle, m))
            })
            .collect();
    }

    /// ImageModel for an image-bearing entity. `None` when it cannot decode.
    pub(super) fn image_seed_for(
        &self,
        entity: &codec::entities::EntityType,
    ) -> Option<ImageModel> {
        match entity {
            EntityType::RasterImage(img) => ImageModel::from_raster_image(img),
            EntityType::Ole2Frame(ole) => ImageModel::from_ole2frame(ole),
            EntityType::Underlay(u) => match self.document.objects.get(&u.definition_handle) {
                Some(codec::objects::ObjectType::UnderlayDefinition(def)) => {
                    // A paper-space underlay adjusts to the sheet, not the canvas.
                    let owner = u.common.owner_handle;
                    let background = if owner.is_null() || owner == self.model_space_block_handle()
                    {
                        self.bg_color
                    } else {
                        self.paper_bg_color
                    };
                    ImageModel::from_underlay(u, def, background, self.underlay_world_per_pixel)
                }
                _ => None,
            },
            _ => None,
        }
    }

    pub fn populate_meshes_from_document(&mut self) {
        self.populate_meshes_impl(false, true);
    }

    /// Like [`populate_meshes_from_document`] but tessellates only solids
    /// whose handle is not already cached — the existing meshes are kept.
    ///
    /// Used after an XREF merge: the host document's solids were already
    /// tessellated by the background loader, and the merge assigns brand-new
    /// handles to every imported xref entity (see `merge_xref_into_block`),
    /// so cached handles are guaranteed to be host solids. This turns the
    /// post-xref pass from "re-tessellate host + all xrefs" into "tessellate
    /// only the newly merged xref solids" — the dominant cost when a drawing
    /// attaches several large xrefs. (#203)
    pub fn populate_missing_meshes_from_document(&mut self) {
        self.populate_meshes_impl(true, true);
    }

    fn populate_meshes_impl(&mut self, incremental: bool, bump: bool) {
        if !incremental {
            self.meshes.clear();
            self.block_meshes.clear();
            // The B-rep cache follows the entities too: an undo that rebuilds
            // everything left the undone fillet's body here, and the next
            // edge operation brought it back (#1107). Restored on demand.
            self.solid_models.clear();
        }
        // BLOCK-entity handles of the layout (model + paper) blocks. A solid
        // owned by one of these is top-level; anything else lives in a block
        // definition and is instanced per INSERT instead. (#123)
        let layout_blocks: std::collections::HashSet<Handle> = self
            .document
            .objects
            .values()
            .filter_map(|o| match o {
                codec::objects::ObjectType::Layout(l) if !l.block_record.is_null() => {
                    Some(l.block_record)
                }
                _ => None,
            })
            .collect();
        // Resolve color through `render_style` so the same bg adaptation
        // wires use kicks in (pure black on dark bg → white, pure white
        // on light bg → black). Without this, ACIS meshes ignore
        // `adapt_to_bg` and stay invisible against matching bg colours.
        let entries: Vec<(Handle, EntityType, [f32; 4], bool)> = self
            .document
            .entities()
            .filter_map(|e| match e {
                EntityType::Solid3D(_)
                | EntityType::Region(_)
                | EntityType::Body(_)
                | EntityType::Surface(_)
                | EntityType::Mesh(_)
                | EntityType::PolygonMesh(_)
                | EntityType::PolyfaceMesh(_) => {
                    let handle = e.common().handle;
                    // Incremental (post-xref) pass: leave already-tessellated
                    // host solids untouched, only build the newly merged ones.
                    if incremental
                        && (self.meshes.contains_key(&handle)
                            || self.block_meshes.contains_key(&handle))
                    {
                        return None;
                    }
                    let color = self.render_style(e).0;
                    let top_level = layout_blocks.contains(&e.common().owner_handle);
                    Some((handle, e.clone(), color, top_level))
                }
                _ => None,
            })
            .collect();

        use crate::par::prelude::*;
        let facet_res = self.document.header.facet_resolution;
        let chordal_deflection =
            crate::entities::solid3d::display_deflection(&self.document.header, facet_res);
        let isolines =
            crate::entities::solid3d::clamp_header_isolines(self.document.header.isolines);
        // Top-level solids: offset into the render frame, drawn flat.
        // Block-definition solids: keep block-local coords for per-INSERT
        // instancing (no offset applied here).
        let built: Vec<(Handle, MeshLodSet, bool)> = entries
            .into_par_iter()
            .filter_map(|(handle, entity, color, top_level)| {
                crate::entities::solid3d::tessellate_volume(
                    &entity,
                    color,
                    facet_res,
                    chordal_deflection,
                    isolines,
                )
                .map(|mut mesh| {
                    let material = crate::scene::model::material_model::resolve_material_with_base(
                        &self.document,
                        &entity,
                        color,
                        None,
                        self.material_base_dir.as_deref(),
                    );
                    material.apply_to_with_face_overrides(
                        &mut mesh,
                        &self.document,
                        self.material_base_dir.as_deref(),
                    );
                    crate::scene::model::visual_style_model::apply_mesh_visual_style(
                        &mut mesh,
                        &self.document,
                        &entity,
                    );
                    let mesh = if top_level {
                        offset_mesh_lod_set(mesh)
                    } else {
                        mesh
                    };
                    (handle, mesh, top_level)
                })
            })
            .collect();
        for (handle, mut m, top_level) in built {
            if top_level {
                self.meshes.insert(handle, m);
            } else {
                m.prepare_instance_source(handle);
                self.block_meshes.insert(handle, m);
            }
        }

        if bump {
            self.bump_geometry();
        }
    }

    /// Rebuild hatch / image / mesh caches after the document is modified
    /// outside the normal `add_entity` path (e.g. REFCLOSE SAVE).
    pub fn rebuild_derived_caches(&mut self) {
        self.invalidate_dependency_index();
        self.populate_hatches_from_document_unbumped();
        self.populate_images_from_document_unbumped();
        self.populate_meshes_impl(false, false);
        self.bump_geometry();
    }

    /// Build a solid-fill HatchModel for a DXF Solid entity.
    /// SOLID corners use Z-order. Preserve it in the projected hatch as well so
    /// intentionally crossing geometry is not silently rewritten.
    pub(super) fn solid_hatch_model(solid: &DxfSolid, color: [f32; 4]) -> HatchModel {
        // Keep the corners in f64 until the AABB centre is known, then store
        // each as a small f32 offset from it — same precision-preserving anchor
        // `hatch_model_from_dxf` uses. Casting the absolute WCS corner straight
        // to f32 costs ~0.06 units of resolution at UTM magnitudes (~1e6), so
        // the quad snapped to a grid and the fill drifted off its outline.
        let wcs = crate::entities::solid::wcs_corners(solid);
        let order = [0, 1, 3, 2];
        let corners: [[f64; 2]; 4] = order.map(|index| [wcs[index][0], wcs[index][1]]);
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for c in &corners {
            for i in 0..2 {
                if c[i] < min[i] {
                    min[i] = c[i];
                }
                if c[i] > max[i] {
                    max[i] = c[i];
                }
            }
        }
        let world_origin = if min[0].is_finite() && min[1].is_finite() {
            [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5]
        } else {
            [0.0, 0.0]
        };
        let boundary = corners
            .iter()
            .map(|c| {
                [
                    (c[0] - world_origin[0]) as f32,
                    (c[1] - world_origin[1]) as f32,
                ]
            })
            .collect();
        HatchModel {
            pattern_origin: None,
            render_instance: None,
            boundary: std::sync::Arc::new(boundary),
            boundary_wcs: None,
            fill_plane: None,
            fill_plane_boundary: None,
            boundary_exterior: None,
            boundary_sources: None,
            boundary_paths: None,
            style: codec::entities::HatchStyleType::Normal,
            pattern: model::hatch_model::HatchPattern::Solid,
            name: "SOLID".into(),
            color,
            aci: 0,
            line_weight_px: 1.0,
            angle_offset: 0.0,
            scale: 1.0,
            world_origin,
            draw_depth: 0.0,
        }
    }

    pub fn add_hatch(
        &mut self,
        model: HatchModel,
        layer: Option<&str>,
        entity_style: Option<(codec::types::Color, codec::types::Transparency)>,
    ) -> Handle {
        let mut dxf = DxfHatch::new();
        dxf.style = model.style;
        if let Some(plane) = model.fill_plane {
            let x = glam::DVec3::from_array(plane.x_axis);
            let y = glam::DVec3::from_array(plane.y_axis);
            let normal = x.cross(y).normalize_or(glam::DVec3::Z);
            dxf.normal = codec::types::Vector3::new(normal.x, normal.y, normal.z);
            dxf.elevation = glam::DVec3::from_array(plane.origin).dot(normal);
        }
        dxf.is_solid = matches!(
            model.pattern,
            crate::scene::model::hatch_model::HatchPattern::Solid
        );
        // Persist analytic command geometry when available.
        if let Some(paths) = model.boundary_paths.as_deref() {
            dxf.paths = paths.clone();
        } else {
            // Otherwise reconstruct every ring from render offsets.
            let reconstructed_wcs: Vec<[f64; 2]> = if model.boundary_wcs.is_none() {
                let [wx, wy] = model.world_origin;
                model
                    .boundary
                    .iter()
                    .map(|&[x, y]| {
                        if x.is_finite() && y.is_finite() {
                            [x as f64 + wx, y as f64 + wy]
                        } else {
                            [f64::NAN, f64::NAN]
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let wcs = model
                .boundary_wcs
                .as_deref()
                .map(|points| points.as_slice())
                .unwrap_or(reconstructed_wcs.as_slice());
            let mut ring: Vec<Vector2> = Vec::new();
            let mut first = true;
            let mut ring_index = 0usize;
            let mut push_ring = |r: &mut Vec<Vector2>, is_outer: bool, index: usize| {
                if !r.is_empty() {
                    let edge = PolylineEdge::new(std::mem::take(r), true);
                    let handles: Vec<_> = model
                        .boundary_sources
                        .as_deref()
                        .and_then(|sources| sources.get(index))
                        .into_iter()
                        .flatten()
                        .copied()
                        .filter(|handle| handle.is_valid())
                        .collect();
                    let mut bits = 0;
                    if is_outer {
                        bits |= codec::entities::hatch::BoundaryPathFlags::OUTERMOST.bits();
                        bits |= codec::entities::hatch::BoundaryPathFlags::EXTERNAL.bits();
                    }
                    let mut path = BoundaryPath::with_flags(
                        codec::entities::hatch::BoundaryPathFlags::from_bits(bits),
                    );
                    path.add_edge(BoundaryEdge::Polyline(edge));
                    for handle in handles {
                        path.add_boundary_handle(handle);
                    }
                    dxf.paths.push(path);
                }
            };
            for &[x, y] in wcs {
                if x.is_finite() && y.is_finite() {
                    ring.push(Vector2::new(x, y));
                } else {
                    let is_outer = model
                        .boundary_exterior
                        .as_deref()
                        .and_then(|roles| roles.get(ring_index))
                        .copied()
                        .unwrap_or(first);
                    first = false;
                    if !ring.is_empty() {
                        push_ring(&mut ring, is_outer, ring_index);
                        ring_index += 1;
                    }
                }
            }
            let is_outer = model
                .boundary_exterior
                .as_deref()
                .and_then(|roles| roles.get(ring_index))
                .copied()
                .unwrap_or(first);
            push_ring(&mut ring, is_outer, ring_index);
        }
        dxf.is_associative = dxf
            .paths
            .iter()
            .any(|path| !path.boundary_handles.is_empty());
        let pattern_scale = if model.scale.abs() > 1e-6 {
            model.scale as f64
        } else {
            1.0
        };
        let pattern_origin = model
            .fill_plane_boundary
            .as_deref()
            .and_then(|points| {
                points
                    .iter()
                    .find(|point| point[0].is_finite() && point[1].is_finite())
            })
            .map(|point| [point[0] as f64, point[1] as f64])
            .unwrap_or(model.world_origin);
        if let crate::scene::model::hatch_model::HatchPattern::Pattern(families) = &model.pattern {
            let mut pattern = codec::entities::HatchPattern::new(&model.name);
            let rotation = model.angle_offset as f64;
            let (rotation_sin, rotation_cos) = rotation.sin_cos();
            for family in families {
                let family_angle = (family.angle_deg as f64).to_radians();
                let angle = family_angle + rotation;
                let (family_sin, family_cos) = family_angle.sin_cos();
                let local_offset_x = family.dx as f64 * family_cos - family.dy as f64 * family_sin;
                let local_offset_y = family.dx as f64 * family_sin + family.dy as f64 * family_cos;
                let base_x = family.x0 as f64 * pattern_scale;
                let base_y = family.y0 as f64 * pattern_scale;
                pattern.lines.push(codec::entities::HatchPatternLine {
                    angle,
                    base_point: Vector2::new(
                        pattern_origin[0] + base_x * rotation_cos - base_y * rotation_sin,
                        pattern_origin[1] + base_x * rotation_sin + base_y * rotation_cos,
                    ),
                    offset: Vector2::new(
                        (local_offset_x * rotation_cos - local_offset_y * rotation_sin)
                            * pattern_scale,
                        (local_offset_x * rotation_sin + local_offset_y * rotation_cos)
                            * pattern_scale,
                    ),
                    dash_lengths: family
                        .dashes
                        .iter()
                        .map(|dash| *dash as f64 * pattern_scale)
                        .collect(),
                });
            }
            dxf.pattern = pattern;
        }
        dxf.pattern_angle = model.angle_offset as f64;
        dxf.pattern_scale = pattern_scale;
        // A gradient fill must be encoded on the DXF entity itself: the render
        // model is rebuilt from the entity below (`add_entity` →
        // `hatch_model_from_dxf`), so a gradient kept only on the command's
        // model silently degraded to a plain pattern hatch.
        if let crate::scene::model::hatch_model::HatchPattern::Gradient {
            angle_deg,
            color2,
            kind,
            invert,
            shift,
        } = &model.pattern
        {
            let to_color = |c: [f32; 4]| codec::types::Color::Rgb {
                r: (c[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                g: (c[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                b: (c[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            };
            dxf.is_solid = true;
            dxf.gradient_color.enabled = true;
            dxf.gradient_color.name = kind.dxf_name(*invert).to_string();
            // Keep both persisted angle fields aligned.
            dxf.pattern_angle = (*angle_deg as f64).to_radians();
            dxf.gradient_color.angle = (*angle_deg as f64).to_radians();
            dxf.gradient_color.shift = *shift as f64;
            dxf.gradient_color.is_single_color = false;
            // Linear has no INV name in the standard set: persist an inverted
            // linear by swapping the colour stops instead.
            let (c0, c1) = if *invert
                && matches!(kind, crate::scene::model::hatch_model::GradientKind::Linear)
            {
                (*color2, model.color)
            } else {
                (model.color, *color2)
            };
            dxf.gradient_color.colors = vec![
                codec::entities::hatch::GradientColorEntry {
                    value: 0.0,
                    color: to_color(c0),
                    ..Default::default()
                },
                codec::entities::hatch::GradientColorEntry {
                    value: 1.0,
                    color: to_color(c1),
                    ..Default::default()
                },
            ];
        }
        // `add_entity` already builds the render model from the DXF entity via
        // `hatch_model_from_dxf` and inserts it with a correct `world_origin`
        // (AABB-centred) for the relative-to-eye fill. The command-built `model`
        // carries `world_origin: [0, 0]`, which after the world_offset removal
        // leaves the fill mis-placed and effectively invisible until a later
        // edit rebuilds it from the DXF — so keep the seed, don't overwrite it.
        if let Some(origin) = model.pattern_origin {
            dxf.record_pattern_origin(Vector2::new(origin[0], origin[1]));
        }
        let mut entity = EntityType::Hatch(Box::new(dxf));

        if let Some(layer) = layer {
            entity.as_entity_mut().set_layer(layer.to_string());
        }
        if let Some((color, transparency)) = entity_style {
            entity.common_mut().color = color;
            entity.common_mut().transparency = transparency;
        } else {
            entity.common_mut().transparency = self.document.current_entity_transparency();
        }

        self.add_entity(entity)
    }

    pub fn clear(&mut self) {
        self.document.record_all_entities_for_transaction();
        self.document = Document::new();
        self.replace_selection(HashSet::default());
        self.preview_wires = vec![];
        self.preview_text = vec![];
        self.current_layout = "Model".to_string();
        self.hatches = HashMap::default();
        self.associative_hatch_source_cache.borrow_mut().take();
        self.images = HashMap::default();
        self.meshes = HashMap::default();
        self.block_meshes = HashMap::default();
        self.solid_models = HashMap::default();
        *self.camera.borrow_mut() = Camera::default();
        self.camera_generation += 1;
        // A brand-new/replaced document has none of these yet — without
        // resetting them, a "new"/"open" (e.g. via automation, which reuses
        // this same reset) would leak the previous document's constraints
        // and named parameters into the fresh one.
        self.parametric_constraints.clear();
        self.named_parameters = crate::scene::named_parameters::ParameterTable::new();
        self.bump_constraints_epoch();
        self.bump_geometry();
        self.bump_layout_epoch();
        self.bump_scale_epoch();
    }

    /// Reset to the drawing File → New produces. Every path that starts a
    /// fresh drawing (the New tab, CLEAR, the automation `new` op) goes
    /// through here so they all agree on what a blank drawing contains.
    pub fn reset_to_new_drawing(&mut self) {
        self.clear();
        self.populate_new_drawing_defaults();
    }

    /// What a blank document gets on top of the codec's defaults: the
    /// standard linetypes and a compatible page setup on every paper layout.
    pub fn populate_new_drawing_defaults(&mut self) {
        crate::io::linetypes::populate_document(&mut self.document);
        // The visual styles of the reference's new drawing.
        crate::io::visual_styles::populate_document(&mut self.document);
        // A new drawing is metric, as the reference's new drawing: metre
        // insertion units, metric measurement, and the ISO-25 dimension style
        // current. Standard stays available for imperial work.
        self.document.header.measurement = 1;
        self.document.header.insertion_units = 6;
        // Its solids are drawn with eight isolines.
        self.document.header.isolines = 8;
        let iso = crate::scene::creation_style::ensure_iso_dim_style(&mut self.document);
        self.document.header.current_dimstyle_name = "ISO-25".to_string();
        crate::scene::creation_style::sync_header_dim_vars(&mut self.document);
        // The name alone leaves the header pointing at the previous style, and
        // a DWG writes the current style by handle only.
        self.document.header.current_dimstyle_handle = iso;
        // The annotation multiplier depends on those units; Scene::new()
        // left it at the unitless 1, which drew annotative objects 1000x too
        // large until the first CANNOSCALE change shrank them. (#1674)
        self.sync_annotation_scale_from_header();
        for obj in self.document.objects.values_mut() {
            match obj {
                codec::objects::ObjectType::Layout(l) if l.name != "Model" => {
                    crate::scene::apply_default_page_setup(l, "");
                    self.unopened_layouts.insert(l.name.clone());
                }
                // The codec's Standard multileader style is sized in inches;
                // the metric template's is in millimetres. Left as-is, a new
                // leader's arrow and landing were a fraction of a millimetre,
                // invisible on a sheet. (#2027)
                codec::objects::ObjectType::MultiLeaderStyle(s)
                    if s.name.eq_ignore_ascii_case("Standard") =>
                {
                    s.text_height = 4.0;
                    s.arrowhead_size = 4.0;
                    s.landing_gap = 2.0;
                    s.landing_distance = 8.0;
                    s.break_gap_size = 3.75;
                }
                // Likewise the Standard table style: the metric template's
                // cells are 4.5 high (title 6) with 1.5 margins, not 0.18.
                codec::objects::ObjectType::TableStyle(s)
                    if s.name.eq_ignore_ascii_case("Standard") =>
                {
                    s.set_margins(1.5);
                    s.data_row_style.text_height = 4.5;
                    s.header_row_style.text_height = 4.5;
                    s.title_row_style.text_height = 6.0;
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod image_definition_graph_tests {
    use super::*;
    use codec::entities::RasterImage;
    use codec::objects::ObjectType;

    fn image(path: &str) -> EntityType {
        EntityType::RasterImage(RasterImage::with_size(
            path,
            codec::types::Vector3::ZERO,
            4.0,
            4.0,
            40.0,
            40.0,
        ))
    }

    fn raster(scene: &Scene, handle: Handle) -> &RasterImage {
        let Some(EntityType::RasterImage(image)) = scene.document.get_entity(handle) else {
            panic!("expected image {handle:?}");
        };
        image
    }

    fn assert_reactor(scene: &Scene, handle: Handle, definition: Handle) -> Handle {
        let image = raster(scene, handle);
        assert_eq!(image.definition_handle, Some(definition));
        let reactor_handle = image.definition_reactor_handle.expect("image reactor link");
        let Some(ObjectType::ImageDefinitionReactor(reactor)) =
            scene.document.objects.get(&reactor_handle)
        else {
            panic!("missing image reactor");
        };
        assert_eq!(reactor.handle, reactor_handle);
        assert_eq!(reactor.owner, handle);
        assert_eq!(reactor.image_handle, handle);
        let Some(ObjectType::ImageDefinition(def)) = scene.document.objects.get(&definition) else {
            panic!("missing image definition");
        };
        assert!(def.reactors.contains(&reactor_handle));
        reactor_handle
    }

    #[test]
    fn attach_same_file_shares_registered_definition_and_owns_reactors() {
        let mut scene = Scene::new();
        let first = scene.add_entity(image("shared.png"));
        let second = scene.add_entity(image("shared.png"));
        let definitions: Vec<_> = scene
            .document
            .objects
            .iter()
            .filter_map(|(h, o)| {
                if let ObjectType::ImageDefinition(def) = o {
                    Some((*h, def))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(definitions.len(), 1, "one definition per file");
        let (definition, def) = definitions[0];
        let root = scene.document.header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(root)) = scene.document.objects.get(&root) else {
            panic!("missing named object dictionary");
        };
        let dictionary = root.get("ACAD_IMAGE_DICT").expect("image dictionary entry");
        let Some(ObjectType::Dictionary(entries)) = scene.document.objects.get(&dictionary) else {
            panic!("missing image dictionary");
        };
        assert_eq!(
            entries.owner,
            scene.document.header.named_objects_dict_handle
        );
        assert_eq!(entries.get("shared"), Some(definition));
        assert_eq!(def.owner, dictionary);
        assert!(def.reactors.contains(&dictionary));
        assert_ne!(
            assert_reactor(&scene, first, definition),
            assert_reactor(&scene, second, definition)
        );
        assert_eq!(def.reactors.len(), 3);
    }

    #[test]
    fn copy_shares_definition_with_fresh_owned_reactor() {
        let mut scene = Scene::new();
        let source = scene.add_entity(image("shared.png"));
        scene.begin_undo_recording();
        let copies = scene.copy_entities(
            &[source],
            &crate::command::EntityTransform::Translate(glam::DVec3::new(20.0, 0.0, 0.0)),
        );
        assert_eq!(copies.len(), 1);
        let copy = copies[0];
        assert_ne!(
            raster(&scene, copy).definition_reactor_handle,
            raster(&scene, source).definition_reactor_handle
        );
        let definition = raster(&scene, source)
            .definition_handle
            .expect("source definition");
        let source_reactor = assert_reactor(&scene, source, definition);
        let copy_reactor = assert_reactor(&scene, copy, definition);
        let recording = scene.take_undo_recording().expect("undo recording");
        assert!(!recording.is_poisoned());
        let (_, objects, _, _, _) = recording.into_recorded_images();
        assert!(objects
            .iter()
            .any(|(h, before)| *h == copy_reactor && before.is_none()));
        let before = objects
            .iter()
            .find(|(h, _)| *h == definition)
            .expect("shared definition before-image");
        let Some(ObjectType::ImageDefinition(def)) = &before.1 else {
            panic!("missing definition before-image");
        };
        assert!(def.reactors.contains(&source_reactor));
        assert!(!def.reactors.contains(&copy_reactor));
    }

    #[test]
    fn erase_image_copy_removes_only_its_owned_reactor() {
        let mut scene = Scene::new();
        let source = scene.add_entity(image("shared.png"));
        let copy = scene.copy_entities(
            &[source],
            &crate::command::EntityTransform::Translate(glam::DVec3::new(20.0, 0.0, 0.0)),
        )[0];
        let definition = raster(&scene, source).definition_handle.unwrap();
        let source_reactor = assert_reactor(&scene, source, definition);
        let copy_reactor = assert_reactor(&scene, copy, definition);

        scene.erase_entities(&[copy]);

        assert!(scene.document.get_entity(copy).is_none());
        assert!(!scene.document.objects.contains_key(&copy_reactor));
        assert_eq!(assert_reactor(&scene, source, definition), source_reactor);
        let Some(ObjectType::ImageDefinition(def)) = scene.document.objects.get(&definition) else {
            panic!("missing image definition");
        };
        assert!(!def.reactors.contains(&copy_reactor));
        assert!(def.reactors.contains(&def.owner));

        // Removing the final image keeps the definition and dictionary entry.
        let dictionary = def.owner;
        let source_image = scene.document.get_entity_arc(source).unwrap();
        scene.erase_entities(&[source]);
        assert!(!scene.document.objects.contains_key(&source_reactor));
        let Some(ObjectType::ImageDefinition(def)) = scene.document.objects.get(&definition) else {
            panic!("missing retained image definition");
        };
        assert_eq!(def.reactors, vec![dictionary]);
        let Some(ObjectType::Dictionary(entries)) = scene.document.objects.get(&dictionary) else {
            panic!("missing retained image dictionary");
        };
        assert_eq!(entries.get("shared"), Some(definition));

        assert_eq!(
            scene.restore_erased_entities(vec![source_image]),
            vec![source]
        );
        let restored_reactor = assert_reactor(&scene, source, definition);
        assert_ne!(restored_reactor, source_reactor);
    }

    #[test]
    fn erase_image_copy_undo_restores_reactor_exactly_once() {
        let mut scene = Scene::new();
        let source = scene.add_entity(image("shared.png"));
        let copy = scene.copy_entities(
            &[source],
            &crate::command::EntityTransform::Translate(glam::DVec3::new(20.0, 0.0, 0.0)),
        )[0];
        let definition = raster(&scene, source).definition_handle.unwrap();
        let source_reactor = assert_reactor(&scene, source, definition);
        let copy_reactor = assert_reactor(&scene, copy, definition);
        scene.begin_undo_recording();
        scene.erase_entities(&[copy]);
        let recording = scene.take_undo_recording().expect("erase undo recording");
        assert!(!recording.is_poisoned());
        let (entities, objects, _, _, _) = recording.into_recorded_images();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].0, copy);
        assert_eq!(
            objects.len(),
            2,
            "definition and owned reactor before-images"
        );
        assert!(objects.iter().any(|(h, before)| *h == copy_reactor
            && matches!(
                before, Some(ObjectType::ImageDefinitionReactor(r)) if r.owner == copy
            )));
        assert!(objects.iter().any(|(h, before)| *h == definition && matches!(
            before, Some(ObjectType::ImageDefinition(def)) if def.reactors.contains(&copy_reactor)
        )));
        let entities: Vec<_> = entities
            .into_iter()
            .map(|(h, before)| (h, before, scene.document.get_entity_arc(h)))
            .collect();
        let objects: Vec<_> = objects
            .into_iter()
            .map(|(h, before)| (h, before, scene.document.objects.get(&h).cloned()))
            .collect();

        for undo in [true, false, true] {
            // Match history replay: object images first, then entity images.
            for (h, before, after) in &objects {
                if let Some(object) = if undo { before } else { after } {
                    scene.document.objects.insert(*h, object.clone());
                } else {
                    scene.document.objects.remove(h);
                }
            }
            scene.apply_entity_delta(&entities, undo);
            assert_eq!(assert_reactor(&scene, source, definition), source_reactor);
            if undo {
                assert_eq!(assert_reactor(&scene, copy, definition), copy_reactor);
                let mut restored = scene.document.get_entity(copy).unwrap().clone();
                let object_count = scene.document.objects.len();
                scene.ensure_image_definition(&mut restored);
                scene.ensure_image_definition(&mut restored);
                assert_eq!(scene.document.objects.len(), object_count);
                let EntityType::RasterImage(restored) = restored else {
                    panic!("expected restored image");
                };
                assert_eq!(restored.definition_reactor_handle, Some(copy_reactor));
            } else {
                assert!(scene.document.get_entity(copy).is_none());
                assert!(!scene.document.objects.contains_key(&copy_reactor));
            }
            let Some(ObjectType::ImageDefinition(def)) = scene.document.objects.get(&definition)
            else {
                panic!("missing image definition");
            };
            assert_eq!(
                def.reactors.iter().filter(|h| **h == copy_reactor).count(),
                usize::from(undo)
            );
        }
    }
}
