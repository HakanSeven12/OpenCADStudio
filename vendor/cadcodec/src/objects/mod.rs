//! Non-graphical objects (OBJECTS section)
//!
//! Objects are non-graphical elements in a DXF file, such as dictionaries,
//! layouts, groups, and other organizational structures.

mod associative;
mod block_visibility;
mod class_object;
mod data_objects;
mod dgn_linestyle;
mod dictionary_variable;
mod dynamic_block;
mod field;
mod group;
mod image_definition;
mod mlinestyle;
mod multileader_style;
mod object_context_data;
mod plot_settings;
mod scale;
pub(crate) mod semantic_property;
mod sort_entities_table;
mod stub_objects;
mod table_style;
mod xrecord;

pub use associative::*;
pub use block_visibility::{
    BlockEvalValue, BlockParameterConnection, BlockParameterPropertyInfo, BlockVisibilityParameter,
    BlockVisibilityState,
};
pub use class_object::{
    AcMeCommandHistory, AcMeScope, AcMeStateManager, ClassObject, ClassObjectData,
    ContextDataEntry, ContextDataManager, ContextDataSubManager, CsacDocumentOptions, CurvePath,
    DataLink, DataLinkCustomData, DataTable, DataTableColumn, DataTableValue, DetailViewStyle,
    GeoMapImage, GradientBackground, GroundPlaneBackground, IblBackground, ImageBackground,
    LayerFilter, LightList, LightListEntry, MentalRayRenderSettings, ModelDocViewStyle, MotionPath,
    NavisworksModelDefinition, PartialViewingIndex, PartialViewingIndexEntry,
    PersistentSubentityManager, PointCloudColorMap, PointCloudColorRamp, PointCloudDefinition,
    PointCloudDefinitionReactor, PointPath, RapidRtRenderSettings, RenderEntry, RenderEnvironment,
    RenderGlobal, RenderSettings, SectionGeometrySettings, SectionManager, SectionSettings,
    SectionTypeSettings, SectionViewStyle, SkyLightBackground, SolidBackground, SpatialIndex, Sun,
    SunStudy, SunStudyDate, TvDeviceProperties, VbaProject, ViewRep, ViewRepBlockPath,
    ViewRepBlockPathEntry, ViewRepGuid, ViewRepModelSpaceSource, ViewRepModelSpaceViewSelectionSet,
    ViewRepObjectPath, ViewRepOrientation, ViewRepSectionDefinition, ViewRepSketch,
    ViewRepSketchGeometry, ViewRepSketchReference, ViewRepSourceManager, ViewRepStandard,
};
pub use data_objects::{
    BreakData, BreakPointReference, CellStyleMap, DataObject, DataObjectData, IdBuffer, Index,
    LayerIndex, LayerIndexEntry, PartialViewingFilter, TableGeometry, TableGeometryCell,
};
pub use dgn_linestyle::{
    DgnLineStyleData, DgnLineStyleObject, DgnLsComponent, DgnLsComponentData, DgnLsComponentType,
    DgnLsCompoundComponent, DgnLsCompoundEntry, DgnLsDefinition, DgnLsInternalComponent,
    DgnLsPhaseMode, DgnLsPointComponent, DgnLsStroke, DgnLsStrokePattern, DgnLsSymbolComponent,
    DgnLsSymbolReference,
};
pub use dictionary_variable::DictionaryVariable;
pub use dynamic_block::{
    BlockAction, BlockActionOffsets, BlockActionWithBasePoint, BlockAlignmentParameter,
    BlockAngularConstraintParameter, BlockAngularConstraintParameterEntity, BlockArrayAction,
    BlockBasePointAction, BlockBasePointParameter, BlockConnection, BlockConstraintParameter,
    BlockDistanceConstraintParameter, BlockElement, BlockEvalExpression, BlockEvaluationEdge,
    BlockEvaluationGraph, BlockEvaluationNode, BlockFlipAction, BlockFlipGrip, BlockFlipParameter,
    BlockGrip, BlockGripExpression, BlockLinearConstraintParameter, BlockLinearParameter,
    BlockLookupAction, BlockLookupParameter, BlockLookupRow, BlockMoveAction,
    BlockOnePointParameter, BlockOrientedGrip, BlockParameter, BlockParameterDependencyBody,
    BlockParameterProperty, BlockParameterValueSet, BlockPointParameter, BlockPolarParameter,
    BlockPolarStretchAction, BlockRepresentationData, BlockRotationParameter, BlockStretchAction,
    BlockStretchCode, BlockStretchHandle, BlockTwoPointParameter, BlockUserParameter,
    BlockXYParameter, DynamicBlockData, DynamicBlockObject, SolidHistory, SolidHistoryBoolean,
    SolidHistoryBox, SolidHistoryBrep, SolidHistoryChamfer, SolidHistoryCone, SolidHistoryCylinder,
    SolidHistoryFillet, SolidHistoryLoft, SolidHistoryLoftParameters, SolidHistoryNodeBase,
    SolidHistoryOperation, SolidHistoryPyramid, SolidHistoryRevolve, SolidHistorySphere,
    SolidHistorySweep, SolidHistoryTorus,
};
pub use field::{Field, FieldChildValue, FieldList};
pub use group::Group;
pub use image_definition::{ImageDefinition, ImageDefinitionReactor, ResolutionUnit};
// UnderlayDefinition is a non-graphical object, defined alongside the underlay
// entity; re-exported here so it can back an ObjectType variant like its raster
// analogue ImageDefinition.
pub use crate::entities::underlay::UnderlayDefinition;
pub use mlinestyle::{MLineStyle, MLineStyleElement, MLineStyleFlags};
pub use multileader_style::{
    BlockContentConnectionType, LeaderContentType, LeaderDrawOrderType,
    LeaderLinePropertyOverrideFlags, MultiLeaderDrawOrderType, MultiLeaderPathType,
    MultiLeaderPropertyOverrideFlags, MultiLeaderStyle, TextAlignmentType, TextAngleType,
    TextAttachmentDirectionType, TextAttachmentType,
};
pub use object_context_data::{
    DimContext, DimSubtype, EmbeddedMTextContext, HatchLoopContext, HatchScaleContext,
    HatchViewContext, LeaderContext, MTextAttributeContext, MTextColumns, MTextContext,
    ObjectContextData, ObjectContextKind,
};
pub use plot_settings::{
    PaperMargin, PlotFlags, PlotPaperUnits, PlotRotation, PlotSettings, PlotType, PlotWindow,
    ScaledType, ShadePlotMode, ShadePlotResolutionLevel,
};
pub use scale::Scale;
pub use semantic_property::{
    ProxyObject, ProxyObjectReference, ProxyPayload, ProxyPayloadEncoding, ProxyPayloadRecord,
    ProxyReferenceKind, RegisteredClassObject, SemanticProperty, SemanticPropertyValue,
};
pub use sort_entities_table::{SortEntitiesTable, SortEntsEntry};
pub use stub_objects::{
    BookColor, DictionaryWithDefault, GeoData, GeoDataMeshFace, GeoDataMeshPoint, Material,
    MaterialColor, MaterialMap, MaterialProceduralValue, MaterialTexture, PlaceHolder,
    RasterVariables, SpatialFilter, StubObject, VisualStyle, VisualStyleProperty,
    VisualStylePropertyValue, WipeoutVariables,
};
pub use table_style::{
    CellAlignment, NamedTableCellStyle, RowCellStyle, TableBorderPropertyFlags, TableBorderType,
    TableCellBorder, TableCellStyleData, TableCellStylePropertyFlags, TableContentFormat,
    TableFlowDirection, TableGridFormat, TableStyle, TableStyleFlags,
};
pub use xrecord::{
    DictionaryCloningFlags, KnownXRecordKind, XRecord, XRecordEntry, XRecordSection, XRecordValue,
    XRecordValueType,
};

