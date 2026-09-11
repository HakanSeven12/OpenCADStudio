//! Section symbol entity (`AcDbSectionSymbol`, DXF class "SECTIONLINE").
//!
//! The section "A-A" cut mark drawn on a Model-Documentation base view. Its
//! `AcDbViewSymbol` base, complete repeated point records, and object references
//! are decoded and encoded as native fields.

use super::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transform, Transparency, Vector3};

/// One repeated point record in an `AcDbSectionSymbol`.
///
/// The field order and DXF codes are verified against ODA exports of native
/// R2013/R2018 Model Documentation entities: point (10/20/30), bulge (40),
/// label (1), label offset (11/21/31), and the trailing byte (280).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectionSymbolPoint {
    /// Section-line vertex.
    pub point: Vector3,
    /// Bulge at this vertex.
    pub bulge: f64,
    /// Label attached to this vertex.
    pub label: String,
    /// Label offset from the vertex.
    pub label_offset: Vector3,
    /// Verified trailing group-280 byte; its public semantic name is unknown.
    pub raw_flag_280: u8,
}

impl SectionSymbolPoint {
    /// Create an empty point record.
    pub fn new() -> Self {
        Self {
            point: Vector3::ZERO,
            bulge: 0.0,
            label: String::new(),
            label_offset: Vector3::ZERO,
            raw_flag_280: 0,
        }
    }
}

impl Default for SectionSymbolPoint {
    fn default() -> Self {
        Self::new()
    }
}

/// A Model-Documentation section mark (`AcDbSectionSymbol`).
///
/// [`points`](Self::points) is the canonical serialized geometry. `end_*`,
/// `tick_*`, and `label` remain as compatibility projections for renderers
/// written before the complete point schema was known; they are not separate
/// fields on disk.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectionSymbol {
    /// Common entity data (handle, layer, color, …).
    pub common: EntityCommon,
    /// `AcDbViewSymbol` version (first group 70 in that subclass).
    pub view_symbol_version: i16,
    /// The symbol's `AcDbSectionViewStyle` handle (group 340).
    pub style_handle: Handle,
    /// `AcDbViewSymbol` scale (group 40).
    pub symbol_scale: f64,
    /// Parent `AcDbViewRep` handle (group 330).
    pub view_rep_handle: Handle,
    /// Verified second group 70 in `AcDbViewSymbol`; public semantics unknown.
    pub raw_view_symbol_70: i16,
    /// `AcDbSectionSymbol` version (first group 70 in that subclass).
    pub version: i16,
    /// Verified first group 90; equals the point-record count in the corpus.
    pub raw_point_count_90: i32,
    /// Verified second group 90 after the point count; public semantics unknown.
    pub raw_flags_90: i32,
    /// Verified third group 90; equals the point count in the available corpus.
    pub raw_point_record_count: i32,
    /// Complete repeated section-point records.
    pub points: Vec<SectionSymbolPoint>,
    /// First cut-line endpoint (paper-space X, Y).
    ///
    /// Compatibility projection of the first item in [`points`](Self::points).
    pub end_a: [f64; 2],
    /// Second cut-line endpoint (paper-space X, Y).
    ///
    /// Compatibility projection of the last item in [`points`](Self::points).
    pub end_b: [f64; 2],
    /// Legacy renderer value for the first label-offset Y.
    ///
    /// Compatibility projection of the first point's label-offset Y.
    pub tick_a: f64,
    /// Legacy renderer value for the last label-offset Y.
    ///
    /// Compatibility projection of the last point's label-offset Y.
    pub tick_b: f64,
    /// Section identifier text (drawn at each end).
    ///
    /// Compatibility projection of the first non-empty point label.
    pub label: String,
}

impl SectionSymbol {
    /// Create an empty section symbol.
    pub fn new() -> Self {
        SectionSymbol {
            common: EntityCommon::new(),
            view_symbol_version: 0,
            style_handle: Handle::NULL,
            symbol_scale: 1.0,
            view_rep_handle: Handle::NULL,
            raw_view_symbol_70: 0,
            version: 0,
            raw_point_count_90: 0,
            raw_flags_90: 0,
            raw_point_record_count: 0,
            points: Vec::new(),
            end_a: [0.0; 2],
            end_b: [0.0; 2],
            tick_a: 0.0,
            tick_b: 0.0,
            label: String::new(),
        }
    }

