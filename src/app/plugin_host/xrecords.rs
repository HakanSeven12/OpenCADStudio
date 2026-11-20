// XRecord dictionary tree CRUD and geolocation.

use super::validation::{validated_symbol_name, validated_xrecord_entries};
use super::*;

impl<'a> HostSession<'a> {
    /// The dictionary addressed by `dictionary_path` under the named-object
    /// dictionary root (an empty path is the root itself). Nothing is created:
    /// a missing fragment is an error, and so is a fragment that exists but
    /// names a non-dictionary object.
    pub(super) fn xrecord_dictionary_at(&self, dictionary_path: &[&str]) -> Result<Handle, String> {
        use document::objects::ObjectType;
        let root = self.document().header.named_objects_dict_handle;
        if !matches!(
            self.document().objects.get(&root),
            Some(ObjectType::Dictionary(_))
        ) {
            return Err("the drawing has no named-object dictionary".to_owned());
        }
        let mut current = root;
        for fragment in dictionary_path {
            let name = validated_symbol_name(fragment, "a dictionary name")?;
            match self.document().objects.get(&current) {
                Some(ObjectType::Dictionary(dictionary)) => match dictionary.get(&name) {
                    Some(child) => {
                        if !matches!(
                            self.document().objects.get(&child),
                            Some(ObjectType::Dictionary(_))
                        ) {
                            return Err(format!("dictionary entry {name:?} is not a dictionary"));
                        }
                        current = child;
                    }
                    None => return Err(format!("dictionary {name:?} does not exist")),
                },
                _ => return Err(format!("dictionary {current:?} does not exist")),
            }
        }
        Ok(current)
    }

    /// The deepest ancestor of `dictionary_path` that already exists, and how
    /// deep the path resolved. Errors when a fragment that exists names a
    /// non-dictionary object. Creation calls this first so its refusals happen
    /// before the undo step is recorded.
    fn deepest_existing_xrecord_dictionary(
        &self,
        dictionary_path: &[&str],
    ) -> Result<(Handle, usize), String> {
        use document::objects::ObjectType;
        let root = self.document().header.named_objects_dict_handle;
        if !matches!(
            self.document().objects.get(&root),
            Some(ObjectType::Dictionary(_))
        ) {
            return Err("the drawing has no named-object dictionary".to_owned());
        }
        let mut current = root;
        let mut depth = 0usize;
        for fragment in dictionary_path {
            let name = validated_symbol_name(fragment, "a dictionary name")?;
            match self.document().objects.get(&current) {
                Some(ObjectType::Dictionary(dictionary)) => match dictionary.get(&name) {
                    Some(child) => {
                        if !matches!(
                            self.document().objects.get(&child),
                            Some(ObjectType::Dictionary(_))
                        ) {
                            return Err(format!("dictionary entry {name:?} is not a dictionary"));
                        }
                        current = child;
                        depth += 1;
                    }
                    None => break,
                },
                _ => return Err(format!("dictionary {current:?} does not exist")),
            }
        }
        Ok((current, depth))
    }

