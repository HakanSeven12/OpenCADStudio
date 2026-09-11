//! XRecord object - Extended record storage for arbitrary data

use crate::objects::{ProxyObjectReference, ProxyReferenceKind};
use crate::types::{Handle, Vector3};

#[cfg(feature = "serde")]
fn xrecord_entries_complete() -> bool {
    true
}

/// Dictionary cloning behavior flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DictionaryCloningFlags {
    /// Not applicable
    #[default]
    NotApplicable = 0,
    /// Keep existing record
    KeepExisting = 1,
    /// Use clone
    UseClone = 2,
    /// XRef name-based cloning
    XrefName = 3,
    /// Name-based cloning
    Name = 4,
    /// Unmangle name
    UnmangleName = 5,
}

impl DictionaryCloningFlags {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            1 => DictionaryCloningFlags::KeepExisting,
            2 => DictionaryCloningFlags::UseClone,
            3 => DictionaryCloningFlags::XrefName,
            4 => DictionaryCloningFlags::Name,
            5 => DictionaryCloningFlags::UnmangleName,
            _ => DictionaryCloningFlags::NotApplicable,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }

    /// Convert to DXF code (alias for to_value)
    pub fn to_code(&self) -> i16 {
        self.to_value()
    }
}

/// Group code value type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum XRecordValueType {
    /// String value
    String,
    /// 3D point
    Point3D,
    /// Double value
    Double,
    /// Byte value
    Byte,
    /// 16-bit integer
    Int16,
    /// 32-bit integer
    Int32,
    /// 64-bit integer
    Int64,
    /// Handle value
    Handle,
    /// Object ID (handle reference)
    ObjectId,
    /// Boolean value
    Bool,
    /// Binary chunk
    Chunk,
    /// Unknown type
    Unknown,
}

impl XRecordValueType {
    /// Determine value type from DXF group code
    pub fn from_code(code: i32) -> Self {
        match code {
            code if code < 0 => XRecordValueType::Handle,
            5 | 105 | 320..=329 | 480..=481 => XRecordValueType::Handle,
            330..=369 => XRecordValueType::ObjectId,
            390..=399 | 1005 => XRecordValueType::Handle,
            0..=4
            | 6..=9
            | 100..=102
            | 300..=309
            | 410..=419
            | 430..=439
            | 470..=479
            | 999
            | 1000..=1003 => XRecordValueType::String,
            10..=37 | 110..=139 | 210..=269 | 1010..=1039 | 1043..=1069 => {
                XRecordValueType::Point3D
            }
            38..=59 | 140..=149 | 460..=469 | 1040..=1042 => XRecordValueType::Double,
            280..=289 => XRecordValueType::Byte,
            60..=79 | 170..=179 | 270..=279 | 370..=389 | 400..=409 | 1070 => {
                XRecordValueType::Int16
            }
            80..=99 | 420..=429 | 440..=459 | 1071 => XRecordValueType::Int32,
            150..=169 => XRecordValueType::Int64,
            290..=299 => XRecordValueType::Bool,
            310..=319 | 1004 => XRecordValueType::Chunk,
            _ => XRecordValueType::Unknown,
        }
    }

    /// Check if this type represents a handle/reference
    pub fn is_handle(&self) -> bool {
        matches!(self, XRecordValueType::Handle | XRecordValueType::ObjectId)
    }
}

/// XRecord entry value
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum XRecordValue {
    /// String value
    String(String),
    /// Double value
    Double(f64),
    /// 16-bit integer
    Int16(i16),
    /// 32-bit integer
    Int32(i32),
    /// 64-bit integer
    Int64(i64),
    /// Byte value
    Byte(u8),
    /// Boolean value
    Bool(bool),
    /// Handle/Object reference
    Handle(Handle),
    /// 3D point (x, y, z)
    Point3D(f64, f64, f64),
    /// Binary data chunk
    Chunk(Vec<u8>),
}

