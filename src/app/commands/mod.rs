use super::{Message, OpenCADStudio};
use crate::command::CadCommand;
use iced::Task;
use std::path::PathBuf;

mod attdef;
mod attman;
mod block_cmdline;
mod blocks;
mod blockvars;
mod center_vars;
mod purge;
pub(crate) use purge::purge_pass;
mod block_editor_vars;
mod bvisibility;
mod count;
mod dash;
mod dgn_import;
mod dim;
mod dim_cmd;
mod dim_inspect;
mod dim_layer;
mod layer_variable;
pub(crate) mod extraction_eval;
mod mleader_layer;
pub(crate) mod display;
mod draw;
mod field;
mod fileops;
mod hyperlink;
pub(crate) use hyperlink::{hyperlink_options, set_hyperlink_options};
mod inquiry;
mod layerprops;
mod layers;
mod model_import;
mod ole;
pub(crate) use ole::{ole_quality, set_ole_quality};
mod pc_colormap;
mod pc_extract;
mod pdf_dialogs;
pub(crate) mod pdf_import;
mod pdf_password;
mod pdf_shx_text;
mod pdf_underlay;
mod plotvars;
mod point_cloud;
mod refedit;
pub(crate) mod sheet_set;
mod spell;
mod styleprops;
mod text_edit;
mod table_style_var;
mod text_layer;
mod ucsfollow;
mod view;
mod view_presets;
mod xclip;
mod xref_attach;

// `DrawOrderRefCommand` lives in the `view` family file but is referenced by
// path (`commands::DrawOrderRefCommand`) from `update.rs`, so re-export it at
// the module root to keep that path valid.
pub(crate) use view::DrawOrderCommand;

impl OpenCADStudio {
    /// First `"{prefix}{n}"` (n ≥ 1) not already used by a block record in the
    /// active drawing. Used to auto-name a paste-as-block definition.
    pub(super) fn unique_block_name(&self, prefix: &str) -> String {
        let i = self.active_tab;
        let mut n = 1;
        loop {
            let name = format!("{prefix}{n}");
            if self.tabs[i]
                .scene
                .document
                .block_records
                .get(&name)
                .is_none()
            {
                return name;
            }
            n += 1;
        }
    }

    /// Per-command start state every new command begins from — also used by
    /// entry points that install a command without going through dispatch
    /// (the block palette), so they don't inherit the previous command's last
    /// point, snaps or dynamic-input values. (#1525)
    pub(in crate::app) fn reset_command_start_state(&mut self, i: usize) {
        self.tabs[i].measurement.clear();
        // A command parked behind a transparent zoom / MTP goes with it.
        self.tabs[i].suspended_cmd = None;
        self.tabs[i].transparent_resume = false;
        // Starting any command leaves interactive navigation modes (their own
        // command arms below re-enable the selected one).
        self.tabs[i].pan_mode = false;
        self.tabs[i].orbit_mode = false;
        self.tabs[i].zoom_dynamic_mode = false;
        // Reset the last committed point so the first click of the new command
        // is not constrained by ortho/polar relative to a previous command's endpoint.
        self.last_point = None;
        // A new command collects its own points, so the previous command's
        // accepted snaps must not leak into it.
        self.clear_accepted_snaps();
        self.snapper.clear_override();
        // Starting a command restarts the right-click cycle, so its first
        // right-click acts as Enter rather than opening the context menu.
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
            .input
            .right_click_entered = false;
        // A fresh command starts at the polar/cartesian default — clear
        // any `,`-driven reshape and locked dynamic-input values from a
        // previous command. Otherwise a bare Enter on the first point prompt
        // can commit that stale coordinate instead of accepting the command's
        // default (LIMITS then compares an unintended lower-left point with
        // the displayed default upper-right).
        self.dyn_user_reshaped = false;
        self.dyn_coord_absolute = false;
        self.tabs[i].dyn_fields.clear();
        self.tabs[i].dyn_active = 0;
    }

    pub(super) fn dispatch_command(&mut self, cmd: &str) -> Task<Message> {
        let task = self.dispatch_command_inner(cmd, false);
        self.clear_idle_tool_highlight();
        task
    }

    /// Run a ribbon command. A `;` stands for Enter, as in a menu macro: each
    /// part answers the running command's prompt, or starts a command when
    /// none runs (`PDFIMPORT;FILE`).
    pub(super) fn dispatch_ribbon_command(&mut self, cmd: &str) -> Task<Message> {
        if !cmd.contains(';') {
            return self.dispatch_command(cmd);
        }
        let i = self.active_tab;
        let mut tasks = Vec::new();
        for part in cmd.split(';') {
            tasks.push(if self.tabs[i].active_cmd.is_some() {
                self.feed_active_cmd(part)
            } else {
                self.dispatch_command(part)
            });
        }
        Task::batch(tasks)
    }

    /// Turn the ribbon highlight off when the command just dispatched left
    /// nothing running: no interactive command, dialog or navigation mode.
    /// One-shot commands (view changes, clipboard, toggles, audits…) would
    /// otherwise leave it lit forever; interactive commands and dialog owners
    /// keep theirs until the command end / modal close clears it. (#355)
    pub(in crate::app) fn clear_idle_tool_highlight(&mut self) {
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_none()
            && self.active_modal.is_none()
            && !self.tabs[i].pan_mode
            && !self.tabs[i].orbit_mode
            && !self.tabs[i].zoom_dynamic_mode
        {
            self.ribbon.deactivate_tool();
        }
    }

    /// Dispatch a verb typed at the interactive command line, falling back to
    /// the closest autocomplete suggestion when the verb matches no command
    /// family. Lets a partial command run on Enter (`BAC` → `BACKGROUND`),
    /// the standard DWG command-line behavior. The fallback only fires for
    /// genuinely unknown verbs, so complete aliases that resolve through a
    /// dispatch family (`LT`, `ZO`, …) still run as typed. Programmatic
    /// callers (ribbon, plugins, headless automation) use `dispatch_command`
    /// and never get silent substitution.
    pub(super) fn dispatch_command_or_suggest(&mut self, cmd: &str) -> Task<Message> {
        let task = self.dispatch_command_inner(cmd, true);
        self.clear_idle_tool_highlight();
        task
    }

    fn dispatch_command_inner(&mut self, cmd: &str, allow_suggest: bool) -> Task<Message> {
        self.try_dispatch_command(cmd, allow_suggest)
            .unwrap_or_else(|| {
                self.command_line
                    .push_error(crate::tf!("Unknown command: {cmd}").as_ref());
                self.finish_dispatch(cmd)
            })
    }