    /// Walk `dictionary_path` from the named-object dictionary root, creating
    /// missing dictionaries on the way — the same bootstrap
    /// `ensure_image_dictionary` uses for ACAD_IMAGE_DICT: each new dictionary
    /// owns its handle, is owned by its parent and is registered in the parent
    /// under its name. Mutates; only valid after the caller validated the path
    /// and recorded its undo step.
    fn xrecord_dictionary_at_or_create(
        &mut self,
        dictionary_path: &[&str],
    ) -> Result<Handle, String> {
        use document::objects::{Dictionary, ObjectType};
        let root = self.document().header.named_objects_dict_handle;
        if !matches!(
            self.document().objects.get(&root),
            Some(ObjectType::Dictionary(_))
        ) {
            return Err("the drawing has no named-object dictionary".to_owned());
        }
        let mut current = root;
        for fragment in dictionary_path {
            let name = validated_symbol_name(fragment, "a dictionary name")?;
            let existing = match self.document().objects.get(&current) {
                Some(ObjectType::Dictionary(dictionary)) => dictionary.get(&name),
                _ => return Err(format!("dictionary {current:?} does not exist")),
            };
            match existing {
                Some(child) => {
                    if !matches!(
                        self.document().objects.get(&child),
                        Some(ObjectType::Dictionary(_))
                    ) {
                        return Err(format!("dictionary entry {name:?} is not a dictionary"));
                    }
                    current = child;
                }
                None => {
                    let child = self.document_mut().allocate_handle();
                    let mut dictionary = Dictionary::new();
                    dictionary.handle = child;
                    dictionary.owner = current;
                    self.document_mut()
                        .objects
                        .insert(child, ObjectType::Dictionary(dictionary));
                    if let Some(ObjectType::Dictionary(entries)) =
                        self.document_mut().objects.get_mut(&current)
                    {
                        entries.add_entry(name, child);
                    }
                    current = child;
                }
            }
        }
        Ok(current)
    }

    /// Undo composition for XRecord writes. Plugins may open one `push_undo`
    /// scope for a whole operation — the same contract entity writes follow
    /// (`add_entity` and friends perform no undo bookkeeping of their own) —
    /// and every XRecord mutation inside it joins that open scope, so the
    /// finished scope is a single undo step under the plugin's own label that
    /// restores (and redoes) the whole operation at once. Only a mutation with
    /// no scope open opens its own — and then the caller finishes it again,
    /// keeping a scope-less call one undo step (the `table_operation` /
    /// `update_entities_transaction` behavior). Refusals never reach this:
    /// they are decided before any change. Returns whether this call opened
    /// its own scope.
    pub(super) fn ensure_xrecord_undo_scope(&mut self, label: &str) -> bool {
        if self.app.tabs[self.tab].history.pending.is_none() {
            self.push_undo(label);

            true
        } else {
            false
        }
    }

    /// Create an XRecord named `name` under `dictionary_path`, storing
    /// `entries` (API v8, additive). Missing dictionaries on the path are
    /// created, so the first call seeds a plugin's namespace; the name must be
    /// unused (dictionary lookup is case-insensitive). Every refusal happens
    /// before the undo scope is touched, so a failure changes nothing. The
    /// created record is registered in — and owned by — its dictionary, and
    /// the new handle is returned.
    ///
    /// Undo composition: a mutation inside a plugin-opened `push_undo` scope
    /// joins it (see [`HostSession::ensure_xrecord_undo_scope`]); a scope-less
    /// call is one undo step of its own.
    pub fn create_xrecord(
        &mut self,
        dictionary_path: &[&str],
        name: &str,
        entries: Vec<document::objects::XRecordEntry>,
    ) -> Result<Handle, String> {
        use document::objects::{ObjectType, XRecord};
        let name = validated_symbol_name(name, "an XRecord name")?;
        validated_xrecord_entries(&entries)?;
        // Pre-check the path: fragments that already exist must be
        // dictionaries, and when the full path exists the name must be free —
        // create must never silently overwrite a key.
        let (deepest, depth) = self.deepest_existing_xrecord_dictionary(dictionary_path)?;
        if depth == dictionary_path.len() {
            if let Some(ObjectType::Dictionary(dictionary)) = self.document().objects.get(&deepest)
            {
                if dictionary.get(&name).is_some() {
                    return Err(format!("XRecord {name:?} already exists"));
                }
            }
        }
        let own_scope = self.ensure_xrecord_undo_scope("Create XRecord");
        // The pre-check ruled out every failure mode; the walk now only creates
        // the dictionary fragments that were missing.
        let dictionary = self
            .xrecord_dictionary_at_or_create(dictionary_path)
            .expect("validated dictionary path");
        let handle = self.document_mut().allocate_handle();
        let mut record = XRecord::named(name.clone());
        record.handle = handle;
        record.owner = dictionary;
        record.entries = entries;
        record.synchronize_object_references();
        self.document_mut()
            .objects
            .insert(handle, ObjectType::XRecord(record));
        if let Some(ObjectType::Dictionary(entries)) =
            self.document_mut().objects.get_mut(&dictionary)
        {
            entries.add_entry(name, handle);
        }
        self.set_dirty();
        self.publish_document_view();
        if own_scope {
            self.app.finish_pending_history(self.tab);
        }
        Ok(handle)
    }

