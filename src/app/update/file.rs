//! `file` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    ortho_constrain, parse_coord, polar_constrain_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis,
    CoordKind,
};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::model::object::GripApply;
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};

pub(super) fn background_task<T, F, M>(work: F, map: M) -> Task<Message>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
    M: FnOnce(T) -> Message + Send + 'static,
{
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(work());
        });
        Task::perform(
            async move { rx.await.expect("background export worker dropped") },
            map,
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        Task::perform(async move { work() }, map)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn native_paths_match(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (
        std::fs::canonicalize(left),
        std::fs::canonicalize(right),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

impl OpenCADStudio {
    /// Before a save, give every cached truck solid that still has no ACIS
    /// geometry (EXTRUDE/REVOLVE/SWEEP/LOFT/boolean results) an exact modeler
    /// body derived from its truck B-rep, so the written DWG/DXF carries real
    /// 3-D geometry other CAD apps can open instead of an empty data stream.
    /// Curved solids that the exact planar path can't yet express are left
    /// untouched (handled by the NURBS path).
    #[cfg(feature = "solid3d")]
    fn sync_truck_solids_to_acis(&mut self, i: usize) {
        use acadrust::EntityType;
        let scene = &mut self.tabs[i].scene;
        let targets: Vec<acadrust::Handle> = scene
            .solid_models
            .keys()
            .copied()
            .filter(|h| {
                matches!(
                    scene.document.get_entity(*h),
                    Some(EntityType::Solid3D(s)) if !s.acis_data.has_data()
                )
            })
            .collect();
        for h in targets {
            // Build the SAT while borrowing solid_models; the returned document
            // is owned, so the borrow ends before we mutate the entity.
            let sat = scene
                .solid_models
                .get(&h)
                .and_then(crate::scene::convert::acis_export::planar_solid_to_sat);
            if let Some(sat) = sat {
                if let Some(EntityType::Solid3D(s)) = scene.document.get_entity_mut(h) {
                    s.set_sat_document(&sat);
                }
            }
        }
    }

    #[cfg(not(feature = "solid3d"))]
    fn sync_truck_solids_to_acis(&mut self, _i: usize) {}

    /// Snapshot the persisted UI preferences from live state.
    pub(in crate::app) fn current_settings(&self) -> crate::app::settings::UserSettings {
        crate::app::settings::UserSettings {
            dyn_input: self.dyn_input,
            polar: self.polar_mode,
            polar_increment_deg: self.polar_increment_deg,
            otrack: self.snapper.otrack_enabled,
            default_assoc_prompted: self.default_assoc_prompted,
            disabled_plugins: {
                let mut v: Vec<String> = self.disabled_plugins.iter().cloned().collect();
                v.sort();
                v
            },
            plugin_repos: self.plugin_repos.clone(),
            literal_spaces: self.command_line.literal_spaces,
            osmode: crate::app::settings::osmode_from_snaps(
                self.snapper.enabled.iter(),
                self.snapper.snap_enabled,
            ),
            texteditmode: self.texteditmode,
            textfill: crate::scene::text::sdf_atlas::textfill(),
            backup_on_save: self.backup_on_save,
            file_assoc_enabled: self.file_assoc_enabled,
            savetime_min: self.savetime_min,
            default_save_format: self.default_save_format.clone(),
            pick_add: self.pick_add,
            pick_drag_rect: self.pick_drag_rect,
            bg_color: self.default_bg_color.map(f4_to_u3),
            paper_bg_color: self.default_paper_bg_color.map(f4_to_u3),
        }
    }

    /// Apply restored preferences to live state.
    pub(in crate::app) fn apply_settings(&mut self, s: &crate::app::settings::UserSettings) {
        self.dyn_input = s.dyn_input;
        self.polar_mode = s.polar;
        self.polar_increment_deg = s.polar_increment_deg;
        // Ortho + running OSNAP are per-drawing (adopted from the header on
        // open / tab switch), not app-global, so they are not applied here.
        self.snapper.otrack_enabled = s.otrack;
        self.default_assoc_prompted = s.default_assoc_prompted;
        self.disabled_plugins = s.disabled_plugins.iter().cloned().collect();
        self.plugin_repos = s.plugin_repos.clone();
        self.command_line.literal_spaces = s.literal_spaces;
        let (modes, snap_enabled) = crate::app::settings::snaps_from_osmode(s.osmode);
        self.snapper.enabled = modes.into_iter().collect();
        self.snapper.snap_enabled = snap_enabled;
        self.texteditmode = s.texteditmode;
        crate::scene::text::sdf_atlas::set_textfill(s.textfill);
        self.backup_on_save = s.backup_on_save;
        self.file_assoc_enabled = s.file_assoc_enabled;
        self.savetime_min = s.savetime_min;
        self.default_save_format =
            crate::io::canonical_save_format(&s.default_save_format).to_string();
        self.pick_add = s.pick_add;
        self.pick_drag_rect = s.pick_drag_rect;
        self.default_bg_color = s.bg_color.map(u3_to_f4);
        self.default_paper_bg_color = s.paper_bg_color.map(u3_to_f4);
        // Push the restored background onto every drawing tab that exists now
        // (the start tab and any initial drawing). Tabs created later pick it
        // up via `apply_bg_default` at their construction site.
        for idx in 0..self.tabs.len() {
            self.apply_bg_default(idx);
        }
        self.rebuild_ribbon_modules();
    }

    /// Adopt the per-drawing sysvars stored in tab `i`'s document header —
    /// Ortho (`$ORTHOMODE`) and the running object-snap set (`$OSMODE`) — into
    /// the live app state, so each drawing keeps its own. Called when a drawing
    /// becomes active (open, tab switch). The app-settings values seed the boot
    /// default; the per-file header overrides it here.
    pub(in crate::app) fn adopt_header_sysvars(&mut self, i: usize) {
        if self.tabs[i].is_start {
            return;
        }
        let (ortho, osmode) = {
            let h = &self.tabs[i].scene.document.header;
            (h.ortho_mode, h.object_snap_mode)
        };
        self.ortho_mode = ortho;
        // Ortho and Polar are mutually exclusive (ToggleOrtho clears Polar).
        if ortho {
            self.polar_mode = false;
        }
        // OSMODE has no file slot in modern DWG (R2000+ moved it to the
        // registry), so a header value of 0 just means "absent" — keep the
        // user's app-level set instead of wiping it. Only a legacy R13/R14 or
        // DXF file that really carries a nonzero mask overrides.
        if osmode != 0 {
            let (modes, snap_enabled) = crate::app::settings::snaps_from_osmode(osmode);
            self.snapper.enabled = modes.into_iter().collect();
            self.snapper.snap_enabled = snap_enabled;
        }
    }

    /// Stamp the live per-drawing sysvars (Ortho, running OSNAP) onto tab `i`'s
    /// document header just before it is written to disk (or before switching
    /// away from it), so they persist with the drawing.
    pub(in crate::app) fn stamp_header_sysvars(&mut self, i: usize) {
        if self.tabs[i].is_start {
            return;
        }
        let osmode = crate::app::settings::osmode_from_snaps(
            self.snapper.enabled.iter(),
            self.snapper.snap_enabled,
        );
        let h = &mut self.tabs[i].scene.document.header;
        h.ortho_mode = self.ortho_mode;
        h.object_snap_mode = osmode;
    }

    /// Apply the persisted default background(s) to tab `idx`. No-op for the
    /// start tab or when no default is set. Refreshes the tab's cached wires
    /// and meshes so background-adaptive colours pick up the change.
    pub(in crate::app) fn apply_bg_default(&mut self, idx: usize) {
        let bg = self.default_bg_color;
        let paper_bg = self.default_paper_bg_color;
        if bg.is_none() && paper_bg.is_none() {
            return;
        }
        let tab = &mut self.tabs[idx];
        if tab.is_start {
            return;
        }
        if let Some(c) = bg {
            tab.bg_color = Some(c);
            tab.scene.bg_color = c;
        }
        if let Some(c) = paper_bg {
            tab.paper_bg_color = Some(c);
            tab.scene.paper_bg_color = c;
        }
        tab.scene.recolor_meshes();
        tab.scene.bump_geometry();
    }

    /// Check if a suspended command exists on the active tab and resume it
    /// with the outcome of the text editor.
    pub(in crate::app) fn post_editor_closed(&mut self, committed: bool) -> Task<Message> {
        let i = self.active_tab;
        if let Some(mut cmd) = self.tabs[i].suspended_cmd.take() {
            let res = cmd.on_editor_closed(committed);
            self.tabs[i].active_cmd = Some(cmd);
            self.apply_cmd_result(res)
        } else {
            Task::none()
        }
    }

    /// Rebuild the ribbon's tab list from the registry, dropping the tabs of any
    /// disabled plugins. Call after `disabled_plugins` changes.
    pub(in crate::app) fn rebuild_ribbon_modules(&mut self) {
        let modules =
            crate::plugin::ribbon_modules_enabled(&self.disabled_plugins);
        self.ribbon.set_modules(modules);
        // Refresh the command-line autocomplete pool so a newly enabled plugin's
        // commands become typeable (and a disabled one's drop out). This runs on
        // startup load, settings reload, and every enable/disable toggle (#272).
        self.command_line.dynamic_commands =
            crate::plugin::plugin_command_names(&self.disabled_plugins);
    }

    /// Snapshot of disabled plugin ids — lets the registry skip them while it
    /// holds a `&mut` borrow of the app via `HostSession`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn disabled_plugin_ids(&self) -> rustc_hash::FxHashSet<String> {
        self.disabled_plugins.clone()
    }

    /// Background task: fetch the curated plugin registry.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn fetch_registry_task(&self) -> Task<Message> {
        Task::perform(
            async { crate::plugin::marketplace::fetch_registry() },
            Message::PluginRegistryFetched,
        )
    }

    /// Background task: fetch `owner/repo`'s installable release tags.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn fetch_releases_task(&self, repo: String) -> Task<Message> {
        let label = repo.clone();
        Task::perform(
            async move {
                crate::plugin::marketplace::fetch_releases(&repo).map(|rs| {
                    rs.into_iter()
                        .filter(|r| r.installable())
                        .map(|r| r.tag)
                        .collect::<Vec<_>>()
                })
            },
            move |res| Message::PluginReleasesFetched(label, res),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn fetch_releases_task(&self, _repo: String) -> Task<Message> {
        Task::none()
    }

    /// Background task: download and install the `tag` release of `owner/repo`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn install_task(&self, repo: String, tag: String) -> Task<Message> {
        Task::perform(
            async move {
                let releases = crate::plugin::marketplace::fetch_releases(&repo)?;
                let rel = releases
                    .into_iter()
                    .find(|r| r.tag == tag)
                    .ok_or_else(|| format!("release {tag} not found"))?;
                crate::plugin::marketplace::install(&rel)
            },
            Message::PluginInstalled,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn install_task(&self, _repo: String, _tag: String) -> Task<Message> {
        Task::none()
    }

    /// Gather the full persisted config (all sections) from live app state.
    pub(in crate::app) fn current_config(&self) -> crate::app::config::AppConfig {
        crate::app::config::AppConfig {
            settings: self.current_settings(),
            theme: self.ui_theme.clone(),
            recent: crate::app::config::RecentConfig {
                files: self
                    .recent_files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                limit: self.recent_limit,
            },
            start: crate::app::config::StartConfig {
                section: self.start_section,
            },
            statusbar: self.statusbar_config.clone(),
            ribbon: crate::app::config::RibbonConfig {
                collapse: self.ribbon.collapse_mode(),
            },
            plot: self.plot_dialog.clone(),
        }
    }

    /// Distribute a loaded config into live app state (called once at startup).
    pub(in crate::app) fn apply_config(&mut self, cfg: crate::app::config::AppConfig) {
        self.apply_settings(&cfg.settings);
        self.ui_theme = cfg.theme.clone();
        self.active_theme = self.ui_theme.to_iced();
        self.theme_color_inputs = self.ui_theme.palette.hex_values();
        self.recent_files = cfg
            .recent
            .files
            .iter()
            .map(std::path::PathBuf::from)
            .collect();
        self.recent_limit = cfg
            .recent
            .limit
            .clamp(crate::app::recent::RECENT_MIN, crate::app::recent::RECENT_MAX);
        self.recent_files.truncate(self.recent_limit);
        self.recent_limit_input = self.recent_limit.to_string();
        // Thumbnails are decoded by a background task queued at boot
        // (`refresh_recent_thumbs`) — never here on the boot path.
        self.start_section = cfg.start.section;
        self.statusbar_config = cfg.statusbar;
        self.ribbon.set_collapse_mode(cfg.ribbon.collapse);
        self.plot_dialog = cfg.plot;
    }

    /// Write the config to disk only when it changed since the last write, so a
    /// toggle persists immediately without thrashing the file.
    pub(in crate::app) fn save_config(&mut self) {
        let cur = self.current_config();
        if self.last_saved_config.as_ref() != Some(&cur) {
            cur.save();
            self.last_saved_config = Some(cur);
        }
    }

    /// Back-compat name for the many "a preference changed, persist it" sites.
    pub(in crate::app) fn persist_settings_if_changed(&mut self) {
        self.save_config();
    }

    /// Record that the one-time default-association prompt has been answered and
    /// flush it to disk, so the dialog never reappears on later launches.
    pub(in crate::app) fn mark_assoc_prompted(&mut self) {
        self.default_assoc_prompted = true;
        self.persist_settings_if_changed();
    }

pub(super) fn on_open_file(&mut self) -> Task<Message> {
                // Native: pick a path, then load on a worker thread. Web: the
                // browser hands back bytes, so pick + parse in one step and feed
                // the shared `FileOpened` handler directly.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(crate::io::pick_open_path(), Message::OpenPathPicked)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    // `FileOpened` only installs the result when an open is in
                    // progress, so mark one. The browser picker + parse happen
                    // inside `pick_and_load_web`; the real name is unknown until
                    // then, so show a generic label meanwhile.
            let state = std::sync::Arc::new(crate::io::OpenProgressState::new(
                crate::app::OPEN_PHASE_READING,
            ));
                    self.opening = Some(crate::app::OpenProgress {
                        name: "Opening…".into(),
                        size_bytes: 0,
                        state: state.clone(),
                        started: Instant::now(),
                    });
                    Task::perform(crate::io::pick_and_load_web(state), Message::FileOpened)
                }
    }

    /// Index of a tab already showing `path`, or `None`.
    ///
    /// Compares resolved paths, so the same drawing reached through a symlink,
    /// a `..` segment or a different relative spelling is recognised as the one
    /// already open rather than loaded a second time. A path that cannot be
    /// resolved (deleted since) matches nothing and falls through to the normal
    /// open, which reports the miss.
    pub(in crate::app) fn tab_showing(&self, path: &std::path::Path) -> Option<usize> {
        let want = std::fs::canonicalize(path).ok()?;
        self.tabs.iter().position(|t| {
            t.current_path
                .as_deref()
                .and_then(|p| std::fs::canonicalize(p).ok())
                .is_some_and(|p| p == want)
        })
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
    fn install_native_edit_guard(
        &mut self,
        i: usize,
        path: &std::path::Path,
        loaded_fingerprint: Option<crate::io::edit_lock::FileFingerprint>,
    ) {
        let current_fingerprint =
            crate::io::edit_lock::FileFingerprint::capture(path).ok();
        if loaded_fingerprint
            .as_ref()
            .zip(current_fingerprint.as_ref())
            .is_some_and(|(loaded, current)| loaded != current)
        {
            self.command_line.push_error_once(
                "Drawing changed on disk while it was opening; Save will require conflict resolution.",
            );
        }
        self.tabs[i].disk_fingerprint =
            loaded_fingerprint.or(current_fingerprint);
        match crate::io::edit_lock::EditLease::acquire(path) {
            Ok(lease) => {
                if let Some(warning) = lease.platform_warning() {
                    self.command_line.push_info(&format!(
                        "Edit lease active; {warning}. External-change checks remain active."
                    ));
                }
                self.tabs[i].edit_lease = Some(lease);
                self.tabs[i].edit_lock_conflict = false;
            }
            Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                self.tabs[i].edit_lease = None;
                self.tabs[i].edit_lock_conflict = true;
                self.command_line.push_error_once(&format!(
                    "Opened read-only against other editors: {error}"
                ));
            }
            Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                self.tabs[i].edit_lease = None;
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(&format!(
                    "{error}. External-change checks remain active."
                ));
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
                    self.command_line.push_info(&format!(
                        "{warning}. External-change checks remain active."
                    ));
                }
            }
            Some(Err(crate::io::edit_lock::EditLeaseError::Locked(error))) => {
                self.tabs[i].edit_lock_conflict = true;
                self.command_line.push_error_once(&format!(
                    "Saved, but the refreshed drawing is locked by another editor: {error}"
                ));
            }
            Some(Err(crate::io::edit_lock::EditLeaseError::Unavailable(error))) => {
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(&format!(
                    "{error}. External-change checks remain active."
                ));
            }
            None => self.install_native_edit_guard(i, path, None),
        }
        self.tabs[i].disk_fingerprint =
            crate::io::edit_lock::FileFingerprint::capture(path).ok();
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn save_tab_synchronously_protected(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
        set_current_path: bool,
    ) -> Result<(), crate::io::SaveFailure> {
        let path_changed = self.tabs[i]
            .current_path
            .as_deref()
            .is_none_or(|current| !native_paths_match(current, &path));
        if !path_changed && self.tabs[i].edit_lock_conflict {
            return Err(crate::io::SaveFailure::file_in_use(
                "drawing edit lock is held by another editor",
            ));
        }

        let destination_lease = if path_changed {
            match crate::io::edit_lock::EditLease::acquire(&path) {
                Ok(lease) => Some(lease),
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    return Err(crate::io::SaveFailure::file_in_use(error));
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.command_line.push_info(&format!(
                        "{error}. External-change checks remain active."
                    ));
                    None
                }
            }
        } else {
            None
        };

        let expected_fingerprint = if path_changed {
            match crate::io::edit_lock::FileFingerprint::capture(&path) {
                Ok(fingerprint) => Some(fingerprint),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(crate::io::SaveFailure::other(format!(
                        "could not verify {} before saving: {error}",
                        path.display()
                    )));
                }
            }
        } else {
            self.tabs[i].disk_fingerprint.clone().or_else(|| {
                crate::io::edit_lock::FileFingerprint::capture(&path).ok()
            })
        };

        self.prepare_native_save(i);
        let version = self.tabs[i].scene.document.version;
        let snapshot = self.tabs[i].scene.document.clone();
        crate::io::save_owned_as_version_atomic(
            snapshot,
            &path,
            version,
            self.backup_on_save,
            expected_fingerprint,
        )?;

        if set_current_path {
            self.tabs[i].current_path = Some(path.clone());
        }
        self.refresh_native_edit_guard_after_save(
            i,
            &path,
            path_changed,
            destination_lease,
        );
        self.tabs[i].dirty = false;
        let _ = std::fs::remove_file(path.with_extension("sv$"));
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn retry_native_edit_guard(
        &mut self,
        i: usize,
        path: &std::path::Path,
    ) -> Result<(), String> {
        if let Some(lease) = self.tabs[i].edit_lease.as_mut() {
            match lease.refresh_drawing_lock(path) {
                Ok(()) => {
                    let warning = lease.platform_warning().map(str::to_owned);
                    self.tabs[i].edit_lock_conflict = false;
                    if let Some(warning) = warning {
                        self.command_line.push_info(&format!(
                            "{warning}. External-change checks remain active."
                        ));
                    }
                    return Ok(());
                }
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    return Err(error);
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.tabs[i].edit_lock_conflict = false;
                    self.command_line.push_info(&format!(
                        "{error}. External-change checks remain active."
                    ));
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
                    self.command_line.push_info(&format!(
                        "{warning}. External-change checks remain active."
                    ));
                }
                Ok(())
            }
            Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => Err(error),
            Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                self.tabs[i].edit_lock_conflict = false;
                self.command_line.push_info(&format!(
                    "{error}. External-change checks remain active."
                ));
                Ok(())
            }
        }
    }

    pub(super) fn on_file_opened(&mut self, name: String, path: std::path::PathBuf, doc: acadrust::CadDocument,
        mut caches: crate::scene::DerivedCaches,
    ) -> Task<Message> {
                // If the user clicked Cancel while the parser was running, the
                // overlay state was cleared and we silently drop the result.
                if self.opening.is_none() {
                    return Task::none();
                }
                let open_started = self.opening.as_ref().map(|p| p.started);
                let timings = caches.timings;
                let entity_count = doc.entities().count();
                self.command_line
                    .push_output(&format!("Opened \"{name}\" — {entity_count} entities"));
                if caches.corrupt_dropped > 0 {
                    self.command_line.push_error(&format!(
                        "Warning: {} corrupt entities dropped (parser junk — bad normals / counts)",
                        caches.corrupt_dropped
                    ));
                }
        if caches.xref_dropped > 0 {
            self.command_line.push_error(&format!(
                "Warning: {} corrupt xref entities dropped",
                caches.xref_dropped
            ));
        }
        for info in &caches.xrefs {
            match info.status {
                crate::io::xref::XrefStatus::Loaded => {
                    self.command_line
                        .push_output(&format!("XREF  Loaded \"{}\"", info.name));
                }
                crate::io::xref::XrefStatus::NotFound => {
                    self.command_line.push_error(&format!(
                        "XREF  Not found: \"{}\" ({})",
                        info.name, info.path
                    ));
                }
                crate::io::xref::XrefStatus::Unloaded => {
                    self.command_line
                        .push_info(&format!("XREF  Unloaded (skipped): \"{}\"", info.name));
                }
            }
        }
                let thumbs_task = self.push_recent(path.clone());

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
                    let new_tab = crate::app::document::DocumentTab::new_drawing(self.tab_counter);
                    self.tabs.push(new_tab);
                    let idx = self.tabs.len() - 1;
                    self.active_tab = idx;
                    self.apply_bg_default(idx);
                    idx
                };

                #[cfg(not(target_arch = "wasm32"))]
                let opened_fingerprint = self
                    .opening
                    .as_ref()
                    .and_then(|opening| opening.fingerprint.clone());
                self.tabs[i].current_path = Some(path.clone());
                #[cfg(not(target_arch = "wasm32"))]
                self.install_native_edit_guard(i, &path, opened_fingerprint);
                self.tabs[i].scene.material_base_dir =
                    path.parent().map(std::path::Path::to_path_buf);
                self.tabs[i].scene.document = doc;
                // A file saved without the built-in Standard styles (foreign
                // or damaged) gets them re-seeded so nothing dangles (#366).
                crate::app::style_ops::ensure_standard_styles(
                    &mut self.tabs[i].scene.document,
                );
                // Follow the file's saved current UCS from the moment it opens.
                self.tabs[i].adopt_active_ucs_from_header();
                // Adopt the drawing's own Ortho ($ORTHOMODE) and running OSNAP
                // ($OSMODE) so each file keeps its state, like AutoCAD.
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
                // Current model-space annotation scale comes from the drawing's
                // CANNOSCALEVALUE (paper/drawing factor); the multiplier we use
                // for text/dim sizing is its inverse (1:50 -> 0.02 -> 50.0).
                let cannoscale_value = self.tabs[i].scene.document.header.annotation_scale_value;
                self.tabs[i].scene.annotation_scale = if cannoscale_value > 1e-9 {
                    (1.0 / cannoscale_value) as f32
                } else {
                    1.0
                };

                // Open-time breakdown so regressions are visible immediately.
                // `total` is wall time from the Open click to here (post-xref,
                // pre-first-frame); the phase figures are the background-thread
                // parse/purge/cache spans plus the UI-thread xref resolve.
                let total_ms = open_started
                    .map(|s| s.elapsed().as_millis() as u32)
                    .unwrap_or(0);
                self.command_line.push_info(&format!(
                    "  parse {}ms · purge {}ms · caches {}ms · xref {}ms · total {}ms",
                    timings.parse_ms, timings.purge_ms, timings.caches_ms, timings.xref_ms, total_ms
                ));

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
                self.tabs[i].scene.selected = rustc_hash::FxHashSet::default();
                self.tabs[i].scene.preview_wires = vec![];
                // Reopen in whichever space the file was saved in — the CTAB
                // tab name when recorded, else the $TILEMODE model/paper flag —
                // instead of always landing in Model.
                {
                    let names = self.tabs[i].scene.layout_names();
                    let saved = crate::io::saved_active_layout(&self.tabs[i].scene.document)
                        .and_then(|n| {
                            names.iter().find(|x| x.eq_ignore_ascii_case(&n)).cloned()
                        });
                    self.tabs[i].scene.current_layout = match saved {
                        Some(n) => n,
                        None if self.tabs[i].scene.document.header.show_model_space => {
                            "Model".to_string()
                        }
                        // Paper was active but no CTAB — fall back to the first
                        // paper layout (names[0] is always "Model").
                        None => names
                            .into_iter()
                            .nth(1)
                            .unwrap_or_else(|| "Model".to_string()),
                    };
                }
                // Object isolation is session-only. A newly opened drawing must
                // not inherit the previous tab's filter, and persisted entity
                // visibility remains independent (not an isolation session).
                self.tabs[i].scene.reset_transient_visibility();
                crate::io::linetypes::populate_document(&mut self.tabs[i].scene.document);
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
                self.tabs[i].dirty = false;
                self.tabs[i].history = crate::app::document::HistoryState::default();
                self.refresh_properties();
                #[cfg(not(target_arch = "wasm32"))]
                let interaction_task = {
                    let wires = self.tabs[i].scene.hit_test_wires();
                    let screen_height = self.tabs[i].scene.selection.borrow().vp_size.1;
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
                let pending_open_task = self.drain_pending_open();
                Task::batch([thumbs_task, pending_open_task, interaction_task])
    }

    pub(super) fn on_wblock_save_result_some(&mut self, block_name: String, path: std::path::PathBuf,
    ) -> Task<Message> {
                let i = self.active_tab;
                let document = self.tabs[i].scene.document.clone();
                    let handles: Vec<_> = self.tabs[i].scene.selected.iter().copied().collect();
        let worker_name = block_name.clone();
        let worker_path = path.clone();
        background_task(
            move || {
                let document = if worker_name == "*" {
                    crate::modules::insert::wblock::extract_entities_to_doc(
                        &document,
                        &handles,
                    )
                } else {
                    crate::modules::insert::wblock::extract_block_to_doc(
                        &document,
                        &worker_name)
                }
                .map_err(|e| e.to_string())?; crate::io::save(&document, &worker_path).map_err(|e| e.to_string())
                    },
            move |result| Message::WblockWriteFinished(block_name, path, result),)
    }

    pub(super) fn on_stl_export_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
                // Re-build STL bytes (we can't easily pass them through the message).
                let i = self.active_tab;
                // STL gets the highest-resolution LOD (slot 0) so the
                // exported geometry isn't downgraded by the view-dependent
                // mesh LOD ladder used for rendering.
                let meshes: Vec<crate::scene::model::mesh_model::MeshModel> = self.tabs[i]
                    .scene
                    .meshes
                    .values()
                    .filter_map(|s| s.lods.first().cloned())
                    .collect();
                let worker_path = path.clone();
        background_task(
            move || {
                let mesh_refs: Vec<_> = meshes.iter().collect();
                let bytes = crate::io::stl::build_stl(&mesh_refs)
                    .ok_or_else(|| "no mesh data to export".to_string())?;
                std::fs::write(&worker_path, bytes).map_err(|e| e.to_string())
                    },
            move |result| Message::StlExportFinished(path, result),)
    }

    pub(super) fn on_step_export_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
                let i = self.active_tab;
                // Export uses LOD 0 (full resolution); see StlExportPath above.
                let meshes: Vec<crate::scene::model::mesh_model::MeshModel> = self.tabs[i]
                    .scene
                    .meshes
                    .values()
                    .filter_map(|s| s.lods.first().cloned())
                    .collect();
                let worker_path = path.clone();
        background_task(
            move || {
                let mesh_refs: Vec<_> = meshes.iter().collect();
                let text = crate::io::step::build_step(&mesh_refs)
                    .ok_or_else(|| "no mesh data to export".to_string())?;
                std::fs::write(&worker_path, text.as_bytes()).map_err(|e| e.to_string())
                    },
            move |result| Message::StepExportFinished(path, result),)
    }

    pub(super) fn on_obj_import_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
                let tab_id = self.tabs[self.active_tab].id;
        let worker_path = path.clone();
        background_task(
            move || {
                let src = std::fs::read_to_string(&worker_path).map_err(|e| e.to_string())?; crate::io::obj::parse_obj(&src, [0.7, 0.7, 0.85, 1.0])
                            .ok_or_else(|| "no usable geometry in file".to_string())
            },
            move |result| Message::ObjImportFinished(tab_id, path, result),)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn prepare_native_save(&mut self, i: usize) {
        self.sync_vport_display(i);
        sync_annotation_scale_header(&mut self.tabs[i].scene);
        self.stamp_header_sysvars(i);
        self.tabs[i].scene.document.header.user_real1 =
            self.tabs[i].scene.annotation_scale as f64;
        self.sync_truck_solids_to_acis(i);
    }

    #[cfg(target_arch = "wasm32")]
    fn stamp_thumbnail(&mut self, i: usize, version: acadrust::DxfVersion) {
        let scene = &self.tabs[i].scene;
        let preview = crate::io::thumbnail::from_snapshot(
            &scene.entity_wires(),
            &scene.camera.borrow(),
            scene.bg_color,
            version >= acadrust::DxfVersion::AC1027,
            self.vp_size,
        );
        self.tabs[i].scene.document.preview = preview;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn queue_native_save(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
        version: acadrust::DxfVersion,
        purpose: crate::app::SavePurpose,
        continuation: crate::app::SaveContinuation,
        set_current_path: bool,
        check_external_change: bool,
    ) -> Task<Message> {
        let tab_id = self.tabs[i].id;
        if self.active_save_jobs.contains_key(&tab_id) {
            if purpose != crate::app::SavePurpose::Autosave {
                self.command_line
                    .push_info("Save already running for this drawing.");
            }
            return Task::none();
        }
        if purpose != crate::app::SavePurpose::Autosave
            && !set_current_path
            && self.tabs[i].edit_lock_conflict
        {
            let error = "Drawing edit lock is held by another editor.".to_string();
            self.command_line.push_error_once(&format!(
                "Unable to save \"{}\": {error}",
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string())
            ));
            self.pending_save_failure = Some(crate::app::PendingSaveFailure {
                tab_id,
                path,
                version,
                purpose,
                continuation,
                set_current_path,
                error,
            });
            self.restore_failed_save_continuation(continuation, i);
            self.active_modal = Some(crate::app::ModalKind::FileInUse);
            return Task::none();
        }

        let destination_is_current = self.tabs[i]
            .current_path
            .as_deref()
            .is_some_and(|current| native_paths_match(current, &path));
        if set_current_path && !destination_is_current {
            match crate::io::edit_lock::EditLease::acquire(&path) {
                Ok(lease) => {
                    self.pending_save_leases.insert(tab_id, lease);
                }
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    self.command_line.push_error_once(&format!(
                        "Unable to save \"{}\": {error}",
                        path.file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string())
                    ));
                    self.pending_save_failure = Some(crate::app::PendingSaveFailure {
                        tab_id,
                        path,
                        version,
                        purpose,
                        continuation,
                        set_current_path,
                        error,
                    });
                    self.restore_failed_save_continuation(continuation, i);
                    self.active_modal = Some(crate::app::ModalKind::FileInUse);
                    return Task::none();
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.command_line.push_info(&format!(
                        "{error}. External-change checks remain active."
                    ));
                }
            }
        }

        let epoch = self.tabs[i].scene.geometry_epoch;
        let revision = self.tabs[i].edit_revision;
        let camera_generation = self.tabs[i].scene.camera_generation;
        let thumbnail_key = (purpose != crate::app::SavePurpose::Autosave).then(|| {
            let scene = &self.tabs[i].scene;
            crate::app::ThumbnailCacheKey {
                epoch,
                camera_generation,
                bg_color: scene.bg_color.map(f32::to_bits),
                png: version >= acadrust::DxfVersion::AC1027,
                viewport: [self.vp_size.0.to_bits(), self.vp_size.1.to_bits()],
            }
        });
        let thumbnail = if thumbnail_key == self.tabs[i].thumbnail_cache_key {
            None
        } else {
            thumbnail_key.map(|key| {
                let scene = &self.tabs[i].scene;
                (
                    scene.entity_wires(),
                    scene.camera.borrow().clone(),
                    scene.bg_color,
                    key.png,
                    self.vp_size,
                )
            })
        };
        let clone_started = iced::time::Instant::now();
        let mut snapshot = self.tabs[i].scene.document.clone();
        let clone_ms = clone_started.elapsed().as_secs_f64() * 1000.0;
        if crate::perf::enabled() {
            crate::perf_record!(
                "[perf] save-snapshot {:.1}ms entities={} objects={} purpose={purpose:?}",
                clone_ms,
                snapshot.entities().count(),
                snapshot.objects.len(),
            );
        }

        self.save_job_serial = self.save_job_serial.wrapping_add(1);
        let job_id = self.save_job_serial;
        self.active_save_jobs.insert(tab_id, job_id);
        let previous_autosave = set_current_path.then(|| self.autosave_target(i));
        let backup = purpose != crate::app::SavePurpose::Autosave && self.backup_on_save;
        let expected_fingerprint =
            if check_external_change && purpose != crate::app::SavePurpose::Autosave {
                if set_current_path {
                    crate::io::edit_lock::FileFingerprint::capture(&path).ok()
                } else {
                    self.tabs[i].disk_fingerprint.clone()
                }
            } else {
                None
            };
        let worker_path = path.clone();

        Task::perform(
            async move {
                let (result, refreshed_preview) = std::thread::spawn(move || {
                    let mut refreshed_preview = None;
                    if let Some((wires, camera, bg_color, png, viewport)) = thumbnail {
                        let started = iced::time::Instant::now();
                        snapshot.preview = crate::io::thumbnail::from_snapshot(
                            &wires,
                            &camera,
                            bg_color,
                            png,
                            viewport,
                        );
                        if crate::perf::enabled() {
                            crate::perf_record!(
                                "[perf] save-thumbnail {:.1}ms wires={}",
                                started.elapsed().as_secs_f64() * 1000.0,
                                wires.len(),
                            );
                        }
                        refreshed_preview = Some(snapshot.preview.clone());
                    }
                    let result = crate::io::save_owned_as_version_atomic(
                        snapshot,
                        &worker_path,
                        version,
                        backup,
                        expected_fingerprint,
                    );
                    (result, refreshed_preview)
                })
                .join()
                .unwrap_or_else(|_| {
                    (
                        Err(crate::io::SaveFailure::other("save worker panicked")),
                        None,
                    )
                });
                crate::app::SaveOutcome {
                    job_id,
                    tab_id,
                    epoch,
                    revision,
                    camera_generation,
                    path,
                    version,
                    previous_autosave,
                    set_current_path,
                    purpose,
                    continuation,
                    thumbnail_key,
                    refreshed_preview,
                    result,
                }
            },
            Message::SaveFinished,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_save_finished(
        &mut self,
        outcome: crate::app::SaveOutcome,
    ) -> Task<Message> {
        let latest = self.active_save_jobs.get(&outcome.tab_id).copied()
            == Some(outcome.job_id);
        if latest {
            self.active_save_jobs.remove(&outcome.tab_id);
        }
        let destination_lease = self.pending_save_leases.remove(&outcome.tab_id);

        let Some(i) = self.tabs.iter().position(|tab| tab.id == outcome.tab_id) else {
            if outcome.purpose == crate::app::SavePurpose::Autosave {
                let _ = std::fs::remove_file(&outcome.path);
            }
            return Task::none();
        };
        if !latest {
            if outcome.purpose == crate::app::SavePurpose::Autosave {
                let _ = std::fs::remove_file(&outcome.path);
            }
            return Task::none();
        }

        if let Err(error) = &outcome.result {
            if error.externally_modified
                && outcome.purpose != crate::app::SavePurpose::Autosave
            {
                let file_name = outcome
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| outcome.path.display().to_string());
                self.command_line.push_error_once(&format!(
                    "Save stopped: \"{file_name}\" changed outside Open CAD Studio."
                ));
                self.pending_external_change = Some(crate::app::PendingExternalChange {
                    tab_id: outcome.tab_id,
                    path: outcome.path.clone(),
                    version: outcome.version,
                    purpose: outcome.purpose,
                    continuation: outcome.continuation,
                    set_current_path: outcome.set_current_path,
                });
                self.restore_failed_save_continuation(outcome.continuation, i);
                self.active_modal = Some(crate::app::ModalKind::ExternalChange);
                return Task::none();
            }
            if error.file_in_use && outcome.purpose != crate::app::SavePurpose::Autosave {
                let file_name = outcome
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| outcome.path.display().to_string());
                self.command_line.push_error_once(&format!(
                    "Unable to save \"{file_name}\": file is in use by another application."
                ));
                self.pending_save_failure = Some(crate::app::PendingSaveFailure {
                    tab_id: outcome.tab_id,
                    path: outcome.path.clone(),
                    version: outcome.version,
                    purpose: outcome.purpose,
                    continuation: outcome.continuation,
                    set_current_path: outcome.set_current_path,
                    error: error.to_string(),
                });
                match outcome.continuation {
                    crate::app::SaveContinuation::CloseTab => {
                        self.pending_close = Some(crate::app::PendingClose::Tab(i));
                    }
                    crate::app::SaveContinuation::Quit => {
                        self.pending_close = Some(crate::app::PendingClose::Quit);
                    }
                    crate::app::SaveContinuation::None => {}
                }
                self.active_modal = Some(crate::app::ModalKind::FileInUse);
                return Task::none();
            }
            self.command_line
                .push_error(&format!("Save failed: {error}"));
            return match outcome.continuation {
                crate::app::SaveContinuation::CloseTab => {
                    self.pending_close = Some(crate::app::PendingClose::Tab(i));
                    self.open_unsaved_dialog_window()
                }
                crate::app::SaveContinuation::Quit => {
                    self.pending_close = Some(crate::app::PendingClose::Quit);
                    self.open_unsaved_dialog_window()
                }
                crate::app::SaveContinuation::None => Task::none(),
            };
        }

        let snapshot_is_current = self.tabs[i].scene.geometry_epoch == outcome.epoch
            && self.tabs[i].edit_revision == outcome.revision
            && self.tabs[i].scene.camera_generation == outcome.camera_generation;
        if snapshot_is_current && outcome.purpose != crate::app::SavePurpose::Autosave {
            if let Some(preview) = outcome.refreshed_preview {
                self.tabs[i].scene.document.preview = preview;
            }
            self.tabs[i].thumbnail_cache_key = outcome.thumbnail_key;
        }
        let mut tasks = Vec::new();
        match outcome.purpose {
            crate::app::SavePurpose::Autosave => {
                self.command_line.push_output("Autosaved 1 drawing");
            }
            crate::app::SavePurpose::Manual | crate::app::SavePurpose::SaveAs => {
                let path_changed = outcome.set_current_path
                    && self.tabs[i]
                        .current_path
                        .as_deref()
                        .is_none_or(|current| {
                            !native_paths_match(current, &outcome.path)
                        });
                self.command_line
                    .push_output(&format!("Saved: {}", outcome.path.display()));
                self.recent_thumbs.remove(&outcome.path);
                if let Some(previous) = outcome.previous_autosave {
                    if previous != outcome.path {
                        let _ = std::fs::remove_file(previous);
                    }
                }
                if outcome.set_current_path {
                    self.tabs[i].current_path = Some(outcome.path.clone());
                    self.tabs[i].scene.document.version = outcome.version;
                    tasks.push(self.push_recent(outcome.path.clone()));
                }
                self.refresh_native_edit_guard_after_save(
                    i,
                    &outcome.path,
                    path_changed,
                    destination_lease,
                );
                if snapshot_is_current {
                    self.tabs[i].dirty = false;
                    let _ = std::fs::remove_file(outcome.path.with_extension("sv$"));
                }
            }
        }

        match outcome.continuation {
            crate::app::SaveContinuation::None => {}
            crate::app::SaveContinuation::CloseTab if snapshot_is_current => {
                self.pending_close = None;
                tasks.push(self.close_unsaved_dialog_window());
                tasks.push(self.update(Message::TabClose(i)));
                tasks.push(self.continue_tab_close_queue());
            }
            crate::app::SaveContinuation::Quit if snapshot_is_current => {
                self.pending_close = None;
                if self.tabs.iter().any(|tab| tab.dirty) {
                    self.pending_close = Some(crate::app::PendingClose::Quit);
                    tasks.push(self.open_unsaved_dialog_window());
                } else {
                    tasks.push(self.close_unsaved_dialog_window());
                    tasks.push(self.exit_app());
                }
            }
            crate::app::SaveContinuation::CloseTab => {
                self.pending_close = Some(crate::app::PendingClose::Tab(i));
                tasks.push(self.open_unsaved_dialog_window());
            }
            crate::app::SaveContinuation::Quit => {
                self.pending_close = Some(crate::app::PendingClose::Quit);
                tasks.push(self.open_unsaved_dialog_window());
            }
        }
        Task::batch(tasks)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn restore_failed_save_continuation(
        &mut self,
        continuation: crate::app::SaveContinuation,
        tab_idx: usize,
    ) {
        self.pending_close = match continuation {
            crate::app::SaveContinuation::None => None,
            crate::app::SaveContinuation::CloseTab => {
                Some(crate::app::PendingClose::Tab(tab_idx))
            }
            crate::app::SaveContinuation::Quit => Some(crate::app::PendingClose::Quit),
        };
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_save_file_in_use_retry(&mut self) -> Task<Message> {
        let Some(mut failure) = self.pending_save_failure.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == failure.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        if self.tabs[i].edit_lock_conflict {
            if let Err(error) = self.retry_native_edit_guard(i, &failure.path) {
                let continuation = failure.continuation;
                failure.error = error.clone();
                self.pending_save_failure = Some(failure);
                self.restore_failed_save_continuation(continuation, i);
                self.command_line
                    .push_error_once(&format!("Unable to acquire edit lock: {error}"));
                self.active_modal = Some(crate::app::ModalKind::FileInUse);
                return Task::none();
            }
        }
        self.close_active_modal();
        self.restore_failed_save_continuation(failure.continuation, i);
        self.active_tab = i;
        self.prepare_native_save(i);
        self.queue_native_save(
            i,
            failure.path,
            failure.version,
            failure.purpose,
            failure.continuation,
            failure.set_current_path,
            true,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_save_file_in_use_save_as(&mut self) -> Task<Message> {
        let Some(failure) = self.pending_save_failure.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == failure.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        self.restore_failed_save_continuation(failure.continuation, i);
        self.active_tab = i;
        self.save_dialog_for_unsaved =
            failure.continuation != crate::app::SaveContinuation::None;
        self.open_save_dialog_window(i)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_external_change_overwrite(&mut self) -> Task<Message> {
        let Some(conflict) = self.pending_external_change.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == conflict.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        self.restore_failed_save_continuation(conflict.continuation, i);
        self.active_tab = i;
        self.prepare_native_save(i);
        self.queue_native_save(
            i,
            conflict.path,
            conflict.version,
            conflict.purpose,
            conflict.continuation,
            conflict.set_current_path,
            false,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_external_change_save_as(&mut self) -> Task<Message> {
        let Some(conflict) = self.pending_external_change.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == conflict.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        self.close_active_modal();
        self.restore_failed_save_continuation(conflict.continuation, i);
        self.active_tab = i;
        self.save_dialog_for_unsaved =
            conflict.continuation != crate::app::SaveContinuation::None;
        self.open_save_dialog_window(i)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_external_change_reload(&mut self) -> Task<Message> {
        let Some(conflict) = self.pending_external_change.take() else {
            self.close_active_modal();
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == conflict.tab_id) else {
            self.close_active_modal();
            return Task::none();
        };
        let metadata = match std::fs::metadata(&conflict.path) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.close_active_modal();
                self.command_line
                    .push_error(&format!("Reload failed: {error}"));
                return Task::none();
            }
        };
        self.close_active_modal();
        self.pending_close = None;
        self.tab_counter += 1;
        self.tabs[i] = crate::app::document::DocumentTab::new_drawing(self.tab_counter);
        self.active_tab = i;
        self.apply_bg_default(i);
        self.update(Message::OpenPathPicked(Some((
            conflict.path,
            metadata.len(),
        ))))
    }

    pub(super) fn on_save_file(&mut self) -> Task<Message> {
                if self.read_only {
                    self.command_line
                        .push_error("Read-only session (--read-only): saving is disabled.");
                    return Task::none();
                }
                let i = self.active_tab;
                // Web serializes immediately below. Native preparation happens
                // once after the destination/version is known.
                #[cfg(target_arch = "wasm32")]
                {
                    self.sync_vport_display(i);
                    self.stamp_header_sysvars(i);
                }
                // Native: save straight to the known path. Web has no path
                // (downloads instead), so always go through the Save dialog.
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(path) = self.tabs[i].current_path.clone() {
                    // A direct Save preserves the document's current version.
                    let ver = self.tabs[i].scene.document.version;
                    self.prepare_native_save(i);
                    return self.queue_native_save(
                        i,
                        path,
                        ver,
                        crate::app::SavePurpose::Manual,
                        crate::app::SaveContinuation::None,
                        false,
                        true,
                    );
                }
                self.save_dialog_for_unsaved = false;
                self.save_with_default_format(i)
    }

    /// Save without the version picker: use the configured default and go straight to
    /// the native destination dialog (native) or the browser download (web).
    /// Used by plain Save (QSAVE) on an as-yet-unsaved drawing and by the
    /// save-before-close flow — the version picker is reserved for Save As.
    pub(in crate::app) fn save_with_default_format(&mut self, tab_idx: usize) -> Task<Message> {
        self.active_tab = tab_idx;
        self.save_dialog_format = self.default_save_format.clone();
        let (ext, _) = crate::io::parse_save_format(&self.save_dialog_format);
        self.save_dialog_filename = self.tabs[tab_idx]
            .current_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.{ext}", self.tabs[tab_idx].tab_display_name()));
        self.aec_drop_acknowledged = false;
        self.on_save_dialog_confirm()
    }

    pub(super) fn on_save_dialog_confirm(&mut self) -> Task<Message> {
                let (ext, version) = crate::io::parse_save_format(&self.save_dialog_format);
                // Warn before a lossy Save-As that would drop unsupported
                // (AEC / application) objects kept only as verbatim
                // source-version bytes — let the user keep them by saving in the
                // source version, or proceed and drop them.
                if !self.aec_drop_acknowledged {
                    let is_dxf = ext.eq_ignore_ascii_case("dxf");
                    let n = crate::io::dropped_on_save_count(
                        &self.tabs[self.active_tab].scene.document,
                        version,
                        is_dxf,
                    );
                    if n > 0 {
                        self.aec_drop_count = n;
                        self.active_modal = Some(crate::app::ModalKind::AecDropWarning);
                        return Task::none();
                    }
                }
                // The user need not type an extension: append the selected
                // format's one when the entered name carries none.
                let name = self.save_dialog_filename.trim();
                let filename = if name.is_empty() {
                    format!("drawing.{ext}")
                } else if std::path::Path::new(name).extension().is_none() {
                    format!("{name}.{ext}")
                } else {
                    name.to_string()
                };
                self.save_dialog_filename = filename.clone();
                let i = self.active_tab;

                // Native: hand the destination choice to the OS save dialog —
                // it provides folder browsing and overwrite confirmation. The
                // chosen version rides in `save_dialog_format` and the write
                // happens in `on_save_dialog_path_picked` once a path returns.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let seed_dir = self.tabs[i]
                        .current_path
                        .as_ref()
                        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
                    let default_name = filename;
                    let (filter_label, filter_ext): (&str, &str) =
                        if ext.eq_ignore_ascii_case("dxf") {
                            ("DXF Files", "dxf")
                        } else {
                            ("DWG Files", "dwg")
                        };
                    let close = self.close_save_dialog_window();
                    let pick = Task::perform(
                        async move {
                            let mut dlg = crate::sys::file_dialog()
                                .set_title("Save Drawing As")
                                .set_file_name(default_name)
                                .add_filter(filter_label, &[filter_ext]);
                            if let Some(dir) = seed_dir {
                                dlg = dlg.set_directory(dir);
                            }
                            dlg.save_file().await.map(|h| crate::sys::handle_path(&h))
                        },
                        Message::SaveDialogPathPicked,
                    );
                    Task::batch([close, pick])
                }
                // Web: no filesystem — serialize and hand the browser a download.
                #[cfg(target_arch = "wasm32")]
                {
                    let close = self.close_save_dialog_window();
                    sync_annotation_scale_header(&mut self.tabs[i].scene);
                    self.stamp_header_sysvars(i);
                    self.sync_truck_solids_to_acis(i);
                    self.stamp_thumbnail(i, version);
                    let saved =
                        match crate::io::save_to_bytes(&self.tabs[i].scene.document, ext, version) {
                        Ok(bytes) => {
                            crate::sys::download_bytes(&filename, &bytes);
                            self.tabs[i].scene.document.version = version;
                            self.tabs[i].dirty = false;
                            self.command_line.push_output(&format!("Saved: {filename}"));
                            true
                        }
                        Err(e) => {
                            self.command_line.push_error(&format!("Save failed: {e}"));
                            false
                        }
                    };
                    // Continue a pending tab close.
                    if self.save_dialog_for_unsaved {
                        if saved {
                            if let Some(crate::app::PendingClose::Tab(idx)) =
                                self.pending_close.take()
                            {
                                let cont = self.update(Message::TabClose(idx));
                                let rest = self.continue_tab_close_queue();
                                return Task::batch([close, cont, rest]);
                            }
                        } else if self.pending_close.is_some() {
                            let retry = self.open_unsaved_dialog_window();
                            return Task::batch([close, retry]);
                        }
                    }
                    close
                }
    }

    /// Native: a destination path came back from the OS save dialog — write the
    /// file there in the chosen version. `None` means the user cancelled.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_save_dialog_path_picked(
        &mut self,
        picked: Option<std::path::PathBuf>,
    ) -> Task<Message> {
        let Some(path) = picked else {
            // Cancelled the OS dialog — if this came from the unsaved-changes →
            // save → close flow, re-show the confirmation so the tab isn't lost.
            if self.save_dialog_for_unsaved && self.pending_close.is_some() {
                return self.open_unsaved_dialog_window();
            }
            return Task::none();
        };
        let (_ext, version) = crate::io::parse_save_format(&self.save_dialog_format);
        let i = self.active_tab;
        self.prepare_native_save(i);
        let continuation = if self.save_dialog_for_unsaved {
            match self.pending_close {
                Some(crate::app::PendingClose::Tab(_)) => crate::app::SaveContinuation::CloseTab,
                Some(crate::app::PendingClose::Quit) => crate::app::SaveContinuation::Quit,
                None => crate::app::SaveContinuation::None,
            }
        } else {
            crate::app::SaveContinuation::None
        };
        self.queue_native_save(
            i,
            path,
            version,
            crate::app::SavePurpose::SaveAs,
            continuation,
            true,
            true,
        )
    }

    /// AEC-drop warning → "Save anyway": accept the loss and proceed with the
    /// format the user already chose.
    pub(super) fn on_aec_drop_proceed(&mut self) -> Task<Message> {
        self.aec_drop_acknowledged = true;
        self.active_modal = Some(crate::app::ModalKind::SaveDialog);
        self.on_save_dialog_confirm()
    }

    /// AEC-drop warning → "Save in source version": switch the target to the
    /// document's source type and version, then save.
    pub(super) fn on_aec_drop_same_version(&mut self) -> Task<Message> {
        let tab = &self.tabs[self.active_tab];
        let is_dxf = tab
            .current_path
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("dxf"))
            .unwrap_or(false);
        let document = &tab.scene.document;
        let src = if is_dxf {
            document.version
        } else {
            document.dwg_source_version.unwrap_or(document.version)
        };
        self.save_dialog_format = crate::io::format_for_version(src, is_dxf);
        // Strip the old extension so the confirm path appends the source type.
        let stem = std::path::Path::new(&self.save_dialog_filename)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.save_dialog_filename.clone());
        self.save_dialog_filename = stem;
        self.aec_drop_acknowledged = true;
        self.active_modal = Some(crate::app::ModalKind::SaveDialog);
        self.on_save_dialog_confirm()
    }

    /// Where the autosave recovery copy for tab `i` lives: beside a saved
    /// drawing as `<file>.sv$`, or — for an unsaved drawing with no path yet —
    /// under the system temp dir keyed by the tab's display name.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn autosave_target(&self, i: usize) -> std::path::PathBuf {
        match &self.tabs[i].current_path {
            Some(p) => p.with_extension("sv$"),
            None => {
                let safe: String = self.tabs[i]
                    .tab_display_name()
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect();
                std::env::temp_dir().join(format!("OpenCADStudio_{safe}.sv$"))
            }
        }
    }

    /// Periodic autosave (SAVETIME): write a `.sv$` recovery copy for every
    /// dirty tab — beside the file if it's saved, else under the temp dir — at
    /// the document's own DWG version. Best-effort and non-destructive: it never
    /// touches the original file or the dirty flag.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn on_autosave(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for i in 0..self.tabs.len() {
            if !self.tabs[i].dirty
                || self.active_save_jobs.contains_key(&self.tabs[i].id)
            {
                continue;
            }
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

    #[cfg(target_arch = "wasm32")]
    pub(super) fn on_autosave(&mut self) -> Task<Message> {
        Task::none()
    }

    /// Delete the `.sv$` autosave recovery files for all open drawings. They
    /// exist only to survive a crash, so a clean save or exit removes them.
    pub(in crate::app) fn cleanup_autosaves(&self) {
        #[cfg(not(target_arch = "wasm32"))]
        for i in 0..self.tabs.len() {
            let _ = std::fs::remove_file(self.autosave_target(i));
        }
    }

    /// Remove the autosave recovery files, then quit the application.
    pub(in crate::app) fn exit_app(&self) -> Task<Message> {
        self.cleanup_autosaves();
        iced::exit()
    }

    /// Write the given plot page settings into the active layout's Layout +
    /// PlotSettings objects (paper size, plot area, offset, rotation, scale).
    /// No-op on the Model tab (which has no paper layout). Marks the tab dirty
    /// and re-tessellates the sheet. Called by the Plot dialog on commit.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn apply_plot_page_settings(
        &mut self,
        w: f64,
        h: f64,
        extents: bool,
        center: bool,
        offset_x: f64,
        offset_y: f64,
        rotation: i16,
        scale_str: &str,
    ) {
                let i = self.active_tab;
                let layout_name = self.tabs[i].scene.current_layout.clone();
                if layout_name != "Model" {
                    let w: f64 = w.max(1.0);
                    let h: f64 = h.max(1.0);
                    let plot_area = if extents { "Extents" } else { "Layout" };

                    // Update the Layout object's limits AND its embedded
                    // PlotSettings fields. `paper_limits()` (sheet rendering) and
                    // the DWG writer both read these from the Layout, so a page
                    // setup that only touched a side PlotSettings object would not
                    // reflect on screen or survive a save. The dialog's w/h are
                    // the final sheet dimensions, so store them verbatim with no
                    // further rotation swap (#156).
                    for obj in self.tabs[i].scene.document.objects.values_mut() {
                        if let acadrust::objects::ObjectType::Layout(l) = obj {
                            if l.name == layout_name {
                                l.min_limits = (0.0, 0.0);
                                l.max_limits = (w, h);
                                l.min_extents = (0.0, 0.0, 0.0);
                                l.max_extents = (w, h, 0.0);
                                l.paper_width = w;
                                l.paper_height = h;
                                l.plot_rotation = 0;
                                l.plot_paper_units = 1; // millimetres
                                l.plot_origin_x = offset_x;
                                l.plot_origin_y = offset_y;
                                // Custom dimensions no longer match a named size.
                                l.paper_size = String::new();
                                break;
                            }
                        }
                    }

                    // Find or create the PlotSettings object for this layout.
                    use acadrust::objects::{
                        ObjectType, PlotPaperUnits, PlotRotation, PlotSettings, PlotType,
                    };
                    let plot_handle =
                        self.tabs[i]
                            .scene
                            .document
                            .objects
                            .iter()
                            .find_map(|(h, obj)| {
                                if let ObjectType::PlotSettings(ps) = obj {
                                    if ps.page_name == layout_name {
                                        Some(*h)
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            });

                    let ps_entry = if let Some(h) = plot_handle {
                        self.tabs[i].scene.document.objects.get_mut(&h)
                    } else {
                        // Create a new PlotSettings object and insert it.
                        let mut ps = PlotSettings::new(layout_name.clone());
                        ps.handle = self.tabs[i].scene.document.allocate_handle();
                        let h = ps.handle;
                        self.tabs[i]
                            .scene
                            .document
                            .objects
                            .insert(h, ObjectType::PlotSettings(ps));
                        self.tabs[i].scene.document.objects.get_mut(&h)
                    };

                    if let Some(ObjectType::PlotSettings(ps)) = ps_entry {
                        ps.paper_width = w;
                        ps.paper_height = h;
                        ps.paper_units = PlotPaperUnits::Millimeters;
                        ps.plot_type = if plot_area == "Extents" {
                            PlotType::Extents
                        } else {
                            PlotType::Layout
                        };
                        ps.flags.plot_centered = center;
                        ps.origin_x = offset_x;
                        ps.origin_y = offset_y;
                        ps.rotation = match rotation {
                            90 => PlotRotation::Degrees90,
                            180 => PlotRotation::Degrees180,
                            270 => PlotRotation::Degrees270,
                            _ => PlotRotation::None,
                        };
                        // Apply plot scale.
                        use acadrust::objects::ScaledType;
                        let (num, den) = parse_plot_scale(scale_str);
                        if scale_str == "Fit" {
                            ps.set_scale_to_fit();
                        } else {
                            ps.scale_type = ScaledType::CustomScale;
                            ps.scale_numerator = num;
                            ps.scale_denominator = den;
                        }
                    }

                    self.tabs[i].dirty = true;
                    // The paper sheet fill is cached; bump geometry so the new
                    // sheet size re-tessellates and shows immediately.
                    self.tabs[i].scene.bump_geometry();
                    self.command_line.push_info(&format!(
                        "Page setup: {w:.1}×{h:.1} mm  area={plot_area}  \
                         center={center}  rot={rotation}°"
                    ));
                }
    }

    pub(in crate::app) fn on_plot_export_path_some(
        &mut self,
        path: std::path::PathBuf,
    ) -> Task<Message> {
                let i = self.active_tab;
                let scene = &self.tabs[i].scene;
                let wires = scene.entity_wires();
                let hatches = scene.paper_canvas_hatches();
                let wipeouts = scene.paper_canvas_wipeouts();

                // Read PlotSettings for current layout (if available).
                use acadrust::objects::PlotType;
                let ps_snap = scene.effective_plot_settings();

                // Determine paper size and drawing offset.
                let (paper_w, paper_h, mut draw_ox, mut draw_oy, rotation_deg) =
                    if let Some(((x0, y0), (x1, y1))) = scene.paper_limits() {
                        // `paper_limits()` is in paper-space units; the PDF page is
                // physical millimetres, so scale the sheet size back to mm via
                // the layout's unit factor (identity for millimetre layouts).
                let uf = scene.paper_space_unit_factor();
                let (pw, ph) = ((x1 - x0) / uf, (y1 - y0) / uf);

                        // If PlotSettings says Extents, use model space extents instead.
                        let use_extents = ps_snap
                            .as_ref()
                            .map(|ps| matches!(ps.plot_type, PlotType::Extents))
                            .unwrap_or(false);

                        let (ox, oy) = if use_extents {
                            if let Some((mn, _mx)) = scene.model_space_extents() {
                                (-mn.x as f64, -mn.y as f64)
                            } else {
                                (-x0, -y0)
                            }
                        } else {
                            (-x0, -y0)
                        };

                        let rot = ps_snap
                            .as_ref()
                            .map(|ps| ps.rotation.to_degrees() as i32)
                            .unwrap_or(0);

                        (pw, ph, ox, oy, rot)
                    } else {
                        // Model space: fit with 5% margin.
                        let margin = 1.05_f64;
                        if let Some((mn, mx)) = scene.model_space_extents() {
                            let w = ((mx.x - mn.x) as f64 * margin).max(1.0);
                            let h = ((mx.y - mn.y) as f64 * margin).max(1.0);
                            let pad_x = (w - (mx.x - mn.x) as f64) * 0.5;
                            let pad_y = (h - (mx.y - mn.y) as f64) * 0.5;
                            (w, h, -(mn.x as f64) + pad_x, -(mn.y as f64) + pad_y, 0)
                        } else {
                            (297.0, 210.0, 0.0, 0.0, 0)
                        }
                    };

                // Apply PlotSettings offset and centering.
                if let Some(ref ps) = ps_snap {
                    if ps.flags.plot_centered {
                        // Centering: compute wire extents and re-centre.
                        let all_x: Vec<f32> = wires
                            .iter()
                            .flat_map(|w| w.points.iter().map(|p| p[0]))
                            .filter(|v| !v.is_nan())
                            .collect();
                        let all_y: Vec<f32> = wires
                            .iter()
                            .flat_map(|w| w.points.iter().map(|p| p[1]))
                            .filter(|v| !v.is_nan())
                            .collect();
                        if let (Some(&min_x), Some(&max_x), Some(&min_y), Some(&max_y)) = (
                            all_x.iter().copied().reduce(f32::min).as_ref(),
                            all_x.iter().copied().reduce(f32::max).as_ref(),
                            all_y.iter().copied().reduce(f32::min).as_ref(),
                            all_y.iter().copied().reduce(f32::max).as_ref(),
                        ) {
                            let cx = (min_x + max_x) as f64 / 2.0;
                            let cy = (min_y + max_y) as f64 / 2.0;
                            draw_ox += paper_w / 2.0 - cx;
                            draw_oy += paper_h / 2.0 - cy;
                        }
                    } else {
                        draw_ox += ps.origin_x;
                        draw_oy += ps.origin_y;
                    }
                }

                // For rotation: swap paper dimensions and note angle for export.
                let (eff_w, eff_h) = match rotation_deg {
                    90 | 270 => (paper_h, paper_w),
                    _ => (paper_w, paper_h),
                };

        let plot_style = self.active_plot_style.clone();
        let worker_path = path.clone();
        background_task(
            move || {
                crate::io::pdf_export::export_pdf(
                    &wires,
                    hatches.as_slice(),
                    wipeouts.as_slice(),
                    eff_w,
                    eff_h,
                    draw_ox,
                    draw_oy,
                    rotation_deg,
                    1.0,
                    None,
                    &worker_path,
                    plot_style.as_ref(),
                )
                .map(|_| format!("Exported: {}", worker_path.display()))
                .map_err(|e| format!("Export failed: {e}"))
            },
            |result| Message::BackgroundIoFinished(result, false),
        )
    }

    /// Export the pending model-space plot window (set by PLOTWINDOW while on
    /// the Model tab) to PDF, using the chosen paper size/orientation/scale.
    pub(super) fn on_plot_window_export_path_some(&mut self, path: std::path::PathBuf,
    ) -> Task<Message> {
                use crate::io::paper_sizes::{sheet_mm, window_to_sheet, PlotScale};
                let i = self.active_tab;
                if self.tabs[i].scene.current_layout != "Model" {
                    self.command_line
                        .push_error("Plot window export is for model space.");
                    return Task::none();
                }
                let Some((x0, y0, x1, y1)) = self.plot_window else {
                    self.command_line.push_error("No plot window. Pick one first.");
                    return Task::none();
                };
                if (x1 - x0) < 1e-6 || (y1 - y0) < 1e-6 {
                    self.command_line
                        .push_error("Plot window is empty. Pick a larger window.");
                    return Task::none();
                }
                let (sheet_w, sheet_h) = sheet_mm(self.plot_format, self.plot_orientation);
                let win_w = (x1 - x0).max(1e-9);
                let win_h = (y1 - y0).max(1e-9);
                let scale_sel = if self.plot_scale.trim().eq_ignore_ascii_case("fit") {
                    PlotScale::Fit
                } else {
                    let (num, den) = parse_plot_scale(&self.plot_scale);
                    if num > 0.0 && den > 0.0 {
                        PlotScale::Ratio(num / den)
                    } else {
                        PlotScale::Fit
                    }
                };
                let (scale, ox, oy) = window_to_sheet((win_w, win_h), (sheet_w, sheet_h), scale_sel);

                let scene = &self.tabs[i].scene;
                // Cull to the window so a small-area plot doesn't write the whole
                // drawing into the PDF; the clip below trims the partial crossers.
                // aabb is world X/Y [minx, miny, maxx, maxy].
                let (wx0, wy0, wx1, wy1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
                let wires: Vec<_> = scene
                    .entity_wires()
                    .iter()
                    .filter(|w| {
                        w.aabb[0] <= wx1 && w.aabb[2] >= wx0 && w.aabb[1] <= wy1 && w.aabb[3] >= wy0
                    })
                    .cloned()
                    .collect();
                let hatches = scene.paper_canvas_hatches();
                let wipeouts = scene.paper_canvas_wipeouts();

                // build_pdf maps a wire coordinate to sheet mm as (coord + offset) *
                // scale (rotation_deg == 0 adds no further CTM translation).
                // window_to_sheet's (ox, oy) is the sheet-mm target for the window's
                // min corner, so: scale * (x0 + offset_x) = ox  =>  offset_x = ox/scale - x0.
                let offset_x = (ox / scale) - x0;
                let offset_y = (oy / scale) - y0;
                // Clip rect in pre-scale space (build_pdf's CTM applies `scale` to it,
                // same as the wires) so the final sheet-mm rect lands at (ox, oy).
                let clip = Some(((ox / scale) as f32, (oy / scale) as f32, win_w as f32, win_h as f32));

                let plot_style = self.active_plot_style.clone();
        let worker_path = path.clone();
        self.close_active_modal();
        background_task(
            move || {
                crate::io::pdf_export::export_pdf(
                    &wires,
                    hatches.as_slice(),
                    wipeouts.as_slice(),
                    sheet_w,
                    sheet_h,
                    offset_x,
                    offset_y,
                    0,
                    scale as f32,
                    clip,
                    &worker_path,
                    plot_style.as_ref(),
                )
                .map(|_| {format!(
                            "Plotted window to {}",
                        worker_path
                            .file_name().unwrap_or_default().to_string_lossy()
                        )
                }).map_err(|e| format!("Plot failed: {e}"))
            },
            |result| Message::BackgroundIoFinished(result, false),
        )
    }

    /// Build the render inputs and page geometry for a full-layout plot: wires
    /// plus hatch / wipeout fills, the effective (rotation-swapped) sheet size,
    /// the draw-origin offset, and the rotation. Shared by print + preview so
    /// the Plot dialog reuses the same tested derivation.
    #[allow(clippy::type_complexity)]
    pub(super) fn layout_plot_params(
        &self,
    ) -> (
        std::sync::Arc<Vec<crate::scene::WireModel>>,
        Vec<crate::scene::model::hatch_model::HatchModel>,
        Vec<crate::scene::model::hatch_model::HatchModel>,
        f64,
        f64,
        // Draw offsets stay f64: they cancel an absolute world coordinate that
        // an f32 cannot hold at UTM scale.
        f64,
        f64,
        i32,
    ) {
        let i = self.active_tab;
        let scene = &self.tabs[i].scene;
        let wires = scene.entity_wires();
        let hatches: Vec<_> = scene.paper_canvas_hatches().as_ref().clone();
        let wipeouts: Vec<_> = scene.paper_canvas_wipeouts().as_ref().clone();
        use acadrust::objects::PlotType;
        let ps_snap = scene.effective_plot_settings();
        let (paper_w, paper_h, draw_ox, draw_oy, rotation_deg) =
            if let Some(((x0, y0), (x1, y1))) = scene.paper_limits() {
                // `paper_limits()` is in paper-space units; the PDF page is
                // physical millimetres, so scale the sheet size back to mm via
                // the layout's unit factor (identity for millimetre layouts).
                let uf = scene.paper_space_unit_factor();
                let (pw, ph) = ((x1 - x0) / uf, (y1 - y0) / uf);
                let use_extents = ps_snap
                    .as_ref()
                    .map(|ps| matches!(ps.plot_type, PlotType::Extents))
                    .unwrap_or(false);
                let (ox, oy) = if use_extents {
                    if let Some((mn, _mx)) = scene.model_space_extents() {
                        (-mn.x as f64, -mn.y as f64)
                    } else {
                        (-x0, -y0)
                    }
                } else {
                    (-x0, -y0)
                };
                let rot = ps_snap
                    .as_ref()
                    .map(|ps| ps.rotation.to_degrees() as i32)
                    .unwrap_or(0);
                (pw, ph, ox, oy, rot)
            } else if let Some((mn, mx)) = scene.model_space_extents() {
                let margin = 1.05_f64;
                let w = ((mx.x - mn.x) as f64 * margin).max(1.0);
                let h = ((mx.y - mn.y) as f64 * margin).max(1.0);
                let pad_x = (w - (mx.x - mn.x) as f64) * 0.5;
                let pad_y = (h - (mx.y - mn.y) as f64) * 0.5;
                (w, h, -(mn.x as f64) + pad_x, -(mn.y as f64) + pad_y, 0)
            } else {
                (297.0, 210.0, 0.0, 0.0, 0)
            };
        let (eff_w, eff_h) = match rotation_deg {
            90 | 270 => (paper_h, paper_w),
            _ => (paper_w, paper_h),
        };
        (
            wires,
            hatches,
            wipeouts,
            eff_w,
            eff_h,
            draw_ox,
            draw_oy,
            rotation_deg,
        )
    }

    pub(super) fn on_print_to_printer(&mut self) -> Task<Message> {
        let (wires, hatches, wipeouts, eff_w, eff_h, ox, oy, rotation_deg) =
            self.layout_plot_params();
        let plot_style = self.active_plot_style.clone();
        self.command_line.push_info("Sending to system printer…");
        background_task(
            move || {
                iced::futures::executor::block_on(
                crate::io::print_to_printer::print_wires(
                    wires, hatches, wipeouts, eff_w, eff_h, ox, oy, rotation_deg, plot_style,
                ))
            },
            Message::PrintResult,
        )
    }

    /// QUICKPRINT / QP — use the current selection's bounding box as the plot
    /// window and export a PDF (drawing folder + name + timestamp) with the
    /// active page setup, no dialog. Model space only. (#325)
    pub(crate) fn on_quick_print_handles(
        &mut self,
        handles: Vec<acadrust::Handle>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].scene.current_layout != "Model" {
            self.command_line
                .push_error("Quick print works in model space.");
            return Task::none();
        }
        let set: std::collections::HashSet<acadrust::Handle> = handles.into_iter().collect();
        // Union the AABBs of the picked entities' wires (world XY), matched by
        // each wire's handle.
        let (x0, y0, x1, y1, any) = {
            let scene = &self.tabs[i].scene;
            let mut x0 = f32::INFINITY;
            let mut y0 = f32::INFINITY;
            let mut x1 = f32::NEG_INFINITY;
            let mut y1 = f32::NEG_INFINITY;
            let mut any = false;
            for w in scene.entity_wires().iter() {
                let picked = crate::scene::Scene::handle_from_wire_name(&w.name)
                    .is_some_and(|h| set.contains(&h));
                if !picked {
                    continue;
                }
                let [ax0, ay0, ax1, ay1] = w.aabb;
                if ax0.is_finite() && ay0.is_finite() && ax1.is_finite() && ay1.is_finite() {
                    x0 = x0.min(ax0);
                    y0 = y0.min(ay0);
                    x1 = x1.max(ax1);
                    y1 = y1.max(ay1);
                    any = true;
                }
            }
            (x0, y0, x1, y1, any)
        };
        if !any {
            self.command_line
                .push_error("Selection has no printable geometry.");
            return Task::none();
        }
        if !(x1 > x0 && y1 > y0) {
            self.command_line
                .push_error("Selection has no printable area.");
            return Task::none();
        }
        // Small margin so the outermost strokes aren't clipped flush to the edge.
        let mx = ((x1 - x0) * 0.02).max(0.0);
        let my = ((y1 - y0) * 0.02).max(0.0);
        self.plot_window = Some((
            (x0 - mx) as f64,
            (y0 - my) as f64,
            (x1 + mx) as f64,
            (y1 + my) as f64,
        ));
        let path = self.quick_print_path();
        self.on_plot_window_export_path_some(path)
    }

    /// Auto output path for quick print: the drawing's folder + name + a
    /// timestamp, falling back to the temp dir / "drawing" when unsaved.
    fn quick_print_path(&self) -> std::path::PathBuf {
        let i = self.active_tab;
        let cur = self.tabs[i].current_path.as_deref();
        let dir = cur
            .and_then(|p| p.parent())
            .map(|d| d.to_path_buf())
            .unwrap_or_else(std::env::temp_dir);
        let stem = cur
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "drawing".into());
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        dir.join(format!("{stem}_{ts}.pdf"))
    }

    /// Open the full Plot / Print dialog, seeding its state from the active
    /// layout's plot settings and the printers found on the system.
    pub(super) fn on_plot_dialog_open(&mut self) -> Task<Message> {
        use crate::io::paper_sizes::Orientation;
        // Keep the user's last print preferences (printer, copies, quality,
        // output options — persisted in `self.plot_dialog`); only refresh the
        // runtime printer list and reseed the drawing-specific fields from the
        // active layout.
        let d = &mut self.plot_dialog;
        d.printers = crate::io::print_to_printer::list_printers();
        d.paper = self.plot_format.label().to_string();
        d.orientation = match self.plot_orientation {
            Orientation::Portrait => "Portrait",
            Orientation::Landscape => "Landscape",
        }
        .to_string();
        d.style_name = self
            .active_plot_style
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        // `area` and `scale` are remembered user choices (default Window / Fit),
        // not reseeded from the layout. Offset / center / rotation ARE layout
        // properties, so reflect them.
        if let Some(ps) = self.tabs[self.active_tab].scene.effective_plot_settings() {
            let d = &mut self.plot_dialog;
            d.center = ps.flags.plot_centered;
            d.offset_x = format!("{:.2}", ps.origin_x);
            d.offset_y = format!("{:.2}", ps.origin_y);
            let deg = ps.rotation.to_degrees() as i32;
            d.rotation = format!("{deg}°");
        }
        self.plot_dialog.name_input = None;
        self.plot_dialog.name_rename = false;
        // Refresh the list (<none> / <previous> / layouts / named setups) and
        // snapshot the just-loaded settings as the `<previous>` restore point.
        self.refresh_page_setups();
        self.plot_prev = Some(self.plot_dialog.clone());
        // Default selection: model space → <none>; a layout → its own setup.
        let cur = self.tabs[self.active_tab].scene.current_layout.clone();
        if cur == "Model" {
            self.select_page_setup(crate::ui::window::plot::SETUP_NONE);
        } else {
            self.select_page_setup(&format!("*{cur}*"));
        }
        self.active_modal = Some(crate::app::ModalKind::Plot);
        Task::none()
    }

    /// Handle one edit / action from the Plot dialog.
    pub(super) fn on_plot_dlg(
        &mut self,
        msg: crate::ui::window::plot::PlotDlgMsg,
    ) -> Task<Message> {
        use crate::ui::window::plot::{PlotDlgMsg as M, PlotFlag, OUT_DEFAULT, OUT_PDF};
        match msg {
            M::Close => {
                self.close_active_modal();
                Task::none()
            }
            M::Printer(s) => {
                if s == OUT_PDF {
                    self.plot_dialog.to_file = true;
                } else if s == OUT_DEFAULT {
                    self.plot_dialog.to_file = false;
                    self.plot_dialog.printer = None;
                } else {
                    self.plot_dialog.to_file = false;
                    self.plot_dialog.printer = Some(s);
                }
                Task::none()
            }
            M::Paper(s) => {
                self.plot_dialog.paper = s;
                Task::none()
            }
            M::Orientation(s) => {
                self.plot_dialog.orientation = s;
                Task::none()
            }
            M::Rotation(s) => {
                self.plot_dialog.rotation = s;
                Task::none()
            }
            M::Area(s) => {
                self.plot_dialog.area = s;
                Task::none()
            }
            M::Scale(s) => {
                self.plot_dialog.scale = s;
                Task::none()
            }
            M::Quality(s) => {
                self.plot_dialog.quality = s;
                Task::none()
            }
            M::Shade(s) => {
                self.plot_dialog.shade = s;
                Task::none()
            }
            M::Copies(s) => {
                self.plot_dialog.copies = s;
                Task::none()
            }
            M::OffsetX(s) => {
                self.plot_dialog.offset_x = s;
                Task::none()
            }
            M::OffsetY(s) => {
                self.plot_dialog.offset_y = s;
                Task::none()
            }
            M::Dpi(s) => {
                self.plot_dialog.dpi = s;
                Task::none()
            }
            M::Flag(f) => {
                let d = &mut self.plot_dialog;
                match f {
                    PlotFlag::Center => d.center = !d.center,
                    PlotFlag::ScaleLw => d.scale_lw = !d.scale_lw,
                    PlotFlag::Mono => d.mono = !d.mono,
                    PlotFlag::Lineweights => d.lineweights = !d.lineweights,
                    PlotFlag::WithStyles => d.with_styles = !d.with_styles,
                    PlotFlag::Transparency => d.transparency = !d.transparency,
                    PlotFlag::PaperspaceLast => d.paperspace_last = !d.paperspace_last,
                    PlotFlag::HidePaperspace => d.hide_paperspace = !d.hide_paperspace,
                    PlotFlag::Stamp => d.stamp = !d.stamp,
                    PlotFlag::SaveLayout => d.save_layout = !d.save_layout,
                }
                Task::none()
            }
            M::LoadStyle => Task::done(Message::PlotStyleLoad),
            M::ClearStyle => {
                self.active_plot_style = None;
                self.plot_dialog.style_name.clear();
                Task::none()
            }
            M::PickWindow => {
                self.close_active_modal();
                Task::done(Message::Command("PLOTWINDOW".into()))
            }
            M::SelectSetup(name) => {
                self.select_page_setup(&name);
                Task::none()
            }
            M::SetCurrent => {
                self.apply_dialog_to_layout();
                self.command_line.push_info("Page setup applied to the layout.");
                Task::none()
            }
            M::NewSetup => {
                // Create a setup from the current editor values, then start an
                // inline rename so the user can name it.
                let name = self.next_page_setup_name("Setup");
                let ps = self.dialog_to_plotsettings();
                self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                self.tabs[self.active_tab].dirty = true;
                self.plot_dialog.selected_setup = name.clone();
                self.refresh_page_setups();
                self.plot_dialog.name_input = Some(name);
                self.plot_dialog.name_rename = true;
                Task::none()
            }
            M::CopySetup => {
                // Duplicate the selected entry — a layout OR a named setup —
                // into a new standalone named page setup.
                let sel = self.plot_dialog.selected_setup.clone();
                let scene = &self.tabs[self.active_tab].scene;
                let (src_ps, base) = if is_layout_entry(&sel) {
                    let ln = layout_entry_name(&sel);
                    (scene.plot_settings_for(ln), format!("{ln} copy"))
                } else {
                    (scene.page_setup_get(&sel), format!("{sel} copy"))
                };
                if let Some(ps) = src_ps {
                    let name = self.next_page_setup_name(&base);
                    self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                    self.tabs[self.active_tab].dirty = true;
                    self.plot_dialog.selected_setup = name.clone();
                    self.refresh_page_setups();
                    self.plot_dialog.name_input = Some(name);
                    self.plot_dialog.name_rename = true;
                }
                Task::none()
            }
            M::RenameStart(name) => {
                // Only standalone named setups can be renamed.
                if is_layout_entry(&name) || is_special_entry(&name) {
                    return Task::none();
                }
                self.plot_dialog.selected_setup = name.clone();
                if let Some(ps) = self.tabs[self.active_tab].scene.page_setup_get(&name) {
                    self.load_plotsettings_into_dialog(&ps);
                }
                self.plot_dialog.name_input = Some(name);
                self.plot_dialog.name_rename = true;
                Task::none()
            }
            M::DeleteSetup => {
                let sel = self.plot_dialog.selected_setup.clone();
                if !sel.is_empty() && !is_layout_entry(&sel) {
                    self.tabs[self.active_tab].scene.page_setup_delete(&sel);
                    self.tabs[self.active_tab].dirty = true;
                    self.plot_dialog.selected_setup.clear();
                    self.refresh_page_setups();
                }
                Task::none()
            }
            M::NameInput(s) => {
                self.plot_dialog.name_input = Some(s);
                Task::none()
            }
            M::NameCommit => {
                if let Some(name) = self.plot_dialog.name_input.take() {
                    let name = name.trim().to_string();
                    if !name.is_empty() {
                        if self.plot_dialog.name_rename {
                            let old = self.plot_dialog.selected_setup.clone();
                            self.tabs[self.active_tab].scene.page_setup_rename(&old, &name);
                        } else {
                            let ps = self.dialog_to_plotsettings();
                            self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                        }
                        self.tabs[self.active_tab].dirty = true;
                        self.plot_dialog.selected_setup = name;
                        self.refresh_page_setups();
                    }
                }
                self.plot_dialog.name_rename = false;
                Task::none()
            }
            M::NameCancel => {
                self.plot_dialog.name_input = None;
                self.plot_dialog.name_rename = false;
                Task::none()
            }
            M::Preview => self.on_plot_dlg_commit(true),
            M::Commit => self.on_plot_dlg_commit(false),
        }
    }

    fn refresh_page_setups(&mut self) {
        use crate::ui::window::plot::{SETUP_NONE, SETUP_PREV};
        let scene = &self.tabs[self.active_tab].scene;
        // <none> / <previous>, then layouts (`*name*`), then named setups.
        let mut list = vec![SETUP_NONE.to_string(), SETUP_PREV.to_string()];
        list.extend(scene.layout_names().into_iter().map(|n| format!("*{n}*")));
        list.extend(scene.page_setup_names());
        self.plot_dialog.page_setups = list;
    }

    /// Apply a page-setup list selection to the editor. Handles the pseudo
    /// entries (`<none>` / `<previous>`), layout rows (`*name*`) and named
    /// setups.
    fn select_page_setup(&mut self, name: &str) {
        use crate::ui::window::plot::{SETUP_NONE, SETUP_PREV};
        self.plot_dialog.selected_setup = name.to_string();
        if name == SETUP_NONE {
            // No page setup: default geometry + PDF output.
            let is_model = self.tabs[self.active_tab].scene.current_layout == "Model";
            let d = &mut self.plot_dialog;
            d.to_file = true;
            d.paper = "A4".into();
            d.orientation = "Landscape".into();
            d.area = if is_model { "Window".into() } else { "Layout".into() };
            d.center = true;
            d.offset_x = "0.0".into();
            d.offset_y = "0.0".into();
            d.rotation = "0°".into();
            d.scale = "Fit".into();
        } else if name == SETUP_PREV {
            if let Some(prev) = self.plot_prev.clone() {
                self.plot_dialog.copy_settings_from(&prev);
            }
        } else if is_layout_entry(name) {
            if let Some(ps) = self
                .tabs[self.active_tab]
                .scene
                .plot_settings_for(layout_entry_name(name))
            {
                self.load_plotsettings_into_dialog(&ps);
            }
        } else if let Some(ps) = self.tabs[self.active_tab].scene.page_setup_get(name) {
            self.load_plotsettings_into_dialog(&ps);
        }
    }

    /// A page-setup name based on `base` that isn't already taken (`base`,
    /// `base 2`, `base 3`, …).
    fn next_page_setup_name(&self, base: &str) -> String {
        let existing = self.tabs[self.active_tab].scene.page_setup_names();
        if !existing.iter().any(|n| n == base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|c| !existing.iter().any(|n| n == c))
            .unwrap_or_else(|| base.to_string())
    }

    /// Build a `PlotSettings` from the current dialog fields (for saving a named
    /// page setup).
    fn dialog_to_plotsettings(&self) -> acadrust::objects::PlotSettings {
        use acadrust::objects::{
            PlotPaperUnits, PlotRotation, PlotSettings, PlotType, ScaledType,
        };
        use crate::io::paper_sizes::{sheet_mm, Orientation, PaperSize};
        let d = &self.plot_dialog;
        let paper = match d.paper.as_str() {
            "A3" => PaperSize::A3,
            "A2" => PaperSize::A2,
            "A1" => PaperSize::A1,
            "A0" => PaperSize::A0,
            _ => PaperSize::A4,
        };
        let orient = if d.orientation == "Portrait" {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        };
        let (w, h) = sheet_mm(paper, orient);
        let mut ps = PlotSettings::new("");
        ps.paper_width = w;
        ps.paper_height = h;
        ps.paper_units = PlotPaperUnits::Millimeters;
        ps.plot_type = match d.area.as_str() {
            "Window" => PlotType::Window,
            "Layout" => PlotType::Layout,
            _ => PlotType::Extents,
        };
        ps.flags.plot_centered = d.center;
        ps.origin_x = d.offset_x.parse::<f64>().unwrap_or(0.0);
        ps.origin_y = d.offset_y.parse::<f64>().unwrap_or(0.0);
        let rot: i16 = d.rotation.trim_end_matches('°').parse().unwrap_or(0);
        ps.rotation = match rot {
            90 => PlotRotation::Degrees90,
            180 => PlotRotation::Degrees180,
            270 => PlotRotation::Degrees270,
            _ => PlotRotation::None,
        };
        if d.scale == "Fit" {
            ps.set_scale_to_fit();
        } else {
            let (num, den) = parse_plot_scale(&d.scale);
            ps.scale_type = ScaledType::CustomScale;
            ps.scale_numerator = num;
            ps.scale_denominator = den;
        }
        ps.printer_name = d.printer.clone().unwrap_or_default();
        ps.current_style_sheet = d.style_name.clone();
        ps
    }

    /// Load a `PlotSettings` into the dialog editor fields.
    fn load_plotsettings_into_dialog(&mut self, ps: &acadrust::objects::PlotSettings) {
        use acadrust::objects::PlotType;
        let (paper, orient) = paper_label_from_dims(ps.paper_width, ps.paper_height);
        let d = &mut self.plot_dialog;
        d.paper = paper;
        d.orientation = orient;
        d.area = match ps.plot_type {
            PlotType::Window => "Window",
            PlotType::Layout => "Layout",
            _ => "Extents",
        }
        .to_string();
        d.center = ps.flags.plot_centered;
        d.offset_x = format!("{:.2}", ps.origin_x);
        d.offset_y = format!("{:.2}", ps.origin_y);
        let deg = ps.rotation.to_degrees() as i32;
        d.rotation = format!("{deg}°");
        d.scale = if ps.is_scale_to_fit() {
            "Fit".into()
        } else {
            let n = ps.scale_numerator;
            let m = ps.scale_denominator;
            if (n - 1.0).abs() < 1e-9 {
                format!("1:{}", m as i64)
            } else if (m - 1.0).abs() < 1e-9 {
                format!("{}:1", n as i64)
            } else {
                "Fit".into()
            }
        };
    }

    /// Write the current dialog fields into the active layout's plot settings.
    /// Shared by the "Set current" action and the print/export commit.
    fn apply_dialog_to_layout(&mut self) {
        use crate::io::paper_sizes::{sheet_mm, Orientation, PaperSize};
        let d = self.plot_dialog.clone();
        let paper = match d.paper.as_str() {
            "A3" => PaperSize::A3,
            "A2" => PaperSize::A2,
            "A1" => PaperSize::A1,
            "A0" => PaperSize::A0,
            _ => PaperSize::A4,
        };
        let orient = if d.orientation == "Portrait" {
            Orientation::Portrait
        } else {
            Orientation::Landscape
        };
        let (sheet_w, sheet_h) = sheet_mm(paper, orient);
        self.plot_format = paper;
        self.plot_orientation = orient;
        self.plot_scale = d.scale.clone();
        let rotation: i16 = d.rotation.trim_end_matches('°').parse().unwrap_or(0);
        let off_x = d.offset_x.parse::<f64>().unwrap_or(0.0);
        let off_y = d.offset_y.parse::<f64>().unwrap_or(0.0);
        let extents = d.area != "Layout";
        self.apply_plot_page_settings(
            sheet_w, sheet_h, extents, d.center, off_x, off_y, rotation, &d.scale,
        );
    }

    /// Apply the dialog's page settings to the layout, then either open a
    /// preview PDF, export a PDF, or send the job to the chosen printer.
    fn on_plot_dlg_commit(&mut self, preview: bool) -> Task<Message> {
        // Remember the user's print preferences across sessions.
        self.save_config();
        let d = self.plot_dialog.clone();
        // Persist the dialog's page settings into the layout, then reuse the
        // tested layout-plot derivation.
        self.apply_dialog_to_layout();
        self.active_modal = None;

        let plot_style = if d.with_styles {
            self.active_plot_style.clone()
        } else {
            None
        };

        // ── Window area: clipped model-space plot ────────────────────────────
        if d.area == "Window" {
            // Preview renders to a temp PDF and opens it — never a save dialog,
            // whatever the output target is.
            if preview {
                let Some((w_wires, w_hatches, w_wipeouts, sw, sh, wox, woy, wscale, wclip)) =
                    self.window_plot_job()
                else {
                    self.command_line
                        .push_error("Pick a plot window first (model space).");
                    self.active_modal = Some(crate::app::ModalKind::Plot);
                    return Task::none();
                };
                let tmp = std::env::temp_dir().join("open_cad_studio_preview.pdf");
                return background_task(
                    move || {
                        crate::io::pdf_export::export_pdf(
                    &w_wires, &w_hatches, &w_wipeouts, sw, sh, wox, woy, 0, wscale, wclip, &tmp,
                    plot_style.as_ref(),
                ).and_then(|_| crate::io::print_to_printer::open_in_viewer(&tmp))
                        .map(|_| "Opened plot preview.".to_string()).map_err(|e| format!("Preview failed: {e}"))
                    },
                    |result| Message::BackgroundIoFinished(result, true),
                );
            }
            if d.to_file {
                // Tested clipped export (opens a save dialog).
                return Task::done(Message::PlotWindowExport);
            }
            let Some((w_wires, w_hatches, w_wipeouts, sw, sh, wox, woy, wscale, wclip)) =
                self.window_plot_job()
            else {
                self.command_line
                    .push_error("Pick a plot window first (model space).");
                self.active_modal = Some(crate::app::ModalKind::Plot);
                return Task::none();
            };
            let tmp = std::env::temp_dir().join("open_cad_studio_print.pdf");
            let opts = self.plot_print_options(&d);
            return background_task(
                move || {
                    crate::io::pdf_export::export_pdf(
                &w_wires, &w_hatches, &w_wipeouts, sw, sh, wox, woy, 0, wscale, wclip, &tmp,
                plot_style.as_ref(),
            ).and_then(|_| crate::io::print_to_printer::print_existing_pdf(&tmp, &opts))
                    .map(|printer| format!("Sent to printer: {printer}"))
                    .map_err(|e| format!("Print failed: {e}"))
                },
                |result| Message::BackgroundIoFinished(result, false),
            );
        }

        let (wires, hatches, wipeouts, eff_w, eff_h, ox, oy, rot) = self.layout_plot_params();

        if preview {
            let tmp = std::env::temp_dir().join("open_cad_studio_preview.pdf");
            return background_task(
                move || {
                    crate::io::pdf_export::export_pdf(
                &wires,
                &hatches,
                &wipeouts,
                eff_w,
                eff_h,
                ox,
                oy,
                rot,
                1.0,
                None,
                &tmp,
                plot_style.as_ref(),
            ).and_then(|_| crate::io::print_to_printer::open_in_viewer(&tmp))
                    .map(|_| "Opened plot preview.".to_string()).map_err(|e| format!("Preview failed: {e}"))
                },
                |result| Message::BackgroundIoFinished(result, true),
            );
        }

        if d.to_file {
            // Reuse the tested PDF export flow (it opens a save dialog).
            return Task::done(Message::PlotExport);
        }

        let opts = self.plot_print_options(&d);
        self.command_line.push_info("Sending to system printer…");
        background_task(
            move || {
                iced::futures::executor::block_on(
                crate::io::print_to_printer::print_wires_with(
                    wires, hatches, wipeouts, eff_w, eff_h, ox, oy, rot, plot_style, opts,
                ))
            },
            Message::PrintResult,
        )
    }

    /// Build a [`PrintOptions`](crate::io::print_to_printer::PrintOptions) from
    /// the dialog state.
    fn plot_print_options(
        &self,
        d: &crate::ui::window::plot::PlotDialogState,
    ) -> crate::io::print_to_printer::PrintOptions {
        crate::io::print_to_printer::PrintOptions {
            printer: d.printer.clone(),
            copies: d.copies.trim().parse::<u32>().unwrap_or(1).max(1),
            mono: d.mono,
            quality: Some(d.quality.clone()),
            dpi: d.dpi.trim().parse::<u32>().ok().filter(|v| *v > 0),
        }
    }

    /// Render inputs + page geometry for a clipped model-space window plot
    /// (the pending `plot_window`, fitted onto the dialog's sheet). Returns
    /// `None` when not in model space or no window has been picked. Mirrors
    /// [`Self::on_plot_window_export_path_some`] but hands back the params so
    /// the plot can go to a printer or preview, not only a save-dialog PDF.
    #[allow(clippy::type_complexity)]
    fn window_plot_job(
        &self,
    ) -> Option<(
        Vec<crate::scene::WireModel>,
        Vec<crate::scene::model::hatch_model::HatchModel>,
        Vec<crate::scene::model::hatch_model::HatchModel>,
        f64,
        f64,
        // Offsets stay f64 (they cancel a UTM-scale world coordinate); the plot
        // scale is a small ratio and stays f32.
        f64,
        f64,
        f32,
        Option<(f32, f32, f32, f32)>,
    )> {
        use crate::io::paper_sizes::{sheet_mm, window_to_sheet, PlotScale};
        let i = self.active_tab;
        if self.tabs[i].scene.current_layout != "Model" {
            return None;
        }
        let (x0, y0, x1, y1) = self.plot_window?;
        if (x1 - x0) < 1e-6 || (y1 - y0) < 1e-6 {
            return None;
        }
        let (sheet_w, sheet_h) = sheet_mm(self.plot_format, self.plot_orientation);
        let win_w = (x1 - x0).max(1e-9);
        let win_h = (y1 - y0).max(1e-9);
        let scale_sel = if self.plot_scale.trim().eq_ignore_ascii_case("fit") {
            PlotScale::Fit
        } else {
            let (num, den) = parse_plot_scale(&self.plot_scale);
            if num > 0.0 && den > 0.0 {
                PlotScale::Ratio(num / den)
            } else {
                PlotScale::Fit
            }
        };
        let (scale, ox, oy) = window_to_sheet((win_w, win_h), (sheet_w, sheet_h), scale_sel);
        let scene = &self.tabs[i].scene;
        let (wx0, wy0, wx1, wy1) = (x0 as f32, y0 as f32, x1 as f32, y1 as f32);
        let wires: Vec<_> = scene
            .entity_wires()
            .iter()
            .filter(|w| w.aabb[0] <= wx1 && w.aabb[2] >= wx0 && w.aabb[1] <= wy1 && w.aabb[3] >= wy0)
            .cloned()
            .collect();
        let hatches: Vec<_> = scene.paper_canvas_hatches().as_ref().clone();
        let wipeouts: Vec<_> = scene.paper_canvas_wipeouts().as_ref().clone();
        let offset_x = (ox / scale) - x0;
        let offset_y = (oy / scale) - y0;
        let clip = Some((
            (ox / scale) as f32,
            (oy / scale) as f32,
            win_w as f32,
            win_h as f32,
        ));
        Some((
            wires, hatches, wipeouts, sheet_w, sheet_h, offset_x, offset_y, scale as f32, clip,
        ))
    }

    pub(super) fn on_plot_style_panel_apply(&mut self) -> Task<Message> {
                let aci = self.plotstyle_panel_aci as usize;
                if let Some(table) = self.active_plot_style.as_mut() {
                    if let Some(entry) = table.aci_entries.get_mut(aci) {
                        // Parse color.
                        let color_str = self.ps_color_buf.trim();
                        if color_str.is_empty() {
                            entry.color = None;
                        } else if color_str.starts_with('#') && color_str.len() == 7 {
                            let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(0);
                            let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(0);
                            let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(0);
                            entry.color = Some([r, g, b]);
                        }
                        if let Ok(lw) = self.ps_lineweight_buf.trim().parse::<u8>() {
                            entry.lineweight = lw;
                        }
                        if let Ok(sc) = self.ps_screening_buf.trim().parse::<u8>() {
                            entry.screening = sc.min(100);
                        }
                        self.command_line
                            .push_output(&format!("Plot style ACI {aci} updated."));
                    }
                } else {
                    // No table loaded: create an identity table and apply.
                    let mut table = crate::io::plot_style::PlotStyleTable::identity("Custom.ctb");
                    if let Some(entry) = table.aci_entries.get_mut(aci) {
                        let color_str = self.ps_color_buf.trim();
                        if color_str.starts_with('#') && color_str.len() == 7 {
                            let r = u8::from_str_radix(&color_str[1..3], 16).unwrap_or(0);
                            let g = u8::from_str_radix(&color_str[3..5], 16).unwrap_or(0);
                            let b = u8::from_str_radix(&color_str[5..7], 16).unwrap_or(0);
                            entry.color = Some([r, g, b]);
                        }
                        if let Ok(lw) = self.ps_lineweight_buf.trim().parse::<u8>() {
                            entry.lineweight = lw;
                        }
                        if let Ok(sc) = self.ps_screening_buf.trim().parse::<u8>() {
                            entry.screening = sc.min(100);
                        }
                    }
                    self.active_plot_style = Some(table);
                    self.command_line
                        .push_output(&format!("Created new CTB table, ACI {aci} updated."));
                }
                Task::none()
    }

    pub(super) fn on_plot_style_panel_save(&mut self) -> Task<Message> {
                if self.active_plot_style.is_none() {
                    self.command_line
                        .push_error("No plot style table loaded. Load or create one first.");
                    return Task::none();
                }
                let default_name = self
                    .active_plot_style
                    .as_ref()
                    .map(|t| t.name.clone())
                    .unwrap_or("export.ctb".into());
                Task::perform(
                    async move {
                        crate::sys::file_dialog()
                            .set_title("Save Plot Style Table")
                            .set_file_name(&default_name)
                            .add_filter("Plot Style Files", &["ctb", "stb", "CTB", "STB"])
                            .add_filter("All Files", &["*"])
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h))
                    },
                    Message::PlotStylePanelSavePath,
                )
    }
}