impl XRecordValue {
    /// Get as string if this is a string value
    pub fn as_string(&self) -> Option<&str> {
        match self {
            XRecordValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get as f64 if this is a double value
    pub fn as_double(&self) -> Option<f64> {
        match self {
            XRecordValue::Double(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as i32 if this is an integer value
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            XRecordValue::Int32(v) => Some(*v),
            XRecordValue::Int16(v) => Some(*v as i32),
            XRecordValue::Byte(v) => Some(*v as i32),
            _ => None,
        }
    }

    /// Get as handle if this is a handle value
    pub fn as_handle(&self) -> Option<Handle> {
        match self {
            XRecordValue::Handle(h) => Some(*h),
            _ => None,
        }
    }

    /// Get as bool if this is a boolean value
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            XRecordValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Get as 3D point if this is a point value
    pub fn as_point3d(&self) -> Option<(f64, f64, f64)> {
        match self {
            XRecordValue::Point3D(x, y, z) => Some((*x, *y, *z)),
            _ => None,
        }
    }
}

/// XRecord entry with group code and value
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XRecordEntry {
    /// DXF group code (1-369, except 5 and 105)
    pub code: i32,
    /// The stored value
    pub value: XRecordValue,
}

impl XRecordEntry {
    /// Create a new entry
    pub fn new(code: i32, value: XRecordValue) -> Self {
        Self { code, value }
    }

    /// Create a string entry
    pub fn string(code: i32, value: impl Into<String>) -> Self {
        Self::new(code, XRecordValue::String(value.into()))
    }

    /// Create a double entry
    pub fn double(code: i32, value: f64) -> Self {
        Self::new(code, XRecordValue::Double(value))
    }

    /// Create an i16 entry
    pub fn int16(code: i32, value: i16) -> Self {
        Self::new(code, XRecordValue::Int16(value))
    }

    /// Create an i32 entry
    pub fn int32(code: i32, value: i32) -> Self {
        Self::new(code, XRecordValue::Int32(value))
    }

    /// Create a handle entry
    pub fn handle(code: i32, value: Handle) -> Self {
        Self::new(code, XRecordValue::Handle(value))
    }

    /// Create a bool entry
    pub fn bool(code: i32, value: bool) -> Self {
        Self::new(code, XRecordValue::Bool(value))
    }

    /// Create a point entry
    pub fn point3d(x_code: i32, x: f64, y: f64, z: f64) -> Self {
        Self::new(x_code, XRecordValue::Point3D(x, y, z))
    }

    /// Get the value type for this entry
    pub fn value_type(&self) -> XRecordValueType {
        XRecordValueType::from_code(self.code)
    }

    /// Check if this entry contains a linked object reference
    pub fn has_linked_object(&self) -> bool {
        matches!(self.value, XRecordValue::Handle(_))
    }
}

/// Well-known application schemas stored in an [`XRecord`].
///
/// The payload always remains available through [`XRecord::entries`].  This
/// classification only adds a stable, typed access path for schemas used by
/// AutoCAD and Autodesk vertical products; unknown/private schemas are kept
/// losslessly as ordinary group-code/value pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum KnownXRecordKind {
    LayerViewportAlphaOverride,
    LayerViewportColorOverride,
    LayerViewportLinetypeOverride,
    LayerViewportLineweightOverride,
    LayerReconciled,
    LayerStateAnnotationScale,
    AnnotationScale,
    MeshTextureCoordinates,
    XrecordRoundTrip,
    MTextRoundTrip,
    HeaderRoundTrip,
    RecomposeData,
    DynamicBlockHistory,
    LayoutThumbnail,
    ViewTransition,
    PhotometricLight,
    LastSavedVersion,
    PreviousProductInfo,
    DrawingProperties,
    FingerprintGuid,
    DimensionStyleData,
    UcsData,
    PlotData,
    Hyperlink,
    AdvancedMaterial,
    MaterialAsset,
    Metadata,
    Unknown,
}

impl KnownXRecordKind {
    /// Classify an XRecord dictionary key without changing its payload.
    pub fn from_name(name: &str) -> Self {
        match name.to_ascii_uppercase().as_str() {
            "ADSK_XREC_LAYER_ALPHA_OVR" => Self::LayerViewportAlphaOverride,
            "ADSK_XREC_LAYER_COLOR_OVR" => Self::LayerViewportColorOverride,
            "ADSK_XREC_LAYER_LINETYPE_OVR" => Self::LayerViewportLinetypeOverride,
            "ADSK_XREC_LAYER_LINEWT_OVR" => Self::LayerViewportLineweightOverride,
            "ADSK_XREC_LAYER_RECONCILED" => Self::LayerReconciled,
            "ACADLAYERSTATEANNOSCALE" => Self::LayerStateAnnotationScale,
            "ASDK_XREC_ANNOTATION_SCALE_INFO"
            | "ASDK_XREC_ANNO_SCALE_INFO"
            | "ADSK_XREC_VTR_ANNOSCALE_DATA" => Self::AnnotationScale,
            "ADSK_XREC_SUBDVERTEXTEXCOORDS" => Self::MeshTextureCoordinates,
            "ACAD_XREC_ROUNDTRIP" => Self::XrecordRoundTrip,
            "ACAD_MTEXT_RT" | "ACAD_MTEXT_2008_RT" => Self::MTextRoundTrip,
            "ACDBHEADERROUNDTRIPXREC" | "AUXHDRRNDTRIPDATA" => Self::HeaderRoundTrip,
            "ACDB_RECOMPOSE_DATA" => Self::RecomposeData,
            "ACAD_ENHANCEDBLOCKHISTORY" => Self::DynamicBlockHistory,
            "ASEBLOCKHIERARCHYINDEXRECORD" => Self::DynamicBlockHistory,
            "ADSK_XREC_LAYOUTTHUMBNAIL" => Self::LayoutThumbnail,
            "ADSK_XREC_VTRANIMATIONINFO" | "ADSK_XREC_VTRTHUMBNAIL" | "ADSK_XREC_VTRVIEWINFO" => {
                Self::ViewTransition
            }
            "ADSK_XREC_PHOTOMETRICLIGHTINFO" | "LIGHTINGQUALITY" => Self::PhotometricLight,
            "ACAD_LAST_SAVED_VERSION_INFO" => Self::LastSavedVersion,
            "ACAD_CIP_PREVIOUS_PRODUCT_INFO" => Self::PreviousProductInfo,
            "DWGPROPS" => Self::DrawingProperties,
            "FINGERPRINTGUID" => Self::FingerprintGuid,
            "ACADDIM" | "DIMSTYLEDATA" | "DIMLTEX1" | "DIMLTEX2" | "DIMLTYPE" | "TSTACKALIGN"
            | "TSTACKSIZE" => Self::DimensionStyleData,
            "PUCSBASE" | "PUCSORGBACK" | "PUCSORGBOTTOM" | "PUCSORGFRONT" | "PUCSORGLEFT"
            | "PUCSORGRIGHT" | "PUCSORGTOP" | "PUCSORTHOREF" | "PUCSORTHOVIEW" | "UCSBASE"
            | "UCSORGBACK" | "UCSORGBOTTOM" | "UCSORGFRONT" | "UCSORGLEFT" | "UCSORGRIGHT"
            | "UCSORGTOP" | "UCSORTHOREF" | "UCSORTHOVIEW" => Self::UcsData,
            "ACAD_LAYOUTSELFREF" | "LAYOUTDICT" | "PLOTSETDICT" | "PLOTSTYLNAMDICT"
            | "PSVPSCALE" | "STYLESHEET" => Self::PlotData,
            "HYPERLINKBASE" => Self::Hyperlink,
            "ADVMATERIAL" => Self::AdvancedMaterial,
            "FBXASSET"
            | "BUMPTILE"
            | "DIFFUSETILE"
            | "OPACITYTILE"
            | "REFLECTIONTILE"
            | "REFRACTIONTILE"
            | "SPECULARTILE"
            | "BUMP"
            | "DIFFUSE"
            | "MATERIALDICT"
            | "VIZ XML MATERIAL DEFINITION" => Self::MaterialAsset,
            "ACAD_MLATT"
            | "ACAD_VIEWS_VIEW_CUSTOM"
            | "AECDEPRECATIONHISTORY"
            | "CEPSNID"
            | "CEPSNTYPE"
            | "COLORDICT"
            | "INSUNITS"
            | "LWETCUNION"
            | "MCS_DOCUMENT_ID"
            | "MCS_PARAMS_DATA"
            | "MC_VERSION_DATA"
            | "VERSIONGUID" => Self::Metadata,
            _ => Self::Unknown,
        }
    }
}

/// A code-102 delimited section embedded in an XRecord payload.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XRecordSection {
    pub name: String,
    pub entries: Vec<XRecordEntry>,
}

/// XRecord object - stores arbitrary extended data
///
/// XRecords can store any DXF group code/value pairs and are commonly
/// used by applications to store custom data in DXF/DWG files.
///
/// # Example
/// ```ignore
/// use acadrust::objects::XRecord;
///
/// let mut xrecord = XRecord::new();
/// xrecord.add_string(1, "Custom Data");
/// xrecord.add_double(40, 3.14159);
/// xrecord.add_int32(90, 42);
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XRecord {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle
    pub owner: Handle,
    /// Objects notified when this record changes.
    pub reactors: Vec<Handle>,
    /// Extension dictionary attached to this record.
    pub xdictionary_handle: Option<Handle>,
    /// Record name (optional, for named XRecords)
    pub name: String,
    /// Cloning behavior flags
    pub cloning_flags: DictionaryCloningFlags,
    /// Collection of data entries
    pub entries: Vec<XRecordEntry>,
    /// Object-id handle stream paired with 330-369 entries in DWG files.
    ///
    /// Reference kind is retained because ownership/pointer semantics are
    /// encoded in the DWG handle header, not in the eight-byte XRecord value.
    #[cfg_attr(feature = "serde", serde(default))]
    pub object_references: Vec<ProxyObjectReference>,
    /// Preserve the exact decoded DWG handle vector, including an intentionally
    /// empty vector. Cleared by typed mutation helpers so references are then
    /// regenerated from 330-369 entries.
    #[cfg_attr(feature = "serde", serde(default))]
    pub preserve_object_reference_stream: bool,
    /// Whether every byte in `raw_data` was decoded into `entries`.
    ///
    /// An incomplete payload is emitted verbatim for same-version DWG saves,
    /// which prevents a private or future group code from being truncated.
    #[cfg_attr(feature = "serde", serde(default = "xrecord_entries_complete"))]
    pub entries_complete: bool,
    /// Raw DWG data bytes (for roundtripping when entries are not parsed)
    pub raw_data: Vec<u8>,
    /// Original merged DWG object record for exact-version round-trips.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_dwg_data: Option<Vec<u8>>,
    /// Handle-stream bit count stored alongside `raw_dwg_data`.
    pub raw_dwg_handle_bits: i64,
    /// DWG version that produced `raw_dwg_data`.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub raw_dwg_version: Option<crate::types::DxfVersion>,
}

impl XRecord {
    /// Object type name
    pub const OBJECT_TYPE: &'static str = "XRECORD";

