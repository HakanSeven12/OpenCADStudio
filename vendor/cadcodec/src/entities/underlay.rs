//! Underlay entity implementations.
//!
//! Underlay entities allow embedding external files (PDF, DWF, DGN)
//! as reference images in a drawing.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector2, Vector3};

use bitflags::bitflags;

// ============================================================================
// Flags and Enums
// ============================================================================

bitflags! {
    /// Display flags for underlay entities.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct UnderlayDisplayFlags: u8 {
        /// No flags.
        const NONE = 0;
        /// Clip content.
        const CLIPPING = 1;
        /// Underlay is on (visible).
        const ON = 2;
        /// Monochrome display.
        const MONOCHROME = 4;
        /// Adjust colors for background.
        const ADJUST_FOR_BACKGROUND = 8;
        /// Clip is inside mode (inverted).
        const CLIP_INSIDE = 16;
        /// Default flags (on, no clipping).
        const DEFAULT = Self::ON.bits();
    }
}

/// Type of underlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum UnderlayType {
    /// PDF file underlay.
    #[default]
    Pdf,
    /// DWF file underlay.
    Dwf,
    /// DGN file underlay (MicroStation).
    Dgn,
}

impl UnderlayType {
    /// Returns the entity type name for DXF.
    pub fn entity_name(&self) -> &'static str {
        match self {
            UnderlayType::Pdf => "PDFUNDERLAY",
            UnderlayType::Dwf => "DWFUNDERLAY",
            UnderlayType::Dgn => "DGNUNDERLAY",
        }
    }

    /// Returns the definition type name for DXF.
    pub fn definition_name(&self) -> &'static str {
        match self {
            UnderlayType::Pdf => "PDFDEFINITION",
            UnderlayType::Dwf => "DWFDEFINITION",
            UnderlayType::Dgn => "DGNDEFINITION",
        }
    }

    /// Returns the subclass marker.
    pub fn subclass_marker(&self) -> &'static str {
        "AcDbUnderlayReference"
    }

    /// Returns the definition subclass marker.
    pub fn definition_subclass_marker(&self) -> &'static str {
        match self {
            UnderlayType::Pdf => "AcDbPdfDefinition",
            UnderlayType::Dwf => "AcDbDwfDefinition",
            UnderlayType::Dgn => "AcDbDgnDefinition",
        }
    }
}

// ============================================================================
// Underlay Definition
// ============================================================================

/// Underlay definition (object that stores the file reference).
///
/// This is a non-graphical object that stores the path to the external
/// file and page/sheet information.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UnderlayDefinition {
    /// Object handle.
    pub handle: Handle,

    /// Owner handle (usually object dictionary).
    pub owner_handle: Handle,

    /// Type of underlay.
    pub underlay_type: UnderlayType,

    /// File path to the underlay (PDF, DWF, or DGN file).
    /// DXF code: 1
    pub file_path: String,

    /// Page/sheet name (for PDF) or item name.
    /// DXF code: 2
    pub page_name: String,

    /// Definition name (used for lookups).
    /// DXF code: 3 (for DGN)
    pub name: String,

    /// Reactors (entities referencing this definition).
    pub reactors: Vec<Handle>,
}

impl UnderlayDefinition {
    /// Creates a new underlay definition.
    pub fn new(underlay_type: UnderlayType) -> Self {
        UnderlayDefinition {
            handle: Handle::NULL,
            owner_handle: Handle::NULL,
            underlay_type,
            file_path: String::new(),
            page_name: String::new(),
            name: String::new(),
            reactors: Vec::new(),
        }
    }

    /// Creates a PDF underlay definition.
    pub fn pdf(file_path: &str, page_name: &str) -> Self {
        UnderlayDefinition {
            underlay_type: UnderlayType::Pdf,
            file_path: file_path.to_string(),
            page_name: page_name.to_string(),
            ..Self::new(UnderlayType::Pdf)
        }
    }

    /// Creates a DWF underlay definition.
    pub fn dwf(file_path: &str, sheet_name: &str) -> Self {
        UnderlayDefinition {
            underlay_type: UnderlayType::Dwf,
            file_path: file_path.to_string(),
            page_name: sheet_name.to_string(),
            ..Self::new(UnderlayType::Dwf)
        }
    }

