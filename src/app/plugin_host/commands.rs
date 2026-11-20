// Script gating and command execution.

use super::*;

impl<'a> HostSession<'a> {
    pub fn run_command(
        &mut self,
        request: ocs_plugin_api::host::CommandRequest,
    ) -> Result<ocs_plugin_api::host::CommandOutcome, String> {
        let quiet = matches!(
            request,
            ocs_plugin_api::host::CommandRequest::RunQuiet { .. }
        );
        PluginRequest::new(self, quiet).run_command_inner(request)
    }

    fn run_command_inner(
        &mut self,
        request: ocs_plugin_api::host::CommandRequest,
    ) -> Result<ocs_plugin_api::host::CommandOutcome, String> {
        use crate::command::StepInput;
        use ocs_plugin_api::host::CommandRequest as R;
        if self.tab != self.app.active_tab {
            return Err("commands act on the active document; switch to it first".to_owned());
        }
        let tab = self.tab;
        // Cancel stays allowed: if the user turns scripts off mid-run, a command a script left
        // waiting must still be closable rather than stranded.
        if !matches!(request, R::Cancel) {
            self.script_commands_allowed()?;
        }
        let active = self.app.tabs[tab].active_cmd.is_some();
        let before = self.document().entities().count() as i64;
        let error_revision = self.app.command_line.error_revision;
        let editor_before = self.app.text_inline.is_some();
        let mtext_before = self.app.mtext_editor.is_some();
        let modal_before = self.app.active_modal.is_some();
        self.app.command_line.unconsumed.clear();
        let task = match request {
            // API v13: `RunQuiet` is `Run` with the command-line echo
            // suppressed for the whole request (see `run_command`); errors
            // still surface.
            R::Run { line } | R::RunQuiet { line } => {
                if line.trim().is_empty() {
                    return Err("a command line is empty".to_owned());
                }
                self.script_command_allowed(&line)?;
                if active {
                    return Err("a command is already active; cancel it first".to_owned());
                }
                self.app.suppress_plugin_dispatch = true;
                let task = self.app.run_command_line(&line);
                self.app.suppress_plugin_dispatch = false;
                task
            }
            R::Start { name } => {
                if name.trim().is_empty() || name.split_whitespace().count() != 1 {
                    return Err("a command name is one word".to_owned());
                }
                self.script_command_allowed(&name)?;
                if active {
                    return Err("a command is already active; cancel it first".to_owned());
                }
                self.app.suppress_plugin_dispatch = true;
                let task = self.app.dispatch_command(name.trim());
                self.app.suppress_plugin_dispatch = false;
                task
            }
            R::Cancel => {
                let task = self.app.update(crate::app::Message::CommandEscape);
                self.app.drive_headless_task(task)?;
                // Close what the command opened: an editor or a dialog would leave
                // the interface waiting on a script that has moved on.
                if !editor_before {
                    self.app.text_inline = None;
                }
                if !mtext_before && self.app.mtext_editor.is_some() {
                    self.app.mtext_cancel();
                }
                if !modal_before {
                    self.app.active_modal = None;
                }
                return Ok(self.command_outcome(before, None));
            }
            other => {
                if !active {
                    return Err("no command is waiting for input".to_owned());
                }
                let input = match other {
                    R::Point { point } => {
                        if point.iter().any(|v| !v.is_finite()) {
                            return Err("a point must be finite".to_owned());
                        }
                        StepInput::Point(glam::DVec3::new(point[0], point[1], point[2]))
                    }
                    R::Text { text } => StepInput::Text(text),
                    R::Token { text } => {
                        let task = self.app.feed_active_cmd(&text);
                        self.app.drive_headless_task(task)?;
                        return Ok(self.finish_command_step(
                            before,
                            error_revision,
                            editor_before,
                            mtext_before,
                            modal_before,
                        ));
                    }
                    R::Entity { handle, point } => {
                        if point.iter().any(|v| !v.is_finite()) {
                            return Err("a point must be finite".to_owned());
                        }
                        if self.document().get_entity(handle).is_none() {
                            return Err(format!("entity {} does not exist", handle.value()));
                        }
                        StepInput::EntityPick(
                            handle,
                            glam::DVec3::new(point[0], point[1], point[2]),
                        )
                    }
                    R::Selection => StepInput::SelectionComplete(
                        self.app.tabs[tab].scene.selected_handles_in_order(),
                    ),
                    R::Enter => StepInput::Enter,
                    R::Run { .. } | R::Start { .. } | R::RunQuiet { .. } | R::Cancel => {
                        unreachable!()
                    }
                };
                self.app.feed_command(input)
            }
        };
        self.app.drive_headless_task(task)?;
        Ok(self.finish_command_step(
            before,
            error_revision,
            editor_before,
            mtext_before,
            modal_before,
        ))
    }

    fn finish_command_step(
        &mut self,
        before: i64,
        error_revision: u64,
        editor_before: bool,
        mtext_before: bool,
        modal_before: bool,
    ) -> ocs_plugin_api::host::CommandOutcome {
        let error = (self.app.command_line.error_revision != error_revision)
            .then(|| self.app.command_line.last_error.clone().unwrap_or_default());
        let blocked = if self.app.tabs[self.tab].active_cmd.is_some() {
            Some("command".to_owned())
        } else if let (false, Some(modal)) = (modal_before, self.app.active_modal.as_ref()) {
            Some(format!("modal:{modal:?}"))
        } else if !editor_before && self.app.text_inline.is_some() {
            Some("text_editor".to_owned())
        } else if !mtext_before && self.app.mtext_editor.is_some() {
            Some("mtext_editor".to_owned())
        } else {
            None
        };
        self.set_dirty();
        self.publish_document_view();
        let mut outcome = self.command_outcome(before, blocked);
        outcome.error = error;
        outcome
    }

