// Plugin class registration, staging and parameter properties.

use super::validation::validated_symbol_name;
use super::*;

impl<'a> HostSession<'a> {
    /// Register a plugin-scoped entity class (API v14, additive): TAB-SCOPED
    /// display-session state (the class survives across requests of the
    /// owning plugin's session — unlike a session-local registry, the store
    /// rides the tab like the grip sets do; nothing persists: entries die
    /// with the plugin, re-declared per load). Re-registering the same
    /// identity with an equal schema is idempotent; a mismatch refuses
    /// without touching the live class — a parameter-set change re-stages
    /// the instances, it never hot-swaps a schema under live entities. The
    /// plugin id never travels the wire: the runner supplies the serving
    /// process's own id, so a plugin can only declare in its own name.
    pub fn register_plugin_class(
        &mut self,
        plugin_id: &str,
        class: &ocs_plugin_api::host::PluginClassDeclaration,
    ) -> Result<(), String> {
        let known = crate::plugin::grip_points::visibility(&self.app.disabled_plugins)(plugin_id);
        if !known {
            return Err(format!(
                "plugin {plugin_id:?} is not a loaded, alive plugin; classes must be declared \
                 by their owner"
            ));
        }
        if class.class_id.is_empty() {
            return Err("a plugin class declaration needs its identity key".to_owned());
        }
        if let Some(entry) = class
            .grip_schema
            .iter()
            .find(|entry| entry.param_name.is_empty())
        {
            return Err(format!(
                "class {}: grip schema entry {} needs its parameter name",
                class.class_id, entry.grip_id
            ));
        }
        let mut ids: Vec<u64> = class
            .grip_schema
            .iter()
            .map(|entry| entry.grip_id)
            .collect();
        ids.sort_unstable();
        if ids.len() > 1 && ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(format!(
                "class {} declares duplicate grip ids in its schema",
                class.class_id
            ));
        }
        for parameter in &class.parameters {
            if parameter.name.is_empty() {
                return Err(format!(
                    "class {}: a declared parameter needs its name",
                    class.class_id
                ));
            }
        }
        let mut sorted: Vec<&str> = class.parameters.iter().map(|p| p.name.as_str()).collect();
        sorted.sort_unstable();
        if sorted.len() > 1 && sorted.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(format!(
                "class {} declares duplicate parameter names in its schema",
                class.class_id
            ));
        }
        self.app.tabs[self.tab]
            .plugin_classes
            .declare(plugin_id, class.clone())
    }

    /// Stage one instance of a plugin-registered class (API v14, additive) —
    /// [`HostSession::stage_parametric_chain`]'s contract generalized: the
    /// design descriptor is host-evaluated before any change (the v14 engine
    /// for the `ParametricChain` kind; a kind the host does not know refuses
    /// at the contract), the staged set carries the class identity key in
    /// XDATA and a reference-only designation, and the undo composition is
    /// the entity-write contract. The caller must be the class's owner (the
    /// store is keyed by declaring plugin; the plugin id never travels the
    /// wire — the server supplies the process's own id).
    pub fn stage_plugin_class_instance(
        &mut self,
        plugin_id: &str,
        instance: &ocs_plugin_api::host::PluginClassInstance,
    ) -> Result<Vec<Handle>, String> {
        let known = crate::plugin::grip_points::visibility(&self.app.disabled_plugins)(plugin_id);
        if !known {
            return Err(format!(
                "plugin {plugin_id:?} is not a loaded, alive plugin; instances must be staged \
                 by their owner"
            ));
        }
        let registry = &self.app.tabs[self.tab].plugin_classes;
        let registered = registry
            .find(plugin_id, &instance.class_id)
            .ok_or_else(|| {
                let owner = registry.owner_of(&instance.class_id);
                match owner {
                    Some(owner) => format!(
                        "class {:?} is owned by plugin {owner:?}; instances must be staged by \
                         their owner",
                        instance.class_id
                    ),
                    None => format!(
                        "class {:?} is not registered in this session; register it before \
                         staging instances",
                        instance.class_id
                    ),
                }
            })?;
        if registered.schema_version != instance.schema_version {
            return Err(format!(
                "instance of class {:?} was authored for schema version {}, the session \
                 declares {}",
                instance.class_id, instance.schema_version, registered.schema_version
            ));
        }
        // The exhaustive match is the expand-velocity point: a newer
        // plugin kind lands here as a compile-time arm, never as a guess.
        let (entities, _carriers) = match &instance.design {
            ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(chain) => {
                crate::parametric::build_display_entities(chain, instance.tolerance)
                    .map_err(|e| format!("instance of class {:?}: {e}", instance.class_id))?
            }
        };
        if entities.is_empty() {
            // Zero-geometry designs record nothing, like zero-length chains.
            return Ok(Vec::new());
        }
        // P1-11 preflight: every staged entity's layer is checked BEFORE any
        // insertion, so a locked layer refuses the whole instance instead of
        // leaving entities without their identity stamp (the old behavior
        // staged first and stamped after — a refusal mid-way left a
        // partially-staged instance no re-stage could complete).
        if let Some(entity) = entities
            .iter()
            .find(|entity| self.new_entity_layer_locked(entity))
        {
            return Err(format!(
                "instance of class {:?} targets locked layer {:?}; refusing the whole instance",
                instance.class_id,
                entity.common().layer
            ));
        }
        let own_scope = self.ensure_xrecord_undo_scope("Stage plugin class instance");
        let handles = self.add_entities(entities);
        // All-or-nothing stamping: after the preflight a stamp refusal can
        // only mean an unexpected condition — the staged entities are erased
        // again so no half-staged instance survives (P1-11).
        let mut stamp_failed: Option<usize> = None;
        for (index, handle) in handles.iter().enumerate() {
            // Identity stamp: the class key as the XDATA application on every
            // staged entity (write_record registers the APPID so the stamp
            // survives the file), carrying the schema version and the record
            // linkage.
            let mut stamp = ocs_plugin_api::host::document::xdata::ExtendedDataRecord::new(
                instance.class_id.clone(),
            );
            stamp.add_value(
                ocs_plugin_api::host::document::xdata::XDataValue::Integer32(
                    instance.schema_version as i32,
                ),
            );
            if !instance.design_record.is_empty() {
                stamp.add_value(ocs_plugin_api::host::document::xdata::XDataValue::String(
                    instance.design_record.clone(),
                ));
            }
            if instance.selectable_custom {
                // Flags have a fixed third slot, even without record linkage.
                if instance.design_record.is_empty() {
                    stamp.add_value(ocs_plugin_api::host::document::xdata::XDataValue::String(
                        String::new(),
                    ));
                }
                stamp.add_value(ocs_plugin_api::host::document::xdata::XDataValue::Integer32(1));
            }
            if !self.write_record_quiet(*handle, stamp) {
                stamp_failed = Some(index);
                break;
            }
            // Designation inside the SAME scope, the before-image
            // pending-scoped the way set_reference_only does.
            let before = self.app.tabs[self.tab]
                .scene
                .reference_only
                .is_marked(*handle);
            self.app
                .record_reference_designation_before_pending(self.tab, *handle, before);
            self.app.tabs[self.tab]
                .scene
                .set_reference_only_membership(*handle, !instance.selectable_custom);
        }
        if let Some(index) = stamp_failed {
            self.app.tabs[self.tab]
                .scene
                .erase_entities_plugin(&handles);
            self.app.sync_registered_layers(self.tab);
            self.bump_geometry();
            self.set_dirty();
            self.publish_document_view();
            // Close the staging scope before the error so follow-up plugin
            // writes cannot silently merge into it (review finding 8).
            if own_scope {
                self.app.finish_pending_history(self.tab);
            }
            return Err(format!(
                "stamping the staged instance of class {:?} failed at entity {:?}; the whole \
                 staged instance was rolled back",
                instance.class_id, handles[index]
            ));
        }
        self.bump_geometry();
        self.set_dirty();
        self.publish_document_view();
        if own_scope {
            self.app.finish_pending_history(self.tab);
        }
        Ok(handles)
    }

    /// Read a class instance's declared parameter rows (API v14, additive):
    /// the identity stamp (XDATA class key) resolves against the tab's
    /// registry, the design-record linkage resolves under the NOD (root
    /// pass, then one sub-dictionary level), and every declared parameter
    /// reads its (name, value) entry pair. Values print exactly what the
    /// record row carries â€” the host never invents units or semantics for a
    /// name or kind it does not declare. Strict on purpose (a broken
    /// linkage or a stale stamp refuses â€” the caller re-stages); the host's
    /// properties panel renders through the shared core directly, with the
    /// graceful declared-rows-with-absent-values view. The wire carries the
    /// read (`PluginRequest::PluginClassInstanceProperties` served through
    /// the [`HostApi`] impl here; the python `ocs` surface exposes it too).
    pub fn plugin_class_instance_properties(
        &self,
        handle: Handle,
    ) -> Result<Vec<PluginClassParameterRow>, String> {
        let binding = resolve_plugin_class_binding(self.app, self.tab, handle)?;
        let (_, entries) =
            find_design_record_entries(self.app, self.tab, handle, &binding.design_record)?;
        rows_from_entries(&binding, &entries)
    }

    /// Write one declared, editable parameter's (name, value) pair (API v14,
    /// additive): the design record's entries are patched in place through
    /// the session's XRecord update path — one undo step of its own (a
    /// scope-less call), the record marked dirty so the declaring plugin
    /// re-stages. Refusals — absent entity, no registered class key, a
    /// stale schema, an unknown or non-editable parameter, a value that
    /// does not parse per the declared kind, a record without the
    /// parameter's pair — all happen before any change.
    pub fn update_plugin_class_instance_parameter(
        &mut self,
        handle: Handle,
        name: &str,
        value: &str,
    ) -> Result<(), String> {
        let binding = resolve_plugin_class_binding(self.app, self.tab, handle)?;
        let parameter = binding
            .parameters
            .iter()
            .find(|p| p.name == name)
            .ok_or_else(|| {
                format!(
                    "class {:?} has no parameter named {:?}; the host never invents parameter \
                     semantics",
                    binding.class_id, name
                )
            })?;
        if !parameter.editable {
            return Err(format!(
                "parameter {name:?} of class {:?} is declared read-only; its row never writes",
                binding.class_id
            ));
        }
        let (path, entries) =
            find_design_record_entries(self.app, self.tab, handle, &binding.design_record)?;
        let entries = update_parameter_pair(&binding, name, parameter.value_kind, value, entries)?;
        let path: Vec<&str> = path.iter().map(String::as_str).collect();
        self.update_xrecord(&path, &binding.design_record, entries)?;
        Ok(())
    }
}