    /// Dispatch `cmd`, or `None` when no command answers to it (nothing is
    /// reported then). A script line uses it to tell a command that took
    /// the words after it from one that did not.
    pub(in crate::app) fn try_dispatch_command(
        &mut self,
        cmd: &str,
        allow_suggest: bool,
    ) -> Option<Task<Message>> {
        let i = self.active_tab;
        // Expand a command alias ("L" → "LINE") on the leading token before any
        // routing, so every path below (Start-tab gate, plugins, all dispatch
        // families, the Repeat menu) sees the canonical command. Arguments after
        // the first space are left untouched. A non-alias passes through as-is.
        // The transparent prefix of commercial solutions: `'PAN` / `'ZOOM` run in the middle
        // of another command and hand control back to it afterwards.
        let (cmd, quoted) = match cmd.trim().strip_prefix('\'') {
            Some(rest) => (rest.trim(), true),
            None => (cmd, false),
        };
        let resolved = self.resolve_alias(cmd);
        let cmd = resolved.as_deref().unwrap_or(cmd);
        if let Some(task) = self.dispatch_block_editor_vars(cmd, i) {
            return Some(task);
        }
        if cmd.trim_start_matches(['_', '.']) == "BGRIPSET" {
            return Some(self.start_block_grip_set());
        }
        let construction = cmd.trim_start_matches(['_', '.']);
        if construction == "BCONSTRUCTION" || construction.starts_with("BCONSTRUCTION ") {
            return Some(self.start_block_construction(construction.strip_prefix("BCONSTRUCTION").unwrap().trim()));
        }
        if cmd.trim_start_matches(['_', '.']) == "BCYCLEORDER" {
            return Some(self.open_block_cycle_order());
        }
        let verb = cmd
            .split([' ', ':'])
            .next()
            .unwrap_or(cmd)
            .trim_start_matches(['_', '.']);
        if self.block_editor_preferences.locked
            && matches!(verb, "BEDIT" | "BEDIT_BEGIN" | "BEDIT_OPEN" | "BEDIT_NEW")
        {
            self.command_line.push_error(
                crate::t!("The Block Editor is locked by BLOCKEDITLOCK.").as_ref(),
            );
            return Some(Task::none());
        }
        if cmd == "BACTIONSET" {
            if let Some(error) = self.action_set_error() {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Some(Task::none());
            }
        }
        if cmd == "BASSOCIATE" {
            if let Some(error)=self.action_association_error() {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Some(Task::none());
            }
        }
        if cmd == "BACTIONBAR" || cmd.starts_with("BACTIONBAR ") {
            if let Some(error)=self.action_bar_error() {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Some(Task::none());
            }
        }
        if self.block_editor_preferences.action_bars && self.tabs[i].active_block_edit_session().is_some()
            && matches!(verb,"BACTIONSET"|"BASSOCIATE"|"BLOOKUPTABLE") {
            self.command_line.push_error(crate::t!("Use the action bar to edit this action, or set BACTIONBARMODE to 0.").as_ref());
            return Some(Task::none());
        }
        if matches!(
            cmd.split(' ').next(),
            Some("BACTION" | "BACTIONTOOL" | "BARRAY")
        ) {
            let args = if cmd == "BARRAY" || cmd.starts_with("BARRAY ") {
                "ARRAY"
            } else {
                cmd.split_once(' ').map_or("", |(_, a)| a.trim())
            };
            if !args.is_empty() && crate::scene::block_actions::Kind::parse(args).is_none() {
                self.command_line.push_error(
                    crate::t!(
                        "Supported actions: Move, Scale, Stretch, Polar Stretch, Rotate, Flip, Array. Other action types are not wired yet."
                    )
                    .as_ref(),
                );
                return Some(Task::none());
            }
            if let Some(error) = self.action_error(crate::scene::block_actions::Kind::parse(args)) {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Some(Task::none());
            }
        }
        if cmd == "BPARAMETER" || cmd.starts_with("BPARAMETER ") {
            if let Some(error) =
                self.parameter_placement_error(cmd.strip_prefix("BPARAMETER").unwrap().trim())
            {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Some(Task::none());
            }
        }
        if cmd == "BLOOKUPTABLE" || cmd.starts_with("BLOOKUPTABLE ") {
            if let Some(error) = self.lookup_table_error() {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Some(Task::none());
            }
        }
        if cmd == "BTABLE" || cmd == "BPROPERTIESTABLE" || cmd.starts_with("BPROPERTIESTABLE ") {
            if let Some(error) = self.properties_table_error() {
                self.command_line.push_error(crate::t!(error).as_ref());
                return Some(Task::none());
            }
        }
        if matches!(cmd, "RESETBLOCK" | "RESETBLOCK APPLY")
            && self.tabs[i].active_block_edit_session().is_some()
        {
            self.command_line.push_error(
                crate::t!("RESETBLOCK is not available inside the Block Editor.").as_ref(),
            );
            return Some(Task::none());
        }
        if self.tabs[i].block_test.is_some() {
            let name = cmd.split([' ', ':']).next().unwrap_or(cmd);
            if matches!(
                name,
                "BEDIT"
                    | "BEDIT_BEGIN"
                    | "BEDIT_OPEN"
                    | "REFEDIT"
                    | "REFEDIT_BEGIN"
                    | "BSAVE"
                    | "BSAVEAS"
                    | "SAVE"
                    | "QSAVE"
                    | "SAVEAS"
                    | "SAVEALL"
                    | "WBLOCK"
                    | "EXPORT"
                    | "ETRANSMIT"
                    | "ARCHIVE"
            ) {
                self.command_line.push_error(
                    crate::t!("Close Test Block before saving or opening another block editor.")
                        .as_ref(),
                );
                return Some(Task::none());
            }
        }
        // V7 (reference check): inside a block editor session a set of
        // commands is refused outright. The hook sits after alias resolution
        // and before the transparent split and the running-command teardown,
        // so a rejected command leaves whatever is running untouched.
        if self.tabs[i].active_block_edit.is_some() {
            let name = cmd.split([' ', ':']).next().unwrap_or(cmd);
            if matches!(
                name,
                "LAYOUT"
                    | "-LAYOUT"
                    | "-PURGE"
                    | "-RENAME"
                    | "-PLOT"
                    | "VPORTS"
                    | "MVIEW"
                    | "WBLOCK"
                    | "PURGE"
                    | "RENAME"
                    | "REFEDIT"
                    | "REFEDIT_BEGIN"
                    | "PLOT"
            ) {
                self.command_line.push_error(
                    crate::tf!("{} command not allowed in block editor", name).as_ref(),
                );
                return Some(Task::none());
            }
        }
        if is_spacemouse_command(cmd) {
            return Some(self.run_action(cmd));
        }
        // A drafting aid only flips a flag, so it must not disturb whatever is
        // already running: pressing F8 partway through a LINE means "constrain
        // the rest of this line", not "abandon it". Everything below tears the
        // running command down, so a transparent one skips straight past it.
        // (#677) A quoted navigation command does the same; an interactive
        // one (ZOOM Window) parks the running command and resumes it when the
        // zoom ends, as does a ZOOM the zoom prompt itself hands off to.
        let transparent = is_transparent(cmd)
            || (quoted && is_transparent_capable(cmd))
            || (self.tabs[i].transparent_resume && cmd.starts_with("ZOOM"));
        if transparent {
            if zoom_prompts(cmd) && self.tabs[i].active_cmd.is_some() {
                self.tabs[i].suspended_cmd = self.tabs[i].active_cmd.take();
                self.tabs[i].transparent_resume = true;
                self.tabs[i].scene.clear_preview_wire();
            }
            let task = self.dispatch_families(cmd, i).unwrap_or_else(Task::none);
            // A one-shot zoom is over already — resume straight away.
            self.resume_transparent_parent(i);
            return Some(task);
        }
        // A walk is only a temporary hide, and these commands take ownership of
        // layer visibility from here on: starting one puts the drawing back
        // first, so the override never swallows the result it is hiding.
        if self.tabs[i].layer_walk.is_some() && Self::layer_walk_conflicts_with(cmd) {
            self.abandon_layer_walk_on(i);
        }
        // A new command abandons any grip edit and its numeric input.
        let had_pending_grip_input = self.grip_pending.take().is_some();
        let had_active_grip = self.cancel_active_grip_edit();
        if had_pending_grip_input || had_active_grip {
            self.command_line.input.clear();
        }
        // Starting a command closes any open ribbon dropdown (e.g. a style
        // combo left open) so it does not stay stuck behind the new tool.
        self.ribbon.close_dropdown();
        // Selection keywords last only for the round that asked for them: a
        // Remove or a fixed Window sense must not quietly still be in force
        // when the next command asks for objects. (#596)
        self.select_remove_mode = false;
        {
            let mut selection = self.tabs[i].scene.selection.borrow_mut();
            // Cancel the active selection gesture before the new command starts.
            selection.clear_left_selection_gesture();
        }
        // Cancel any running command before starting a new one.
        if self.tabs[i].active_cmd.is_some() {
            self.tabs[i].pending_parameter_set_follow_up = None;
            self.tabs[i].pending_xy_box_set = None;
            self.tabs[i].pending_action_pair = None;
            self.tabs[i].scene.clear_preview_wire();
            self.tabs[i].active_cmd = None;
            // Interrupting an ADDSELECTED draw with another command reverts its
            // template-property override too (#239).
            self.restore_add_selected_defaults();
        }
        self.reset_command_start_state(i);

        if let Some(path_str) = cmd.strip_prefix("OPEN_RECENT:") {
            let path = PathBuf::from(path_str);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            return Some(Task::done(Message::OpenPathPicked(Some((path, size)))));
        }

        // The Start (welcome) tab has no drawing to act on, so a drawing
        // command would silently do nothing. Allow only the commands that
        // make sense there (create / open a document, or quit) and tell the
        // user otherwise instead of running a no-op. See #96.
        // Anything that acts on the application rather than a drawing belongs
        // here: document lifecycle, the links, and the app-wide configuration
        // editors (shortcuts, aliases) — none of them read the scene. This is
        // the single place that decides; `on_ribbon_tool_click` defers to it
        // rather than keeping a second, blunter copy (#388, #389).
        if self.tabs[i].is_start && !start_allowed(strip_command_prefixes(cmd).unwrap_or(cmd)) {
            self.command_line.push_info(
                crate::t!("No drawing open. Use NEW or OPEN to start a drawing.").as_ref(),
            );
            return Some(Task::none());
        }

        // Light the ribbon button of the command now starting, however it was
        // started (typed, alias, shortcut, Repeat), so the user sees where it
        // lives. A transparent command returned above and leaves the running
        // command's highlight alone.
        self.ribbon
            .show_command(strip_command_prefixes(cmd).unwrap_or(cmd));

        if !self.suppress_plugin_dispatch && crate::plugin::try_dispatch(self, i, cmd) {
            // try_dispatch returns true for both finished commands and interactive
            // commands that it just installed. If no command is now active, the
            // tool was a one-shot and we must turn the ribbon highlight off here —
            // normally apply_cmd_result does that, but plugin dispatch can return
            // without producing a CmdResult.
            self.tabs[i].last_cmd = Some(cmd.to_string());
            self.command_line.record_recent(cmd);
            if self.tabs[i].active_cmd.is_none() {
                self.ribbon.deactivate_tool();
            }
            return Some(self.finish_dispatch(cmd));
        }

        // Direct plugin panel toggle command matching
        let cmd_upper = cmd.trim().to_uppercase();
        let stripped_cmd = cmd_upper.replace([' ', '_', '-'], "");
        let panel_match = self.plugin_panels.iter().find_map(|(id, state)| {
            let matches_cmd = state.def.command.as_deref().map(|c| {
                let cu = c.to_uppercase();
                cu == cmd_upper || cu.replace([' ', '_', '-'], "") == stripped_cmd
            }).unwrap_or(false);
            let matches_id = {
                let id_u = id.to_uppercase();
                id_u == cmd_upper || id_u.replace([' ', '_', '-'], "") == stripped_cmd
            };
            let matches_title = {
                let tu = state.def.title.to_uppercase();
                tu == cmd_upper || tu.replace([' ', '_', '-'], "") == stripped_cmd
            };
            if matches_cmd || matches_id || matches_title {
                Some((id.clone(), state.def.title.clone()))
            } else {
                None
            }
        });
        if let Some((panel_id, title)) = panel_match {
            #[cfg(not(target_arch = "wasm32"))]
            {
                use ocs_plugin_api::host::HostApi;
                let mut host = crate::app::plugin_host::HostSession::new(self, i);
                let toggled = host.toggle_panel(&panel_id, None);
                if toggled {
                    self.command_line.push_info(&format!("{title} palette toggled."));
                    self.tabs[i].last_cmd = Some(cmd.to_string());
                    self.command_line.record_recent(cmd);
                    return Some(self.finish_dispatch(cmd));
                }
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (panel_id, title);
            }
        }

        // Command families are dispatched in source order (see
        // `dispatch_families`); the first whose `match` arm matches handles it.
        if let Some(t) = self.dispatch_families(cmd, i) {
            // A command resolved — record it for the right-click Repeat menu so
            // commands from every source (command line, ribbon, context menu,
            // shortcuts) appear there, not only typed ones. Recorded after
            // resolution so a partial verb completed via the suggestion
            // fallback (`BAC`) stores the real command (`BACKGROUND`).
            self.command_line.record_recent(cmd);
            return Some(t);
        }

        // Scripts and typed input may carry the international (`_`) and
        // built-in (`.`) name prefixes (`_LINE`, `._LINE`). Ribbon tools
        // dispatch internal names that start with `_` themselves
        // (`_PCCROPPOLY`), so a name is tried as written first and loses its
        // prefixes only when nothing handles it.
        if let Some(bare) = strip_command_prefixes(cmd) {
            return self.try_dispatch_command(bare, allow_suggest);
        }

        // No family matched. From the interactive command line, run the
        // closest autocomplete suggestion instead of erroring, so a partial
        // command completes on Enter (`BAC` → `BACKGROUND`). The verb's own
        // input was already cleared, so rank against `cmd` directly.
        if allow_suggest {
            if let Some(top) = crate::ui::command_line::ranked_matches(
                cmd,
                &self.command_line.dynamic_commands,
                &self.command_line.command_aliases,
            )
            .into_iter()
            .next()
            {
                if !top.eq_ignore_ascii_case(cmd) {
                    return self.try_dispatch_command(&top, false);
                }
            }
        }
        None
    }

