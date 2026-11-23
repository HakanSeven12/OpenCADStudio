//! Stored panel scenes (api v29). The caller resolves ownership and
//! attachment; this store applies the revision rules of a publish and keeps
//! no renderer state.

use ocs_plugin_api::scene::{PanelScene, SceneRevision, SurfaceReject, SurfaceRejectCode};

/// Stored surface per panel id: strictly one authoritative record per
/// panel, owned by the authenticated plugin that published it. A panel
/// owns ONE scene AND one widget surface side by side (finding F1: the
/// two kinds advance on independent revisions).
#[derive(Debug, Default)]
pub struct PanelSurfaceStore {
    scenes: std::collections::BTreeMap<String, SceneRecord>,
    widgets: std::collections::BTreeMap<String, WidgetRecord>,
    tables: std::collections::BTreeMap<String, TableRecord>,
}

#[derive(Debug, Clone, PartialEq)]
struct TableRecord {
    owner_plugin_id: String,
    surface: ocs_plugin_api::tables::TableSurface,
}

#[derive(Debug, Clone, PartialEq)]
struct SceneRecord {
    owner_plugin_id: String,
    scene: PanelScene,
}

#[derive(Debug, Clone, PartialEq)]
struct WidgetRecord {
    owner_plugin_id: String,
    surface: ocs_plugin_api::widget::WidgetSurface,
}