    /// Creates a DGN underlay definition.
    pub fn dgn(file_path: &str, model_name: &str) -> Self {
        UnderlayDefinition {
            underlay_type: UnderlayType::Dgn,
            file_path: file_path.to_string(),
            page_name: model_name.to_string(),
            ..Self::new(UnderlayType::Dgn)
        }
    }

    /// Returns the entity type name for DXF.
    pub fn entity_name(&self) -> &'static str {
        self.underlay_type.definition_name()
    }

    /// Returns the subclass marker.
    pub fn subclass_marker(&self) -> &'static str {
        self.underlay_type.definition_subclass_marker()
    }

    /// Returns true if the file exists (basic check).
    pub fn file_exists(&self) -> bool {
        std::path::Path::new(&self.file_path).exists()
    }

    /// Returns the file extension.
    pub fn file_extension(&self) -> Option<&str> {
        std::path::Path::new(&self.file_path)
            .extension()
            .and_then(|s| s.to_str())
    }
}

impl Default for UnderlayDefinition {
    fn default() -> Self {
        Self::new(UnderlayType::Pdf)
    }
}

// ============================================================================
// Underlay Entity (Base)
// ============================================================================

/// Base underlay entity.
///
/// Represents an underlay reference placed in the drawing.
/// This is a common structure for PDF, DWF, and DGN underlays.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Underlay {
    /// Common entity data.
    pub common: EntityCommon,

    /// Type of underlay.
    pub underlay_type: UnderlayType,

    /// Handle to the underlay definition object.
    /// DXF code: 340
    pub definition_handle: Handle,

    /// Insertion point in WCS.
    /// DXF codes: 10, 20, 30
    pub insertion_point: Vector3,

    /// X scale factor.
    /// DXF code: 41
    pub x_scale: f64,

    /// Y scale factor.
    /// DXF code: 42
    pub y_scale: f64,

    /// Z scale factor.
    /// DXF code: 43
    pub z_scale: f64,

    /// Rotation angle in radians.
    /// DXF code: 50
    pub rotation: f64,

    /// Normal vector (extrusion direction).
    /// DXF codes: 210, 220, 230
    pub normal: Vector3,

    /// Display flags.
    /// DXF code: 280
    pub flags: UnderlayDisplayFlags,

    /// Contrast value (0-100).
    /// DXF code: 281
    pub contrast: u8,

    /// Fade value (0-80).
    /// DXF code: 282
    pub fade: u8,

    /// Clipping boundary vertices (in local coordinates).
    /// DXF codes: 11, 21 (repeated)
    pub clip_boundary_vertices: Vec<Vector2>,

    /// Whether clip boundary is inverted.
    pub clip_inverted: bool,
}

impl Underlay {
    /// Entity type name.
    pub const ENTITY_NAME: &'static str = "UNDERLAY";

    /// DXF subclass marker.
    pub const SUBCLASS_MARKER: &'static str = "AcDbUnderlayReference";

    /// Creates a new underlay with default values.
    pub fn new(underlay_type: UnderlayType) -> Self {
        Underlay {
            common: EntityCommon::default(),
            underlay_type,
            definition_handle: Handle::NULL,
            insertion_point: Vector3::ZERO,
            x_scale: 1.0,
            y_scale: 1.0,
            z_scale: 1.0,
            rotation: 0.0,
            normal: Vector3::UNIT_Z,
            flags: UnderlayDisplayFlags::DEFAULT,
            contrast: 100,
            fade: 0,
            clip_boundary_vertices: Vec::new(),
            clip_inverted: false,
        }
    }

    /// Creates a PDF underlay.
    pub fn pdf() -> Self {
        Self::new(UnderlayType::Pdf)
    }

    /// Creates a DWF underlay.
    pub fn dwf() -> Self {
        Self::new(UnderlayType::Dwf)
    }

    /// Creates a DGN underlay.
    pub fn dgn() -> Self {
        Self::new(UnderlayType::Dgn)
    }

    /// Creates an underlay at a specific location.
    pub fn at_point(underlay_type: UnderlayType, point: Vector3) -> Self {
        Underlay {
            insertion_point: point,
            ..Self::new(underlay_type)
        }
    }