    /// Refresh the compatibility display fields from the complete point list.
    pub fn sync_display_fields(&mut self) {
        let Some(first) = self.points.first() else {
            return;
        };
        let last = self.points.last().unwrap_or(first);
        self.end_a = [first.point.x, first.point.y];
        self.end_b = [last.point.x, last.point.y];
        self.tick_a = first.label_offset.y;
        self.tick_b = last.label_offset.y;
        self.label = self
            .points
            .iter()
            .find_map(|point| (!point.label.is_empty()).then(|| point.label.clone()))
            .unwrap_or_default();
    }
}

impl Default for SectionSymbol {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for SectionSymbol {
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
    fn set_line_weight(&mut self, weight: LineWeight) {
        self.common.line_weight = weight;
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
        let points = if self.points.is_empty() {
            vec![
                Vector3::new(self.end_a[0], self.end_a[1], 0.0),
                Vector3::new(self.end_b[0], self.end_b[1], 0.0),
            ]
        } else {
            self.points.iter().map(|point| point.point).collect()
        };
        BoundingBox3D::from_points(&points)
            .unwrap_or_else(|| BoundingBox3D::from_point(Vector3::ZERO))
    }
    fn translate(&mut self, offset: Vector3) {
        for point in &mut self.points {
            point.point = point.point + offset;
        }
        self.end_a[0] += offset.x;
        self.end_a[1] += offset.y;
        self.end_b[0] += offset.x;
        self.end_b[1] += offset.y;
    }
    fn entity_type(&self) -> &'static str {
        "SECTIONLINE"
    }
    fn apply_transform(&mut self, transform: &Transform) {
        for point in &mut self.points {
            let original = point.point;
            let transformed = transform.apply(original);
            let offset_end = transform.apply(original + point.label_offset);
            point.point = transformed;
            point.label_offset = offset_end - transformed;
        }
        let end_a = transform.apply(Vector3::new(self.end_a[0], self.end_a[1], 0.0));
        let end_b = transform.apply(Vector3::new(self.end_b[0], self.end_b[1], 0.0));
        self.end_a = [end_a.x, end_a.y];
        self.end_b = [end_b.x, end_b.y];
    }
}

/// Display-relevant fields of an `AcDbSectionViewStyle` (DXF class
/// "ACDBSECTIONVIEWSTYLE"), the named style that controls how a section mark
/// is drawn. These are the fields the editor needs to render the mark.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SectionViewStyle {
    /// Whether direction arrowheads are drawn (style `flags` bit 0x02).
    pub show_arrows: bool,
    /// Whether the full cutting-plane line is drawn through the view (`flags`
    /// bit 0x08). Off = the familiar "broken" section line: only the end
    /// segments are drawn.
    pub show_plane_line: bool,
    /// Whether the end (and bend) line segments are drawn (`flags` bit 0x20).
    pub show_end_lines: bool,
    /// Arrowhead size (`arrow_symbol_size`).
    pub arrow_size: f64,
    /// How far the arrow extends past the cut line (`arrow_symbol_extension_length`).
    pub arrow_extension: f64,
    /// Section identifier ("A") text height (`identifier_height`).
    pub label_height: f64,
    /// Gap between the cut line and the identifier text (`identifier_offset`).
    pub label_offset: f64,
    /// Identifier placement enum (`identifier_position`), raw value.
    pub label_position: i32,
    /// Arrow placement enum (`arrow_position`), raw value.
    pub arrow_position: i32,
    /// End-segment length (`end_line_length`) — with the overshoot this equals
    /// the symbol's per-end tick.
    pub end_line_length: f64,
    /// Extension of the end segment beyond the arrow anchor (`end_line_overshoot`).
    pub end_line_overshoot: f64,
    /// Arrowhead block-record handles for the start / end of the section line
    /// (`arrow_start_symbol` / `arrow_end_symbol`). `0` (null) selects the
    /// built-in default arrow — the same ClosedFilled block dimensions and
    /// leaders default to.
    pub arrow_start_handle: u64,
    /// See [`arrow_start_handle`](Self::arrow_start_handle).
    pub arrow_end_handle: u64,
    /// True when both arrow symbol handles are null, i.e. the built-in default
    /// (solid/filled) arrowhead is used rather than a custom arrow block.
    pub arrow_is_default: bool,
}