use crate::types::Handle;

/// Dictionary object - stores key-value pairs of object handles
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Dictionary {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle (soft pointer)
    pub owner: Handle,
    /// Dictionary entries (key -> handle)
    pub entries: Vec<(String, Handle)>,
    /// Entry keys that were encoded as hard-owner references (DXF code 360)
    /// even when `hard_owner` is false.
    #[cfg_attr(feature = "serde", serde(default))]
    pub hard_owner_entries: Vec<String>,
    /// Duplicate record cloning flag
    pub duplicate_cloning: i16,
    /// Hard owner flag
    pub hard_owner: bool,
    /// Reactor handles ({ACAD_REACTORS})
    pub reactors: Vec<Handle>,
    /// Extended dictionary handle ({ACAD_XDICTIONARY})
    pub xdictionary_handle: Option<Handle>,
}

impl Dictionary {
    /// Create a new dictionary
    pub fn new() -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            entries: Vec::new(),
            hard_owner_entries: Vec::new(),
            duplicate_cloning: 1,
            hard_owner: false,
            reactors: Vec::new(),
            xdictionary_handle: None,
        }
    }

    /// Add an entry to the dictionary
    pub fn add_entry(&mut self, key: impl Into<String>, handle: Handle) {
        self.entries.push((key.into(), handle));
    }

    /// Record whether a specific entry uses hard ownership.
    pub fn set_entry_hard_owner(&mut self, key: &str, hard_owner: bool) {
        self.hard_owner_entries
            .retain(|name| !name.eq_ignore_ascii_case(key));
        if hard_owner {
            self.hard_owner_entries.push(key.to_string());
        }
    }

    /// Return the per-entry ownership mode retained from DXF.
    pub fn is_entry_hard_owner(&self, key: &str) -> bool {
        self.hard_owner_entries
            .iter()
            .any(|name| name.eq_ignore_ascii_case(key))
    }

    /// Named-object-dictionary keys that must be written as hard-owner
    /// references (DXF code 360) even when the dictionary-wide hard-owner
    /// flag is clear. ACAD_FIELD owns the drawing's FIELDLIST; applications
    /// reject the file when it dangles (issue #63).
    ///
    /// Note: ACAD_PLOTSTYLENAME and ACAD_LAYOUT are intentionally NOT in
    /// this list - BricsCAD's own DXF export writes every NOD entry except
    /// ACAD_FIELD as a soft pointer (350), and hard-owning the plot style
    /// dictionary makes BricsCAD's audit reject the layers' PlotStyleName
    /// references (issue #51).
    pub fn is_canonical_hard_owner_key(key: &str) -> bool {
        matches!(key.to_ascii_uppercase().as_str(), "ACAD_FIELD")
    }

    /// Get a handle by key
    pub fn get(&self, key: &str) -> Option<Handle> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, h)| *h)
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the dictionary is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}

