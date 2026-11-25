//! Tab-owned drawing resources and occurrence reports. Never serialized.
use crate::ifccad::{ExternalSourceBytes, IfccadSession};
use document::{Handle, IfccadRoundtripContext};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct ExternalReferenceRow {
    pub key: u64,
    pub parent_key: u64,
    pub name: String,
    pub locator: String,
    pub found_at: Option<String>,
    pub status: String,
    pub diagnostic: Option<String>,
    pub overlay: bool,
    pub owner_resource: Option<u64>,
    pub source_id: u64,
}

#[derive(Clone, Debug)]
pub struct ExternalDrawingSnapshot {
    pub resource_path: String,
    pub lookup_path: std::path::PathBuf,
    pub document: document::Document,
    pub context: Option<IfccadRoundtripContext>,
    pub content: Option<document::ifccad::IfccadExternalSourceContent>,
}

#[derive(Clone, Debug)]
pub(crate) struct XrefSourceRegistry {
    next_id: u64,
    generation: u64,
    resources: BTreeMap<u64, ExternalSourceBytes>,
    bindings: BTreeMap<(u64, u64, String), u64>,
    contexts: BTreeMap<u64, IfccadRoundtripContext>,
    sources: BTreeMap<(u64, u64), document::ifccad::IfccadExternalSource>,
    snapshots: BTreeMap<String, Arc<ExternalDrawingSnapshot>>,
    rows: BTreeMap<Handle, Vec<ExternalReferenceRow>>,
}

impl Default for XrefSourceRegistry {
    fn default() -> Self {
        Self {
            next_id: 2,
            generation: 0,
            resources: BTreeMap::new(),
            bindings: BTreeMap::new(),
            contexts: BTreeMap::new(),
            sources: BTreeMap::new(),
            snapshots: BTreeMap::new(),
            rows: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct SourceRegistryCheckpoint {
    state: XrefSourceRegistry,
    guids: BTreeMap<document::ifccad::IfccadExternalSourceId, String>,
}

impl IfccadSession {
    pub fn invalidate_source_generation(&self) {
        if let Ok(mut state) = self.2.lock() {
            state.generation = state.generation.saturating_add(1);
        }
    }
    pub fn source_generation(&self) -> u64 {
        self.2.lock().map(|s| s.generation).unwrap_or(0)
    }
    pub fn source_snapshot(&self, key: &str) -> Option<Arc<ExternalDrawingSnapshot>> {
        self.2.lock().ok()?.snapshots.get(key).cloned()
    }
    pub fn remember_source_snapshot(&self, key: String, source: ExternalDrawingSnapshot) {
        if let Ok(mut state) = self.2.lock() {
            state.snapshots.insert(key, Arc::new(source));
        }
    }
    pub fn reset_resource_sources(&self, owner: u64) {
        if let Ok(mut state) = self.2.lock() {
            state.sources.retain(|(id, _), _| *id != owner);
        }
    }
    pub fn source_registry_checkpoint(&self) -> Result<SourceRegistryCheckpoint, String> {
        self.2
            .lock()
            .map(|state| SourceRegistryCheckpoint {
                state: state.clone(),
                guids: self.3.lock().map(|g| g.clone()).unwrap_or_default(),
            })
            .map_err(|_| "External sources unavailable".into())
    }
    pub fn restore_source_registry(&self, checkpoint: SourceRegistryCheckpoint) {
        if let Ok(mut state) = self.2.lock() {
            let next = state.next_id;
            let generation = state.generation;
            *state = checkpoint.state;
            state.next_id = state.next_id.max(next);
            state.generation = generation.saturating_add(1);
            if let Ok(mut guids) = self.3.lock() {
                *guids = checkpoint.guids;
            }
        }
    }
    pub fn source_resource_choices(&self) -> Vec<(u64, String)> {
        let mut choices = vec![(1, "Current drawing".into())];
        if let Ok(state) = self.2.lock() {
            choices.extend(
                state
                    .resources
                    .iter()
                    .map(|(id, r)| (*id, r.file_name.clone())),
            );
        }
        choices
    }

    pub fn source_resource_bytes(&self, resource: u64) -> Option<ExternalSourceBytes> {
        self.2.lock().ok()?.resources.get(&resource).cloned()
    }
    pub fn resource_binding(
        &self,
        owner: u64,
        source: u64,
        locator: &str,
    ) -> Option<ExternalSourceBytes> {
        let state = self.2.lock().ok()?;
        let resource = state.bindings.get(&(owner, source, locator.to_owned()))?;
        if *resource == 1 {
            return Some(ExternalSourceBytes {
                resource_id: 1,
                file_name: "Current drawing".into(),
                bytes: Arc::new(Vec::new()),
            });
        }
        state.resources.get(resource).cloned()
    }

    pub fn bind_resource_bytes(
        &self,
        owner: u64,
        source: u64,
        locator: &str,
        file_name: String,
        bytes: Arc<Vec<u8>>,
    ) -> Result<u64, String> {
        let mut state = self.2.lock().map_err(|_| "External sources unavailable")?;
        let key = (owner, source, locator.to_owned());
        let id = state
            .bindings
            .get(&key)
            .copied()
            .filter(|id| *id != 1)
            .unwrap_or_else(|| {
                let id = state.next_id;
                state.next_id += 1;
                id
            });
        state.resources.insert(
            id,
            ExternalSourceBytes {
                resource_id: id,
                file_name,
                bytes,
            },
        );
        state.bindings.insert(key, id);
        state.generation = state.generation.saturating_add(1);
        Ok(id)
    }

    pub fn reuse_source_resource(
        &self,
        owner: u64,
        source: u64,
        locator: &str,
        resource: u64,
    ) -> Result<(), String> {
        let mut state = self.2.lock().map_err(|_| "External sources unavailable")?;
        if resource != 1 && !state.resources.contains_key(&resource) {
            return Err("Resource does not belong to this drawing tab".into());
        }
        state
            .bindings
            .insert((owner, source, locator.into()), resource);
        state.generation = state.generation.saturating_add(1);
        Ok(())
    }

    pub fn remember_resource_context(&self, resource: u64, context: IfccadRoundtripContext) {
        if let Ok(mut state) = self.2.lock() {
            state.contexts.insert(resource, context);
        }
    }

    pub fn remember_resource_source(
        &self,
        owner: u64,
        source: document::ifccad::IfccadExternalSource,
    ) {
        if let Ok(mut state) = self.2.lock() {
            state.sources.insert((owner, source.id.0), source);
        }
    }

    pub fn resource_source(
        &self,
        owner: u64,
        source: u64,
    ) -> Option<document::ifccad::IfccadExternalSource> {
        if owner == 1 {
            return self.source_by_id(source);
        }
        let state = self.2.lock().ok()?;
        state.sources.get(&(owner, source)).cloned().or_else(|| {
            state
                .contexts
                .get(&owner)?
                .external_source_definitions()
                .find(|(_, s)| s.id.0 == source)
                .map(|(_, s)| s.clone())
        })
    }

    pub fn set_nested_source_rows(&self, root: Handle, rows: Vec<ExternalReferenceRow>) {
        if let Ok(mut state) = self.2.lock() {
            state.rows.insert(root, rows);
        }
    }

    pub fn nested_source_rows(&self) -> Vec<(Handle, ExternalReferenceRow)> {
        self.2
            .lock()
            .map(|state| {
                state
                    .rows
                    .iter()
                    .flat_map(|(h, rows)| rows.iter().cloned().map(|row| (*h, row)))
                    .collect()
            })
            .unwrap_or_default()
    }
}
