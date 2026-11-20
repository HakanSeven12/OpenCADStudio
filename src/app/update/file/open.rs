use super::*;
use super::super::util::*;

impl OpenCADStudio {
    pub(in crate::app) fn on_open_file(&mut self) -> Task<Message> {
        // Native: pick a path, then load on a worker thread. Web: the
        // browser hands back bytes, so pick + parse in one step and feed
        // the shared `FileOpened` handler directly.
        // Web guard: a second open (Ctrl+O, the OPEN command, host
        // automation) while a drop/restore/picker open is still in flight
        // would overwrite `self.opening` — the in-flight open's
        // `WebFileOpened` then misses the id check and its bytes are lost,
        // and any pending restore prompt stalls for the session. Native
        // keeps the old behaviour: its `OpenPathPicked` round-trip is
        // guarded downstream.
        #[cfg(target_arch = "wasm32")]
        if self.opening.is_some() {
            self.command_line
                .push_info(crate::tf!("An open is already in progress.").as_ref());
            return Task::none();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Task::perform(crate::io::pick_open_path(), Message::OpenPathPicked)
        }
        #[cfg(target_arch = "wasm32")]
        {
            // The browser picker + parse happen inside `pick_and_load_web`;
            // the real name is unknown until then, so show a generic label
            // meanwhile.
            let (open_id, state) = self.begin_web_open("Opening…".into(), 0);
            Task::perform(crate::io::pick_and_load_web(state), move |outcome| {
                Message::WebFileOpened(open_id, outcome)
            })
        }
    }