/// The resolved binding of an entity stamped as a plugin class instance
/// (API v14): the registered class, its live declared parameter schema, and
/// the design-record linkage the identity stamp carries.
pub(in crate::app) struct PluginClassInstanceBinding {
    pub class_id: String,
    pub parameters: Vec<ocs_plugin_api::host::PluginClassParameter>,
    pub design_record: String,
}

/// Resolve an entity stamped as a plugin class instance (API v14 core,
/// shared by the session pair and the properties panel): the first XDATA
/// record whose application name is a registered class key wins; its
/// Integer32 schema version and String design-record linkage read like
/// staging wrote them; a stale schema refuses (the record layer stays
/// authoritative — the stamp re-stages instead of rendering).
pub(in crate::app) fn resolve_plugin_class_binding(
    app: &OpenCADStudio,
    tab: usize,
    handle: document::Handle,
) -> Result<PluginClassInstanceBinding, String> {
    let Some(entity) = app.tabs[tab].scene.document.get_entity(handle) else {
        return Err(format!("entity {handle:?} does not exist"));
    };
    let registry = &app.tabs[tab].plugin_classes;
    for record in entity.common().extended_data.records() {
        if record.application_name.is_empty() {
            continue;
        }
        let Some(owner) = registry.owner_of(&record.application_name) else {
            continue;
        };
        let Some(declaration) = registry.find(owner, &record.application_name) else {
            continue;
        };
        let Some(document::xdata::XDataValue::Integer32(schema)) = record.values.first() else {
            return Err(format!(
                "entity {handle:?} carries the class stamp {:?} without its schema version",
                record.application_name
            ));
        };
        let schema = u32::try_from(*schema).map_err(|_| {
            format!(
                "entity {handle:?} carries class stamp {:?} with a version that is not a \
                 version number",
                record.application_name
            )
        })?;
        let design_record = match record.values.get(1) {
            Some(document::xdata::XDataValue::String(linkage)) => linkage.clone(),
            _ => String::new(),
        };
        if schema != declaration.schema_version {
            return Err(format!(
                "entity {handle:?} carries an instance of class {:?} authored for schema \
                 version {schema}, the session declares {}",
                declaration.class_id, declaration.schema_version
            ));
        }
        return Ok(PluginClassInstanceBinding {
            class_id: record.application_name.clone(),
            parameters: declaration.parameters.clone(),
            design_record,
        });
    }
    Err(format!(
        "entity {handle:?} carries no registered plugin class key in its XDATA"
    ))
}

