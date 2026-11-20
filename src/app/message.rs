use super::*;

#[derive(Debug, Clone)]
pub enum Message {
    GraphicAttributes(crate::ui::window::graphic_attributes::GraphicAttributesMsg),
    SpaceMouseWake,
    SpaceMouseFrame(iced::time::Instant),
    SpaceMouseFocus(iced::window::Id, bool),
    SpaceMouseEnabled(bool),
    SpaceMouseMode(crate::input::spacemouse::NavigationMode),
    SpaceMousePanSpeed(u16),
    SpaceMousePanReversed(bool),
    SpaceMousePause,
    SpaceMousePreferences,
    SpaceMouseDriverSettings,
    SpaceMouseDetails,
    /// A touch screen finger event, for multi-finger gestures.
    Touch(iced::touch::Event),
    /// One magnification delta from a trackpad pinch, positive zooming in.
    TrackpadPinch(f32),
    ControlRequest(control::Envelope),
    PollWebControl,
    ControlStep(String, Box<Message>),
    ControlTaskDone(String),
    ControlScreenshot(String, Option<iced::window::Screenshot>),
    /// A frame has been drawn (or the anti-hang timer expired) for a parked
    /// `capture` request: take the screenshot now, so it shows the latest
    /// state instead of the previous frame (C11).
    ControlCaptureFrame {
        redrew: bool,
    },
    ControlToggle,
    /// Copy a ready-to-paste MCP client config for this install (#2401).
    ControlCopySetup,
    /// Node graph overlay interaction.
    Graph(crate::ui::node_graph::GraphMsg),
    /// The end of a read of a file that a node graph node reads.
    GraphFileRead(Box<crate::app::node_graph::files::FileRead>),
    /// The end of the automation `node_graph` `load` read: the drawing it
    /// was read for, that document's generation, and the file's bytes or
    /// the failure.
    GraphLoadRead(
        u64,
        u64,
        Result<crate::ui::node_graph::LoadedGraph, serde_json::Value>,
    ),
    Tick(Instant),
    /// Periodic drain of plugin-to-host requests that arrived outside a host
    /// call (e.g. mutations from the Python REPL).
    #[cfg(not(target_arch = "wasm32"))]
    DrainPluginRequests,
    /// A pump tick for the active custom-grip drag (API v11): collects at
    /// most one outstanding move answer, sends the newest parked point when
    /// the channel is free, and reschedules itself while the drag lives.
    #[cfg(not(target_arch = "wasm32"))]
    PluginGripPump,
    /// Web: periodic check for per-script fonts a drawing needs but hasn't
    /// fetched yet (#141). Native: never emitted.
    PollWebFonts,
    /// Web: a per-script font finished fetching — `Ok(bytes)` or `Err(reason)`.
    WebFontLoaded(
        crate::scene::text::web_font::Script,
        Result<Vec<u8>, String>,
    ),
    /// Register a font already held by the shared web store with the UI renderer.
    ApplyWebFont(crate::scene::text::web_font::Script),
    /// Completion of the UI renderer's runtime font registration.
    WebUiFontLoaded(crate::scene::text::web_font::Script, Result<(), String>),
    /// The desktop reported (or a poll may have found) a UI-font change.
    UiFontSettingsChanged,
    /// A re-read of the desktop UI font finished.
    UiFontDetected(Option<crate::sys::ui_font::SystemUiFont>),
    /// Ctrl+V. Routed by `update` into an open text editor, the drawing-object
    /// clipboard, or the system text clipboard.
    PasteShortcut,
    /// Which text input held focus when `PasteShortcut` arrived.
    PasteShortcutResolved(PasteFocus),
    /// Completion of the system clipboard read PASTESPEC starts: its text,
    /// its picture and the source a copied range of cells links to.
    PasteSpecial(
        Option<String>,
        Option<crate::io::ole_embed::EmbeddedImage>,
        Option<crate::sys::clipboard_link::SourceLink>,
    ),
    /// Completion of the system text clipboard read PASTEASHYPERLINK starts.
    PasteAsHyperlink(Option<String>),
    /// Completion of a system text clipboard read requested by PASTECLIP.
    SystemClipboardPaste(SystemClipboardText),
    /// Completion of the clipboard-image read the paste fallback starts when
    /// no text was found — `Some((name, bytes))` attaches the image, `None`
    /// reports the usual unsupported-content error.
    SystemClipboardPasteImage(Option<(String, Vec<u8>)>),
    /// Ctrl/Cmd+A — select all layer rows when the Layer Manager is open, or all
    /// drawing objects otherwise (#236).
    SelectAllShortcut,
    /// Open the shared Find and Replace dialog (Ctrl/Cmd+F or Ctrl/Cmd+H).
    FindReplaceOpen,
    Find(crate::ui::window::find_replace::FindMsg),
    OverkillSettingsChanged(crate::modules::draw::modify::overkill::Settings),
    OverkillToleranceChanged(String),
    OverkillOk,
    /// System-clipboard text read for the MText editor (`None` = empty/denied).
    MTextPasteClip(Option<String>),
    /// System-clipboard text read for the single-line TEXT editor.
    TextInlinePasteClip(Option<String>),
    OpenFile,
    /// File picker returned. `Some((path, size_in_bytes))` → start loading;
    /// `None` → user cancelled the dialog (no overlay shown).
    OpenPathPicked(Option<(PathBuf, u64)>),
    /// A file was dragged from the desktop and dropped on the window (#344).
    FileDropped(PathBuf),
    /// Web only: a drawing dragged onto the page — the DOM drop listener has
    /// already read the bytes; this opens through the web-picker flow (#344).
    /// The third field tags an autosave-restore file with its snapshot key so
    /// the snapshot is deleted only when this exact open succeeds.
    #[cfg(target_arch = "wasm32")]
    WebFileDropped(String, Vec<u8>, Option<String>),
    /// Web only: Ctrl+V captured by a focused text field, whose iced-internal
    /// paste is inert there — read the async clipboard instead (#346).
    WebFieldPaste,
    /// Web only: clipboard text arrived for a focused field — replay it as
    /// synthetic keystrokes on the canvas.
    WebFieldPasteText(Option<String>),
    /// Web only: Ctrl+C captured by a focused text field — collect the
    /// focused input's text via a widget operation.
    WebFieldCopy,
    /// Web only: the focused field's text — write it to the clipboard.
    WebFieldCopyText(Option<String>),
    /// Pick from the one-shot snap override menu (Shift+RMB): only this
    /// snap applies to the next point pick, then the configuration restores (#337).
    SnapOverridePick(crate::snap::SnapType),
    /// Mid Between 2 Points from the snap menu: modal 2-pick modifier over
    /// the active point prompt.
    SnapOverrideMtp,
    SnapOverrideFrom,
    /// Snap Overrides ▸ None: the next pick ignores object snaps.
    SnapOverrideNone,
    /// Close the one-shot snap override menu without picking.
    SnapOverrideClose,
    /// Open a path from the Start tab's recent-documents list (skips the
    /// file picker; the path is already known).
    OpenRecent(PathBuf),
    /// A second launch handed us a drawing to open (single instance). Queued
    /// behind any open already in flight — see `pending_opens`.
    OpenExternal(PathBuf),
    /// Open a URL in the system browser (start-page intro video, links).
    OpenUrl(String),
    /// Select which section a narrow (tabbed) Start page shows.
    StartSectionSelect(StartSection),
    /// Scroll the status-bar layout-tab strip horizontally by `delta` px
    /// (negative = left). Driven by the ‹ › arrows next to the tabs.
    ScrollLayoutTabs(f32),
    /// Drop a path from the recent-documents list.
    RecentRemove(PathBuf),
    /// Set how many recent documents to keep (persisted).
    SetRecentLimit(usize),
    SetRecentView(RecentView),
    /// Live edit of the recent-limit input box (not applied until Enter).
    RecentLimitInput(String),
    /// User clicked Cancel on the loading overlay. The parser thread keeps
    /// running but its result is discarded.
    OpenCancel,
    #[cfg(target_arch = "wasm32")]
    WebFileOpened(u64, crate::io::WebOpenOutcome),
    #[cfg(target_arch = "wasm32")]
    WebFileCached(u64, crate::io::WebOpenOutcome, Result<(), String>),
    FileOpened(
        u64,
        Result<crate::io::OpenedDrawing, crate::io::OpenLoadError>,
    ),
    RecoveryClose,
    RecoveryAttempt,
    RecoveryDecline,
    RecoverySaveAs,
    RecoveryShowLog,
    /// Web: an asynchronous OPFS copy written after Save is ready for recents.
    #[cfg(target_arch = "wasm32")]
    WebRecentStored(Result<PathBuf, String>),
    /// Web: the `?file=` boot deep link settled — its bytes entered the
    /// guarded open queue, or a notice/error text for the command line
    /// (recent copy missing, refused or failed fetch).
    #[cfg(target_arch = "wasm32")]
    WebLinkSettled(crate::io::web_link::LinkOutcome),
    /// Web: the boot scan finished — these autosave snapshots are newer than
    /// the drawings' last saved copies (newest first).
    #[cfg(target_arch = "wasm32")]
    WebAutosavesScanned(Result<Vec<crate::io::web_autosave::WebAutosaveEntry>, String>),
    /// Web: restore the offered autosave snapshot into a recovered tab.
    #[cfg(target_arch = "wasm32")]
    WebAutosaveRestore,
    /// Web: restore EVERY pending autosave snapshot — each is read and
    /// enqueued through the guarded open queue with its own key (the poll
    /// opens them sequentially); a snapshot is still consumed only after its
    /// own open succeeds.
    #[cfg(target_arch = "wasm32")]
    WebAutosaveRestoreAll,
    /// Web: delete the offered autosave snapshot (and offer the next, if any).
    #[cfg(target_arch = "wasm32")]
    WebAutosaveDiscard,
    /// Web: close the restore prompt WITHOUT deleting the offered snapshot
    /// (Escape/✕): it is simply re-offered on the next boot. Only the
    /// Discard button deletes.
    #[cfg(target_arch = "wasm32")]
    WebAutosaveDefer,
    /// Web: the offered snapshot's bytes are in; queue them for the guarded
    /// open poll. The OPFS copy is consumed only once that open succeeds.
    #[cfg(target_arch = "wasm32")]
    WebAutosaveBytes(String, String, Result<Vec<u8>, String>),
    /// Web: one autosave round finished for all dirty tabs (serially).
    #[cfg(target_arch = "wasm32")]
    WebAutosaveSnapshotFinished(Result<(), String>),
    SaveFile,
    SaveAs,
    // ── Custom Save-As dialog ─────────────────────────────────────────────
    SaveDialogFormatChanged(String),
    SaveDialogFilenameChanged(String),
    SaveDialogConfirm,
    SaveDialogCancel,
    /// Destination picked in the native OS save dialog (`None` = cancelled).
    SaveDialogPathPicked(Option<std::path::PathBuf>),
    /// Open the application-wide Options dialog.
    OptionsOpen,
    /// Switch the visible page in Options.
    OptionsTabChanged(crate::ui::window::options::OptionsTab),
    /// Options > Graphics: pin a backend (applies at next launch).
    #[cfg(not(target_arch = "wasm32"))]
    GraphicsBackendChanged(crate::gpu_backend::BackendChoice),
    /// Options > Graphics: force the packed compatibility renderer.
    #[cfg(not(target_arch = "wasm32"))]
    GraphicsCompatToggled(bool),
    /// Options > Graphics: prefer OpenGL on older GPUs.
    #[cfg(not(target_arch = "wasm32"))]
    GraphicsLegacyGlToggled(bool),
    /// Set CURSORSIZE from the Display-page slider.
    CursorSizeChanged(i32),
    /// Set PICKBOX from the Selection-page slider.
    PickBoxChanged(i32),
    /// Set APERTURE (object-snap capture radius) from the Selection-page slider.
    ApertureChanged(i32),
    /// Choose whether double-clicking a block starts BEDIT or REFEDIT.
    DoubleClickBlockRefeditChanged(bool),
    /// Choose whether double-clicking a block with attributes starts ATTEDIT.
    DoubleClickBlockAtteditChanged(bool),
    /// Set CURSORTYPE from Options.
    CursorTypeChanged(settings::CursorType),
    /// Set the model-space lineweight preview scale from Options.
    LineweightDisplayScaleChanged(i32),
    /// Edit the optional crosshair RGB value; blank restores automatic contrast.
    CrosshairColorChanged(String),
    SnapMarkerColorChanged(String),
    CommandTextColorChanged(String),
    /// Options → Files: the default template for new drawings.
    QnewTemplateChanged(String),
    QnewTemplateBrowse,
    /// Options → Open and Save: preserve, structured-by-type, or flat packages.
    TransmittalLayoutChanged(crate::io::transmit::TransmittalLayout),
    /// NEW: the picked template a new drawing starts from (`None` = cancelled).
    NewFromTemplate(Option<std::path::PathBuf>),
    /// Open (true) or close the start page's template list.
    StartTemplates(bool),
    /// Set the default type/version used when first saving a new drawing.
    DefaultSaveFormatChanged(String),
    /// Select one of Iced's built-in themes or the editable Custom theme.
    OptionsThemeChanged(String),
    /// Edit one of Custom theme's six base colours as #RRGGBB.
    OptionsThemeColorChanged(usize, String),
    /// Change Model Space canvas mode (MatchTheme, ClassicDark, Custom).
    ModelSpaceModeChanged(config::ModelSpaceMode),
    /// Change Model Space custom background color as hex or empty for default.
    ModelSpaceBgChanged(String),
    /// Change Paper Space custom sheet background color as hex or empty for default.
    PaperSpaceBgChanged(String),
    /// Change Paper Space desk surround background (#RRGGBB).
    DeskSpaceBgChanged(String),
    /// Change Grid opacity percentage (5–100).
    GridOpacityChanged(u8),
    /// Change Selection Area indicator toggle (SELECTIONAREA).
    SelectionAreaToggled(bool),
    /// Change Selection Area opacity percentage (0–100, SELECTIONAREAOPACITY).
    SelectionOpacityChanged(u8),
    SelectionHighlightOpacityChanged(u8),
    SelectionHighlightWidenChanged(u8),
    /// Change Window selection color ACI index (0 = Theme Primary, 1..=255 = ACI).
    SelectionWindowColorChanged(u8),
    /// Change Crossing selection color ACI index (0 = Theme Success, 1..=255 = ACI).
    SelectionCrossingColorChanged(u8),
    /// Change Selection Highlight color ACI index (0 = Theme Primary, 1..=255 = ACI).
    SelectionHighlightColorChanged(u8),
    /// Change Grip size in pixels (1–25, GRIPSIZE).
    GripSizeChanged(u8),
    /// Change the object-snap marker size in pixels (1–20, SNAPMARKERSIZE).
    SnapMarkerSizeChanged(u8),
    /// Change Unselected Grip color ACI index (0 = Theme Primary, 1..=255 = ACI, GRIPCOLOR).
    GripColorChanged(u8),
    /// Change Selected/Hot Grip color ACI index (0 = Theme Danger, 1..=255 = ACI, GRIPHOT).
    GripHotChanged(u8),
    /// Change Hover/Warm Grip color ACI index (0 = Theme Primary Strong, 1..=255 = ACI, GRIPHOVER).
    GripHoverChanged(u8),
    /// Change the selected-object count past which grips stop being drawn
    /// (0..=32767, 0 = no limit, GRIPOBJLIMIT).
    GripObjectLimitChanged(i32),
    /// Toggle the solid selection highlight (SELECTIONEFFECT).
    SelectionEffectToggled(bool),
    /// Toggle rollover preview while no command is running (SELECTIONPREVIEW bit 1).
    SelectionPreviewIdleToggled(bool),
    /// Toggle rollover preview during a command (SELECTIONPREVIEW bit 2).
    SelectionPreviewCommandToggled(bool),
    /// Toggle "use Shift to add to selection"; this is the inverse of PICKADD.
    ShiftToAddToggled(bool),
    PickDragModeChanged(u8),
    PickAutoBitToggled(u8, bool),
    PickFirstToggled(bool),
    /// Change the automatic-save interval in minutes; 0 disables it (SAVETIME).
    SaveTimeChanged(i32),
    /// Toggle keeping a `.bak` copy when overwriting a drawing (ISAVEBAK).
    BackupOnSaveChanged(bool),
    CheckMissingFontsChanged(bool),
    /// A drawing was picked to import page setups from (`PSETUPIN`).
    PageSetupImportFile(std::path::PathBuf),
    /// Options: open the Plot / Page Setup dialog for every new layout.
    PageSetupOnNewLayoutChanged(bool),
    /// Toggle filled TrueType glyphs (TEXTFILL).
    TextFillChanged(bool),
    /// Change how many prompt lines sit above the command window (CLIPROMPTLINES).
    ClipromptLinesChanged(i32),
    /// Change how long command-line history lines stay visible (COMMANDLINEFADETIME).
    CommandLineFadeChanged(i32),
    /// Change the command line's panel opacity (COMMANDLINEOPACITY).
    CommandLineOpacityChanged(u8),
    /// Options › Display: the command line font family ("" = interface font).
    CommandLineFontChanged(String),
    CommandLineFontSizeChanged(u16),
    /// Toggle reversing the mouse-wheel zoom direction (ZOOMWHEEL).
    ZoomWheelReversedChanged(bool),
    NavigationStyleChanged(crate::app::settings::NavigationStyle),
    /// F6 / UCSDETECT: toggle the dynamic UCS.
    ToggleDynamicUcs,
    TouchpadPanChanged(bool),
    /// Options: show the pull-down menus (MENUBAR).
    MenuBarChanged(bool),
    /// Options: show the ribbon (RIBBON / RIBBONCLOSE).
    RibbonVisibleChanged(bool),
    /// Options: center the ribbon's tabs and panels.
    RibbonCenteredChanged(bool),
    /// Options: the mouse wheel over the ribbon steps through its tabs.
    RibbonWheelTabsChanged(bool),
    /// Options: new hatches are associative (HPASSOC).
    HatchAssociativeChanged(bool),
    /// Interface scale in percent (Options > Display).
    UiScaleChanged(u16),
    /// Change how far one wheel notch zooms, from the Options slider
    /// (ZOOMFACTOR).
    ZoomFactorChanged(i32),
    /// Edit the Options zoom-factor field; committed when it holds a number,
    /// which is how a value above the system variable's own range is set
    /// (ZOOMFACTOR).
    ZoomFactorInputChanged(String),
    /// Options → User Preferences: right-click behaviour (SHORTCUTMENU).
    RightClickModeChanged(settings::RightClickMode),
    /// Options → User Preferences: time-sensitive hold threshold, ms.
    RightClickHoldMsChanged(i32),
    /// Toggle TEXTEDIT ending after one object (TEXTEDITMODE).
    TextEditModeChanged(bool),
    /// Toggle continued dimensions inheriting the base style (DIMCONTINUEMODE).
    DimContinueModeChanged(bool),
    /// Change which points QDIM measures from (0 endpoints, 1 intersections).
    QdimSnapPriorityChanged(u8),
    /// Change which annotative objects pick up a new scale (ANNOAUTOSCALE).
    AnnoAutoScaleChanged(i8),
    /// Edit the drafting rotation field; parsed when it holds a valid angle (SNAPANG).
    SnapAngleInputChanged(String),
    /// Change the polar tracking increment in degrees.
    PolarIncrementChanged(f32),
    /// Toggle the navigation cube (NAVVCUBE).
    ShowViewCubeChanged(bool),
    /// Toggle the UCS icon (UCSICON).
    ShowUcsIconChanged(bool),
    /// Toggle drawing the UCS icon at the origin (UCSICON ORigin).
    UcsIconAtOriginChanged(bool),
    /// Toggle selection cycling from Options; the status-bar pill toggles the same flag.
    SelectionCyclingChanged(bool),
    /// Reveal one of the application's own folders in the system file manager.
    OpenFolder(String),
    /// Choose another folder for fonts or plot styles (`None` result:
    /// cancelled).
    ChooseFolder(crate::ui::window::options::MovableFolder),
    /// Use this folder, or the default one for `None`.
    FolderChosen(
        crate::ui::window::options::MovableFolder,
        Option<std::path::PathBuf>,
    ),
    /// Change isolines per surface in the current drawing (ISOLINES).
    IsolinesChanged(i16),
    /// The isolines slider was released; rebuild the meshes if it moved.
    IsolinesReleased,
    /// Toggle silhouette edges in the current drawing (DISPSILH).
    DispSilhChanged(bool),
    /// Change surface density U in the current drawing (SURFU).
    SurfaceUChanged(i16),
    /// Change surface density V in the current drawing (SURFV).
    SurfaceVChanged(i16),
    /// Change the surface type in the current drawing (SURFTYPE).
    SurfaceTypeChanged(i16),
    /// Toggle recording composite-solid history in the current drawing (SOLIDHIST).
    SolidHistChanged(bool),
    /// Change when solid history is shown in the current drawing (SHOWHIST).
    ShowHistChanged(i16),
    /// Restore Model Space display/canvas appearance to defaults.
    RestoreModelSpaceDisplayDefaults,
    /// Restore Selection visual effect settings to defaults.
    RestoreSelectionVisualDefaults,
    /// Register or unregister as the .dwg/.dxf handler, from Options. Same
    /// setting the FILEASSOC command carries.
    FileAssocChanged(bool),
    /// Toggle showing driven values/named-parameter names on constraint
    /// pills, from Options. See `show_constraint_values`'s doc comment.
    ShowConstraintValuesChanged(bool),
    /// Switch the interface language and redraw localized views.
    LanguageChanged(crate::i18n::Language),
    /// Drop every entity from the active drawing.
    ClearScene,
    // ── Layer Translator (#624) ──────────────────────────────────────────
    /// Pick the drawing whose layers become the translation targets.
    LayerTranslatorLoad,
    /// A target set was read from `path`.
    LayerTranslatorLoaded(std::path::PathBuf),
    /// The end of a read of LAYERSTATE IMPORT's or LAYTRANS's files.
    LayerFileRead(Box<crate::app::commands::layer_file_reads::LayerFileRead>),
    /// The end of a CUIIMPORT read: the file, and its shortcuts or why it
    /// cannot be read.
    ShortcutsFileRead(String, Result<Vec<(String, String)>, String>),
    /// Web: a standards drawing picked and parsed in the browser, with the
    /// target layers already extracted — a name-only path has nothing behind
    /// it for `load_targets` to read. `Ok(None)` = cancelled.
    #[cfg(target_arch = "wasm32")]
    LayerTranslatorLoadedWeb(
        Result<
            Option<(
                String,
                Vec<crate::modules::draw::layers::laytrans::TargetLayer>,
            )>,
            String,
        >,
    ),
    LayerTranslatorSelectFrom(String),
    LayerTranslatorSelectTo(String),
    /// Pair the two selected layers.
    LayerTranslatorMap,
    /// Pair every layer the two drawings name alike.
    LayerTranslatorMapSame,
    /// Drop the mapping that starts at this layer.
    LayerTranslatorUnmap(String),
    LayerTranslatorForceByLayer(bool),
    LayerTranslatorWriteLog(bool),
    LayerTranslatorSaveMappings,
    LayerTranslatorLoadMappings,
    /// Path chosen for saving or loading mappings.
    LayerTranslatorMappingsPath(std::path::PathBuf, bool),
    /// Apply every mapping.
    LayerTranslatorTranslate,
    /// Set the active tab's render mode — one of the seven visual styles, and
    /// the only way a style is ever set.
    SetRenderMode(document::entities::ViewportRenderMode),
    /// Open or close the active viewport's visual-style flyout.
    ToggleRenderModeMenu(document::entities::ViewportRenderMode),
    /// Close the visual-style flyout after Escape or an outside click.
    DismissRenderModeMenu,
    /// Change only the sample shown beside the visual-style list.
    PreviewRenderMode(document::entities::ViewportRenderMode),
    /// Switch camera projection: true = Orthographic, false = Perspective.
    SetProjection(bool),
    /// Select a ribbon module tab by index.
    RibbonSelectTab(usize),
    /// The mouse wheel turned over the ribbon: step through its tabs.
    RibbonWheel(iced::mouse::ScrollDelta),
    /// The ribbon's layout changed what its last view was built on (the
    /// room for the search box): view it anew.
    RibbonRefit,
    /// The ribbon's right-click menu.
    RibbonMenu(crate::ui::ribbon::RibbonMenuMsg),
    /// A menu-bar entry: its command line, run like a ribbon button's. A
    /// space separates the answers to the command's prompts, as in a menu
    /// macro, and no Enter follows the last one.
    MenuCommand(String),
    /// The Quick Access Toolbar's lists and menus.
    RibbonQat(crate::ui::ribbon::QatMsg),
    /// The minimize button and the minimized ribbon's popups.
    RibbonMinimize(crate::ui::ribbon::MinimizeMsg),
    /// The pointer and pins of the ribbon's slide-outs.
    RibbonSlideOut(crate::ui::ribbon::SlideOutMsg),
    /// Time for a ribbon popup or slide-out to open or close.
    RibbonTick,
    /// The command search box and its results.
    RibbonSearch(crate::ui::ribbon::SearchMsg),
    /// Panels of a ribbon tab dropped in a new order: the shown panels'
    /// titles, in that order.
    RibbonReorderPanels {
        module: &'static str,
        titles: Vec<String>,
    },
    /// A ribbon panel dropped off the ribbon: it floats with its top-left
    /// corner at (`x`, `y`).
    RibbonFloatPanel {
        module: &'static str,
        title: String,
        x: f32,
        y: f32,
    },
    /// A floating ribbon panel: pressed, moved, released.
    RibbonFloat(crate::ui::ribbon::FloatMsg),
    /// Ribbon key tips: Alt, typed codes and Esc.
    KeyTip(crate::ui::ribbon::KeyTipInput),
    /// A ribbon tool button was clicked — highlights the tool and dispatches its event.
    RibbonToolClick {
        tool_id: String,
        event: ModuleEvent,
    },
    /// Result of a plugin-requested file picker (`ModuleEvent::PluginFileDialog`).
    /// `path` is `None` when the user cancels. On `Some`, the host dispatches
    /// `"<command> <path>"` to the plugins with original case preserved.
    PluginFileDialogResult {
        command: String,
        path: Option<std::path::PathBuf>,
    },
    /// Live text change in a plugin modal dialog control.
    PluginDialogTextChange(String, String),
    /// Checkbox toggle in a plugin modal dialog control.
    PluginDialogBoolToggle(String, bool),
    /// Dropdown choice selection in a plugin modal dialog control.
    PluginDialogChoiceSelect(String, usize),
    /// Color selection change in a plugin modal dialog control.
    PluginDialogColorChange(String, [u8; 3]),
    /// User clicked Browse on a file picker control in a plugin modal dialog.
    PluginDialogFilePick(String),
    /// File chosen from OS dialog for a file picker control.
    PluginDialogFilePicked(String, Option<std::path::PathBuf>),
    /// Live text edit in a file picker text input in a plugin modal dialog.
    PluginDialogFileTextChange(String, String),
    /// Table cell text change in a plugin modal dialog.
    PluginDialogTableCellChange(String, usize, usize, String),
    /// Add empty row to a table in a plugin modal dialog.
    PluginDialogTableAddRow(String),
    /// Remove a row from a table in a plugin modal dialog.
    PluginDialogTableRemoveRow(String, usize),
    /// Collapse / expand toggle for a property fields section.
    PluginDialogSectionToggle(String),
    /// Submit / OK clicked in a plugin modal dialog.
    PluginDialogSubmit,
    /// Cancel clicked in a plugin modal dialog.
    PluginDialogCancel,
    PluginDialogButtonClick(String, Option<String>),
    PluginDialogControlEvent(String, String, Option<ocs_plugin_api::dialog::DialogValue>),
    // ── Plugin dock panels ─────────────────────────────────────────────────
    PluginPanelButtonClick(String, String, Option<String>),
    PluginPanelTextChange(String, String, String),
    PluginPanelBoolToggle(String, String, bool),
    PluginPanelChoiceSelect(String, String, usize, String),
    PluginPanelFilePick(String, String),
    PluginPanelFilePicked(String, String, Option<std::path::PathBuf>),
    PluginPanelFileTextChange(String, String, String),
    PluginPanelTableCellChange(String, String, usize, usize, String),
    PluginPanelTableAddRow(String, String),
    PluginPanelTableRemoveRow(String, String, usize),
    PluginPanelSectionToggle(String, String),
    // ── Document tabs ──────────────────────────────────────────────────────
    /// Create a new empty document tab.
    TabNew,
    /// Switch to the given tab index.
    TabSwitch(usize),
    /// Drawing tab currently under the pointer; controls integrated close affordance.
    DocTabHover(Option<usize>),
    /// Open or close the tab bar's ☰ menu; with a tab index, also switch to
    /// that tab (a pick from the menu).
    DocTabMenu(bool, Option<usize>),
    /// The pointer has rested on tab `usize` (hover generation `u64`): list
    /// its layouts if it is still there.
    DocTabLayoutsSettled(usize, u64),
    /// Close a tab's layouts list.
    DocTabLayoutsClose,
    /// Switch to drawing tab `usize` and its layout `String`.
    DocTabLayout(usize, String),
    /// Run the drawing tabs across the window above the palettes (`true`)
    /// or between the palettes, over the drawing.
    DocTabsAboveDocks(bool),
    /// Move a drawing tab before/after another drawing tab.
    TabReorder {
        from: usize,
        to: usize,
        after: bool,
    },
    /// Close the tab with this document id. Carries the stable id, not a
    /// vector index: iced dispatches every message built from one view
    /// snapshot before rebuilding it, so an index captured alongside other
    /// messages can already be stale (and point at a different tab) by the
    /// time it is handled.
    TabClose(u64),
    /// Save every drawing that already has a file path.
    DocTabSaveAll,
    /// Close every non-Start drawing tab.
    DocTabCloseAll,
    /// Close every non-Start drawing tab except the given one.
    DocTabCloseOthers(usize),
    /// Copy the saved drawing's absolute path to the system clipboard.
    DocTabCopyFullPath(usize),
    /// Reveal the saved drawing in the platform file manager.
    DocTabOpenFileLocation(usize),
    // ── Unsaved-changes confirmation dialog ───────────────────────────────
    /// User clicked "Save" in the unsaved-changes dialog.
    UnsavedDialogSave,
    /// User clicked "Discard" in the unsaved-changes dialog.
    UnsavedDialogDiscard,
    /// User clicked "Cancel" in the unsaved-changes dialog.
    UnsavedDialogCancel,
    BlockEditChangesSave,
    BlockEditChangesDiscard,
    BlockEditChangesCancel,
    BlockSaveAsName(String),
    BlockSaveAsApply,
    BlockVisibility(crate::ui::window::block_visibility::Action),
    BlockLookup(crate::ui::window::block_lookup::Action),
    BlockPropertiesTable(crate::ui::window::block_properties_table::Action),
    // ── Parameters Manager ──────────────────────────────────────────────
    /// The operator clicked a row's numeric cell: the field opens on that row.
    ParametersManagerEdit(String, crate::ui::window::parameters_manager::Cell),
    ParametersManagerInput(String),
    ParametersManagerFilter(String),
    /// The funnel menu entry: which rows the list keeps.
    ParametersManagerFilterMode(crate::scene::block_parameters_manager::FilterMode),
    /// Toggles the funnel's two-entry menu.
    ParametersManagerFilterMenu,
    /// Toggles the header's column menu.
    ParametersManagerHeaderMenu,
    /// Shows or hides one header column.
    ParametersManagerColumn(crate::ui::window::parameters_manager::Column),
    ParametersManagerSearch(String),
    ParametersManagerCommit,
    /// The widget focused after a click while a manager cell is open. A click
    /// off the field commits what was typed, as Enter does.
    ParametersManagerFocus(Option<iced::widget::Id>),
    /// The row's Show cell, which is the parameter's own visibility flag.
    ParametersManagerShow(String),
    BlockEditPickerName(String),
    BlockEditPickerSelect(String),
    BlockEditPickerApply,
    Rename(crate::ui::window::rename::RenameMsg),
    DesignCenter(crate::ui::window::design_center::DesignCenterMsg),
    Purge(crate::ui::window::purge::PurgeMsg),
    DwgProps(crate::ui::window::drawing_properties::DwgPropsMsg),
    GeographicLocation(crate::ui::window::geographic_location::GeographicLocationMsg),
    /// Select Similar settings: flip one SELECTSIMILARMODE bit.
    SelectSimilarSettingsToggle(u16),
    SelectSimilarSettingsOk,
    // ── AEC / unsupported-object drop warning (lossy Save-As) ──────────────
    /// Save as the source DWG version so the unsupported objects survive.
    AecDropSameVersion,
    /// Proceed with the chosen target format, dropping the unsupported objects.
    AecDropProceed,
    /// Go back to the Save dialog from the AEC-drop warning.
    AecDropBack,
    /// Save with the converter anyway, accepting the listed losses.
    ConversionLossProceed,
    IfccadAlphaProceed,
    /// Drop the held-back save from the conversion-loss warning.
    ConversionLossBack,
    /// Periodic autosave tick — write `.sv$` recovery files for dirty tabs.
    AutoSave,
    /// A clean viewport frame is ready for thumbnail capture.
    ThumbnailCaptureFrame,
    /// Restore drawing UI after the compositor screenshot is captured.
    ThumbnailCaptureFinished,
    /// Native background save/autosave completed.
    #[cfg(not(target_arch = "wasm32"))]
    SaveFinished(SaveOutcome),
    /// Web viewport capture completed; serialize and download the drawing.
    #[cfg(target_arch = "wasm32")]
    WebSaveScreenshot {
        tab_id: u64,
        filename: String,
        ext: String,
        version: document::DxfVersion,
        bounds: Option<iced::Rectangle>,
        screenshot: Option<iced::window::Screenshot>,
    },
    /// Web SAVE: the serializer worker (or the inline fallback) produced the
    /// drawing bytes; download them and finish the save bookkeeping. The
    /// epoch/revision/camera markers snapshot the tab when serialization
    /// started — the web counterpart of native's `snapshot_is_current`, so a
    /// save that raced user edits does not wipe the newer dirty flag.
    #[cfg(target_arch = "wasm32")]
    WebSaveSerialized {
        tab_id: u64,
        filename: String,
        autosave_key: String,
        version: document::DxfVersion,
        geometry_epoch: u64,
        edit_revision: u64,
        camera_generation: u64,
        result: Result<Vec<u8>, String>,
    },
    /// Web SAVE with the File System Access API: the showSaveFilePicker
    /// promise settled. The picker ran in the save-confirm handler — the
    /// user-gesture moment — because the serialization that follows may
    /// outlast the transient-activation window; the retained handle (kept
    /// in io::fs_access's tab-keyed registry, since a JsValue handle cannot
    /// ride in a Send Message) is written to later without needing a
    /// gesture. `Ok(Some(name))` = destination chosen (`name` is its real,
    /// possibly renamed name); `Ok(None)` = the user dismissed the picker
    /// (cancel the save, like desktop); `Err` = the picker itself failed
    /// (fall back to the legacy download).
    #[cfg(target_arch = "wasm32")]
    WebSaveHandlePicked {
        tab_id: u64,
        filename: String,
        ext: String,
        version: document::DxfVersion,
        picked: Result<Option<String>, String>,
    },
    /// Web SAVE in-place completion: the retained FS Access handle's
    /// createWritable write settled (on failure the task already fell back
    /// to a download); run the shared save bookkeeping, including the OPFS
    /// recent-copy store the download path performs. Same race-guard
    /// markers as `WebSaveSerialized`.
    #[cfg(target_arch = "wasm32")]
    WebSaveWritten {
        tab_id: u64,
        filename: String,
        autosave_key: String,
        version: document::DxfVersion,
        geometry_epoch: u64,
        edit_revision: u64,
        camera_generation: u64,
        result: Result<(), String>,
        bytes: Vec<u8>,
    },
    /// Web WBLOCK: the serializer worker (or the inline fallback) produced
    /// the block document bytes; download them under `filename`, then run
    /// the same command-line bookkeeping as `WblockWriteFinished`.
    #[cfg(target_arch = "wasm32")]
    WebWblockSerialized {
        block_name: String,
        path: std::path::PathBuf,
        filename: String,
        result: Result<Vec<u8>, String>,
    },
    /// Web SAVEALL round completion: one entry per tab serialized on the
    /// worker (or the inline fallback). Downloads fire per entry; dirty
    /// flags clear only for tabs whose serialized snapshot is still current
    /// (the same race guard a single web Save uses).
    #[cfg(target_arch = "wasm32")]
    WebSaveallSerialized {
        skipped: usize,
        results: Vec<WebSaveallEntry>,
    },
    /// Web ARCHIVE/ETRANSMIT completion: the serializer worker (or the
    /// inline fallback) produced the drawing bytes; assemble the transmittal
    /// ZIP from the dependencies collected at command time, download it and
    /// report — including the skipped-dependency lines.
    #[cfg(target_arch = "wasm32")]
    WebArchiveSerialized {
        cmd: String,
        zip_name: String,
        filename: String,
        deps: Vec<(String, Vec<u8>)>,
        skipped: Vec<String>,
        warnings: Vec<String>,
        result: Result<Vec<u8>, String>,
    },
    /// Retry the failed save after the other application releases the file.
    #[cfg(not(target_arch = "wasm32"))]
    SaveFileInUseRetry,
    /// Choose a different destination for the failed save.
    #[cfg(not(target_arch = "wasm32"))]
    SaveFileInUseSaveAs,
    /// Cancel the failed save and any pending close/quit continuation.
    #[cfg(not(target_arch = "wasm32"))]
    SaveFileInUseCancel,
    /// Reload the externally changed drawing and discard local edits.
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChangeReload,
    /// Keep local edits but choose a different destination.
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChangeSaveAs,
    /// Replace the externally changed disk copy with the local drawing.
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChangeOverwrite,
    /// Cancel the conflicted save and any pending close/quit continuation.
    #[cfg(not(target_arch = "wasm32"))]
    ExternalChangeCancel,
    // ─────────────────────────────────────────────────────────────────────
    CommandInput(String),
    /// SVG markup pasted into the command line: the drawing's own copy or a
    /// picture, never typed text (#2623).
    CommandPasteMarkup,
    CommandSubmit,
    Command(String),
    /// Execute one complete line read from a command script. Unlike UI/ribbon
    /// dispatch, this accepts an interactive verb and all of its arguments on
    /// the same line (`BOX 0,0,0 10,10,0 10`).
    ScriptLine(String),
    /// Run the next queued script row (see `script_queue`).
    ScriptPump,
    /// Web SCRIPT: the picked .scr file's name and text. `None` = the
    /// browser dialog was cancelled.
    #[cfg(target_arch = "wasm32")]
    ScriptFilePicked(Option<(String, String)>),
    /// Web CENTERLTYPEFILE: the picked .lin/.dnl file's name and text.
    /// `None` = cancelled.
    #[cfg(target_arch = "wasm32")]
    CenterlineLinetypeFilePicked(Option<(String, String)>),
    /// Web DATALINK: the picked .csv file's name and raw bytes — registered
    /// in the picked-source registry, then fed to the same table-insert path
    /// the native typed-path command uses. `None` = cancelled.
    #[cfg(target_arch = "wasm32")]
    DataLinkCsvPicked(Option<(String, Vec<u8>)>),
    /// Completion of a DATALINK read off the UI thread: the read (tab,
    /// generation, command line, file), and the file's text or why it
    /// cannot be read.
    DataLinkCsvRead(
        Box<crate::app::commands::display::DataLinkRead>,
        Result<crate::app::commands::display::DataLinkCsv, String>,
    ),
    /// Append one typed character to the command-line input from the
    /// global key-press subscription. Used when the text-input widget
    /// itself isn't focused (focus parked on viewport / button / etc.)
    /// so typing still routes to the command line.
    CommandAppendChar(String),
    /// A character key went up: ends a held Shift+letter override.
    HeldOverrideRelease,
    /// Pop the trailing character off the command-line input — backspace
    /// counterpart to `CommandAppendChar`.
    CommandBackspace,
    /// TAB pressed: move focus to the next dynamic-input field or
    /// autocomplete suggestion.
    DynTabNext,
    /// Shift+TAB: move to the previous contextual autocomplete suggestion.
    DynTabPrev,
    /// Split the active Model viewport in two. `true` → horizontal divider
    /// (top / bottom); `false` → vertical divider (left / right).
    SplitModelViewport(bool),
    /// Close the active Model viewport, merging it into a neighbour.
    /// Only meaningful when more than one model tile exists.
    CloseModelViewport,
    /// Recall previous command in history (↑ arrow key).
    CommandHistoryPrev,
    /// Recall next command in history (↓ arrow key).
    CommandHistoryNext,
    /// An unconsumed arrow key; the active editor gets first choice, otherwise
    /// the configurable shortcut table handles it.
    ArrowKeyPressed {
        direction: ArrowKey,
        shortcut: String,
        extend_selection: bool,
    },
    /// A text field captured SPACE while it is rebound away from
    /// COMMANDSPACE; run the binding only if the command input has focus.
    CommandLineSpaceProbe,
    /// The command input has focus: drop the space it just typed and run the
    /// SPACE binding.
    CommandLineSpaceResolved,
    /// A widget captured Up/Down; resolve it only if the command input owns
    /// keyboard focus.
    CommandLineArrowProbe {
        direction: ArrowKey,
        extend_selection: bool,
    },
    /// Result of the command-input focus query for a captured Up/Down key.
    CommandLineArrowResolved {
        direction: ArrowKey,
        focused: bool,
        extend_selection: bool,
    },
    /// Toggle the dropdown listing the full command-line history.
    CommandHistoryToggle,
    /// Grab/move/release the expanded history panel's top resize edge.
    CommandHistoryResizeGrab,
    /// Drag the command line by its grip (floating mode).
    CommandLineMoveGrab,
    CommandLineMoveMove(Point),
    CommandLineMoveRelease,
    /// Put a floating command line back at the bottom of the drawing area.
    CommandLineDock,
    CommandHistoryResizeMove(Point),
    CommandHistoryResizeRelease,
    /// Restore the expanded history panel to its default height.
    CommandHistoryHeightReset,
    /// Start dragging the Layer Manager's Name-column divider.
    LayerNameColGrab,
    /// Start dragging the divider after a Layer Manager column.
    LayerColGrab(crate::ui::window::layers::LayerSortCol),
    /// Open or close the Layer Manager's column chooser.
    LayerColumnsMenu(bool),
    /// Show or hide a Layer Manager column.
    LayerColumnToggle(crate::ui::window::layers::LayerSortCol),
    /// Move a Layer Manager column left (-1) or right (1).
    LayerColumnMove(crate::ui::window::layers::LayerSortCol, i8),
    /// Toggle the persistent literal-space mode (the `>` button): while on,
    /// every command line behaves as if it started with `>` — Space stays in
    /// the input instead of submitting. Saved in the user config.
    CommandLiteralToggle,
    /// Copy the full command-line history (every line) to the system
    /// clipboard as plain text — issue #232, so output can be pasted for
    /// debugging instead of screenshotted.
    CommandHistoryCopy,
    #[cfg(target_arch = "wasm32")]
    CommandHistoryCopied(bool),
    /// Clear every line from the command-line history.
    CommandHistoryClear,
    /// Copy every line currently retained by the PERF panel.
    PerfCopy,
    /// Clear the PERF panel's retained trace.
    PerfClear,
    /// Text-editor action from the read-only history dropdown. Only
    /// non-editing actions (cursor moves, selection, scroll) are applied so
    /// the log stays read-only while remaining drag-selectable and copyable.
    CommandHistoryEdit(iced::widget::text_editor::Action),
    /// User clicked an autocomplete suggestion — fill the input with
    /// the chosen command name and dispatch it.
    CommandSuggestionPick(String),
    /// User clicked a command-line option button — feed its keyword to the
    /// active command as if typed (empty keyword = Enter). (#304)
    CommandOptionPick(String),
    /// The pointer entered (true) or left the command line's overlay history.
    CommandHistoryHover(bool),
    ToggleLayers,
    /// Open/focus (or close) the Reference Manager palette.
    ToggleXrefManager,
    /// Rescan the active drawing's references into the palette.
    XrefManagerRefresh,
    /// Toggle one palette row (stable row id) in the multi-selection set.
    XrefManagerSelect(crate::ui::window::xref_manager::XrefRowId),
    /// Right-click on a palette row: single-select it when outside the
    /// selection (the context menu then acts on the selection).
    XrefRowRightClick(crate::ui::window::xref_manager::XrefRowId),
    /// Flip the palette's list/tree presentation.
    XrefManagerToggleTree,
    /// Start a reference-table column divider drag (column index).
    XrefColGrab(usize),
    /// Pointer moved (table header space) during a column drag.
    XrefColMove(iced::Point),
    /// Pointer released during a column drag.
    XrefColRelease,
    /// Start a table/lower-pane split divider drag.
    XrefSplitGrab,
    /// Pointer moved (panel space) during a split drag.
    XrefSplitMove(iced::Point),
    /// Pointer released during a split drag.
    XrefSplitRelease,
    /// Flip the palette's details/preview lower pane.
    XrefManagerTogglePreview,
    /// The end of the worker that decoded an Xref Manager preview: the
    /// panel's refresh count when it was asked for, the cache key, and the
    /// preview (`None`: the file has none).
    XrefPreviewRead(u64, (u64, String), Option<iced::widget::image::Handle>),
    /// Toggle the Attach dropdown menu.
    XrefManagerAttachMenu,
    /// Toggle the Refresh dropdown menu.
    XrefManagerRefreshMenu,
    /// Toggle the Change Path dropdown menu.
    XrefManagerPathMenu,
    /// Open the Reference Manager help window.
    XrefHelpOpen,
    /// Close all palette dropdown menus (overlay dismissal).
    XrefManagerDismissMenus,
    /// Open the file picker for Select New Path (anchor entry).
    XrefPathPick,
    /// Result of the Select New Path picker.
    XrefPathPickResult(Result<std::path::PathBuf, String>),
    /// Reload every direct reference (toolbar Reload All).
    XrefManagerReloadAll,
    /// Expand/collapse one tree parent (block-record handle key).
    XrefManagerToggleExpand(u64),
    /// Selection-scoped palette operation (detach/unload/reload/overlay/pathtype).
    XrefManagerOp(crate::ui::window::xref_manager::XrefPaletteOp),
    /// Row-scoped palette operation: selects the row by stable id, then
    /// applies the operation to it.
    XrefRowOp(
        crate::ui::window::xref_manager::XrefRowId,
        crate::ui::window::xref_manager::XrefPaletteOp,
    ),
    /// Prefill the command line for Find & Replace across references
    /// (`XREF Path Find <old> <new>`); the CLI parses and runs it.
    XrefFindReplacePrompt,
    /// Pick a new path for a specific row (stable id).
    XrefRowPathPick(crate::ui::window::xref_manager::XrefRowId),
    /// Prefill the command line for Find & Replace for a specific row id.
    XrefRowFindReplacePrompt(crate::ui::window::xref_manager::XrefRowId),
    XrefRowChangePathEnter,
    XrefRowChangePathLeave,
    LayerToggleVisible(String),
    LayerToggleLock(String),
    LayerToggleFreeze(String),
    LayerTogglePlot(String),
    /// Sort the Layer Manager table by a clicked column header.
    LayerSort(crate::ui::window::layers::LayerSortCol),
    /// Toggle per-viewport freeze: (layer_name, vp_col_index)
    LayerToggleVpFreeze(String, usize),
    LayerNew,
    /// Show a node of the Layer manager's filter tree.
    LayerShow(crate::ui::window::layers::TreeNode),
    /// The Layer manager tree's rename field: typed, Enter, Esc.
    LayerTreeRenameEdit(String),
    LayerTreeRenameCommit,
    LayerTreeRenameCancel,
    /// A tree node's eye: all its layers lit, or none of them.
    LayerNodeEye(crate::ui::window::layers::TreeNode),
    /// The search text as a new property filter (#2610 LM-22a).
    LayerSaveSearch,
    /// The ribbon's layer list follows the chosen node, or lists them all.
    LayerRibbonFollows(bool),
    /// The compact Layer manager toolbar's "…" menu open or closed.
    LayerMoreMenu(bool),
    /// An item of that menu: close it, then do this.
    LayerMoreRun(Box<Message>),
    /// The compact toolbar's search field open or closed.
    LayerSearchOpen(bool),
    /// Pressed rows released somewhere that takes no drop.
    LayerDragEnd,
    /// The dragged rows are over this group (`None`: off it).
    LayerDragOver(Option<String>),
    /// The dragged rows dropped onto this group.
    LayerDrop(String),
    /// Open or close a section of the Layer manager's filter tree.
    LayerTreeSection(crate::ui::window::layers::TreeSection),
    /// Show the layers the node leaves out instead.
    LayerInvert(bool),
    /// Turn a layer's eye on or off: its objects highlighted in the drawing.
    LayerEye(String),
    /// Open FILTER over the layers: a new filter from the search text
    /// (`None`), or the LAYERFILTER filter named.
    LayerFilterEdit(Option<String>),
    /// A filter tree node's right-click menu.
    LayerTreeAction(
        crate::ui::window::layers::TreeNode,
        crate::ui::window::layers::TreeAction,
    ),
    /// Import layers from another drawing.
    LayerImport(layer_import::LayerImportMsg),
    /// A new layer, frozen in every layout viewport.
    LayerNewVpFrozen,
    /// A layer row's right-click menu: (the row's layer, what to do).
    LayerMenu(String, crate::ui::window::layers::LayerMenuAction),
    LayerDelete,
    /// Confirm deleting a non-empty layer (erases its objects too).
    LayerDeleteConfirm,
    LayerSetCurrent,
    LayerSelect(String),
    LayerRenameStart(String),
    LayerRenameEdit(String),
    LayerColorPickerToggle(String),
    LayerColorMorePalette,
    LayerColorSet(document::types::Color),
    LayerLinetypeSet(String),
    LayerLineweightSet(LineWeight),
    /// Typing in a layer's transparency field: (layer name, text).
    LayerTransparencyEdit(String, String),
    /// Enter in the transparency field applies the typed value.
    LayerTransparencyCommit,
    /// Typing in a layer's description: (layer name, text).
    LayerDescriptionEdit(String, String),
    LayerDescriptionCommit,
    /// The widget focused after a click while a description is being typed.
    LayerDescriptionFocus(Option<iced::advanced::widget::Id>),
    /// The widget focused after a click while a transparency is being typed.
    LayerTransparencyFocus(Option<iced::advanced::widget::Id>),
    LayerRenameCommit,
    /// The widget focused after a click while a layer rename is open.
    LayerRenameFocus(Option<iced::advanced::widget::Id>),
    /// The widget focused after a click while a style rename is open.
    StyleRenameFocus(Option<iced::advanced::widget::Id>),
    // ── Layer State Manager ─────────────────────────────────────────────
    LayerStateManagerOpen,
    LayerStateManagerSelect(String),
    LayerStateManagerNew,
    LayerStateManagerFilter(String),
    LayerStateManagerName(String),
    LayerStateManagerDescription(String),
    LayerStateManagerSave,
    LayerStateManagerRestore,
    /// Restore a saved layer state picked in the ribbon's layer dropdown.
    RibbonLayerStateRestore(String),
    LayerStateManagerDelete,
    LayerStateManagerEdit,
    LayerStateEditorMaskToggle(LayerStateProperty),
    LayerStateEditorLayerFlagToggle(usize, LayerStateLayerFlag),
    LayerStateEditorLayerColorToggle(usize),
    LayerStateEditorLayerColor(usize, document::types::Color),
    LayerStateEditorLayerLinetype(usize, String),
    LayerStateEditorLayerLineweight(usize, LineWeight),
    LayerStateEditorLayerPlotStyle(usize, String),
    LayerStateEditorLayerTransparency(usize, Option<document::types::Transparency>),
    LayerStateEditorName(String),
    LayerStateEditorDescription(String),
    LayerStateEditorCurrentLayer(String),
    LayerStateEditorFilter(String),
    LayerStateEditorSave,
    LayerStateEditorCancel,
    /// ViewCube-local cursor movement, tagged with the floating viewport that
    /// owned the overlay when the event was produced (`None` = Model layout).
    CursorMoved(Point, Option<document::Handle>),
    /// ViewCube press with the same owner tag, so a stale overlay event can
    /// never fall through and rotate a different camera.
    ViewportClick(Option<document::Handle>),
    ViewportMove(Point),
    /// Position recorded by the pointer event itself, not the last hover.
    ViewportLeftPressAt(Point),
    ViewportLeftPress,
    ViewportLeftRelease,
    ViewportRightPress,
    ViewportRightRelease,
    ViewportMiddlePress,
    ViewportMiddleRelease,
    ViewportScroll(mouse::ScrollDelta),
    ViewportExit,
    // ── Per-pane Model viewport (pane_grid) ───────────────────────────────
    /// A pane_grid divider was dragged — resize the split.
    PaneResized(iced::widget::pane_grid::ResizeEvent),
    /// A pane body was clicked — focus that pane.
    PaneClicked(iced::widget::pane_grid::Pane),
    /// A pane was drag-and-dropped onto another — swap them.
    PaneDragged(iced::widget::pane_grid::DragEvent),
    /// Per-pane mouse events. `usize` = the pane's tile index; the `Point` is
    /// pane-local (offset to canvas coords + focus in the handler).
    PaneMove(usize, Point),
    PanePress(usize),
    /// Button-event position in pane-local coordinates.
    PanePressAt(usize, Point),
    PaneRelease(usize),
    PaneRightPress(usize),
    PaneRightRelease(usize),
    PaneMiddlePress(usize),
    PaneMiddleRelease(usize),
    PaneScroll(usize, mouse::ScrollDelta),
    /// Drag-handle pressed on the active pane's controls bar: arm a pane move —
    /// the next pane released over is swapped with the active pane.
    PaneMoveStart,
    /// Viewport chip: switch the hovered-viewport outline (VPHIGHLIGHT).
    ViewportHighlightToggle,
    /// Viewport chip: wheel over the outline switch sets its strength
    /// (VPHIGHLIGHTOPACITY, percent).
    ViewportHighlightOpacity(u8),
    /// The pointer entered (true) or left the viewport chip's outline switch.
    ViewportOutlineSwitchHover(bool),
    ViewCubeSnap(CubeRegion),
    /// World-frame view snap from a compass cardinal (N/E/S/W), bypassing the
    /// UCS so the compass stays world-aligned.
    ViewCubeSnapWorld(CubeRegion),
    /// ViewCube home button → jump to the default isometric view.
    ViewCubeHome,
    /// ViewCube roll arrow → roll the view 90° (true = clockwise).
    ViewCubeRoll(bool),
    /// ViewCube nudge triangle → tip / spin the view 90°.
    ViewCubeNudge(crate::scene::NudgeDir),
    /// WCS/UCS selector under the cube — empty string = World.
    SetViewcubeUcs(String),
    /// User picked an item in the multi-functional grip popup menu —
    /// the index is into `grip_popup.items`.
    GripMenuPick(usize),
    /// User picked a dynamic-block visibility state — index into the
    /// visibility dropdown's items.
    VisibilityPick(usize),
    TableRowPick(usize),
    /// The Test Block window's visibility widget picked a state.
    TestVisibilityPick(usize),
    /// The Test Block window's lookup widget picked a table entry.
    TestLookupPick(usize),
    /// Timer pulse while the cursor is dwelling on a grip; drives the
    /// dwell-to-popup transition without requiring further mouse motion.
    GripDwellTick,
    /// The notice redraw allows the next frame to build the scene.
    LayoutSettled,
    /// Timer pulse while a rollover hit-test is queued; fires when the
    /// cursor has been still long enough to safely run the pick.
    HoverDwellTick,
    /// Timer pulse while a deferred object-snap pass is queued; fires when
    /// the cursor has been still long enough for the full snap on large
    /// drawings.
    SnapDwellTick,
    /// Frame pulse while a coalesced pointer move is owed an answer; runs the
    /// full pass at the position the gate withheld.
    MoveCoalesceTick,
    BoxPreviewTick,
    /// Frame pulse while a huge selection's Properties rebuild is queued, so
    /// the click / box release that committed the selection is not blocked by
    /// the (potentially hundred-millisecond) panel build.
    PropertiesRefreshTick,
    /// Large projected interaction index finished preparing off the UI thread.
    InteractionIndexReady {
        tab_id: u64,
        epoch: u64,
        source: usize,
        wires: std::sync::Weak<Vec<crate::scene::WireModel>>,
        index: std::sync::Arc<crate::scene::pick::interaction_index::InteractionIndex>,
        build_ms: f64,
    },
    WindowResized(f32, f32),
    /// A window opened at this size; the main window's sets the layout as
    /// a resize does.
    WindowOpened(window::Id, f32, f32),
    /// Enter pressed globally — finalises the active command (no text-input involvement).
    CommandFinalize,
    /// Space pressed globally — a literal space in the MText preview, otherwise
    /// finalises like Enter.
    CommandSpace,
    /// Escape pressed globally — cancels the active command.
    CommandEscape,
    /// Toggle the global snap on/off (OSNAP button body click).
    ToggleSnapEnabled,
    /// Toggle 3D object snap on/off — F4.
    ToggleSnap3dEnabled,
    /// Toggle grid-snap on/off — F9 / SNAP status-bar button.
    ToggleGridSnap,
    /// Enable or disable isometric drafting.
    ToggleIsometricDrafting,
    /// Select one isometric drafting axis pair.
    SetIsoPlane(settings::IsoPlane),
    /// Advance Left → Top → Right, enabling isometric drafting if necessary.
    CycleIsoPlane,
    /// Reset SNAPANG and return the active drafting coordinate system to World.
    ResetDraftingRotation,
    /// Toggle the ViewCube 3D gizmo visibility (NAVVCUBE).
    ToggleViewCube,
    /// Toggle the Properties panel visibility (PROPERTIES).
    ToggleProperties,
    /// Toggle the Graphic Attributes panel visibility (GRAPHICATTRIBUTES).
    ToggleGraphicAttributes,
    /// Toggle the document file tabs at the top (FILETAB).
    ToggleFileTabs,
    /// Toggle the layout tabs at the bottom (LAYOUTTAB).
    ToggleLayoutTabs,
    /// Toggle grid display in the viewport — F7 / GRID status-bar button.
    ToggleGrid,
    /// Toggle orthogonal drawing constraint — F8 / ORTHO status-bar button.
    ToggleOrtho,
    /// Toggle LWDISPLAY header flag — LWT status-bar button.
    ToggleLineweightDisplay,
    /// Toggle polar-angle constraint — F10 / POLAR status-bar button.
    TogglePolar,
    /// Set polar tracking angle increment (POLAR picker / right-click cycle).
    SetPolarAngle(f32),
    /// Open/close the polar-tracking angle picker (POLAR pill caret).
    TogglePolarPopup,
    /// Close the polar-tracking angle picker.
    ClosePolarPopup,
    /// Live text edit of the polar picker's custom-angle field.
    PolarCustomInput(String),
    /// Apply the typed custom polar angle (Enter in the picker's field).
    SubmitPolarCustom,
    /// Set the model-space annotation scale (CANNOSCALE equivalent).
    SetAnnotationScale(String),
    /// Set the active viewport's custom_scale (paper space).
    SetViewportScale(String),
    ToggleAnnotationVisibility,
    ToggleAnnotationAutoAdd,
    SyncViewportAnnotationScale,
    /// Status bar: toggle the display lock of the entered or selected viewports.
    ToggleViewportLock,
    /// Toggle the scale picker popup open/closed.
    ToggleScalePopup,
    /// Close the scale picker popup.
    CloseScalePopup,
    /// Open the annotation-scale manager (from the scale popup's Manage row).
    ScaleManagerOpen,
    /// Open the Annotation Object Scale dialog for the current single selection.
    AnnoObjectScaleOpen,
    /// Open and edit the selected objects' PE_URL hyperlink collection.
    PropHyperlinkOpen,
    HyperlinkUrlChanged(String),
    HyperlinkDescriptionChanged(String),
    HyperlinkApply,
    HyperlinkRemove,
    HyperlinkCancel,
    /// Toggle whether the dialog's object has a representation for this scale.
    AnnoObjectScaleToggle(String),
    /// Annotation Object Scale window: list only the shared scales.
    AnnoObjectScaleCommon(bool),
    /// Annotation Object Scale window: pick a scale in the list.
    AnnoObjectScaleSelect(String),
    /// Annotation Object Scale window: open the list of scales to add.
    AnnoObjectScaleAdd,
    /// Annotation Object Scale window: tick a scale to add.
    AnnoObjectScaleAddToggle(String),
    /// Annotation Object Scale window: add the ticked scales to the list.
    AnnoObjectScaleAddOk,
    /// Annotation Object Scale window: leave the Add list unchanged.
    AnnoObjectScaleAddCancel,
    /// Annotation Object Scale window: take the selected scale off the list.
    AnnoObjectScaleDelete,
    /// Annotation Object Scale window: apply the staged edits.
    AnnoObjectScaleOk,
    /// Select a scale row in the manager (loads it into the editor).
    ScaleManagerSelect(String),
    /// Move the selected scale one place up (-1) or down (1) in the list.
    ScaleManagerMove(i32),
    /// Ask which default scale list to restore.
    ScaleManagerReset,
    /// Restore the metric (0), imperial (1) or both (2) lists; 3 cancels.
    ScaleManagerResetChoice(u8),
    /// Apply the staged list and close the window.
    ScaleManagerOk,
    /// Add a new scale to the list (staged) and select it for editing.
    ScaleManagerNew,
    /// Duplicate the selected scale under a unique name (staged).
    ScaleManagerCopy,
    /// Begin inline rename of a scale row (double-click).
    ScaleRenameStart(String),
    /// Edit the inline-rename buffer.
    ScaleRenameEdit(String),
    /// Commit the inline rename.
    ScaleRenameCommit,
    /// Delete the selected scale.
    ScaleManagerDelete,
    /// The delete question: drop the picked scales not in use, or not.
    ScaleManagerDeleteChoice(bool),
    /// Opens the Edit card on the picked scale.
    ScaleManagerEdit,
    /// Closes the Edit / Add card without a change.
    ScaleManagerEditCancel,
    /// Set the selected scale as the current annotation scale.
    ScaleManagerSetCurrent,
    /// Apply the editor fields to the selected scale (rename / re-ratio).
    ScaleManagerApply,
    /// Annotation-scale manager editor field edits.
    ScaleManagerPaperBuf(String),
    ScaleManagerDrawingBuf(String),
    /// Toggle the leftmost hamburger's Model/layout list dropdown.
    ToggleLayoutList,
    /// Close the Model/layout list dropdown.
    CloseLayoutList,
    /// Cycle the coordinate readout mode ($COORDS): static → live → polar.
    CycleCoordsMode,
    /// Removes one flagged redundant or conflicting constraint from the
    /// current parametric scope.
    /// No-op if the scope currently has no flagged conflict.
    ResolveOneParametricConflict,
    /// Toggle the status-bar customization menu open/closed.
    ToggleStatusBarMenu,
    /// Close the status-bar customization menu.
    CloseStatusBarMenu,
    /// Show/hide a single status-bar pill.
    ToggleStatusPill(crate::ui::statusbar::statusbar_config::StatusPill),
    /// Toggle clean-screen mode (hide ribbon + side panels).
    ToggleCleanScreen,
    /// Toggle whether entity transparency is shown on screen.
    ToggleTransparencyDisplay,
    /// Toggle the Quick Properties floating panel.
    ToggleQuickProperties,
    /// Toggle selection cycling for overlapping objects.
    ToggleSelectionCycling,
    /// Add an object from the selection-cycling list box to the selection.
    CycleSelect(document::Handle),
    /// Preview (highlight) a cycling-list row's object, or clear with `None`.
    CycleHover(Option<document::Handle>),
    /// Cursor left a cycling-list row; clear the preview only if it still
    /// points at this row (guards against enter/exit event reordering).
    CycleHoverExit(document::Handle),
    /// Dismiss the selection-cycling list box without picking.
    CycleCancel,
    /// Toggle the selection-filter type picker open/closed.
    ToggleSelectionFilterPopup,
    /// Close the selection-filter type picker.
    CloseSelectionFilterPopup,
    /// Include/exclude an entity type from interactive selection.
    ToggleSelectionFilterType(String),
    /// Make every entity type selectable again (clear the filter).
    SelectionFilterSelectAll,
    /// Exclude every present entity type from selection.
    SelectionFilterClearAll,
    /// Toggle the drawing-units picker open/closed.
    ToggleUnitsPopup,
    /// Close the drawing-units picker.
    CloseUnitsPopup,
    /// Set the drawing units (INSUNITS) for the active drawing.
    SetDrawingUnits(i16),
    /// LUNITS — how lengths are written, from the status-bar units button.
    SetLinearFormat(i16),
    /// Open the Drawing Units dialog, seeded from the active drawing.
    OpenDrawingUnits,
    /// One field of the Drawing Units dialog changed.
    DrawingUnitsField(crate::ui::window::drawing_units::Field),
    /// Drawing Units OK — write the working copy into the drawing.
    DrawingUnitsApply,
    /// Block Definition dialog field updates
    BlockDefName(String),
    BlockDefNameSelect(String),
    BlockDefBaseOnScreen(bool),
    BlockDefPickPoint,
    BlockDefBaseX(String),
    BlockDefBaseY(String),
    BlockDefBaseZ(String),
    BlockDefObjectsOnScreen(bool),
    BlockDefSelectObjects,
    BlockDefQuickSelect,
    BlockDefObjectMode(crate::ui::window::block_definition::BlockObjectMode),
    BlockDefAnnotative(bool),
    BlockDefMatchOrientation(bool),
    BlockDefScaleUniformly(bool),
    BlockDefAllowExploding(bool),
    BlockDefUnit(i16),
    BlockDefDescription(String),
    BlockDefDescriptionAction(iced::widget::text_editor::Action),
    BlockDefHyperlink,
    BlockDefApply,
    BlockDefConfirmRedefine(bool),
    BlockDefDismissError,
    BlockDefHelp,
    /// Block Definition: open the new block in the block editor.
    BlockDefOpenInEditor(bool),
    /// Block Definition: define the block with no objects.
    BlockDefCreateEmpty,
    /// Write Block (WBLOCK) dialog messages
    WblockSourceMode(crate::ui::window::wblock::WblockSourceMode),
    WblockBlockName(String),
    WblockBlockSelect(String),
    WblockPickPoint,
    WblockBaseX(String),
    WblockBaseY(String),
    WblockBaseZ(String),
    WblockSelectObjects,
    WblockQuickSelect,
    WblockObjectMode(crate::ui::window::wblock::WblockObjectMode),
    WblockFilePath(String),
    WblockBrowsePath,
    WblockBrowsePathResult(Option<std::path::PathBuf>),
    WblockUnit(i16),
    WblockApply,
    WblockDismissError,
    WblockHelp,
    /// One structured feature-control-frame field changed.
    ToleranceDialogField(crate::ui::window::geometric_tolerance::Field),
    /// One structured feature-control-frame option changed.
    ToleranceDialogToggle(crate::ui::window::geometric_tolerance::Toggle),
    /// Apply edits without closing the structured editor.
    ToleranceDialogApply,
    /// Commit edits or continue to insertion-point placement.
    ToleranceDialogOk,
    /// Toggle the Isolate pill's action menu open/closed.
    ToggleIsolatePopup,
    /// Close the Isolate action menu.
    CloseIsolatePopup,
    /// Toggle dynamic input overlay (F12).
    ToggleDynInput,
    /// Toggle object snap tracking (F11).
    ToggleOTrack,
    /// Toggle an individual snap mode (from popup row click).
    ToggleSnap(crate::snap::SnapType),
    /// Open / close the OSNAP popup (▾ arrow click).
    ToggleSnapPopup,
    /// Close the OSNAP popup (click-catcher outside the panel).
    CloseSnapPopup,
    /// Enable all snap modes.
    SnapSelectAll,
    /// Disable all snap modes.
    SnapClearAll,
    // ── Drafting Settings Dialog ──────────────────────────────────────────
    DraftingSettingsTabChanged(crate::ui::window::drafting_settings::DraftingSettingsTab),
    /// Open Drafting Settings (or bring it up) on `tab`: a status pill's
    /// "… Settings..." entry.
    OpenDraftingSettings(crate::ui::window::drafting_settings::DraftingSettingsTab),
    DraftingSettingsToggleGrid,
    DraftingSettingsToggleSnap,
    DraftingSettingsSnapXChanged(String),
    DraftingSettingsSnapYChanged(String),
    DraftingSettingsGridXChanged(String),
    DraftingSettingsGridYChanged(String),
    DraftingSettingsGridMajorChanged(String),
    DraftingSettingsToggleAdaptiveGrid,
    DraftingSettingsToggleBeyondLimits,
    DraftingSettingsToggleGridAxes,
    DraftingSettingsToggleEqualSnap,
    DraftingSettingsToggleIsometric,
    DraftingSettingsSetIsoPlane(crate::app::settings::IsoPlane),
    DraftingSettingsResetRotation,
    DraftingSettingsTogglePolar,
    DraftingSettingsToggleOrtho,
    DraftingSettingsToggleOsnap,
    DraftingSettingsOsnapApertureChanged(String),
    DraftingSettingsToggleOtrack,
    DraftingSettingsToggleSnapMode(crate::snap::SnapType),
    DraftingSettingsToggleSnapMode3d(crate::snap::SnapType),
    DraftingSettingsSnapSelectAll,
    DraftingSettingsSnapClearAll,
    DraftingSettingsToggle3dOsnap,
    DraftingSettingsToggleDynInput,
    DraftingSettingsDynPrefs(crate::ui::window::drafting_settings::DynInputPrefs),
    DraftingSettingsToggleQuickProps,
    /// Quick Properties palette location: floating (true) or by the cursor.
    DraftingSettingsQuickPropsFloat(bool),
    /// The Quick Properties title bar was pressed: start moving the panel.
    QuickPropertiesGrab,
    DraftingSettingsToggleSelCycling,
    DraftingSettingsApply,
    DraftingSettingsOk,
    DraftingSettingsClose,
    DraftingSettingsCloseDiscard,
    DraftingSettingsCloseKeep,
    /// Options window: commit the changes made so far.
    OptionsApply,
    /// Options window: every preference back to its default, uncommitted.
    OptionsRestoreDefaults,
    /// Options window: commit and close.
    OptionsOk,
    /// Options window: close, asking first when changes would be lost.
    OptionsClose,
    OptionsCloseDiscard,
    OptionsCloseKeep,
    AutoConstrainSelectRow(usize),
    AutoConstrainToggleKind(settings::AutoConstraintKind),
    AutoConstrainMoveUp,
    AutoConstrainMoveDown,
    AutoConstrainSelectAll,
    AutoConstrainClearAll,
    AutoConstrainReset,
    AutoConstrainToggleTangentPoint,
    AutoConstrainTogglePerpendicularIntersection,
    AutoConstrainDistanceChanged(String),
    AutoConstrainAngleChanged(String),
    AutoConstrainApply,
    AutoConstrainOk,
    AutoConstrainCancel,
    /// Toggle a ribbon dropdown open/closed.
    ToggleRibbonDropdown(String),
    /// Toggle a collapsed ribbon panel's flyout open/closed (by panel title).
    ToggleRibbonPanel(String),
    /// Close any open ribbon dropdown (click-catcher outside the panel).
    CloseRibbonDropdown,
    /// User selected a specific item from a ribbon dropdown.
    DropdownSelectItem {
        dropdown_id: &'static str,
        cmd: &'static str,
    },
    /// Delete key — erase all currently selected entities.
    DeleteSelected,
    Undo,
    Redo,
    UndoMany(usize),
    RedoMany(usize),
    // ── Ribbon ────────────────────────────────────────────────────────────
    /// User selected a layer from the layer combobox in the ribbon.
    RibbonLayerChanged(String),
    /// Live text of the ribbon layer dropdown's search box (#343).
    RibbonLayerFilterChanged(String),
    /// Live text of the Layer Manager's search box (#343).
    LayerManagerFilterChanged(String),
    /// A new layer group from the selected layers.
    LayerGroupNew,
    /// Add the selected layers to this group.
    LayerGroupAdd(String),
    /// Take the selected layers out of the shown group.
    LayerGroupRemove,
    /// Delete the shown group (its layers stay).
    LayerGroupDelete,
    /// User changed the active color in the Properties toolbar.
    RibbonColorChanged(AcadColor),
    RibbonLayerColorChanged {
        layer: String,
        color: AcadColor,
    },
    /// Toggle the full ACI palette inside the ribbon color picker.
    RibbonColorPaletteToggle,
    /// User changed the active linetype in the Properties toolbar.
    RibbonLinetypeChanged(String),
    /// User changed the active lineweight in the Properties toolbar.
    RibbonLineweightChanged(LineWeight),
    /// User selected a style from a style combobox in the ribbon.
    RibbonStyleChanged {
        key: crate::modules::StyleKey,
        name: String,
    },

