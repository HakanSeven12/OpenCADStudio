//! Solid3D (3DSOLID) entity implementation.
//!
//! The Solid3D entity represents a 3D solid body with geometry stored
//! in ACIS/SAT format. It also provides wireframe and silhouette data
//! for visualization without parsing the full ACIS data.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

// ============================================================================
// Wire Type
// ============================================================================

/// Wire type for wireframe display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum WireType {
    /// Unknown wire type.
    #[default]
    Unknown = 0,
    /// Silhouette edge.
    Silhouette = 1,
    /// Visible edge.
    VisibleEdge = 2,
    /// Hidden edge.
    HiddenEdge = 3,
    /// Isoparametric curve.
    Isoline = 4,
}

impl From<u8> for WireType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Silhouette,
            2 => Self::VisibleEdge,
            3 => Self::HiddenEdge,
            4 => Self::Isoline,
            _ => Self::Unknown,
        }
    }
}

// ============================================================================
// Wire Data
// ============================================================================

/// Wireframe edge data for visualization.
///
/// Provides display geometry without requiring full ACIS parsing.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Wire {
    /// ACIS entity index this wire belongs to.
    pub acis_index: i32,
    /// Wire type.
    pub wire_type: WireType,
    /// Selection marker for picking.
    pub selection_marker: i32,
    /// Wire color.
    pub color: Color,
    /// Points defining the wire path.
    pub points: Vec<Vector3>,

    // Transform data (when present)
    /// Whether transform data is present.
    pub has_transform: bool,
    /// Transform has rotation.
    pub has_rotation: bool,
    /// Transform has shear.
    pub has_shear: bool,
    /// Transform has reflection.
    pub has_reflection: bool,
    /// Transform scale factors.
    pub scale: Vector3,
    /// Transform translation.
    pub translation: Vector3,
    /// X axis of transform.
    pub x_axis: Vector3,
    /// Y axis of transform.
    pub y_axis: Vector3,
    /// Z axis of transform.
    pub z_axis: Vector3,
}

impl Wire {
    /// Creates a new wire with default transform.
    pub fn new() -> Self {
        Self {
            acis_index: 0,
            wire_type: WireType::Unknown,
            selection_marker: 0,
            color: Color::ByLayer,
            points: Vec::new(),
            has_transform: false,
            has_rotation: false,
            has_shear: false,
            has_reflection: false,
            scale: Vector3::new(1.0, 1.0, 1.0),
            translation: Vector3::ZERO,
            x_axis: Vector3::UNIT_X,
            y_axis: Vector3::UNIT_Y,
            z_axis: Vector3::UNIT_Z,
        }
    }

    /// Creates a wire from a list of points.
    pub fn from_points(points: Vec<Vector3>) -> Self {
        let mut wire = Self::new();
        wire.points = points;
        wire
    }

    /// Adds a point to the wire.
    pub fn add_point(&mut self, point: Vector3) {
        self.points.push(point);
    }

    /// Returns the number of points.
    pub fn point_count(&self) -> usize {
        self.points.len()
    }

    /// Returns the bounding box of the wire points.
    pub fn bounding_box(&self) -> Option<BoundingBox3D> {
        if self.points.is_empty() {
            return None;
        }

        let first = self.points[0];
        let mut min = first;
        let mut max = first;

        for pt in &self.points[1..] {
            min.x = min.x.min(pt.x);
            min.y = min.y.min(pt.y);
            min.z = min.z.min(pt.z);
            max.x = max.x.max(pt.x);
            max.y = max.y.max(pt.y);
            max.z = max.z.max(pt.z);
        }

        Some(BoundingBox3D::new(min, max))
    }
}

impl Default for Wire {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Silhouette
// ============================================================================

/// Silhouette data for a specific viewport.
///
/// Provides pre-computed silhouette curves for different view directions.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Silhouette {
    /// Viewport identifier.
    pub viewport_id: i64,
    /// Viewport view direction.
    pub view_direction: Vector3,
    /// Viewport up vector.
    pub up_vector: Vector3,
    /// Viewport target point.
    pub target: Vector3,
    /// Whether viewport uses perspective.
    pub is_perspective: bool,
    /// Whether the source record carried the silhouette-wire list gate.
    ///
    /// This can be true while `wires` is empty; preserving that distinction is
    /// required for an exact COMMON_3DSOLID round-trip.
    #[cfg_attr(feature = "serde", serde(default))]
    pub has_wires: bool,
    /// Silhouette wires for this viewport.
    pub wires: Vec<Wire>,
}

impl Silhouette {
    /// Creates a new silhouette for the given viewport.
    pub fn new(viewport_id: i64) -> Self {
        Self {
            viewport_id,
            view_direction: Vector3::new(0.0, 0.0, 1.0),
            up_vector: Vector3::new(0.0, 1.0, 0.0),
            target: Vector3::ZERO,
            is_perspective: false,
            has_wires: false,
            wires: Vec::new(),
        }
    }

    /// Creates a silhouette with view direction.
    pub fn with_view(viewport_id: i64, view_direction: Vector3, up_vector: Vector3) -> Self {
        Self {
            viewport_id,
            view_direction: view_direction.normalize(),
            up_vector: up_vector.normalize(),
            target: Vector3::ZERO,
            is_perspective: false,
            has_wires: false,
            wires: Vec::new(),
        }
    }

    /// Adds a wire to the silhouette.
    pub fn add_wire(&mut self, wire: Wire) {
        self.has_wires = true;
        self.wires.push(wire);
    }