    /// Create a new empty XRecord
    pub fn new() -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            reactors: Vec::new(),
            xdictionary_handle: None,
            name: String::new(),
            cloning_flags: DictionaryCloningFlags::NotApplicable,
            entries: Vec::new(),
            object_references: Vec::new(),
            preserve_object_reference_stream: false,
            entries_complete: true,
            raw_data: Vec::new(),
            raw_dwg_data: None,
            raw_dwg_handle_bits: 0,
            raw_dwg_version: None,
        }
    }

    /// Create a named XRecord
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::new()
        }
    }

    /// Add an entry to the record
    pub fn add_entry(&mut self, entry: XRecordEntry) {
        let is_object_id = (330..=369).contains(&entry.code);
        self.entries.push(entry);
        if is_object_id {
            self.synchronize_object_references();
        }
        self.entries_complete = true;
    }

    /// Create and add a string entry
    pub fn add_string(&mut self, code: i32, value: impl Into<String>) {
        self.entries.push(XRecordEntry::string(code, value));
        self.entries_complete = true;
    }

    /// Create and add a double entry
    pub fn add_double(&mut self, code: i32, value: f64) {
        self.entries.push(XRecordEntry::double(code, value));
        self.entries_complete = true;
    }

    /// Create and add an i16 entry
    pub fn add_int16(&mut self, code: i32, value: i16) {
        self.entries.push(XRecordEntry::int16(code, value));
        self.entries_complete = true;
    }

    /// Create and add an i32 entry
    pub fn add_int32(&mut self, code: i32, value: i32) {
        self.entries.push(XRecordEntry::int32(code, value));
        self.entries_complete = true;
    }

    /// Create and add a handle entry
    pub fn add_handle(&mut self, code: i32, value: Handle) {
        self.entries.push(XRecordEntry::handle(code, value));
        if (330..=369).contains(&code) {
            self.synchronize_object_references();
        }
        self.entries_complete = true;
    }

    /// Create and add a bool entry
    pub fn add_bool(&mut self, code: i32, value: bool) {
        self.entries.push(XRecordEntry::bool(code, value));
        self.entries_complete = true;
    }

    /// Create and add a point entry
    pub fn add_point3d(&mut self, x_code: i32, x: f64, y: f64, z: f64) {
        self.entries.push(XRecordEntry::point3d(x_code, x, y, z));
        self.entries_complete = true;
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the record is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an entry by index
    pub fn get(&self, index: usize) -> Option<&XRecordEntry> {
        self.entries.get(index)
    }

    /// Get all entries with a specific code
    pub fn get_by_code(&self, code: i32) -> Vec<&XRecordEntry> {
        self.entries.iter().filter(|e| e.code == code).collect()
    }

    /// Get the first entry with a specific code
    pub fn get_first_by_code(&self, code: i32) -> Option<&XRecordEntry> {
        self.entries.iter().find(|e| e.code == code)
    }

    /// Get all string values with a specific code
    pub fn get_strings(&self, code: i32) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.code == code)
            .filter_map(|e| e.value.as_string())
            .collect()
    }

    /// Get the first string value with a specific code
    pub fn get_string(&self, code: i32) -> Option<&str> {
        self.get_first_by_code(code)?.value.as_string()
    }

    /// Get the first double value with a specific code
    pub fn get_double(&self, code: i32) -> Option<f64> {
        self.get_first_by_code(code)?.value.as_double()
    }

    /// Get the first i32 value with a specific code
    pub fn get_i32(&self, code: i32) -> Option<i32> {
        self.get_first_by_code(code)?.value.as_i32()
    }

    /// Remove all entries with a specific code
    pub fn remove_by_code(&mut self, code: i32) {
        self.entries.retain(|e| e.code != code);
        if (330..=369).contains(&code) {
            self.synchronize_object_references();
        }
        self.entries_complete = true;
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
        self.object_references.clear();
        self.preserve_object_reference_stream = false;
        self.entries_complete = true;
    }

    /// Get all referenced handles
    pub fn get_references(&self) -> Vec<Handle> {
        let mut references: Vec<Handle> = self
            .entries
            .iter()
            .filter_map(|e| e.value.as_handle())
            .collect();
        references.extend(self.object_references.iter().map(|value| value.handle));
        references.sort_by_key(|handle| handle.value());
        references.dedup();
        references
    }

    /// Iterate over entries
    pub fn iter(&self) -> impl Iterator<Item = &XRecordEntry> {
        self.entries.iter()
    }

    /// Return the well-known schema associated with this record's dictionary
    /// key. Unknown schemas remain fully editable through `entries`.
    pub fn known_kind(&self) -> KnownXRecordKind {
        KnownXRecordKind::from_name(&self.name)
    }

    /// Parse balanced code-102 `{NAME` / `}` groups while preserving entry
    /// order. Nested groups are supported.
    pub fn sections(&self) -> Vec<XRecordSection> {
        let mut sections = Vec::new();
        let mut stack: Vec<(String, Vec<XRecordEntry>)> = Vec::new();
        for entry in &self.entries {
            match (entry.code, &entry.value) {
                (102, XRecordValue::String(value)) if value.starts_with('{') => {
                    stack.push((value.trim_start_matches('{').to_string(), Vec::new()));
                }
                (102, XRecordValue::String(value))
                    if stack.last().is_some_and(|(name, _)| {
                        value == "}"
                            || value
                                .strip_suffix('}')
                                .is_some_and(|closing| closing.eq_ignore_ascii_case(name))
                    }) =>
                {
                    if let Some((name, entries)) = stack.pop() {
                        let section = XRecordSection { name, entries };
                        if let Some((_, parent)) = stack.last_mut() {
                            parent.push(XRecordEntry::string(102, format!("{{{}", section.name)));
                            parent.extend(section.entries.clone());
                            parent.push(XRecordEntry::string(102, "}"));
                        }
                        sections.push(section);
                    }
                }
                _ => {
                    if let Some((_, entries)) = stack.last_mut() {
                        entries.push(entry.clone());
                    }
                }
            }
        }
        sections
    }

    /// Get a code-102 section by name.
    pub fn section(&self, name: &str) -> Option<XRecordSection> {
        self.sections()
            .into_iter()
            .find(|section| section.name.eq_ignore_ascii_case(name))
    }

    /// Replace or append a balanced code-102 section.
    pub fn set_section(&mut self, name: &str, entries: Vec<XRecordEntry>) {
        let start_text = format!("{{{name}");
        let mut start = None;
        let mut depth = 0usize;
        let mut end = None;
        for (index, entry) in self.entries.iter().enumerate() {
            match (entry.code, &entry.value) {
                (102, XRecordValue::String(value)) if value.eq_ignore_ascii_case(&start_text) => {
                    if start.is_none() {
                        start = Some(index);
                        depth = 1;
                    } else if depth > 0 {
                        depth += 1;
                    }
                }
                (102, XRecordValue::String(value)) if start.is_some() && value.starts_with('{') => {
                    depth += 1;
                }
                (102, XRecordValue::String(value))
                    if start.is_some()
                        && (value == "}" || (!value.starts_with('{') && value.ends_with('}'))) =>
                {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let replacement = std::iter::once(XRecordEntry::string(102, start_text))
            .chain(entries)
            .chain(std::iter::once(XRecordEntry::string(102, "}")));
        if let (Some(start), Some(end)) = (start, end) {
            self.entries.splice(start..=end, replacement);
        } else {
            self.entries.extend(replacement);
        }
        self.synchronize_object_references();
        self.entries_complete = true;
    }

    /// Decode Autodesk mesh UVW triples (codes 43, 44, 45).
    pub fn mesh_texture_coordinates(&self) -> Vec<Vector3> {
        let mut result = Vec::new();
        let mut values = self.entries.iter();
        while let Some(entry) = values.next() {
            if entry.code != 43 {
                continue;
            }
            let Some(u) = entry.value.as_double() else {
                continue;
            };
            let Some(v_entry) = values.next() else {
                break;
            };
            let Some(w_entry) = values.next() else {
                break;
            };
            if v_entry.code == 44 && w_entry.code == 45 {
                if let (Some(v), Some(w)) = (v_entry.value.as_double(), w_entry.value.as_double()) {
                    result.push(Vector3::new(u, v, w));
                }
            }
        }
        result
    }

    /// Replace Autodesk mesh UVW triples without disturbing unrelated entries.
    pub fn set_mesh_texture_coordinates(&mut self, coordinates: &[Vector3]) {
        self.entries.retain(|entry| !matches!(entry.code, 43..=45));
        for coordinate in coordinates {
            self.entries.push(XRecordEntry::double(43, coordinate.x));
            self.entries.push(XRecordEntry::double(44, coordinate.y));
            self.entries.push(XRecordEntry::double(45, coordinate.z));
        }
        self.entries_complete = true;
    }

    /// Viewport annotation-scale object referenced by code 340.
    pub fn annotation_scale_handle(&self) -> Option<Handle> {
        self.entries
            .iter()
            .find(|entry| entry.code == 340)
            .and_then(|entry| entry.value.as_handle())
    }

    /// Set the viewport annotation-scale reference while retaining all other
    /// application data.
    pub fn set_annotation_scale_handle(&mut self, handle: Handle) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.code == 340) {
            entry.value = XRecordValue::Handle(handle);
        } else {
            self.entries.push(XRecordEntry::handle(340, handle));
        }
        self.synchronize_object_references();
        self.entries_complete = true;
    }

    /// Decode viewport-specific layer override pairs. The viewport handle is
    /// code 335; `value_code` is 440 (alpha), 420 (color), 343 (linetype) or
    /// 91 (lineweight).
    pub fn layer_viewport_overrides(&self, value_code: i32) -> Vec<(Handle, XRecordValue)> {
        let mut result = Vec::new();
        let mut viewport = None;
        for entry in &self.entries {
            if entry.code == 335 {
                viewport = entry.value.as_handle();
            } else if entry.code == value_code {
                if let Some(handle) = viewport.take() {
                    result.push((handle, entry.value.clone()));
                }
            }
        }
        result
    }

    /// Set one viewport-specific layer override in its Autodesk section.
    pub fn set_layer_viewport_override(
        &mut self,
        section_name: &str,
        value_code: i32,
        viewport: Handle,
        value: XRecordValue,
    ) {
        let mut updated = false;
        let mut index = 0usize;
        while index < self.entries.len() {
            if self.entries[index].code == 335
                && self.entries[index].value.as_handle() == Some(viewport)
            {
                if let Some(value_entry) = self.entries[index + 1..]
                    .iter_mut()
                    .take_while(|entry| {
                        entry.code != 335
                            && !matches!(
                                &entry.value,
                                XRecordValue::String(value)
                                    if entry.code == 102 && value.ends_with('}')
                            )
                    })
                    .find(|entry| entry.code == value_code)
                {
                    value_entry.value = value.clone();
                    updated = true;
                    break;
                }
            }
            index += 1;
        }
        if !updated {
            self.entries
                .push(XRecordEntry::string(102, format!("{{{section_name}")));
            self.entries.push(XRecordEntry::handle(335, viewport));
            self.entries.push(XRecordEntry::new(value_code, value));
            self.entries.push(XRecordEntry::string(102, "}"));
        }
        self.entries_complete = true;
        self.synchronize_object_references();
    }

    /// Rebuild the DWG object-id handle vector from 330-369 entries while
    /// retaining decoded reference kinds where they still line up.
    pub fn synchronize_object_references(&mut self) {
        let previous = self.object_references.clone();
        self.object_references = self
            .entries
            .iter()
            .filter(|entry| (330..=369).contains(&entry.code))
            .filter_map(|entry| entry.value.as_handle().map(|handle| (entry.code, handle)))
            .enumerate()
            .map(|(index, (code, handle))| ProxyObjectReference {
                handle,
                kind: previous
                    .get(index)
                    .map(|reference| reference.kind)
                    .unwrap_or(match code {
                        330..=339 => ProxyReferenceKind::SoftPointer,
                        340..=349 => ProxyReferenceKind::HardPointer,
                        350..=359 => ProxyReferenceKind::SoftOwnership,
                        360..=369 => ProxyReferenceKind::HardOwnership,
                        _ => ProxyReferenceKind::Undefined,
                    }),
            })
            .collect();
        self.preserve_object_reference_stream = false;
    }
}