    /// Web: open a drawing from bytes the caller already holds, through the
    /// same path as a file chosen in the browser picker.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn open_web_bytes(&mut self, name: String, bytes: Vec<u8>) -> Task<Message> {
        let (open_id, state) = self.begin_web_open(name.clone(), bytes.len() as u64);
        Task::perform(
            crate::io::open_bytes_web(name, std::sync::Arc::from(bytes), state),
            move |outcome| Message::WebFileOpened(open_id, outcome),
        )
    }

    /// `FileOpened` only installs the result when an open is in progress, so
    /// mark one.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn begin_web_open(
        &mut self,
        name: String,
        size_bytes: u64,
    ) -> (u64, std::sync::Arc<crate::io::OpenProgressState>) {
        let state = std::sync::Arc::new(crate::io::OpenProgressState::new(
            crate::app::OPEN_PHASE_READING,
        ));
        let open_id = self.next_open_id();
        self.opening = Some(crate::app::OpenProgress {
            id: open_id,
            name,
            source_path: None,
            size_bytes,
            state: state.clone(),
            started: Instant::now(),
            recovery_error: None,
            recovery_read_stats: None,
            recovery_bytes: None,
        });
        (open_id, state)
    }

    pub(in crate::app) fn next_open_id(&mut self) -> u64 {
        self.open_job_serial = self.open_job_serial.wrapping_add(1).max(1);
        self.open_job_serial
    }

    /// Index of a tab already showing `path`, or `None`.
    ///
    /// Compares resolved paths, so the same drawing reached through a symlink,
    /// a `..` segment or a different relative spelling is recognised as the one
    /// already open rather than loaded a second time. A path that cannot be
    /// resolved (deleted since) matches nothing and falls through to the normal
    /// open, which reports the miss.
    pub(in crate::app) fn tab_showing(&self, path: &std::path::Path) -> Option<usize> {
        #[cfg(target_arch = "wasm32")]
        {
            return self
                .tabs
                .iter()
                .position(|tab| tab.current_path.as_deref() == Some(path));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let want = std::fs::canonicalize(path).ok()?;
            self.tabs.iter().position(|t| {
                t.current_path
                    .as_deref()
                    .and_then(|p| std::fs::canonicalize(p).ok())
                    .is_some_and(|p| p == want)
            })
        }
    }

    /// Read through this session's lease when it covers `path`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn read_drawing(&self, path: &std::path::Path) -> std::io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};

        let lease = self
            .tab_showing(path)
            .and_then(|i| self.tabs[i].edit_lease.as_ref());
        let leased = match lease {
            Some(lease) => lease.reader()?,
            None => None,
        };
        let Some(mut reader) = leased else {
            return std::fs::read(path);
        };
        reader.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Start the next drawing a second launch handed us, if any.
    ///
    /// Must be called from EVERY path that clears `opening` — completion, error
    /// and cancel alike. Draining only the success path would strand the queue
    /// forever the first time a file fails to parse.
    pub(in crate::app) fn drain_pending_open(&mut self) -> Task<Message> {
        match self.pending_opens.pop_front() {
            Some(p) => Task::done(Message::OpenExternal(p)),
            None => Task::none(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn install_native_edit_guard(
        &mut self,
        i: usize,
        path: &std::path::Path,
        loaded_fingerprint: Option<crate::io::edit_lock::FileFingerprint>,
    ) {
        let current_fingerprint = crate::io::edit_lock::FileFingerprint::capture(path).ok();
        if loaded_fingerprint
            .as_ref()
            .zip(current_fingerprint.as_ref())
            .is_some_and(|(loaded, current)| loaded != current)
        {
            self.command_line.push_error_once(
                crate::t!(
                    "Drawing changed on disk while it was opening; Save will require conflict resolution."
                )
                .as_ref(),
            );
        }
        self.tabs[i].disk_fingerprint = loaded_fingerprint.or(current_fingerprint);
        match crate::io::edit_lock::EditLease::acquire(path) {
            Ok(lease) => {
                if let Some(warning) = lease.platform_warning() {
                    self.command_line.push_info(
                        crate::tf!(
                            "Edit lease active; {warning}. External-change checks remain active."
                        )
                        .as_ref(),
                    );
                }
                self.tabs[i].edit_lease = Some(lease);
                self.tabs[i].edit_lock_conflict = false;
            }
            Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                self.tabs[i].edit_lease = None;
                self.tabs[i].edit_lock_conflict = true;
                self.command_line.push_error_once(
                    crate::tf!("Opened read-only against other editors: {error}").as_ref(),
                );
            }
            Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                self.tabs[i].edit_lease = None;
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(
                    crate::tf!("{error}. External-change checks remain active.").as_ref(),
                );
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn refresh_native_edit_guard_after_save(
        &mut self,
        i: usize,
        path: &std::path::Path,
        path_changed: bool,
        destination_lease: Option<crate::io::edit_lock::EditLease>,
    ) {
        if path_changed {
            self.tabs[i].edit_lease = None;
            self.tabs[i].edit_lease = destination_lease;
            self.tabs[i].edit_lock_conflict = false;
            if self.tabs[i].edit_lease.is_none() {
                self.install_native_edit_guard(i, path, None);
                return;
            }
        }

        let refresh = self.tabs[i].edit_lease.as_mut().map(|lease| {
            lease
                .refresh_drawing_lock(path)
                .map(|_| lease.platform_warning().map(str::to_owned))
        });
        match refresh {
            Some(Ok(warning)) => {
                self.tabs[i].edit_lock_conflict = false;
                if let Some(warning) = warning {
                    self.command_line.push_info(
                        crate::tf!("{warning}. External-change checks remain active.").as_ref(),
                    );
                }
            }
            Some(Err(crate::io::edit_lock::EditLeaseError::Locked(error))) => {
                self.tabs[i].edit_lock_conflict = true;
                self.command_line.push_error_once(
                    crate::tf!(
                        "Saved, but the refreshed drawing is locked by another editor: {error}"
                    )
                    .as_ref(),
                );
            }
            Some(Err(crate::io::edit_lock::EditLeaseError::Unavailable(error))) => {
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(
                    crate::tf!("{error}. External-change checks remain active.").as_ref(),
                );
            }
            None => self.install_native_edit_guard(i, path, None),
        }
        self.tabs[i].disk_fingerprint = crate::io::edit_lock::FileFingerprint::capture(path).ok();
        #[cfg(not(target_arch = "wasm32"))]
        // Any other open drawing that references `path` re-reads it, the
        // same as an external reload: a source edited and saved in this
        // session is not allowed to go stale in its hosts.
        self.reload_xrefs_pointing_at(i, path);
    }

    /// Reload, in every other tab that references `path`, the definitions
    /// the save refreshed. `resolve_xrefs_for_keys` does the merge; this
    /// finds the matching records, refreshes the stat cache and marks the
    /// host dirty.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn reload_xrefs_pointing_at(
        &mut self,
        saved: usize,
        path: &std::path::Path,
    ) {
        let mut matches: Vec<(usize, rustc_hash::FxHashSet<document::Handle>)> = Vec::new();
        for (j, tab) in self.tabs.iter().enumerate() {
            if j == saved || tab.block_test.is_some() {
                continue;
            }
            let base_dir = tab
                .current_path
                .as_deref()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let keys: rustc_hash::FxHashSet<document::Handle> = tab
                .scene
                .document
                .block_records
                .iter()
                .filter(|br| {
                    (br.flags.is_xref || br.flags.is_xref_overlay) && !br.xref_path.is_empty()
                })
                .filter_map(|br| {
                    crate::io::xref::resolve_path(&br.xref_path, &base_dir)
                        .filter(|resolved| native_paths_match(resolved, path))
                        .map(|_| br.handle)
                })
                .collect();
            if !keys.is_empty() {
                matches.push((j, keys));
            }
        }
        for (j, keys) in matches {
            let base_dir = self.tabs[j]
                .current_path
                .as_deref()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            let infos = crate::io::xref::resolve_xrefs_for_keys(
                &mut self.tabs[j].scene.document,
                &base_dir,
                &keys,
            );
            if infos.is_empty() {
                continue;
            }
            // The stat cache reports the saved file as current again.
            let fresh = crate::io::xref::collect_entries_with_prev(
                &self.tabs[j].scene.document,
                &base_dir,
                self.tabs[j].xref_unloaded.as_set(),
                &self.tabs[j].xref_stat_cache.0,
            );
            for entry in &fresh {
                if let Some(modified) = entry.modified {
                    self.tabs[j].xref_stat_cache.insert(entry.key, modified);
                }
            }
            self.tabs[j].dirty = true;
            self.tabs[j].scene.bump_geometry_no_blocks();
            self.post_ref_op(j);
            self.command_line.push_info(
                crate::tf!("XREF: references to the saved source were reloaded.").as_ref(),
            );
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn save_tab_synchronously_protected(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
        set_current_path: bool,
    ) -> Result<(), crate::io::SaveFailure> {
        let version = self.tabs[i].scene.document.version;
        self.save_tab_synchronously_protected_as(i, path, version, set_current_path, false)
    }

    /// Synchronous protected save with an explicit output version. Automation
    /// uses this path so format conversion never depends on an implicit source
    /// version or silently upgrades to the newest DWG.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn save_tab_synchronously_protected_as(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
        version: document::DxfVersion,
        set_current_path: bool,
        require_new_destination: bool,
    ) -> Result<(), crate::io::SaveFailure> {
        if self.tabs[i].block_test.is_some() {
            return Err(crate::io::SaveFailure::other(
                "Test Block is temporary and cannot be saved",
            ));
        }
        let previous_autosave = self.autosave_target(i);
        if self.tabs[i].recovery_save_as_required
            && self.tabs[i]
                .current_path
                .as_deref()
                .is_some_and(|source| native_paths_match(source, &path))
        {
            return Err(crate::io::SaveFailure::other(
                "repaired drawing must be saved to a new file",
            ));
        }
        let path_changed = self.tabs[i]
            .current_path
            .as_deref()
            .is_none_or(|current| !native_paths_match(current, &path));
        if !path_changed && self.tabs[i].edit_lock_conflict {
            return Err(crate::io::SaveFailure::file_in_use(
                "drawing edit lock is held by another editor",
            ));
        }

        let mut destination_lease = if path_changed {
            match crate::io::edit_lock::EditLease::acquire(&path) {
                Ok(lease) => Some(lease),
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    return Err(crate::io::SaveFailure::file_in_use(error));
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.command_line.push_info(
                        crate::tf!("{error}. External-change checks remain active.").as_ref(),
                    );
                    None
                }
            }
        } else {
            None
        };

        let expected_fingerprint = if path_changed {
            None
        } else {
            self.tabs[i].disk_fingerprint.clone()
        };
        let lease = if path_changed {
            destination_lease.as_mut()
        } else {
            self.tabs[i].edit_lease.as_mut()
        };
        let (expected_fingerprint, verify_reader) =
            Self::native_save_verification(&path, lease, expected_fingerprint)?;

        self.prepare_native_save(i);
        let mut snapshot = self.tabs[i].document_for_save();
        // A Save As renames the drawing, but the save event above evaluated
        // the name-reporting fields against the path it was opened from. Stamp
        // the destination into the snapshot and re-evaluate there, so the
        // written file carries its own name and a failed write changes
        // nothing in the open drawing.
        if set_current_path && path_changed {
            snapshot.source_path = Some(path.to_string_lossy().into_owned());
            crate::entities::field::update_fields(&mut snapshot, 2, None);
        }
        let write_path = if path_changed {
            path.clone()
        } else {
            std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone())
        };
        crate::io::save_owned_as_version_atomic(
            snapshot,
            &write_path,
            version,
            self.backup_on_save,
            expected_fingerprint,
            verify_reader,
            false,
            require_new_destination,
            (!path_changed).then_some(path.as_path()),
            self.tabs[i].ifccad_session.as_ref(),
        )?;

        if set_current_path {
            self.tabs[i].current_path = Some(path.clone());
            self.tabs[i].scene.document.version = version;
            if path_changed {
                self.adopt_saved_path(i, &path);
            }
        }
        self.refresh_native_edit_guard_after_save(i, &path, path_changed, destination_lease);
        self.tabs[i].dirty = false;
        self.tabs[i].scene.note_tables_saved();
        if set_current_path {
            self.tabs[i].recovery_save_as_required = false;
        }
        let _ = std::fs::remove_file(previous_autosave);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn retry_native_edit_guard(&mut self, i: usize, path: &std::path::Path) -> Result<(), String> {
        if let Some(lease) = self.tabs[i].edit_lease.as_mut() {
            match lease.refresh_drawing_lock(path) {
                Ok(()) => {
                    let warning = lease.platform_warning().map(str::to_owned);
                    self.tabs[i].edit_lock_conflict = false;
                    if let Some(warning) = warning {
                        self.command_line.push_info(
                            crate::tf!("{warning}. External-change checks remain active.").as_ref(),
                        );
                    }
                    return Ok(());
                }
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    return Err(error);
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.tabs[i].edit_lock_conflict = false;
                    self.command_line.push_info(
                        crate::tf!("{error}. External-change checks remain active.").as_ref(),
                    );
                    return Ok(());
                }
            }
        }

        match crate::io::edit_lock::EditLease::acquire(path) {
            Ok(lease) => {
                let warning = lease.platform_warning().map(str::to_owned);
                self.tabs[i].edit_lease = Some(lease);
                self.tabs[i].edit_lock_conflict = false;
                if let Some(warning) = warning {
                    self.command_line.push_info(
                        crate::tf!("{warning}. External-change checks remain active.").as_ref(),
                    );
                }
                Ok(())
            }
            Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => Err(error),
            Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(
                    crate::tf!("{error}. External-change checks remain active.").as_ref(),
                );
                Ok(())
            }
        }
    }

    /// Drop Plot-dialog state that belongs to the previously active drawing.
    ///
    /// Plot settings such as paper size and CTB are document/layout data.
    /// A newly opened DWG may reuse the same tab index and layout name as the
    /// startup drawing, so `(tab, layout)` alone is not enough to identify the
    /// context. Keeping that cache makes the first Plot dialog show the startup
    /// A4/no-CTB state; the second opening then appears correct only because the
    /// first one refreshed the runtime state.
    pub(in crate::app) fn reset_plot_session_for_opened_document(&mut self) {
        self.plot_prev = None;
        self.plot_prev_context = None;
        self.plot_reconcile_printer_media = None;
        self.plot_setup_template = None;
        self.plot_window = None;
    }

    pub(in crate::app) fn on_file_opened(
        &mut self,
        name: String,
        path: std::path::PathBuf,
        doc: document::Document,
        mut caches: crate::scene::DerivedCaches,
        ifccad_session: Option<ocs_save::IfccadSession>,
    ) -> Task<Message> {
        // If the user clicked Cancel while the parser was running, the
        // overlay state was cleared and we silently drop the result.
        if self.opening.is_none() {
            return Task::none();
        }
        let ui_started = iced::time::Instant::now();
        // Web drawings live under bare file names: two tabs sharing a name
        // also share the single browser-stored copy, and the last save
        // silently replaces the other tab's. Warn at open time — the
        // desktop identifies drawings by full path and needs no such check.
        #[cfg(target_arch = "wasm32")]
        if self.tab_showing(&path).is_some() {
            self.command_line.push_warning(
                crate::tf!(
                    "\"{}\" is already open in another tab. On the web both \
                     tabs share one stored copy — the last save overwrites \
                     the other.",
                    name
                )
                .as_ref(),
            );
        }
        let open_started = self.opening.as_ref().map(|p| p.started);
        let size_bytes = self
            .opening
            .as_ref()
            .map(|progress| progress.size_bytes)
            .unwrap_or(0);
        let timings = caches.timings;
        let entity_count = doc.entities().count();
        let parser_errors_recovered = caches.read_stats.as_ref().is_some_and(|stats| {
            stats.recovered() || stats.skipped_source_records > 0 || !stats.stream_completed
        }) || doc
            .notifications
            .iter()
            .any(|item| item.notification_type == document::notification::NotificationType::Error);
        let reference_recovered = caches
            .xrefs
            .iter()
            .any(|item| item.status == crate::io::xref::XrefStatus::Recovered);
        let reference_failed = caches
            .xrefs
            .iter()
            .any(|item| item.status == crate::io::xref::XrefStatus::Failed);
        let document_repaired = parser_errors_recovered || reference_recovered;
        let recovery_needed = document_repaired || reference_failed;
        let total_ms = open_started
            .map(|started| started.elapsed().as_millis() as u32)
            .unwrap_or(0);
        self.command_line
            .push_output(crate::tf!("Opened \"{name}\" — {entity_count} entities").as_ref());
        // What a converter could not carry over is a warning: the drawing
        // opened and is usable.
        for diagnostic in caches
            .read_stats
            .iter()
            .flat_map(|stats| &stats.diagnostics)
        {
            if diagnostic.code == ocs_save::CONVERSION_LOSS {
                self.command_line.push_warning(&diagnostic.message);
            }
        }
        // Reference problems are warnings, not errors: the host drawing
        // opened and is usable, and automation reads an error as a failed
        // open (#1971). The full list goes to `state.xrefs`.
        for info in &caches.xrefs {
            match info.status {
                crate::io::xref::XrefStatus::Loaded => {
                    self.command_line
                        .push_output(crate::tf!("XREF  Loaded \"{}\"", info.name).as_ref());
                    if info.other_drawing {
                        self.command_line.push_warning(
                            crate::tf!(
                                "XREF  \"{}\" now loads a different drawing ({})",
                                info.name,
                                info.path
                            )
                            .as_ref(),
                        );
                    }
                }
                crate::io::xref::XrefStatus::Recovered => {
                    self.command_line.push_warning(
                        crate::tf!("XREF  Recovered with warnings: \"{}\"", info.name).as_ref(),
                    );
                }
                // Web: NotFound xrefs are summarized in one warning after the
                // loop (see below) — the per-reference line carries a
                // filesystem path no browser can open, and a drawing with
                // several unresolved xrefs would repeat the same hint.
                #[cfg(not(target_arch = "wasm32"))]
                crate::io::xref::XrefStatus::NotFound => {
                    self.command_line.push_warning(
                        crate::tf!("XREF  Not found: \"{}\" ({})", info.name, info.path).as_ref(),
                    );
                }
                #[cfg(target_arch = "wasm32")]
                crate::io::xref::XrefStatus::NotFound => {}
                crate::io::xref::XrefStatus::Failed => {
                    self.command_line.push_warning(
                        crate::tf!("XREF  Recovery failed: \"{}\" ({})", info.name, info.path)
                            .as_ref(),
                    );
                }
                crate::io::xref::XrefStatus::Unloaded => {
                    self.command_line.push_info(
                        crate::tf!("XREF  Unloaded (skipped): \"{}\"", info.name).as_ref(),
                    );
                }
            }
        }
        // Web: one warning per open lists every unresolved xref and the way
        // to fix it — dropping the files on the page registers their bytes,
        // and reopening the drawing (or XRELOAD) then resolves them.
        #[cfg(target_arch = "wasm32")]
        self.warn_unresolved_xrefs(&caches.xrefs);
        #[cfg(not(target_arch = "wasm32"))]
        let thumbs_task = self.push_recent(path.clone());
        #[cfg(target_arch = "wasm32")]
        let thumbs_task = Task::none();

        let current_is_empty = {
            let t = &self.tabs[self.active_tab];
            !t.is_start
                && t.current_path.is_none()
                && !t.dirty
                && self.tabs[self.active_tab].scene.document.entities().count() == 0
        };
        let i = if current_is_empty {
            self.active_tab
        } else {
            self.tab_counter += 1;
            let new_tab = crate::app::tab_document::DocumentTab::new_drawing(self.tab_counter);
            self.tabs.push(new_tab);
            let idx = self.tabs.len() - 1;
            self.active_tab = idx;
            self.apply_display_defaults(idx);
            idx
        };
        // A Restore-all batch's LAST restore inserts its tab here — one
        // async round AFTER its settle message. The focus flag therefore
        // also fires here: the message-boundary hook alone could run
        // before this insertion and be overwritten by it.
        #[cfg(target_arch = "wasm32")]
        if self.web_session_focus_pending && self.web_autosave_restore_all_remaining == 0 {
            self.web_session_focus_pending = false;
            let _ = self.select_remembered_session_tab();
        }

        // Missing-on-open: neutral command-line notice plus a flag
        // the Reference Manager reads. Never an auto-open modal.
        let missing = caches
            .xrefs
            .iter()
            .filter(|item| item.status == crate::io::xref::XrefStatus::NotFound)
            .count();
        self.tabs[i].xref_missing = missing;
        self.tabs[i].xref_report = caches
            .xrefs
            .iter()
            .map(|item| {
                (
                    item.name.clone(),
                    item.path.clone(),
                    crate::app::control::xref_status_name(&item.status),
                )
            })
            .collect();
        if missing > 0 {
            self.command_line.push_output(
                crate::tf!(
                "{} reference(s) not found — open the reference manager with EXTERNALREFERENCES.",
                missing
            )
                .as_ref(),
            );
        }

        let mut recovery_report = recovery_needed.then(|| {
            crate::io::recovery::RecoveryReport::recovered(
                self.tabs[i].id,
                &path,
                size_bytes,
                caches.source_sha256.clone(),
                caches.read_stats.clone(),
                entity_count,
                &caches.xrefs,
                &doc.notifications,
                document_repaired,
                timings,
                total_ms,
            )
        });
        if let Some(report) = recovery_report.as_mut() {
            report.persist();
        }

        #[cfg(not(target_arch = "wasm32"))]
        let opened_fingerprint = self
            .opening
            .as_ref()
            .and_then(|opening| opening.fingerprint.clone());
        self.tabs[i].current_path = Some(path.clone());
        #[cfg(not(target_arch = "wasm32"))]
        self.install_native_edit_guard(i, &path, opened_fingerprint);
        self.tabs[i].scene.material_base_dir = path.parent().map(std::path::Path::to_path_buf);
        self.tabs[i].scene.document = doc;
        self.tabs[i].xref_unloaded =
            crate::io::xref_model::UnloadSet::from_document(&self.tabs[i].scene.document);
        self.tabs[i].ifccad_alpha_acknowledged = false;
        if let Some(session) = ifccad_session.as_ref().filter(|s| s.has_blocked_entities()) {
            for (id, reason) in session.blocked_entities() {
                self.command_line.push_info(
                    crate::tf!("IFCCAD entity {id} retained without geometry: {reason}").as_ref(),
                );
            }
            self.command_line.push_info(
                crate::t!("Autosave is disabled for this drawing because it contains retained IFCCAD entities. Save as IFCCAD to keep them.").as_ref(),
            );
        }
        // A walk belongs to the drawing it was opened on. Loading a document
        // into this tab ends any walk it still held, so the new drawing can
        // never render under the previous one's hide.
        self.reset_layer_walk_for(i);
        self.tabs[i].scene.bump_layout_epoch();
        self.tabs[i].scene.bump_scale_epoch();
        self.tabs[i].scene.bump_ucs_epoch();
        // Load parameters first so imported dimensional constraints
        // can resolve their named driving values.
        self.tabs[i].scene.load_named_parameters_from_document();
        self.tabs[i]
            .scene
            .load_parametric_constraints_from_document();
        self.tabs[i].active_layer = self.tabs[i]
            .scene
            .document
            .header
            .current_layer_name
            .clone();
        // A file saved without the built-in Standard styles (foreign
        // or damaged) gets them re-seeded so nothing dangles (#366).
        crate::app::style_ops::ensure_standard_styles(&mut self.tabs[i].scene.document);
        // Copies made by earlier releases kept their source's dimension
        // association; such a file is reported as damaged elsewhere.
        crate::scene::dimension_assoc::drop_stray_dimension_reactors(
            &mut self.tabs[i].scene.document,
        );
        // Follow the file's saved current UCS from the moment it opens.
        self.tabs[i].adopt_active_ucs_from_header();
        // Adopt the drawing's own Ortho ($ORTHOMODE) and running OSNAP
        // ($OSMODE) so each file keeps its own state.
        self.adopt_header_sysvars(i);
        // Route shared CJK ideographs to the language matching this
        // drawing's code page (web per-language font split). Drop the
        // glyph cache if it changed so Han re-resolves to the new
        // language's font; geometry is (re)built below regardless. (#141)
        if crate::scene::text::web_font::set_cjk_lang_from_codepage(
            &self.tabs[i].scene.document.header.code_page,
        ) {
            crate::scene::text::ttf_glyph::clear_fallback_cache();
        }
        self.tabs[i].scene.sync_annotation_scale_from_header();

        // Open-time breakdown so regressions are visible immediately.
        // `total` is wall time from the Open click to here (post-xref,
        // pre-first-frame); the phase figures are the background-thread
        // parse/purge/cache spans plus the UI-thread xref resolve.
        self.command_line.push_info(
            crate::tf!(
                "  parse {}ms · purge {}ms · caches {}ms · xref {}ms · geometry {}ms · total {}ms",
                timings.parse_ms,
                timings.purge_ms,
                timings.caches_ms,
                timings.xref_ms,
                timings.finalize_ms,
                total_ms
            )
            .as_ref(),
        );

        // Caches were built on the background thread inside open_path().
        self.tabs[i].scene.local_extent_max = caches.local_extent_max;
        self.tabs[i].scene.local_center = caches.local_center;
        self.tabs[i].scene.hatches = caches.hatches;
        self.tabs[i].scene.images = caches.images;
        self.tabs[i].scene.meshes = caches.meshes;
        self.tabs[i].scene.block_meshes = caches.block_meshes;
        self.tabs[i].scene.object_data_cache = caches.object_data;
        let prepared_geometry = caches.prepared_geometry.take();
        // Invalidate the wire cache so the new document is tessellated.
        self.tabs[i].scene.bump_geometry();
        if let Some(prepared) = prepared_geometry {
            self.tabs[i].scene.install_prepared_open_geometry(prepared);
        }
        // Pay for the interaction index here, not on the first hover.
        self.tabs[i].scene.warm_interaction_caches();
        self.tabs[i]
            .scene
            .replace_selection(rustc_hash::FxHashSet::default());
        self.tabs[i].scene.preview_wires = vec![];
        // Reopen in whichever space the file was saved in — the CTAB
        // tab name when recorded, else the $TILEMODE model/paper flag —
        // instead of always landing in Model.
        {
            let names = self.tabs[i].scene.layout_names();
            let saved = crate::io::saved_active_layout(&self.tabs[i].scene.document)
                .and_then(|n| names.iter().find(|x| x.eq_ignore_ascii_case(&n)).cloned());
            self.tabs[i].scene.current_layout = match saved {
                Some(n) => n,
                None if self.tabs[i].scene.document.header.show_model_space => "Model".to_string(),
                // Paper was active but no CTAB — fall back to the first
                // paper layout (names[0] is always "Model").
                None => names
                    .into_iter()
                    .nth(1)
                    .unwrap_or_else(|| "Model".to_string()),
            };
            self.tabs[i].scene.load_current_layout_state();
            self.tabs[i].refresh_active_ucs();
        }
        // A layout saved with a model viewport entered reopens inside it,
        // with that viewport's UCS (#1670).
        let enter_viewport_task = self.tabs[i]
            .scene
            .saved_entered_viewport()
            .map_or_else(Task::none, |handle| {
                Task::done(Message::EnterViewport(handle))
            });

        // The tab may have been reused for the newly opened file. Any Plot
        // dialog cache belongs to the previous/startup drawing and must not
        // win over this DWG's embedded layout settings on the first Ctrl+P.
        self.reset_plot_session_for_opened_document();

        // Object isolation is session-only. A newly opened drawing must
        // not inherit the previous tab's filter, and persisted entity
        // visibility remains independent (not an isolation session).
        self.tabs[i].scene.reset_transient_visibility();
        crate::io::linetypes::repair_document(&mut self.tabs[i].scene.document);
        self.tabs[i].properties = PropertiesPanel::empty();
        // Seed the current table / multileader style from the file's
        // header so the ✓ marks the right one (text/dim/mline come from
        // the document header directly). DXF provides these via
        // $CTABLESTYLE / $CMLEADERSTYLE; DWG leaves them at "Standard".
        self.ribbon.active_table_style = self.tabs[i]
            .scene
            .document
            .header
            .current_table_style_name
            .clone();
        #[cfg(not(target_arch = "wasm32"))]
        self.notice_newer_autosave(i, &path);
        self.tabs[i].active_mleader_style = self.tabs[i]
            .scene
            .document
            .header
            .current_mleader_style_name
            .clone();
        let doc_layers = self.tabs[i].scene.document.layers.clone();
        let vp_info = self.tabs[i].scene.viewport_list();
        self.tabs[i]
            .layers
            .sync_with_viewports(&doc_layers, vp_info);
        self.sync_ribbon_layers();
        // Load the Annotate-ribbon style dropdowns (text / dimension /
        // multileader / table) from the opened document instead of
        // leaving them on the hard-coded "Standard" default.
        self.sync_ribbon_styles();
        // Reset the Home-ribbon Color / Linetype / Lineweight chips
        // to the newly opened document's CECOLOR / CELTYPE / CELWEIGHT
        // defaults (or to ByLayer when the file leaves them empty).
        // Without this they stick to whatever the prior tab had
        // selected — see #21.
        self.sync_ribbon_from_selection();
        self.tabs[i].scene.restore_saved_camera();
        // Grid/snap are per-drawing view settings — adopt the opened
        // file's active viewport state rather than a global preference.
        self.adopt_view_display(i);
        self.sync_render_mode_to_active_tile(i);
        self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
        self.tabs[i].dirty = document_repaired;
        self.tabs[i].recovery_save_as_required = document_repaired;
        self.tabs[i].history = crate::app::tab_document::HistoryState::default();
        self.refresh_properties();
        #[cfg(not(target_arch = "wasm32"))]
        let interaction_task = {
            let wires = self.tabs[i].scene.hit_test_wires();
            let screen_height = self.tabs[i].scene.selection.borrow().view.vp_size.1;
            self.prepare_interaction_index_task(i, wires, screen_height)
                .unwrap_or_else(Task::none)
        };
        #[cfg(target_arch = "wasm32")]
        let interaction_task = Task::none();
        if let Some(opening) = &self.opening {
            opening
                .state
                .set(crate::app::OPEN_PHASE_FINALIZING, 10000, 1, 1);
        }
        self.opening.take();
        // The part of the open that blocks the UI thread, kept apart from the
        // loader-thread phases so a slow handoff shows up on its own.
        crate::perf_record!(
            "[perf] open-ui-thread={}ms entities={}",
            ui_started.elapsed().as_millis(),
            entity_count,
        );
        // The document and its derived caches have settled: the next message
        // boundary announces `DocumentOpened` to native plugins exactly once
        // for this load (API v9).
        self.tabs[i].note_document_loaded(ifccad_session);
        // The tab list just changed (and the active tab with it): remember
        // it for the next boot's restore flow (web session memory).
        #[cfg(target_arch = "wasm32")]
        self.persist_web_session();
        // A running script or an automation open does not stop at the
        // recovery dialog; the report is already written and the repairs
        // were on the command line.
        let recovery_report = recovery_report.filter(|_| {
            !self.script_running && !crate::app::control::controlled_open_pending(self)
        });
        let pending_open_task = if let Some(report) = recovery_report {
            self.recovery_report = Some(report);
            self.active_modal = Some(crate::app::ModalKind::Recovery);
            Task::none()
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                // A drawing referencing fonts this machine lacks offers to
                // fetch them from the community repository before the user
                // studies garbled substitute text (unless recovery already
                // owns the modal slot).
                let mut missing =
                    crate::io::font_repo::missing_shx_fonts(&self.tabs[i].scene.document);
                missing.retain(|name| {
                    !self
                        .suppressed_missing_fonts
                        .contains(&crate::io::font_repo::font_key(name))
                });
                // Read-only/MCP evaluation sessions must remain non-blocking:
                // report missing fonts through control state, but never offer
                // a network/download mutation from a read-only launch.
                // A running script, or a user who turned the window off, gets
                // them on the command line instead of a dialog.
                if !missing.is_empty() && (self.script_running || !self.check_missing_fonts) {
                    let list = format!("{}: {}", crate::t!("Missing fonts"), missing.join(", "));
                    self.command_line.push_warning(&list);
                } else if !missing.is_empty() && self.check_missing_fonts && !self.read_only {
                    self.font_source_input = self.font_source_url.clone();
                    self.missing_fonts = Some(missing);
                    self.missing_fonts_path = self.tabs[i].current_path.clone();
                    self.missing_fonts_downloading = false;
                    self.active_modal = Some(crate::app::ModalKind::MissingFonts);
                }
            }
            self.drain_pending_open()
        };
        let startup_script_task = if !self.pending_startup_script_lines.is_empty() {
            let lines = std::mem::take(&mut self.pending_startup_script_lines);
            self.script_queue.extend(lines);
            Task::done(Message::ScriptPump)
        } else {
            Task::none()
        };
        let sheet_set_task = self.sheet_set_after_open(i);
        Task::batch([
            thumbs_task,
            pending_open_task,
            interaction_task,
            startup_script_task,
            sheet_set_task,
            enter_viewport_task,
        ])
    }

}
