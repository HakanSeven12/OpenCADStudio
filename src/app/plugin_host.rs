// HostSession — plugin-facing API implemented inside `app` (private field access).

use std::any::Any;

use acadrust::tables::AppId;
use acadrust::xdata::ExtendedDataRecord;
use ocs_plugin_api::host::{CadDocument, EntityType, Handle, HostApi, HostSettingValue};
use ocs_plugin_api::shm::{DocumentSnapshotStore, DocumentViewData};

use super::OpenCADStudio;
#[cfg(not(target_arch = "wasm32"))]
use crate::plugin::v4_support;

/// Session adapter: one active document tab, command line, undo.
pub(crate) struct HostSession<'a> {
    app: &'a mut OpenCADStudio,
    tab: usize,
    doc_store: Option<DocumentSnapshotStore<DocumentViewData>>,
}

impl<'a> HostSession<'a> {
    pub(crate) fn new(app: &'a mut OpenCADStudio, tab: usize) -> Self {
        Self {
            app,
            tab,
            doc_store: None,
        }
    }

    pub fn tab_index(&self) -> usize {
        self.tab
    }

    pub fn tab_id(&self) -> u64 {
        self.app.tabs[self.tab].id
    }

    pub fn document_path(&self, tab_id: u64) -> Option<std::path::PathBuf> {
        self.app
            .tabs
            .iter()
            .find(|t| t.id == tab_id)
            .and_then(|t| t.current_path.clone())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn document_view_v4(
        &mut self,
        tab_id: u64,
    ) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        if tab_id != self.tab_id() {
            return None;
        }
        v4_support::open_document_view_v4(tab_id, self.document())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn close_document_view_v4(&mut self, tab_id: u64) {
        if tab_id == self.tab_id() {
            v4_support::close_document_view_v4(tab_id);
        }
    }

    pub fn document(&self) -> &CadDocument {
        &self.app.tabs[self.tab].scene.document
    }

    pub fn document_mut(&mut self) -> &mut CadDocument {
        &mut self.app.tabs[self.tab].scene.document
    }

    fn nested_attribute(&self, handle: Handle) -> Option<EntityType> {
        self.document().entities().find_map(|entity| {
            let EntityType::Insert(insert) = entity else {
                return None;
            };
            insert
                .attributes
                .iter()
                .find(|attribute| attribute.common.handle == handle)
                .cloned()
                .map(EntityType::AttributeEntity)
        })
    }

    fn host_model_entity(&self, handle: Handle) -> Option<EntityType> {
        self.document()
            .get_entity(handle)
            .cloned()
            .or_else(|| self.nested_attribute(handle))
    }

    fn replace_nested_attribute(&mut self, attribute: acadrust::entities::AttributeEntity) -> bool {
        let owner = attribute.common.owner_handle;
        let handle = attribute.common.handle;
        let Some(EntityType::Insert(mut insert)) = self.document().get_entity(owner).cloned()
        else {
            return false;
        };
        let Some(slot) = insert
            .attributes
            .iter_mut()
            .find(|candidate| candidate.common.handle == handle)
        else {
            return false;
        };
        *slot = attribute;
        self.app.tabs[self.tab]
            .scene
            .update_entity(EntityType::Insert(insert))
    }

    pub fn document_view(&mut self) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        if self.doc_store.is_none() {
            let mut store =
                DocumentSnapshotStore::<DocumentViewData>::new(self.tab as u64, 8 * 1024 * 1024)
                    .ok()?;
            store.publish(&self.document().into()).ok()?;
            self.doc_store = Some(store);
        }
        let store = self.doc_store.as_ref()?;
        Some(ocs_plugin_api::shm::DocumentViewInfo {
            path: store.path().to_string_lossy().to_string(),
            version: store.version(),
        })
    }

    fn publish_document_view(&mut self) {
        let doc = &self.app.tabs[self.tab].scene.document;
        // Only publish views that have been opened by a consumer. V3 is lazily
        // created by document_view(); V4 is tracked per-tab by the V4 manager.
        if let Some(store) = self.doc_store.as_mut() {
            if let Err(e) = store.publish(&doc.into()) {
                eprintln!(
                    "[host] failed to publish document view for tab {}: {e}",
                    self.tab
                );
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let key = (self.tab_id(), self.app.tabs[self.tab].scene.geometry_epoch);
            if self.app.last_plugin_document != Some(key) {
                v4_support::publish_drawing_changed(key.0, key.1);
            }
            v4_support::publish_document_view_v4(self.tab_id(), doc);
            self.app.last_plugin_document =
                Some((self.tab_id(), self.app.tabs[self.tab].scene.geometry_epoch));
        }
    }

    /// Recompute host-derived state for entities a script created or edited.
    fn normalize_scripted_entity(
        &self,
        old: Option<&EntityType>,
        entity: &mut EntityType,
    ) -> Result<(), String> {
        if let EntityType::Dimension(dimension) = entity {
            crate::entities::dimension::normalize_scripted_dimension(dimension);
        }
        if let EntityType::Viewport(viewport) = entity {
            // A viewport needs an id unique in its layout and at least 2 (id 1
            // is the never-drawn sheet viewport, and a lone id 0 can be taken for
            // it), and its scale follows its paper height and model view height;
            // both are what the MVIEW command sets.
            if old.is_none() {
                let owner = viewport.common.owner_handle;
                let highest = self
                    .document()
                    .entities()
                    .filter_map(|e| match e {
                        EntityType::Viewport(other) if other.common.owner_handle == owner => Some(other.id),
                        _ => None,
                    })
                    .max()
                    .unwrap_or(1);
                viewport.id = (highest + 1).max(2);
            }
            viewport.custom_scale = viewport.height / viewport.view_height;
        }
        if let EntityType::SectionSymbol(symbol) = entity {
            // `points` is the canonical geometry: the point counts follow it
            // (they equal the point count in every verified export) and the
            // end, tick and label projections are refreshed from it. A symbol
            // whose points did not change is left exactly as it was read.
            let unchanged = matches!(old, Some(EntityType::SectionSymbol(previous)) if previous.points == symbol.points);
            if !unchanged {
                let count = i32::try_from(symbol.points.len()).unwrap_or(i32::MAX);
                symbol.raw_point_count_90 = count;
                symbol.raw_point_record_count = count;
                symbol.sync_display_fields();
            }
        }
        if let EntityType::Spline(spline) = entity {
            // The DXF writer emits weights only for a rational spline, so the
            // flag must follow the weights a script sets.
            spline.flags.rational = !spline.weights.is_empty();
        }
        if let EntityType::Helix(new) = entity {
            let old = match old {
                Some(EntityType::Helix(old)) => Some(old),
                _ => None,
            };
            crate::entities::helix::normalize_scripted_helix(old, new)?;
        }
        if let EntityType::Table(new) = entity {
            let old = match old {
                Some(EntityType::Table(old)) => Some(old),
                _ => None,
            };
            crate::entities::table::normalize_scripted_table(old, new);
        }
        if let EntityType::MLine(new) = entity {
            let old = match old {
                Some(EntityType::MLine(old)) => Some(old),
                _ => None,
            };
            crate::entities::mline::normalize_scripted_mline(old, new, self.document())?;
        }
        Ok(())
    }

    /// The `ACAD_IMAGE_DICT` dictionary under the named-objects root, created
    /// when the drawing has none. `None` when the drawing has no root dictionary.
    fn ensure_image_dictionary(&mut self) -> Option<Handle> {
        use acadrust::objects::{Dictionary, ObjectType};
        let root = self.document().header.named_objects_dict_handle;
        let Some(ObjectType::Dictionary(root_dictionary)) = self.document().objects.get(&root) else {
            return None;
        };
        if let Some(existing) = root_dictionary.get("ACAD_IMAGE_DICT") {
            if matches!(self.document().objects.get(&existing), Some(ObjectType::Dictionary(_))) {
                return Some(existing);
            }
        }
        let handle = self.document_mut().allocate_handle();
        let mut dictionary = Dictionary::new();
        dictionary.handle = handle;
        dictionary.owner = root;
        self.document_mut().objects.insert(handle, ObjectType::Dictionary(dictionary));
        if let Some(ObjectType::Dictionary(root_dictionary)) = self.document_mut().objects.get_mut(&root) {
            root_dictionary.add_entry("ACAD_IMAGE_DICT", handle);
        }
        Some(handle)
    }

    /// A scripted RasterImage names a file; the host creates the image
    /// definition object it needs (pixel size read from the file) and links it.
    fn prepare_scripted_raster_image(&mut self, entity: &mut EntityType) -> Result<(), String> {
        let EntityType::RasterImage(image) = entity else {
            return Ok(());
        };
        if image.definition_handle.is_some_and(|handle| !handle.is_null()) {
            return Ok(());
        }
        let (width, height) = image::image_dimensions(&image.file_path)
            .map_err(|error| format!("cannot read image {:?}: {error}", image.file_path))?;
        // One definition per file, as AutoCAD keeps it: reuse an existing one.
        let existing = self.document().objects.iter().find_map(|(handle, object)| match object {
            acadrust::objects::ObjectType::ImageDefinition(definition)
                if definition.file_name == image.file_path => Some(*handle),
            _ => None,
        });
        let handle = match existing {
            Some(handle) => handle,
            None => {
                let handle = self.document_mut().allocate_handle();
                let mut definition = acadrust::objects::ImageDefinition::with_dimensions(
                    image.file_path.clone(),
                    width,
                    height,
                );
                definition.handle = handle;
                definition.is_loaded = true;
                // Register it in ACAD_IMAGE_DICT under the file's name, and let
                // that dictionary own it, like an image AutoCAD attached.
                let stem = std::path::Path::new(&image.file_path)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.is_empty())
                    .unwrap_or("IMAGE")
                    .to_owned();
                if let Some(dictionary) = self.ensure_image_dictionary() {
                    definition.owner = dictionary;
                    if let Some(acadrust::objects::ObjectType::Dictionary(entries)) =
                        self.document_mut().objects.get_mut(&dictionary)
                    {
                        let mut key = stem.clone();
                        let mut suffix = 1;
                        while entries.get(&key).is_some() {
                            suffix += 1;
                            key = format!("{stem}_{suffix}");
                        }
                        entries.add_entry(key, handle);
                    }
                }
                self.document_mut()
                    .objects
                    .insert(handle, acadrust::objects::ObjectType::ImageDefinition(definition));
                handle
            }
        };
        image.definition_handle = Some(handle);
        // `size` is the image's pixel size, and an untouched clip boundary
        // covers the whole image, so both follow the file.
        let untouched_clip = image.clip_boundary
            == acadrust::entities::ClipBoundary::full_image(image.size.x, image.size.y);
        image.size = acadrust::types::Vector2::new(f64::from(width), f64::from(height));
        if untouched_clip {
            image.clip_boundary = acadrust::entities::ClipBoundary::full_image(
                f64::from(width),
                f64::from(height),
            );
        }
        Ok(())
    }

    pub fn add_entity(&mut self, mut entity: EntityType) -> Handle {
        if self.prepare_scripted_raster_image(&mut entity).is_err() {
            return Handle::NULL;
        }
        if self.normalize_scripted_entity(None, &mut entity).is_err() {
            return Handle::NULL;
        }
        if let EntityType::AttributeEntity(mut attribute) = entity {
            let owner = attribute.common.owner_handle;
            let Some(EntityType::Insert(mut insert)) = self.document().get_entity(owner).cloned()
            else {
                return Handle::NULL;
            };
            if attribute.common.handle.is_null() {
                attribute.common.handle = self.document_mut().allocate_handle();
            }
            let handle = attribute.common.handle;
            if insert
                .attributes
                .iter()
                .any(|existing| existing.common.handle == handle)
            {
                return Handle::NULL;
            }
            insert.attributes.push(attribute);
            if !self.app.tabs[self.tab]
                .scene
                .update_entity(EntityType::Insert(insert))
            {
                return Handle::NULL;
            }
            self.publish_document_view();
            return handle;
        }
        let handle = self.app.tabs[self.tab].scene.add_entity(entity);
        self.publish_document_view();
        handle
    }

    pub fn add_entities(&mut self, mut entities: Vec<EntityType>) -> Vec<Handle> {
        for entity in &mut entities {
            let _ = self.prepare_scripted_raster_image(entity);
            // Creation input was validated by the caller; a degenerate MLine
            // simply keeps its supplied (unnormalized) vertices.
            let _ = self.normalize_scripted_entity(None, entity);
        }
        let handles = self.app.tabs[self.tab].scene.add_entities(entities);
        self.publish_document_view();
        handles
    }

    pub fn bump_geometry(&mut self) {
        self.app.tabs[self.tab].scene.bump_geometry();
    }

    /// Replace the entity carrying `entity`'s handle in place, refreshing the
    /// scene's derived caches. Returns `false` when no entity has that handle.
    pub fn update_entity(&mut self, mut entity: EntityType) -> bool {
        let old = self.document().get_entity(entity.common().handle).cloned();
        if self.normalize_scripted_entity(old.as_ref(), &mut entity).is_err() {
            return false;
        }
        let ok = match entity {
            EntityType::AttributeEntity(attribute) => self.replace_nested_attribute(attribute),
            entity => self.app.tabs[self.tab].scene.update_entity(entity),
        };
        if ok {
            self.publish_document_view();
        }
        ok
    }

    pub fn update_entities_transaction(
        &mut self,
        label: &str,
        mut entities: Vec<EntityType>,
    ) -> Result<(), String> {
        use std::collections::HashSet;
        if label.trim().is_empty() {
            return Err("transaction label is empty".into());
        }
        if entities.is_empty() {
            return Ok(());
        }
        let mut seen = HashSet::new();
        for entity in &mut entities {
            let handle = entity.common().handle;
            if handle.is_null() || !seen.insert(handle) {
                return Err(format!("null or duplicate entity handle: {handle:?}"));
            }
            let Some(existing) = self.host_model_entity(handle) else {
                return Err(format!("entity {handle:?} does not exist"));
            };
            if std::mem::discriminant(&existing) != std::mem::discriminant(entity) {
                return Err(format!("entity {handle:?} changes kind"));
            }
            if existing.common().owner_handle != entity.common().owner_handle {
                return Err(format!("entity {handle:?} changes owner"));
            }
            ocs_plugin_api::entity_coverage::bind_canvas_entity_references(self.document(), entity)
                .map_err(|error| format!("entity {handle:?}: {error}"))?;
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&existing, entity)
                .map_err(|error| format!("entity {handle:?}: {error}"))?;
            self.normalize_scripted_entity(Some(&existing), entity)
                .map_err(|error| format!("entity {handle:?}: {error}"))?;
            let lock_handle = if matches!(entity, EntityType::AttributeEntity(_)) {
                entity.common().owner_handle
            } else {
                handle
            };
            if self.app.tabs[self.tab].scene.is_layer_locked(lock_handle) {
                return Err(format!("entity {handle:?} is on a locked layer"));
            }
        }
        self.push_undo(label);
        for entity in entities {
            // Validation above makes this infallible while the session owns
            // the document exclusively.
            let updated = match entity {
                EntityType::AttributeEntity(attribute) => self.replace_nested_attribute(attribute),
                entity => self.app.tabs[self.tab].scene.update_entity(entity),
            };
            assert!(updated);
        }
        self.set_dirty();
        self.publish_document_view();
        Ok(())
    }

    pub fn selection(&self) -> Vec<Handle> {
        self.app.tabs[self.tab].scene.selected_handles_in_order()
    }

    pub fn set_selection(&mut self, handles: &[Handle]) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for handle in handles {
            if !seen.insert(*handle) {
                return Err(format!("duplicate selected entity: {handle:?}"));
            }
            if self.document().get_entity(*handle).is_none() {
                return Err(format!("entity {handle:?} does not exist"));
            }
        }
        if self.selection() == handles {
            return Ok(());
        }
        let scene = &mut self.app.tabs[self.tab].scene;
        scene.replace_selection_exact(handles);
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.tab == self.app.active_tab {
                self.app.last_plugin_selection = Some((
                    self.tab_id(),
                    self.app.tabs[self.tab].scene.selection_fingerprint(),
                ));
            }
            crate::plugin::v4_support::publish_selection_changed_v4(
                self.tab_id(),
                self.selection(),
            );
        }
        Ok(())
    }

    /// Delete the entity with `handle`, keeping the scene's render caches in
    /// sync. Returns `false` when the entity is absent or on a locked layer
    /// (which `erase_entities` refuses to remove).
    pub fn remove_entity(&mut self, handle: Handle) -> bool {
        if self.document().get_entity(handle).is_none() {
            let Some(EntityType::AttributeEntity(attribute)) = self.nested_attribute(handle) else {
            return false;
            };
            let owner = attribute.common.owner_handle;
            let Some(EntityType::Insert(mut insert)) = self.document().get_entity(owner).cloned()
            else {
                return false;
            };
            insert
                .attributes
                .retain(|candidate| candidate.common.handle != handle);
            let removed = self.app.tabs[self.tab]
                .scene
                .update_entity(EntityType::Insert(insert));
            if removed {
                self.publish_document_view();
            }
            return removed;
        }
        let owner = self.document().get_entity(handle).map(|entity| entity.common().owner_handle);
        self.app.tabs[self.tab].scene.erase_entities(&[handle]);
        let removed = self.document().get_entity(handle).is_none();
        if removed {
            // The document keeps a block record's member list separately from the
            // entity store, and removing an entity leaves its handle listed; drop
            // it so a definition never lists a member that no longer exists.
            if let Some(owner) = owner {
                if let Some(record) = self.document_mut().block_records.iter_mut().find(|record| record.handle == owner) {
                    record.entity_handles.retain(|member| *member != handle);
                }
            }
            self.publish_document_view();
        }
        removed
    }

    // ── XDATA convenience ──────────────────────────────────────────────────
    // Plugins persist domain data as XDATA on plain entities so it round-trips
    // through DWG/DXF. These wrap the `acadrust::xdata` API keyed by entity
    // handle and keep the APPID table in sync.

    /// Read the XDATA record for `app_name` attached to entity `handle`, if any.
    pub fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord> {
        self.document()
            .get_entity(handle)?
            .common()
            .extended_data
            .get_record(app_name)
    }

    /// Attach `record` to entity `handle`, replacing any existing record for the
    /// same application. Registers the application in the APPID table when
    /// missing so the file stays valid for other CAD apps. Returns `false` when
    /// the entity does not exist.
    pub fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool {
        if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
            return false;
        }
        let app = record.application_name.clone();
        self.ensure_app_id(&app);
        let app_handle = self.document().app_ids.get(&app).map(|a| a.handle.value());
        let Some(entity) = self.document_mut().get_entity_mut(handle) else {
            return false;
        };
        let xd = &mut entity.common_mut().extended_data;
        // Drop any existing record for this app, then append the new one.
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| r.application_name != app)
            .cloned()
            .collect();
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        xd.add_record(record);
        // Drop stale verbatim EED for this app so the fresh record — not the
        // pre-edit bytes captured on a prior read — wins on the next save.
        // Otherwise a plugin's edit made after a save/reopen (which registered
        // the app in `raw_dwg_eed`) would not persist.
        if let Some(ah) = app_handle {
            xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
        }
        self.bump_geometry();
        self.set_dirty();
        self.publish_document_view();
        true
    }

    /// Remove the XDATA record for `app_name` from entity `handle`. Returns
    /// `true` when a record was actually removed.
    pub fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool {
        if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
            return false;
        }
        let app_handle = self
            .document()
            .app_ids
            .get(app_name)
            .map(|a| a.handle.value());
        let Some(entity) = self.document_mut().get_entity_mut(handle) else {
            return false;
        };
        let xd = &mut entity.common_mut().extended_data;
        let kept: Vec<_> = xd
            .records()
            .iter()
            .filter(|r| r.application_name != app_name)
            .cloned()
            .collect();
        let removed_record = kept.len() != xd.records().len();
        // Also drop verbatim EED for this app so the removal persists across a
        // save (a record read back from DWG lives in `raw_dwg_eed`).
        let removed_raw = app_handle
            .map(|ah| xd.raw_dwg_eed.iter().any(|(a, _)| *a == ah))
            .unwrap_or(false);
        if !removed_record && !removed_raw {
            return false;
        }
        xd.clear();
        for r in kept {
            xd.add_record(r);
        }
        if let Some(ah) = app_handle {
            xd.raw_dwg_eed.retain(|(a, _)| *a != ah);
        }
        self.bump_geometry();
        self.set_dirty();
        self.publish_document_view();
        true
    }

    /// Register `name` in the APPID table if it is not already present, so XDATA
    /// written under it survives a DWG/DXF round-trip. The entry is given a real
    /// handle — a null-handle APPID is written as handle 0, which the DWG EED
    /// reference then can't resolve, so the XDATA would be dropped on reopen.
    fn ensure_app_id(&mut self, name: &str) {
        let doc = self.document_mut();
        if !doc.app_ids.contains(name) {
            let mut app = AppId::new(name);
            app.handle = doc.allocate_handle();
            let _ = doc.app_ids.add(app);
        }
    }

    /// Kernel-backed solid create or transform. Every result is verified by
    /// `solid_to_sat` (the exported payload must lift back with no loss) and an
    /// existing payload is only transformed when it lifts losslessly, so a
    /// failure never changes the document.
    pub fn solid_operation(
        &mut self,
        operation: ocs_plugin_api::host::SolidOperation,
    ) -> Result<Handle, String> {
        use crate::scene::model::solid_model as model;
        use ocs_plugin_api::host::{SolidOperation, SolidPrimitive};
        let positive = |name: &str, value: f64| {
            if value.is_finite() && value > 0.0 {
                Ok(())
            } else {
                Err(format!("{name} must be finite and greater than zero"))
            }
        };
        let finite3 = |name: &str, value: [f64; 3]| {
            if value.iter().all(|v| v.is_finite()) {
                Ok(())
            } else {
                Err(format!("{name} must be finite"))
            }
        };
        match operation {
            SolidOperation::Create { primitive, layer } => {
                let body = match primitive {
                    SolidPrimitive::Box { center, size } => {
                        finite3("box center", center)?;
                        for (name, v) in [("length", size[0]), ("width", size[1]), ("height", size[2])] {
                            positive(&format!("box {name}"), v)?;
                        }
                        model::box_solid(center, size[0], size[1], size[2])
                    }
                    SolidPrimitive::Wedge { origin, size } => {
                        finite3("wedge origin", origin)?;
                        for (name, v) in [("length", size[0]), ("width", size[1]), ("height", size[2])] {
                            positive(&format!("wedge {name}"), v)?;
                        }
                        model::wedge_solid(origin, size[0], size[1], size[2])
                    }
                    SolidPrimitive::Cylinder { center, radius, height } => {
                        finite3("cylinder center", center)?;
                        positive("cylinder radius", radius)?;
                        positive("cylinder height", height)?;
                        model::cylinder_solid(center, radius, height)
                    }
                    SolidPrimitive::Sphere { center, radius } => {
                        finite3("sphere center", center)?;
                        positive("sphere radius", radius)?;
                        model::sphere_solid(center, radius)
                    }
                    SolidPrimitive::Torus { center, major, minor } => {
                        finite3("torus center", center)?;
                        positive("torus major radius", major)?;
                        positive("torus minor radius", minor)?;
                        if minor >= major {
                            return Err("torus minor radius must be smaller than the major radius".into());
                        }
                        model::torus_solid(center, major, minor)
                    }
                    SolidPrimitive::Pyramid { center, radius, height, sides } => {
                        finite3("pyramid center", center)?;
                        positive("pyramid radius", radius)?;
                        positive("pyramid height", height)?;
                        if !(3..=1024).contains(&sides) {
                            return Err("pyramid sides must be between 3 and 1024".into());
                        }
                        model::pyramid_solid(center, radius, height, sides as usize)
                    }
                }
                .ok_or("the geometry kernel refused these dimensions")?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                    .ok_or("the solid could not be exported losslessly")?;
                let mut solid = acadrust::entities::Solid3D::new();
                solid.wires = model::edge_wires(&body);
                solid.set_sat_document(&sat);
                if let Some(layer) = layer {
                    if layer.trim().is_empty() {
                        return Err("layer name is empty".into());
                    }
                    solid.common.layer = layer;
                }
                self.push_undo("Create solid");
                let handle = self.add_entity(EntityType::Solid3D(solid));
                if handle.is_null() {
                    return Err("the solid could not be added".into());
                }
                Ok(handle)
            }
            SolidOperation::RegionFromProfile { source, layer, delete_source } => {
                let (entity, plane, loops, closed, layer) = self.profile_source(source, layer, delete_source)?;
                if matches!(entity, EntityType::Region(_)) {
                    return Err("the source is already a region".into());
                }
                if !closed {
                    return Err("the profile must be closed".into());
                }
                let body = cadkernel::brep::planar_region(plane, &loops)
                    .ok_or("the geometry kernel could not build a region from this profile")?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                    .ok_or("the region could not be exported losslessly")?;
                let mut region = acadrust::entities::Region::new();
                region.point_of_reference =
                    acadrust::types::Vector3::new(plane.origin[0], plane.origin[1], plane.origin[2]);
                region.wires = model::edge_wires(&body);
                region.set_sat_document(&sat);
                region.common.layer = layer;
                self.commit_profile_result(EntityType::Region(region), source, delete_source, "Create region")
            }
            SolidOperation::SurfaceFromProfile { source, layer, delete_source } => {
                let (entity, plane, loops, closed, layer) = self.profile_source(source, layer, delete_source)?;
                if matches!(entity, EntityType::Region(_)) {
                    return Err("the source is already a region".into());
                }
                if !closed {
                    return Err("the profile must be closed".into());
                }
                let body = cadkernel::brep::planar_region(plane, &loops)
                    .ok_or("the geometry kernel could not build a surface from this profile")?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                    .ok_or("the surface could not be exported losslessly")?;
                let mut surface = acadrust::entities::Surface::new(acadrust::entities::SurfaceKind::Plane);
                surface.point_of_reference =
                    acadrust::types::Vector3::new(plane.origin[0], plane.origin[1], plane.origin[2]);
                surface.wires = model::edge_wires(&body);
                surface.acis_data = acadrust::entities::AcisData::from_sat(&sat.to_sat_string());
                surface.common.layer = layer;
                self.commit_profile_result(EntityType::Surface(surface), source, delete_source, "Create surface")
            }
            SolidOperation::Extrude { source, direction, layer, delete_source } => {
                if direction.iter().any(|v| !v.is_finite()) {
                    return Err("extrusion direction must be finite".into());
                }
                if direction.iter().map(|v| v * v).sum::<f64>().sqrt() <= 1e-9 {
                    return Err("extrusion direction must be nonzero".into());
                }
                let (entity, _plane, _loops, closed, layer) = self.profile_source(source, layer, delete_source)?;
                if matches!(entity, EntityType::Region(_)) {
                    return Err("extrude a curve profile; a region cannot be extruded here".into());
                }
                let body = crate::scene::model::presspull_model::extrusion_body(&entity, direction)
                    .ok_or("the geometry kernel could not extrude this profile in that direction")?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
                    .ok_or("the extrusion could not be exported losslessly")?;
                let result = if closed {
                    let mut solid = acadrust::entities::Solid3D::new();
                    solid.wires = model::edge_wires(&body);
                    solid.set_sat_document(&sat);
                    solid.common.layer = layer;
                    EntityType::Solid3D(solid)
                } else {
                    // An open profile sweeps to a surface. The generic kind
                    // carries only the payload, as OCS does after a boolean.
                    let mut surface = acadrust::entities::Surface::new(acadrust::entities::SurfaceKind::Generic);
                    surface.wires = model::edge_wires(&body);
                    surface.acis_data = acadrust::entities::AcisData::from_sat(&sat.to_sat_string());
                    surface.common.layer = layer;
                    EntityType::Surface(surface)
                };
                self.commit_profile_result(result, source, delete_source, "Extrude profile")
            }
            SolidOperation::EmbedPicture { path, origin, width, layer } => {
                finite3("picture origin", origin)?;
                positive("picture width", width)?;
                if layer.as_ref().is_some_and(|name| name.trim().is_empty()) {
                    return Err("layer name is empty".into());
                }
                let picture = crate::io::ole_embed::EmbeddedImage::from_file(std::path::Path::new(&path))
                    .map_err(|error| format!("cannot read the picture {path:?}: {error}"))?;
                let (upper_left, lower_right) = crate::io::ole_embed::corners_from_placement(
                    acadrust::types::Vector3::new(origin[0], origin[1], origin[2]),
                    width,
                    picture.aspect(),
                );
                let mut frame = crate::io::ole_embed::build_embedded_ole(&picture, upper_left, lower_right);
                if let Some(layer) = layer {
                    frame.common.layer = layer;
                }
                self.push_undo("Embed picture");
                let handle = self.add_entity(EntityType::Ole2Frame(frame));
                if handle.is_null() {
                    return Err("the picture frame could not be added".into());
                }
                Ok(handle)
            }
            SolidOperation::Boolean { first, second, operation, layer, keep_operands } => {
                use ocs_plugin_api::host::SolidBoolean;
                if first == second {
                    return Err("a solid cannot be combined with itself".into());
                }
                if layer.as_ref().is_some_and(|name| name.trim().is_empty()) {
                    return Err("layer name is empty".into());
                }
                let operand = |host: &Self, handle: Handle, which: &str| -> Result<acadrust::entities::Solid3D, String> {
                    match host.document().get_entity(handle) {
                        Some(EntityType::Solid3D(solid)) => Ok(solid.clone()),
                        Some(_) => Err(format!("the {which} operand must be a Solid3D")),
                        None => Err(format!("entity {handle:?} does not exist")),
                    }
                };
                let a = operand(self, first, "first")?;
                let b = operand(self, second, "second")?;
                if !keep_operands {
                    for handle in [first, second] {
                        if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
                            return Err(format!("entity {handle:?} is on a locked layer"));
                        }
                    }
                }
                let lift = |solid: &acadrust::entities::Solid3D, which: &str| {
                    crate::scene::convert::solid3d_tess::kernel_acis_body(&solid.acis_data).ok_or_else(|| {
                        format!("the {which} solid's payload cannot be lifted losslessly, so nothing was changed")
                    })
                };
                let (body_a, body_b) = (lift(&a, "first")?, lift(&b, "second")?);
                let kind = match operation {
                    SolidBoolean::Union => model::Bool::Union,
                    SolidBoolean::Subtract => model::Bool::Subtract,
                    SolidBoolean::Intersect => model::Bool::Intersect,
                };
                let combined = model::boolean_result(kind, &body_a, &body_b).map_err(|snag| {
                    let reason = match snag {
                        cadkernel::brep::Snag::Coincident => "two faces lie on the same surface, which the kernel cannot resolve",
                        cadkernel::brep::Snag::CutRefused => "a face could not be cut along the intersection curve",
                        cadkernel::brep::Snag::NoClosedForm => "the surfaces meet along a curve the kernel has no closed form for",
                    };
                    format!("the geometry kernel refused this {operation:?} ({snag:?}): {reason}; nothing was changed")
                })?;
                let sat = crate::scene::convert::acis_export::solid_to_sat(&combined)
                    .ok_or("the result could not be exported losslessly, so nothing was changed")?;
                let mut solid = acadrust::entities::Solid3D::new();
                solid.common = a.common.clone();
                solid.common.handle = Handle::NULL;
                solid.common.owner_handle = Handle::NULL;
                solid.wires = model::edge_wires(&combined);
                solid.set_sat_document(&sat);
                if let Some(layer) = layer {
                    solid.common.layer = layer;
                }
                self.push_undo("Boolean solids");
                let handle = self.add_entity(EntityType::Solid3D(solid));
                if handle.is_null() {
                    return Err("the result could not be added".into());
                }
                if !keep_operands {
                    self.app.tabs[self.tab].scene.erase_entities(&[first, second]);
                    self.publish_document_view();
                }
                Ok(handle)
            }
            SolidOperation::Transform { handle, matrix } => {
                if matrix.iter().any(|v| !v.is_finite()) {
                    return Err("transform matrix must be finite".into());
                }
                let column = |i: usize| [matrix[i], matrix[i + 1], matrix[i + 2]];
                let (x, y, z) = (column(0), column(4), column(8));
                let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                let rigid = [x, y, z].iter().all(|c| (dot(*c, *c) - 1.0).abs() < 1e-6)
                    && dot(x, y).abs() < 1e-6
                    && dot(x, z).abs() < 1e-6
                    && dot(y, z).abs() < 1e-6
                    && matrix[3].abs() < 1e-9
                    && matrix[7].abs() < 1e-9
                    && matrix[11].abs() < 1e-9
                    && (matrix[15] - 1.0).abs() < 1e-9;
                if !rigid {
                    return Err("only rigid moves and mirrors are supported; scaling and shear are not".into());
                }
                let mut entity = self
                    .document()
                    .get_entity(handle)
                    .cloned()
                    .ok_or_else(|| format!("entity {handle:?} does not exist"))?;
                if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
                    return Err(format!("entity {handle:?} is on a locked layer"));
                }
                macro_rules! retarget {
                    ($value:expr) => {{
                        let body = crate::scene::convert::solid3d_tess::kernel_acis_body(&$value.acis_data)
                            .ok_or("this body's payload cannot be lifted losslessly, so it is left untouched")?;
                        let moved = model::by_matrix(&body, matrix)
                            .ok_or("the geometry kernel refused this transform")?;
                        let sat = crate::scene::convert::acis_export::solid_to_sat(&moved)
                            .ok_or("the moved body could not be exported losslessly")?;
                        $value.wires = model::edge_wires(&moved);
                        $value.silhouettes.clear();
                        $value.history_handle = None;
                        $value.set_sat_document(&sat);
                    }};
                }
                match &mut entity {
                    EntityType::Solid3D(value) => retarget!(value),
                    EntityType::Body(value) => retarget!(value),
                    EntityType::Region(value) => retarget!(value),
                    EntityType::Surface(value) => {
                        let body = crate::scene::convert::solid3d_tess::kernel_acis_body(&value.acis_data)
                            .ok_or("this surface's payload cannot be lifted losslessly, so it is left untouched")?;
                        let moved = model::by_matrix(&body, matrix)
                            .ok_or("the geometry kernel refused this transform")?;
                        let sat = crate::scene::convert::acis_export::solid_to_sat(&moved)
                            .ok_or("the moved surface could not be exported losslessly")?;
                        value.wires = model::edge_wires(&moved);
                        value.silhouettes.clear();
                        value.history_handle = None;
                        value.acis_data = acadrust::entities::AcisData::from_sat(&sat.to_sat_string());
                        // A moved plane is still a plane; a swept surface no longer
                        // matches its stored sweep parameters, so it becomes generic.
                        if value.kind != acadrust::entities::SurfaceKind::Plane {
                            value.kind = acadrust::entities::SurfaceKind::Generic;
                            value.surface_data = acadrust::entities::SurfaceData::Generic;
                        }
                    }
                    _ => return Err("transform applies to Solid3D, Body, Region and Surface entities".into()),
                }
                self.push_undo("Transform solid");
                if !self.update_entity(entity) {
                    return Err(format!("entity {handle:?} could not be updated"));
                }
                Ok(handle)
            }
        }
    }

    /// Look up a profile entity and its exact planar loops for the kernel.
    /// Returns the entity, its plane, loops and closed flag, and the layer the
    /// result should use (`layer`, or the profile's own).
    #[allow(clippy::type_complexity)]
    fn profile_source(
        &self,
        source: Handle,
        layer: Option<String>,
        delete_source: bool,
    ) -> Result<(EntityType, cadkernel::space::Plane, Vec<Vec<cadkernel::geom2d::Curve>>, bool, String), String> {
        if layer.as_ref().is_some_and(|name| name.trim().is_empty()) {
            return Err("layer name is empty".into());
        }
        let entity = self
            .document()
            .get_entity(source)
            .cloned()
            .ok_or_else(|| format!("entity {source:?} does not exist"))?;
        if delete_source && self.app.tabs[self.tab].scene.is_layer_locked(source) {
            return Err(format!("entity {source:?} is on a locked layer"));
        }
        if matches!(entity, EntityType::Solid3D(_) | EntityType::Body(_) | EntityType::Surface(_)) {
            return Err("the source must be a curve profile, not an existing solid or surface".into());
        }
        let (plane, loops, closed) = crate::scene::model::presspull_model::profile_geometry(&entity)
            .ok_or("the source is not a planar profile")?;
        let layer = layer.unwrap_or_else(|| entity.common().layer.clone());
        Ok((entity, plane, loops, closed, layer))
    }

    /// Record one undo step, add `result` and optionally erase the profile.
    fn commit_profile_result(
        &mut self,
        result: EntityType,
        source: Handle,
        delete_source: bool,
        label: &str,
    ) -> Result<Handle, String> {
        self.push_undo(label);
        let handle = self.add_entity(result);
        if handle.is_null() {
            return Err("the result could not be added".into());
        }
        if delete_source {
            self.app.tabs[self.tab].scene.erase_entities(&[source]);
            self.publish_document_view();
        }
        Ok(handle)
    }

    pub fn push_undo(&mut self, label: &str) {
        self.app.push_undo_snapshot(self.tab, label);
    }

    pub fn set_dirty(&mut self) {
        self.app.tabs[self.tab].dirty = true;
    }

    pub fn system_variable(&self, name: &str) -> Option<HostSettingValue> {
        match name.to_ascii_uppercase().as_str() {
            "CLAYER" => Some(HostSettingValue::Text(
                self.document().header.current_layer_name.clone(),
            )),
            "SNAPANG" => Some(HostSettingValue::Number(self.app.snap_angle_deg as f64)),
            "CTAB" => Some(HostSettingValue::Text(self.app.tabs[self.tab].scene.current_layout.clone())),
            _ => None,
        }
    }

    pub fn set_system_variable(
        &mut self,
        name: &str,
        value: HostSettingValue,
    ) -> Result<HostSettingValue, String> {
        match (name.to_ascii_uppercase().as_str(), value) {
            ("CLAYER", HostSettingValue::Text(layer)) => {
                self.app.set_current_layer_name(self.tab, &layer)?;
                Ok(HostSettingValue::Text(layer))
            }
            ("SNAPANG", HostSettingValue::Number(angle)) if (angle as f32).is_finite() => {
                self.app.snap_angle_deg = (angle as f32).rem_euclid(360.0);
                Ok(HostSettingValue::Number(self.app.snap_angle_deg as f64))
            }
            ("SNAPANG", HostSettingValue::Number(_)) => {
                Err("SNAPANG: finite number required".to_owned())
            }
            ("CLAYER", _) | ("SNAPANG", _) => {
                Err(format!("{}: wrong value type", name.to_ascii_uppercase()))
            }
            _ => Err(format!("unsupported system variable {name:?}")),
        }
    }

    pub fn push_info(&mut self, msg: &str) {
        self.app.command_line.push_info(msg);
    }

    pub fn push_output(&mut self, msg: &str) {
        self.app.command_line.push_output(msg);
    }

    pub fn push_error(&mut self, msg: &str) {
        self.app.command_line.push_error(msg);
    }

    pub fn add_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> Option<Handle> {
        let trimmed = config.name.trim();
        if trimmed.is_empty() {
            return None;
        }

        let doc = self.document_mut();
        if doc.layers.contains(trimmed) {
            return None;
        }

        let resolved_lt = match config.linetype {
            // A name the drawing does not carry would leave the LAYER record
            // pointing at no LTYPE, so it is refused rather than written.
            Some(ref lt_name) => match resolve_linetype(doc, lt_name) {
                Some(name) => name,
                None => return None,
            },
            None => "Continuous".to_string(),
        };

        let mut layer = acadrust::tables::Layer::new(trimmed);
        let handle = doc.allocate_handle();
        layer.handle = handle;
        layer.color = config.color.unwrap_or(acadrust::types::Color::Index(7));
        layer.line_type = resolved_lt;
        layer.line_weight = config.lineweight.unwrap_or(acadrust::types::LineWeight::ByLayer);
        layer.flags.off = config.off.unwrap_or(false);
        if let Some(frz) = config.frozen {
            if frz {
                layer.freeze();
            } else {
                layer.thaw();
            }
        }
        layer.flags.locked = config.locked.unwrap_or(false);
        layer.is_plottable = config.plottable.unwrap_or(true);
        layer.transparency = config.transparency.unwrap_or(acadrust::types::Transparency::ByLayer);
        layer.description = config.description.unwrap_or_default();

        let _ = doc.layers.add(layer);

        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab]
            .scene
            .invalidate_layer_dependencies(&[trimmed.to_string()]);
        self.app.refresh_layer_panel();
        self.publish_document_view();
        Some(handle)
    }

    /// Validated, undoable layer-table operations (API v7, additive). Every
    /// refusal happens before the undo step is recorded, so a failure leaves the
    /// drawing and its history untouched.
    pub fn table_operation(
        &mut self,
        operation: ocs_plugin_api::host::TableOperation,
    ) -> Result<Handle, String> {
        use ocs_plugin_api::host::TableOperation;
        let layer_handle = |doc: &CadDocument, name: &str| -> Result<Handle, String> {
            doc.layers
                .get(name)
                .map(|layer| layer.handle)
                .ok_or_else(|| format!("layer {name:?} does not exist"))
        };
        let protected = |name: &str| {
            let name = name.trim();
            name == "0" || name.eq_ignore_ascii_case("Defpoints")
        };
        let same_name = |a: &str, b: &str| a.trim().to_uppercase() == b.trim().to_uppercase();
        let check_color = |color: &acadrust::types::Color| match color {
            acadrust::types::Color::ByLayer
            | acadrust::types::Color::ByBlock
            | acadrust::types::Color::None => Err("a layer color must be an index 1-255 or an RGB value".to_owned()),
            acadrust::types::Color::Index(0) => Err("a layer color index must be 1-255".to_owned()),
            _ => Ok(()),
        };
        let check_config = |config: &ocs_plugin_api::host::LayerConfig| -> Result<(), String> {
            if let Some(color) = &config.color {
                check_color(color)?;
            }
            if let Some(acadrust::types::LineWeight::Value(value)) = config.lineweight {
                if !(0..=211).contains(&value) {
                    return Err("a layer lineweight must be between 0 and 211 (1/100 mm)".to_owned());
                }
            }
            Ok(())
        };
        match operation {
            TableOperation::LayerCreate { config } => {
                let name = validated_layer_name(&config.name)?;
                if self.document().layers.contains(&name) {
                    return Err(format!("layer {name:?} already exists"));
                }
                check_config(&config)?;
                if let Some(linetype) = &config.linetype {
                    if resolve_linetype(self.document_mut(), linetype).is_none() {
                        return Err(format!("linetype {linetype:?} does not exist in this drawing"));
                    }
                }
                self.push_undo("Create layer");
                let config = ocs_plugin_api::host::LayerConfig { name, ..config };
                self.add_layer(config).ok_or_else(|| "the layer could not be created".to_owned())
            }
            TableOperation::LayerModify { config } => {
                let name = config.name.trim().to_owned();
                let handle = layer_handle(self.document(), &name)?;
                if config.color.is_none()
                    && config.linetype.is_none()
                    && config.lineweight.is_none()
                    && config.off.is_none()
                    && config.frozen.is_none()
                    && config.locked.is_none()
                    && config.plottable.is_none()
                    && config.transparency.is_none()
                    && config.description.is_none()
                {
                    return Err("no layer properties to change".to_owned());
                }
                check_config(&config)?;
                if config.frozen == Some(true)
                    && same_name(&self.document().header.current_layer_name, &name)
                {
                    return Err("the current layer cannot be frozen".to_owned());
                }
                if let Some(linetype) = &config.linetype {
                    if resolve_linetype(self.document_mut(), linetype).is_none() {
                        return Err(format!("linetype {linetype:?} does not exist in this drawing"));
                    }
                }
                self.push_undo("Modify layer");
                if self.modify_layer(config) {
                    Ok(handle)
                } else {
                    Err("the layer could not be changed".to_owned())
                }
            }
            TableOperation::LayerRename { from, to } => {
                let handle = layer_handle(self.document(), from.trim())?;
                if protected(&from) {
                    return Err(format!("layer {:?} cannot be renamed", from.trim()));
                }
                let to = validated_layer_name(&to)?;
                if from.trim() == to {
                    return Err("the new layer name is the same as the current one".to_owned());
                }
                if !same_name(&from, &to) && self.document().layers.contains(&to) {
                    return Err(format!("layer {to:?} already exists"));
                }
                self.push_undo("Rename layer");
                if !self.app.tabs[self.tab].rename_layer(from.trim(), &to) {
                    return Err("the layer could not be renamed".to_owned());
                }
                self.finish_layer_change(&[from.trim().to_owned(), to]);
                Ok(handle)
            }
            TableOperation::LayerDelete { name, erase_objects } => {
                let name = name.trim().to_owned();
                let handle = layer_handle(self.document(), &name)?;
                if protected(&name) {
                    return Err(format!("layer {name:?} cannot be deleted"));
                }
                if same_name(&self.document().header.current_layer_name, &name) {
                    return Err("the current layer cannot be deleted".to_owned());
                }
                if name.contains('|') {
                    return Err("an externally referenced layer cannot be deleted".to_owned());
                }
                let key = name.to_uppercase();
                let on_layer: Vec<Handle> = self
                    .document()
                    .entities()
                    .filter(|entity| entity.common().layer.to_uppercase() == key)
                    .map(|entity| entity.common().handle)
                    .collect();
                if !on_layer.is_empty() && !erase_objects {
                    return Err(format!(
                        "layer {name:?} still holds {} object(s); pass erase_objects=True to erase them with the layer",
                        on_layer.len()
                    ));
                }
                self.push_undo("Delete layer");
                self.document_mut().layers.remove(&name);
                if !on_layer.is_empty() {
                    self.app.tabs[self.tab].scene.erase_entities(&on_layer);
                }
                self.finish_layer_change(&[name]);
                Ok(handle)
            }
            TableOperation::LayerSetCurrent { name } => {
                let name = name.trim().to_owned();
                let handle = layer_handle(self.document(), &name)?;
                let stored = self.document().layers.get(&name).map(|l| l.name.clone()).unwrap_or(name);
                self.app.set_current_layer_name(self.tab, &stored)?;
                Ok(handle)
            }
            other @ (TableOperation::BlockCreate { .. }
            | TableOperation::BlockModify { .. }
            | TableOperation::BlockRename { .. }
            | TableOperation::BlockDelete { .. }
            | TableOperation::BlockEntityAdd { .. }) => self.block_operation(other),
            other @ (TableOperation::LinetypeCreate { .. }
            | TableOperation::LinetypeModify { .. }
            | TableOperation::LinetypeRename { .. }
            | TableOperation::LinetypeDelete { .. }) => self.linetype_operation(other),
            other @ (TableOperation::LayoutCreate { .. }
            | TableOperation::LayoutRename { .. }
            | TableOperation::LayoutDelete { .. }
            | TableOperation::LayoutSetCurrent { .. }
            | TableOperation::LayoutSetPage { .. }) => self.layout_operation(other),
            other => self.style_operation(other),
        }
    }

    /// Drive a real OCS command one step at a time (see
    /// `ocs_plugin_api::host::CommandRequest`). The same primitives the
    /// automation channel uses, run synchronously on this thread, so a step
    /// never waits on anything that has to arrive from outside.
    /// The user's SCRIPTCOMMANDS switch. Every route a script can take to the
    /// command line goes through this, not only `run_command`.
    pub(super) fn script_commands_allowed(&self) -> Result<(), String> {
        if self.app.script_commands {
            Ok(())
        } else {
            Err("running commands from a script is turned off (SCRIPTCOMMANDS = 0); turn it on at the command line with SCRIPTCOMMANDS 1".to_owned())
        }
    }

    /// Whether a script may run this command line.
    ///
    /// The alias is resolved first: the dispatcher resolves it too, so a
    /// denylist that reads the token as typed would let a user-defined alias
    /// (`Q` for `QUIT`, added with ALIASEDIT) walk straight past it.
    pub(super) fn script_command_allowed(&self, line: &str) -> Result<(), String> {
        const DENIED: &[&str] = &[
            "QUIT", "EXIT", "CLOSE", "CLOSEALL", "NEW", "QNEW", "OPEN", "SAVE", "QSAVE", "SAVEAS",
            "SAVEALL", "RECOVER", "SCRIPT", "RUNSCRIPT", "SCRIPTCOMMANDS",
        ];
        let Some(token) = line.split_whitespace().next() else {
            return Ok(());
        };
        let token = token.trim_start_matches(['\'', '_']);
        let resolved = self.app.resolve_alias(token);
        let first = resolved.as_deref().unwrap_or(token).to_uppercase();
        let first = first.split_whitespace().next().unwrap_or(&first);
        if DENIED.contains(&first) || first.starts_with("PY_") {
            Err(format!("the command {first} cannot be run from a script"))
        } else {
            Ok(())
        }
    }

    pub fn run_command(
        &mut self,
        request: ocs_plugin_api::host::CommandRequest,
    ) -> Result<ocs_plugin_api::host::CommandOutcome, String> {
        use crate::command::StepInput;
        use ocs_plugin_api::host::CommandRequest as R;
        if self.tab != self.app.active_tab {
            return Err("commands act on the active document; switch to it first".to_owned());
        }
        let tab = self.tab;
        // Cancel stays allowed: if the user turns scripts off mid-run, a command a script left
        // waiting must still be closable rather than stranded.
        if !matches!(request, R::Cancel) {
            self.script_commands_allowed()?;
        }
        let active = self.app.tabs[tab].active_cmd.is_some();
        let before = self.document().entities().count() as i64;
        let error_revision = self.app.command_line.error_revision;
        let editor_before = self.app.text_inline.is_some();
        let mtext_before = self.app.mtext_editor.is_some();
        let modal_before = self.app.active_modal.is_some();
        self.app.command_line.unconsumed.clear();
        let task = match request {
            R::Run { line } => {
                if line.trim().is_empty() {
                    return Err("a command line is empty".to_owned());
                }
                self.script_command_allowed(&line)?;
                if active {
                    return Err("a command is already active; cancel it first".to_owned());
                }
                self.app.suppress_plugin_dispatch = true;
                let task = self.app.run_command_line(&line);
                self.app.suppress_plugin_dispatch = false;
                task
            }
            R::Start { name } => {
                if name.trim().is_empty() || name.split_whitespace().count() != 1 {
                    return Err("a command name is one word".to_owned());
                }
                self.script_command_allowed(&name)?;
                if active {
                    return Err("a command is already active; cancel it first".to_owned());
                }
                self.app.suppress_plugin_dispatch = true;
                let task = self.app.dispatch_command(name.trim());
                self.app.suppress_plugin_dispatch = false;
                task
            }
            R::Cancel => {
                let task = self.app.update(crate::app::Message::CommandEscape);
                self.app.drive_headless_task(task)?;
                // Close what the command opened: an editor or a dialog would leave
                // the interface waiting on a script that has moved on.
                if !editor_before {
                    self.app.text_inline = None;
                }
                if !mtext_before {
                    self.app.mtext_editor = None;
                }
                if !modal_before {
                    self.app.active_modal = None;
                }
                return Ok(self.command_outcome(before, None));
            }
            other => {
                if !active {
                    return Err("no command is waiting for input".to_owned());
                }
                let input = match other {
                    R::Point { point } => {
                        if point.iter().any(|v| !v.is_finite()) {
                            return Err("a point must be finite".to_owned());
                        }
                        StepInput::Point(glam::DVec3::new(point[0], point[1], point[2]))
                    }
                    R::Text { text } => StepInput::Text(text),
                    R::Token { text } => {
                        let task = self.app.feed_active_cmd(&text);
                        self.app.drive_headless_task(task)?;
                        return Ok(self.finish_command_step(before, error_revision, editor_before, mtext_before, modal_before));
                    }
                    R::Entity { handle, point } => {
                        if point.iter().any(|v| !v.is_finite()) {
                            return Err("a point must be finite".to_owned());
                        }
                        if self.document().get_entity(handle).is_none() {
                            return Err(format!("entity {} does not exist", handle.value()));
                        }
                        StepInput::EntityPick(handle, glam::DVec3::new(point[0], point[1], point[2]))
                    }
                    R::Selection => StepInput::SelectionComplete(self.app.tabs[tab].scene.selected_handles_in_order()),
                    R::Enter => StepInput::Enter,
                    R::Run { .. } | R::Start { .. } | R::Cancel => unreachable!(),
                };
                self.app.feed_command(input)
            }
        };
        self.app.drive_headless_task(task)?;
        Ok(self.finish_command_step(before, error_revision, editor_before, mtext_before, modal_before))
    }

    fn finish_command_step(
        &mut self,
        before: i64,
        error_revision: u64,
        editor_before: bool,
        mtext_before: bool,
        modal_before: bool,
    ) -> ocs_plugin_api::host::CommandOutcome {
        let error = (self.app.command_line.error_revision != error_revision)
            .then(|| self.app.command_line.last_error.clone().unwrap_or_default());
        let blocked = if self.app.tabs[self.tab].active_cmd.is_some() {
            Some("command".to_owned())
        } else if let (false, Some(modal)) = (modal_before, self.app.active_modal.as_ref()) {
            Some(format!("modal:{modal:?}"))
        } else if !editor_before && self.app.text_inline.is_some() {
            Some("text_editor".to_owned())
        } else if !mtext_before && self.app.mtext_editor.is_some() {
            Some("mtext_editor".to_owned())
        } else {
            None
        };
        self.set_dirty();
        self.publish_document_view();
        let mut outcome = self.command_outcome(before, blocked);
        outcome.error = error;
        outcome
    }

    fn command_outcome(&self, before: i64, blocked_by: Option<String>) -> ocs_plugin_api::host::CommandOutcome {
        let now = self.document().entities().count() as i64;
        let metadata = self.app.tabs[self.tab]
            .active_cmd
            .as_deref()
            .map(crate::app::control::active_command_metadata);
        let text = |key: &str| metadata.as_ref().and_then(|m| m[key].as_str()).unwrap_or("").to_owned();
        let list = |key: &str| -> Vec<String> {
            metadata
                .as_ref()
                .and_then(|m| m[key].as_array())
                .map(|items| items.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default()
        };
        let options = metadata
            .as_ref()
            .and_then(|m| m["options"].as_array())
            .map(|items| items.iter().filter_map(|o| o["keyword"].as_str().filter(|k| !k.is_empty()).map(str::to_owned)).collect())
            .unwrap_or_default();
        ocs_plugin_api::host::CommandOutcome {
            status: if blocked_by.is_some() { "waiting_input" } else { "completed" }.to_owned(),
            blocked_by,
            command: text("name"),
            prompt: text("prompt"),
            accepts: list("accepts"),
            options,
            entities: now as u64,
            added: now - before,
            unconsumed: self.app.command_line.unconsumed.clone(),
            error: None,
        }
    }

    /// Simple linetype create/modify/rename/delete.
    fn linetype_operation(&mut self, operation: ocs_plugin_api::host::TableOperation) -> Result<Handle, String> {
        use ocs_plugin_api::host::TableOperation;
        let protected = |name: &str| {
            ["CONTINUOUS", "BYLAYER", "BYBLOCK"].contains(&name.trim().to_uppercase().as_str())
        };
        let check_pattern = |pattern: &[f64]| -> Result<(), String> {
            if !(2..=12).contains(&pattern.len()) {
                return Err("a linetype pattern needs 2 to 12 elements".to_owned());
            }
            if pattern.iter().any(|v| !v.is_finite() || v.abs() > 1.0e6) {
                return Err("linetype pattern lengths must be finite and within 1e6".to_owned());
            }
            if !pattern.iter().any(|v| *v >= 0.0) || !pattern.iter().any(|v| *v < 0.0) {
                return Err("a linetype pattern needs at least one dash or dot and one gap".to_owned());
            }
            if pattern.iter().map(|v| v.abs()).sum::<f64>() <= 0.0 {
                return Err("a linetype pattern must have a nonzero length".to_owned());
            }
            Ok(())
        };
        let build = |lt: &mut acadrust::tables::LineType, pattern: &[f64]| {
            lt.elements = pattern
                .iter()
                .map(|length| acadrust::tables::linetype::LineTypeElement { length: *length, complex: None })
                .collect();
            lt.pattern_length = pattern.iter().map(|v| v.abs()).sum();
        };
        let check_description = |text: &str| {
            if text.chars().any(|c| c.is_control()) || text.chars().count() > 255 {
                Err("a linetype description must be text of at most 255 characters".to_owned())
            } else {
                Ok(())
            }
        };
        // Loading the standard linetypes first means a name that already exists as a
        // standard pattern (Dashed, Center, ...) is seen as taken.
        crate::io::linetypes::populate_document(self.document_mut());
        match operation {
            TableOperation::LinetypeCreate { name, description, pattern } => {
                let name = validated_symbol_name(&name, "a linetype name")?;
                if self.document().line_types.contains(&name) {
                    return Err(format!("linetype {name:?} already exists"));
                }
                check_pattern(&pattern)?;
                check_description(&description)?;
                self.push_undo("Create linetype");
                let mut lt = acadrust::tables::LineType::new(name.clone());
                lt.description = description;
                build(&mut lt, &pattern);
                let handle = self.document_mut().allocate_handle();
                lt.handle = handle;
                self.document_mut().line_types.add(lt).map_err(|e| format!("the linetype could not be created: {e}"))?;
                self.finish_linetype_change(&[name]);
                Ok(handle)
            }
            TableOperation::LinetypeModify { name, description, pattern } => {
                let existing = self
                    .document()
                    .line_types
                    .get(name.trim())
                    .cloned()
                    .ok_or_else(|| format!("linetype {:?} does not exist", name.trim()))?;
                if protected(&existing.name) || existing.xref_dependent {
                    return Err(format!("linetype {:?} cannot be changed", existing.name));
                }
                if existing.elements.iter().any(|e| e.complex.is_some()) {
                    return Err("a linetype with text or shape elements cannot be changed from a script".to_owned());
                }
                if description.is_none() && pattern.is_none() {
                    return Err("no linetype properties to change".to_owned());
                }
                if let Some(text) = &description {
                    check_description(text)?;
                }
                if let Some(pattern) = &pattern {
                    check_pattern(pattern)?;
                }
                self.push_undo("Modify linetype");
                let mut lt = existing.clone();
                if let Some(text) = description {
                    lt.description = text;
                }
                if let Some(pattern) = &pattern {
                    build(&mut lt, pattern);
                }
                if let Some(slot) = self.document_mut().line_types.get_mut(&existing.name) {
                    *slot = lt;
                }
                self.finish_linetype_change(&[existing.name.clone()]);
                Ok(existing.handle)
            }
            TableOperation::LinetypeRename { from, to } => {
                let existing = self
                    .document()
                    .line_types
                    .get(from.trim())
                    .cloned()
                    .ok_or_else(|| format!("linetype {:?} does not exist", from.trim()))?;
                if protected(&existing.name) || existing.xref_dependent {
                    return Err(format!("linetype {:?} cannot be renamed", existing.name));
                }
                let to = validated_symbol_name(&to, "a linetype name")?;
                if existing.name.eq_ignore_ascii_case(&to) {
                    return Err("the new linetype name matches the current one (a case-only change is not supported)".to_owned());
                }
                if self.document().line_types.contains(&to) {
                    return Err(format!("linetype {to:?} already exists"));
                }
                self.push_undo("Rename linetype");
                let old = existing.name.clone();
                let handle = existing.handle;
                let doc = self.document_mut();
                let mut lt = existing;
                lt.name = to.clone();
                doc.line_types.remove(&old);
                doc.line_types.add(lt).map_err(|e| format!("the linetype could not be renamed: {e}"))?;
                for layer in doc.layers.iter_mut() {
                    if layer.line_type.eq_ignore_ascii_case(&old) {
                        layer.line_type = to.clone();
                    }
                }
                for entity in doc.entities_mut() {
                    if entity.common().linetype.eq_ignore_ascii_case(&old) {
                        entity.common_mut().linetype = to.clone();
                    }
                }
                if doc.header.current_linetype_name.eq_ignore_ascii_case(&old) {
                    doc.header.current_linetype_name = to.clone();
                }
                self.finish_linetype_change(&[old, to]);
                Ok(handle)
            }
            TableOperation::LinetypeDelete { name } => {
                let existing = self
                    .document()
                    .line_types
                    .get(name.trim())
                    .cloned()
                    .ok_or_else(|| format!("linetype {:?} does not exist", name.trim()))?;
                if protected(&existing.name) || existing.xref_dependent {
                    return Err(format!("linetype {:?} cannot be deleted", existing.name));
                }
                let doc = self.document();
                let in_use = doc.header.current_linetype_name.eq_ignore_ascii_case(&existing.name)
                    || doc.layers.iter().any(|layer| layer.line_type.eq_ignore_ascii_case(&existing.name))
                    || doc.entities().any(|entity| entity.common().linetype.eq_ignore_ascii_case(&existing.name))
                    || doc.dim_styles.iter().any(|style| {
                        [style.dimltex_handle, style.dimltex1_handle, style.dimltex2_handle].contains(&existing.handle)
                    });
                if in_use {
                    return Err(format!("linetype {:?} is current or still in use", existing.name));
                }
                self.push_undo("Delete linetype");
                self.document_mut().line_types.remove(&existing.name);
                self.finish_linetype_change(&[existing.name]);
                Ok(existing.handle)
            }
            _ => Err("unsupported table operation".to_owned()),
        }
    }

    fn finish_linetype_change(&mut self, names: &[String]) {
        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab].scene.invalidate_layer_dependencies(names);
        self.app.tabs[self.tab].scene.bump_geometry();
        self.publish_document_view();
    }

    /// Paper-space layout operations (create, rename, delete, switch, page).
    /// They act on the active document because switching layouts drives the
    /// application's view state.
    fn layout_operation(&mut self, operation: ocs_plugin_api::host::TableOperation) -> Result<Handle, String> {
        use acadrust::objects::ObjectType;
        use ocs_plugin_api::host::TableOperation;
        if self.tab != self.app.active_tab {
            return Err("layout operations act on the active document; switch to it first".to_owned());
        }
        let find = |doc: &CadDocument, name: &str| -> Result<(String, Handle), String> {
            doc.objects
                .values()
                .find_map(|object| match object {
                    ObjectType::Layout(layout)
                        if !layout.block_record.is_null() && layout.name.eq_ignore_ascii_case(name) =>
                    {
                        Some((layout.name.clone(), layout.handle))
                    }
                    _ => None,
                })
                .ok_or_else(|| format!("layout {name:?} does not exist"))
        };
        let names_taken = |app: &crate::app::OpenCADStudio, tab: usize, name: &str| {
            app.tabs[tab].scene.layout_names().iter().any(|existing| existing.eq_ignore_ascii_case(name))
        };
        match operation {
            TableOperation::LayoutCreate { name } => {
                let name = validated_symbol_name(&name, "a layout name")?;
                if names_taken(self.app, self.tab, &name) {
                    return Err(format!("layout {name:?} already exists"));
                }
                self.push_undo("Create layout");
                let handle = self
                    .document_mut()
                    .add_layout(&name)
                    .map_err(|e| format!("the layout could not be created: {e}"))?;
                let flags = i16::from(self.document().header.paper_space_linetype_scaling)
                    | (i16::from(self.document().header.paper_space_limit_check) << 1);
                let plot_style = self
                    .app
                    .active_plot_style
                    .as_ref()
                    .map(|style| style.name.clone())
                    .unwrap_or_default();
                for object in self.document_mut().objects.values_mut() {
                    if let ObjectType::Layout(layout) = object {
                        if layout.name == name {
                            layout.flags = flags;
                            crate::scene::apply_default_page_setup(layout, &plot_style);
                            break;
                        }
                    }
                }
                self.app.tabs[self.tab].scene.ensure_sheet_viewport(&name);
                self.finish_layout_change();
                Ok(handle)
            }
            TableOperation::LayoutRename { from, to } => {
                let (from, handle) = find(self.document(), from.trim())?;
                if from.eq_ignore_ascii_case("Model") {
                    return Err("the Model layout cannot be renamed".to_owned());
                }
                let to = validated_symbol_name(&to, "a layout name")?;
                if to.eq_ignore_ascii_case("Model") {
                    return Err("a layout cannot be named Model".to_owned());
                }
                if from.eq_ignore_ascii_case(&to) {
                    return Err("the new layout name matches the current one (a case-only change is not supported)".to_owned());
                }
                if names_taken(self.app, self.tab, &to) {
                    return Err(format!("layout {to:?} already exists"));
                }
                self.push_undo("Rename layout");
                self.app.tabs[self.tab].scene.rename_layout(&from, &to);
                // The layout dictionary is keyed by name; keep it in step.
                let dict = self.document().header.acad_layout_dict_handle;
                if let Some(ObjectType::Dictionary(dictionary)) = self.document_mut().objects.get_mut(&dict) {
                    for (key, _) in dictionary.entries.iter_mut() {
                        if *key == from {
                            *key = to.clone();
                        }
                    }
                }
                if self.app.tabs[self.tab].scene.current_layout == from {
                    self.app.tabs[self.tab].scene.set_current_layout(to);
                }
                self.finish_layout_change();
                Ok(handle)
            }
            TableOperation::LayoutDelete { name } => {
                let (name, handle) = find(self.document(), name.trim())?;
                if name.eq_ignore_ascii_case("Model") {
                    return Err("the Model layout cannot be deleted".to_owned());
                }
                self.push_undo("Delete layout");
                if self.app.tabs[self.tab].scene.current_layout == name {
                    let _ = self.app.on_layout_switch("Model".to_owned());
                }
                if !self.app.tabs[self.tab].scene.delete_layout(&name) {
                    return Err("the layout could not be deleted".to_owned());
                }
                self.finish_layout_change();
                Ok(handle)
            }
            TableOperation::LayoutSetCurrent { name } => {
                let name = name.trim();
                if name.eq_ignore_ascii_case("Model") {
                    let _ = self.app.on_layout_switch("Model".to_owned());
                    return Ok(Handle::NULL);
                }
                let (name, handle) = find(self.document(), name)?;
                let _ = self.app.on_layout_switch(name);
                self.publish_document_view();
                Ok(handle)
            }
            TableOperation::LayoutSetPage { name, paper_size, rotation, scale } => {
                let (name, handle) = find(self.document(), name.trim())?;
                if name.eq_ignore_ascii_case("Model") {
                    return Err("the Model layout has no sheet setup".to_owned());
                }
                if paper_size.is_none() && rotation.is_none() && scale.is_none() {
                    return Err("no page setup properties to change".to_owned());
                }
                if let Some([width, height]) = paper_size {
                    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 && width <= 100_000.0 && height <= 100_000.0) {
                        return Err("the paper size must be positive and at most 100000 mm each way".to_owned());
                    }
                }
                let rotation_code = match rotation {
                    None => None,
                    Some(0) => Some(acadrust::objects::PlotRotation::None),
                    Some(90) => Some(acadrust::objects::PlotRotation::Degrees90),
                    Some(180) => Some(acadrust::objects::PlotRotation::Degrees180),
                    Some(270) => Some(acadrust::objects::PlotRotation::Degrees270),
                    Some(_) => return Err("the plot rotation must be 0, 90, 180 or 270 degrees".to_owned()),
                };
                if let Some([numerator, denominator]) = scale {
                    if !(numerator.is_finite() && denominator.is_finite() && numerator > 0.0 && denominator > 0.0) {
                        return Err("the plot scale numerator and denominator must be positive".to_owned());
                    }
                }
                self.push_undo("Layout page setup");
                for object in self.document_mut().objects.values_mut() {
                    let ObjectType::Layout(layout) = object else { continue };
                    if layout.name != name {
                        continue;
                    }
                    if let Some([width, height]) = paper_size {
                        layout.paper_width = width;
                        layout.paper_height = height;
                        layout.paper_size = String::new();
                    }
                    if let Some(code) = rotation_code {
                        layout.plot_rotation = code.to_code();
                    }
                    if let Some([numerator, denominator]) = scale {
                        layout.plot_scale_numerator = numerator;
                        layout.plot_scale_denominator = denominator;
                        layout.plot_scale_factor = numerator / denominator;
                        layout.plot_scale_type = acadrust::objects::ScaledType::CustomScale.to_code();
                        layout.plot_flags.use_standard_scale = false;
                    }
                    let quarter_turn = layout.plot_rotation == acadrust::objects::PlotRotation::Degrees90.to_code()
                        || layout.plot_rotation == acadrust::objects::PlotRotation::Degrees270.to_code();
                    let (x, y) = if quarter_turn {
                        (layout.paper_height, layout.paper_width)
                    } else {
                        (layout.paper_width, layout.paper_height)
                    };
                    layout.min_limits = (0.0, 0.0);
                    layout.max_limits = (x, y);
                    layout.min_extents = (0.0, 0.0, 0.0);
                    layout.max_extents = (x, y, 0.0);
                    break;
                }
                self.app.tabs[self.tab].scene.invalidate_display_plot_style();
                self.finish_layout_change();
                Ok(handle)
            }
            _ => Err("unsupported table operation".to_owned()),
        }
    }

    fn finish_layout_change(&mut self) {
        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab].scene.bump_geometry();
        self.publish_document_view();
    }

    /// Block definition operations: create from entities, modify settings,
    /// rename (inserts follow) and delete an unreferenced definition.
    fn block_operation(&mut self, operation: ocs_plugin_api::host::TableOperation) -> Result<Handle, String> {
        use acadrust::entities::EntityType as E;
        use ocs_plugin_api::host::TableOperation;
        let editable_block = |doc: &CadDocument, name: &str| -> Result<(String, Handle), String> {
            let record = doc
                .block_records
                .get(name)
                .ok_or_else(|| format!("block {name:?} does not exist"))?;
            if record.is_layout() || record.is_anonymous() || record.flags.is_xref || record.flags.is_xref_overlay || record.name.contains('|') {
                return Err(format!("block {:?} is a layout, anonymous or externally referenced block", record.name));
            }
            Ok((record.name.clone(), record.handle))
        };
        match operation {
            TableOperation::BlockCreate { name, entities, base_point, erase_originals, description } => {
                let name = validated_symbol_name(&name, "a block name")?;
                if name.starts_with('*') {
                    return Err("a block name cannot start with '*'".to_owned());
                }
                if self.document().block_records.get(&name).is_some() {
                    return Err(format!("block {name:?} already exists"));
                }
                if base_point.iter().any(|v| !v.is_finite()) {
                    return Err("the block base point must be finite".to_owned());
                }
                if entities.is_empty() {
                    return Err("a block needs at least one entity".to_owned());
                }
                let mut seen = std::collections::HashSet::new();
                let mut sources = Vec::with_capacity(entities.len());
                for handle in &entities {
                    if !seen.insert(handle.value()) {
                        return Err(format!("entity {} is listed twice", handle.value()));
                    }
                    let entity = self
                        .document()
                        .get_entity(*handle)
                        .ok_or_else(|| format!("entity {} does not exist", handle.value()))?;
                    let owner = entity.common().owner_handle;
                    let in_canvas = self
                        .document()
                        .block_records
                        .iter()
                        .any(|record| record.handle == owner && record.is_layout());
                    if !in_canvas {
                        return Err(format!("entity {} is not in model or paper space", handle.value()));
                    }
                    if matches!(entity, E::Viewport(_) | E::Block(_) | E::BlockEnd(_) | E::AttributeEntity(_) | E::Unknown(_)) {
                        return Err(format!("entity {} of this kind cannot be placed in a block", handle.value()));
                    }
                    sources.push(entity.clone());
                }
                if let Some(text) = &description {
                    if text.chars().any(|c| c.is_control() && c != '\n') {
                        return Err("a block description cannot contain control characters".to_owned());
                    }
                }
                self.push_undo("Create block");
                let base = glam::DVec3::new(base_point[0], base_point[1], base_point[2]);
                self.app.tabs[self.tab]
                    .scene
                    .define_block_from_owned_entities(sources, &name, base)?;
                let (stored, handle) = editable_block(self.document(), &name)?;
                if let Some(record) = self.document_mut().block_records.get_mut(&stored) {
                    record.base_point = acadrust::types::Vector3::new(base_point[0], base_point[1], base_point[2]);
                    if let Some(text) = description {
                        record.description = text;
                    }
                }
                let marker = self.document().block_records.get(&stored).map(|r| r.block_entity_handle);
                if let Some(marker) = marker {
                    if let Some(E::Block(block)) = self.document_mut().get_entity_mut(marker) {
                        block.base_point = acadrust::types::Vector3::new(base_point[0], base_point[1], base_point[2]);
                    }
                }
                if erase_originals {
                    self.app.tabs[self.tab].scene.erase_entities(&entities);
                }
                self.finish_block_change();
                Ok(handle)
            }
            TableOperation::BlockEntityAdd { block, mut entity } => {
                let (name, record_handle) = editable_block(self.document(), block.trim())?;
                if matches!(
                    entity,
                    E::Viewport(_) | E::Block(_) | E::BlockEnd(_) | E::AttributeEntity(_) | E::Unknown(_) | E::RasterImage(_)
                ) {
                    return Err("an entity of this kind cannot be added to a block from a script".to_owned());
                }
                if self.app.tabs[self.tab].scene.block_edit_block.is_some() {
                    return Err("finish the open block editor before adding entities to a block".to_owned());
                }
                if let E::Insert(insert) = &entity {
                    if insert.block_name.eq_ignore_ascii_case(&name) || block_nests(self.document(), &insert.block_name, &name, 0) {
                        return Err(format!("inserting {:?} into {name:?} would nest a block inside itself", insert.block_name));
                    }
                }
                entity.common_mut().owner_handle = record_handle;
                ocs_plugin_api::entity_coverage::validate_new_canvas_entity(&entity)?;
                ocs_plugin_api::entity_coverage::bind_canvas_entity_references(self.document(), &mut entity)?;
                if self.document().layers.get(&entity.common().layer).is_some_and(|layer| layer.is_locked()) {
                    return Err(format!("layer {:?} is locked", entity.common().layer));
                }
                self.push_undo("Add to block");
                entity.common_mut().handle = Handle::NULL;
                entity.common_mut().owner_handle = record_handle;
                // The scene routes an add into the block being edited; reuse that
                // path so the entity gets the same preparation as any new entity.
                self.app.tabs[self.tab].scene.block_edit_block = Some(record_handle);
                let handle = self.add_entity(entity);
                self.app.tabs[self.tab].scene.block_edit_block = None;
                if handle.is_null() {
                    return Err(format!("the entity could not be added to block {name:?}"));
                }
                self.finish_block_change();
                Ok(handle)
            }
            TableOperation::BlockModify { name, description, explodable, scale_uniformly } => {
                let (name, handle) = editable_block(self.document(), name.trim())?;
                if description.is_none() && explodable.is_none() && scale_uniformly.is_none() {
                    return Err("no block properties to change".to_owned());
                }
                if let Some(text) = &description {
                    if text.chars().any(|c| c.is_control() && c != '\n') {
                        return Err("a block description cannot contain control characters".to_owned());
                    }
                }
                self.push_undo("Modify block");
                if let Some(record) = self.document_mut().block_records.get_mut(&name) {
                    if let Some(text) = description {
                        record.description = text;
                    }
                    if let Some(value) = explodable {
                        record.explodable = value;
                    }
                    if let Some(value) = scale_uniformly {
                        record.scale_uniformly = value;
                    }
                }
                self.finish_block_change();
                Ok(handle)
            }
            TableOperation::BlockRename { from, to } => {
                let (from, handle) = editable_block(self.document(), from.trim())?;
                let to = validated_symbol_name(&to, "a block name")?;
                if to.starts_with('*') {
                    return Err("a block name cannot start with '*'".to_owned());
                }
                if from.eq_ignore_ascii_case(&to) {
                    return Err("the new block name matches the current one (a case-only change is not supported)".to_owned());
                }
                if self.document().block_records.get(&to).is_some() {
                    return Err(format!("block {to:?} already exists"));
                }
                self.push_undo("Rename block");
                if !self.app.tabs[self.tab].scene.rename_block(&from, &to) {
                    return Err("the block could not be renamed".to_owned());
                }
                self.finish_block_change();
                Ok(handle)
            }
            TableOperation::BlockDelete { name } => {
                let (name, handle) = editable_block(self.document(), name.trim())?;
                let doc = self.document();
                let referenced = doc.entities().any(|entity| match entity {
                    E::Insert(insert) => insert.block_name.eq_ignore_ascii_case(&name),
                    E::MultiLeader(leader) => {
                        leader.block_content_handle == Some(handle)
                            || leader.context.block_content_handle == Some(handle)
                    }
                    _ => false,
                }) || doc.dim_styles.iter().any(|style| {
                    [style.dimblk, style.dimblk1, style.dimblk2, style.dimldrblk].contains(&handle)
                });
                if referenced {
                    return Err(format!("block {name:?} is still inserted or used by a style or leader"));
                }
                let members: Vec<Handle> = {
                    let record = doc.block_records.get(&name).expect("checked above");
                    let mut members = record.entity_handles.clone();
                    members.push(record.block_entity_handle);
                    members.push(record.block_end_handle);
                    members
                };
                self.push_undo("Delete block");
                let doc = self.document_mut();
                for member in members {
                    doc.remove_entity(member);
                }
                doc.block_records.remove(&name);
                let orphans: Vec<Handle> = {
                    let live: std::collections::HashSet<Handle> = doc.block_records.iter().map(|r| r.handle).collect();
                    doc.objects
                        .iter()
                        .filter_map(|(h, object)| match object {
                            acadrust::objects::ObjectType::SortEntitiesTable(table) if !live.contains(&table.block_owner_handle) => Some(*h),
                            _ => None,
                        })
                        .collect()
                };
                for orphan in orphans {
                    doc.objects.remove(&orphan);
                }
                self.finish_block_change();
                Ok(handle)
            }
            _ => Err("unsupported table operation".to_owned()),
        }
    }

    fn finish_block_change(&mut self) {
        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab].scene.invalidate_dependency_index();
        self.app.tabs[self.tab].scene.bump_geometry();
        self.publish_document_view();
    }

    /// Text and dimension style operations. They act on the active document
    /// because the shared style machinery (in-use scan, rename, ribbon sync) is
    /// keyed by the active tab.
    fn style_operation(&mut self, operation: ocs_plugin_api::host::TableOperation) -> Result<Handle, String> {
        use crate::app::style_ops::StyleKind;
        use ocs_plugin_api::host::{TableOperation, TableStyleKind};
        if self.tab != self.app.active_tab {
            return Err("style operations act on the active document; switch to it first".to_owned());
        }
        let kind_of = |kind: TableStyleKind| match kind {
            TableStyleKind::Text => StyleKind::Text,
            TableStyleKind::Dim => StyleKind::Dim,
        };
        let stored = |doc: &CadDocument, kind: TableStyleKind, name: &str| -> Result<(String, Handle), String> {
            let found = match kind {
                TableStyleKind::Text => doc.text_styles.get(name).map(|s| (s.name.clone(), s.handle)),
                TableStyleKind::Dim => doc.dim_styles.get(name).map(|s| (s.name.clone(), s.handle)),
            };
            found.ok_or_else(|| format!("{} style {name:?} does not exist", match kind {
                TableStyleKind::Text => "text",
                TableStyleKind::Dim => "dimension",
            }))
        };
        match operation {
            TableOperation::TextStyleCreate { config } => {
                let name = validated_symbol_name(&config.name, "a text style name")?;
                if self.app.style_exists(StyleKind::Text, &name) {
                    return Err(format!("text style {name:?} already exists"));
                }
                let mut style = acadrust::tables::TextStyle::new(name.clone());
                apply_text_style_config(&mut style, &config)?;
                self.push_undo("Create text style");
                let handle = self.document_mut().allocate_handle();
                style.handle = handle;
                self.document_mut().text_styles.add(style).map_err(|e| format!("the text style could not be created: {e}"))?;
                self.finish_style_change(StyleKind::Text);
                Ok(handle)
            }
            TableOperation::TextStyleModify { config } => {
                let (name, handle) = stored(self.document(), TableStyleKind::Text, config.name.trim())?;
                let mut style = self.document().text_styles.get(&name).cloned().expect("checked above");
                let before = style.clone();
                if config == (ocs_plugin_api::host::TextStyleConfig { name: config.name.clone(), ..Default::default() }) {
                    return Err("no text style properties to change".to_owned());
                }
                apply_text_style_config(&mut style, &config)?;
                if style == before {
                    return Ok(handle);
                }
                self.push_undo("Modify text style");
                if let Some(slot) = self.document_mut().text_styles.get_mut(&name) {
                    *slot = style;
                }
                self.finish_style_change(StyleKind::Text);
                Ok(handle)
            }
            TableOperation::DimStyleCreate { name, copy_from, properties } => {
                let name = validated_symbol_name(&name, "a dimension style name")?;
                if self.app.style_exists(StyleKind::Dim, &name) {
                    return Err(format!("dimension style {name:?} already exists"));
                }
                let base = match &copy_from {
                    Some(source) => {
                        let (source, _) = stored(self.document(), TableStyleKind::Dim, source.trim())?;
                        self.document().dim_styles.get(&source).cloned().expect("checked above")
                    }
                    None => acadrust::tables::DimStyle::new(name.clone()),
                };
                let mut style = apply_dim_style_json(self.document(), &base, &properties)?;
                self.push_undo("Create dimension style");
                let handle = self.document_mut().allocate_handle();
                style.handle = handle;
                style.name = name;
                self.document_mut().dim_styles.add(style).map_err(|e| format!("the dimension style could not be created: {e}"))?;
                self.finish_style_change(StyleKind::Dim);
                Ok(handle)
            }
            TableOperation::DimStyleModify { name, properties } => {
                let (name, handle) = stored(self.document(), TableStyleKind::Dim, name.trim())?;
                let base = self.document().dim_styles.get(&name).cloned().expect("checked above");
                let style = apply_dim_style_json(self.document(), &base, &properties)?;
                if style == base {
                    return Ok(handle);
                }
                self.push_undo("Modify dimension style");
                if let Some(slot) = self.document_mut().dim_styles.get_mut(&name) {
                    *slot = style;
                }
                self.finish_style_change(StyleKind::Dim);
                Ok(handle)
            }
            TableOperation::StyleRename { kind, from, to } => {
                let (from, handle) = stored(self.document(), kind, from.trim())?;
                if from.eq_ignore_ascii_case("Standard") {
                    return Err("the Standard style cannot be renamed".to_owned());
                }
                let to = validated_symbol_name(&to, "a style name")?;
                if from.eq_ignore_ascii_case(&to) {
                    return Err("the new style name matches the current one (a case-only change is not supported)".to_owned());
                }
                if self.app.style_exists(kind_of(kind), &to) {
                    return Err(format!("style {to:?} already exists"));
                }
                self.push_undo("Rename style");
                self.app.rename_style_storage(kind_of(kind), &from, &to);
                if kind == TableStyleKind::Text {
                    // A dimension style names its text style; follow the rename.
                    for dim in self.document_mut().dim_styles.iter_mut() {
                        if dim.dimtxsty.eq_ignore_ascii_case(&from) {
                            dim.dimtxsty = to.clone();
                        }
                    }
                }
                self.finish_style_change(kind_of(kind));
                Ok(handle)
            }
            TableOperation::StyleDelete { kind, name } => {
                let (name, handle) = stored(self.document(), kind, name.trim())?;
                if name.eq_ignore_ascii_case("Standard") {
                    return Err("the Standard style cannot be deleted".to_owned());
                }
                if self.app.style_in_use(kind_of(kind), &name) {
                    return Err(format!("style {name:?} is current or still in use"));
                }
                self.push_undo("Delete style");
                if !self.app.remove_style_storage(kind_of(kind), &name) {
                    return Err("the style could not be deleted".to_owned());
                }
                self.finish_style_change(kind_of(kind));
                Ok(handle)
            }
            TableOperation::StyleSetCurrent { kind, name } => {
                let (name, handle) = stored(self.document(), kind, name.trim())?;
                let header = &mut self.document_mut().header;
                match kind {
                    TableStyleKind::Text => {
                        header.current_text_style_handle = handle;
                        header.current_text_style_name = name.clone();
                        self.app.ribbon.active_text_style = name;
                    }
                    TableStyleKind::Dim => {
                        header.current_dimstyle_handle = handle;
                        header.current_dimstyle_name = name.clone();
                        self.app.ribbon.active_dim_style = name;
                    }
                }
                self.app.tabs[self.tab].dirty = true;
                Ok(handle)
            }
            _ => Err("unsupported table operation".to_owned()),
        }
    }

    fn finish_style_change(&mut self, kind: crate::app::style_ops::StyleKind) {
        self.app.tabs[self.tab].dirty = true;
        self.app.after_style_change(kind);
        self.app.tabs[self.tab].scene.bump_geometry();
        self.publish_document_view();
    }

    fn finish_layer_change(&mut self, names: &[String]) {
        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab].scene.invalidate_layer_dependencies(names);
        self.app.refresh_layer_panel();
        self.publish_document_view();
    }

    pub fn modify_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> bool {
        let trimmed = config.name.trim();
        if trimmed.is_empty() {
            return false;
        }

        let resolved_lt = match config.linetype {
            Some(ref lt_name) => match resolve_linetype(self.document_mut(), lt_name) {
                Some(name) => Some(name),
                None => return false,
            },
            None => None,
        };

        let doc = self.document_mut();
        let Some(existing) = doc.layers.get_mut(trimmed) else {
            return false;
        };

        if let Some(c) = config.color {
            existing.color = c;
            existing.color_name = None;
            existing.book_name = None;
        }
        if let Some(lt) = resolved_lt {
            existing.line_type = lt;
        }
        if let Some(lw) = config.lineweight {
            existing.line_weight = lw;
        }
        if let Some(off) = config.off {
            existing.flags.off = off;
        }
        if let Some(frz) = config.frozen {
            if frz {
                existing.freeze();
            } else {
                existing.thaw();
            }
        }
        if let Some(lck) = config.locked {
            existing.flags.locked = lck;
        }
        if let Some(plt) = config.plottable {
            existing.is_plottable = plt;
        }
        if let Some(tr) = config.transparency {
            existing.transparency = tr;
        }
        if let Some(desc) = config.description {
            existing.description = desc;
        }

        self.app.tabs[self.tab].dirty = true;
        self.app.tabs[self.tab]
            .scene
            .invalidate_layer_dependencies(&[trimmed.to_string()]);
        self.app.refresh_layer_panel();
        self.publish_document_view();
        true
    }

    /// Run a command string on the active tab's command line (AutoLISP style).
    /// Supports AutoCAD-style `PAUSE` (and `\`) input queuing, explicit `ENTER` /
    /// `RETURN` tokens, and `\n` trailing newline execution.
    pub fn execute_command(&mut self, cmd: &str) -> bool {
        let has_newline = cmd.ends_with('\n') || cmd.ends_with('\r');
        let trimmed = cmd.trim();

        // The same two guards `run_command` applies: this is the other route a
        // script reaches the command line by, so it cannot be the unguarded one.
        // Cancelling stays allowed, as it does there.
        let cancelling = trimmed.eq_ignore_ascii_case("ESC")
            || trimmed.eq_ignore_ascii_case("ESCAPE")
            || trimmed.eq_ignore_ascii_case("CANCEL");
        if !cancelling
            && (self.script_commands_allowed().is_err()
                || self.script_command_allowed(trimmed).is_err())
        {
            return false;
        }

        // 1. ESC / CANCEL handling
        if trimmed.eq_ignore_ascii_case("ESC")
            || trimmed.eq_ignore_ascii_case("ESCAPE")
            || trimmed.eq_ignore_ascii_case("CANCEL")
        {
            self.app.tabs[self.tab].pending_pause_tokens = None;
            let _ = self.app.feed_command(crate::command::StepInput::Escape);
            self.publish_document_view();
            return true;
        }

        // 2. If a PAUSE is already pending for this tab:
        if let Some(ref mut queue) = self.app.tabs[self.tab].pending_pause_tokens {
            if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("ENTER")
                || trimmed.eq_ignore_ascii_case("RETURN")
            {
                queue.push("ENTER".to_string());
            } else {
                for part in trimmed.split_whitespace() {
                    queue.push(part.to_string());
                }
                if has_newline {
                    queue.push("ENTER".to_string());
                }
            }
            return true;
        }

        // 3. ENTER / RETURN / empty string handling when no pause is pending
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("ENTER")
            || trimmed.eq_ignore_ascii_case("RETURN")
        {
            if self.app.tabs[self.tab].active_cmd.is_some() {
                let _ = self.app.feed_command(crate::command::StepInput::Enter);
                self.publish_document_view();
                return true;
            }
            return false;
        }

        // 4. Command execution. The command line's own driver owns this: it
        //    resolves the alias, tears down whatever was running, dispatches an
        //    inline-argument line as a whole (`CLAYER TEST`, `UCS ORIGIN 0,0`),
        //    and otherwise starts the tool and feeds it the rest of the tokens,
        //    PAUSE included. Leaving that here rather than repeating it keeps a
        //    scripted line and a typed one on the same path.
        let tab = self.tab;
        self.app.suppress_plugin_dispatch = true;
        let task = if self.app.tabs[tab].active_cmd.is_some() {
            // A running command owns these tokens: they answer its prompts.
            let mut queue: Vec<String> =
                trimmed.split_whitespace().map(str::to_string).collect();
            if has_newline {
                queue.push("ENTER".to_string());
            }
            self.app.tabs[tab].pending_pause_tokens = Some(queue);
            self.app.drain_pending_pause_tokens(tab)
        } else {
            self.app.run_command_line_streaming(trimmed, has_newline)
        };
        let _ = self.app.drive_headless_task(task);
        self.app.suppress_plugin_dispatch = false;

        self.app.refresh_layer_panel();
        self.publish_document_view();
        true
    }
}

/// True when block `outer` (transitively) contains an insert of `target`.
fn block_nests(doc: &CadDocument, outer: &str, target: &str, depth: usize) -> bool {
    if depth > 64 {
        return true;
    }
    let Some(record) = doc.block_records.get(outer) else {
        return false;
    };
    record.entity_handles.iter().filter_map(|h| doc.get_entity(*h)).any(|entity| match entity {
        EntityType::Insert(insert) => {
            insert.block_name.eq_ignore_ascii_case(target) || block_nests(doc, &insert.block_name, target, depth + 1)
        }
        _ => false,
    })
}

/// A layer name AutoCAD would accept: 1-255 characters, none of `<>/\":;?*|=\``.
fn validated_layer_name(raw: &str) -> Result<String, String> {
    validated_symbol_name(raw, "a layer name")
}

/// A symbol-table name AutoCAD would accept (layers, styles): 1-255 characters,
/// none of `<>/\":;?*|=` or a backquote, and no control characters.
fn validated_symbol_name(raw: &str, what: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(format!("{what} cannot be empty"));
    }
    if name.chars().count() > 255 {
        return Err(format!("{what} is limited to 255 characters"));
    }
    if let Some(bad) = name.chars().find(|c| "<>/\\\":;?*|=`".contains(*c) || c.is_control()) {
        return Err(format!("{what} cannot contain {bad:?}"));
    }
    Ok(name.to_owned())
}

fn apply_text_style_config(
    style: &mut acadrust::tables::TextStyle,
    config: &ocs_plugin_api::host::TextStyleConfig,
) -> Result<(), String> {
    let text = |what: &str, value: &str| {
        if value.chars().any(|c| c.is_control()) || value.chars().count() > 255 {
            Err(format!("{what} must be text of at most 255 characters"))
        } else {
            Ok(value.trim().to_owned())
        }
    };
    if let Some(height) = config.height {
        if !height.is_finite() || height < 0.0 {
            return Err("text style height must be finite and not negative (0 = variable)".to_owned());
        }
        style.height = height;
    }
    if let Some(width) = config.width_factor {
        if !width.is_finite() || width <= 0.0 || width > 100.0 {
            return Err("text style width factor must be greater than 0 and at most 100".to_owned());
        }
        style.width_factor = width;
    }
    if let Some(angle) = config.oblique_angle {
        if !angle.is_finite() || angle.abs() > 85f64.to_radians() + 1e-9 {
            return Err("text style oblique angle must be within 85 degrees either way".to_owned());
        }
        style.oblique_angle = angle;
    }
    if let Some(font) = &config.font_file {
        style.font_file = text("font file", font)?;
    }
    if let Some(font) = &config.big_font_file {
        style.big_font_file = text("big font file", font)?;
    }
    if let Some(font) = &config.true_type_font {
        style.true_type_font = text("TrueType font", font)?;
    }
    if let Some(value) = config.backward {
        style.flags.backward = value;
    }
    if let Some(value) = config.upside_down {
        style.flags.upside_down = value;
    }
    if let Some(value) = config.vertical {
        style.is_vertical = value;
    }
    if let Some(value) = config.annotative {
        style.annotative = value;
    }
    if style.font_file.is_empty() && style.true_type_font.is_empty() {
        return Err("a text style needs a font file or a TrueType font".to_owned());
    }
    Ok(())
}

/// Apply a JSON object of DimStyle fields on top of `base`. Handles and xref
/// fields belong to the host and the name is set separately, so those keys are
/// refused; unknown keys and wrongly typed values are refused too. `dimtxsty`
/// must name an existing text style, whose handle is linked here.
fn apply_dim_style_json(
    doc: &CadDocument,
    base: &acadrust::tables::DimStyle,
    properties: &str,
) -> Result<acadrust::tables::DimStyle, String> {
    let patch: serde_json::Value = serde_json::from_str(properties)
        .map_err(|e| format!("dimension style properties are not valid JSON: {e}"))?;
    let serde_json::Value::Object(patch) = patch else {
        return Err("dimension style properties must be an object".to_owned());
    };
    let mut value = serde_json::to_value(base).map_err(|e| e.to_string())?;
    let object = value.as_object_mut().ok_or("dimension style did not serialize to an object")?;
    for (key, new) in patch {
        if key == "handle" || key == "name" || key.starts_with("xref_") || key.ends_with("_handle") {
            return Err(format!("dimension style property {key:?} is managed by the host"));
        }
        if !object.contains_key(&key) {
            return Err(format!("unknown dimension style property {key:?}"));
        }
        object.insert(key, new);
    }
    let mut style: acadrust::tables::DimStyle = serde_json::from_value(value)
        .map_err(|e| format!("invalid dimension style value: {e}"))?;
    if !(style.dimscale.is_finite() && style.dimscale > 0.0) {
        return Err("dimscale must be greater than zero".to_owned());
    }
    if !(style.dimtxt.is_finite() && style.dimtxt > 0.0) {
        return Err("dimtxt (text height) must be greater than zero".to_owned());
    }
    if !(style.dimasz.is_finite() && style.dimasz >= 0.0) {
        return Err("dimasz (arrow size) cannot be negative".to_owned());
    }
    let Some(text_style) = doc.text_styles.get(&style.dimtxsty) else {
        return Err(format!("dimtxsty {:?} is not a text style in this drawing", style.dimtxsty));
    };
    style.dimtxsty = text_style.name.clone();
    style.dimtxsty_handle = text_style.handle;
    Ok(style)
}

/// The stored spelling of `name` in the drawing's linetype table, loading the
/// standard linetypes first when it is not there yet. `None` when the drawing
/// cannot supply it: a layer must not reference a linetype that does not exist.
fn resolve_linetype(doc: &mut acadrust::CadDocument, name: &str) -> Option<String> {
    let stored = |doc: &acadrust::CadDocument| doc.line_types.get(name).map(|lt| lt.name.clone());
    stored(doc).or_else(|| {
        crate::io::linetypes::populate_document(doc);
        stored(doc)
    })
}

/// The stable contract a plugin's `dispatch` sees. Each method forwards to the
/// inherent `HostSession` method of the same name (inherent methods take
/// resolution priority, so this is plain delegation, not recursion). The
/// per-tab plugin-state accessors expose the raw `Any` box; the typed
/// `ocs_plugin_api::host::plugin_state*` helpers wrap them.
impl HostApi for HostSession<'_> {
    fn tab_index(&self) -> usize {
        self.tab_index()
    }
    fn document(&self) -> &CadDocument {
        self.document()
    }
    fn document_mut(&mut self) -> &mut CadDocument {
        self.document_mut()
    }
    fn add_entity(&mut self, entity: EntityType) -> Handle {
        self.add_entity(entity)
    }
    fn add_entities(&mut self, entities: Vec<EntityType>) -> Vec<Handle> {
        self.add_entities(entities)
    }
    fn update_entity(&mut self, entity: EntityType) -> bool {
        self.update_entity(entity)
    }
    fn update_entities_transaction(
        &mut self,
        label: &str,
        entities: Vec<EntityType>,
    ) -> Result<(), String> {
        self.update_entities_transaction(label, entities)
    }
    fn selection(&self) -> Vec<Handle> {
        self.selection()
    }
    fn set_selection(&mut self, handles: &[Handle]) -> Result<(), String> {
        self.set_selection(handles)
    }
    fn solid_operation(
        &mut self,
        operation: ocs_plugin_api::host::SolidOperation,
    ) -> Result<Handle, String> {
        self.solid_operation(operation)
    }
    fn remove_entity(&mut self, handle: Handle) -> bool {
        self.remove_entity(handle)
    }
    fn bump_geometry(&mut self) {
        self.bump_geometry()
    }
    fn read_record(&self, handle: Handle, app_name: &str) -> Option<&ExtendedDataRecord> {
        self.read_record(handle, app_name)
    }
    fn write_record(&mut self, handle: Handle, record: ExtendedDataRecord) -> bool {
        self.write_record(handle, record)
    }
    fn remove_record(&mut self, handle: Handle, app_name: &str) -> bool {
        self.remove_record(handle, app_name)
    }
    fn push_undo(&mut self, label: &str) {
        self.push_undo(label)
    }
    fn set_dirty(&mut self) {
        self.set_dirty()
    }
    fn push_info(&mut self, msg: &str) {
        self.push_info(msg)
    }
    fn push_output(&mut self, msg: &str) {
        self.push_output(msg)
    }
    fn push_error(&mut self, msg: &str) {
        self.push_error(msg)
    }
    fn start_interactive(&mut self, command: Box<dyn ocs_plugin_api::host::InteractiveCommand>) {
        self.app.tabs[self.tab].active_cmd =
            Some(Box::new(PluginInteractiveAdapter { inner: command }));
    }
    fn plugin_state_any(&self, plugin_id: &str) -> Option<&(dyn Any + Send + Sync)> {
        self.app.tabs[self.tab]
            .plugin_state
            .get(plugin_id)
            .map(|b| b.as_ref())
    }
    fn plugin_state_any_mut(&mut self, plugin_id: &str) -> Option<&mut (dyn Any + Send + Sync)> {
        self.app.tabs[self.tab]
            .plugin_state
            .get_mut(plugin_id)
            .map(|b| b.as_mut())
    }
    fn ensure_plugin_state_any(
        &mut self,
        plugin_id: &'static str,
        init: &mut dyn FnMut() -> Box<dyn Any + Send + Sync>,
    ) -> &mut (dyn Any + Send + Sync) {
        self.app.tabs[self.tab]
            .plugin_state
            .entry(plugin_id)
            .or_insert_with(|| init())
            .as_mut()
    }
    fn document_reader(&self) -> Box<dyn ocs_plugin_api::host::DocumentReader + '_> {
        Box::new(ocs_plugin_api::host::CadDocumentReader(self.document()))
    }
    fn document_view(&mut self) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        self.document_view()
    }
    fn tab_id(&self) -> u64 {
        self.tab_id()
    }
    fn document_path(&self, tab_id: u64) -> Option<std::path::PathBuf> {
        self.document_path(tab_id)
    }
    fn system_variable(&self, name: &str) -> Option<HostSettingValue> {
        self.system_variable(name)
    }
    fn set_system_variable(
        &mut self,
        name: &str,
        value: HostSettingValue,
    ) -> Result<HostSettingValue, String> {
        self.set_system_variable(name, value)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn document_view_v4(&mut self, tab_id: u64) -> Option<ocs_plugin_api::shm::DocumentViewInfo> {
        self.document_view_v4(tab_id)
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn close_document_view_v4(&mut self, tab_id: u64) {
        self.close_document_view_v4(tab_id)
    }
    fn add_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> Option<Handle> {
        self.add_layer(config)
    }
    fn modify_layer(&mut self, config: ocs_plugin_api::host::LayerConfig) -> bool {
        self.modify_layer(config)
    }
    fn execute_command(&mut self, cmd: &str) -> bool {
        self.execute_command(cmd)
    }
    fn table_operation(&mut self, operation: ocs_plugin_api::host::TableOperation) -> Result<Handle, String> {
        self.table_operation(operation)
    }
    fn run_command(&mut self, request: ocs_plugin_api::host::CommandRequest) -> Result<ocs_plugin_api::host::CommandOutcome, String> {
        self.run_command(request)
    }
}

/// Bridges a plugin's [`InteractiveCommand`](ocs_plugin_api::host::InteractiveCommand)
/// to the host's internal `CadCommand`, so a plugin tool drives the host's
/// point-collection flow (viewport clicks or `--serve` coordinates) just like a
/// built-in tool.
struct PluginInteractiveAdapter {
    inner: Box<dyn ocs_plugin_api::host::InteractiveCommand>,
}

impl crate::command::CadCommand for PluginInteractiveAdapter {
    fn name(&self) -> &'static str {
        "PLUGIN"
    }
    // Every call into the plugin runs under a panic guard (#145): a buggy plugin
    // that panics mid-command leaves the host running — the command just ends.
    fn prompt(&self) -> String {
        crate::plugin::guard("InteractiveCommand::prompt", || self.inner.prompt())
            .unwrap_or_default()
    }
    fn on_point(&mut self, pt: glam::DVec3) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_point", || {
            self.inner.on_point([pt.x as f64, pt.y as f64, pt.z as f64])
        })
        .map(plugin_step_to_result)
        .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn on_enter(&mut self) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_enter", || self.inner.on_enter())
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel)
    }
    fn needs_entity_pick(&self) -> bool {
        crate::plugin::guard("InteractiveCommand::needs_object_pick", || {
            self.inner.needs_object_pick()
        })
        .unwrap_or(false)
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: glam::DVec3) -> crate::command::CmdResult {
        crate::plugin::guard("InteractiveCommand::on_object_pick", || {
            self.inner
                .on_object_pick(handle, [pt.x as f64, pt.y as f64, pt.z as f64])
        })
        .map(plugin_step_to_result)
        .unwrap_or(crate::command::CmdResult::Cancel)
    }
}

/// Bridges an out-of-process plugin's interactive command to the host's
/// `CadCommand`. Events are sent over IPC and the returned `CommandStep` is
/// translated into a `CmdResult`. Prompt and object-pick mode are cached and
/// refreshed after each event.
pub(crate) struct PluginProcessInteractiveAdapter {
    pub process: std::sync::Arc<ocs_plugin_api::process::PluginProcess>,
    pub command_id: u64,
    prompt: Option<String>,
    needs_entity_pick: Option<bool>,
    is_done: bool,
}

impl PluginProcessInteractiveAdapter {
    pub(crate) fn new(
        process: std::sync::Arc<ocs_plugin_api::process::PluginProcess>,
        command_id: u64,
    ) -> Self {
        let prompt = process.get_prompt(command_id).ok();
        let needs_entity_pick = process.needs_entity_pick(command_id).ok();
        Self {
            process,
            command_id,
            prompt,
            needs_entity_pick,
            is_done: false,
        }
    }

    fn refresh(&mut self) {
        self.prompt = self.process.get_prompt(self.command_id).ok();
        self.needs_entity_pick = self.process.needs_entity_pick(self.command_id).ok();
    }

    fn cancel(&mut self) {
        if !self.is_done {
            self.is_done = true;
            use ocs_plugin_api::ipc::protocol::InteractiveEvent;
            let _ = self.process.interactive_event(self.command_id, InteractiveEvent::Cancel);
        }
    }
}

impl Drop for PluginProcessInteractiveAdapter {
    fn drop(&mut self) {
        // Tell the command it was cancelled before the runner forgets it:
        // a plugin waiting on a step would otherwise never be released.
        self.cancel();
        let _ = self.process.drop_interactive(self.command_id);
    }
}

impl crate::command::CadCommand for PluginProcessInteractiveAdapter {
    fn name(&self) -> &'static str {
        "PLUGIN"
    }
    fn prompt(&self) -> String {
        self.prompt.clone().unwrap_or_default()
    }
    fn on_point(&mut self, pt: glam::DVec3) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(self.command_id, InteractiveEvent::Point([pt.x, pt.y, pt.z]))
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(result, crate::command::CmdResult::CommitAndExit(_) | crate::command::CmdResult::Cancel) {
            self.is_done = true;
        }
        self.refresh();
        result
    }
    fn on_enter(&mut self) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(self.command_id, InteractiveEvent::Enter)
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(result, crate::command::CmdResult::CommitAndExit(_) | crate::command::CmdResult::Cancel) {
            self.is_done = true;
        }
        self.refresh();
        result
    }
    fn on_escape(&mut self) -> crate::command::CmdResult {
        self.cancel();
        crate::command::CmdResult::Cancel
    }
    fn needs_entity_pick(&self) -> bool {
        self.needs_entity_pick.unwrap_or(false)
    }
    fn on_entity_pick(&mut self, handle: Handle, pt: glam::DVec3) -> crate::command::CmdResult {
        use ocs_plugin_api::ipc::protocol::InteractiveEvent;
        let result = self
            .process
            .interactive_event(
                self.command_id,
                InteractiveEvent::ObjectPick {
                    handle,
                    pt: [pt.x, pt.y, pt.z],
                },
            )
            .map(plugin_step_to_result)
            .unwrap_or(crate::command::CmdResult::Cancel);
        if matches!(result, crate::command::CmdResult::CommitAndExit(_) | crate::command::CmdResult::Cancel) {
            self.is_done = true;
        }
        self.refresh();
        result
    }
}

fn plugin_step_to_result(step: ocs_plugin_api::host::CommandStep) -> crate::command::CmdResult {
    use crate::command::CmdResult;
    use ocs_plugin_api::host::CommandStep;
    match step {
        CommandStep::NeedPoint => CmdResult::NeedPoint,
        CommandStep::Commit(e) => CmdResult::CommitEntity(e),
        CommandStep::CommitAndEnd(e) => CmdResult::CommitAndExit(e),
        CommandStep::Done | CommandStep::Cancel => CmdResult::Cancel,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use crate::entities::traits::RenderConvertible;
    use acadrust::entities::{Line, Point};
    use acadrust::xdata::XDataValue;
    use ocs_plugin_api::host::DocumentReader;

    #[test]
    fn selection_query_is_live_and_replacement_is_validated() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let a = host.add_entity(EntityType::Point(Point::new()));
        let b = host.add_entity(EntityType::Point(Point::new()));
        assert!(host.selection().is_empty());
        host.set_selection(&[b, a]).unwrap();
        assert_eq!(host.selection(), vec![b, a]);
        assert!(host.set_selection(&[Handle::new(99999)]).is_err());
        assert!(host.set_selection(&[a, a]).is_err());
        assert_eq!(host.selection(), vec![b, a]);
        host.set_selection(&[]).unwrap();
        assert!(host.selection().is_empty());
    }

    #[test]
    fn nested_insert_attribute_crud_uses_parent_storage_and_undo() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let (insert_handle, attribute_handle);
        {
            let mut host = HostSession::new(&mut app, 0);
            let next = host.document().next_handle();
            let record_handle = Handle::new(next);
            let block_handle = Handle::new(next + 1);
            let block_end_handle = Handle::new(next + 2);
            let mut record = acadrust::tables::BlockRecord::new("TAGBLOCK");
            record.handle = record_handle;
            record.block_entity_handle = block_handle;
            record.block_end_handle = block_end_handle;
            host.document_mut().block_records.add(record).unwrap();
            let mut block =
                acadrust::entities::Block::new("TAGBLOCK", acadrust::types::Vector3::ZERO);
            block.common.handle = block_handle;
            block.common.owner_handle = record_handle;
            host.document_mut()
                .add_entity(EntityType::Block(block))
                .unwrap();
            let mut end = acadrust::entities::BlockEnd::new();
            end.common.handle = block_end_handle;
            end.common.owner_handle = record_handle;
            host.document_mut()
                .add_entity(EntityType::BlockEnd(end))
                .unwrap();
            let mut definition = acadrust::entities::AttributeDefinition::new(
                "PART_NO".into(),
                "Part number".into(),
                "PN-001".into(),
            );
            definition.common.owner_handle = record_handle;
            let definition_handle = host
                .document_mut()
                .add_entity(EntityType::AttributeDefinition(definition))
                .unwrap();
            insert_handle = host
                .document_mut()
                .add_entity(EntityType::Insert(acadrust::entities::Insert::new(
                    "TAGBLOCK",
                    acadrust::types::Vector3::new(10.0, 0.0, 0.0),
                )))
                .unwrap();

            let mut attribute =
                acadrust::entities::AttributeEntity::new("PART_NO".into(), "PN-101".into());
            attribute.common.owner_handle = insert_handle;
            attribute.attdef_handle = definition_handle;
            attribute.insertion_point = acadrust::types::Vector3::new(10.0, 2.0, 0.0);
            host.push_undo("Create attribute");
            attribute_handle = host.add_entity(EntityType::AttributeEntity(attribute));
            assert!(!attribute_handle.is_null());
            assert!(
                host.document().get_entity(attribute_handle).is_none(),
                "nested attributes must not become orphan flat entities"
            );
            assert!(matches!(host.host_model_entity(attribute_handle),
                Some(EntityType::AttributeEntity(value)) if value.value == "PN-101"));
            let view = ocs_plugin_api::shm::DocumentViewDataV4::from(host.document());
            assert!(view
                .entities
                .iter()
                .any(|entity| entity.handle == attribute_handle.value()));

            let mut changed = host.host_model_entity(attribute_handle).unwrap();
            let EntityType::AttributeEntity(value) = &mut changed else {
                unreachable!()
            };
            value.value = "PN-102".into();
            value.insertion_point.x = 12.0;
            host.update_entities_transaction("Edit attribute", vec![changed])
                .unwrap();
            assert!(matches!(host.host_model_entity(attribute_handle),
                Some(EntityType::AttributeEntity(value))
                    if value.value == "PN-102" && value.insertion_point.x == 12.0));

            host.push_undo("Delete attribute");
            assert!(host.remove_entity(attribute_handle));
            assert!(host.host_model_entity(attribute_handle).is_none());
        }
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.len() == 1
                && insert.attributes[0].value == "PN-102")
        );
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes[0].value == "PN-101")
        );
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.is_empty())
        );
        app.redo_steps(3);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.is_empty())
        );
    }

    #[test]
    fn scripted_selection_does_not_expand_linked_leader_annotation() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let annotation = host.add_entity(EntityType::Text(acadrust::entities::Text::default()));
        let mut leader = acadrust::entities::Leader::default();
        leader.annotation_handle = annotation;
        let leader_handle = host.add_entity(EntityType::Leader(leader));
        host.set_selection(&[leader_handle]).unwrap();
        assert_eq!(host.selection(), vec![leader_handle]);
    }

    #[test]
    fn entity_transaction_validates_before_mutation_and_undoes_as_one_step() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let (first, second);
        {
            let mut host = HostSession::new(&mut app, 0);
            first = host.add_entity(EntityType::Point(Point::at(acadrust::types::Vector3::new(
                1.0, 0.0, 0.0,
            ))));
            second = host.add_entity(EntityType::Point(Point::at(acadrust::types::Vector3::new(
                2.0, 0.0, 0.0,
            ))));
            let mut a = host.document().get_entity(first).unwrap().clone();
            let mut b = host.document().get_entity(second).unwrap().clone();
            if let EntityType::Point(p) = &mut a {
                p.location.x = 10.0;
            }
            if let EntityType::Point(p) = &mut b {
                p.location.x = 20.0;
            }
            let mut invalid = b.clone();
            invalid.common_mut().owner_handle = Handle::new(99999);
            assert!(host
                .update_entities_transaction("Move points", vec![a.clone(), invalid])
                .is_err());
            let mut invalid_geometry = b.clone();
            if let EntityType::Point(p) = &mut invalid_geometry {
                p.location.x = f64::NAN;
            }
            assert!(host
                .update_entities_transaction("Move points", vec![a.clone(), invalid_geometry])
                .unwrap_err()
                .contains("Point.location"));
            assert_eq!(host.app.tabs[0].history.undo_stack.len(), 0);
            assert!(
                matches!(host.document().get_entity(first), Some(EntityType::Point(p)) if p.location.x == 1.0)
            );
            host.update_entities_transaction("Move points", vec![a, b])
                .unwrap();
            assert!(
                matches!(host.document().get_entity(first), Some(EntityType::Point(p)) if p.location.x == 10.0)
            );
        }
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(first), Some(EntityType::Point(p)) if p.location.x == 1.0)
        );
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(second), Some(EntityType::Point(p)) if p.location.x == 2.0)
        );
        app.redo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(first), Some(EntityType::Point(p)) if p.location.x == 10.0)
        );
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(second), Some(EntityType::Point(p)) if p.location.x == 20.0)
        );
    }

    /// Opt-in end-to-end check using the staged Python cdylib and the actual
    /// OCS executable as its out-of-process runner. Run with OCS_TEST_PYTHON_PLUGIN
    /// and OCS_PLUGIN_RUNNER_EXE set; unlike the local-socket protocol test,
    /// this executes Python and calls the app's HostSession over real IPC.
    #[test]
    fn staged_python_plugin_line_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let plugin_path = std::path::PathBuf::from(plugin_path);
        assert!(
            plugin_path.is_file(),
            "missing staged Python plugin: {}",
            plugin_path.display()
        );
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());

        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            &plugin_path,
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .expect("spawn staged Python plugin");
        assert_eq!(process.id(), "opencad.python");
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch"));
        };
        dispatch(
            &mut host,
            concat!(
            "PY_EVAL ocs.active_document.create_entity('Line',",
            "start={'x':0.0,'y':0.0,'z':0.0},",
            "end={'x':1.0,'y':0.0,'z':0.0}).handle"
            ),
        );
        let handles: Vec<_> = host
            .document()
            .entities()
            .map(|entity| entity.common().handle)
            .collect();
        assert_eq!(handles.len(), 1, "Python add did not reach the host");
        let handle = handles[0];
        assert!(
            matches!(host.document().get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 1.0)
        );
        let before_edit = host.document().get_entity(handle).unwrap().clone();
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Move line', [{{'handle':{},'end':{{'x':5.0,'y':0.0,'z':0.0}}}}])",
            handle.value()
        ));
        let mut expected_after_edit = before_edit;
        let EntityType::Line(expected_line) = &mut expected_after_edit else {
            unreachable!()
        };
        expected_line.end.x = 5.0;
        assert_eq!(
            host.document().get_entity(handle),
            Some(&expected_after_edit),
            "partial Python edit must preserve every unmentioned common and geometry field"
        );
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Reject NaN', [{{'handle':{},'start':{{'x':float('nan'),'y':0.0,'z':0.0}}}}])",
            handle.value()
        ));
        assert_eq!(
            host.document().get_entity(handle),
            Some(&expected_after_edit),
            "invalid geometry changed the line"
        );
        assert!(host
            .app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("finite coordinates"));
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Reject duplicate', [{{'handle':{},'end':{{'x':8.0,'y':0.0,'z':0.0}}}},{{'handle':{},'end':{{'x':9.0,'y':0.0,'z':0.0}}}}])",
            handle.value(), handle.value()
        ));
        assert!(
            matches!(host.document().get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 5.0),
            "rejected batch changed the line"
        );
        let dwg_bytes =
            acadrust::DwgWriter::write_to_vec(host.document()).expect("write edited DWG");
        let dwg_doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(dwg_bytes))
            .read()
            .expect("reopen edited DWG");
        assert!(
            matches!(dwg_doc.get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 5.0)
        );
        let dxf_bytes = acadrust::DxfWriter::new(host.document())
            .write_to_vec()
            .expect("write edited DXF");
        let dxf_doc = acadrust::DxfReader::from_reader(std::io::Cursor::new(dxf_bytes))
            .expect("open edited DXF")
            .read()
            .expect("reopen edited DXF");
        assert!(
            matches!(dxf_doc.get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 5.0)
        );
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.delete_entity({})",
                handle.value()
            ),
        );
        assert!(
            host.document().get_entity(handle).is_none(),
            "Python removal did not reach the host"
        );
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(
            app.tabs[0].history.undo_stack.len(),
            3,
            "create, edit, and delete should each form one undo entry"
        );
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 5.0)
        );
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Line(line)) if line.end.x == 1.0)
        );
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn staged_python_tolerance_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .expect("spawn staged Python plugin");
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch"));
        };
        dispatch(
            &mut host,
            concat!(
            "PY_EVAL ocs.active_document.create_entity('Tolerance',",
            "insertion_point={'x':1.0,'y':2.0,'z':0.0},",
            "text='POSITION%%v0.1').handle"
            ),
        );
        let handle = host
            .document()
            .entities()
            .next()
            .expect("Python created a Tolerance")
            .common()
            .handle;
        let Some(EntityType::Tolerance(created)) = host.document().get_entity(handle) else {
            panic!("expected Tolerance");
        };
        assert_eq!(
            created.insertion_point,
            acadrust::types::Vector3::new(1.0, 2.0, 0.0)
        );
        assert_eq!(created.dimension_style_name, "Standard");
        let created = created.clone();
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.entities[{}].text",
                handle.value()
            ),
        );
        assert!(host
            .app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("POSITION%%v0.1"));

        let script_path = std::env::temp_dir().join(format!(
            "ocs_tolerance_document_model_{}.py",
            std::process::id()
        ));
        std::fs::write(
            &script_path,
            format!(
                concat!(
            "doc = ocs.active_document\n",
            "tolerance = doc.entities[{}]\n",
            "with doc.transaction('Edit tolerance'):\n",
            "    tolerance.insertion_point = (4.0, 5.0, 0.0)\n",
            "    tolerance.direction = (0.0, 1.0, 0.0)\n",
            "    tolerance.text = 'POSITION%%v0.2'\n",
            "doc.selection = [tolerance]\n"
                ),
                handle.value()
            ),
        )
        .unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script_path.display()));
        let _ = std::fs::remove_file(&script_path);
        let expected = host.document().get_entity(handle).unwrap().clone();
        let EntityType::Tolerance(edited) = &expected else {
            unreachable!()
        };
        assert_eq!(
            edited.insertion_point,
            acadrust::types::Vector3::new(4.0, 5.0, 0.0)
        );
        assert_eq!(edited.direction, acadrust::types::Vector3::UNIT_Y);
        assert_eq!(edited.text, "POSITION%%v0.2");
        assert_eq!(edited.common, created.common);
        assert_eq!(edited.normal, created.normal);
        assert_eq!(edited.dimension_style_name, created.dimension_style_name);
        assert_eq!(
            edited.dimension_style_handle,
            created.dimension_style_handle
        );
        assert_eq!(edited.text_height, created.text_height);
        assert_eq!(edited.dimension_gap, created.dimension_gap);
        assert_eq!(edited.dwg_unknown_short, created.dwg_unknown_short);
        assert_eq!(
            host.selection(),
            vec![handle],
            "Python selection did not reach the canvas"
        );

        dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Reject direction', [{{'handle':{},'direction':{{'x':2.0,'y':0.0,'z':0.0}}}}])",
            handle.value()));
        assert_eq!(host.document().get_entity(handle), Some(&expected));
        assert!(host
            .app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("unit vector"));
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Reject style', [{{'handle':{},'dimension_style_name':'Missing'}}])",
            handle.value()));
        assert_eq!(host.document().get_entity(handle), Some(&expected));
        assert!(host
            .app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("does not exist"));

        let dwg_bytes = acadrust::DwgWriter::write_to_vec(host.document()).unwrap();
        let dwg_doc = acadrust::DwgReader::from_stream(std::io::Cursor::new(dwg_bytes))
            .read()
            .unwrap();
        assert!(
            matches!(dwg_doc.get_entity(handle), Some(EntityType::Tolerance(value))
            if value.insertion_point.x == 4.0 && value.text == "POSITION%%v0.2")
        );
        let dxf_bytes = acadrust::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap();
        let dxf_doc = acadrust::DxfReader::from_reader(std::io::Cursor::new(dxf_bytes))
            .unwrap()
            .read()
            .unwrap();
        assert!(
            matches!(dxf_doc.get_entity(handle), Some(EntityType::Tolerance(value))
            if value.insertion_point.x == 4.0 && value.text == "POSITION%%v0.2")
        );

        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.delete_entity({})",
                handle.value()
            ),
        );
        assert!(host.document().get_entity(handle).is_none());
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert_eq!(
            app.tabs[0].scene.document.get_entity(handle),
            Some(&expected)
        );
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Tolerance(value))
            if value.insertion_point.x == 1.0 && value.text == "POSITION%%v0.1")
        );
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn staged_python_shape_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());

        let shx_path =
            std::env::temp_dir().join(format!("ocs_host_shape_{}.shx", std::process::id()));
        let mut shx = b"AutoCAD-86 shapes 1.0\r\n\x1A".to_vec();
        for value in [1_u16, 1, 1, 1, 9] {
            shx.extend_from_slice(&value.to_le_bytes());
        }
        shx.extend_from_slice(b"ARROW\0\x10\x14\0");
        std::fs::write(&shx_path, shx).unwrap();

        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let style_handle = host.document_mut().allocate_handle();
        let mut style = acadrust::tables::TextStyle::new("TestShapes");
        style.handle = style_handle;
        style.is_shape_file = true;
        style.font_file = shx_path.to_string_lossy().into_owned();
        host.document_mut().text_styles.add(style).unwrap();

        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .expect("spawn staged Python plugin");
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch"));
        };
        dispatch(
            &mut host,
            concat!(
            "PY_EVAL ocs.active_document.create_entity('Shape',",
            "insertion_point={'x':1.0,'y':2.0,'z':0.0},size=2.0,",
            "shape_name='ARROW',shape_number=1,style_name='TestShapes').handle"
            ),
        );
        let handle = host
            .document()
            .entities()
            .next()
            .expect("Python created a Shape")
            .common()
            .handle;
        let Some(EntityType::Shape(created)) = host.document().get_entity(handle) else {
            panic!("expected Shape");
        };
        assert_eq!(created.style_handle, Some(style_handle));
        let created = created.clone();
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.entities[{}].shape_name",
                handle.value()
            ),
        );
        assert!(host
            .app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("ARROW"));

        let script_path = std::env::temp_dir().join(format!(
            "ocs_shape_document_model_{}.py",
            std::process::id()
        ));
        std::fs::write(
            &script_path,
            format!(
                concat!(
            "doc = ocs.active_document\n",
            "shape = doc.entities[{}]\n",
            "with doc.transaction('Edit shape'):\n",
            "    shape.insertion_point = (4.0, 5.0, 0.0)\n",
            "    shape.size = 3.0\n",
            "    shape.rotation = 0.5\n",
            "    shape.relative_x_scale = 1.5\n",
            "doc.selection = [shape]\n"
                ),
                handle.value()
            ),
        )
        .unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script_path.display()));
        let _ = std::fs::remove_file(&script_path);
        let expected = host.document().get_entity(handle).unwrap().clone();
        let EntityType::Shape(edited) = &expected else {
            unreachable!()
        };
        assert_eq!(
            edited.insertion_point,
            acadrust::types::Vector3::new(4.0, 5.0, 0.0)
        );
        assert_eq!(edited.size, 3.0);
        assert_eq!(edited.rotation, 0.5);
        assert_eq!(edited.relative_x_scale, 1.5);
        assert_eq!(edited.common, created.common);
        assert_eq!(edited.shape_name, created.shape_name);
        assert_eq!(edited.shape_number, created.shape_number);
        assert_eq!(edited.style_name, created.style_name);
        assert_eq!(edited.style_handle, created.style_handle);
        assert_eq!(edited.normal, created.normal);
        assert_eq!(edited.thickness, created.thickness);
        assert_eq!(host.selection(), vec![handle]);

        let assert_real_glyph = |entity: &EntityType, document: &CadDocument| {
            let EntityType::Shape(shape) = entity else {
                panic!("expected Shape");
            };
            let render = shape.to_render(document).expect("shape render");
            let crate::scene::convert::acad_to_render::RenderObject::Lines(points) = render.object
            else {
                panic!("expected shape linework");
            };
            assert_eq!(
                points.len(),
                3,
                "expected SHX glyph rather than diamond placeholder"
            );
            assert!(points.iter().flatten().all(|value| value.is_finite()));
        };
        assert_real_glyph(&expected, host.document());

        dispatch(
            &mut host,
            &format!(
            "PY_EVAL ocs.update_many('Reject size', [{{'handle':{},'size':0.0}}])",
                handle.value()
            ),
        );
        assert_eq!(host.document().get_entity(handle), Some(&expected));
        dispatch(
            &mut host,
            &format!(
            "PY_EVAL ocs.update_many('Reject style', [{{'handle':{},'style_name':'Missing'}}])",
                handle.value()
            ),
        );
        assert_eq!(host.document().get_entity(handle), Some(&expected));
        assert!(host
            .app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("does not exist"));

        let dwg_bytes = acadrust::DwgWriter::write_to_vec(host.document()).unwrap();
        let dwg_doc = crate::io::load_bytes("shape.dwg", dwg_bytes).unwrap();
        let dwg_entity = dwg_doc.get_entity(handle).expect("Shape survives DWG");
        assert!(matches!(dwg_entity, EntityType::Shape(value)
            if value.shape_number == 1 && value.size == 3.0 && value.rotation == 0.5));
        assert_real_glyph(dwg_entity, &dwg_doc);
        let dxf_bytes = acadrust::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap();
        let dxf_doc = crate::io::load_bytes("shape.dxf", dxf_bytes).unwrap();
        let dxf_entity = dxf_doc.get_entity(handle).expect("Shape survives DXF");
        assert!(matches!(dxf_entity, EntityType::Shape(value)
            if value.shape_name == "ARROW" && value.size == 3.0 && (value.rotation - 0.5).abs() < 1e-12));
        assert_real_glyph(dxf_entity, &dxf_doc);

        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.delete_entity({})",
                handle.value()
            ),
        );
        assert!(host.document().get_entity(handle).is_none());
        drop(process);
        drop(host);
        let _ = std::fs::remove_file(&shx_path);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert_eq!(
            app.tabs[0].scene.document.get_entity(handle),
            Some(&expected)
        );
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Shape(value))
            if value.insertion_point.x == 1.0 && value.size == 2.0)
        );
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn staged_python_attribute_definition_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());

        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let next = host.document().next_handle();
        let block_record_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let block_end_handle = Handle::new(next + 2);
        let mut block_record = acadrust::tables::BlockRecord::new("TAGBLOCK");
        block_record.handle = block_record_handle;
        block_record.block_entity_handle = block_handle;
        block_record.block_end_handle = block_end_handle;
        host.document_mut().block_records.add(block_record).unwrap();
        let mut block = acadrust::entities::Block::new("TAGBLOCK", acadrust::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = block_record_handle;
        host.document_mut()
            .add_entity(EntityType::Block(block))
            .unwrap();
        let mut block_end = acadrust::entities::BlockEnd::new();
        block_end.common.handle = block_end_handle;
        block_end.common.owner_handle = block_record_handle;
        host.document_mut()
            .add_entity(EntityType::BlockEnd(block_end))
            .unwrap();
        let insert = acadrust::entities::Insert::new(
            "TAGBLOCK",
            acadrust::types::Vector3::new(10.0, 0.0, 0.0),
        );
        let insert_handle = host
            .document_mut()
            .add_entity(EntityType::Insert(insert))
            .unwrap();

        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .expect("spawn staged Python plugin");
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch"));
        };
        dispatch(
            &mut host,
            &format!(
                concat!(
            "PY_EVAL ocs.active_document.create_entity('AttributeDefinition',",
            "owner_handle={},tag='PART_NO',prompt='Part number',default_value='PN-001',",
            "insertion_point={{'x':1.0,'y':2.0,'z':0.0}},height=2.5).handle"
                ),
                block_record_handle.value()
            ),
        );
        let handle = host
            .document()
            .entities()
            .find(|entity| matches!(entity, EntityType::AttributeDefinition(_)))
            .expect("Python created an AttributeDefinition")
            .common()
            .handle;
        let Some(EntityType::AttributeDefinition(created)) = host.document().get_entity(handle)
        else {
            panic!("expected AttributeDefinition");
        };
        assert_eq!(created.common.owner_handle, block_record_handle);
        assert_eq!(created.tag, "PART_NO");
        assert_eq!(created.text_style, "Standard");
        assert!(host
            .document()
            .block_records
            .get("TAGBLOCK")
            .unwrap()
            .entity_handles
            .contains(&handle));
        let created = created.clone();
        dispatch(&mut host, &format!(
            "PY_EVAL (ocs.active_document.entities[{}].tag, ocs.active_document.entities[{}].owner_handle)",
            handle.value(), handle.value()));
        let readback = &host.app.command_line.history.last().unwrap().text;
        assert!(
            readback.contains("PART_NO")
                && readback.contains(&block_record_handle.value().to_string())
        );

        let script_path = std::env::temp_dir().join(format!(
            "ocs_attribute_definition_document_model_{}.py",
            std::process::id()
        ));
        std::fs::write(
            &script_path,
            format!(
                concat!(
            "doc = ocs.active_document\n",
            "definition = doc.entities[{}]\n",
            "with doc.transaction('Edit attribute definition'):\n",
            "    definition.insertion_point = (4.0, 5.0, 0.0)\n",
            "    definition.default_value = 'PN-002'\n",
            "    definition.rotation = 0.25\n",
            "    definition.width_factor = 1.25\n",
            "doc.selection = [definition]\n"
                ),
                handle.value()
            ),
        )
        .unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script_path.display()));
        let _ = std::fs::remove_file(&script_path);
        let expected = host.document().get_entity(handle).unwrap().clone();
        let EntityType::AttributeDefinition(edited) = &expected else {
            unreachable!()
        };
        assert_eq!(
            edited.insertion_point,
            acadrust::types::Vector3::new(4.0, 5.0, 0.0)
        );
        assert_eq!(edited.default_value, "PN-002");
        assert_eq!(edited.rotation, 0.25);
        assert_eq!(edited.width_factor, 1.25);
        assert_eq!(edited.common, created.common);
        assert_eq!(edited.tag, created.tag);
        assert_eq!(edited.prompt, created.prompt);
        assert_eq!(edited.text_style, created.text_style);
        assert_eq!(edited.flags, created.flags);
        assert_eq!(edited.embedded_mtext, created.embedded_mtext);
        assert_eq!(host.selection(), vec![handle]);
        assert!(
            matches!(host.document().get_entity(insert_handle), Some(EntityType::Insert(value))
            if value.block_name == "TAGBLOCK")
        );

        let render = edited
            .to_render(host.document())
            .expect("attribute definition render");
        let crate::scene::convert::acad_to_render::RenderObject::Text(strokes) = render.object
        else {
            panic!("expected attribute definition text");
        };
        assert!(!strokes.is_empty());
        assert!(strokes.iter().any(|stroke| stroke
            .run
            .as_ref()
            .is_some_and(|run| run.text.contains("PN-002"))));

        for (label, patch, message) in [
            ("Reject tag", "'tag':'BAD TAG'", "whitespace"),
            ("Reject height", "'height':0.0", "greater than zero"),
            ("Reject style", "'text_style':'Missing'", "does not exist"),
            ("Reject owner", "'owner_handle':999999", "read-only"),
        ] {
            dispatch(
                &mut host,
                &format!(
                    "PY_EVAL ocs.update_many({label:?}, [{{'handle':{}, {patch}}}])",
                    handle.value()
                ),
            );
            assert_eq!(host.document().get_entity(handle), Some(&expected));
            assert!(host
                .app
                .command_line
                .history
                .last()
                .unwrap()
                .text
                .contains(message));
        }

        let assert_persisted = |label: &str, document: &CadDocument, full_text_state: bool| {
            let actual = document.get_entity(handle);
            assert!(
                matches!(actual, Some(EntityType::AttributeDefinition(value))
                if value.common.owner_handle == block_record_handle
                    && value.tag == "PART_NO" && value.default_value == "PN-002"
                    && value.insertion_point == acadrust::types::Vector3::new(4.0, 5.0, 0.0)
                    && (value.rotation - 0.25).abs() < 1e-12),
                "{label}: {actual:?}"
            );
            if full_text_state {
                assert!(
                    matches!(actual, Some(EntityType::AttributeDefinition(value))
                    if value.width_factor == 1.25 && value.text_style == "Standard"
                        && value.prompt == "Part number"),
                    "{label}: {actual:?}"
                );
            }
            assert!(document
                .block_records
                .get("TAGBLOCK")
                .unwrap()
                .entity_handles
                .contains(&handle));
            assert!(
                matches!(document.get_entity(insert_handle), Some(EntityType::Insert(value))
                if value.block_name == "TAGBLOCK")
            );
        };
        let dwg_bytes = acadrust::DwgWriter::write_to_vec(host.document()).unwrap();
        let dwg_doc = crate::io::load_bytes("attribute-definition.dwg", dwg_bytes).unwrap();
        assert_persisted("DWG", &dwg_doc, true);
        let dxf_bytes = acadrust::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap();
        let dxf_doc = crate::io::load_bytes("attribute-definition.dxf", dxf_bytes).unwrap();
        // The pinned cadcodec DXF ATTDEF reader retains identity, placement,
        // height, value, prompt and rotation, but currently drops several
        // optional AcDbText fields such as width factor. Keep that external
        // codec gap explicit in the coverage ledger rather than claiming a
        // complete DXF property round trip here.
        assert_persisted("DXF", &dxf_doc, false);

        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.delete_entity({})",
                handle.value()
            ),
        );
        assert!(host.document().get_entity(handle).is_none());
        // CadDocument intentionally leaves the block membership slot in place
        // so delta undo can restore the shared entity without rewriting the
        // record. Writers must still omit the absent entity.
        let deleted_dwg = acadrust::DwgWriter::write_to_vec(host.document()).unwrap();
        assert!(
            crate::io::load_bytes("attribute-definition-deleted.dwg", deleted_dwg)
                .unwrap()
                .get_entity(handle)
                .is_none()
        );
        let deleted_dxf = acadrust::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap();
        assert!(
            crate::io::load_bytes("attribute-definition-deleted.dxf", deleted_dxf)
                .unwrap()
                .get_entity(handle)
                .is_none()
        );
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert_eq!(
            app.tabs[0].scene.document.get_entity(handle),
            Some(&expected)
        );
        app.undo_steps(1);
        assert!(matches!(app.tabs[0].scene.document.get_entity(handle),
            Some(EntityType::AttributeDefinition(value))
                if value.default_value == "PN-001" && value.insertion_point.x == 1.0));
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn staged_python_attribute_entity_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let next = host.document().next_handle();
        let record_handle = Handle::new(next);
        let block_handle = Handle::new(next + 1);
        let end_handle = Handle::new(next + 2);
        let mut record = acadrust::tables::BlockRecord::new("TAGBLOCK");
        record.handle = record_handle;
        record.block_entity_handle = block_handle;
        record.block_end_handle = end_handle;
        host.document_mut().block_records.add(record).unwrap();
        let mut block = acadrust::entities::Block::new("TAGBLOCK", acadrust::types::Vector3::ZERO);
        block.common.handle = block_handle;
        block.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::Block(block))
            .unwrap();
        let mut end = acadrust::entities::BlockEnd::new();
        end.common.handle = end_handle;
        end.common.owner_handle = record_handle;
        host.document_mut()
            .add_entity(EntityType::BlockEnd(end))
            .unwrap();
        let mut definition = acadrust::entities::AttributeDefinition::new(
            "PART_NO".into(),
            "Part number".into(),
            "PN-001".into(),
        );
        definition.common.owner_handle = record_handle;
        definition.insertion_point = acadrust::types::Vector3::new(0.0, 2.0, 0.0);
        let definition_handle = host
            .document_mut()
            .add_entity(EntityType::AttributeDefinition(definition))
            .unwrap();
        let insert_handle = host
            .document_mut()
            .add_entity(EntityType::Insert(acadrust::entities::Insert::new(
                "TAGBLOCK",
                acadrust::types::Vector3::new(10.0, 0.0, 0.0),
            )))
            .unwrap();

        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process
                .dispatch(host, command, &mut |_| {})
                .expect("Python dispatch"));
        };
        dispatch(
            &mut host,
            &format!(
                concat!(
                    "PY_EVAL ocs.active_document.create_entity('AttributeEntity',owner_handle={},",
                    "tag='PART_NO',value='PN-101',",
                    "insertion_point={{'x':10.0,'y':2.0,'z':0.0}},height=2.5).handle"
                ),
                insert_handle.value()
            ),
        );
        let created_insert = match host.document().get_entity(insert_handle).unwrap() {
            EntityType::Insert(insert) => insert,
            _ => unreachable!(),
        };
        assert_eq!(created_insert.attributes.len(), 1);
        let attribute_handle = created_insert.attributes[0].common.handle;
        assert_eq!(
            created_insert.attributes[0].attdef_handle,
            definition_handle
        );
        let created = created_insert.attributes[0].clone();
        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.entities[{}].value",
                attribute_handle.value()
            ),
        );
        let readback = &host.app.command_line.history.last().unwrap().text;
        assert!(readback.contains("PN-101"), "{readback}");

        let script =
            std::env::temp_dir().join(format!("ocs_attribute_entity_{}.py", std::process::id()));
        std::fs::write(
            &script,
            format!(
                concat!(
                    "doc = ocs.active_document\n",
                    "attribute = doc.entities[{}]\n",
                    "with doc.transaction('Edit attribute'):\n",
                    "    attribute.value = 'PN-102'\n",
                    "    attribute.insertion_point = (12.0, 3.0, 0.0)\n",
                    "    attribute.rotation = 0.25\n",
                    "doc.selection = [doc.entities[{}]]\n"
                ),
                attribute_handle.value(),
                insert_handle.value()
            ),
        )
        .unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let expected = host.nested_attribute(attribute_handle).unwrap();
        let EntityType::AttributeEntity(edited) = &expected else {
            unreachable!()
        };
        assert_eq!(edited.value, "PN-102");
        assert_eq!(
            edited.insertion_point,
            acadrust::types::Vector3::new(12.0, 3.0, 0.0)
        );
        assert_eq!(edited.common, created.common);
        assert_eq!(edited.attdef_handle, definition_handle);
        assert_eq!(host.selection(), vec![insert_handle]);
        let render = edited.to_render(host.document()).unwrap();
        assert!(matches!(render.object,
            crate::scene::convert::acad_to_render::RenderObject::Text(ref strokes)
                if !strokes.is_empty()));

        for (patch, message) in [
            ("'tag':'MISSING'", "no definition"),
            ("'height':0.0", "greater than zero"),
            ("'owner_handle':999999", "read-only"),
        ] {
            dispatch(
                &mut host,
                &format!(
                    "PY_EVAL ocs.update_many('Reject attribute', [{{'handle':{}, {patch}}}])",
                    attribute_handle.value()
                ),
            );
            assert_eq!(
                host.nested_attribute(attribute_handle),
                Some(expected.clone())
            );
            assert!(host
                .app
                .command_line
                .history
                .last()
                .unwrap()
                .text
                .contains(message));
        }

        let assert_saved = |document: &CadDocument| {
            assert!(
                matches!(document.get_entity(insert_handle), Some(EntityType::Insert(insert))
                if insert.attributes.len() == 1
                    && insert.attributes[0].common.handle == attribute_handle
                    && insert.attributes[0].value == "PN-102"
                    && (insert.attributes[0].rotation - 0.25).abs() < 1e-12)
            );
        };
        assert_saved(
            &crate::io::load_bytes(
                "attribute.dwg",
                acadrust::DwgWriter::write_to_vec(host.document()).unwrap(),
            )
            .unwrap(),
        );
        assert_saved(
            &crate::io::load_bytes(
                "attribute.dxf",
                acadrust::DxfWriter::new(host.document())
                    .write_to_vec()
                    .unwrap(),
            )
            .unwrap(),
        );

        dispatch(
            &mut host,
            &format!(
                "PY_EVAL ocs.active_document.delete_entity({})",
                attribute_handle.value()
            ),
        );
        assert!(host.nested_attribute(attribute_handle).is_none());
        let assert_deleted = |document: &CadDocument| {
            assert!(
                matches!(document.get_entity(insert_handle), Some(EntityType::Insert(insert))
                    if insert.attributes.is_empty())
            );
        };
        assert_deleted(
            &crate::io::load_bytes(
                "attribute-deleted.dwg",
                acadrust::DwgWriter::write_to_vec(host.document()).unwrap(),
            )
            .unwrap(),
        );
        assert_deleted(
            &crate::io::load_bytes(
                "attribute-deleted.dxf",
                acadrust::DxfWriter::new(host.document())
                    .write_to_vec()
                    .unwrap(),
            )
            .unwrap(),
        );
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.len() == 1
                && insert.attributes[0].value == "PN-102")
        );
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes[0].value == "PN-101")
        );
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.is_empty())
        );
        app.redo_steps(3);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(insert_handle),
            Some(EntityType::Insert(insert)) if insert.attributes.is_empty())
        );
    }

    #[test]
    fn staged_python_hatch_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let script = std::env::temp_dir().join(format!("ocs_hatch_create_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "def p(x, y): return {'x': x, 'y': y}\n",
            "def boundary(size):\n",
            "    pairs = [(0.0,0.0,size,0.0),(size,0.0,size,size),(size,size,0.0,size),(0.0,size,0.0,0.0)]\n",
            "    edges = [{'kind':'Line','value':{'start':p(a,b),'end':p(c,d)}} for a,b,c,d in pairs]\n",
            "    return {'flags':1,'edges':edges,'boundary_handles':[]}\n",
            "doc = ocs.active_document\n",
            "hatch = doc.create_entity('Hatch', paths=[boundary(10.0)])\n",
        )).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let hatch_handle = host.document().entities().find_map(|entity| match entity {
            EntityType::Hatch(hatch) => Some(hatch.common.handle),
            _ => None,
        }).unwrap_or_else(|| panic!("Python did not create Hatch: {:?}",
            host.app.command_line.history.last().map(|entry| &entry.text)));
        let created = host.document().get_entity(hatch_handle).unwrap().clone();
        assert!(host.app.tabs[0].scene.hatches.get(&hatch_handle)
            .is_some_and(|model| model.boundary.len() >= 4 && model.name == "SOLID"));
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.active_document.entities[{}].pattern['name']", hatch_handle.value()));
        assert!(host.app.command_line.history.last().unwrap().text.contains("SOLID"));

        let script = std::env::temp_dir().join(format!("ocs_hatch_edit_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "def p(x, y): return {{'x': x, 'y': y}}\n",
            "def boundary(size):\n",
            "    pairs = [(0.0,0.0,size,0.0),(size,0.0,size,size),(size,size,0.0,size),(0.0,size,0.0,0.0)]\n",
            "    edges = [{{'kind':'Line','value':{{'start':p(a,b),'end':p(c,d)}}}} for a,b,c,d in pairs]\n",
            "    return {{'flags':1,'edges':edges,'boundary_handles':[]}}\n",
            "doc = ocs.active_document\n",
            "hatch = doc.entities[{}]\n",
            "with doc.transaction('Edit hatch'):\n",
            "    hatch.paths = [boundary(12.0)]\n",
            "    hatch.is_solid = False\n",
            "    hatch.pattern = {{'name':'TEST','description':'test pattern','lines':[",
            "{{'angle':0.0,'base_point':p(0.0,0.0),'offset':p(0.0,2.0),'dash_lengths':[1.0,-1.0]}}]}}\n",
            "    hatch.pattern_scale = 2.0\n",
            "    hatch.pattern_angle = 0.25\n",
            "doc.selection = [hatch]\n"
        ), hatch_handle.value())).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let expected = host.document().get_entity(hatch_handle).unwrap().clone();
        let EntityType::Hatch(edited) = &expected else { unreachable!() };
        assert!(!edited.is_solid);
        assert_eq!(edited.pattern.name, "TEST");
        assert_eq!(edited.pattern.lines.len(), 1);
        assert_eq!(edited.paths[0].edges.len(), 4);
        assert!((edited.pattern_scale - 2.0).abs() < 1e-12);
        assert_eq!(edited.gradient_color, match &created { EntityType::Hatch(value) => value.gradient_color.clone(), _ => unreachable!() });
        assert_eq!(host.selection(), vec![hatch_handle]);
        assert!(host.app.tabs[0].scene.hatches.get(&hatch_handle).is_some_and(|model|
            model.boundary.len() >= 4 && model.name == "TEST"
                && matches!(&model.pattern,
                    crate::scene::model::hatch_model::HatchPattern::Pattern(lines)
                        if !lines.is_empty())));

        for (patch, message) in [
            ("'pattern_scale':0.0", "greater than zero"),
            ("'is_associative':True", "requires boundary handles"),
            ("'gradient_color':{}", "outside the editable schema"),
            ("'paths':[{'flags':1,'edges':[{'kind':'Line','value':{'start':{'x':0.0,'y':0.0},'end':{'x':1.0,'y':0.0}}}],'boundary_handles':[]}]", "not closed"),
        ] {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject hatch', [{{'handle':{}, {patch}}}])",
                hatch_handle.value()));
            assert_eq!(host.document().get_entity(hatch_handle), Some(&expected));
            assert!(host.app.command_line.history.last().unwrap().text.contains(message));
        }

        let assert_saved = |document: &CadDocument| {
            assert!(matches!(document.get_entity(hatch_handle), Some(EntityType::Hatch(hatch))
                if !hatch.is_solid && hatch.pattern.name == "TEST"
                    && hatch.pattern.lines.len() == 1 && hatch.paths[0].edges.len() == 4
                    && (hatch.pattern_scale - 2.0).abs() < 1e-12));
        };
        let reopened_dwg = crate::io::load_bytes("hatch.dwg",
            acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let reopened_dxf = crate::io::load_bytes("hatch.dxf",
            acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        assert_saved(&reopened_dwg);
        assert_saved(&reopened_dxf);
        for (name, mut document) in [("hatch-reedit.dwg", reopened_dwg),
            ("hatch-reedit.dxf", reopened_dxf)] {
            let before = document.get_entity(hatch_handle).unwrap().clone();
            let mut after = before.clone();
            let EntityType::Hatch(hatch) = &mut after else { unreachable!() };
            hatch.elevation = 3.0;
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
            *document.get_entity_mut(hatch_handle).unwrap() = after;
            let bytes = if name.ends_with("dwg") {
                acadrust::DwgWriter::write_to_vec(&document).unwrap()
            } else {
                acadrust::DxfWriter::new(&document).write_to_vec().unwrap()
            };
            assert!(matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(hatch_handle),
                Some(EntityType::Hatch(hatch)) if (hatch.elevation - 3.0).abs() < 1e-12));
        }
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.active_document.delete_entity({})", hatch_handle.value()));
        assert!(host.document().get_entity(hatch_handle).is_none());
        for (name, bytes) in [
            ("hatch-deleted.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()),
            ("hatch-deleted.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()),
        ] {
            assert!(crate::io::load_bytes(name, bytes).unwrap().get_entity(hatch_handle).is_none());
        }
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(hatch_handle), Some(&expected));
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(hatch_handle), Some(&created));
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(hatch_handle).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(hatch_handle).is_none());
    }

    #[test]
    fn staged_python_leader_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last_output = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

        dispatch(&mut host, concat!(
            "PY_EVAL ocs.active_document.create_entity('Text', text='NOTE', ",
            "insertion={'x':20.0,'y':10.0,'z':0.0}, height=2.5).handle"
        ));
        let text_handle = host.document().entities().find_map(|entity| match entity {
            EntityType::Text(text) => Some(text.common.handle),
            _ => None,
        }).unwrap_or_else(|| panic!("Python did not create Text: {}", last_output(&host)));
        let script = std::env::temp_dir().join(format!("ocs_leader_create_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "doc = ocs.active_document\n",
            "doc.create_entity('Leader', vertices=[{{'x':0.0,'y':0.0,'z':0.0}},",
            "{{'x':10.0,'y':10.0,'z':0.0}},{{'x':20.0,'y':10.0,'z':0.0}}], ",
            "annotation_handle={}, hookline_enabled=True)\n"
        ), text_handle.value())).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let leader_handle = host.document().entities().find_map(|entity| match entity {
            EntityType::Leader(leader) => Some(leader.common.handle),
            _ => None,
        }).unwrap_or_else(|| panic!("Python did not create Leader: {}", last_output(&host)));
        let created = host.document().get_entity(leader_handle).unwrap().clone();
        let EntityType::Leader(created_leader) = &created else { unreachable!() };
        assert_eq!(created_leader.annotation_handle, text_handle);
        assert_eq!(created_leader.vertices.len(), 3);

        let assert_geometry = |entity: &EntityType, document: &CadDocument| {
            let EntityType::Leader(leader) = entity else { panic!("expected Leader") };
            let render = leader.to_render(document).expect("leader render");
            let crate::scene::convert::acad_to_render::RenderObject::Lines(points) = render.object else {
                panic!("expected leader linework");
            };
            assert!(points.len() >= leader.vertices.len());
            assert!(points.iter().flatten().all(|value| value.is_nan() || value.is_finite()));
        };
        assert_geometry(&created, host.document());
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.active_document.entities[{}].annotation_handle", leader_handle.value()));
        assert!(last_output(&host).contains(&text_handle.value().to_string()));

        // GUI picking groups a leader with its annotation, while scripted
        // selection stays exact.
        host.app.tabs[0].scene.select_entities(&[leader_handle]);
        let mut grouped = host.selection();
        grouped.sort_by_key(|handle| handle.value());
        assert_eq!(grouped, vec![text_handle, leader_handle]);
        host.app.tabs[0].scene.replace_selection_exact(&[]);

        let script = std::env::temp_dir().join(format!("ocs_leader_edit_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "doc = ocs.active_document\n",
            "leader = doc.entities[{}]\n",
            "with doc.transaction('Edit leader'):\n",
            "    leader.vertices = [{{'x':1.0,'y':1.0,'z':0.0}},{{'x':11.0,'y':12.0,'z':0.0}},",
            "{{'x':22.0,'y':12.0,'z':0.0}},{{'x':30.0,'y':12.0,'z':0.0}}]\n",
            "    leader.text_height = 4.0\n",
            "    leader.arrow_enabled = False\n",
            "    leader.annotation_offset = (1.0, 2.0, 0.0)\n",
            "    leader.override_color = {{'kind':'Rgb','value':{{'r':255,'g':0,'b':0}}}}\n",
            "doc.selection = [leader]\n"
        ), leader_handle.value())).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let expected = host.document().get_entity(leader_handle).unwrap().clone();
        let EntityType::Leader(edited) = &expected else { unreachable!() };
        assert_eq!(edited.vertices.len(), 4, "edit failed: {}", last_output(&host));
        assert_eq!(edited.text_height, 4.0);
        assert!(!edited.arrow_enabled);
        assert_eq!(edited.annotation_offset, acadrust::types::Vector3::new(1.0, 2.0, 0.0));
        assert_eq!(edited.annotation_handle, text_handle);
        assert_eq!(edited.common, created_leader.common);
        assert_eq!(edited.dimension_style, created_leader.dimension_style);
        assert_eq!(edited.hookline_enabled, created_leader.hookline_enabled);
        assert_eq!(edited.normal, created_leader.normal);
        assert_eq!(edited.origin, created_leader.origin);
        assert_eq!(edited.override_color, acadrust::types::Color::Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(host.selection(), vec![leader_handle]);
        assert_geometry(&expected, host.document());

        for (patch, message) in [
            ("'annotation_handle':999999", "does not exist"),
            ("'creation_type':'WithTolerance'", "does not match creation_type"),
            ("'creation_type':'NoAnnotation'", "no annotation"),
            ("'vertices':[{'x':0.0,'y':0.0,'z':0.0}]", "at least 2"),
            ("'text_height':0.0", "greater than zero"),
            ("'dimension_style':'Missing'", "dimension style"),
            ("'override_color':{'kind':'Index','value':300}", "Color"),
        ] {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject leader', [{{'handle':{}, {patch}}}])",
                leader_handle.value()));
            assert_eq!(host.document().get_entity(leader_handle), Some(&expected), "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }
        // A valid edit paired with an invalid one is atomic across entities.
        dispatch(&mut host, &format!(concat!(
            "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'text_width':5.0}},",
            "{{'handle':{},'text_height':0.0}}])"
        ), leader_handle.value(), text_handle.value()));
        assert_eq!(host.document().get_entity(leader_handle), Some(&expected));

        // Both formats keep the path, annotation link, arrow flag and offset.
        // DWG R2010+ derives text_height/text_width/hookline_enabled instead of
        // storing them (acadrust writes them for R13-R2007/R13-R14 only), so
        // only DXF must round-trip those three.
        let assert_saved = |document: &CadDocument, is_dxf: bool| {
            let Some(EntityType::Leader(leader)) = document.get_entity(leader_handle) else {
                panic!("Leader missing after reopen");
            };
            assert_eq!(leader.vertices.len(), 4);
            assert_eq!(leader.annotation_handle, text_handle);
            assert!(!leader.arrow_enabled);
            // The script sets a true colour. DXF's LEADER group 77 holds an ACI
            // index only, so it cannot carry one, and the DWG writer does not
            // store `override_color` at all (BLOCKER, cadcodec): it reopens as
            // ByLayer in both formats.
            assert_eq!(leader.override_color, acadrust::types::Color::ByLayer,
                "acadrust now persists a true-colour Leader.override_color; drop the blocker");
            assert_eq!(leader.annotation_offset, acadrust::types::Vector3::new(1.0, 2.0, 0.0));
            assert_eq!(leader.dimension_style, "Standard");
            if is_dxf {
                assert!((leader.text_height - 4.0).abs() < 1e-12);
                assert!(leader.hookline_enabled);
            } else {
                assert_eq!(leader.text_height, 2.5, "DWG R2010+ does not store text_height");
                assert!(!leader.hookline_enabled, "DWG R2010+ does not store hookline_enabled");
            }
            assert!(matches!(document.get_entity(text_handle), Some(EntityType::Text(_))));
        };
        let reopened_dwg = crate::io::load_bytes("leader.dwg",
            acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let reopened_dxf = crate::io::load_bytes("leader.dxf",
            acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        assert_saved(&reopened_dwg, false);
        assert_saved(&reopened_dxf, true);
        for (name, mut document) in [("leader-reedit.dwg", reopened_dwg),
            ("leader-reedit.dxf", reopened_dxf)] {
            assert_geometry(document.get_entity(leader_handle).unwrap(), &document);
            let before = document.get_entity(leader_handle).unwrap().clone();
            let mut after = before.clone();
            let EntityType::Leader(leader) = &mut after else { unreachable!() };
            leader.vertices[3] = acadrust::types::Vector3::new(35.0, 12.0, 0.0);
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
            ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after).unwrap();
            *document.get_entity_mut(leader_handle).unwrap() = after;
            let bytes = if name.ends_with("dwg") {
                acadrust::DwgWriter::write_to_vec(&document).unwrap()
            } else {
                acadrust::DxfWriter::new(&document).write_to_vec().unwrap()
            };
            assert!(matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(leader_handle),
                Some(EntityType::Leader(leader)) if (leader.vertices[3].x - 35.0).abs() < 1e-9));
        }

        // Erasing a leader also erases its annotation (scene leader grouping).
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.active_document.delete_entity({})", leader_handle.value()));
        assert!(host.document().get_entity(leader_handle).is_none());
        assert!(host.document().get_entity(text_handle).is_none());
        for (name, bytes) in [
            ("leader-deleted.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()),
            ("leader-deleted.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()),
        ] {
            let document = crate::io::load_bytes(name, bytes).unwrap();
            assert!(document.get_entity(leader_handle).is_none());
            assert!(document.get_entity(text_handle).is_none());
        }
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        // Text create, Leader create, edit transaction, Leader delete.
        assert_eq!(app.tabs[0].history.undo_stack.len(), 4);
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(leader_handle), Some(&expected));
        assert!(app.tabs[0].scene.document.get_entity(text_handle).is_some());
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(leader_handle), Some(&created));
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(leader_handle).is_none());
        assert!(app.tabs[0].scene.document.get_entity(text_handle).is_some());
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(text_handle).is_none());
        app.redo_steps(4);
        assert!(app.tabs[0].scene.document.get_entity(leader_handle).is_none());
        assert!(app.tabs[0].scene.document.get_entity(text_handle).is_none());
    }

    #[test]
    fn staged_python_mline_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last_output = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let style_handle = host.document().objects.iter().find_map(|(handle, object)| match object {
            acadrust::objects::ObjectType::MLineStyle(style) if style.name == "Standard" => Some(*handle),
            _ => None,
        }).expect("document carries the Standard MLineStyle");

        let script = std::env::temp_dir().join(format!("ocs_mline_create_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "def p(x, y): return {'position': {'x': x, 'y': y, 'z': 0.0}}\n",
            "doc = ocs.active_document\n",
            "doc.create_entity('MLine', vertices=[p(0.0, 0.0), p(10.0, 0.0)])\n",
        )).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let handle = host.document().entities().find_map(|entity| match entity {
            EntityType::MLine(mline) => Some(mline.common.handle),
            _ => None,
        }).unwrap_or_else(|| panic!("Python did not create MLine: {}", last_output(&host)));
        let created = host.document().get_entity(handle).unwrap().clone();
        let EntityType::MLine(created_mline) = &created else { unreachable!() };
        assert_eq!(created_mline.style_handle, Some(style_handle));
        assert_eq!(created_mline.style_element_count, 2);
        assert_eq!(created_mline.start_point, acadrust::types::Vector3::ZERO);
        let first_parameters = |mline: &acadrust::entities::MLine, vertex: usize| -> Vec<f64> {
            mline.vertices[vertex].segments.iter().map(|segment| segment.parameters[0]).collect()
        };
        assert_eq!(first_parameters(created_mline, 0), vec![0.5, -0.5]);
        assert!(created_mline.vertices.iter().all(|vertex| {
            (vertex.direction.x - 1.0).abs() < 1e-9 && (vertex.miter.y.abs() - 1.0).abs() < 1e-9
        }));
        let assert_lines = |entity: &EntityType, document: &CadDocument, expected_vertices: usize| {
            let EntityType::MLine(mline) = entity else { panic!("expected MLine") };
            assert_eq!(mline.vertices.len(), expected_vertices);
            let lines = crate::entities::mline::mline_lines(mline, document);
            assert!(lines.len() >= 2, "expected styled element lines");
            assert!(lines.iter().flat_map(|line| &line.points).flatten().all(|v| v.is_finite()));
        };
        assert_lines(&created, host.document(), 2);

        let script = std::env::temp_dir().join(format!("ocs_mline_edit_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "def p(x, y): return {{'position': {{'x': x, 'y': y, 'z': 0.0}}}}\n",
            "doc = ocs.active_document\n",
            "mline = doc.entities[{}]\n",
            "with doc.transaction('Edit mline'):\n",
            "    mline.vertices = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]\n",
            "    mline.scale_factor = 2.0\n",
            "    mline.justification = 'Top'\n",
            "doc.selection = [mline]\n"
        ), handle.value())).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let expected = host.document().get_entity(handle).unwrap().clone();
        let EntityType::MLine(edited) = &expected else { unreachable!() };
        assert_eq!(edited.vertices.len(), 3, "edit failed: {}", last_output(&host));
        assert_eq!(edited.scale_factor, 2.0);
        assert_eq!(edited.justification, acadrust::entities::MLineJustification::Top);
        assert_eq!(edited.style_handle, created_mline.style_handle);
        assert_eq!(edited.common, created_mline.common);
        assert_eq!(edited.normal, created_mline.normal);
        assert_eq!(edited.flags.bits() & !1, 0);
        // Top justification with scale 2: first element sits on the path,
        // the second 2.0 to the right of travel; the corner miter widens it.
        let end = first_parameters(edited, 0);
        assert!(end[0].abs() < 1e-9 && (end[1] + 2.0).abs() < 1e-9, "{end:?}");
        let corner = first_parameters(edited, 1);
        assert!(corner[1].abs() > 2.0 && corner[1].is_finite(), "miter must widen at the corner: {corner:?}");
        assert!((edited.vertices[1].direction.y - edited.vertices[1].direction.x).abs() > 1e-6);
        assert_lines(&expected, host.document(), 3);
        assert_eq!(host.selection(), vec![handle]);

        for (patch, message) in [
            ("'style_name':'Missing'", "does not exist"),
            ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2"),
            ("'scale_factor':0.0", "nonzero"),
            ("'flags':64", "unknown bits"),
            ("'flags':3,'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':0.0,'z':0.0}}]", "requires at least 3"),
            ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':0.0,'y':0.0,'z':0.0}}]", "coincide"),
            ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':float('nan'),'z':0.0}}]", "finite"),
            ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0},'segments':[{}]},{'position':{'x':1.0,'y':0.0,'z':0.0}}]", "segments but style"),
            ("'start_point':{'x':5.0,'y':5.0,'z':0.0}", "read-only"),
        ] {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject mline', [{{'handle':{}, {patch}}}])", handle.value()));
            assert_eq!(host.document().get_entity(handle), Some(&expected), "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }
        dispatch(&mut host, &format!(concat!(
            "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'scale_factor':3.0}},",
            "{{'handle':{},'scale_factor':0.0}}])"
        ), handle.value(), handle.value()));
        assert_eq!(host.document().get_entity(handle), Some(&expected));

        let assert_saved = |document: &CadDocument| {
            let Some(EntityType::MLine(mline)) = document.get_entity(handle) else {
                panic!("MLine missing after reopen");
            };
            assert_eq!(mline.vertices.len(), 3);
            assert_eq!(mline.scale_factor, 2.0);
            assert_eq!(mline.justification, acadrust::entities::MLineJustification::Top);
            assert_eq!(mline.style_name, "Standard");
            assert_eq!(mline.vertices[2].position, acadrust::types::Vector3::new(10.0, 10.0, 0.0));
            let (a, b) = (first_parameters(mline, 1), first_parameters(edited, 1));
            assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9), "{a:?} vs {b:?}");
            assert_lines(&document.get_entity(handle).unwrap().clone(), document, 3);
        };
        let reopened_dwg = crate::io::load_bytes("mline.dwg",
            acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let reopened_dxf = crate::io::load_bytes("mline.dxf",
            acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        assert_saved(&reopened_dwg);
        assert_saved(&reopened_dxf);
        for (name, mut document) in [("mline-reedit.dwg", reopened_dwg), ("mline-reedit.dxf", reopened_dxf)] {
            let before = document.get_entity(handle).unwrap().clone();
            let mut after = before.clone();
            let EntityType::MLine(mline) = &mut after else { unreachable!() };
            mline.vertices[2].position = acadrust::types::Vector3::new(10.0, 12.0, 0.0);
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
            ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after).unwrap();
            let EntityType::MLine(mline) = &mut after else { unreachable!() };
            let old = match &before { EntityType::MLine(old) => old, _ => unreachable!() };
            crate::entities::mline::normalize_scripted_mline(Some(old), mline, &document).unwrap();
            *document.get_entity_mut(handle).unwrap() = after;
            let bytes = if name.ends_with("dwg") {
                acadrust::DwgWriter::write_to_vec(&document).unwrap()
            } else {
                acadrust::DxfWriter::new(&document).write_to_vec().unwrap()
            };
            assert!(matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle),
                Some(EntityType::MLine(mline)) if (mline.vertices[2].position.y - 12.0).abs() < 1e-9));
        }

        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", handle.value()));
        assert!(host.document().get_entity(handle).is_none());
        for (name, bytes) in [
            ("mline-deleted.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()),
            ("mline-deleted.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()),
        ] {
            assert!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle).is_none());
        }
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(handle), Some(&expected));
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(handle), Some(&created));
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn staged_python_dimension_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last_output = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

        // (subtype, python fields, expected measurement)
        let fixtures: [(&str, &str, f64); 9] = [
            ("Linear", "definition_point=P(10,5), first_point=P(0,0), second_point=P(10,0)", 10.0),
            ("Aligned", "definition_point=P(1,5.5), first_point=P(0,0), second_point=P(3,4)", 5.0),
            ("Radius", "definition_point=P(5,0), angle_vertex=P(0,0)", 5.0),
            ("Diameter", "definition_point=P(10,0), angle_vertex=P(0,0)", 10.0),
            ("Angular3Pt", "definition_point=P(3,3), first_point=P(5,0), second_point=P(0,5), angle_vertex=P(0,0)", 90.0),
            ("Angular2Ln", "definition_point=P(0,5), dimension_arc=P(3,3), first_point=P(0,0), second_point=P(5,0), angle_vertex=P(0,0)", 90.0),
            ("Ordinate", "feature_location=P(5,3), leader_endpoint=P(5,8), is_ordinate_type_x=True", 5.0),
            ("Arc", "definition_point=P(3.5,3.5), first_extension_point=P(5,0), second_extension_point=P(0,5), center_point=P(0,0), arc_start_parameter=0.0, arc_end_parameter=1.5707963267948966", 5.0 * std::f64::consts::FRAC_PI_2),
            ("LargeRadial", "definition_point=P(10,0), chord_point=P(4,0)", 6.0),
        ];
        let mut script = String::from("def P(x, y): return {'x': x, 'y': y, 'z': 0.0}\ndoc = ocs.active_document\n");
        for (subtype, fields, _) in &fixtures {
            script.push_str(&format!("doc.create_entity('Dimension', subtype='{subtype}', {fields})\n"));
        }
        let path = std::env::temp_dir().join(format!("ocs_dimension_create_{}.py", std::process::id()));
        std::fs::write(&path, script).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", path.display()));
        let _ = std::fs::remove_file(&path);
        let subtype_of = |dimension: &acadrust::entities::Dimension| -> &'static str {
            use acadrust::entities::Dimension as D;
            match dimension {
                D::Aligned(_) => "Aligned", D::Linear(_) => "Linear", D::Radius(_) => "Radius",
                D::Diameter(_) => "Diameter", D::Angular2Ln(_) => "Angular2Ln",
                D::Angular3Pt(_) => "Angular3Pt", D::Ordinate(_) => "Ordinate",
                D::Arc(_) => "Arc", D::LargeRadial(_) => "LargeRadial",
            }
        };
        let handles_by_subtype = |document: &CadDocument| {
            let mut map = std::collections::BTreeMap::new();
            for entity in document.entities() {
                if let EntityType::Dimension(dimension) = entity {
                    map.insert(subtype_of(dimension), dimension.base().common.handle);
                }
            }
            map
        };
        let handles = handles_by_subtype(host.document());
        assert_eq!(handles.len(), 9, "created: {handles:?}; {}", last_output(&host));
        let measured = |document: &CadDocument, handle: Handle| -> f64 {
            let Some(EntityType::Dimension(dimension)) = document.get_entity(handle) else {
                panic!("Dimension missing");
            };
            dimension.base().actual_measurement
        };
        for (subtype, _, expected) in &fixtures {
            let value = measured(host.document(), handles[subtype]);
            assert!((value - expected).abs() < 1e-6, "{subtype}: measured {value}, expected {expected}");
            let Some(EntityType::Dimension(dimension)) = host.document().get_entity(handles[subtype]) else { unreachable!() };
            assert_eq!(dimension.measurement(), value, "{subtype}: stored measurement is derived");
            let wires = host.app.tabs[0].scene.wire_models_for(&[handles[subtype]]);
            assert!(!wires.is_empty(), "{subtype}: dimension produced no canvas geometry");
        }
        let linear = handles["Linear"];
        let created_linear = host.document().get_entity(linear).unwrap().clone();

        // Edit: move a defining point (the measurement follows), change text
        // and style-independent placement; untouched fields survive.
        let script = std::env::temp_dir().join(format!("ocs_dimension_edit_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "doc = ocs.active_document\n",
            "linear = doc.entities[{}]\n",
            "radius = doc.entities[{}]\n",
            "with doc.transaction('Edit dimensions'):\n",
            "    linear.second_point = (20.0, 0.0, 0.0)\n",
            "    linear.text = '<> mm'\n",
            "    linear.attachment_point = 'TopCenter'\n",
            "    radius.definition_point = (8.0, 0.0, 0.0)\n",
            "doc.selection = [linear]\n"
        ), linear.value(), handles["Radius"].value())).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let expected_linear = host.document().get_entity(linear).unwrap().clone();
        let EntityType::Dimension(edited) = &expected_linear else { unreachable!() };
        let EntityType::Dimension(before) = &created_linear else { unreachable!() };
        assert!((edited.base().actual_measurement - 20.0).abs() < 1e-9, "edit failed: {}", last_output(&host));
        assert!((measured(host.document(), handles["Radius"]) - 8.0).abs() < 1e-9);
        assert_eq!(edited.base().text, "<> mm");
        assert_eq!(edited.base().attachment_point, acadrust::entities::AttachmentPointType::TopCenter);
        assert_eq!(edited.base().common, before.base().common);
        assert_eq!(edited.base().style_name, before.base().style_name);
        assert_eq!(edited.base().normal, before.base().normal);
        assert_eq!(host.selection(), vec![linear]);
        let expected_radius = host.document().get_entity(handles["Radius"]).unwrap().clone();
        assert!(!host.app.tabs[0].scene.wire_models_for(&[linear]).is_empty());

        let reject_cases: [(&str, &str, &str); 9] = [
            ("Linear", "'leader_length':2.0", "does not apply to the Linear subtype"),
            ("Linear", "'subtype':'Radius'", "read-only"),
            ("Linear", "'actual_measurement':5.0", "read-only"),
            ("Linear", "'style_name':'Missing'", "does not exist"),
            ("Linear", "'first_point':{'x':20.0,'y':0.0,'z':0.0}", "must not coincide"),
            ("Linear", "'first_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ("Linear", "'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ("Radius", "'angle_vertex':{'x':8.0,'y':0.0,'z':0.0}", "must not coincide"),
            ("Linear", "'attachment_point':'Nowhere'", "unsupported AttachmentPointType"),
        ];
        for (subtype, patch, message) in reject_cases {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject dimension', [{{'handle':{}, {patch}}}])",
                handles[subtype].value()));
            assert_eq!(host.document().get_entity(linear), Some(&expected_linear), "{patch}");
            assert_eq!(host.document().get_entity(handles["Radius"]), Some(&expected_radius), "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }
        dispatch(&mut host, &format!(concat!(
            "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'text':'changed'}},",
            "{{'handle':{},'style_name':'Missing'}}])"
        ), linear.value(), handles["Radius"].value()));
        assert_eq!(host.document().get_entity(linear), Some(&expected_linear));
        for (patch, message) in [
            ("ocs.active_document.create_entity('Dimension', definition_point=P(0,0))", "requires subtype"),
            ("ocs.active_document.create_entity('Dimension', subtype='Cone')", "unsupported Dimension subtype"),
            ("ocs.active_document.create_entity('Dimension', subtype='Linear', first_point=P(0,0), second_point=P(1,0))", "requires definition_point"),
        ] {
            dispatch(&mut host, &format!("PY_EVAL (lambda P: {patch})(lambda x, y: {{'x': x, 'y': y, 'z': 0.0}})"));
            assert_eq!(handles_by_subtype(host.document()).len(), 9, "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }

        // DWG and DXF: every subtype is recorded separately.
        let mut failures = Vec::new();
        let dwg = crate::io::load_bytes("dimension.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("dimension.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
            let reopened = handles_by_subtype(document);
            for (subtype, _, expected) in &fixtures {
                let expected = match *subtype { "Linear" => 20.0, "Radius" => 8.0, _ => *expected };
                match reopened.get(subtype) {
                    None => failures.push(format!("{format} {subtype}: missing")),
                    Some(handle) => {
                        let value = measured(document, *handle);
                        if (value - expected).abs() > 1e-6 {
                            failures.push(format!("{format} {subtype}: measurement {value} != {expected}"));
                        }
                    }
                }
            }
            let Some(EntityType::Dimension(dimension)) = document.get_entity(linear) else {
                failures.push(format!("{format} Linear handle lost"));
                continue;
            };
            if dimension.base().text != "<> mm" { failures.push(format!("{format} Linear text {:?}", dimension.base().text)); }
        }
        assert!(failures.is_empty(), "persistence gaps: {failures:#?}");
        for (name, mut document) in [("dimension-reedit.dwg", dwg), ("dimension-reedit.dxf", dxf)] {
            let before = document.get_entity(linear).unwrap().clone();
            let mut after = before.clone();
            let EntityType::Dimension(dimension) = &mut after else { unreachable!() };
            if let acadrust::entities::Dimension::Linear(value) = dimension { value.second_point.x = 30.0; }
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
            ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after).unwrap();
            let EntityType::Dimension(dimension) = &mut after else { unreachable!() };
            crate::entities::dimension::normalize_scripted_dimension(dimension);
            *document.get_entity_mut(linear).unwrap() = after;
            let bytes = if name.ends_with("dwg") {
                acadrust::DwgWriter::write_to_vec(&document).unwrap()
            } else {
                acadrust::DxfWriter::new(&document).write_to_vec().unwrap()
            };
            let reopened = crate::io::load_bytes(name, bytes).unwrap();
            assert!((measured(&reopened, linear) - 30.0).abs() < 1e-6, "{name}");
        }

        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", linear.value()));
        assert!(host.document().get_entity(linear).is_none());
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(linear), Some(&expected_linear));
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(linear), Some(&created_linear));
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(linear).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(linear).is_none());
    }

    #[test]
    fn staged_python_multileader_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last_output = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

        let script = std::env::temp_dir().join(format!("ocs_mleader_create_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "def P(x, y): return {'x': x, 'y': y, 'z': 0.0}\n",
            "doc = ocs.active_document\n",
            "doc.create_entity('MultiLeader', content_type='MText', context={\n",
            "    'has_text_contents': True, 'text_string': 'NOTE', 'text_location': P(20, 10),\n",
            "    'text_height': 2.5,\n",
            "    'leader_roots': [{'connection_point': P(15, 10), 'direction': P(1, 0), 'landing_distance': 2.0,\n",
            "        'lines': [{'points': [P(0, 0), P(10, 10), P(15, 10)]}]}]})\n",
        )).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let handle = host.document().entities().find_map(|entity| match entity {
            EntityType::MultiLeader(value) => Some(value.common.handle),
            _ => None,
        }).unwrap_or_else(|| panic!("Python did not create MultiLeader: {}", last_output(&host)));
        let created = host.document().get_entity(handle).unwrap().clone();
        let EntityType::MultiLeader(created_ml) = &created else { unreachable!() };
        assert_eq!(created_ml.context.text_string, "NOTE");
        assert_eq!(created_ml.context.leader_roots[0].lines[0].points.len(), 3);
        assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty(), "no canvas geometry");

        let script = std::env::temp_dir().join(format!("ocs_mleader_edit_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "def P(x, y): return {{'x': x, 'y': y, 'z': 0.0}}\n",
            "doc = ocs.active_document\n",
            "ml = doc.entities[{}]\n",
            "roots = ml.context['leader_roots']\n",
            "roots[0]['lines'][0]['points'] = [P(0, 0), P(12, 12), P(18, 12), P(22, 12)]\n",
            "with doc.transaction('Edit multileader'):\n",
            "    ml.context = {{'leader_roots': roots, 'text_string': 'EDITED', 'text_location': P(26, 12), 'text_height': 3.0}}\n",
            "    ml.dogleg_length = 3.0\n",
            "    ml.line_color = {{'kind': 'Rgb', 'value': {{'r': 0, 'g': 128, 'b': 255}}}}\n",
            "doc.selection = [ml]\n"
        ), handle.value())).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let expected = host.document().get_entity(handle).unwrap().clone();
        let EntityType::MultiLeader(edited) = &expected else { unreachable!() };
        assert_eq!(edited.context.text_string, "EDITED", "edit failed: {}", last_output(&host));
        assert_eq!(edited.context.leader_roots[0].lines[0].points.len(), 4);
        assert_eq!(edited.dogleg_length, 3.0);
        assert_eq!(edited.context.text_height, 3.0);
        assert_eq!(edited.text_height, created_ml.text_height);
        assert_eq!(edited.line_color, acadrust::types::Color::Rgb { r: 0, g: 128, b: 255 });
        // Untouched context fields and the entity identity survive a partial patch.
        assert_eq!(edited.common, created_ml.common);
        assert_eq!(edited.context.scale_factor, created_ml.context.scale_factor);
        assert_eq!(edited.content_type, created_ml.content_type);
        assert_eq!(edited.context.leader_roots[0].connection_point, created_ml.context.leader_roots[0].connection_point);
        assert_eq!(host.selection(), vec![handle]);
        assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty());

        for (patch, message) in [
            ("'text_height':0.0", "read-only"),
            ("'scale_factor':-1.0", "greater than zero"),
            ("'context':{'text_height':float('inf')}", "finite"),
            ("'style_handle':999999", "does not exist"),
            ("'text_style_handle':999999", "does not exist"),
            ("'content_type':'None'", "cannot carry"),
            ("'context':{'nonsense':1}", "outside the editable schema"),
            ("'context':{'leader_roots':[{'lines':[{'points':[]}]}]}", "has no points"),
            ("'context':{'text_location':{'x':float('nan'),'y':0.0,'z':0.0}}", "finite"),
            ("'dwg_version':5", "read-only"),
            ("'line_color':{'kind':'Index','value':300}", "Color"),
        ] {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject multileader', [{{'handle':{}, {patch}}}])", handle.value()));
            assert_eq!(host.document().get_entity(handle), Some(&expected), "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }
        dispatch(&mut host, &format!(concat!(
            "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'dogleg_length':9.0}},",
            "{{'handle':{},'scale_factor':-1.0}}])"
        ), handle.value(), handle.value()));
        assert_eq!(host.document().get_entity(handle), Some(&expected));

        let mut gaps = Vec::new();
        let dwg = crate::io::load_bytes("mleader.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("mleader.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
            let Some(EntityType::MultiLeader(value)) = document.get_entity(handle) else {
                gaps.push(format!("{format}: MultiLeader missing"));
                continue;
            };
            if value.context.text_string != "EDITED" { gaps.push(format!("{format}: text {:?}", value.context.text_string)); }
            let points = value.context.leader_roots.first().and_then(|r| r.lines.first()).map_or(0, |l| l.points.len());
            if points != 4 { gaps.push(format!("{format}: leader points {points}")); }
            if value.dogleg_length != 3.0 { gaps.push(format!("{format}: dogleg {}", value.dogleg_length)); }
            if value.context.text_height != edited.context.text_height { gaps.push(format!("{format}: context.text_height {}", value.context.text_height)); }
            if value.line_color != edited.line_color { gaps.push(format!("{format}: line_color {:?}", value.line_color)); }
        }
        assert!(gaps.is_empty(), "persistence gaps: {gaps:#?}");
        for (name, mut document) in [("mleader-reedit.dwg", dwg), ("mleader-reedit.dxf", dxf)] {
            let before = document.get_entity(handle).unwrap().clone();
            let mut after = before.clone();
            let EntityType::MultiLeader(value) = &mut after else { unreachable!() };
            value.dogleg_length = 4.5;
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
            ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after).unwrap();
            *document.get_entity_mut(handle).unwrap() = after;
            let bytes = if name.ends_with("dwg") {
                acadrust::DwgWriter::write_to_vec(&document).unwrap()
            } else {
                acadrust::DxfWriter::new(&document).write_to_vec().unwrap()
            };
            assert!(matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle),
                Some(EntityType::MultiLeader(value)) if value.dogleg_length == 4.5), "{name}");
        }

        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", handle.value()));
        assert!(host.document().get_entity(handle).is_none());
        for (name, bytes) in [
            ("mleader-deleted.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()),
            ("mleader-deleted.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()),
        ] {
            assert!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle).is_none());
        }
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(handle), Some(&expected));
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(handle), Some(&created));
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn staged_python_table_lifecycle_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last_output = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();

        dispatch(&mut host, "PY_EVAL ocs.active_document.create_entity('Table', insertion_point={'x':5.0,'y':5.0,'z':0.0}).handle");
        let handle = host.document().entities().find_map(|entity| match entity {
            EntityType::Table(value) => Some(value.common.handle),
            _ => None,
        }).unwrap_or_else(|| panic!("Python did not create Table: {}", last_output(&host)));
        let created = host.document().get_entity(handle).unwrap().clone();
        let EntityType::Table(created_table) = &created else { unreachable!() };
        assert_eq!((created_table.rows.len(), created_table.columns.len()), (3, 3));
        assert_eq!(created_table.insertion_point, acadrust::types::Vector3::new(5.0, 5.0, 0.0));
        assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty(), "no canvas geometry");

        let script = std::env::temp_dir().join(format!("ocs_table_edit_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "doc = ocs.active_document\n",
            "t = doc.entities[{}]\n",
            "rows = t.rows\n",
            "cols = t.columns\n",
            "rows.append(dict(rows[0]))\n",
            "cols[0]['width'] = 30.0\n",
            "rows[1]['cells'][1]['contents'] = [{{'content_type': 'Value', 'scale': 1.0, 'text_height': 2.5,\n",
            "    'value': {{'value_type': 'String', 'raw_type_code': 4, 'text': 'Hello', 'formatted_value': 'Hello'}}}}]\n",
            "with doc.transaction('Edit table'):\n",
            "    t.rows = rows\n",
            "    t.columns = cols\n",
            "    t.merged_ranges = [{{'top_row': 0, 'left_col': 0, 'bottom_row': 0, 'right_col': 2}}]\n",
            "    t.break_spacing = 2.0\n",
            "doc.selection = [t]\n"
        ), handle.value())).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let expected = host.document().get_entity(handle).unwrap().clone();
        let EntityType::Table(edited) = &expected else { unreachable!() };
        assert_eq!(edited.rows.len(), 4, "edit failed: {}", last_output(&host));
        assert_eq!(edited.columns[0].width, 30.0);
        assert_eq!(edited.rows[1].cells[1].contents[0].value.text, "Hello");
        assert_eq!(edited.merged_ranges.len(), 1);
        assert_eq!(edited.break_spacing, 2.0);
        assert_eq!(edited.common, created_table.common);
        assert_eq!(edited.insertion_point, created_table.insertion_point);
        assert_eq!(edited.normal, created_table.normal);
        assert_eq!(edited.columns[1], created_table.columns[1]);
        assert_eq!(edited.rows[2], created_table.rows[2]);
        assert_eq!(edited.table_style_handle, created_table.table_style_handle);
        assert_eq!(host.selection(), vec![handle]);
        assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty());

        for (patch, message) in [
            ("'columns':[{'name':'A','width':0.0},{'width':1.0},{'width':1.0}]", "greater than zero"),
            ("'columns':[{'width':5.0}]", "cells but the table has 1 columns"),
            ("'columns':[]", "at least one row and one column"),
            ("'merged_ranges':[{'top_row':0,'left_col':0,'bottom_row':9,'right_col':0}]", "outside the"),
            ("'merged_ranges':[{'top_row':1,'left_col':0,'bottom_row':2,'right_col':1},{'top_row':2,'left_col':1,'bottom_row':3,'right_col':2}]", "overlaps"),
            ("'table_style_handle':999999", "does not exist"),
            ("'block_name':'x'", "read-only"),
            ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ("'break_spacing':-1.0", "non-negative"),
            ("'insertion_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
        ] {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject table', [{{'handle':{}, {patch}}}])", handle.value()));
            assert_eq!(host.document().get_entity(handle), Some(&expected), "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }
        dispatch(&mut host, &format!(concat!(
            "PY_EVAL ocs.update_many('Atomic', [{{'handle':{},'break_spacing':7.0}},",
            "{{'handle':{},'break_spacing':-1.0}}])"
        ), handle.value(), handle.value()));
        assert_eq!(host.document().get_entity(handle), Some(&expected));

        let mut gaps = Vec::new();
        let dwg = crate::io::load_bytes("table.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("table.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
            let Some(EntityType::Table(value)) = document.get_entity(handle) else {
                gaps.push(format!("{format}: Table missing"));
                continue;
            };
            if value.rows.len() != 4 { gaps.push(format!("{format}: rows {}", value.rows.len())); }
            if value.columns.first().map(|c| c.width) != Some(30.0) { gaps.push(format!("{format}: column width {:?}", value.columns.first().map(|c| c.width))); }
            let text = value.rows.get(1).and_then(|r| r.cells.get(1)).and_then(|c| c.contents.first()).map(|c| c.value.text.clone());
            if text.as_deref() != Some("Hello") { gaps.push(format!("{format}: cell text {text:?}")); }
            // DWG stores the ranges and DXF the origin cell's merge dimensions;
            // the DXF reader rebuilds the ranges from those, so both formats
            // come back with the merge in both forms.
            let origin = &value.rows[0].cells[0];
            if format == "DXF" && (origin.merge_width, origin.merge_height) != (3, 1) {
                gaps.push(format!("DXF: merge dims {}x{}", origin.merge_width, origin.merge_height));
            }
            if value.merged_ranges.len() != 1 {
                gaps.push(format!("{format}: merged_ranges {}", value.merged_ranges.len()));
            }
            if value.insertion_point != edited.insertion_point { gaps.push(format!("{format}: insertion {:?}", value.insertion_point)); }
        }
        assert!(gaps.is_empty(), "persistence gaps: {gaps:#?}");
        for (name, mut document) in [("table-reedit.dwg", dwg), ("table-reedit.dxf", dxf)] {
            let before = document.get_entity(handle).unwrap().clone();
            let mut after = before.clone();
            let EntityType::Table(value) = &mut after else { unreachable!() };
            value.columns[1].width = 12.0;
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
            ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after).unwrap();
            *document.get_entity_mut(handle).unwrap() = after;
            let bytes = if name.ends_with("dwg") {
                acadrust::DwgWriter::write_to_vec(&document).unwrap()
            } else {
                acadrust::DxfWriter::new(&document).write_to_vec().unwrap()
            };
            assert!(matches!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle),
                Some(EntityType::Table(value)) if value.columns[1].width == 12.0), "{name}");
        }

        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", handle.value()));
        assert!(host.document().get_entity(handle).is_none());
        for (name, bytes) in [
            ("table-deleted.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()),
            ("table-deleted.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()),
        ] {
            assert!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle).is_none());
        }
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 3);
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(handle), Some(&expected));
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(handle), Some(&created));
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
        app.redo_steps(3);
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }

    /// One scripted lifecycle shared by the mesh kinds: create, edit,
    /// reject, save/reopen both formats, re-edit, delete, undo/redo.
    struct MeshCase {
        create: &'static str,
        edit: &'static str,
        rejects: &'static [(&'static str, &'static str)],
        is_kind: fn(&EntityType) -> bool,
        digest: fn(&EntityType) -> String,
        reedit: fn(&mut EntityType),
        expect_created: &'static str,
        expect_edited: &'static str,
        expect_reedited: &'static str,
        /// Digest a DXF reopen yields when the engine loses a field (empty:
        /// same as DWG). A canary that fails once the engine round-trips it.
        expect_edited_dxf: &'static str,
        expect_reedited_dxf: &'static str,
        /// Same idea for DWG (empty: same as the in-session digest).
        expect_edited_dwg: &'static str,
        expect_reedited_dwg: &'static str,
    }

    fn run_mesh_lifecycle(case: &MeshCase) {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path),
            &mut host,
            crate::plugin::v4_support::notification_handler(),
        )
        .unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last_output = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let run_script = |host: &mut HostSession<'_>, tag: &str, body: &str| {
            let path = std::env::temp_dir().join(format!("ocs_mesh_{tag}_{}.py", std::process::id()));
            std::fs::write(&path, format!("def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\ndoc = ocs.active_document\n{body}")).unwrap();
            dispatch(host, &format!("PY_RUN {}", path.display()));
            let _ = std::fs::remove_file(&path);
        };

        // Substituted into Python string literals: on Windows a backslash path such as
        // `C:\Users\...` is read as escape sequences (`\U` is a unicode escape), and Windows
        // accepts forward slashes, so hand the script a forward-slash path.
        let tmp = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        let mut create_script = case.create.replace("TMPDIR", &tmp);
        // Insert is update-only: the host makes a block and one insert, and the
        // "create" script only has to exist.
        let make_polyline = create_script.contains("MAKEPOLYLINE");
        if make_polyline {
            // The legacy Polyline is update-only: the host makes one to edit.
            let points = [(0.0, 0.0, 0.0), (10.0, 0.0, 2.0), (10.0, 5.0, 4.0)]
                .map(|(x, y, z)| acadrust::types::Vector3::new(x, y, z));
            host.add_entity(EntityType::Polyline(acadrust::entities::Polyline::from_points(points.to_vec())));
        }
        let make_ole = create_script.contains("MAKEOLE");
        if make_ole {
            // An Ole2Frame is update-only: the host embeds a real picture.
            let mut png = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 120, 200, 255]))
                .write_to(&mut png, image::ImageFormat::Png)
                .unwrap();
            let picture = crate::io::ole_embed::EmbeddedImage {
                bytes: png.into_inner(),
                pixel_width: 4,
                pixel_height: 3,
                name: "audit.png".into(),
            };
            crate::io::ole_embed::add_embedded_image(
                host.document_mut(),
                &picture,
                acadrust::types::Vector3::new(10.0, 10.0, 0.0),
                20.0,
            )
            .unwrap();
        }
        let update_only = create_script.contains("MAKEINSERT") || make_polyline || make_ole;
        if update_only || create_script.contains("MAKEBLOCK") {
            let next = host.document().next_handle();
            let (record_handle, block_handle, end_handle) = (Handle::new(next), Handle::new(next + 1), Handle::new(next + 2));
            let mut record = acadrust::tables::BlockRecord::new("AUDITBLK");
            record.handle = record_handle;
            record.block_entity_handle = block_handle;
            record.block_end_handle = end_handle;
            host.document_mut().block_records.add(record).unwrap();
            let mut block = acadrust::entities::Block::new("AUDITBLK", acadrust::types::Vector3::ZERO);
            block.common.handle = block_handle;
            block.common.owner_handle = record_handle;
            host.document_mut().add_entity(EntityType::Block(block)).unwrap();
            let mut end = acadrust::entities::BlockEnd::new();
            end.common.handle = end_handle;
            end.common.owner_handle = record_handle;
            host.document_mut().add_entity(EntityType::BlockEnd(end)).unwrap();
            if update_only {
                let insert = acadrust::entities::Insert::new("AUDITBLK", acadrust::types::Vector3::new(1.0, 2.0, 0.0));
                host.add_entity(EntityType::Insert(insert));
            }
            create_script = create_script.replace("BLOCKRECORD", &record_handle.value().to_string());
        }
        let paper = host.document().block_records.iter().find(|r| r.is_paper_space()).map(|r| r.handle.value());
        let layer0 = host.document().layers.iter().find(|l| l.name == "0").map(|l| l.handle.value());
        if create_script.contains("VIEWPORTHANDLE") || create_script.contains("SCALEHANDLE") {
            // A view border ties to an existing viewport and scale.
            let owner = host.document().block_records.iter().find(|r| r.is_paper_space()).map(|r| r.handle).unwrap();
            let mut viewport = acadrust::entities::Viewport::new();
            viewport.common.owner_handle = owner;
            let viewport_handle = host.add_entity(EntityType::Viewport(viewport));
            let scale_handle = host.document_mut().allocate_handle();
            let mut scale = acadrust::objects::Scale::new("1:1", 1.0, 1.0);
            scale.handle = scale_handle;
            host.document_mut().objects.insert(scale_handle, acadrust::objects::ObjectType::Scale(scale));
            create_script = create_script
                .replace("VIEWPORTHANDLE", &viewport_handle.value().to_string())
                .replace("SCALEHANDLE", &scale_handle.value().to_string());
        }
        create_script = create_script
            .replace("PAPERSPACE", &paper.unwrap_or_default().to_string())
            .replace("LAYER0", &layer0.unwrap_or_default().to_string());
        if create_script.contains("DEFHANDLE") {
            // Scripts reference an underlay definition that the drawing owns.
            let handle = host.document_mut().allocate_handle();
            let mut definition = acadrust::objects::UnderlayDefinition::pdf("plan.pdf", "1");
            definition.handle = handle;
            host.document_mut().objects.insert(handle, acadrust::objects::ObjectType::UnderlayDefinition(definition));
            create_script = create_script.replace("DEFHANDLE", &handle.value().to_string());
        }
        run_script(&mut host, "create", &create_script);
        let handle = host.document().entities().find(|entity| (case.is_kind)(entity))
            .map(|entity| entity.common().handle)
            .unwrap_or_else(|| panic!("Python did not create the mesh: {}", last_output(&host)));
        let created = host.document().get_entity(handle).unwrap().clone();
        assert_eq!((case.digest)(&created), case.expect_created);
        // A paper-space viewport draws only on its layout tab, so the canvas
        // oracle first activates the layout that owns it.
        if matches!(created, EntityType::Viewport(_)) {
            let owner = created.common().owner_handle;
            let layout = host.document().objects.values().find_map(|object| match object {
                acadrust::objects::ObjectType::Layout(l) if l.block_record == owner => Some(l.name.clone()),
                _ => None,
            }).expect("the viewport's owner is a layout block");
            host.app.tabs[0].scene.set_current_layout(layout);
        }
        let has_wires = true;
        assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty(), "no canvas geometry");

        run_script(&mut host, "edit", &case.edit.replace("HANDLE", &handle.value().to_string()).replace("TMPDIR", &tmp).replace("LAYER0", &layer0.unwrap_or_default().to_string()));
        let expected = host.document().get_entity(handle).unwrap().clone();
        assert_eq!((case.digest)(&expected), case.expect_edited, "edit failed: {}", last_output(&host));
        assert_eq!(expected.common(), created.common());
        assert_eq!(host.selection(), vec![handle]);
        if has_wires {
            assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty());
        }

        for (patch, message) in case.rejects {
            dispatch(&mut host, &format!(
                "PY_EVAL ocs.update_many('Reject mesh', [{{'handle':{}, {patch}}}])", handle.value()));
            assert_eq!(host.document().get_entity(handle), Some(&expected), "{patch}");
            assert!(last_output(&host).contains(message), "{patch}: {}", last_output(&host));
        }
        let (first_patch, _) = case.rejects[0];
        dispatch(&mut host, &format!(
            "PY_EVAL ocs.update_many('Atomic', [{{'handle':{h}, 'layer':'Other'}}, {{'handle':{h}, {first_patch}}}])",
            h = handle.value()));
        assert_eq!(host.document().get_entity(handle), Some(&expected));

        let dwg = crate::io::load_bytes("mesh.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("mesh.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        let mut gaps = Vec::new();
        for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
            let expected_digest = if format == "DXF" && !case.expect_edited_dxf.is_empty() {
                case.expect_edited_dxf
            } else if format == "DWG" && !case.expect_edited_dwg.is_empty() {
                case.expect_edited_dwg
            } else {
                case.expect_edited
            };
            match document.get_entity(handle) {
                Some(entity) if (case.digest)(entity) == expected_digest => {}
                Some(entity) => gaps.push(format!("{format}: {}", (case.digest)(entity))),
                None => gaps.push(format!("{format}: entity missing")),
            }
        }
        assert!(gaps.is_empty(), "persistence gaps (expected {} / DXF {:?}): {gaps:#?}", case.expect_edited, case.expect_edited_dxf);
        for (name, mut document) in [("mesh-reedit.dwg", dwg), ("mesh-reedit.dxf", dxf)] {
            let before = document.get_entity(handle).unwrap().clone();
            let mut after = before.clone();
            (case.reedit)(&mut after);
            ocs_plugin_api::entity_coverage::validate_entity_mutation(&before, &after).unwrap();
            ocs_plugin_api::entity_coverage::validate_canvas_entity_references(&document, &after).unwrap();
            *document.get_entity_mut(handle).unwrap() = after;
            let bytes = if name.ends_with("dwg") {
                acadrust::DwgWriter::write_to_vec(&document).unwrap()
            } else {
                acadrust::DxfWriter::new(&document).write_to_vec().unwrap()
            };
            let reopened = crate::io::load_bytes(name, bytes).unwrap();
            let expected_digest = if name.ends_with("dxf") && !case.expect_reedited_dxf.is_empty() {
                case.expect_reedited_dxf
            } else if name.ends_with("dwg") && !case.expect_reedited_dwg.is_empty() {
                case.expect_reedited_dwg
            } else {
                case.expect_reedited
            };
            assert_eq!((case.digest)(reopened.get_entity(handle).unwrap()), expected_digest, "{name}");
        }

        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", handle.value()));
        assert!(host.document().get_entity(handle).is_none());
        for (name, bytes) in [
            ("mesh-deleted.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()),
            ("mesh-deleted.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()),
        ] {
            assert!(crate::io::load_bytes(name, bytes).unwrap().get_entity(handle).is_none());
        }
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), if update_only { 2 } else { 3 });
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(handle), Some(&expected));
        app.undo_steps(1);
        assert_eq!(app.tabs[0].scene.document.get_entity(handle), Some(&created));
        if !update_only {
            app.undo_steps(1);
            assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
        }
        app.redo_steps(if update_only { 2 } else { 3 });
        assert!(app.tabs[0].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn staged_python_polygon_mesh_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "verts = [{'location': P(i, j, 0.0)} for i in range(3) for j in range(3)]\n",
                "doc.create_entity('PolygonMesh', m_vertex_count=3, n_vertex_count=3, vertices=verts)\n"),
            edit: concat!(
                "mesh = doc.entities[HANDLE]\n",
                "verts = mesh.vertices\n",
                "verts[4]['location'] = P(1.0, 1.0, 2.5)\n",
                "with doc.transaction('Edit mesh'):\n",
                "    mesh.vertices = verts\n",
                "    mesh.m_smooth_density = 6\n",
                "    mesh.n_smooth_density = 6\n",
                "    mesh.smooth_type = 'Cubic'\n",
                "doc.selection = [mesh]\n"),
            rejects: &[
                ("'m_vertex_count':4", "requires 12"),
                ("'n_vertex_count':1", "at least 2"),
                ("'m_smooth_density':-1", "non-negative"),
                ("'elevation':float('nan')", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
                ("'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}}]*9", "finite"),
                ("'smooth_type':'Spline'", "unsupported"),
            ],
            is_kind: |entity| matches!(entity, EntityType::PolygonMesh(_)),
            digest: |entity| match entity {
                EntityType::PolygonMesh(m) => format!("{}x{} v{} z{} d{}/{} {:?}", m.m_vertex_count, m.n_vertex_count,
                    m.vertices.len(), m.vertices[4].location.z, m.m_smooth_density, m.n_smooth_density, m.smooth_type),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::PolygonMesh(m) = entity { m.vertices[0].location.z = 1.0; },
            expect_created: "3x3 v9 z0 d0/0 NoSmooth",
            expect_edited: "3x3 v9 z2.5 d6/6 Cubic",
            expect_reedited: "3x3 v9 z2.5 d6/6 Cubic",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_polyface_mesh_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "verts = [{'location': P(0.0, 0.0, 0.0)}, {'location': P(4.0, 0.0, 0.0)},\n",
                "         {'location': P(4.0, 4.0, 0.0)}, {'location': P(0.0, 4.0, 0.0)}]\n",
                "faces = [{'index1': 1, 'index2': 2, 'index3': 3}, {'index1': 1, 'index2': 3, 'index3': 4}]\n",
                "doc.create_entity('PolyfaceMesh', vertices=verts, faces=faces)\n"),
            edit: concat!(
                "mesh = doc.entities[HANDLE]\n",
                "verts = mesh.vertices\n",
                "verts[2]['location'] = P(4.0, 4.0, 3.0)\n",
                "verts.append({'location': P(2.0, 6.0, 0.0)})\n",
                "faces = mesh.faces\n",
                "faces.append({'index1': 3, 'index2': -4, 'index3': 5})\n",
                "with doc.transaction('Edit mesh'):\n",
                "    mesh.vertices = verts\n",
                "    mesh.faces = faces\n",
                "doc.selection = [mesh]\n"),
            rejects: &[
                ("'faces':[{'index1':1,'index2':2,'index3':9}]", "references vertex 9"),
                ("'faces':[{'index1':1,'index2':2}]", "at least 3 vertex indices"),
                ("'faces':[]", "at least 1 face"),
                ("'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 3"),
                ("'thickness':float('inf')", "finite"),
                ("'seqend_handle':5", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::PolyfaceMesh(_)),
            digest: |entity| match entity {
                EntityType::PolyfaceMesh(m) => format!("v{} f{} z{} last {:?}", m.vertices.len(), m.faces.len(),
                    m.vertices[2].location.z,
                    m.faces.last().map(|f| (f.index1, f.index2, f.index3))),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::PolyfaceMesh(m) = entity { m.vertices[0].location.z = 1.0; },
            expect_created: "v4 f2 z0 last Some((1, 3, 4))",
            expect_edited: "v5 f3 z3 last Some((3, -4, 5))",
            expect_reedited: "v5 f3 z3 last Some((3, -4, 5))",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_mesh_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "verts = [P(0.0, 0.0, 0.0), P(4.0, 0.0, 0.0), P(4.0, 4.0, 0.0), P(0.0, 4.0, 0.0)]\n",
                "doc.create_entity('Mesh', vertices=verts, faces=[{'vertices': [0, 1, 2, 3]}],\n",
                "    edges=[{'start': 0, 'end': 1, 'crease': 1.5}])\n"),
            edit: concat!(
                "mesh = doc.entities[HANDLE]\n",
                "verts = mesh.vertices\n",
                "verts.append(P(2.0, 6.0, 0.0))\n",
                "faces = mesh.faces\n",
                "faces.append({'vertices': [3, 2, 4]})\n",
                "with doc.transaction('Edit mesh'):\n",
                "    mesh.vertices = verts\n",
                "    mesh.faces = faces\n",
                "    mesh.subdivision_level = 1\n",
                "doc.selection = [mesh]\n"),
            rejects: &[
                ("'faces':[{'vertices':[0,1,9]}]", "references vertex 9"),
                ("'faces':[{'vertices':[0,1]}]", "at least 3 vertices"),
                ("'edges':[{'start':0,'end':9}]", "out of range"),
                ("'edges':[{'start':2,'end':2}]", "identical endpoints"),
                ("'edges':[{'start':0,'end':1,'crease':-1.0}]", "non-negative"),
                ("'subdivision_level':-1", "non-negative"),
                ("'version':3", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Mesh(_)),
            digest: |entity| match entity {
                EntityType::Mesh(m) => format!("v{} f{} e{} sub{} crease {:?}", m.vertices.len(), m.faces.len(),
                    m.edges.len(), m.subdivision_level, m.edges.first().and_then(|e| e.crease)),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Mesh(m) = entity { m.vertices[0].z = 1.0; },
            expect_created: "v4 f1 e1 sub0 crease Some(1.5)",
            expect_edited: "v5 f2 e1 sub1 crease Some(1.5)",
            expect_reedited: "v5 f2 e1 sub1 crease Some(1.5)",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_helix_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('Helix', axis_base_point=P(0.0, 0.0, 0.0), axis_vector=P(0.0, 0.0, 1.0),\n",
                "    start_point=P(5.0, 0.0, 0.0), radius=5.0, turns=3.0, turn_height=2.0)\n"),
            edit: concat!(
                "helix = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit helix'):\n",
                "    helix.turns = 5.0\n",
                "    helix.handedness = False\n",
                "doc.selection = [helix]\n"),
            rejects: &[
                ("'radius':0.0", "greater than zero"),
                ("'turns':-1.0", "greater than zero"),
                ("'turn_height':float('nan')", "greater than zero"),
                ("'axis_vector':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
                ("'start_point':{'x':0.0,'y':0.0,'z':3.0}", "on the axis"),
                ("'spline':{}", "read-only"),
                ("'major_version':1", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Helix(_)),
            digest: |entity| match entity {
                EntityType::Helix(h) => format!("r{} t{} h{} endz{:.1} ccw{} cp{}", h.radius, h.turns, h.turn_height,
                    h.spline.control_points.last().map_or(f64::NAN, |p| p.z), h.handedness,
                    !h.spline.control_points.is_empty()),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Helix(h) = entity { h.turns = 6.0; },
            expect_created: "r5 t3 h2 endz6.0 ccwtrue cptrue",
            expect_edited: "r5 t5 h2 endz10.0 ccwfalse cptrue",
            expect_reedited: "r5 t6 h2 endz10.0 ccwfalse cptrue",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_raster_image_lifecycle_over_real_ipc() {
        image::RgbaImage::from_pixel(8, 4, image::Rgba([200, 30, 30, 255]))
            .save(std::env::temp_dir().join("ocs_raster_test.png"))
            .unwrap();
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('RasterImage', file_path='TMPDIR/ocs_raster_test.png',\n",
                "    insertion_point=P(10.0, 20.0, 0.0), u_vector=P(0.5, 0.0, 0.0), v_vector=P(0.0, 0.5, 0.0))\n"),
            edit: concat!(
                "img = doc.entities[HANDLE]\n",
                "cb = img.clip_boundary\n",
                "cb['vertices'] = [{'x': 1.0, 'y': 1.0}, {'x': 7.0, 'y': 3.0}]\n",
                "with doc.transaction('Edit image'):\n",
                "    img.insertion_point = (12.0, 22.0, 0.0)\n",
                "    img.u_vector = (1.0, 0.0, 0.0)\n",
                "    img.brightness = 70\n",
                "    img.contrast = 40\n",
                "    img.fade = 10\n",
                "    img.clip_boundary = cb\n",
                "    img.clipping_enabled = True\n",
                "doc.selection = [img]\n"),
            rejects: &[
                ("'file_path':'other.png'", "fixed at creation"),
                ("'brightness':101", "within 0..=100"),
                ("'u_vector':{'x':0.0,'y':0.0,'z':0.0}", "not parallel"),
                ("'v_vector':{'x':2.0,'y':0.0,'z':0.0}", "not parallel"),
                ("'size':{'x':5.0,'y':5.0}", "read-only"),
                ("'definition_handle':5", "read-only"),
                ("'flags':64", "unknown or out-of-range bits"),
                ("'clip_boundary':{'vertices':[]}", "at least 2"),
            ],
            is_kind: |entity| matches!(entity, EntityType::RasterImage(_)),
            digest: |entity| match entity {
                EntityType::RasterImage(i) => format!("{} {:.1},{:.1} u{:.1} b{} c{} f{} clip{} n{} {}x{} def{}",
                    std::path::Path::new(&i.file_path).file_name().unwrap().to_string_lossy(),
                    i.insertion_point.x, i.insertion_point.y, i.u_vector.x, i.brightness, i.contrast, i.fade,
                    i.clipping_enabled, i.clip_boundary.vertices.len(), i.size.x, i.size.y, i.definition_handle.is_some()),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::RasterImage(i) = entity { i.brightness = 80; },
            expect_created: "ocs_raster_test.png 10.0,20.0 u0.5 b50 c50 f0 clipfalse n2 8x4 deftrue",
            expect_edited: "ocs_raster_test.png 12.0,22.0 u1.0 b70 c40 f10 cliptrue n2 8x4 deftrue",
            expect_reedited: "ocs_raster_test.png 12.0,22.0 u1.0 b80 c40 f10 cliptrue n2 8x4 deftrue",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_wipeout_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('Wipeout', insertion_point=P(0.0, 0.0, 0.0),\n",
                "    u_vector=P(10.0, 0.0, 0.0), v_vector=P(0.0, 6.0, 0.0))\n"),
            edit: concat!(
                "w = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit wipeout'):\n",
                "    w.clip_type = 'Polygonal'\n",
                "    w.clip_boundary_vertices = [{'x': -0.5, 'y': -0.5}, {'x': 0.5, 'y': -0.5}, {'x': 0.0, 'y': 0.5}]\n",
                "    w.insertion_point = (2.0, 3.0, 0.0)\n",
                "    w.clip_mode = 'Inside'\n",
                "doc.selection = [w]\n"),
            rejects: &[
                ("'clip_boundary_vertices':[{'x':0.0,'y':0.0}]", "at least 3"),
                ("'clip_type':'Rectangular'", "exactly 2"),
                ("'u_vector':{'x':0.0,'y':0.0,'z':0.0}", "not parallel"),
                ("'brightness':200", "within 0..=100"),
                ("'size':{'x':0.0,'y':1.0}", "greater than zero"),
                ("'insertion_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'definition_handle':5", "read-only"),
                ("'clip_type':'Circle'", "unsupported"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Wipeout(_)),
            digest: |entity| match entity {
                EntityType::Wipeout(w) => format!("{:?} {:?} n{} at{:.1},{:.1} u{:.1} v{:.1}", w.clip_type, w.clip_mode,
                    w.clip_boundary_vertices.len(), w.insertion_point.x, w.insertion_point.y, w.u_vector.x, w.v_vector.y),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Wipeout(w) = entity { w.u_vector.x = 12.0; },
            expect_created: "Rectangular Outside n2 at0.0,0.0 u10.0 v6.0",
            expect_edited: "Polygonal Inside n3 at2.0,3.0 u10.0 v6.0",
            expect_reedited: "Polygonal Inside n3 at2.0,3.0 u12.0 v6.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_underlay_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('Underlay', underlay_type='Pdf', definition_handle=DEFHANDLE,\n",
                "    insertion_point=P(5.0, 5.0, 0.0), x_scale=2.0, y_scale=2.0)\n"),
            edit: concat!(
                "u = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit underlay'):\n",
                "    u.insertion_point = (8.0, 9.0, 0.0)\n",
                "    u.x_scale = 3.0\n",
                "    u.y_scale = 3.0\n",
                "    u.rotation = 0.5\n",
                "    u.contrast = 70\n",
                "    u.fade = 20\n",
                "    u.clip_boundary_vertices = [{'x': 0.0, 'y': 0.0}, {'x': 4.0, 'y': 3.0}]\n",
                "doc.selection = [u]\n"),
            rejects: &[
                ("'definition_handle':999999", "does not exist"),
                ("'underlay_type':'Dwf'", "does not match its definition"),
                ("'x_scale':0.0", "nonzero"),
                ("'rotation':float('nan')", "finite"),
                ("'contrast':101", "within 0..=100"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
                ("'clip_boundary_vertices':[{'x':1.0,'y':1.0}]", "at least 2"),
                ("'flags':64", "unknown or out-of-range bits"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Underlay(_)),
            digest: |entity| match entity {
                EntityType::Underlay(u) => format!("{:?} at{:.1},{:.1} s{:.1} r{:.1} c{} f{} clip{}", u.underlay_type,
                    u.insertion_point.x, u.insertion_point.y, u.x_scale, u.rotation, u.contrast, u.fade,
                    u.clip_boundary_vertices.len()),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Underlay(u) = entity { u.x_scale = 4.0; },
            expect_created: "Pdf at5.0,5.0 s2.0 r0.0 c100 f0 clip0",
            expect_edited: "Pdf at8.0,9.0 s3.0 r0.5 c70 f20 clip2",
            expect_reedited: "Pdf at8.0,9.0 s4.0 r0.5 c70 f20 clip2",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_viewport_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('Viewport', owner_handle=PAPERSPACE, center=P(100.0, 100.0, 0.0),\n",
                "    width=80.0, height=60.0, view_center=P(0.0, 0.0, 0.0), view_height=50.0)\n"),
            edit: concat!(
                "vp = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit viewport'):\n",
                "    vp.center = (120.0, 110.0, 0.0)\n",
                "    vp.width = 90.0\n",
                "    vp.view_height = 75.0\n",
                "    vp.twist_angle = 0.25\n",
                "    vp.frozen_layers = [LAYER0]\n",
                "doc.selection = [vp]\n"),
            rejects: &[
                ("'width':0.0", "greater than zero"),
                ("'view_height':float('nan')", "greater than zero"),
                ("'view_direction':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
                ("'frozen_layers':[999999]", "does not exist"),
                ("'circle_sides':2", "at least 3"),
                ("'id':7", "read-only"),
                ("'clip_boundary_handle':5", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Viewport(_)),
            digest: |entity| match entity {
                EntityType::Viewport(v) => format!("at{:.1},{:.1} {:.1}x{:.1} vh{:.1} tw{:.2} frozen{}", v.center.x, v.center.y,
                    v.width, v.height, v.view_height, v.twist_angle, v.frozen_layers.len()),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Viewport(v) = entity { v.width = 95.0; },
            expect_created: "at100.0,100.0 80.0x60.0 vh50.0 tw0.00 frozen0",
            expect_edited: "at120.0,110.0 90.0x60.0 vh75.0 tw0.25 frozen1",
            expect_reedited: "at120.0,110.0 95.0x60.0 vh75.0 tw0.25 frozen1",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_view_border_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('ViewBorder', min=[0.0, 0.0], max=[40.0, 30.0], center=[20.0, 15.0], scale=1.0,\n",
                "    active_viewport=VIEWPORTHANDLE, scale_handle=SCALEHANDLE)\n"),
            edit: concat!(
                "vb = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit view border'):\n",
                "    vb.max = [60.0, 45.0]\n",
                "    vb.center = [30.0, 22.5]\n",
                "    vb.scale = 2.0\n",
                "    vb.rotation_angle = 0.3\n",
                "doc.selection = [vb]\n"),
            rejects: &[
                ("'max':[-1.0, 5.0]", "below max"),
                ("'scale':0.0", "greater than zero"),
                ("'rotation_angle':float('nan')", "finite"),
                ("'center':[float('inf'), 0.0]", "finite"),
                ("'active_viewport':999999", "does not exist"),
                ("'scale_handle':999999", "does not exist"),
                ("'version':3", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::ViewBorder(_)),
            digest: |entity| match entity {
                EntityType::ViewBorder(v) => format!("max{:.1},{:.1} c{:.1},{:.1} s{:.1} r{:.1}", v.max[0], v.max[1],
                    v.center[0], v.center[1], v.scale, v.rotation_angle),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::ViewBorder(v) = entity { v.scale = 3.0; },
            expect_created: "max40.0,30.0 c20.0,15.0 s1.0 r0.0",
            expect_edited: "max60.0,45.0 c30.0,22.5 s2.0 r0.3",
            expect_reedited: "max60.0,45.0 c30.0,22.5 s3.0 r0.3",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn staged_python_light_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('Light', name='Key', light_type=3, position=P(10.0, 10.0, 10.0),\n",
                "    target=P(0.0, 0.0, 0.0), intensity=1.5, hotspot_angle=0.4, falloff_angle=0.8,\n",
                "    light_color={'kind': 'Rgb', 'value': {'r': 255, 'g': 240, 'b': 200}})\n"),
            edit: concat!(
                "lt = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit light'):\n",
                "    lt.intensity = 3.0\n",
                "    lt.target = (5.0, 5.0, 0.0)\n",
                "    lt.position = (12.0, 8.0, 10.0)\n",
                "    lt.cast_shadows = True\n",
                "doc.selection = [lt]\n"),
            rejects: &[
                ("'light_type':9", "must be 1 (distant)"),
                ("'intensity':-1.0", "non-negative"),
                ("'target':{'x':12.0,'y':8.0,'z':10.0}", "different from its position"),
                ("'hotspot_angle':2.0", "must not exceed falloff_angle"),
                ("'position':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'name':''", "empty"),
                ("'class_version':2", "read-only"),
                ("'light_color':{'kind':'Index','value':300}", "Color"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Light(_)),
            digest: |entity| match entity {
                EntityType::Light(l) => format!("{} t{} i{:.1} pos{:.0},{:.0},{:.0} tgt{:.0},{:.0} sh{} {:?}", l.name, l.light_type,
                    l.intensity, l.position.x, l.position.y, l.position.z, l.target.x, l.target.y, l.cast_shadows, l.light_color),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Light(l) = entity { l.intensity = 4.0; },
            expect_created: "Key t3 i1.5 pos10,10,10 tgt0,0 shfalse Rgb { r: 255, g: 240, b: 200 }",
            expect_edited: "Key t3 i3.0 pos12,8,10 tgt5,5 shtrue Rgb { r: 255, g: 240, b: 200 }",
            expect_reedited: "Key t3 i4.0 pos12,8,10 tgt5,5 shtrue Rgb { r: 255, g: 240, b: 200 }",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_point_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Point', location=P(1.0, 2.0, 3.0))",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.location = (4.0, 5.0, 6.0)\n", "    e.thickness = 2.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'location':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'thickness':float('inf')", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Point(_)),
            digest: |entity| match entity {
                EntityType::Point(v) => format!("{:.1},{:.1},{:.1} th{:.1}", v.location.x, v.location.y, v.location.z, v.thickness),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Point(v) = entity { v.location.z = 9.0; },
            expect_created: "1.0,2.0,3.0 th0.0",
            expect_edited: "4.0,5.0,6.0 th2.0",
            expect_reedited: "4.0,5.0,9.0 th2.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_line_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Line', start=P(0.0, 0.0, 0.0), end=P(10.0, 0.0, 0.0))",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.end = (10.0, 5.0, 0.0)\n", "    e.thickness = 1.5\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'start':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'thickness':float('inf')", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Line(_)),
            digest: |entity| match entity {
                EntityType::Line(v) => format!("{:.1},{:.1} th{:.1}", v.end.x, v.end.y, v.thickness),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Line(v) = entity { v.start.x = 1.0; },
            expect_created: "10.0,0.0 th0.0",
            expect_edited: "10.0,5.0 th1.5",
            expect_reedited: "10.0,5.0 th1.5",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_circle_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Circle', center=P(0.0, 0.0, 0.0), radius=5.0)",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.radius = 7.0\n", "    e.center = (1.0, 1.0, 0.0)\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'radius':0.0", "radius"),
                ("'radius':-3.0", "radius"),
                ("'center':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Circle(_)),
            digest: |entity| match entity {
                EntityType::Circle(v) => format!("{:.1},{:.1} r{:.1}", v.center.x, v.center.y, v.radius),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Circle(v) = entity { v.radius = 8.0; },
            expect_created: "0.0,0.0 r5.0",
            expect_edited: "1.0,1.0 r7.0",
            expect_reedited: "1.0,1.0 r8.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_arc_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Arc', center=P(0.0, 0.0, 0.0), radius=5.0, start_angle=0.0, end_angle=1.5)",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.end_angle = 2.0\n", "    e.radius = 6.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'radius':0.0", "radius"),
                ("'end_angle':float('nan')", "finite"),
                ("'center':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Arc(_)),
            digest: |entity| match entity {
                EntityType::Arc(v) => format!("r{:.1} {:.1}..{:.1}", v.radius, v.start_angle, v.end_angle),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Arc(v) = entity { v.start_angle = 0.5; },
            expect_created: "r5.0 0.0..1.5",
            expect_edited: "r6.0 0.0..2.0",
            expect_reedited: "r6.0 0.5..2.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_ellipse_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Ellipse', center=P(0.0, 0.0, 0.0), major_axis=P(10.0, 0.0, 0.0), minor_axis_ratio=0.5)",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.minor_axis_ratio = 0.25\n", "    e.end_parameter = 3.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'minor_axis_ratio':0.0", "ratio"),
                ("'minor_axis_ratio':2.0", "ratio"),
                ("'major_axis':{'x':0.0,'y':0.0,'z':0.0}", "axis"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Ellipse(_)),
            digest: |entity| match entity {
                EntityType::Ellipse(v) => format!("q{:.2} ..{:.1}", v.minor_axis_ratio, v.end_parameter),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Ellipse(v) = entity { v.minor_axis_ratio = 0.75; },
            expect_created: "q0.50 ..6.3",
            expect_edited: "q0.25 ..3.0",
            expect_reedited: "q0.75 ..3.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_ray_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Ray', base_point=P(0.0, 0.0, 0.0), direction=P(1.0, 0.0, 0.0))",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.base_point = (2.0, 2.0, 0.0)\n", "    e.direction = (0.0, 1.0, 0.0)\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'direction':{'x':2.0,'y':0.0,'z':0.0}", "unit"),
                ("'base_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Ray(_)),
            digest: |entity| match entity {
                EntityType::Ray(v) => format!("{:.1},{:.1} d{:.1},{:.1}", v.base_point.x, v.base_point.y, v.direction.x, v.direction.y),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Ray(v) = entity { v.base_point.x = 3.0; },
            expect_created: "0.0,0.0 d1.0,0.0",
            expect_edited: "2.0,2.0 d0.0,1.0",
            expect_reedited: "3.0,2.0 d0.0,1.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_xline_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('XLine', base_point=P(0.0, 0.0, 0.0), direction=P(1.0, 0.0, 0.0))",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.base_point = (2.0, 2.0, 0.0)\n", "    e.direction = (0.0, 1.0, 0.0)\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'direction':{'x':2.0,'y':0.0,'z':0.0}", "unit"),
                ("'base_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::XLine(_)),
            digest: |entity| match entity {
                EntityType::XLine(v) => format!("{:.1},{:.1} d{:.1},{:.1}", v.base_point.x, v.base_point.y, v.direction.x, v.direction.y),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::XLine(v) = entity { v.base_point.x = 3.0; },
            expect_created: "0.0,0.0 d1.0,0.0",
            expect_edited: "2.0,2.0 d0.0,1.0",
            expect_reedited: "3.0,2.0 d0.0,1.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_solid_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Solid', first_corner=P(0.0, 0.0, 0.0), second_corner=P(4.0, 0.0, 0.0), third_corner=P(0.0, 4.0, 0.0), fourth_corner=P(4.0, 4.0, 0.0))",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.fourth_corner = (5.0, 5.0, 0.0)\n", "    e.thickness = 2.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'first_corner':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'thickness':float('inf')", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Solid(_)),
            digest: |entity| match entity {
                EntityType::Solid(v) => format!("{:.1},{:.1} th{:.1}", v.fourth_corner.x, v.fourth_corner.y, v.thickness),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Solid(v) = entity { v.first_corner.x = 1.0; },
            expect_created: "4.0,4.0 th0.0",
            expect_edited: "5.0,5.0 th2.0",
            expect_reedited: "5.0,5.0 th2.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_face3d_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Face3D', first_corner=P(0.0, 0.0, 0.0), second_corner=P(4.0, 0.0, 0.0), third_corner=P(4.0, 4.0, 1.0), fourth_corner=P(0.0, 4.0, 1.0))",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.third_corner = (4.0, 4.0, 2.0)\n", "    e.invisible_edges = 3\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'first_corner':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'invisible_edges':64", "bits"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Face3D(_)),
            digest: |entity| match entity {
                EntityType::Face3D(v) => format!("z{:.1} inv{}", v.third_corner.z, v.invisible_edges.bits()),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Face3D(v) = entity { v.first_corner.x = 1.0; },
            expect_created: "z1.0 inv0",
            expect_edited: "z2.0 inv3",
            expect_reedited: "z2.0 inv3",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_text_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('Text', text='Hello', insertion=P(1.0, 2.0, 0.0), height=2.5)",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.text = 'World'\n", "    e.rotation = 0.5\n", "    e.height = 3.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'height':0.0", "height"),
                ("'height':-1.0", "height"),
                ("'insertion':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Text(_)),
            digest: |entity| match entity {
                EntityType::Text(v) => format!("{} h{:.1} r{:.1}", v.value, v.height, v.rotation),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Text(v) = entity { v.height = 4.0; },
            expect_created: "Hello h2.5 r0.0",
            expect_edited: "World h3.0 r0.5",
            expect_reedited: "World h4.0 r0.5",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_mtext_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "doc.create_entity('MText', text='Line one', insertion=P(1.0, 2.0, 0.0), height=2.5, rectangle_width=30.0)",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.text = 'Line two'\n", "    e.rectangle_width = 40.0\n", "    e.rotation = 0.5\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'height':0.0", "height"),
                ("'rectangle_width':-1.0", "width"),
                ("'insertion':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::MText(_)),
            digest: |entity| match entity {
                EntityType::MText(v) => format!("{} w{:.1} r{:.1}", v.value, v.rectangle_width, v.rotation),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::MText(v) = entity { v.rectangle_width = 50.0; },
            expect_created: "Line one w30.0 r0.0",
            expect_edited: "Line two w40.0 r0.5",
            expect_reedited: "Line two w50.0 r0.5",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_lwpolyline_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!("doc.create_entity('LwPolyline', vertices=[{'location': {'x': 0.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 5.0}}])\n"),
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 0.0}}, {'location': {'x': 10.0, 'y': 8.0}, 'bulge': 0.5}, {'location': {'x': 0.0, 'y': 8.0}}]\n", "    e.is_closed = True\n", "    e.constant_width = 0.5\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'vertices':[{'location':{'x':0.0,'y':0.0}}]", "at least 2"),
                ("'constant_width':-1.0", "non-negative"),
                ("'vertices':[{'location':{'x':float('nan'),'y':0.0}},{'location':{'x':1.0,'y':0.0}}]", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::LwPolyline(_)),
            digest: |entity| match entity {
                EntityType::LwPolyline(v) => format!("n{} closed{} w{:.1} bulge{:.1}", v.vertices.len(), v.is_closed, v.constant_width, v.vertices.get(2).map_or(0.0, |x| x.bulge)),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::LwPolyline(v) = entity { v.constant_width = 1.0; },
            expect_created: "n3 closedfalse w0.0 bulge0.0",
            expect_edited: "n4 closedtrue w0.5 bulge0.5",
            expect_reedited: "n4 closedtrue w1.0 bulge0.5",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_polyline2d_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!("doc.create_entity('Polyline2D', vertices=[{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 5.0, 'z': 0.0}}])\n"),
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 8.0, 'z': 0.0}}, {'location': {'x': 0.0, 'y': 8.0, 'z': 0.0}}]\n", "    e.closed = True\n", "    e.thickness = 1.0\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2"),
                ("'thickness':float('inf')", "finite"),
                ("'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}},{'location':{'x':1.0,'y':0.0,'z':0.0}}]", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Polyline2D(_)),
            digest: |entity| match entity {
                EntityType::Polyline2D(v) => format!("n{} closed{} th{:.1}", v.vertices.len(), v.flags.is_closed(), v.thickness),
                other => format!("wrong kind {}", format!("{other:?}").split('(').next().unwrap()),
            },
            reedit: |entity| if let EntityType::Polyline2D(v) = entity { v.thickness = 2.0; },
            expect_created: "n3 closedfalse th0.0",
            expect_edited: "n4 closedtrue th1.0",
            expect_reedited: "n4 closedtrue th2.0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "wrong kind LwPolyline",
            expect_reedited_dxf: "wrong kind LwPolyline",
        });
    }

    #[test]
    fn audit_python_polyline_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: "# MAKEPOLYLINE
",
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.vertices = [{'location': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'location': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'location': {'x': 10.0, 'y': 8.0, 'z': 6.0}}, {'location': {'x': 0.0, 'y': 8.0, 'z': 6.0}}]\n", "    e.closed = True\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'vertices':[{'location':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2"),
                ("'vertices':[{'location':{'x':float('nan'),'y':0.0,'z':0.0}},{'location':{'x':1.0,'y':0.0,'z':0.0}}]", "finite"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Polyline(_)),
            digest: |entity| match entity {
                EntityType::Polyline(v) => format!("n{} closed{} lastz{:.1}", v.vertices.len(), v.flags.is_closed(), v.vertices.last().map_or(0.0, |x| x.location.z)),
                other => format!("wrong kind {}", format!("{other:?}").split('(').next().unwrap()),
            },
            reedit: |entity| if let EntityType::Polyline(v) = entity { v.vertices[0].location.z = 1.0; },
            expect_created: "n3 closedfalse lastz4.0",
            expect_edited: "n4 closedtrue lastz6.0",
            expect_reedited: "n4 closedtrue lastz6.0",
            expect_edited_dwg: "wrong kind Polyline3D",
            expect_reedited_dwg: "wrong kind Polyline3D",
            expect_edited_dxf: "wrong kind Polyline3D",
            expect_reedited_dxf: "wrong kind Polyline3D",
        });
    }

    #[test]
    fn audit_python_polyline3d_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!("doc.create_entity('Polyline3D', vertices=[{'position': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'position': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'position': {'x': 10.0, 'y': 5.0, 'z': 4.0}}])\n"),
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.vertices = [{'position': {'x': 0.0, 'y': 0.0, 'z': 0.0}}, {'position': {'x': 10.0, 'y': 0.0, 'z': 2.0}}, {'position': {'x': 10.0, 'y': 8.0, 'z': 6.0}}, {'position': {'x': 0.0, 'y': 8.0, 'z': 6.0}}]\n", "    e.elevation = 1.5\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'vertices':[{'position':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2"),
                ("'elevation':float('inf')", "finite"),
                ("'vertices':[{'position':{'x':float('nan'),'y':0.0,'z':0.0}},{'position':{'x':1.0,'y':0.0,'z':0.0}}]", "finite"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Polyline3D(_)),
            digest: |entity| match entity {
                EntityType::Polyline3D(v) => format!("n{} el{:.1} lastz{:.1}", v.vertices.len(), v.elevation, v.vertices.last().map_or(0.0, |x| x.position.z)),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Polyline3D(v) = entity { v.elevation = 2.5; },
            expect_created: "n3 el0.0 lastz4.0",
            expect_edited: "n4 el1.5 lastz6.0",
            expect_reedited: "n4 el2.5 lastz6.0",
            expect_edited_dwg: "n4 el0.0 lastz6.0",
            expect_reedited_dwg: "n4 el0.0 lastz6.0",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_spline_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!("doc.create_entity('Spline', degree=2, knots=[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], control_points=[{'x': 0.0, 'y': 0.0, 'z': 0.0}, {'x': 5.0, 'y': 5.0, 'z': 0.0}, {'x': 10.0, 'y': 0.0, 'z': 0.0}])\n"),
            edit: concat!("e = doc.entities[HANDLE]\n", "with doc.transaction('Edit'):\n", "    e.control_points = [{'x': 0.0, 'y': 0.0, 'z': 0.0}, {'x': 5.0, 'y': 8.0, 'z': 0.0}, {'x': 10.0, 'y': 0.0, 'z': 0.0}]\n", "    e.weights = [1.0, 2.0, 1.0]\n", "doc.selection = [e]\n"),
            rejects: &[
                ("'degree':0", "at least 1"),
                ("'knots':[0.0,1.0]", "must hold"),
                ("'weights':[1.0,0.0,1.0]", "greater than zero"),
                ("'weights':[1.0]", "match"),
                ("'control_points':[{'x':float('nan'),'y':0.0,'z':0.0},{'x':1.0,'y':0.0,'z':0.0},{'x':2.0,'y':0.0,'z':0.0}]", "finite"),
                ("'knots':[0.0,0.0,1.0,0.5,1.0,1.0]", "nondecreasing"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Spline(_)),
            digest: |entity| match entity {
                EntityType::Spline(v) => format!("deg{} cp{} y{:.1} w{:?}", v.degree, v.control_points.len(), v.control_points[1].y, v.weights),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Spline(v) = entity { v.control_points[1].y = 9.0; },
            expect_created: "deg2 cp3 y5.0 w[]",
            expect_edited: "deg2 cp3 y8.0 w[1.0, 2.0, 1.0]",
            expect_reedited: "deg2 cp3 y9.0 w[1.0, 2.0, 1.0]",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
        });
    }

    #[test]
    fn audit_python_insert_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!("# MAKEBLOCK\n", "doc.create_entity('Insert', block_name='AUDITBLK', insert_point=P(1.0, 2.0, 0.0))\n"),
            edit: concat!(
                "e = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit'):\n",
                "    e.insert_point = (4.0, 5.0, 0.0)\n",
                "    e.x_scale = 2.0\n",
                "    e.y_scale = 3.0\n",
                "    e.rotation = 0.5\n",
                "    e.column_count = 2\n",
                "    e.column_spacing = 10.0\n",
                "doc.selection = [e]\n"),
            rejects: &[
                ("'x_scale':0.0", "nonzero"),
                ("'rotation':float('nan')", "finite"),
                ("'column_count':0", "greater than zero"),
                ("'insert_point':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'block_name':'OTHER'", "does not exist"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Insert(_)),
            digest: |entity| match entity {
                EntityType::Insert(v) => format!("{} at{:.1},{:.1} s{:.1},{:.1} r{:.1} cols{}x{:.1}", v.block_name, v.insert_point.x,
                    v.insert_point.y, v.x_scale(), v.y_scale(), v.rotation, v.column_count, v.column_spacing),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Insert(v) = entity { v.rotation = 1.0; },
            expect_created: "AUDITBLK at1.0,2.0 s1.0,1.0 r0.0 cols1x0.0",
            expect_edited: "AUDITBLK at4.0,5.0 s2.0,3.0 r0.5 cols2x10.0",
            expect_reedited: "AUDITBLK at4.0,5.0 s2.0,3.0 r1.0 cols2x10.0",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
        });
    }

    #[test]
    fn audit_python_attribute_definition_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "# MAKEBLOCK\n",
                "doc.create_entity('AttributeDefinition', owner_handle=BLOCKRECORD, tag='PART_NO', prompt='Part number',\n",
                "    default_value='PN-001', insertion_point=P(1.0, 2.0, 0.0), height=2.5, width_factor=1.25, oblique_angle=0.1,\n",
                "    rotation=0.25, horizontal_alignment='Center', vertical_alignment='Top', alignment_point=P(3.0, 4.0, 0.0),\n",
                "    flags={'invisible': True, 'constant': False, 'verify': True, 'preset': False, 'locked_position': False, 'annotative': False},\n",
                "    field_length=12, text_generation_flags=2, lock_position=True)\n"),
            edit: concat!(
                "e = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit'):\n",
                "    e.insertion_point = (4.0, 5.0, 0.0)\n",
                "    e.default_value = 'PN-002'\n",
                "    e.prompt = 'Serial'\n",
                "    e.height = 3.0\n",
                "    e.width_factor = 0.8\n",
                "    e.rotation = 0.5\n",
                "    e.oblique_angle = 0.2\n",
                "    e.horizontal_alignment = 'Right'\n",
                "    e.vertical_alignment = 'Middle'\n",
                "    e.flags = {'invisible': False, 'constant': True, 'verify': False, 'preset': True, 'locked_position': False, 'annotative': False}\n",
                "    e.field_length = 20\n",
                "doc.selection = [e]\n"),
            rejects: &[
                ("'tag':'BAD TAG'", "whitespace"),
                ("'height':0.0", "greater than zero"),
                ("'width_factor':0.0", "width_factor"),
                ("'text_style':'Missing'", "does not exist"),
                ("'owner_handle':999999", "read-only"),
                ("'normal':{'x':0.0,'y':0.0,'z':0.0}", "nonzero"),
                ("'rotation':float('nan')", "finite"),
                ("'horizontal_alignment':'Nowhere'", "unsupported"),
            ],
            is_kind: |entity| matches!(entity, EntityType::AttributeDefinition(_)),
            digest: |entity| match entity {
                EntityType::AttributeDefinition(v) => format!(
                    "{}|{}|{} ins{:.1},{:.1} al{:.1},{:.1} h{:.1} r{:.2} wf{:.2} ob{:.2} {:?}/{:?} f{}{}{}{} fl{} tg{} lock{}",
                    v.tag, v.prompt, v.default_value, v.insertion_point.x, v.insertion_point.y,
                    v.alignment_point.x, v.alignment_point.y, v.height, v.rotation, v.width_factor, v.oblique_angle,
                    v.horizontal_alignment, v.vertical_alignment,
                    u8::from(v.flags.invisible), u8::from(v.flags.constant), u8::from(v.flags.verify), u8::from(v.flags.preset),
                    v.field_length, v.text_generation_flags, u8::from(v.lock_position)),
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::AttributeDefinition(v) = entity { v.default_value = "PN-003".into(); },
            expect_created: "PART_NO|Part number|PN-001 ins1.0,2.0 al3.0,4.0 h2.5 r0.25 wf1.25 ob0.10 Center/Top f1010 fl12 tg2 lock1",
            expect_edited: "PART_NO|Serial|PN-002 ins4.0,5.0 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
            expect_reedited: "PART_NO|Serial|PN-003 ins4.0,5.0 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock1",
            // BLOCKER (cadcodec): the DXF ATTDEF writer emits no group 280 at
            // all — neither the version byte nor the lock-position flag the
            // reader looks for after it — so `lock_position` reopens false. Every
            // other field round-trips since cadcodec dd1d7bf.
            expect_edited_dxf: "PART_NO|Serial|PN-002 ins4.0,5.0 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock0",
            expect_reedited_dxf: "PART_NO|Serial|PN-003 ins4.0,5.0 al3.0,4.0 h3.0 r0.50 wf0.80 ob0.20 Right/Middle f0101 fl20 tg2 lock0",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
        });
    }

    #[test]
    fn audit_python_insert_creation_rules_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let mut records = std::collections::BTreeMap::new();
        for name in ["OUTER", "INNER"] {
            let next = host.document().next_handle();
            let (record_handle, block_handle, end_handle) = (Handle::new(next), Handle::new(next + 1), Handle::new(next + 2));
            let mut record = acadrust::tables::BlockRecord::new(name);
            record.handle = record_handle;
            record.block_entity_handle = block_handle;
            record.block_end_handle = end_handle;
            host.document_mut().block_records.add(record).unwrap();
            let mut block = acadrust::entities::Block::new(name, acadrust::types::Vector3::ZERO);
            block.common.handle = block_handle;
            block.common.owner_handle = record_handle;
            host.document_mut().add_entity(EntityType::Block(block)).unwrap();
            let mut end = acadrust::entities::BlockEnd::new();
            end.common.handle = end_handle;
            end.common.owner_handle = record_handle;
            host.document_mut().add_entity(EntityType::BlockEnd(end)).unwrap();
            records.insert(name, record_handle);
        }
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let count = |host: &HostSession<'_>| host.document().entities().filter(|e| matches!(e, EntityType::Insert(_))).count();
        let create = |name: &str, extra: &str| format!(
            "PY_EVAL ocs.active_document.create_entity('Insert', block_name='{name}', insert_point={{'x':0.0,'y':0.0,'z':0.0}}{extra})");

        // A valid insert of INNER into the drawing works.
        dispatch(&mut host, &create("INNER", ""));
        assert_eq!(count(&host), 1, "{}", last(&host));
        // Refused: unknown block, model space, bad scale, zero array count.
        for (command, message) in [
            (create("MISSING", ""), "does not exist"),
            (create("*Model_Space", ""), "model-space or paper-space"),
            (create("INNER", ", x_scale=0.0"), "nonzero"),
            (create("INNER", ", column_count=0"), "greater than zero"),
            (create("", ""), "empty"),
        ] {
            dispatch(&mut host, &command);
            assert_eq!(count(&host), 1, "{command}");
            assert!(last(&host).contains(message), "{command}: {}", last(&host));
        }
        // A block cannot contain itself, directly or through another block.
        let owner = |name: &str| format!(", owner_handle={}", records[name].value());
        dispatch(&mut host, &create("OUTER", &owner("OUTER")));
        assert_eq!(count(&host), 1);
        assert!(last(&host).contains("contain itself"), "{}", last(&host));
        dispatch(&mut host, &create("INNER", &owner("OUTER")));
        assert_eq!(count(&host), 2, "{}", last(&host));
        dispatch(&mut host, &create("OUTER", &owner("INNER")));
        assert_eq!(count(&host), 2);
        assert!(last(&host).contains("contain itself"), "{}", last(&host));
        // The legacy Polyline cannot be created and says what to use instead.
        dispatch(&mut host, "PY_EVAL ocs.active_document.create_entity('Polyline', vertices=[])");
        assert!(last(&host).contains("Polyline2D or Polyline3D"), "{}", last(&host));
    }

    #[test]
    fn audit_python_ole2frame_lifecycle_over_real_ipc() {
        image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 120, 200, 255]))
            .save(std::env::temp_dir().join("ocs_ole_audit.png"))
            .unwrap();
        run_mesh_lifecycle(&MeshCase {
            create: "doc.embed_picture('TMPDIR/ocs_ole_audit.png', origin=(10, 10, 0), width=20)\n",
            edit: concat!(
                "e = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit'):\n",
                "    e.upper_left_corner = (12.0, 40.0, 0.0)\n",
                "    e.lower_right_corner = (42.0, 10.0, 0.0)\n",
                "    e.lock_aspect = 1\n",
                "doc.selection = [e]\n"),
            rejects: &[
                ("'upper_left_corner':{'x':42.0,'y':40.0,'z':0.0}", "nonzero width and height"),
                ("'lock_aspect':2", "0 or 1"),
                ("'lower_right_corner':{'x':float('nan'),'y':0.0,'z':0.0}", "finite"),
                ("'version':3", "read-only"),
                ("'storage':{}", "outside the editable schema"),
            ],
            is_kind: |entity| matches!(entity, EntityType::Ole2Frame(_)),
            digest: |entity| match entity {
                EntityType::Ole2Frame(v) => {
                    let picture = match acadrust::entities::extract_presentation(&v.encoded_payload()) {
                        Some(acadrust::entities::OlePresentation::Raster(bytes)) => bytes.len(),
                        _ => 0,
                    };
                    format!("ul{:.1},{:.1} lr{:.1},{:.1} lock{} picture{}", v.upper_left_corner.x, v.upper_left_corner.y,
                        v.lower_right_corner.x, v.lower_right_corner.y, v.lock_aspect, picture)
                }
                _ => "wrong kind".into(),
            },
            reedit: |entity| if let EntityType::Ole2Frame(v) = entity { v.lower_right_corner.x = 50.0; },
            expect_created: "ul10.0,25.0 lr30.0,10.0 lock0 picture119",
            expect_edited: "ul12.0,40.0 lr42.0,10.0 lock1 picture119",
            expect_reedited: "ul12.0,40.0 lr50.0,10.0 lock1 picture119",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
        });
    }

    /// Evidence for docs/cadkernel-body-path.md: primitive, transform and
    /// boolean bodies become Solid3D/Body payloads that lift back losslessly
    /// and survive DWG and DXF. Slow in a debug build (booleans take ~23 s).
    #[test]
    #[ignore = "kernel spike; slow in debug builds"]
    fn spike_kernel_body_round_trip() {
        use crate::scene::model::solid_model as sm;
        use crate::scene::convert::solid3d_tess::{kernel_acis_body, kernel_body};
        let make = |body: &cadkernel::brep::Body| -> acadrust::entities::Solid3D {
            let sat = crate::scene::convert::acis_export::solid_to_sat(body).expect("solid_to_sat");
            let mut solid = acadrust::entities::Solid3D::new();
            solid.wires = sm::edge_wires(body);
            solid.set_sat_document(&sat);
            solid
        };
        let report = |label: &str, solid: &acadrust::entities::Solid3D| {
            let body = kernel_body(solid);
            eprintln!("SPIKE {label}: lift={} volume={:?} wires={} sat_bytes={} binary={}",
                body.is_some(), body.as_ref().map(sm::volume), solid.wires.len(),
                solid.acis_data.sat_data.len(), solid.acis_data.is_binary);
            body
        };
        let boxed = sm::box_solid([0.0, 0.0, 0.0], 10.0, 6.0, 4.0).unwrap();
        let mut doc = acadrust::CadDocument::new();
        let solid = make(&boxed);
        report("created box", &solid);
        let handle = doc.add_entity(EntityType::Solid3D(solid.clone())).unwrap();

        // Round trip both formats and compare payloads.
        let dwg = crate::io::load_bytes("s.dwg", acadrust::DwgWriter::write_to_vec(&doc).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("s.dxf", acadrust::DxfWriter::new(&doc).write_to_vec().unwrap()).unwrap();
        for (label, reopened) in [("DWG", &dwg), ("DXF", &dxf)] {
            let Some(EntityType::Solid3D(back)) = reopened.get_entity(handle) else { panic!("{label} lost the solid") };
            report(&format!("{label} reopen"), back);
            eprintln!("SPIKE {label}: acis_data equal={} wires equal={} silhouettes {}", back.acis_data == solid.acis_data,
                back.wires.len() == solid.wires.len(), back.silhouettes.len());
        }

        // Transforms: translate, rotate, mirror, matrix.
        let moved = sm::by_matrix(&boxed, [1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0., 5.,5.,5.,1.]).unwrap();
        let turned = sm::turned(&boxed, 2, std::f64::consts::FRAC_PI_4, [0.0, 0.0, 0.0]).unwrap();
        let mirrored = sm::mirrored(&boxed, 0, [0.0, 0.0, 0.0]).unwrap();
        for (label, body) in [("moved", &moved), ("turned", &turned), ("mirrored", &mirrored)] {
            let solid = make(body);
            let extent = sm::extent(body);
            eprintln!("SPIKE {label}: volume={:.3} extent={:?} lifts_back={}", sm::volume(body), extent, kernel_body(&solid).is_some());
        }

        // Booleans and primitives.
        let cyl = sm::cylinder_solid([0.0, 0.0, -3.0], 2.0, 10.0).unwrap();
        for (label, op) in [("union", sm::Bool::Union), ("subtract", sm::Bool::Subtract), ("intersect", sm::Bool::Intersect)] {
            match sm::boolean_result(op, &boxed, &cyl) {
                Ok(result) => { let solid = make(&result); eprintln!("SPIKE box {label} cylinder: volume={:.3} faces_wires={} lifts_back={}", sm::volume(&result), solid.wires.len(), kernel_body(&solid).is_some()); }
                Err(snag) => eprintln!("SPIKE box {label} cylinder: refused {snag:?}"),
            }
        }
        for (label, body) in [
            ("sphere", sm::sphere_solid([0.0; 3], 3.0)), ("torus", sm::torus_solid([0.0; 3], 5.0, 1.0)),
            ("wedge", sm::wedge_solid([0.0; 3], 4.0, 3.0, 2.0)), ("pyramid", sm::pyramid_solid([0.0; 3], 3.0, 4.0, 5)),
        ] {
            match body {
                Some(body) => { let solid = make(&body); eprintln!("SPIKE {label}: volume={:.3} lifts_back={}", sm::volume(&body), kernel_body(&solid).is_some()); }
                None => eprintln!("SPIKE {label}: kernel returned None"),
            }
        }

        // The same payload inside a Body entity and a Region-like container.
        let mut body_entity = acadrust::entities::Body::new();
        body_entity.set_sat_document(&crate::scene::convert::acis_export::solid_to_sat(&boxed).unwrap());
        let lifted = kernel_acis_body(&body_entity.acis_data);
        eprintln!("SPIKE Body entity: lift={} volume={:?}", lifted.is_some(), lifted.as_ref().map(sm::volume));
        let bh = doc.add_entity(EntityType::Body(body_entity)).unwrap();
        let dwg = crate::io::load_bytes("b.dwg", acadrust::DwgWriter::write_to_vec(&doc).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("b.dxf", acadrust::DxfWriter::new(&doc).write_to_vec().unwrap()).unwrap();
        for (label, reopened) in [("DWG", &dwg), ("DXF", &dxf)] {
            match reopened.get_entity(bh) {
                Some(EntityType::Body(back)) => eprintln!("SPIKE Body {label}: lift={}", kernel_acis_body(&back.acis_data).is_some()),
                other => eprintln!("SPIKE Body {label}: came back as {:?}", other.map(|e| format!("{e:?}").chars().take(30).collect::<String>())),
            }
        }
    }

    /// Per-operation kernel timings for docs/cadkernel-body-path.md.
    #[test]
    #[ignore = "kernel spike; slow in debug builds"]
    fn spike_kernel_timings() {
        use crate::scene::model::solid_model as sm;
        use std::time::Instant;
        let time = |label: &str, f: &mut dyn FnMut() -> String| {
            let start = Instant::now();
            let note = f();
            eprintln!("TIMING {label}: {:.2}s {note}", start.elapsed().as_secs_f64());
        };
        let boxed = sm::box_solid([0.0; 3], 10.0, 6.0, 4.0).unwrap();
        let cyl = sm::cylinder_solid([0.0, 0.0, -3.0], 2.0, 10.0).unwrap();
        time("box primitive", &mut || format!("{}", sm::box_solid([0.0; 3], 10.0, 6.0, 4.0).is_some()));
        time("sphere primitive", &mut || format!("{}", sm::sphere_solid([0.0; 3], 3.0).is_some()));
        time("solid_to_sat(box)", &mut || format!("{}", crate::scene::convert::acis_export::solid_to_sat(&boxed).is_some()));
        time("edge_wires(box)", &mut || format!("{}", sm::edge_wires(&boxed).len()));
        time("transform (matrix)", &mut || format!("{}", sm::by_matrix(&boxed, [1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0., 5.,5.,5.,1.]).is_some()));
        time("union", &mut || format!("{}", sm::boolean_result(sm::Bool::Union, &boxed, &cyl).is_ok()));
        time("subtract", &mut || format!("{}", sm::boolean_result(sm::Bool::Subtract, &boxed, &cyl).is_ok()));
        time("intersect", &mut || format!("{}", sm::boolean_result(sm::Bool::Intersect, &boxed, &cyl).is_ok()));
        let result = sm::boolean_result(sm::Bool::Union, &boxed, &cyl).unwrap();
        time("solid_to_sat(union)", &mut || format!("{}", crate::scene::convert::acis_export::solid_to_sat(&result).is_some()));
        time("edge_wires(union)", &mut || format!("{}", sm::edge_wires(&result).len()));
        time("volume(union)", &mut || format!("{:.1}", sm::volume(&result)));
    }

    #[test]
    fn audit_python_solid3d_lifecycle_over_real_ipc() {
        use crate::scene::convert::solid3d_tess::kernel_body;
        use crate::scene::model::solid_model as sm;
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let solids = |host: &HostSession<'_>| -> Vec<Handle> {
            let mut list: Vec<_> = host.document().entities().filter_map(|e| match e {
                EntityType::Solid3D(s) => Some(s.common.handle), _ => None }).collect();
            list.sort_by_key(|h| h.value());
            list
        };
        let body_of = |host: &HostSession<'_>, handle: Handle| {
            let Some(EntityType::Solid3D(solid)) = host.document().get_entity(handle) else { panic!("no solid {handle:?}") };
            kernel_body(solid).expect("payload lifts losslessly")
        };
        let volume = |host: &HostSession<'_>, handle: Handle| sm::volume(&body_of(host, handle));
        let extent = |host: &HostSession<'_>, handle: Handle| sm::extent(&body_of(host, handle)).unwrap();
        let close = |a: f64, b: f64, tolerance: f64| (a - b).abs() <= tolerance * b.abs().max(1.0);

        // C: every primitive, checked against its analytic volume.
        let script = std::env::temp_dir().join(format!("ocs_solid_create_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "s = ocs.active_document.solids\n",
            "s.box(center=(0, 0, 0), size=(10, 6, 4))\n",
            "s.cylinder(center=(20, 0, 0), radius=2, height=5)\n",
            "s.sphere(center=(40, 0, 0), radius=3)\n",
            "s.torus(center=(60, 0, 0), major=5, minor=1)\n",
            "s.wedge(origin=(80, 0, 0), size=(4, 3, 2))\n",
            "s.pyramid(center=(100, 0, 0), radius=3, height=4, sides=5)\n",
            "s.box(center=(0, 20, 0), size=(2, 2, 2), layer='SOLIDS')\n",
        )).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let list = solids(&host);
        assert_eq!(list.len(), 7, "{}", last(&host));
        let pi = std::f64::consts::PI;
        let five_gon = 0.5 * 5.0 * 3.0f64.powi(2) * (2.0 * pi / 5.0).sin();
        for (index, expected, tolerance) in [
            (0, 240.0, 1e-9), (1, pi * 4.0 * 5.0, 0.02), (2, 4.0 / 3.0 * pi * 27.0, 0.02),
            (3, 2.0 * pi * pi * 5.0 * 1.0, 0.02), (4, 12.0, 1e-9), (5, five_gon * 4.0 / 3.0, 0.02), (6, 8.0, 1e-9),
        ] {
            let actual = volume(&host, list[index]);
            assert!(close(actual, expected, tolerance), "solid {index}: volume {actual}, expected {expected}");
        }
        let Some(EntityType::Solid3D(first)) = host.document().get_entity(list[0]) else { unreachable!() };
        assert_eq!((first.wires.len(), first.common.layer.as_str()), (12, "0"));
        assert!(matches!(host.document().get_entity(list[6]), Some(EntityType::Solid3D(s)) if s.common.layer == "SOLIDS"));
        assert!(!host.app.tabs[0].scene.wire_models_for(&[list[0]]).is_empty(), "no canvas geometry");
        let read = |host: &mut HostSession<'_>, expression: &str| {
            dispatch(host, &format!("PY_EVAL {expression}"));
            last(host)
        };
        assert!(read(&mut host, &format!("ocs.active_document.entities[{}].kind", list[0].value())).contains("Solid3D"));

        // E: rigid moves keep the volume and move the extent exactly.
        let box_handle = list[0];
        let before = host.document().get_entity(box_handle).unwrap().clone();
        let undo_before = host.app.tabs[0].history.undo_stack.len();
        let (low, high) = extent(&host, box_handle);
        assert!(close(low[0], -5.0, 1e-6) && close(high[2], 2.0, 1e-6), "{low:?} {high:?}");
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (5, 5, 5)).handle", box_handle.value()));
        let (low, high) = extent(&host, box_handle);
        assert!(close(low[0], 0.0, 1e-6) && close(high[1], 8.0, 1e-6) && close(high[2], 7.0, 1e-6), "{low:?} {high:?}");
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.rotate({}, 'z', 0.7853981633974483, (5, 5, 5)).handle", box_handle.value()));
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.mirror({}, 'x', (0, 0, 0)).handle", box_handle.value()));
        assert!(close(volume(&host, box_handle), 240.0, 1e-9), "moves preserve volume");
        let Some(EntityType::Solid3D(moved)) = host.document().get_entity(box_handle) else { unreachable!() };
        assert_eq!((moved.wires.len(), moved.history_handle, moved.common.handle), (12, None, box_handle));
        assert_eq!(moved.common.layer, "0");
        assert_eq!(host.app.tabs[0].history.undo_stack.len() - undo_before, 3, "one undo step per move");

        // V: refused operations change nothing and leave no undo step.
        let snapshot = host.document().get_entity(box_handle).unwrap().clone();
        let count_before = solids(&host).len();
        let undo_now = host.app.tabs[0].history.undo_stack.len();
        let empty_payload = acadrust::entities::Solid3D::new();
        let unliftable = host.add_entity(EntityType::Solid3D(empty_payload.clone()));
        let unliftable_before = host.document().get_entity(unliftable).unwrap().clone();
        for (command, message) in [
            ("ocs.active_document.solids.box(size=(0, 1, 1))".to_string(), "greater than zero"),
            ("ocs.active_document.solids.box(center=(float('nan'), 0, 0))".to_string(), "must be finite"),
            ("ocs.active_document.solids.torus(major=1, minor=2)".to_string(), "smaller than the major"),
            ("ocs.active_document.solids.pyramid(sides=2)".to_string(), "between 3 and 1024"),
            ("ocs.active_document.solids.sphere(radius=-1)".to_string(), "greater than zero"),
            ("ocs.active_document.solids.box(layer='')".to_string(), "layer name is empty"),
            ("ocs.solid_create('blob', [1.0], None)".to_string(), "unknown primitive"),
            ("ocs.solid_create('box', [1.0], None)".to_string(), "takes 6 numbers"),
            (format!("ocs.active_document.solids.transform({}, [2,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1])", box_handle.value()), "rigid moves"),
            (format!("ocs.active_document.solids.transform({}, [1]*16)", box_handle.value()), "rigid moves"),
            (format!("ocs.active_document.solids.translate({}, (float('nan'), 0, 0))", box_handle.value()), "must be finite"),
            (format!("ocs.active_document.solids.translate({}, (1, 0, 0))", unliftable.value()), "cannot be lifted losslessly"),
            ("ocs.active_document.solids.translate(999999, (1, 0, 0))".to_string(), "does not exist"),
        ] {
            dispatch(&mut host, &format!("PY_EVAL {command}"));
            assert!(last(&host).contains(message), "{command}: {}", last(&host));
        }
        assert_eq!(host.document().get_entity(box_handle), Some(&snapshot));
        assert_eq!(host.document().get_entity(unliftable), Some(&unliftable_before));
        assert_eq!(solids(&host).len(), count_before + 1, "only the fixture was added");
        assert_eq!(host.app.tabs[0].history.undo_stack.len(), undo_now, "refusals record no undo step");
        // Transforming a non-solid is refused.
        let line = host.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            acadrust::types::Vector3::ZERO, acadrust::types::Vector3::new(1.0, 0.0, 0.0))));
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (1, 0, 0))", line.value()));
        assert!(last(&host).contains("outside the Python schema") || last(&host).contains("applies to Solid3D"), "{}", last(&host));

        // W: both formats reopen the moved solid losslessly; the reopened
        // payload can be moved again.
        let moved_volume = volume(&host, box_handle);
        let moved_extent = extent(&host, box_handle);
        let dwg = crate::io::load_bytes("solid.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("solid.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
            let Some(EntityType::Solid3D(back)) = document.get_entity(box_handle) else { panic!("{format} lost the solid") };
            let body = kernel_body(back).unwrap_or_else(|| panic!("{format} payload does not lift"));
            assert!(close(sm::volume(&body), moved_volume, 1e-6), "{format}");
            let reopened_extent = sm::extent(&body).unwrap();
            for axis in 0..3 {
                assert!(close(reopened_extent.0[axis], moved_extent.0[axis], 1e-6) && close(reopened_extent.1[axis], moved_extent.1[axis], 1e-6), "{format} extent");
            }
            let again = host.add_entity(EntityType::Solid3D(back.clone()));
            dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (100, 0, 0)).handle", again.value()));
            let (low, _) = extent(&host, again);
            assert!(close(low[0], reopened_extent.0[0] + 100.0, 1e-6), "{format} re-edit");
        }

        // D and U: delete, then undo and redo restore the moved state.
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", box_handle.value()));
        assert!(host.document().get_entity(box_handle).is_none());
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        // Steps after creation: 3 moves, 2 re-edits of reopened copies, 1 delete.
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(box_handle).is_some(), "undo restores the deleted solid");
        app.undo_steps(5);
        assert_eq!(app.tabs[0].scene.document.get_entity(box_handle), Some(&before), "undoing every move restores the original solid exactly");
        app.redo_steps(6);
        assert!(app.tabs[0].scene.document.get_entity(box_handle).is_none(), "redo replays the moves and the delete");
    }

    #[test]
    fn audit_python_body_transform_over_real_ipc() {
        use crate::scene::model::solid_model as sm;
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        // A Body entity carrying a kernel box; scripts cannot create one.
        let boxed = sm::box_solid([0.0; 3], 4.0, 4.0, 4.0).unwrap();
        let mut body = acadrust::entities::Body::new();
        body.set_sat_document(&crate::scene::convert::acis_export::solid_to_sat(&boxed).unwrap());
        body.wires = sm::edge_wires(&boxed);
        let handle = host.add_entity(EntityType::Body(body));
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let extent = |host: &HostSession<'_>| {
            let Some(EntityType::Body(b)) = host.document().get_entity(handle) else { panic!("no body") };
            sm::extent(&crate::scene::convert::solid3d_tess::kernel_acis_body(&b.acis_data).unwrap()).unwrap()
        };
        assert!((extent(&host).0[0] + 2.0).abs() < 1e-6);
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.entities[{}].kind", handle.value()));
        assert!(host.app.command_line.history.last().unwrap().text.contains("Body"));
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (10, 0, 0)).handle", handle.value()));
        assert!((extent(&host).0[0] - 8.0).abs() < 1e-6, "{:?}", extent(&host));
        let bytes = acadrust::DwgWriter::write_to_vec(host.document()).unwrap();
        let reopened = crate::io::load_bytes("body.dwg", bytes).unwrap();
        let Some(EntityType::Body(back)) = reopened.get_entity(handle) else { panic!("DWG lost the body") };
        let lifted = crate::scene::convert::solid3d_tess::kernel_acis_body(&back.acis_data).expect("DWG body lifts");
        assert!((sm::volume(&lifted) - 64.0).abs() < 1e-6);
        // Scripts cannot create a Body and get the explanatory message.
        dispatch(&mut host, "PY_EVAL ocs.active_document.create_entity('Body')");
        assert!(host.app.command_line.history.last().unwrap().text.contains("use doc.solids"), "{}", host.app.command_line.history.last().unwrap().text);
    }

    #[test]
    fn audit_python_region_lifecycle_over_real_ipc() {
        use crate::scene::convert::solid3d_tess::kernel_acis_body;
        use crate::scene::model::solid_model as sm;
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let regions = |host: &HostSession<'_>| -> Vec<Handle> {
            let mut list: Vec<_> = host.document().entities().filter_map(|e| match e {
                EntityType::Region(r) => Some(r.common.handle), _ => None }).collect();
            list.sort_by_key(|h| h.value());
            list
        };
        let extent_in = |document: &CadDocument, handle: Handle| {
            let Some(EntityType::Region(region)) = document.get_entity(handle) else { panic!("no region {handle:?}") };
            let body = kernel_acis_body(&region.acis_data).expect("region payload lifts losslessly");
            sm::extent(&body).unwrap()
        };
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;

        // C: regions from a circle (source kept), a closed rectangle (source
        // deleted) and a second circle on its own layer.
        let script = std::env::temp_dir().join(format!("ocs_region_create_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "doc = ocs.active_document\n",
            "circle = doc.create_entity('Circle', center=(0, 0, 0), radius=5)\n",
            "rect = doc.create_entity('LwPolyline', is_closed=True, vertices=[{'location': {'x': 20.0, 'y': 0.0}}, {'location': {'x': 30.0, 'y': 0.0}}, {'location': {'x': 30.0, 'y': 6.0}}, {'location': {'x': 20.0, 'y': 6.0}}])\n",
            "doc.solids.region(circle)\n",
            "doc.solids.region(rect, delete_source=True)\n",
            "other = doc.create_entity('Circle', center=(50, 0, 0), radius=2, layer='PROFILES')\n",
            "doc.solids.region(other, layer='REGIONS')\n",
        )).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let list = regions(&host);
        assert_eq!(list.len(), 3, "{}", last(&host));
        let (low, high) = extent_in(host.document(), list[0]);
        assert!(close(low[0], -5.0) && close(high[0], 5.0) && close(low[1], -5.0) && close(high[1], 5.0) && close(low[2], 0.0) && close(high[2], 0.0), "{low:?} {high:?}");
        let (low, high) = extent_in(host.document(), list[1]);
        assert!(close(low[0], 20.0) && close(high[0], 30.0) && close(high[1], 6.0), "{low:?} {high:?}");
        let layer_of = |host: &HostSession<'_>, handle: Handle| host.document().get_entity(handle).unwrap().common().layer.clone();
        assert_eq!(layer_of(&host, list[0]), "0", "defaults to the profile's layer");
        assert_eq!(layer_of(&host, list[2]), "REGIONS");
        let circles = host.document().entities().filter(|e| matches!(e, EntityType::Circle(_))).count();
        let polylines = host.document().entities().filter(|e| matches!(e, EntityType::LwPolyline(_))).count();
        assert_eq!((circles, polylines), (2, 0), "delete_source removed only the rectangle");
        let Some(EntityType::Region(first)) = host.document().get_entity(list[0]) else { unreachable!() };
        assert_eq!((first.wires.is_empty(), first.point_of_reference.z), (false, 0.0));
        assert!(!host.app.tabs[0].scene.wire_models_for(&[list[0]]).is_empty(), "no canvas geometry");
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.entities[{}].kind", list[0].value()));
        assert!(last(&host).contains("Region"));

        // V: refusals change nothing and leave no undo step.
        let open_line = host.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            acadrust::types::Vector3::ZERO, acadrust::types::Vector3::new(5.0, 0.0, 0.0))));
        let mut open_poly = acadrust::entities::LwPolyline::new();
        open_poly.vertices = vec![
            acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(0.0, 0.0)),
            acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(4.0, 0.0)),
            acadrust::entities::LwVertex::new(acadrust::types::Vector2::new(4.0, 4.0)),
        ];
        let open_poly = host.add_entity(EntityType::LwPolyline(open_poly));
        let ray = host.add_entity(EntityType::Ray(acadrust::entities::Ray::default()));
        let entity_count = host.document().entities().count();
        let undo_now = host.app.tabs[0].history.undo_stack.len();
        for (command, message) in [
            (format!("ocs.active_document.solids.region({})", open_line.value()), "must be closed"),
            (format!("ocs.active_document.solids.region({})", open_poly.value()), "must be closed"),
            (format!("ocs.active_document.solids.region({})", ray.value()), "planar profile"),
            (format!("ocs.active_document.solids.region({})", list[0].value()), "already a region"),
            ("ocs.active_document.solids.region(999999)".to_string(), "does not exist"),
            (format!("ocs.active_document.solids.region({}, layer='')", open_line.value()), "layer name is empty"),
        ] {
            dispatch(&mut host, &format!("PY_EVAL {command}"));
            assert!(last(&host).contains(message), "{command}: {}", last(&host));
        }
        assert_eq!(host.document().entities().count(), entity_count);
        assert_eq!(host.app.tabs[0].history.undo_stack.len(), undo_now, "refusals record no undo step");

        // E: a region moves through the same kernel channel.
        let before = host.document().get_entity(list[0]).unwrap().clone();
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (10, 5, 0)).handle", list[0].value()));
        let (low, high) = extent_in(host.document(), list[0]);
        assert!(close(low[0], 5.0) && close(high[0], 15.0) && close(low[1], 0.0) && close(high[1], 10.0), "{low:?} {high:?}");

        // W: both formats reopen a region that lifts with the same extent, and
        // a reopened region moves again.
        let dwg = crate::io::load_bytes("region.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("region.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        let moved = extent_in(host.document(), list[0]);
        for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
            assert_eq!(regions_in(document).len(), 3, "{format}");
            let reopened = extent_in(document, list[0]);
            for axis in 0..3 {
                assert!(close(reopened.0[axis], moved.0[axis]) && close(reopened.1[axis], moved.1[axis]), "{format} extent {reopened:?} vs {moved:?}");
            }
            let Some(EntityType::Region(back)) = document.get_entity(list[0]) else { unreachable!() };
            let again = host.add_entity(EntityType::Region(back.clone()));
            dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (100, 0, 0)).handle", again.value()));
            assert!(close(extent_in(host.document(), again).0[0], reopened.0[0] + 100.0), "{format} re-edit");
        }
        fn regions_in(document: &CadDocument) -> Vec<Handle> {
            document.entities().filter_map(|e| match e { EntityType::Region(r) => Some(r.common.handle), _ => None }).collect()
        }

        // D and U: delete, undo, and undo the creation steps exactly.
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", list[2].value()));
        assert!(host.document().get_entity(list[2]).is_none());
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(list[2]).is_some(), "undo restores the deleted region");
        // Two re-edits of reopened copies, then the move of the first region.
        app.undo_steps(3);
        assert_eq!(app.tabs[0].scene.document.get_entity(list[0]), Some(&before), "undoing the move restores the region exactly");
    }

    #[test]
    fn audit_python_surface_lifecycle_over_real_ipc() {
        use crate::scene::convert::solid3d_tess::kernel_acis_body;
        use crate::scene::model::solid_model as sm;
        use acadrust::entities::SurfaceKind;
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let sorted = |mut list: Vec<Handle>| { list.sort_by_key(|h| h.value()); list };
        let surfaces_in = |document: &CadDocument| sorted(document.entities().filter_map(|e| match e {
            EntityType::Surface(s) => Some(s.common.handle), _ => None }).collect());
        let solids_in = |document: &CadDocument| sorted(document.entities().filter_map(|e| match e {
            EntityType::Solid3D(s) => Some(s.common.handle), _ => None }).collect());
        let body_of = |document: &CadDocument, handle: Handle| match document.get_entity(handle) {
            Some(EntityType::Surface(s)) => kernel_acis_body(&s.acis_data).expect("surface payload lifts losslessly"),
            Some(EntityType::Solid3D(s)) => kernel_acis_body(&s.acis_data).expect("solid payload lifts losslessly"),
            other => panic!("no surface or solid {handle:?}: {other:?}"),
        };
        let extent_in = |document: &CadDocument, handle: Handle| sm::extent(&body_of(document, handle)).unwrap();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
        let kind_in = |document: &CadDocument, handle: Handle| match document.get_entity(handle) {
            Some(EntityType::Surface(s)) => s.kind, other => panic!("no surface {handle:?}: {other:?}") };

        // C: plane surfaces and extrusions from real profile entities.
        let script = std::env::temp_dir().join(format!("ocs_surface_create_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "doc = ocs.active_document\n",
            "s = doc.solids\n",
            "circle = doc.create_entity('Circle', center=(0, 0, 0), radius=3)\n",
            "rect = doc.create_entity('LwPolyline', is_closed=True, vertices=[{'location': {'x': 20.0, 'y': 0.0}}, {'location': {'x': 30.0, 'y': 0.0}}, {'location': {'x': 30.0, 'y': 6.0}}, {'location': {'x': 20.0, 'y': 6.0}}])\n",
            "open_path = doc.create_entity('LwPolyline', vertices=[{'location': {'x': 50.0, 'y': 0.0}}, {'location': {'x': 54.0, 'y': 0.0}}, {'location': {'x': 54.0, 'y': 4.0}}])\n",
            "temp = doc.create_entity('Circle', center=(70, 0, 0), radius=2)\n",
            "s.surface(circle)\n",
            "s.surface(rect, layer='SURFACES')\n",
            "s.extrude(rect, (0, 0, 4))\n",
            "s.extrude(circle, (0, 0, 5))\n",
            "s.extrude(open_path, (0, 0, 3))\n",
            "s.extrude(temp, (0, 0, 2), delete_source=True)\n",
        )).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let surfaces = surfaces_in(host.document());
        let solids = solids_in(host.document());
        assert_eq!((surfaces.len(), solids.len()), (3, 3), "{}", last(&host));
        // surfaces[0] plane of the circle, [1] plane of the rectangle, [2] extruded open path.
        assert_eq!((kind_in(host.document(), surfaces[0]), kind_in(host.document(), surfaces[1]), kind_in(host.document(), surfaces[2])),
            (SurfaceKind::Plane, SurfaceKind::Plane, SurfaceKind::Generic));
        let (low, high) = extent_in(host.document(), surfaces[0]);
        assert!(close(low[0], -3.0) && close(high[1], 3.0) && close(low[2], 0.0) && close(high[2], 0.0), "{low:?} {high:?}");
        let (low, high) = extent_in(host.document(), surfaces[2]);
        assert!(close(low[0], 50.0) && close(high[0], 54.0) && close(low[2], 0.0) && close(high[2], 3.0), "open path extrusion {low:?} {high:?}");
        assert_eq!(host.document().get_entity(surfaces[1]).unwrap().common().layer, "SURFACES");
        let pi = std::f64::consts::PI;
        for (index, expected, tolerance) in [(0, 240.0, 1e-9), (1, pi * 9.0 * 5.0, 0.02), (2, pi * 4.0 * 2.0, 0.02)] {
            let volume = sm::volume(&body_of(host.document(), solids[index]));
            assert!((volume - expected).abs() <= tolerance * expected, "solid {index}: {volume} vs {expected}");
        }
        assert_eq!(host.document().entities().filter(|e| matches!(e, EntityType::Circle(_))).count(), 1,
            "delete_source removed only the extruded temporary circle");
        assert!(!host.app.tabs[0].scene.wire_models_for(&[surfaces[0]]).is_empty(), "no canvas geometry");
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.entities[{}].kind", surfaces[0].value()));
        assert!(last(&host).contains("Surface"));

        // V: refusals change nothing and leave no undo step.
        let ray = host.add_entity(EntityType::Ray(acadrust::entities::Ray::default()));
        let circle = host.document().entities().find_map(|e| match e { EntityType::Circle(c) => Some(c.common.handle), _ => None }).unwrap();
        let counts = (host.document().entities().count(), host.app.tabs[0].history.undo_stack.len());
        for (command, message) in [
            (format!("ocs.active_document.solids.extrude({}, (0, 0, 0))", circle.value()), "nonzero"),
            (format!("ocs.active_document.solids.extrude({}, (float('nan'), 0, 1))", circle.value()), "must be finite"),
            (format!("ocs.active_document.solids.extrude({}, (0, 0, 1))", ray.value()), "planar profile"),
            (format!("ocs.active_document.solids.extrude({}, (0, 0, 1))", solids[0].value()), "curve profile"),
            (format!("ocs.active_document.solids.surface({})", surfaces[0].value()), "curve profile"),
            (format!("ocs.active_document.solids.surface({})", ray.value()), "planar profile"),
            (format!("ocs.active_document.solids.surface({}, layer='')", circle.value()), "layer name is empty"),
            ("ocs.active_document.solids.surface(999999)".to_string(), "does not exist"),
            ("ocs.solid_extrude(1, [1.0], None, False)".to_string(), "3 numbers"),
        ] {
            dispatch(&mut host, &format!("PY_EVAL {command}"));
            assert!(last(&host).contains(message), "{command}: {}", last(&host));
        }
        assert_eq!((host.document().entities().count(), host.app.tabs[0].history.undo_stack.len()), counts,
            "refusals change nothing and record no undo step");

        // E: isoline density is scriptable and validated; kind and payload are not.
        let plane = surfaces[0];
        let plane_before = host.document().get_entity(plane).unwrap().clone();
        let script = std::env::temp_dir().join(format!("ocs_surface_edit_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "doc = ocs.active_document\n",
            "e = doc.entities[{}]\n",
            "with doc.transaction('Edit surface'):\n",
            "    e.u_isolines = 8\n",
            "    e.v_isolines = 6\n",
        ), plane.value())).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let Some(EntityType::Surface(edited)) = host.document().get_entity(plane) else { unreachable!() };
        assert_eq!((edited.u_isolines, edited.v_isolines, edited.kind), (8, 6, SurfaceKind::Plane), "{}", last(&host));
        let edited_state = host.document().get_entity(plane).unwrap().clone();
        for (patch, message) in [("'u_isolines':500", "between 0 and 200"), ("'surface_kind':'Extruded'", "read-only"),
            ("'surface_data':{}", "outside the editable schema"), ("'v_isolines':-1", "between 0 and 200")] {
            dispatch(&mut host, &format!("PY_EVAL ocs.update_many('Reject surface', [{{'handle':{}, {patch}}}])", plane.value()));
            assert_eq!(host.document().get_entity(plane), Some(&edited_state), "{patch}");
            assert!(last(&host).contains(message), "{patch}: {}", last(&host));
        }
        // Moves keep a plane a plane and turn a swept surface generic.
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (10, 5, 0)).handle", plane.value()));
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (0, 0, 7)).handle", surfaces[2].value()));
        assert_eq!(kind_in(host.document(), plane), SurfaceKind::Plane);
        let (low, high) = extent_in(host.document(), plane);
        assert!(close(low[0], 7.0) && close(high[0], 13.0) && close(low[1], 2.0) && close(high[1], 8.0), "{low:?} {high:?}");
        let (low, _) = extent_in(host.document(), surfaces[2]);
        assert!(close(low[2], 7.0), "{low:?}");

        // W: both formats reopen surfaces that lift with the same extents, keep
        // the isolines, and a reopened surface moves again.
        let dwg = crate::io::load_bytes("surface.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("surface.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        let moved = extent_in(host.document(), plane);
        for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
            assert_eq!((surfaces_in(document).len(), solids_in(document).len()), (3, 3), "{format}");
            let reopened = extent_in(document, plane);
            for axis in 0..3 {
                assert!(close(reopened.0[axis], moved.0[axis]) && close(reopened.1[axis], moved.1[axis]), "{format} extent");
            }
            let Some(EntityType::Surface(back)) = document.get_entity(plane) else { unreachable!() };
            assert_eq!((back.u_isolines, back.v_isolines), (8, 6), "{format} isolines");
            assert_eq!(back.kind, SurfaceKind::Plane, "{format} keeps the plane kind");
            let again = host.add_entity(EntityType::Surface(back.clone()));
            dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (100, 0, 0)).handle", again.value()));
            assert!(close(extent_in(host.document(), again).0[0], reopened.0[0] + 100.0), "{format} re-edit");
        }

        // D and U.
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", surfaces[1].value()));
        assert!(host.document().get_entity(surfaces[1]).is_none());
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(surfaces[1]).is_some(), "undo restores the deleted surface");
        // Two re-edits of reopened copies, two moves, and the isoline edit come before.
        app.undo_steps(2 + 2 + 1);
        assert_eq!(app.tabs[0].scene.document.get_entity(plane), Some(&plane_before), "undoing the edits restores the surface exactly");
    }

    #[test]
    fn audit_python_boolean_lifecycle_over_real_ipc() {
        use crate::scene::convert::solid3d_tess::kernel_body;
        use crate::scene::model::solid_model as sm;
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let solids_in = |document: &CadDocument| {
            let mut list: Vec<Handle> = document.entities().filter_map(|e| match e {
                EntityType::Solid3D(s) => Some(s.common.handle), _ => None }).collect();
            list.sort_by_key(|h| h.value());
            list
        };
        let body_in = |document: &CadDocument, handle: Handle| {
            let Some(EntityType::Solid3D(solid)) = document.get_entity(handle) else { panic!("no solid {handle:?}") };
            kernel_body(solid).expect("payload lifts losslessly")
        };
        let volume_in = |document: &CadDocument, handle: Handle| sm::volume(&body_in(document, handle));
        let extent_in = |document: &CadDocument, handle: Handle| sm::extent(&body_in(document, handle)).unwrap();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-6 * b.abs().max(1.0);

        // C: all three operations on two overlapping boxes, keeping the
        // operands. A: x 0..10, B: x 5..15, both 6 by 4; the overlap is 120.
        let script = std::env::temp_dir().join(format!("ocs_boolean_create_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "s = ocs.active_document.solids\n",
            "a = s.box(center=(5, 3, 2), size=(10, 6, 4))\n",
            "b = s.box(center=(10, 3, 2), size=(10, 6, 4))\n",
            "s.union(a, b, keep_operands=True)\n",
            "s.subtract(a, b, keep_operands=True)\n",
            "s.intersect(a, b, keep_operands=True)\n",
        )).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let list = solids_in(host.document());
        assert_eq!(list.len(), 5, "operands kept and three results: {}", last(&host));
        let (a, b) = (list[0], list[1]);
        for (index, expected) in [(2, 360.0), (3, 120.0), (4, 120.0)] {
            let actual = volume_in(host.document(), list[index]);
            assert!(close(actual, expected), "result {index}: volume {actual}, expected {expected}");
        }
        let (low, high) = extent_in(host.document(), list[3]);
        assert!(close(low[0], 0.0) && close(high[0], 5.0), "subtract keeps x 0..5: {low:?} {high:?}");
        let (low, high) = extent_in(host.document(), list[4]);
        assert!(close(low[0], 5.0) && close(high[0], 10.0), "intersect keeps x 5..10: {low:?} {high:?}");
        let Some(EntityType::Solid3D(union)) = host.document().get_entity(list[2]) else { unreachable!() };
        assert_eq!((union.wires.is_empty(), union.history_handle), (false, None));
        assert!(!host.app.tabs[0].scene.wire_models_for(&[list[2]]).is_empty(), "no canvas geometry");
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.entities[{}].kind", list[2].value()));
        assert!(last(&host).contains("Solid3D"));

        // Consuming operands: A minus B leaves only the result, on a new layer.
        let before_consume = (host.document().get_entity(a).unwrap().clone(), host.document().get_entity(b).unwrap().clone());
        let undo_before = host.app.tabs[0].history.undo_stack.len();
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.subtract({}, {}, layer='CUT').handle", a.value(), b.value()));
        assert!(host.document().get_entity(a).is_none() && host.document().get_entity(b).is_none(), "operands are consumed");
        let cut = *solids_in(host.document()).last().unwrap();
        assert!(close(volume_in(host.document(), cut), 120.0));
        assert_eq!(host.document().get_entity(cut).unwrap().common().layer, "CUT");
        assert_eq!(host.app.tabs[0].history.undo_stack.len() - undo_before, 1, "one undo step per boolean");

        // A second planar operation chains on the result: drill a through-hole.
        dispatch(&mut host, "PY_EVAL ocs.active_document.solids.box(center=(2.5, 3, 2), size=(2, 2, 6)).handle");
        let hole = *solids_in(host.document()).last().unwrap();
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.subtract({}, {}).handle", cut.value(), hole.value()));
        let drilled = *solids_in(host.document()).last().unwrap();
        assert!(close(volume_in(host.document(), drilled), 104.0), "120 minus a 2x2x4 hole is 104");

        // V: refusals change nothing and leave no undo step.
        let line = host.add_entity(EntityType::Line(acadrust::entities::Line::from_points(
            acadrust::types::Vector3::ZERO, acadrust::types::Vector3::new(1.0, 0.0, 0.0))));
        let unliftable = host.add_entity(EntityType::Solid3D(acadrust::entities::Solid3D::new()));
        let mut plane = acadrust::entities::Surface::new(acadrust::entities::SurfaceKind::Plane);
        plane.acis_data = acadrust::entities::AcisData::new();
        let surface = host.add_entity(EntityType::Surface(plane));
        let far = { dispatch(&mut host, "PY_EVAL ocs.active_document.solids.box(center=(500, 500, 500), size=(1, 1, 1)).handle");
            *solids_in(host.document()).last().unwrap() };
        let counts = (host.document().entities().count(), host.app.tabs[0].history.undo_stack.len());
        let z = drilled.value();
        for (command, message) in [
            (format!("ocs.active_document.solids.union({z}, {z})"), "cannot be combined with itself"),
            (format!("ocs.active_document.solids.union({z}, {})", line.value()), "must be a Solid3D"),
            (format!("ocs.active_document.solids.union({}, {z})", surface.value()), "must be a Solid3D"),
            (format!("ocs.active_document.solids.union({z}, 999999)"), "does not exist"),
            (format!("ocs.active_document.solids.union({z}, {})", unliftable.value()), "cannot be lifted losslessly"),
            (format!("ocs.active_document.solids.union({z}, {}, layer='')", far.value()), "layer name is empty"),
            (format!("ocs.solid_boolean({z}, {}, 'xor', None, False)", far.value()), "unknown operation"),
            (format!("ocs.active_document.solids.intersect({z}, {})", far.value()), "changed"),
        ] {
            dispatch(&mut host, &format!("PY_EVAL {command}"));
            assert!(last(&host).contains(message), "{command}: {}", last(&host));
        }
        assert_eq!((host.document().entities().count(), host.app.tabs[0].history.undo_stack.len()), counts,
            "refusals change nothing and record no undo step");

        // W: both formats reopen the drilled result losslessly; it moves again.
        let expected_volume = volume_in(host.document(), drilled);
        let expected_extent = extent_in(host.document(), drilled);
        let dwg = crate::io::load_bytes("boolean.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("boolean.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        for (format, document) in [("DWG", &dwg), ("DXF", &dxf)] {
            assert!(close(volume_in(document, drilled), expected_volume), "{format} volume");
            let reopened = extent_in(document, drilled);
            for axis in 0..3 {
                assert!(close(reopened.0[axis], expected_extent.0[axis]) && close(reopened.1[axis], expected_extent.1[axis]), "{format} extent");
            }
            let Some(EntityType::Solid3D(back)) = document.get_entity(drilled) else { unreachable!() };
            let again = host.add_entity(EntityType::Solid3D(back.clone()));
            dispatch(&mut host, &format!("PY_EVAL ocs.active_document.solids.translate({}, (100, 0, 0)).handle", again.value()));
            assert!(close(extent_in(host.document(), again).0[0], reopened.0[0] + 100.0), "{format} re-edit");
        }

        // D and U: delete, then undo restores it, and undoing the consuming
        // boolean brings both operands back exactly.
        dispatch(&mut host, &format!("PY_EVAL ocs.active_document.delete_entity({})", drilled.value()));
        assert!(host.document().get_entity(drilled).is_none());
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(1);
        assert!(app.tabs[0].scene.document.get_entity(drilled).is_some(), "undo restores the deleted result");
        // Two re-edits, the far box, the drilling subtract and the hole box, then the consuming subtract.
        app.undo_steps(2 + 1 + 1 + 1 + 1);
        assert_eq!(app.tabs[0].scene.document.get_entity(a), Some(&before_consume.0), "undo restores operand A exactly");
        assert_eq!(app.tabs[0].scene.document.get_entity(b), Some(&before_consume.1), "undo restores operand B exactly");
        assert!(app.tabs[0].scene.document.get_entity(cut).is_none(), "undo removes the boolean result");
    }

    /// Release-only evidence for docs/cadkernel-body-path.md: the kernel's
    /// refusals reach the script as messages. Curved booleans take about
    /// 23 s in a debug build, so this is ignored by default.
    #[test]
    #[ignore = "curved boolean; slow in debug builds"]
    fn audit_python_boolean_kernel_refusal_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let script = std::env::temp_dir().join(format!("ocs_boolean_refusal_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "s = ocs.active_document.solids\n",
            "box = s.box(center=(0, 0, 0), size=(10, 6, 4))\n",
            "sphere = s.sphere(center=(0, 0, 0), radius=3)\n",
            "s.subtract(box, sphere)\n",
        )).unwrap();
        dispatch(&mut host, &format!("PY_RUN {}", script.display()));
        let _ = std::fs::remove_file(&script);
        let message = host.app.command_line.history.last().unwrap().text.clone();
        assert!(message.contains("Coincident") && message.contains("nothing was changed"), "{message}");
    }

    #[test]
    fn audit_python_section_symbol_lifecycle_over_real_ipc() {
        run_mesh_lifecycle(&MeshCase {
            create: concat!(
                "doc.create_entity('SectionSymbol', symbol_scale=1.0, points=[\n",
                "    {'point': P(0.0, 0.0, 0.0), 'label': 'A', 'label_offset': P(0.0, 2.0, 0.0)},\n",
                "    {'point': P(40.0, 0.0, 0.0), 'label': 'A', 'label_offset': P(0.0, 2.0, 0.0)}])\n"),
            edit: concat!(
                "e = doc.entities[HANDLE]\n",
                "with doc.transaction('Edit'):\n",
                "    e.points = [{'point': P(0.0, 0.0, 0.0), 'label': 'B', 'label_offset': P(0.0, 3.0, 0.0)},\n",
                "        {'point': P(20.0, 10.0, 0.0), 'bulge': 0.0}, {'point': P(40.0, 0.0, 0.0), 'label': 'B', 'label_offset': P(0.0, -3.0, 0.0)}]\n",
                "    e.symbol_scale = 2.0\n",
                "doc.selection = [e]\n"),
            rejects: &[
                ("'points':[{'point':{'x':0.0,'y':0.0,'z':0.0}}]", "at least 2 points"),
                ("'points':[{'point':{'x':float('nan'),'y':0.0,'z':0.0}},{'point':{'x':1.0,'y':0.0,'z':0.0}}]", "finite"),
                ("'symbol_scale':0.0", "greater than zero"),
                ("'style_handle':999999", "not an existing SectionViewStyle"),
                ("'view_rep_handle':999999", "not an existing ViewRep"),
                ("'raw_point_count_90':9", "read-only"),
                ("'end_a':[1.0, 1.0]", "read-only"),
            ],
            is_kind: |entity| matches!(entity, EntityType::SectionSymbol(_)),
            digest: |entity| match entity {
                EntityType::SectionSymbol(v) => format!("n{} ends {:.1},{:.1}->{:.1},{:.1} label{} tick{:.1}/{:.1} scale{:.1} counts{}/{}",
                    v.points.len(), v.end_a[0], v.end_a[1], v.end_b[0], v.end_b[1], v.label, v.tick_a, v.tick_b,
                    v.symbol_scale, v.raw_point_count_90, v.raw_point_record_count),
                other => format!("wrong kind {}", format!("{other:?}").split('(').next().unwrap()),
            },
            reedit: |entity| if let EntityType::SectionSymbol(v) = entity { v.symbol_scale = 3.0; },
            expect_created: "n2 ends 0.0,0.0->40.0,0.0 labelA tick2.0/2.0 scale1.0 counts2/2",
            expect_edited: "n3 ends 0.0,0.0->40.0,0.0 labelB tick3.0/-3.0 scale2.0 counts3/3",
            expect_reedited: "n3 ends 0.0,0.0->40.0,0.0 labelB tick3.0/-3.0 scale3.0 counts3/3",
            expect_edited_dxf: "",
            expect_reedited_dxf: "",
            expect_edited_dwg: "",
            expect_reedited_dwg: "",
        });
    }

    #[test]
    fn audit_python_section_symbol_references_over_real_ipc() {
        use acadrust::objects::{ClassObject, ClassObjectData, ObjectType};
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let object = |host: &mut HostSession<'_>, data: ClassObjectData| {
            let handle = host.document_mut().allocate_handle();
            let mut class_object = ClassObject::new(data);
            class_object.handle = handle;
            host.document_mut().objects.insert(handle, ObjectType::ClassObject(class_object));
            handle
        };
        let style = object(&mut host, ClassObjectData::SectionViewStyle(Default::default()));
        let view_rep = object(&mut host, ClassObjectData::ViewRep(Default::default()));
        let scale = host.document_mut().allocate_handle();
        let mut scale_object = acadrust::objects::Scale::new("1:1", 1.0, 1.0);
        scale_object.handle = scale;
        host.document_mut().objects.insert(scale, ObjectType::Scale(scale_object));
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let count = |host: &HostSession<'_>| host.document().entities().filter(|e| matches!(e, EntityType::SectionSymbol(_))).count();
        let create = |extra: &str| format!(
            "PY_EVAL ocs.active_document.create_entity('SectionSymbol', points=[{{'point': {{'x': 0.0, 'y': 0.0, 'z': 0.0}}}}, {{'point': {{'x': 10.0, 'y': 0.0, 'z': 0.0}}}}]{extra}).handle");
        // A symbol may reference the drawing's own style and view representation.
        dispatch(&mut host, &create(&format!(", style_handle={}, view_rep_handle={}", style.value(), view_rep.value())));
        assert_eq!(count(&host), 1, "{}", last(&host));
        let Some(EntityType::SectionSymbol(symbol)) = host.document().entities().find(|e| matches!(e, EntityType::SectionSymbol(_))) else { unreachable!() };
        assert_eq!((symbol.style_handle, symbol.view_rep_handle), (style, view_rep));
        // A standalone symbol needs neither.
        dispatch(&mut host, &create(""));
        assert_eq!(count(&host), 2, "{}", last(&host));
        // Wrong object kinds are refused: a style slot cannot hold a view rep, a
        // scale object is neither, and a swapped pair fails.
        for (extra, message) in [
            (format!(", style_handle={}", view_rep.value()), "not an existing SectionViewStyle"),
            (format!(", style_handle={}", scale.value()), "not an existing SectionViewStyle"),
            (format!(", view_rep_handle={}", style.value()), "not an existing ViewRep"),
            (", view_rep_handle=424242".to_string(), "not an existing ViewRep"),
        ] {
            dispatch(&mut host, &create(&extra));
            assert_eq!(count(&host), 2, "{extra}");
            assert!(last(&host).contains(message), "{extra}: {}", last(&host));
        }
    }

    #[test]
    fn audit_python_ole2frame_creation_rules_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let dir = std::env::temp_dir();
        let png = dir.join("ocs_ole_rules.png");
        let jpeg = dir.join("ocs_ole_rules.jpg");
        let tiff = dir.join("ocs_ole_rules.tif");
        let text = dir.join("ocs_ole_rules.txt");
        let picture = image::RgbImage::from_pixel(8, 4, image::Rgb([200, 40, 40]));
        picture.save(&png).unwrap();
        picture.save(&jpeg).unwrap();
        picture.save(&tiff).unwrap();
        std::fs::write(&text, "not a picture").unwrap();
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let frames = |host: &HostSession<'_>| host.document().entities().filter(|e| matches!(e, EntityType::Ole2Frame(_))).count();
        let picture_len = |host: &HostSession<'_>, index: usize| {
            let mut all: Vec<_> = host.document().entities().filter_map(|e| match e {
                EntityType::Ole2Frame(f) => Some(f.clone()), _ => None }).collect();
            all.sort_by_key(|f| f.common.handle.value());
            match acadrust::entities::extract_presentation(&all[index].encoded_payload()) {
                Some(acadrust::entities::OlePresentation::Raster(bytes)) => bytes,
                other => panic!("no embedded raster: {other:?}"),
            }
        };
        let embed = |file: &std::path::Path, extra: &str| format!(
            "PY_EVAL ocs.active_document.embed_picture({:?}, origin=(1, 2, 3), width=16{extra}).handle", file.display().to_string());

        // C: PNG and JPEG are stored as read, a TIFF is re-encoded as PNG, and
        // the frame keeps the picture's 2:1 aspect and the requested layer.
        dispatch(&mut host, &embed(&png, ", layer='PICTURES'"));
        dispatch(&mut host, &embed(&jpeg, ""));
        dispatch(&mut host, &embed(&tiff, ""));
        assert_eq!(frames(&host), 3, "{}", last(&host));
        assert_eq!(picture_len(&host, 0), std::fs::read(&png).unwrap(), "PNG is stored as read");
        assert_eq!(picture_len(&host, 1), std::fs::read(&jpeg).unwrap(), "JPEG is stored as read");
        assert_eq!(&picture_len(&host, 2)[..4], b"\x89PNG", "TIFF is re-encoded as PNG");
        let mut all: Vec<_> = host.document().entities().filter_map(|e| match e {
            EntityType::Ole2Frame(f) => Some(f.clone()), _ => None }).collect();
        all.sort_by_key(|f| f.common.handle.value());
        assert_eq!((all[0].common.layer.as_str(), all[1].common.layer.as_str()), ("PICTURES", "0"));
        let frame = &all[0];
        assert!((frame.lower_right_corner.x - frame.upper_left_corner.x - 16.0).abs() < 1e-9, "width 16");
        assert!((frame.upper_left_corner.y - frame.lower_right_corner.y - 8.0).abs() < 1e-9, "height 8 keeps the 2:1 aspect");
        assert!((frame.lower_right_corner.y - 2.0).abs() < 1e-9 && (frame.upper_left_corner.x - 1.0).abs() < 1e-9, "origin is the bottom-left corner");

        // V: refusals change nothing and record no undo step.
        let undo = host.app.tabs[0].history.undo_stack.len();
        let missing = dir.join("ocs_ole_rules_missing.png");
        for (command, message) in [
            (embed(&missing, ""), "cannot read the picture"),
            (embed(&text, ""), "cannot read the picture"),
            (embed(&png, ", layer=''").replace("width=16", "width=16"), "layer name is empty"),
            (embed(&png, "").replace("width=16", "width=0"), "must be finite and greater than zero"),
            (embed(&png, "").replace("origin=(1, 2, 3)", "origin=(float('nan'), 0, 0)"), "must be finite"),
            ("ocs.embed_picture('x.png', [1.0], 5.0, None)".to_string(), "origin needs 3 numbers"),
            ("ocs.active_document.create_entity('Ole2Frame', upper_left_corner=(0, 0, 0), lower_right_corner=(1, 1, 0))".to_string(), "doc.embed_picture"),
        ] {
            dispatch(&mut host, &format!("PY_EVAL {}", command.trim_start_matches("PY_EVAL ")));
            assert!(last(&host).contains(message), "{command}: {}", last(&host));
        }
        assert_eq!(frames(&host), 3);
        assert_eq!(host.app.tabs[0].history.undo_stack.len(), undo, "refusals record no undo step");
        for file in [&png, &jpeg, &tiff, &text] {
            let _ = std::fs::remove_file(file);
        }
    }

    #[test]
    fn audit_python_viewport_ids_and_scale_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let paper = host.document().block_records.iter().find(|r| r.is_paper_space()).map(|r| r.handle).unwrap();
        let layout = host.document().objects.values().find_map(|o| match o {
            acadrust::objects::ObjectType::Layout(l) if l.block_record == paper => Some(l.name.clone()), _ => None }).unwrap();
        host.app.tabs[0].scene.set_current_layout(layout);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dispatch = |host: &mut HostSession<'_>, command: &str| {
            assert!(process.dispatch(host, command, &mut |_| {}).expect("Python dispatch"));
        };
        let last = |host: &HostSession<'_>| host.app.command_line.history.last().unwrap().text.clone();
        let viewports = |host: &HostSession<'_>| {
            let mut list: Vec<_> = host.document().entities().filter_map(|e| match e {
                EntityType::Viewport(v) => Some((v.common.handle, v.id)), _ => None }).collect();
            list.sort_by_key(|(h, _)| h.value());
            list
        };
        let create = |width: f64, height: f64, view_height: f64| format!(
            "PY_EVAL ocs.active_document.create_entity('Viewport', owner_handle={}, center=(50, 50, 0), width={width}, height={height}, view_height={view_height}).handle", paper.value());
        dispatch(&mut host, &create(80.0, 60.0, 50.0));
        dispatch(&mut host, &create(40.0, 30.0, 10.0));
        dispatch(&mut host, &create(20.0, 20.0, 20.0));
        let list = viewports(&host);
        assert_eq!(list.len(), 3, "{}", last(&host));
        let ids: Vec<i16> = list.iter().map(|(_, id)| *id).collect();
        assert_eq!(ids, vec![2, 3, 4], "scripted viewports get unique ids of at least 2: {ids:?}");
        for (handle, expected) in [(list[0].0, 60.0 / 50.0), (list[1].0, 30.0 / 10.0), (list[2].0, 1.0)] {
            let Some(EntityType::Viewport(v)) = host.document().get_entity(handle) else { unreachable!() };
            assert!((v.custom_scale - expected).abs() < 1e-12, "custom_scale {} vs {expected}", v.custom_scale);
            assert!(!host.app.tabs[0].scene.wire_models_for(&[handle]).is_empty(), "viewport {handle:?} draws a frame");
        }
        // The scale follows the geometry; it cannot be set directly.
        let second = list[0].0;
        dispatch(&mut host, &format!("PY_EVAL ocs.update_many('Zoom', [{{'handle':{}, 'view_height':25.0}}])", second.value()));
        let Some(EntityType::Viewport(zoomed)) = host.document().get_entity(second) else { unreachable!() };
        assert!((zoomed.custom_scale - 60.0 / 25.0).abs() < 1e-12, "{}", last(&host));
        assert_eq!(zoomed.id, 2, "an edit keeps the id");
        dispatch(&mut host, &format!("PY_EVAL ocs.update_many('Resize', [{{'handle':{}, 'height':90.0}}])", second.value()));
        let Some(EntityType::Viewport(resized)) = host.document().get_entity(second) else { unreachable!() };
        assert!((resized.custom_scale - 90.0 / 25.0).abs() < 1e-12);
        let before = host.document().get_entity(second).unwrap().clone();
        dispatch(&mut host, &format!("PY_EVAL ocs.update_many('Scale', [{{'handle':{}, 'custom_scale':3.0}}])", second.value()));
        assert!(last(&host).contains("read-only"), "{}", last(&host));
        assert_eq!(host.document().get_entity(second), Some(&before));
        // Control: the engine's default id 0 is not invisible here (there is no
        // sheet viewport to confuse it with), so the derived id matters for
        // uniqueness, not for visibility.
        let mut bare = acadrust::entities::Viewport::new();
        bare.common.owner_handle = paper;
        bare.width = 40.0;
        bare.height = 30.0;
        let bare = host.document_mut().add_entity(EntityType::Viewport(bare)).unwrap();
        assert_eq!(host.document().get_entity(bare).map(|e| match e { EntityType::Viewport(v) => v.id, _ => -1 }), Some(0));
        assert!(!host.app.tabs[0].scene.wire_models_for(&[bare]).is_empty());
    }

    /// Evidence for the RasterImage ledger row: what a save does with a bare
    /// image (OCS's native IMAGE command) and with a definition-linked one.
    #[test]
    #[ignore = "exploratory evidence"]
    fn spike_raster_image_definition_persistence() {
        let dir = std::env::temp_dir();
        let path = dir.join("ocs_spike_image.png").to_string_lossy().into_owned();
        image::RgbaImage::from_pixel(8, 4, image::Rgba([1, 2, 3, 255])).save(&path).unwrap();
        for linked in [false, true] {
            let mut doc = acadrust::CadDocument::new();
            let mut img = acadrust::entities::RasterImage::with_size(&path, acadrust::types::Vector3::ZERO, 8.0, 4.0, 16.0, 8.0);
            if linked {
                let h = doc.allocate_handle();
                let mut def = acadrust::objects::ImageDefinition::with_dimensions(path.clone(), 8, 4);
                def.handle = h;
                def.is_loaded = true;
                doc.objects.insert(h, acadrust::objects::ObjectType::ImageDefinition(def));
                img.definition_handle = Some(h);
            }
            let handle = doc.add_entity(EntityType::RasterImage(img)).unwrap();
            for (label, document) in [
                ("DWG", crate::io::load_bytes("i.dwg", acadrust::DwgWriter::write_to_vec(&doc).unwrap()).unwrap()),
                ("DXF", crate::io::load_bytes("i.dxf", acadrust::DxfWriter::new(&doc).write_to_vec().unwrap()).unwrap()),
            ] {
                let entity = document.get_entity(handle);
                let defs = document.objects.values().filter(|o| matches!(o, acadrust::objects::ObjectType::ImageDefinition(_))).count();
                let reactors = document.objects.values().filter(|o| matches!(o, acadrust::objects::ObjectType::ImageDefinitionReactor(_))).count();
                let dicts = document.objects.values().filter(|o| matches!(o, acadrust::objects::ObjectType::Dictionary(d) if d.entries.iter().any(|(k, _)| k.contains("IMAGE")))).count();
                match entity {
                    Some(EntityType::RasterImage(i)) => eprintln!("SPIKE linked={linked} {label}: file_path_ok={} definition={:?} reactor={:?} defs={defs} reactors={reactors} image_dicts={dicts}",
                        i.file_path == path, i.definition_handle.map(|h| h.value()), i.definition_reactor_handle.map(|h| h.value())),
                    other => eprintln!("SPIKE linked={linked} {label}: came back as {:?}", other.map(|e| format!("{e:?}").chars().take(24).collect::<String>())),
                }
            }
        }
    }

    /// Layer table through Python over the real runner: create, modify, rename,
    /// delete and make current, with refusals that leave the drawing untouched,
    /// one undo step per change, and DWG/DXF persistence of every property.
    #[test]
    fn audit_python_layer_table_over_real_ipc() {
        use acadrust::types::{Color, LineWeight};
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dir = std::env::temp_dir();
        let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
            let script = dir.join(format!("ocs_layers_{tag}_{}.py", std::process::id()));
            std::fs::write(&script, format!(concat!(
                "def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + ' '.join(refused) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
            assert!(process.dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {}).unwrap());
            let _ = std::fs::remove_file(&script);
        };
        let report = |document: &CadDocument| -> (Vec<String>, Vec<String>) {
            let name = document.layers.iter().map(|l| l.name.clone()).filter(|n| n.starts_with("REPORT")).last()
                .expect("script report layer");
            let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
            let words = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
            (words(refused), words(accepted))
        };

        // Build: two layers with full properties, a modify, an entity, a rename, and the refusals.
        run(&mut host, "build", r#"
L.create('Walls', color=1, lineweight=50, description='load bearing', transparency=30)
L.create('Grid', color=(10, 200, 30), linetype='Continuous', off=True, plottable=False)
L.modify('Walls', color=5, locked=True)
doc.create_entity('Line', start=P(0, 0, 0), end=P(1, 0, 0), layer='Walls')
L.rename('Walls', 'Structure')
check('dup', lambda: L.create('structure'))
check('badname', lambda: L.create('a<b'))
check('empty', lambda: L.create('  '))
check('badcolor0', lambda: L.create('C1', color=0))
check('badcolorlayer', lambda: L.create('C1', color={'kind': 'ByLayer'}))
check('badltype', lambda: L.create('C2', linetype='Nope'))
check('badweight', lambda: L.create('C3', lineweight=999))
check('badtransp', lambda: L.create('C4', transparency=95))
check('unknownkey', lambda: L.create('C5', colour=1))
check('rename0', lambda: L.rename('0', 'X'))
check('renameclash', lambda: L.rename('Grid', 'STRUCTURE'))
check('renamemissing', lambda: L.rename('Nope', 'X2'))
check('del0', lambda: L.delete('0'))
check('delcurrent', lambda: L.delete(L.current['name']))
check('delused', lambda: L.delete('Structure'))
check('freezecurrent', lambda: L.modify(L.current['name'], frozen=True))
check('modnone', lambda: L.modify('Grid'))
check('modmissing', lambda: L.modify('Nope', off=True))
check('currentmissing', lambda: L.set_current('Nope'))
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "accepted invalid requests: {accepted:?}");
        assert_eq!(refused.len(), 19, "{refused:?}");

        let check_state = |label: &str, document: &CadDocument| {
            let names: Vec<_> = document.layers.iter().map(|l| l.name.as_str()).collect();
            assert!(!names.iter().any(|n| n.eq_ignore_ascii_case("Walls")), "{label}: old name gone: {names:?}");
            assert!(!names.iter().any(|n| n.starts_with('C') && n.len() == 2), "{label}: a refused request left a layer: {names:?}");
            let structure = document.layers.get("Structure").unwrap_or_else(|| panic!("{label}: Structure missing"));
            assert_eq!(structure.color, Color::Index(5), "{label}");
            assert_eq!(structure.line_weight, LineWeight::Value(50), "{label}");
            assert!(structure.flags.locked, "{label}");
            assert_eq!(structure.description, "load bearing", "{label}");
            assert_ne!(structure.transparency, acadrust::types::Transparency::ByLayer, "{label}: transparency");
            let grid = document.layers.get("Grid").unwrap_or_else(|| panic!("{label}: Grid missing"));
            assert_eq!(grid.color, Color::Rgb { r: 10, g: 200, b: 30 }, "{label}");
            assert!(grid.flags.off && !grid.is_plottable, "{label}");
            assert_eq!(grid.line_type.to_uppercase(), "CONTINUOUS", "{label}");
            let line = document.entities().find_map(|e| match e { EntityType::Line(l) => Some(l.clone()), _ => None })
                .unwrap_or_else(|| panic!("{label}: line missing"));
            assert_eq!(line.common.layer, "Structure", "{label}: entity followed the rename");
        };
        check_state("live", host.document());
        let dwg = crate::io::load_bytes("layers.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("layers.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        check_state("DWG", &dwg);
        check_state("DXF", &dxf);

        // Destroy: current layer, delete with and without objects.
        run(&mut host, "destroy", r#"
L.set_current('Grid')
check('delcurrent2', lambda: L.delete('Grid'))
check('freezecurrent2', lambda: L.modify('Grid', frozen=True))
L.set_current('0')
L.delete('Grid')
check('delused2', lambda: L.delete('Structure'))
L.delete('Structure', erase_objects=True)
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "{accepted:?}");
        assert_eq!(refused, vec!["delcurrent2", "freezecurrent2", "delused2"]);
        let document = host.document();
        assert!(document.layers.get("Grid").is_none() && document.layers.get("Structure").is_none());
        assert_eq!(document.header.current_layer_name, "0");
        assert!(!document.entities().any(|e| matches!(e, EntityType::Line(_))), "erase_objects erased the line");

        // U: the last two steps are the report layer and the erasing delete;
        // undoing them restores the deleted layer and its object together.
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        let before = app.tabs[0].history.undo_stack.len();
        app.undo_steps(2);
        assert_eq!(app.tabs[0].history.undo_stack.len(), before - 2);
        let document = &app.tabs[0].scene.document;
        assert!(document.layers.get("Structure").is_some(), "undo restored the layer");
        assert!(document.entities().any(|e| matches!(e, EntityType::Line(_))), "undo restored the line");
    }

    /// Text and dimension styles through Python over the real runner: create,
    /// modify, copy, rename (references follow), delete and make current, with
    /// refusals that change nothing, DWG/DXF persistence and one-step undo.
    #[test]
    fn audit_python_text_and_dim_styles_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dir = std::env::temp_dir();
        let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
            let script = dir.join(format!("ocs_styles_{tag}_{}.py", std::process::id()));
            std::fs::write(&script, format!(concat!(
                "doc = ocs.active_document\nS = doc.text_styles\nD = doc.dim_styles\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
            assert!(process.dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {}).unwrap());
            let _ = std::fs::remove_file(&script);
        };
        let report = |document: &CadDocument| -> (usize, Vec<String>) {
            let name = document.layers.iter().map(|l| l.name.clone()).filter(|n| n.starts_with("REPORT")).last()
                .expect("script report layer");
            let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
            (refused.trim().parse().unwrap(), accepted.split_whitespace().map(str::to_owned).collect())
        };

        run(&mut host, "build", r#"
S.create('Title', height=5, width_factor=0.8, oblique=15, font='romans', big_font='bigfont', backward=True, annotative=True)
S.create('Notes', font='arial.ttf')
S.modify('Title', height=6, upside_down=True)
D.create('Metric', dimscale=2, dimtxt=3.5, dimasz=2.5, dimtxsty='Title')
D.create('Metric2', copy_from='Metric', dimtxt=4)
D.modify('Metric', dimscale=3)
S.rename('Notes', 'Remarks')
S.rename('Title', 'Heading')
check('t_dup', lambda: S.create('heading'))
check('t_badname', lambda: S.create('a|b'))
check('t_height', lambda: S.create('T1', height=-1))
check('t_width0', lambda: S.create('T2', width_factor=0))
check('t_oblique', lambda: S.create('T3', oblique=89))
check('t_nofont', lambda: S.create('T4', font=''))
check('t_unknown', lambda: S.create('T5', fnt='x'))
check('t_modmissing', lambda: S.modify('Nope', height=1))
check('t_modnone', lambda: S.modify('Heading'))
check('t_renameStandard', lambda: S.rename('Standard', 'Std2'))
check('t_rename_case', lambda: S.rename('Heading', 'HEADING'))
check('t_rename_clash', lambda: S.rename('Remarks', 'heading'))
check('t_delStandard', lambda: S.delete('Standard'))
check('t_delInUse', lambda: S.delete('Heading'))
check('t_current_missing', lambda: S.set_current('Nope'))
check('d_dup', lambda: D.create('metric'))
check('d_unknown', lambda: D.create('D1', dimbogus=1))
check('d_handle', lambda: D.create('D2', dimtxsty_handle=5))
check('d_name', lambda: D.modify('Metric', name='X'))
check('d_type', lambda: D.create('D3', dimscale='big'))
check('d_scale0', lambda: D.create('D4', dimscale=0))
check('d_txt', lambda: D.create('D5', dimtxt=-1))
check('d_txsty', lambda: D.create('D6', dimtxsty='Nope'))
check('d_copy_missing', lambda: D.create('D7', copy_from='Nope'))
check('d_delStandard', lambda: D.delete('Standard'))
check('d_modmissing', lambda: D.modify('Nope', dimscale=2))
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "accepted invalid requests: {accepted:?}");
        assert_eq!(refused, 26);

        let check_state = |label: &str, document: &CadDocument| {
            assert!(document.text_styles.get("Title").is_none() && document.text_styles.get("Notes").is_none(), "{label}: old names gone");
            let title = document.text_styles.get("Heading").unwrap_or_else(|| panic!("{label}: Heading missing"));
            assert_eq!(title.height, 6.0, "{label}");
            assert!((title.width_factor - 0.8).abs() < 1e-9, "{label}");
            assert!((title.oblique_angle - 15f64.to_radians()).abs() < 1e-6, "{label}: oblique {}", title.oblique_angle);
            assert_eq!(title.font_file.to_lowercase(), "romans", "{label}");
            assert_eq!(title.big_font_file.to_lowercase(), "bigfont", "{label}");
            assert!(title.flags.backward && title.flags.upside_down, "{label}");
            let remarks = document.text_styles.get("Remarks").unwrap_or_else(|| panic!("{label}: Remarks missing"));
            assert_eq!(remarks.font_file.to_lowercase(), "arial.ttf", "{label}: {remarks:?}");
            let metric = document.dim_styles.get("Metric").unwrap_or_else(|| panic!("{label}: Metric missing"));
            assert_eq!((metric.dimscale, metric.dimtxt, metric.dimasz), (3.0, 3.5, 2.5), "{label}");
            assert_eq!(metric.dimtxsty, "Heading", "{label}: dimtxsty followed the rename");
            if label == "DXF" {
                assert_eq!(metric.dimtxsty_handle, title.handle, "{label}: text style link by handle");
            }
            let copy = document.dim_styles.get("Metric2").unwrap_or_else(|| panic!("{label}: Metric2 missing"));
            assert_eq!((copy.dimscale, copy.dimtxt), (2.0, 4.0), "{label}: copied before Metric changed");
        };
        check_state("live", host.document());
        let live_title = host.document().text_styles.get("Heading").unwrap().clone();
        assert!(live_title.annotative, "annotative is kept in the live style");
        assert_eq!(host.document().dim_styles.get("Metric").unwrap().dimtxsty_handle, live_title.handle, "dim style links the text style handle");
        let dwg = crate::io::load_bytes("styles.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("styles.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        check_state("DWG", &dwg);
        check_state("DXF", &dxf);

        run(&mut host, "destroy", r#"
S.set_current('Remarks')
D.set_current('Metric2')
check('t_delcurrent', lambda: S.delete('Remarks'))
check('d_delcurrent', lambda: D.delete('Metric2'))
check('d_delInUseByCurrentOnly', lambda: D.delete('Metric2'))
S.set_current('Standard')
D.set_current('Standard')
S.delete('Remarks')
D.delete('Metric2')
D.delete('Metric')
S.delete('Heading')
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "{accepted:?}");
        assert_eq!(refused, 3);
        let document = host.document();
        for name in ["Heading", "Remarks"] {
            assert!(document.text_styles.get(name).is_none(), "{name} deleted");
        }
        for name in ["Metric", "Metric2"] {
            assert!(document.dim_styles.get(name).is_none(), "{name} deleted");
        }
        assert_eq!(document.header.current_text_style_name, "Standard");
        assert_eq!(document.header.current_dimstyle_name, "Standard");

        // U: the final report layer and the four deletes are five steps; undoing
        // them all brings the styles back.
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(5);
        let document = &app.tabs[0].scene.document;
        assert!(document.text_styles.get("Heading").is_some() && document.text_styles.get("Remarks").is_some(), "undo restored the text styles");
        assert!(document.dim_styles.get("Metric").is_some() && document.dim_styles.get("Metric2").is_some(), "undo restored the dimension styles");
    }

    /// Block definitions through Python over the real runner: create from
    /// entities (with and without erasing them), modify, rename with inserts
    /// following, guarded delete, DWG/DXF persistence and undo.
    #[test]
    fn audit_python_blocks_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dir = std::env::temp_dir();
        let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
            let script = dir.join(format!("ocs_blocks_{tag}_{}.py", std::process::id()));
            std::fs::write(&script, format!(concat!(
                "def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nB = doc.blocks\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
            assert!(process.dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {}).unwrap());
            let _ = std::fs::remove_file(&script);
        };
        let report = |document: &CadDocument| -> (usize, Vec<String>) {
            let name = document.layers.iter().map(|l| l.name.clone()).filter(|n| n.starts_with("REPORT")).last()
                .expect("script report layer");
            let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
            (refused.trim().parse().unwrap(), accepted.split_whitespace().map(str::to_owned).collect())
        };

        run(&mut host, "build", r#"
line = doc.create_entity('Line', start=P(0, 0, 0), end=P(4, 0, 0))
circle = doc.create_entity('Circle', center=P(2, 2, 0), radius=1)
B.create('Widget', [line, circle], base_point=(1, 1, 0), description='a widget')
B.create('Widget2', [line.handle], erase_originals=True)
doc.create_entity('Insert', block_name='Widget', insert_point=P(10, 0, 0))
doc.create_entity('Insert', block_name='widget', insert_point=P(20, 0, 0))
B.modify('Widget', explodable=False, description='a gadget')
B.rename('Widget', 'Gadget')
member = B['Gadget']['entities'][0]['handle']
check('dup', lambda: B.create('gadget', [circle]))
check('empty', lambda: B.create('E1', []))
check('badname', lambda: B.create('a<b', [circle]))
check('star', lambda: B.create('*U9', [circle]))
check('missing_entity', lambda: B.create('E2', [999999]))
check('twice', lambda: B.create('E3', [circle, circle]))
check('nonfinite_base', lambda: B.create('E4', [circle], base_point=(1e999, 0, 0)))
check('in_block', lambda: B.create('E5', [member]))
check('unknown_key', lambda: ocs.block_operation('create', 'E6', {'entities': [circle.handle], 'origin': [0, 0, 0]}))
check('rename_missing', lambda: B.rename('Nope', 'X'))
check('rename_clash', lambda: B.rename('Gadget', 'WIDGET2'))
check('rename_case', lambda: B.rename('Gadget', 'GADGET'))
check('rename_star', lambda: B.rename('Gadget', '*Y'))
check('delete_missing', lambda: B.delete('Nope'))
check('delete_inserted', lambda: B.delete('Gadget'))
check('delete_layout', lambda: B.delete('*Model_Space'))
check('modify_none', lambda: B.modify('Gadget'))
check('modify_missing', lambda: B.modify('Nope', description='x'))
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "accepted invalid requests: {accepted:?}");
        assert_eq!(refused, 18);

        let check_state = |label: &str, document: &CadDocument| {
            assert!(document.block_records.get("Widget").is_none(), "{label}: old block name gone");
            let members = |name: &str| -> Vec<EntityType> {
                let record = document.block_records.get(name).unwrap_or_else(|| panic!("{label}: block {name} missing"));
                document.entities().filter(|e| e.common().owner_handle == record.handle
                    && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))).cloned().collect()
            };
            let gadget = members("Gadget");
            assert_eq!(gadget.len(), 2, "{label}: Gadget members");
            let line = gadget.iter().find_map(|e| match e { EntityType::Line(l) => Some(l.clone()), _ => None }).unwrap_or_else(|| panic!("{label}: line"));
            assert_eq!((line.start.x, line.start.y, line.end.x, line.end.y), (-1.0, -1.0, 3.0, -1.0), "{label}: shifted by -base");
            let circle = gadget.iter().find_map(|e| match e { EntityType::Circle(c) => Some(c.clone()), _ => None }).unwrap_or_else(|| panic!("{label}: circle"));
            assert_eq!((circle.center.x, circle.center.y, circle.radius), (1.0, 1.0, 1.0), "{label}");
            let record = document.block_records.get("Gadget").unwrap();
            assert_eq!((record.base_point.x, record.base_point.y), (1.0, 1.0), "{label}: base point");
            assert!(!record.explodable, "{label}: explodable");
            assert_eq!(record.description, "a gadget", "{label}: description");
            assert_eq!(members("Widget2").len(), 1, "{label}: Widget2 members");
            let inserts: Vec<_> = document.entities().filter_map(|e| match e { EntityType::Insert(i) => Some(i.block_name.clone()), _ => None }).collect();
            assert_eq!(inserts.iter().filter(|n| n.as_str() == "Gadget").count(), 2, "{label}: inserts follow the rename: {inserts:?}");
            // originals: Widget kept the circle and line in model space, Widget2 erased its line copy's source
            let canvas_kinds: Vec<_> = document.entities().filter(|e| {
                document.block_records.iter().any(|r| r.handle == e.common().owner_handle && r.is_layout())
            }).map(|e| format!("{:?}", e).chars().take(6).collect::<String>()).collect();
            assert_eq!(canvas_kinds.iter().filter(|k| k.starts_with("Circle")).count(), 1, "{label}: circle original kept: {canvas_kinds:?}");
            assert_eq!(canvas_kinds.iter().filter(|k| k.starts_with("Line(")).count(), 0, "{label}: line original erased: {canvas_kinds:?}");
        };
        check_state("live", host.document());
        assert_eq!(host.document().block_records.get("Gadget").unwrap().description, "a gadget");
        let dwg = crate::io::load_bytes("blocks.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("blocks.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        check_state("DWG", &dwg);
        check_state("DXF", &dxf);

        run(&mut host, "destroy", r#"
for entity in list(doc.entities):
    if entity.kind == 'Insert':
        doc.delete_entity(entity)
B.delete('Gadget')
B.delete('Widget2')
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "{accepted:?}");
        assert_eq!(refused, 0);
        let document = host.document();
        assert!(document.block_records.get("Gadget").is_none() && document.block_records.get("Widget2").is_none());
        assert!(!document.entities().any(|e| matches!(e, EntityType::Line(_))), "block members were removed with their definitions");
        assert_eq!(document.entities().filter(|e| matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_))).count(), 0);

        // U: the report layer and both block deletes; undoing them restores the definitions.
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(3);
        let document = &app.tabs[0].scene.document;
        let members = |name: &str| document.block_records.get(name).map(|r| r.entity_handles.len());
        assert_eq!(members("Gadget"), Some(2), "undo restored Gadget with its members");
        assert_eq!(members("Widget2"), Some(1), "undo restored Widget2");
    }

    /// Block contents through Python over the real runner: add entities
    /// (including an attribute definition) to a definition, edit and delete them
    /// with the ordinary entity API, refuse bad requests, persist through
    /// DWG/DXF and undo.
    #[test]
    fn audit_python_block_contents_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dir = std::env::temp_dir();
        let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
            let script = dir.join(format!("ocs_bcontents_{tag}_{}.py", std::process::id()));
            std::fs::write(&script, format!(concat!(
                "def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nB = doc.blocks\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
            assert!(process.dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {}).unwrap());
            let _ = std::fs::remove_file(&script);
        };
        let report = |document: &CadDocument| -> (usize, Vec<String>) {
            let name = document.layers.iter().map(|l| l.name.clone()).filter(|n| n.starts_with("REPORT")).last()
                .expect("script report layer");
            let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
            (refused.trim().parse().unwrap(), accepted.split_whitespace().map(str::to_owned).collect())
        };

        run(&mut host, "build", r#"
seed = doc.create_entity('Circle', center=P(0, 0, 0), radius=1)
B.create('Part', [seed], erase_originals=True)
line = doc.create_entity('Line', start=P(0, 0, 0), end=P(5, 0, 0), block='Part')
arc = doc.create_entity('Arc', center=P(0, 0, 0), radius=5.0, start_angle=0.0, end_angle=1.5, block='Part')
tag = doc.create_entity('AttributeDefinition', block='Part', tag='PART_NO', prompt='Part number', default_value='PN-001', insertion_point=P(1, 2, 0), height=2.5)
with doc.transaction('Lengthen'):
    line.end = (8, 0, 0)
doc.delete_entity(arc)
B.create('Outer', [doc.create_entity('Circle', center=P(0, 0, 0), radius=1)], erase_originals=True)
doc.create_entity('Insert', block_name='Part', insert_point=P(0, 0, 0), block='Outer')
check('missing_block', lambda: doc.create_entity('Line', start=P(0, 0, 0), end=P(1, 0, 0), block='Nope'))
check('layout_block', lambda: doc.create_entity('Line', start=P(0, 0, 0), end=P(1, 0, 0), block='*Model_Space'))
check('zero_circle', lambda: doc.create_entity('Circle', center=P(1, 1, 0), radius=0, block='Part'))
check('viewport', lambda: doc.create_entity('Viewport', block='Part'))
check('bad_tag', lambda: doc.create_entity('AttributeDefinition', block='Part', tag='  ', prompt='p', default_value='', insertion_point=P(0, 0, 0), height=1))
check('bad_height', lambda: doc.create_entity('AttributeDefinition', block='Part', tag='T', prompt='p', default_value='', insertion_point=P(0, 0, 0), height=0))
check('self_insert', lambda: doc.create_entity('Insert', block_name='Part', insert_point=P(0, 0, 0), block='Part'))
check('cycle', lambda: doc.create_entity('Insert', block_name='Outer', insert_point=P(0, 0, 0), block='Part'))
check('unknown_insert_block', lambda: doc.create_entity('Insert', block_name='Ghost', insert_point=P(0, 0, 0), block='Part'))
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "accepted invalid requests: {accepted:?}");
        assert_eq!(refused, 9);

        let check_state = |label: &str, document: &CadDocument| {
            let record = document.block_records.get("Part").unwrap_or_else(|| panic!("{label}: Part missing"));
            let members: Vec<EntityType> = document.entities()
                .filter(|e| e.common().owner_handle == record.handle && !matches!(e, EntityType::Block(_) | EntityType::BlockEnd(_)))
                .cloned().collect();
            assert_eq!(members.len(), 3, "{label}: circle, line, attribute definition (the arc was deleted): {:?}",
                members.iter().map(|e| format!("{e:?}").chars().take(10).collect::<String>()).collect::<Vec<_>>());
            let line = members.iter().find_map(|e| match e { EntityType::Line(l) => Some(l.clone()), _ => None }).unwrap_or_else(|| panic!("{label}: line"));
            assert_eq!((line.end.x, line.end.y), (8.0, 0.0), "{label}: edited inside the block");
            assert!(members.iter().any(|e| matches!(e, EntityType::Circle(_))), "{label}: circle from the seed");
            let attdef = members.iter().find_map(|e| match e { EntityType::AttributeDefinition(a) => Some(a.clone()), _ => None }).unwrap_or_else(|| panic!("{label}: attdef"));
            assert_eq!((attdef.tag.as_str(), attdef.prompt.as_str(), attdef.default_value.as_str()), ("PART_NO", "Part number", "PN-001"), "{label}");
            assert!(!members.iter().any(|e| matches!(e, EntityType::Arc(_))), "{label}: arc deleted");
            let outer = document.block_records.get("Outer").unwrap_or_else(|| panic!("{label}: Outer missing"));
            let nested = document.entities().any(|e| matches!(e, EntityType::Insert(i) if i.block_name == "Part" && i.common.owner_handle == outer.handle));
            assert!(nested, "{label}: Part is inserted inside Outer");
        };
        check_state("live", host.document());
        let dwg = crate::io::load_bytes("bc.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("bc.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        check_state("DWG", &dwg);
        check_state("DXF", &dxf);

        // U: one more add, then the report layer; undoing both leaves the block as before.
        run(&mut host, "extra", "doc.create_entity('Circle', center=P(9, 9, 0), radius=2, block='Part')");
        assert_eq!(report(host.document()).1.len(), 0);
        {
            let document = host.document();
            let record = document.block_records.get("Part").unwrap();
            let live: Vec<_> = record.entity_handles.iter().map(|h| (h.value(), document.get_entity(*h).is_some())).collect();
            assert_eq!(record.entity_handles.len(), 4, "record entity_handles (handle, exists): {live:?}");
        }
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(2);
        // Membership bookkeeping is intrinsic to an add and stays listed across an entity-delta
        // undo (see `Scene::add_entity_internal`), so count the entities that really exist.
        let document = &app.tabs[0].scene.document;
        let part = document.block_records.get("Part").unwrap();
        let existing = part.entity_handles.iter().filter(|h| document.get_entity(**h).is_some()).count();
        assert_eq!(existing, 3, "undo removed the added member");
    }

    /// Linetypes and layouts through Python over the real runner: create,
    /// modify, rename (layers follow), guarded delete, page setup, entities
    /// placed on a layout, DWG/DXF persistence and undo.
    #[test]
    fn audit_python_linetypes_and_layouts_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dir = std::env::temp_dir();
        let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
            let script = dir.join(format!("ocs_ltlay_{tag}_{}.py", std::process::id()));
            std::fs::write(&script, format!(concat!(
                "def P(x, y, z): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nLT = doc.linetypes\nLY = doc.layouts\nL = doc.layers\n",
                "refused, accepted = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        accepted.append(tag)\n",
                "try:\n{body}\n",
                "except Exception as error:\n",
                "    accepted.append('SCRIPTERROR_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:120])\n",
                "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(accepted))\n",
            ), body = body.lines().map(|line| format!("    {line}")).collect::<Vec<_>>().join("\n"))).unwrap();
            assert!(process.dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {}).unwrap());
            let _ = std::fs::remove_file(&script);
        };
        let report = |document: &CadDocument| -> (usize, Vec<String>) {
            let name = document.layers.iter().map(|l| l.name.clone()).filter(|n| n.starts_with("REPORT")).last()
                .expect("script report layer");
            let (refused, accepted) = name["REPORT".len()..].split_once('~').unwrap();
            (refused.trim().parse().unwrap(), accepted.split_whitespace().map(str::to_owned).collect())
        };

        run(&mut host, "build", r#"
LT.create('Bracket', [12, -3, 2, -3], 'Phantom test')
LT.create('Dots', [0, -2], 'dots')
LT.modify('Dots', pattern=[0, -4], description='sparse dots')
LT.create('Temp', [1, -1])
L.create('Hidden2', linetype='Bracket')
L.create('Dotted2', linetype='Dots')
LT.rename('Dots', 'Sparse')
LY.create('Sheet1')
LY.create('Sheet2')
LY.set_page('Sheet1', paper_size=(420, 297), rotation=0, scale=(1, 50))
LY.rename('Sheet2', 'Details')
LY.set_current('Sheet1')
doc.create_entity('Circle', center=P(50, 50, 0), radius=5)
LY.set_current('Model')
check('lt_dup', lambda: LT.create('bracket', [1, -1]))
check('lt_standard_dup', lambda: LT.create('Dashed', [1, -1]))
check('lt_badname', lambda: LT.create('a|b', [1, -1]))
check('lt_empty', lambda: LT.create('X1', []))
check('lt_single', lambda: LT.create('X2', [1]))
check('lt_nogap', lambda: LT.create('X3', [1, 2]))
check('lt_nodash', lambda: LT.create('X4', [-1, -2]))
check('lt_nonfinite', lambda: LT.create('X5', [1e999, -1]))
check('lt_toomany', lambda: LT.create('X6', [1, -1] * 7))
check('lt_modnone', lambda: LT.modify('Bracket'))
check('lt_modmissing', lambda: LT.modify('Nope', description='x'))
check('lt_modcontinuous', lambda: LT.modify('Continuous', description='x'))
check('lt_renamecontinuous', lambda: LT.rename('Continuous', 'Solid2'))
check('lt_renameclash', lambda: LT.rename('Temp', 'BRACKET'))
check('lt_renamecase', lambda: LT.rename('Temp', 'TEMP'))
check('lt_delcontinuous', lambda: LT.delete('ByLayer'))
check('lt_delinuse', lambda: LT.delete('Bracket'))
check('lt_delmissing', lambda: LT.delete('Nope'))
check('ly_dup', lambda: LY.create('sheet1'))
check('ly_model', lambda: LY.create('Model'))
check('ly_badname', lambda: LY.create('a<b'))
check('ly_renamemodel', lambda: LY.rename('Model', 'X'))
check('ly_renamemissing', lambda: LY.rename('Nope', 'X'))
check('ly_renameclash', lambda: LY.rename('Details', 'SHEET1'))
check('ly_renamecase', lambda: LY.rename('Details', 'DETAILS'))
check('ly_deletemodel', lambda: LY.delete('Model'))
check('ly_deletemissing', lambda: LY.delete('Nope'))
check('ly_currentmissing', lambda: LY.set_current('Nope'))
check('ly_pagemissing', lambda: LY.set_page('Nope', rotation=0))
check('ly_pagemodel', lambda: LY.set_page('Model', rotation=0))
check('ly_pagenone', lambda: LY.set_page('Sheet1'))
check('ly_badsize', lambda: LY.set_page('Sheet1', paper_size=(0, 10)))
check('ly_badrotation', lambda: LY.set_page('Sheet1', rotation=45))
check('ly_badscale', lambda: LY.set_page('Sheet1', scale=(0, 1)))
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "accepted invalid requests: {accepted:?}");
        assert_eq!(refused, 34);

        let check_state = |label: &str, document: &CadDocument| {
            assert!(document.line_types.get("Dots").is_none(), "{label}: old linetype name gone");
            let phantom = document.line_types.get("Bracket").unwrap_or_else(|| panic!("{label}: Bracket missing"));
            let lengths: Vec<f64> = phantom.elements.iter().map(|e| e.length).collect();
            assert_eq!(lengths, vec![12.0, -3.0, 2.0, -3.0], "{label}: pattern");
            assert_eq!(phantom.pattern_length, 20.0, "{label}");
            let sparse = document.line_types.get("Sparse").unwrap_or_else(|| panic!("{label}: Sparse missing"));
            assert_eq!(sparse.elements.iter().map(|e| e.length).collect::<Vec<_>>(), vec![0.0, -4.0], "{label}: modified pattern");
            assert_eq!(document.layers.get("Dotted2").unwrap().line_type, "Sparse", "{label}: layer followed the rename");
            assert_eq!(document.layers.get("Hidden2").unwrap().line_type, "Bracket", "{label}");
            let layouts = |name: &str| document.objects.values().find_map(|o| match o {
                acadrust::objects::ObjectType::Layout(l) if l.name == name && !l.block_record.is_null() => Some(l.clone()),
                _ => None,
            });
            assert!(layouts("Sheet2").is_none(), "{label}: old layout name gone");
            let sheet = layouts("Sheet1").unwrap_or_else(|| panic!("{label}: Sheet1 missing"));
            assert_eq!((sheet.paper_width, sheet.paper_height), (420.0, 297.0), "{label}: paper size");
            assert_eq!(sheet.plot_rotation, 0, "{label}: rotation");
            assert_eq!((sheet.plot_scale_numerator, sheet.plot_scale_denominator), (1.0, 50.0), "{label}: scale");
            assert!(layouts("Details").is_some(), "{label}: Details missing");
            let on_sheet = document.entities().filter(|e| e.common().owner_handle == sheet.block_record && matches!(e, EntityType::Circle(_))).count();
            assert_eq!(on_sheet, 1, "{label}: the circle was created on the sheet");
            assert!(!document.entities().any(|e| matches!(e, EntityType::Circle(_)) && document.block_records.iter().any(|r| r.name == "*Model_Space" && r.handle == e.common().owner_handle)),
                "{label}: no circle in model space");
        };
        check_state("live", host.document());
        assert_eq!(host.app.tabs[0].scene.current_layout, "Model", "the script switched back to Model");
        let dwg = crate::io::load_bytes("ltl.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("ltl.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        check_state("DWG", &dwg);
        check_state("DXF", &dxf);

        run(&mut host, "destroy", r#"
LY.set_current('Details')
LY.delete('Details')
LY.delete('Sheet1')
LT.delete('Temp')
"#);
        let (refused, accepted) = report(host.document());
        assert!(accepted.is_empty(), "{accepted:?}");
        assert_eq!(refused, 0);
        let document = host.document();
        assert!(document.line_types.get("Temp").is_none());
        assert!(!document.objects.values().any(|o| matches!(o, acadrust::objects::ObjectType::Layout(l) if l.name == "Sheet1" || l.name == "Details")));
        assert!(!document.entities().any(|e| matches!(e, EntityType::Circle(_))), "the sheet's circle went with its layout");
        assert_eq!(host.app.tabs[0].scene.current_layout, "Model", "deleting the current layout fell back to Model");

        // U: the report layer and the three deletes; undoing them restores the layouts and the linetype.
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(4);
        let document = &app.tabs[0].scene.document;
        assert!(document.line_types.get("Temp").is_some(), "undo restored the linetype");
        let names = app.tabs[0].scene.layout_names();
        assert!(names.contains(&"Sheet1".to_owned()) && names.contains(&"Details".to_owned()), "undo restored the layouts: {names:?}");
        assert!(document.entities().any(|e| matches!(e, EntityType::Circle(_))), "undo restored the sheet's circle");
    }

    /// The modify wrappers through Python over the real runner: each drives
    /// the real OCS command and the resulting geometry is checked exactly.
    #[test]
    fn audit_python_modify_wrappers_over_real_ipc() {
        // PEDIT's line-to-polyline conversion runs deep inside a nested command step; a debug
        // build needs more than the 2 MiB a test thread gets (the application's main thread has 8 MiB).
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(modify_wrappers_audit_body)
            .unwrap()
            .join()
            .unwrap();
    }

    fn modify_wrappers_audit_body() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let dir = std::env::temp_dir();
        let run = |host: &mut HostSession<'_>, tag: &str, body: &str| {
            let script = dir.join(format!("ocs_modify_{tag}_{}.py", std::process::id()));
            std::fs::write(&script, format!(concat!(
                "def P(x, y, z=0.0): return {{'x': x, 'y': y, 'z': z}}\n",
                "doc = ocs.active_document\nM = doc.modify\nL = doc.layers\n",
                "refused, failed = [], []\n",
                "def check(tag, fn):\n",
                "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError):\n        refused.append(tag)\n",
                "    else:\n        failed.append('ACCEPTED_' + tag)\n",
                "def step(tag, fn):\n",
                "    try:\n        return fn()\n    except Exception as error:\n",
                "        failed.append(tag + '_' + ''.join(c if c.isalnum() else '_' for c in str(error))[:45])\n",
                "{body}\n",
                "L.create(('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(failed))[:250])\n",
            ), body = body)).unwrap();
            assert!(process.dispatch(host, &format!("PY_RUN {}", script.display()), &mut |_| {}).unwrap());
            let _ = std::fs::remove_file(&script);
        };
        let report = |document: &CadDocument| -> (usize, Vec<String>) {
            let name = document.layers.iter().map(|l| l.name.clone()).filter(|n| n.starts_with("REPORT")).last()
                .expect("script report layer");
            let (refused, failed) = name["REPORT".len()..].split_once('~').unwrap();
            (refused.trim().parse().unwrap(), failed.split_whitespace().map(str::to_owned).collect())
        };

        run(&mut host, "build", r#"
def line(a, b): return doc.create_entity('Line', start=P(*a), end=P(*b))
def circle(c, r): return doc.create_entity('Circle', center=P(*c), radius=r)
a = line((0, 0), (10, 0))
step('offset', lambda: M.offset(a, 2, (5, 5)))
h = line((100, 0), (110, 0)); v = line((105, -5), (105, 5))
step('trim', lambda: M.trim(h, (108, 0)))
h2 = line((200, 0), (209, 0)); w = line((215, -5), (215, 5))
step('extend', lambda: M.extend(h2, (208, 0)))
f1 = line((300, 0), (310, 0)); f2 = line((300, 0), (300, 10))
step('fillet', lambda: M.fillet(f1, f2, 2, (308, 0), (300, 8)))
m = circle((400, 0), 1)
step('move', lambda: M.move([m], (0, 0, 0), (5, 5, 0)))
c = circle((500, 0), 1)
step('copy', lambda: M.copy([c], (0, 0, 0), (0, 10, 0)))
r = line((600, 0), (610, 0))
step('rotate', lambda: M.rotate([r], (600, 0, 0), 90))
sc = line((700, 0), (710, 0))
step('scale', lambda: M.scale([sc], (700, 0, 0), 2))
mi = line((800, 0), (810, 0))
step('mirror', lambda: M.mirror([mi], (0, 5, 0), (1, 5, 0)))
e = line((900, 0), (910, 0))
step('erase', lambda: M.erase([e]))
ca = line((1300, 0), (1310, 0)); cb = line((1300, 0), (1300, 10))
step('chamfer', lambda: M.chamfer(ca, cb, 2, 3, (1308, 0), (1300, 8)))
ar = line((1400, 0), (1401, 0))
step('array_rect', lambda: M.array_rect([ar], 2, 3, 10, 20))
ap = line((1500, 0), (1501, 0))
step('array_polar', lambda: M.array_polar([ap], (1500, 10, 0), 4, 360))
pl = doc.create_entity('LwPolyline', vertices=[{'location': P(1600, 0)}, {'location': P(1610, 0)}, {'location': P(1610, 10)}])
step('explode', lambda: M.explode([pl]))
j1 = line((1700, 0), (1705, 0)); j2 = line((1705, 0), (1710, 0))
step('join', lambda: M.join([j1, j2]))
bk = line((1800, 0), (1810, 0))
step('break', lambda: M.break_entity(bk, (1802, 0, 0), (1805, 0, 0)))
st = line((1900, 0), (1910, 0))
step('stretch', lambda: M.stretch((1908, -2, 0), (1912, 2, 0), (1910, 0, 0), (1915, 0, 0)))
ln = line((2000, 0), (2010, 0))
step('lengthen', lambda: M.lengthen(ln, 5, (2009, 0, 0)))
def lwp(pts, closed=False): return doc.create_entity('LwPolyline', is_closed=closed, vertices=[{'location': P(*p)} for p in pts])
pc = lwp([(2100, 0), (2110, 0), (2110, 10)])
step('polyline_close', lambda: M.polyline_close(pc))
po = lwp([(2150, 0), (2160, 0), (2160, 10)], True)
step('polyline_open', lambda: M.polyline_open(po))
pw = lwp([(2200, 0), (2210, 0), (2210, 10)])
step('polyline_width', lambda: M.polyline_width(pw, 0.5))
pr = lwp([(2300, 0), (2310, 0), (2310, 10)])
step('polyline_reverse', lambda: M.polyline_reverse(pr))
pj = lwp([(2400, 0), (2410, 0)]); pjl = line((2410, 0), (2420, 0))
step('polyline_join', lambda: M.polyline_join(pj, [pjl]))
pline = line((2500, 0), (2510, 0))
step('polyline_from_line', lambda: M.polyline_width(pline, 1))
pb = line((2600, 0), (2601, 0)); pp = line((2600, 50), (2650, 50))
step('array_path', lambda: M.array_path([pb], pp, 5))
a3 = line((2700, 0), (2701, 0))
step('array_3d', lambda: M.array_3d([a3], 2, 2, 2, 10, 20, 30))
step('run_line', lambda: doc.command('LINE 1100,0 1110,10'))
def interactive():
    with doc.start_command('CIRCLE') as c:
        c.point((1200, 0, 0))
        c.text(3)
        if c.waiting:
            raise RuntimeError('circle still waiting')
step('interactive', interactive)
check('quit', lambda: doc.start_command('QUIT'))
check('newdoc', lambda: doc.command('NEW'))
check('py', lambda: doc.command('PY_RUN nothing'))
check('unknown', lambda: doc.start_command('NOSUCHCOMMAND'))
check('incomplete', lambda: doc.command('OFFSET'))
check('input_without_command', lambda: ocs.command_step('point', {'point': [0, 0, 0]}))
open_command = doc.start_command('LINE')
check('second_command', lambda: doc.start_command('CIRCLE'))
check('run_while_active', lambda: doc.command('POINT 1,1'))
open_command.cancel()
check('bad_point', lambda: ocs.command_step('point', {'point': [0, 0, 0]}))
"#);
        let (refused, failed) = report(host.document());
        assert!(failed.is_empty(), "wrapper failures: {failed:?}");
        assert_eq!(refused, 9);

        let lines: Vec<acadrust::entities::Line> = host.document().entities().filter_map(|e| match e { EntityType::Line(l) => Some(l.clone()), _ => None }).collect();
        let circles: Vec<acadrust::entities::Circle> = host.document().entities().filter_map(|e| match e { EntityType::Circle(c) => Some(c.clone()), _ => None }).collect();
        let arcs: Vec<acadrust::entities::Arc> = host.document().entities().filter_map(|e| match e { EntityType::Arc(a) => Some(a.clone()), _ => None }).collect();
        let near = |a: f64, b: f64| (a - b).abs() < 1e-6;
        let has_line = |x1: f64, y1: f64, x2: f64, y2: f64| lines.iter().any(|l| {
            (near(l.start.x, x1) && near(l.start.y, y1) && near(l.end.x, x2) && near(l.end.y, y2))
                || (near(l.start.x, x2) && near(l.start.y, y2) && near(l.end.x, x1) && near(l.end.y, y1))
        });
        let describe = || lines.iter().map(|l| format!("({},{})-({},{})", l.start.x, l.start.y, l.end.x, l.end.y)).collect::<Vec<_>>();
        assert!(has_line(0.0, 0.0, 10.0, 0.0) && has_line(0.0, 2.0, 10.0, 2.0), "offset: {:?}", describe());
        assert!(has_line(100.0, 0.0, 105.0, 0.0), "trim: {:?}", describe());
        assert!(has_line(200.0, 0.0, 215.0, 0.0), "extend: {:?}", describe());
        assert!(arcs.iter().any(|a| near(a.radius, 2.0) && near(a.center.x, 302.0) && near(a.center.y, 2.0)), "fillet arc: {arcs:?}");
        assert!(circles.iter().any(|c| near(c.center.x, 405.0) && near(c.center.y, 5.0)), "move: {circles:?}");
        assert!(!circles.iter().any(|c| near(c.center.x, 400.0)), "move left the original: {circles:?}");
        assert!(circles.iter().any(|c| near(c.center.x, 500.0) && near(c.center.y, 0.0)) && circles.iter().any(|c| near(c.center.x, 500.0) && near(c.center.y, 10.0)), "copy: {circles:?}");
        assert!(has_line(600.0, 0.0, 600.0, 10.0), "rotate: {:?}", describe());
        assert!(has_line(700.0, 0.0, 720.0, 0.0), "scale: {:?}", describe());
        assert!(has_line(800.0, 0.0, 810.0, 0.0) && has_line(800.0, 10.0, 810.0, 10.0), "mirror keeps the source and adds the reflection: {:?}", describe());
        assert!(!has_line(900.0, 0.0, 910.0, 0.0), "erase: {:?}", describe());
        assert!(has_line(1100.0, 0.0, 1110.0, 10.0), "doc.command run: {:?}", describe());
        assert!(has_line(1302.0, 0.0, 1300.0, 3.0), "chamfer: {:?}", describe());
        for (x, y) in [(1400.0, 0.0), (1420.0, 0.0), (1440.0, 0.0), (1400.0, 10.0), (1420.0, 10.0), (1440.0, 10.0)] {
            assert!(has_line(x, y, x + 1.0, y), "rectangular array member at ({x}, {y}): {:?}", describe());
        }
        assert!(has_line(1500.0, 0.0, 1501.0, 0.0) && has_line(1500.0, 20.0, 1499.0, 20.0), "polar array (start and half turn): {:?}", describe());
        assert_eq!(lines.iter().filter(|l| l.start.x > 1450.0 && l.start.x < 1520.0 || l.end.x > 1450.0 && l.end.x < 1520.0).count(), 4, "polar array has four members");
        assert!(has_line(1600.0, 0.0, 1610.0, 0.0) && has_line(1610.0, 0.0, 1610.0, 10.0), "explode: {:?}", describe());
        assert!(!host.document().entities().any(|e| matches!(e, EntityType::LwPolyline(p) if p.vertices.first().is_some_and(|v| near(v.location.x, 1600.0)))), "explode removed the polyline");
        assert!(has_line(1700.0, 0.0, 1710.0, 0.0) && !has_line(1700.0, 0.0, 1705.0, 0.0), "join: {:?}", describe());
        assert!(has_line(1800.0, 0.0, 1802.0, 0.0) && has_line(1805.0, 0.0, 1810.0, 0.0) && !has_line(1800.0, 0.0, 1810.0, 0.0), "break: {:?}", describe());
        assert!(has_line(1900.0, 0.0, 1915.0, 0.0), "stretch: {:?}", describe());
        assert!(has_line(2000.0, 0.0, 2015.0, 0.0), "lengthen: {:?}", describe());
        let polylines: Vec<acadrust::entities::LwPolyline> = host.document().entities().filter_map(|e| match e { EntityType::LwPolyline(p) => Some(p.clone()), _ => None }).collect();
        let at = |x: f64| polylines.iter().find(|p| p.vertices.iter().any(|v| near(v.location.x, x)));
        let pc = at(2100.0).expect("closed polyline");
        assert!(pc.is_closed, "polyline_close");
        assert!(!at(2150.0).expect("opened polyline").is_closed, "polyline_open");
        let pw = at(2200.0).expect("widened polyline");
        assert!(pw.constant_width == 0.5 || pw.vertices.iter().all(|v| v.start_width == 0.5 && v.end_width == 0.5), "polyline_width: {pw:?}");
        let pr = at(2300.0).expect("reversed polyline");
        assert_eq!((pr.vertices[0].location.x, pr.vertices[0].location.y), (2310.0, 10.0), "polyline_reverse: {pr:?}");
        let pj = polylines.iter().find(|p| p.vertices.iter().any(|v| near(v.location.x, 2420.0))).expect("joined polyline");
        assert_eq!(pj.vertices.len(), 3, "polyline_join: {pj:?}");
        assert!(!has_line(2410.0, 0.0, 2420.0, 0.0), "polyline_join consumed the line: {:?}", describe());
        let from_line = at(2500.0).expect("a line turned into a polyline");
        assert!(from_line.constant_width == 1.0 || from_line.vertices.iter().all(|v| v.start_width == 1.0), "polyline_from_line: {from_line:?}");
        assert!(!has_line(2500.0, 0.0, 2510.0, 0.0), "the line became the polyline: {:?}", describe());
        let path_copies: Vec<(f64, f64)> = lines.iter().filter(|l| l.start.x > 2599.0 && l.start.x < 2660.0).map(|l| (l.start.x, l.start.y)).collect();
        for x in [2600.0, 2612.5, 2625.0, 2637.5, 2650.0] {
            assert!(path_copies.iter().any(|p| near(p.0, x) && near(p.1, 0.0)), "array_path item at x={x} (five evenly spaced along the 50-long path): {path_copies:?}");
        }
        let three_d: Vec<(f64, f64, f64)> = lines.iter().filter(|l| l.start.x > 2699.0 && l.start.x < 2725.0).map(|l| (l.start.x, l.start.y, l.start.z)).collect();
        for x in [2700.0, 2720.0] { for y in [0.0, 10.0] { for z in [0.0, 30.0] {
            assert!(three_d.iter().any(|p| near(p.0, x) && near(p.1, y) && near(p.2, z)), "array_3d member ({x},{y},{z}): {three_d:?}");
        } } }
        assert!(circles.iter().any(|c| near(c.center.x, 1200.0) && near(c.radius, 3.0)), "start_command session: {circles:?}");
        assert!(host.app.tabs[0].active_cmd.is_none(), "no command is left running");

        // U: a wrapper's change is one undoable step (the report layer is the other).
        run(&mut host, "undo", r#"
u = doc.create_entity('Circle', center=P(1000, 0), radius=1)
step('undo_move', lambda: M.move([u], (0, 0, 0), (7, 0, 0)))
"#);
        assert!(report(host.document()).1.is_empty());
        let moved = |document: &CadDocument, x: f64| document.entities().any(|e| matches!(e, EntityType::Circle(c) if near(c.center.x, x) && near(c.center.y, 0.0)));
        assert!(moved(host.document(), 1007.0));
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(2);
        assert!(moved(&app.tabs[0].scene.document, 1000.0), "undo put the circle back");
        assert!(!moved(&app.tabs[0].scene.document, 1007.0));
    }

    /// A scripted dict is checked all the way down: a nested record (a polyline
    /// vertex, a mesh vertex) may not carry an unknown key or omit the field that
    /// defines its geometry, so a typo never turns into a silent zero coordinate.
    #[test]
    fn audit_python_nested_input_is_validated_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let script = std::env::temp_dir().join(format!("ocs_nested_{}.py", std::process::id()));
        std::fs::write(&script, concat!(
            "def P(x, y, z=0.0): return {'x': x, 'y': y, 'z': z}\n",
            "doc = ocs.active_document\nL = doc.layers\n",
            "refused, problems = [], []\n",
            "def check(tag, fn, expect):\n",
            "    try:\n        fn()\n    except (RuntimeError, TypeError, ValueError) as error:\n",
            "        if expect in str(error):\n            refused.append(tag)\n",
            "        else:\n            problems.append('WRONGMESSAGE_' + tag)\n",
            "    else:\n        problems.append('ACCEPTED_' + tag)\n",
            "def lw(vertices): return lambda: doc.create_entity('LwPolyline', vertices=vertices)\n",
            "ok = {'location': P(0, 0)}\n",
            "check('lw_xy_keys', lw([ok, {'x': 5, 'y': 5}]), 'no property')\n",
            "check('lw_empty_vertex', lw([ok, {}]), 'needs location')\n",
            "check('lw_null_location', lw([ok, {'location': None}]), 'needs location')\n",
            "check('lw_typo', lw([ok, {'location': P(1, 1), 'bulgee': 0.5}]), 'no property')\n",
            "check('lw_message_lists_keys', lw([ok, {'lokation': P(1, 1)}]), 'location')\n",
            "check('poly2d_missing_location', lambda: doc.create_entity('Polyline2D', vertices=[{'flags': 0}, {'location': P(1, 1)}]), 'needs location')\n",
            "check('poly2d_typo', lambda: doc.create_entity('Polyline2D', vertices=[{'location': P(0, 0)}, {'loc': P(1, 1)}]), 'no property')\n",
            "check('mesh_missing_location', lambda: doc.create_entity('PolygonMesh', vertices=[{}]), 'needs location')\n",
            "good = doc.create_entity('LwPolyline', vertices=[ok, {'location': P(10, 0), 'bulge': 0.5}, {'location': P(10, 10)}])\n",
            "good2 = doc.create_entity('Polyline2D', vertices=[{'location': P(0, 0)}, {'location': P(5, 5)}])\n",
            "L.create('REPORT ' + str(len(refused)) + ' ~ ' + ' '.join(problems))\n",
        )).unwrap();
        assert!(process.dispatch(&mut host, &format!("PY_RUN {}", script.display()), &mut |_| {}).unwrap());
        let _ = std::fs::remove_file(&script);
        let name = host.document().layers.iter().map(|l| l.name.clone()).filter(|n| n.starts_with("REPORT")).last().expect("script report layer");
        let (refused, problems) = name["REPORT".len()..].split_once('~').unwrap();
        assert_eq!(problems.trim(), "", "problems: {problems}");
        assert_eq!(refused.trim(), "8");
        // Nothing invalid reached the drawing; the two valid polylines did.
        let document = host.document();
        let lw: Vec<_> = document.entities().filter_map(|e| match e { EntityType::LwPolyline(p) => Some(p.clone()), _ => None }).collect();
        assert_eq!(lw.len(), 1, "only the valid LwPolyline was created");
        assert_eq!(lw[0].vertices.len(), 3);
        assert_eq!(lw[0].vertices[1].bulge, 0.5);
        assert_eq!(document.entities().filter(|e| matches!(e, EntityType::Polyline2D(_))).count(), 1);
        assert!(!document.entities().any(|e| matches!(e, EntityType::PolygonMesh(_))));
        drop(process);
    }

    /// The user's SCRIPTCOMMANDS setting gates the command runner, and a script cannot change it.
    #[test]
    fn script_commands_setting_gates_the_runner() {
        use ocs_plugin_api::host::CommandRequest as R;
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        assert!(app.script_commands, "scripts may run commands by default");
        let run = |app: &mut OpenCADStudio, line: &str| HostSession::new(app, 0).run_command(R::Run { line: line.into() });
        assert!(run(&mut app, "LINE 0,0 10,0").is_ok());

        // A script cannot touch the setting, on or off.
        let refused = run(&mut app, "SCRIPTCOMMANDS 0").unwrap_err();
        assert!(refused.contains("cannot be run from a script"), "{refused}");
        assert!(app.script_commands);

        // The user turns it off at the command line.
        let reply = app.automation_op(r#"{"op":"run","cmd":"SCRIPTCOMMANDS 0"}"#);
        assert_eq!(reply["ok"], true, "{reply}");
        assert!(!app.script_commands);
        let refused = run(&mut app, "LINE 0,0 20,0").unwrap_err();
        assert!(refused.contains("turned off"), "{refused}");
        let refused = HostSession::new(&mut app, 0).run_command(R::Start { name: "LINE".into() }).unwrap_err();
        assert!(refused.contains("turned off"), "{refused}");
        assert_eq!(app.tabs[0].scene.document.entities().count(), 1, "nothing ran while it was off");
        assert!(HostSession::new(&mut app, 0).run_command(R::Cancel).is_ok(), "Cancel is still allowed while it is off");

        // And back on.
        app.automation_op(r#"{"op":"run","cmd":"SCRIPTCOMMANDS 1"}"#);
        assert!(app.script_commands);
        assert!(run(&mut app, "LINE 0,0 20,0").is_ok());
        assert_eq!(app.tabs[0].scene.document.entities().count(), 2);
    }

    /// Exploration harness (not a gate): prints how the real commands answer
    /// each step so the wrappers are written against observed prompts.
    #[test]
    #[ignore]
    #[allow(unused_must_use)]
    fn spike_command_runner_prompts() {
        use ocs_plugin_api::host::CommandRequest as R;
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let show = |label: &str, r: &Result<ocs_plugin_api::host::CommandOutcome, String>| match r {
            Ok(o) => eprintln!("SPIKE {label}: {} prompt={:?} accepts={:?} opts={:?} added={} err={:?}",
                o.status, o.prompt, o.accepts, o.options, o.added, o.error),
            Err(e) => eprintln!("SPIKE {label}: ERR {e}"),
        };
        let run = |host: &mut HostSession<'_>, label: &str, r: R| { let out = host.run_command(r); show(label, &out); out };
        let mk = |host: &mut HostSession<'_>, lines: &[&str]| -> Vec<Handle> {
            for l in lines { host.run_command(R::Run { line: (*l).into() }).unwrap(); }
            host.document().entities().map(|e| e.common().handle).collect()
        };
        let hs = mk(&mut host, &["PLINE 2400,0 2410,0", "LINE 2410,0 2420,0"]);
        run(&mut host, "PEDIT start", R::Start { name: "PEDIT".into() });
        run(&mut host, "pick", R::Entity { handle: hs[0], point: [2400.0, 0.0, 0.0] });
        run(&mut host, "J", R::Token { text: "J".into() });
        host.app.tabs[0].scene.replace_selection_exact(&[hs[1]]);
        run(&mut host, "selection", R::Selection);
        run(&mut host, "enter", R::Enter);
        run(&mut host, "X", R::Token { text: "X".into() });
        eprintln!("SPIKE final entity count {}", host.document().entities().count());
    }





    #[test]
    fn audit_python_raster_image_definition_linkage_over_real_ipc() {
        use acadrust::objects::ObjectType;
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let dir = std::env::temp_dir();
        let (a, b) = (dir.join("ocs_link_a.png"), dir.join("ocs_link_b.png"));
        image::RgbaImage::from_pixel(8, 4, image::Rgba([9, 9, 9, 255])).save(&a).unwrap();
        image::RgbaImage::from_pixel(6, 6, image::Rgba([90, 9, 9, 255])).save(&b).unwrap();
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let process = ocs_plugin_api::process::PluginProcess::spawn(
            std::path::Path::new(&plugin_path), &mut host, crate::plugin::v4_support::notification_handler(),
        ).unwrap();
        let script = dir.join(format!("ocs_link_{}.py", std::process::id()));
        std::fs::write(&script, format!(concat!(
            "doc = ocs.active_document\n",
            "doc.create_entity('RasterImage', file_path={a:?}, insertion_point=(0, 0, 0))\n",
            "doc.create_entity('RasterImage', file_path={a:?}, insertion_point=(20, 0, 0))\n",
            "doc.create_entity('RasterImage', file_path={b:?}, insertion_point=(40, 0, 0))\n",
        ), a = a.display().to_string(), b = b.display().to_string())).unwrap();
        assert!(process.dispatch(&mut host, &format!("PY_RUN {}", script.display()), &mut |_| {}).unwrap());
        let _ = std::fs::remove_file(&script);

        // The structure: one definition per file, registered and owned by ACAD_IMAGE_DICT.
        let structure = |document: &CadDocument| {
            let root = document.header.named_objects_dict_handle;
            let Some(ObjectType::Dictionary(root_dict)) = document.objects.get(&root) else { panic!("no root dictionary") };
            let dict_handle = root_dict.get("ACAD_IMAGE_DICT");
            let entries = dict_handle.and_then(|h| match document.objects.get(&h) {
                Some(ObjectType::Dictionary(d)) => Some(d.entries.clone()), _ => None }).unwrap_or_default();
            let defs: Vec<_> = document.objects.iter().filter_map(|(h, o)| match o {
                ObjectType::ImageDefinition(d) => Some((*h, d.owner, d.file_name.clone())), _ => None }).collect();
            let mut images: Vec<_> = document.entities().filter_map(|e| match e {
                EntityType::RasterImage(i) => Some((i.common.handle, i.definition_handle, i.file_path.clone())), _ => None }).collect();
            images.sort_by_key(|(h, _, _)| h.value());
            (dict_handle, entries, defs, images)
        };
        let check = |label: &str, document: &CadDocument| {
            let (dict, entries, defs, images) = structure(document);
            let dict = dict.unwrap_or_else(|| panic!("{label}: no ACAD_IMAGE_DICT"));
            assert_eq!(defs.len(), 2, "{label}: one definition per file");
            let mut keys: Vec<_> = entries.iter().map(|(k, _)| k.as_str()).collect();
            keys.sort();
            assert_eq!(keys, vec!["ocs_link_a", "ocs_link_b"], "{label}");
            for (key, handle) in &entries {
                let (_, owner, file) = defs.iter().find(|(h, _, _)| h == handle).unwrap_or_else(|| panic!("{label}: dangling entry {key}"));
                assert_eq!(*owner, dict, "{label}: {key} is owned by ACAD_IMAGE_DICT");
                assert!(file.contains(key.as_str()), "{label}: {key} -> {file}");
            }
            assert_eq!(images.len(), 3, "{label}");
            assert_eq!(images[0].1, images[1].1, "{label}: images of one file share a definition");
            assert_ne!(images[0].1, images[2].1, "{label}");
            for (_, definition, path) in &images {
                assert!(defs.iter().any(|(h, _, _)| Some(*h) == *definition), "{label}: definition resolves");
                assert!(path.contains("ocs_link_"), "{label}: file path kept: {path}");
            }
        };
        check("live", host.document());
        let dwg = crate::io::load_bytes("link.dwg", acadrust::DwgWriter::write_to_vec(host.document()).unwrap()).unwrap();
        let dxf = crate::io::load_bytes("link.dxf", acadrust::DxfWriter::new(host.document()).write_to_vec().unwrap()).unwrap();
        check("DWG", &dwg);
        check("DXF", &dxf);

        // U: undoing the last image leaves no dangling dictionary entry.
        drop(process);
        drop(host);
        app.finish_pending_history(0);
        app.undo_steps(1);
        let (_, entries, defs, images) = structure(&app.tabs[0].scene.document);
        assert!(images.len() < 3);
        for (key, handle) in &entries {
            assert!(defs.iter().any(|(h, _, _)| h == handle), "undo left a dangling entry {key}");
        }
        for file in [&a, &b] {
            let _ = std::fs::remove_file(file);
        }
    }

    #[test]
    fn staged_python_point_pick_and_cancel_over_real_ipc() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let process = {
            let mut host = HostSession::new(&mut app, 0);
            std::sync::Arc::new(
                ocs_plugin_api::process::PluginProcess::spawn(
                    std::path::Path::new(&plugin_path),
                    &mut host,
                crate::plugin::v4_support::notification_handler(),
                )
                .expect("spawn staged Python plugin"),
            )
        };
        let request = |app: &mut OpenCADStudio, prompt: &str, entity: bool| {
            let mut started = None;
            {
                let mut host = HostSession::new(app, 0);
                let method = if entity {
                    "request_entity"
                } else {
                    "request_point"
                };
                let cmd = format!("PY_EVAL ocs.active_document.{method}('{prompt}')");
                assert!(process
                    .dispatch(&mut host, &cmd, &mut |id| started = Some(id))
                    .unwrap());
            }
            let output = app.command_line.history.last().unwrap().text.clone();
            let token = output
                .split_whitespace()
                .find_map(|part| part.parse::<u64>().ok())
                .unwrap_or_else(|| panic!("Python pick token missing from {output:?}"));
            let id = started.expect("Python request started an interactive command");
            app.set_active_command(
                0,
                Box::new(PluginProcessInteractiveAdapter::new(
                    std::sync::Arc::clone(&process),
                    id,
                )),
            );
            token
        };

        let picked_token = request(&mut app, "Pick a point", false);
        let result = app.tabs[0]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_point(glam::DVec3::new(3.0, 4.0, 0.0));
        let _ = app.apply_cmd_result(result);
        assert!(app.tabs[0].active_cmd.is_none());
        {
            let mut host = HostSession::new(&mut app, 0);
            assert!(process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({picked_token})"),
                    &mut |_| {}
                )
                .unwrap());
        }
        assert!(app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("point"));

        let target = {
            let mut host = HostSession::new(&mut app, 0);
            host.add_entity(EntityType::Line(Line::from_points(
                acadrust::types::Vector3::new(6.0, 7.0, 0.0),
                acadrust::types::Vector3::new(9.0, 7.0, 0.0),
            )))
        };
        let entity_token = request(&mut app, "Pick an entity", true);
        assert!(app.tabs[0].active_cmd.as_ref().unwrap().needs_entity_pick());
        let result = app.tabs[0]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(target, glam::DVec3::new(6.0, 7.0, 0.0));
        let _ = app.apply_cmd_result(result);
        {
            let mut host = HostSession::new(&mut app, 0);
            assert!(process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({entity_token})"),
                    &mut |_| {}
                )
                .unwrap());
        }
        assert!(app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("entity"));
        assert!(app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains(&target.value().to_string()));

        let cancelled_token = request(&mut app, "Cancel a point", false);
        let result = app.tabs[0].active_cmd.as_mut().unwrap().on_enter();
        let _ = app.apply_cmd_result(result);
        {
            let mut host = HostSession::new(&mut app, 0);
            assert!(process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({cancelled_token})"),
                    &mut |_| {}
                )
                .unwrap());
        }
        assert!(app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("cancelled"));
    }

    #[test]
    fn staged_python_tabs_isolate_tokens_and_notifications() {
        let Some(plugin_path) = std::env::var_os("OCS_TEST_PYTHON_PLUGIN") else {
            return;
        };
        let runner_path = std::env::var_os("OCS_PLUGIN_RUNNER_EXE")
            .expect("set OCS_PLUGIN_RUNNER_EXE to the built OpenCADStudio executable");
        assert!(std::path::Path::new(&runner_path).is_file());
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        app.tabs
            .push(crate::app::document::DocumentTab::new_drawing(2));
        app.tabs[1].is_start = false;
        let tab0 = app.tabs[0].id;
        let tab1 = app.tabs[1].id;
        assert_ne!(tab0, tab1);
        let process = {
            let mut host = HostSession::new(&mut app, 0);
            std::sync::Arc::new(
                ocs_plugin_api::process::PluginProcess::spawn(
                    std::path::Path::new(&plugin_path),
                    &mut host,
                crate::plugin::v4_support::notification_handler(),
                )
                .expect("spawn staged Python plugin"),
            )
        };
        let mut started = None;
        {
            let mut host = HostSession::new(&mut app, 0);
            assert!(process
                .dispatch(
                    &mut host,
                    "PY_EVAL ocs.active_document.request_point('Tab zero')",
                    &mut |id| started = Some(id)
                )
                .unwrap());
        }
        let token = app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .split_whitespace()
            .find_map(|part| part.parse::<u64>().ok())
            .expect("pick token");
        app.set_active_command(
            0,
            Box::new(PluginProcessInteractiveAdapter::new(
                std::sync::Arc::clone(&process),
                started.unwrap(),
            )),
        );
        {
            let mut host = HostSession::new(&mut app, 1);
            assert!(process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                    &mut |_| {}
                )
                .unwrap());
        }
        assert!(
            app.command_line
                .history
                .last()
                .unwrap()
                .text
                .contains("None"),
            "other tab must not consume the pending token"
        );
        let result = app.tabs[0]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_point(glam::DVec3::new(2.0, 3.0, 0.0));
        let _ = app.apply_cmd_result(result);
        {
            let mut host = HostSession::new(&mut app, 1);
            assert!(process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                    &mut |_| {}
                )
                .unwrap());
        }
        assert!(app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("None"));
        {
            let mut host = HostSession::new(&mut app, 0);
            assert!(process
                .dispatch(
                    &mut host,
                    &format!("PY_EVAL ocs.active_document.poll_input({token})"),
                    &mut |_| {}
                )
                .unwrap());
        }
        assert!(app
            .command_line
            .history
            .last()
            .unwrap()
            .text
            .contains("point"));

        use ocs_plugin_api::host::HostNotification;
        process
            .notify_plugin(
                None,
                HostNotification::DrawingChanged {
                    tab_id: tab0,
                    epoch: 101,
                },
            )
            .unwrap();
        process
            .notify_plugin(
                None,
                HostNotification::DrawingChanged {
                    tab_id: tab1,
                    epoch: 202,
                },
            )
            .unwrap();
        // Notifications are best-effort and consumed by the runner's reader
        // thread before its next Dispatch. Give that thread a bounded handoff.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let poll_events_until = |app: &mut OpenCADStudio, tab: usize, epoch: &str| {
            let mut output = String::new();
            for _ in 0..10 {
                {
                    let mut host = HostSession::new(app, tab);
                    assert!(process
                        .dispatch(
                            &mut host,
                            "PY_EVAL ocs.active_document.poll_events()",
                            &mut |_| {}
                        )
                        .unwrap());
                }
                output = app.command_line.history.last().unwrap().text.clone();
                if output.contains(epoch) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            output
        };
        let other_events = poll_events_until(&mut app, 1, "202");
        assert!(
            other_events.contains("202") && !other_events.contains("101"),
            "{other_events}"
        );
        let first_events = poll_events_until(&mut app, 0, "101");
        assert!(
            first_events.contains("101") && !first_events.contains("202"),
            "{first_events}"
        );

        for epoch in 0..257 {
            process
                .notify_plugin(
                    None,
                    HostNotification::DrawingChanged {
                        tab_id: tab0,
                        epoch,
                    },
                )
                .unwrap();
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        {
            let mut host = HostSession::new(&mut app, 0);
            assert!(process
                .dispatch(
                    &mut host,
                    "PY_EVAL ocs.active_document.poll_events()",
                    &mut |_| {}
                )
                .unwrap());
        }
        let overflow = &app.command_line.history.last().unwrap().text;
        assert!(
            overflow.contains("overflow") && overflow.contains("dropped"),
            "{overflow}"
        );

        process
            .notify_plugin(
                None,
                HostNotification::DrawingChanged {
                    tab_id: tab0,
                    epoch: 303,
                },
            )
            .unwrap();
        process
            .notify_plugin(None, HostNotification::DocumentTabClosed { tab_id: tab0 })
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        {
            let mut host = HostSession::new(&mut app, 0);
            assert!(process
                .dispatch(
                    &mut host,
                    "PY_EVAL ocs.active_document.poll_events()",
                    &mut |_| {}
                )
                .unwrap());
        }
        assert!(
            app.command_line.history.last().unwrap().text.contains("[]"),
            "closing a tab must discard its queued events"
        );
    }

    #[test]
    fn unmapped_canvas_kind_layer_change_undoes() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let handle;
        {
            let mut host = HostSession::new(&mut app, 0);
            handle = host.add_entity(EntityType::MLine(acadrust::entities::MLine::default()));
            let original = host.document().get_entity(handle).unwrap();
            let changed =
                ocs_plugin_api::entity_coverage::patch_canvas_layer(original, "HATCHES").unwrap();
            host.update_entities_transaction("Move hatch layer", vec![changed])
                .unwrap();
            assert_eq!(
                host.document().get_entity(handle).unwrap().common().layer,
                "HATCHES"
            );
        }
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
        app.undo_steps(1);
        assert_eq!(
            app.tabs[0]
                .scene
                .document
                .get_entity(handle)
                .unwrap()
                .common()
                .layer,
            "0"
        );
    }

    #[test]
    fn insert_transform_transaction_undoes_without_changing_block_identity() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let handle;
        {
            let mut host = HostSession::new(&mut app, 0);
            let insert = acadrust::entities::Insert::new(
                "DOOR",
                acadrust::types::Vector3::new(1.0, 2.0, 0.0),
            );
            handle = host.add_entity(EntityType::Insert(insert));
            let mut changed = host.document().get_entity(handle).unwrap().clone();
            if let EntityType::Insert(insert) = &mut changed {
                insert.insert_point.x = 4.0;
                insert.set_x_scale(2.0);
            }
            host.update_entities_transaction("Move block", vec![changed])
                .unwrap();
            assert!(
                matches!(host.document().get_entity(handle), Some(EntityType::Insert(insert))
                if insert.insert_point.x == 4.0 && insert.x_scale() == 2.0 && insert.block_name == "DOOR")
            );
        }
        app.finish_pending_history(0);
        assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
        app.undo_steps(1);
        assert!(
            matches!(app.tabs[0].scene.document.get_entity(handle), Some(EntityType::Insert(insert))
            if insert.insert_point.x == 1.0 && insert.x_scale() == 1.0 && insert.block_name == "DOOR")
        );
    }

    #[test]
    fn system_variables_change_without_command_reentry() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        assert_eq!(
            host.system_variable("clayer"),
            Some(HostSettingValue::Text("0".into()))
        );
        assert_eq!(
            host.system_variable("SNAPANG"),
            Some(HostSettingValue::Number(0.0))
        );
        assert!(host
            .set_system_variable("CLAYER", HostSettingValue::Text("Missing".into()))
            .is_err());
        assert_eq!(
            host.system_variable("CLAYER"),
            Some(HostSettingValue::Text("0".into()))
        );

        let mut point = Point::new();
        point.common.layer = "Annotations".into();
        host.add_entity(EntityType::Point(point));
        assert_eq!(
            host.set_system_variable("clayer", HostSettingValue::Text("Annotations".into())),
            Ok(HostSettingValue::Text("Annotations".into()))
        );
        assert_eq!(
            host.system_variable("CLAYER"),
            Some(HostSettingValue::Text("Annotations".into()))
        );
        assert_eq!(host.app.tabs[0].active_layer, "Annotations");
        assert_eq!(host.app.tabs[0].layers.current_layer, "Annotations");
        assert_eq!(host.app.ribbon.active_layer, "Annotations");
        assert!(host.app.tabs[0].dirty);

        assert_eq!(
            host.set_system_variable("snapang", HostSettingValue::Number(450.0)),
            Ok(HostSettingValue::Number(90.0))
        );
        assert_eq!(
            host.system_variable("SNAPANG"),
            Some(HostSettingValue::Number(90.0))
        );
        assert!(host
            .set_system_variable("SNAPANG", HostSettingValue::Number(f64::INFINITY))
            .is_err());
        assert!(host
            .set_system_variable("SNAPANG", HostSettingValue::Number(f64::MAX))
            .is_err());
        assert_eq!(
            host.system_variable("SNAPANG"),
            Some(HostSettingValue::Number(90.0))
        );
    }

    #[test]
    fn xdata_record_round_trips_and_registers_appid() {
        let mut app = OpenCADStudio::new_for_test();
        let mut host = HostSession::new(&mut app, 0);
        let h = host.add_entity(EntityType::Point(Point::new()));

        let mut rec = ExtendedDataRecord::new("DEMO_SURVEY");
        rec.add_value(XDataValue::String("PNT-1".to_string()));
        rec.add_value(XDataValue::Integer32(42));
        assert!(host.write_record(h, rec));

        let got = host.read_record(h, "DEMO_SURVEY").expect("record missing");
        assert_eq!(got.values.len(), 2);
        // APPID registered so the XDATA survives a DWG/DXF round-trip.
        assert!(host.document().app_ids.contains("DEMO_SURVEY"));

        // A second write replaces rather than duplicates the record.
        let mut rec2 = ExtendedDataRecord::new("DEMO_SURVEY");
        rec2.add_value(XDataValue::String("PNT-2".to_string()));
        assert!(host.write_record(h, rec2));
        let got = host.read_record(h, "DEMO_SURVEY").unwrap();
        assert_eq!(got.values.len(), 1);

        // Removal reports whether anything was dropped.
        assert!(host.remove_record(h, "DEMO_SURVEY"));
        assert!(host.read_record(h, "DEMO_SURVEY").is_none());
        assert!(!host.remove_record(h, "DEMO_SURVEY"));
    }

    #[test]
    fn plugin_state_round_trips_through_hostapi_trait() {
        use ocs_plugin_api::host::{self, HostApi};
        let mut app = OpenCADStudio::new_for_test();
        let mut session = HostSession::new(&mut app, 0);
        let host: &mut dyn HostApi = &mut session;

        // Absent before first use.
        assert!(host::plugin_state::<u32>(&*host, "opencad.demo").is_none());
        // Insert via ensure, then mutate.
        *host::ensure_plugin_state(host, "opencad.demo", || 7u32) += 1;
        assert_eq!(
            *host::plugin_state::<u32>(&*host, "opencad.demo").unwrap(),
            8
        );
        *host::plugin_state_mut::<u32>(host, "opencad.demo").unwrap() = 100;
        assert_eq!(
            *host::plugin_state::<u32>(&*host, "opencad.demo").unwrap(),
            100
        );
    }

    #[test]
    fn add_entities_batch_assigns_handles_and_publishes_once() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);

        let pts: Vec<EntityType> = (0..5)
            .map(|i| {
                EntityType::Point(Point::at(acadrust::types::Vector3::new(
                    i as f64, i as f64, 0.0,
                )))
            })
            .collect();
        let handles = host.add_entities(pts);

        assert_eq!(handles.len(), 5);
        assert!(handles.iter().all(|h| !h.is_null()));
        // Each handle is unique.
        let mut set = std::collections::HashSet::new();
        for h in &handles {
            assert!(set.insert(h.value()));
        }
        // All entities are in the document.
        for h in &handles {
            assert!(host.document().get_entity(*h).is_some());
        }
    }

    #[test]
    fn update_entity_replaces_in_place_preserving_handle() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let h = host.add_entity(EntityType::Point(Point::at(acadrust::types::Vector3::new(
            1.0, 1.0, 0.0,
        ))));
        let epoch_before = host.app.tabs[0].scene.geometry_epoch;

        // Edit a snapshot copy (as a plugin would) and commit it.
        let mut edited = host.document().get_entity(h).unwrap().clone();
        edited.common_mut().layer = "PLUGIN_EDIT".to_string();
        assert!(host.update_entity(edited));

        // Same handle, edit applied, geometry re-tessellated.
        let got = host
            .document()
            .get_entity(h)
            .expect("entity kept its handle");
        assert_eq!(got.common().layer, "PLUGIN_EDIT");
        assert_ne!(
            host.app.tabs[0].scene.geometry_epoch, epoch_before,
            "update should bump geometry"
        );

        // Updating an unknown handle fails and changes nothing.
        let mut ghost = Point::new();
        ghost.common.handle = Handle::new(999_999);
        assert!(!host.update_entity(EntityType::Point(ghost)));
    }

    #[test]
    fn add_and_update_auto_register_novel_layers_with_real_handles() {
        // A plugin adds/edits an entity naming a layer no LAYER command ever
        // created. The layer must gain a real table entry (non-null handle) so
        // it survives a DWG save instead of collapsing to layer 0 (#252, #67).
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);

        assert!(!host.document().layers.contains("PLUGIN-LAYER"));
        let mut pt = Point::at(acadrust::types::Vector3::new(3.0, 3.0, 0.0));
        pt.common.layer = "PLUGIN-LAYER".to_string();
        host.add_entity(EntityType::Point(pt));

        let layer = host
            .document()
            .layers
            .get("PLUGIN-LAYER")
            .expect("novel layer auto-registered on add_entity");
        assert!(
            !layer.handle.is_null(),
            "auto-registered layer must carry a real handle (#67)"
        );

        // Adding again on the same (now-existing) layer must not duplicate or
        // error — the always-present default "0" is likewise never re-created.
        let count_before = host.document().layers.len();
        host.add_entity(EntityType::Point(Point::new())); // default layer "0"
        let mut pt2 = Point::new();
        pt2.common.layer = "PLUGIN-LAYER".to_string();
        host.add_entity(EntityType::Point(pt2));
        assert_eq!(host.document().layers.len(), count_before);

        // Retargeting an entity to a novel layer via update_entity registers it.
        let h = host.add_entity(EntityType::Point(Point::new()));
        let mut edited = host.document().get_entity(h).unwrap().clone();
        edited.common_mut().layer = "EDIT-LAYER".to_string();
        assert!(host.update_entity(edited));
        assert!(host.document().layers.contains("EDIT-LAYER"));
    }

    #[test]
    fn remove_entity_deletes_and_clears_caches() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let h = host.add_entity(EntityType::Point(Point::at(acadrust::types::Vector3::new(
            2.0, 2.0, 0.0,
        ))));
        assert!(host.document().get_entity(h).is_some());

        assert!(host.remove_entity(h));
        assert!(host.document().get_entity(h).is_none());
        assert!(!host.app.tabs[0].scene.hatches.contains_key(&h));
        assert!(!host.app.tabs[0].scene.meshes.contains_key(&h));

        // Removing an already-gone handle reports false.
        assert!(!host.remove_entity(h));
    }

    /// A plugin command: second point commits a Point and ends.
    struct PlacePoint {
        got_first: bool,
    }
    impl ocs_plugin_api::host::InteractiveCommand for PlacePoint {
        fn prompt(&self) -> String {
            crate::t!("Pick a point").into_owned()
        }
        fn on_point(&mut self, pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
            use ocs_plugin_api::host::CommandStep;
            if self.got_first {
                let p = acadrust::entities::Point::at(acadrust::types::Vector3::new(
                    pt[0], pt[1], pt[2],
                ));
                CommandStep::CommitAndEnd(acadrust::EntityType::Point(p))
            } else {
                self.got_first = true;
                CommandStep::NeedPoint
            }
        }
    }

    #[test]
    fn plugin_interactive_command_drives_host_flow() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        {
            let mut host = HostSession::new(&mut app, 0);
            host.start_interactive(Box::new(PlacePoint { got_first: false }));
        }
        assert!(app.tabs[0].active_cmd.is_some());
        for pt in [
            glam::DVec3::new(0.0, 0.0, 0.0),
            glam::DVec3::new(5.0, 5.0, 0.0),
        ] {
            let r = app.tabs[0].active_cmd.as_mut().unwrap().on_point(pt);
            let _ = app.apply_cmd_result(r);
        }
        assert_eq!(app.tabs[0].scene.document.entities().count(), 1);
        assert!(
            app.tabs[0].active_cmd.is_none(),
            "command should have ended"
        );
    }

    /// A plugin command that picks an existing object, then marks it.
    struct PickThenMark;
    impl ocs_plugin_api::host::InteractiveCommand for PickThenMark {
        fn prompt(&self) -> String {
            crate::t!("Pick an object").into_owned()
        }
        fn on_point(&mut self, _pt: [f64; 3]) -> ocs_plugin_api::host::CommandStep {
            ocs_plugin_api::host::CommandStep::Cancel
        }
        fn needs_object_pick(&self) -> bool {
            true
        }
        fn on_object_pick(
            &mut self,
            _handle: acadrust::Handle,
            pt: [f64; 3],
        ) -> ocs_plugin_api::host::CommandStep {
            let p =
                acadrust::entities::Point::at(acadrust::types::Vector3::new(pt[0], pt[1], pt[2]));
            ocs_plugin_api::host::CommandStep::CommitAndEnd(acadrust::EntityType::Point(p))
        }
    }

    #[test]
    fn plugin_object_pick_routes_to_command() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let target = {
            let mut host = HostSession::new(&mut app, 0);
            let h = host.add_entity(acadrust::EntityType::Point(acadrust::entities::Point::at(
                acadrust::types::Vector3::new(3.0, 4.0, 0.0),
            )));
            host.start_interactive(Box::new(PickThenMark));
            h
        };
        // The command requested an entity pick, not a free point.
        assert!(app.tabs[0].active_cmd.as_ref().unwrap().needs_entity_pick());
        let r = app.tabs[0]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_entity_pick(target, glam::DVec3::new(3.0, 4.0, 0.0));
        let _ = app.apply_cmd_result(r);
        // Original point + the mark the command committed.
        assert_eq!(app.tabs[0].scene.document.entities().count(), 2);
    }

    #[test]
    fn host_document_reader_sees_entities() {
        use ocs_plugin_api::host::ReaderEntityKind;
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        host.add_entity(acadrust::EntityType::Point(acadrust::entities::Point::at(
            acadrust::types::Vector3::new(7.0, 8.0, 0.0),
        )));
        let reader = host.document_reader();
        assert_eq!(reader.entity_count(), 1);
        let mut kinds = Vec::new();
        reader.for_each_entity(&mut |e| kinds.push(e.kind));
        assert_eq!(kinds, vec![ReaderEntityKind::Point]);
    }

    #[test]
    fn host_document_view_publish_and_read_shared() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let info = host.document_view().unwrap();
        let reader = ocs_plugin_api::shm::SharedDocumentReader::<
            ocs_plugin_api::shm::DocumentViewData,
        >::open(std::path::Path::new(&info.path))
                .unwrap();
        assert_eq!(reader.entity_count(), 0);

        host.add_entity(acadrust::EntityType::Point(acadrust::entities::Point::at(
            acadrust::types::Vector3::new(1.0, 2.0, 0.0),
        )));

        assert_eq!(reader.entity_count(), 1);
    }

    /// Read an entity handle from the live document, write XDATA for that
    /// handle, read it back, and remove it.
    #[test]
    fn document_reader_to_xdata_roundtrip() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let h = host.add_entity(EntityType::Point(Point::at(acadrust::types::Vector3::new(
            7.0, 8.0, 0.0,
        ))));

        {
            let reader = host.document_reader();
            assert_eq!(reader.entity_count(), 1);
            let mut handles = Vec::new();
            reader.for_each_entity(&mut |e| handles.push(e.handle));
            assert_eq!(handles, vec![h]);
        }

        let mut rec = ExtendedDataRecord::new("ROUNDTRIP");
        rec.add_value(XDataValue::String("from-reader".to_string()));
        assert!(host.write_record(h, rec));

        let got = host.read_record(h, "ROUNDTRIP").expect("record missing");
        assert_eq!(got.values.len(), 1);
        assert!(matches!(got.values[0], XDataValue::String(ref s) if s == "from-reader"));

        assert!(host.remove_record(h, "ROUNDTRIP"));
        assert!(host.read_record(h, "ROUNDTRIP").is_none());
    }

    /// Publish a shared document view, read the entity handle from shared
    /// memory, then write and read-back XDATA through the normal HostApi RPCs.
    #[test]
    fn shared_document_view_read_then_write_xdata_roundtrip() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);
        let info = host.document_view().unwrap();
        let reader = ocs_plugin_api::shm::SharedDocumentReader::<
            ocs_plugin_api::shm::DocumentViewData,
        >::open(std::path::Path::new(&info.path))
                .unwrap();

        let h = host.add_entity(EntityType::Point(Point::at(acadrust::types::Vector3::new(
            1.0, 2.0, 0.0,
        ))));
        assert_eq!(reader.entity_count(), 1);

        let mut handles = Vec::new();
        reader.for_each_entity(&mut |e| handles.push(e.handle));
        assert_eq!(handles, vec![h]);

        let mut rec = ExtendedDataRecord::new("SHM_ROUNDTRIP");
        rec.add_value(XDataValue::Integer32(123));
        assert!(host.write_record(h, rec));

        let got = host
            .read_record(h, "SHM_ROUNDTRIP")
            .expect("record missing");
        assert_eq!(got.values.len(), 1);
        assert!(matches!(got.values[0], XDataValue::Integer32(123)));
    }

    #[test]
    fn test_plugin_add_layer_with_defaults_and_modify() {
        use ocs_plugin_api::host::LayerConfig;

        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);

        // 1. Add layer with minimal config (only name specified)
        let config = LayerConfig {
            name: "ELECTRICAL".to_string(),
            ..Default::default()
        };
        let handle = host.add_layer(config).expect("should create layer");
        assert_ne!(handle, acadrust::Handle::NULL);

        // Verify defaults were applied
        let layer = host.document().layers.get("ELECTRICAL").expect("layer should exist");
        assert_eq!(layer.color, acadrust::types::Color::Index(7));
        assert_eq!(layer.line_type, "Continuous");
        assert_eq!(layer.line_weight, acadrust::types::LineWeight::ByLayer);
        assert!(!layer.flags.off);
        assert!(!layer.flags.frozen);
        assert!(!layer.flags.locked);
        assert!(layer.is_plottable);

        // 2. Duplicate add_layer should be rejected (return None)
        let dup_config = LayerConfig {
            name: "ELECTRICAL".to_string(),
            color: Some(acadrust::types::Color::Index(1)),
            ..Default::default()
        };
        assert!(host.add_layer(dup_config).is_none(), "duplicate layer should return None");

        // 3. Modify only color and locked; other properties should remain untouched
        let mod_config = LayerConfig {
            name: "ELECTRICAL".to_string(),
            color: Some(acadrust::types::Color::Index(1)),
            locked: Some(true),
            ..Default::default()
        };
        assert!(host.modify_layer(mod_config));

        let updated = host.document().layers.get("ELECTRICAL").expect("layer should exist");
        assert_eq!(updated.color, acadrust::types::Color::Index(1)); // Modified to red
        assert!(updated.flags.locked);                               // Modified to locked
        assert_eq!(updated.line_type, "Continuous");                 // Kept as-is
        assert_eq!(updated.line_weight, acadrust::types::LineWeight::ByLayer); // Kept as-is
        assert!(!updated.flags.off);                                 // Kept as-is

        // 4. Modify nonexistent layer returns false
        let non_existent = LayerConfig {
            name: "DOES_NOT_EXIST".to_string(),
            color: Some(acadrust::types::Color::Index(2)),
            ..Default::default()
        };
        assert!(!host.modify_layer(non_existent));
    }

    #[test]
    fn test_plugin_execute_command() {
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].is_start = false;
        let mut host = HostSession::new(&mut app, 0);

        // 1. Single-line command with newline finishes command
        assert!(host.execute_command("LINE 0,0 10,10\n"));
        assert!(host.app.tabs[0].active_cmd.is_none());
        assert_eq!(host.document().entities().count(), 1);

        // 2. Streamed command without newline leaves tool active
        assert!(host.execute_command("LINE 10,10"));
        assert!(host.app.tabs[0].active_cmd.is_some());
        // Feed next point without newline
        assert!(host.execute_command("20,20"));
        assert!(host.app.tabs[0].active_cmd.is_some());
        // Send explicit ENTER
        assert!(host.execute_command("ENTER"));
        assert!(host.app.tabs[0].active_cmd.is_none());
        assert_eq!(host.document().entities().count(), 2);

        // 3. Command with PAUSE buffers remaining tokens until point is provided
        assert!(host.execute_command("LINE 20,20 PAUSE ENTER"));
        assert!(host.app.tabs[0].active_cmd.is_some());
        assert_eq!(
            host.app.tabs[0].pending_pause_tokens,
            Some(vec!["ENTER".to_string()])
        );
        // User clicks second point in viewport (which calls on_point + apply_cmd_result)
        let r = host.app.tabs[0]
            .active_cmd
            .as_mut()
            .unwrap()
            .on_point(glam::DVec3::new(30.0, 30.0, 0.0));
        let _ = host.app.apply_cmd_result(r);
        // Draining in apply_cmd_result should have executed ENTER and completed LINE
        assert!(host.app.tabs[0].active_cmd.is_none());
        assert_eq!(host.document().entities().count(), 3);

        // 4. Command with PAUSE and trailing \n
        assert!(host.execute_command("LINE 30,30 PAUSE\n"));
        assert!(host.app.tabs[0].active_cmd.is_some());
        assert_eq!(
            host.app.tabs[0].pending_pause_tokens,
            Some(vec!["ENTER".to_string()])
        );
        // Escape cancels paused command cleanly
        assert!(host.execute_command("ESC"));
        assert!(host.app.tabs[0].active_cmd.is_none());
        assert_eq!(host.app.tabs[0].pending_pause_tokens, None);

        // 5. CLAYER command
        host.add_layer(ocs_plugin_api::host::LayerConfig {
            name: "TEST".to_string(),
            ..Default::default()
        });
        assert!(host.execute_command("CLAYER TEST\n"));
        assert_eq!(host.document().header.current_layer_name, "TEST");

        // 6. Non-typical command: POINT
        assert!(host.execute_command("POINT 50,50\n"));
        assert!(host.app.tabs[0].active_cmd.is_none());
        assert_eq!(host.document().entities().count(), 4);

        // 7. Non-typical command: DONUT
        assert!(host.execute_command("DONUT 10 30 100,100 \n"));
        assert!(host.app.tabs[0].active_cmd.is_none());
        assert!(host.document().entities().count() >= 5);

        // 8. Non-typical command: ELLIPSE
        assert!(host.execute_command("ELLIPSE 0,0 80,0 30\n"));
        assert!(host.app.tabs[0].active_cmd.is_none());

        // 9. Non-typical inline commands: UCS
        assert!(host.execute_command("UCS ORIGIN 50,50,0\n"));
        assert!(host.app.tabs[0].active_ucs.is_some());
        assert!(host.execute_command("UCS W\n"));
        assert!(host.app.tabs[0].active_ucs.is_none());

        // 10. Non-typical inline commands: SETVAR
        assert!(host.execute_command("SETVAR PDMODE 35\n"));

        // 11. Viewport visual style via drive_headless_task
        assert!(host.execute_command("VSCURRENT FLATSHADED\n"));
        assert_eq!(
            host.app.tabs[0].render_mode,
            ocs_plugin_api::host::acadrust::entities::ViewportRenderMode::FlatShaded
        );

        // 12. Drafting aids toggle via drive_headless_task
        let initial_grid = host.app.show_grid;
        assert!(host.execute_command("GRID\n"));
        assert_eq!(host.app.show_grid, !initial_grid);
    }
}