    /// Read the entries of an existing XRecord by name (API v8, additive).
    /// The entries are returned verbatim; the path, the name and the target
    /// kind are validated first, so a refusal changes nothing.
    pub fn read_xrecord(
        &self,
        dictionary_path: &[&str],
        name: &str,
    ) -> Result<Vec<document::objects::XRecordEntry>, String> {
        use document::objects::ObjectType;
        let (_, _, handle) = self.xrecord_lookup(dictionary_path, name)?;
        match self.document().objects.get(&handle) {
            Some(ObjectType::XRecord(record)) => Ok(record.entries.clone()),
            _ => unreachable!("validated XRecord"),
        }
    }

    /// Atomically replace the entries of an existing XRecord (API v8,
    /// additive). Neither the path nor its dictionaries are created — an
    /// update must never create anything silently. Identity (handle, owning
    /// dictionary, name) stays; the entries and their DWG handle-reference
    /// stream are replaced and the record's verbatim raw payloads are dropped,
    /// so later saves emit the new content. Every refusal happens before the
    /// undo scope is touched. Undo composition: a mutation inside a
    /// plugin-opened `push_undo` scope joins it (see
    /// [`HostSession::ensure_xrecord_undo_scope`]); a scope-less call is one
    /// undo step of its own.
    pub fn update_xrecord(
        &mut self,
        dictionary_path: &[&str],
        name: &str,
        entries: Vec<document::objects::XRecordEntry>,
    ) -> Result<Handle, String> {
        let handle = self.update_xrecord_quiet(dictionary_path, name, entries)?;
        self.set_dirty();
        self.publish_document_view();
        Ok(handle)
    }

    /// [`Self::update_xrecord`] without the boundary publish/settle — the
    /// batch route publishes once for the whole set (P1-17). The undo-scope
    /// handling matches the visible variant: a mutation inside a plugin-opened
    /// `push_undo` scope joins it; a scope-less call opens its own and the
    /// visible wrapper finishes it.
    fn update_xrecord_quiet(
        &mut self,
        dictionary_path: &[&str],
        name: &str,
        entries: Vec<document::objects::XRecordEntry>,
    ) -> Result<Handle, String> {
        use document::objects::ObjectType;
        validated_xrecord_entries(&entries)?;
        let (_, _, handle) = self.xrecord_lookup(dictionary_path, name)?;
        let own_scope = self.ensure_xrecord_undo_scope("Update XRecord");
        let record = match self.document_mut().objects.get_mut(&handle) {
            Some(ObjectType::XRecord(record)) => record,
            _ => unreachable!("validated XRecord"),
        };
        record.entries = entries;
        // Rebuild the DWG object-reference stream from the new 330-369 entries
        // and drop the verbatim bytes an earlier load kept: the update fully
        // defines the record now.
        record.synchronize_object_references();
        record.preserve_object_reference_stream = false;
        record.entries_complete = true;
        record.raw_data.clear();
        record.raw_dwg_data = None;
        record.raw_dwg_handle_bits = 0;
        record.raw_dwg_version = None;
        // API v13 dirty regions — before the publish that carries them.
        let epoch = self.geometry_epoch();
        self.app.tabs[self.tab].mark_plugin_record_dirty(handle, "NOD".to_string(), epoch);
        if own_scope {
            // The dirty flag must flip BEFORE the scope closes:
            // finish_pending_history snapshots `dirty_after` into the undo
            // entry, and redo replays that — a clean-tab XRecord update that
            // closes first would record dirty=false and redo silently loses
            // the change on close (review finding 1).
            self.set_dirty();
            self.app.finish_pending_history(self.tab);
        }
        Ok(handle)
    }

