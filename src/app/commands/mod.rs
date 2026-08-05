use super::{Message, OpenCADStudio};
use crate::command::CadCommand;
use crate::scene::Scene;
use iced::Task;
use std::path::PathBuf;

mod blocks;
mod dim;
mod display;
mod draw;
mod fileops;
mod inquiry;
mod layerprops;
mod layers;
mod styleprops;
mod view;

// `DrawOrderRefCommand` lives in the `view` family file but is referenced by
// path (`commands::DrawOrderRefCommand`) from `update.rs`, so re-export it at
// the module root to keep that path valid.
pub(crate) use view::DrawOrderRefCommand;

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

    pub(super) fn dispatch_command(&mut self, cmd: &str) -> Task<Message> {
        self.dispatch_command_inner(cmd, false)
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
        self.dispatch_command_inner(cmd, true)
    }

    fn dispatch_command_inner(&mut self, cmd: &str, allow_suggest: bool) -> Task<Message> {
        let i = self.active_tab;
        // Expand a command alias ("L" → "LINE") on the leading token before any
        // routing, so every path below (Start-tab gate, plugins, all dispatch
        // families, the Repeat menu) sees the canonical command. Arguments after
        // the first space are left untouched. A non-alias passes through as-is.
        let resolved = self.resolve_alias(cmd);
        let cmd = resolved.as_deref().unwrap_or(cmd);
        // Starting a command closes any open ribbon dropdown (e.g. a style
        // combo left open) so it does not stay stuck behind the new tool.
        self.ribbon.close_dropdown();
        // Cancel any running command before starting a new one.
        if self.tabs[i].active_cmd.is_some() {
            self.tabs[i].scene.clear_preview_wire();
            self.tabs[i].active_cmd = None;
            // Interrupting an ADDSELECTED draw with another command reverts its
            // template-property override too (#239).
            self.restore_add_selected_defaults();
        }
        // Starting any command leaves interactive navigation modes (their own
        // command arms below re-enable the selected one).
        self.tabs[i].pan_mode = false;
        self.tabs[i].orbit_mode = false;
        // Reset the last committed point so the first click of the new command
        // is not constrained by ortho/polar relative to a previous command's endpoint.
        self.last_point = None;
        // Starting a command restarts the right-click cycle, so its first
        // right-click acts as Enter rather than opening the context menu.
        self.tabs[i]
            .scene
            .selection
            .borrow_mut()
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

        if let Some(path_str) = cmd.strip_prefix("OPEN_RECENT:") {
            let path = PathBuf::from(path_str);
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            return Task::done(Message::OpenPathPicked(Some((path, size))));
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
        if self.tabs[i].is_start && !start_allowed(cmd) {
            self.command_line
                .push_info(crate::t!("No drawing open. Use NEW or OPEN to start a drawing.").as_ref());
            return Task::none();
        }

        if crate::plugin::try_dispatch(self, i, cmd) {
            // try_dispatch returns true for both finished commands and interactive
            // commands that it just installed. If no command is now active, the
            // tool was a one-shot and we must turn the ribbon highlight off here —
            // normally apply_cmd_result does that, but plugin dispatch can return
            // without producing a CmdResult.
            self.command_line.record_recent(cmd);
            if self.tabs[i].active_cmd.is_none() {
                self.ribbon.deactivate_tool();
            }
            return Task::none();
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
            return t;
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
                    return self.dispatch_command_inner(&top, false);
                }
            }
        }
        self.command_line
            .push_error(crate::tf!("Unknown command: {cmd}").as_ref());
        self.finish_dispatch(cmd)
    }

    /// Try each command family in source order, returning the first that
    /// handles `cmd`, or `None` when none match. Each family returns
    /// `Some(task)` for an arm it owns (early-returning or falling through to
    /// `finish_dispatch`), or `None` to defer to the next — equivalent to one
    /// sequential `match` over all arms.
    fn dispatch_families(&mut self, cmd: &str, i: usize) -> Option<Task<Message>> {
        if let Some(t) = self.dispatch_fileops(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_layers(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_blocks(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_draw(cmd, i) {
            return Some(t);
        }
        if let Some(t) = self.dispatch_dim(cmd, i) {
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
    fn finish_dispatch(&mut self, cmd: &str) -> Task<Message> {
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

/// Whether `cmd` makes sense on the Start (welcome) tab — document lifecycle,
/// links, and app-wide configuration; nothing that reads the scene. Single
/// source of truth: the dispatch gate refuses everything else, and the ribbon
/// dims the tools this rejects.
pub fn start_allowed(cmd: &str) -> bool {
    matches!(
        cmd,
        "NEW"
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
        "CLEANSCREEN",
        "CUI",
        "DSETTINGS",
        "GRID",
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
        "PRINTALL",
        "DDIM",
        // Inquiry: list the whole drawing database.
        "DBLIST",
        // Layer management.
        "LAYDEL",
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
        "HIDDENLINE",
        "XRAY",
        "REALISTIC",
        "CONCEPTUAL",
        "2DWIREFRAME",
        "3DWIREFRAME",
        // Raster image brightness / contrast / fade.
        "ADJUST",
        // Block list + block-attribute list (command-line forms).
        "BLOCKPALETTE",
        "BLOCKSPALETTE",
        "ATTMAN",
        "BATTMAN",
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
        // Import CSV into a table + LandXML survey points.
        "DATALINK",
        "LANDXMLIMPORT",
        // Keyboard-shortcut (CUI) export / import.
        "CUIEXPORT",
        "CUIIMPORT",
        "CUILOAD",
        // Save every open drawing.
        "SAVEALL",
        // Draw-order: all text/dims to front or back.
        "TEXTTOFRONT",
        "TEXTTOBACK",
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
        "ARCHIVE",
        "ETRANSMIT",
        "FLATSHOT",
        "CONVTOSURFACE",
        // Slice/section + interference + press-pull/thicken + 3D transforms + wall/pyramid.
        "SLICE",
        "SL",
        "INTERFERE",
        "INF",
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
        "PYRAMID",
        "PYR",
        "SPLINEFIT",
        "FITSPLINE",
        // System variables (typeable directly).
        "MIRRTEXT",
        "ATTREQ",
        "ATTDIA",
        "DIMASSOC",
        "ANGBASE",
        "ANGDIR",
        "REGENMODE",
        "BLIPMODE",
        "SPLFRAME",
        "DELOBJ",
        "PLINEGEN",
        "PSLTSCALE",
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
        "TEXTQLTY",
        "SORTENTS",
        "XCLIPFRAME",
        "HALOGAP",
        "TRACEWID",
        "SKETCHINC",
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
        "DE",
        "DESELALL",
        "DESELECT",
        "DIMSTYLE",
        "DONATE",
        "DRAWORDER",
        "DWGPROP",
        "DWGPROPS",
        "EATTEXT",
        "EXIT",
        "EXPORT",
        "EXPORTSTEP",
        "EXPORTSTL",
        "EXTRIM",
        "FILETAB",
        "FIND",
        "FLATTEN",
        "HELP",
        "HIDEOBJECTS",
        "IM",
        "IMAGE",
        "IMAGEATTACH",
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
        "NAVVCUBE",
        "NEW",
        "OBJIMPORT",
        "OPEN",
        "ORTHO",
        "P",
        "PAN",
        "PAGESETUP",
        "PERF",
        "PERSP",
        "PLOT",
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
        "SHEETSET",
        "SHORTCUTS",
        "SSM",
        "STEPOUT",
        "STLOUT",
        "STPOUT",
        "STYLE",
        "STYLESMANAGER",
        "TABLESTYLE",
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
        "VPORTS",
        "VS",
        "VW",
        "WB",
        "WBLOCK",
        "WEBVERSION",
        "WIREFRAME",
        "XA",
        "XATTACH",
        "XDATA",
        "XR",
        "XREF",
        "XRELOAD",
        "ZOOM",
        "ZS",
    ]
});
