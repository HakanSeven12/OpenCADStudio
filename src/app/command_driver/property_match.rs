use super::*;

impl OpenCADStudio {
    pub(super) fn handle_match_entity_layer(&mut self, dest: Vec<Handle>, src: Handle) {
        let i = self.active_tab;
        self.tabs[i].active_cmd = None;
        self.tabs[i].snap_result = None;
        self.tabs[i].scene.clear_preview_wire();
        let src_layer = self.tabs[i]
            .scene
            .document
            .get_entity(src)
            .map(|e| e.common().layer.clone());
        let dest: Vec<_> = dest
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        if dest.is_empty() {
            self.command_line
                .push_info(crate::t!("No editable objects selected.").as_ref());
        } else if let Some(layer) = src_layer {
            self.push_undo_snapshot(i, "LAYMATCH");
            // Pure mutation lives in the shared kernel; lock-filter (above)
            // plus undo, invalidate, dirty and echo stay caller-side.
            let _ = crate::entities::match_props::match_layer_kernel(
                &mut self.tabs[i].scene.document,
                &dest,
                src,
            );
            // New layer changes the baked by-layer colour/linetype/
            // lineweight — re-tessellate the moved entities so they
            // repaint immediately (issue #231 class).
            self.invalidate_property_targets(i, &dest);
            self.tabs[i].dirty = true;
            self.command_line
                .push_info(crate::tf!("Layer matched to \"{layer}\".").as_ref());
            self.sync_ribbon_layers();
        } else {
            self.command_line
                .push_error(crate::t!("Source object not found.").as_ref());
        }
    }


    pub(super) fn handle_match_properties(
        &mut self,
        mut dest: Vec<Handle>,
        src: Handle,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        dest.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        if dest.is_empty() {
            return Some(Task::none());
        }
        // The command stays active after each apply so more targets
        // can keep being picked; Enter / Esc ends it (#362).
        let src_clone = self.tabs[i].scene.document.get_entity(src).cloned();
        // This application record is the hatch background colour.
        // Keep the outer Option to distinguish "source is not a
        // hatch" from "source hatch has no background".
        let hatch_background_xdata: Option<Option<Vec<acadrust::xdata::XDataValue>>> =
            match src_clone.as_ref() {
                Some(acadrust::EntityType::Hatch(h)) => Some(
                    h.common
                        .extended_data
                        .get_record("HATCHBACKGROUNDCOLOR")
                        .map(|r| r.values.clone()),
                ),
                _ => None,
            };
        // Dimension-style overrides ride the ACAD record, identified
        // by a leading DSTYLE string. Matching replicates that payload
        // (or clears the destination when the source has none).
        let dstyle_xdata: Option<Vec<(i16, acadrust::xdata::XDataValue)>> = src_clone
            .as_ref()
            .filter(|e| {
                matches!(
                    e,
                    acadrust::EntityType::Dimension(_) | acadrust::EntityType::Leader(_)
                )
            })
            .map(|e| crate::entities::dim_override::pairs(&e.common().extended_data));

        if src_clone.is_some() {
            // Entity-level transfer lives in the shared kernel so headless
            // callers share it; the `&mut Document` xdata paths, undo,
            // tessellation, selection and echo stay caller-side here.
            // `all()` preserves the interactive behaviour; the xdata paths
            // below honour the matching flags for future narrow callers.
            let match_opts = crate::entities::match_props::MatchOpts::all();
            self.apply_property_op(i, "MATCHPROP", &dest, |app, handle| {
                let mut is_dim = false;
                let mut is_hatch = false;
                if let Some(se) = &src_clone {
                    if let Some(e) = app.tabs[i].scene.document.get_entity_mut(handle) {
                        is_dim = matches!(e, acadrust::EntityType::Dimension(_));
                        is_hatch = matches!(e, acadrust::EntityType::Hatch(_));
                        crate::entities::match_props::match_properties_kernel(se, e, &match_opts);
                    }
                }
                if is_hatch && match_opts.copy_hatch_background {
                    if let Some(values) = &hatch_background_xdata {
                        crate::scene::view::dispatch::set_entity_xdata(
                            &mut app.tabs[i].scene.document,
                            handle,
                            "HATCHBACKGROUNDCOLOR",
                            values.clone(),
                        );
                    }
                }
                // Dim-style overrides follow the style for dimension /
                // leader destinations — through set_entity_xdata so no
                // stale raw record survives. Caller-side: needs `&mut Document`.
                // (Collapsed: the old outer `dstyle_xdata.is_some() || dst-is-dim`
                // gate is subsumed — the replace only ever ran when both dst
                // and src are dim/leader.)
                if match_opts.copy_dim_overrides
                    && matches!(
                        app.tabs[i].scene.document.get_entity(handle),
                        Some(acadrust::EntityType::Dimension(_) | acadrust::EntityType::Leader(_))
                    )
                    && matches!(
                        src_clone,
                        Some(acadrust::EntityType::Dimension(_) | acadrust::EntityType::Leader(_))
                    )
                {
                    crate::entities::dim_override::replace(
                        &mut app.tabs[i].scene.document,
                        handle,
                        dstyle_xdata.clone().unwrap_or_default(),
                    );
                }
                // A restyled dimension renders from its baked *D block —
                // drop the stale block so the new style shows (#398).
                if is_dim {
                    app.tabs[i].scene.invalidate_dim_block_recorded(handle);
                }
                // Hatch fills render from a prebuilt model (#415).
                app.tabs[i].scene.refresh_fill_model(handle);
            });
            // Color / linetype / lineweight are baked into the cached
            // wires at tessellation time; re-tessellate only the matched
            // objects instead of rebuilding a large drawing.
            let changes: Vec<_> = dest
                .iter()
                .copied()
                .map(|handle| (handle, crate::scene::ChangeKind::Modified))
                .collect();
            self.tabs[i].scene.bump_entities(&changes);
            self.command_line
                .push_info(crate::tf!("Properties matched to {} object(s).", dest.len()).as_ref());
            // Clear the consumed target selection and keep prompting.
            self.tabs[i].scene.deselect_all();
            if let Some(cmd) = &self.tabs[i].active_cmd {
                self.command_line.push_info(&cmd.prompt());
            }
        } else {
            self.command_line
                .push_error(crate::t!("Source object not found.").as_ref());
            self.tabs[i].active_cmd = None;
            self.tabs[i].snap_result = None;
            self.tabs[i].scene.clear_preview_wire();
        }
        None
    }

}