impl Default for XRecord {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xrecord_creation() {
        let xrecord = XRecord::new();
        assert!(xrecord.is_empty());
        assert_eq!(xrecord.cloning_flags, DictionaryCloningFlags::NotApplicable);
    }

    #[test]
    fn test_xrecord_named() {
        let xrecord = XRecord::named("MyRecord");
        assert_eq!(xrecord.name, "MyRecord");
    }

    #[test]
    fn test_xrecord_add_entries() {
        let mut xrecord = XRecord::new();
        xrecord.add_string(1, "Test");
        xrecord.add_double(40, 3.14);
        xrecord.add_int32(90, 42);
        xrecord.add_bool(290, true);

        assert_eq!(xrecord.len(), 4);
    }

    #[test]
    fn test_xrecord_get_values() {
        let mut xrecord = XRecord::new();
        xrecord.add_string(1, "Hello");
        xrecord.add_double(40, 2.5);
        xrecord.add_int32(90, 100);

        assert_eq!(xrecord.get_string(1), Some("Hello"));
        assert_eq!(xrecord.get_double(40), Some(2.5));
        assert_eq!(xrecord.get_i32(90), Some(100));
        assert_eq!(xrecord.get_string(999), None);
    }

    #[test]
    fn test_xrecord_get_by_code() {
        let mut xrecord = XRecord::new();
        xrecord.add_string(1, "First");
        xrecord.add_string(1, "Second");
        xrecord.add_string(2, "Other");

        let code_1 = xrecord.get_by_code(1);
        assert_eq!(code_1.len(), 2);

        let strings = xrecord.get_strings(1);
        assert_eq!(strings, vec!["First", "Second"]);
    }