    /// Creates an underlay with scale.
    pub fn with_scale(underlay_type: UnderlayType, point: Vector3, scale: f64) -> Self {
        Underlay {
            insertion_point: point,
            x_scale: scale,
            y_scale: scale,
            z_scale: scale,
            ..Self::new(underlay_type)
        }
    }

    /// Returns the entity type name for DXF.
    pub fn entity_name(&self) -> &'static str {
        self.underlay_type.entity_name()
    }

    /// Returns the subclass marker.
    pub fn subclass_marker(&self) -> &'static str {
        Self::SUBCLASS_MARKER
    }

    /// Sets uniform scale.
    pub fn set_scale(&mut self, scale: f64) {
        self.x_scale = scale;
        self.y_scale = scale;
        self.z_scale = scale;
    }

    /// Sets non-uniform scale.
    pub fn set_scale_xyz(&mut self, x: f64, y: f64, z: f64) {
        self.x_scale = x;
        self.y_scale = y;
        self.z_scale = z;
    }

    /// Returns the uniform scale if all scales are equal.
    pub fn uniform_scale(&self) -> Option<f64> {
        if (self.x_scale - self.y_scale).abs() < 1e-10
            && (self.y_scale - self.z_scale).abs() < 1e-10
        {
            Some(self.x_scale)
        } else {
            None
        }
    }

    /// Sets the rotation in degrees.
    pub fn set_rotation_degrees(&mut self, degrees: f64) {
        self.rotation = degrees.to_radians();
    }

    /// Gets the rotation in degrees.
    pub fn rotation_degrees(&self) -> f64 {
        self.rotation.to_degrees()
    }

    /// Sets a rectangular clip boundary.
    pub fn set_rectangular_clip(&mut self, min: Vector2, max: Vector2) {
        self.clip_boundary_vertices = vec![
            min,
            Vector2::new(max.x, min.y),
            max,
            Vector2::new(min.x, max.y),
        ];
        self.flags |= UnderlayDisplayFlags::CLIPPING;
    }

    /// Sets a polygonal clip boundary.
    pub fn set_polygon_clip(&mut self, vertices: &[Vector2]) {
        if vertices.len() >= 3 {
            self.clip_boundary_vertices = vertices.to_vec();
            self.flags |= UnderlayDisplayFlags::CLIPPING;
        }
    }

    /// Clears the clip boundary.
    pub fn clear_clip(&mut self) {
        self.clip_boundary_vertices.clear();
        self.flags -= UnderlayDisplayFlags::CLIPPING;
    }

    /// Returns true if clipping is enabled.
    pub fn is_clipping(&self) -> bool {
        self.flags.contains(UnderlayDisplayFlags::CLIPPING)
    }

    /// Returns true if the underlay is visible.
    pub fn is_on(&self) -> bool {
        self.flags.contains(UnderlayDisplayFlags::ON)
    }

    /// Sets the underlay visibility.
    pub fn set_on(&mut self, on: bool) {
        if on {
            self.flags |= UnderlayDisplayFlags::ON;
        } else {
            self.flags -= UnderlayDisplayFlags::ON;
        }
    }

    /// Returns true if displaying in monochrome.
    pub fn is_monochrome(&self) -> bool {
        self.flags.contains(UnderlayDisplayFlags::MONOCHROME)
    }

    /// Sets monochrome display mode.
    pub fn set_monochrome(&mut self, monochrome: bool) {
        if monochrome {
            self.flags |= UnderlayDisplayFlags::MONOCHROME;
        } else {
            self.flags -= UnderlayDisplayFlags::MONOCHROME;
        }
    }

    /// Sets contrast (0-100).
    pub fn set_contrast(&mut self, value: u8) {
        self.contrast = value.min(100);
    }

    /// Sets fade (0-80).
    pub fn set_fade(&mut self, value: u8) {
        self.fade = value.min(80);
    }

    /// Returns the clip boundary vertices in world coordinates.
    pub fn world_clip_boundary(&self) -> Vec<Vector3> {
        let cos_r = self.rotation.cos();
        let sin_r = self.rotation.sin();

        self.clip_boundary_vertices
            .iter()
            .map(|v| {
                let x = v.x * self.x_scale;
                let y = v.y * self.y_scale;
                let rx = x * cos_r - y * sin_r;
                let ry = x * sin_r + y * cos_r;
                self.insertion_point + Vector3::new(rx, ry, 0.0)
            })
            .collect()
    }

    /// Returns the number of clip boundary vertices.
    pub fn clip_vertex_count(&self) -> usize {
        self.clip_boundary_vertices.len()
    }
}

