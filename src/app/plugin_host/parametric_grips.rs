// Constraints, parametric chains, grips and quad registration.

use super::validation::validated_symbol_name;
use super::*;

impl<'a> HostSession<'a> {
    /// Enumerate an entity's built-in grips with viewport parity (API v11):
    /// the same per-entity enumeration the selection cache builder produces —
    /// whatever each entity module declares, generic over every entity type.
    /// Reference-only entities and locked layers answer an empty list, the
    /// same answer the viewport gives. This is a pure read: it opens no undo
    /// scope, records nothing, and audits nothing.
    pub fn entity_grips(
        &self,
        handle: Handle,
    ) -> Result<Vec<ocs_plugin_api::host::GripInfo>, String> {
        let grips = self.app.entity_grips_for(self.tab, handle)?;
        Ok(grips
            .into_iter()
            .map(|grip| ocs_plugin_api::host::GripInfo {
                grip_id: grip.id,
                point: grip.world.to_array(),
                moves_entity: grip.is_midpoint,
                shape: format!("{:?}", grip.shape),
                axis: grip.axis.map(|axis| axis.to_array()),
            })
            .collect())
    }

    /// Perform the grip edit an interactive drag performs (API v11): the ONE
    /// shared pipeline (select → engage → move with snaps off → commit),
    /// exactly one undo step labeled like the interactive grip step,
    /// byte-exact restore on undo, refusal parity (unknown entity/grip,
    /// reference-only, locked layer, point out of view).
    pub fn grip_drag(
        &mut self,
        handle: Handle,
        grip_id: usize,
        to: [f64; 3],
    ) -> Result<(), String> {
        let task = self
            .app
            .perform_grip_drag(self.tab, handle, grip_id, to)
            .map_err(|(code, message)| format!("grip drag failed ({code}): {message}"))?;
        let _ = task;
        Ok(())
    }

    /// Read specific entities by handle (API v12) — the live document's
    /// current entity per handle, `None` for an unknown handle. The cheap
    /// refresh-pass path: a plugin that writes and re-reads never needs the
    /// invalidated full-document snapshot (the v11.1 per-boundary cost).
    pub fn entities_batch(
        &self,
        handles: &[Handle],
    ) -> Result<Vec<Option<document::EntityType>>, String> {
        if self.app.tabs.len() <= self.tab {
            return Err(format!("tab {} does not exist", self.tab));
        }
        Ok(handles
            .iter()
            .map(|handle| self.document().get_entity(*handle).cloned())
            .collect())
    }

    /// Create a parametric constraint directly (API v13): the SAME constraint
    /// machinery the interactive COINCIDENT (`GCCOINCIDENT`) and TANGENT
    /// (`TCONSTRAINT`) commands drive, reached without the command line — no
    /// prompts, no echo — through the command driver's own apply path, so
    /// validation, initial solve and undo composition behave identically.
    /// Inside a plugin-opened `push_undo` scope the constraint joins that
    /// scope (the entity-write contract); the re-bond a plugin composes of
    /// writes + constraints is its single labeled step.
    pub fn constraint_create(
        &mut self,
        constraint: ocs_plugin_api::host::PluginConstraint,
    ) -> Result<(), String> {
        if self.tab != self.app.active_tab {
            return Err("constraints act on the active document; switch to it first".to_owned());
        }
        let tab = self.tab;
        let to_pick =
            |pick: &ocs_plugin_api::host::PluginConstraintPick| crate::command::CoincidentPick {
                handle: pick.handle,
                point: glam::DVec3::new(pick.point[0], pick.point[1], pick.point[2]),
                whole_curve: pick.whole_curve,
            };
        let result = match constraint {
            ocs_plugin_api::host::PluginConstraint::Coincident { first, second } => {
                crate::command::CmdResult::AddCoincidentConstraint {
                    first: to_pick(&first),
                    second: to_pick(&second),
                    multiple: false,
                    label: "Coincident constraint",
                }
            }
            ocs_plugin_api::host::PluginConstraint::Tangent { first, second } => {
                // Resolve the picks the way the interactive TANGENT command
                // does, against the live document and the tab's scope.
                let scope = self.app.tabs[tab].current_parametric_scope();
                let document = &self.app.tabs[tab].scene.document;
                let resolve = |pick: &ocs_plugin_api::host::PluginConstraintPick,
                               exclude: Option<Handle>| {
                    let point =
                        document::types::Vector3::new(pick.point[0], pick.point[1], pick.point[2]);
                    if pick.whole_curve {
                        pick.handle.and_then(|handle| {
                            crate::scene::parametric_constraints::parametric_curve_ref_for_pick(
                                document, scope, handle, point,
                            )
                        })
                    } else if let Some(handle) = pick.handle {
                        crate::scene::parametric_constraints::nearest_parametric_point_on_entity(
                            document, scope, handle, point,
                        )
                    } else {
                        crate::scene::parametric_constraints::nearest_parametric_point(
                            document, scope, point, exclude,
                        )
                    }
                };
                let first_ref = resolve(&first, None);
                let second_ref = first_ref.and_then(|r| resolve(&second, Some(r.entity)));
                let (Some(first), Some(second)) = (first_ref, second_ref) else {
                    return Err(
                        "tangent picks did not resolve to parametric references on the live \
                         document; pick concrete points/curves of existing entities"
                            .to_owned(),
                    );
                };
                crate::command::CmdResult::AddTangentConstraint {
                    first,
                    second,
                    label: "Tangent constraint",
                }
            }
        };
        // The driver's success feedback ("Coincident constraint applied") is
        // interactive echo — the direct API path is quiet by contract
        // (errors still surface; the gate is the same one RunQuiet uses).
        let mut request = PluginRequest::new(self, true);
        let task = request.app.apply_cmd_result(result);
        request.app.drive_headless_task(task)
    }