    /// Returns the number of wires.
    pub fn wire_count(&self) -> usize {
        self.wires.len()
    }
}

impl Default for Silhouette {
    fn default() -> Self {
        Self::new(0)
    }
}

// ============================================================================
// ACIS Data
// ============================================================================

/// ACIS/SAT data format version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(u8)]
pub enum AcisVersion {
    /// Version 1: SAT data with character encoding.
    #[default]
    Version1 = 1,
    /// Version 2: Text SAT or binary SAB.
    Version2 = 2,
}

impl From<u8> for AcisVersion {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::Version1,
            2 => Self::Version2,
            _ => Self::Version1,
        }
    }
}

/// Container for ACIS/SAT solid data.
///
/// The ACIS data represents the actual 3D solid geometry in Spatial
/// Corporation's proprietary format.
/// Modeler-geometry revision block (`COMMON_3DSOLID`, R2013+).
///
/// Every 3DSOLID/REGION/BODY in an R2013+ (AC1027+) DWG carries a revision
/// block identifying the ACIS/ShapeManager modeler revision. It must be
/// preserved on write; omitting it corrupts the entity stream and prevents the
/// file from opening in AutoCAD/TrueView/BricsCAD.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AcisRevision {
    /// Whether a revision GUID is present.
    pub has_guid: bool,
    /// GUID major component (BitLong).
    pub major: u32,
    /// GUID minor component 1 (BitShort).
    pub minor1: i16,
    /// GUID minor component 2 (BitShort).
    pub minor2: i16,
    /// The 8 raw GUID bytes.
    pub bytes: [u8; 8],
    /// Trailing end marker (BitLong).
    pub end_marker: u32,
}

/// Material binding stored in the shared modeler-geometry body.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AcisMaterial {
    pub array_index: i32,
    pub absolute_reference: i32,
    pub material_handle: Option<Handle>,
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AcisData {
    /// Version of the modeler format.
    pub version: AcisVersion,
    /// Raw SAT text data (for Version 1 or text Version 2).
    pub sat_data: String,
    /// Binary SAB data (for binary Version 2).
    pub sab_data: Vec<u8>,
    /// Whether this is binary SAB format.
    pub is_binary: bool,
    /// R2013+ modeler-geometry revision block (preserved for round-trip).
    pub revision: AcisRevision,
    /// R2007+ material bindings attached to binary modeler geometry.
    pub materials: Vec<AcisMaterial>,
    /// Whether the source record carried the optional wireframe cache.
    #[cfg_attr(feature = "serde", serde(default))]
    pub wireframe_data_present: bool,
    /// Whether the wireframe cache carried its reference point.
    #[cfg_attr(feature = "serde", serde(default))]
    pub wireframe_point_present: bool,
    /// Whether the wireframe cache carried the isoline/wire list.
    #[cfg_attr(feature = "serde", serde(default))]
    pub wireframe_isoline_present: bool,
    /// Secondary COMMON_3DSOLID empty flag, preserved independently from the
    /// leading modeler-data empty flag.
    #[cfg_attr(feature = "serde", serde(default))]
    pub acis_empty_bit: bool,
    /// Optional secondary modeler stream carried after the wireframe cache.
    ///
    /// AutoCAD uses this for construction geometry on surfaces. It is a
    /// separate SAT/SAB stream, not a raw copy of the containing DWG object.
    #[cfg_attr(feature = "serde", serde(default))]
    pub extra_acis_data: Option<Box<AcisData>>,
    /// ISOLINES display setting from the DWG wireframe section (preserved
    /// for round-trip; 0 when the entity carried no wireframe data).
    #[cfg_attr(feature = "serde", serde(default))]
    pub wireframe_isolines: i32,
}

impl AcisData {
    /// Creates empty ACIS data.
    pub fn new() -> Self {
        Self {
            version: AcisVersion::Version1,
            sat_data: String::new(),
            sab_data: Vec::new(),
            is_binary: false,
            revision: AcisRevision::default(),
            materials: Vec::new(),
            wireframe_data_present: false,
            wireframe_point_present: false,
            wireframe_isoline_present: false,
            acis_empty_bit: false,
            extra_acis_data: None,
            wireframe_isolines: 0,
        }
    }

    /// Creates ACIS data from SAT text.
    ///
    /// The terminator line (`End-of-ACIS-data` / `End-of-ASM-data`) is
    /// stripped if present — the DWG/DXF writers add it back as needed.
    pub fn from_sat(sat: &str) -> Self {
        Self {
            version: AcisVersion::Version1,
            sat_data: Self::strip_sat_terminator(sat),
            sab_data: Vec::new(),
            is_binary: false,
            revision: AcisRevision::default(),
            materials: Vec::new(),
            wireframe_data_present: false,
            wireframe_point_present: false,
            wireframe_isoline_present: false,
            acis_empty_bit: false,
            extra_acis_data: None,
            wireframe_isolines: 0,
        }
    }

    /// Creates ACIS data from binary SAB.
    pub fn from_sab(sab: Vec<u8>) -> Self {
        Self {
            version: AcisVersion::Version2,
            sat_data: String::new(),
            sab_data: sab,
            is_binary: true,
            revision: AcisRevision::default(),
            materials: Vec::new(),
            wireframe_data_present: false,
            wireframe_point_present: false,
            wireframe_isoline_present: false,
            acis_empty_bit: false,
            extra_acis_data: None,
            wireframe_isolines: 0,
        }
    }

    /// Returns true if this contains valid data.
    pub fn has_data(&self) -> bool {
        !self.sat_data.is_empty() || !self.sab_data.is_empty()
    }