    // ── Properties panel ──────────────────────────────────────────────────
    /// User selected a layer from the layer pick_list in the Properties panel.
    PropLayerChanged(String),
    PropSelectionGroupChanged(crate::ui::properties::SelectionGroup),
    /// User picked a color from the Properties color picker.
    PropColorChanged(AcadColor),
    /// User selected a lineweight from the Properties pick_list.
    PropLwChanged(LineWeight),
    /// User selected an object-specific lineweight override.
    PropFieldLwChanged {
        field: &'static str,
        value: LineWeight,
    },
    /// User selected a linetype from the linetype pick_list.
    PropLinetypeChanged(String),
    /// User toggled a boolean property (e.g. Invisible).
    PropBoolToggle(&'static str),
    /// User stepped the Current Vertex selector by ±1 (polyline vertex nav).
    PropVertexStep(i8),
    /// User selected a hatch pattern from the pattern pick_list in Properties.
    PropHatchPatternChanged(String),
    /// Open or close the visual hatch-pattern picker.
    PropHatchPatternPickerToggle(String),
    /// Filter the visual hatch-pattern picker.
    PropHatchPatternSearchChanged(String),
    /// Move the visual pattern grid focus to a hovered card.
    PropHatchPatternFocus(usize),
    /// Move keyboard focus by one card or one two-column row.
    PropHatchPatternNavigate(i8),
    /// Select the keyboard-focused visual pattern card.
    PropHatchPatternConfirm,
    /// User selected a generic choice field in the Properties panel.
    PropGeomChoiceChanged {
        field: &'static str,
        value: String,
    },
    /// Open or close one generic choice row's list. It carries the surface it
    /// was clicked on, so the docked panel and Quick Properties each keep
    /// their own open state.
    PropChoiceToggle {
        field: &'static str,
        quick: bool,
    },
    /// The list dismissed itself: the overlay's Escape or a click outside; the
    /// global shortcut repeats the same Escape a moment later.
    PropChoiceDismiss {
        field: &'static str,
        quick: bool,
    },
    /// User is typing in an editable geometry field (live buffer update).
    PropGeomInput {
        field: &'static str,
        value: String,
    },
    /// User committed a geometry/common field edit (Enter pressed).
    PropGeomCommit(&'static str),
    PropBlockInput {
        key: crate::scene::model::object::BlockParameterKey,
        value: String,
    },
    PropBlockCommit(crate::scene::model::object::BlockParameterKey),
    PropBlockChoice {
        key: crate::scene::model::object::BlockParameterKey,
        index: usize,
    },
    /// Open or close the dynamic-value list of one row. It carries the surface
    /// it was clicked on, so the same row's two copies toggle independently.
    PropBlockPickerToggle {
        key: crate::scene::model::object::BlockParameterKey,
        quick: bool,
    },
    /// The list's overlay dismissed itself: a click outside it, or the same
    /// Escape whose global shortcut arrives a moment later.
    PropBlockPickerDismiss {
        key: crate::scene::model::object::BlockParameterKey,
        quick: bool,
    },

    /// Toggle a collapsed coordinate group ("Position", "Scale", …) open or
    /// closed in the Properties panel, keyed `section:base`.
    PropGroupToggle(String),
    /// Toggle an entire Properties-panel section, keyed by its title.
    PropSectionToggle(String),
    /// Select a Properties row by pressing its name, keyed `section/name`.
    PropRowSelect(String),
    /// Toggle an editable-dropdown row's option list open/closed, by field.
    PropEditChoiceToggle(&'static str),
    /// User is typing in a block-attribute value field (live buffer update),
    /// keyed by the attribute tag.
    PropAttrInput {
        tag: String,
        value: String,
    },
    /// User committed a block-attribute value edit (Enter pressed).
    PropAttrCommit(String),
    /// User is typing in a plugin class parameter value field (API v14,
    /// live buffer update), keyed by the declared parameter name.
    PropPluginParamInput {
        name: String,
        value: String,
    },
    /// User committed a plugin class parameter value edit (Enter pressed).
    PropPluginParamCommit(String),
    /// Reports the currently keyboard-focused widget (if any) after a
    /// [`sync_active_field_task`](crate::ui::properties::sync_active_field_task)
    /// sweep, so the update handler can keep the active-row marker reconciled
    /// against real focus instead of pointer hover.
    PropSyncActive(Option<iced::widget::Id>),
    /// A left mouse button was pressed anywhere. The clicked widget has already
    /// been given focus by the time this arrives (the widget tree processes the
    /// event before the runtime broadcasts it to subscriptions), so a focus
    /// sweep — which resolves as `PropSyncActive` — reveals whether a property
    /// value field was clicked and can select its whole value.
    PropPointerPressed,
    /// Toggle the inline color picker dropdown open/closed.
    PropColorPickerToggle,
    /// A Properties dropdown toggle and whether Quick Properties sent it.
    PropPickerSurface(bool, Box<Message>),
    /// Toggle the MTEXT background-colour picker dropdown open/closed.
    PropBgColorPickerToggle,
    /// User picked an MTEXT background fill colour — sets `background_color`
    /// and switches the background mode to Fill so it shows immediately.
    PropBgColorChanged(AcadColor),
    /// Toggle the generic per-field colour picker (e.g. a hatch gradient colour)
    /// identified by its property field name.
    PropColorFieldToggle(String),
    /// User picked a colour for a generic per-field colour row (hatch gradient
    /// `Color 1` / `Color 2`), routed by the field name.
    PropColorFieldChanged {
        field: String,
        color: AcadColor,
    },
    /// Collapse the inline color picker dropdown. Fired when another
    /// properties-panel dropdown (a combo_box) opens, so at most one panel
    /// dropdown is open at a time and they can't overlap. (#235)
    PropColorPickerClose,
    /// Enter the model-space editing mode inside the given viewport (MSPACE).
    EnterViewport(document::Handle),
    /// Exit MSPACE and return to paper-space editing (PSPACE).
    ExitViewport,
    /// MS command: enter MSPACE for the first available viewport.
    MspaceCommand,
    /// PS command: exit MSPACE (PSPACE).
    PspaceCommand,
    /// Switch to a named layout ("Model" or paper space layout name).
    LayoutSwitch(String),
    /// Switch to an already-open BEDIT block tab.
    BlockEditSwitch(String),
    /// Move a paper layout before/after another paper layout.
    LayoutReorder {
        from: String,
        to: String,
        after: bool,
    },
    /// Create a new paper space layout.
    LayoutCreate,
    /// Copy the named layout, its page setup and paper-space objects.
    LayoutCopy(String),
    /// Open the named layout's page setup.
    LayoutPageSetup(String),
    /// Plot the named layout.
    LayoutPlot(String),
    /// Delete the named paper space layout (Model cannot be deleted).
    LayoutDelete(String),
    /// Begin inline rename for the given layout tab.
    LayoutRenameStart(String),
    /// Live-update the rename text input buffer.
    LayoutRenameEdit(String),
    /// Commit the rename (Enter pressed in the text input).
    LayoutRenameCommit,
    /// Cancel an in-progress rename (Escape).
    LayoutRenameCancel,
    // ── Layout Manager Panel ────────────────────────────────────────────
    LayoutManagerOpen,
    #[allow(dead_code)]
    LayoutManagerClose,
    LayoutManagerSelect(String),
    LayoutManagerRenameBuf(String),
    LayoutManagerRenameCommit,
    LayoutManagerNew,
    LayoutManagerDelete,
    LayoutManagerMoveLeft,
    LayoutManagerMoveRight,
    LayoutManagerSetCurrent,
    /// Switch the UI color scheme.
    SetTheme(Theme),
    // ── Keyboard Shortcut Editor ────────────────────────────────────────
    ShortcutsPanelOpen,
    #[allow(dead_code)]
    ShortcutsPanelClose,
    ShortcutEditorInput {
        idx: usize,
        field: crate::ui::window::shortcuts::ShortcutField,
        value: String,
    },
    /// A row of a Command box's suggestion list was picked: the command
    /// replaces the typed text of row `idx`.
    ShortcutEditorSuggestionPick {
        idx: usize,
        command: String,
    },
    /// Close the suggestion list (Esc, or a click outside it).
    ShortcutEditorSuggestionDismiss,
    ShortcutEditorFilter(String),
    ShortcutEditorAdd,
    ShortcutEditorRemove(usize),
    ShortcutEditorApply,
    /// Apply the working rows and close the dialog.
    ShortcutEditorApplyExit,
    /// The check button on a draft row: finish the addition without applying.
    ShortcutEditorDraftAccept,
    /// Pick an xlsx/csv/txt shortcut file and merge it into the editor rows.
    ShortcutEditorImport,
    /// The picked file's bytes, or the read error (none when cancelled).
    ShortcutEditorImported(Option<Result<Vec<u8>, String>>),
    /// Show the "Reset to default" confirmation.
    ShortcutEditorResetAsk,
    /// Reset bindings and rows to the shipped defaults.
    ShortcutEditorResetConfirm,
    /// Hide the reset confirmation without resetting.
    ShortcutEditorResetDeny,
    /// Discard un-applied rows and close the editor.
    ShortcutEditorCloseDiscard,
    /// Keep editing: hide the discard confirmation.
    ShortcutEditorCloseKeep,
    /// The user clicked a row's Key cell: arm capture for that row.
    ShortcutCaptureStart(usize),
    /// A key combination was pressed while a row's Key cell was armed.
    ShortcutCaptureKey(String),
    /// Disarm capture without filling the row.
    ShortcutCaptureClear,
    /// Cancel capture (Esc): also discards an unfinished draft row.
    ShortcutCaptureCancel,
    /// Canonical key emitted by the global keyboard subscription.
    ShortcutPressed(String),
    // ── Command Alias Editor (ALIASEDIT) ────────────────────────────────
    /// Open the command-alias editor modal, seeding rows from the alias table.
    AliasEditorOpen,
    /// Pick an AutoCAD/OpenCAD PGP file, then merge supported aliases for review.
    AliasEditorImport,
    AliasEditorImported(Option<Result<Vec<u8>, String>>),
    /// Export the shortcut (`false`) or alias (`true`) list to a spreadsheet.
    KeyListExport(bool),
    /// An edit of the right-click menu in its in-place editor.
    ContextMenuEdit(ContextMenuEditMsg),
    /// Open Customize User Interface (CUI).
    RibbonCustomizeOpen,
    /// An edit in Customize User Interface.
    RibbonCustomize(crate::ui::window::ribbon_customize::Msg),
    /// An edit in the Hatch panel shown while HATCH runs.
    HatchPanel(crate::ui::window::hatch_panel::HatchPanelMsg),
    /// The file the export goes to (none when the picker was cancelled),
    /// with the rows to write.
    KeyListExportTo(Option<std::path::PathBuf>, Vec<Vec<String>>),
    /// Live edit of the alias or command in row `idx`.
    AliasEditorInput {
        idx: usize,
        field: crate::ui::window::alias_editor::AliasField,
        value: String,
    },
    /// Append a blank alias row.
    AliasEditorAdd,
    /// Remove alias row `idx`.
    AliasEditorRemove(usize),
    /// Commit the edited rows to the alias table (Apply button); stays open.
    AliasEditorApply,
    /// Apply the working rows and close the dialog.
    AliasEditorApplyExit,
    /// The check button on a draft row: finish the addition without applying.
    AliasEditorDraftAccept,
    /// Cancel a pending draft row (Esc / Cancel add).
    AliasEditorDraftCancel,
    /// Show the "Reset to default" confirmation.
    AliasEditorResetAsk,
    /// Reset aliases to the shipped defaults.
    AliasEditorResetConfirm,
    /// Hide the reset confirmation without resetting.
    AliasEditorResetDeny,
    /// Discard un-applied rows and close the editor.
    AliasEditorCloseDiscard,
    /// Keep editing: hide the discard confirmation.
    AliasEditorCloseKeep,
    // ── Named Parameters (PARAMETERS) ───────────────────────────────────
    /// Open the named-parameter editor, seeding rows from the active tab's
    /// `Scene::named_parameters`.
    NamedParametersOpen,
    BlockEditorSettingsInput(usize, String),
    /// Visual Styles Manager: crease angle typed / submitted, occluded edges toggled.
    VisualStylesCreaseInput(String),
    VisualStylesCreaseApply,
    VisualStylesOccludedEdges(bool),
    BlockConstraintColorInput(usize, String),
    ToggleBlockConstraintStatus,
    BlockEditorSettingsReset,
    BlockEditorSettingsApply,
    BlockCycleSelect(usize),
    BlockCycleEnable(usize, bool),
    BlockCycleMove(usize, bool),
    BlockCycleApply,
    BlockActionBarSelect(document::Handle),
    BlockActionBarEdit(document::Handle),
    BlockActionBarMove(document::Handle, iced::Vector),
    BlockActionBarHide(document::Handle),
    /// Live edit of the name or formula in row `idx`.
    NamedParametersInput {
        idx: usize,
        field: crate::ui::window::named_parameters::ParamField,
        value: String,
    },
    /// Append a blank parameter row.
    NamedParametersAdd,
    /// Remove parameter row `idx`.
    NamedParametersRemove(usize),
    /// Commit the edited rows to `Scene::named_parameters` (Apply button)
    /// and re-solve every constraint that reads a named parameter; stays
    /// open.
    NamedParametersApply,
    // ── Parameters / Constraints sections embedded in Properties ──────────
    /// Live text of one column of parameter row `index`, keyed by its
    /// `ParameterTable::iter()` position — see `PropValue::ParamRow`.
    PropParamInput {
        index: usize,
        field: crate::ui::window::named_parameters::ParamField,
        value: String,
    },
    /// Commit row `index`'s buffered edit for `field` to `Scene::
    /// named_parameters` (Enter / losing focus) and re-solve whatever it
    /// drives.
    PropParamCommit {
        index: usize,
        field: crate::ui::window::named_parameters::ParamField,
    },
    /// Remove parameter row `index` immediately.
    PropParamDelete(usize),
    /// Append a fresh, uniquely-named parameter to `Scene::named_parameters`.
    PropParamAddNew,
    /// A Constraints-section row was clicked: select every entity in the
    /// list (replacing the current selection) in the viewport.
    PropConstraintLinkClick(Vec<document::Handle>),
    /// Remove one parametric constraint selected from Properties or the viewport.
    PropConstraintDelete(crate::scene::parametric_constraints::ConstraintId),
    // ── About window ────────────────────────────────────────────────────
    AboutOpen,
    // ── Graphics warning ────────────────────────────────────────────────
    /// The status bar's ⚠ pill: reopen the graphics warning.
    GpuWarningOpen,
    /// "Don't show again for this device": close the warning and remember
    /// the verdict it described, so only a different one prompts again.
    GpuWarningSilence,
    /// Close whatever in-canvas modal dialog is open (Plan B).
    CloseModal,
    // ── Attribute editor dialog ───────────────────────────────────────────
    /// Open the attribute editor for an INSERT (double-click / ATTEDIT).
    AttrEditorOpen(document::Handle),
    /// Switch the editor's active tab.
    AttrEditorTab(crate::ui::window::attribute_editor::AttrTab),
    /// Select the row the Text Options / Properties tabs act on.
    AttrEditorSelect(usize),
    /// Live edit of the attribute value at row `idx` (Attribute tab).
    AttrEditorInput {
        idx: usize,
        value: String,
    },
    // Text Options — all act on the selected row:
    AttrEditorTextStyle(String),
    AttrEditorJustify(String),
    AttrEditorHeight(String),
    AttrEditorRotation(String),
    AttrEditorWidth(String),
    AttrEditorOblique(String),
    AttrEditorBackwards(bool),
    AttrEditorUpsideDown(bool),
    // Properties — all act on the selected row:
    AttrEditorLayer(String),
    AttrEditorLinetype(String),
    AttrEditorColor(String),
    AttrEditorLineweight(document::types::LineWeight),
    /// Keep every attribute edit (Apply); the window stays open.
    AttrEditorApply,
    /// Apply and close.
    AttrEditorOk,
    /// Take back what was not applied and close.
    AttrEditorCancel,
    AttrEditorHelp,
    /// Pick another block (asks first when there are changes).
    AttrEditorSelectBlock,
    AttrEditorAnswer(crate::ui::window::attribute_editor::AttrAnswer),
    AttrEditorDismissError,
    /// Edit a multi-line value in the text editor.
    AttrEditorEditValue,
    /// Set the value from the Field dialog.
    AttrEditorInsertField,
    AttrEditorBoundary(String),
    AttrEditorAnnotative(bool),
    /// A colour chosen in the colour window.
    AttrEditorColorValue(document::types::Color),
    /// Title-bar pressed: begin dragging the active modal.
    ModalGrab,
    /// Cursor moved while dragging the modal title bar.
    ModalDragMove(Point),
    /// Title-bar released: stop dragging.
    ModalDragRelease,
    MTextToolbarGrab,
    MTextToolbarMove(Point),
    MTextToolbarRelease,
    AboutCopyInfo,
    // ── Plugin Manager window ───────────────────────────────────────────
    PluginManagerOpen,
    #[allow(dead_code)]
    PluginManagerClose,
    /// Enable (`true`) or disable (`false`) the plugin with this id.
    SetPluginEnabled(String, bool),
    // ── Plugin marketplace (install from a linked repo's releases) ─────────
    /// Edit the add-repository text field.
    PluginRepoInput(String),
    /// Filter installed and available plugin cards.
    PluginSearchInput(String),
    /// Link the repository currently in the text field.
    PluginRepoAdd,
    /// Unlink a repository.
    PluginRepoRemove(String),
    /// The curated registry was fetched.
    PluginRegistryFetched(Result<Vec<crate::plugin::external::RegistryEntry>, String>),
    /// Retry the curated registry request after a connection failure.
    PluginRegistryRetry,
    /// Expand or collapse raw registry error details.
    PluginRegistryErrorDetailsToggle,
    /// Copy registry URL, platform, version, and raw error details.
    PluginRegistryCopyDiagnostics,
    /// Patreon supporters fetched at boot for the Start page (name, USD cents).
    PatronsFetched(Result<Vec<(String, i64)>, String>),
    /// Tutorial-playlist videos fetched at boot for the Start page.
    VideosFetched(Result<Vec<crate::videos::VideoEntry>, String>),
    /// GitHub Discussions fetched at boot for the Start page.
    DiscussionsFetched(Result<Vec<crate::discussions::DiscussionEntry>, String>),
    /// Recent-file DWG preview thumbnails decoded on a background thread.
    RecentThumbsLoaded(Vec<(std::path::PathBuf, Option<iced::widget::image::Handle>)>),
    /// Installable releases and manifest API versions fetched for `owner/repo`.
    PluginReleasesFetched(
        String,
        Result<Vec<crate::plugin::external::ReleaseInfo>, String>,
    ),
    /// Choose a release tag for a repo (`repo`, `tag`).
    PluginReleaseSelect(String, String),
    /// Select a plugin source and show its GitHub README in the detail pane.
    PluginReadmeSelect(String),
    /// A GitHub README fetch finished (`repo`, Markdown or error).
    PluginReadmeFetched(String, Result<String, String>),
    /// Install the selected release of `owner/repo`.
    PluginInstall(String),
    /// Install a specific newer release from an installed plugin card.
    PluginUpdate(String, String),
    /// Result of installing `owner/repo`: the plugin id, or an error message.
    PluginInstalled(String, Result<String, String>),
    /// Quit (asking about unsaved drawings) and start the app again.
    PluginRestart,
    /// Delete an installed plugin's folder (effective next restart).
    PluginUninstall(String),
    // ── Point Style (DDPTYPE) dialog ──────────────────────────────────────
    /// Set the full PDMODE value from a glyph-grid cell.
    PointStyleSetMode(i16),
    /// Choose size mode: `true` = relative to screen (PDSIZE < 0), `false` =
    /// absolute units (PDSIZE > 0).
    PointStyleSizeRelative(bool),
    /// Edit the point-size text field (magnitude only).
    PointStyleSizeInput(String),
    /// Commit the point-size text field to the header with the current sign.
    PointStyleApplySize,
    /// Apply the point-size field and close the dialog (OK button).
    PointStyleOk,
    // ── Quick Select / Select Similar ───────────────────────────────────
    /// Extend the current selection with every entity in the active
    /// layout that matches a selected entity by (type, layer).
    SelectSimilar,
    /// Replace the current selection with every other selectable object in
    /// the active layout (the complement of what's selected now).
    InvertSelection,
    /// Keyboard modifier state changed — tracks whether Shift is held so the
    /// pick path can do subtractive (Shift+click) selection.
    SetModifiers {
        shift: bool,
        ctrl: bool,
        alt: bool,
    },
    // ── In-place MText editor ───────────────────────────────────────────
    /// Text-area edit action from the multi-line editor widget.
    MTextEdit(iced::widget::text_editor::Action),
    /// Toolbar character-format toggle applied to the selection.
    MTextFmt(mtext_editor::MTextFmt),
    /// Toolbar height field changed.
    MTextHeight(String),
    /// Toolbar text-style dropdown changed.
    MTextStyle(String),
    /// The toolbar font picker: open, search or pick.
    MTextFontPicker(crate::ui::font_picker::FontPickerEvent),
    /// "Reset to style" on a first-row field (font, height, width, oblique).
    MTextResetToStyle(&'static str),
    /// Style menu: open the name of a new style from the selection, or from
    /// its Cancel go back.
    MTextNewStyleOpen,
    /// Style menu: the new style's name, as typed.
    MTextNewStyleName(String),
    /// Style menu: make the new style from the selection.
    MTextNewStyleCreate,
    /// Style menu: update the text's style from the selection.
    MTextUpdateStyleFromSelection,
    /// Style menu: open the Text Style Manager on the text's style.
    MTextStyleManager,
    /// Toolbar oblique-angle field changed.
    MTextOblique(String),
    /// Toolbar width-factor field changed.
    MTextWidth(String),
    /// Toolbar character-spacing field changed.
    MTextCharSpace(String),
    /// Undo / redo editor text and inline-format operations.
    MTextUndo,
    MTextInsertField,
    MTextRedo,
    /// Convert the selected numerator/separator/denominator to a stacked run.
    MTextStack,
    /// Superscript (true) or subscript (false) the selection.
    MTextScript(bool),
    /// Remove inline character formatting from the selection (or all text).
    MTextClearFormatting,
    /// Insert a predefined symbol or field token at the caret.
    MTextInsert(String),
    /// Per-object annotation flag edited from the text toolbar.
    MTextAnnotative(bool),
    /// The rotation field, in degrees, as typed.
    MTextRotation(String),
    /// Open a toolbar menu, or close it (`None`, or its own trigger again).
    MTextMenu(Option<mtext_editor::EditorMenu>),
    /// A menu action: the menu closes, then the message runs.
    MTextMenuPick(Box<Message>),
    /// Right-click on the in-place editor's text at a visible offset.
    MTextContextOpen(iced::Point, usize),
    /// Open (or close) a submenu of the editor's right-click menu.
    MTextContextGroup(Option<mtext_editor::context_menu::ContextGroup>),
    /// Close the editor's right-click menu and the tag menu.
    MTextContextClose,
    /// A row of the right-click menu or of a tag: a `_MTED` tool.
    MTextContextRun(String),
    /// Open or close the menu of the tag under the text.
    MTextSmartTag(bool),
    /// Paste Special: the clipboard text, pasted `PLAIN`, `NOCHAR` or
    /// `NOPARA`.
    MTextPasteAs(String, Option<String>),
    /// Bullets, numbering, or (`None`) plain paragraphs again.
    MTextList(Option<mtext_editor::ListKind>),
    /// Insert a tab at the caret.
    MTextTab,
    /// The line spacing factor typed in the Paragraph menu.
    MTextLineSpacingInput(String),
    /// Line spacing exactly the factor (`true`) or at least it.
    MTextLineSpacingMode(bool),
    /// Background mask on or off.
    MTextMaskToggle,
    /// Mask colour: the drawing background (`None`) or a colour of its own.
    MTextMaskColor(Option<AcadColor>),
    /// The mask's border offset factor, as typed.
    MTextMaskScale(String),
    /// The MTEXT's drawing direction.
    MTextDirection(document::entities::DrawingDirection),
    /// Open (or, from its own row, close) the Fraction panel.
    MTextFractionOpen,
    /// A Fraction panel field, as typed.
    MTextFractionPart(mtext_editor::FractionPart, String),
    /// The Fraction panel's stacking form.
    MTextFractionKind(document::entities::mtext_format::StackingType),
    /// Insert the Fraction panel's fraction.
    MTextFractionInsert,
    /// The status bar's box-width field, as typed.
    MTextRectWidthField(String),
    /// A ruler drag: the width while it moves, `None` when it ends.
    MTextBoxDrag(Option<f64>),
    MTextHeightDrag(Option<f64>),
    /// Column layout controls.
    MTextColumnMode(String),
    MTextColumnCount(String),
    MTextColumnWidth(String),
    MTextColumnGutter(String),
    MTextColumnHeight(String),
    MTextColumnFlowReversed(bool),
    /// Paragraph indent/spacing controls.
    MTextParagraphNumber(mtext_editor::ParaNumber, String),
    MTextFindText(String),
    MTextReplaceText(String),
    MTextFindNext,
    MTextReplaceNext,
    MTextReplaceAll,
    /// Toolbar colour picker (same widget as Properties) — applies to the
    /// selection, or the whole text when nothing is selected.
    MTextColorChanged(AcadColor),
    /// Open / close the MText colour picker popup.
    MTextColorPickerToggle,
    /// Toolbar justification / attachment-point change.
    MTextJustify(document::entities::mtext::AttachmentPoint),
    /// Toolbar paragraph-alignment change.
    MTextAlign(mtext_editor::ParaAlign),
    /// Toolbar line-spacing change.
    MTextLineSpacing(f64),
    /// Switch the editor body between raw code input (`false`) and the
    /// rendered preview (`true`).
    MTextShowPreview(bool),
    /// Enter in a toolbar field: typing goes back to the text.
    MTextFieldSubmit,
    /// Begin a preview text selection at the given visible-character offset.
    MTextSelStart(usize),
    /// Extend the preview selection to the given visible-character offset.
    MTextSelTo(usize),
    /// Move the preview caret by N visible characters.
    MTextCaretMove(i32),
    /// A pointer selection in the text editor is finished (Match takes it).
    MTextSelEnd,
    /// Import Text: the file picked.
    MTextImport(String),
    /// The end of the worker that read an Import Text file: the editor it
    /// was read for, and the file's MTEXT source or why it cannot be read.
    MTextImportRead(u64, Result<String, String>),
    /// Timer tick toggling the preview caret's blink phase.
    MTextCaretBlink,
    TableEditorClick(iced::Point),
    /// Commit the editor: create or update the MText entity.
    MTextOk,
    /// Apply the buffer to the entity but leave the editor open (the button).
    MTextApply,
    /// Grab the resizable modal's corner grip (a drag resizes it).
    ModalResizeGrab,
    /// The shared modal body finished layout at this size.
    ModalContentResized(iced::Size),
    /// Discard the editor without creating / changing the entity.
    MTextCancel,
    // ── In-place single-line TEXT editor ────────────────────────────────
    /// Text-field input changed.
    TextInlineInput(String),
    /// Commit the editor: create or update the TEXT entity.
    TextInlineOk,
    // ── Viewport right-click context menu ───────────────────────────────
    /// A context-menu row was picked (mouse or keyboard). Closes the menu and
    /// runs the row's action through the same message the equivalent typed
    /// input / shortcut would have produced.
    ContextMenuPick(crate::ui::popup::context_menu::MenuAction),
    /// Expand / collapse an accordion submenu of the open context menu.
    ContextMenuSubmenuToggle(crate::ui::popup::context_menu::SubmenuId),
    /// Keyboard navigation inside the open context menu.
    ContextMenuNavigate(ContextMenuNav),
    // ── Quad cursor menu ──────────────────────────────────────────────────
    /// A quad cursor menu interaction (toggle / expand / tab / pick / close)
    /// from the overlay or the keyboard while the quad is open.
    Quad(crate::ui::popup::quad_menu::QuadMsg),
    /// A Quad display-option change (Options window, the status-bar popup,
    /// or `SETVAR`'s mapping). Carries the new value.
    QuadOption(crate::QuadOptionMsg),
    /// Begin an interactive reference-object pick to move the current
    /// selection above (`true`) or below (`false`) the picked object.
    DrawOrderPickRef(bool),
    /// Open the Quick Select panel. Initialises filters from the current
    /// selection's first entity (type + layer) when one is selected.
    QSelectOpen,
    /// Close the Quick Select panel without applying.
    QSelectClose,
    /// Candidate scope: active space or the current selection.
    QSelectSetScope(QSelectScope),
    /// Type filter — `None` means "any type".
    QSelectSetType(Option<String>),
    /// Property to compare. `None` means "no property filter — just type
    /// filter applies"; the operator and value fields are ignored in
    /// that case.
    QSelectSetProperty(Option<QSelectPropertyChoice>),
    /// Comparison operator.
    QSelectSetOperator(QSelectOp),
    /// Compare-against value (free-text input).
    QSelectSetValue(String),
    /// Include or exclude objects matching the filter.
    QSelectSetMode(QSelectMode),
    QSelectSetResult(QSelectResult),
    /// Highlight the matches in the drawing.
    QSelectSetShow(bool),
    /// Tick or untick a picked object's property.
    QSelectSampleToggle(usize),
    /// Add the ticked properties as conditions.
    QSelectSampleAdd,
    QSelectSampleCancel,
    /// Apply the current filter and close the panel.
    QSelectAddCondition,
    /// Hide the filter and sample an object's filterable properties.
    QSelectPickObject,
    /// Window…: step aside, pick the objects to search in, come back with
    /// them as the selection.
    QSelectWindow,
    QSelectEditCondition(usize),
    QSelectRemoveCondition(usize),
    QSelectClearConditions,
    QSelectCancelEdit,
    QSelectApply,
    /// Quick Select's conditions, scope and result, carried over to FILTER.
    QSelectAdvanced,
    /// The FILTER dialog.
    FilterDialog(filter_dialog::FilterMsg),
    /// The user clicked the title-bar ✕ (fires before the window closes).
    WindowCloseRequested(window::Id),
    /// A window was fully closed (fires after `window::close()` is called).
    OsWindowClosed(window::Id),
    /// No-op — used as a fallback when a TabEvent has no host mapping.
    Noop,
    /// A message from an open tool dialog's frame or window: it runs with
    /// that tool as the active modal (see `tool_windows`).
    ToolWindow(ModalKind, Box<Message>),
    /// What a task a tool dialog's message started answers with: it runs
    /// with that tool swapped in again while the tool is open.
    ToolWindowTask(ModalKind, Box<Message>),
    /// A tool dialog's title bar was pressed: start moving it.
    ToolWindowGrab(ModalKind),
    /// A tool dialog's resize grip was pressed.
    ToolWindowResizeGrab(ModalKind),
    ToolWindowDragMove(iced::Point),
    ToolWindowDragRelease,
    /// A press on a tool dialog's frame: bring it to the front.
    ToolWindowRaise(ModalKind),
    /// Desktop: a window of its own's "back to the main window" button.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    OsWindowReturn(window::Id),
    /// Desktop: Esc or Enter in a window of its own, by shortcut name.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    OsWindowKey(window::Id, String),
    /// Desktop: a window other than the main one was resized or moved.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    OsWindowResized(window::Id, f32, f32),
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    OsWindowMoved(window::Id, iced::Point),
    /// A window of its own opened on, or moved to, a screen of this scale.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    OsWindowScale(window::Id, f32),
    /// Suppress menu-root tooltips between clicking the root and leaving it.
    StatusMenuTooltipHidden(bool),
    /// GitHub releases API returned a result. `Some(version)` means a
    /// newer release exists; we open the update-notice window.
    UpdateCheckResult(Option<crate::io::update_check::UpdateInfo>),
    /// CHECKFORUPDATES' answer: shown at once, and reported when there is
    /// nothing newer.
    UpdateCheckManual(Option<crate::io::update_check::UpdateInfo>),
    /// The process was asked to stop (SIGTERM).
    #[cfg(not(target_arch = "wasm32"))]
    Terminate,
    /// User dismissed the update-notice window.
    UpdateNoticeClose,
    DonationPromptDonate,
    /// First-launch default-association prompt: user accepted — register this
    /// app as the default handler for .dwg / .dxf.
    AssocPromptYes,
    /// First-launch default-association prompt: user declined (or "not now").
    AssocPromptNo,
    /// Result of the platform default-association call.
    AssocResult(Result<String, String>),
    /// User clicked the "Open release page" button — opens the GitHub
    /// release URL in the OS default browser and closes the notice.
    UpdateNoticeOpenRelease,
    // ── Plot / Export ─────────────────────────────────────────────────────
    /// Show the SVG save-file dialog and trigger export.
    PlotExport,
    /// The same export with the save dialog proposing a DWF file.
    PlotExportDwf,
    /// Callback after the user picks (or cancels) the export path.
    PlotExportPath(Option<std::path::PathBuf>),
    /// Export the pending model-space plot window (from PLOTWINDOW) to PDF.
    PlotWindowExport,
    /// Callback after the user picks (or cancels) the window-export path.
    PlotWindowExportPath(Option<std::path::PathBuf>),
    /// Completion of a PDF/preview/print job performed outside the UI thread.
    /// The boolean restores the Plot dialog after a preview.
    BackgroundIoFinished(Result<String, String>, bool),
    /// A background worker thread panicked or stopped without reporting a
    /// result. Surfaced on the command line instead of taking the whole
    /// process down with it.
    /// A point cloud read that the drawing started ended: its clouds draw
    /// again with the points.
    /// The end of the worker that drew a zoom step's underlay and OLE
    /// rasters: the drawing it was started for, that document's generation,
    /// and the rasters.
    UnderlayRastersDrawn(u64, u64, Box<crate::scene::ReseedDone>),
    PointCloudRead(crate::scene::model::point_cloud::ReadDone),
    /// The end of the worker that read an automation `embed_image` picture:
    /// the drawing it was started for, that document's generation, and the
    /// read.
    #[cfg(not(target_arch = "wasm32"))]
    EmbedImageRead(u64, u64, Box<control::EmbedImageRead>),
    BackgroundTaskFailed {
        /// Which operation the worker was running (e.g. "STL export").
        context: String,
        /// Panic message (or channel-failure description) from the worker.
        detail: String,
    },
    /// Send current layout to the system printer (via lp / lpr).
    PrintToPrinter,
    /// Callback from the async printer job.
    /// Web never constructs it — printing opens the PDF preview instead.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    PrintResult(Result<String, String>),
    /// Open the full Plot / Print dialog (seeds state from the layout).
    PlotDialogOpen,
    /// An edit inside the Plot / Print dialog.
    PlotDlg(crate::ui::window::plot::PlotDlgMsg),
    /// The answer to the outdated extraction table window.
    ExtractionOutdated(crate::ui::window::extraction_outdated::OutdatedChoice),
    /// An edit inside the docked Insert Block panel.
    BlockPalette(crate::ui::window::block_palette::BlockPaletteMsg),
    /// A dock chrome interaction (grab/resize/pin/hover/dock move) on a side
    /// panel.
    Dock(crate::ui::dock::DockMsg),
    AuthoringPaletteTabSelected(crate::ui::window::authoring_palettes::Tab),
    /// Open the paper-layout batch output dialog.
    PrintAllOpen,
    /// Toggle one paper layout in the batch.
    PrintAllToggle(String),
    /// Select or clear every paper layout in the batch.
    PrintAllSelectAll,
    PrintAllSelectNone,
    /// Edit the shared batch output settings in the Plot dialog.
    PrintAllOptions,
    /// Save the selected layouts as one multi-page PDF.
    PrintAllPdf,
    /// Save the selected layouts as one multi-page PDF at a given path
    /// (automation: no file picker).
    PrintAllPdfTo(std::path::PathBuf),
    /// Callback after the multi-page PDF path is picked or cancelled.
    PrintAllPdfPath(Option<std::path::PathBuf>),
    /// Send the selected layouts to the configured printer as one job.
    PrintAllPrint,
    /// Completion of a Print All PDF or printer job.
    PrintAllFinished(Result<String, String>),
    // ── Plot Style Table ─────────────────────────────────────────────────
    /// Open file dialog to load a CTB/STB plot style table.
    PlotStyleLoad,
    /// LINETYPE Load: the picked `.lin` file's text, `None` when cancelled.
    /// A picked `.lin`: its folder (native only) and text.
    LinetypeLoaded(Option<(Option<std::path::PathBuf>, String)>),
    /// Callback when the user picks (or cancels) a CTB/STB file.
    /// The Load… picker finished: a table, nothing (cancelled), or why the
    /// file could not be read.
    PlotStyleLoaded(
        u64,
        String,
        Result<Option<crate::io::plot_style::PickedPlotStyle>, String>,
    ),
    /// Clear the active plot style table.
    PlotStyleClear,
    /// Open/close the Plot Style panel.
    PlotStylePanelOpen,
    #[allow(dead_code)]
    PlotStylePanelClose,
    /// Select an ACI entry in the panel.
    PlotStylePanelSelectAci(u8),
    /// Edit buffers changed.
    PlotStylePanelColorBuf(String),
    PlotStylePanelLwBuf(String),
    PlotStylePanelLwSet(u8),
    PlotStylePanelScreenBuf(String),
    /// Apply current edit buffers to the selected ACI entry.
    PlotStylePanelApply,
    /// Save the modified table directly over the currently edited CTB.
    PlotStylePanelSaveDirect,
    /// Save the modified table under a chosen name/path.
    PlotStylePanelSave,
    /// Save callback.
    PlotStylePanelSavePath(Option<std::path::PathBuf>),
    // ── TextStyle Font Browser ────────────────────────────────────────────
    TextStyleDialogOpen,
    #[allow(dead_code)]
    TextStyleDialogClose,
    TextStyleDialogSelect(String),
    /// "Styles in use": show only the styles texts use.
    TextStyleInUseFilter(bool),
    /// The preview's sample text, as typed.
    TextStyleSample(String),
    TextStyleDialogCompare(String),
    TextStyleDialogSetCurrent,
    TextStyleDialogNew,
    TextStyleDialogCopy,
    // Shared inline-rename messages for every style manager. `StyleKind`
    // routes the commit to the right backing store.
    StyleRenameStart(StyleKind, String),
    StyleRenameEdit(String),
    StyleRenameCommit(StyleKind),
    StyleRenameCancel,
    TextStyleDialogDelete,
    StyleImportPick(StyleKind),
    StyleImportFile(StyleKind, Option<std::path::PathBuf>),
    StyleImportLoaded(StyleKind, Result<(String, document::Document), String>),
    StyleImportToggle(String),
    StyleImportApply,
    StyleImportConfirm(bool),
    StyleImportCancel,
    /// Edit a string field (FontFile / Width / Oblique).
    TextStyleEdit {
        field: &'static str,
        value: String,
    },
    /// Commit edits to the selected text style.
    TextStyleApply,
    /// The text style window's cards: New, a refused name, Delete, a style
    /// in use, unsaved changes.
    TextStyleCard(crate::ui::style::textstyle::CardMsg),
    /// A right click on a text style: select it and show its actions.
    TextStyleContext(String),
    /// The font picker: open, search or pick a stroke or TrueType font.
    TextStyleFont(crate::ui::font_picker::FontPickerEvent),
    /// The big font picker: open, search or pick a double-byte SHX font.
    TextStyleBigFont(crate::ui::font_picker::FontPickerEvent),
    /// Pick the TrueType face: regular, bold, italic or both.
    TextStyleFaceStyle(crate::ui::font_picker::FaceStyle),
    /// Flip a boolean flag on the selected text style (backward / upside_down /
    /// vertical / annotative), applied immediately.
    TextStyleToggle(&'static str),
    // ── TableStyle Dialog ─────────────────────────────────────────────────
    TableStyleDialogOpen,
    #[allow(dead_code)]
    TableStyleDialogClose,
    TableStyleDialogSelect(String),
    TableStyleListInUse(bool),
    TableStyleDialogTab(u8),
    TableStyleDialogCompare(String),
    TableStyleDialogNew,
    TableStyleDialogCopy,
    TableStyleDialogDelete,
    TableStyleDialogSetCurrent,
    /// Toggle the Annotative flag on the selected table style.
    TableStyleToggleAnnotative,
    /// Toggle a boolean flag (title_suppressed / header_suppressed / flow) on
    /// the selected table style.
    TableStyleToggle(&'static str),
    /// Update a general edit buffer (description).
    TableStyleEdit {
        field: &'static str,
        value: String,
    },
    /// Write the general edit buffers back into the selected table style.
    TableStyleApply,
    /// Edit the named cell style.
    TableStyleCellSelect(String),
    /// The cell style cards (New / Manage), the Borders page's line
    /// properties and buttons, and the Format / Text style "…" windows.
    TableStyleCells(crate::ui::style::tablestyle::TsCellMsg),
    /// A field of the edited cell style: a text buffer ("height", "angle",
    /// "hmargin", "vmargin") or a choice ("align", "class", "textstyle",
    /// "textcolor", "fillcolor", "merge").
    TableStyleCellEdit {
        field: &'static str,
        value: String,
    },
    /// Toggle the expanded colour palette of a cell colour field.
    TableColorMore(&'static str),
    /// Set the table flow direction from the dropdown.
    TableStyleSetFlow(String),
    /// Pick a table in the drawing as the style's starting table.
    TableStyleTemplatePick,
    /// Drop the style's starting table.
    TableStyleTemplateRemove,
    // ── MLineStyle Dialog ─────────────────────────────────────────────────
    Linetype(crate::ui::style::linetype::LinetypeMsg),
    MlStyleDialogOpen,
    #[allow(dead_code)]
    MlStyleDialogClose,
    MlStyleDialogSelect(String),
    MlStyleDialogTab(u8),
    MlStyleDialogCompare(String),
    MlStyleDialogSetCurrent,
    MlStyleApply,
    MlStyleDialogNew,
    MlStyleDialogCopy,
    MlStyleDialogDelete,
    MlStyleEdit {
        field: &'static str,
        value: String,
    },
    MlStyleToggle(&'static str),
    MlStyleElementEdit {
        index: usize,
        field: &'static str,
        value: String,
    },
    MlStyleElementAdd,
    MlStyleElementDelete(usize),
    // ── MLeaderStyle Dialog ───────────────────────────────────────────────
    MLeaderStyleDialogOpen,
    /// The manager's list, faces and choosers.
    MLeaderStyleDlg(crate::ui::style::mleaderstyle::MlsMsg),
    MLeaderStyleEdit {
        field: &'static str,
        value: String,
    },
    /// Toggle the expanded colour palette for an MLeaderStyle colour field.
    MLeaderColorMore(&'static str),
    MLeaderStyleToggle(&'static str),
    MLeaderStyleSetEnum {
        field: &'static str,
        value: String,
    },
    MLeaderStyleLineWeightChanged(LineWeight),
    /// Set an Option<Handle> field (linetype / arrowhead / text style / block)
    /// from a dropdown of record names ("None" clears it).
    MLeaderStyleSetHandle {
        field: &'static str,
        value: String,
    },
    // ── DimStyle Dialog ───────────────────────────────────────────────────
    DimStyleDialogOpen,
    /// Interaction in the dimension style window.
    DimStyleDlg(crate::ui::style::dimstyle::DsMsg),
    /// Open the shared CAD colour picker for a field and its current colour.
    OpenColorWindow(ColorPickTarget, AcadColor),
    /// Interaction inside the Select Color panel.
    ColorPicker(crate::ui::color_select::ColorPickerMsg),
    /// Close the colour picker without choosing.
    CloseColorPicker,
    /// Commit the colour chosen in the shared picker, with its `BOOK$NAME`
    /// when it comes from a colour book.
    ColorWindowPick(document::types::Color, Option<String>),
    /// An `.acb` file was read (and, on desktop, copied into the first
    /// colour book folder).
    ColorBookLoaded(Result<crate::ui::color_select::book::ColorBook, String>),
    // ── Raster Image ──────────────────────────────────────────────────────
    /// Open file-picker dialog for IMAGE command (async).
    ImagePick,
    /// Result of the image file picker + pixel dimension decode.
    ImagePickResult(Result<(std::path::PathBuf, u32, u32), String>),
    /// Open file-picker dialog for IMAGEEMBED command (async).
    ImageEmbedPick,
    /// Result of the embedded-image picker: bytes prepared for an OLE2FRAME.
    ImageEmbedPickResult(Result<crate::io::ole_embed::EmbeddedImage, String>),
    /// INSERTOBJ: the picture chosen in the Insert Object window.
    InsertObjectResult(Result<crate::io::ole_embed::EmbeddedImage, String>),
    /// Look for embedded documents saved in their own program.
    OleEditPoll,
    /// An embedded document drawn again after a save: tab id, object.
    OleEditRendered(
        u64,
        document::Handle,
        Result<crate::io::ole_embed::EmbeddedImage, String>,
    ),
    // ── Missing fonts ─────────────────────────────────────────────────────
    /// Download the offered missing fonts from the community repository.
    MissingFontsDownload,
    /// Edit the custom font source URL in the missing-fonts prompt.
    MissingFontsSourceChanged(String),
    /// Close the missing-fonts prompt without downloading.
    MissingFontsDismiss,
    MissingFontsSilence(bool),
    /// Fonts fetched (or failed); payload lists (name, saved-path) pairs.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    MissingFontsResult(Result<Vec<(String, std::path::PathBuf)>, String>),
    // ── PDF Underlay ──────────────────────────────────────────────────────
    /// Open file-picker dialog for PDFATTACH command (async).
    PdfAttachPick,
    /// PDFIMPORT File: pick the PDF to import.
    PdfImportPick,
    /// An edit in one of the PDF dialogs.
    PdfDialog(crate::ui::window::pdf_dialogs::PdfDialogMsg),
    HatchDialog(crate::ui::window::hatch_dialogs::HatchDialogMsg),
    AttdefDialog(crate::ui::window::attdef_dialog::AttdefDialogMsg),
    InsertAttrDialog(crate::ui::window::insert_attr_dialog::InsertAttrMsg),
    /// Block Attribute Manager edits.
    AttMan(crate::ui::window::attman_dialog::AttManMsg),
    FieldDialog(crate::ui::window::field_dialog::FieldDialogMsg),
    /// An edit in the Edit Reference dialog.
    RefEditDialog(crate::ui::window::refedit_dialog::RefEditDialogMsg),
    /// An edit in the Password Protected PDF window.
    PdfPassword(crate::ui::window::pdf_password_dialog::PdfPasswordMsg),
    /// An edit in the Inspection Dimension window.
    DimInspect(crate::ui::window::dim_inspect_dialog::DimInspectMsg),
    /// An edit in a picked table cell window (Cell Borders, Cell Contents).
    TableCellDialog(crate::ui::window::table_cell_dialog::TableCellDialogMsg),
    AnnoScalePrompt(crate::ui::window::anno_scale_prompt::AnnoScaleMsg),
    MTextDialog(crate::ui::window::mtext_dialogs::MTextDialogMsg),
    HyperlinkDialog(crate::ui::window::hyperlink_dialog::HyperlinkMsg),
    /// Check Spelling window.
    Spell(crate::ui::window::spell_dialog::SpellMsg),
    PdfShxDialog(crate::ui::window::pdf_shx_dialog::ShxDialogMsg),
    /// A click or search in the Point Cloud Manager.
    PcManager(crate::ui::window::pc_manager::PcManagerMsg),
    Count(crate::ui::window::count_palette::CountMsg),
    /// The LayerWalk pallet (LAYWALK).
    Laywalk(crate::ui::window::layer_walk::LayerWalkMsg),
    /// The QuickCalc pallet.
    QuickCalc(crate::ui::window::quick_calc::QuickCalcMsg),
    /// The Assistant pallet.
    Assistant(crate::ui::window::assistant::AssistantMsg),
    SheetSet(crate::ui::window::sheet_set::SheetSetMsg),
    ToolPalettes(crate::ui::window::tool_palettes::ToolPalettesMsg),
    PdfImportPickResult(Result<(std::path::PathBuf, std::sync::Arc<Vec<u8>>), String>),
    /// Result of the PDFATTACH file picker.
    PdfAttachPickResult(Result<(std::path::PathBuf, std::sync::Arc<Vec<u8>>), String>),

    // ── XREF ──────────────────────────────────────────────────────────────
    /// Open file-picker dialog for XATTACH command (async).
    XAttachPick,
    /// Result of the XATTACH file picker.
    XAttachPickResult(Result<std::path::PathBuf, String>),
    /// ATTACH: pick a drawing, image or PDF to reference.
    AttachPick,
    /// DWFATTACH / DGNATTACH: pick the file (the result goes the ATTACH way).
    UnderlayAttachPick(document::entities::UnderlayType),
    /// POINTCLOUDATTACH: pick the scan or project (the result goes the
    /// ATTACH way).
    PointCloudAttachPick,
    /// The Reference slide-out's xref fading: amount dragged, drag done,
    /// switch.
    XrefFadeSlide(u8),
    XrefFadeCommit,
    XrefFadeToggle,
    /// Annotate > Text: the find box changed / was submitted.
    RibbonTextFind(String),
    RibbonTextFindSubmit,
    /// Annotate > Text: the text height box changed / was submitted / is
    /// focused by its key tip.
    RibbonTextHeight(String),
    RibbonTextHeightSubmit,
    RibbonTextHeightFocus,
    /// Annotate > Text slide-out: TEXTLAYER picked (empty: Use Current).
    RibbonTextLayer(String),
    /// Annotate > Dimensions: DIMLAYER picked (empty: Use Current).
    RibbonDimLayer(String),
    /// Annotate > Leaders: MLEADERLAYER picked (empty: Use Current).
    RibbonMLeaderLayer(String),
    /// Annotate > Centerlines: CENTERLAYER picked (empty: Use Current).
    RibbonCenterLayer(String),
    /// Result of the ATTACH file picker.
    AttachPickResult(Result<std::path::PathBuf, String>),
    /// An edit in the Attach External Reference dialog.
    XrefAttach(crate::ui::window::xref_attach::XrefAttachMsg),
    /// Result of the dialog's Browse picker.
    XrefAttachBrowseResult(Result<std::path::PathBuf, String>),
    // ── WBLOCK ────────────────────────────────────────────────────────────
    /// Trigger the WBLOCK save dialog for `block_name` (or `*` = selection).
    WblockSave(String),
    /// Result of the WBLOCK save path dialog.
    WblockSaveResult(String, Option<std::path::PathBuf>),
    /// Background extraction/write completion.
    WblockWriteFinished(String, std::path::PathBuf, Result<(), String>),
    // ── DATAEXTRACTION ────────────────────────────────────────────────────
    TableInsertStyle(String),
    /// Insert Table's button to the Table Style dialog.
    TableInsertManageStyles,
    TableInsertField(crate::ui::window::annotation_data::TableInsertField),
    TableInsertApply,
    DataLinkManagerOpen,
    DataLinkNew,
    DataLinkSelect(document::types::Handle),
    DataLinkEdit,
    DataLinkEditCancel,
    DataLinkField(crate::ui::window::annotation_data::DataLinkField),
    DataLinkBrowse,
    DataLinkBrowseResult(Option<std::path::PathBuf>),
    DataLinkSave,
    DataLinkDelete,
    DataLinkInsert,
    DataLinkClose,
    DataExtractionOpen,
    DataExtractionField(crate::ui::window::annotation_data::DataExtractionField),
    DataExtractionBack,
    DataExtractionNext,
    DataExtractionBrowseSettings,
    DataExtractionBrowseSettingsResult(Option<std::path::PathBuf>),
    DataExtractionAddDrawings,
    /// Picked source drawings — `Err` carries a pick/parse failure (web
    /// parses each file on the worker at pick time).
    DataExtractionAddDrawingsResult(Result<Vec<std::path::PathBuf>, String>),
    DataExtractionAddFolder,
    DataExtractionAddFolderResult(Option<std::path::PathBuf>),
    DataExtractionClearSources,
    DataExtractionBrowseOutput,
    DataExtractionBrowseOutputResult(Option<std::path::PathBuf>),
    DataExtractionFinish,
    /// Save the never-saved drawing so the extraction can start.
    DataExtractionSaveDrawing,
    DataExtractionSaveDrawingResult(Option<std::path::PathBuf>),
    /// Save the pre-built CSV string to a file chosen by the user.
    DataExtractionSave(String),
    /// Path chosen (or None = cancelled).
    DataExtractionSaveResult(String, Option<std::path::PathBuf>),
    /// ATTOUT text and the picked output file.
    AttOutSave(String, Option<std::path::PathBuf>),
    /// ATTIN file contents, `None` when the picker was cancelled.
    AttInPicked(Option<String>),
    // ── STL export ────────────────────────────────────────────────────────
    /// Trigger STL export: collect meshes and show save dialog.
    StlExport,
    /// Callback after the user picks (or cancels) the STL save path.
    StlExportPath(Option<std::path::PathBuf>),
    StlExportFinished(std::path::PathBuf, Result<(), String>),
    // ── STEP export ───────────────────────────────────────────────────────
    /// Trigger STEP AP214 export: show save dialog.
    StepExport,
    /// Callback after the user picks (or cancels) the STEP save path.
    StepExportPath(Option<std::path::PathBuf>),
    StepExportFinished(std::path::PathBuf, Result<(), String>),
    // ── FBX export ────────────────────────────────────────────────────────
    /// Trigger FBX export: show save dialog.
    FbxExport,
    /// Callback after the user picks (or cancels) the FBX save path.
    FbxExportPath(Option<std::path::PathBuf>),
    FbxExportFinished(std::path::PathBuf, Result<(), String>),
    // ── OBJ import ────────────────────────────────────────────────────────
    /// Trigger OBJ import: show open-file dialog.
    ObjImport,
    /// Callback after the user picks (or cancels) the OBJ file path.
    /// Web never constructs it — the browser handle's bytes are read directly.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    ObjImportPath(Option<std::path::PathBuf>),
    ObjImportFinished(
        u64,
        std::path::PathBuf,
        Result<Vec<document::entities::mesh::Mesh>, String>,
    ),
    // ── Model import (ACIS/STEP/IGES) ─────────────────────────────────────
    /// Completion of a model import read off the UI thread: the tab it was
    /// started for, that document's generation (the id survives a document
    /// replacement; the generation does not), the file, its format, and the
    /// reader's result.
    ModelImportFinished(
        u64,
        u64,
        String,
        crate::io::model_import::ModelFormat,
        Result<crate::io::model_import::Imported, crate::io::model_import::ImportFailure>,
    ),
    /// Completion of a LANDXMLIMPORT read off the UI thread: the tab it
    /// was started for, that document's generation, the file, and its
    /// survey points or why it cannot be read.
    LandXmlImportFinished(u64, u64, String, Result<Vec<[f64; 3]>, String>),
    // ── Picture import (WMF/SVG) ──────────────────────────────────────────
    /// Completion of a WMF or SVG import read off the UI thread: the tab it
    /// was started for, that document's generation, the file (none for SVG
    /// pasted from the clipboard), its format, whether the insertion prompts
    /// place it, and the reader's result.
    PictureImportFinished(
        u64,
        u64,
        Option<String>,
        crate::io::picture_import::PictureFormat,
        bool,
        crate::io::picture_import::PictureRead,
    ),
    /// An edit in the Import File or DGN Import window.
    ImportDialog(crate::ui::window::import_dialogs::ImportDialogMsg),
}