/// Layout object - represents a layout (model space or paper space)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Layout {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle (soft pointer)
    pub owner: Handle,
    /// Layout name
    pub name: String,
    /// Layout flags
    pub flags: i16,
    /// Tab order
    pub tab_order: i16,
    /// Minimum limits
    pub min_limits: (f64, f64),
    /// Maximum limits
    pub max_limits: (f64, f64),
    /// Insertion base point
    pub insertion_base: (f64, f64, f64),
    /// Minimum extents
    pub min_extents: (f64, f64, f64),
    /// Maximum extents
    pub max_extents: (f64, f64, f64),
    /// Elevation (code 146)
    pub elevation: f64,
    /// UCS origin (codes 13/23/33)
    pub ucs_origin: (f64, f64, f64),
    /// UCS X axis direction (codes 16/26/36)
    pub ucs_x_axis: (f64, f64, f64),
    /// UCS Y axis direction (codes 17/27/37)
    pub ucs_y_axis: (f64, f64, f64),
    /// UCS orthographic type (code 76)
    pub ucs_ortho_type: i16,
    /// Associated block record handle
    pub block_record: Handle,
    /// Viewport handle
    pub viewport: Handle,
    /// Viewports associated with this layout (native DWG R2004+ list).
    pub viewports: Vec<Handle>,
    /// Base UCS for an orthographic layout UCS.
    pub base_ucs: Handle,
    /// Named UCS used by the layout.
    pub named_ucs: Handle,
    /// Reactor handles ({ACAD_REACTORS})
    pub reactors: Vec<Handle>,
    /// Extended dictionary handle ({ACAD_XDICTIONARY})
    pub xdictionary_handle: Option<Handle>,
    /// Raw DXF AcDbPlotSettings group-code pairs for round-trip preservation.
    /// Layouts embed PlotSettings in the DXF LAYOUT object; since our Layout
    /// struct does not duplicate all PlotSettings fields we capture the raw
    /// pairs on read and replay them verbatim on write.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_plot_settings_codes: Option<Vec<(i32, String)>>,
    /// Physical paper width in mm (from embedded PlotSettings, code 44).
    /// Zero means unknown / not read from the file.
    pub paper_width: f64,
    /// Physical paper height in mm (from embedded PlotSettings, code 45).
    /// Zero means unknown / not read from the file.
    pub paper_height: f64,
    /// Plot rotation from PlotSettings (code 73): 0=none, 1=90°, 2=180°, 3=270°.
    pub plot_rotation: i16,
    /// Full embedded PlotSettings flags.
    pub plot_flags: PlotFlags,

    // ── Remaining embedded PlotSettings fields ──────────────────────────────
    // The LAYOUT object embeds a full PlotSettings record. Preserving only the
    // paper size left the sheet unsized in AutoCAD (rendered tiny in the corner
    // because the paper-size name / units / margins were dropped). Keep the rest
    // so the layout round-trips faithfully. See issue #156.
    pub plot_page_name: String,
    pub plot_printer_name: String,
    /// Paper-size name (e.g. "ISO_A4_(210.00_x_297.00_MM)"). AutoCAD renders the
    /// sheet from this; an empty name shows no/wrong paper.
    pub paper_size: String,
    pub plot_view_name: String,
    pub plot_style_sheet: String,
    pub plot_margin_left: f64,
    pub plot_margin_bottom: f64,
    pub plot_margin_right: f64,
    pub plot_margin_top: f64,
    pub plot_origin_x: f64,
    pub plot_origin_y: f64,
    pub plot_window_min_x: f64,
    pub plot_window_min_y: f64,
    pub plot_window_max_x: f64,
    pub plot_window_max_y: f64,
    /// Paper units (code 72): 0=inches, 1=mm, 2=pixels.
    pub plot_paper_units: i16,
    pub plot_type: i16,
    pub plot_scale_numerator: f64,
    pub plot_scale_denominator: f64,
    pub plot_scale_type: i16,
    pub plot_scale_factor: f64,
    pub paper_image_origin_x: f64,
    pub paper_image_origin_y: f64,
    pub shade_plot_mode: i16,
    pub shade_plot_resolution: i16,
    pub shade_plot_dpi: i16,
    /// Native DWG plot-view reference.
    pub plot_view_handle: Handle,
    /// Shade-plot visual-style reference (DXF code 333).
    pub visual_style_handle: Handle,
}