    /// Returns true when this ACIS data yields a SAB blob for the AcDs section
    /// on write (binary SAB present, or SAT text convertible to SAB). Matches
    /// the push condition in the DWG writer's `queue_sab_entry`, so the R2013+
    /// `has_ds_data` entity flag stays in lockstep with the blobs actually
    /// emitted — a mismatch would orphan a blob or a solid.
    pub fn contributes_sab(&self) -> bool {
        (self.is_binary && !self.sab_data.is_empty()) || !self.sat_data.is_empty()
    }

    /// Strip the `End-of-ACIS-data` / `End-of-ASM-data` terminator line
    /// (and any trailing blank lines) from raw SAT text.
    ///
    /// Internally, `sat_data` never contains the terminator — the writers
    /// append it at serialisation time.  This keeps the representation
    /// consistent regardless of the data source (DXF reader, DWG reader,
    /// user API).
    pub fn strip_sat_terminator(sat: &str) -> String {
        let mut result = String::with_capacity(sat.len());
        for line in sat.lines() {
            if line.starts_with("End-of-ACIS-data") || line.starts_with("End-of-ASM-data") {
                break;
            }
            result.push_str(line);
            result.push('\n');
        }
        result
    }

    /// Returns the data size in bytes.
    pub fn size(&self) -> usize {
        if self.is_binary {
            self.sab_data.len()
        } else {
            self.sat_data.len()
        }
    }

    /// Parses the SAT text data into a structured [`SatDocument`].
    ///
    /// Returns `None` if the data is empty or binary (SAB).
    pub fn parse_sat(&self) -> Option<crate::entities::acis::SatDocument> {
        if self.is_binary || self.sat_data.is_empty() {
            return None;
        }
        crate::entities::acis::SatDocument::parse(&self.sat_data).ok()
    }

    /// Creates ACIS data from a [`SatDocument`].
    pub fn from_sat_document(doc: &crate::entities::acis::SatDocument) -> Self {
        Self::from_sat(&doc.to_sat_string())
    }

    /// Parse the ACIS payload into a [`SatDocument`], decoding binary SAB via
    /// the SAB reader. `None` when the data is empty or cannot be parsed.
    /// Unlike [`parse_sat`](Self::parse_sat), this also handles binary data.
    pub fn parse(&self) -> Option<crate::entities::acis::SatDocument> {
        if self.is_binary {
            if self.sab_data.is_empty() {
                return None;
            }
            crate::entities::acis::SabReader::read(&self.sab_data).ok()
        } else {
            if self.sat_data.is_empty() {
                return None;
            }
            crate::entities::acis::SatDocument::parse(&self.sat_data).ok()
        }
    }

    /// The body's placement origin in world space: the translation of the
    /// ACIS `transform` record. `None` when the data has no transform record
    /// (geometry baked at world coordinates) or can't be parsed. A 3D solid
    /// carries no insertion point of its own, so this placement is its natural
    /// reference point.
    pub fn placement_origin(&self) -> Option<Vector3> {
        let doc = self.parse()?;
        if !doc.records.iter().any(|r| r.entity_type == "transform") {
            return None;
        }
        let (_m, t, _s) = doc.placement();
        Some(Vector3::new(t[0], t[1], t[2]))
    }

    /// The centre of the body's vertex-point bounding box, in world space
    /// (body placement applied). `None` when the data can't be parsed or
    /// carries no `point` records. Approximates the geometric centre well for
    /// faceted bodies; used as the reference point when the file stores no
    /// wireframe anchor — bodies baked at world coordinates would otherwise
    /// report (0,0,0).
    pub fn geometry_centre(&self) -> Option<Vector3> {
        let doc = self.parse()?;
        let (m, tr, s) = doc.placement();
        let mut min = [f64::MAX; 3];
        let mut max = [f64::MIN; 3];
        let mut count = 0usize;
        for rec in &doc.records {
            if rec.entity_type != "point" {
                continue;
            }
            // SAT text stores a point as three trailing floats; SAB stores it
            // as a coordinate tag. Both forms are exposed by SatToken's typed
            // accessors while retaining the original SAB bytes for writing.
            let mut p: Option<[f64; 3]> = None;
            let mut floats: Vec<f64> = Vec::with_capacity(4);
            for t in &rec.tokens {
                if let Some((components, len)) = t.coordinate_components() {
                    if len == 3 {
                        p = Some(components);
                    }
                } else if let Some(value) = t.as_float() {
                    floats.push(value);
                }
            }
            let Some([x, y, z]) = p.or_else(|| {
                (floats.len() >= 3).then(|| {
                    let n = floats.len();
                    [floats[n - 3], floats[n - 2], floats[n - 1]]
                })
            }) else {
                continue;
            };
            // ACIS row-vector convention: world = scale·(p·M) + T.
            let w = [
                s * (x * m[0][0] + y * m[1][0] + z * m[2][0]) + tr[0],
                s * (x * m[0][1] + y * m[1][1] + z * m[2][1]) + tr[1],
                s * (x * m[0][2] + y * m[1][2] + z * m[2][2]) + tr[2],
            ];
            for k in 0..3 {
                min[k] = min[k].min(w[k]);
                max[k] = max[k].max(w[k]);
            }
            count += 1;
        }
        if count == 0 {
            return None;
        }
        Some(Vector3::new(
            (min[0] + max[0]) * 0.5,
            (min[1] + max[1]) * 0.5,
            (min[2] + max[2]) * 0.5,
        ))
    }
}