/// Resolve the design record the binding links (API v14 core): NOD root
/// first, then one sub-dictionary level (a plugin keeps its records under
/// one) — the identity stamp names the record, never its
/// path, so the resolver searches rather than invents a layout. Returns the
/// dictionary path (usable by the XRecord update) and the entries.
pub(in crate::app) fn find_design_record_entries(
    app: &OpenCADStudio,
    tab: usize,
    _handle: document::Handle,
    name: &str,
) -> Result<(Vec<String>, Vec<document::objects::XRecordEntry>), String> {
    use document::objects::ObjectType;
    let name = validated_symbol_name(name, "a design record name")?;
    let doc = &app.tabs[tab].scene.document;
    let root = doc.header.named_objects_dict_handle;
    let is_dictionary = |candidate: document::Handle| {
        matches!(doc.objects.get(&candidate), Some(ObjectType::Dictionary(_)))
    };
    if is_dictionary(root) {
        let Some(ObjectType::Dictionary(root_dict)) = doc.objects.get(&root) else {
            unreachable!("checked by is_dictionary");
        };
        if let Some(record) = root_dict.get(&name) {
            return match doc.objects.get(&record) {
                Some(ObjectType::XRecord(xrecord)) => Ok((Vec::new(), xrecord.entries.clone())),
                _ => Err(format!("dictionary entry {name:?} is not an XRecord")),
            };
        }
        for (key, child) in &root_dict.entries {
            if !is_dictionary(*child) {
                continue;
            }
            let Some(ObjectType::Dictionary(sub)) = doc.objects.get(child) else {
                unreachable!("checked by is_dictionary");
            };
            if let Some(record) = sub.get(&name) {
                return match doc.objects.get(&record) {
                    Some(ObjectType::XRecord(xrecord)) => {
                        Ok((vec![key.clone()], xrecord.entries.clone()))
                    }
                    _ => Err(format!("dictionary entry {name:?} is not an XRecord")),
                };
            }
        }
    }
    Err(format!(
        "the design record {name:?} cannot be resolved from the NOD root or one level of its \
         sub-dictionaries; the identity stamp names the record, not its path"
    ))
}