    #[test]
    fn test_xrecord_remove_by_code() {
        let mut xrecord = XRecord::new();
        xrecord.add_string(1, "Keep");
        xrecord.add_double(40, 1.0);
        xrecord.add_double(40, 2.0);

        xrecord.remove_by_code(40);
        assert_eq!(xrecord.len(), 1);
        assert_eq!(xrecord.get_string(1), Some("Keep"));
    }

    #[test]
    fn test_xrecord_entry_types() {
        let entry = XRecordEntry::string(1, "test");
        assert_eq!(entry.value_type(), XRecordValueType::String);

        let entry = XRecordEntry::double(40, 1.0);
        assert_eq!(entry.value_type(), XRecordValueType::Double);

        let entry = XRecordEntry::int32(90, 42);
        assert_eq!(entry.value_type(), XRecordValueType::Int32);

        let entry = XRecordEntry::handle(330, Handle::new(100));
        assert_eq!(entry.value_type(), XRecordValueType::ObjectId);
        assert!(entry.has_linked_object());
    }

    #[test]
    fn test_xrecord_point3d() {
        let mut xrecord = XRecord::new();
        xrecord.add_point3d(10, 1.0, 2.0, 3.0);

        let entry = xrecord.get(0).unwrap();
        assert_eq!(entry.value.as_point3d(), Some((1.0, 2.0, 3.0)));
    }

