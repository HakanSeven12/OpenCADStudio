//! Plugin quad-panel session store (API v15).
//!
//! The quad cursor menu's contributing surface: an out-of-process plugin
//! registers declarative [`QuadPanelDef`]s once per session (`PluginRequest::
//! RegisterQuadPanels`), the host keeps them here keyed by the plugin's own
//! id, and every quad build merges the *visible* plugins' panels after the
//! builtin catalog (`quad_panels_for_target`'s `plugin_panels` argument).
//! Session-owned exactly like `src/plugin/grip_points.rs`' custom-grip
//! declarations — a plugin's panels live no longer than its registration.
//!
//! Laws (enforced here, one refusal reason each):
//!
//! - **Ownership**: one plugin owns a raw `panel_id` at a time. A batch
//!   replaces per-panel only for the same owner; a foreign id is refused
//!   ("panel-id belongs to a different plugin").
//! - **Caps**: at most [`MAX_QUAD_PANELS_PER_PLUGIN`] panels per plugin and
//!   [`MAX_QUAD_ITEMS_PER_PANEL`] items per panel (grip-cap style: honest and
//!   capped, the excess is refused, the accepted prefix stands).
//! - **Integrity**: ids must be non-empty and free of `:` (the `quadplugin:`
//!   dispatch scheme parses the triple verbatim), and control ids are unique
//!   inside one panel.
//! - **Namespacing**: the host's exposed quad panel id (the UI lookup key) is
//!   `quad_panel_ui_id` — `plugin:<plugin_id>:<panel_id>` — so plugin panels
//!   can never collide with the builtin `quad.*` panels; the store keeps the
//!   plugin's own raw ids.
//! - **Icon budget**: enforced at UI conversion, not here — the store keeps
//!   the declared icon bytes and the conversion (`plugin_quad_panels` in
//!   `src/app/plugin_host.rs`) degrades an oversized or unparseable icon to a
//!   glyph tile (§4 "Custom plugin icons": defensive, the panel survives).
//!
//! Liveness: the quad is app-global (like `quad_mru` — the menu follows the
//! user, not the drawing), so this store is a process-global instance rather
//! than a per-tab field; readers filter by the same visibility predicate the
//! grips apply (`grip_points::visibility` — loaded, alive, not disabled), so
//! a dead or disabled plugin's panels drop out of the quad the way its grips
//! do. `clear_plugin` / `retain_alive` exist for explicit teardown parity.

use std::sync::OnceLock;

use ocs_plugin_api::dialog::{QuadActionDef, QuadPanelDef};
use rustc_hash::FxHashMap;

/// Panels a single plugin may register (grip-cap style budget).
pub(crate) const MAX_QUAD_PANELS_PER_PLUGIN: usize = 16;

/// Items one quad panel may contain.
pub(crate) const MAX_QUAD_ITEMS_PER_PANEL: usize = 64;

/// Icon bytes a quad item may carry before the UI conversion falls back to
/// the glyph tile (§4 "Custom plugin icons": ≤ 8 KiB per icon).
pub(crate) const MAX_QUAD_ICON_BYTES: usize = 8 * 1024;

/// The UI-side unique id for a plugin panel: the lookup key the quad model
/// uses ([`crate::ui::popup::quad_menu`]) and the `plugin` segment of every
/// `quadplugin:` command the model's items dispatch.
pub(crate) fn quad_panel_ui_id(plugin_id: &str, panel_id: &str) -> String {
    format!("plugin:{plugin_id}:{panel_id}")
}

/// Parse a `quadplugin:` command into its `(plugin_id, panel_id, control_id)`
/// triple. Exactly three non-empty segments ride the scheme — excess colons,
/// an empty segment or a non-`quadplugin:` line refuse (`None`); the pick
/// path (`app::commands` `quad_plugin_action`) is the only caller, so a
/// malformed string is never a dispatchable command.
pub(crate) fn split_action(cmd: &str) -> Option<(String, String, String)> {
    let rest = cmd.strip_prefix(crate::plugin::QUAD_PLUGIN_PREFIX)?;
    let segments = rest.split(':').collect::<Vec<&str>>();
    let [plugin_id, panel_id, control_id] = segments.as_slice() else {
        return None;
    };
    if plugin_id.is_empty() || panel_id.is_empty() || control_id.is_empty() {
        return None;
    }
    Some((
        (*plugin_id).to_string(),
        (*panel_id).to_string(),
        (*control_id).to_string(),
    ))
}