impl Layout {
    /// Create a new layout
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            name: name.into(),
            flags: 0,
            tab_order: 0,
            min_limits: (0.0, 0.0),
            max_limits: (0.0, 0.0),
            insertion_base: (0.0, 0.0, 0.0),
            min_extents: (0.0, 0.0, 0.0),
            max_extents: (12.0, 9.0, 0.0),
            elevation: 0.0,
            ucs_origin: (0.0, 0.0, 0.0),
            ucs_x_axis: (1.0, 0.0, 0.0),
            ucs_y_axis: (0.0, 1.0, 0.0),
            ucs_ortho_type: 0,
            block_record: Handle::NULL,
            viewport: Handle::NULL,
            viewports: Vec::new(),
            base_ucs: Handle::NULL,
            named_ucs: Handle::NULL,
            reactors: Vec::new(),
            xdictionary_handle: None,
            raw_plot_settings_codes: None,
            paper_width: 0.0,
            paper_height: 0.0,
            plot_rotation: 0,
            plot_flags: PlotFlags::default(),
            plot_page_name: String::new(),
            plot_printer_name: String::new(),
            paper_size: String::new(),
            plot_view_name: String::new(),
            plot_style_sheet: String::new(),
            plot_margin_left: 0.0,
            plot_margin_bottom: 0.0,
            plot_margin_right: 0.0,
            plot_margin_top: 0.0,
            plot_origin_x: 0.0,
            plot_origin_y: 0.0,
            plot_window_min_x: 0.0,
            plot_window_min_y: 0.0,
            plot_window_max_x: 0.0,
            plot_window_max_y: 0.0,
            plot_paper_units: 0,
            plot_type: 5,
            plot_scale_numerator: 1.0,
            plot_scale_denominator: 1.0,
            plot_scale_type: 0,
            plot_scale_factor: 1.0,
            paper_image_origin_x: 0.0,
            paper_image_origin_y: 0.0,
            shade_plot_mode: 0,
            shade_plot_resolution: 0,
            shade_plot_dpi: 300,
            plot_view_handle: Handle::NULL,
            visual_style_handle: Handle::NULL,
        }
    }
}

