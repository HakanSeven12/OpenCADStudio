use super::super::*;

impl OpenCADStudio {
    pub(super) fn dispatch_input_file(&mut self, msg: Message) -> Task<Message> {
        match msg {
            Message::SpaceMouseWake => self.on_spacemouse_wake(),
            Message::TrackpadPinch(magnification) => self.on_pinch_zoom(magnification),
            Message::Touch(event) => self.on_touch(event),
            Message::SpaceMouseFrame(time) => {
                self.spacemouse
                    .frame(time.saturating_duration_since(self.start).as_secs_f64() * 1000.);
                Task::none()
            }
            Message::SpaceMouseFocus(id, focused) => {
                #[cfg(not(target_arch = "wasm32"))]
                let float = {
                    if focused {
                        self.focused_window = Some(id);
                    } else if self.focused_window == Some(id) {
                        self.focused_window = None;
                    }
                    // Moving between the app's own windows sends Unfocused then
                    // Focused, so they end up floating again.
                    crate::app::tool_windows::float_os_windows(
                        &self.os_windows,
                        focused,
                        self.focused_window,
                    )
                };
                #[cfg(target_arch = "wasm32")]
                let float = Task::none();
                if Some(id) == self.main_window {
                    self.spacemouse_ui.focused = focused;
                    if !focused {
                        self.release_pointer_navigation();
                    }
                }
                // Key tips close when the window loses focus, and an Alt
                // held while it left or arrived (Alt+Tab, window activation)
                // no longer counts as a lone tap.
                crate::ui::ribbon::keytip_note_focus_changed();
                if !focused && self.ribbon.keytips_active() {
                    return Task::batch([float, self.hide_keytips()]);
                }
                float
            }
            Message::SpaceMouseEnabled(enabled) => {
                self.spacemouse_ui.preferences.enabled = enabled;
                Task::none()
            }
            Message::SpaceMouseMode(mode) => {
                self.spacemouse_ui.preferences.mode = mode;
                Task::none()
            }
            Message::SpaceMousePanSpeed(speed) => {
                self.spacemouse_ui.preferences.pan_speed = speed.clamp(10, 300);
                Task::none()
            }
            Message::SpaceMousePanReversed(reversed) => {
                self.spacemouse_ui.preferences.pan_reversed = reversed;
                Task::none()
            }
            Message::SpaceMousePause => {
                self.spacemouse_ui.paused = !self.spacemouse_ui.paused;
                Task::none()
            }
            Message::SpaceMousePreferences => {
                self.open_spacemouse_preferences();
                Task::none()
            }
            Message::SpaceMouseDriverSettings => self.open_spacemouse_driver_settings(),
            Message::SpaceMouseDetails => {
                self.spacemouse_ui.details = !self.spacemouse_ui.details;
                Task::none()
            }
            Message::ControlRequest(envelope) => {
                // Mirror the automation trail into the message list so the
                // person at the screen can review what clients asked and
                // where it failed — the JSON reply alone reaches only the
                // caller. High-frequency polls are traffic, not activity.
                let op = envelope.request["op"].as_str().unwrap_or("").to_owned();
                let (response, task) = self.control_request(envelope.request);
                if !matches!(
                    op.as_str(),
                    "" | "state" | "hello" | "operation" | "events" | "capabilities"
                ) {
                    if response["ok"] == false {
                        let code = response["code"].as_str().unwrap_or("failed");
                        let detail = response["error"].as_str().unwrap_or("");
                        self.command_line
                            .push_error(&format!("automation {op}: {code} — {detail}"));
                    } else if matches!(
                        response["status"].as_str(),
                        Some("completed" | "cancelled" | "waiting_input")
                    ) && !matches!(
                        op.as_str(),
                        // These already narrate themselves in richer lines.
                        "user_select" | "getpoint" | "cancel"
                    ) {
                        self.command_line.push_info(&format!(
                            "automation {op}: {}",
                            response["status"].as_str().unwrap_or("")
                        ));
                    }
                }
                envelope.reply.send(response);
                task
            }
            #[cfg(target_arch = "wasm32")]
            Message::PollWebControl => {
                let control = if let Some(request) = super::super::super::control::web_request() {
                    self.update(Message::ControlRequest(request))
                } else {
                    Task::none()
                };
                // Dragged-in drawings ride the same poll (#344): start one
                // only while the open slot is free — the same guard as a
                // desktop drop — and leave the rest queued, so a multi-file
                // drop opens sequentially without its own subscription.
                let dropped =
                    if self.opening.is_none() && !self.dialog_open(super::super::super::ModalKind::Recovery) {
                        crate::io::web_drop::take_dropped()
                            .map(|file| {
                                self.update(Message::WebFileDropped(
                                    file.name,
                                    file.bytes,
                                    file.restore_key,
                                    file.cache_recent,
                                ))
                            })
                            .unwrap_or_else(Task::none)
                    } else {
                        Task::none()
                    };
                // Dropped raster images register their bytes (name-keyed) and
                // re-resolve any RasterImage the open drawings reference but
                // couldn't load — the web stand-in for the drawing folder the
                // images live beside natively.
                let mut registered: Vec<String> = Vec::new();
                while let Some((name, bytes)) = crate::io::web_drop::take_dropped_image() {
                    crate::scene::model::image_model::register_source(
                        &name,
                        std::sync::Arc::new(bytes),
                    );
                    registered.push(name);
                }
                // Remote http(s) raster refs the background fetch settled:
                // successes already registered their bytes under the URL (the
                // fetch task does it), failures surface on the command line —
                // a network/CORS block, an HTTP status, the size cap, or the
                // deadline reads differently to the user.
                use crate::scene::model::image_model::{RemoteFetchError, RemoteFetchOutcome};
                let mut fetched: Vec<String> = Vec::new();
                while let Some(outcome) =
                    crate::scene::model::image_model::take_remote_fetch_outcome()
                {
                    match outcome {
                        RemoteFetchOutcome::Loaded(url) => fetched.push(url),
                        RemoteFetchOutcome::Failed(url, reason) => {
                            let note = match reason {
                                RemoteFetchError::Network => crate::tf!(
                                    "Remote image \"{url}\" could not be fetched: the network request failed (offline, DNS, or CORS)."
                                )
                                .into_owned(),
                                RemoteFetchError::Status(status) => crate::tf!(
                                    "Remote image \"{url}\" could not be fetched: the server returned HTTP {status}."
                                )
                                .into_owned(),
                                RemoteFetchError::TooLarge => crate::tf!(
                                    "Remote image \"{url}\" was not loaded: it exceeds the 64 MB remote-image limit."
                                )
                                .into_owned(),
                                RemoteFetchError::Timeout => crate::tf!(
                                    "Remote image \"{url}\" could not be fetched: the request timed out."
                                )
                                .into_owned(),
                            };
                            self.command_line.push_error(&note);
                        }
                    }
                }
                registered.extend(fetched);
                if crate::scene::model::image_model::take_overflowed() {
                    // A settled fetch's rebuild trigger was dropped at the
                    // outcome cap; do one unconditional catch-up pass over
                    // every registered name.
                    registered.extend(crate::scene::model::image_model::registered_image_names());
                }
                if !registered.is_empty() {
                    for tab in self.tabs.iter_mut() {
                        tab.scene.resolve_registered_image_names(&registered);
                    }
                }
                // Over-budget decode refusals from this poll's re-resolves
                // reach the command line; the affected images keep their
                // broken-reference placeholders.
                for note in crate::scene::model::image_model::take_decode_refusals() {
                    self.command_line.push_error(&note);
                }
                let embed_task = self.poll_embed_bridge();
                Task::batch([control, dropped, embed_task])
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::PollWebControl => Task::none(),
            Message::ControlStep(id, message) => self.control_step(id, *message),
            Message::ControlTaskDone(id) => {
                self.control_task_done(&id);
                Task::none()
            }
            Message::ControlScreenshot(path, screenshot) => {
                self.control_screenshot(path, screenshot);
                Task::none()
            }
            Message::ControlOffscreenCaptureDone(result) => {
                self.control_offscreen_capture_done(result);
                Task::none()
            }
            Message::ControlCaptureFrame { redrew } => self.on_control_capture_frame(redrew),
            Message::Graph(message) => self.on_graph(message),
            Message::GraphFileRead(read) => self.finish_graph_file_read(*read),
            Message::GraphLoadRead(tab_id, generation, read) => {
                self.finish_control_graph_load(tab_id, generation, read)
            }

            Message::ControlToggle => {
                self.control.enabled = !self.control.enabled;
                self.control.start_enabled = self.control.enabled;
                self.persist_settings_if_changed();
                Task::none()
            }
            Message::ControlCopySetup => {
                let (config, add_line) = mcp_client_setup();
                self.command_line
                    .push_info(crate::t!("MCP client setup copied to the clipboard").as_ref());
                for line in add_line.lines().chain(config.lines()) {
                    self.command_line.push_info(line);
                }
                crate::sys::clipboard_write(config)
            }
            // Drain plugin-to-host requests that arrived outside of a host call.
            // This runs on a periodic timer so long-lived plugin sessions such
            // as the Python REPL can mutate the document without requiring a
            // user-generated message. Desktop only.
            #[cfg(not(target_arch = "wasm32"))]
            Message::DrainPluginRequests => {
                for tab in 0..self.tabs.len() {
                    let mut host = crate::app::plugin_host::HostSession::new(self, tab);
                    crate::plugin::external::with_manager(|mgr| {
                        mgr.drain_requests(&mut host, &mut |_| {});
                    });
                }
                // API v13: process queued ChannelReady attach acks on the
                // main thread — the manager lock is free here (the reader
                // thread only enqueues), boot-window broadcasts flush.
                crate::plugin::v4_support::drain_channel_ready_acks();
                // Custom grips live exactly as long as their declaring
                // plugin: reap the sets of dead/unloaded plugins (and cancel
                // a drag or hover the dying plugin owned).
                self.reap_plugin_grips();
                crate::plugin::external::with_manager(|mgr| {
                    for line in mgr.drain_io() {
                        let prefixed =
                            format!("[{} {}] {}", line.plugin_id, line.source, line.text);
                        // Every line reaches the host's stderr (the gui.log),
                        // which a headless or automation session can read;
                        // the runner's own trace stays off the command line.
                        eprintln!("{prefixed}");
                        if line.text.starts_with("[runner]")
                            || line.text.starts_with("[plugin] ")
                            || line.text.starts_with("[python-repl]")
                        {
                            continue;
                        }
                        // A plugin's stderr is its ordinary trace, not an
                        // error; real failures come through push_error.
                        self.command_line.push_output(&prefixed);
                    }
                });
                Task::none()
            }

            // Web: fetch every script queued by startup language selection or
            // drawing text discovery. Each script has one shared store entry.
            Message::PollWebFonts => {
                #[cfg(target_arch = "wasm32")]
                let autosave =
                    if !crate::io::host_bridge::is_embed_mode() && web_autosave_due(self.savetime_min) {
                        self.on_autosave()
                    } else {
                        Task::none()
                    };
                #[cfg(not(target_arch = "wasm32"))]
                let autosave = Task::none();
                let pending = crate::scene::text::web_font::take_pending();
                if pending.is_empty() {
                    return autosave;
                }
                Task::batch(
                    std::iter::once(autosave).chain(pending.into_iter().map(|script| {
                        Task::perform(crate::scene::text::web_font::fetch(script), move |res| {
                            Message::WebFontLoaded(script, res)
                        })
                    })),
                )
            }

            // Web: a per-script font arrived. The same bytes feed drawing text,
            // the UI renderer, and the navigation-cube label atlas.
            Message::WebFontLoaded(script, res) => {
                match res {
                    Ok(bytes) => {
                        crate::scene::text::web_font::insert(script, Some(bytes));
                        crate::scene::text::ttf_glyph::clear_fallback_cache();
                        for tab in self.tabs.iter_mut() {
                            tab.scene.invalidate_text_geometry_dependencies();
                        }
                        return Task::done(Message::ApplyWebFont(script));
                    }
                    Err(e) => {
                        crate::scene::text::web_font::insert(script, None);
                        self.command_line
                            .push_error(crate::tf!("Font load failed ({script:?}): {e}").as_ref());
                    }
                }
                Task::none()
            }

            Message::ApplyWebFont(script) => {
                let Some(bytes) = crate::scene::text::web_font::loaded(script) else {
                    return Task::none();
                };
                iced::font::load((*bytes).clone()).map(move |result| {
                    Message::WebUiFontLoaded(script, result.map_err(|error| format!("{error:?}")))
                })
            }

            Message::WebUiFontLoaded(script, result) => {
                if let Err(error) = result {
                    self.command_line
                        .push_error(crate::tf!("Font load failed ({script:?}): {error}").as_ref());
                    return Task::none();
                }
                if script == crate::scene::text::web_font::primary_script() {
                    let family = iced::font::Family::name(script.family());
                    return iced::font::set_defaults(iced::Font::with_family(family), 16.0);
                }
                Task::none()
            }

            #[cfg(not(target_arch = "wasm32"))]
            Message::UiFontSettingsChanged => Task::perform(
                async { crate::sys::ui_font::detect() },
                Message::UiFontDetected,
            ),
            #[cfg(target_arch = "wasm32")]
            Message::UiFontSettingsChanged => Task::none(),

            // Re-setting the defaults recreates the renderers and lays out
            // every window again with the remapped sans-serif family.
            #[cfg(not(target_arch = "wasm32"))]
            Message::UiFontDetected(system) => {
                if crate::sys::ui_font::apply(system.as_ref()) {
                    iced::font::set_defaults(iced::Font::DEFAULT, 16.0)
                } else {
                    Task::none()
                }
            }
            #[cfg(target_arch = "wasm32")]
            Message::UiFontDetected(_) => Task::none(),

            Message::Tick(t) => self.on_tick(t),

            Message::OpenFile => self.on_open_file(),

            Message::OpenPathPicked(None) => Task::none(),

            Message::OpenUrl(url) => crate::sys::open_url(&url, self.main_window),

            Message::StartSectionSelect(section) => {
                self.start_section = section;
                self.save_config();
                Task::none()
            }

            Message::ScrollLayoutTabs(dx) => iced::widget::operation::scroll_by(
                iced::advanced::widget::Id::new(crate::ui::statusbar::LAYOUT_TABS_SCROLL_ID),
                iced::widget::scrollable::AbsoluteOffset { x: dx, y: 0.0 },
            ),

            Message::OpenRecent(path) => {
                // Opened from the application menu, which closes.
                self.ribbon.close_app_menu();
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Recents are read from disk every save → the path may be
                    // stale. Skip silently if the file no longer exists; the
                    // entry stays in the list so the user can clean it up.
                    match std::fs::metadata(&path) {
                        Ok(m) => self.update(Message::OpenPathPicked(Some((path, m.len())))),
                        Err(_) => {
                            self.command_line.push_error(
                                crate::tf!("Recent file no longer exists: {}", path.display())
                                    .as_ref(),
                            );
                            Task::none()
                        }
                    }
                }

                #[cfg(target_arch = "wasm32")]
                {
                    if let Some(idx) = self.tab_showing(&path) {
                        return self.update(Message::TabSwitch(idx));
                    }
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string_lossy().into_owned());
                    // The start page stays clickable under the open overlay,
                    // so a recent can race an in-flight open and clobber it
                    // (its WebFileOpened then fails the id check and the
                    // bytes are lost) — refuse like on_open_file does.
                    if self.opening.is_some() {
                        self.command_line
                            .push_info(crate::t!("An open is already in progress.").as_ref());
                        return Task::none();
                    }
                    let state = std::sync::Arc::new(crate::io::OpenProgressState::new(
                        crate::app::OPEN_PHASE_READING,
                    ));
                    let open_id = self.next_open_id();
                    self.opening = Some(crate::app::OpenProgress {
                        id: open_id,
                        name,
                        source_path: Some(path.clone()),
                        size_bytes: 0,
                        state: state.clone(),
                        started: Instant::now(),
                        recovery_error: None,
                        recovery_read_stats: None,
                        recovery_bytes: None,
                    });
                    Task::perform(crate::io::open_recent_web(path, state), move |outcome| {
                        Message::WebFileOpened(open_id, outcome)
                    })
                }
            }

            Message::OpenExternal(path) => {
                // A second launch forwarded this drawing. Route it through
                // `OpenRecent` so the redirect and a cold start share one path:
                // it stats the file and reports a missing one visibly, instead
                // of a boot that appears to do nothing.
                //
                // Raising is best-effort and cannot be made reliable from here.
                // `gain_focus` reaches winit's `focus_window`, which on Wayland
                // has an empty body — it is `request_user_attention` that walks
                // the xdg-activation path, and it mints its token without a seat
                // serial, which a compositor may refuse to honour. So expect an
                // attention mark rather than a raise on Wayland; X11 does raise.
                // A real raise needs the activation token from the launching
                // process, and neither iced 0.14 nor winit 0.30 can apply one to
                // an existing window.
                let raise = match self.main_window {
                    Some(id) => Task::batch([
                        iced::window::gain_focus(id),
                        iced::window::request_user_attention(
                            id,
                            Some(iced::window::UserAttention::Critical),
                        ),
                    ]),
                    None => Task::none(),
                };
                if self.opening.is_some() || self.dialog_open(super::super::super::ModalKind::Recovery) {
                    self.pending_opens.push_back(path);
                    raise
                } else if let Some(idx) = self.tab_showing(&path) {
                    Task::batch([raise, self.update(Message::TabSwitch(idx))])
                } else {
                    Task::batch([raise, self.update(Message::OpenRecent(path))])
                }
            }

            Message::WebFieldPaste => {
                // The MText / inline-TEXT editors have their own web paste
                // paths — don't double-feed them.
                if self.mtext_editor.is_some() || self.text_inline.is_some() {
                    return self.on_paste_shortcut();
                }
                #[cfg(target_arch = "wasm32")]
                return Task::perform(
                    crate::sys::read_clipboard_text(),
                    Message::WebFieldPasteText,
                );
                #[cfg(not(target_arch = "wasm32"))]
                Task::none()
            }

            Message::WebFieldPasteText(text) => {
                #[cfg(target_arch = "wasm32")]
                if let Some(t) = &text {
                    crate::sys::synthesize_typing(t);
                }
                let _ = text;
                Task::none()
            }

            Message::WebFieldCopy => {
                // Walk the widget tree for the focused text input's visible
                // text. iced calls `text_input` then `focusable` back-to-back
                // on the same widget, so remembering the last text seen pairs
                // it with the focus check. (An empty field reports its
                // placeholder — that's iced's "visible text" contract.)
                use iced::advanced::widget::operation::{Focusable, Outcome, TextInput};
                use iced::advanced::widget::{Id, Operation};
                #[derive(Default)]
                struct FocusedText {
                    last_text: Option<String>,
                    found: Option<String>,
                }
                impl Operation<Option<String>> for FocusedText {
                    fn text_input(
                        &mut self,
                        _id: Option<&Id>,
                        _bounds: iced::Rectangle,
                        state: &mut dyn TextInput,
                    ) {
                        self.last_text = Some(state.text().to_owned());
                    }
                    fn focusable(
                        &mut self,
                        _id: Option<&Id>,
                        _bounds: iced::Rectangle,
                        state: &mut dyn Focusable,
                    ) {
                        let text = self.last_text.take();
                        if state.is_focused() && self.found.is_none() {
                            self.found = text;
                        }
                    }
                    fn traverse(
                        &mut self,
                        operate: &mut dyn FnMut(&mut dyn Operation<Option<String>>),
                    ) {
                        operate(self);
                    }
                    fn finish(&self) -> Outcome<Option<String>> {
                        Outcome::Some(self.found.clone())
                    }
                }
                iced::advanced::widget::operate(FocusedText::default())
                    .map(Message::WebFieldCopyText)
            }

            Message::WebFieldCopyText(text) => {
                #[cfg(target_arch = "wasm32")]
                if let Some(t) = &text {
                    if !t.is_empty() {
                        crate::sys::write_clipboard_text(t);
                    }
                }
                let _ = text;
                Task::none()
            }

            Message::SnapOverridePick(t) => {
                self.apply_snap_override(crate::snap::SnapOverrideKind::Mode(t));
                Task::none()
            }

            Message::SnapOverrideNone => {
                self.apply_snap_override(crate::snap::SnapOverrideKind::None);
                Task::none()
            }

            Message::SnapOverrideMtp | Message::SnapOverrideFrom => {
                let from = matches!(msg, Message::SnapOverrideFrom);
                self.snap_override_popup = None;
                self.tabs[self.active_tab]
                    .scene
                    .selection
                    .borrow_mut()
                    .menu
                    .close();
                // Same guard as typed MTP/M2P: point step, not entity pick.
                let i = self.active_tab;
                let allowed = self.tabs[i].active_cmd.as_ref().is_some_and(|c| {
                    (!c.input_kind().wants_text() || c.point_step_accepts_keywords())
                        && !c.needs_entity_pick()
                }) || (!from && self.grip_point_prompt(i));
                if allowed {
                    if from {
                        self.start_from_modifier(i);
                    } else {
                        self.start_mtp_modifier(i);
                    }
                } else {
                    self.command_line.push_info(
                        crate::t!(if from {
                            "FROM needs an active point prompt."
                        } else {
                            "MTP needs an active point prompt."
                        })
                        .as_ref(),
                    );
                }
                Task::none()
            }

            Message::SnapOverrideClose => {
                self.snap_override_popup = None;
                Task::none()
            }

            Message::FileDropped(path) => {
                // Desktop drag & drop (#344): accept the formats the Open
                // dialog accepts — a drop has no picker filter, so anything
                // else reports instead of failing silently in the parser.
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if ext == "scr" {
                    return self.dispatch_command(&format!("SCRIPT \"{}\"", path.display()));
                }
                if matches!(ext.as_str(), "svg" | "svgz") {
                    return self.dispatch_command(&format!("_SVGINFILE {}", path.display()));
                }
                // A picture is attached as IMAGEATTACH attaches it.
                if matches!(
                    ext.as_str(),
                    "png" | "jpg" | "jpeg" | "bmp" | "tif" | "tiff" | "webp" | "gif"
                ) {
                    return self.update(Message::ImagePickResult(crate::io::image_file_size(path)));
                }
                if !matches!(ext.as_str(), "dwg" | "dxf" | "bak" | "sv$")
                    && ocs_save::external_format(&ext).is_none()
                {
                    self.command_line.push_error(
                        crate::tf!("Unsupported file type: {}", path.display()).as_ref(),
                    );
                    return Task::none();
                }
                // A load or recovery report owns the open slot; queue another
                // drop until that state is acknowledged.
                if self.opening.is_some() || self.dialog_open(super::super::super::ModalKind::Recovery) {
                    self.pending_opens.push_back(path);
                    Task::none()
                } else if let Some(idx) = self.tab_showing(&path) {
                    self.update(Message::TabSwitch(idx))
                } else {
                    self.update(Message::OpenRecent(path))
                }
            }

            Message::RecentRemove(path) => {
                self.remove_recent(&path);
                Task::none()
            }

            Message::SetRecentLimit(limit) => {
                self.set_recent_limit(limit);
                // Resync the input box to the clamped, applied value.
                self.recent_limit_input = self.recent_limit.to_string();
                Task::none()
            }

            Message::SetRecentView(view) => {
                self.recent_view = view;
                self.save_config();
                Task::none()
            }

            Message::RecentLimitInput(s) => {
                // Keep only digits while typing; applied on Enter (SetRecentLimit).
                self.recent_limit_input = s.chars().filter(|c| c.is_ascii_digit()).collect();
                Task::none()
            }

            Message::OpenPathPicked(Some((path, size_bytes))) => {
                // SDI 1: one drawing at a time, so the current one closes first.
                let i = self.active_tab;
                let sdi_close = if self.sdi && !self.tabs[i].is_start {
                    if self.tabs[i].dirty {
                        self.command_line.push_error(
                            crate::tf!(
                                "SDI: save or close \"{}\" before opening another drawing.",
                                self.tabs[i].tab_title
                            )
                            .as_ref(),
                        );
                        return Task::none();
                    }
                    self.on_tab_close(i)
                } else {
                    Task::none()
                };
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".into());
                let progress = std::sync::Arc::new(crate::io::OpenProgressState::new(
                    super::super::super::OPEN_PHASE_READING,
                ));
                let open_id = self.next_open_id();
                self.opening = Some(super::super::super::OpenProgress {
                    id: open_id,
                    name: name.clone(),
                    source_path: Some(path.clone()),
                    size_bytes,
                    state: progress.clone(),
                    started: Instant::now(),
                    recovery_error: None,
                    recovery_read_stats: None,
                    #[cfg(target_arch = "wasm32")]
                    recovery_bytes: None,
                    #[cfg(not(target_arch = "wasm32"))]
                    fingerprint: crate::io::edit_lock::FileFingerprint::capture(&path).ok(),
                });
                let size_label = format_size(size_bytes);
                self.command_line
                    .push_info(crate::tf!("Opening \"{name}\" ({size_label})…").as_ref());
                let model_bg = self
                    .default_bg_color
                    .unwrap_or_else(|| self.model_space.resolve_model_bg(&self.active_theme));
                Task::batch([
                    sdi_close,
                    Task::perform(
                        crate::io::open_path_with_phase(path, progress, model_bg),
                        move |result| Message::FileOpened(open_id, result),
                    ),
                ])
            }

            Message::OpenCancel => {
                #[cfg(target_arch = "wasm32")]
                crate::io::cancel_active_parse_workers();
                #[cfg(target_arch = "wasm32")]
                let cancelled_id = self.opening.as_ref().map(|opening| opening.id);
                if let Some(p) = self.opening.take() {
                    self.command_line
                        .push_info(crate::tf!("Open cancelled: \"{}\"", p.name).as_ref());
                }
                // A cancelled recovered open keeps its snapshot (re-offered
                // next boot — it is the only copy); the remaining queued
                // snapshots can still be offered this session. Match on the
                // open's id so cancelling an unrelated open leaves a pending
                // restore marker alone.
                #[cfg(target_arch = "wasm32")]
                if cancelled_id.is_some_and(|id| {
                    self.web_autosave.consumed
                        .as_ref()
                        .is_some_and(|(_, recorded)| *recorded == id)
                }) && self.web_autosave.consumed.take().is_some()
                {
                    self.queue_next_web_autosave_prompt();
                }
                self.drain_pending_open()
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebFileDropped(name, bytes, restore_key, cache_recent) => {
                // Bytes are already in hand (no picker round-trip), so this is
                // `on_open_file`'s web branch with the real name and size on
                // the overlay from the start. An open already in flight must
                // not be clobbered (its bytes are gone once dequeued), so the
                // entry goes back to the queue for the next poll tick.
                if self.opening.is_some() {
                    // A full queue rejects the bytes here; io-side console
                    // warning only (the drop path has no user-facing
                    // surface for rejections by design — see QUEUE_CAP).
                    let _ = match restore_key {
                        Some(key) => crate::io::web_drop::enqueue_restore(name, bytes, key),
                        None if cache_recent => crate::io::web_drop::enqueue_open(name, bytes),
                        None => crate::io::web_drop::enqueue_open_uncached(name, bytes),
                    };
                    return Task::none();
                }
                let state = std::sync::Arc::new(crate::io::OpenProgressState::new(
                    crate::app::OPEN_PHASE_READING,
                ));
                let open_id = self.next_open_id();
                // Identity, not display name: only this exact open may delete
                // the snapshot the restore came from.
                if let Some(key) = restore_key {
                    self.web_autosave.consumed = Some((key, open_id));
                }
                self.opening = Some(crate::app::OpenProgress {
                    id: open_id,
                    name: name.clone(),
                    source_path: Some(std::path::PathBuf::from(&name)),
                    size_bytes: bytes.len() as u64,
                    state: state.clone(),
                    started: Instant::now(),
                    recovery_error: None,
                    recovery_read_stats: None,
                    recovery_bytes: None,
                });
                Task::perform(
                    crate::io::web_drop::open_dropped(name, bytes, state, cache_recent),
                    move |outcome| Message::WebFileOpened(open_id, outcome),
                )
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebFileOpened(open_id, mut outcome) => {
                if self.opening.as_ref().map(|opening| opening.id) != Some(open_id) {
                    return Task::none();
                }
                // A crash-recovery open settling here: success deletes its
                // OPFS snapshot (the only copy, kept until now); a parse
                // failure keeps it for the next boot. Either way the next
                // queued snapshot can be offered again.
                if self
                    .web_autosave.consumed
                    .as_ref()
                    .is_some_and(|(_, recorded)| *recorded == open_id)
                {
                    if outcome.result.is_ok() {
                        self.web_autosave_opened(&outcome.name);
                    } else {
                        self.web_autosave.consumed = None;
                        self.queue_next_web_autosave_prompt();
                    }
                }
                // A browser tab has a few GB of memory at most; a very large
                // drawing that fails here usually opens in the desktop app.
                const WEB_LARGE_DRAWING: u64 = 150 * 1024 * 1024;
                if outcome.result.is_err() && outcome.size_bytes > WEB_LARGE_DRAWING {
                    self.command_line.push_info(
                        crate::tf!(
                            "This drawing is {} MB, more than the browser version handles reliably. Open it in the desktop app.",
                            outcome.size_bytes / (1024 * 1024)
                        )
                        .as_ref(),
                    );
                }
                if let Some(opening) = self.opening.as_mut() {
                    opening.name = outcome.name.clone();
                    opening.source_path = Some(std::path::PathBuf::from(&outcome.name));
                    if outcome.size_bytes > 0 || opening.size_bytes == 0 {
                        opening.size_bytes = outcome.size_bytes;
                    }
                    opening.recovery_bytes = outcome.recovery_bytes.take();
                }
                if let Some(bytes) = outcome
                    .cache_bytes
                    .take()
                    .filter(|_| !crate::io::host_bridge::is_embed_mode())
                {
                    let name = outcome.name.clone();
                    return Task::perform(
                        async move {
                            let result =
                                crate::io::web_recent::store_open(&name, bytes, open_id).await;
                            (outcome, result)
                        },
                        move |(outcome, result)| Message::WebFileCached(open_id, outcome, result),
                    );
                }
                let recent_task = if outcome.record_recent && outcome.result.is_ok() {
                    self.push_recent(std::path::PathBuf::from(&outcome.name))
                } else {
                    Task::none()
                };
                let opened_task = self.update(Message::FileOpened(open_id, outcome.result));
                Task::batch([recent_task, opened_task])
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebFileCached(open_id, outcome, cache_result) => {
                if self.opening.as_ref().map(|opening| opening.id) != Some(open_id) {
                    return Task::none();
                }
                let recent_task = match cache_result {
                    Ok(()) => self.push_recent(std::path::PathBuf::from(&outcome.name)),
                    Err(error) => {
                        self.command_line.push_error(
                            crate::tf!(
                                "Opened drawing, but recent copy could not be stored: {error}"
                            )
                            .as_ref(),
                        );
                        Task::none()
                    }
                };
                let opened_task = self.update(Message::FileOpened(open_id, outcome.result));
                Task::batch([recent_task, opened_task])
            }

            Message::FileOpened(open_id, Ok((name, path, doc, caches, ifccad_session))) => {
                if self.opening.as_ref().map(|opening| opening.id) != Some(open_id) {
                    return Task::none();
                }
                #[cfg(target_arch = "wasm32")]
                let loaded_name = name.clone();
                let task = self.on_file_opened(name, path, doc, caches, ifccad_session);
                #[cfg(target_arch = "wasm32")]
                if crate::io::host_bridge::is_embed_mode() {
                    crate::io::host_bridge::emit_loaded(&loaded_name);
                    let dirty = self.tabs.get(self.active_tab).is_some_and(|tab| tab.dirty);
                    crate::io::host_bridge::emit_dirty(dirty);
                }
                // DXEVAL: an open compares the drawing's extraction tables.
                let task = match self.evaluate_extraction_tables(
                    self.active_tab,
                    crate::app::commands::extraction_eval::EVENT_OPEN,
                    None,
                ) {
                    Some(prompt) => Task::batch([task, prompt]),
                    None => task,
                };
                // Web: raster files dropped while this open was in flight
                // registered after its caches were built — re-resolve any
                // image the (possibly new) drawing references by name.
                #[cfg(target_arch = "wasm32")]
                {
                    let names = crate::scene::model::image_model::registered_image_names();
                    if !names.is_empty() {
                        for tab in self.tabs.iter_mut() {
                            tab.scene.resolve_registered_image_names(&names);
                        }
                    }
                }
                // Images in (or dropped alongside) the opened drawing that
                // exceeded the decode budget refused with a note; surface
                // them now that the open has settled — the entities
                // themselves show the broken-reference placeholder.
                for note in crate::scene::model::image_model::take_decode_refusals() {
                    self.command_line.push_error(&note);
                }
                task
            }

            Message::FileOpened(open_id, Err(e)) => {
                if self.opening.as_ref().map(|opening| opening.id) != Some(open_id) {
                    return Task::none();
                }
                #[cfg(target_arch = "wasm32")]
                if crate::io::host_bridge::is_embed_mode() {
                    crate::io::host_bridge::emit_error("open_failed", &e.message);
                }
                // An automation open has nobody to answer the recovery
                // prompt, so it takes the recovery read straight away, as a
                // headless read does.
                let controlled = crate::app::control::controlled_open_pending(self);
                if e.recovery_available {
                    if let Some(opening) = self.opening.as_mut() {
                        opening.recovery_error = Some(e.message);
                        opening.recovery_read_stats = e.read_stats;
                        if controlled {
                            return self.update(Message::RecoveryAttempt);
                        }
                        self.active_modal = Some(super::super::super::ModalKind::RecoveryPrompt);
                        return Task::none();
                    }
                }
                // If the user cancelled, the overlay was already cleared and
                // we suppress the noise.
                let opening = self.opening.take();
                if let Some(opening) = opening.filter(|_| e.message != "Cancelled") {
                    self.command_line
                        .push_error(crate::tf!("Open failed: {e}").as_ref());
                    let total_ms = opening.started.elapsed().as_millis() as u32;
                    let failure_phase = crate::io::open_phase_name(
                        opening
                            .state
                            .phase
                            .load(std::sync::atomic::Ordering::Acquire),
                    )
                    .to_string();
                    let mut report = crate::io::recovery::RecoveryReport::failed(
                        opening.source_path,
                        opening.name,
                        opening.size_bytes,
                        e.source_sha256,
                        e.read_stats,
                        failure_phase,
                        e.message,
                        total_ms,
                    );
                    report.persist();
                    self.recovery_report = Some(report);
                    // The report modal holds the open queue; an automation
                    // client already has the error and must be able to open
                    // the next drawing.
                    if !controlled {
                        self.active_modal = Some(super::super::super::ModalKind::Recovery);
                        return Task::none();
                    }
                }
                // A drawing that fails to parse must not strand the ones queued
                // behind it.
                self.drain_pending_open()
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebRecentStored(result) => match result {
                Ok(path) => self.push_recent(path),
                Err(error) => {
                    self.command_line.push_error(
                        crate::tf!("Saved download, but recent copy could not be stored: {error}")
                            .as_ref(),
                    );
                    Task::none()
                }
            },

            // The `?file=` boot deep link settled. Queued bytes are drained by
            // the control poll's drop queue like any drop — nothing to say
            // until that open starts (or its parse error surfaces there).
            #[cfg(target_arch = "wasm32")]
            Message::WebLinkSettled(outcome) => {
                use crate::io::web_link::LinkOutcome;
                match outcome {
                    LinkOutcome::Queued => {}
                    LinkOutcome::Info(text) => self.command_line.push_info(&text),
                    LinkOutcome::Error(text) => self.command_line.push_error(&text),
                }
                Task::none()
            }

            #[cfg(target_arch = "wasm32")]
            Message::WebAutosavesScanned(result) => self.on_web_autosaves_scanned(result),
            #[cfg(target_arch = "wasm32")]
            Message::WebAutosaveRestore => self.on_web_autosave_restore(),
            #[cfg(target_arch = "wasm32")]
            Message::WebAutosaveRestoreAll => self.on_web_autosave_restore_all(),
            #[cfg(target_arch = "wasm32")]
            Message::WebAutosaveDiscard => self.on_web_autosave_discard(),
            #[cfg(target_arch = "wasm32")]
            Message::WebAutosaveDefer => self.on_web_autosave_defer(),
            #[cfg(target_arch = "wasm32")]
            Message::WebAutosaveBytes(name, key, result) => {
                self.on_web_autosave_bytes(name, key, result)
            }
            #[cfg(target_arch = "wasm32")]
            Message::WebAutosaveSnapshotFinished(result) => self.on_web_autosave_finished(result),

            Message::ImagePick => {
                Task::perform(crate::io::pick_image_file(), Message::ImagePickResult)
            }

            Message::ImagePickResult(Ok((path, pw, ph))) => {
                use crate::command::CadCommand;
                use crate::modules::draw::draw::raster_image::ImageCommand;
                let path_str = path.to_string_lossy().into_owned();
                let short = std::path::Path::new(&path_str)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&path_str)
                    .to_string();
                self.command_line
                    .push_output(crate::tf!("IMAGE  \"{short}\": {pw}×{ph} px").as_ref());
                let cmd = ImageCommand::new(path_str, pw, ph);
                let i = self.active_tab;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
                Task::none()
            }

            Message::ImagePickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line
                        .push_error(crate::tf!("IMAGE: {e}").as_ref());
                }
                Task::none()
            }

            Message::ImageEmbedPick => Task::perform(
                crate::io::pick_embedded_image_file(),
                Message::ImageEmbedPickResult,
            ),

            Message::ImageEmbedPickResult(Ok(image)) => {
                use crate::command::CadCommand;
                use crate::modules::draw::draw::raster_image::ImageCommand;
                self.command_line.push_output(
                    crate::tf!(
                        "IMAGEEMBED  \"{name}\": {w}×{h} px (embedded)",
                        name = image.name.as_str(),
                        w = image.pixel_width,
                        h = image.pixel_height,
                    )
                    .as_ref(),
                );
                let cmd = ImageCommand::new_embedded(image);
                let i = self.active_tab;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
                Task::none()
            }

            Message::InsertObjectResult(Ok(image)) => {
                use crate::command::CadCommand;
                let cmd =
                    crate::modules::draw::draw::raster_image::ImageCommand::new_embedded(image);
                let i = self.active_tab;
                self.command_line.push_info(&cmd.prompt());
                self.tabs[i].active_cmd = Some(Box::new(cmd));
                Task::none()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Message::OleEditPoll => self.poll_ole_edits(),
            #[cfg(not(target_arch = "wasm32"))]
            Message::OleEditRendered(tab, handle, result) => {
                self.on_ole_edit_rendered(tab, handle, result)
            }
            #[cfg(target_arch = "wasm32")]
            Message::OleEditPoll | Message::OleEditRendered(..) => Task::none(),
            Message::InsertObjectResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(&format!("INSERTOBJ: {e}"));
                }
                Task::none()
            }
            Message::ImageEmbedPickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line
                        .push_error(crate::tf!("IMAGEEMBED: {e}").as_ref());
                }
                Task::none()
            }

            Message::MissingFontsSourceChanged(url) => {
                self.font_source_input = url;
                Task::none()
            }
            Message::MissingFontsDownload => {
                if self.missing_fonts_downloading {
                    return Task::none();
                }
                // Remember the source across sessions — and across drawings.
                let source = self.font_source_input.trim().to_string();
                if self.font_source_url != source {
                    self.font_source_url = source.clone();
                    self.save_config();
                }
                let fonts = self.missing_fonts.clone().unwrap_or_default();
                if fonts.is_empty() {
                    return Task::none();
                }
                self.missing_fonts_downloading = true;
                Task::perform(
                    async move {
                        let source = crate::io::font_repo::FontSource::from_url(&source);
                        crate::io::font_repo::download_fonts(&fonts, &source)
                    },
                    Message::MissingFontsResult,
                )
            }
            Message::MissingFontsDismiss => {
                if self.missing_fonts_downloading {
                    return Task::none();
                }
                for name in self.missing_fonts.take().unwrap_or_default() {
                    self.suppressed_missing_fonts
                        .insert(crate::io::font_repo::font_key(&name));
                }
                self.missing_fonts_path = None;
                self.close_active_modal();
                // Later drawings list their missing fonts on the command line.
                if std::mem::take(&mut self.missing_fonts_silence) {
                    self.check_missing_fonts = false;
                    self.persist_settings_if_changed();
                }
                Task::none()
            }
            Message::MissingFontsSilence(on) => {
                self.missing_fonts_silence = on;
                Task::none()
            }
            Message::MissingFontsResult(result) => {
                self.missing_fonts_downloading = false;
                match result {
                    Ok(pairs) if pairs.is_empty() => {
                        let unavailable = self.missing_fonts.take().unwrap_or_default();
                        for name in &unavailable {
                            self.suppressed_missing_fonts
                                .insert(crate::io::font_repo::font_key(name));
                        }
                        self.missing_fonts_path = None;
                        self.close_active_modal();
                        self.command_line.push_error(crate::t!(
                            "None of the requested fonts are available from the selected source. Substitute fonts remain active; this notice will not repeat during this session."
                        ).as_ref());
                    }
                    Ok(pairs) => {
                        let requested = self.missing_fonts.take().unwrap_or_default();
                        let downloaded: rustc_hash::FxHashSet<String> = pairs
                            .iter()
                            .map(|(name, _)| crate::io::font_repo::font_key(name))
                            .collect();
                        let unavailable: Vec<String> = requested
                            .into_iter()
                            .filter(|name| {
                                !downloaded.contains(&crate::io::font_repo::font_key(name))
                            })
                            .collect();
                        for name in &unavailable {
                            self.suppressed_missing_fonts
                                .insert(crate::io::font_repo::font_key(name));
                        }
                        for (name, path) in &pairs {
                            self.command_line.push_output(
                                crate::tf!(
                                    "FONT  Downloaded {name} → {path}",
                                    path = path.display()
                                )
                                .as_ref(),
                            );
                        }
                        if !unavailable.is_empty() {
                            self.command_line.push_info(crate::tf!(
                                "FONT  Not available from the selected source: {fonts}. Substitute fonts remain active.",
                                fonts = unavailable.join(", ")
                            ).as_ref());
                        }
                        // The downloaded files change glyph resolution for the
                        // whole drawing — reload it through the standard open
                        // pipeline so every wire is rebuilt with the real fonts.
                        let path = self.missing_fonts_path.take();
                        self.close_active_modal();
                        if let Some(path) = path {
                            return Task::done(Message::OpenExternal(path));
                        }
                        self.command_line.push_info(
                            crate::t!("Save and reopen the drawing to apply the new fonts.")
                                .as_ref(),
                        );
                    }
                    Err(e) => {
                        // Keep the prompt open: the user can correct a custom
                        // source or retry a transient network failure.
                        self.command_line
                            .push_error(crate::tf!("Font download failed: {e}").as_ref());
                    }
                }
                Task::none()
            }
            Message::PdfAttachPick => Task::perform(
                async {
                    let handle = crate::sys::file_dialog()
                        .set_title(crate::t!("Select PDF Underlay").as_ref())
                        .add_filter(crate::t!("PDF Files").as_ref(), &["pdf", "PDF"])
                        .pick_file()
                        .await;

                    match handle {
                        Some(h) => {
                            let path = crate::sys::handle_path(&h);
                            let bytes = std::sync::Arc::new(h.read().await);
                            Ok((path, bytes))
                        }
                        None => Err("Cancelled".to_string()),
                    }
                },
                Message::PdfAttachPickResult,
            ),

            Message::PdfAttachPickResult(Ok((path, bytes))) => {
                let path_str = path.to_string_lossy().into_owned();
                crate::scene::model::pdf_raster::register_source(&path_str, bytes);
                // Pages, placement and path type are chosen in the Attach
                // dialog; the definition is created with the underlay.
                self.open_pdf_attach_dialog(&path_str)
            }

            Message::PdfImportPick => Task::perform(
                async {
                    // A file named by automation comes back without the picker.
                    if let Some(path) = crate::sys::take_pdf_import_pick() {
                        return std::fs::read(&path)
                            .map(|bytes| (path, std::sync::Arc::new(bytes)))
                            .map_err(|e| e.to_string());
                    }
                    let handle = crate::sys::file_dialog()
                        .set_title(crate::t!("Select PDF File").as_ref())
                        .add_filter(crate::t!("PDF Files").as_ref(), &["pdf", "PDF"])
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => {
                            let path = crate::sys::handle_path(&h);
                            let bytes = std::sync::Arc::new(h.read().await);
                            Ok((path, bytes))
                        }
                        None => Err("Cancelled".to_string()),
                    }
                },
                Message::PdfImportPickResult,
            ),
            Message::PdfImportPickResult(Ok((path, bytes))) => {
                let path_str = path.to_string_lossy().into_owned();
                crate::scene::model::pdf_raster::register_source(&path_str, bytes);
                self.open_pdf_import_file(&path_str)
            }
            Message::PdfImportPickResult(Err(_)) => Task::none(),
            Message::PdfAttachPickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line
                        .push_error(crate::tf!("PDFATTACH: {e}").as_ref());
                }

                Task::none()
            }
            Message::XAttachPick => {
                // Web: the browser hands back bytes, not a path — read them,
                // register them name-keyed (so reloads and re-opens of the
                // host drawing re-resolve the xref in-session) and pre-parse
                // on the worker in one step. The returned name-only PathBuf
                // is what the native branch's `handle_path` yields there.
                #[cfg(target_arch = "wasm32")]
                {
                    Task::perform(
                        crate::io::pick_and_register_xref_web(),
                        Message::XAttachPickResult,
                    )
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(
                        async {
                            let handle = crate::sys::file_dialog()
                                .set_title(crate::t!("Select External Reference File").as_ref())
                                .add_filter(
                                    crate::tr!("ifc2plan", "reference-files"),
                                    &["dwg", "dxf", "bak", "ifc", "DWG", "DXF", "BAK", "IFC"],
                                )
                                .add_filter(crate::t!("DWG Files").as_ref(), &["dwg", "DWG"])
                                .add_filter(crate::t!("DXF Files").as_ref(), &["dxf", "DXF"])
                                .add_filter("IFC (*.ifc)", &["ifc", "IFC"])
                                .add_filter(crate::t!("Backup Files").as_ref(), &["bak", "BAK"])
                                .pick_file()
                                .await;
                            match handle {
                                Some(h) => Ok(crate::sys::handle_path(&h)),
                                None => Err("Cancelled".to_string()),
                            }
                        },
                        Message::XAttachPickResult,
                    )
                }
            }

            Message::XAttachPickResult(Ok(path)) => self.open_xref_attach_dialog(path),
            message @ (Message::AttachPick
            | Message::UnderlayAttachPick(_)
            | Message::PointCloudAttachPick
            | Message::AttachPickResult(_)
            | Message::XrefAttach(_)
            | Message::XrefAttachBrowseResult(_)) => self.update_xref_attach(message),

            Message::XAttachPickResult(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line
                        .push_error(crate::tf!("XATTACH: {e}").as_ref());
                }
                Task::none()
            }

            Message::WblockSave(block_name) => {
                if self.read_only {
                    self.command_line.push_error(
                        crate::t!("Read-only session (--read-only): saving is disabled.").as_ref(),
                    );
                    return Task::none();
                }
                let name = block_name.clone();
                let title = if block_name == "-" {
                    crate::t!("Create Drawing File").into_owned()
                } else {
                    crate::t!("Save Block As").into_owned()
                };
                Task::perform(
                    async move {
                        let path = crate::sys::file_dialog()
                            .set_title(&title)
                            .set_file_name("block.dwg")
                            .add_filter(crate::t!("DWG Files").as_ref(), &["dwg"])
                            .add_filter(crate::t!("DXF Files").as_ref(), &["dxf"])
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h));
                        (name, path)
                    },
                    |(name, path)| Message::WblockSaveResult(name, path),
                )
            }

            // -WBLOCK with FILEDIA 1: the file came from the dialog, the
            // command line asks for the rest.
            Message::WblockSaveResult(block_name, Some(path)) if block_name == "-" => {
                self.dispatch_command(&format!("_-WBLOCKFILE {}", path.display()))
            }
            Message::WblockSaveResult(block_name, Some(path)) => {
                self.on_wblock_save_result_some(block_name, path)
            }

            Message::WblockSaveResult(_, None) => Task::none(),

            Message::WblockWriteFinished(block_name, path, result) => {
                // A written file is not reported (as in the reference); a
                // failed write is.
                let _ = (&block_name, &path);
                if let Err(error) = result {
                    self.command_line
                        .push_error(crate::tf!("WBLOCK save failed: {error}").as_ref());
                }
                Task::none()
            }

            // Web WBLOCK completion: the download is the only artifact of
            // the save — fire it even if the source tab was closed while
            // serialization ran — then reuse the WblockWriteFinished
            // bookkeeping for the command-line report.
            #[cfg(target_arch = "wasm32")]
            Message::WebWblockSerialized {
                block_name,
                path,
                filename,
                result,
            } => {
                if let Ok(bytes) = &result {
                    crate::sys::download_bytes(&filename, bytes);
                }
                self.update(Message::WblockWriteFinished(
                    block_name,
                    path,
                    result.map(|_| ()),
                ))
            }

            Message::TableInsertStyle(value) => self.on_table_insert_style(value),
            Message::TableInsertManageStyles => {
                let task = self.update(Message::TableStyleDialogOpen);
                self.tablestyle_parent_table = true;
                task
            }
            Message::TableInsertField(field) => self.on_table_insert_field(field),
            Message::TableInsertApply => self.on_table_insert_apply(),
            Message::DataLinkManagerOpen => {
                let from_table = self.active_modal == Some(super::super::super::ModalKind::InsertTable);
                self.open_data_link_manager(from_table);
                Task::none()
            }
            Message::DataLinkNew => self.on_data_link_new(),
            Message::DataLinkSelect(handle) => self.on_data_link_select(handle),
            Message::DataLinkEdit => self.on_data_link_edit(),
            Message::DataLinkEditCancel => {
                self.data_link_manager.editing = false;
                self.data_link_manager.editing_handle = None;
                self.data_link_manager.status.clear();
                Task::none()
            }
            Message::DataLinkField(field) => self.on_data_link_field(field),
            Message::DataLinkBrowse => self.on_data_link_browse(),
            Message::DataLinkBrowseResult(path) => self.on_data_link_browse_result(path),
            Message::DataLinkSave => self.on_data_link_save(),
            Message::DataLinkDelete => self.on_data_link_delete(),
            Message::DataLinkInsert => self.on_data_link_insert(),
            Message::DataLinkClose => self.on_data_link_close(),
            Message::DataExtractionOpen => {
                self.open_data_extraction();
                Task::none()
            }
            Message::DataExtractionField(field) => self.on_data_extraction_field(field),
            Message::DataExtractionBack => self.on_data_extraction_back(),
            Message::DataExtractionNext => self.on_data_extraction_next(),
            Message::DataExtractionBrowseSettings => self.on_data_extraction_browse_settings(),
            Message::DataExtractionBrowseSettingsResult(path) => {
                self.on_data_extraction_browse_settings_result(path)
            }
            Message::DataExtractionAddDrawings => self.on_data_extraction_add_drawings(),
            Message::DataExtractionAddDrawingsResult(Ok(paths)) => {
                self.on_data_extraction_add_drawings_result(paths)
            }
            Message::DataExtractionAddDrawingsResult(Err(error)) => {
                self.data_extraction.error = error;
                Task::none()
            }
            Message::DataExtractionAddFolder => self.on_data_extraction_add_folder(),
            Message::DataExtractionAddFolderResult(path) => {
                self.on_data_extraction_add_folder_result(path)
            }
            Message::DataExtractionClearSources => {
                self.data_extraction.source_files.clear();
                Task::none()
            }
            Message::DataExtractionBrowseOutput => self.on_data_extraction_browse_output(),
            Message::DataExtractionBrowseOutputResult(path) => {
                self.on_data_extraction_browse_output_result(path)
            }
            Message::DataExtractionFinish => self.on_data_extraction_finish(),
            Message::DataExtractionSaveDrawing => self.on_data_extraction_save_drawing(),
            Message::DataExtractionSaveDrawingResult(path) => {
                self.on_data_extraction_save_drawing_result(path)
            }

            Message::DataExtractionSave(csv) => {
                let csv_clone = csv.clone();
                Task::perform(
                    async move {
                        let path = crate::sys::file_dialog()
                            .set_title(crate::t!("Save Data Extraction").as_ref())
                            .set_file_name("extraction.csv")
                            .add_filter(crate::t!("CSV").as_ref(), &["csv"])
                            .add_filter(crate::t!("All Files").as_ref(), &["*"])
                            .save_file()
                            .await
                            .map(|h| crate::sys::handle_path(&h));
                        (csv_clone, path)
                    },
                    |(csv, path)| Message::DataExtractionSaveResult(csv, path),
                )
            }

            Message::DataExtractionSaveResult(csv, Some(path)) => {
                let fname = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned());
                // The browser has no writable path behind the picked name:
                // the CSV leaves as a download instead of an fs::write.
                #[cfg(target_arch = "wasm32")]
                let write_result: Result<(), String> = {
                    crate::sys::download_bytes(&fname, csv.as_bytes());
                    Ok(())
                };
                #[cfg(not(target_arch = "wasm32"))]
                let write_result = std::fs::write(&path, csv.as_bytes()).map_err(|e| e.to_string());
                match write_result {
                    Ok(()) => {
                        let rows = csv.lines().count().saturating_sub(1);
                        self.command_line.push_output(
                            crate::tf!("DATAEXTRACTION  {rows} rows → \"{fname}\"").as_ref(),
                        );
                    }
                    Err(e) => self
                        .command_line
                        .push_error(crate::tf!("DATAEXTRACTION: write failed: {e}").as_ref()),
                }
                Task::none()
            }

            Message::DataExtractionSaveResult(_, None) => Task::none(),

            Message::AttOutSave(text, Some(path)) => {
                self.on_attout_save(text, path);
                Task::none()
            }
            Message::AlignmentPointFilePicked(None) => Task::none(),
            Message::AlignmentPointFilePicked(Some(text)) => {
                use crate::command::CadCommand;
                let i = self.active_tab;
                let c = crate::modules::civil::point_alignment::AlignmentPointCommand::import(text);
                if let Some(menu) = c.intro() {
                    self.command_line.push_output(&menu);
                }
                self.command_line.push_info(&c.prompt());
                self.tabs[i].active_cmd = Some(Box::new(c));
                Task::none()
            }
            Message::AttOutSave(_, None) | Message::AttInPicked(None) => Task::none(),
            Message::TableCellsPaste(text) => {
                if let Some(text) = text {
                    self.paste_into_table_cells(&text);
                }
                Task::none()
            }
            Message::AttInPicked(Some(text)) => {
                self.on_attin_text(text);
                Task::none()
            }

            Message::StlExport => {
                let i = self.active_tab;
                if self.tabs[i].scene.meshes.is_empty() {
                    self.command_line
                        .push_error(crate::t!("STLOUT: no 3D mesh data in this drawing.").as_ref());
                    return Task::none();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(
                        async {
                            crate::sys::file_dialog()
                                .set_title(crate::t!("Export STL").as_ref())
                                .set_file_name("export.stl")
                                .add_filter(crate::t!("STL Files").as_ref(), &["stl"])
                                .add_filter(crate::t!("All Files").as_ref(), &["*"])
                                .save_file()
                                .await
                                .map(|h| crate::sys::handle_path(&h))
                        },
                        Message::StlExportPath,
                    )
                }
                // Web: rfd's save dialog shows no UI, so skip straight to a
                // virtual path — the browser download supplies the file name.
                #[cfg(target_arch = "wasm32")]
                {
                    let stem = self.tabs[i]
                        .current_path
                        .as_deref()
                        .and_then(|p: &std::path::Path| p.file_stem())
                        .map(|s: &std::ffi::OsStr| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "drawing".into());
                    Task::perform(
                        crate::io::stl::pick_stl_path_owned(stem),
                        Message::StlExportPath,
                    )
                }
            }

            Message::StlExportPath(Some(path)) => self.on_stl_export_path_some(path),

            Message::StlExportPath(None) => Task::none(),

            Message::StlExportFinished(path, result) => {
                match result {
                    Ok(()) => self.command_line.push_output(
                        crate::tf!("STLOUT: exported to \"{}\"", path.display()).as_ref(),
                    ),
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("STLOUT: {error}").as_ref()),
                }
                Task::none()
            }

            // ── STEP AP214 export ─────────────────────────────────────────
            Message::StepExport => {
                let i = self.active_tab;
                if self.tabs[i].scene.meshes.is_empty() {
                    self.command_line.push_error(
                        crate::t!("STEPOUT: no 3D mesh data in this drawing.").as_ref(),
                    );
                    return Task::none();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(
                        async {
                            crate::sys::file_dialog()
                                .set_title(crate::t!("Export STEP AP214").as_ref())
                                .set_file_name("export.step")
                                .add_filter(crate::t!("STEP Files").as_ref(), &["step", "stp"])
                                .add_filter(crate::t!("All Files").as_ref(), &["*"])
                                .save_file()
                                .await
                                .map(|h| crate::sys::handle_path(&h))
                        },
                        Message::StepExportPath,
                    )
                }
                // Web: rfd's save dialog shows no UI, so skip straight to a
                // virtual path — the browser download supplies the file name.
                #[cfg(target_arch = "wasm32")]
                {
                    let stem = self.tabs[i]
                        .current_path
                        .as_deref()
                        .and_then(|p: &std::path::Path| p.file_stem())
                        .map(|s: &std::ffi::OsStr| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "drawing".into());
                    Task::perform(
                        crate::io::step::pick_step_path_owned(stem),
                        Message::StepExportPath,
                    )
                }
            }

            // ── FBX export ────────────────────────────────────────────────
            Message::FbxExport => {
                let i = self.active_tab;
                if self.tabs[i].scene.meshes.is_empty() {
                    self.command_line.push_error(
                        crate::t!("FBXEXPORT: no 3D mesh data in this drawing.").as_ref(),
                    );
                    return Task::none();
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    Task::perform(
                        async {
                            crate::sys::file_dialog()
                                .set_title(crate::t!("Export FBX").as_ref())
                                .set_file_name("export.fbx")
                                .add_filter(crate::t!("FBX Files").as_ref(), &["fbx"])
                                .add_filter(crate::t!("All Files").as_ref(), &["*"])
                                .save_file()
                                .await
                                .map(|h| crate::sys::handle_path(&h))
                        },
                        Message::FbxExportPath,
                    )
                }
                #[cfg(target_arch = "wasm32")]
                {
                    let stem = self.tabs[i]
                        .current_path
                        .as_deref()
                        .and_then(|p: &std::path::Path| p.file_stem())
                        .map(|s: &std::ffi::OsStr| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "drawing".into());
                    Task::perform(
                        crate::io::fbx::pick_fbx_path_owned(stem),
                        Message::FbxExportPath,
                    )
                }
            }

            Message::FbxExportPath(Some(path)) => self.on_fbx_export_path_some(path),

            Message::FbxExportPath(None) => Task::none(),

            Message::FbxExportFinished(path, result) => {
                match result {
                    Ok(()) => self.command_line.push_output(
                        crate::tf!("FBXEXPORT: exported to \"{}\"", path.display()).as_ref(),
                    ),
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("FBXEXPORT: {error}").as_ref()),
                }
                Task::none()
            }

            Message::StepExportPath(Some(path)) => self.on_step_export_path_some(path),

            Message::StepExportPath(None) => Task::none(),

            Message::StepExportFinished(path, result) => {
                match result {
                    Ok(()) => self.command_line.push_output(
                        crate::tf!("STEPOUT: exported to \"{}\"", path.display()).as_ref(),
                    ),
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("STEPOUT: {error}").as_ref()),
                }
                Task::none()
            }

            // ── OBJ import ────────────────────────────────────────────────
            #[cfg(not(target_arch = "wasm32"))]
            Message::ObjImport => Task::perform(
                async {
                    crate::sys::file_dialog()
                        .set_title(crate::t!("Import OBJ Mesh").as_ref())
                        .add_filter(crate::t!("Wavefront OBJ").as_ref(), &["obj", "OBJ"])
                        .add_filter("glTF", &["gltf", "glb", "GLTF", "GLB"])
                        .add_filter(crate::t!("All Files").as_ref(), &["*"])
                        .pick_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                Message::ObjImportPath,
            ),

            // Web: the browser picker hands back a handle whose bytes must be
            // read directly — the name-only `PathBuf` it maps to cannot be
            // opened — so pick, read, and parse in one step and feed the
            // shared finished handler. A cancelled picker stays silent.
            #[cfg(target_arch = "wasm32")]
            Message::ObjImport => {
                let tab_id = self.tabs[self.active_tab].id;
                Task::perform(
                    async move {
                        let handle = crate::sys::file_dialog()
                            .set_title(crate::t!("Import OBJ Mesh").as_ref())
                            .add_filter(crate::t!("Wavefront OBJ").as_ref(), &["obj", "OBJ"])
                        .add_filter("glTF", &["gltf", "glb", "GLTF", "GLB"])
                            .add_filter(crate::t!("All Files").as_ref(), &["*"])
                            .pick_file()
                            .await?;
                        let path = crate::sys::handle_path(&handle);
                        let bytes = handle.read().await;
                        let result = crate::io::obj::parse_mesh_file(&path, &bytes, None, None)
                            .ok_or_else(|| "no usable geometry in file".to_string());
                        Some((tab_id, path, result))
                    },
                    |picked| match picked {
                        Some((tab_id, path, result)) => {
                            Message::ObjImportFinished(tab_id, path, result)
                        }
                        None => Message::Noop,
                    },
                )
            }

            Message::ObjImportPath(Some(path)) => self.on_obj_import_path_some(path),

            Message::ObjImportPath(None) => Task::none(),

            Message::ObjImportFinished(tab_id, path, result) => {
                match result {
                    Err(error) => self
                        .command_line
                        .push_error(crate::tf!("IMPORTOBJ: {error}").as_ref()),
                    Ok(meshes) => {
                        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
                            self.command_line.push_info(
                                crate::t!("IMPORTOBJ: target drawing was closed.").as_ref(),
                            );
                            return Task::none();
                        };
                        let file_stem = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "obj_mesh".into());
                        self.push_undo_snapshot(i, "IMPORTOBJ");
                        let count = meshes
                            .into_iter()
                            .map(|mesh| {
                                self.tabs[i].scene.add_entity(document::EntityType::Mesh(mesh))
                            })
                            .filter(|handle| !handle.is_null())
                            .count();
                        if count > 0 {
                            self.tabs[i].dirty = true;
                            self.command_line.push_output(
                                crate::tf!("IMPORTOBJ: imported \"{file_stem}\" as mesh.").as_ref(),
                            );
                        }
                    }
                }
                Task::none()
            }

            Message::ModelImportFinished(tab_id, generation, path, format, result) => {
                self.finish_model_import(tab_id, generation, &path, format, result)
            }

            Message::LandXmlImportFinished(tab_id, generation, path, result) => {
                self.finish_landxml_import(tab_id, generation, &path, result);
                Task::none()
            }

            Message::PictureImportFinished(tab_id, generation, path, format, prompt, read) => self
                .finish_picture_import(tab_id, generation, path.as_deref(), format, prompt, read),

            Message::SaveFile => self.on_save_file(),
            Message::ExtractionOutdated(choice) => self.on_extraction_outdated(choice),

            Message::SaveAs => {
                if self.read_only {
                    self.command_line.push_error(
                        crate::t!("Read-only session (--read-only): saving is disabled.").as_ref(),
                    );
                    return Task::none();
                }
                let i = self.active_tab;
                self.save_dialog_for_unsaved = false;
                self.open_save_dialog_window(i)
            }

            Message::SaveDialogFormatChanged(fmt) => {
                let (ext, _) = crate::io::parse_save_format(&fmt);
                let stem = std::path::Path::new(&self.save_dialog_filename)
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "drawing".to_string());
                self.save_dialog_filename = format!("{stem}.{ext}");
                self.save_dialog_format = fmt;
                Task::none()
            }

            Message::SaveDialogFilenameChanged(name) => {
                self.save_dialog_filename = name;
                Task::none()
            }

            Message::SaveDialogConfirm => self.on_save_dialog_confirm(),

            Message::SaveDialogCancel => self.close_save_dialog_window(),

            #[cfg(not(target_arch = "wasm32"))]
            Message::SaveDialogPathPicked(picked) => self.on_save_dialog_path_picked(picked),

            #[cfg(target_arch = "wasm32")]
            Message::SaveDialogPathPicked(_) => Task::none(),

            Message::ClearScene => {
                let i = self.active_tab;
                if self.tabs[i]
                    .ifccad_session
                    .as_ref()
                    .is_some_and(ocs_save::IfccadSession::has_native_context)
                {
                    self.command_line.push_info(
                        crate::t!("This drawing has retained IFCCAD content. Create a new drawing instead.").as_ref(),
                    );
                    return Task::none();
                }
                self.push_undo_snapshot(i, "CLEAR");
                self.tabs[i].scene.reset_to_new_drawing();
                self.tabs[i].note_drawing_cleared();
                self.tabs[i].properties = PropertiesPanel::empty();
                let doc_layers = self.tabs[i].scene.document.layers.clone();
                let vp_info = self.tabs[i].scene.viewport_list();
                self.tabs[i]
                    .layers
                    .sync_with_viewports(&doc_layers, vp_info);
                self.command_line
                    .push_output(crate::t!("Scene cleared. Standard linetypes loaded.").as_ref());
                self.tabs[i].current_path = None;
                self.tabs[i].dirty = true;
                self.sync_ribbon_layers();
                Task::none()
            }

            Message::SetRenderMode(mode) => {
                self.render_mode_menu_open = false;
                self.render_mode_preview = None;
                self.on_set_render_mode(mode)
            }

            Message::ToggleRenderModeMenu(mode) => {
                self.render_mode_menu_open = !self.render_mode_menu_open;
                self.render_mode_preview = self.render_mode_menu_open.then_some(mode);
                Task::none()
            }

            Message::DismissRenderModeMenu => {
                self.render_mode_menu_open = false;
                self.render_mode_preview = None;
                Task::none()
            }

            Message::PreviewRenderMode(mode) => {
                if self.render_mode_menu_open {
                    self.render_mode_preview = Some(mode);
                }
                Task::none()
            }

            Message::SetProjection(ortho) => {
                use crate::scene::Projection;
                let proj = if ortho {
                    Projection::Orthographic
                } else {
                    Projection::Perspective
                };
                let i = self.active_tab;
                self.tabs[i].scene.set_projection_preserving_frame(proj);
                self.ribbon.set_ortho(ortho);
                self.command_line.push_output(
                    crate::t!(if ortho {
                        "Projection: Orthographic"
                    } else {
                        "Projection: Perspective"
                    })
                    .as_ref(),
                );
                Task::none()
            }

            Message::PdfDialog(message) => self.update_pdf_dialog(message),
            Message::HatchDialog(message) => self.update_hatch_dialog(message),
            Message::ImportDialog(message) => self.update_import_dialog(message),
            Message::AttdefDialog(message) => self.on_attdef_dialog(message),
            Message::InsertAttrDialog(message) => self.on_insert_attr_dialog(message),
            Message::AttMan(message) => self.on_attman(message),
            Message::FieldDialog(message) => self.on_field_dialog(message),
            Message::RefEditDialog(message) => self.update_refedit_dialog(message),
            Message::PdfPassword(message) => self.update_pdf_password(message),
            Message::DimInspect(message) => self.update_dim_inspect(message),
            Message::TableCellDialog(message) => self.update_table_cell_dialog(message),
            Message::AnnoScalePrompt(message) => self.on_anno_scale_prompt(message),
            Message::SurfaceList(message) => self.on_surface_list(message),
            Message::PointGroup(message) => self.on_point_group(message),
            Message::PointTool(message) => self.on_point_tool(message),
            Message::MTextDialog(message) => self.on_mtext_dialog(message),
            Message::HyperlinkDialog(message) => self.update_hyperlink_dialog(message),
            Message::Spell(message) => self.update_spell(message),
            Message::PdfShxDialog(message) => self.update_pdf_shx_dialog(message),
            Message::PcManager(message) => self.update_pc_manager(message),
            Message::Count(message) => self.on_count(message),
            Message::GeometryCleanup(message) => self.on_geometry_cleanup(message),
            Message::BricscadVisibility(message) => self.on_bricscad_visibility(message),
            Message::FilesPalette(message) => self.on_files_palette(message),
            Message::Laywalk(message) => self.on_layer_walk(message),
            Message::QuickCalc(message) => self.on_quick_calc(message),
            Message::Assistant(message) => self.on_assistant(message),
            Message::SheetSet(message) => self.on_sheet_set(message),
            Message::KeyTip(input) => self.on_keytip(input),
            Message::RibbonSelectTab(idx) => {
                if self.ribbon.click_tab(idx, iced::time::Instant::now()) {
                    self.save_config();
                }
                Task::none()
            }
            Message::RibbonWheel(delta) => {
                self.ribbon.on_wheel(delta, iced::time::Instant::now());
                Task::none()
            }
            Message::RibbonRefit => {
                self.ribbon.refit();
                Task::none()
            }
            Message::RibbonMinimize(msg) => {
                if self.ribbon.on_minimize(msg, iced::time::Instant::now()) {
                    self.save_config();
                }
                Task::none()
            }
            Message::RibbonQat(msg) => {
                let out = self.ribbon.on_qat(msg);
                if out.save {
                    self.save_config();
                }
                let focus = if out.focus {
                    iced::widget::operation::focus(iced::widget::Id::new(
                        crate::ui::ribbon::QAT_PICKER_INPUT,
                    ))
                } else {
                    Task::none()
                };
                let run = out.run.map_or_else(Task::none, |run| self.update(run));
                Task::batch([focus, run])
            }
            Message::RibbonSlideOut(msg) => {
                if self.ribbon.on_slide_out(msg, iced::time::Instant::now()) {
                    self.save_config();
                }
                Task::none()
            }
            Message::RibbonTick => {
                self.ribbon.tick(iced::time::Instant::now());
                Task::none()
            }
            Message::RibbonSearch(msg) => {
                let out = self.ribbon.on_search(msg, iced::time::Instant::now());
                if out.save {
                    self.save_config();
                }
                let mut tasks = Vec::new();
                if out.focus {
                    tasks.push(iced::widget::operation::focus(iced::widget::Id::new(
                        crate::ui::ribbon::SEARCH_INPUT_ID,
                    )));
                }
                if let Some(y) = out.scroll_to {
                    tasks.push(iced::widget::operation::scroll_to(
                        crate::ui::ribbon::results_scroll_id(),
                        iced::widget::scrollable::AbsoluteOffset { x: 0.0, y },
                    ));
                }
                if let Some(run) = out.run {
                    tasks.push(self.update(run));
                }
                Task::batch(tasks)
            }
            Message::MenuCommand(line) => {
                let sweep = self.sync_active_field_if_any();
                self.ribbon.close_dropdown();
                // As a typed command does: the application menu, the
                // drawing's context menu and the Isolate menu close.
                self.ribbon.close_app_menu();
                let i = self.active_tab;
                self.tabs[i].scene.selection.borrow_mut().menu.close();
                self.isolate_popup_open = false;
                // A single word dispatches as a ribbon button does; a line
                // with answers runs its command and feeds them to its
                // prompts, leaving the command at its next one.
                let run = if line.split_whitespace().nth(1).is_some() {
                    self.run_command_line_streaming(&line, false)
                } else {
                    self.dispatch_ribbon_command(&line)
                };
                Task::batch([sweep, run])
            }
            Message::RibbonFloatPanel {
                module,
                title,
                x,
                y,
            } => {
                if self.ribbon.float_panel(module, &title, x, y) {
                    self.save_config();
                }
                Task::none()
            }
            Message::RibbonFloat(msg) => {
                self.ribbon.closed = self.ribbon_closed;
                if self.ribbon.on_float(msg, iced::time::Instant::now()) {
                    self.save_config();
                }
                Task::none()
            }
            Message::RibbonReorderPanels { module, titles } => {
                self.ribbon.reorder_panels(module, &titles);
                self.save_config();
                Task::none()
            }
            Message::RibbonMenu(msg) => {
                use crate::ui::ribbon::RibbonMenuMsg;
                let follow = match &msg {
                    RibbonMenuMsg::ShowMenuBar(shown) => Some(Message::MenuBarChanged(*shown)),
                    RibbonMenuMsg::ShowRibbon(shown) => Some(Message::RibbonVisibleChanged(*shown)),
                    _ => None,
                };
                if self.ribbon.on_menu(msg) {
                    self.save_config();
                }
                follow.map_or_else(Task::none, Task::done)
            }

            Message::PointsMenu(msg) => {
                self.points_menu.update(msg);
                Task::none()
            }
            Message::RibbonToolClick { tool_id, event } => {
                // A tool clicked in a minimized ribbon's popup closes it.
                self.ribbon.close_popup();
                let sweep = self.sync_active_field_if_any();
                Task::batch(vec![sweep, self.on_ribbon_tool_click(tool_id, event)])
            }
            Message::PluginFileDialogResult { command, path } => {
                if let Some(path) = path {
                    // Dispatch "<command> <path>" with original case intact —
                    // the command line would upper-case the whole string and
                    // mangle case-sensitive paths on Linux/macOS.
                    let line = format!("{} {}", command, path.to_string_lossy());
                    if let Some(task) = self.defer_plugin_line(&line, || {
                        crate::app::plugin_frame::Resume::Direct(line.clone())
                    }) {
                        return task;
                    }
                    return self.dispatch_plugin_answer(&line, false);
                }
                Task::none()
            }

            Message::PanelSurfacePointer {
                owner,
                panel,
                pointer,
                element,
                pos,
            } => {
                // The router owns the semantics (click vs drag, coalescing,
                // settle/cancel). Delivery is owner-targeted and
                // best-effort — the same law as every v4 notification.
                // Desktop-only: the web runtime has no plugin host, so its
                // routed events simply die here (honest no-op — the paired
                // stub convention; no scene surface can exist on web).
                let routed =
                    self.panel_event_router
                        .on_pointer(&panel, pointer, element, pos);
                #[cfg(not(target_arch = "wasm32"))]
                for ev in routed {
                    crate::plugin::v4_support::publish_surface_event_to_plugin(
                        &owner, ev.panel_id, ev.sequence, ev.event,
                    );
                }
                #[cfg(target_arch = "wasm32")]
                let _ = (owner, routed);
                Task::none()
            }

            Message::PanelWidgetNodeActivated {
                owner,
                panel,
                node,
            } => {
                // Activation is the one widget gesture the plugin needs:
                // best-effort, owner-targeted, sequence-identified.
                let routed = self.panel_event_router.on_node_activated(&panel, node);
                #[cfg(not(target_arch = "wasm32"))]
                crate::plugin::v4_support::publish_surface_event_to_plugin(
                    &owner, routed.panel_id, routed.sequence, routed.event,
                );
                #[cfg(target_arch = "wasm32")]
                let _ = (owner, routed);
                Task::none()
            }
            Message::PanelWidgetExpandToggled {
                owner,
                panel,
                node,
            } => {
                // Host-local toggle; ONLY expanding an `Unloaded` row
                // notifies the plugin — it is the data source for those
                // children (lazy expansion, finding F4).
                let children = self
                    .panel_surfaces
                    .widget(&owner, &panel)
                    .and_then(|surface| surface.rows.get(&node))
                    .map(|row| row.children.clone());
                let ui = self.panel_widget_ui.entry(panel.clone()).or_default();
                let notify = match children.as_ref() {
                    Some(children) => ui.toggle_expanded(node, children),
                    None => false,
                };
                if notify {
                    let routed = self.panel_event_router.on_node_expanded(&panel, node);
                    #[cfg(not(target_arch = "wasm32"))]
                    crate::plugin::v4_support::publish_surface_event_to_plugin(
                        &owner, routed.panel_id, routed.sequence, routed.event,
                    );
                    #[cfg(target_arch = "wasm32")]
                    let _ = routed;
                }
                Task::none()
            }

            _ => Task::none(),
        }
    }
}