    /// Delete the XRecord named `name` and its dictionary entry (API v8,
    /// additive). Returns the removed record with its prior content, so audits
    /// keep what they removed. Every refusal happens before the undo scope is
    /// touched. Undo composition: a mutation inside a plugin-opened
    /// `push_undo` scope joins it (see
    /// [`HostSession::ensure_xrecord_undo_scope`]); a scope-less call is one
    /// undo step of its own.
    pub fn delete_xrecord(
        &mut self,
        dictionary_path: &[&str],
        name: &str,
    ) -> Result<document::objects::XRecord, String> {
        use document::objects::ObjectType;
        let (dictionary, name, handle) = self.xrecord_lookup(dictionary_path, name)?;
        let own_scope = self.ensure_xrecord_undo_scope("Delete XRecord");
        let removed = match self.document_mut().objects.remove(&handle) {
            Some(ObjectType::XRecord(record)) => record,
            _ => unreachable!("validated XRecord"),
        };
        if let Some(ObjectType::Dictionary(entries)) =
            self.document_mut().objects.get_mut(&dictionary)
        {
            entries
                .entries
                .retain(|(key, value)| !key.eq_ignore_ascii_case(&name) || *value != handle);
        }
        self.set_dirty();
        self.publish_document_view();
        if own_scope {
            self.app.finish_pending_history(self.tab);
        }
        Ok(removed)
    }

    /// Create or update the model-space GEODATA object (API v22, additive):
    /// the CRS through the same path as GEOGRAPHICLOCATION, then the plugin's
    /// design/reference points and north. Every refusal happens before the
    /// undo scope is touched.
    pub fn set_geo_location(
        &mut self,
        location: &ocs_plugin_api::host::GeoLocation,
    ) -> Result<Handle, String> {
        use document::objects::ObjectType;
        use document::types::{Vector2, Vector3};
        let crs = crate::io::crs::CrsDefinition::from_text(&location.coordinate_system)?;
        crs.apply_to_geodata(&mut document::objects::GeoData::default())?;
        let values = location
            .design_point
            .iter()
            .chain(&location.reference_point)
            .chain(&location.north_direction);
        if !values.into_iter().all(|value| value.is_finite()) {
            return Err("geographic location values must be finite".to_owned());
        }
        let [nx, ny] = location.north_direction;
        let length = nx.hypot(ny);
        if length < 1e-12 {
            return Err("north direction must not be zero".to_owned());
        }
        if !self.document().block_records.contains("*Model_Space") {
            return Err("The drawing has no model-space block record".to_owned());
        }
        let own_scope = self.ensure_xrecord_undo_scope("Set Geographic Location");
        let handle = crate::io::crs::set_document_crs(self.document_mut(), &crs)?;
        if let Some(ObjectType::GeoData(geo)) = self.document_mut().objects.get_mut(&handle) {
            let [x, y, z] = location.design_point;
            geo.design_point = Vector3::new(x, y, z);
            let [x, y, z] = location.reference_point;
            geo.reference_point = Vector3::new(x, y, z);
            geo.north_direction = Vector2::new(nx / length, ny / length);
        }
        self.app.tabs[self.tab].scene.refresh_object_data_cache();
        self.app.tabs[self.tab].scene.bump_geometry();
        self.set_dirty();
        self.publish_document_view();
        if own_scope {
            self.app.finish_pending_history(self.tab);
        }
        Ok(handle)
    }

