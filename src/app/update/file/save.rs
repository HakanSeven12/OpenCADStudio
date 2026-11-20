use super::*;
use super::super::util::*;

impl OpenCADStudio {
    /// Write `source` to `path` (WBLOCK and -WBLOCK): an existing block, the
    /// whole drawing, or objects, which then are kept, converted to a block
    /// named after the file, or deleted.
    pub(in crate::app) fn wblock_write(
        &mut self,
        i: usize,
        source: crate::modules::insert::block_cmdline::WblockSource,
        path: std::path::PathBuf,
        unit: i16,
    ) -> Task<Message> {
        let mut path = path;
        if path.extension().is_none() {
            path.set_extension("dwg");
        }
        let ifccad_session = if matches!(
            &source,
            crate::modules::insert::block_cmdline::WblockSource::Drawing
        ) {
            self.tabs[i].ifccad_session.clone()
        } else {
            None
        };
        if let Some(session) = &ifccad_session {
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or_default();
            if let Err(error) = session.check_destination(&self.tabs[i].scene.document, ext) {
                return Task::done(Message::WblockWriteFinished("*".into(), path, Err(error)));
            }
        }
        let document = self.tabs[i].document_for_save();
        if let crate::modules::insert::block_cmdline::WblockSource::Objects {
            handles,
            base,
            mode,
        } = &source
        {
            match mode {
                crate::ui::window::block_definition::BlockObjectMode::Retain => {}
                crate::ui::window::block_definition::BlockObjectMode::Delete => {
                    self.push_undo_snapshot(i, "WBLOCK");
                    self.tabs[i].scene.erase_entities(handles);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
                crate::ui::window::block_definition::BlockObjectMode::Convert => {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("WBLOCK")
                        .to_string();
                    self.push_undo_snapshot(i, "WBLOCK");
                    let ucs = self.tabs[i].ucs_xform();
                    let options = crate::scene::CreateBlockOptions {
                        name,
                        handles: handles.clone(),
                        base_point: *base,
                        world_to_block: ucs.to_ucs_transform_at(*base),
                        block_to_world: ucs.to_wcs_transform_at(*base),
                        mode: crate::ui::window::block_definition::BlockObjectMode::Convert,
                        annotative: false,
                        match_orientation: false,
                        scale_uniformly: false,
                        allow_exploding: true,
                        unit,
                        description: String::new(),
                        hyperlink_url: String::new(),
                        hyperlink_desc: String::new(),
                        redefine: true,
                    };
                    let _ = self.tabs[i].scene.create_block_with_options(options);
                    self.tabs[i].dirty = true;
                    self.refresh_properties();
                }
            }
        }
        let display = match &source {
            crate::modules::insert::block_cmdline::WblockSource::Block(name) => name.clone(),
            _ => "*".to_string(),
        };
        let extract = move || -> Result<document::Document, String> {
            use crate::modules::insert::{block_cmdline::WblockSource, wblock};
            let mut extracted = match source {
                WblockSource::Block(name) => wblock::extract_block_to_doc(&document, &name),
                WblockSource::Drawing => Ok(document),
                WblockSource::Objects { handles, base, .. } => {
                    wblock::extract_entities_to_doc_with_base(&document, &handles, base, unit)
                }
            }
            .map_err(|error| error.to_string())?;
            extracted.header.insertion_units = unit;
            Ok(extracted)
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let worker_path = path.clone();
            background_task(
                crate::t!("WBLOCK save"),
                move || {
                    crate::io::save_wblock_snapshot(
                        extract()?,
                        &worker_path,
                        ifccad_session.as_ref(),
                    )
                },
                move |result| Message::WblockWriteFinished(display, path, result),
            )
        }
        #[cfg(target_arch = "wasm32")]
        {
            let ext = path
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            Task::perform(
                async move {
                    let document = extract()?;
                    let version = document.version;
                    let prepared = crate::io::prepare_document_for_save(document, version);
                    crate::io::serialize_prepared_document(
                        &prepared,
                        &ext,
                        false,
                        ifccad_session.as_ref(),
                    )
                    .await
                },
                move |result| Message::WebWblockSerialized {
                    block_name: display,
                    path,
                    filename,
                    result,
                },
            )
        }
    }

    pub(in crate::app) fn on_wblock_save_result_some(
        &mut self,
        block_name: String,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        let i = self.active_tab;
        let document = self.tabs[i].document_for_save();
        let handles: Vec<_> = self.tabs[i].scene.selected.iter().copied().collect();
        // Native: extraction and the file write both run on a background
        // thread, unchanged.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let worker_name = block_name.clone();
            let worker_path = path.clone();
            background_task(
                crate::t!("WBLOCK save"),
                move || {
                    let document = if worker_name == "*" {
                        crate::modules::insert::wblock::extract_entities_to_doc(&document, &handles)
                    } else {
                        crate::modules::insert::wblock::extract_block_to_doc(
                            &document,
                            &worker_name,
                        )
                    }
                    .map_err(|e| e.to_string())?;
                    crate::io::save(&document, &worker_path).map_err(|e| e.to_string())
                },
                move |result| Message::WblockWriteFinished(block_name, path, result),
            )
        }
        // Web: extraction is pure document manipulation (it reads the clone
        // above and builds a fresh document) — run it inline on the main
        // thread as the prepare step, together with the shared save
        // preparation, then hand the DWG/DXF writer pass to the serializer
        // worker the way the web Save flow does
        // (`serialize_prepared_document` falls back inline when the worker
        // cannot help). Completion (`WebWblockSerialized`) delivers the
        // browser download plus the same `WblockWriteFinished` command-line
        // bookkeeping.
        #[cfg(target_arch = "wasm32")]
        {
            let extracted = if block_name == "*" {
                crate::modules::insert::wblock::extract_entities_to_doc(&document, &handles)
            } else {
                crate::modules::insert::wblock::extract_block_to_doc(&document, &block_name)
            }
            .map_err(|e| e.to_string());
            let document = match extracted {
                Ok(document) => document,
                // Same reporting the background path used for a failed
                // extraction — straight through the existing message.
                Err(error) => {
                    return Task::done(Message::WblockWriteFinished(block_name, path, Err(error)))
                }
            };
            let ext = path
                .extension()
                .map(|ext| ext.to_string_lossy().into_owned())
                .unwrap_or_else(|| "dwg".to_string());
            let filename = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "block.dwg".into());
            let version = document.version;
            let prepared = crate::io::prepare_document_for_save(document, version);
            Task::perform(
                async move {
                    crate::io::serialize_prepared_document(&prepared, &ext, false, None).await
                },
                move |result| Message::WebWblockSerialized {
                    block_name,
                    path,
                    filename,
                    result,
                },
            )
        }
    }

    /// Web SAVEALL: every dirty tab that already has a name-only path is
    /// serialized and downloaded in one pass — the worker-routed counterpart
    /// of a single web Save. State sync and snapshot capture happen up front;
    /// style/dimension preparation happens one document at a time immediately
    /// before its serial DWG/DXF writer pass on the serializer worker — the
    /// same single-future structure the autosave round uses. The browser may
    /// ask once for permission to download several files, which is the
    /// platform's price. Clean tabs are skipped (re-downloading identical
    /// bytes is noise); tabs with no path still need SAVEAS. Completion
    /// (`on_web_saveall_serialized`) fires the downloads and clears each
    /// tab's dirty flag — but only if the tab did not change while its
    /// writer ran.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn saveall_web(&mut self) -> Task<Message> {
        // Collect per-document alpha consent before capturing or writing any batch output.
        if let Some(tab_id) = self.tabs.iter().enumerate().find_map(|(i, tab)| {
            let ext = tab.current_path.as_ref()?.extension()?.to_str()?;
            (tab.dirty
                && !tab.is_start
                && tab.block_test.is_none()
                && self.needs_ifccad_alpha_consent(i, ext))
            .then_some(tab.id)
        }) {
            self.pending_ifccad_alpha_save =
                Some(crate::app::PendingConversionSave::WebSaveAll { tab_id });
            self.active_modal = Some(crate::app::ModalKind::IfccadAlphaWarning);
            return Task::none();
        }
        let mut skipped = 0usize;
        let mut pending = Vec::new();
        for t in 0..self.tabs.len() {
            if self.tabs[t].is_start || self.tabs[t].block_test.is_some() {
                continue;
            }
            let Some(path) = self.tabs[t].current_path.clone() else {
                skipped += 1;
                continue;
            };
            if !self.tabs[t].dirty {
                continue;
            }
            // The same preparation a single web Save makes on the tab.
            self.sync_view_state_for_save(t);
            sync_annotation_scale_header(&mut self.tabs[t].scene);
            self.stamp_header_sysvars(t);
            self.sync_solid_models_for_save(t);
            self.tabs[t].scene.sync_native_parametric_graph();
            let ext = path
                .extension()
                .map(|ext| ext.to_string_lossy().into_owned())
                .unwrap_or_else(|| "dwg".to_string());
            let version = self.tabs[t].scene.document.version;
            let filename = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            // Race guard (the same snapshot a single web Save takes): the
            // markers are read after the state syncs, so completion only
            // clears the dirty flag if the tab did not change while the
            // writer ran off-thread.
            let tab = &self.tabs[t];
            let entry = crate::app::WebSaveallEntry {
                tab_id: tab.id,
                path,
                filename,
                ext,
                version,
                geometry_epoch: tab.scene.geometry_epoch,
                edit_revision: tab.edit_revision,
                camera_generation: tab.scene.camera_generation,
                // Placeholder — the serialization future below overwrites
                // it before the completion message is built.
                result: Ok(Vec::new()),
            };
            pending.push((
                (entry, tab.ifccad_session.clone()),
                tab.document_for_save(),
                version,
            ));
        }
        if pending.is_empty() {
            self.report_saveall_web(0, skipped);
            return Task::none();
        }
        Task::perform(
            async move {
                let mut results = Vec::with_capacity(pending.len());
                for ((mut entry, session), prepared) in crate::io::prepare_save_batch(pending) {
                    entry.result = crate::io::serialize_prepared_document(
                        &prepared,
                        &entry.ext,
                        false,
                        session.as_ref(),
                    )
                    .await;
                    results.push(entry);
                }
                results
            },
            move |results| Message::WebSaveallSerialized { skipped, results },
        )
    }

    /// Web SAVEALL completion: the serializer worker (or inline fallback)
    /// produced each tab's bytes. The download is the only artifact of a
    /// tab's save — fire it even if the tab was closed while serialization
    /// was in flight. Bookkeeping matches the old synchronous pass (version
    /// stamped, recovery flag cleared, dirty cleared under the race guard,
    /// one error line per failed tab), then the shared summary line.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_saveall_serialized(
        &mut self,
        skipped: usize,
        results: Vec<crate::app::WebSaveallEntry>,
    ) -> Task<Message> {
        let mut saved = 0usize;
        for entry in results {
            match entry.result {
                Ok(bytes) => {
                    crate::sys::download_bytes(&entry.filename, &bytes);
                    saved += 1;
                    let Some(i) = self.tabs.iter().position(|tab| tab.id == entry.tab_id) else {
                        continue;
                    };
                    self.tabs[i].scene.document.version = entry.version;
                    self.tabs[i].recovery_save_as_required = false;
                    let snapshot_is_current = self.tabs[i].scene.geometry_epoch
                        == entry.geometry_epoch
                        && self.tabs[i].edit_revision == entry.edit_revision
                        && self.tabs[i].scene.camera_generation == entry.camera_generation;
                    if snapshot_is_current {
                        self.tabs[i].dirty = false;
                        self.tabs[i].scene.note_tables_saved();
                    }
                }
                Err(e) => self.command_line.push_error(
                    crate::tf!("SAVEALL: {} failed: {e}", entry.path.display()).as_ref(),
                ),
            }
        }
        self.report_saveall_web(saved, skipped);
        Task::none()
    }

    /// The SAVEALL summary line — shared by the nothing-queued early return
    /// and the worker completion.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn report_saveall_web(&mut self, saved: usize, skipped: usize) {
        self.command_line.push_output(
            crate::tf!(
                "SAVEALL: saved {saved} drawing(s){}.",
                if skipped > 0 {
                    crate::tf!("; {skipped} need SAVEAS (no file path yet)").into_owned()
                } else {
                    String::new()
                }
            )
            .as_ref(),
        );
    }

    pub(in crate::app) fn on_stl_export_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
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
            crate::t!("STL export"),
            move || {
                let mesh_refs: Vec<_> = meshes.iter().collect();
                let bytes = crate::io::stl::build_stl(&mesh_refs)
                    .ok_or_else(|| "no mesh data to export".to_string())?;
                write_export(&worker_path, &bytes)
            },
            move |result| Message::StlExportFinished(path, result),
        )
    }

    pub(in crate::app) fn on_step_export_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
        use document::EntityType;
        let i = self.active_tab;
        let scene = &self.tabs[i].scene;
        // Every object with a 3D mesh, in handle order: its ACIS data (lifted
        // on the worker) for an exact B-rep, and the full-resolution mesh
        // (LOD 0; see StlExportPath above) to fall back on.
        let mut handles: Vec<_> = scene.meshes.keys().copied().collect();
        handles.sort_unstable_by_key(|h| h.value());
        let objects: Vec<_> = handles
            .into_iter()
            .filter_map(|handle| {
                let mesh = scene.meshes.get(&handle)?.lods.first()?.clone();
                let entity = scene.document.get_entity(handle);
                let (kind, acis, solid) = match entity {
                    Some(EntityType::Solid3D(e)) => ("Solid", Some(e.acis_data.clone()), true),
                    Some(EntityType::Body(e)) => ("Body", Some(e.acis_data.clone()), true),
                    Some(EntityType::Region(e)) => ("Region", Some(e.acis_data.clone()), false),
                    Some(EntityType::Surface(e)) => ("Surface", Some(e.acis_data.clone()), false),
                    _ => ("Mesh", None, true),
                };
                let layer = entity.map(|e| e.common().layer.clone());
                let [r, g, b, _] = mesh.color;
                let name = format!("{kind} {:X}", handle.value());
                Some((acis, solid, mesh, name, layer, [r, g, b].map(f64::from)))
            })
            .collect();
        let unit_mm = crate::app::properties::insunits_to_mm(scene.document.header.insertion_units)
            .unwrap_or(1.0);
        let schema = self.step_schema;
        let product = self.tabs[i]
            .current_path
            .as_deref()
            .and_then(|p| p.file_stem())
            .map_or_else(
                || "OpenCADStudio".to_string(),
                |s| s.to_string_lossy().into_owned(),
            );
        let worker_path = path.clone();
        background_task(
            crate::t!("STEP export"),
            move || {
                let items: Vec<_> = objects
                    .into_iter()
                    .map(
                        |(acis, solid, mesh, name, layer, color)| crate::io::step::StepItem {
                            body: acis
                                .as_ref()
                                .and_then(crate::scene::convert::solid3d_tess::acis_record),
                            solid,
                            mesh: Some(mesh),
                            name,
                            layer,
                            color: Some(color.map(|c| c.clamp(0.0, 1.0))),
                        },
                    )
                    .collect();
                let text = crate::io::step::build_step(&items, unit_mm, schema, &product)?;
                write_export(&worker_path, text.as_bytes())
            },
            move |result| Message::StepExportFinished(path, result),
        )
    }

    pub(in crate::app) fn on_fbx_export_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let i = self.active_tab;
        // Export uses LOD 0 (full resolution); see StlExportPath above.
        let meshes: Vec<crate::scene::model::mesh_model::MeshModel> = self.tabs[i]
            .scene
            .meshes
            .values()
            .filter_map(|s| s.lods.first().cloned())
            .collect();
        let header = &self.tabs[i].scene.document.header;
        // Centimetres per drawing unit (INSUNITS); unitless metric drawings
        // are taken as millimetres, imperial ones as inches.
        let unit_cm = match header.insertion_units {
            1 => 2.54,
            2 => 30.48,
            4 => 0.1,
            5 => 1.0,
            6 => 100.0,
            14 => 10.0,
            _ if header.measurement == 1 => 0.1,
            _ => 2.54,
        };
        let worker_path = path.clone();
        background_task(
            crate::t!("FBX export"),
            move || {
                let mesh_refs: Vec<_> = meshes.iter().collect();
                let bytes = crate::io::fbx::build_fbx(&mesh_refs, unit_cm)
                    .ok_or_else(|| "no mesh data to export".to_string())?;
                write_export(&worker_path, &bytes)
            },
            move |result| Message::FbxExportFinished(path, result),
        )
    }

    pub(in crate::app) fn on_obj_import_path_some(
        &mut self,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        let tab_id = self.tabs[self.active_tab].id;
        let worker_path = path.clone();
        background_task(
            crate::t!("OBJ import"),
            move || {
                let src = std::fs::read_to_string(&worker_path).map_err(|e| e.to_string())?;
                crate::io::obj::parse_obj(&src)
                    .ok_or_else(|| "no usable geometry in file".to_string())
            },
            move |result| Message::ObjImportFinished(tab_id, path, result),
        )
    }

    pub(in crate::app) fn sync_view_state_for_save(&mut self, i: usize) {
        self.sync_vport_display(i);
        if self.tabs[i].active_block_edit.is_none() {
            self.tabs[i].scene.sync_camera_to_document();
            self.tabs[i].last_synced_camera_gen = self.tabs[i].scene.camera_generation;
            self.tabs[i].scene.sync_active_viewport_to_document();
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn prepare_native_save(&mut self, i: usize) {
        // A save is a field evaluation event (FIELDEVAL bit 2).
        self.tabs[i].scene.update_fields(2, None);
        self.prepare_snapshot_save(i);
    }

    /// Point the drawing at the file it now lives in. The fields that report
    /// the drawing's own file (FILENAME, DWGNAME, DWGPREFIX) read
    /// `document.source_path`, which is only written when a file is opened —
    /// so without this a Save As left them naming the file the drawing came
    /// from (and a new drawing stuck on `Drawing1.dwg`). The save event is
    /// re-raised so the displayed text follows the new name.
    pub(in crate::app) fn adopt_saved_path(&mut self, i: usize, path: &std::path::Path) {
        self.tabs[i].scene.document.source_path = Some(path.to_string_lossy().into_owned());
        self.tabs[i].scene.update_fields(2, None);
    }

    /// Bring the drawing state the file stores up to date, without the save
    /// event an autosave does not raise.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn prepare_snapshot_save(&mut self, i: usize) {
        self.sync_view_state_for_save(i);
        sync_annotation_scale_header(&mut self.tabs[i].scene);
        self.stamp_header_sysvars(i);
        self.sync_solid_models_for_save(i);
        self.tabs[i].scene.sync_native_parametric_graph();
        self.stamp_sheet_set_data(i);
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn native_save_verification(
        path: &std::path::Path,
        mut lease: Option<&mut crate::io::edit_lock::EditLease>,
        expected: Option<crate::io::edit_lock::FileFingerprint>,
    ) -> Result<
        (
            Option<crate::io::edit_lock::FileFingerprint>,
            Option<std::fs::File>,
        ),
        crate::io::SaveFailure,
    > {
        let expected = match expected {
            Some(expected) => Some(expected),
            None => {
                let captured = match lease.as_deref_mut() {
                    Some(lease) => match lease.fingerprint() {
                        Ok(Some(fingerprint)) => Ok(fingerprint),
                        Ok(None) => crate::io::edit_lock::FileFingerprint::capture(path),
                        Err(error) => Err(error),
                    },
                    None => crate::io::edit_lock::FileFingerprint::capture(path),
                };
                match captured {
                    Ok(fingerprint) => Some(fingerprint),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => {
                        return Err(crate::io::SaveFailure::other(format!(
                            "could not verify {} before saving: {error}",
                            path.display()
                        )));
                    }
                }
            }
        };
        let reader = if expected.is_some() {
            match lease.as_deref() {
                Some(lease) => lease.reader().map_err(|error| {
                    crate::io::SaveFailure::other(format!(
                        "could not verify {} before saving: {error}",
                        path.display()
                    ))
                })?,
                None => None,
            }
        } else {
            None
        };
        Ok((expected, reader))
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_thumbnail_capture_frame(&mut self) -> Task<Message> {
        let Some(pending) = self.pending_web_thumbnail_save.take() else {
            return Task::none();
        };
        Task::done(Message::WebSaveScreenshot {
            tab_id: pending.tab_id,
            filename: pending.filename,
            ext: pending.ext,
            version: pending.version,
            bounds: Some(pending.bounds),
            screenshot: crate::sys::capture_canvas(),
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_thumbnail_capture_frame(&mut self) -> Task<Message> {
        let Some(pending) = self.pending_native_thumbnail_save.take() else {
            return Task::none();
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == pending.tab_id) else {
            self.thumbnail_capture_clean = false;
            return Task::none();
        };
        self.conversion_loss_allowed = pending.allow_conversion_loss;
        self.queue_native_save(
            i,
            pending.path,
            pending.version,
            pending.purpose,
            pending.continuation,
            pending.set_current_path,
            pending.check_external_change,
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_save_screenshot(
        &mut self,
        tab_id: u64,
        filename: String,
        ext: String,
        version: document::DxfVersion,
        bounds: Option<iced::Rectangle>,
        screenshot: Option<iced::window::Screenshot>,
    ) -> Task<Message> {
        self.thumbnail_capture_clean = false;
        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            // The tab closed while its capture was queued: the save is over
            // (no serialization follows), so release the in-flight slot.
            crate::io::fs_access::end_web_save(tab_id);
            self.conversion_loss_allowed = false;
            return Task::none();
        };
        let preview = screenshot.as_ref().and_then(|screenshot| {
            bounds.and_then(|bounds| {
                crate::io::thumbnail::from_screenshot(
                    screenshot,
                    bounds,
                    version >= document::DxfVersion::AC1027,
                )
            })
        });
        if let Some(preview) = preview {
            self.tabs[i].scene.document.preview = Some(preview);
        }

        // The name the autosave snapshot for this tab is keyed by — capture it
        // before the save re-points `current_path` at the download name.
        let autosave_key = Self::web_autosave_key(&self.tabs[i]);
        // Race guard (the web counterpart of native's `snapshot_is_current`):
        // completion only clears the dirty flag if the tab did not change
        // while the writer ran off-thread.
        let geometry_epoch = self.tabs[i].scene.geometry_epoch;
        let edit_revision = self.tabs[i].edit_revision;
        let camera_generation = self.tabs[i].scene.camera_generation;
        // Prepare the document here (clone + style sync + dimension bake — the
        // app-side half of the pipeline), then hand the DWG/DXF writer pass to
        // the browser worker so the UI thread stays responsive on large
        // drawings. Completion (download + bookkeeping) continues in
        // `on_web_save_serialized`.
        let prepared =
            crate::io::prepare_document_for_save(self.tabs[i].document_for_save(), version);
        let allow_loss = std::mem::take(&mut self.conversion_loss_allowed);
        let ifccad_session = self.tabs[i].ifccad_session.clone();
        Task::perform(
            async move {
                crate::io::serialize_prepared_document(
                    &prepared,
                    &ext,
                    allow_loss,
                    ifccad_session.as_ref(),
                )
                .await
            },
            move |result| Message::WebSaveSerialized {
                tab_id,
                filename,
                autosave_key,
                version,
                geometry_epoch,
                edit_revision,
                camera_generation,
                result,
            },
        )
    }

    /// Web SAVE completion: the serializer worker (or inline fallback) has the
    /// drawing bytes. When the tab still exists AND holds a File System
    /// Access handle picked for the format these bytes were serialized as,
    /// they go to it in place (createWritable needs no user gesture, so the
    /// write may happen long after the pick); completion continues in
    /// `on_web_save_written`. Anything else takes the legacy download — the
    /// only artifact of the save, launched even if the tab was closed while
    /// serialization was in flight.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_save_serialized(
        &mut self,
        tab_id: u64,
        filename: String,
        autosave_key: String,
        version: document::DxfVersion,
        geometry_epoch: u64,
        edit_revision: u64,
        camera_generation: u64,
        result: Result<Vec<u8>, String>,
    ) -> Task<Message> {
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => {
                if crate::io::host_bridge::is_embed_mode() {
                    crate::io::host_bridge::emit_error("save_failed", &error);
                }
                self.command_line
                    .push_error(crate::tf!("Save failed: {error}").as_ref());
                crate::io::fs_access::end_web_save(tab_id);
                return self.finish_web_save(false);
            }
        };
        // In-place write eligibility, all three required:
        // * the tab still exists — a tab that closed while serialization ran
        //   means the user declined this save (discard/close), and the
        //   legacy contract for closed tabs is download-only;
        // * a handle is held AND it was picked for the format these bytes
        //   were serialized for (the pending ext recorded at capture time):
        //   a Save As whose re-pick failed falls back to the download, and
        //   the stale handle from the previous format must not receive the
        //   fallback bytes (a .dwg file must never silently hold DXF).
        let tab_exists = self.tabs.iter().position(|tab| tab.id == tab_id).is_some();
        let target_ext = crate::io::fs_access::take_pending_save_ext(tab_id);
        let write_in_place = tab_exists
            && crate::io::fs_access::write_or_download(
                crate::io::fs_access::handle_ext(tab_id).as_deref(),
                target_ext.as_deref(),
            );
        if !write_in_place {
            // Whatever handle is held is stale for these bytes (wrong
            // format, unverifiable format, or a tab that declined the
            // save): drop it so the next save re-picks instead of hitting
            // the same mismatch. No handle held: the drop is a no-op.
            crate::io::fs_access::drop_handle(tab_id);
        } else if let Some(picked_name) = crate::io::fs_access::handle_name(tab_id) {
            // The bookkeeping runs at write completion (WebSaveWritten) so a
            // failed write cannot clear the dirty flag; the bytes ride back
            // with the completion message so the OPFS recent store gets the
            // same copy the download path uses.
            let download_name = picked_name.clone();
            return Task::perform(
                async move {
                    let result = if let Err(error) =
                        crate::io::fs_access::write_handle(tab_id, &bytes).await
                    {
                        // Keep the save alive: the drawing still lands
                        // somewhere via the legacy download (which needs no
                        // activation), and the error travels with the
                        // completion message for the command-line note.
                        crate::sys::download_bytes(&download_name, &bytes);
                        Err(error)
                    } else {
                        Ok(())
                    };
                    (result, bytes)
                },
                move |(result, bytes)| Message::WebSaveWritten {
                    tab_id,
                    filename: picked_name,
                    autosave_key,
                    version,
                    geometry_epoch,
                    edit_revision,
                    camera_generation,
                    result,
                    bytes,
                },
            );
        }
        // The download is the only artifact of the save — fire it even if the
        // tab was closed while serialization was in flight.
        if crate::io::host_bridge::is_embed_mode() {
            if let Some(fmt) = crate::io::host_bridge::cad_save_format(&filename) {
                crate::io::host_bridge::emit_save(
                    &filename,
                    fmt,
                    &bytes,
                    tab_id,
                    geometry_epoch,
                    edit_revision,
                    camera_generation,
                );
            } else {
                crate::sys::download_bytes(&filename, &bytes);
            }
        } else {
            crate::sys::download_bytes(&filename, &bytes);
        }
        self.finalize_web_save(
            tab_id,
            filename,
            autosave_key,
            version,
            geometry_epoch,
            edit_revision,
            camera_generation,
            bytes,
        )
    }

    /// Web SAVE in-place completion: the retained handle's write settled.
    /// Success keeps the handle for the next plain Save; failure drops it
    /// (the task already downloaded a copy — note that in the command line,
    /// and re-pick on the next save instead of failing the same way again).
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_save_written(
        &mut self,
        tab_id: u64,
        filename: String,
        autosave_key: String,
        version: document::DxfVersion,
        geometry_epoch: u64,
        edit_revision: u64,
        camera_generation: u64,
        result: Result<(), String>,
        bytes: Vec<u8>,
    ) -> Task<Message> {
        if let Err(error) = result {
            crate::io::fs_access::drop_handle(tab_id);
            self.command_line.push_error(
                crate::tf!("In-place save failed ({error}); a copy was downloaded instead")
                    .as_ref(),
            );
        }
        self.finalize_web_save(
            tab_id,
            filename,
            autosave_key,
            version,
            geometry_epoch,
            edit_revision,
            camera_generation,
            bytes,
        )
    }

    /// Shared completion half of the web save — download and in-place write
    /// alike: recent-copy store task, tab bookkeeping under the race guard
    /// (the dirty flag only clears when the persisted snapshot is still the
    /// tab's current state), autosave-key retirement and the "Saved" line.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn finalize_web_save(
        &mut self,
        tab_id: u64,
        filename: String,
        autosave_key: String,
        version: document::DxfVersion,
        geometry_epoch: u64,
        edit_revision: u64,
        camera_generation: u64,
        bytes: Vec<u8>,
    ) -> Task<Message> {
        // The save settled (download or in-place write): release the
        // in-flight slot the on_save_file guard refuses against.
        crate::io::fs_access::end_web_save(tab_id);
        let embed_mode = crate::io::host_bridge::is_embed_mode();
        let hold_for_host_ack = crate::io::host_bridge::holds_dirty_for_ack(&filename);
        let mut recent_task = if embed_mode {
            Task::none()
        } else {
            let cache_name = std::path::Path::new(&filename)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| filename.clone());
            let path = std::path::PathBuf::from(cache_name);
            Task::perform(
                async move {
                    crate::io::web_recent::store(&path.to_string_lossy(), &bytes)
                        .await
                        .map(|_| path)
                },
                Message::WebRecentStored,
            )
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return recent_task;
        };
        self.tabs[i].current_path = Some(
            std::path::Path::new(&filename)
                .file_name()
                .map(|name| std::path::PathBuf::from(name.to_string_lossy().into_owned()))
                .unwrap_or_else(|| std::path::PathBuf::from(filename.clone())),
        );
        self.tabs[i].scene.document.version = version;
        self.tabs[i].recovery_save_as_required = false;
        // A save may have renamed the tab (Save As): refresh the remembered
        // session list (web session memory).
        self.persist_web_session();
        let snapshot_is_current = self.tabs[i].scene.geometry_epoch == geometry_epoch
            && self.tabs[i].edit_revision == edit_revision
            && self.tabs[i].scene.camera_generation == camera_generation;
        if snapshot_is_current && !hold_for_host_ack {
            self.tabs[i].dirty = false;
            self.tabs[i].scene.note_tables_saved();
        }
        // A clean save supersedes any crash-recovery snapshot for this
        // drawing — drop the old key and the new one so stale copies don't
        // resurface in the next boot scan. But an autosave round may still be
        // writing the same key: its write would land AFTER the remove and
        // resurrect the snapshot (with a newer mtime, so the next scan would
        // offer it as recoverable). Defer the removes to the end of the round.
        if !embed_mode {
            let new_key = self.tabs[i]
                .current_path
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default();
            if self.web_autosave_in_flight {
                self.web_autosave_clear_after.push(autosave_key);
                if !new_key.is_empty() {
                    self.web_autosave_clear_after.push(new_key);
                }
            } else {
                recent_task = Task::batch([
                    recent_task,
                    Task::perform(
                        async move {
                            let _ = crate::io::web_autosave::remove(&autosave_key).await;
                            let _ = crate::io::web_autosave::remove(&new_key).await;
                        },
                        |_| Message::Noop,
                    ),
                ]);
            }
        }
        if !hold_for_host_ack {
            self.command_line
                .push_output(crate::tf!("Saved: {filename}").as_ref());
        }
        Task::batch([recent_task, self.finish_web_save(true)])
    }

    /// Tail of the web save flow: continue a pending close when the save
    /// succeeded, or re-open the unsaved-changes dialog when it failed.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn finish_web_save(&mut self, saved: bool) -> Task<Message> {
        if self.save_dialog_for_unsaved {
            if saved {
                // Only a Tab pending is consumed here; a Quit pending must
                // survive to continue the quit after this save.
                if let Some(crate::app::PendingClose::Tab(tab_id)) = self
                    .pending_close
                    .take_if(|pending| matches!(pending, crate::app::PendingClose::Tab(_)))
                {
                    let continuation = self.update(Message::TabClose(tab_id));
                    let rest = self.continue_tab_close_queue();
                    return Task::batch([continuation, rest]);
                }
            } else if matches!(self.pending_close, Some(crate::app::PendingClose::Quit)) {
                // Mirror the native Quit continuation: offer to save the
                // next dirty tab, or finish the quit.
                if self.tabs.iter().any(|tab| tab.dirty) {
                    return self.open_unsaved_dialog_window();
                }
                return Task::batch([self.close_unsaved_dialog_window(), self.exit_app()]);
            } else if self.pending_close.is_some() {
                return self.open_unsaved_dialog_window();
            }
        }
        Task::none()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn queue_native_save(
        &mut self,
        i: usize,
        path: std::path::PathBuf,
        version: document::DxfVersion,
        purpose: crate::app::SavePurpose,
        continuation: crate::app::SaveContinuation,
        set_current_path: bool,
        check_external_change: bool,
    ) -> Task<Message> {
        if self.refuse_block_test_save(i) {
            return Task::none();
        }
        let allow_conversion_loss = std::mem::take(&mut self.conversion_loss_allowed);
        if !allow_conversion_loss && purpose != crate::app::SavePurpose::Autosave {
            let ext = path
                .extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if let Some(losses) = self.conversion_losses_to_confirm(i, &ext, version) {
                let save = crate::app::PendingNativeThumbnailSave {
                    tab_id: self.tabs[i].id,
                    path,
                    version,
                    purpose,
                    continuation,
                    set_current_path,
                    check_external_change,
                    allow_conversion_loss: false,
                };
                let pending = crate::app::PendingConversionSave::Native(save);
                return self.ask_conversion_loss(losses, pending);
            }
        }
        if purpose != crate::app::SavePurpose::Autosave
            && self.needs_ifccad_alpha_consent(
                i,
                path.extension().and_then(|e| e.to_str()).unwrap_or(""),
            )
        {
            let save = crate::app::PendingNativeThumbnailSave {
                tab_id: self.tabs[i].id,
                path,
                version,
                purpose,
                continuation,
                set_current_path,
                check_external_change,
                allow_conversion_loss,
            };
            self.pending_ifccad_alpha_save = Some(crate::app::PendingConversionSave::Native(save));
            self.active_modal = Some(crate::app::ModalKind::IfccadAlphaWarning);
            return Task::none();
        }
        let tab_id = self.tabs[i].id;
        if self
            .pending_native_thumbnail_save
            .as_ref()
            .is_some_and(|pending| pending.tab_id == tab_id)
        {
            if purpose != crate::app::SavePurpose::Autosave {
                self.command_line
                    .push_info(crate::t!("Save already running for this drawing.").as_ref());
            }
            return Task::none();
        }
        let capture_ready =
            self.thumbnail_capture_clean && self.pending_native_thumbnail_save.is_none();
        if self.active_save_jobs.contains_key(&tab_id) {
            if purpose != crate::app::SavePurpose::Autosave {
                self.command_line
                    .push_info(crate::t!("Save already running for this drawing.").as_ref());
            }
            if capture_ready {
                self.thumbnail_capture_clean = false;
            }
            return Task::none();
        }
        let destination_is_current = self.tabs[i]
            .current_path
            .as_deref()
            .is_some_and(|current| native_paths_match(current, &path));
        if purpose != crate::app::SavePurpose::Autosave
            && self.tabs[i].recovery_save_as_required
            && destination_is_current
        {
            self.command_line
                .push_error_once(crate::tr!("recovery", "save-new-file-required").as_ref());
            self.restore_failed_save_continuation(continuation, i);
            self.active_tab = i;
            self.save_dialog_for_unsaved = continuation != crate::app::SaveContinuation::None;
            if capture_ready {
                self.thumbnail_capture_clean = false;
            }
            return self.open_save_dialog_window(i);
        }
        if purpose != crate::app::SavePurpose::Autosave
            && destination_is_current
            && self.tabs[i].edit_lock_conflict
        {
            let error = "Drawing edit lock is held by another editor.".to_string();
            self.command_line.push_error_once(
                crate::tf!(
                    "Unable to save \"{}\": {error}",
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.display().to_string())
                )
                .as_ref(),
            );
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
            if capture_ready {
                self.thumbnail_capture_clean = false;
            }
            return Task::none();
        }

        if purpose != crate::app::SavePurpose::Autosave
            && i == self.active_tab
            && !self.thumbnail_capture_clean
            && self.main_window.is_some()
            && crate::ui::wrap_bar::dropdown_bounds(crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID)
                .is_some()
        {
            self.pending_native_thumbnail_save = Some(crate::app::PendingNativeThumbnailSave {
                tab_id,
                path,
                version,
                purpose,
                continuation,
                set_current_path,
                check_external_change,
                allow_conversion_loss,
            });
            self.thumbnail_capture_clean = true;
            return Task::none();
        }

        if set_current_path && !destination_is_current {
            match crate::io::edit_lock::EditLease::acquire(&path) {
                Ok(lease) => {
                    self.pending_save_leases.insert(tab_id, lease);
                }
                Err(crate::io::edit_lock::EditLeaseError::Locked(error)) => {
                    self.command_line.push_error_once(
                        crate::tf!(
                            "Unable to save \"{}\": {error}",
                            path.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.display().to_string())
                        )
                        .as_ref(),
                    );
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
                    if capture_ready {
                        self.thumbnail_capture_clean = false;
                    }
                    return Task::none();
                }
                Err(crate::io::edit_lock::EditLeaseError::Unavailable(error)) => {
                    self.command_line.push_info(
                        crate::tf!("{error}. External-change checks remain active.").as_ref(),
                    );
                }
            }
        }

        let epoch = self.tabs[i].scene.geometry_epoch;
        let revision = self.tabs[i].edit_revision;
        let camera_generation = self.tabs[i].scene.camera_generation;
        let thumbnail = (purpose != crate::app::SavePurpose::Autosave && i == self.active_tab)
            .then_some(version >= document::DxfVersion::AC1027);
        let capture_bounds = thumbnail.and_then(|_| {
            crate::ui::wrap_bar::dropdown_bounds(crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID)
        });
        let clone_started = iced::time::Instant::now();
        let mut snapshot = self.tabs[i].document_for_save();
        // First save: references attached with a relative path type while
        // the drawing had no file become relative to it.
        if self.tabs[i].current_path.is_none() && purpose != crate::app::SavePurpose::Autosave {
            crate::io::xref::make_relative(
                &mut snapshot,
                &self.tabs[i].xref_relative_on_save,
                &path,
            );
        }
        // Save-As across folders: rebase relative reference paths onto the
        // new base dir inside the snapshot only (live strings are untouched).
        if matches!(
            purpose,
            crate::app::SavePurpose::SaveAs | crate::app::SavePurpose::SaveAsNew
        ) {
            let old_base = self.tabs[i]
                .current_path
                .as_ref()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()));
            let new_base = path.parent().map(|p| p.to_path_buf());
            if let (Some(old), Some(new)) = (old_base, new_base) {
                if old != new {
                    crate::io::xref::rebase_relative_paths_for_save_as(&mut snapshot, &old, &new);
                }
            }
        }
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
        let previous_autosave =
            (purpose != crate::app::SavePurpose::Autosave).then(|| self.autosave_target(i));
        let backup = purpose != crate::app::SavePurpose::Autosave && self.backup_on_save;
        let verification = if check_external_change && purpose != crate::app::SavePurpose::Autosave
        {
            let expected = if destination_is_current {
                self.tabs[i].disk_fingerprint.clone()
            } else {
                None
            };
            if self.pending_save_leases.contains_key(&tab_id) {
                Self::native_save_verification(
                    &path,
                    self.pending_save_leases.get_mut(&tab_id),
                    expected,
                )
            } else if destination_is_current {
                Self::native_save_verification(&path, self.tabs[i].edit_lease.as_mut(), expected)
            } else {
                Self::native_save_verification(&path, None, expected)
            }
        } else {
            Ok((None, None))
        };
        let (expected_fingerprint, verify_reader) = match verification {
            Ok(verification) => verification,
            Err(error) => {
                if capture_ready {
                    self.thumbnail_capture_clean = false;
                }
                return Task::perform(
                    async move {
                        crate::app::SaveOutcome {
                            job_id,
                            tab_id,
                            epoch,
                            revision,
                            camera_generation,
                            path,
                            physical_path: None,
                            version,
                            previous_autosave,
                            set_current_path,
                            purpose,
                            continuation,
                            refreshed_preview: None,
                            result: Err(error),
                        }
                    },
                    Message::SaveFinished,
                );
            }
        };
        // Resolve only the writer's path. The logical path below remains the
        // reference base and the path installed on successful completion.
        let worker_path = if destination_is_current {
            std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone())
        } else {
            path.clone()
        };
        let capture_window = thumbnail.and(capture_bounds).and(self.main_window);
        let ifccad_session = self.tabs[i].ifccad_session.clone();
        let mut work = Some((
            snapshot,
            thumbnail,
            capture_bounds,
            worker_path,
            expected_fingerprint,
            verify_reader,
            path,
            previous_autosave,
        ));
        let mut run_save = move |screenshot: Option<iced::window::Screenshot>| {
            let ifccad_session = ifccad_session.clone();
            let (
                mut snapshot,
                thumbnail,
                capture_bounds,
                worker_path,
                expected_fingerprint,
                verify_reader,
                path,
                previous_autosave,
            ) = work
                .take()
                .expect("save capture produced more than one result");
            Task::perform(
                async move {
                    let physical_path = destination_is_current.then(|| worker_path.clone());
                    let logical_path = destination_is_current.then(|| path.clone());
                    let (result, refreshed_preview) = native_save_worker(move || {
                        let mut refreshed_preview = None;
                        if let Some(png) = thumbnail {
                            let started = iced::time::Instant::now();
                            if let Some(preview) = screenshot.as_ref().and_then(|screenshot| {
                                capture_bounds.and_then(|bounds| {
                                    crate::io::thumbnail::from_screenshot(screenshot, bounds, png)
                                })
                            }) {
                                snapshot.preview = Some(preview);
                                refreshed_preview = Some(snapshot.preview.clone());
                            }
                            if crate::perf::enabled() {
                                crate::perf_record!(
                                    "[perf] save-thumbnail {:.1}ms",
                                    started.elapsed().as_secs_f64() * 1000.0,
                                );
                            }
                        }
                        let result = crate::io::save_owned_as_version_atomic(
                            snapshot,
                            &worker_path,
                            version,
                            backup,
                            expected_fingerprint,
                            verify_reader,
                            allow_conversion_loss,
                            purpose == crate::app::SavePurpose::SaveAsNew,
                            logical_path.as_deref(),
                            ifccad_session.as_ref(),
                        );
                        (result, refreshed_preview)
                    })
                    .await
                    .unwrap_or_else(|error| (Err(error), None));
                    crate::app::SaveOutcome {
                        job_id,
                        tab_id,
                        epoch,
                        revision,
                        camera_generation,
                        path,
                        physical_path,
                        version,
                        previous_autosave,
                        set_current_path,
                        purpose,
                        continuation,
                        refreshed_preview,
                        result,
                    }
                },
                Message::SaveFinished,
            )
        };
        match capture_window {
            // The screenshot may never come back (a window the compositor
            // does not draw), so it races a short timeout: whichever answers
            // first runs the save, with or without the picture. A minimized
            // window is not read back at all: the save keeps the old preview.
            Some(window) => {
                fn finish<F>(
                    slot: &std::sync::Mutex<Option<F>>,
                    screenshot: Option<iced::window::Screenshot>,
                ) -> Task<Message>
                where
                    F: FnMut(Option<iced::window::Screenshot>) -> Task<Message>,
                {
                    match slot.lock().ok().and_then(|mut run| run.take()) {
                        Some(mut run_save) => Task::batch([
                            Task::done(Message::ThumbnailCaptureFinished),
                            run_save(screenshot),
                        ]),
                        None => Task::none(),
                    }
                }
                let slot = std::sync::Arc::new(std::sync::Mutex::new(Some(run_save)));
                let (shot_slot, timeout_slot) = (slot.clone(), slot);
                let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = tx.send(());
                });
                Task::batch([
                    crate::app::screenshot_if_drawn(window)
                        .then(move |screenshot| finish(&shot_slot, screenshot)),
                    Task::perform(rx, |_| ()).then(move |_| finish(&timeout_slot, None)),
                ])
            }
            None => {
                self.thumbnail_capture_clean = false;
                run_save(None)
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_save_finished(
        &mut self,
        mut outcome: crate::app::SaveOutcome,
    ) -> Task<Message> {
        let latest = self.active_save_jobs.get(&outcome.tab_id).copied() == Some(outcome.job_id);
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

        if outcome.result.is_ok() {
            if let Some(physical) = outcome.physical_path.as_deref() {
                outcome.result = crate::io::verify_save_destination(&outcome.path, physical);
            }
        }
        if let Err(error) = &outcome.result {
            if error.externally_modified && outcome.purpose != crate::app::SavePurpose::Autosave {
                let file_name = outcome
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| outcome.path.display().to_string());
                self.command_line.push_error_once(
                    crate::tf!("Save stopped: \"{file_name}\" changed outside Open CAD Studio.")
                        .as_ref(),
                );
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
                self.command_line.push_error_once(
                    crate::tf!(
                        "Unable to save \"{file_name}\": file is in use by another application."
                    )
                    .as_ref(),
                );
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
                        self.pending_close = Some(crate::app::PendingClose::Tab(self.tabs[i].id));
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
                .push_error(crate::tf!("Save failed: {error}").as_ref());
            return match outcome.continuation {
                crate::app::SaveContinuation::CloseTab => {
                    self.pending_close = Some(crate::app::PendingClose::Tab(self.tabs[i].id));
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
        }
        let mut tasks = Vec::new();
        match outcome.purpose {
            crate::app::SavePurpose::Autosave => {
                self.command_line
                    .push_output(crate::t!("Autosaved 1 drawing").as_ref());
            }
            crate::app::SavePurpose::Manual
            | crate::app::SavePurpose::SaveAs
            | crate::app::SavePurpose::SaveAsNew => {
                let path_changed = outcome.set_current_path
                    && self.tabs[i]
                        .current_path
                        .as_deref()
                        .is_none_or(|current| !native_paths_match(current, &outcome.path));
                self.command_line
                    .push_output(crate::tf!("Saved: {}", outcome.path.display()).as_ref());
                if let Some(previous) = outcome.previous_autosave {
                    if previous != outcome.path {
                        let _ = std::fs::remove_file(previous);
                    }
                }
                if outcome.set_current_path {
                    let old_path = self.tabs[i].current_path.clone();
                    if old_path.is_none() {
                        let pending = std::mem::take(&mut self.tabs[i].xref_relative_on_save);
                        crate::io::xref::make_relative(
                            &mut self.tabs[i].scene.document,
                            &pending,
                            &outcome.path,
                        );
                    }
                    self.tabs[i].current_path = Some(outcome.path.clone());
                    self.tabs[i].scene.document.version = outcome.version;
                    if matches!(
                        outcome.purpose,
                        crate::app::SavePurpose::SaveAs | crate::app::SavePurpose::SaveAsNew
                    ) {
                        self.tabs[i].recovery_save_as_required = false;
                        // Save-As across folders: the save snapshot was rebased
                        // in `queue_native_save`; rebase the live document too
                        // so the running session agrees with the file just
                        // written. Saves are not undoable, so no undo snapshot
                        // is pushed.
                        if let Some(old) = old_path.as_deref() {
                            let old_base = old.parent().map(|p| p.to_path_buf());
                            let new_base = outcome.path.parent().map(|p| p.to_path_buf());
                            if let (Some(old), Some(new)) = (old_base, new_base) {
                                if old != new {
                                    crate::io::xref::rebase_relative_paths_for_save_as(
                                        &mut self.tabs[i].scene.document,
                                        &old,
                                        &new,
                                    );
                                }
                            }
                        }
                    }
                }
                tasks.push(self.push_recent(outcome.path.clone()));
                self.refresh_native_edit_guard_after_save(
                    i,
                    &outcome.path,
                    path_changed,
                    destination_lease,
                );
                if snapshot_is_current {
                    self.tabs[i].dirty = false;
                    self.tabs[i].scene.note_tables_saved();
                }
            }
        }

        match outcome.continuation {
            crate::app::SaveContinuation::None => {}
            crate::app::SaveContinuation::CloseTab if snapshot_is_current => {
                self.pending_close = None;
                tasks.push(self.close_unsaved_dialog_window());
                tasks.push(self.update(Message::TabClose(self.tabs[i].id)));
                tasks.push(self.continue_tab_close_queue());
            }
            crate::app::SaveContinuation::Quit if snapshot_is_current => {
                self.pending_close = None;
                if self.file_prompts_on_command_line() {
                    // EXIT on the command line asks about the next drawing.
                    tasks.push(self.close_unsaved_dialog_window());
                    tasks.push(self.exit_from_command_line());
                } else if self.tabs.iter().any(|tab| tab.dirty) {
                    self.pending_close = Some(crate::app::PendingClose::Quit);
                    tasks.push(self.open_unsaved_dialog_window());
                } else {
                    tasks.push(self.close_unsaved_dialog_window());
                    tasks.push(self.exit_app());
                }
            }
            crate::app::SaveContinuation::CloseTab => {
                self.pending_close = Some(crate::app::PendingClose::Tab(self.tabs[i].id));
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
    pub(in crate::app) fn restore_failed_save_continuation(
        &mut self,
        continuation: crate::app::SaveContinuation,
        tab_idx: usize,
    ) {
        self.pending_close = match continuation {
            crate::app::SaveContinuation::None => None,
            crate::app::SaveContinuation::CloseTab => self
                .tabs
                .get(tab_idx)
                .map(|tab| crate::app::PendingClose::Tab(tab.id)),
            crate::app::SaveContinuation::Quit => Some(crate::app::PendingClose::Quit),
        };
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_save_file_in_use_retry(&mut self) -> Task<Message> {
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
                    .push_error_once(crate::tf!("Unable to acquire edit lock: {error}").as_ref());
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
    pub(in crate::app) fn on_save_file_in_use_save_as(&mut self) -> Task<Message> {
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
        self.save_dialog_for_unsaved = failure.continuation != crate::app::SaveContinuation::None;
        self.open_save_dialog_window(i)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_external_change_overwrite(&mut self) -> Task<Message> {
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
            // This dialog answer explicitly authorizes replacing the file
            // that appeared while a create-only control save was running.
            if conflict.purpose == crate::app::SavePurpose::SaveAsNew {
                crate::app::SavePurpose::SaveAs
            } else {
                conflict.purpose
            },
            conflict.continuation,
            conflict.set_current_path,
            false,
        )
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_external_change_save_as(&mut self) -> Task<Message> {
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
        self.save_dialog_for_unsaved = conflict.continuation != crate::app::SaveContinuation::None;
        self.open_save_dialog_window(i)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_external_change_reload(&mut self) -> Task<Message> {
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
                    .push_error(crate::tf!("Reload failed: {error}").as_ref());
                return Task::none();
            }
        };
        self.close_active_modal();
        self.pending_close = None;
        // The drawing on this slot is replaced whole and its identity goes
        // with it, so the positions it remembered go too: the drawing read
        // back from disk is not the one that placed them.
        crate::modules::draw::draw::attdef::session().forget_place_of(conflict.tab_id);
        crate::modules::annotate::text::forget_place_of(conflict.tab_id);
        self.tab_counter += 1;
        self.tabs[i] = crate::app::tab_document::DocumentTab::new_drawing(self.tab_counter);
        self.active_tab = i;
        self.apply_display_defaults(i);
        self.update(Message::OpenPathPicked(Some((
            conflict.path,
            metadata.len(),
        ))))
    }

    pub(in crate::app) fn on_save_file(&mut self) -> Task<Message> {
        if self.refuse_block_test_save(self.active_tab) {
            return Task::none();
        }
        if self.read_only {
            #[cfg(target_arch = "wasm32")]
            if crate::io::host_bridge::is_embed_mode() {
                crate::io::host_bridge::emit_error(
                    "read_only",
                    "Read-only session: saving is disabled.",
                );
            }
            self.command_line.push_error(
                crate::t!("Read-only session (--read-only): saving is disabled.").as_ref(),
            );
            return Task::none();
        }
        let i = self.active_tab;
        // DXEVAL: a save may first offer (or make) outdated extraction
        // table updates.
        if let Some(task) = self.evaluate_extraction_tables(
            i,
            crate::app::commands::extraction_eval::EVENT_SAVE,
            Some(Message::SaveFile),
        ) {
            return task;
        }
        // Web serializes immediately below. Native preparation happens
        // once after the destination/version is known.
        #[cfg(target_arch = "wasm32")]
        {
            // The web counterpart of native's active_save_jobs guard: one
            // capture → serialize → write/download chain at a time. The
            // window is seconds, so refuse rather than queue — same
            // localized message the native flow shows.
            if let Some(busy_tab) = crate::io::fs_access::web_save_in_flight() {
                if self.tabs.iter().any(|tab| tab.id == busy_tab) {
                    self.command_line
                        .push_info(crate::t!("Save already running for this drawing.").as_ref());
                    return Task::none();
                }
                // Stale marker (its tab closed before the save could
                // settle): clear it instead of blocking every later save.
                crate::io::fs_access::end_web_save(busy_tab);
            }
            self.sync_view_state_for_save(i);
            self.stamp_header_sysvars(i);
        }
        // Native: save straight to the known path. Web has no path
        // (downloads instead), so always go through the Save dialog.
        #[cfg(not(target_arch = "wasm32"))]
        if !self.tabs[i].recovery_save_as_required {
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
        }
        self.save_dialog_for_unsaved = false;
        self.save_with_default_format(i)
    }

    /// Save without the version picker: use the configured default and go straight to
    /// the native destination dialog (native) or the browser download (web).
    /// Used by plain Save (QSAVE) on an as-yet-unsaved drawing and by the
    /// save-before-close flow — the version picker is reserved for Save As.
    pub(in crate::app) fn save_with_default_format(&mut self, tab_idx: usize) -> Task<Message> {
        if self.refuse_block_test_save(tab_idx) {
            return Task::none();
        }
        self.active_tab = tab_idx;
        // Every caller of this entry (Save/QSAVE, the save-before-close
        // flow) is plain-save semantics: on the web, the tab's retained
        // FS Access handle may be written through directly. Save As opens
        // the dialog modal instead (open_save_dialog_window resets this).
        #[cfg(target_arch = "wasm32")]
        {
            self.save_plain_reuse_fs_handle = true;
        }
        self.save_dialog_format = if self.tabs[tab_idx].recovery_save_as_required {
            let document = &self.tabs[tab_idx].scene.document;
            let is_dxf =
                crate::io::source_is_dxf(self.tabs[tab_idx].current_path.as_deref(), document);
            let version = if is_dxf {
                document.version
            } else {
                document.dwg_source_version.unwrap_or(document.version)
            };
            crate::io::format_for_version(version, is_dxf)
        } else if let Some(format) = self.current_file_format(tab_idx) {
            // A drawing that already has a file keeps that file's format;
            // the configured default is for new drawings (#2215).
            format
        } else {
            self.default_save_format.clone()
        };
        let (ext, _) = crate::io::parse_save_format(&self.save_dialog_format);
        self.save_dialog_filename = self.tabs[tab_idx]
            .current_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{}.{ext}", self.tabs[tab_idx].tab_display_name()));
        if self.tabs[tab_idx].recovery_save_as_required {
            let path = std::path::Path::new(&self.save_dialog_filename);
            let stem = path
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| "drawing".to_string());
            self.save_dialog_filename = format!("{stem}_recovered.{ext}");
        }
        self.aec_drop_acknowledged = false;
        self.on_save_dialog_confirm()
    }

    /// The save format of the tab's current file, by its extension: the
    /// converter for a converter format, else DWG or DXF in the document's
    /// version. `None` for an unsaved drawing or another extension.
    pub(in crate::app) fn current_file_format(&self, tab_idx: usize) -> Option<String> {
        let path = self.tabs[tab_idx].current_path.as_deref()?;
        let ext = path.extension()?.to_str()?;
        if let Some(converter) = ocs_save::external_format(ext) {
            return Some(ocs_save::external_format_label(converter).to_string());
        }
        let is_dxf = ext.eq_ignore_ascii_case("dxf");
        (is_dxf || ext.eq_ignore_ascii_case("dwg")).then(|| {
            crate::io::format_for_version(self.tabs[tab_idx].scene.document.version, is_dxf)
        })
    }

    pub(in crate::app) fn on_save_dialog_confirm(&mut self) -> Task<Message> {
        let (ext, version) = crate::io::parse_save_format(&self.save_dialog_format);
        // Warn before a lossy Save-As that would drop unsupported
        // (AEC / application) objects kept only as verbatim
        // source-version bytes — let the user keep them by saving in the
        // source version, or proceed and drop them.
        if !self.aec_drop_acknowledged && ocs_save::external_format(ext).is_none() {
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
            // A template is saved in the templates folder (#3210).
            let seed_dir = if ext.eq_ignore_ascii_case("dwt") {
                self.template_dir()
            } else {
                self.tabs[i]
                    .current_path
                    .as_ref()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            };
            let default_name = filename;
            let (filter_label, filter_ext): (&str, &str) =
                if let Some(converter) = ocs_save::external_format(ext) {
                    (ocs_save::external_format_label(converter), ext)
                } else if ext.eq_ignore_ascii_case("dxf") {
                    ("DXF Files", "dxf")
                } else if ext.eq_ignore_ascii_case("dwt") {
                    ("Drawing Template (*.dwt)", "dwt")
                } else {
                    ("DWG Files", "dwg")
                };
            let close = self.close_save_dialog_window();
            // macOS: a parentless panel opened while the Save As dialog
            // closes loses the key window and dismisses itself (#3055);
            // attach it to the main window as a sheet instead.
            #[cfg(target_os = "macos")]
            if let Some(window_id) = self.main_window {
                let title = crate::t!("Save Drawing As").to_string();
                let filter_label = filter_label.to_string();
                let filter_ext = filter_ext.to_string();
                let pick = iced::window::run(window_id, move |parent| {
                    let mut dlg = crate::sys::blocking_file_dialog()
                        .set_parent(parent)
                        .set_title(&title)
                        .set_file_name(&default_name)
                        .add_filter(&filter_label, &[filter_ext.as_str()]);
                    if let Some(dir) = seed_dir {
                        dlg = dlg.set_directory(dir);
                    }
                    let path = dlg.save_file()?;
                    crate::config::remember_dialog_dir(&path);
                    Some(path)
                })
                .map(Message::SaveDialogPathPicked);
                return Task::batch([close, pick]);
            }
            let pick = Task::perform(
                async move {
                    let mut dlg = crate::sys::file_dialog()
                        .set_title(crate::t!("Save Drawing As").as_ref())
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
        // Web: no filesystem path — the destination is either a retained
        // File System Access handle (in-place write), a fresh
        // showSaveFilePicker pick, or the legacy download.
        #[cfg(target_arch = "wasm32")]
        {
            let close = self.close_save_dialog_window();
            self.sync_view_state_for_save(i);
            sync_annotation_scale_header(&mut self.tabs[i].scene);
            self.stamp_header_sysvars(i);
            self.sync_solid_models_for_save(i);
            self.tabs[i].scene.sync_native_parametric_graph();
            let tab_id = self.tabs[i].id;
            // File System Access routing, decided HERE: showSaveFilePicker
            // may only run while the page holds the Save click's transient
            // user activation, and the thumbnail capture + worker
            // serialization that follow can outlast that window. So the
            // handle is secured at this gesture moment (or reused from a
            // previous pick — retained handles need no activation to write)
            // and only the write happens later.
            // A registered host save hook takes the bytes through the
            // download path, so no file picker opens.
            let route = crate::io::fs_access::save_route(
                crate::io::fs_access::supported()
                    && crate::sys::save_hook().is_none()
                    && !crate::io::host_bridge::is_embed_mode(),
                self.save_plain_reuse_fs_handle,
                crate::io::fs_access::handle_ext(tab_id).as_deref(),
                ext,
            );
            match route {
                crate::io::fs_access::SaveRoute::WriteInPlace => {
                    // Plain Save on a matching handle — THE point: Ctrl+S
                    // updates the picked file, no dialog at all. The real
                    // name (the user may have renamed it in the picker)
                    // drives the save bookkeeping.
                    let picked_name = crate::io::fs_access::handle_name(tab_id).unwrap_or(filename);
                    Task::batch([
                        close,
                        self.web_save_capture(tab_id, picked_name, ext.to_string(), version),
                    ])
                }
                crate::io::fs_access::SaveRoute::PickHandle => {
                    // The picker IS the save dialog here: it opens inside
                    // the click's activation window, before any serialize
                    // task starts. Cancelling it cancels the save (same as
                    // a native save-dialog cancel); a picker failure falls
                    // back to the legacy download.
                    let suggested = filename;
                    let pick_suggested = suggested.clone();
                    Task::batch([
                        close,
                        Task::perform(
                            async move {
                                crate::io::fs_access::pick_save_handle(tab_id, &pick_suggested, ext)
                                    .await
                            },
                            move |picked| Message::WebSaveHandlePicked {
                                tab_id,
                                filename: suggested,
                                ext: ext.to_string(),
                                version,
                                picked,
                            },
                        ),
                    ])
                }
                // Firefox/Safari (no showSaveFilePicker): untouched legacy
                // download flow.
                crate::io::fs_access::SaveRoute::Download => Task::batch([
                    close,
                    self.web_save_capture(tab_id, filename, ext.to_string(), version),
                ]),
            }
        }
    }

    /// Web save, destination-known half shared by the legacy download path
    /// and both File System Access paths: queue the viewport thumbnail
    /// capture (or skip straight to the serializer when the bounds are
    /// unavailable), which continues in `on_web_save_screenshot`.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn web_save_capture(
        &mut self,
        tab_id: u64,
        filename: String,
        ext: String,
        version: document::DxfVersion,
    ) -> Task<Message> {
        if !self.conversion_loss_allowed {
            let tab = self.tabs.iter().position(|tab| tab.id == tab_id);
            if let Some(losses) =
                tab.and_then(|i| self.conversion_losses_to_confirm(i, &ext, version))
            {
                let save = crate::app::PendingConversionSave::Web {
                    tab_id,
                    filename,
                    ext,
                    version,
                };
                return self.ask_conversion_loss(losses, save);
            }
        }
        if let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) {
            if self.needs_ifccad_alpha_consent(i, &ext) {
                self.pending_ifccad_alpha_save = Some(crate::app::PendingConversionSave::Web {
                    tab_id,
                    filename,
                    ext,
                    version,
                });
                self.active_modal = Some(crate::app::ModalKind::IfccadAlphaWarning);
                return Task::none();
            }
        }
        // The save is now committed to this tab and this format: mark it
        // in flight (the web counterpart of native's active_save_jobs —
        // on_save_file refuses a second save while the slot is held) and
        // record the intended ext, which the completion half re-checks
        // against the retained handle before writing.
        crate::io::fs_access::begin_web_save(tab_id, &ext);
        let bounds =
            crate::ui::wrap_bar::dropdown_bounds(crate::app::view::VIEWPORT_CAPTURE_BOUNDS_ID);
        let Some(bounds) = bounds else {
            return Task::done(Message::WebSaveScreenshot {
                tab_id,
                filename,
                ext,
                version,
                bounds: None,
                screenshot: None,
            });
        };
        self.pending_web_thumbnail_save = Some(crate::app::PendingWebThumbnailSave {
            tab_id,
            filename,
            ext,
            version,
            bounds,
        });
        self.thumbnail_capture_clean = true;
        Task::none()
    }

    /// Web SAVE: the showSaveFilePicker promise from the confirm handler
    /// settled. `Ok(Some(name))` continues the save against the picked
    /// file's real name (the registry already retains the handle, replacing
    /// any previous one — Save As re-picks); `Ok(None)` (dismissed picker)
    /// cancels the save like a native save-dialog cancel; `Err` keeps the
    /// save alive through the legacy download, with a note that the copy
    /// was downloaded — after dropping any previously retained handle,
    /// which was picked for the old format and must not receive the
    /// fallback bytes.
    #[cfg(target_arch = "wasm32")]
    pub(in crate::app) fn on_web_save_handle_picked(
        &mut self,
        tab_id: u64,
        filename: String,
        ext: String,
        version: document::DxfVersion,
        picked: Result<Option<String>, String>,
    ) -> Task<Message> {
        match picked {
            Ok(Some(picked_name)) => self.web_save_capture(tab_id, picked_name, ext, version),
            Ok(None) => {
                // The picker was the save dialog: dismissing it means no
                // save — the download fallback must NOT fire (that would
                // surprise the user with a stray "drawing (1).dwg"). If
                // this came from the unsaved-changes → save → close flow,
                // re-show the confirmation so the tab isn't lost.
                if self.save_dialog_for_unsaved && self.pending_close.is_some() {
                    return self.open_unsaved_dialog_window();
                }
                Task::none()
            }
            Err(error) => {
                // The picker failed and the download fallback is about to
                // run with bytes serialized for `ext` — but any handle still
                // held was picked for (possibly) the OTHER format (Save As
                // to DXF on a .dwg handle, …). Drop it now so the stale
                // handle cannot receive the fallback bytes when they land;
                // the next save re-picks.
                crate::io::fs_access::drop_handle(tab_id);
                self.command_line.push_error(
                    crate::tf!(
                        "Save destination picker failed: {error}; downloading a copy instead"
                    )
                    .as_ref(),
                );
                self.web_save_capture(tab_id, filename, ext, version)
            }
        }
    }

    /// Native: a destination path came back from the OS save dialog — write the
    /// file there in the chosen version. `None` means the user cancelled.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn on_save_dialog_path_picked(
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

    /// What a converter would lose saving tab `i` as `ext`, when the user has to
    /// confirm it first; `None` lets the save go ahead.
    pub(in crate::app) fn conversion_losses_to_confirm(
        &self,
        i: usize,
        ext: &str,
        version: document::DxfVersion,
    ) -> Option<Vec<String>> {
        let Some(format) = ocs_save::external_format(ext) else {
            // An older DWG or DXF target has no place for some drawing
            // properties or MTEXT data: both are listed.
            let mut losses = self
                .drawing_properties_loss(i, ext, version)
                .unwrap_or_default();
            if matches!(ext, "dwg" | "dwt" | "dxf") {
                let doc = &self.tabs[i].scene.document;
                losses.extend(
                    crate::io::mtext_losses_on_save(doc, version, ext == "dxf")
                        .into_iter()
                        .map(|(loss, count)| loss.message(count)),
                );
            }
            return (!losses.is_empty()).then_some(losses);
        };
        let prepared =
            crate::io::prepare_document_for_save(self.tabs[i].document_for_save(), version);
        // A write that fails outright is reported by the save itself.
        let losses = ocs_save::external_losses_for_edit(
            &prepared,
            format,
            self.tabs[i].ifccad_session.as_ref(),
        )
        .ok()?;
        (!losses.is_empty()).then_some(losses)
    }

    /// The drawing-properties warning when a DWG/DXF `version` cannot store the
    /// properties tab `i` has filled in; `None` when there is nothing to lose.
    pub(in crate::app) fn drawing_properties_loss(
        &self,
        i: usize,
        ext: &str,
        version: document::DxfVersion,
    ) -> Option<Vec<String>> {
        let (format, is_dxf) = match ext {
            "dxf" => ("DXF", true),
            "dwg" | "dwt" => ("DWG", false),
            _ => return None,
        };
        let lost = crate::io::drawing_properties_lost_on_save(
            &self.tabs[i].scene.document.summary_info,
            &self.tabs[i].scene.document.header.hyperlink_base,
            version,
            is_dxf,
        );
        (lost > 0).then(|| {
            vec![crate::tr!(
                "dwgprops-dialog",
                "save-loss",
                format = format,
                count = lost.to_string()
            )]
        })
    }

    /// Hold `pending` back and ask whether to save despite `losses`.
    pub(in crate::app) fn ask_conversion_loss(
        &mut self,
        losses: Vec<String>,
        pending: crate::app::PendingConversionSave,
    ) -> Task<Message> {
        self.conversion_losses = losses;
        let ifccad = pending.format_name() == "IFCCAD"
            && (cfg!(target_arch = "wasm32") || self.main_window.is_some());
        self.pending_conversion_save = Some(pending);
        self.active_modal = Some(if ifccad {
            crate::app::ModalKind::IfccadAlphaWarning
        } else {
            crate::app::ModalKind::ConversionLoss
        });
        Task::none()
    }

    /// Conversion-loss warning → "Save anyway": replay the held-back save,
    /// allowed to lose what the warning listed.
    pub(in crate::app) fn on_conversion_loss_proceed(&mut self) -> Task<Message> {
        self.active_modal = None;
        self.conversion_losses.clear();
        let Some(pending) = self.pending_conversion_save.take() else {
            return Task::none();
        };
        self.conversion_loss_allowed = true;
        self.resume_format_save(pending)
    }

    pub(in crate::app) fn needs_ifccad_alpha_consent(&self, i: usize, ext: &str) -> bool {
        (cfg!(target_arch = "wasm32") || self.main_window.is_some())
            && (ext.eq_ignore_ascii_case("ifccad") || ext.eq_ignore_ascii_case("ifcx"))
            && !self.tabs[i].ifccad_alpha_acknowledged
    }

    pub(in crate::app) fn on_ifccad_alpha_proceed(&mut self) -> Task<Message> {
        self.active_modal = None;
        let accepted_losses = self.pending_conversion_save.is_some();
        let pending = self
            .pending_ifccad_alpha_save
            .take()
            .or_else(|| self.pending_conversion_save.take());
        let Some(pending) = pending else {
            return Task::none();
        };
        self.conversion_losses.clear();
        let tab_id = match &pending {
            #[cfg(not(target_arch = "wasm32"))]
            crate::app::PendingConversionSave::Native(save) => save.tab_id,
            #[cfg(target_arch = "wasm32")]
            crate::app::PendingConversionSave::Web { tab_id, .. }
            | crate::app::PendingConversionSave::WebSaveAll { tab_id } => *tab_id,
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            self.conversion_loss_allowed = false;
            return Task::none();
        };
        self.tabs[i].ifccad_alpha_acknowledged = true;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let crate::app::PendingConversionSave::Native(save) = &pending;
            self.conversion_loss_allowed = accepted_losses || save.allow_conversion_loss;
        }
        #[cfg(target_arch = "wasm32")]
        if accepted_losses {
            self.conversion_loss_allowed = true;
        }
        self.resume_format_save(pending)
    }

    pub(in crate::app) fn resume_format_save(&mut self, pending: crate::app::PendingConversionSave) -> Task<Message> {
        match pending {
            #[cfg(target_arch = "wasm32")]
            crate::app::PendingConversionSave::WebSaveAll { .. } => self.saveall_web(),
            #[cfg(not(target_arch = "wasm32"))]
            crate::app::PendingConversionSave::Native(save) => {
                let Some(i) = self.tabs.iter().position(|tab| tab.id == save.tab_id) else {
                    self.conversion_loss_allowed = false;
                    return Task::none();
                };
                self.queue_native_save(
                    i,
                    save.path,
                    save.version,
                    save.purpose,
                    save.continuation,
                    save.set_current_path,
                    save.check_external_change,
                )
            }
            #[cfg(target_arch = "wasm32")]
            crate::app::PendingConversionSave::Web {
                tab_id,
                filename,
                ext,
                version,
            } => self.web_save_capture(tab_id, filename, ext, version),
        }
    }

    /// AEC-drop warning → "Save anyway": accept the loss and proceed with the
    /// format the user already chose.
    pub(in crate::app) fn on_aec_drop_proceed(&mut self) -> Task<Message> {
        self.aec_drop_acknowledged = true;
        self.active_modal = Some(crate::app::ModalKind::SaveDialog);
        self.on_save_dialog_confirm()
    }

    /// AEC-drop warning → "Save in source version": switch the target to the
    /// document's source type and version, then save.
    pub(in crate::app) fn on_aec_drop_same_version(&mut self) -> Task<Message> {
        let tab = &self.tabs[self.active_tab];
        let document = &tab.scene.document;
        let is_dxf = crate::io::source_is_dxf(tab.current_path.as_deref(), document);
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

    /// A recovery copy newer than the drawing holds work a crash cut off.
    /// This session's own copy goes when the drawing is saved or closed, so
    /// the old one moves aside to `<name>.ocs-recovered.sv$` and the notice
    /// says where it is.
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn notice_newer_autosave(&mut self, i: usize, path: &std::path::Path) {
        let modified = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        // Earlier versions kept the copy beside the drawing.
        let beside = path.with_file_name(format!(
            "{}.ocs-autosave.sv$",
            path.file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ));
        let Some(copy) = [self.autosave_target(i), beside].into_iter().find(|copy| {
            modified(copy)
                .zip(modified(path))
                .is_some_and(|(copy_time, file_time)| copy_time > file_time)
        }) else {
            return;
        };
        let name = copy
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let kept = copy.with_file_name(name.replace(".ocs-autosave.sv$", ".ocs-recovered.sv$"));
        let file = if kept != copy && std::fs::rename(&copy, &kept).is_ok() {
            kept
        } else {
            copy
        };
        self.command_line.push_warning(&crate::tr!(
            "recovery",
            "newer-autosave",
            file = file.display().to_string()
        ));
    }

}