    /// Turn a declared parametric chain into reference-only display geometry
    /// (API v14 — the engine lives in `crate::parametric`; the generic ramp
    /// evaluation, the seam audit and the entity builds happen before any
    /// change, so every refusal leaves the document untouched and records
    /// no undo step. `reference_name` is the caller's identity for the
    /// staged set (echoed in refusals); the host keeps no registry.
    pub fn stage_parametric_chain(
        &mut self,
        reference_name: &str,
        chain: &ocs_plugin_api::host::ParametricChain,
        tolerance: f64,
    ) -> Result<Vec<Handle>, String> {
        if chain.segments.is_empty() {
            return Err(format!(
                "chain {reference_name:?} stages nothing: the chain carries no segments"
            ));
        }
        let (entities, _carriers) = crate::parametric::build_display_entities(chain, tolerance)
            .map_err(|e| format!("chain {reference_name:?}: {e}"))?;
        let own_scope = self.ensure_xrecord_undo_scope("Stage parametric chain");
        let handles = self.add_entities(entities);
        for handle in &handles {
            // Designation one-by-one inside the SAME scope, with the
            // before-image pending-scoped the way set_reference_only does.
            let before = self.app.tabs[self.tab]
                .scene
                .reference_only
                .is_marked(*handle);
            self.app
                .record_reference_designation_before_pending(self.tab, *handle, before);
            self.app.tabs[self.tab]
                .scene
                .set_reference_only_membership(*handle, true);
        }
        self.bump_handles_incremental(&handles);
        self.set_dirty();
        self.publish_document_view();
        if own_scope {
            self.app.finish_pending_history(self.tab);
        }
        Ok(handles)
    }

    /// Validate a declared parametric chain without staging it (additive).
    /// Runs the complete display build audit and immediately discards the
    /// entities and carriers. Pure: no document, undo, publish, dirty or
    /// cache state changes.
    pub fn validate_parametric_chain(
        &self,
        chain: &ocs_plugin_api::host::ParametricChain,
        tolerance: f64,
    ) -> Result<(), String> {
        if chain.segments.is_empty() {
            return Err("a parametric chain needs at least one segment".to_owned());
        }
        crate::parametric::build_display_entities(chain, tolerance).map(|_| ())
    }

    /// Evaluate a declared parametric chain at chain position `s` (API v14,
    /// additive): a pure read — no document state, no undo, no publish.
    pub fn sample_parametric(
        &self,
        chain: &ocs_plugin_api::host::ParametricChain,
        s: f64,
    ) -> Result<Option<ocs_plugin_api::host::ParametricSample>, String> {
        crate::parametric::sample_chain(chain, s)
    }

    // ── API v14: plugin class objects (additive) ─────────────────────────