#[derive(Debug, Default)]
pub(crate) struct QuadStore {
    /// Registration-order plugins (the quad merges tabs in registration
    /// order, so a `Vec` keeps `FxHashMap`'s nondeterministic iteration out
    /// of the user-facing order).
    entries: Vec<PluginQuadPanels>,
    /// Raw panel id → owning plugin id (the ownership index).
    owners: FxHashMap<String, String>,
}

#[derive(Debug)]
pub(crate) struct PluginQuadPanels {
    plugin_id: String,
    panels: Vec<QuadPanelDef>,
}

impl QuadStore {
    /// Register a batch for `plugin_id`, one outcome per panel in order.
    /// A panel that replaces the owner's previous declaration keeps that
    /// owner's slot; any refusal is per-panel.
    pub(crate) fn register(&mut self, plugin_id: &str, panels: &[QuadPanelDef]) -> Vec<bool> {
        let mut results = Vec::with_capacity(panels.len());
        for panel in panels {
            results.push(self.register_one(plugin_id, panel));
        }
        results
    }

    /// Remove the plugin's named panels, one outcome per id in order. The
    /// ownership law runs both ways (mirroring [`Self::register`]): an id
    /// another plugin owns is refused, not removed.
    pub(crate) fn unregister(&mut self, plugin_id: &str, panel_ids: &[String]) -> Vec<bool> {
        panel_ids
            .iter()
            .map(|panel_id| {
            if self.owners.get(panel_id).map(String::as_str) != Some(plugin_id) {
                return false;
            }
            self.remove_owned(plugin_id, panel_id)
        })
        .collect()
    }

    /// The stored action definition behind one `quadplugin:` item, if the
    /// triple is registered. NO visibility filtering here — the pick path
    /// guards liveness itself with the same predicate the quad build applies.
    pub(crate) fn resolve(
        &self,
        plugin_id: &str,
        panel_id: &str,
        control_id: &str,
    ) -> Option<&QuadActionDef> {
        let entry = self.entries.iter().find(|entry| entry.plugin_id == plugin_id)?;
        let panel = entry
            .panels
            .iter()
            .find(|panel| panel.panel_id == panel_id)?;
        panel
            .items
            .iter()
            .find(|item| item.control_id == control_id)
            .map(|item| &item.action)
    }

    /// Which plugin owns `panel_id`, if any is registered.
    pub(crate) fn owner_of(&self, panel_id: &str) -> Option<&str> {
        self.owners.get(panel_id).map(String::as_str)
    }

    /// Remove every panel of `plugin_id` (teardown parity with grip/panel
    /// state). `true` if the plugin had panels at all.
    pub(crate) fn clear_plugin(&mut self, plugin_id: &str) -> bool {
        let Some(position) = self.entries.iter().position(|e| e.plugin_id == plugin_id) else {
            return false;
        };
        let entry = self.entries.remove(position);
        for panel in &entry.panels {
            self.owners.remove(&panel.panel_id);
        }
        true
    }

    /// Reap the stores of plugins that died or were disabled (`retain_alive`
    /// parity with grips/classes). `true` if anything was removed.
    pub(crate) fn retain_alive(&mut self, mut alive: impl FnMut(&str) -> bool) -> bool {
        let mut changed = false;
        let mut index = 0;
        while index < self.entries.len() {
            if alive(self.entries[index].plugin_id.as_str()) {
                index += 1;
            } else {
                let plugin_id = self.entries[index].plugin_id.clone();
                changed |= self.clear_plugin(&plugin_id);
            }
        }
        changed
    }