impl PanelSurfaceStore {
    /// Publish a scene for `panel_id` from the authenticated `plugin_id`.
    /// The caller has already resolved attachment (the panel is registered)
    /// and transport identity; the store re-checks ownership so nothing can
    /// bypass it, applies the revision semantics, and answers the stored
    /// revision or a typed refusal. No partial state on refusal.
    pub fn publish(
        &mut self,
        plugin_id: &str,
        panel_id: &str,
        scene: PanelScene,
    ) -> Result<SceneRevision, SurfaceReject> {
        if plugin_id.is_empty() || panel_id.trim().is_empty() {
            return Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                current_revision: None,
            });
        }
        if scene.validate().is_err() {
            return Err(SurfaceReject {
                code: SurfaceRejectCode::InvalidSurface,
                current_revision: None,
            });
        }
        if let Some(record) = self.scenes.get(panel_id) {
            if record.owner_plugin_id != plugin_id {
                return Err(SurfaceReject {
                    code: SurfaceRejectCode::OwnedByOther,
                    current_revision: Some(record.scene.revision),
                });
            }
            let head = record.scene.revision;
            if scene.revision == head {
                if scene == record.scene {
                    return Ok(head); // idempotent: identical at the head
                }
                return Err(SurfaceReject {
                    code: SurfaceRejectCode::NonMonotonicRevision,
                    current_revision: Some(head),
                });
            }
            if scene.revision < head {
                return Err(SurfaceReject {
                    code: SurfaceRejectCode::NonMonotonicRevision,
                    current_revision: Some(head),
                });
            }
        }
        let revision = scene.revision;
        self.scenes.insert(
            panel_id.to_string(),
            SceneRecord {
                owner_plugin_id: plugin_id.to_string(),
                scene,
            },
        );
        Ok(revision)
    }

    /// Apply a bounded, revision-checked patch to the stored scene
    /// atomically. The patch must be based on the stored head
    /// (`SceneError::StaleBase` maps to `NonMonotonicRevision` with the
    /// head attached, so the publisher can rebase); any op failure maps
    /// to `InvalidSurface` and leaves the stored scene untouched. Only a
    /// previously published surface can be patched.
    pub fn patch(
        &mut self,
        plugin_id: &str,
        panel_id: &str,
        patch: ocs_plugin_api::scene::ScenePatch,
    ) -> Result<SceneRevision, SurfaceReject> {
        use ocs_plugin_api::scene::SceneError;
        if plugin_id.is_empty() || panel_id.trim().is_empty() {
            return Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                current_revision: None,
            });
        }
        let Some(record) = self.scenes.get_mut(panel_id) else {
            return Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                current_revision: None,
            });
        };
        if record.owner_plugin_id != plugin_id {
            return Err(SurfaceReject {
                code: SurfaceRejectCode::OwnedByOther,
                current_revision: Some(record.scene.revision),
            });
        }
        match patch.apply_to(&mut record.scene) {
            Ok(revision) => Ok(revision),
            Err(SceneError::StaleBase { current }) => Err(SurfaceReject {
                code: SurfaceRejectCode::NonMonotonicRevision,
                current_revision: Some(current),
            }),
            Err(_) => Err(SurfaceReject {
                code: SurfaceRejectCode::InvalidSurface,
                current_revision: None,
            }),
        }
    }

    /// Ownership-checked read (api v29): a scene reads back only for its
    /// authenticated owner. `None` for unknown panels, foreign owners, and
    /// panels whose surface was never published — callers that must
    /// distinguish those cases read the registration state separately.
    pub fn scene(&self, plugin_id: &str, panel_id: &str) -> Option<&PanelScene> {
        if plugin_id.is_empty() {
            return None;
        }
        self.scenes
            .get(panel_id)
            .filter(|record| record.owner_plugin_id == plugin_id)
            .map(|record| &record.scene)
    }

    /// Forget the scene of `panel_id`: a panel registered again starts
    /// without the scene and the revision of its earlier registration.
    pub fn remove(&mut self, panel_id: &str) {
        self.scenes.remove(panel_id);
    }

    /// Keep only the scenes whose owner `keep` accepts (the live plugins).
    pub fn retain_owners(&mut self, mut keep: impl FnMut(&str) -> bool) {
        self.scenes
            .retain(|_, record| keep(&record.owner_plugin_id));
        self.widgets
            .retain(|_, record| keep(&record.owner_plugin_id));
        self.tables
            .retain(|_, record| keep(&record.owner_plugin_id));
    }

    /// Publish (or replace) the panel's WIDGET surface (api v29, S05):
    /// the same ownership/revision rules as scene publishes, on the
    /// widget tier's independent revision space.
    pub fn publish_widget(
        &mut self,
        plugin_id: &str,
        panel_id: &str,
        surface: ocs_plugin_api::widget::WidgetSurface,
    ) -> Result<ocs_plugin_api::widget::WidgetRevision, ocs_plugin_api::widget::WidgetReject> {
        use ocs_plugin_api::widget::{WidgetReject, WidgetRejectCode};
        if plugin_id.is_empty() || panel_id.trim().is_empty() {
            return Err(WidgetReject {
                code: WidgetRejectCode::Unattached,
                current_revision: None,
            });
        }
        if surface.validate().is_err() {
            return Err(WidgetReject {
                code: WidgetRejectCode::InvalidSurface,
                current_revision: None,
            });
        }
        if let Some(record) = self.widgets.get(panel_id) {
            if record.owner_plugin_id != plugin_id {
                return Err(WidgetReject {
                    code: WidgetRejectCode::OwnedByOther,
                    current_revision: Some(record.surface.revision),
                });
            }
            let head = record.surface.revision;
            if surface.revision == head {
                if surface == record.surface {
                    return Ok(head); // idempotent: identical at the head
                }
                return Err(WidgetReject {
                    code: WidgetRejectCode::NonMonotonicRevision,
                    current_revision: Some(head),
                });
            }
            if surface.revision < head {
                return Err(WidgetReject {
                    code: WidgetRejectCode::NonMonotonicRevision,
                    current_revision: Some(head),
                });
            }
        }
        let revision = surface.revision;
        self.widgets.insert(
            panel_id.to_string(),
            WidgetRecord {
                owner_plugin_id: plugin_id.to_string(),
                surface,
            },
        );
        Ok(revision)
    }

    /// Install a bounded widget page atomically (the lazy-expansion and
    /// paging primitive). Only a previously published widget surface can
    /// be paged; ownership and revision rules match publishing.
    pub fn install_widget_page(
        &mut self,
        plugin_id: &str,
        panel_id: &str,
        page: ocs_plugin_api::widget::WidgetPage,
    ) -> Result<ocs_plugin_api::widget::WidgetRevision, ocs_plugin_api::widget::WidgetReject> {
        use ocs_plugin_api::widget::{WidgetReject, WidgetRejectCode};
        if plugin_id.is_empty() || panel_id.trim().is_empty() {
            return Err(WidgetReject {
                code: WidgetRejectCode::Unattached,
                current_revision: None,
            });
        }
        let Some(record) = self.widgets.get_mut(panel_id) else {
            return Err(WidgetReject {
                code: WidgetRejectCode::Unattached,
                current_revision: None,
            });
        };
        if record.owner_plugin_id != plugin_id {
            return Err(WidgetReject {
                code: WidgetRejectCode::OwnedByOther,
                current_revision: Some(record.surface.revision),
            });
        }
        match record.surface.apply_page(&page) {
            Ok(revision) => Ok(revision),
            Err(ocs_plugin_api::widget::WidgetError::StaleBase) => Err(WidgetReject {
                code: WidgetRejectCode::NonMonotonicRevision,
                current_revision: Some(record.surface.revision),
            }),
            Err(_) => Err(WidgetReject {
                code: WidgetRejectCode::InvalidSurface,
                current_revision: None,
            }),
        }
    }

    /// Ownership-checked widget read (api v29): a widget surface reads
    /// back only for its authenticated owner.
    pub fn widget(
        &self,
        plugin_id: &str,
        panel_id: &str,
    ) -> Option<&ocs_plugin_api::widget::WidgetSurface> {
        if plugin_id.is_empty() {
            return None;
        }
        self.widgets
            .get(panel_id)
            .filter(|record| record.owner_plugin_id == plugin_id)
            .map(|record| &record.surface)
    }

    /// Publish (or replace) the panel's TABLE surface (api v29, S07):
    /// the same ownership/revision rules, on the table tier's independent
    /// revision space.
    pub fn publish_table(
        &mut self,
        plugin_id: &str,
        panel_id: &str,
        surface: ocs_plugin_api::tables::TableSurface,
    ) -> Result<ocs_plugin_api::tables::TableRevision, ocs_plugin_api::tables::TableReject> {
        use ocs_plugin_api::tables::{TableReject, TableRejectCode};
        if plugin_id.is_empty() || panel_id.trim().is_empty() {
            return Err(TableReject {
                code: TableRejectCode::Unattached,
                current_revision: None,
            });
        }
        if surface.validate().is_err() {
            return Err(TableReject {
                code: TableRejectCode::InvalidSurface,
                current_revision: None,
            });
        }
        if let Some(record) = self.tables.get(panel_id) {
            if record.owner_plugin_id != plugin_id {
                return Err(TableReject {
                    code: TableRejectCode::OwnedByOther,
                    current_revision: Some(record.surface.revision),
                });
            }
            let head = record.surface.revision;
            if surface.revision == head {
                if surface == record.surface {
                    return Ok(head);
                }
                return Err(TableReject {
                    code: TableRejectCode::NonMonotonicRevision,
                    current_revision: Some(head),
                });
            }
            if surface.revision < head {
                return Err(TableReject {
                    code: TableRejectCode::NonMonotonicRevision,
                    current_revision: Some(head),
                });
            }
        }
        let revision = surface.revision;
        self.tables.insert(
            panel_id.to_string(),
            TableRecord {
                owner_plugin_id: plugin_id.to_string(),
                surface,
            },
        );
        Ok(revision)
    }

    /// Apply a bounded table patch atomically (cell/row ops). Only a
    /// previously published table can be patched; ownership and revision
    /// rules match publishing.
    pub fn patch_table(
        &mut self,
        plugin_id: &str,
        panel_id: &str,
        patch: ocs_plugin_api::tables::TablePatch,
    ) -> Result<ocs_plugin_api::tables::TableRevision, ocs_plugin_api::tables::TableReject> {
        use ocs_plugin_api::tables::{TableReject, TableRejectCode};
        if plugin_id.is_empty() || panel_id.trim().is_empty() {
            return Err(TableReject {
                code: TableRejectCode::Unattached,
                current_revision: None,
            });
        }
        let Some(record) = self.tables.get_mut(panel_id) else {
            return Err(TableReject {
                code: TableRejectCode::Unattached,
                current_revision: None,
            });
        };
        if record.owner_plugin_id != plugin_id {
            return Err(TableReject {
                code: TableRejectCode::OwnedByOther,
                current_revision: Some(record.surface.revision),
            });
        }
        match record.surface.apply_patch(&patch) {
            Ok(revision) => Ok(revision),
            Err(ocs_plugin_api::tables::TableError::StaleBase) => Err(TableReject {
                code: TableRejectCode::NonMonotonicRevision,
                current_revision: Some(record.surface.revision),
            }),
            Err(_) => Err(TableReject {
                code: TableRejectCode::InvalidSurface,
                current_revision: None,
            }),
        }
    }

    /// Ownership-checked table read (api v29).
    pub fn table(
        &self,
        plugin_id: &str,
        panel_id: &str,
    ) -> Option<&ocs_plugin_api::tables::TableSurface> {
        if plugin_id.is_empty() {
            return None;
        }
        self.tables
            .get(panel_id)
            .filter(|record| record.owner_plugin_id == plugin_id)
            .map(|record| &record.surface)
    }

    /// Owner-agnostic store read for the host's OWN automation interface
    /// (api v29, S08: the MCP facade reads what the operator's app owns —
    /// plugin-to-plugin isolation is untouched: `scene`/`widget`/`table`
    /// keep their owner checks). One bounded summary line per surface.
    pub fn surface_summaries(&self) -> Vec<SurfaceSummary> {
        let mut out = Vec::new();
        for (panel_id, record) in &self.scenes {
            out.push(SurfaceSummary {
                panel_id: panel_id.clone(),
                owner: record.owner_plugin_id.clone(),
                scene: Some((record.scene.revision.0, record.scene.elements.len())),
                widgets: None,
                tables: None,
            });
        }
        for (panel_id, record) in &self.widgets {
            match out.iter_mut().find(|s| &s.panel_id == panel_id) {
                Some(entry) => {
                    entry.widgets = Some((record.surface.revision.0, record.surface.rows.len()))
                }
                None => out.push(SurfaceSummary {
                    panel_id: panel_id.clone(),
                    owner: record.owner_plugin_id.clone(),
                    scene: None,
                    widgets: Some((record.surface.revision.0, record.surface.rows.len())),
                    tables: None,
                }),
            }
        }
        for (panel_id, record) in &self.tables {
            match out.iter_mut().find(|s| &s.panel_id == panel_id) {
                Some(entry) => {
                    entry.tables = Some((
                        record.surface.revision.0,
                        record.surface.rows.len(),
                        record.surface.columns.len(),
                    ))
                }
                None => out.push(SurfaceSummary {
                    panel_id: panel_id.clone(),
                    owner: record.owner_plugin_id.clone(),
                    scene: None,
                    widgets: None,
                    tables: Some((
                        record.surface.revision.0,
                        record.surface.rows.len(),
                        record.surface.columns.len(),
                    )),
                }),
            }
        }
        out
    }

    /// Owner-agnostic tier read for the automation facade (same law as
    /// `surface_summaries`).
    pub fn scene_any_owner(&self, panel_id: &str) -> Option<&PanelScene> {
        self.scenes.get(panel_id).map(|record| &record.scene)
    }

    /// Owner-agnostic widget-tier read (see `scene_any_owner`).
    pub fn widget_any_owner(
        &self,
        panel_id: &str,
    ) -> Option<&ocs_plugin_api::widget::WidgetSurface> {
        self.widgets.get(panel_id).map(|record| &record.surface)
    }

    /// Owner-agnostic table-tier read (see `scene_any_owner`).
    pub fn table_any_owner(&self, panel_id: &str) -> Option<&ocs_plugin_api::tables::TableSurface> {
        self.tables.get(panel_id).map(|record| &record.surface)
    }
}