impl Default for Underlay {
    fn default() -> Self {
        Self::new(UnderlayType::Pdf)
    }
}

impl Entity for Underlay {
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
        // Without knowing the actual underlay dimensions, we can only
        // provide a bounding box based on clip boundary or insertion point
        if !self.clip_boundary_vertices.is_empty() {
            let world_verts = self.world_clip_boundary();
            let mut min = world_verts[0];
            let mut max = world_verts[0];

            for v in &world_verts[1..] {
                min.x = min.x.min(v.x);
                min.y = min.y.min(v.y);
                min.z = min.z.min(v.z);
                max.x = max.x.max(v.x);
                max.y = max.y.max(v.y);
                max.z = max.z.max(v.z);
            }

            BoundingBox3D::new(min, max)
        } else {
            // Return point bounding box
            BoundingBox3D::new(self.insertion_point, self.insertion_point)
        }
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_underlay(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        self.underlay_type.entity_name()
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_underlay(self, transform);
    }
}

// ============================================================================
// Convenience type aliases
// ============================================================================

/// PDF underlay entity.
pub type PdfUnderlay = Underlay;

/// DWF underlay entity.
pub type DwfUnderlay = Underlay;

/// DGN underlay entity.
pub type DgnUnderlay = Underlay;

/// PDF underlay definition.
pub type PdfUnderlayDefinition = UnderlayDefinition;

/// DWF underlay definition.
pub type DwfUnderlayDefinition = UnderlayDefinition;

/// DGN underlay definition.
pub type DgnUnderlayDefinition = UnderlayDefinition;

// ============================================================================
// Convenience constructors
// ============================================================================

impl Underlay {
    /// Creates a PDF underlay at a point.
    pub fn pdf_at(point: Vector3) -> Self {
        Self::at_point(UnderlayType::Pdf, point)
    }

    /// Creates a DWF underlay at a point.
    pub fn dwf_at(point: Vector3) -> Self {
        Self::at_point(UnderlayType::Dwf, point)
    }