/// Object types
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ObjectType {
    /// Dictionary object
    Dictionary(Dictionary),
    /// Layout object
    Layout(Layout),
    /// XRecord object - extended data storage
    XRecord(XRecord),
    /// Group object - named collection of entities
    Group(Group),
    /// MLineStyle object - multiline style definition
    MLineStyle(MLineStyle),
    /// ImageDefinition object - raster image definition
    ImageDefinition(ImageDefinition),
    /// UnderlayDefinition object - PDF/DWF/DGN underlay file reference
    UnderlayDefinition(UnderlayDefinition),
    /// PlotSettings object - plot configuration
    PlotSettings(PlotSettings),
    /// MultiLeaderStyle object - multileader style definition
    MultiLeaderStyle(MultiLeaderStyle),
    /// TableStyle object - table style definition
    TableStyle(TableStyle),
    /// Standalone AcDbTableContent object.
    ///
    /// The binary/DXF payload is the same linked/formatted table-data tree
    /// embedded by modern ACAD_TABLE entities, so it deliberately reuses the
    /// complete semantic `Table` model.
    TableContent(crate::entities::Table),
    /// Scale object - named scale definition
    Scale(Scale),
    /// Annotative per-object context data (`AcDb*ObjectContextData` leaf) — one
    /// per-scale representation of an annotative object.
    ObjectContextData(ObjectContextData),
    /// SortEntitiesTable object - entity draw order
    SortEntitiesTable(SortEntitiesTable),
    /// DictionaryVariable object - named variable in dictionary
    DictionaryVariable(DictionaryVariable),
    /// VisualStyle object
    VisualStyle(VisualStyle),
    /// Material object
    Material(Material),
    /// ImageDefinitionReactor object
    ImageDefinitionReactor(ImageDefinitionReactor),
    /// GeoData object
    GeoData(GeoData),
    /// SpatialFilter object
    SpatialFilter(SpatialFilter),
    /// RasterVariables object
    RasterVariables(RasterVariables),
    /// BookColor (DBCOLOR) object
    BookColor(BookColor),
    /// PlaceHolder object
    PlaceHolder(PlaceHolder),
    /// DictionaryWithDefault object
    DictionaryWithDefault(DictionaryWithDefault),
    /// WipeoutVariables object
    WipeoutVariables(WipeoutVariables),
    /// Dynamic-block visibility parameter.
    BlockVisibilityParameter(BlockVisibilityParameter),
    /// Dynamic-block parameter, grip, action or evaluation/history object.
    DynamicBlock(DynamicBlockObject),
    /// Associative network, dependency, action-body or action-parameter object.
    Associative(AssociativeObject),
    /// Class-registered non-graphical object with a native semantic payload.
    ClassObject(ClassObject),
    /// Database helper object carrying an index/filter/reference graph.
    DataObject(DataObject),
    /// Dynamic text field definition.
    Field(Field),
    /// Collection of field handles.
    FieldList(FieldList),
    /// Registered application object represented by typed DXF properties.
    RegisteredClass(RegisteredClassObject),
    /// DGN line-style definition or component object.
    DgnLineStyle(DgnLineStyleObject),
    /// Fixed ACAD proxy-object schema.
    ProxyObject(ProxyObject),
    /// Unknown object type (stored as raw data)
    Unknown {
        /// Object type name
        type_name: String,
        /// Object handle
        handle: Handle,
        /// Owner handle
        owner: Handle,
        /// Raw DXF object-specific group-code pairs.
        ///
        /// Each entry is `(group_code, value_string)`. When present the
        /// DXF writer emits the object type, handle, owner and these
        /// pairs, reproducing the original object content.
        #[cfg_attr(feature = "serde", serde(skip))]
        raw_dxf_codes: Option<Vec<(i32, String)>>,
        /// Raw DWG merged-stream bytes for verbatim round-trip reconstruction.
        /// Populated by the DWG reader for unrecognised non-entity objects.
        #[cfg_attr(feature = "serde", serde(skip))]
        raw_dwg_data: Option<Vec<u8>>,
        /// DWG handle-stream bit count (needed to reconstruct the correct split).
        raw_dwg_handle_bits: i64,
        /// DWG version the `raw_dwg_data` bytes were read from. Verbatim
        /// passthrough is only valid within the same encoding family; on an
        /// incompatible cross-version save the writer drops the object instead
        /// of emitting corrupt bytes. `None` = unknown source (e.g. DXF).
        #[cfg_attr(feature = "serde", serde(skip))]
        raw_dwg_version: Option<crate::types::DxfVersion>,
    },
}

