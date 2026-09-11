//! Leader entity - Leader annotation line with arrow

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

/// Leader path type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LeaderPathType {
    /// Straight line segments
    #[default]
    StraightLine = 0,
    /// Spline path
    Spline = 1,
}

impl LeaderPathType {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            1 => LeaderPathType::Spline,
            _ => LeaderPathType::StraightLine,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }
}

/// Leader creation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum LeaderCreationType {
    /// Created with text annotation
    #[default]
    WithText = 0,
    /// Created with tolerance
    WithTolerance = 1,
    /// Created with block reference
    WithBlock = 2,
    /// Created with no annotation
    NoAnnotation = 3,
}

impl LeaderCreationType {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            1 => LeaderCreationType::WithTolerance,
            2 => LeaderCreationType::WithBlock,
            3 => LeaderCreationType::NoAnnotation,
            _ => LeaderCreationType::WithText,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }
}

/// Hookline direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum HooklineDirection {
    /// Direction opposite to horizontal
    #[default]
    Opposite = 0,
    /// Direction same as horizontal
    Same = 1,
}

impl HooklineDirection {
    /// Create from DXF value
    pub fn from_value(value: i16) -> Self {
        match value {
            1 => HooklineDirection::Same,
            _ => HooklineDirection::Opposite,
        }
    }

    /// Convert to DXF value
    pub fn to_value(&self) -> i16 {
        *self as i16
    }
}

/// Leader entity - creates an annotation leader line with arrow
///
/// Leaders are used to point to features and connect them with annotation
/// (text, tolerance, or block reference). The leader consists of a series
/// of line segments or a spline path, typically with an arrowhead at the
/// first vertex.
///
/// # DXF Entity Type
/// LEADER
///
/// # Example
/// ```ignore
/// use acadrust::entities::Leader;
/// use acadrust::types::Vector3;
///
/// let mut leader = Leader::new();
/// leader.add_vertex(Vector3::new(0.0, 0.0, 0.0));    // Arrow point
/// leader.add_vertex(Vector3::new(10.0, 10.0, 0.0));  // First bend
/// leader.add_vertex(Vector3::new(20.0, 10.0, 0.0));  // End at annotation
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Leader {
    /// Common entity properties
    pub common: EntityCommon,
    /// Dimension style name
    pub dimension_style: String,
    /// Arrow enabled
    pub arrow_enabled: bool,
    /// Path type
    pub path_type: LeaderPathType,
    /// Creation type (what annotation is attached)
    pub creation_type: LeaderCreationType,
    /// Hookline direction
    pub hookline_direction: HooklineDirection,
    /// Hookline enabled
    pub hookline_enabled: bool,
    /// Text annotation height
    pub text_height: f64,
    /// Text annotation width
    pub text_width: f64,
    /// Leader vertices (arrow point first)
    pub vertices: Vec<Vector3>,
    /// Override color for leader
    pub override_color: Color,
    /// Handle to associated annotation entity
    pub annotation_handle: Handle,
    /// Normal vector (extrusion direction)
    pub normal: Vector3,
    /// Horizontal direction for text
    pub horizontal_direction: Vector3,
    /// Block content offset
    pub block_offset: Vector3,
    /// Annotation placement offset
    pub annotation_offset: Vector3,
    /// Original leader origin stored in DWG.
    pub origin: Vector3,
    /// R13/R14 dimension gap used by the leader.
    pub dimension_gap: f64,
    /// Arrowhead type flags. Bit 3 controls the hookline state.
    pub arrowhead_type: i16,
    /// R13/R14 arrow size.
    pub arrow_size: f64,
    /// R13/R14 by-block color index.
    pub byblock_color: i16,
    /// Undocumented DWG fields preserved for round-trip.
    pub dwg_unknown_bit1: bool,
    pub dwg_unknown_bit2: bool,
    pub dwg_unknown_bit3: bool,
    pub dwg_unknown_bit4: bool,
    pub dwg_unknown_bit5: bool,
    pub dwg_unknown_short1: i16,
}

