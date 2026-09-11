//! Native models for class-based, non-graphical DWG/DXF objects.
//!
//! These objects are registered through the CLASSES section rather than fixed
//! object type codes.  The payloads below follow LibreDWG `dwg2.spec`; unlike
//! proxy/unknown records they remain editable and can be encoded for another
//! file version.

use crate::types::{Color, Handle, Vector2, Vector3};

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ClassObject {
    pub handle: Handle,
    pub owner: Handle,
    pub reactors: Vec<Handle>,
    pub xdictionary_handle: Option<Handle>,
    pub data: ClassObjectData,
}

impl ClassObject {
    pub fn new(data: ClassObjectData) -> Self {
        Self {
            data,
            ..Self::default()
        }
    }

    pub fn dxf_name(&self) -> &'static str {
        self.data.dxf_name()
    }

    pub(crate) fn visit_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        visit(&mut self.owner);
        for handle in &mut self.reactors {
            visit(handle);
        }
        if let Some(handle) = self.xdictionary_handle.as_mut() {
            visit(handle);
        }
        self.data.visit_handles_mut(visit);
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ClassObjectData {
    #[default]
    Empty,
    SpatialIndex(SpatialIndex),
    LayerFilter(LayerFilter),
    PartialViewingIndex(PartialViewingIndex),
    VbaProject(VbaProject),
    SectionManager(SectionManager),
    SectionSettings(SectionSettings),
    LightList(LightList),
    Sun(Sun),
    RenderSettings(RenderSettings),
    MentalRayRenderSettings(MentalRayRenderSettings),
    RapidRtRenderSettings(RapidRtRenderSettings),
    GradientBackground(GradientBackground),
    GroundPlaneBackground(GroundPlaneBackground),
    IblBackground(IblBackground),
    ImageBackground(ImageBackground),
    SkyLightBackground(SkyLightBackground),
    SolidBackground(SolidBackground),
    RenderEntry(RenderEntry),
    RenderEnvironment(RenderEnvironment),
    RenderGlobal(RenderGlobal),
    MotionPath(MotionPath),
    CurvePath(CurvePath),
    PointPath(PointPath),
    TvDeviceProperties(TvDeviceProperties),
    PointCloudDefinition(PointCloudDefinition),
    PointCloudDefinitionEx(PointCloudDefinition),
    PointCloudDefinitionReactor(PointCloudDefinitionReactor),
    PointCloudDefinitionReactorEx(PointCloudDefinitionReactor),
    PointCloudColorMap(PointCloudColorMap),
    NavisworksModelDefinition(NavisworksModelDefinition),
    ContextDataManager(ContextDataManager),
    SunStudy(SunStudy),
    DataTable(DataTable),
    DataLink(DataLink),
    PersistentSubentityManager(PersistentSubentityManager),
    GeoMapImage(GeoMapImage),
    DetailViewStyle(DetailViewStyle),
    SectionViewStyle(SectionViewStyle),
    AcMeCommandHistory(AcMeCommandHistory),
    AcMeScope(AcMeScope),
    AcMeStateManager(AcMeStateManager),
    CsacDocumentOptions(CsacDocumentOptions),
    ViewRepSourceManager(ViewRepSourceManager),
    ViewRepStandard(ViewRepStandard),
    ViewRepOrientationDefinition,
    ViewRepOrientation(ViewRepOrientation),
    ViewRepSectionDefinition(ViewRepSectionDefinition),
    ViewRepModelSpaceViewSelectionSet(ViewRepModelSpaceViewSelectionSet),
    ViewRep(ViewRep),
    ViewRepModelSpaceSource(ViewRepModelSpaceSource),
}