impl ObjectType {
    /// Restore storage-only data omitted from serde after editing an object's
    /// public representation. The source and destination must have the same
    /// variant and identity; callers remain responsible for that check.
    pub fn preserve_storage_data_from(&mut self, source: &Self) {
        match (self, source) {
            (Self::Layout(value), Self::Layout(source)) => {
                value.raw_plot_settings_codes = source.raw_plot_settings_codes.clone();
            }
            (Self::XRecord(value), Self::XRecord(source)) => {
                value.raw_dwg_data = source.raw_dwg_data.clone();
                value.raw_dwg_version = source.raw_dwg_version;
            }
            (Self::TableStyle(value), Self::TableStyle(source)) => {
                value.raw_dxf_codes = source.raw_dxf_codes.clone();
            }
            (Self::SortEntitiesTable(value), Self::SortEntitiesTable(source)) => {
                value.raw_dxf_codes = source.raw_dxf_codes.clone();
                value.raw_dxf_version = source.raw_dxf_version;
            }
            (Self::Associative(value), Self::Associative(source)) => {
                value.source_version = source.source_version;
            }
            (Self::ClassObject(value), Self::ClassObject(source)) => {
                if let (
                    ClassObjectData::CsacDocumentOptions(value),
                    ClassObjectData::CsacDocumentOptions(source),
                ) = (&mut value.data, &source.data)
                {
                    value.raw_dwg_data = source.raw_dwg_data.clone();
                    value.raw_dwg_version = source.raw_dwg_version;
                }
            }
            (Self::RegisteredClass(value), Self::RegisteredClass(source)) => {
                value.raw_dwg_data = source.raw_dwg_data.clone();
                value.raw_dwg_version = source.raw_dwg_version;
            }
            (
                Self::Unknown {
                    raw_dxf_codes,
                    raw_dwg_data,
                    raw_dwg_version,
                    ..
                },
                Self::Unknown {
                    raw_dxf_codes: source_dxf,
                    raw_dwg_data: source_dwg,
                    raw_dwg_version: source_version,
                    ..
                },
            ) => {
                *raw_dxf_codes = source_dxf.clone();
                *raw_dwg_data = source_dwg.clone();
                *raw_dwg_version = *source_version;
            }
            _ => {}
        }
    }