/// The declared schema as rows with absent values (API v14 core, the
/// panel's base view: the host renders the declaration, the record fills
/// the values).
pub(in crate::app) fn rows_from_schema(
    binding: &PluginClassInstanceBinding,
) -> Vec<PluginClassParameterRow> {
    binding
        .parameters
        .iter()
        .map(|parameter| PluginClassParameterRow {
            name: parameter.name.clone(),
            value_kind: parameter.value_kind,
            editable: parameter.editable,
            value: None,
        })
        .collect()
}

/// The rows read the declared layout: alternating (String name, value)
/// entries from the record's first entry — the pair layout the declaration
/// owns; names the schema does not declare are the plugin's own pairs and
/// stay untouched by the reader.
pub(in crate::app) fn rows_from_entries(
    binding: &PluginClassInstanceBinding,
    entries: &[document::objects::XRecordEntry],
) -> Result<Vec<PluginClassParameterRow>, String> {
    use document::objects::XRecordValue;
    use ocs_plugin_api::host::PluginClassParameterKind;
    let mut rows = rows_from_schema(binding);
    let mut index = 0usize;
    while index + 1 < entries.len() {
        let document::objects::XRecordValue::String(pair_name) = &entries[index].value else {
            return Err(format!(
                "the design record of class {:?} breaks its alternating (name, value) pair \
                 layout at entry {index}",
                binding.class_id
            ));
        };
        let value_entry = &entries[index + 1];
        if let Some(row) = rows.iter_mut().find(|row| row.name == *pair_name) {
            let printed = match row.value_kind {
                PluginClassParameterKind::Real => match &value_entry.value {
                    XRecordValue::Double(v) => Some(v.to_string()),
                    XRecordValue::Int16(v) => Some(f64::from(*v).to_string()),
                    XRecordValue::Int32(v) => Some(f64::from(*v).to_string()),
                    XRecordValue::Int64(v) => Some((*v as f64).to_string()),
                    other => {
                        return Err(format!(
                            "parameter {:?} of class {:?} is declared a Real; the record row \
                             carries {other:?}",
                            row.name, binding.class_id
                        ))
                    }
                },
                PluginClassParameterKind::Integer => match &value_entry.value {
                    XRecordValue::Int16(v) => Some(i64::from(*v).to_string()),
                    XRecordValue::Int32(v) => Some(i64::from(*v).to_string()),
                    XRecordValue::Int64(v) => Some(v.to_string()),
                    other => {
                        return Err(format!(
                            "parameter {:?} of class {:?} is declared an Integer; the record \
                             row carries {other:?}",
                            row.name, binding.class_id
                        ))
                    }
                },
                PluginClassParameterKind::Text => match &value_entry.value {
                    XRecordValue::String(v) => Some(v.clone()),
                    other => {
                        return Err(format!(
                            "parameter {:?} of class {:?} is declared Text; the record row \
                             carries {other:?}",
                            row.name, binding.class_id
                        ))
                    }
                },
            };
            row.value = printed;
        }
        index += 2;
    }
    Ok(rows)
}