    #[test]
    fn test_xrecord_get_references() {
        let mut xrecord = XRecord::new();
        xrecord.add_handle(330, Handle::new(100));
        xrecord.add_string(1, "text");
        xrecord.add_handle(340, Handle::new(200));

        let refs = xrecord.get_references();
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&Handle::new(100)));
        assert!(refs.contains(&Handle::new(200)));
    }

    #[test]
    fn test_cloning_flags() {
        assert_eq!(
            DictionaryCloningFlags::from_value(0),
            DictionaryCloningFlags::NotApplicable
        );
        assert_eq!(
            DictionaryCloningFlags::from_value(1),
            DictionaryCloningFlags::KeepExisting
        );
        assert_eq!(
            DictionaryCloningFlags::from_value(2),
            DictionaryCloningFlags::UseClone
        );
        assert_eq!(DictionaryCloningFlags::KeepExisting.to_value(), 1);
    }

    #[test]
    fn test_value_type_from_code() {
        assert_eq!(XRecordValueType::from_code(1), XRecordValueType::String);
        assert_eq!(XRecordValueType::from_code(10), XRecordValueType::Point3D);
        assert_eq!(XRecordValueType::from_code(40), XRecordValueType::Double);
        assert_eq!(XRecordValueType::from_code(90), XRecordValueType::Int32);
        assert_eq!(XRecordValueType::from_code(290), XRecordValueType::Bool);
        assert_eq!(XRecordValueType::from_code(330), XRecordValueType::ObjectId);
    }
}