    /// Creates a DGN underlay at a point.
    pub fn dgn_at(point: Vector3) -> Self {
        Self::at_point(UnderlayType::Dgn, point)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_underlay_definition_creation() {
        let def = UnderlayDefinition::new(UnderlayType::Pdf);
        assert_eq!(def.underlay_type, UnderlayType::Pdf);
        assert!(def.file_path.is_empty());
    }

    #[test]
    fn test_pdf_definition() {
        let def = UnderlayDefinition::pdf("drawings/floor_plan.pdf", "Page 1");
        assert_eq!(def.underlay_type, UnderlayType::Pdf);
        assert_eq!(def.file_path, "drawings/floor_plan.pdf");
        assert_eq!(def.page_name, "Page 1");
    }

    #[test]
    fn test_dwf_definition() {
        let def = UnderlayDefinition::dwf("drawings/assembly.dwf", "Sheet 1");
        assert_eq!(def.underlay_type, UnderlayType::Dwf);
        assert_eq!(def.file_path, "drawings/assembly.dwf");
    }

    #[test]
    fn test_dgn_definition() {
        let def = UnderlayDefinition::dgn("drawings/site.dgn", "Default");
        assert_eq!(def.underlay_type, UnderlayType::Dgn);
        assert_eq!(def.file_path, "drawings/site.dgn");
    }

    #[test]
    fn test_definition_entity_names() {
        let pdf_def = UnderlayDefinition::new(UnderlayType::Pdf);
        assert_eq!(pdf_def.entity_name(), "PDFDEFINITION");

        let dwf_def = UnderlayDefinition::new(UnderlayType::Dwf);
        assert_eq!(dwf_def.entity_name(), "DWFDEFINITION");

        let dgn_def = UnderlayDefinition::new(UnderlayType::Dgn);
        assert_eq!(dgn_def.entity_name(), "DGNDEFINITION");
    }

    #[test]
    fn test_underlay_creation() {
        let underlay = Underlay::new(UnderlayType::Pdf);
        assert_eq!(underlay.underlay_type, UnderlayType::Pdf);
        assert_eq!(underlay.insertion_point, Vector3::ZERO);
        assert_eq!(underlay.x_scale, 1.0);
        assert_eq!(underlay.y_scale, 1.0);
        assert_eq!(underlay.z_scale, 1.0);
    }

    #[test]
    fn test_pdf_underlay() {
        let underlay = Underlay::pdf();
        assert_eq!(underlay.underlay_type, UnderlayType::Pdf);
        assert_eq!(underlay.entity_name(), "PDFUNDERLAY");
    }

    #[test]
    fn test_dwf_underlay() {
        let underlay = Underlay::dwf();
        assert_eq!(underlay.underlay_type, UnderlayType::Dwf);
        assert_eq!(underlay.entity_name(), "DWFUNDERLAY");
    }

    #[test]
    fn test_dgn_underlay() {
        let underlay = Underlay::dgn();
        assert_eq!(underlay.underlay_type, UnderlayType::Dgn);
        assert_eq!(underlay.entity_name(), "DGNUNDERLAY");
    }

    #[test]
    fn test_at_point() {
        let underlay = Underlay::pdf_at(Vector3::new(10.0, 20.0, 0.0));
        assert_eq!(underlay.insertion_point.x, 10.0);
        assert_eq!(underlay.insertion_point.y, 20.0);
    }

    #[test]
    fn test_with_scale() {
        let underlay = Underlay::with_scale(UnderlayType::Pdf, Vector3::ZERO, 2.5);
        assert_eq!(underlay.x_scale, 2.5);
        assert_eq!(underlay.y_scale, 2.5);
        assert_eq!(underlay.z_scale, 2.5);
    }

    #[test]
    fn test_set_scale() {
        let mut underlay = Underlay::pdf();
        underlay.set_scale(3.0);

        assert_eq!(underlay.x_scale, 3.0);
        assert_eq!(underlay.y_scale, 3.0);
        assert_eq!(underlay.z_scale, 3.0);
    }

    #[test]
    fn test_set_scale_xyz() {
        let mut underlay = Underlay::pdf();
        underlay.set_scale_xyz(1.0, 2.0, 3.0);

        assert_eq!(underlay.x_scale, 1.0);
        assert_eq!(underlay.y_scale, 2.0);
        assert_eq!(underlay.z_scale, 3.0);
    }

    #[test]
    fn test_uniform_scale() {
        let mut underlay = Underlay::pdf();
        underlay.set_scale(2.0);
        assert_eq!(underlay.uniform_scale(), Some(2.0));

        underlay.set_scale_xyz(1.0, 2.0, 1.0);
        assert_eq!(underlay.uniform_scale(), None);
    }

    #[test]
    fn test_rotation() {
        let mut underlay = Underlay::pdf();
        underlay.set_rotation_degrees(45.0);

        assert!((underlay.rotation_degrees() - 45.0).abs() < 1e-10);
    }

    #[test]
    fn test_rectangular_clip() {
        let mut underlay = Underlay::pdf();
        underlay.set_rectangular_clip(Vector2::new(0.0, 0.0), Vector2::new(100.0, 100.0));

        assert_eq!(underlay.clip_vertex_count(), 4);
        assert!(underlay.is_clipping());
    }

    #[test]
    fn test_polygon_clip() {
        let mut underlay = Underlay::pdf();
        underlay.set_polygon_clip(&[
            Vector2::new(0.0, 0.0),
            Vector2::new(100.0, 0.0),
            Vector2::new(100.0, 100.0),
            Vector2::new(50.0, 150.0),
            Vector2::new(0.0, 100.0),
        ]);

        assert_eq!(underlay.clip_vertex_count(), 5);
        assert!(underlay.is_clipping());
    }

    #[test]
    fn test_clear_clip() {
        let mut underlay = Underlay::pdf();
        underlay.set_rectangular_clip(Vector2::new(0.0, 0.0), Vector2::new(100.0, 100.0));
        assert!(underlay.is_clipping());

        underlay.clear_clip();
        assert!(!underlay.is_clipping());
        assert_eq!(underlay.clip_vertex_count(), 0);
    }

    #[test]
    fn test_visibility() {
        let mut underlay = Underlay::pdf();
        assert!(underlay.is_on());

        underlay.set_on(false);
        assert!(!underlay.is_on());

        underlay.set_on(true);
        assert!(underlay.is_on());
    }

    #[test]
    fn test_monochrome() {
        let mut underlay = Underlay::pdf();
        assert!(!underlay.is_monochrome());

        underlay.set_monochrome(true);
        assert!(underlay.is_monochrome());
    }

    #[test]
    fn test_contrast_fade() {
        let mut underlay = Underlay::pdf();

        underlay.set_contrast(75);
        assert_eq!(underlay.contrast, 75);

        underlay.set_contrast(150); // Over max
        assert_eq!(underlay.contrast, 100);

        underlay.set_fade(50);
        assert_eq!(underlay.fade, 50);

        underlay.set_fade(100); // Over max
        assert_eq!(underlay.fade, 80);
    }

    #[test]
    fn test_translate() {
        let mut underlay = Underlay::pdf();
        underlay.translate(Vector3::new(10.0, 20.0, 30.0));

        assert_eq!(underlay.insertion_point.x, 10.0);
        assert_eq!(underlay.insertion_point.y, 20.0);
        assert_eq!(underlay.insertion_point.z, 30.0);
    }

    #[test]
    fn test_entity_type() {
        let pdf = Underlay::pdf();
        assert_eq!(pdf.entity_type(), "PDFUNDERLAY");

        let dwf = Underlay::dwf();
        assert_eq!(dwf.entity_type(), "DWFUNDERLAY");

        let dgn = Underlay::dgn();
        assert_eq!(dgn.entity_type(), "DGNUNDERLAY");
    }

    #[test]
    fn test_world_clip_boundary() {
        let mut underlay = Underlay::pdf();
        underlay.insertion_point = Vector3::new(10.0, 10.0, 0.0);
        underlay.x_scale = 2.0;
        underlay.y_scale = 2.0;
        underlay.rotation = 0.0;
        underlay.set_rectangular_clip(Vector2::new(0.0, 0.0), Vector2::new(5.0, 5.0));

        let world_verts = underlay.world_clip_boundary();
        assert_eq!(world_verts.len(), 4);
        // First vertex: 0,0 scaled by 2,2 + 10,10 = 10,10
        assert!((world_verts[0].x - 10.0).abs() < 1e-10);
        assert!((world_verts[0].y - 10.0).abs() < 1e-10);
    }

    #[test]
    fn test_underlay_type_names() {
        assert_eq!(UnderlayType::Pdf.entity_name(), "PDFUNDERLAY");
        assert_eq!(UnderlayType::Dwf.entity_name(), "DWFUNDERLAY");
        assert_eq!(UnderlayType::Dgn.entity_name(), "DGNUNDERLAY");
    }

    #[test]
    fn test_bounding_box_with_clip() {
        let mut underlay = Underlay::pdf();
        underlay.insertion_point = Vector3::new(10.0, 10.0, 0.0);
        underlay.set_rectangular_clip(Vector2::new(0.0, 0.0), Vector2::new(100.0, 50.0));

        let bb = underlay.bounding_box();
        assert!((bb.min.x - 10.0).abs() < 1e-10);
        assert!((bb.min.y - 10.0).abs() < 1e-10);
        assert!((bb.max.x - 110.0).abs() < 1e-10);
        assert!((bb.max.y - 60.0).abs() < 1e-10);
    }

    #[test]
    fn test_flags() {
        let underlay = Underlay::pdf();
        assert!(underlay.flags.contains(UnderlayDisplayFlags::ON));
        assert!(!underlay.flags.contains(UnderlayDisplayFlags::CLIPPING));
        assert!(!underlay.flags.contains(UnderlayDisplayFlags::MONOCHROME));
    }

    #[test]
    fn test_default() {
        let underlay = Underlay::default();
        assert_eq!(underlay.underlay_type, UnderlayType::Pdf);
    }
}