    /// Every *visible* plugin's panels in registration order — the feed for
    /// the quad model's plugin merge (builtin panels first, then plugins in
    /// registration order). Yields the owning plugin's id beside each panel:
    /// the conversion derives the namespaced UI id and the `quadplugin:`
    /// command from the pair.
    pub(crate) fn panels_for_visible(
        &self,
        visible: impl Fn(&str) -> bool,
    ) -> Vec<(String, QuadPanelDef)> {
        self.entries
            .iter()
            .filter(|entry| visible(entry.plugin_id.as_str()))
            .flat_map(|entry| {
                entry
                    .panels
                    .iter()
                    .cloned()
                    .map(move |panel| (entry.plugin_id.clone(), panel))
            })
            .collect()
    }

    /// One panel register/replace for `plugin_id`. The refusals, in order:
    /// an empty or `:`-carrying id (the `quadplugin:` scheme is the delivery
    /// contract), an id another plugin owns ("panel-id belongs to a
    /// different plugin"), over-budget items, duplicate or empty control ids
    /// inside the panel, and the per-plugin panel cap.
    fn register_one(&mut self, plugin_id: &str, panel: &QuadPanelDef) -> bool {
        if panel.panel_id.is_empty()
            || panel.panel_id.contains(':')
            || panel.items.iter().any(|item| item.control_id.contains(':'))
        {
            return false;
        }
        if self
            .owners
            .get(&panel.panel_id)
            .is_some_and(|owner| owner != plugin_id)
        {
            return false;
        }
        if panel.items.len() > MAX_QUAD_ITEMS_PER_PANEL {
            return false;
        }
        let mut seen = rustc_hash::FxHashSet::default();
        for item in &panel.items {
            if item.control_id.is_empty() || !seen.insert(item.control_id.as_str()) {
                return false;
            }
        }
        let entry = match self
            .entries
            .iter_mut()
            .find(|entry| entry.plugin_id == plugin_id)
        {
            Some(entry) => entry,
            None => {
                self.entries.push(PluginQuadPanels {
                    plugin_id: plugin_id.to_string(),
                    panels: Vec::new(),
                });
                self.entries.last_mut().expect("entry just pushed")
            }
        };
        if let Some(position) = entry
            .panels
            .iter()
            .position(|owned| owned.panel_id == panel.panel_id)
        {
            // Replace-if-same-owner keeps the owner's slot stable.
            entry.panels[position] = panel.clone();
        } else {
            if entry.panels.len() >= MAX_QUAD_PANELS_PER_PLUGIN {
                return false;
            }
            entry.panels.push(panel.clone());
        }
        self.owners
            .insert(panel.panel_id.clone(), plugin_id.to_string());
        true
    }

    /// Remove one owned panel id from the owner's entry and the index.
    fn remove_owned(&mut self, plugin_id: &str, panel_id: &str) -> bool {
        let Some(entry) = self
            .entries
            .iter_mut()
            .find(|entry| entry.plugin_id == plugin_id)
        else {
            return false;
        };
        let Some(position) = entry
            .panels
            .iter()
            .position(|owned| owned.panel_id == panel_id)
        else {
            return false;
        };
        entry.panels.remove(position);
        self.owners.remove(panel_id);
        if entry.panels.is_empty() {
            let plugin_id = entry.plugin_id.clone();
            let _ = self.clear_plugin(&plugin_id);
        }
        true
    }
}

// ── Process-global holder ────────────────────────────────────────────────────
//
// The quad is app-global (`quad_mru` precedent): one store for every tab, the
// visibility predicate decides who contributes at build time. Locking is
// short (store mutations only) — the main thread is the only writer and
// reader; the lock is a correctness backstop, not a concurrency surface.

static QUAD_STORE: OnceLock<std::sync::Mutex<QuadStore>> = OnceLock::new();