impl AcisData {
    /// Apply the symmetric SAT/DXF character cipher.
    ///
    /// Every printable ASCII character except space is mapped to
    /// `(159 - c)`.  Spaces, newlines, and non-ASCII bytes pass through
    /// unchanged.  The cipher is its own inverse: `cipher(cipher(x)) == x`.
    fn sat_cipher(text: &str) -> String {
        text.chars()
            .map(|c| {
                let b = c as u32;
                if b >= 0x21 && b <= 0x7E {
                    // Safety: 159 - b is in 0x21..=0x7E, always valid.
                    char::from_u32(159 - b).unwrap_or(c)
                } else {
                    c
                }
            })
            .collect()
    }

    /// Encode plaintext SAT for DXF storage (Version 1 cipher).
    ///
    /// AutoCAD DXF files (R2004 / AC1018 and later) store ACIS SAT data
    /// using a simple symmetric character cipher: every printable ASCII
    /// character except space is mapped to `(159 - c)`.  Spaces, newlines,
    /// and non-ASCII bytes are passed through unchanged.
    ///
    /// After applying the cipher, a protective space is inserted after any
    /// `^` (0x5E) that would otherwise be followed by a character in the
    /// 0x40–0x5F range.  In DXF, the two-character sequence `^X` (where X
    /// is in 0x40–0x5F) is interpreted as a control character and would
    /// corrupt the data stream.  The plaintext letter `A` (0x41) encodes
    /// to `^` (0x5E), so sequences like `AC` become `^\` which DXF readers
    /// would mis-interpret as a File Separator control code.
    pub fn encode_sat(text: &str) -> String {
        let ciphered = Self::sat_cipher(text);
        let bytes = ciphered.as_bytes();
        let mut result = String::with_capacity(ciphered.len() + 16);
        for i in 0..bytes.len() {
            result.push(bytes[i] as char);
            // Insert a protective space after '^' when the next character
            // falls in the DXF control-character trigger range 0x40-0x5F.
            if bytes[i] == 0x5E {
                if let Some(&next) = bytes.get(i + 1) {
                    if (0x40..=0x5F).contains(&next) {
                        result.push(' ');
                    }
                }
            }
        }
        result
    }

    /// Decode DXF-encoded SAT text (Version 1 cipher).
    ///
    /// Strips protective spaces that were inserted after `^` to prevent
    /// DXF control-character interpretation, then applies the cipher.
    pub fn decode_sat(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut cleaned = String::with_capacity(text.len());
        let mut i = 0;
        while i < bytes.len() {
            cleaned.push(bytes[i] as char);
            // If we see '^' followed by a space and then a char in
            // 0x40-0x5F, the space is a protective insertion – skip it.
            if bytes[i] == 0x5E && i + 2 < bytes.len() && bytes[i + 1] == 0x20 {
                let after = bytes[i + 2];
                if (0x40..=0x5F).contains(&after) {
                    i += 1; // skip the protective space
                }
            }
            i += 1;
        }
        Self::sat_cipher(&cleaned)
    }
}

