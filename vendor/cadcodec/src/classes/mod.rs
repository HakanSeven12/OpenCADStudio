//! DXF class definitions (CLASSES section)
//!
//! Classes define custom object types registered in the DXF drawing.
//! Each class maps a DXF entity/object name to its C++ class name and
//! application that registered it.
//!
//! Corresponds to the classic `DxfClass` and `DxfClassCollection`.

use std::collections::HashMap;

/// Proxy capability flags for DXF class definitions.
///
/// These flags control what operations are allowed on proxy entities/objects
/// when the application that created them is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProxyFlags(pub u16);

impl ProxyFlags {
    pub const NONE: Self = Self(0);
    pub const ERASE_ALLOWED: Self = Self(1);
    pub const TRANSFORM_ALLOWED: Self = Self(2);
    pub const COLOR_CHANGE_ALLOWED: Self = Self(4);
    pub const LAYER_CHANGE_ALLOWED: Self = Self(8);
    pub const LINETYPE_CHANGE_ALLOWED: Self = Self(16);
    pub const LINETYPE_SCALE_CHANGE_ALLOWED: Self = Self(32);
    pub const VISIBILITY_CHANGE_ALLOWED: Self = Self(64);
    pub const CLONING_ALLOWED: Self = Self(128);
    pub const LINEWEIGHT_CHANGE_ALLOWED: Self = Self(256);
    pub const PLOT_STYLE_NAME_CHANGE_ALLOWED: Self = Self(512);
    pub const ALL_OPERATIONS_EXCEPT_CLONING: Self = Self(895);
    pub const ALL_OPERATIONS_ALLOWED: Self = Self(1023);
    pub const DISABLES_PROXY_WARNING_DIALOG: Self = Self(1024);
    pub const R13_FORMAT_PROXY: Self = Self(32768);

    /// Check if a specific flag is set
    pub fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

impl Default for ProxyFlags {
    fn default() -> Self {
        Self::NONE
    }
}

impl From<u16> for ProxyFlags {
    fn from(val: u16) -> Self {
        Self(val)
    }
}

impl From<i32> for ProxyFlags {
    fn from(val: i32) -> Self {
        Self(val as u16)
    }
}

/// A single DXF class definition.
///
/// DXF group codes:
/// - 1: DXF class name (e.g. "MLEADERSTYLE")
/// - 2: C++ class name (e.g. "AcDbMLeaderStyle")
/// - 3: Application name (e.g. "ObjectDBX Classes")
/// - 90: Proxy capability flags
/// - 91: Instance count (informational)
/// - 280: Was-a-zombie flag
/// - 281: Is-an-entity flag (1 = can appear in ENTITIES/BLOCKS, 0 = OBJECTS only)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DxfClass {
    /// DXF class name (group code 1) — e.g. "ACDBPLACEHOLDER"
    pub dxf_name: String,
    /// C++ class name (group code 2) — e.g. "AcDbPlaceHolder"
    pub cpp_class_name: String,
    /// Application name (group code 3) — e.g. "ObjectDBX Classes"
    pub application_name: String,
    /// Proxy capability flags (group code 90)
    pub proxy_flags: ProxyFlags,
    /// Instance count for this class in the drawing (group code 91)
    pub instance_count: i32,
    /// Was-a-zombie flag (group code 280) — true if class was a proxy
    pub was_zombie: bool,
    /// Is-an-entity flag (group code 281) — true if instances can appear in ENTITIES/BLOCKS
    pub is_an_entity: bool,
    /// Class number (assigned sequentially starting at 500)
    pub class_number: i16,
    /// Item class ID: 498 for entities, 499 for objects
    pub item_class_id: i16,
    /// DWG schema version recorded by the class entry (R2004+).
    pub dwg_version: i32,
    /// DWG maintenance release recorded by the class entry (R2004+).
    pub maintenance_version: i32,
    /// First reserved class metadata value (normally zero).
    pub unknown1: i32,
    /// Second reserved class metadata value (normally zero).
    pub unknown2: i32,
}

impl DxfClass {
    /// Create a new DXF class definition
    pub fn new(dxf_name: impl Into<String>, cpp_class_name: impl Into<String>) -> Self {
        Self {
            dxf_name: dxf_name.into(),
            cpp_class_name: cpp_class_name.into(),
            application_name: "ObjectDBX Classes".to_string(),
            proxy_flags: ProxyFlags::NONE,
            instance_count: 0,
            was_zombie: false,
            is_an_entity: false,
            class_number: 0,
            item_class_id: 499, // default to object
            dwg_version: 0,
            maintenance_version: 0,
            unknown1: 0,
            unknown2: 0,
        }
    }

    /// Create a class for an entity type (can appear in ENTITIES/BLOCKS)
    pub fn new_entity(dxf_name: impl Into<String>, cpp_class_name: impl Into<String>) -> Self {
        let mut class = Self::new(dxf_name, cpp_class_name);
        class.is_an_entity = true;
        class.item_class_id = 498;
        class
    }
}

/// Collection of DXF class definitions, keyed by DXF name (case-insensitive).
///
/// Corresponds to the classic `DxfClassCollection`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DxfClassCollection {
    entries: Vec<DxfClass>,
    name_index: HashMap<String, usize>,
}