impl Leader {
    /// Create a new empty leader
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            dimension_style: "Standard".to_string(),
            arrow_enabled: true,
            path_type: LeaderPathType::StraightLine,
            creation_type: LeaderCreationType::WithText,
            hookline_direction: HooklineDirection::Opposite,
            hookline_enabled: false,
            text_height: 2.5,
            text_width: 0.0,
            vertices: Vec::new(),
            override_color: Color::ByLayer,
            annotation_handle: Handle::NULL,
            normal: Vector3::UNIT_Z,
            horizontal_direction: Vector3::UNIT_X,
            block_offset: Vector3::ZERO,
            annotation_offset: Vector3::ZERO,
            origin: Vector3::ZERO,
            dimension_gap: 0.0,
            arrowhead_type: 0,
            arrow_size: 0.0,
            byblock_color: 0,
            dwg_unknown_bit1: false,
            dwg_unknown_bit2: false,
            dwg_unknown_bit3: false,
            dwg_unknown_bit4: false,
            dwg_unknown_bit5: false,
            dwg_unknown_short1: 0,
        }
    }

    /// Create a leader from a list of vertices
    pub fn from_vertices(vertices: Vec<Vector3>) -> Self {
        let mut leader = Self::new();
        leader.vertices = vertices;
        leader
    }

    /// Create a simple two-point leader
    pub fn two_point(arrow_point: Vector3, end_point: Vector3) -> Self {
        let mut leader = Self::new();
        leader.vertices.push(arrow_point);
        leader.vertices.push(end_point);
        leader
    }

    /// Create a leader with a horizontal landing
    pub fn with_landing(arrow_point: Vector3, bend_point: Vector3, landing_length: f64) -> Self {
        let mut leader = Self::new();
        leader.vertices.push(arrow_point);
        leader.vertices.push(bend_point);

        // Add horizontal landing
        let landing_end = Vector3::new(bend_point.x + landing_length, bend_point.y, bend_point.z);
        leader.vertices.push(landing_end);
        leader.hookline_enabled = true;
        leader
    }

    /// Add a vertex to the leader path
    pub fn add_vertex(&mut self, vertex: Vector3) {
        self.vertices.push(vertex);
    }

    /// Insert a vertex at a specific index
    pub fn insert_vertex(&mut self, index: usize, vertex: Vector3) {
        if index <= self.vertices.len() {
            self.vertices.insert(index, vertex);
        }
    }

    /// Remove a vertex at a specific index
    pub fn remove_vertex(&mut self, index: usize) -> Option<Vector3> {
        if index < self.vertices.len() {
            Some(self.vertices.remove(index))
        } else {
            None
        }
    }

    /// Get the number of vertices
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Get the arrow point (first vertex)
    pub fn arrow_point(&self) -> Option<Vector3> {
        self.vertices.first().copied()
    }

    /// Get the end point (last vertex)
    pub fn end_point(&self) -> Option<Vector3> {
        self.vertices.last().copied()
    }

    /// Set the arrow point (first vertex)
    pub fn set_arrow_point(&mut self, point: Vector3) {
        if self.vertices.is_empty() {
            self.vertices.push(point);
        } else {
            self.vertices[0] = point;
        }
    }

    /// Calculate the total length of the leader path
    pub fn length(&self) -> f64 {
        if self.vertices.len() < 2 {
            return 0.0;
        }

        let mut total = 0.0;
        for i in 0..self.vertices.len() - 1 {
            total += self.vertices[i].distance(&self.vertices[i + 1]);
        }
        total
    }

    /// Get the direction at the arrow point (for arrow orientation)
    pub fn arrow_direction(&self) -> Option<Vector3> {
        if self.vertices.len() < 2 {
            return None;
        }

        let dir = self.vertices[1] - self.vertices[0];
        if dir.length_squared() > 0.0 {
            Some(dir.normalize())
        } else {
            None
        }
    }

    /// Set the path type
    pub fn set_path_type(&mut self, path_type: LeaderPathType) {
        self.path_type = path_type;
    }

    /// Enable or disable the arrow
    pub fn set_arrow_enabled(&mut self, enabled: bool) {
        self.arrow_enabled = enabled;
    }

    /// Enable or disable the hookline
    pub fn set_hookline_enabled(&mut self, enabled: bool) {
        self.hookline_enabled = enabled;
    }

    /// Set the dimension style
    pub fn set_dimension_style(&mut self, style: impl Into<String>) {
        self.dimension_style = style.into();
    }

    /// Reverse the direction of the leader
    pub fn reverse(&mut self) {
        self.vertices.reverse();
    }

    /// Clear all vertices
    pub fn clear(&mut self) {
        self.vertices.clear();
    }

    /// Builder: Add vertex
    pub fn with_vertex(mut self, vertex: Vector3) -> Self {
        self.vertices.push(vertex);
        self
    }

    /// Builder: Set arrow enabled
    pub fn with_arrow(mut self, enabled: bool) -> Self {
        self.arrow_enabled = enabled;
        self
    }

    /// Builder: Set path type to spline
    pub fn with_spline_path(mut self) -> Self {
        self.path_type = LeaderPathType::Spline;
        self
    }

    /// Builder: Set hookline enabled
    pub fn with_hookline(mut self) -> Self {
        self.hookline_enabled = true;
        self
    }

    /// Builder: Set dimension style
    pub fn with_dimension_style(mut self, style: impl Into<String>) -> Self {
        self.dimension_style = style.into();
        self
    }

    /// Builder: Set layer
    pub fn with_layer(mut self, layer: impl Into<String>) -> Self {
        self.common.layer = layer.into();
        self
    }

    /// Builder: Set color
    pub fn with_color(mut self, color: Color) -> Self {
        self.common.color = color;
        self
    }

    /// Builder: Set creation type
    pub fn with_creation_type(mut self, creation_type: LeaderCreationType) -> Self {
        self.creation_type = creation_type;
        self
    }

    /// Builder: Associate with annotation
    pub fn with_annotation(mut self, handle: Handle) -> Self {
        self.annotation_handle = handle;
        self
    }
}

