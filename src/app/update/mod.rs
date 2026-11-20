use super::{ArrowKey, Message, OpenCADStudio, TextEntryMode};
use crate::command::CadCommand;
use crate::io::plot_style::PlotStyleTableExt as _;
use crate::scene::VIEWCUBE_DRAW_PX;
use crate::ui::font_picker::FontChoice;
use crate::ui::PropertiesPanel;
use iced::time::Instant;
use iced::Task;

/// Keystroke-derived messages that an open modal dialog must swallow so the
/// keyboard can't reach the main window (command line, F-key toggles, edit
/// shortcuts) while a dialog is up. `CommandEscape` is handled separately (it
/// closes the modal); a modal's own text fields emit their own messages, which
/// are not in this set. See [`OpenCADStudio::update`] and #126.
pub(crate) fn is_modal_blocked_key_msg(msg: &Message) -> bool {
    matches!(
        msg,
        Message::CommandInput(_)
            | Message::CommandPasteMarkup
            | Message::CommandAppendChar(_)
            | Message::CommandSpace
            | Message::CommandFinalize
            | Message::CommandBackspace
            | Message::CommandHistoryPrev
            | Message::CommandHistoryNext
            | Message::ArrowKeyPressed { .. }
            | Message::CommandLineArrowProbe { .. }
            | Message::CommandLineArrowResolved { .. }
            | Message::CommandLineSpaceProbe
            | Message::CommandLineSpaceResolved
            | Message::DynTabNext
            | Message::DynTabPrev
            | Message::MTextCaretMove(_)
            | Message::DeleteSelected
            | Message::ToggleSnapEnabled
            | Message::ToggleSnap3dEnabled
            | Message::ToggleGrid
            | Message::ToggleOrtho
            | Message::ToggleGridSnap
            | Message::TogglePolar
            | Message::ToggleOTrack
            | Message::ToggleDynInput
            | Message::ShortcutPressed(_)
            | Message::TabNew
            | Message::OpenFile
            | Message::SaveFile
            | Message::SaveAs
            | Message::Undo
            | Message::Redo
            | Message::FindReplaceOpen
    )
}

fn perf_message_label(msg: &Message) -> &'static str {
    match msg {
        Message::ViewportLeftPressAt(_)
        | Message::ViewportLeftPress
        | Message::PanePress(_)
        | Message::PanePressAt(..) => "pointer-down",
        Message::ViewportLeftRelease | Message::PaneRelease(_) => "pointer-up",
        Message::ViewportMove(_) | Message::PaneMove(_, _) => "pointer-move",
        Message::CommandFinalize => "command-finalize",
        Message::CommandEscape => "command-escape",
        Message::Undo | Message::UndoMany(_) => "undo",
        Message::Redo | Message::RedoMany(_) => "redo",
        Message::DeleteSelected => "delete",
        Message::HoverDwellTick => "hover-dwell",
        Message::SnapDwellTick => "snap-dwell",
        Message::MoveCoalesceTick => "move-coalesce",
        Message::InteractionIndexReady { .. } => "interaction-index-ready",
        _ => "other",
    }
}

/// Capture only the enum variant for slow PERF "other" messages. A bounded
/// fmt sink aborts before formatting potentially huge payloads (documents,
/// clipboard data, etc.), so diagnostics never clone an entire message.
fn perf_variant_name(msg: &Message) -> String {
    struct VariantPrefix(String);
    impl std::fmt::Write for VariantPrefix {
        fn write_str(&mut self, part: &str) -> std::fmt::Result {
            for character in part.chars() {
                if matches!(character, '(' | '{') || character.is_whitespace() || self.0.len() >= 72
                {
                    return Err(std::fmt::Error);
                }
                self.0.push(character);
            }
            Ok(())
        }
    }
    let mut prefix = VariantPrefix(String::with_capacity(32));
    let _ = std::fmt::write(&mut prefix, format_args!("{msg:?}"));
    prefix.0
}

const VIEWCUBE_HIT_SIZE: f32 = VIEWCUBE_DRAW_PX;

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Resolves once `delay` has passed, for hover delays (the dock's, a drawing
/// tab's layouts list): a sleeping helper thread, or a browser timer on the
/// web build.
pub(in crate::app) fn rest(delay: std::time::Duration) -> impl std::future::Future<Output = ()> {
    let (tx, rx) = iced::futures::channel::oneshot::channel::<()>();
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let _ = tx.send(());
    });
    #[cfg(target_arch = "wasm32")]
    if let Some(window) = web_sys::window() {
        let fire = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = tx.send(());
        });
        let fire = js_sys::Function::from(fire);
        let ms = i32::try_from(delay.as_millis()).unwrap_or(i32::MAX);
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments(
            fire.as_ref(),
            ms,
            &js_sys::Array::new(),
        );
    }
    // A dropped sender (no window, failed timer) resolves at once.
    async move {
        let _ = rx.await;
    }
}

fn reorder_insertion_index(from: usize, to: usize, after: bool, len: usize) -> Option<usize> {
    if from >= len || to >= len || from == to {
        return None;
    }
    let mut insertion = to + usize::from(after);
    if from < insertion {
        insertion -= 1;
    }
    (insertion != from).then_some(insertion)
}

#[cfg(test)]
mod block_ops_tests;
pub(super) mod blocks_palette;
pub(in crate::app) mod command;
pub(crate) mod context_menu;
mod dialog;
mod dynamic;
mod file;
mod graphic_attributes;
mod keytips;
pub(in crate::app) mod linetype_manager;
pub(in crate::app) mod mleader_style;
mod dimstyle_dialog;
pub(super) mod tool_palettes;
pub(in crate::app) use file::background_task;
#[cfg(not(target_arch = "wasm32"))]
pub(in crate::app) use file::native_paths_match;
#[cfg(not(target_arch = "wasm32"))]
pub(in crate::app) use file::panic_detail;
mod page_setup_import;
#[cfg(not(target_arch = "wasm32"))]
mod plugin_grips;
mod prop_commit;
#[cfg(not(target_arch = "wasm32"))]
pub(in crate::app) use plugin_grips::SETTLE_WATCHDOG;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod plugin_grips_tests;
mod qselect;
mod style;
pub(in crate::app) mod table_style_cells;
pub(crate) mod style_import;
pub(in crate::app) mod util;
mod viewport;
pub(in crate::app) mod viewport_snap;
#[cfg(test)]
mod xref_manager_preview_tests;

impl OpenCADStudio {
    pub(in crate::app) fn reset_modal_geometry(&mut self) {
        self.modal_offset = iced::Vector::ZERO;
        self.modal_resize = iced::Vector::ZERO;
        self.modal_content_size = None;
        self.modal_drag_last = None;
        self.modal_dragging = false;
        self.modal_resizing = false;
    }

    /// Refill the Command History pallet's log (it shows the same text the
    /// expanded command line does).
    pub(in crate::app) fn sync_command_history_panel(&mut self) {
        let latest = self.command_line.history_plain_text();
        if self.history_content.text() != latest {
            self.history_content = iced::widget::text_editor::Content::with_text(&latest);
        }
    }

    fn sync_open_command_history(&mut self) {
        if !self.command_line.history_open && !self.show_command_history {
            return;
        }
        let latest = self.command_line.history_plain_text();
        if self.history_content.text() == latest {
            return;
        }
        self.history_content = iced::widget::text_editor::Content::with_text(&latest);
        // The outer scrollable is anchored to the newest lines. Leaving the
        // editor cursor at its initial position also keeps horizontal scroll at
        // zero, so the first glyph of each line cannot be clipped.
    }