/// One panel's surface inventory for the automation facade (S08): owner,
/// plus per-tier `(revision, count)` presence. Counts are bounded by the
/// tier budgets; nothing here materializes rows.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceSummary {
    pub panel_id: String,
    pub owner: String,
    pub scene: Option<(u64, usize)>,
    pub widgets: Option<(u64, usize)>,
    pub tables: Option<(u64, usize, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocs_plugin_api::scene::{SceneColor, SceneElement, StableId, ThemeColor, Vec2};

    const PLUGIN: &str = "acme.widgetpack";
    const PANEL: &str = "acme.widgetpack:graph";

    fn one_rect_scene(revision: u64) -> PanelScene {
        PanelScene {
            revision: SceneRevision(revision),
            elements: vec![SceneElement::Rect {
                id: StableId(1),
                name: None,
                pos: Vec2([0.0, 0.0]),
                size: Vec2([10.0, 10.0]),
                fill: Some(SceneColor::Theme(ThemeColor::Normal)),
                stroke: None,
            }],
        }
    }

    #[test]
    fn first_publish_stores_and_answers_the_revision() {
        let mut store = PanelSurfaceStore::default();
        let scene = one_rect_scene(7);
        assert_eq!(
            store.publish(PLUGIN, PANEL, scene.clone()),
            Ok(SceneRevision(7))
        );
        assert_eq!(store.scene(PLUGIN, PANEL), Some(&scene));
    }

    #[test]
    fn a_dead_owner_or_a_new_registration_forgets_the_scene() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(9))
            .expect("first publish");
        store.retain_owners(|owner| owner != PLUGIN);
        assert_eq!(store.scene(PLUGIN, PANEL), None);
        // A restarted plugin starts again from revision 1.
        assert_eq!(
            store.publish(PLUGIN, PANEL, one_rect_scene(1)),
            Ok(SceneRevision(1))
        );
        store.remove(PANEL);
        assert_eq!(
            store.publish("other.plugin", PANEL, one_rect_scene(1)),
            Ok(SceneRevision(1))
        );
    }

    #[test]
    fn identical_resurface_at_head_is_idempotent() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(3))
            .expect("first publish");
        assert_eq!(
            store.publish(PLUGIN, PANEL, one_rect_scene(3)),
            Ok(SceneRevision(3))
        );
    }

    #[test]
    fn same_revision_different_content_is_refused_without_mutation() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(3))
            .expect("first publish");
        let mut changed = one_rect_scene(3);
        if let SceneElement::Rect { size, .. } = &mut changed.elements[0] {
            size.0[0] = 99.0;
        }
        assert_eq!(
            store.publish(PLUGIN, PANEL, changed),
            Err(SurfaceReject {
                code: SurfaceRejectCode::NonMonotonicRevision,
                current_revision: Some(SceneRevision(3)),
            })
        );
    }

    #[test]
    fn stale_revision_is_refused_with_the_stored_head() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(5))
            .expect("first publish");
        assert_eq!(
            store.publish(PLUGIN, PANEL, one_rect_scene(4)),
            Err(SurfaceReject {
                code: SurfaceRejectCode::NonMonotonicRevision,
                current_revision: Some(SceneRevision(5)),
            })
        );
    }

    #[test]
    fn advancing_revision_replaces_wholesale() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(5))
            .expect("first publish");
        assert_eq!(
            store.publish(PLUGIN, PANEL, one_rect_scene(6)),
            Ok(SceneRevision(6))
        );
        assert_eq!(
            store.scene(PLUGIN, PANEL).unwrap().revision,
            SceneRevision(6)
        );
    }

    #[test]
    fn invalid_scene_is_refused_and_touches_nothing() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(5))
            .expect("first publish");
        let mut invalid = one_rect_scene(9);
        if let SceneElement::Rect { size, .. } = &mut invalid.elements[0] {
            size.0[1] = 0.0; // zero height: InvalidRect
        }
        assert_eq!(
            store.publish(PLUGIN, PANEL, invalid),
            Err(SurfaceReject {
                code: SurfaceRejectCode::InvalidSurface,
                current_revision: None,
            })
        );
        assert_eq!(
            store.scene(PLUGIN, PANEL).unwrap().revision,
            SceneRevision(5)
        );
    }

    #[test]
    fn foreign_owner_is_refused_and_cannot_read() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(2))
            .expect("first publish");
        assert_eq!(
            store.publish("other.plugin", PANEL, one_rect_scene(3)),
            Err(SurfaceReject {
                code: SurfaceRejectCode::OwnedByOther,
                current_revision: Some(SceneRevision(2)),
            })
        );
        // Authenticated reads: a foreign plugin reads nothing.
        assert_eq!(store.scene("other.plugin", PANEL), None);
        assert_eq!(
            store.scene(PLUGIN, PANEL).unwrap().revision,
            SceneRevision(2)
        );
    }

    #[test]
    fn blank_identities_are_refused() {
        let mut store = PanelSurfaceStore::default();
        assert!(matches!(
            store.publish("", PANEL, one_rect_scene(1)),
            Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                ..
            })
        ));
        assert!(matches!(
            store.publish(PLUGIN, "   ", one_rect_scene(1)),
            Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                ..
            })
        ));
    }

    fn remove_op(id: u64) -> ocs_plugin_api::scene::SceneOp {
        ocs_plugin_api::scene::SceneOp::Remove {
            id: ocs_plugin_api::scene::StableId(id),
        }
    }

    /// The happy patch path: a patch based on the stored head applies
    /// atomically and the revision advances by exactly one.
    #[test]
    fn patch_applies_atomically_and_advances_one_revision() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(5))
            .expect("publish");
        let patch = ocs_plugin_api::scene::ScenePatch {
            base_revision: ocs_plugin_api::scene::SceneRevision(5),
            ops: vec![remove_op(1), remove_op(2)],
        };
        assert_eq!(
            store.patch(PLUGIN, PANEL, patch),
            Err(SurfaceReject {
                code: SurfaceRejectCode::InvalidSurface,
                current_revision: None,
            })
        );
        // The second remove names a missing id: the WHOLE patch refused,
        // including the first (valid) remove — no partial mutation.
        assert_eq!(
            store.scene(PLUGIN, PANEL).unwrap().elements.len(),
            1,
            "first op must not survive a failed patch"
        );

        let patch = ocs_plugin_api::scene::ScenePatch {
            base_revision: ocs_plugin_api::scene::SceneRevision(5),
            ops: vec![remove_op(1)],
        };
        assert_eq!(store.patch(PLUGIN, PANEL, patch), Ok(SceneRevision(6)));
        assert_eq!(store.scene(PLUGIN, PANEL).unwrap().elements.len(), 0);
    }

    /// A stale base refuses with the stored head so the publisher can
    /// rebase; the stored scene is untouched.
    #[test]
    fn stale_base_patch_refuses_with_the_head() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(5))
            .expect("publish");
        let patch = ocs_plugin_api::scene::ScenePatch {
            base_revision: ocs_plugin_api::scene::SceneRevision(4),
            ops: vec![remove_op(1)],
        };
        assert_eq!(
            store.patch(PLUGIN, PANEL, patch),
            Err(SurfaceReject {
                code: SurfaceRejectCode::NonMonotonicRevision,
                current_revision: Some(SceneRevision(5)),
            })
        );
        assert_eq!(
            store.scene(PLUGIN, PANEL).unwrap().revision,
            SceneRevision(5)
        );
    }

    /// A foreign plugin cannot patch (or read) another plugin's surface.
    #[test]
    fn foreign_patch_is_refused() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, one_rect_scene(2))
            .expect("publish");
        let patch = ocs_plugin_api::scene::ScenePatch {
            base_revision: ocs_plugin_api::scene::SceneRevision(2),
            ops: vec![remove_op(1)],
        };
        assert_eq!(
            store.patch("other.plugin", PANEL, patch),
            Err(SurfaceReject {
                code: SurfaceRejectCode::OwnedByOther,
                current_revision: Some(SceneRevision(2)),
            })
        );
        assert_eq!(
            store.scene(PLUGIN, PANEL).unwrap().elements.len(),
            1,
            "foreign patch must not mutate"
        );
    }

    /// Patching a panel that never published a scene is Unattached —
    /// patches target an existing stored surface, they never create one.
    #[test]
    fn patch_without_a_stored_scene_is_unattached() {
        let mut store = PanelSurfaceStore::default();
        let patch = ocs_plugin_api::scene::ScenePatch {
            base_revision: ocs_plugin_api::scene::SceneRevision(1),
            ops: vec![remove_op(1)],
        };
        assert!(matches!(
            store.patch(PLUGIN, PANEL, patch),
            Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                ..
            })
        ));
    }
}