impl DxfClassCollection {
    /// Create an empty class collection
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            name_index: HashMap::new(),
        }
    }

    /// Add a class. If a class with the same DXF name already exists,
    /// only its instance count is updated (matching the reference behavior).
    pub fn add_or_update(&mut self, mut class: DxfClass) {
        let key = class.dxf_name.to_uppercase();
        if let Some(&idx) = self.name_index.get(&key) {
            self.entries[idx].instance_count = class.instance_count;
        } else {
            if class.class_number < 500 {
                class.class_number = 500 + self.entries.len() as i16;
            }
            let idx = self.entries.len();
            self.name_index.insert(key, idx);
            self.entries.push(class);
        }
    }

    /// Append a class verbatim, preserving order and duplicate dxf names.
    ///
    /// The DWG classes section is *positional*: a custom object's type code is
    /// `500 + index_in_section`, and readers (libredwg, AutoCAD) resolve the
    /// class by that index, not by the stored number. Two distinct classes may
    /// legitimately share a dxf name (different C++ classes), so deduping by
    /// name — as [`add_or_update`](Self::add_or_update) does — drops one entry,
    /// shifts every later class's effective number, and makes those objects
    /// resolve to the wrong class. The DWG reader must preserve every entry.
    pub fn push_preserving(&mut self, class: DxfClass) {
        let key = class.dxf_name.to_uppercase();
        let idx = self.entries.len();
        // Last writer wins for name lookup; positional `entries` keeps all.
        self.name_index.insert(key, idx);
        self.entries.push(class);
    }

    /// Get a class by its DXF name (case-insensitive)
    pub fn get_by_name(&self, dxf_name: &str) -> Option<&DxfClass> {
        let key = dxf_name.to_uppercase();
        self.name_index.get(&key).map(|&idx| &self.entries[idx])
    }

    /// Check if a class with the given DXF name exists
    pub fn contains(&self, dxf_name: &str) -> bool {
        self.name_index.contains_key(&dxf_name.to_uppercase())
    }

    /// Number of class definitions
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the collection is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all class definitions
    pub fn iter(&self) -> impl Iterator<Item = &DxfClass> {
        self.entries.iter()
    }

    /// Clear all class definitions
    pub fn clear(&mut self) {
        self.entries.clear();
        self.name_index.clear();
    }

    /// Retain the class table understood by pre-R2013 DWG writers.
    ///
    /// Modern proxy classes use layouts that the AC15 CLASSES stream cannot
    /// encode. Their presence can make a reader reject the complete drawing,
    /// including otherwise valid primitive entities.
    pub fn retain_legacy_dwg_classes(&mut self) {
        const LEGACY: &[&str] = &[
            "ACDBDICTIONARYWDFLT",
            "DICTIONARYVAR",
            "LAYOUT",
            "ACDBPLACEHOLDER",
            "PLOTSETTINGS",
            "SCALE",
            "MESH",
            "ACAD_TABLE",
            "WIPEOUT",
            "IMAGE",
            "PDFREFERENCE",
            "DWFREFERENCE",
            "DGNREFERENCE",
            "MULTILEADER",
            "OLE2FRAME",
            "MLINE",
            "TABLESTYLE",
            "MATERIAL",
            "VISUALSTYLE",
            "MLEADERSTYLE",
            "CELLSTYLEMAP",
            "XRECORD",
            "SORTENTSTABLE",
            "WIPEOUTVARIABLES",
            "DIMASSOC",
            "TABLECONTENT",
            "TABLEGEOMETRY",
            "RASTERVARIABLES",
            "IMAGEDEF",
            "IMAGEDEF_REACTOR",
            "DBCOLOR",
            "GEODATA",
            "PDFDEFINITION",
            "DWFDEFINITION",
            "DGNDEFINITION",
            "SPATIAL_FILTER",
            "GROUP",
            "MLINESTYLE",
        ];

        self.entries
            .retain(|class| LEGACY.contains(&class.dxf_name.to_ascii_uppercase().as_str()));
        self.name_index.clear();
        for (index, class) in self.entries.iter_mut().enumerate() {
            class.class_number = 500 + index as i16;
            self.name_index
                .insert(class.dxf_name.to_ascii_uppercase(), index);
        }
    }

    /// Populate with default class definitions that AutoCAD expects.
    ///
    /// This mirrors the reference `DxfClassCollection.UpdateDxfClasses()`.
    pub fn update_defaults(&mut self) {
        let defaults = default_classes();
        for class in defaults {
            if !self.contains(&class.dxf_name) {
                self.add_or_update(class);
            }
        }
    }
}

impl Default for DxfClassCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> IntoIterator for &'a DxfClassCollection {
    type Item = &'a DxfClass;
    type IntoIter = std::slice::Iter<'a, DxfClass>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter()
    }
}