    /// Close the active in-canvas modal (Plan B), mirroring what closing the
    /// old OS window did: a style editor discards its staged (un-applied)
    /// changes, and the ribbon tool that launched the dialog is de-highlighted.
    pub(in crate::app) fn close_active_modal(&mut self) {
        self.mark_startup_modal_shown();
        use super::ModalKind::*;
        if self.active_modal == Some(IfccadAlphaWarning) {
            self.pending_ifccad_alpha_save = None;
            self.pending_conversion_save = None;
            self.conversion_losses.clear();
            self.conversion_loss_allowed = false;
            self.pending_close = None;
            self.save_dialog_for_unsaved = false;
        }
        if self.active_modal == Some(BlockEditorSettings) {
            self.block_editor_settings = None;
        }
        if self.active_modal == Some(VisualStyles) {
            self.visual_styles_crease = None;
        }
        if self.active_modal == Some(BlockCycleOrder) {
            self.block_cycle_order = None;
        }
        // Plot Style opened from PLOT behaves as a child modal.
        // Closing it restores the parent Plot dialog instead of returning
        // to the drawing.
        // Options opened from a PDF dialog returns to it.
        if self.active_modal == Some(Options) {
            if let Some(parent) = self.options_parent.take() {
                self.options_saved = None;
                self.options_close_confirm = false;
                self.active_modal = Some(parent);
                return;
            }
        }
        // The Cell Data Format window and the Text Style manager opened from
        // the table style window return to it.
        if self.active_modal == Some(TableCellFormat) && self.ts_format_return {
            self.table_style_format_back(None);
            return;
        }
        if self.active_modal == Some(TextStyle) && self.ts_text_return {
            self.table_style_back_from_text_styles();
            return;
        }
        // The Text Style manager opened from the multileader style window
        // returns to it.
        if self.active_modal == Some(TextStyle) && self.style_parent_stage.is_some() {
            if self.dsd.edit.is_some() {
                self.dim_style_back_from_text_styles();
            } else {
                self.mleader_style_back_from_text_styles();
            }
            return;
        }
        // The multileader and dimension style windows keep what they changed
        // as one step.
        if self.active_modal == Some(MLeaderStyle) {
            self.mleader_style_close();
        }
        if self.active_modal == Some(DimStyle) {
            self.dim_style_close();
        }
        if self.active_modal == Some(TableStyle)
            && std::mem::take(&mut self.tablestyle_parent_table)
        {
            self.return_to_table_insert();
            return;
        }
        if self.active_modal == Some(Plotstyle) {
            if let Some(baseline) = self.plotstyle_panel_baseline.take() {
                self.active_plot_style = baseline;
            }
            if let Some((plot_offset, plot_resize)) = self.plotstyle_parent_plot_geometry.take() {
                self.active_modal = Some(Plot);

                self.reset_modal_geometry();
                self.modal_offset = plot_offset;
                self.modal_resize = plot_resize;

                return;
            }
        }
        if self.active_modal == Some(Plot) && self.print_all_options {
            if let Some(previous) = self.print_all_options_prev.take() {
                self.plot_dialog = previous;
            }
            if let Some(previous) = self.print_all_plot_style_prev.take() {
                self.active_plot_style = previous;
            }
            if let Some(previous) = self.print_all_plot_window_prev.take() {
                self.plot_window = previous;
            }
            if let Some(previous) = self.print_all_plot_setup_prev.take() {
                self.plot_setup_template = previous;
            }
            self.print_all_options = false;
            self.active_modal = Some(PrintAll);
            self.reset_modal_geometry();
            return;
        }
        // The text style window keeps what it did (New, Delete, Rename, Set
        // Current, Apply) as one undo step; what Apply did not write was
        // answered for before closing.
        if self.active_modal == Some(TextStyle) {
            self.textstyle_card = None;
            self.style_stage_commit();
            self.style_stage = None;
        }
        if matches!(
            self.active_modal,
            Some(TextStyle | DimStyle | TableStyle | MLeaderStyle | MlStyle | LinetypeManager)
        ) {
            self.style_stage_discard();
        }
        if self.active_modal == Some(StyleImport) {
            self.style_import_cancel();
            return;
        }
        if self.active_modal == Some(PointStyle) {
            self.point_style_tab_id = None;
            self.point_mode_buf = None;
        }
        if self.active_modal == Some(ScaleManager) {
            self.scale_stage_discard();
        }
        if self.active_modal == Some(DraftingSettings) {
            self.snap_popup_open = false;
            self.drafting_settings_close_confirm = false;
            self.drafting_settings_state = None;
            self.drafting_settings_saved = None;
        }
        if self.active_modal == Some(Options) {
            self.options_saved = None;
            self.options_close_confirm = false;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.active_modal == Some(FileInUse) {
            self.pending_save_failure = None;
            self.pending_close = None;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.active_modal == Some(ExternalChange) {
            self.pending_external_change = None;
            self.pending_close = None;
        }
        match self.active_modal {
            // Dismissing these via ✕ is the cancel/decline path.
            Some(Unsaved) => self.pending_close = None,
            Some(BlockEditChanges) => self.cancel_block_edit_close(),
            Some(AssocPrompt) => self.mark_assoc_prompted(),
            // Cancel: leave the layer (and its objects) untouched.
            Some(LayerDeleteWarning) => self.layer_delete_pending = None,
            // ✕ is Cancel: what was not applied is taken back.
            Some(AttributeEditor) => self.attr_editor_end(false),
            Some(GeometricTolerance) => self.geometric_tolerance = None,
            Some(BlockDefinition) => self.block_definition = None,
            Some(AttMan) => self.attman = None,
            Some(BlockEditPicker) => self.block_edit_picker = None,
            Some(Rename) => self.rename_dialog = None,
            Some(Purge) => self.purge_dialog = None,
            Some(DrawingProperties) => self.drawing_properties = None,
            Some(GeographicLocation) => self.geographic_location = None,
            Some(SelectSimilarSettings) => self.select_similar_settings = None,
            Some(BlockSaveAs) => self.block_save_as = None,
            Some(BlockVisibility) => self.block_visibility = None,
            Some(BlockLookup) => self.block_lookup = None,
            Some(BlockPropertiesTable) => self.block_properties_table = None,
            Some(PdfAttach) => self.pdf_attach = None,
            Some(UnderlayLayers) => self.underlay_layers = None,
            Some(PdfImportSettings) => self.pdf_import_settings = None,
            // OK or Cancel: TXT2MTXT asks for its selection again.
            Some(Txt2MtxtSettings) => {
                self.txt2mtxt_dialog = None;
                if let Some(prompt) = self.tabs[self.active_tab]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.prompt())
                {
                    self.command_line.push_info(&prompt);
                }
            }
            // OK or Cancel: MATCHPROP asks for its destinations again.
            Some(MatchPropSettings) => {
                self.matchprop_dialog = None;
                if let Some(prompt) = self.tabs[self.active_tab]
                    .active_cmd
                    .as_ref()
                    .map(|command| command.prompt())
                {
                    self.command_line.push_info(&prompt);
                }
            }
            Some(PdfImportFile) => self.pdf_import_file = None,
            Some(ImportFile) => self.import_dialogs.file = None,
            Some(DgnImport) => self.import_dialogs.dgn = None,
            Some(TdsImport) => self.import_dialogs.tds = None,
            // Cancel and ✕ stop the import at this mesh in
            // `Message::CloseModal`; closed for another window, the import
            // is dropped.
            Some(TdsMaterial) => self.import_dialogs.tds_material = None,
            // OK or ✕ dismisses this map; the next one follows.
            // Cancel or ✕ leaves the cells as they were.
            Some(TableCellWarning) => self.table_cell_warning = None,
            Some(AnnotationScalePrompt) => self.anno_scale_prompt = None,
            // Esc or ✕ on the text style question changes the style, as Yes.
            Some(MTextDialog) => {
                if let Some(dialog) = self.mtext_dialog.take() {
                    self.mtext_style_question_closed(dialog);
                }
            }
            // Cancel drops the typed value; the cell keeps its value and type.
            Some(TableCellEntry) => self.table_cell_entry = None,
            Some(HatchBoundaryWarning) => self.close_hatch_boundary_warning(),
            Some(TdsMissingMap) => {
                if !self.import_dialogs.tds_maps.is_empty() {
                    self.import_dialogs.tds_maps.remove(0);
                }
            }
            Some(XrefAttach) => self.xref_attach = None,
            Some(WriteBlock) => self.wblock = None,
            Some(Hyperlink) => {
                self.hyperlink_editor_handles.clear();
                self.hyperlink_editor_block = None;
                self.hyperlink_editor_url.clear();
                self.hyperlink_editor_description.clear();
                self.hyperlink_editor_mixed = false;
                self.hyperlink_editor_dirty = false;
                self.hyperlink_options = Default::default();
            }
            // Closing (✕) discards edits made since the last Apply — matching the
            // style editors. Committing happens only through the Apply button.
            Some(Aliases) => {
                self.alias_editor_rows.clear();
                self.alias_pending_add = false;
                self.alias_reset_confirm = false;
                self.alias_close_confirm = false;
            }
            Some(Shortcuts) => {
                self.shortcut_editor_rows.clear();
                self.shortcut_capture_row = None;
                self.shortcut_pending_add = false;
                self.shortcut_reset_confirm = false;
                self.shortcut_close_confirm = false;
            }
            Some(LayerStateEditor) => {
                self.layer_state_edit_draft = None;
                self.layer_state_edit_filter.clear();
                self.layer_state_edit_color_open = None;
            }
            Some(PluginDialog) => self.plugin_dialog_state = None,
            Some(Recovery) => self.recovery_report = None,
            _ => {}
        }
        // The tool that opened this dialog is done with it now. Keep the
        // highlight only while an interactive command still runs (it owns
        // it). Replaces the old per-modal deactivate_tool_if list, which
        // missed every newly added dialog (CUI, Plugin Manager, Point
        // Style, Attribute Editor…). (#355)
        if self.tabs[self.active_tab].active_cmd.is_none() {
            self.ribbon.deactivate_tool();
        }
        // A child of Options closes back to Options.
        let child = self.options_child.take();
        self.active_modal = match child.is_some() && child == self.active_modal {
            true => Some(Options),
            false => None,
        };
        // Recentre / reset the size of the next dialog and drop any drag.
        self.reset_modal_geometry();
        // A 3D Studio import that just ran (or is stopped from its
        // material window) reports each texture map it did not find.
        if !self.import_dialogs.tds_maps.is_empty() {
            self.active_modal = Some(TdsMissingMap);
        }
    }

    /// Fire the focus-sweep once when a property field is active, so a
    /// click-away that moved focus off the field (viewport, toolbar, command
    /// line) clears the active-row highlight. Idle (`Task::none()`) unless a
    /// field is active, and the sweep itself keeps the marker whenever the
    /// active field still holds focus — re-firing it on unrelated messages is
    /// cheap and safe.
    fn sync_active_field_if_any(&self) -> Task<Message> {
        if self.tabs[self.active_tab].properties.active_field.is_some() {
            crate::ui::properties::sync_active_field_task()
        } else {
            Task::none()
        }
    }

    /// Emit `SelectionChangedV4` to V4 plugins when the active tab's selection
    /// set actually changed since the last broadcast.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn notify_plugins_selection_changed(&mut self) {
        if self.active_tab >= self.tabs.len() {
            return;
        }
        let i = self.active_tab;
        let tab_id = self.tabs[i].id;
        let fingerprint = self.tabs[i].scene.selection_fingerprint();
        let key = (tab_id, fingerprint);
        if self.last_plugin_selection == Some(key) {
            return;
        }
        self.last_plugin_selection = Some(key);
        let revision = self.bump_selection_revision(tab_id);
        let handles = self.tabs[i].scene.selected_handles_in_order();
        // Per-endpoint API routing (v22): digest to v22+ runners, the exact
        // legacy streams (v4..v21 handle list; plain below v4) to older ones.
        crate::plugin::v4_support::publish_selection_changed_versioned(
            tab_id,
            handles.clone(),
            revision,
            true,
        );
        self.control_record_selection_event(&handles);
    }