/// Run `f` against the process-global quad store.
pub(crate) fn with_store<R>(f: impl FnOnce(&mut QuadStore) -> R) -> R {
    let store = QUAD_STORE.get_or_init(|| std::sync::Mutex::new(QuadStore::default()));
    let mut guard = store.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

/// Reap the process-global store's dead/disabled owners — the plugin
/// lifecycle pump's call, wired next to the grips/classes reaping in
/// `src/app/update/plugin_grips.rs` (`retain_alive` parity). `true` if
/// anything was removed.
pub(crate) fn retain_alive(alive: impl Fn(&str) -> bool) -> bool {
    with_store(|store| store.retain_alive(alive))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::dialog::{QuadActionDef, QuadItemDef};
    use ocs_plugin_api::ribbon::owned::OwnedIconKind;

    /// A two-item panel with unique control ids and a glyph icon.
    fn panel(id: &str, controls: &[&str]) -> QuadPanelDef {
        let mut def = QuadPanelDef::new(id, id.to_ascii_uppercase());
        for control in controls {
            def = def.with_item(QuadItemDef {
                control_id: (*control).to_string(),
                label: control.to_string(),
                icon: OwnedIconKind::Glyph(control.to_string()),
                action: QuadActionDef::Callback,
            });
        }
        def
    }

    #[test]
    fn registration_is_per_panel_with_owner_tracking() {
        let mut store = QuadStore::default();
        let ok = store.register("A", &[panel("align", &["flip", "report"])]);
        assert_eq!(ok, vec![true]);
        assert_eq!(store.owner_of("align"), Some("A"));

        // Replace-if-same-owner keeps the owner's slot stable.
        let ok = store.register("A", &[panel("align", &["flip"])]);
        assert_eq!(ok, vec![true]);
        assert_eq!(store.owner_of("align"), Some("A"));
        let mine = store.panels_for_visible(|id| id == "A");
        assert_eq!(mine[0].1.panel_id, "align");
        assert_eq!(mine[0].1.items.len(), 1);

        // A different owner's batch cannot replace A's panel…
        let ok = store.register("B", &[panel("align", &["x"])]);
        assert_eq!(ok, vec![false]);
        assert_eq!(store.owner_of("align"), Some("A"));
        // …but a new panel of its own is accepted.
        let ok = store.register("B", &[panel("bolts", &["x"])]);
        assert_eq!(ok, vec![true]);
        assert_eq!(store.owner_of("bolts"), Some("B"));
        assert_eq!(store.panels_for_visible(|id| id == "B").len(), 1);

        // Registration order is the merge order (Vec, not a hash map).
        let visible = store.panels_for_visible(|_| true);
        assert_eq!(visible[0].0, "A");
        assert_eq!(visible[1].0, "B");
        assert_eq!(visible[0].1.panel_id, "align");
    }

    #[test]
    fn caps_refuse_extra_panels_and_oversized_items() {
        let mut store = QuadStore::default();
        let batch: Vec<QuadPanelDef> = (0..MAX_QUAD_PANELS_PER_PLUGIN)
            .map(|index| panel(&format!("p{index}"), &["a"]))
            .collect();
        assert_eq!(store.register("A", &batch), vec![true; MAX_QUAD_PANELS_PER_PLUGIN]);
        // The cap refuses new panels but never removes the accepted prefix.
        let ok = store.register("A", &[panel("over", &["a"])]);
        assert_eq!(ok, vec![false]);
        assert_eq!(
            store.panels_for_visible(|id| id == "A").len(),
            MAX_QUAD_PANELS_PER_PLUGIN
        );
        assert_eq!(store.owner_of("over"), None);

        // Item budget: one item over the cap refuses the whole panel.
        let mut oversized = panel("big", &[]);
        for index in 0..=MAX_QUAD_ITEMS_PER_PANEL {
            oversized = oversized.with_item(QuadItemDef {
                control_id: format!("c{index}"),
                label: "Item".to_string(),
                icon: OwnedIconKind::Glyph("I".to_string()),
                action: QuadActionDef::Callback,
            });
        }
        let ok = store.register("B", &[oversized]);
        assert_eq!(ok, vec![false]);
        assert_eq!(store.owner_of("big"), None);
    }

    #[test]
    fn integrity_rules_refuse_colons_empty_ids_and_duplicate_controls() {
        let mut store = QuadStore::default();
        // A `:` inside an id cannot ride the `quadplugin:` scheme verbatim.
        assert_eq!(store.register("A", &[panel("we:ird", &["a"])]), vec![false]);
        assert_eq!(store.register("A", &[panel("", &["a"])]), vec![false]);
        assert_eq!(store.register("A", &[panel("ok", &["a:b"])]), vec![false]);
        assert_eq!(store.register("A", &[panel("ok", &[""])]), vec![false]);
        // Duplicate control ids inside one panel refuse the panel.
        assert_eq!(
            store.register("A", &[panel("dup", &["flip", "flip"])]),
            vec![false]
        );
        // Nothing was stored under any of the attempted ids.
        assert!(store.panels_for_visible(|_| true).is_empty());

        // The namespacing guard: the exposed lookup key is namespaced with
        // the plugin id, so plugin panels can never collide with the
        // builtin `quad.*` ids regardless of what a plugin names its panels.
        assert_eq!(quad_panel_ui_id("opencad.python", "align"), "plugin:opencad.python:align");
    }

    #[test]
    fn split_action_parses_strict_triples() {
        assert_eq!(
            split_action("quadplugin:opencad.python:align:flip"),
            Some((
                "opencad.python".to_string(),
                "align".to_string(),
                "flip".to_string()
            ))
        );
        // Excess colons, empty segments and non-scheme lines refuse.
        assert_eq!(split_action("quadplugin:a:b:c:d"), None);
        assert_eq!(split_action("quadplugin:a::c"), None);
        assert_eq!(split_action("quadplugin::b:c"), None);
        assert_eq!(split_action("quadplugin:a:b:"), None);
        assert_eq!(split_action("quadplugin:a"), None);
        assert_eq!(split_action("quadplugin:"), None);
        assert_eq!(split_action("LINE"), None);
        assert_eq!(split_action(""), None);
    }

    #[test]
    fn resolve_maps_registered_triples_and_refuses_absent() {
        let mut store = QuadStore::default();
        store.register("A", &[panel("align", &["flip"])]);
        assert_eq!(
            store.resolve("A", "align", "flip"),
            Some(&QuadActionDef::Callback)
        );
        // Absent plugin / panel / control refuse: an unregistered triple is
        // not a dispatchable action.
        assert_eq!(store.resolve("B", "align", "flip"), None);
        assert_eq!(store.resolve("A", "nope", "flip"), None);
        assert_eq!(store.resolve("A", "align", "nope"), None);
        // After the owner unregisters, the triple resolves to nothing.
        assert_eq!(store.unregister("A", &["align".to_string()]), vec![true]);
        assert_eq!(store.resolve("A", "align", "flip"), None);
    }

    #[test]
    fn unregister_is_ownership_scoped_and_clear_reaps() {
        let mut store = QuadStore::default();
        store.register("A", &[panel("align", &["flip"]), panel("dims", &["d"])]);
        store.register("B", &[panel("bolts", &["x"])]);

        // Another plugin's id is refused, not removed.
        let ok = store.unregister("B", &["align".to_string()]);
        assert_eq!(ok, vec![false]);
        assert_eq!(store.owner_of("align"), Some("A"));

        // Unknown ids are refused too.
        let ok = store.unregister("A", &["never".to_string()]);
        assert_eq!(ok, vec![false]);

        // The owner removes its own panels; the empty entry is dropped.
        let ok = store.unregister("A", &["align".to_string(), "dims".to_string()]);
        assert_eq!(ok, vec![true, true]);
        assert_eq!(store.owner_of("align"), None);
        assert!(store.panels_for_visible(|id| id == "A").is_empty());
        assert_eq!(store.panels_for_visible(|_| true).len(), 1);

        // clear_plugin mirrors the per-plugin teardown; retain_alive prunes
        // everything the visibility predicate fails (the reaper's store-law
        // twin).
        store.clear_plugin("B");
        assert!(store.panels_for_visible(|id| id == "B").is_empty());
        let _ = store.register("C", &[panel("c", &["x"])]);
        assert!(store.retain_alive(|id| id != "C"));
        assert!(store.panels_for_visible(|_| true).is_empty());
        assert!(!store.retain_alive(|_| true));
    }

    #[test]
    fn visibility_filters_the_model_feed() {
        let mut store = QuadStore::default();
        store.register("A", &[panel("align", &["flip"])]);
        store.register("B", &[panel("bolts", &["x"])]);
        // A dead plugin drops out of the feed; a live one stays.
        let feed = store.panels_for_visible(|id| id == "A");
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].1.panel_id, "align");
    }
}