    /// Update the object's intrinsic handle when the document resolves a
    /// collision.  Keeping this exhaustive prevents newly-supported object
    /// classes from being written under a map key that disagrees with the
    /// handle encoded inside their record.
    pub(crate) fn set_handle(&mut self, handle: Handle) {
        match self {
            ObjectType::Dictionary(value) => value.handle = handle,
            ObjectType::Layout(value) => value.handle = handle,
            ObjectType::XRecord(value) => value.handle = handle,
            ObjectType::Group(value) => value.handle = handle,
            ObjectType::MLineStyle(value) => value.handle = handle,
            ObjectType::ImageDefinition(value) => value.handle = handle,
            ObjectType::UnderlayDefinition(value) => value.handle = handle,
            ObjectType::PlotSettings(value) => value.handle = handle,
            ObjectType::MultiLeaderStyle(value) => value.handle = handle,
            ObjectType::TableStyle(value) => value.handle = handle,
            ObjectType::TableContent(value) => value.common.handle = handle,
            ObjectType::Scale(value) => value.handle = handle,
            ObjectType::ObjectContextData(value) => value.handle = handle,
            ObjectType::SortEntitiesTable(value) => value.handle = handle,
            ObjectType::DictionaryVariable(value) => value.handle = handle,
            ObjectType::VisualStyle(value) => value.handle = handle,
            ObjectType::Material(value) => value.handle = handle,
            ObjectType::ImageDefinitionReactor(value) => value.handle = handle,
            ObjectType::GeoData(value) => value.handle = handle,
            ObjectType::SpatialFilter(value) => value.handle = handle,
            ObjectType::RasterVariables(value) => value.handle = handle,
            ObjectType::BookColor(value) => value.handle = handle,
            ObjectType::PlaceHolder(value) => value.handle = handle,
            ObjectType::DictionaryWithDefault(value) => value.handle = handle,
            ObjectType::WipeoutVariables(value) => value.handle = handle,
            ObjectType::BlockVisibilityParameter(value) => value.handle = handle,
            ObjectType::DynamicBlock(value) => value.handle = handle,
            ObjectType::Associative(value) => value.handle = handle,
            ObjectType::ClassObject(value) => value.handle = handle,
            ObjectType::DataObject(value) => value.handle = handle,
            ObjectType::Field(value) => value.handle = handle,
            ObjectType::FieldList(value) => value.handle = handle,
            ObjectType::RegisteredClass(value) => value.handle = handle,
            ObjectType::DgnLineStyle(value) => value.handle = handle,
            ObjectType::ProxyObject(value) => value.handle = handle,
            ObjectType::Unknown {
                handle: old_handle, ..
            } => *old_handle = handle,
        }
    }