    /// Remove the model-space GEODATA object and its extension-dictionary
    /// entry (API v22, additive). `Ok(false)` when there was none.
    pub fn remove_geo_location(&mut self) -> Result<bool, String> {
        use document::objects::ObjectType;
        let Some(handle) = crate::io::crs::model_space_geo_data_handle(self.document()) else {
            return Ok(false);
        };
        let own_scope = self.ensure_xrecord_undo_scope("Remove Geographic Location");
        let document = self.document_mut();
        document.objects.remove(&handle);
        for object in document.objects.values_mut() {
            if let ObjectType::Dictionary(dictionary) = object {
                dictionary.entries.retain(|(_, value)| *value != handle);
            }
        }
        self.app.tabs[self.tab].scene.refresh_object_data_cache();
        self.app.tabs[self.tab].scene.bump_geometry();
        self.set_dirty();
        self.publish_document_view();
        if own_scope {
            self.app.finish_pending_history(self.tab);
        }
        Ok(true)
    }

    /// List the XRecord names under the dictionary addressed by
    /// `dictionary_path` (API v8, additive): the dictionary keys that address
    /// XRecord objects, in the dictionary's own order, so consumers can audit
    /// and export without knowing names in advance. Non-XRecord entries are
    /// not listed.
    pub fn list_xrecords(&self, dictionary_path: &[&str]) -> Result<Vec<String>, String> {
        use document::objects::ObjectType;
        let dictionary = self.xrecord_dictionary_at(dictionary_path)?;
        let Some(ObjectType::Dictionary(entries)) = self.document().objects.get(&dictionary) else {
            return Err(format!("dictionary {dictionary:?} does not exist"));
        };
        Ok(entries
            .entries
            .iter()
            .filter_map(|(key, handle)| match self.document().objects.get(handle) {
                Some(ObjectType::XRecord(_)) => Some(key.clone()),
                _ => None,
            })
            .collect())
    }

    /// Mark (or unmark) an entity as reference-only (API v9, additive):
    /// plugin-owned display geometry the built-in edit paths treat like a
    /// locked-layer entity, while the owning plugin keeps full write access
    /// through this session. The designation is a scene-scoped in-memory set
    /// keyed by handle: nothing is written to the saved DWG/DXF, the set dies
    /// with the tab, and a reloaded drawing starts undesignated — the owning
    /// plugin re-marks on the `DocumentOpened` notification, which makes the
    /// in-memory designation fully equivalent to a persisted one for its use
    /// case.
    ///
    /// Every refusal — absent entity, locked layer — is decided before the
    /// undo scope is touched, so a failure changes nothing. A call that would
    /// not change the current designation succeeds without recording an undo
    /// step, without bumping the geometry epoch and without publishing — the
    /// idempotence that terminates the re-mark loop driven by the
    /// document-changed dispatch. Undo composition: a change inside a
    /// plugin-opened `push_undo` scope joins it (see
    /// [`HostSession::ensure_xrecord_undo_scope`]); a scope-less call is one
    /// undo step of its own. The set is scene state, not a document write, so
    /// the undo delta restores it through the explicit designation channel
    /// (`DeltaSnapshot::reference_designations`).
    pub fn set_reference_only(
        &mut self,
        handle: Handle,
        reference_only: bool,
    ) -> Result<(), String> {
        // Refusals precede any change (and any undo step).
        if self.document().get_entity(handle).is_none() {
            return Err(format!("entity {handle:?} does not exist"));
        }
        if self.app.tabs[self.tab].scene.is_layer_locked(handle) {
            return Err(format!("entity {handle:?} is on a locked layer"));
        }
        if self.app.tabs[self.tab].scene.is_reference_only(handle) == reference_only {
            return Ok(());
        }
        let own_scope = self.ensure_xrecord_undo_scope("Set Reference-Only");
        // First-touch before-image for the pending scope's designation
        // channel, recorded before the set changes (see
        // `Scene::record_undo_reference_designation_before`).
        let before = self.app.tabs[self.tab]
            .scene
            .reference_only
            .is_marked(handle);
        self.app
            .record_reference_designation_before_pending(self.tab, handle, before);
        self.app.tabs[self.tab]
            .scene
            .set_reference_only_membership(handle, reference_only);
        debug_assert!(
            self.app.tabs[self.tab]
                .scene
                .reference_only
                .is_marked(handle)
                == reference_only,
            "designation state was checked before the mutation"
        );
        // The wires carry the click-through stamp baked at tessellation, so a
        // designation change must rebuild them incrementally (or rebuild block
        // caches when the entity belongs to a block definition).
        self.bump_handles_incremental(&[handle]);
        self.set_dirty();
        self.publish_document_view();
        if own_scope {
            self.app.finish_pending_history(self.tab);
        }
        Ok(())
    }