    /// Try each command family in source order, returning the first that
    /// handles `cmd`, or `None` when none match. Each family returns
    /// `Some(task)` for an arm it owns (early-returning or falling through to
    /// `finish_dispatch`), or `None` to defer to the next — equivalent to one
    /// sequential `match` over all arms.
    fn dispatch_families(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        if let Some(t) = self.dispatch_text_layer(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_dxeval(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_dim_layer(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_table_style_var(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_layer_variable(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_dim_cmd(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_dim_inspect(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_mleader_layer(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_dash(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_ucsfollow(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_fileops(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_layers(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_xref_attach(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_import(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_model_import(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_bvisibility(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_block_cmdline(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_blocks(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_pdf_underlay(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_pc_colormap(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_pc_extract(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_attdef(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_ole(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_sheet_set(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_field(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_hyperlink(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_text_edit(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_spell(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_count(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_draw(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_dim(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_refedit(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_inquiry(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_view(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_layerprops(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_plotvars(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_blockvars(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_styleprops(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_display(cmd, i) {
            return Some(t);
        }
        None
    }

    /// Shared tail run after a `dispatch_*` family handler whose matched arm
    /// did not early-return. Focuses the command line whenever a command just
    /// became active.
    pub(crate) fn finish_dispatch(&mut self, cmd: &str) -> Task<Message> {
        crate::ui::ribbon::keytip_note_command_finished();
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_some() {
            self.tabs[i].last_cmd = Some(cmd.to_string());
            self.sync_dyn_fields();
            self.focus_cmd_input()
        } else {
            Task::none()
        }
    }
}

/// Commands that toggle a drafting aid and nothing else.
///
/// They are reachable from a function key, and a function key gets pressed
/// mid-command — that is the point of it. Running them through the ordinary
/// path cancelled the active command, cleared its preview and dropped its base
/// point, so F8 during a MOVE both ended the move and made the dragged ghost
/// vanish. Nothing here starts a command, opens a document or reads geometry,
/// so there is nothing for the teardown to protect. (#677)
/// Commands that may run transparently (`'PAN`, `'ZOOM …`): the navigation
/// set, which never edits the drawing.
fn is_transparent_capable(cmd: &str) -> bool {
    cmd == "PAN"
        || cmd == "ZOOM"
        || cmd.starts_with("ZOOM ")
        || matches!(
            cmd,
            "ZW" | "ZE" | "ZA" | "ZP" | "ZI" | "ZO" | "ZD" | "ZEA" | "ZOBJ"
        )
}

/// ZOOM forms that install an interactive prompt (window corners, object
/// pick) and so need the running command parked while they last.
fn zoom_prompts(cmd: &str) -> bool {
    matches!(
        cmd,
        "ZOOM" | "ZOOM WINDOW" | "ZOOM W" | "ZW" | "ZOOM OBJECT" | "ZOOM O" | "ZOBJ"
    )
}

pub fn is_transparent(cmd: &str) -> bool {
    is_spacemouse_command(cmd)
        || matches!(
            cmd,
            "ORTHO"
                | "GRID"
                | "SNAP"
                | "POLAR"
                | "OSNAP"
                | "DSETTINGS"
                | "BAUTHORPALETTE"
                | "BAUTHORPALETTECLOSE"
                | "PARAMETERSCLOSE"
        )
}

fn is_spacemouse_command(cmd: &str) -> bool {
    matches!(
        cmd,
        "SPACEMOUSE"
            | "SPACEMOUSEPAUSE"
            | "SPACEMOUSEPAN"
            | "SPACEMOUSEPANZOOM"
            | "SPACEMOUSEAUTO"
            | "SPACEMOUSE3D"
    )
}

/// Whether `cmd` makes sense on the Start (welcome) tab — document lifecycle,
/// links, and app-wide configuration; nothing that reads the scene. Single
/// source of truth: the dispatch gate refuses everything else, and the ribbon
/// dims the tools this rejects.
pub fn start_allowed(cmd: &str) -> bool {
    // A batch script starts from no drawing: it sets FILEDIA / SDI and
    // opens the drawings it works on by path, and EXIT answers its save
    // questions from any tab.
    let verb = cmd.split_whitespace().next().unwrap_or("");
    is_spacemouse_command(cmd)
        || matches!(verb, "OPEN" | "SCRIPT" | "SCR" | "FILEDIA" | "SDI" | "EXIT")
        || matches!(
            cmd,
            "NEW"
            | "QNEW"
            | "OPEN"
            | "EXIT"
            | "QUIT"
            | "REPORT"
            | "CHANGELOG"
            | "ABOUT"
            | "PLUGINS"
            | "PLUGINMANAGER"
            | "DONATE"
            | "WEBVERSION"
            | "HELP"
            | "PERF"
            | "CUI"
            | "ALIASEDIT"
            | "CUILOAD"
            | "CUIIMPORT"
            // The start page already offers Options as a button, so the
            // command that opens the same dialog belongs here too.
            | "OPTIONS"
            | "OP"
        )
}

// ── Autocomplete registry — one-shot commands ──────────────────────────────
// These commands dispatch a single action (file ops, view, layer/style
// managers, undo/redo, …) rather than installing an interactive `CadCommand`,
// so they have no module of their own to register from. They are surfaced for
// command-line autocomplete here. Internal dispatch tokens that the user never
// types (REFEDIT_BEGIN, REFCLOSE_SAVE, REFCLOSE_DISCARD) are intentionally
// excluded.
inventory::submit!(crate::command::CommandRegistration {
    names: &[
        // Drafting-aid / display toggles + customization entry points wired in the
        // dispatch families (no interactive command module of their own).
        "ALIASEDIT",
        // Implemented command-line block forms must also validate in ALIASEDIT.
        "-BLOCK",
        "-BMAKE",
        "-WBLOCK",
        "-WB",
        "-ATTEDIT",
        // Script forms of the table commands.
        "-LAYER",
        "-PURGE",
        "-OVERKILL",
        "-STYLE",
        "-RENAME",
        "-DIMSTYLE",
        "-LAYOUT",
        "-PLOT",
        "CLEANSCREEN",
        "CUI",
        "DSETTINGS",
        "PARAMETERS",
        "PARAMETERSCLOSE",
        "BTABLE",
        "GRID",
        "GRAPHICATTRIBUTES",
        "ISODRAFT",
        "ISOPLANE",
        "OSNAP",
        "POLAR",
        "QUICKPROPERTIES",
        "SNAP",
        "OPTIONS",
        "OP",
        "SYNCPVIEWPORTS",
        "VPSYNC",
        // Viewport-arrangement shortcuts (delegate to VPORTS configurations).
        "CASCADE",
        "HORIZONTAL",
        "VERTICAL",
        "VPJOIN",
        // Standard aliases for existing commands.
        "BMAKE",
        "EXPORTPDF",
        "EXPORTSVG",
        "PRINTALL",
        "PUBLISH",
        "DDIM",
        // Inquiry: list the whole drawing database.
        "DBLIST",
        // Layer management.
        "LAYDEL",
        "LAYTRANS",
        "DWGUNITS",
        "LAYMRG",
        "LAYERSTATE",
        "LAS",
        "LMAN",
        // 3D move (same operation as MOVE, which already works in 3D).
        "3DMOVE",
        // Quick leader (same as LEADER).
        "QLEADER",
        "QL",
        // Restore the last-erased objects.
        "OOPS",
        // Plan (top) view.
        "PICKADD",
        "PICKDRAG",
        "PLAN",
        // Current object colour (CECOLOR).
        "COLOR",
        "COLOUR",
        "CECOLOR",
        "CETRANSPARENCY",
        "DDCOLOR",
        "BYLAYER",
        // Synchronise block attributes.
        "ATTSYNC",
        // Drawing units picker.
        "UNITS",
        "UN",
        "DDUNITS",
        // Visual styles (mapped to the wireframe / shaded view).
        "VSCURRENT",
        "SHADEMODE",
        // Raster image brightness / contrast / fade.
        "ADJUST",
        // Block list + block-attribute list (command-line forms).
        "BLOCKPALETTE",
        "BLOCKSPALETTE",
        "BLOCKSPALETTECLOSE",
        "-INSERT",
        "BLOCKMRULIST",
        "BLOCKREDEFINEMODE",
        "BLOCKNAVIGATE",
        "BLOCKSTATE",
        "EXPLMODE",
        "INSNAME",
        "ATTMAN",
        "BATTMAN",
        "-BEDIT",
        "INSBASE",
        // Drawing-content overview.
        "ADCENTER",
        "CONTENTBROWSER",
        "ADC",
        // Annotation scale.
        "ANNOSCALE",
        "CANNOSCALE",
        "ANNOALLVISIBLE",
        "ANNOAUTOSCALE",
        "ANNOUPDATE",
        "SCALELISTEDIT",
        "OBJECTSCALE",
        "ANNORESET",
        // Import CSV into a table + LandXML survey points.
        "DATALINK",
        "DATALINKUPDATE",
        "LANDXMLIMPORT",
        // Keyboard-shortcut (CUI) export / import.
        "CUIEXPORT",
        "CUIIMPORT",
        "CUILOAD",
        // Save every open drawing.
        "SAVEALL",
        // Draw-order: all text/dims to front or back; hatches to back.
        "TEXTTOFRONT",
        "TEXTTOBACK",
        "HATCHTOBACK",
        "HB",
        // Criteria-based selection (same as QSELECT) + copy with picked base.
        "FILTER",
        "FI",
        "COPYBASE",
        // Change justification of selected text/mtext.
        "JUSTIFYTEXT",
        // Command-line arithmetic calculator.
        "CAL",
        // Text tools: case, mask, width fit, sequential numbering, arc layout.
        "TCASE",
        "TEXTMASK",
        "TEXTFIT",
        "TEXTFILL",
        "TCOUNT",
        "ARCTEXT",
        // Jogged radius dimension.
        "DIMJOGGED",
        "DJO",
        "DIMJOG",
        // Mark objects annotative.
        "OBJECTSCALE",
        // Copy nested objects out of a block.
        "NCOPY",
        "NCOPYALL",
        // Close tab, hidden-line / visual styles, calculator, hyperlink.
        "CLOSE",
        "HIDE",
        "HI",
        "VISUALSTYLES",
        "QUICKCALC",
        "QC",
        "HYPERLINK",
        "XOPEN",
        "REGION",
        "REG",
        // Redraw / regenerate the display caches.
        "REDRAW",
        "REDRAWALL",
        "REGEN",
        "REGENALL",
        "ARCHIVE",
        "ETRANSMIT",
        "FLATSHOT",
        "CONVTOSURFACE",
        // Slice/section + interference + press-pull/thicken + 3D transforms + wall/pyramid.
        "SLICE",
        "SL",
        "INTERFERE",
        "INF",
        "SECTIONCULL",
        "PRESSPULL",
        "THICKEN",
        "3DROTATE",
        "ROTATE3D",
        "POLYSOLID",
        "3DMIRROR",
        "MIRROR3D",
        "3DALIGN",
        "ALIGN3D",
        "SECTION",
        "SECTIONPLANE",
        "PYRAMID",
        "PYR",
        "SPLINEFIT",
        "FITSPLINE",
        // System variables (typeable directly).
        "MIRRTEXT",
        "PERSPECTIVE",
        "GRIDMODE",
        "GRIDUNIT",
        "QPMODE",
        "QPLOCATION",
        "ZOOMWHEEL",
        "ZOOMFACTOR",
        "SHORTCUTMENU",
        "SHORTCUTMENUDURATION",
        "CURSORSIZE",
        "PICKBOX",
        "APERTURE",
        "CURSORTYPE",
        "SNAPANG",
        "ATTREQ",
        "ATTDIA",
        "DIMASSOC",
        "ANGBASE",
        "ANGDIR",
        "REGENMODE",
        "BLIPMODE",
        "SPLFRAME",
        "CAMERADISPLAY",
        "DELOBJ",
        "PLINEGEN",
        "PSLTSCALE",
        // Plot preferences and the current-layout variables (commands/plotvars.rs).
        "PLOTOFFSET",
        "PAPERUPDATE",
        "PLOTROTMODE",
        "PLOTTRANSPARENCYOVERRIDE",
        "BACKGROUNDPLOT",
        "CTAB",
        "TILEMODE",
        "PSETUPIN",
        "-PSETUPIN",
        "DISPSILH",
        "WORLDVIEW",
        "LIMCHECK",
        "DRAGMODE",
        "LUNITS",
        "LUPREC",
        "AUNITS",
        "AUPREC",
        "THICKNESS",
        "ELEVATION",
        "INSUNITS",
        "SPLINETYPE",
        "ISOLINES",
        "DIMASO",
        "DIMSHO",
        "QTEXTMODE",
        "PLIMCHECK",
        "VISRETAIN",
        "USRTIMER",
        "ATTMODE",
        "COORDS",
        "OSMODE",
        "PICKSTYLE",
        "SPLINESEGS",
        "SURFU",
        "SURFV",
        "SURFTYPE",
        "SHADEDGE",
        "MAXACTVP",
        "CMLJUST",
        "CMLSCALE",
        "CMLSTYLE",
        "TEXTQLTY",
        "SORTENTS",
        "FRAME",
        "IMAGEFRAME",
        "PDFFRAME",
        "DWFFRAME",
        "DGNFRAME",
        "PDFOSNAP",
        "UOSNAP",
        "FIELDDISPLAY",
        "FILEDIA",
        "SDI",
        "TRIMEXTENDMODE",
        "PELLIPSE",
        "SELECTIONCYCLING",
        "OSNAPZ",
        "SELECTSIMILARMODE",
        "UCSDETECT",
        "SURFACEASSOCIATIVITY",
        "DGNIMPORTMODE",
        "LAYOUTCREATEVIEWPORT",
        "TXT2MTXTCOMBINEMTEXT",
        "TXT2MTXTTEXTORDER",
        "TXT2MTXTWORDWRAP",
        "TXT2MTXTLINESPACING",
        "FIELDEVAL",
        "DXEVAL",
        "PDFIMPORTMODE",
        "PDFIMPORTFILTER",
        "PDFIMPORTLAYERS",
        "PDFIMPORTIMAGEPATH",
        "PDFSHXLAYER",
        "PDFSHXBESTFONT",
        "PDFSHXTEXTSETTINGS",
        "XDWGFADECTL",
        "POINTCLOUDCLIPFRAME",
        "POINTCLOUDDENSITY",
        "POINTCLOUDPOINTSIZE",
        "POINTCLOUDLOCK",
        "POINTCLOUDAUTOUPDATE",
        "POINTCLOUDBOUNDARY",
        "POINTCLOUDRTDENSITY",
        "POINTCLOUDLOD",
        "POINTCLOUDPOINTMAX",
        "POINTCLOUDVISRETAIN",
        "POINTCLOUDSHADING",
        "POINTCLOUDCACHESIZE",
        "POINTCLOUD2DVSDISPLAY",
        "POINTCLOUDLIGHTING",
        "POINTCLOUDLIGHTSOURCE",
        "POINTCLOUDPOINTMAXLEGACY",
        "XCLIPFRAME",
        "WIPEOUTFRAME",
        "ATTMODE0",
        "ATTMODE1",
        "ATTMODE2",
        "FRAMES0",
        "FRAMES1",
        "FRAMES3",
        "FRAMES2",
        "UOSNAP0",
        "UOSNAP1",
        "HALOGAP",
        "TRACEWID",
        "SKETCHINC",
        "SKPOLY",
        "SKTOLERANCE",
        "CLIPROMPTLINES",
        "COMMANDLINEFADETIME",
        "COMMANDLINEOPACITY",
        "MENUBAR",
        "RIBBON",
        "RIBBONCLOSE",
        "COLORTHEME",
        "SELECTIONAREA",
        "SELECTIONAREAOPACITY",
        "SELECTIONEFFECT",
        "SELECTIONEFFECTCOLOR",
        "VSEDGECOLOR",
        "WINDOWSAREACOLOR",
        "WINDOWAREACOLOR",
        "CROSSINGAREACOLOR",
        "SELECTIONPREVIEW",
        "GRIPSIZE",
        "GRIPCOLOR",
        "GRIPHOT",
        "GRIPHOVER",
        "GRIPOBJLIMIT",
        // Dispatched all along, but absent from the registry, so command-line
        // completion never offered them.
        "ISAVEBAK",
        "SAVETIME",
        "FILEASSOC",
        // Reset selected entities' overrides to follow their layer.
        "SETBYLAYER",
        // Remove duplicate objects; set drawing base point; audit integrity;
        // read/write system variables.
        "OVERKILL",
        "BASE",
        "AUDIT",
        "SETVAR",
        "SCRIPT",
        "SCR",
        "FINDNONPURGEABLE",
        "3DORBIT",
        "3O",
        "ABOUT",
        "ATTACH",
        "ATTDISP",
        "ATTEXT",
        "BACKGROUND",
        "CDIMSTY",
        "CELTSCALE",
        "CHANGELOG",
        "CHPROP",
        "CLAYER",
        "CLEAR",
        "CLR",
        "COLORSCHEME",
        "COUNT",
        "DATAEXTRACTION",
        "-DATAEXTRACTION",
        "DE",
        "DESELALL",
        "DESELECT",
        "DIMSTYLE",
        "DONATE",
        "DRAWORDER",
        "DRAWORDER_FRONT",
        "DRAWORDER_BACK",
        "DRAWORDER_ABOVE",
        "DRAWORDER_UNDER",
        "DWGPROP",
        "DWGPROPS",
        "EATTEXT",
        "EXIT",
        "EXPORT",
        "EXPORTSTEP",
        "EXPORTSTL",
        "EXTERNALREFERENCES",
        "EXTERNALREFERENCESCLOSE",
        "ERSTATE",
        "POINTCLOUDMANAGER",
        "POINTCLOUDMANAGERCLOSE",
        "EXTRIM",
        "FILETAB",
        "FIND",
        "FLATTEN",
        "HELP",
        "HIDEOBJECTS",
        "IM",
        "IMAGE",
        "IMAGEATTACH",
        "TRANSPARENCY",
        "IMAGEEMBED",
        "IMPORTOBJ",
        "ISOLATEOBJECTS",
        "LA",
        "LAYER",
        "LAYERS",
        "LAYISO",
        "LAYON",
        "LAYOUTMANAGER",
        "LAYOUTPANEL",
        "LAYOUTTAB",
        "LAYTHW",
        "LAYUNISO",
        "LAYWALK",
        "LI",
        "LINETYPE",
        "LIST",
        "LTSCALE",
        "LWDISPLAY",
        "MASSPROP",
        "MLEADERSTYLE",
        "MLSTYLE",
        "MS",
        "MSPACE",
        "CHSPACE",
        "NAVVCUBE",
        "NEW",
        "QNEW",
        "OBJIMPORT",
        "IMPORT",
        "ACISIN",
        "WMFIN",
        "DGNIMPORT",
        "-DGNIMPORT",
        "OPEN",
        "ORTHO",
        "P",
        "PAN",
        "PAGESETUP",
        "PERF",
        "PERSP",
        "PLOT",
        "PRINTERS",
        "PLOTSTYLE",
        "PLOTSTYLEEDITOR",
        "PLOTSTYLEPANEL",
        "PR",
        "PRINT",
        "PROPERTIES",
        "PROPS",
        "PSPACE",
        "PURGE",
        "QP",
        "QUICKPRINT",
        "QS",
        "QSAVE",
        "QSELECT",
        "QUIT",
        "REDO",
        "RENAME",
        "REPORT",
        "SA",
        "SAVE",
        "SAVEAS",
        "SCALETEXT",
        "SELECTALL",
        "SELECTSIMILAR",
        "SELSIM",
        // Draw a new object of the same type as the selected one. (#239)
        "ADDSELECTED",
        // Outline of origin planes, open sketch and solid bodies.
        "BROWSER",
        // Open and close a sketch: a drawing plane, a square-on view and
        // snapping, held together until the profile is done.
        "CREATESKETCH",
        "FINISHSKETCH",
        "SHEETSET",
        "SHEETSETHIDE",
        "NEWSHEETSET",
        "OPENSHEETSET",
        "SSMAUTOOPEN",
        "SSLOCATE",
        "SSMPOLLTIME",
        "SSMSHEETSTATUS",
        "SSMSTATE",
        "SSFOUND",
        "SHORTCUTS",
        "SSM",
        "STEPOUT",
        "FBXEXPORT",
        "STLOUT",
        "STPOUT",
        "STYLE",
        "STYLESMANAGER",
        "TABLESTYLE",
        "BAUTHORPALETTE",
        "BAUTHORPALETTECLOSE",
        "TOOLPALETTES",
        "TP",
        "TS",
        "U",
        "UCS",
        "UCSICON",
        "UNDERLAY",
        "UNDO",
        "UNISOLATEOBJECTS",
        "USERI",
        "USERR",
        "VIEW",
        "-VIEW",
        "VPORTS",
        "VS",
        "VW",
        "WB",
        "WBLOCK",
        "WEBVERSION",
        "XA",
        "XATTACH",
        "XDATA",
        "XR",
        "XREF",
        "-XREF",
        "-ATTACH",
        "XRELOAD",
        "ZOOM",
        "ZS",
    ]
});

#[cfg(test)]
mod marquee_cancel_tests {
    use crate::app::{GripPendingValue, Message, OpenCADStudio};
    use crate::scene::model::object::GripMenuAction;
    use crate::scene::pick::grip::GripEdit;
    use codec::Handle;
    use iced::time::Instant;

    fn fresh() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// Arm a held box drag.
    fn arm_marquee(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let mut sel = app.tabs[i].scene.selection.borrow_mut();
        sel.input.left_down = true;
        sel.input.left_press_pos = Some(iced::Point::new(10.0, 10.0));
        sel.input.left_press_time = Some(Instant::now());
        sel.input.left_dragging = true;
        sel.gesture.box_anchor = Some(iced::Point::new(10.0, 10.0));
        sel.gesture.box_anchor_world = Some(glam::DVec3::new(1.0, 2.0, 0.0));
        sel.gesture.box_current = Some(iced::Point::new(40.0, 40.0));
        sel.gesture.box_crossing = true;
        sel.gesture.box_crossing_locked = true;
    }

    /// Arm a held lasso drag.
    fn arm_lasso(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let mut sel = app.tabs[i].scene.selection.borrow_mut();
        sel.input.left_down = true;
        sel.input.left_press_pos = Some(iced::Point::new(10.0, 10.0));
        sel.input.left_press_time = Some(Instant::now());
        sel.input.left_dragging = true;
        sel.gesture.poly_active = true;
        sel.gesture.poly_points = vec![iced::Point::new(10.0, 10.0), iced::Point::new(20.0, 30.0)];
        sel.gesture.poly_crossing = true;
    }

    #[test]
    fn typing_a_command_cancels_a_half_drawn_marquee() {
        let mut app = fresh();
        arm_marquee(&mut app);
        let i = app.active_tab;

        let _ = app.dispatch_command("LINE");

        let sel = app.tabs[i].scene.selection.borrow();
        assert!(!sel.input.left_down);
        assert!(sel.input.left_press_pos.is_none());
        assert!(sel.input.left_press_time.is_none());
        assert!(!sel.input.left_dragging);
        assert!(sel.gesture.box_anchor.is_none());
        assert!(sel.gesture.box_anchor_world.is_none());
        assert!(sel.gesture.box_current.is_none());
        assert!(!sel.gesture.box_crossing);
        assert!(!sel.gesture.box_crossing_locked);
    }

    #[test]
    fn typing_a_command_cancels_a_held_lasso() {
        let mut app = fresh();
        arm_lasso(&mut app);
        let i = app.active_tab;

        let _ = app.dispatch_command("LINE");

        let sel = app.tabs[i].scene.selection.borrow();
        assert!(!sel.input.left_down);
        assert!(sel.input.left_press_pos.is_none());
        assert!(sel.input.left_press_time.is_none());
        assert!(!sel.input.left_dragging);
        assert!(!sel.gesture.poly_active);
        assert!(sel.gesture.poly_points.is_empty());
        assert!(!sel.gesture.poly_crossing);
    }

    #[test]
    fn an_aliased_command_cancels_it_as_well() {
        let mut app = fresh();
        arm_marquee(&mut app);
        let i = app.active_tab;

        let _ = app.dispatch_command("L");

        assert_eq!(
            app.tabs[i].active_cmd.as_deref().map(|cmd| cmd.name()),
            Some("LINE")
        );
        let sel = app.tabs[i].scene.selection.borrow();
        assert!(sel.gesture.box_anchor.is_none());
        assert!(sel.gesture.box_anchor_world.is_none());
    }

    #[test]
    fn a_transparent_command_leaves_the_marquee_alone() {
        let mut app = fresh();
        arm_marquee(&mut app);
        let i = app.active_tab;

        let _ = app.dispatch_command("ORTHO");

        let sel = app.tabs[i].scene.selection.borrow();
        assert!(sel.input.left_down);
        assert!(sel.input.left_press_pos.is_some());
        assert!(sel.input.left_press_time.is_some());
        assert!(sel.input.left_dragging);
        assert!(sel.gesture.box_anchor.is_some());
        assert!(sel.gesture.box_anchor_world.is_some());
        assert!(sel.gesture.box_current.is_some());
        assert!(sel.gesture.box_crossing);
        assert!(sel.gesture.box_crossing_locked);
    }

    #[test]
    fn starting_a_command_cancels_a_grip_value_edit() {
        let mut app = fresh();
        let i = app.active_tab;
        let handle = Handle::new(42);
        app.tabs[i].active_grip =
            Some(GripEdit::radius(handle, 1, glam::DVec3::new(2.0, 0.0, 0.0)));
        app.grip_pending = Some(GripPendingValue {
            handle,
            grip_id: 1,
            action: GripMenuAction::Radius,
            label: "New radius",
        });
        app.command_line.input = "5".to_string();

        let _ = app.dispatch_command("LINE");

        assert!(app.tabs[i].active_grip.is_none());
        assert!(app.grip_pending.is_none());
        assert!(app.command_line.input.is_empty());
        assert_eq!(
            app.tabs[i].active_cmd.as_deref().map(|cmd| cmd.name()),
            Some("LINE")
        );
    }

    #[test]
    fn command_finalize_repeats_last_cmd() {
        let mut app = fresh();
        let i = app.active_tab;
        assert_eq!(app.tabs[i].last_cmd, None);

        // Run LINE command
        let _ = app.dispatch_command("LINE");
        assert_eq!(app.tabs[i].last_cmd.as_deref(), Some("LINE"));

        // Cancel LINE with Escape
        let _ = app.update(Message::CommandEscape);
        assert!(app.tabs[i].active_cmd.is_none());
        assert_eq!(app.tabs[i].last_cmd.as_deref(), Some("LINE"));

        // Press Enter (CommandFinalize) on empty command line repeats LINE
        let _ = app.update(Message::CommandFinalize);
        assert_eq!(
            app.tabs[i].active_cmd.as_deref().map(|cmd| cmd.name()),
            Some("LINE")
        );
    }

    #[test]
    fn plugin_panel_toggle_by_command_and_title() {
        use ocs_plugin_api::dialog::PanelDef;
        use ocs_plugin_api::host::HostApi;

        let mut app = fresh();
        let i = app.active_tab;

        // Register a panel
        let panel_def = PanelDef::new("sample_palette", "Sample Palette")
            .with_width(360.0)
            .with_command("SAMPLEPALETTE");

        {
            let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
            assert!(host.register_panel(panel_def));
        }

        // Initially panel is registered and not visible
        assert!(!app.plugin_panels.get("sample_palette").unwrap().visible);
        assert!(app.command_line.dynamic_commands.contains(&"SAMPLEPALETTE".to_string()));

        // Dispatch SAMPLEPALETTE command to toggle on
        let _ = app.dispatch_command("SAMPLEPALETTE");
        assert!(app.plugin_panels.get("sample_palette").unwrap().visible);

        // Dispatch SAMPLEPALETTE command to toggle off
        let _ = app.dispatch_command("SAMPLEPALETTE");
        assert!(!app.plugin_panels.get("sample_palette").unwrap().visible);

        // Dispatch with spaces or lowercase "sample palette"
        let _ = app.dispatch_command("sample palette");
        assert!(app.plugin_panels.get("sample_palette").unwrap().visible);

        let _ = app.dispatch_command("samplepalette");
        assert!(!app.plugin_panels.get("sample_palette").unwrap().visible);
    }

    #[test]
    fn plugin_show_dialog_opens_modal() {
        use ocs_plugin_api::dialog::{DialogControl, DialogDef, DialogResponse, DialogRow};
        use ocs_plugin_api::host::HostApi;

        let mut app = fresh();
        let i = app.active_tab;

        let dialog = DialogDef::new("Test Modal").with_row(DialogRow::single(DialogControl::Text {
            id: "name".to_string(),
            label: "Name".to_string(),
            default: "DefaultVal".to_string(),
        }));

        {
            let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
            let resp = host.show_dialog(dialog);
            assert!(matches!(resp, DialogResponse::Cancelled));
        }

        assert!(app.plugin_dialog_state.is_some());
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::PluginDialog)
        );
        assert_eq!(
            app.plugin_dialog_state.as_ref().unwrap().title(),
            "Test Modal"
        );
    }
}

/// `cmd` without its leading `_` / `.` name prefixes (`_LINE`, `.LINE`,
/// `._LINE`), or `None` when it carries none.
fn strip_command_prefixes(cmd: &str) -> Option<&str> {
    let bare = cmd.trim_start_matches(['_', '.']);
    (bare.len() != cmd.len() && !bare.is_empty()).then_some(bare)
}

#[cfg(test)]
mod ribbon_highlight_tests {
    use crate::app::OpenCADStudio;

    fn fresh() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn a_typed_alias_lights_the_button_of_the_command_it_starts() {
        let mut app = fresh();
        let _ = app.dispatch_command_or_suggest("O");
        assert!(app.tabs[app.active_tab].active_cmd.is_some());
        assert_eq!(app.ribbon.active_tool(), Some("OFFSET"));
        let _ = app.dispatch_command_or_suggest("L");
        assert_eq!(app.ribbon.active_tool(), Some("LINE"));
    }

    #[test]
    fn a_transparent_command_keeps_the_running_command_lit() {
        let mut app = fresh();
        let _ = app.dispatch_command_or_suggest("LINE");
        let _ = app.dispatch_command_or_suggest("'ZOOM E");
        assert!(app.tabs[app.active_tab].active_cmd.is_some());
        assert_eq!(app.ribbon.active_tool(), Some("LINE"));
    }

    #[test]
    fn a_one_shot_command_leaves_nothing_lit() {
        // REGEN cancels the running LINE and finishes at once, so neither
        // its own button nor LINE's may stay lit.
        let mut app = fresh();
        let _ = app.dispatch_command_or_suggest("LINE");
        let _ = app.dispatch_command_or_suggest("REGEN");
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        assert_eq!(app.ribbon.active_tool(), None);
    }
}
