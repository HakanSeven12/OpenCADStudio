mod input_file;
mod plugins_panels;
mod layers_xrefs;
mod view_tools;
mod mtext_properties;
mod blocks_settings;
mod styles_plot;

use super::*;

impl OpenCADStudio {
    pub(in crate::app) fn update_inner(&mut self, msg: Message) -> Task<Message> {
        match msg {
            m @ (
                Message::PointsMenu(..)
                | Message::SpaceMouseWake
                | Message::SpaceMouseFrame(..)
                | Message::SpaceMouseFocus(..)
                | Message::SpaceMouseEnabled(..)
                | Message::SpaceMouseMode(..)
                | Message::SpaceMousePanSpeed(..)
                | Message::SpaceMousePanReversed(..)
                | Message::SpaceMousePause
                | Message::SpaceMousePreferences
                | Message::SpaceMouseDriverSettings
                | Message::SpaceMouseDetails
                | Message::Touch(..)
                | Message::TrackpadPinch(..)
                | Message::ControlRequest(..)
                | Message::PollWebControl
                | Message::ControlStep(..)
                | Message::ControlTaskDone(..)
                | Message::ControlScreenshot(..)
                | Message::ControlOffscreenCaptureDone(..)
                | Message::ControlCaptureFrame { .. }
                | Message::ControlToggle
                | Message::ControlCopySetup
                | Message::Graph(..)
                | Message::GraphFileRead(..)
                | Message::GraphLoadRead(..)
                | Message::Tick(..)
                | Message::PollWebFonts
                | Message::WebFontLoaded(..)
                | Message::ApplyWebFont(..)
                | Message::WebUiFontLoaded(..)
                | Message::UiFontSettingsChanged
                | Message::UiFontDetected(..)
                | Message::OpenFile
                | Message::OpenPathPicked(..)
                | Message::FileDropped(..)
                | Message::WebFieldPaste
                | Message::WebFieldPasteText(..)
                | Message::WebFieldCopy
                | Message::WebFieldCopyText(..)
                | Message::SnapOverridePick(..)
                | Message::SnapOverrideMtp
                | Message::SnapOverrideFrom
                | Message::SnapOverrideNone
                | Message::SnapOverrideClose
                | Message::OpenRecent(..)
                | Message::OpenExternal(..)
                | Message::OpenUrl(..)
                | Message::StartSectionSelect(..)
                | Message::ScrollLayoutTabs(..)
                | Message::RecentRemove(..)
                | Message::SetRecentLimit(..)
                | Message::SetRecentView(..)
                | Message::RecentLimitInput(..)
                | Message::OpenCancel
                | Message::FileOpened(..)
                | Message::SaveFile
                | Message::SaveAs
                | Message::SaveDialogFormatChanged(..)
                | Message::SaveDialogFilenameChanged(..)
                | Message::SaveDialogConfirm
                | Message::SaveDialogCancel
                | Message::SaveDialogPathPicked(..)
                | Message::ClearScene
                | Message::SetRenderMode(..)
                | Message::ToggleRenderModeMenu(..)
                | Message::DismissRenderModeMenu
                | Message::PreviewRenderMode(..)
                | Message::SetProjection(..)
                | Message::RibbonSelectTab(..)
                | Message::RibbonWheel(..)
                | Message::RibbonRefit
                | Message::RibbonMenu(..)
                | Message::MenuCommand(..)
                | Message::RibbonQat(..)
                | Message::RibbonMinimize(..)
                | Message::RibbonSlideOut(..)
                | Message::RibbonTick
                | Message::RibbonSearch(..)
                | Message::RibbonReorderPanels { .. }
                | Message::RibbonFloatPanel { .. }
                | Message::RibbonFloat(..)
                | Message::KeyTip(..)
                | Message::RibbonToolClick { .. }
                | Message::PluginFileDialogResult { .. }
                | Message::PanelSurfacePointer { .. }
                | Message::PanelWidgetNodeActivated { .. }
                | Message::PanelWidgetExpandToggled { .. }
                | Message::ExtractionOutdated(..)
                | Message::ImagePick
                | Message::ImagePickResult(..)
                | Message::ImageEmbedPick
                | Message::ImageEmbedPickResult(..)
                | Message::InsertObjectResult(..)
                | Message::OleEditPoll
                | Message::OleEditRendered(..)
                | Message::MissingFontsDownload
                | Message::MissingFontsSourceChanged(..)
                | Message::MissingFontsDismiss
                | Message::MissingFontsSilence(..)
                | Message::MissingFontsResult(..)
                | Message::PdfAttachPick
                | Message::PdfImportPick
                | Message::PdfDialog(..)
                | Message::HatchDialog(..)
                | Message::AttdefDialog(..)
                | Message::InsertAttrDialog(..)
                | Message::AttMan(..)
                | Message::FieldDialog(..)
                | Message::RefEditDialog(..)
                | Message::PdfPassword(..)
                | Message::DimInspect(..)
                | Message::TableCellDialog(..)
                | Message::AnnoScalePrompt(..)
                | Message::SurfaceList(..)
                | Message::PointGroup(..)
                | Message::PointTool(..)
                | Message::MTextDialog(..)
                | Message::HyperlinkDialog(..)
                | Message::Spell(..)
                | Message::PdfShxDialog(..)
                | Message::PcManager(..)
                | Message::Count(..)
                | Message::GeometryCleanup(..)
                | Message::BricscadVisibility(..)
                | Message::FilesPalette(..)
                | Message::Laywalk(..)
                | Message::QuickCalc(..)
                | Message::Assistant(..)
                | Message::SheetSet(..)
                | Message::PdfImportPickResult(..)
                | Message::PdfAttachPickResult(..)
                | Message::XAttachPick
                | Message::XAttachPickResult(..)
                | Message::AttachPick
                | Message::UnderlayAttachPick(..)
                | Message::PointCloudAttachPick
                | Message::AttachPickResult(..)
                | Message::XrefAttach(..)
                | Message::XrefAttachBrowseResult(..)
                | Message::WblockSave(..)
                | Message::WblockSaveResult(..)
                | Message::WblockWriteFinished(..)
                | Message::TableInsertStyle(..)
                | Message::TableInsertManageStyles
                | Message::TableInsertField(..)
                | Message::TableInsertApply
                | Message::DataLinkManagerOpen
                | Message::DataLinkNew
                | Message::DataLinkSelect(..)
                | Message::DataLinkEdit
                | Message::DataLinkEditCancel
                | Message::DataLinkField(..)
                | Message::DataLinkBrowse
                | Message::DataLinkBrowseResult(..)
                | Message::DataLinkSave
                | Message::DataLinkDelete
                | Message::DataLinkInsert
                | Message::DataLinkClose
                | Message::DataExtractionOpen
                | Message::DataExtractionField(..)
                | Message::DataExtractionBack
                | Message::DataExtractionNext
                | Message::DataExtractionBrowseSettings
                | Message::DataExtractionBrowseSettingsResult(..)
                | Message::DataExtractionAddDrawings
                | Message::DataExtractionAddDrawingsResult(..)
                | Message::DataExtractionAddFolder
                | Message::DataExtractionAddFolderResult(..)
                | Message::DataExtractionClearSources
                | Message::DataExtractionBrowseOutput
                | Message::DataExtractionBrowseOutputResult(..)
                | Message::DataExtractionFinish
                | Message::DataExtractionSaveDrawing
                | Message::DataExtractionSaveDrawingResult(..)
                | Message::DataExtractionSave(..)
                | Message::DataExtractionSaveResult(..)
                | Message::AttOutSave(..)
                | Message::AttInPicked(..)
                | Message::TableCellsPaste(..)
                | Message::AlignmentPointFilePicked(..)
                | Message::StlExport
                | Message::StlExportPath(..)
                | Message::StlExportFinished(..)
                | Message::StepExport
                | Message::StepExportPath(..)
                | Message::StepExportFinished(..)
                | Message::FbxExport
                | Message::FbxExportPath(..)
                | Message::FbxExportFinished(..)
                | Message::ObjImport
                | Message::ObjImportPath(..)
                | Message::ObjImportFinished(..)
                | Message::ModelImportFinished(..)
                | Message::LandXmlImportFinished(..)
                | Message::PictureImportFinished(..)
                | Message::ImportDialog(..)
            ) => self.dispatch_input_file(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::DrainPluginRequests => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebFileDropped(..) => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebFileOpened(..) => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebFileCached(..) => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebRecentStored(..) => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebLinkSettled(..) => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebAutosavesScanned(..) => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebAutosaveRestore => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebAutosaveRestoreAll => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebAutosaveDiscard => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebAutosaveDefer => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebAutosaveBytes(..) => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebAutosaveSnapshotFinished(..) => self.dispatch_input_file(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebWblockSerialized { .. } => self.dispatch_input_file(m),
            m @ (
                Message::LayerTranslatorLoad
                | Message::LayerTranslatorLoaded(..)
                | Message::ShortcutsFileRead(..)
                | Message::LayerTranslatorSelectFrom(..)
                | Message::LayerTranslatorSelectTo(..)
                | Message::LayerTranslatorMap
                | Message::LayerTranslatorMapSame
                | Message::LayerTranslatorUnmap(..)
                | Message::PluginDialogTextChange(..)
                | Message::PanelTableCellDraft { .. }
                | Message::PanelTableCellCommitted { .. }
                | Message::PanelTableCellSelected { .. }
                | Message::PluginDialogBoolToggle(..)
                | Message::PluginDialogChoiceSelect(..)
                | Message::PluginDialogColorChange(..)
                | Message::PluginDialogFilePick(..)
                | Message::PluginDialogFilePicked(..)
                | Message::PluginDialogFileTextChange(..)
                | Message::PluginDialogDate(..)
                | Message::PluginDialogTableCellChange(..)
                | Message::PluginDialogTableAddRow(..)
                | Message::PluginDialogTableRemoveRow(..)
                | Message::PluginDialogSectionToggle(..)
                | Message::PluginDialogSubmit
                | Message::PluginDialogCancel
                | Message::PluginDialogButtonClick(..)
                | Message::PluginDialogControlEvent(..)
                | Message::PluginPanelButtonClick(..)
                | Message::PluginPanelTextChange(..)
                | Message::PluginPanelBoolToggle(..)
                | Message::PluginPanelChoiceSelect(..)
                | Message::PluginPanelFilePick(..)
                | Message::PluginPanelFilePicked(..)
                | Message::PluginPanelFileTextChange(..)
                | Message::PluginPanelDate(..)
                | Message::PluginPanelTableCellChange(..)
                | Message::PluginPanelTableAddRow(..)
                | Message::PluginPanelTableRemoveRow(..)
                | Message::PluginPanelSectionToggle(..)
                | Message::TabNew
                | Message::TabSwitch(..)
                | Message::DocTabHover(..)
                | Message::DocTabMenu(..)
                | Message::DocTabLayoutsSettled(..)
                | Message::DocTabLayoutsClose
                | Message::DocTabLayout(..)
                | Message::DocTabsAboveDocks(..)
                | Message::TabReorder { .. }
                | Message::TabClose(..)
                | Message::DocTabSaveAll
                | Message::RetryWindowView(..)
                | Message::DocTabCloseAll
                | Message::DocTabCloseOthers(..)
                | Message::DocTabCopyFullPath(..)
                | Message::DocTabOpenFileLocation(..)
                | Message::CommandInput(..)
                | Message::CommandSubmit
                | Message::Command(..)
                | Message::ScriptLine(..)
                | Message::ScriptPump
                | Message::DataLinkCsvRead(..)
                | Message::CommandAppendChar(..)
                | Message::HeldOverrideRelease
                | Message::CommandBackspace
                | Message::DynTabNext
                | Message::DynTabPrev
                | Message::SplitModelViewport(..)
                | Message::CloseModelViewport
                | Message::CommandHistoryPrev
                | Message::CommandHistoryNext
                | Message::ArrowKeyPressed { .. }
                | Message::CommandLineSpaceProbe
                | Message::CommandLineSpaceResolved
                | Message::CommandLineArrowProbe { .. }
                | Message::CommandLineArrowResolved { .. }
                | Message::CommandHistoryToggle
                | Message::CommandHistoryResizeGrab
                | Message::CommandLineMoveGrab
                | Message::CommandLineMoveMove(..)
                | Message::CommandLineMoveRelease
                | Message::CommandLineDock
                | Message::CommandHistoryResizeMove(..)
                | Message::CommandHistoryResizeRelease
                | Message::CommandHistoryHeightReset
                | Message::CommandLiteralToggle
                | Message::CommandHistoryCopy
                | Message::CommandHistoryClear
                | Message::PerfCopy
                | Message::PerfClear
                | Message::CommandHistoryEdit(..)
                | Message::CommandSuggestionPick(..)
                | Message::ContentSuggestionPick(..)
                | Message::CommandOptionPick(..)
                | Message::CommandHistoryHover(..)
                | Message::ToggleLayers
                | Message::ToggleXrefManager
                | Message::XrefManagerRefresh
                | Message::XrefManagerSelect(..)
                | Message::XrefRowRightClick(..)
                | Message::XrefManagerToggleTree
                | Message::XrefColGrab(..)
                | Message::XrefColMove(..)
                | Message::XrefColRelease
                | Message::XrefSplitGrab
                | Message::XrefSplitMove(..)
                | Message::XrefSplitRelease
                | Message::XrefManagerTogglePreview
                | Message::XrefPreviewRead(..)
                | Message::XrefManagerAttachMenu
                | Message::XrefManagerRefreshMenu
                | Message::XrefManagerPathMenu
                | Message::XrefHelpOpen
                | Message::XrefManagerDismissMenus
                | Message::XrefPathPick
                | Message::XrefPathPickResult(..)
                | Message::XrefManagerReloadAll
                | Message::XrefManagerToggleExpand(..)
                | Message::XrefManagerOp(..)
                | Message::XrefRowOp(..)
                | Message::XrefFindReplacePrompt
                | Message::XrefRowPathPick(..)
                | Message::XrefRowFindReplacePrompt(..)
                | Message::XrefRowChangePathEnter
                | Message::XrefRowChangePathLeave
                | Message::LayerStateManagerOpen
                | Message::CommandFinalize
                | Message::CommandSpace
                | Message::CommandEscape
            ) => self.dispatch_plugins_panels(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::PluginGripPump => self.dispatch_plugins_panels(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::LayerTranslatorLoadedWeb(..) => self.dispatch_plugins_panels(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::ScriptFilePicked(..) => self.dispatch_plugins_panels(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::CenterlineLinetypeFilePicked(..) => self.dispatch_plugins_panels(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::DataLinkCsvPicked(..) => self.dispatch_plugins_panels(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::CommandHistoryCopied(..) => self.dispatch_plugins_panels(m),
            m @ (
                Message::LayerFileRead(..)
                | Message::LayerTranslatorForceByLayer(..)
                | Message::LayerTranslatorWriteLog(..)
                | Message::LayerTranslatorSaveMappings
                | Message::LayerTranslatorLoadMappings
                | Message::LayerTranslatorMappingsPath(..)
                | Message::LayerTranslatorTranslate
                | Message::LayerToggleVisible(..)
                | Message::LayerToggleLock(..)
                | Message::LayerToggleFreeze(..)
                | Message::LayerTogglePlot(..)
                | Message::LayerSort(..)
                | Message::LayerToggleVpFreeze(..)
                | Message::LayerNew
                | Message::LayerImport(..)
                | Message::LayerNewVpFrozen
                | Message::LayerMenu(..)
                | Message::LayerDelete
                | Message::LayerDeleteConfirm
                | Message::LayerSetCurrent
                | Message::LayerSelect(..)
                | Message::LayerRenameStart(..)
                | Message::LayerRenameEdit(..)
                | Message::LayerColorPickerToggle(..)
                | Message::LayerColorMorePalette
                | Message::LayerColorSet(..)
                | Message::LayerLinetypeSet(..)
                | Message::LayerLineweightSet(..)
                | Message::LayerTransparencyEdit(..)
                | Message::LayerTransparencyCommit
                | Message::LayerDescriptionEdit(..)
                | Message::LayerDescriptionCommit
                | Message::LayerDescriptionFocus(..)
                | Message::LayerTransparencyFocus(..)
                | Message::LayerRenameCommit
                | Message::LayerRenameFocus(..)
                | Message::LayerStateManagerSelect(..)
                | Message::LayerStateManagerNew
                | Message::LayerStateManagerFilter(..)
                | Message::LayerStateManagerName(..)
                | Message::LayerStateManagerDescription(..)
                | Message::LayerStateManagerSave
                | Message::LayerStateManagerRestore
                | Message::RibbonLayerStateRestore(..)
                | Message::LayerStateManagerDelete
                | Message::LayerStateManagerEdit
                | Message::LayerStateEditorMaskToggle(..)
                | Message::LayerStateEditorLayerFlagToggle(..)
                | Message::LayerStateEditorLayerColorToggle(..)
                | Message::LayerStateEditorLayerColor(..)
                | Message::LayerStateEditorLayerLinetype(..)
                | Message::LayerStateEditorLayerLineweight(..)
                | Message::LayerStateEditorLayerPlotStyle(..)
                | Message::LayerStateEditorLayerTransparency(..)
                | Message::LayerStateEditorName(..)
                | Message::LayerStateEditorDescription(..)
                | Message::LayerStateEditorCurrentLayer(..)
                | Message::LayerStateEditorFilter(..)
                | Message::LayerStateEditorSave
                | Message::LayerStateEditorCancel
                | Message::CursorMoved(..)
                | Message::ViewportClick(..)
                | Message::ViewportMove(..)
                | Message::ViewportLeftPressAt(..)
                | Message::ViewportLeftPress
                | Message::ViewportLeftRelease
                | Message::ViewportRightPress
                | Message::ViewportRightRelease
                | Message::ViewportMiddlePress
                | Message::ViewportMiddleRelease
                | Message::ViewportScroll(..)
                | Message::ViewportExit
                | Message::PaneResized(..)
                | Message::PaneClicked(..)
                | Message::PaneDragged(..)
                | Message::PaneMove(..)
                | Message::PanePress(..)
                | Message::PanePressAt(..)
                | Message::PaneRelease(..)
                | Message::PaneRightPress(..)
                | Message::PaneRightRelease(..)
                | Message::PaneMiddlePress(..)
                | Message::PaneMiddleRelease(..)
                | Message::PaneScroll(..)
                | Message::PaneMoveStart
                | Message::ViewportHighlightToggle
                | Message::ViewportHighlightOpacity(..)
                | Message::ViewportOutlineSwitchHover(..)
                | Message::ViewCubeSnap(..)
                | Message::ViewCubeSnapWorld(..)
                | Message::ViewCubeHome
                | Message::ViewCubeRoll(..)
                | Message::ViewCubeNudge(..)
                | Message::SetViewcubeUcs(..)
                | Message::GripMenuPick(..)
                | Message::VisibilityPick(..)
                | Message::TableRowPick(..)
                | Message::TestVisibilityPick(..)
                | Message::TestLookupPick(..)
                | Message::GripDwellTick
                | Message::LayoutSettled
                | Message::PluginFrameDrawn
                | Message::PluginFrameResume
                | Message::PluginRunWake
                | Message::PluginRunCancel
                | Message::HoverDwellTick
                | Message::SnapDwellTick
                | Message::MoveCoalesceTick
                | Message::BoxPreviewTick
                | Message::PropertiesRefreshTick
                | Message::InteractionIndexReady { .. }
                | Message::WindowResized(..)
                | Message::WindowOpened(..)
                | Message::ToggleSnapEnabled
                | Message::ToggleSnap3dEnabled
                | Message::ToggleGridSnap
                | Message::ToggleIsometricDrafting
                | Message::SetIsoPlane(..)
                | Message::CycleIsoPlane
                | Message::ResetDraftingRotation
                | Message::ToggleViewCube
                | Message::ToggleProperties
                | Message::ToggleGraphicAttributes
                | Message::ToggleFileTabs
                | Message::ToggleLayoutTabs
                | Message::ToggleGrid
                | Message::ToggleOrtho
                | Message::ToggleLineweightDisplay
                | Message::TogglePolar
                | Message::SetPolarAngle(..)
                | Message::TogglePolarPopup
                | Message::ClosePolarPopup
                | Message::PolarCustomInput(..)
                | Message::SubmitPolarCustom
                | Message::SetAnnotationScale(..)
                | Message::SetViewportScale(..)
                | Message::ToggleAnnotationVisibility
                | Message::ToggleAnnotationAutoAdd
                | Message::SyncViewportAnnotationScale
                | Message::ToggleViewportLock
                | Message::ToggleScalePopup
                | Message::CloseScalePopup
                | Message::ScaleManagerOpen
                | Message::AnnoObjectScaleOpen
                | Message::PropHyperlinkOpen
                | Message::HyperlinkUrlChanged(..)
                | Message::HyperlinkDescriptionChanged(..)
                | Message::HyperlinkApply
                | Message::HyperlinkRemove
                | Message::HyperlinkCancel
                | Message::AnnoObjectScaleToggle(..)
                | Message::AnnoObjectScaleCommon(..)
                | Message::AnnoObjectScaleSelect(..)
                | Message::AnnoObjectScaleAdd
                | Message::AnnoObjectScaleAddToggle(..)
                | Message::AnnoObjectScaleAddOk
                | Message::AnnoObjectScaleAddCancel
                | Message::AnnoObjectScaleDelete
                | Message::AnnoObjectScaleOk
                | Message::CycleCoordsMode
                | Message::ResolveOneParametricConflict
                | Message::ToggleDynInput
                | Message::ToggleOTrack
                | Message::WindowCloseRequested(..)
                | Message::OsWindowClosed(..)
            ) => self.dispatch_layers_xrefs(m),
            m @ (
                Message::ScaleManagerSelect(..)
                | Message::ScaleManagerMove(..)
                | Message::ScaleManagerReset
                | Message::ScaleManagerResetChoice(..)
                | Message::ScaleManagerOk
                | Message::ScaleManagerNew
                | Message::ScaleManagerCopy
                | Message::ScaleRenameStart(..)
                | Message::ScaleRenameEdit(..)
                | Message::ScaleRenameCommit
                | Message::ScaleManagerDelete
                | Message::ScaleManagerDeleteChoice(..)
                | Message::ScaleManagerEdit
                | Message::ScaleManagerEditCancel
                | Message::ScaleManagerSetCurrent
                | Message::ScaleManagerApply
                | Message::ScaleManagerPaperBuf(..)
                | Message::ScaleManagerDrawingBuf(..)
                | Message::ToggleLayoutList
                | Message::CloseLayoutList
                | Message::ToggleStatusBarMenu
                | Message::CloseStatusBarMenu
                | Message::ToggleStatusPill(..)
                | Message::ToggleCleanScreen
                | Message::ToggleTransparencyDisplay
                | Message::ToggleQuickProperties
                | Message::ToggleSelectionCycling
                | Message::CycleSelect(..)
                | Message::CycleHover(..)
                | Message::CycleHoverExit(..)
                | Message::CycleCancel
                | Message::ToggleSelectionFilterPopup
                | Message::CloseSelectionFilterPopup
                | Message::ToggleSelectionFilterType(..)
                | Message::SelectionFilterSelectAll
                | Message::SelectionFilterClearAll
                | Message::ToggleUnitsPopup
                | Message::CloseUnitsPopup
                | Message::SetDrawingUnits(..)
                | Message::SetLinearFormat(..)
                | Message::OpenDrawingUnits
                | Message::DrawingUnitsField(..)
                | Message::DrawingUnitsApply
                | Message::BlockDefName(..)
                | Message::BlockDefNameSelect(..)
                | Message::BlockDefBaseOnScreen(..)
                | Message::BlockDefPickPoint
                | Message::BlockDefBaseX(..)
                | Message::BlockDefBaseY(..)
                | Message::BlockDefBaseZ(..)
                | Message::BlockDefObjectsOnScreen(..)
                | Message::BlockDefSelectObjects
                | Message::BlockDefQuickSelect
                | Message::BlockDefObjectMode(..)
                | Message::BlockDefAnnotative(..)
                | Message::BlockDefMatchOrientation(..)
                | Message::BlockDefScaleUniformly(..)
                | Message::BlockDefAllowExploding(..)
                | Message::BlockDefUnit(..)
                | Message::BlockDefDescription(..)
                | Message::BlockDefDescriptionAction(..)
                | Message::BlockDefHyperlink
                | Message::BlockDefApply
                | Message::BlockDefConfirmRedefine(..)
                | Message::BlockDefDismissError
                | Message::BlockDefHelp
                | Message::BlockDefOpenInEditor(..)
                | Message::BlockDefCreateEmpty
                | Message::WblockSourceMode(..)
                | Message::WblockBlockName(..)
                | Message::WblockBlockSelect(..)
                | Message::WblockPickPoint
                | Message::WblockBaseX(..)
                | Message::WblockBaseY(..)
                | Message::WblockBaseZ(..)
                | Message::WblockSelectObjects
                | Message::WblockQuickSelect
                | Message::WblockObjectMode(..)
                | Message::WblockFilePath(..)
                | Message::WblockBrowsePath
                | Message::WblockBrowsePathResult(..)
                | Message::WblockUnit(..)
                | Message::WblockApply
                | Message::WblockDismissError
                | Message::WblockHelp
                | Message::ToleranceDialogField(..)
                | Message::ToleranceDialogToggle(..)
                | Message::ToleranceDialogApply
                | Message::ToleranceDialogOk
                | Message::ToggleIsolatePopup
                | Message::CloseIsolatePopup
                | Message::ToggleSnap(..)
                | Message::ToggleSnapPopup
                | Message::CloseSnapPopup
                | Message::SnapSelectAll
                | Message::SnapClearAll
                | Message::DraftingSettingsTabChanged(..)
                | Message::OpenDraftingSettings(..)
                | Message::DraftingSettingsToggleGrid
                | Message::DraftingSettingsToggleSnap
                | Message::DraftingSettingsSnapXChanged(..)
                | Message::DraftingSettingsSnapYChanged(..)
                | Message::DraftingSettingsGridXChanged(..)
                | Message::DraftingSettingsGridYChanged(..)
                | Message::DraftingSettingsGridMajorChanged(..)
                | Message::DraftingSettingsToggleAdaptiveGrid
                | Message::DraftingSettingsToggleBeyondLimits
                | Message::DraftingSettingsToggleGridAxes
                | Message::DraftingSettingsToggleEqualSnap
                | Message::DraftingSettingsToggleIsometric
                | Message::DraftingSettingsSetIsoPlane(..)
                | Message::DraftingSettingsResetRotation
                | Message::DraftingSettingsTogglePolar
                | Message::DraftingSettingsToggleOrtho
                | Message::DraftingSettingsToggleOsnap
                | Message::DraftingSettingsOsnapApertureChanged(..)
                | Message::DraftingSettingsToggleOtrack
                | Message::DraftingSettingsToggleSnapMode(..)
                | Message::DraftingSettingsToggleSnapMode3d(..)
                | Message::DraftingSettingsSnapSelectAll
                | Message::DraftingSettingsSnapClearAll
                | Message::DraftingSettingsToggle3dOsnap
                | Message::DraftingSettingsToggleDynInput
                | Message::DraftingSettingsDynPrefs(..)
                | Message::DraftingSettingsToggleQuickProps
                | Message::DraftingSettingsQuickPropsFloat(..)
                | Message::QuickPropertiesGrab
                | Message::DraftingSettingsToggleSelCycling
                | Message::DraftingSettingsApply
                | Message::DraftingSettingsOk
                | Message::DraftingSettingsClose
                | Message::DraftingSettingsCloseDiscard
                | Message::DraftingSettingsCloseKeep
                | Message::AutoConstrainSelectRow(..)
                | Message::AutoConstrainToggleKind(..)
                | Message::AutoConstrainMoveUp
                | Message::AutoConstrainMoveDown
                | Message::AutoConstrainSelectAll
                | Message::AutoConstrainClearAll
                | Message::AutoConstrainReset
                | Message::AutoConstrainToggleTangentPoint
                | Message::AutoConstrainTogglePerpendicularIntersection
                | Message::AutoConstrainDistanceChanged(..)
                | Message::AutoConstrainAngleChanged(..)
                | Message::AutoConstrainApply
                | Message::AutoConstrainOk
                | Message::AutoConstrainCancel
                | Message::ToggleRibbonDropdown(..)
                | Message::ToggleRibbonPanel(..)
                | Message::CloseRibbonDropdown
                | Message::DropdownSelectItem { .. }
                | Message::DeleteSelected
                | Message::SetModifiers { .. }
                | Message::MTextEdit(..)
                | Message::MTextFmt(..)
                | Message::MTextHeight(..)
                | Message::MTextStyle(..)
                | Message::MTextFontPicker(..)
                | Message::MTextResetToStyle(..)
                | Message::MTextNewStyleOpen
                | Message::MTextNewStyleName(..)
                | Message::MTextNewStyleCreate
                | Message::MTextUpdateStyleFromSelection
                | Message::MTextStyleManager
                | Message::MTextOblique(..)
                | Message::MTextWidth(..)
                | Message::MTextCharSpace(..)
                | Message::MTextUndo
                | Message::MTextInsertField
                | Message::MTextRedo
                | Message::MTextStack
                | Message::MTextScript(..)
                | Message::MTextClearFormatting
                | Message::MTextInsert(..)
                | Message::MTextAnnotative(..)
                | Message::MTextRotation(..)
                | Message::MTextMenu(..)
                | Message::MTextMenuPick(..)
                | Message::MTextContextOpen(..)
                | Message::MTextContextGroup(..)
                | Message::MTextContextClose
                | Message::MTextContextRun(..)
                | Message::MTextSmartTag(..)
                | Message::MTextPasteAs(..)
                | Message::MTextList(..)
                | Message::MTextTab
                | Message::MTextLineSpacingInput(..)
                | Message::MTextLineSpacingMode(..)
                | Message::MTextMaskToggle
                | Message::MTextMaskColor(..)
                | Message::MTextMaskScale(..)
                | Message::MTextDirection(..)
                | Message::MTextFractionOpen
                | Message::MTextFractionPart(..)
                | Message::MTextFractionKind(..)
                | Message::MTextFractionInsert
                | Message::MTextRectWidthField(..)
                | Message::MTextBoxDrag(..)
                | Message::MTextHeightDrag(..)
                | Message::MTextColumnMode(..)
                | Message::MTextColumnCount(..)
                | Message::MTextColumnWidth(..)
                | Message::MTextColumnGutter(..)
                | Message::MTextColumnHeight(..)
                | Message::MTextColumnFlowReversed(..)
                | Message::MTextParagraphNumber(..)
                | Message::MTextFindText(..)
                | Message::MTextReplaceText(..)
                | Message::MTextFindNext
                | Message::MTextReplaceNext
                | Message::MTextReplaceAll
                | Message::MTextColorChanged(..)
                | Message::MTextColorPickerToggle
                | Message::MTextJustify(..)
                | Message::MTextAlign(..)
                | Message::MTextLineSpacing(..)
                | Message::MTextShowPreview(..)
                | Message::MTextSelStart(..)
                | Message::MTextImport(..)
                | Message::MTextImportRead(..)
                | Message::TableEditorClick(..)
                | Message::XrefFadeSlide(..)
                | Message::XrefFadeCommit
                | Message::XrefFadeToggle
                | Message::RibbonTextFind(..)
                | Message::RibbonTextFindSubmit
                | Message::RibbonTextHeight(..)
                | Message::RibbonTextHeightSubmit
                | Message::RibbonTextHeightFocus
                | Message::RibbonTextLayer(..)
                | Message::RibbonDimLayer(..)
                | Message::RibbonMLeaderLayer(..)
                | Message::RibbonCenterLayer(..)
            ) => self.dispatch_view_tools(m),
            m @ (
                Message::PasteShortcut
                | Message::PasteShortcutResolved(..)
                | Message::PasteSpecial(..)
                | Message::PasteAsHyperlink(..)
                | Message::SystemClipboardPaste(..)
                | Message::SystemClipboardPasteImage(..)
                | Message::SelectAllShortcut
                | Message::FindReplaceOpen
                | Message::Find(..)
                | Message::OverkillSettingsChanged(..)
                | Message::OverkillToleranceChanged(..)
                | Message::OverkillOk
                | Message::MTextPasteClip(..)
                | Message::TextInlinePasteClip(..)
                | Message::CommandPasteMarkup
                | Message::RibbonLayerChanged(..)
                | Message::RibbonColorChanged(..)
                | Message::RibbonLayerColorChanged { .. }
                | Message::RibbonColorPaletteToggle
                | Message::RibbonLinetypeChanged(..)
                | Message::RibbonLineweightChanged(..)
                | Message::RibbonStyleChanged { .. }
                | Message::PropLayerChanged(..)
                | Message::PropSelectionGroupChanged(..)
                | Message::PropColorChanged(..)
                | Message::PropLwChanged(..)
                | Message::PropFieldLwChanged { .. }
                | Message::PropLinetypeChanged(..)
                | Message::PropBoolToggle(..)
                | Message::PropVertexStep(..)
                | Message::PropHatchPatternChanged(..)
                | Message::PropHatchPatternPickerToggle(..)
                | Message::PropHatchPatternSearchChanged(..)
                | Message::PropHatchPatternFocus(..)
                | Message::PropHatchPatternNavigate(..)
                | Message::PropHatchPatternConfirm
                | Message::PropGeomChoiceChanged { .. }
                | Message::PropChoiceToggle { .. }
                | Message::PropChoiceDismiss { .. }
                | Message::PropGeomInput { .. }
                | Message::PropGeomCommit(..)
                | Message::PropBlockInput { .. }
                | Message::PropBlockCommit(..)
                | Message::PropBlockChoice { .. }
                | Message::PropBlockPickerToggle { .. }
                | Message::PropBlockPickerDismiss { .. }
                | Message::PropGroupToggle(..)
                | Message::PropSectionToggle(..)
                | Message::PropRowSelect(..)
                | Message::PropEditChoiceToggle(..)
                | Message::PropAttrInput { .. }
                | Message::PropAttrCommit(..)
                | Message::PropPluginParamInput { .. }
                | Message::PropPluginParamCommit(..)
                | Message::PropSyncActive(..)
                | Message::PropPointerPressed
                | Message::PropColorPickerToggle
                | Message::PropPickerSurface(..)
                | Message::PropBgColorPickerToggle
                | Message::PropBgColorChanged(..)
                | Message::PropColorFieldToggle(..)
                | Message::PropColorFieldChanged { .. }
                | Message::PropColorPickerClose
                | Message::LayoutSwitch(..)
                | Message::BlockEditSwitch(..)
                | Message::LayoutReorder { .. }
                | Message::LayoutCreate
                | Message::LayoutCopy(..)
                | Message::LayoutPageSetup(..)
                | Message::LayoutPlot(..)
                | Message::LayoutDelete(..)
                | Message::LayoutRenameStart(..)
                | Message::LayoutRenameEdit(..)
                | Message::LayoutRenameCommit
                | Message::LayoutRenameCancel
                | Message::LayoutManagerOpen
                | Message::LayoutManagerClose
                | Message::LayoutManagerSelect(..)
                | Message::LayoutManagerRenameBuf(..)
                | Message::LayoutManagerRenameCommit
                | Message::LayoutManagerNew
                | Message::LayoutManagerDelete
                | Message::LayoutManagerMoveLeft
                | Message::LayoutManagerMoveRight
                | Message::LayoutManagerSetCurrent
                | Message::SetTheme(..)
                | Message::ShortcutsPanelOpen
                | Message::ShortcutsPanelClose
                | Message::ShortcutEditorInput { .. }
                | Message::ShortcutEditorSuggestionPick { .. }
                | Message::ShortcutEditorSuggestionDismiss
                | Message::ShortcutEditorFilter(..)
                | Message::ShortcutEditorAdd
                | Message::ShortcutEditorRemove(..)
                | Message::ShortcutEditorApply
                | Message::ShortcutEditorApplyExit
                | Message::ShortcutEditorDraftAccept
                | Message::ShortcutEditorImport
                | Message::ShortcutEditorImported(..)
                | Message::ShortcutEditorResetAsk
                | Message::ShortcutEditorResetConfirm
                | Message::ShortcutEditorResetDeny
                | Message::ShortcutEditorCloseDiscard
                | Message::ShortcutEditorCloseKeep
                | Message::ShortcutCaptureStart(..)
                | Message::ShortcutCaptureKey(..)
                | Message::ShortcutCaptureClear
                | Message::ShortcutCaptureCancel
                | Message::ShortcutPressed(..)
                | Message::AliasEditorOpen
                | Message::AliasEditorImport
                | Message::KeyListExport(..)
                | Message::ContextMenuEdit(..)
                | Message::RibbonCustomizeOpen
                | Message::RibbonCustomize(..)
                | Message::HatchPanel(..)
                | Message::KeyListExportTo(..)
                | Message::AliasEditorInput { .. }
                | Message::SelectSimilar
                | Message::InvertSelection
                | Message::MTextFieldSubmit
                | Message::MTextSelTo(..)
                | Message::MTextCaretMove(..)
                | Message::MTextSelEnd
                | Message::MTextCaretBlink
                | Message::MTextOk
                | Message::MTextApply
                | Message::MTextCancel
                | Message::TextInlineInput(..)
                | Message::TextInlineOk
                | Message::ContextMenuPick(..)
                | Message::ContextMenuSubmenuToggle(..)
                | Message::ContextMenuNavigate(..)
                | Message::Quad(..)
                | Message::QuadOption(..)
                | Message::DrawOrderPickRef(..)
                | Message::QSelectOpen
                | Message::QSelectClose
                | Message::QSelectSetScope(..)
                | Message::QSelectSetType(..)
                | Message::QSelectSetProperty(..)
                | Message::QSelectSetOperator(..)
                | Message::QSelectSetValue(..)
                | Message::QSelectSetMode(..)
                | Message::QSelectSetResult(..)
                | Message::QSelectSetShow(..)
                | Message::QSelectSampleToggle(..)
                | Message::QSelectSampleAdd
                | Message::QSelectSampleCancel
                | Message::QSelectAddCondition
                | Message::QSelectPickObject
                | Message::QSelectWindow
                | Message::QSelectEditCondition(..)
                | Message::QSelectRemoveCondition(..)
                | Message::QSelectClearConditions
                | Message::QSelectCancelEdit
                | Message::QSelectApply
                | Message::QSelectAdvanced
                | Message::FilterDialog(..)
            ) => self.dispatch_mtext_properties(m),
            m @ (
                Message::RecoveryClose
                | Message::RecoveryAttempt
                | Message::RecoveryDecline
                | Message::RecoverySaveAs
                | Message::RecoveryShowLog
                | Message::OptionsOpen
                | Message::OptionsTabChanged(..)
                | Message::CursorSizeChanged(..)
                | Message::PickBoxChanged(..)
                | Message::ApertureChanged(..)
                | Message::DoubleClickBlockRefeditChanged(..)
                | Message::DoubleClickBlockAtteditChanged(..)
                | Message::CursorTypeChanged(..)
                | Message::LineweightDisplayScaleChanged(..)
                | Message::CrosshairColorChanged(..)
                | Message::SnapMarkerColorChanged(..)
                | Message::CommandTextColorChanged(..)
                | Message::QnewTemplateChanged(..)
                | Message::QnewTemplateBrowse
                | Message::TransmittalLayoutChanged(..)
                | Message::NewFromTemplate(..)
                | Message::StartTemplates(..)
                | Message::DefaultSaveFormatChanged(..)
                | Message::OptionsThemeChanged(..)
                | Message::OptionsThemeColorChanged(..)
                | Message::ModelSpaceModeChanged(..)
                | Message::ModelSpaceBgChanged(..)
                | Message::PaperSpaceBgChanged(..)
                | Message::DeskSpaceBgChanged(..)
                | Message::GridOpacityChanged(..)
                | Message::SelectionAreaToggled(..)
                | Message::SelectionOpacityChanged(..)
                | Message::SelectionHighlightOpacityChanged(..)
                | Message::SelectionHighlightWidenChanged(..)
                | Message::SelectionWindowColorChanged(..)
                | Message::SelectionCrossingColorChanged(..)
                | Message::SelectionHighlightColorChanged(..)
                | Message::GripSizeChanged(..)
                | Message::SnapMarkerSizeChanged(..)
                | Message::AutoSnapMagnetChanged(..)
                | Message::GripColorChanged(..)
                | Message::GripHotChanged(..)
                | Message::GripHoverChanged(..)
                | Message::GripObjectLimitChanged(..)
                | Message::SelectionEffectToggled(..)
                | Message::SelectionPreviewIdleToggled(..)
                | Message::SelectionPreviewCommandToggled(..)
                | Message::ShiftToAddToggled(..)
                | Message::PickDragModeChanged(..)
                | Message::PickAutoBitToggled(..)
                | Message::PickFirstToggled(..)
                | Message::SaveTimeChanged(..)
                | Message::BackupOnSaveChanged(..)
                | Message::CheckMissingFontsChanged(..)
                | Message::PageSetupImportFile(..)
                | Message::PageSetupOnNewLayoutChanged(..)
                | Message::TextFillChanged(..)
                | Message::ClipromptLinesChanged(..)
                | Message::CommandLineFadeChanged(..)
                | Message::CommandLineOpacityChanged(..)
                | Message::CommandLineFontChanged(..)
                | Message::CommandLineFontSizeChanged(..)
                | Message::ZoomWheelReversedChanged(..)
                | Message::NavigationStyleChanged(..)
                | Message::ToggleDynamicUcs
                | Message::TouchpadPanChanged(..)
                | Message::KeypadDecimalPointChanged(..)
                | Message::MenuBarChanged(..)
                | Message::RibbonVisibleChanged(..)
                | Message::RibbonCenteredChanged(..)
                | Message::RibbonWheelTabsChanged(..)
                | Message::HatchAssociativeChanged(..)
                | Message::UiScaleChanged(..)
                | Message::ZoomFactorChanged(..)
                | Message::ZoomFactorInputChanged(..)
                | Message::RightClickModeChanged(..)
                | Message::RightClickHoldMsChanged(..)
                | Message::TextEditModeChanged(..)
                | Message::DimContinueModeChanged(..)
                | Message::QdimSnapPriorityChanged(..)
                | Message::AnnoAutoScaleChanged(..)
                | Message::SnapAngleInputChanged(..)
                | Message::PolarIncrementChanged(..)
                | Message::ShowViewCubeChanged(..)
                | Message::ShowUcsIconChanged(..)
                | Message::UcsIconAtOriginChanged(..)
                | Message::SelectionCyclingChanged(..)
                | Message::OpenFolder(..)
                | Message::ChooseFolder(..)
                | Message::FolderChosen(..)
                | Message::IsolinesChanged(..)
                | Message::IsolinesReleased
                | Message::DispSilhChanged(..)
                | Message::SurfaceUChanged(..)
                | Message::SurfaceVChanged(..)
                | Message::SurfaceTypeChanged(..)
                | Message::SolidHistChanged(..)
                | Message::ShowHistChanged(..)
                | Message::RestoreModelSpaceDisplayDefaults
                | Message::RestoreSelectionVisualDefaults
                | Message::FileAssocChanged(..)
                | Message::ShowConstraintValuesChanged(..)
                | Message::LanguageChanged(..)
                | Message::LayerNameColGrab
                | Message::LayerColGrab(..)
                | Message::LayerColumnsShowAll
                | Message::LayerColumnToggle(..)
                | Message::LayerColumnMove(..)
                | Message::LayerShow(..)
                | Message::LayerTreeRenameEdit(..)
                | Message::LayerTreeRenameCommit
                | Message::LayerTreeRenameCancel
                | Message::LayerNodeEye(..)
                | Message::LayerSaveSearch
                | Message::LayerRibbonFollows(..)
                | Message::LayerMoreMenu(..)
                | Message::LayerMoreRun(..)
                | Message::LayerSearchOpen(..)
                | Message::LayerDragEnd
                | Message::LayerDragOver(..)
                | Message::LayerDrop(..)
                | Message::LayerTreeSection(..)
                | Message::LayerInvert(..)
                | Message::LayerEye(..)
                | Message::LayerFilterEdit(..)
                | Message::LayerTreeAction(..)
                | Message::OptionsApply
                | Message::OptionsRestoreDefaults
                | Message::OptionsOk
                | Message::OptionsClose
                | Message::OptionsCloseDiscard
                | Message::OptionsCloseKeep
                | Message::RibbonLayerFilterChanged(..)
                | Message::LayerManagerFilterChanged(..)
                | Message::LayerGroupNew
                | Message::LayerGroupAdd(..)
                | Message::LayerGroupRemove
                | Message::LayerGroupDelete
                | Message::AliasEditorImported(..)
                | Message::AliasEditorAdd
                | Message::AliasEditorRemove(..)
                | Message::AliasEditorApply
                | Message::AliasEditorApplyExit
                | Message::AliasEditorDraftAccept
                | Message::AliasEditorDraftCancel
                | Message::AliasEditorResetAsk
                | Message::AliasEditorResetConfirm
                | Message::AliasEditorResetDeny
                | Message::AliasEditorCloseDiscard
                | Message::AliasEditorCloseKeep
                | Message::NamedParametersOpen
                | Message::BlockEditorSettingsInput(..)
                | Message::VisualStylesCreaseInput(..)
                | Message::VisualStylesCreaseApply
                | Message::VisualStylesOccludedEdges(..)
                | Message::BlockConstraintColorInput(..)
                | Message::ToggleBlockConstraintStatus
                | Message::BlockEditorSettingsReset
                | Message::BlockEditorSettingsApply
                | Message::BlockCycleSelect(..)
                | Message::BlockCycleEnable(..)
                | Message::BlockCycleMove(..)
                | Message::BlockCycleApply
                | Message::BlockActionBarSelect(..)
                | Message::BlockActionBarEdit(..)
                | Message::BlockActionBarMove(..)
                | Message::BlockActionBarHide(..)
                | Message::NamedParametersInput { .. }
                | Message::NamedParametersAdd
                | Message::NamedParametersRemove(..)
                | Message::NamedParametersApply
                | Message::PropParamInput { .. }
                | Message::PropParamCommit { .. }
                | Message::PropParamDelete(..)
                | Message::PropParamAddNew
                | Message::PropConstraintLinkClick(..)
                | Message::PropConstraintDelete(..)
                | Message::AboutOpen
                | Message::GpuWarningOpen
                | Message::GpuWarningSilence
                | Message::CloseModal
                | Message::AttrEditorOpen(..)
                | Message::AttrEditorTab(..)
                | Message::AttrEditorSelect(..)
                | Message::AttrEditorInput { .. }
                | Message::AttrEditorTextStyle(..)
                | Message::AttrEditorJustify(..)
                | Message::AttrEditorHeight(..)
                | Message::AttrEditorRotation(..)
                | Message::AttrEditorWidth(..)
                | Message::AttrEditorOblique(..)
                | Message::AttrEditorBackwards(..)
                | Message::AttrEditorUpsideDown(..)
                | Message::AttrEditorLayer(..)
                | Message::AttrEditorLinetype(..)
                | Message::AttrEditorColor(..)
                | Message::AttrEditorLineweight(..)
                | Message::AttrEditorApply
                | Message::AttrEditorOk
                | Message::AttrEditorCancel
                | Message::AttrEditorHelp
                | Message::AttrEditorSelectBlock
                | Message::AttrEditorAnswer(..)
                | Message::AttrEditorDismissError
                | Message::AttrEditorEditValue
                | Message::AttrEditorInsertField
                | Message::AttrEditorBoundary(..)
                | Message::AttrEditorAnnotative(..)
                | Message::AttrEditorColorValue(..)
                | Message::ModalGrab
                | Message::ModalDragMove(..)
                | Message::ModalDragRelease
                | Message::MTextToolbarGrab
                | Message::MTextToolbarMove(..)
                | Message::MTextToolbarRelease
                | Message::AboutCopyInfo
                | Message::PluginManagerOpen
                | Message::PluginManagerClose
                | Message::SetPluginEnabled(..)
                | Message::PluginRepoInput(..)
                | Message::PluginSearchInput(..)
                | Message::PluginRepoAdd
                | Message::ModalResizeGrab
                | Message::ModalContentResized(..)
                | Message::TitleBar(..)
                | Message::ToolWindow(..)
                | Message::ToolWindowTask(..)
                | Message::ToolWindowGrab(..)
                | Message::ToolWindowResizeGrab(..)
                | Message::ToolWindowDragMove(..)
                | Message::ToolWindowDragRelease
                | Message::ToolWindowRaise(..)
                | Message::OsWindowReturn(..)
                | Message::OsWindowKey(..)
                | Message::OsWindowResized(..)
                | Message::OsWindowMoved(..)
                | Message::OsWindowScale(..)
            ) => self.dispatch_blocks_settings(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::GraphicsBackendChanged(..) => self.dispatch_blocks_settings(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::GraphicsCompatToggled(..) => self.dispatch_blocks_settings(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::GraphicsLegacyGlToggled(..) => self.dispatch_blocks_settings(m),
            m @ (
                Message::GraphicAttributes(..)
                | Message::UnsavedDialogSave
                | Message::UnsavedDialogDiscard
                | Message::UnsavedDialogCancel
                | Message::BlockEditChangesSave
                | Message::BlockEditChangesDiscard
                | Message::BlockEditChangesCancel
                | Message::BlockSaveAsName(..)
                | Message::BlockSaveAsApply
                | Message::BlockVisibility(..)
                | Message::BlockLookup(..)
                | Message::BlockPropertiesTable(..)
                | Message::ParametersManagerEdit(..)
                | Message::ParametersManagerInput(..)
                | Message::ParametersManagerFilter(..)
                | Message::ParametersManagerFilterMode(..)
                | Message::ParametersManagerFilterMenu
                | Message::ParametersManagerColumn(..)
                | Message::ParametersManagerSearch(..)
                | Message::ParametersManagerCommit
                | Message::ParametersManagerFocus(..)
                | Message::ParametersManagerShow(..)
                | Message::BlockEditPickerName(..)
                | Message::BlockEditPickerSelect(..)
                | Message::BlockEditPickerApply
                | Message::Rename(..)
                | Message::DesignCenter(..)
                | Message::Purge(..)
                | Message::DwgProps(..)
                | Message::GeographicLocation(..)
                | Message::SelectSimilarSettingsToggle(..)
                | Message::SelectSimilarSettingsOk
                | Message::AecDropSameVersion
                | Message::AecDropProceed
                | Message::AecDropBack
                | Message::ConversionLossProceed
                | Message::IfccadAlphaProceed
                | Message::ConversionLossBack
                | Message::AutoSave
                | Message::ThumbnailCaptureFrame
                | Message::ThumbnailCaptureFinished
                | Message::StyleRenameFocus(..)
                | Message::Undo
                | Message::Redo
                | Message::UndoMany(..)
                | Message::RedoMany(..)
                | Message::EnterViewport(..)
                | Message::ExitViewport
                | Message::MspaceCommand
                | Message::PspaceCommand
                | Message::PluginRepoRemove(..)
                | Message::PluginRegistryFetched(..)
                | Message::PluginRegistryRetry
                | Message::PluginRegistryErrorDetailsToggle
                | Message::PluginRegistryCopyDiagnostics
                | Message::PatronsFetched(..)
                | Message::VideosFetched(..)
                | Message::DiscussionsFetched(..)
                | Message::RecentThumbsLoaded(..)
                | Message::PluginReleasesFetched(..)
                | Message::PluginReleaseSelect(..)
                | Message::PluginReadmeSelect(..)
                | Message::PluginReadmeFetched(..)
                | Message::PluginInstall(..)
                | Message::PluginUpdate(..)
                | Message::PluginInstalled(..)
                | Message::PluginRestart
                | Message::PluginUninstall(..)
                | Message::PluginApprove(..)
                | Message::PluginConsentDismiss
                | Message::PointStyleSetMode(..)
                | Message::PointStyleSizeRelative(..)
                | Message::PointStyleSizeInput(..)
                | Message::PointStyleApplySize
                | Message::PointStyleOk
                | Message::Noop
                | Message::StatusMenuTooltipHidden(..)
                | Message::UpdateCheckResult(..)
                | Message::UpdateCheckManual(..)
                | Message::UpdateNoticeClose
                | Message::DonationPromptDonate
                | Message::AssocPromptYes
                | Message::AssocPromptNo
                | Message::AssocResult(..)
                | Message::UpdateNoticeOpenRelease
                | Message::PlotExport
                | Message::PlotExportDwf
                | Message::PlotExportPath(..)
                | Message::PlotWindowExport
                | Message::PlotWindowExportPath(..)
                | Message::BackgroundIoFinished(..)
                | Message::UnderlayRastersDrawn(..)
                | Message::PointCloudRead(..)
                | Message::BackgroundTaskFailed { .. }
                | Message::PrintToPrinter
                | Message::PrintResult(..)
                | Message::PlotDialogOpen
                | Message::PlotDlg(..)
                | Message::BlockPalette(..)
                | Message::Dock(..)
                | Message::AuthoringPaletteTabSelected(..)
                | Message::PrintAllOpen
                | Message::PrintAllToggle(..)
                | Message::PrintAllSelectAll
                | Message::PrintAllSelectNone
                | Message::PrintAllOptions
                | Message::PrintAllPdf
                | Message::PrintAllPdfTo(..)
                | Message::PrintAllPdfPath(..)
                | Message::PrintAllPrint
                | Message::PrintAllFinished(..)
                | Message::PlotStyleLoad
                | Message::LinetypeLoaded(..)
                | Message::PlotStyleLoaded(..)
                | Message::PlotStyleClear
                | Message::PlotStylePanelOpen
                | Message::PlotStylePanelClose
                | Message::PlotStylePanelSelectAci(..)
                | Message::PlotStylePanelColorBuf(..)
                | Message::PlotStylePanelLwBuf(..)
                | Message::PlotStylePanelLwSet(..)
                | Message::PlotStylePanelScreenBuf(..)
                | Message::PlotStylePanelApply
                | Message::PlotStylePanelSaveDirect
                | Message::PlotStylePanelSave
                | Message::PlotStylePanelSavePath(..)
                | Message::TextStyleDialogOpen
                | Message::TextStyleDialogClose
                | Message::TextStyleDialogSelect(..)
                | Message::TextStyleInUseFilter(..)
                | Message::TextStyleSample(..)
                | Message::TextStyleDialogCompare(..)
                | Message::TextStyleDialogSetCurrent
                | Message::TextStyleDialogNew
                | Message::TextStyleDialogCopy
                | Message::StyleRenameStart(..)
                | Message::StyleRenameEdit(..)
                | Message::StyleRenameCommit(..)
                | Message::StyleRenameCancel
                | Message::TextStyleDialogDelete
                | Message::StyleImportPick(..)
                | Message::StyleImportFile(..)
                | Message::StyleImportLoaded(..)
                | Message::StyleImportToggle(..)
                | Message::StyleImportApply
                | Message::StyleImportConfirm(..)
                | Message::StyleImportCancel
                | Message::TextStyleEdit { .. }
                | Message::TextStyleApply
                | Message::TextStyleCard(..)
                | Message::TextStyleContext(..)
                | Message::TextStyleFont(..)
                | Message::TextStyleBigFont(..)
                | Message::TextStyleFaceStyle(..)
                | Message::TextStyleToggle(..)
                | Message::TableStyleDialogOpen
                | Message::TableStyleDialogClose
                | Message::TableStyleDialogSelect(..)
                | Message::TableStyleListInUse(..)
                | Message::TableStyleDialogTab(..)
                | Message::TableStyleDialogCompare(..)
                | Message::TableStyleDialogNew
                | Message::TableStyleDialogCopy
                | Message::TableStyleDialogDelete
                | Message::TableStyleDialogSetCurrent
                | Message::TableStyleToggleAnnotative
                | Message::TableStyleToggle(..)
                | Message::TableStyleEdit { .. }
                | Message::TableStyleApply
                | Message::TableStyleCellSelect(..)
                | Message::TableStyleCells(..)
                | Message::TableStyleCellEdit { .. }
                | Message::TableColorMore(..)
                | Message::TableStyleSetFlow(..)
                | Message::TableStyleTemplatePick
                | Message::TableStyleTemplateRemove
                | Message::Linetype(..)
                | Message::MlStyleDialogOpen
                | Message::MlStyleDialogClose
                | Message::MlStyleDialogSelect(..)
                | Message::MlStyleDialogTab(..)
                | Message::MlStyleDialogCompare(..)
                | Message::MlStyleDialogSetCurrent
                | Message::MlStyleApply
                | Message::MlStyleDialogNew
                | Message::MlStyleDialogCopy
                | Message::MlStyleDialogDelete
                | Message::MlStyleEdit { .. }
                | Message::MlStyleToggle(..)
                | Message::MlStyleElementEdit { .. }
                | Message::MlStyleElementAdd
                | Message::MlStyleElementDelete(..)
                | Message::MLeaderStyleDialogOpen
                | Message::MLeaderStyleDlg(..)
                | Message::MLeaderStyleEdit { .. }
                | Message::MLeaderColorMore(..)
                | Message::MLeaderStyleToggle(..)
                | Message::MLeaderStyleSetEnum { .. }
                | Message::MLeaderStyleLineWeightChanged(..)
                | Message::MLeaderStyleSetHandle { .. }
                | Message::DimStyleDialogOpen
                | Message::DimStyleDlg(..)
                | Message::OpenColorWindow(..)
                | Message::ColorPicker(..)
                | Message::CloseColorPicker
                | Message::ColorWindowPick(..)
                | Message::ColorBookLoaded(..)
                | Message::ToolPalettes(..)
            ) => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::SaveFinished(..) => self.dispatch_styles_plot(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebSaveScreenshot { .. } => self.dispatch_styles_plot(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebSaveSerialized { .. } => self.dispatch_styles_plot(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebSaveHandlePicked { .. } => self.dispatch_styles_plot(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebSaveWritten { .. } => self.dispatch_styles_plot(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebSaveallSerialized { .. } => self.dispatch_styles_plot(m),
            #[cfg(target_arch = "wasm32")]
            m @ Message::WebArchiveSerialized { .. } => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::SaveFileInUseRetry => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::SaveFileInUseSaveAs => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::SaveFileInUseCancel => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::ExternalChangeReload => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::ExternalChangeSaveAs => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::ExternalChangeOverwrite => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::ExternalChangeCancel => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::Terminate => self.dispatch_styles_plot(m),
            #[cfg(not(target_arch = "wasm32"))]
            m @ Message::EmbedImageRead(..) => self.dispatch_styles_plot(m),
        }
    }
}