/// Patch one declared parameter's (name, value) pair in the design record's
/// entries (API v14 core): the value entry's group code and its neighbours
/// stay untouched; nothing is created in the plugin's record layer.
pub(in crate::app) fn update_parameter_pair(
    binding: &PluginClassInstanceBinding,
    name: &str,
    value_kind: ocs_plugin_api::host::PluginClassParameterKind,
    value: &str,
    entries: Vec<document::objects::XRecordEntry>,
) -> Result<Vec<document::objects::XRecordEntry>, String> {
    let Some(name_index) =
        find_parameter_pair(&binding.class_id, &binding.design_record, name, &entries)?
    else {
        return Err(format!(
            "the design record {:?} of class {:?} does not carry {:?}; the host creates \
             nothing in the plugin's record layer",
            binding.design_record, binding.class_id, name
        ));
    };
    let parsed = parse_parameter_value(value_kind, value, &binding.class_id, name)?;
    let value_index = name_index + 1;
    let code = entries[value_index].code;
    let mut entries = entries;
    entries[value_index] = document::objects::XRecordEntry::new(code, parsed);
    Ok(entries)
}

/// Locate the named pair in the alternating (name, value) layout: the index
/// of the name entry whose value entry follows. Duplicates and a name
/// without its value refuse.
fn find_parameter_pair(
    class_id: &str,
    record_name: &str,
    name: &str,
    entries: &[document::objects::XRecordEntry],
) -> Result<Option<usize>, String> {
    let mut found: Option<usize> = None;
    let mut index = 0usize;
    while index < entries.len() {
        let document::objects::XRecordValue::String(pair_name) = &entries[index].value else {
            return Err(format!(
                "the design record {record_name:?} of class {class_id:?} breaks its \
                 alternating (name, value) pair layout at entry {index}"
            ));
        };
        if pair_name == name {
            if found.is_some() {
                return Err(format!(
                    "the design record {record_name:?} of class {class_id:?} names {name:?} \
                     more than once"
                ));
            }
            if index + 1 >= entries.len() {
                return Err(format!(
                    "the design record {record_name:?} of class {class_id:?} carries {name:?} \
                     without its value pair"
                ));
            }
            found = Some(index);
        }
        index += 2;
    }
    Ok(found)
}

/// Parse one parameter value per its declared kind (API v14 core): Real =
/// a finite double, Integer = an integer carried Int32-when-it-fits,
/// Text = the string verbatim.
fn parse_parameter_value(
    value_kind: ocs_plugin_api::host::PluginClassParameterKind,
    value: &str,
    class_id: &str,
    name: &str,
) -> Result<document::objects::XRecordValue, String> {
    use document::objects::XRecordValue;
    use ocs_plugin_api::host::PluginClassParameterKind;
    match value_kind {
        PluginClassParameterKind::Real => {
            let parsed: f64 = value.trim().parse().map_err(|_| {
                format!(
                    "parameter {name:?} of class {class_id:?} needs a number; {value:?} does \
                     not parse"
                )
            })?;
            if !parsed.is_finite() {
                return Err(format!(
                    "parameter {name:?} of class {class_id:?} writes a finite number only"
                ));
            }
            Ok(XRecordValue::Double(parsed))
        }
        PluginClassParameterKind::Integer => {
            let parsed: i64 = value.trim().parse().map_err(|_| {
                format!(
                    "parameter {name:?} of class {class_id:?} needs an integer; {value:?} \
                     does not parse"
                )
            })?;
            if (i32::MIN as i64..=i32::MAX as i64).contains(&parsed) {
                Ok(XRecordValue::Int32(parsed as i32))
            } else {
                Ok(XRecordValue::Int64(parsed))
            }
        }
        PluginClassParameterKind::Text => Ok(XRecordValue::String(value.to_string())),
    }
}