impl Default for AcisData {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Solid3D Entity
// ============================================================================

/// 3D Solid (3DSOLID) entity.
///
/// Represents a 3D solid body with geometry stored in ACIS/SAT format.
/// The wireframe and silhouette data provide visualization hints without
/// requiring full ACIS parsing.
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::Solid3D;
/// use acadrust::types::Vector3;
///
/// // Create a 3D solid (typically from DXF/DWG import)
/// let mut solid = Solid3D::new();
///
/// // Set ACIS data (from file import)
/// solid.set_sat_data("ACIS data here...");
///
/// // Access wireframe for visualization
/// for wire in &solid.wires {
///     for point in &wire.points {
///         // Draw wire segment
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Solid3D {
    /// Common entity data.
    pub common: EntityCommon,
    /// Unique identifier within the file.
    pub uid: String,
    /// Point of reference (typically origin).
    pub point_of_reference: Vector3,
    /// ACIS/SAT solid data.
    pub acis_data: AcisData,
    /// Wireframe edges for visualization.
    pub wires: Vec<Wire>,
    /// Silhouette data for viewports.
    pub silhouettes: Vec<Silhouette>,
    /// Handle to edit history object (R2007+).
    pub history_handle: Option<Handle>,
}

impl Solid3D {
    /// Creates a new empty 3D solid.
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            uid: String::new(),
            point_of_reference: Vector3::ZERO,
            acis_data: AcisData::new(),
            wires: Vec::new(),
            silhouettes: Vec::new(),
            history_handle: None,
        }
    }

    /// Creates a 3D solid from SAT text data.
    pub fn from_sat(sat: &str) -> Self {
        let mut solid = Self::new();
        solid.acis_data = AcisData::from_sat(sat);
        solid
    }

    /// Creates a 3D solid from binary SAB data.
    pub fn from_sab(sab: Vec<u8>) -> Self {
        let mut solid = Self::new();
        solid.acis_data = AcisData::from_sab(sab);
        solid
    }

    /// Sets the SAT data.
    pub fn set_sat_data(&mut self, sat: &str) {
        self.acis_data = AcisData::from_sat(sat);
    }

    /// Sets the SAB data.
    pub fn set_sab_data(&mut self, sab: Vec<u8>) {
        self.acis_data = AcisData::from_sab(sab);
    }

    /// Returns true if this solid has valid ACIS data.
    pub fn has_acis_data(&self) -> bool {
        self.acis_data.has_data()
    }

    /// Parses the raw SAT text data into a structured [`SatDocument`].
    ///
    /// Returns `None` if the ACIS data is empty or binary (SAB).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let solid = Solid3D::from_sat("700 0 1 0\n...");
    /// if let Some(doc) = solid.parse_sat() {
    ///     for face in doc.faces() {
    ///         println!("Face sense: {:?}", face.sense());
    ///     }
    /// }
    /// ```
    pub fn parse_sat(&self) -> Option<crate::entities::acis::SatDocument> {
        if self.acis_data.is_binary || self.acis_data.sat_data.is_empty() {
            return None;
        }
        crate::entities::acis::SatDocument::parse(&self.acis_data.sat_data).ok()
    }

    /// Generates SAT text from a [`SatDocument`] and stores it in this entity.
    ///
    /// This replaces any existing ACIS data with text SAT format data.
    pub fn set_sat_document(&mut self, doc: &crate::entities::acis::SatDocument) {
        self.acis_data = AcisData::from_sat(&doc.to_sat_string());
    }

    /// Returns the ACIS data size.
    pub fn acis_size(&self) -> usize {
        self.acis_data.size()
    }

    /// Adds a wireframe edge.
    pub fn add_wire(&mut self, wire: Wire) {
        self.wires.push(wire);
    }

    /// Creates and adds a simple wire from points.
    pub fn add_wire_from_points(&mut self, points: Vec<Vector3>) -> &mut Wire {
        self.wires.push(Wire::from_points(points));
        self.wires.last_mut().unwrap()
    }

    /// Returns the number of wireframe edges.
    pub fn wire_count(&self) -> usize {
        self.wires.len()
    }

    /// Adds silhouette data.
    pub fn add_silhouette(&mut self, silhouette: Silhouette) {
        self.silhouettes.push(silhouette);
    }

    /// Returns silhouette for a viewport.
    pub fn silhouette_for_viewport(&self, viewport_id: i64) -> Option<&Silhouette> {
        self.silhouettes
            .iter()
            .find(|s| s.viewport_id == viewport_id)
    }

    /// Clears all visualization data (wires and silhouettes).
    pub fn clear_visualization(&mut self) {
        self.wires.clear();
        self.silhouettes.clear();
    }

    /// Calculates bounding box from wireframe data.
    fn wireframe_bounding_box(&self) -> BoundingBox3D {
        if self.wires.is_empty() {
            return BoundingBox3D::default();
        }

        let mut min = Vector3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut max = Vector3::new(f64::MIN, f64::MIN, f64::MIN);
        let mut has_points = false;

        for wire in &self.wires {
            for pt in &wire.points {
                has_points = true;
                min.x = min.x.min(pt.x);
                min.y = min.y.min(pt.y);
                min.z = min.z.min(pt.z);
                max.x = max.x.max(pt.x);
                max.y = max.y.max(pt.y);
                max.z = max.z.max(pt.z);
            }
        }

        if has_points {
            BoundingBox3D::new(min, max)
        } else {
            BoundingBox3D::default()
        }
    }
}

impl Default for Solid3D {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Solid3D {
    fn handle(&self) -> Handle {
        self.common.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.common.handle = handle;
    }

    fn layer(&self) -> &str {
        &self.common.layer
    }

    fn set_layer(&mut self, layer: String) {
        self.common.layer = layer;
    }

    fn color(&self) -> Color {
        self.common.color
    }

    fn set_color(&mut self, color: Color) {
        self.common.color = color;
    }

    fn line_weight(&self) -> LineWeight {
        self.common.line_weight
    }

    fn set_line_weight(&mut self, line_weight: LineWeight) {
        self.common.line_weight = line_weight;
    }

    fn transparency(&self) -> Transparency {
        self.common.transparency
    }

    fn set_transparency(&mut self, transparency: Transparency) {
        self.common.transparency = transparency;
    }

    fn is_invisible(&self) -> bool {
        self.common.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }

    fn bounding_box(&self) -> BoundingBox3D {
        self.wireframe_bounding_box()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_solid3d(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "3DSOLID"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_solid3d(self, transform);
    }
}

// ============================================================================
// Region Entity (similar structure, 2D enclosed area)
// ============================================================================

/// Region entity.
///
/// Represents a 2D enclosed area stored in ACIS format.
/// Similar to Solid3D but for 2D geometry.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Region {
    /// Common entity data.
    pub common: EntityCommon,
    /// Unique identifier within the file.
    pub uid: String,
    /// Point of reference.
    pub point_of_reference: Vector3,
    /// ACIS/SAT region data.
    pub acis_data: AcisData,
    /// Wireframe edges for visualization.
    pub wires: Vec<Wire>,
    /// Silhouette data for viewports.
    pub silhouettes: Vec<Silhouette>,
    /// Handle of the associated history object (H 350, R2007+); NULL when the
    /// region records no construction history.
    pub history_handle: Option<Handle>,
}

impl Region {
    /// Creates a new empty region.
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            uid: String::new(),
            point_of_reference: Vector3::ZERO,
            acis_data: AcisData::new(),
            wires: Vec::new(),
            silhouettes: Vec::new(),
            history_handle: None,
        }
    }

    /// Creates a region from SAT text data.
    pub fn from_sat(sat: &str) -> Self {
        let mut region = Self::new();
        region.acis_data = AcisData::from_sat(sat);
        region
    }

    /// Returns true if this region has valid ACIS data.
    pub fn has_acis_data(&self) -> bool {
        self.acis_data.has_data()
    }

    /// Parses the raw SAT text data into a structured [`SatDocument`].
    ///
    /// Returns `None` if the ACIS data is empty or binary (SAB).
    pub fn parse_sat(&self) -> Option<crate::entities::acis::SatDocument> {
        if self.acis_data.is_binary || self.acis_data.sat_data.is_empty() {
            return None;
        }
        crate::entities::acis::SatDocument::parse(&self.acis_data.sat_data).ok()
    }

    /// Generates SAT text from a [`SatDocument`] and stores it in this entity.
    pub fn set_sat_document(&mut self, doc: &crate::entities::acis::SatDocument) {
        self.acis_data = AcisData::from_sat(&doc.to_sat_string());
    }

    /// Adds a wireframe edge.
    pub fn add_wire(&mut self, wire: Wire) {
        self.wires.push(wire);
    }
}