    /// Whether the object's intrinsic handle is NULL. Used by document
    /// resolution to find records that arrived without a handle.
    pub(crate) fn has_null_handle(&self) -> bool {
        match self {
            ObjectType::Dictionary(value) => value.handle.is_null(),
            ObjectType::Layout(value) => value.handle.is_null(),
            ObjectType::XRecord(value) => value.handle.is_null(),
            ObjectType::Group(value) => value.handle.is_null(),
            ObjectType::MLineStyle(value) => value.handle.is_null(),
            ObjectType::ImageDefinition(value) => value.handle.is_null(),
            ObjectType::UnderlayDefinition(value) => value.handle.is_null(),
            ObjectType::PlotSettings(value) => value.handle.is_null(),
            ObjectType::MultiLeaderStyle(value) => value.handle.is_null(),
            ObjectType::TableStyle(value) => value.handle.is_null(),
            ObjectType::TableContent(value) => value.common.handle.is_null(),
            ObjectType::Scale(value) => value.handle.is_null(),
            ObjectType::ObjectContextData(value) => value.handle.is_null(),
            ObjectType::SortEntitiesTable(value) => value.handle.is_null(),
            ObjectType::DictionaryVariable(value) => value.handle.is_null(),
            ObjectType::VisualStyle(value) => value.handle.is_null(),
            ObjectType::Material(value) => value.handle.is_null(),
            ObjectType::ImageDefinitionReactor(value) => value.handle.is_null(),
            ObjectType::GeoData(value) => value.handle.is_null(),
            ObjectType::SpatialFilter(value) => value.handle.is_null(),
            ObjectType::RasterVariables(value) => value.handle.is_null(),
            ObjectType::BookColor(value) => value.handle.is_null(),
            ObjectType::PlaceHolder(value) => value.handle.is_null(),
            ObjectType::DictionaryWithDefault(value) => value.handle.is_null(),
            ObjectType::WipeoutVariables(value) => value.handle.is_null(),
            ObjectType::BlockVisibilityParameter(value) => value.handle.is_null(),
            ObjectType::DynamicBlock(value) => value.handle.is_null(),
            ObjectType::Associative(value) => value.handle.is_null(),
            ObjectType::ClassObject(value) => value.handle.is_null(),
            ObjectType::DataObject(value) => value.handle.is_null(),
            ObjectType::Field(value) => value.handle.is_null(),
            ObjectType::FieldList(value) => value.handle.is_null(),
            ObjectType::RegisteredClass(value) => value.handle.is_null(),
            ObjectType::DgnLineStyle(value) => value.handle.is_null(),
            ObjectType::ProxyObject(value) => value.handle.is_null(),
            ObjectType::Unknown { handle, .. } => handle.is_null(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dictionary_creation() {
        let mut dict = Dictionary::new();
        assert!(dict.is_empty());

        dict.add_entry("KEY1", Handle::new(100));
        assert_eq!(dict.len(), 1);
        assert_eq!(dict.get("KEY1"), Some(Handle::new(100)));
        assert_eq!(dict.get("KEY2"), None);
    }

    #[test]
    fn test_layout_creation() {
        let layout = Layout::new("Layout1");
        assert_eq!(layout.name, "Layout1");
        assert_eq!(layout.tab_order, 0);
    }

    #[test]
    fn public_record_edit_can_preserve_opaque_object_data() {
        let source = ObjectType::Unknown {
            type_name: "CUSTOM_OBJECT".into(),
            handle: Handle::new(0x42),
            owner: Handle::NULL,
            raw_dxf_codes: Some(vec![(1, "source".into())]),
            raw_dwg_data: Some(vec![1, 2, 3]),
            raw_dwg_handle_bits: 7,
            raw_dwg_version: Some(crate::types::DxfVersion::AC1032),
        };
        let mut edited = ObjectType::Unknown {
            type_name: "EDITED_OBJECT".into(),
            handle: Handle::new(0x42),
            owner: Handle::NULL,
            raw_dxf_codes: None,
            raw_dwg_data: None,
            raw_dwg_handle_bits: 7,
            raw_dwg_version: None,
        };

        edited.preserve_storage_data_from(&source);

        let ObjectType::Unknown {
            type_name,
            raw_dxf_codes,
            raw_dwg_data,
            raw_dwg_version,
            ..
        } = edited
        else {
            unreachable!()
        };
        assert_eq!(type_name, "EDITED_OBJECT");
        assert_eq!(raw_dxf_codes, Some(vec![(1, "source".into())]));
        assert_eq!(raw_dwg_data, Some(vec![1, 2, 3]));
        assert_eq!(raw_dwg_version, Some(crate::types::DxfVersion::AC1032));
    }
}