/// Build the set of default DXF classes that AutoCAD registers.
///
/// DXF names, proxy flags, and application names match AutoCAD R2013 (AC1027)
/// reference output.
fn default_classes() -> Vec<DxfClass> {
    // (dxf_name, cpp_class_name, proxy_flags, app_name, is_entity)
    let defs: &[(&str, &str, u16, &str, bool)] = &[
        // ── Entity classes ──────────────────────────────────────────
        ("MESH", "AcDbSubDMesh", 4095,
            "AcDbSubDMesh|Description: AutoCAD subD mesh", true),
        ("ACAD_TABLE", "AcDbTable", 1025, "ObjectDBX Classes", true),
        ("WIPEOUT", "AcDbWipeout", 127,
            "WipeOut|AutoCAD Express Tool|www.autodesk.com", true),
        ("IMAGE", "AcDbRasterImage", 127, "ISM", true),
        ("PDFREFERENCE", "AcDbPdfReference", 1, "ObjectDBX Classes", true),
        ("DWFREFERENCE", "AcDbDwfReference", 1, "ObjectDBX Classes", true),
        ("DGNREFERENCE", "AcDbDgnReference", 1, "ObjectDBX Classes", true),
        // Underlay reference entities (PDF/DWF/DGN). The matching definition
        // classes (PDFDEFINITION etc.) are registered in the object section
        // below; the reference classes must be present too, or the DWG writer
        // has no class number to emit for an underlay and silently drops it.
        ("PDFUNDERLAY", "AcDbPdfReference", 1, "ObjectDBX Classes", true),
        ("DWFUNDERLAY", "AcDbDwfReference", 1, "ObjectDBX Classes", true),
        ("DGNUNDERLAY", "AcDbDgnReference", 1, "ObjectDBX Classes", true),
        ("HELIX", "AcDbHelix", 0, "ObjectDBX Classes", true),
        ("MULTILEADER", "AcDbMLeader", 1025, "ACDB_MLEADER_CLASS", true),
        ("OLE2FRAME", "AcDbOle2Frame", 1, "ObjectDBX Classes", true),
        ("MLINE", "AcDbMline", 1, "ObjectDBX Classes", true),
        ("ARC_DIMENSION", "AcDbArcDimension", 0, "ObjectDBX Classes", true),
        (
            "LARGE_RADIAL_DIMENSION",
            "AcDbRadialDimensionLarge",
            0,
            "ObjectDBX Classes",
            true,
        ),
        ("CAMERA", "AcDbCamera", 0, "ObjectDBX Classes", true),
        ("SECTIONOBJECT", "AcDbSection", 0, "ObjectDBX Classes", true),
        (
            "ARCALIGNEDTEXT",
            "AcDbArcAlignedText",
            127,
            "AutoCAD Express Tools",
            true,
        ),
        ("RTEXT", "AcDbRText", 127, "AutoCAD Express Tools", true),
        (
            "POSITIONMARKER",
            "AcDbGeoPositionMarker",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "COORDINATION_MODEL",
            "AcDbNavisworksModel",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "ACDBPOINTCLOUD",
            "AcDbPointCloud",
            0,
            "AcDbPointCloudObj",
            true,
        ),
        (
            "ACDBPOINTCLOUDEX",
            "AcDbPointCloudEx",
            0,
            "AcDbPointCloudObj",
            true,
        ),
        ("MPOLYGON", "AcDbMPolygon", 127, "ObjectDBX Classes", true),
        (
            "ALIGNMENTPARAMETERENTITY",
            "AcDbBlockAlignmentParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "BASEPOINTPARAMETERENTITY",
            "AcDbBlockBasepointParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "FLIPPARAMETERENTITY",
            "AcDbBlockFlipParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "LINEARPARAMETERENTITY",
            "AcDbBlockLinearParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "POINTPARAMETERENTITY",
            "AcDbBlockPointParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "ROTATIONPARAMETERENTITY",
            "AcDbBlockRotationParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "VISIBILITYPARAMETERENTITY",
            "AcDbBlockVisibilityParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "FLIPGRIPENTITY",
            "AcDbBlockFlipGripEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "LINEARGRIPENTITY",
            "AcDbBlockLinearGripEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "POLARGRIPENTITY",
            "AcDbBlockPolarGripEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "ROTATIONGRIPENTITY",
            "AcDbBlockRotationGripEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "VISIBILITYGRIPENTITY",
            "AcDbBlockVisibilityGripEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "XYGRIPENTITY",
            "AcDbBlockXYGripEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "XYPARAMETERENTITY",
            "AcDbBlockXYParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),
        (
            "BLOCKANGULARCONSTRAINTPARAMETERENTITY",
            "AcDbBlockAngularConstraintParameterEntity",
            0,
            "ObjectDBX Classes",
            true,
        ),

        // Native loft sheets must retain their subtype in DWG class streams.
        ("LOFTEDSURFACE", "AcDbLoftedSurface", 0, "ObjectDBX Classes", true),

        // ── Object classes ──────────────────────────────────────────
        ("ACDBASSOCDEPENDENCY", "AcDbAssocDependency", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCVALUEDEPENDENCY", "AcDbAssocValueDependency", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCGEOMDEPENDENCY", "AcDbAssocGeomDependency", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCACTION", "AcDbAssocAction", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCNETWORK", "AcDbAssocNetwork", 0, "ObjectDBX Classes", false),
        ("ACDBASSOC2DCONSTRAINTGROUP", "AcDbAssoc2dConstraintGroup", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCVARIABLE", "AcDbAssocVariable", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCPERSSUBENTMANAGER", "AcDbAssocPersSubentManager", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCACTIONPARAM", "AcDbAssocActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCCOMPOUNDACTIONPARAM", "AcDbAssocCompoundActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCOSNAPPOINTREFACTIONPARAM", "AcDbAssocOsnapPointRefActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCPOINTREFACTIONPARAM", "AcDbAssocPointRefActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCOBJECTACTIONPARAM", "AcDbAssocObjectActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCPATHACTIONPARAM", "AcDbAssocPathActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCEDGEACTIONPARAM", "AcDbAssocEdgeActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCFACEACTIONPARAM", "AcDbAssocFaceActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCVERTEXACTIONPARAM", "AcDbAssocVertexActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCASMBODYACTIONPARAM", "AcDbAssocAsmbodyActionParam", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCDIMDEPENDENCYBODY", "AcDbAssocDimDependencyBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCPLANESURFACEACTIONBODY", "AcDbAssocPlaneSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCEXTENDSURFACEACTIONBODY", "AcDbAssocExtendSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCEXTRUDEDSURFACEACTIONBODY", "AcDbAssocExtrudedSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCLOFTEDSURFACEACTIONBODY", "AcDbAssocLoftedSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCNETWORKSURFACEACTIONBODY", "AcDbAssocNetworkSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCOFFSETSURFACEACTIONBODY", "AcDbAssocOffsetSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCREVOLVEDSURFACEACTIONBODY", "AcDbAssocRevolvedSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCTRIMSURFACEACTIONBODY", "AcDbAssocTrimSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCBLENDSURFACEACTIONBODY", "AcDbAssocBlendSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCPATCHSURFACEACTIONBODY", "AcDbAssocPatchSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCFILLETSURFACEACTIONBODY", "AcDbAssocFilletSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCSWEPTSURFACEACTIONBODY", "AcDbAssocSweptSurfaceActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCEDGECHAMFERACTIONBODY", "AcDbAssocEdgeChamferActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCEDGEFILLETACTIONBODY", "AcDbAssocEdgeFilletActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCRESTOREENTITYSTATEACTIONBODY", "AcDbAssocRestoreEntityStateActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCMLEADERACTIONBODY", "AcDbAssocMLeaderActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCALIGNEDDIMACTIONBODY", "AcDbAssocAlignedDimActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOC3POINTANGULARDIMACTIONBODY", "AcDbAssoc3PointAngularDimActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCORDINATEDIMACTIONBODY", "AcDbAssocOrdinateDimActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCROTATEDDIMACTIONBODY", "AcDbAssocRotatedDimActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCARRAYMODIFYPARAMETERS", "AcDbAssocArrayModifyParameters", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCARRAYPATHPARAMETERS", "AcDbAssocArrayPathParameters", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCARRAYPOLARPARAMETERS", "AcDbAssocArrayPolarParameters", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCARRAYRECTANGULARPARAMETERS", "AcDbAssocArrayRectangularParameters", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCARRAYACTIONBODY", "AcDbAssocArrayActionBody", 0, "ObjectDBX Classes", false),
        ("ACDBASSOCARRAYMODIFYACTIONBODY", "AcDbAssocArrayModifyActionBody", 0, "ObjectDBX Classes", false),
        ("PERSUBENTMGR", "AcDbPersSubentManager", 0, "ObjectDBX Classes", false),
        ("ACSH_BOOLEAN_CLASS", "AcDbShBoolean", 0, "ObjectDBX Classes", false),
        ("ACSH_BOX_CLASS", "AcDbShBox", 0, "ObjectDBX Classes", false),
        ("ACSH_BREP_CLASS", "AcDbShBrep", 0, "ObjectDBX Classes", false),
        ("ACSH_CHAMFER_CLASS", "AcDbShChamfer", 0, "ObjectDBX Classes", false),
        ("ACSH_CONE_CLASS", "AcDbShCone", 0, "ObjectDBX Classes", false),
        ("ACSH_CYLINDER_CLASS", "AcDbShCylinder", 0, "ObjectDBX Classes", false),
        ("ACSH_EXTRUSION_CLASS", "AcDbShExtrusion", 0, "ObjectDBX Classes", false),
        ("ACSH_FILLET_CLASS", "AcDbShFillet", 0, "ObjectDBX Classes", false),
        ("ACSH_HISTORY_CLASS", "AcDbShHistory", 0, "ObjectDBX Classes", false),
        ("ACSH_LOFT_CLASS", "AcDbShLoft", 0, "ObjectDBX Classes", false),
        ("ACSH_PYRAMID_CLASS", "AcDbShPyramid", 0, "ObjectDBX Classes", false),
        ("ACSH_REVOLVE_CLASS", "AcDbShRevolve", 0, "ObjectDBX Classes", false),
        ("ACSH_SPHERE_CLASS", "AcDbShSphere", 0, "ObjectDBX Classes", false),
        ("ACSH_SWEEP_CLASS", "AcDbShSweep", 0, "ObjectDBX Classes", false),
        ("ACSH_TORUS_CLASS", "AcDbShTorus", 0, "ObjectDBX Classes", false),
        ("ACSH_WEDGE_CLASS", "AcDbShWedge", 0, "ObjectDBX Classes", false),
        ("ACDB_BLOCKREPRESENTATION_DATA", "AcDbBlockRepresentationData", 0, "ObjectDBX Classes", false),
        ("ACDB_DYNAMICBLOCKPURGEPREVENTER_VERSION", "AcDbDynamicBlockPurgePreventer", 0, "ObjectDBX Classes", false),
        ("ACDB_DYNAMICBLOCKPROXYNODE", "AcDbDynamicBlockProxyNode", 0, "ObjectDBX Classes", false),
        ("ACAD_EVALUATION_GRAPH", "AcDbEvalGraph", 0, "ObjectDBX Classes", false),
        ("BLOCKGRIPLOCATIONCOMPONENT", "AcDbBlockGripExpr", 0, "ObjectDBX Classes", false),
        ("BLOCKALIGNMENTPARAMETER", "AcDbBlockAlignmentParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKALIGNMENTGRIP", "AcDbBlockAlignmentGrip", 0, "ObjectDBX Classes", false),
        ("BLOCKBASEPOINTPARAMETER", "AcDbBlockBasepointParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKFLIPACTION", "AcDbBlockFlipAction", 0, "ObjectDBX Classes", false),
        ("BLOCKFLIPPARAMETER", "AcDbBlockFlipParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKFLIPGRIP", "AcDbBlockFlipGrip", 0, "ObjectDBX Classes", false),
        ("BLOCKLINEARGRIP", "AcDbBlockLinearGrip", 0, "ObjectDBX Classes", false),
        ("BLOCKLOOKUPGRIP", "AcDbBlockLookupGrip", 0, "ObjectDBX Classes", false),
        ("BLOCKROTATIONGRIP", "AcDbBlockRotationGrip", 0, "ObjectDBX Classes", false),
        ("BLOCKMOVEACTION", "AcDbBlockMoveAction", 0, "ObjectDBX Classes", false),
        ("BLOCKROTATEACTION", "AcDbBlockRotationAction", 0, "ObjectDBX Classes", false),
        ("BLOCKSCALEACTION", "AcDbBlockScaleAction", 0, "ObjectDBX Classes", false),
        ("BLOCKVISIBILITYGRIP", "AcDbBlockVisibilityGrip", 0, "ObjectDBX Classes", false),
        ("BLOCKLINEARPARAMETER", "AcDbBlockLinearParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKROTATIONPARAMETER", "AcDbBlockRotationParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKXYPARAMETER", "AcDbBlockXYParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKVISIBILITYPARAMETER", "AcDbBlockVisibilityParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKPOLARPARAMETER", "AcDbBlockPolarParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKPOLARGRIP", "AcDbBlockPolarGrip", 0, "ObjectDBX Classes", false),
        ("ACDBBLOCKPARAMDEPENDENCYBODY", "AcDbBlockParameterDependencyBody", 0, "ObjectDBX Classes", false),
        ("BLOCKPARAMDEPENDENCYBODY", "AcDbBlockParameterDependencyBody", 0, "ObjectDBX Classes", false),
        ("ACMECOMMANDHISTORY", "AcMeCommandHistory", 0, "AcMeServices", false),
        ("ACMESCOPE", "AcMeScope", 0, "AcMeServices", false),
        ("ACMESTATEMGR", "AcMeStateMgr", 0, "AcMeServices", false),
        ("CSACDOCUMENTOPTIONS", "CDocDataContainer", 4095, "\"DERU003_CSAPP\"", false),
        ("BLOCKALIGNEDCONSTRAINTPARAMETER", "AcDbBlockAlignedConstraintParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKANGULARCONSTRAINTPARAMETER", "AcDbBlockAngularConstraintParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKARRAYACTION", "AcDbBlockArrayAction", 0, "ObjectDBX Classes", false),
        ("BLOCKDIAMETRICCONSTRAINTPARAMETER", "AcDbBlockDiametricConstraintParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKHORIZONTALCONSTRAINTPARAMETER", "AcDbBlockHorizontalConstraintParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKLINEARCONSTRAINTPARAMETER", "AcDbBlockLinearConstraintParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKRADIALCONSTRAINTPARAMETER", "AcDbBlockRadialConstraintParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKVERTICALCONSTRAINTPARAMETER", "AcDbBlockVerticalConstraintParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKLOOKUPACTION", "AcDbBlockLookupAction", 0, "ObjectDBX Classes", false),
        ("BLOCKLOOKUPPARAMETER", "AcDbBlockLookupParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKPOINTPARAMETER", "AcDbBlockPointParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKPOLARSTRETCHACTION", "AcDbBlockPolarStretchAction", 0, "ObjectDBX Classes", false),
        ("BLOCKSTRETCHACTION", "AcDbBlockStretchAction", 0, "ObjectDBX Classes", false),
        ("BLOCKUSERPARAMETER", "AcDbBlockUserParameter", 0, "ObjectDBX Classes", false),
        ("BLOCKXYGRIP", "AcDbBlockXYGrip", 0, "ObjectDBX Classes", false),
        ("BLOCKPROPERTIESTABLE", "AcDbBlockPropertiesTable", 0, "ObjectDBX Classes", false),
        ("BLOCKPROPERTIESTABLEGRIP", "AcDbBlockPropertiesTableGrip", 0, "ObjectDBX Classes", false),
        ("SPATIAL_INDEX", "AcDbSpatialIndex", 0, "ObjectDBX Classes", false),
        ("LAYERFILTER", "AcDbLayerFilter", 0, "ObjectDBX Classes", false),
        ("PARTIAL_VIEWING_INDEX", "OdDbPartialViewingIndex", 0, "ObjectDBX Classes", false),
        ("VBA_PROJECT", "AcDbVbaProject", 0, "ObjectDBX Classes", false),
        ("SECTION_MANAGER", "AcDbSectionManager", 0, "ObjectDBX Classes", false),
        ("SECTION_SETTINGS", "AcDbSectionSettings", 0, "ObjectDBX Classes", false),
        ("LIGHTLIST", "AcDbLightList", 0, "SCENEOE", false),
        ("SUN", "AcDbSun", 0, "SCENEOE", false),
        ("SUNSTUDY", "AcDbSunStudy", 0, "SCENEOE", false),
        ("RENDERSETTINGS", "AcDbRenderSettings", 0, "SCENEOE", false),
        ("MENTALRAYRENDERSETTINGS", "AcDbMentalRayRenderSettings", 0, "SCENEOE", false),
        ("RAPIDRTRENDERSETTINGS", "AcDbRapidRTRenderSettings", 0, "ObjectDBX Classes", false),
        ("RENDERENVIRONMENT", "AcDbRenderEnvironment", 0, "SCENEOE", false),
        ("RENDERGLOBAL", "AcDbRenderGlobal", 0, "SCENEOE", false),
        ("RENDERENTRY", "AcDbRenderEntry", 0, "SCENEOE", false),
        ("RAPIDRTRENDERENVIRONMENT", "AcDbIBLBackground", 0, "SCENEOE", false),
        ("SKYLIGHT_BACKGROUND", "AcDbSkyBackground", 0, "SCENEOE", false),
        ("IMAGE_BACKGROUND", "AcDbImageBackground", 0, "SCENEOE", false),
        ("SOLID_BACKGROUND", "AcDbSolidBackground", 0, "SCENEOE", false),
        ("GROUND_PLANE_BACKGROUND", "AcDbGroundPlaneBackground", 0, "SCENEOE", false),
        ("GRADIENT_BACKGROUND", "AcDbGradientBackground", 0, "SCENEOE", false),
        ("ACDBCURVEPATH", "AcDbCurvePath", 0, "SCENEOE", false),
        ("ACDBMOTIONPATH", "AcDbMotionPath", 0, "SCENEOE", false),
        ("ACDBPOINTPATH", "AcDbPointPath", 0, "SCENEOE", false),
        ("TVDEVICEPROPERTIES", "AcDbTvDeviceProperties", 0, "SCENEOE", false),
        ("ACDBPOINTCLOUDDEF", "AcDbPointCloudDef", 0, "AcDbPointCloudObj", false),
        ("ACDBPOINTCLOUDDEFEX", "AcDbPointCloudDefEx", 0, "AcDbPointCloudObj", false),
        ("ACDBPOINTCLOUDDEF_REACTOR", "AcDbPointCloudDefReactor", 0, "AcDbPointCloudObj", false),
        ("ACDBPOINTCLOUDDEF_REACTOR_EX", "AcDbPointCloudDefReactorEx", 0, "AcDbPointCloudObj", false),
        ("ACDBPOINTCLOUDCOLORMAP", "AcDbPointCloudColorMap", 0, "AcDbPointCloudObj", false),
        ("NAVISWORKSMODELDEF", "AcDbNavisworksModelDef", 0, "ObjectDBX Classes", false),
        ("CONTEXTDATAMANAGER", "AcDbContextDataManager", 0, "ObjectDBX Classes", false),
        ("DATATABLE", "AcDbDataTable", 0, "ObjectDBX Classes", false),
        ("DATALINK", "AcDbDataLink", 0, "ObjectDBX Classes", false),
        ("ACDBPERSSUBENTMANAGER", "AcDbPersSubentManager", 0, "ObjectDBX Classes", false),
        ("GEOMAPIMAGE", "AcDbGeomapImage", 0, "ObjectDBX Classes", false),
        ("ACDBDETAILVIEWSTYLE", "AcDbDetailViewStyle", 0, "ObjectDBX Classes", false),
        ("ACDBSECTIONVIEWSTYLE", "AcDbSectionViewStyle", 0, "ObjectDBX Classes", false),
        ("ACAD_PROXY_ENTITY_WRAPPER", "AcDbProxyEntityWrapper", 0, "ObjectDBX Classes", true),
        ("ACAD_PROXY_OBJECT_WRAPPER", "AcDbProxyObjectWrapper", 0, "ObjectDBX Classes", false),
        ("AEC_REFEDIT_STATUS_TRACKER", "AecDbRefEditStatusTracker", 0, "AecArchBase", false),
        ("EXACXREFPANELOBJECT", "ExAcXREFPanelObject", 0, "EXAC_ESW", false),
        ("XREFPANELOBJECT", "ExAcXREFPanelObject", 0, "EXAC_ESW", false),
        ("NPOCOLLECTION", "AcDbImpNonPersistentObjectsCollection", 0, "ObjectDBX Classes", false),
        ("LSDEFINITION", "AcDbLSDefinition", 0, "DgnLS", false),
        ("LSSYMBOLCOMPONENT", "AcDbLSSymbolComponent", 0, "DgnLS", false),
        ("LSCOMPOUNDCOMPONENT", "AcDbLSCompoundComponent", 0, "DgnLS", false),
        ("LSSTROKEPATTERNCOMPONENT", "AcDbLSStrokePatternComponent", 0, "DgnLS", false),
        ("LSPOINTCOMPONENT", "AcDbLSPointComponent", 0, "DgnLS", false),
        ("LSINTERNALCOMPONENT", "AcDbLSInternalComponent", 0, "DgnLS", false),
        ("McDbContainer2", "McDbContainer2", 0, "NanoSPDS", false),
        ("Wall", "PtDbWall", 0, "NanoSPDS", true),
        ("mcsDbObject", "mcsDbObject", 0, "NanoSPDS", true),
        ("NOTEPOSITION", "mcsDbObjectNotePosition", 0, "NanoSPDS", true),
        ("spdsLevelMark", "mcsDbObjectLevelMark", 0, "NanoSPDS", true),
        ("spdsRelationMark", "mcsDbObjectRelationMark", 0, "NanoSPDS", true),
        ("SECTIONLINE", "AcDbSectionSymbol", 0, "ObjectDBX Classes", true),
        ("DRAWINGVIEW", "AcDbViewBorder", 0, "ObjectDBX Classes", true),
        ("ACDB_ANNOTSCALEOBJECTCONTEXTDATA_CLASS", "AcDbAnnotScaleObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_BLKREFOBJECTCONTEXTDATA_CLASS", "AcDbBlkrefObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_TEXTOBJECTCONTEXTDATA_CLASS", "AcDbTextObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_MTEXTOBJECTCONTEXTDATA_CLASS", "AcDbMTextObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_ALDIMOBJECTCONTEXTDATA_CLASS", "AcDbAlignedDimensionObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_ANGDIMOBJECTCONTEXTDATA_CLASS", "AcDbAngularDimensionObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_DMDIMOBJECTCONTEXTDATA_CLASS", "AcDbDiametricDimensionObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_RADIMOBJECTCONTEXTDATA_CLASS", "AcDbRadialDimensionObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_RADIMLGOBJECTCONTEXTDATA_CLASS", "AcDbRadialDimensionLargeObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_ORDDIMOBJECTCONTEXTDATA_CLASS", "AcDbOrdinateDimensionObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_MLEADEROBJECTCONTEXTDATA_CLASS", "AcDbMLeaderObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_MTEXTATTRIBUTEOBJECTCONTEXTDATA_CLASS", "AcDbMTextAttributeObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_LEADEROBJECTCONTEXTDATA_CLASS", "AcDbLeaderObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_FCFOBJECTCONTEXTDATA_CLASS", "AcDbFcfObjectContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_HATCHSCALECONTEXTDATA_CLASS", "AcDbHatchScaleContextData", 0, "ObjectDBX Classes", false),
        ("ACDB_HATCHVIEWCONTEXTDATA_CLASS", "AcDbHatchViewContextData", 0, "ObjectDBX Classes", false),
        ("ACDBDICTIONARYWDFLT", "AcDbDictionaryWithDefault", 0, "ObjectDBX Classes", false),
        ("ACDBPLACEHOLDER", "AcDbPlaceHolder", 0, "ObjectDBX Classes", false),
        ("LAYOUT", "AcDbLayout", 0, "ObjectDBX Classes", false),
        ("DICTIONARYVAR", "AcDbDictionaryVar", 0, "ObjectDBX Classes", false),
        ("TABLESTYLE", "AcDbTableStyle", 1025, "ObjectDBX Classes", false),
        ("MATERIAL", "AcDbMaterial", 1025, "ObjectDBX Classes", false),
        ("VISUALSTYLE", "AcDbVisualStyle", 4095, "ObjectDBX Classes", false),
        ("SCALE", "AcDbScale", 1153, "ObjectDBX Classes", false),
        ("MLEADERSTYLE", "AcDbMLeaderStyle", 4095, "ACDB_MLEADERSTYLE_CLASS", false),
        ("CELLSTYLEMAP", "AcDbCellStyleMap", 1025, "ObjectDBX Classes", false),
        ("ACDSRECORD", "AcDsRecord", 0, "AcDsData", false),
        ("ACDSSCHEMA", "AcDsSchema", 0, "AcDsData", false),
        ("XRECORD", "AcDbXrecord", 0, "ObjectDBX Classes", false),
        ("SORTENTSTABLE", "AcDbSortentsTable", 0, "ObjectDBX Classes", false),
        ("BREAKDATA", "AcDbBreakData", 0, "ObjectDBX Classes", false),
        ("BREAKPOINTREF", "AcDbBreakPointRef", 0, "ObjectDBX Classes", false),
        ("IDBUFFER", "AcDbIdBuffer", 0, "ObjectDBX Classes", false),
        ("INDEX", "AcDbIndex", 0, "ObjectDBX Classes", false),
        ("LAYER_INDEX", "AcDbLayerIndex", 0, "ObjectDBX Classes", false),
        (
            "PARTIAL_VIEWING_FILTER",
            "OdDbPartialViewingFilter",
            0,
            "OdDbPartialViewing|https://www.opendesign.com Teigha(R) Core Db",
            false,
        ),
        ("WIPEOUTVARIABLES", "AcDbWipeoutVariables", 0,
            "WipeOut|AutoCAD Express Tool|www.autodesk.com", false),
        ("DIMASSOC", "AcDbDimAssoc", 0,
            "AcDbDimAssoc|Product Desc:     AcDim ARX App For Dimension|Company:          Autodesk|WEB Address:      www.autodesk.com", false),
        ("TABLECONTENT", "AcDbTableContent", 1025, "ObjectDBX Classes", false),
        ("TABLEGEOMETRY", "AcDbTableGeometry", 1025, "ObjectDBX Classes", false),
        ("Format", "mcsDbObjectFormat", 0, "NanoSPDS", true),
        ("RASTERVARIABLES", "AcDbRasterVariables", 0, "ISM", false),
        ("IMAGEDEF", "AcDbRasterImageDef", 0, "ISM", false),
        ("IMAGEDEF_REACTOR", "AcDbRasterImageDefReactor", 1, "ISM", false),
        ("DBCOLOR", "AcDbColor", 1025, "ObjectDBX Classes", false),
        ("GEODATA", "AcDbGeoData", 1025, "ObjectDBX Classes", false),
        ("PDFDEFINITION", "AcDbPdfDefinition", 1, "ObjectDBX Classes", false),
        ("DWFDEFINITION", "AcDbDwfDefinition", 1, "ObjectDBX Classes", false),
        ("DGNDEFINITION", "AcDbDgnDefinition", 1, "ObjectDBX Classes", false),
        ("SPATIAL_FILTER", "AcDbSpatialFilter", 1, "ObjectDBX Classes", false),
        ("PLOTSETTINGS", "AcDbPlotSettings", 0, "ObjectDBX Classes", false),
        ("GROUP", "AcDbGroup", 0, "ObjectDBX Classes", false),
        ("MLINESTYLE", "AcDbMlineStyle", 0, "ObjectDBX Classes", false),
    ];

    defs.iter()
        .map(|&(dxf, cpp, flags, app, is_entity)| {
            if is_entity {
                let mut c = DxfClass::new_entity(dxf, cpp);
                c.proxy_flags = ProxyFlags(flags);
                c.application_name = app.to_string();
                c
            } else {
                let mut c = DxfClass::new(dxf, cpp);
                c.proxy_flags = ProxyFlags(flags);
                c.application_name = app.to_string();
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dxf_class_creation() {
        let class = DxfClass::new("MLEADERSTYLE", "AcDbMLeaderStyle");
        assert_eq!(class.dxf_name, "MLEADERSTYLE");
        assert_eq!(class.cpp_class_name, "AcDbMLeaderStyle");
        assert!(!class.is_an_entity);
        assert_eq!(class.item_class_id, 499);
    }

    #[test]
    fn test_entity_class() {
        let class = DxfClass::new_entity("MESH", "AcDbSubDMesh");
        assert!(class.is_an_entity);
        assert_eq!(class.item_class_id, 498);
    }

    #[test]
    fn test_collection_add_or_update() {
        let mut coll = DxfClassCollection::new();

        let mut c = DxfClass::new("XRECORD", "AcDbXrecord");
        c.instance_count = 5;
        coll.add_or_update(c);
        assert_eq!(coll.len(), 1);
        assert_eq!(coll.get_by_name("XRECORD").unwrap().instance_count, 5);
        assert_eq!(coll.get_by_name("XRECORD").unwrap().class_number, 500);

        // Update instance count
        let mut c2 = DxfClass::new("xrecord", "AcDbXrecord");
        c2.instance_count = 10;
        coll.add_or_update(c2);
        assert_eq!(coll.len(), 1);
        assert_eq!(coll.get_by_name("XRECORD").unwrap().instance_count, 10);
    }

    #[test]
    fn test_collection_defaults() {
        let mut coll = DxfClassCollection::new();
        coll.update_defaults();
        assert!(coll.len() > 20);
        assert!(coll.contains("MESH"));
        assert!(coll.contains("LAYOUT"));
        assert!(coll.contains("MLEADERSTYLE"));
    }

    #[test]
    fn test_proxy_flags() {
        let flags = ProxyFlags::ALL_OPERATIONS_ALLOWED;
        assert!(flags.contains(ProxyFlags::ERASE_ALLOWED));
        assert!(flags.contains(ProxyFlags::TRANSFORM_ALLOWED));
        assert!(flags.contains(ProxyFlags::CLONING_ALLOWED));
    }
}