    /// Declare (or clear) the calling plugin's custom grip points (API v11).
    /// The whole batch REPLACES the plugin's previous set; the host applies
    /// it as a diff keyed by grip id so an unchanged re-declare is near-free
    /// (the pull model's message-boundary re-declare). Display-session state
    /// only: nothing is persisted, nothing is host-interpreted, and the set
    /// dies with the plugin's process.
    ///
    /// `plugin_id` is the identity the transport attached: over the plugin
    /// IPC the host serves this with the speaking process's own manifest id
    /// and refuses mismatches, so a plugin cannot declare grips in another
    /// plugin's name; an in-process host (tests) registers the id it passes.
    pub fn set_plugin_grips(
        &mut self,
        plugin_id: &str,
        grips: Vec<ocs_plugin_api::host::PluginGrip>,
    ) -> Result<(), String> {
        let known = crate::plugin::grip_points::visibility(&self.app.disabled_plugins)(plugin_id);
        if !known {
            return Err(format!(
                "plugin {plugin_id:?} is not a loaded, alive plugin; custom grips must be \
                 declared by their owner"
            ));
        }
        // Anchors accept ANY live entity handle — never a type restriction —
        // and never add host-side movement semantics (the points are absolute).
        for grip in &grips {
            if let Some(anchor) = grip.anchor {
                if self.document().get_entity(anchor).is_none() {
                    return Err(format!(
                        "grip {} anchors entity {anchor:?}, which does not exist",
                        grip.grip_id
                    ));
                }
            }
        }
        let tab = self.tab;
        self.app.tabs[tab].plugin_grips.declare(plugin_id, grips)?;
        Ok(())
    }

    // ── API v15: quad cursor menu panels (additive) ──────────────────────────

    /// Register the calling plugin's declarative quad panels (API v15,
    /// additive): the SESSION store the quad builds read from (the
    /// `src/plugin/quad.rs` process-global, quad panels are app-global like
    /// `quad_mru` — not tab state). Per-panel replace-if-same-owner; the
    /// ownership law is the custom-grip one: over the plugin IPC the host
    /// serves this with the speaking process's own manifest id (the runner
    /// passes the id nominally), so a plugin can only register in its own
    /// name; an in-process host (tests) registers the id it passes.
    ///
    /// Rejections push the short reason to the command line (the wire's
    /// response carries one `bool` per panel plus a generic short string —
    /// the same shape the v7 PanelRegistered story keeps).
    pub fn register_quad_panels(
        &mut self,
        plugin_id: &str,
        panels: &[ocs_plugin_api::dialog::QuadPanelDef],
    ) -> Vec<bool> {
        let known = crate::plugin::grip_points::visibility(&self.app.disabled_plugins)(plugin_id);
        if !known {
            if plugin_id.is_empty() {
                // The boot-window registration storm fires before the handshake
                // pins the process's manifest id (the empty-attribution class —
                // HOST-FIXES-TODO #1). The plugin genuinely registered nothing
                // in its own name, so the refusals that would follow name a
                // plugin that never spoke: SILENCE is the honest noise law the
                // user ordered (2026-11-13). Replay-after-handshake stays the
                // #1 fix's direction; until then these requests simply do not
                // exist for the user.
                return vec![false; panels.len()];
            }
            for panel in panels {
                self.push_error(&format!(
                    "quad panel {id:?} refused: plugin {plugin_id:?} is not a loaded, alive \
                     plugin; quad panels must be declared by their owner",
                    id = panel.panel_id
                ));
            }
            return vec![false; panels.len()];
        }
        let results = crate::plugin::quad::with_store(|store| store.register(plugin_id, panels));
        for (panel, ok) in panels.iter().zip(results.iter()) {
            if !ok {
                self.push_error(&format!(
                    "quad panel {id:?} refused: it belongs to a different plugin, breaches the \
                     {} panels / {} items caps, or repeats a control id / uses an unusable id",
                    crate::plugin::quad::MAX_QUAD_PANELS_PER_PLUGIN,
                    crate::plugin::quad::MAX_QUAD_ITEMS_PER_PANEL,
                    id = panel.panel_id
                ));
            }
        }
        results
    }

    /// Remove this plugin's quad panels again (API v15, additive). The
    /// ownership law runs both ways (mirroring
    /// [`Self::register_quad_panels`]'s `plugin_id` scoping): ids another
    /// plugin owns are refused, not removed.
    pub fn unregister_quad_panels(&mut self, plugin_id: &str, panel_ids: &[&str]) -> Vec<bool> {
        let owned: Vec<String> = panel_ids.iter().map(|id| (*id).to_string()).collect();
        crate::plugin::quad::with_store(|store| store.unregister(plugin_id, &owned))
    }
}