    fn command_outcome(
        &self,
        before: i64,
        blocked_by: Option<String>,
    ) -> ocs_plugin_api::host::CommandOutcome {
        let now = self.document().entities().count() as i64;
        let metadata = self.app.tabs[self.tab]
            .active_cmd
            .as_deref()
            .map(crate::app::control::active_command_metadata);
        let text = |key: &str| {
            metadata
                .as_ref()
                .and_then(|m| m[key].as_str())
                .unwrap_or("")
                .to_owned()
        };
        let list = |key: &str| -> Vec<String> {
            metadata
                .as_ref()
                .and_then(|m| m[key].as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let options = metadata
            .as_ref()
            .and_then(|m| m["options"].as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|o| {
                        o["keyword"]
                            .as_str()
                            .filter(|k| !k.is_empty())
                            .map(str::to_owned)
                    })
                    .collect()
            })
            .unwrap_or_default();
        ocs_plugin_api::host::CommandOutcome {
            status: if blocked_by.is_some() {
                "waiting_input"
            } else {
                "completed"
            }
            .to_owned(),
            blocked_by,
            command: text("name"),
            prompt: text("prompt"),
            accepts: list("accepts"),
            options,
            entities: now as u64,
            added: now - before,
            unconsumed: self.app.command_line.unconsumed.clone(),
            error: None,
        }
    }

    /// Run a command string on the active tab's command line (AutoLISP style).
    /// Supports AutoCAD-style `PAUSE` (and `\`) input queuing, explicit `ENTER` /
    /// `RETURN` tokens, and `\n` trailing newline execution.
    pub fn execute_command(&mut self, cmd: &str) -> bool {
        let has_newline = cmd.ends_with('\n') || cmd.ends_with('\r');
        let trimmed = cmd.trim();

        // The same two guards `run_command` applies: this is the other route a
        // script reaches the command line by, so it cannot be the unguarded one.
        // Cancelling stays allowed, as it does there.
        let cancelling = trimmed.eq_ignore_ascii_case("ESC")
            || trimmed.eq_ignore_ascii_case("ESCAPE")
            || trimmed.eq_ignore_ascii_case("CANCEL");
        if !cancelling
            && (self.script_commands_allowed().is_err()
                || self.script_command_allowed(trimmed).is_err())
        {
            return false;
        }

        // 1. ESC / CANCEL handling
        if trimmed.eq_ignore_ascii_case("ESC")
            || trimmed.eq_ignore_ascii_case("ESCAPE")
            || trimmed.eq_ignore_ascii_case("CANCEL")
        {
            self.app.tabs[self.tab].pending_pause_tokens = None;
            let _ = self.app.feed_command(crate::command::StepInput::Escape);
            self.publish_document_view();
            return true;
        }

        // 2. If a PAUSE is already pending for this tab:
        if let Some(ref mut queue) = self.app.tabs[self.tab].pending_pause_tokens {
            if trimmed.is_empty()
                || trimmed.eq_ignore_ascii_case("ENTER")
                || trimmed.eq_ignore_ascii_case("RETURN")
            {
                queue.push("ENTER".to_string());
            } else {
                for part in trimmed.split_whitespace() {
                    queue.push(part.to_string());
                }
                if has_newline {
                    queue.push("ENTER".to_string());
                }
            }
            return true;
        }

        // 3. ENTER / RETURN / empty string handling when no pause is pending
        if trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("ENTER")
            || trimmed.eq_ignore_ascii_case("RETURN")
        {
            if self.app.tabs[self.tab].active_cmd.is_some() {
                let _ = self.app.feed_command(crate::command::StepInput::Enter);
                self.publish_document_view();
                return true;
            }
            return false;
        }

        // 4. Command execution. The command line's own driver owns this: it
        //    resolves the alias, tears down whatever was running, dispatches an
        //    inline-argument line as a whole (`CLAYER TEST`, `UCS ORIGIN 0,0`),
        //    and otherwise starts the tool and feeds it the rest of the tokens,
        //    PAUSE included. Leaving that here rather than repeating it keeps a
        //    scripted line and a typed one on the same path.
        let tab = self.tab;
        self.app.suppress_plugin_dispatch = true;
        let task = if self.app.tabs[tab].active_cmd.is_some() {
            // A running command owns these tokens: they answer its prompts.
            let mut queue: Vec<String> = trimmed.split_whitespace().map(str::to_string).collect();
            if has_newline {
                queue.push("ENTER".to_string());
            }
            self.app.tabs[tab].pending_pause_tokens = Some(queue);
            self.app.drain_pending_pause_tokens(tab)
        } else {
            self.app.run_command_line_streaming(trimmed, has_newline)
        };
        let _ = self.app.drive_headless_task(task);
        self.app.suppress_plugin_dispatch = false;

        self.app.refresh_layer_panel();
        self.publish_document_view();
        true
    }

    // ── XRecord objects (API v8, additive) ────────────────────────────────────
    //
    // XRecords are non-graphical objects: name-addressed typed group-code/value
    // pairs stored under the drawing's named-object dictionary, where a
    // drawing keeps application data. They give scripts a record carrier that
    // lives in the file and survives model view entirely, with no carrier
    // entity.
}