    /// Read several XRecords in one call (API v13): per-entry results, `Err`
    /// per missing path/name instead of failing the whole batch.
    pub fn read_xrecords(
        &self,
        reads: &[ocs_plugin_api::host::XRecordRead],
    ) -> Result<Vec<Result<Vec<ocs_plugin_api::host::XRecordEntry>, String>>, String> {
        Ok(reads
            .iter()
            .map(|read| {
                let path: Vec<&str> = read.dictionary_path.iter().map(|s| s.as_str()).collect();
                self.read_xrecord(&path, &read.name)
            })
            .collect())
    }

    /// Replace the entries of several XRecords in one call (API v13): each
    /// entry applies the single-update contract; the batch shares the
    /// entity-write undo composition (inside a plugin-opened `push_undo`
    /// scope it joins that scope).
    pub fn update_xrecords(
        &mut self,
        updates: &[ocs_plugin_api::host::XRecordUpdate],
    ) -> Result<Vec<Result<Handle, String>>, String> {
        // P1-17: the batch publishes ONCE for the whole set instead of once
        // per record — a 100-record batch no longer fans out into 100
        // publication passes.
        let results: Vec<Result<Handle, String>> = updates
            .iter()
            .map(|update| {
                let path: Vec<&str> = update.dictionary_path.iter().map(|s| s.as_str()).collect();
                self.update_xrecord_quiet(&path, &update.name, update.entries.clone())
            })
            .collect();
        if results.iter().any(|result| result.is_ok()) {
            self.set_dirty();
            self.publish_document_view();
        }
        Ok(results)
    }

    /// List a dictionary's XRecords WITH their entries in one call (API v13):
    /// the discovery walk (list, then one read per record) collapses to a
    /// single frame.
    pub fn list_xrecords_with_entries(
        &self,
        dictionary_path: &[&str],
    ) -> Result<Vec<(String, Vec<ocs_plugin_api::host::XRecordEntry>)>, String> {
        let names = self.list_xrecords(dictionary_path)?;
        Ok(names
            .into_iter()
            .map(|name| {
                let entries = self
                    .read_xrecord(dictionary_path, &name)
                    .unwrap_or_default();
                (name, entries)
            })
            .collect())
    }

    /// Resolve an existing XRecord to its containing dictionary, its validated
    /// name and its handle, after validating the path, the name and the target
    /// kind.
    pub(super) fn xrecord_lookup(
        &self,
        dictionary_path: &[&str],
        name: &str,
    ) -> Result<(Handle, String, Handle), String> {
        use document::objects::ObjectType;
        let dictionary = self.xrecord_dictionary_at(dictionary_path)?;
        let name = validated_symbol_name(name, "an XRecord name")?;
        let Some(ObjectType::Dictionary(entries)) = self.document().objects.get(&dictionary) else {
            return Err(format!("dictionary {dictionary:?} does not exist"));
        };
        let Some(handle) = entries.get(&name) else {
            return Err(format!("XRecord {name:?} does not exist"));
        };
        if !matches!(
            self.document().objects.get(&handle),
            Some(ObjectType::XRecord(_))
        ) {
            return Err(format!("dictionary entry {name:?} is not an XRecord"));
        }
        Ok((dictionary, name, handle))
    }
}
