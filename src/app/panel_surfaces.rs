//! Pure panel surface publish semantics (api v29).
//!
//! Ownership and attachment are resolved by the caller (the host session
//! knows the authenticated transport owner and the registered panels); this
//! module owns the structural and revision semantics of a publish and never
//! touches the renderer. Nothing here draws, and nothing here depends on
//! iced — the UI projects from the stored state.
//!
//! Publish contract (mirrors the wire documentation on
//! `PluginRequest::PanelSurfacePublish`): the first publish stores the
//! scene; a later publish replaces it wholesale exactly when its revision
//! strictly advances over the stored head; an identical resurface at the
//! head is an idempotent no-op; anything else is refused with a typed
//! [`SurfaceReject`] and no partial state change.

use ocs_plugin_api::scene::{PanelScene, SceneRevision};
use ocs_plugin_api::scene::{PanelSurface, SurfaceReject, SurfaceRejectCode};

/// Stored surface per panel id: strictly one authoritative record per
/// panel, owned by the authenticated plugin that published it.
#[derive(Debug, Default)]
pub struct PanelSurfaceStore {
    scenes: std::collections::BTreeMap<String, SceneRecord>,
}

#[derive(Debug, Clone, PartialEq)]
struct SceneRecord {
    owner_plugin_id: String,
    scene: PanelScene,
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
        surface: PanelSurface,
    ) -> Result<SceneRevision, SurfaceReject> {
        if plugin_id.is_empty() || panel_id.trim().is_empty() {
            return Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                current_revision: None,
            });
        }
        let PanelSurface::Scene(scene) = surface;
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
            store.publish(PLUGIN, PANEL, PanelSurface::Scene(scene.clone())),
            Ok(SceneRevision(7))
        );
        assert_eq!(store.scene(PLUGIN, PANEL), Some(&scene));
    }

    #[test]
    fn identical_resurface_at_head_is_idempotent() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(3)))
            .expect("first publish");
        assert_eq!(
            store.publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(3))),
            Ok(SceneRevision(3))
        );
    }

    #[test]
    fn same_revision_different_content_is_refused_without_mutation() {
        let mut store = PanelSurfaceStore::default();
        store
            .publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(3)))
            .expect("first publish");
        let mut changed = one_rect_scene(3);
        if let SceneElement::Rect { size, .. } = &mut changed.elements[0] {
            size.0[0] = 99.0;
        }
        assert_eq!(
            store.publish(PLUGIN, PANEL, PanelSurface::Scene(changed)),
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
            .publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(5)))
            .expect("first publish");
        assert_eq!(
            store.publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(4))),
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
            .publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(5)))
            .expect("first publish");
        assert_eq!(
            store.publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(6))),
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
            .publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(5)))
            .expect("first publish");
        let mut invalid = one_rect_scene(9);
        if let SceneElement::Rect { size, .. } = &mut invalid.elements[0] {
            size.0[1] = 0.0; // zero height: InvalidRect
        }
        assert_eq!(
            store.publish(PLUGIN, PANEL, PanelSurface::Scene(invalid)),
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
            .publish(PLUGIN, PANEL, PanelSurface::Scene(one_rect_scene(2)))
            .expect("first publish");
        assert_eq!(
            store.publish(
                "other.plugin",
                PANEL,
                PanelSurface::Scene(one_rect_scene(3))
            ),
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
            store.publish("", PANEL, PanelSurface::Scene(one_rect_scene(1))),
            Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                ..
            })
        ));
        assert!(matches!(
            store.publish(PLUGIN, "   ", PanelSurface::Scene(one_rect_scene(1))),
            Err(SurfaceReject {
                code: SurfaceRejectCode::Unattached,
                ..
            })
        ));
    }
}
