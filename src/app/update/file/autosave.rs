use super::*;
use super::super::util::*;

impl OpenCADStudio {
    /// SAVEFILEPATH: the folder recovery copies go to, `autosave` in the
    /// settings folder unless one was set.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn save_file_folder(&self) -> std::path::PathBuf {
        let set = self.save_file_path.trim();
        if !set.is_empty() {
            return std::path::PathBuf::from(set);
        }
        crate::config::config_dir()
            .map(|dir| dir.join("autosave"))
            .unwrap_or_else(std::env::temp_dir)
    }

    /// Where the autosave recovery copy for tab `i` lives: in the
    /// SAVEFILEPATH folder, named after the drawing and its full path so two
    /// drawings of one name in different folders keep their own copy.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn autosave_target(&self, i: usize) -> std::path::PathBuf {
        let folder = self.save_file_folder();
        let _ = std::fs::create_dir_all(&folder);
        match &self.tabs[i].current_path {
            Some(p) => {
                let name = p
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".to_string());
                // FNV-1a: the same name in every build, so a copy from before
                // a crash is found after an update.
                let hash = p
                    .to_string_lossy()
                    .bytes()
                    .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
                        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
                    });
                folder.join(format!("{name}.{:08x}.ocs-autosave.sv$", hash as u32))
            }
            None => {
                let safe: String = self.tabs[i]
                    .tab_display_name()
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect();
                // Tab ids start again every session: the session keeps one
                // session's copy from replacing another's.
                folder.join(format!(
                    "OpenCADStudio_{safe}_{}_{}.sv$",
                    autosave_session(),
                    self.tabs[i].id
                ))
            }
        }
    }

    /// At startup: the recovery copies of unsaved drawings whose session has
    /// ended (a crash, a forced quit) are moved aside, so this session's
    /// autosaves never replace them, and the command line says where each one
    /// is. A copy of an editor still running is left alone: each session holds
    /// a lock for as long as it runs, which the system drops however it ends.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn recover_ended_session_autosaves(&mut self) {
        // Locks of sessions that ended with no copy left are removed here.
        let locks = autosave_session_dir().and_then(|dir| std::fs::read_dir(dir).ok());
        for entry in locks.into_iter().flatten().flatten() {
            if let Some(id) = entry.file_name().to_string_lossy().strip_suffix(".lock") {
                session_running(id);
            }
        }
        let Ok(entries) = std::fs::read_dir(self.save_file_folder()) else {
            return;
        };
        for entry in entries.flatten() {
            let copy = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = name
                .strip_prefix("OpenCADStudio_")
                .and_then(|rest| rest.strip_suffix(".sv$"))
            else {
                continue;
            };
            if stem.ends_with(".ocs-recovered") {
                continue;
            }
            // `<name>_<session>_<tab>`; a copy from before sessions were
            // named has no lock to find and counts as ended.
            let session = stem.rsplit('_').nth(1).unwrap_or_default();
            if session_running(session) {
                continue;
            }
            let kept = copy.with_file_name(format!("OpenCADStudio_{stem}.ocs-recovered.sv$"));
            if std::fs::rename(&copy, &kept).is_ok() {
                self.command_line.push_warning(&crate::tr!(
                    "recovery",
                    "unsaved-autosave",
                    file = kept.display().to_string()
                ));
            }
        }
    }

    /// The OPFS autosave snapshot key for `tab`: the drawing's display name
    /// (the recent copy's name), or the tab title for untitled tabs — the
    /// single derivation every web snapshot/remove site must agree on, or a
    /// remove misses the snapshot a round wrote (and vice versa).
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn web_autosave_key(tab: &crate::app::tab_document::DocumentTab) -> String {
        tab.current_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| tab.tab_display_name())
    }

    /// Periodic autosave (SAVETIME): write a `.sv$` recovery copy for every
    /// dirty tab — beside the file if it's saved, else under the temp dir — at
    /// the document's own DWG version. Best-effort and non-destructive: it never
    /// touches the original file or the dirty flag.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_autosave(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for i in 0..self.tabs.len() {
            if self.tabs[i].block_test.is_some()
                || !self.tabs[i].dirty
                || self.tabs[i]
                    .ifccad_session
                    .as_ref()
                    .is_some_and(ocs_save::IfccadSession::has_blocked_entities)
                || self.active_save_jobs.contains_key(&self.tabs[i].id)
            {
                continue;
            }
            self.prepare_snapshot_save(i);
            let version = self.tabs[i].scene.document.version;
            let target = self.autosave_target(i);
            tasks.push(self.queue_native_save(
                i,
                target,
                version,
                crate::app::SavePurpose::Autosave,
                crate::app::SaveContinuation::None,
                false,
                false,
            ));
        }
        Task::batch(tasks)
    }

    /// Periodic autosave on the web (SAVETIME): serialize every dirty tab to
    /// an OPFS snapshot (`io::web_autosave`), the browser counterpart of the
    /// native `.sv$` copy — best-effort and non-destructive: no download, no
    /// recents update, dirty flag untouched. Snapshots run serially in one
    /// round; while it lasts (or a web save's thumbnail capture is pending)
    /// further ticks are dropped, not queued. Each tab's snapshot is captured
    /// up front. Style sync and dimension baking run one snapshot at a time,
    /// just before its worker writer pass and OPFS write, so pending tabs do
    /// not retain every drawing's baked geometry simultaneously.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_autosave(&mut self) -> Task<Message> {
        if self.web_autosave_in_flight || self.pending_web_thumbnail_save.is_some() {
            return Task::none();
        }
        let mut pending = Vec::new();
        for i in 0..self.tabs.len() {
            if self.tabs[i].block_test.is_some()
                || !self.tabs[i].dirty
                || self.tabs[i]
                    .ifccad_session
                    .as_ref()
                    .is_some_and(ocs_save::IfccadSession::has_blocked_entities)
            {
                continue;
            }
            // Same pre-serialize state sync the web Save flow performs
            // (on_save_dialog_confirm): stamp the live camera, vport and
            // sysvars into the document so the snapshot round-trips.
            self.sync_view_state_for_save(i);
            sync_annotation_scale_header(&mut self.tabs[i].scene);
            self.stamp_header_sysvars(i);
            self.sync_solid_models_for_save(i);
            self.tabs[i].scene.sync_native_parametric_graph();
            let tab = &self.tabs[i];
            // Key by the drawing's display name (the recent copy's name), so
            // the boot scan can compare the two; untitled tabs use the tab
            // title, which is what Save would download as.
            let name = Self::web_autosave_key(tab);
            let is_dxf = crate::io::source_is_dxf(tab.current_path.as_deref(), &tab.scene.document);
            let ext = if is_dxf { "dxf" } else { "dwg" }.to_string();
            let version = tab.scene.document.version;
            pending.push(((name, ext), tab.scene.document.clone(), version));
        }
        if pending.is_empty() {
            return Task::none();
        }
        self.web_autosave_in_flight = true;
        Task::perform(
            async move {
                let mut failure = None;
                for ((name, ext), prepared) in crate::io::prepare_save_batch(pending) {
                    // Autosave exists to recover geometry, not pretty recents:
                    // no thumbnail capture, just bytes into OPFS.
                    match crate::io::serialize_prepared_document(&prepared, &ext, false, None).await
                    {
                        Ok(bytes) => {
                            if let Err(error) =
                                crate::io::web_autosave::snapshot(&name, &bytes, None).await
                            {
                                failure = Some(error);
                            }
                        }
                        Err(error) => failure = Some(error),
                    }
                }
                failure.map_or(Ok(()), Err)
            },
            Message::WebAutosaveSnapshotFinished,
        )
    }

    /// One web autosave round finished. Silent on success; on failure one
    /// command-line warning until a round succeeds again. Also the drain
    /// point for snapshot removes a save deferred while the round was
    /// writing: the round's writes are done, so the keys can be cleared
    /// for real now (no late write can resurrect them).
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_autosave_finished(&mut self, result: Result<(), String>) -> Task<Message> {
        self.web_autosave_in_flight = false;
        let deferred = std::mem::take(&mut self.web_autosave_clear_after);
        if !deferred.is_empty() {
            wasm_bindgen_futures::spawn_local(async move {
                for key in &deferred {
                    let _ = crate::io::web_autosave::remove(key).await;
                }
            });
        }
        match result {
            Ok(()) => self.web_autosave_warned = false,
            Err(error) => {
                if !self.web_autosave_warned {
                    self.web_autosave_warned = true;
                    self.command_line.push_warning(
                        crate::tf!("Autosave copy could not be stored: {error}").as_ref(),
                    );
                }
            }
        }
        Task::none()
    }

    /// The boot scan decided which snapshots are recoverable; queue the
    /// restore prompt (behind any already-queued startup modal). An empty
    /// scan — a fresh profile, or everything already cleaned up — must not
    /// surface an empty "Recover Drawing" modal, so the prompt is only
    /// queued when there is something to offer. The remembered session list
    /// is loaded right after the scan: it never opens anything itself, it
    /// only names what the session had (a line in the restore modal) and
    /// picks the active tab once restores complete.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_autosaves_scanned(
        &mut self,
        result: Result<Vec<crate::io::web_autosave::WebAutosaveEntry>, String>,
    ) -> Task<Message> {
        self.web_session = crate::io::web_session::load();
        let Ok(mut entries) = result else {
            // Best-effort feature: no storage access, no prompt.
            return Task::none();
        };
        entries.sort_by(|a, b| b.modified_ms.total_cmp(&a.modified_ms));
        self.web_autosave_pending = entries;
        self.queue_autosave_restore_prompt();
        Task::none()
    }

    /// Restore the offered snapshot: read the OPFS copy — leaving it in
    /// place — and hand the bytes to the 50 ms poll's guarded open queue.
    /// The snapshot is the only copy, so it is deleted only once the
    /// recovered drawing actually opens (`web_autosave_opened`, on the
    /// matching `WebFileOpened` success); a parse failure or cancel leaves
    /// it recoverable for the next boot.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_autosave_restore(&mut self) -> Task<Message> {
        let Some(entry) = self.web_autosave_pending.first().cloned() else {
            self.close_active_modal();
            return Task::none();
        };
        self.web_autosave_pending.remove(0);
        self.close_active_modal();
        let open_name = crate::io::web_autosave::recovered_name(&entry.name);
        let key = entry.name;
        // The snapshot key travels with the bytes: the marker (and the
        // deletion it authorises) is recorded only when the poll actually
        // starts this open, keyed by that open's id — never by display name.
        let read_key = key.clone();
        Task::perform(
            async move { crate::io::web_autosave::read(&read_key).await },
            move |result| Message::WebAutosaveBytes(open_name, key, result),
        )
    }

    /// "Restore all": the whole pending list goes through the exact same
    /// guarded path as a single restore — one read per snapshot, then
    /// `enqueue_restore` with its own key, and the 50 ms poll opens them
    /// sequentially. Nothing about consumption changes: each snapshot is
    /// still deleted only when its own open succeeds (matched by open id),
    /// and any failure (unreadable, parse error, cancel) keeps that
    /// snapshot in OPFS for the next boot. The batch counts its settles in
    /// `web_autosave_restore_all_remaining`; every settle path funnels
    /// through `queue_next_web_autosave_prompt`, so the countdown — and the
    /// remembered session's tab selection when it hits zero — needs no
    /// extra bookkeeping sites.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_autosave_restore_all(&mut self) -> Task<Message> {
        // Fewer than two pending: this is just Restore.
        if self.web_autosave_pending.len() < 2 {
            return self.on_web_autosave_restore();
        }
        let entries = std::mem::take(&mut self.web_autosave_pending);
        self.web_autosave_restore_all_remaining = entries.len();
        self.close_active_modal();
        self.command_line
            .push_info(crate::tf!("Restoring {} drawing(s)…", entries.len()).as_ref());
        let reads = crate::io::web_autosave::restore_all_plan(&entries)
            .into_iter()
            .map(|(open_name, key)| {
                // Same contract as the single restore: the key travels with
                // the bytes so the deletion marker is recorded by open id.
                let read_key = key.clone();
                Task::perform(
                    async move { crate::io::web_autosave::read(&read_key).await },
                    move |result| Message::WebAutosaveBytes(open_name, key, result),
                )
            })
            .collect::<Vec<_>>();
        Task::batch(reads)
    }

    /// The recovered drawing opened: its autosave snapshot — held back until
    /// now — can finally be deleted, and the next queued snapshot can be
    /// offered. The caller already matched the marker to this open's id.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn web_autosave_opened(&mut self, open_name: &str) {
        let Some((key, _)) = self.web_autosave_consumed.take() else {
            return;
        };
        self.command_line
            .push_output(crate::tf!("Restored \"{}\" from the autosave copy", open_name).as_ref());
        wasm_bindgen_futures::spawn_local(async move {
            let _ = crate::io::web_autosave::remove(&key).await;
        });
        self.queue_next_web_autosave_prompt();
    }

    /// Discard the offered snapshot; the prompt stays up for the next one.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_autosave_discard(&mut self) -> Task<Message> {
        if let Some(entry) = self.web_autosave_pending.first().cloned() {
            self.web_autosave_pending.remove(0);
            wasm_bindgen_futures::spawn_local(async move {
                let _ = crate::io::web_autosave::remove(&entry.name).await;
            });
        }
        if self.web_autosave_pending.is_empty() {
            self.close_active_modal();
        }
        Task::none()
    }

    /// Defer the offered snapshot (Escape/✕ on the restore modal): unlike
    /// Discard the OPFS copy is NOT removed — one stray keystroke must not
    /// destroy the only recovery copy — it is simply re-offered on the next
    /// boot. The prompt moves on to the next queued snapshot, if any.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_autosave_defer(&mut self) -> Task<Message> {
        if !self.web_autosave_pending.is_empty() {
            self.web_autosave_pending.remove(0);
        }
        if self.web_autosave_pending.is_empty() {
            self.close_active_modal();
        }
        Task::none()
    }

    /// The offered snapshot's bytes arrived (or did not). On success the
    /// bytes ride the 50 ms poll's guarded open queue — the poll starts an
    /// open only when the slot is free, so the recovered drawing can
    /// neither clobber an open in flight nor be lost to one, and the
    /// snapshot survives until that open lands (`web_autosave_opened`).
    /// On failure the snapshot stays in OPFS (discardable on a later boot)
    /// and the prompt moves on to the next one.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_autosave_bytes(
        &mut self,
        open_name: String,
        key: String,
        result: Result<Vec<u8>, String>,
    ) -> Task<Message> {
        match result {
            Ok(bytes) => {
                // The next prompt is queued only when this open settles
                // (`web_autosave_opened` / the failure hook), not here — the
                // poll has not started the open yet, so queueing now would
                // show the modal before the recovered drawing loads.
                // Exception: a rejected enqueue (queue full) means no open
                // will ever start for these bytes, so the prompt cycle must
                // continue here or stall for the session. The snapshot
                // survives in OPFS and is re-offered on the next boot.
                if !crate::io::web_drop::enqueue_restore(open_name, bytes, key) {
                    self.command_line.push_warning(
                        crate::tf!("Autosave copy could not be queued: too many pending opens")
                            .as_ref(),
                    );
                    self.queue_next_web_autosave_prompt();
                }
                Task::none()
            }
            Err(error) => {
                // Unreadable snapshot: warn, but neither delete it nor
                // re-offer it this session — it was already taken out of
                // `web_autosave_pending`, so the cycle cannot loop on it.
                // It stays discardable on the next boot.
                self.command_line
                    .push_warning(crate::tf!("Autosave copy could not be read: {error}").as_ref());
                self.queue_next_web_autosave_prompt();
                Task::none()
            }
        }
    }

    /// Re-offer the remaining boot snapshots once the current one settled.
    /// No-op when none are left (or the modal is already queued). This is
    /// also the single funnel every restore settle runs through (opened,
    /// failed, cancelled, unreadable, or queue-rejected), so a Restore-all
    /// batch counts down here — and when its last snapshot settles, the
    /// remembered session decides which tab ends up active.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn queue_next_web_autosave_prompt(&mut self) {
        if self.web_autosave_restore_all_remaining > 0 {
            self.web_autosave_restore_all_remaining -= 1;
            if self.web_autosave_restore_all_remaining == 0 {
                // Applied at the next message boundary: the last restore's
                // settle (`web_autosave_opened`) runs BEFORE its own open
                // inserts the tab, so focusing here could not see it yet.
                self.web_session_focus_pending = true;
            }
        }
        if !self.web_autosave_pending.is_empty() {
            self.queue_autosave_restore_prompt();
        }
    }

    /// A Restore-all batch has fully settled: focus the tab the crashed
    /// session remembered as active, if it actually came back (or is open
    /// under its own name). Restored tabs carry the "(recovered)" marker —
    /// `recovered_name` maps the remembered name onto it idempotently. A
    /// no-op without remembered memory, or when the remembered tab did not
    /// return; the last-opened tab then stays active, which is fine. Called
    /// from the after-message hook (`web_session_focus_pending`), so the
    /// whole settle batch — including the last tab insertion — is done.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn select_remembered_session_tab(&mut self) -> Task<Message> {
        let Some(session) = self.web_session.as_ref() else {
            return Task::none();
        };
        let Some(remembered) = session.tabs.get(session.active) else {
            return Task::none();
        };
        let recovered = crate::io::web_autosave::recovered_name(remembered);
        let Some(index) = self.tabs.iter().position(|tab| {
            let name = tab.tab_display_name();
            name == remembered.as_str() || name == recovered
        }) else {
            return Task::none();
        };
        self.active_tab = index;
        // The active tab changed under the ribbon; re-sync its chips like
        // every other tab switch does (#21).
        self.sync_ribbon_layers();
        self.sync_ribbon_styles();
        self.sync_ribbon_from_selection();

        Task::none()
    }

    /// Remember the open-tab list (names + active index) in localStorage —
    /// the web session memory the next boot's restore flow consults. Cheap
    /// by design: a small `Vec<String>` + index, rewritten in full. Called
    /// on every save/open/close; the Start tab is not a drawing and is
    /// skipped, with the active index remapped onto the filtered list.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn persist_web_session(&self) {
        if crate::io::host_bridge::is_embed_mode() {
            return;
        }
        let mut names: Vec<String> = Vec::new();
        let mut active = 0;
        for (index, tab) in self.tabs.iter().enumerate() {
            if tab.is_start || tab.block_test.is_some() {
                continue;
            }
            if index == self.active_tab {
                active = names.len();
            }
            names.push(tab.tab_display_name());
        }
        crate::io::web_session::persist(&names, active);
    }

    /// Delete the `.sv$` autosave recovery files for all open drawings. They
    /// exist only to survive a crash, so a clean save or exit removes them.
    pub(in crate::app) fn cleanup_autosaves(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        for i in 0..self.tabs.len() {
            let _ = std::fs::remove_file(self.autosave_target(i));
        }
        #[cfg(not(target_arch = "wasm32"))]
        for path in &self.sheet_set.locks {
            crate::app::commands::sheet_set::release_lock(path);
        }
        // Web counterpart: drop each open tab's OPFS snapshot, plus any
        // removes a save deferred while a snapshot round was still writing.
        // A tab close is not an exit, so this only runs on the explicit exit
        // path; anything the teardown race leaves behind is caught by the
        // next boot scan.
        #[cfg(target_arch = "wasm32")]
        if !crate::io::host_bridge::is_embed_mode() {
            let mut keys = std::mem::take(&mut self.web_autosave_clear_after);
            // An in-flight round may still be writing snapshots right now;
            // its late writes would land AFTER these removes and resurrect
            // exactly what was removed, so the per-tab sweep is pointless in
            // that race — skip it entirely (the boot scan bounds leftovers:
            // `SNAPSHOT_CAP` eviction discards the oldest overflow).
            if !self.web_autosave_in_flight {
                for tab in &self.tabs {
                    keys.push(Self::web_autosave_key(tab));
                }
            }
            wasm_bindgen_futures::spawn_local(async move {
                for key in &keys {
                    let _ = crate::io::web_autosave::remove(key).await;
                }
            });
        }
    }

    /// Quit without removing the recovery copies (see `Message::Terminate`).
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn leave_keeping_recovery(&mut self) -> Task<Message> {
        self.exit_requested = true;
        iced::exit()
    }

    /// Remove the autosave recovery files, then quit the application.
    pub(in crate::app) fn exit_app(&mut self) -> Task<Message> {
        self.cleanup_autosaves();
        self.exit_requested = true;
        iced::exit()
    }

}