impl ClassObjectData {
    pub fn dxf_name(&self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::SpatialIndex(_) => "SPATIAL_INDEX",
            Self::LayerFilter(_) => "LAYERFILTER",
            Self::PartialViewingIndex(_) => "PARTIAL_VIEWING_INDEX",
            Self::VbaProject(_) => "VBA_PROJECT",
            Self::SectionManager(_) => "SECTION_MANAGER",
            Self::SectionSettings(_) => "SECTION_SETTINGS",
            Self::LightList(_) => "LIGHTLIST",
            Self::Sun(_) => "SUN",
            Self::RenderSettings(_) => "RENDERSETTINGS",
            Self::MentalRayRenderSettings(_) => "MENTALRAYRENDERSETTINGS",
            Self::RapidRtRenderSettings(_) => "RAPIDRTRENDERSETTINGS",
            Self::GradientBackground(_) => "GRADIENT_BACKGROUND",
            Self::GroundPlaneBackground(_) => "GROUND_PLANE_BACKGROUND",
            Self::IblBackground(_) => "RAPIDRTRENDERENVIRONMENT",
            Self::ImageBackground(_) => "IMAGE_BACKGROUND",
            Self::SkyLightBackground(_) => "SKYLIGHT_BACKGROUND",
            Self::SolidBackground(_) => "SOLID_BACKGROUND",
            Self::RenderEntry(_) => "RENDERENTRY",
            Self::RenderEnvironment(_) => "RENDERENVIRONMENT",
            Self::RenderGlobal(_) => "RENDERGLOBAL",
            Self::MotionPath(_) => "ACDBMOTIONPATH",
            Self::CurvePath(_) => "ACDBCURVEPATH",
            Self::PointPath(_) => "ACDBPOINTPATH",
            Self::TvDeviceProperties(_) => "TVDEVICEPROPERTIES",
            Self::PointCloudDefinition(_) => "ACDBPOINTCLOUDDEF",
            Self::PointCloudDefinitionEx(_) => "ACDBPOINTCLOUDDEFEX",
            Self::PointCloudDefinitionReactor(_) => "ACDBPOINTCLOUDDEF_REACTOR",
            Self::PointCloudDefinitionReactorEx(_) => "ACDBPOINTCLOUDDEF_REACTOR_EX",
            Self::PointCloudColorMap(_) => "ACDBPOINTCLOUDCOLORMAP",
            Self::NavisworksModelDefinition(_) => "NAVISWORKSMODELDEF",
            Self::ContextDataManager(_) => "CONTEXTDATAMANAGER",
            Self::SunStudy(_) => "SUNSTUDY",
            Self::DataTable(_) => "DATATABLE",
            Self::DataLink(_) => "DATALINK",
            Self::PersistentSubentityManager(_) => "ACDBPERSSUBENTMANAGER",
            Self::GeoMapImage(_) => "GEOMAPIMAGE",
            Self::DetailViewStyle(_) => "ACDBDETAILVIEWSTYLE",
            Self::SectionViewStyle(_) => "ACDBSECTIONVIEWSTYLE",
            Self::AcMeCommandHistory(_) => "ACMECOMMANDHISTORY",
            Self::AcMeScope(_) => "ACMESCOPE",
            Self::AcMeStateManager(_) => "ACMESTATEMGR",
            Self::CsacDocumentOptions(_) => "CSACDOCUMENTOPTIONS",
            Self::ViewRepSourceManager(_) => "ACDBVIEWREPSOURCEMGR",
            Self::ViewRepStandard(_) => "ACDBVIEWREPSTANDARD",
            Self::ViewRepOrientationDefinition => "ACDBVIEWREPORIENTATIONDEF",
            Self::ViewRepOrientation(_) => "ACDBVIEWREPORIENTATION",
            Self::ViewRepSectionDefinition(_) => "ACDBVIEWREPSECTIONDEFINITION",
            Self::ViewRepModelSpaceViewSelectionSet(_) => "ACDBSYMODELSPACEVIEWSELSET",
            Self::ViewRep(_) => "ACDBVIEWREP",
            Self::ViewRepModelSpaceSource(_) => "ACDBVIEWREPMODELSPACESOURCE",
        }
    }

    pub(crate) fn visit_handles_mut(&mut self, visit: &mut impl FnMut(&mut Handle)) {
        match self {
            Self::Empty
            | Self::LayerFilter(_)
            | Self::VbaProject(_)
            | Self::Sun(_)
            | Self::RenderSettings(_)
            | Self::MentalRayRenderSettings(_)
            | Self::RapidRtRenderSettings(_)
            | Self::GradientBackground(_)
            | Self::GroundPlaneBackground(_)
            | Self::ImageBackground(_)
            | Self::SolidBackground(_)
            | Self::RenderEntry(_)
            | Self::RenderEnvironment(_)
            | Self::RenderGlobal(_)
            | Self::PointPath(_)
            | Self::TvDeviceProperties(_)
            | Self::PointCloudDefinition(_)
            | Self::PointCloudDefinitionEx(_)
            | Self::PointCloudDefinitionReactor(_)
            | Self::PointCloudDefinitionReactorEx(_)
            | Self::PointCloudColorMap(_)
            | Self::NavisworksModelDefinition(_)
            | Self::DataTable(_)
            | Self::PersistentSubentityManager(_)
            | Self::GeoMapImage(_)
            | Self::AcMeCommandHistory(_)
            | Self::AcMeScope(_)
            | Self::AcMeStateManager(_)
            | Self::CsacDocumentOptions(_)
            | Self::ViewRepStandard(_)
            | Self::ViewRepOrientationDefinition
            | Self::ViewRepOrientation(_)
            | Self::ViewRepSectionDefinition(_) => {}
            Self::SpatialIndex(value) => {
                for handle in &mut value.indexed_objects {
                    visit(handle);
                }
            }
            Self::PartialViewingIndex(value) => {
                for entry in &mut value.entries {
                    visit(&mut entry.object);
                }
            }
            Self::SectionManager(value) => {
                for handle in &mut value.sections {
                    visit(handle);
                }
            }
            Self::SectionSettings(value) => {
                for settings in &mut value.types {
                    for handle in &mut settings.sources {
                        visit(handle);
                    }
                    visit(&mut settings.destination_block);
                }
            }
            Self::LightList(value) => {
                for light in &mut value.lights {
                    visit(&mut light.handle);
                }
            }
            Self::IblBackground(value) => {
                visit(&mut value.secondary_background);
            }
            Self::SkyLightBackground(value) => visit(&mut value.sun),
            Self::MotionPath(value) => {
                visit(&mut value.camera_path);
                visit(&mut value.target_path);
                visit(&mut value.view);
            }
            Self::CurvePath(value) => visit(&mut value.entity),
            Self::ContextDataManager(value) => {
                visit(&mut value.object_context);
                for manager in &mut value.sub_managers {
                    visit(&mut manager.handle);
                    for entry in &mut manager.entries {
                        visit(&mut entry.item);
                    }
                }
            }
            Self::SunStudy(value) => {
                visit(&mut value.page_setup_wizard);
                visit(&mut value.view);
                visit(&mut value.visual_style);
                visit(&mut value.text_style);
            }
            Self::DataLink(value) => {
                for data in &mut value.custom_data {
                    visit(&mut data.target);
                }
                visit(&mut value.hard_owner);
            }
            Self::DetailViewStyle(value) => {
                visit(&mut value.identifier_style);
                visit(&mut value.arrow_symbol);
                visit(&mut value.boundary_linetype);
                visit(&mut value.view_label_text_style);
                visit(&mut value.connection_linetype);
                visit(&mut value.border_linetype);
            }
            Self::SectionViewStyle(value) => {
                visit(&mut value.identifier_style);
                visit(&mut value.arrow_start_symbol);
                visit(&mut value.arrow_end_symbol);
                visit(&mut value.plane_linetype);
                visit(&mut value.bend_linetype);
                visit(&mut value.view_label_text_style);
            }
            Self::ViewRepSourceManager(value) => {
                visit(&mut value.source);
            }
            Self::ViewRepModelSpaceViewSelectionSet(value) => {
                for handle in &mut value.entities {
                    visit(handle);
                }
            }
            Self::ViewRep(value) => {
                for sketch in &mut value.sketches {
                    for reference in &mut sketch.references {
                        visit(&mut reference.object);
                    }
                }
                for handle in &mut value.related_objects {
                    visit(handle);
                }
                visit(&mut value.source_manager);
                for handle in &mut value.owned_objects {
                    visit(handle);
                }
                for handle in &mut value.optional_objects {
                    visit(handle);
                }
                visit(&mut value.orientation);
                for handle in &mut value.linked_views {
                    visit(handle);
                }
                for path in &mut value.section_sketches {
                    for handle in &mut path.objects {
                        visit(handle);
                    }
                }
                if let Some(handle) = value.action.as_mut() {
                    visit(handle);
                }
                visit(&mut value.parent);
                if let Some(path) = value.block_path.as_mut() {
                    for entry in &mut path.entries {
                        visit(&mut entry.object);
                    }
                }
                visit(&mut value.style);
            }
            Self::ViewRepModelSpaceSource(value) => {
                visit(&mut value.model);
                for handle in &mut value.references {
                    visit(handle);
                }
                visit(&mut value.orientation);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepSourceManager {
    pub has_source: bool,
    pub source: Handle,
    pub status: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepStandard {
    pub values: [i32; 6],
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepOrientation {
    pub camera: Vector3,
    pub target: Vector3,
    pub normal: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepSectionDefinition {
    pub version: i32,
    pub section_depth: f64,
    pub flags: [i32; 2],
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepModelSpaceViewSelectionSet {
    pub version: i32,
    pub entities: Vec<Handle>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepGuid {
    pub data1: i32,
    pub data2: i16,
    pub data3: i16,
    pub data4: [u8; 8],
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepModelSpaceSource {
    pub enabled: bool,
    pub header_values: [i32; 6],
    pub transform: [f64; 16],
    pub source_version: i32,
    pub source_status: i32,
    pub model: Handle,
    pub guid: ViewRepGuid,
    pub references: [Handle; 2],
    pub tail_values: [i32; 2],
    pub orientation: Handle,
}

impl Default for ViewRepModelSpaceSource {
    fn default() -> Self {
        Self {
            enabled: false,
            header_values: [0; 6],
            transform: [0.0; 16],
            source_version: 0,
            source_status: 0,
            model: Handle::NULL,
            guid: ViewRepGuid::default(),
            references: [Handle::NULL; 2],
            tail_values: [0; 2],
            orientation: Handle::NULL,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepSketchReference {
    pub object: Handle,
    pub flag: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ViewRepSketchGeometry {
    #[default]
    None,
    Line {
        type_code: i32,
        first: Vector3,
        second: Vector3,
    },
    Circle {
        type_code: i32,
        center: Vector3,
        normal: Vector3,
        direction: Vector3,
        radius: f64,
        start_parameter: f64,
        end_parameter: f64,
        reserved: f64,
    },
    Nurb {
        type_code: i32,
        flags: [bool; 2],
        degree: i32,
        tolerance: f64,
        knot_header: [i32; 3],
        knots: Vec<f64>,
        weight_header: [i32; 3],
        weights: Vec<f64>,
        point_header: [i32; 3],
        control_points: Vec<Vector3>,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepSketch {
    pub id: i32,
    pub version: i32,
    pub references: Vec<ViewRepSketchReference>,
    pub reserved: i32,
    pub enabled: bool,
    pub geometry: ViewRepSketchGeometry,
    pub final_flag: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepObjectPath {
    pub class_name: String,
    pub objects: Vec<Handle>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepBlockPathEntry {
    pub flag: u8,
    pub kind: u8,
    pub object: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRepBlockPath {
    pub class_name: String,
    pub version: i32,
    pub entries: Vec<ViewRepBlockPathEntry>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ViewRep {
    pub header_values: [i32; 5],
    pub name: String,
    pub scale: i32,
    pub header_status: i32,
    pub description: String,
    pub source_id: i64,
    pub source_enabled: bool,
    pub source_version: i32,
    pub model_id: i64,
    pub guid: ViewRepGuid,
    pub marker: u8,
    pub transform: [f64; 16],
    pub transform_version: i32,
    pub database_id: i64,
    pub geometry_version: i32,
    pub geometry_marker: i32,
    pub sketches: Vec<ViewRepSketch>,
    pub related_objects: [Handle; 2],
    pub source_manager: Handle,
    pub owned_objects: [Handle; 2],
    pub optional_objects: [Handle; 2],
    pub position: Vector2,
    pub rotation: f64,
    pub orientation: Handle,
    pub is_active: bool,
    pub projection: i16,
    pub linked_views: [Handle; 2],
    pub section_sketches: Vec<ViewRepObjectPath>,
    pub action_mode: i32,
    pub action: Option<Handle>,
    pub has_parent: bool,
    pub parent: Handle,
    pub tail_version: i32,
    pub tail_state: i32,
    pub tail_id: i64,
    pub path_count: i32,
    pub path_version: i32,
    pub path_id: i64,
    pub has_block_path: bool,
    pub block_path: Option<ViewRepBlockPath>,
    pub style: Handle,
}

impl Default for ViewRep {
    fn default() -> Self {
        Self {
            header_values: [0; 5],
            name: String::new(),
            scale: 0,
            header_status: 0,
            description: String::new(),
            source_id: 0,
            source_enabled: false,
            source_version: 0,
            model_id: 0,
            guid: ViewRepGuid::default(),
            marker: 0,
            transform: [0.0; 16],
            transform_version: 0,
            database_id: 0,
            geometry_version: 0,
            geometry_marker: 0,
            sketches: Vec::new(),
            related_objects: [Handle::NULL; 2],
            source_manager: Handle::NULL,
            owned_objects: [Handle::NULL; 2],
            optional_objects: [Handle::NULL; 2],
            position: Vector2::ZERO,
            rotation: 0.0,
            orientation: Handle::NULL,
            is_active: false,
            projection: 0,
            linked_views: [Handle::NULL; 2],
            section_sketches: Vec::new(),
            action_mode: 0,
            action: None,
            has_parent: false,
            parent: Handle::NULL,
            tail_version: 0,
            tail_state: 0,
            tail_id: 0,
            path_count: 0,
            path_version: 0,
            path_id: 0,
            has_block_path: false,
            block_path: None,
            style: Handle::NULL,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AcMeCommandHistory {
    pub class_version: i16,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AcMeScope {
    pub class_version: i16,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AcMeStateManager {
    pub class_version: i16,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CsacDocumentOptions {
    pub class_version: i16,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_dwg_data: Option<Vec<u8>>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub raw_dwg_handle_bits: i64,
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_dwg_version: Option<crate::types::DxfVersion>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpatialIndex {
    pub last_updated_julian_day: i32,
    pub last_updated_milliseconds: i32,
    pub min_corner: Vector3,
    pub max_corner: Vector3,
    /// Indexed entity IDs, in the spatial index's persisted traversal order.
    pub indexed_objects: Vec<Handle>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LayerFilter {
    pub names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PartialViewingIndexEntry {
    pub extents_min: Vector3,
    pub extents_max: Vector3,
    pub object: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PartialViewingIndex {
    pub has_entries: bool,
    pub entries: Vec<PartialViewingIndexEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct VbaProject {
    pub storage: crate::compound_file::StructuredStoragePayload,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectionManager {
    pub is_live: bool,
    pub sections: Vec<Handle>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectionGeometrySettings {
    pub geometry_count: i32,
    pub index: i32,
    pub flags: i32,
    pub color: Color,
    pub layer: String,
    pub linetype: String,
    pub linetype_scale: f64,
    pub plot_style: String,
    pub lineweight: i32,
    pub face_transparency: i16,
    pub edge_transparency: i16,
    pub hatch_type: i16,
    pub hatch_pattern: String,
    pub hatch_angle: f64,
    pub hatch_spacing: f64,
    pub hatch_scale: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectionTypeSettings {
    pub section_type: i32,
    pub generation: i32,
    pub sources: Vec<Handle>,
    pub destination_block: Handle,
    pub destination_file: String,
    pub geometry: Vec<SectionGeometrySettings>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectionSettings {
    pub current_type: i32,
    pub types: Vec<SectionTypeSettings>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LightListEntry {
    pub handle: Handle,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LightList {
    pub class_version: i32,
    pub lights: Vec<LightListEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Sun {
    pub class_version: i32,
    pub is_on: bool,
    pub color: Color,
    pub intensity: f64,
    pub has_shadow: bool,
    pub julian_day: i32,
    pub milliseconds: i32,
    pub is_daylight_savings_on: bool,
    pub shadow_type: i32,
    pub shadow_map_size: i16,
    pub shadow_softness: u8,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RenderSettings {
    pub class_version: i32,
    pub name: String,
    pub fog_enabled: bool,
    pub fog_background_enabled: bool,
    pub backfaces_enabled: bool,
    pub environment_image_enabled: bool,
    pub environment_image_filename: String,
    pub description: String,
    pub display_index: i32,
    pub has_predefined: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MentalRayRenderSettings {
    pub base: RenderSettings,
    pub version: i32,
    pub sampling_min: i32,
    pub sampling_max: i32,
    pub sampling_filter: i16,
    pub sampling_filter_width: f64,
    pub sampling_filter_height: f64,
    pub sampling_contrast: [f64; 4],
    pub shadow_mode: i16,
    pub shadow_maps_enabled: bool,
    pub ray_tracing_enabled: bool,
    pub ray_trace_depth: [i32; 3],
    pub global_illumination_enabled: bool,
    pub global_illumination_sample_count: i32,
    pub global_illumination_sample_radius_enabled: bool,
    pub global_illumination_sample_radius: f64,
    pub photons_per_light: i32,
    pub photon_trace_depth: [i32; 3],
    pub final_gathering_enabled: bool,
    pub final_gathering_ray_count: i32,
    pub final_gathering_sample_radius_state: [bool; 3],
    pub final_gathering_sample_radius: [f64; 2],
    pub light_luminance_scale: f64,
    pub diagnostics_mode: i16,
    pub diagnostics_grid_mode: i16,
    pub diagnostics_grid_size: f64,
    pub diagnostics_photon_mode: i16,
    pub diagnostics_bsp_mode: i16,
    pub export_mi_enabled: bool,
    pub description: String,
    pub tile_size: i32,
    pub tile_order: i16,
    pub memory_limit: i32,
    pub diagnostics_samples_mode: bool,
    pub energy_multiplier: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RapidRtRenderSettings {
    pub base: RenderSettings,
    pub version: i32,
    pub render_target: i32,
    pub render_level: i32,
    pub render_time: i32,
    pub lighting_model: i32,
    pub filter_type: i32,
    pub filter_width: f64,
    pub filter_height: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GradientBackground {
    pub class_version: i32,
    pub color_top: u32,
    pub color_middle: u32,
    pub color_bottom: u32,
    pub horizon: f64,
    pub height: f64,
    pub rotation: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GroundPlaneBackground {
    pub class_version: i32,
    pub color_sky_zenith: u32,
    pub color_sky_horizon: u32,
    pub color_underground_horizon: u32,
    pub color_underground_azimuth: u32,
    pub color_near: u32,
    pub color_far: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct IblBackground {
    pub class_version: i32,
    pub enabled: bool,
    pub name: String,
    pub rotation: f64,
    pub display_image: bool,
    pub secondary_background: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ImageBackground {
    pub class_version: i32,
    pub filename: String,
    pub fit_to_screen: bool,
    pub maintain_aspect_ratio: bool,
    pub use_tiling: bool,
    pub offset: Vector2,
    pub scale: Vector2,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SkyLightBackground {
    pub class_version: i32,
    pub sun: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SolidBackground {
    pub class_version: i32,
    pub color: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RenderEntry {
    pub class_version: i32,
    pub image_filename: String,
    pub preset_name: String,
    pub view_name: String,
    pub width: i32,
    pub height: i32,
    pub start_year: i16,
    pub start_month: i16,
    pub start_day: i16,
    pub start_hour: i16,
    pub start_minute: i16,
    pub start_second: i16,
    pub start_millisecond: i16,
    pub end_year: i16,
    pub end_month: i16,
    pub end_day: i16,
    pub end_hour: i16,
    pub end_minute: i16,
    pub end_second: i16,
    pub end_millisecond: i16,
    pub render_time: f64,
    pub memory_amount: i32,
    pub material_count: i32,
    pub light_count: i32,
    pub triangle_count: i32,
    pub display_index: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RenderEnvironment {
    pub class_version: i32,
    pub fog_enabled: bool,
    pub fog_background_enabled: bool,
    pub fog_color: [u8; 3],
    pub fog_density_near: f64,
    pub fog_density_far: f64,
    pub fog_distance_near: f64,
    pub fog_distance_far: f64,
    pub environment_image_enabled: bool,
    pub environment_image_filename: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RenderGlobal {
    pub class_version: i32,
    pub procedure: i32,
    pub destination: i32,
    pub save_enabled: bool,
    pub save_filename: String,
    pub image_width: i32,
    pub image_height: i32,
    pub predefined_presets_first: bool,
    pub high_level_info: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MotionPath {
    pub class_version: i32,
    pub camera_path: Handle,
    pub target_path: Handle,
    pub view: Handle,
    pub frames: i16,
    pub frame_rate: i16,
    pub corner_deceleration: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CurvePath {
    pub class_version: i32,
    pub entity: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PointPath {
    pub class_version: i16,
    pub point: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TvDeviceProperties {
    pub flags: u32,
    pub max_regen_threads: i16,
    pub use_lut_palette: i32,
    pub alternate_highlight: i64,
    pub alternate_highlight_color: i64,
    pub geometry_shader_usage: i64,
    pub blending_mode: i32,
    pub antialiasing_level: f64,
    pub reserved_double: f64,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PointCloudDefinition {
    pub class_version: i32,
    pub source_filename: String,
    pub is_loaded: bool,
    pub point_count: i64,
    pub extents_min: Vector3,
    pub extents_max: Vector3,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PointCloudDefinitionReactor {
    pub class_version: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PointCloudColorRamp {
    pub class_version: i16,
    pub color_schemes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PointCloudColorMap {
    pub class_version: i16,
    pub default_intensity_scheme: String,
    pub default_elevation_scheme: String,
    pub default_classification_scheme: String,
    pub color_ramps: Vec<PointCloudColorRamp>,
    pub classification_color_ramps: Vec<PointCloudColorRamp>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NavisworksModelDefinition {
    pub flags: i16,
    pub path: String,
    pub status: bool,
    pub extents_min: Vector3,
    pub extents_max: Vector3,
    pub host_drawing_visibility: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ContextDataEntry {
    pub item: Handle,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ContextDataSubManager {
    pub handle: Handle,
    pub entries: Vec<ContextDataEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ContextDataManager {
    pub object_context: Handle,
    pub sub_managers: Vec<ContextDataSubManager>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SunStudyDate {
    pub julian_day: i32,
    pub milliseconds: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SunStudy {
    pub class_version: i32,
    pub setup_name: String,
    pub description: String,
    pub output_type: i32,
    pub use_subset: bool,
    pub sheet_set_name: String,
    pub sheet_subset_name: String,
    pub select_dates_from_calendar: bool,
    pub dates: Vec<SunStudyDate>,
    pub select_range_of_dates: bool,
    pub start_time: i32,
    pub end_time: i32,
    pub interval: i32,
    pub hours: Vec<bool>,
    pub shade_plot_type: i32,
    pub viewport_count: i32,
    pub rows: i32,
    pub columns: i32,
    pub spacing: f64,
    pub lock_viewports: bool,
    pub label_viewports: bool,
    pub page_setup_wizard: Handle,
    pub view: Handle,
    pub visual_style: Handle,
    pub text_style: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DataTableValue {
    pub integer: i32,
    pub real: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DataTableColumn {
    pub value_type: i32,
    pub name: String,
    pub rows: Vec<DataTableValue>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DataTable {
    pub flags: i16,
    pub name: String,
    pub row_count: i32,
    pub columns: Vec<DataTableColumn>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DataLinkCustomData {
    pub target: Handle,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DataLink {
    pub data_adapter: String,
    pub description: String,
    pub tooltip: String,
    pub connection_string: String,
    pub option: i32,
    pub update_option: i32,
    pub flags: i32,
    pub year: i16,
    pub month: i16,
    pub day: i16,
    pub hour: i16,
    pub minute: i16,
    pub second: i16,
    pub millisecond: i16,
    pub path_option: i16,
    pub status_flags: i32,
    pub update_status: String,
    pub custom_data: Vec<DataLinkCustomData>,
    pub hard_owner: Handle,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PersistentSubentityManager {
    pub class_version: i32,
    pub reserved_zero: i32,
    pub reserved_two: i32,
    pub associated_step_count: i32,
    pub associated_subentity_count: i32,
    pub steps: Vec<i32>,
    pub subentities: Vec<i32>,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GeoMapImage {
    pub class_version: i32,
    pub origin: Vector3,
    pub image_size: Vector2,
    pub display_properties: i16,
    pub clipping_enabled: bool,
    pub brightness: u8,
    pub contrast: u8,
    pub fade: u8,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ModelDocViewStyle {
    pub class_version: i16,
    pub description: String,
    pub modified_for_recompute: bool,
    pub display_name: String,
    pub flags: i32,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DetailViewStyle {
    pub base: ModelDocViewStyle,
    pub class_version: i16,
    pub flags: i32,
    pub identifier_style: Handle,
    pub identifier_color: Color,
    pub identifier_height: f64,
    pub identifier_excluded_characters: String,
    pub identifier_offset: f64,
    pub identifier_placement: u8,
    pub arrow_symbol: Handle,
    pub arrow_symbol_color: Color,
    pub arrow_symbol_size: f64,
    pub boundary_linetype: Handle,
    pub boundary_lineweight: i32,
    pub boundary_color: Color,
    pub view_label_text_style: Handle,
    pub view_label_text_color: Color,
    pub view_label_text_height: f64,
    pub view_label_attachment: i32,
    pub view_label_offset: f64,
    pub view_label_alignment: i32,
    pub view_label_pattern: String,
    pub connection_linetype: Handle,
    pub connection_lineweight: i32,
    pub connection_color: Color,
    pub border_linetype: Handle,
    pub border_lineweight: i32,
    pub border_color: Color,
    pub model_edge: u8,
}

#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectionViewStyle {
    pub base: ModelDocViewStyle,
    pub class_version: i16,
    pub flags: i32,
    pub identifier_style: Handle,
    pub identifier_color: Color,
    pub identifier_height: f64,
    pub arrow_start_symbol: Handle,
    pub arrow_end_symbol: Handle,
    pub arrow_symbol_color: Color,
    pub arrow_symbol_size: f64,
    pub identifier_excluded_characters: String,
    pub arrow_symbol_extension_length: f64,
    pub plane_linetype: Handle,
    pub plane_lineweight: i32,
    pub plane_color: Color,
    pub bend_linetype: Handle,
    pub bend_lineweight: i32,
    pub bend_color: Color,
    pub bend_line_length: f64,
    pub end_line_length: f64,
    pub view_label_text_style: Handle,
    pub view_label_text_color: Color,
    pub view_label_text_height: f64,
    pub view_label_attachment: i32,
    pub view_label_offset: f64,
    pub view_label_alignment: i32,
    pub view_label_pattern: String,
    pub hatch_color: Color,
    pub hatch_background_color: Color,
    pub hatch_pattern: String,
    pub hatch_scale: f64,
    pub hatch_transparency: i32,
    pub reserved_flags: [bool; 2],
    pub identifier_position: i32,
    pub identifier_offset: f64,
    pub arrow_position: i32,
    pub end_line_overshoot: f64,
    pub hatch_angles: Vec<f64>,
}