    /// Next monotonic selection revision for `tab_id`, used by the v22
    /// `SelectionChangedDigest` so workers can order/dedup results.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn bump_selection_revision(&mut self, tab_id: u64) -> u64 {
        let entry = self.selection_revision_by_tab.entry(tab_id).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Refresh the shared V4 document after built-in geometry edits. Plugin
    /// writes publish through HostSession immediately; the fingerprint avoids
    /// sending a duplicate notification at this message boundary.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn notify_plugins_document_changed(&mut self) {
        if self.active_tab >= self.tabs.len() {
            return;
        }
        let tab = &self.tabs[self.active_tab];
        let key = (tab.id, tab.scene.geometry_epoch);
        if self.last_plugin_document == Some(key) {
            return;
        }
        self.last_plugin_document = Some(key);
        crate::plugin::v4_support::publish_drawing_changed(tab.id, tab.scene.geometry_epoch);
        if let Err(e) =
            crate::plugin::v4_support::publish_document_view_v4(tab.id, &tab.scene.document)
        {
            // P1-18: a failed publication is logged, not ignored — the epoch
            // still reaches native plugins, but the snapshot side named its
            // failure on stderr instead of silently going stale.
            eprintln!("[host] V4 document view failed to publish: {e}");
        }
        // Same guarded boundary, second surface (API v9): the plain
        // `DocumentChanged` carries the settled geometry epoch to every
        // loaded native plugin through `BuiltinPlugin::on_notification`.
        crate::plugin::v4_support::publish_document_changed(tab.scene.geometry_epoch);
        // API v13 dirty regions: plugin writes publish their sets at their own
        // boundary, so an epoch change reaching this one came from elsewhere
        // and the sets say "walk everything" (empty).
        let tab = &mut self.tabs[self.active_tab];
        tab.plugin_dirty_overflow = true;
        let (entities, records) = tab.take_plugin_dirty();
        crate::plugin::v4_support::publish_document_changed_regions(
            tab.id,
            tab.scene.geometry_epoch,
            entities,
            records,
        );
    }

    /// Announce that a whole document became resident in the active tab —
    /// opened from file, created from a template, or loaded by a system open
    /// (API v9). Each tab carries its own pending flag, set only when its
    /// content is wholesale replaced, so neither an edit nor a tab switch can
    /// fire it twice and a reload fires it exactly once. Part of the same
    /// post-message boundary as the selection and document dispatches: runs
    /// after the mutation settles and pending history scopes closed, never
    /// inside history replay or interactive picking.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn notify_plugins_document_opened(&mut self) {
        if self.active_tab >= self.tabs.len() {
            return;
        }
        let i = self.active_tab;
        if self.tabs[i].is_start {
            return;
        }
        if !std::mem::take(&mut self.tabs[i].document_opened_pending) {
            return;
        }
        crate::plugin::v4_support::publish_document_opened(self.tabs[i].id);
    }

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        crate::sys::startup_times::update_tick();
        // Whatever makes another document active (switching, opening,
        // creating or closing a tab), the contextual ribbon tab follows that
        // document's selection.
        let before = self.tabs.get(self.active_tab).map(|tab| tab.id);
        // A cursor position does not change drawing-wide ribbon variables.
        // Keep FRAME/UOSNAP/ATTMODE sync for edits, input and tab changes,
        // not for every mouse packet in a dense scene.
        let pointer_motion = matches!(
            &msg,
            Message::ViewportMove(_) | Message::PaneMove(_, _) | Message::CursorMoved(..)
        );
        // A click in the drawing or typing a command hands the keys back
        // from the Layer manager's table.
        if matches!(
            &msg,
            Message::ViewportLeftPressAt(_)
                | Message::ViewportLeftPress
                | Message::PanePress(_)
                | Message::PanePressAt(..)
                | Message::CommandInput(_)
                | Message::CommandAppendChar(_)
        ) {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                tab.layers.keys_active = false;
            }
        }
        // The attribute editor checks a field as the cursor leaves it.
        if let Some(task) = self.attr_editor_leave_check(&msg) {
            return task;
        }
        // Pickers offer the catalog linetypes the drawing lacks; the one
        // picked is loaded before it is assigned.
        if let Message::LayerLinetypeSet(name)
        | Message::LayerStateEditorLayerLinetype(_, name)
        | Message::RibbonLinetypeChanged(name)
        | Message::PropLinetypeChanged(name)
        | Message::AttrEditorLinetype(name) = &msg
        {
            if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                if crate::io::linetypes::ensure_catalog_linetype(&mut tab.scene.document, name) {
                    tab.scene
                        .invalidate_linetype_dependencies(std::slice::from_ref(name));
                }
            }
        }
        let from_options = self.active_modal == Some(super::ModalKind::Options);
        // The viewport chip's outline switch can vanish under a resting
        // pointer (another document or layout, a viewport count change, a
        // narrower window); a removed mouse_area sends no exit, so its hover
        // ends here.
        let outline_switch_before = self.outline_switch_hover.then(|| {
            (
                matches!(
                    &msg,
                    Message::WindowResized(..) | Message::OsWindowResized(..)
                ),
                self.outline_switch_key(),
            )
        });
        let mut task = self.update_message(msg);
        // A selection or a refresh in the Xref Manager asked for a preview:
        // it decodes on a worker.
        if let Some(request) = self.xref_manager.take_preview_request() {
            let read = self.read_xref_manager_preview_with(
                request,
                crate::ui::window::xref_manager::read_preview,
            );
            task = Task::batch([task, read]);
        }
        if let Some((resized, key)) = outline_switch_before {
            if resized || self.outline_switch_key() != key {
                self.outline_switch_hover = false;
            }
        }
        self.refresh_layer_counts();
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let notices = std::mem::take(&mut tab.scene.center_notices);
            for notice in notices {
                self.command_line.push_output(&notice);
            }
        }
        // A window opened from Options (shortcuts, aliases, a colour, units,
        // plot, plugins) is its child: closing it comes back to Options.
        if from_options
            && self.active_modal.is_some()
            && self.active_modal != Some(super::ModalKind::Options)
        {
            self.options_child = self.active_modal;
        } else if self.options_child.is_some() && self.active_modal != self.options_child {
            // It closed some other way (OK, Apply): Options comes back too.
            self.options_child = None;
            if self.active_modal.is_none() {
                self.active_modal = Some(super::ModalKind::Options);
                self.reset_modal_geometry();
            }
        }
        // What a message changed is only known while it is handled, also when
        // it returned early.
        self.block_palette.change_hint = Default::default();
        // A dock change leaves the drawing where it was on screen, and the
        // viewport's size follows dock and window changes at once.
        self.dock_keep_view();
        // Thumbnails the block palette asked for are read in the background.
        task = Task::batch([task, self.palette_thumbnail_task()]);
        // A script waiting on an open or a save goes on once it is done,
        // however that open or save ended.
        if self.script_paused && !self.script_busy() {
            self.script_paused = false;
            task = Task::batch([task, Task::done(Message::ScriptPump)]);
        }
        self.restore_dynamic_ucs_after_command();
        self.sync_block_visibility_display();
        if self.tabs.get(self.active_tab).map(|tab| tab.id) != before {
            self.sync_underlay_tab();
        }
        if !pointer_motion {
            self.sync_frame_dropdown();
        }
        let block_editing = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| tab.active_block_edit.is_some());
        let testing = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| tab.block_test.is_some());
        if testing {
            self.ribbon.set_block_editing(false);
            self.ribbon.set_block_testing(true);
        } else {
            self.ribbon.set_block_testing(false);
            self.ribbon.set_block_editing(block_editing);
        }
        let cells_picked = !testing
            && !block_editing
            && self
                .tabs
                .get(self.active_tab)
                .is_some_and(|tab| tab.scene.table_cells.is_some());
        self.ribbon.set_table_cell_context(cells_picked);
        // The Text Editor tab follows the in-place text editor.
        if !pointer_motion {
            self.sync_text_editor_ribbon();
        }
        // The ribbon light follows the pane that is actually rendered, so a
        // session ending, a clean screen or another tab closes the light with it.
        self.ribbon.show_parameters_manager =
            self.dock_panel_visible(crate::ui::dock::PanelId::ParametersManager);
        self.ribbon.show_authoring_palettes =
            self.dock_panel_visible(crate::ui::dock::PanelId::AuthoringPalettes);
        self.ribbon.show_tool_palettes =
            self.dock_panel_visible(crate::ui::dock::PanelId::ToolPalettes);
        self.ribbon.show_viewport_highlight = self.model_space.viewport_hover_highlight;
        task
    }

    /// What decides whether the viewport chip with its outline switch is on
    /// screen: the document, its layout and the number of model viewports.
    fn outline_switch_key(&self) -> Option<(u64, String, usize)> {
        self.tabs.get(self.active_tab).map(|tab| {
            (
                tab.id,
                tab.scene.current_layout.clone(),
                tab.scene.model_tiles.borrow().len(),
            )
        })
    }

    /// A dynamic UCS lasts for the command that set it: once that tab's
    /// command has ended, the UCS it replaced comes back.
    fn restore_dynamic_ucs_after_command(&mut self) {
        let Some((tab_id, _)) = self.dynamic_ucs_restore else {
            return;
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            self.dynamic_ucs_restore = None;
            return;
        };
        if self.tabs[i].active_cmd.is_some() {
            return;
        }
        if let Some((_, saved)) = self.dynamic_ucs_restore.take() {
            self.tabs[i].active_ucs = saved;
            self.tabs[i].sync_ucs_to_scene();
        }
    }

    /// The Frames list on the ribbon shows the active drawing's frame state
    /// (FRAME), however it was changed: the list, a command, undo or another
    /// drawing.
    fn sync_frame_dropdown(&mut self) {
        let Some(tab) = self.tabs.get(self.active_tab).filter(|tab| !tab.is_start) else {
            return;
        };
        let current = match crate::scene::frame::master_mode(&tab.scene.document) {
            0 => "FRAMES0",
            1 => "FRAMES1",
            2 => "FRAMES2",
            _ => "FRAMES3",
        };
        self.ribbon.set_dropdown_current("FRAMES_DROPDOWN", current);
        let uosnap = match crate::scene::model::pdf_vector::uosnap() {
            0 => "UOSNAP0",
            1 => "UOSNAP1",
            _ => "UOSNAP2",
        };
        self.ribbon.set_dropdown_current("UOSNAP_DROPDOWN", uosnap);
        let attmode = match tab.scene.document.header.attribute_visibility {
            0 => "ATTMODE0",
            2 => "ATTMODE2",
            _ => "ATTMODE1",
        };
        self.ribbon
            .set_dropdown_current("ATTMODE_DROPDOWN", attmode);
        // Annotate > Text: TEXTSIZE (unless it is being typed in) and
        // TEXTLAYER.
        if !self.ribbon.text_height_editing {
            self.ribbon.text_height =
                crate::modules::annotate::text_height_label(tab.scene.document.header.text_height);
        }
        self.ribbon.text_layer =
            crate::io::drawing_variable(&tab.scene.document, "TEXTLAYER").unwrap_or_default();
        self.ribbon.dim_layer = crate::io::drawing_variable(&tab.scene.document, "DIMLAYER")
            .filter(|layer| layer.trim() != ".")
            .unwrap_or_default();
        self.ribbon.mleader_layer =
            crate::io::drawing_variable(&tab.scene.document, "MLEADERLAYER")
                .filter(|layer| layer.trim() != ".")
                .unwrap_or_default();
        self.ribbon.center_layer = tab.scene.centerline_settings().layer;
        if self.ribbon.center_layer.trim() == "." {
            self.ribbon.center_layer.clear();
        }
    }

    fn update_message(&mut self, msg: Message) -> Task<Message> {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            crate::entities::common::set_unit_context(
                crate::entities::common::UnitContext::from_header(&tab.scene.document.header),
            );
        }
        self.control_observe_user_message(&msg);
        let perf_started = crate::perf::enabled().then(Instant::now);
        let perf_label = perf_message_label(&msg);
        let perf_other_variant =
            (perf_started.is_some() && perf_label == "other").then(|| perf_variant_name(&msg));
        // Keep the stable tab id because dispatch may switch or close tabs.
        let perf_edit_before = perf_started.map(|started| {
            let tab = &self.tabs[self.active_tab];
            (started, tab.id, tab.scene.geometry_epoch)
        });
        // The Select Color panel can open on top of another dialog, so it
        // takes the keyboard first: Esc cancels, Enter confirms, the arrows
        // walk the index palette, and every other keystroke is swallowed.
        if self.color_picker.is_some() {
            use crate::ui::color_select::ColorPickerMsg;
            match &msg {
                Message::CommandEscape => {
                    return self.update(Message::ColorPicker(ColorPickerMsg::Cancel));
                }
                Message::CommandFinalize => {
                    return self.update(Message::ColorPicker(ColorPickerMsg::Ok));
                }
                Message::ShortcutPressed(key) => match key.rsplit('+').next() {
                    Some("ESCAPE") => {
                        return self.update(Message::ColorPicker(ColorPickerMsg::Cancel));
                    }
                    Some("ENTER" | "RETURN") => {
                        return self.update(Message::ColorPicker(ColorPickerMsg::Ok));
                    }
                    Some("TAB") => {
                        return crate::ui::color_select::picker::focus_step(
                            !key.contains("SHIFT+"),
                        );
                    }
                    _ => {}
                },
                Message::ArrowKeyPressed { direction, .. } => {
                    return self.update(Message::ColorPicker(ColorPickerMsg::Arrow(*direction)));
                }
                _ => {}
            }
            if is_modal_blocked_key_msg(&msg) {
                return Task::none();
            }
        }
        // A modal dialog must capture the keyboard the same way it already
        // captures the mouse. Otherwise keystrokes from the global key
        // subscription leak past the modal into the command line and fire as
        // commands once the dialog closes. While a modal is open, Escape
        // closes it and every other keystroke-derived message is swallowed;
        // the modal's own text fields keep working because they emit their own
        // (non-blocked) messages. (#126)
        if self.active_modal == Some(super::ModalKind::LinetypeManager) {
            let step = match &msg {
                Message::ShortcutPressed(key) if key == "ARROWUP" => Some(false),
                Message::ShortcutPressed(key) if key == "ARROWDOWN" => Some(true),
                Message::ArrowKeyPressed {
                    direction: ArrowKey::Up,
                    extend_selection: false,
                    ..
                } => Some(false),
                Message::ArrowKeyPressed {
                    direction: ArrowKey::Down,
                    extend_selection: false,
                    ..
                } => Some(true),
                _ => None,
            };
            if let Some(down) = step {
                return self.linetype_list_step(down);
            }
        }
        if self.active_modal.is_some() {
            if matches!(msg, Message::CommandEscape)
                || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ESCAPE"))
            {
                // Esc backs out of a pending ALIASEDIT draft first, mirroring
                // the shortcut editor's capture cancel; the next Esc closes.
                if self.active_modal == Some(super::ModalKind::Aliases) && self.alias_pending_add {
                    return self.update(Message::AliasEditorDraftCancel);
                }
                if self.active_modal == Some(super::ModalKind::BlockDefinition) {
                    if let Some(ref mut state) = self.block_definition {
                        if state.confirm_redefine.is_some() {
                            state.confirm_redefine = None;
                            return Task::none();
                        }
                        if state.error_message.is_some() {
                            state.error_message = None;
                            return Task::none();
                        }
                    }
                }
                if self.active_modal == Some(super::ModalKind::WriteBlock) {
                    if let Some(ref mut state) = self.wblock {
                        if state.error_message.is_some() {
                            state.error_message = None;
                            return Task::none();
                        }
                    }
                }
                return self.update(Message::CloseModal);
            }
            if self.active_modal == Some(super::ModalKind::BlockDefinition)
                && (matches!(msg, Message::CommandFinalize)
                    || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ENTER") || key.rsplit('+').next() == Some("RETURN")))
            {
                if let Some(ref state) = self.block_definition {
                    if state.confirm_redefine.is_some() {
                        // Default to "No" so an accidental Enter doesn't nuke a definition.
                        return self.update(Message::BlockDefConfirmRedefine(false));
                    } else {
                        return self.update(Message::BlockDefApply);
                    }
                }
            }
            // Enter in the unsaved-block question saves and closes.
            if self.active_modal == Some(super::ModalKind::BlockEditChanges)
                && (matches!(msg, Message::CommandFinalize)
                    || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ENTER") || key.rsplit('+').next() == Some("RETURN")))
            {
                return self.update(Message::BlockEditChangesSave);
            }
            if self.active_modal == Some(super::ModalKind::BlockEditPicker)
                && (matches!(msg, Message::CommandFinalize)
                    || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ENTER") || key.rsplit('+').next() == Some("RETURN")))
            {
                return self.apply_bedit_dialog();
            }
            if self.active_modal == Some(super::ModalKind::BlockSaveAs)
                && (matches!(msg, Message::CommandFinalize)
                    || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ENTER") || key.rsplit('+').next() == Some("RETURN")))
            {
                return self.apply_block_save_as_dialog();
            }
            if self.active_modal == Some(super::ModalKind::InsertAttributes)
                && (matches!(msg, Message::CommandFinalize)
                    || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ENTER") || key.rsplit('+').next() == Some("RETURN")))
            {
                return self.update(Message::InsertAttrDialog(
                    crate::ui::window::insert_attr_dialog::InsertAttrMsg::Ok,
                ));
            }
            if self.active_modal == Some(super::ModalKind::WriteBlock)
                && (matches!(msg, Message::CommandFinalize)
                    || matches!(&msg, Message::ShortcutPressed(key) if key.rsplit('+').next() == Some("ENTER") || key.rsplit('+').next() == Some("RETURN")))
            {
                return self.update(Message::WblockApply);
            }
            if is_modal_blocked_key_msg(&msg) {
                return Task::none();
            }
        }
        // The Shift+right-click snap menu owns the keyboard too: a snap's
        // letter picks it, and no other key reaches the command line.
        if self.snap_override_popup.is_some() {
            if let Some(task) = self.intercept_snap_override_key(&msg) {
                return task;
            }
        }
        // Tab in a Properties field applies it and moves to the next field
        // (Shift+Tab: the previous one) instead of cycling Dynamic Input.
        if let Message::ShortcutPressed(key) = &msg {
            if matches!(key.as_str(), "TAB" | "SHIFT+TAB") {
                if let Some(task) = self.prop_tab(key == "TAB") {
                    return task;
                }
            }
        }
        // After a click in the Layer manager's table, ↑/↓ move through its
        // rows (#2610).
        if let Message::ArrowKeyPressed {
            direction: direction @ (crate::app::ArrowKey::Up | crate::app::ArrowKey::Down),
            extend_selection,
            ..
        } = &msg
        {
            let down = matches!(direction, crate::app::ArrowKey::Down);
            if self.layer_manager_arrow(down, *extend_selection) {
                return Task::none();
            }
        }
        // With Dynamic Input on, Down opens the running command's options at
        // the cursor, the first one highlighted; arrows and Enter then drive
        // that menu.
        if let Message::ArrowKeyPressed {
            direction: crate::app::ArrowKey::Down,
            ..
        }
        | Message::CommandLineArrowProbe {
            direction: crate::app::ArrowKey::Down,
            ..
        } = &msg
        {
            if !self.context_menu_open() {
                if let Some(task) = self.open_dynamic_options() {
                    return task;
                }
            }
        }
        // The open right-click context menu owns the keyboard the same way:
        // arrows / Enter / mnemonic letters drive it, any other key closes it
        // and falls through to the command line (the behaviour of commercial solutions).
        if self.context_menu_open() {
            if let Some(task) = self.intercept_context_menu_key(&msg) {
                return task;
            }
        }
        // The open quad cursor menu owns the keyboard the same way: Escape
        // closes it, Enter picks the primary item, Tab cycles its panels and
        // the digits 1–9 pick them; any other key closes it and falls
        // through to the command line.
        if self.quad_open() {
            if let Some(task) = self.intercept_quad_key(&msg) {
                return task;
            }
        }
        // An open property list owns Escape, Enter and the arrows: without
        // this they reach the command line or the viewport and dismiss the
        // selection the list was opened for. A list whose panel is hidden or
        // tucked away — collapsed on an auto-hiding edge, behind another
        // group's tab, collapsed while floating — is not on screen, so it
        // must not keep the guard on.
        let panel = &self.tabs[self.active_tab].properties;
        let shown = |quick: bool| {
            if quick {
                self.quick_properties
            } else {
                let id = crate::ui::dock::PanelId::Properties;
                self.dock_panel_visible(id) && self.dock_panel_on_screen(id)
            }
        };
        let picker_shown = panel
            .open_block_picker
            .is_some_and(|(_, quick)| shown(quick))
            || panel.open_choice.is_some_and(|(_, quick)| shown(quick));
        if picker_shown || self.block_picker_dismissed {
            if let Some(task) = self.intercept_property_list_key(&msg) {
                return task;
            }
        }
        // A command's name is `&'static str`, so this runs on every message —
        // a mouse move included — without allocating.
        #[cfg(not(target_arch = "wasm32"))]
        let active_command = |app: &Self| -> Option<(u64, Option<&'static str>)> {
            app.tabs.get(app.active_tab).map(|tab| {
                (
                    tab.id,
                    tab.active_cmd.as_ref().map(|command| command.name()),
                )
            })
        };
        #[cfg(not(target_arch = "wasm32"))]
        let command_before = active_command(self);
        // A coalesced pointer move may still be owed an answer, and
        // `last_move_pos` carries the cursor for whoever asks next: a bare
        // `ViewportLeftPress` (the overlay click-catchers, and the presses the
        // control API scripts) resolves its point from there, and only an
        // answered move writes it. Answer the withheld position first, so the
        // order stays the order the pointer reported — the hazard
        // `on_snap_dwell_tick` documents is a replay that lands *after*.
        let owed = self.flush_owed_pointer_move(&msg);
        let task = Task::batch([owed, self.update_inner(msg)]);
        let perf_dispatch_ms = crate::perf::elapsed_ms(perf_started);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let command_after = active_command(self);
            if command_before != command_after {
                if let Some((tab_id, command)) = command_after {
                    crate::plugin::v4_support::publish_command_state_changed(
                        tab_id,
                        command.map(str::to_owned),
                    );
                }
            }
        }
        self.refresh_gpu_status();
        if self.graphic_attributes.tracks_selection() {
            let selected = self.tabs[self.active_tab].scene.selected_handles_in_order();
            self.graphic_attributes.retain_selection(&selected);
        }
        self.show_next_startup_modal();
        // A Restore-all batch finished settling on this message: now that
        // the last restored open has inserted its tab, focus the tab the
        // remembered session had active (web session memory).
        #[cfg(target_arch = "wasm32")]
        if self.web_session_focus_pending {
            self.web_session_focus_pending = false;
            return self.select_remembered_session_tab();
        }
        self.sync_open_command_history();
        // Close the document-level first-touch transaction started by
        // push_undo_snapshot at this message boundary.
        self.finish_all_pending_history();
        if self.tabs[self.active_tab].active_cmd.is_none() {
            self.tool_palettes.placing = None;
        }
        // After every message, mirror the active command step's prompt so
        // its history line stays pinned (non-fading) until the step changes.
        // A pending client pick (`user_select` / `getpoint`) pins its labeled
        // request line the same way while no command owns the prompt.
        let prompt = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .map(|c| c.prompt())
            // A step with nothing to ask yet keeps no line.
            .filter(|prompt| !prompt.is_empty())
            .or_else(|| self.graphic_attributes.fill_close_prompt())
            .or_else(|| self.pending_pick_label());
        // In Remove mode a selection step asks "Remove objects:" in place of
        // its own wording, whatever the command, until Add turns it back.
        let gathering = self.tabs[self.active_tab]
            .active_cmd
            .as_ref()
            .is_some_and(|c| c.is_selection_gathering());
        let prompt = match prompt {
            Some(p) if gathering && self.select_remove_mode => Some(match p.split_once("  ") {
                Some((cmd, _)) => crate::t!("%{cmd}  Remove objects:", cmd = cmd).into_owned(),
                None => crate::t!("Remove objects:").into_owned(),
            }),
            other => other,
        };
        // A clicked box waiting for its opposite corner asks for it, with the
        // paths that may take its place as its options.
        let box_step = self.box_corner_step();
        let prompt = match &box_step {
            Some((text, _)) => Some(text.clone()),
            None => prompt,
        };
        // After each "N found" line the selection step asks again, under it.
        if std::mem::take(&mut self.selection_reprompt) && gathering {
            self.command_line.set_step_prompt(None);
        }
        self.command_line.set_step_prompt(prompt);
        // Mirror the step's clickable options so they render as buttons (#304).
        let opts = box_step
            .map(|(_, options)| options)
            .or_else(|| {
                self.tabs[self.active_tab]
                    .active_cmd
                    .as_ref()
                    .map(|c| c.options())
            })
            .or_else(|| {
                self.graphic_attributes
                    .pending_fill_close
                    .as_ref()
                    .map(|_| {
                        vec![
                            crate::command::CmdOption::new("Yes", "Y"),
                            crate::command::CmdOption::new("No", "N"),
                        ]
                    })
            })
            .unwrap_or_default();
        self.command_line.set_step_options(opts);
        // The status bar's QUAD field mirrors the live quad display flags —
        // the bar has no other path to the app's settings, and one message
        // arriving is one chance too many to render a stale pill.
        self.status_bar.quad = crate::ui::popup::quad_status::QuadStatusFlags {
            on: self.quad,
            on_hover: self.quad_on_hover,
            on_select: self.quad_on_select,
            on_right_click: self.quad_on_right_click,
            suppress_hover_when_selection: self.quad_suppress_hover_when_selection,
        };
        // Persist UI preferences whenever a toggle changes them (issue #68).
        self.persist_settings_if_changed();
        // The block panel watches the drawing's block list and rebuilds its
        // thumbnails whenever the names change (BLOCK define, file open, …).
        self.refresh_block_palette_if_stale();
        if self.show_block_palette {
            self.ensure_block_previews();
        }
        if self.block_gallery_open() {
            self.ensure_gallery_previews();
        }
        // What this message changed is only known while it is handled.
        self.block_palette.change_hint = Default::default();
        // The Reference Manager watches the active drawing and re-scans when
        // the palette is open on another tab's entries.
        self.refresh_xref_manager_if_stale();
        // Count mode recolours its references after the drawing changes.
        self.refresh_count_if_stale();
        // Let V4 plugins observe selection changes that happened while handling
        // this message (picking, window select, QSELECT, SELECTALL, grip edits,
        // and plugin request draining).
        #[cfg(not(target_arch = "wasm32"))]
        self.notify_plugins_selection_changed();
        #[cfg(not(target_arch = "wasm32"))]
        self.notify_plugins_document_changed();
        // A message may also have installed a whole document into a tab (file
        // open, template new); announce it on the same boundary (API v9).
        #[cfg(not(target_arch = "wasm32"))]
        self.notify_plugins_document_opened();
        // The `N found, M total` count belongs to the command that gathered
        // it; the next command, even one boxed at the same address, starts
        // from nothing.
        if self.tabs[self.active_tab].active_cmd.is_none() {
            self.selection_report = None;
        }
        // OTRACK acquires tracking points only while a command or grip drag is
        // running; drop them once neither is active so the temporary tracking
        // points / vectors disappear when the command ends (issue #64). A grip
        // drag's polar/ortho guide sets the vector with no tracking point, so
        // it is dropped too. (#1456)
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_none()
            && self.tabs[i].active_grip.is_none()
            && (!self.snapper.tracking_points.is_empty() || self.otrack_active.is_some())
        {
            self.snapper.clear_tracking();
            self.otrack_active = None;
            self.otrack_cross = None;
            self.otrack_kind = None;
        }
        if let Some((started, tab_id, before)) = perf_edit_before {
            if let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) {
                if tab.scene.geometry_epoch != before {
                    tab.scene.record_nav_perf_caused(
                        crate::scene::NavPerfOp::Edit,
                        perf_label,
                        started,
                    );
                }
            }
        }
        if let Some(started) = perf_started {
            let elapsed_us = started.elapsed().as_micros() as u64;
            crate::perf::note_update_time(elapsed_us);
            let elapsed_ms = elapsed_us as f64 / 1000.0;
            if elapsed_ms >= 5.0 {
                if let Some(variant) = perf_other_variant.as_deref() {
                    crate::perf_record!(
                        "[perf] update {:>7.1}ms message={perf_label} variant={variant} dispatch={:.2}ms settle={:.2}ms",
                        elapsed_ms,
                        perf_dispatch_ms,
                        (elapsed_ms - perf_dispatch_ms).max(0.0),
                    );
                } else {
                    crate::perf_record!(
                        "[perf] update {:>7.1}ms message={perf_label} dispatch={:.2}ms settle={:.2}ms",
                        elapsed_ms,
                        perf_dispatch_ms,
                        (elapsed_ms - perf_dispatch_ms).max(0.0),
                    );
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        crate::sys::set_unsaved_changes_warning(
            self.tabs
                .iter()
                .any(|tab| tab.dirty && tab.block_test.is_none()),
        );
        if self.tabs[i].active_cmd.is_none() {
            self.block_palette.placing = None;
        }
        self.control_settle();
        self.sync_spacemouse();
        task
    }

    /// Drop the OTRACK acquired points and the live alignment vector once a
    /// point has been committed to the active command. Temporary tracking
    /// points are reset on every input so they don't pile up across a
    /// multi-point command and overwhelm the next pick (issue #85).
    pub(in crate::app) fn reset_tracking_after_point(&mut self) {
        // A typed `<angle` and a Shift axis lock hold for the next point only.
        self.angle_override = None;
        self.axis_lock_dir = None;
        self.snapper.clear_tracking();
        self.otrack_active = None;
        self.otrack_cross = None;
        self.otrack_kind = None;
    }

}

mod dispatch;

impl OpenCADStudio {

    /// The colour the Select Color panel shows as "Current", and whether the
    /// selection it edits has mixed colours. A mixed selection starts from the
    /// colour of its most recently drawn object (the highest handle).
    /// Also returns the colour-book identity (`BOOK$NAME`) of that colour.
    fn color_picker_current(
        &self,
        target: &crate::app::ColorPickTarget,
        color: document::types::Color,
    ) -> (document::types::Color, bool, Option<String>) {
        let i = self.active_tab;
        let document = &self.tabs[i].scene.document;
        let name_of = |handle| {
            document
                .get_entity(handle)
                .and_then(|entity| entity.common().color_name.clone())
        };
        match target {
            crate::app::ColorPickTarget::Properties => {}
            crate::app::ColorPickTarget::Ribbon => {
                return (color, false, self.ribbon.active_color_name());
            }
            crate::app::ColorPickTarget::Layer(layer)
            | crate::app::ColorPickTarget::RibbonLayer(layer) => {
                let name = document
                    .layers
                    .get(layer)
                    .and_then(crate::ui::color_select::drawing_colors::layer_identity);
                return (color, false, name);
            }
            _ => return (color, false, None),
        }
        let mut colors = self
            .property_target_handles(i)
            .into_iter()
            .filter_map(|handle| {
                document
                    .get_entity(handle)
                    .map(|entity| (handle, entity.common().color))
            })
            .collect::<Vec<_>>();
        let Some(&(first_handle, first)) = colors.first() else {
            return (color, false, None);
        };
        if colors.iter().all(|&(_, c)| c == first) {
            return (first, false, name_of(first_handle));
        }
        colors.sort_by_key(|&(handle, _)| handle);
        let (newest_handle, newest) = colors.last().copied().unwrap_or((first_handle, color));
        (newest, true, name_of(newest_handle))
    }

    /// The colour ByLayer stands for in the Select Color panel: the layer of
    /// the newest selected object, or the current layer for new objects.
    fn color_picker_layer_color(
        &self,
        target: &crate::app::ColorPickTarget,
    ) -> Option<document::types::Color> {
        let i = self.active_tab;
        let document = &self.tabs[i].scene.document;
        let layer = match target {
            crate::app::ColorPickTarget::Properties => self
                .property_target_handles(i)
                .into_iter()
                .max()
                .and_then(|handle| document.get_entity(handle))
                .map_or(self.tabs[i].active_layer.as_str(), |entity| {
                    entity.as_entity().layer()
                }),
            crate::app::ColorPickTarget::Ribbon => self.tabs[i].active_layer.as_str(),
            crate::app::ColorPickTarget::RibbonLayer(layer) => layer.as_str(),
            _ => return None,
        };
        document.layers.get(layer).map(|layer| layer.color)
    }

    /// Read the built-in colour book and every `.acb` in the colour book
    /// folder, once per session. A book in the folder replaces a built-in
    /// one of the same name.
    fn read_color_books(&mut self) {
        if std::mem::replace(&mut self.color_books_read, true) {
            return;
        }
        self.color_books = std::sync::Arc::new(crate::ui::color_select::book::read_all());
    }

    /// Colour the selected objects (or set the current colour when nothing
    /// is selected). `name` is a colour-book identity, `BOOK$NAME`.
    pub(super) fn set_property_color(
        &mut self,
        color: document::types::Color,
        name: Option<String>,
    ) -> Task<Message> {
        let i = self.active_tab;
        let handles = self.property_target_handles(i);
        if handles.is_empty() {
            let task = self.on_ribbon_color_changed(color);
            self.ribbon.active_color_book = name.map(|name| (color, name));
            self.refresh_properties();
            return task;
        }
        self.apply_property_op(i, "CHPROP", &handles, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                crate::scene::view::dispatch::apply_named_color(entity, color, name.clone());
            }
        });
        self.tabs[i].properties.color_picker_open = false;
        Task::none()
    }

    /// Colour every selected layer (multi-select), not just one. `name` is a
    /// colour-book identity, `BOOK$NAME`.
    pub(super) fn set_layer_color(
        &mut self,
        color: document::types::Color,
        name: Option<&str>,
    ) -> Task<Message> {
        let i = self.active_tab;
        let names = self.selected_layer_names(i);
        self.set_layer_colors(&names, color, name)
    }

    pub(super) fn set_ribbon_layer_color(
        &mut self,
        layer: &str,
        color: document::types::Color,
        name: Option<&str>,
    ) -> Task<Message> {
        if !matches!(
            color,
            document::types::Color::Index(1..=255) | document::types::Color::Rgb { .. }
        ) || !self.tabs[self.active_tab]
            .scene
            .document
            .layers
            .contains(layer)
        {
            return Task::none();
        }
        self.ribbon.close_dropdown();
        self.note_recent_color(color);
        self.set_layer_colors(&[layer.to_owned()], color, name)
    }

    fn set_layer_colors(
        &mut self,
        names: &[String],
        color: document::types::Color,
        name: Option<&str>,
    ) -> Task<Message> {
        let i = self.active_tab;
        // Layers keep the book and the colour name apart.
        let (book_name, color_name) = match name.map(|name| name.split_once('$')) {
            Some(Some((book, name))) => (Some(book), Some(name)),
            Some(None) => (None, name),
            None => (None, None),
        };
        if !names.is_empty() {
            let undo = self.begin_layer_undo(i, "LAYER COLOR", names);
            for name in names {
                if let Some(dl) = self.tabs[i].scene.document.layers.get_mut(name) {
                    dl.color = color;
                    dl.color_name = color_name.map(str::to_string);
                    dl.book_name = book_name.map(str::to_string);
                }
            }
            for pl in self.tabs[i].layers.layers.iter_mut() {
                if names.contains(&pl.name) {
                    pl.color = color;
                    pl.color_name = color_name.map(str::to_string);
                    pl.book_name = book_name.map(str::to_string);
                }
            }
            self.tabs[i].dirty = true;
            self.commit_layer_undo(i, undo);
            // ByLayer color is baked into the cached wires at
            // tessellation time, so bump the geometry epoch to
            // invalidate the wire cache and repaint with the new color.
            self.tabs[i].scene.invalidate_layer_dependencies(names);
            self.tabs[i].layers.color_picker_row = None;
            self.tabs[i].layers.color_full_palette = false;
            self.sync_ribbon_layers();
        }
        Task::none()
    }

    pub(crate) fn note_recent_color(&mut self, color: document::types::Color) {
        // Keep only real colours in the recent list. ByLayer / ByBlock / None
        // are logical CAD states rather than reusable colours.
        if matches!(
            &color,
            document::types::Color::Index(_) | document::types::Color::Rgb { .. }
        ) {
            // No duplicates: selecting an existing colour moves it to the front.
            let recent = &mut self.tabs[self.active_tab].recent_colors;
            if let Some(pos) = recent.iter().position(|c| c == &color) {
                recent.remove(pos);
            }

            recent.insert(0, color);
            recent.truncate(12);
        }
    }

    /// Load a named scale into the scale-manager editor buffers (name + the
    /// paper / drawing units); blank ratios when the scale isn't found.
    fn load_scale_editor(&mut self, name: &str) {
        let i = self.active_tab;
        self.scale_manager_selected = name.to_string();
        match self.tabs[i].scene.scale_paper_drawing(name) {
            Some((p, d)) => {
                self.scale_manager_paper_buf = format!("{p}");
                self.scale_manager_drawing_buf = format!("{d}");
            }
            None => {
                self.scale_manager_paper_buf.clear();
                self.scale_manager_drawing_buf.clear();
            }
        }
    }

    /// Fold the editor's paper:drawing ratio into the selected scale, keeping
    /// its name (renaming is done inline in the list). Staged, no commit — so
    /// the ratio edit survives Apply *and* switching to another row. Editing a
    /// built-in fallback scale materialises it as a real one.
    /// The open edit card shows a scale whose units are locked.
    fn scale_card_units_locked(&self) -> bool {
        !self.scale_edit_adding
            && self
                .scale_rename
                .as_deref()
                .is_some_and(|old| self.scale_units_locked(self.active_tab, old))
    }

    /// A scale in use, the current one and the drawing's unit scale keep
    /// their ratio: the card shows their units greyed (measured).
    pub(in crate::app) fn scale_units_locked(&self, i: usize, name: &str) -> bool {
        !name.is_empty()
            && (self.anno_scale_referenced(i, name)
                || self.tabs[i]
                    .scene
                    .unit_scale_names()
                    .iter()
                    .any(|unit| unit.eq_ignore_ascii_case(name)))
    }

    fn scale_apply_current(&mut self) {
        let i = self.active_tab;
        let sel = self.scale_manager_selected.clone();
        if sel.is_empty() {
            return;
        }
        let paper = self.scale_manager_paper_buf.trim().parse::<f64>().ok();
        let drawing = self.scale_manager_drawing_buf.trim().parse::<f64>().ok();
        if let (Some(paper), Some(drawing)) = (paper, drawing) {
            if paper > 0.0 && drawing > 0.0 {
                // Skip when the editor still holds the stored ratio, so merely
                // navigating between scales doesn't dirty the drawing.
                if let Some((cp, cd)) = self.tabs[i].scene.scale_paper_drawing(&sel) {
                    if (cp - paper).abs() < 1e-9 && (cd - drawing).abs() < 1e-9 {
                        return;
                    }
                }
                let changed = self.tabs[i].scene.edit_scale(&sel, &sel, paper, drawing)
                    || (self.tabs[i].scene.scale_paper_drawing(&sel).is_none()
                        && self.tabs[i].scene.add_scale(&sel, paper, drawing));
                if changed {
                    self.scale_stage_mark();
                }
            }
        }
    }

    /// Write the table-style editor buffer (description) into the
    /// selected style (staged, no commit), so edits survive switching as well
    /// as Apply.
    fn stage_tablestyle_bufs(&mut self) {
        use document::objects::ObjectType;
        let i = self.active_tab;
        let name = self.tablestyle_selected.clone();
        let desc = self.ts_description.clone();
        for obj in self.tabs[i].scene.document.objects.values_mut() {
            if let ObjectType::TableStyle(s) = obj {
                if s.name == name && s.description != desc {
                    s.description = desc.clone();
                    s.raw_dxf_codes = None;
                }
            }
        }
    }

    /// A scale name based on `base`, suffixed " (n)" until it's unique in the
    /// drawing's scale list (used by New / Copy).
    fn unique_scale_name(&self, base: &str) -> String {
        let existing: std::collections::HashSet<String> = self.tabs[self.active_tab]
            .scene
            .scale_list()
            .into_iter()
            .map(|(n, _, _)| n.to_ascii_lowercase())
            .collect();
        if !existing.contains(&base.to_ascii_lowercase()) {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let candidate = format!("{base} ({n})");
            if !existing.contains(&candidate.to_ascii_lowercase()) {
                return candidate;
            }
            n += 1;
        }
    }
}

#[cfg(test)]
mod prop_pointer_tests;

#[cfg(test)]
mod anno_scale_toggle_tests;

#[cfg(test)]
mod dock_visibility_tests;

#[cfg(test)]
mod free_text_entry_tests;

/// Focus the layer-name field with its whole text selected, so typing
/// replaces the name instead of reaching the command line (#2610).
fn focus_layer_name_field() -> Task<Message> {
    let id = crate::ui::window::layers::rename_input_id();
    Task::batch([
        iced::widget::operation::focus(id.clone()),
        iced::widget::operation::select_all(id),
    ])
}

/// Set or clear one bit of `SELECTIONPREVIEW`.
fn set_preview_bit(current: u8, bit: u8, enabled: bool) -> u8 {
    if enabled {
        current | bit
    } else {
        current & !bit
    }
}

/// Web autosave deadline (SAVETIME minutes), polled from a short tick: the
/// first call arms it, a later one past it re-arms it and answers true.
#[cfg(target_arch = "wasm32")]
fn web_autosave_due(savetime_min: i32) -> bool {
    use std::cell::Cell;
    thread_local!(static DUE_MS: Cell<f64> = const { Cell::new(0.0) });
    let now = js_sys::Date::now();
    let period = savetime_min.max(0) as f64 * 60_000.0;
    DUE_MS.with(|due| {
        if period == 0.0 || due.get() == 0.0 {
            due.set(if period == 0.0 { 0.0 } else { now + period });
            return false;
        }
        let fire = now >= due.get();
        if fire {
            due.set(now + period);
        }
        fire
    })
}

#[cfg(test)]
mod point_style_dialog_tests;

/// The MCP client config (`mcpServers` JSON) and the matching
/// `claude mcp add` line for this install. AppImage and Flatpak builds are
/// started through their launcher, not the binary inside the bundle.
fn mcp_client_setup() -> (String, String) {
    let (command, mut args): (String, Vec<String>) = if let Ok(id) = std::env::var("FLATPAK_ID") {
        ("flatpak".into(), vec!["run".into(), id])
    } else if let Ok(appimage) = std::env::var("APPIMAGE") {
        (appimage, Vec::new())
    } else {
        let exe = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "OpenCADStudio".into());
        (exe, Vec::new())
    };
    args.push("--mcp".into());
    let config = serde_json::json!({
        "mcpServers": { "opencadstudio": { "command": command, "args": args } }
    });
    let config = serde_json::to_string_pretty(&config).unwrap_or_default();
    let quoted: Vec<String> = std::iter::once(&command)
        .chain(&args)
        .map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() })
        .collect();
    let add_line = format!("claude mcp add opencadstudio -- {}", quoted.join(" "));
    (config, add_line)
}

#[cfg(test)]
mod doc_tab_layouts_tests;

#[cfg(test)]
mod shortcut_suggestion_tests;
