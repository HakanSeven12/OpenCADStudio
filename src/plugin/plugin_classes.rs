//! Plugin-declared entity class registry (API v14).
//!
//! One rule, copied from the grip sets: plugin-DECLARE, plugin-OWN,
//! tab-scoped display-session state. A class entry dies with its owning
//! plugin's process and is re-declared on load; the drawing file keeps only
//! what the staged instances carry (the entities' XDATA identity stamps and
//! the record linkages) plus the plugin's own record-layer rows. Nothing in
//! this store is ever persisted — unlike the grips there is no spatial
//! index either: the registry is a small per-plugin declaration map keyed by
//! stable class ids, O(classes of the tab) at lookup and bounded by cap.

use ocs_plugin_api::host::PluginClassDeclaration;
use rustc_hash::FxHashMap;

/// Cap on declared classes per plugin (the grip-store cap pattern: an honest
/// bound so an unbounded declaration loop cannot silently grow host state).
pub(crate) const MAX_PLUGIN_CLASSES_PER_PLUGIN: usize = 256;
/// Total cap across all plugins on one tab.
pub(crate) const MAX_PLUGIN_CLASSES_PER_TAB: usize = 2048;

/// Plugin-owned class registrations of one tab (display-session state only).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PluginClassStore {
    entries: FxHashMap<String, Vec<PluginClassDeclaration>>,
}

impl PluginClassStore {
    /// The whole registration set of `plugin`: idempotent for an equal schema,
    /// refusing a mismatch so a schema change re-stages instances instead of
    /// hot-swapping under live entities.
    pub(crate) fn declare(
        &mut self,
        plugin: &str,
        class: PluginClassDeclaration,
    ) -> Result<(), String> {
        if self.total() >= MAX_PLUGIN_CLASSES_PER_TAB {
            return Err(format!(
                "the tab declares {MAX_PLUGIN_CLASSES_PER_TAB} plugin classes already; the \
                 per-tab cap is reached"
            ));
        }
        let set = match self.entries.get_mut(plugin) {
            Some(set) => set,
            None => {
                self.entries.insert(plugin.to_string(), Vec::new());
                self.entries.get_mut(plugin).expect("just inserted")
            }
        };
        if set.len() >= MAX_PLUGIN_CLASSES_PER_PLUGIN {
            return Err(format!(
                "plugin {plugin} declares {} classes already; the per-plugin cap is \
                 {MAX_PLUGIN_CLASSES_PER_PLUGIN}",
                set.len()
            ));
        }
        if let Some(existing) = set
            .iter()
            .find(|entry| entry.class_id == class.class_id)
        {
            if existing.schema_version == class.schema_version
                && existing.grip_schema == class.grip_schema
                && existing.parameters == class.parameters
            {
                return Ok(());
            }
            return Err(format!(
                "class {class_id:?} is already registered with a different schema ({version}); \
                 re-stage its instances instead",
                class_id = class.class_id,
                version = existing.schema_version
            ));
        }
        set.push(class);
        Ok(())
    }

    /// The owning plugin's declaration, or `None` (the caller then refuses —
    /// staging requires the caller to BE the owner).
    pub(crate) fn find(
        &self,
        plugin: &str,
        class_id: &str,
    ) -> Option<&PluginClassDeclaration> {
        self.entries
            .get(plugin)?
            .iter()
            .find(|entry| entry.class_id == class_id)
    }

    /// Which plugin (if any) holds this class, for refusals that can afford
    /// honesty about ownership.
    pub(crate) fn owner_of(&self, class_id: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(_, set)| set.iter().any(|entry| entry.class_id == class_id))
            .map(|(plugin, _)| plugin.as_str())
    }

    pub(crate) fn total(&self) -> usize {
        self.entries.values().map(|set| set.len()).sum()
    }

    /// Reap every plugin the predicate reports as gone; returns whether
    /// anything changed (the grip-store retention pump's pattern).
    pub(crate) fn retain_alive(&mut self, alive: impl Fn(&str) -> bool) -> bool {
        let total = self.total();
        self.entries.retain(|plugin, _| alive(plugin));
        self.total() != total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class(id: &str, version: u32) -> PluginClassDeclaration {
        PluginClassDeclaration {
            class_id: id.to_string(),
            schema_version: version,
            grip_schema: vec![],
            parameters: vec![],
        }
    }

    #[test]
    fn ids_are_refusing_or_idempotent() {
        let mut store = PluginClassStore::default();
        store.declare("p", class("road", 4)).expect("first declare");
        assert_eq!(store.total(), 1);
        // A re-declare of the same schema is idempotent.
        assert!(store.declare("p", class("road", 4)).is_ok());
        assert_eq!(store.total(), 1);
        // A different schema version refuses without touching the live entry.
        let err = store.declare("p", class("road", 5)).unwrap_err();
        assert!(err.contains("different schema"), "got: {err}");
        assert_eq!(store.find("p", "road").expect("live entry").schema_version, 4);
        // Another plugin cannot see (and so cannot stage) the class, but a
        // refusal can name the honest owner.
        assert!(store.find("other", "road").is_none());
        assert_eq!(store.owner_of("road"), Some("p"));
    }

    #[test]
    fn caps_and_reaping() {
        let mut store = PluginClassStore::default();
        for i in 0..MAX_PLUGIN_CLASSES_PER_PLUGIN {
            store
                .declare("full", class(&format!("class-{i}"), 1))
                .expect("within the per-plugin cap");
        }
        assert!(store.declare("full", class("one-too-many", 1)).is_err());
        store.declare("lean", class("lean", 1)).expect("other plugin");
        assert_eq!(store.total(), MAX_PLUGIN_CLASSES_PER_PLUGIN + 1);
        // Retention drops the dead plugin's whole set and reports the change.
        assert!(store.retain_alive(|p| p != "full"));
        assert_eq!(store.total(), 1);
        assert!(store.find("lean", "lean").is_some());
        assert!(!store.retain_alive(|_| true), "nothing to reap");
    }
}