impl Default for Leader {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for Leader {
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
        BoundingBox3D::from_points(&self.vertices).unwrap_or_default()
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_leader(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "LEADER"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_leader(self, transform);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_leader_creation() {
        let leader = Leader::new();
        assert!(leader.arrow_enabled);
        assert_eq!(leader.path_type, LeaderPathType::StraightLine);
        assert_eq!(leader.vertex_count(), 0);
    }

    #[test]
    fn test_leader_from_vertices() {
        let vertices = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
            Vector3::new(20.0, 10.0, 0.0),
        ];
        let leader = Leader::from_vertices(vertices);
        assert_eq!(leader.vertex_count(), 3);
    }

    #[test]
    fn test_leader_two_point() {
        let leader = Leader::two_point(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 10.0, 0.0));
        assert_eq!(leader.vertex_count(), 2);
        assert_eq!(leader.arrow_point(), Some(Vector3::new(0.0, 0.0, 0.0)));
        assert_eq!(leader.end_point(), Some(Vector3::new(10.0, 10.0, 0.0)));
    }

    #[test]
    fn test_leader_with_landing() {
        let leader = Leader::with_landing(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
            5.0,
        );
        assert_eq!(leader.vertex_count(), 3);
        assert!(leader.hookline_enabled);
        assert_eq!(leader.end_point(), Some(Vector3::new(15.0, 10.0, 0.0)));
    }

    #[test]
    fn test_leader_add_vertex() {
        let mut leader = Leader::new();
        leader.add_vertex(Vector3::new(0.0, 0.0, 0.0));
        leader.add_vertex(Vector3::new(10.0, 10.0, 0.0));
        assert_eq!(leader.vertex_count(), 2);
    }

    #[test]
    fn test_leader_insert_remove_vertex() {
        let mut leader = Leader::from_vertices(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(20.0, 0.0, 0.0),
        ]);

        leader.insert_vertex(1, Vector3::new(10.0, 10.0, 0.0));
        assert_eq!(leader.vertex_count(), 3);

        let removed = leader.remove_vertex(1);
        assert_eq!(removed, Some(Vector3::new(10.0, 10.0, 0.0)));
        assert_eq!(leader.vertex_count(), 2);
    }

    #[test]
    fn test_leader_length() {
        let leader = Leader::from_vertices(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
        ]);
        assert!((leader.length() - 20.0).abs() < 1e-10);
    }

    #[test]
    fn test_leader_arrow_direction() {
        let leader = Leader::from_vertices(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
        ]);

        let dir = leader.arrow_direction().unwrap();
        assert!((dir.x - 1.0).abs() < 1e-10);
        assert!(dir.y.abs() < 1e-10);
    }

    #[test]
    fn test_leader_reverse() {
        let mut leader = Leader::from_vertices(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
        ]);

        leader.reverse();

        assert_eq!(leader.arrow_point(), Some(Vector3::new(10.0, 10.0, 0.0)));
        assert_eq!(leader.end_point(), Some(Vector3::new(0.0, 0.0, 0.0)));
    }

    #[test]
    fn test_leader_translate() {
        let mut leader = Leader::from_vertices(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
        ]);

        leader.translate(Vector3::new(5.0, 5.0, 0.0));

        assert_eq!(leader.arrow_point(), Some(Vector3::new(5.0, 5.0, 0.0)));
        assert_eq!(leader.end_point(), Some(Vector3::new(15.0, 15.0, 0.0)));
    }

    #[test]
    fn test_leader_path_type() {
        assert_eq!(LeaderPathType::from_value(0), LeaderPathType::StraightLine);
        assert_eq!(LeaderPathType::from_value(1), LeaderPathType::Spline);
        assert_eq!(LeaderPathType::Spline.to_value(), 1);
    }

    #[test]
    fn test_leader_creation_type() {
        assert_eq!(
            LeaderCreationType::from_value(0),
            LeaderCreationType::WithText
        );
        assert_eq!(
            LeaderCreationType::from_value(2),
            LeaderCreationType::WithBlock
        );
        assert_eq!(LeaderCreationType::WithBlock.to_value(), 2);
    }

    #[test]
    fn test_leader_builder() {
        let leader = Leader::new()
            .with_vertex(Vector3::new(0.0, 0.0, 0.0))
            .with_vertex(Vector3::new(10.0, 10.0, 0.0))
            .with_hookline()
            .with_spline_path()
            .with_dimension_style("ISO-25")
            .with_layer("LEADERS");

        assert_eq!(leader.vertex_count(), 2);
        assert!(leader.hookline_enabled);
        assert_eq!(leader.path_type, LeaderPathType::Spline);
        assert_eq!(leader.dimension_style, "ISO-25");
        assert_eq!(leader.common.layer, "LEADERS");
    }

    #[test]
    fn test_leader_bounding_box() {
        let leader = Leader::from_vertices(vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
            Vector3::new(20.0, 5.0, 0.0),
        ]);

        let bbox = leader.bounding_box();
        assert_eq!(bbox.min.x, 0.0);
        assert_eq!(bbox.min.y, 0.0);
        assert_eq!(bbox.max.x, 20.0);
        assert_eq!(bbox.max.y, 10.0);
    }
}