impl Default for Region {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Region {
    fn handle(&self) -> Handle {
        self.common.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.common.handle = handle;
    }

    fn layer(&self) -> &str {
        &self.common.layer
    }

    fn set_layer(&mut self, layer: String) {
        self.common.layer = layer;
    }

    fn color(&self) -> Color {
        self.common.color
    }

    fn set_color(&mut self, color: Color) {
        self.common.color = color;
    }

    fn line_weight(&self) -> LineWeight {
        self.common.line_weight
    }

    fn set_line_weight(&mut self, line_weight: LineWeight) {
        self.common.line_weight = line_weight;
    }

    fn transparency(&self) -> Transparency {
        self.common.transparency
    }

    fn set_transparency(&mut self, transparency: Transparency) {
        self.common.transparency = transparency;
    }

    fn is_invisible(&self) -> bool {
        self.common.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }

    fn bounding_box(&self) -> BoundingBox3D {
        if self.wires.is_empty() {
            return BoundingBox3D::default();
        }

        let mut min = Vector3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut max = Vector3::new(f64::MIN, f64::MIN, f64::MIN);

        for wire in &self.wires {
            for pt in &wire.points {
                min.x = min.x.min(pt.x);
                min.y = min.y.min(pt.y);
                min.z = min.z.min(pt.z);
                max.x = max.x.max(pt.x);
                max.y = max.y.max(pt.y);
                max.z = max.z.max(pt.z);
            }
        }

        BoundingBox3D::new(min, max)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_region(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "REGION"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_region(self, transform);
    }
}

// ============================================================================
// Body Entity (similar structure, 3D body)
// ============================================================================

/// Body entity.
///
/// Represents a 3D body stored in ACIS format.
/// Similar to Solid3D but a different entity type.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Body {
    /// Common entity data.
    pub common: EntityCommon,
    /// Unique identifier within the file.
    pub uid: String,
    /// Point of reference.
    pub point_of_reference: Vector3,
    /// ACIS/SAT body data.
    pub acis_data: AcisData,
    /// Wireframe edges for visualization.
    pub wires: Vec<Wire>,
    /// Silhouette data for viewports.
    pub silhouettes: Vec<Silhouette>,
    /// Handle of the associated history object (H 350, R2007+); NULL when the
    /// body records no construction history.
    pub history_handle: Option<Handle>,
}

impl Body {
    /// Creates a new empty body.
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            uid: String::new(),
            point_of_reference: Vector3::ZERO,
            acis_data: AcisData::new(),
            wires: Vec::new(),
            silhouettes: Vec::new(),
            history_handle: None,
        }
    }

    /// Creates a body from SAT text data.
    pub fn from_sat(sat: &str) -> Self {
        let mut body = Self::new();
        body.acis_data = AcisData::from_sat(sat);
        body
    }

    /// Returns true if this body has valid ACIS data.
    pub fn has_acis_data(&self) -> bool {
        self.acis_data.has_data()
    }

    /// Parses the raw SAT text data into a structured [`SatDocument`].
    ///
    /// Returns `None` if the ACIS data is empty or binary (SAB).
    pub fn parse_sat(&self) -> Option<crate::entities::acis::SatDocument> {
        if self.acis_data.is_binary || self.acis_data.sat_data.is_empty() {
            return None;
        }
        crate::entities::acis::SatDocument::parse(&self.acis_data.sat_data).ok()
    }

    /// Generates SAT text from a [`SatDocument`] and stores it in this entity.
    pub fn set_sat_document(&mut self, doc: &crate::entities::acis::SatDocument) {
        self.acis_data = AcisData::from_sat(&doc.to_sat_string());
    }

    /// Adds a wireframe edge.
    pub fn add_wire(&mut self, wire: Wire) {
        self.wires.push(wire);
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Body {
    fn handle(&self) -> Handle {
        self.common.handle
    }

    fn set_handle(&mut self, handle: Handle) {
        self.common.handle = handle;
    }

    fn layer(&self) -> &str {
        &self.common.layer
    }

    fn set_layer(&mut self, layer: String) {
        self.common.layer = layer;
    }

    fn color(&self) -> Color {
        self.common.color
    }

    fn set_color(&mut self, color: Color) {
        self.common.color = color;
    }

    fn line_weight(&self) -> LineWeight {
        self.common.line_weight
    }

    fn set_line_weight(&mut self, line_weight: LineWeight) {
        self.common.line_weight = line_weight;
    }

    fn transparency(&self) -> Transparency {
        self.common.transparency
    }

    fn set_transparency(&mut self, transparency: Transparency) {
        self.common.transparency = transparency;
    }

    fn is_invisible(&self) -> bool {
        self.common.invisible
    }

    fn set_invisible(&mut self, invisible: bool) {
        self.common.invisible = invisible;
    }

    fn bounding_box(&self) -> BoundingBox3D {
        if self.wires.is_empty() {
            return BoundingBox3D::default();
        }

        let mut min = Vector3::new(f64::MAX, f64::MAX, f64::MAX);
        let mut max = Vector3::new(f64::MIN, f64::MIN, f64::MIN);

        for wire in &self.wires {
            for pt in &wire.points {
                min.x = min.x.min(pt.x);
                min.y = min.y.min(pt.y);
                min.z = min.z.min(pt.z);
                max.x = max.x.max(pt.x);
                max.y = max.y.max(pt.y);
                max.z = max.z.max(pt.z);
            }
        }

        BoundingBox3D::new(min, max)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_body(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "BODY"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_body(self, transform);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_solid3d_creation() {
        let solid = Solid3D::new();
        assert!(solid.uid.is_empty());
        assert_eq!(solid.point_of_reference, Vector3::ZERO);
        assert!(!solid.has_acis_data());
    }

    #[test]
    fn test_solid3d_from_sat() {
        let sat = "400 0 1 0\n16 ASM-BODY 1.0 0\n";
        let solid = Solid3D::from_sat(sat);
        assert!(solid.has_acis_data());
        assert!(!solid.acis_data.is_binary);
        assert_eq!(solid.acis_data.sat_data, sat);
    }

    #[test]
    fn test_solid3d_from_sab() {
        let sab = b"ACIS BinaryFile".to_vec();
        let solid = Solid3D::from_sab(sab.clone());
        assert!(solid.has_acis_data());
        assert!(solid.acis_data.is_binary);
        assert_eq!(solid.acis_data.sab_data, sab);
    }

    #[test]
    fn test_geometry_centre_decodes_sab_coordinate_tokens() {
        use crate::entities::acis::{SabReader, SabWriter, SatDocument, SatToken};

        let mut doc = SatDocument::new_body();
        doc.add_point(0.0, 0.0, 0.0);
        doc.add_point(4.0, 8.0, 12.0);
        let sab = SabWriter::write(&doc);
        let parsed = SabReader::read(&sab).expect("parse SAB");
        assert!(matches!(
            parsed.records_of_type("point")[0].tokens[1],
            SatToken::Sab { tag: 0x13, .. }
        ));

        let acis = AcisData::from_sab(sab);
        assert_eq!(acis.geometry_centre(), Some(Vector3::new(2.0, 4.0, 6.0)));
    }

    #[test]
    fn test_wire_creation() {
        let wire = Wire::new();
        assert_eq!(wire.wire_type, WireType::Unknown);
        assert!(wire.points.is_empty());
        assert!(!wire.has_transform);
    }

    #[test]
    fn test_wire_from_points() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(1.0, 1.0, 0.0),
        ];
        let wire = Wire::from_points(points.clone());
        assert_eq!(wire.point_count(), 3);
        assert_eq!(wire.points, points);
    }

    #[test]
    fn test_wire_bounding_box() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 5.0, 3.0),
            Vector3::new(5.0, 10.0, 1.0),
        ];
        let wire = Wire::from_points(points);
        let bbox = wire.bounding_box().unwrap();
        assert_eq!(bbox.min, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(bbox.max, Vector3::new(10.0, 10.0, 3.0));
    }

    #[test]
    fn test_silhouette_creation() {
        let silhouette = Silhouette::new(42);
        assert_eq!(silhouette.viewport_id, 42);
        assert!(!silhouette.is_perspective);
        assert_eq!(silhouette.wire_count(), 0);
    }

    #[test]
    fn test_silhouette_with_view() {
        let silhouette =
            Silhouette::with_view(1, Vector3::new(0.0, 0.0, -1.0), Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(silhouette.viewport_id, 1);
        assert_eq!(silhouette.view_direction.z, -1.0);
    }

    #[test]
    fn test_solid3d_add_wire() {
        let mut solid = Solid3D::new();
        solid.add_wire(Wire::from_points(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 1.0, 1.0),
        ]));
        assert_eq!(solid.wire_count(), 1);
    }

    #[test]
    fn test_solid3d_translate() {
        let mut solid = Solid3D::new();
        solid.point_of_reference = Vector3::new(1.0, 2.0, 3.0);
        solid.add_wire(Wire::from_points(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ]));

        solid.translate(Vector3::new(10.0, 20.0, 30.0));

        assert_eq!(solid.point_of_reference, Vector3::new(11.0, 22.0, 33.0));
        assert_eq!(solid.wires[0].points[0], Vector3::new(10.0, 20.0, 30.0));
        assert_eq!(solid.wires[0].points[1], Vector3::new(11.0, 20.0, 30.0));
    }

    #[test]
    fn test_acis_data() {
        let mut data = AcisData::new();
        assert!(!data.has_data());
        assert_eq!(data.size(), 0);

        data.sat_data = "test".to_string();
        assert!(data.has_data());
        assert_eq!(data.size(), 4);
    }

    #[test]
    fn test_acis_version() {
        assert_eq!(AcisVersion::from(1), AcisVersion::Version1);
        assert_eq!(AcisVersion::from(2), AcisVersion::Version2);
        assert_eq!(AcisVersion::from(99), AcisVersion::Version1);
    }

    #[test]
    fn test_wire_type() {
        assert_eq!(WireType::from(1), WireType::Silhouette);
        assert_eq!(WireType::from(2), WireType::VisibleEdge);
        assert_eq!(WireType::from(3), WireType::HiddenEdge);
        assert_eq!(WireType::from(4), WireType::Isoline);
        assert_eq!(WireType::from(99), WireType::Unknown);
    }

    #[test]
    fn test_region_creation() {
        let region = Region::new();
        assert!(!region.has_acis_data());
        assert_eq!(region.entity_type(), "REGION");
    }

    #[test]
    fn test_body_creation() {
        let body = Body::new();
        assert!(!body.has_acis_data());
        assert_eq!(body.entity_type(), "BODY");
    }

    #[test]
    fn test_solid3d_silhouette_for_viewport() {
        let mut solid = Solid3D::new();
        solid.add_silhouette(Silhouette::new(1));
        solid.add_silhouette(Silhouette::new(2));

        assert!(solid.silhouette_for_viewport(1).is_some());
        assert!(solid.silhouette_for_viewport(2).is_some());
        assert!(solid.silhouette_for_viewport(3).is_none());
    }

    #[test]
    fn test_solid3d_clear_visualization() {
        let mut solid = Solid3D::new();
        solid.add_wire(Wire::new());
        solid.add_silhouette(Silhouette::new(1));

        solid.clear_visualization();

        assert_eq!(solid.wire_count(), 0);
        assert!(solid.silhouettes.is_empty());
    }

    #[test]
    fn test_solid3d_parse_sat() {
        let sat_text = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            asmheader $-1 -1 @12 700 7 0 0 @5 ACIS @3 7.0 @24 Thu Jan 01 00:00:00 2023 #\n\
            body $-1 -1 $-1 $2 $-1 $-1 #\n\
            lump $-1 -1 $-1 $-1 $3 $1 #\n\
            shell $-1 -1 $-1 $-1 $-1 $4 $-1 $2 #\n\
            face $-1 -1 $-1 $-1 $5 $3 $-1 $6 forward single #\n\
            loop $-1 -1 $-1 $-1 $-1 $4 #\n\
            plane-surface $-1 -1 $-1 0 0 5 0 0 1 1 0 0 forward_v I I I I #\n\
            End-of-ACIS-data\n";

        let solid = Solid3D::from_sat(sat_text);
        assert!(solid.has_acis_data());

        let doc = solid.parse_sat().unwrap();
        assert_eq!(doc.header.version, crate::entities::acis::SatVersion::V7_0);
        assert_eq!(doc.records.len(), 7);

        // Check body
        let bodies = doc.bodies();
        assert_eq!(bodies.len(), 1);
        assert_eq!(bodies[0].lump(), crate::entities::acis::SatPointer::new(2));

        // Check faces
        let faces = doc.faces();
        assert_eq!(faces.len(), 1);
        assert_eq!(faces[0].sense(), crate::entities::acis::Sense::Forward);

        // Check plane surface
        let planes = doc.records_of_type("plane-surface");
        assert_eq!(planes.len(), 1);
        let plane = crate::entities::acis::SatPlaneSurface::from_record(planes[0]).unwrap();
        assert_eq!(plane.root_point(), (0.0, 0.0, 5.0));
        assert_eq!(plane.normal(), (0.0, 0.0, 1.0));
    }

    #[test]
    fn test_solid3d_set_sat_document() {
        let mut doc = crate::entities::acis::SatDocument::new_body();
        doc.add_plane_surface([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);

        let mut solid = Solid3D::new();
        solid.set_sat_document(&doc);

        assert!(solid.has_acis_data());
        assert!(!solid.acis_data.is_binary);
        assert!(solid.acis_data.sat_data.contains("plane-surface"));
        assert!(solid.acis_data.sat_data.contains("700"));

        // Roundtrip: parse back
        let doc2 = solid.parse_sat().unwrap();
        assert_eq!(doc2.records.len(), doc.records.len());
    }

    #[test]
    fn test_region_parse_sat() {
        let sat_text = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            -0 asmheader $-1 -1 @12 700 7 0 0 @5 ACIS @3 7.0 @24 Thu Jan 01 00:00:00 2023 #\n\
            -1 body $-1 $-1 $-1 $-1 #\n\
            End-of-ACIS-data\n";

        let region = Region::from_sat(sat_text);
        let doc = region.parse_sat().unwrap();
        assert_eq!(doc.records.len(), 2);
    }

    #[test]
    fn test_body_parse_sat() {
        let sat_text = "700 0 1 0\n\
            @8 acadrust @8 ACIS 7.0 @24 Thu Jan 01 00:00:00 2023\n\
            1e-06 9.9999999999999995e-07\n\
            -0 asmheader $-1 -1 @12 700 7 0 0 @5 ACIS @3 7.0 @24 Thu Jan 01 00:00:00 2023 #\n\
            -1 body $-1 $-1 $-1 $-1 #\n\
            End-of-ACIS-data\n";

        let body = Body::from_sat(sat_text);
        let doc = body.parse_sat().unwrap();
        assert_eq!(doc.records.len(), 2);
    }

    #[test]
    fn test_acis_data_from_sat_document() {
        let doc = crate::entities::acis::SatDocument::new_body();
        let acis = AcisData::from_sat_document(&doc);
        assert!(acis.has_data());
        assert!(!acis.is_binary);
        assert!(acis.sat_data.contains("body"));
    }

    #[test]
    fn test_solid3d_parse_sat_binary_returns_none() {
        let solid = Solid3D::from_sab(b"ACIS BinaryFile".to_vec());
        assert!(solid.parse_sat().is_none());
    }

    #[test]
    fn test_solid3d_parse_sat_empty_returns_none() {
        let solid = Solid3D::new();
        assert!(solid.parse_sat().is_none());
    }
}
