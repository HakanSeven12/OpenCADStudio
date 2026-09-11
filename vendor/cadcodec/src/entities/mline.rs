//! MLine (Multiline) entity implementation.
//!
//! The MLine entity represents a complex of parallel lines with configurable
//! styles, caps, and joints.

use crate::entities::{Entity, EntityCommon};
use crate::types::{BoundingBox3D, Color, Handle, LineWeight, Transparency, Vector3};

use bitflags::bitflags;
use std::f64::consts::PI;

// ============================================================================
// Enums
// ============================================================================

/// Justification for MLine entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[repr(i16)]
pub enum MLineJustification {
    /// Justify to top line.
    Top = 0,
    /// Justify to zero offset (center).
    #[default]
    Zero = 1,
    /// Justify to bottom line.
    Bottom = 2,
}

impl From<i16> for MLineJustification {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::Top,
            1 => Self::Zero,
            2 => Self::Bottom,
            _ => Self::Zero,
        }
    }
}

// ============================================================================
// Bitflags
// ============================================================================

bitflags! {
    /// Flags for MLine entity.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct MLineFlags: i16 {
        /// Has at least one vertex.
        const HAS_VERTICES = 1;
        /// MLine is closed.
        const CLOSED = 2;
        /// Suppress start caps.
        const NO_START_CAPS = 4;
        /// Suppress end caps.
        const NO_END_CAPS = 8;
    }
}

bitflags! {
    /// Flags for MLineStyle.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct MLineStyleFlags: i16 {
        /// No flags.
        const NONE = 0;
        /// Fill between lines is on.
        const FILL_ON = 1;
        /// Display miters at joints (inner vertices).
        const DISPLAY_JOINTS = 2;
        /// Start square (line) cap.
        const START_SQUARE_CAP = 16;
        /// Start inner arcs cap.
        const START_INNER_ARCS_CAP = 32;
        /// Start round (outer arcs) cap.
        const START_ROUND_CAP = 64;
        /// End square (line) cap.
        const END_SQUARE_CAP = 256;
        /// End inner arcs cap.
        const END_INNER_ARCS_CAP = 512;
        /// End round (outer arcs) cap.
        const END_ROUND_CAP = 1024;
    }
}

// ============================================================================
// MLineStyle Element
// ============================================================================

/// An element (parallel line) in an MLineStyle.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineStyleElement {
    /// Offset from center line.
    /// Positive values are above/left, negative are below/right.
    pub offset: f64,
    /// Element color.
    pub color: Color,
    /// Line type handle.
    pub line_type_handle: Option<Handle>,
    /// Line type name.
    pub line_type_name: String,
}

impl MLineStyleElement {
    /// Creates a new element with the given offset.
    pub fn new(offset: f64) -> Self {
        Self {
            offset,
            color: Color::ByLayer,
            line_type_handle: None,
            line_type_name: "ByLayer".to_string(),
        }
    }

    /// Creates an element with offset and color.
    pub fn with_color(offset: f64, color: Color) -> Self {
        let mut elem = Self::new(offset);
        elem.color = color;
        elem
    }
}

impl Default for MLineStyleElement {
    fn default() -> Self {
        Self::new(0.0)
    }
}

// ============================================================================
// MLineStyle
// ============================================================================

/// Style definition for MLine entities.
///
/// Defines the appearance of multilines including:
/// - Number and offset of parallel lines (elements)
/// - Colors and line types for each element
/// - Cap styles (square, round, inner arcs)
/// - Fill color and visibility
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineStyle {
    /// Object handle.
    pub handle: Handle,
    /// Style name (up to 32 characters).
    pub name: String,
    /// Style description (up to 255 characters).
    pub description: String,
    /// Style flags.
    pub flags: MLineStyleFlags,
    /// Fill color (when fill is enabled).
    pub fill_color: Color,
    /// Start angle in radians (default: π/2 = 90°).
    pub start_angle: f64,
    /// End angle in radians (default: π/2 = 90°).
    pub end_angle: f64,
    /// Elements (parallel lines) in the style.
    pub elements: Vec<MLineStyleElement>,
}

impl MLineStyle {
    /// Creates a new MLineStyle with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            handle: Handle::NULL,
            name: name.to_string(),
            description: String::new(),
            flags: MLineStyleFlags::NONE,
            fill_color: Color::ByLayer,
            start_angle: PI / 2.0,
            end_angle: PI / 2.0,
            elements: Vec::new(),
        }
    }

    /// Creates the Standard MLineStyle.
    pub fn standard() -> Self {
        let mut style = Self::new("Standard");
        // Standard has two elements at +0.5 and -0.5 offset
        style.add_element(MLineStyleElement::new(0.5));
        style.add_element(MLineStyleElement::new(-0.5));
        style
    }

    /// Adds an element to the style.
    pub fn add_element(&mut self, element: MLineStyleElement) {
        self.elements.push(element);
    }

    /// Creates and adds an element with the given offset.
    pub fn add_element_with_offset(&mut self, offset: f64) -> &mut MLineStyleElement {
        self.elements.push(MLineStyleElement::new(offset));
        self.elements.last_mut().unwrap()
    }

    /// Returns the number of elements.
    pub fn element_count(&self) -> usize {
        self.elements.len()
    }

    /// Calculates the total width of the style.
    pub fn total_width(&self) -> f64 {
        if self.elements.is_empty() {
            return 0.0;
        }

        let min_offset = self
            .elements
            .iter()
            .map(|e| e.offset)
            .fold(f64::INFINITY, f64::min);
        let max_offset = self
            .elements
            .iter()
            .map(|e| e.offset)
            .fold(f64::NEG_INFINITY, f64::max);

        max_offset - min_offset
    }

    /// Sets fill on.
    pub fn set_fill_on(&mut self, fill_on: bool) {
        if fill_on {
            self.flags |= MLineStyleFlags::FILL_ON;
        } else {
            self.flags &= !MLineStyleFlags::FILL_ON;
        }
    }

    /// Returns true if fill is enabled.
    pub fn is_fill_on(&self) -> bool {
        self.flags.contains(MLineStyleFlags::FILL_ON)
    }

    /// Sets whether to display joints (miters).
    pub fn set_display_joints(&mut self, display: bool) {
        if display {
            self.flags |= MLineStyleFlags::DISPLAY_JOINTS;
        } else {
            self.flags &= !MLineStyleFlags::DISPLAY_JOINTS;
        }
    }

    /// Returns true if joints are displayed.
    pub fn displays_joints(&self) -> bool {
        self.flags.contains(MLineStyleFlags::DISPLAY_JOINTS)
    }

    /// Sorts elements by offset (ascending).
    pub fn sort_elements(&mut self) {
        self.elements
            .sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap());
    }
}

impl Default for MLineStyle {
    fn default() -> Self {
        Self::standard()
    }
}

// ============================================================================
// MLine Segment
// ============================================================================

/// Segment data for one element at a vertex.
#[derive(Debug, Clone, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineSegment {
    /// Segment parameters (distances along the mline element).
    pub parameters: Vec<f64>,
    /// Area fill parameters.
    pub area_fill_parameters: Vec<f64>,
}

impl MLineSegment {
    /// Creates a new empty segment.
    pub fn new() -> Self {
        Self {
            parameters: Vec::new(),
            area_fill_parameters: Vec::new(),
        }
    }

    /// Adds a segment parameter.
    pub fn add_parameter(&mut self, param: f64) {
        self.parameters.push(param);
    }

    /// Adds an area fill parameter.
    pub fn add_area_fill_parameter(&mut self, param: f64) {
        self.area_fill_parameters.push(param);
    }
}

// ============================================================================
// MLine Vertex
// ============================================================================

/// A vertex in an MLine entity.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineVertex {
    /// Vertex position in WCS.
    pub position: Vector3,
    /// Direction vector of segment starting at this vertex.
    pub direction: Vector3,
    /// Direction vector of miter at this vertex.
    pub miter: Vector3,
    /// Segment data for each element in the style.
    pub segments: Vec<MLineSegment>,
}

impl MLineVertex {
    /// Creates a new vertex at the given position.
    pub fn new(position: Vector3) -> Self {
        Self {
            position,
            direction: Vector3::new(1.0, 0.0, 0.0),
            miter: Vector3::new(0.0, 1.0, 0.0),
            segments: Vec::new(),
        }
    }

    /// Creates a vertex with position and direction.
    pub fn with_direction(position: Vector3, direction: Vector3) -> Self {
        // Calculate miter as perpendicular to direction
        let miter = Vector3::new(-direction.y, direction.x, 0.0).normalize();
        Self {
            position,
            direction: direction.normalize(),
            miter,
            segments: Vec::new(),
        }
    }

    /// Adds a segment for an element.
    pub fn add_segment(&mut self, segment: MLineSegment) {
        self.segments.push(segment);
    }

    /// Initializes segments for a given number of style elements.
    pub fn init_segments(&mut self, element_count: usize) {
        self.segments.clear();
        for _ in 0..element_count {
            self.segments.push(MLineSegment::new());
        }
    }
}

impl Default for MLineVertex {
    fn default() -> Self {
        Self::new(Vector3::ZERO)
    }
}

// ============================================================================
// MLine Entity
// ============================================================================

/// MLine (Multiline) entity.
///
/// A multiline is a complex of parallel lines that can have caps, joints,
/// and fill. The appearance is controlled by an MLineStyle.
///
/// # Example
///
/// ```ignore
/// use acadrust::entities::{MLine, MLineStyle};
/// use acadrust::types::Vector3;
///
/// // Create a style with 3 parallel lines
/// let mut style = MLineStyle::new("Triple");
/// style.add_element_with_offset(1.0);
/// style.add_element_with_offset(0.0);
/// style.add_element_with_offset(-1.0);
///
/// // Create an MLine with the style
/// let mut mline = MLine::new();
/// mline.add_vertex(Vector3::new(0.0, 0.0, 0.0));
/// mline.add_vertex(Vector3::new(10.0, 0.0, 0.0));
/// mline.add_vertex(Vector3::new(10.0, 10.0, 0.0));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLine {
    /// Common entity data.
    pub common: EntityCommon,
    /// MLine flags.
    pub flags: MLineFlags,
    /// Justification (top, zero, bottom).
    pub justification: MLineJustification,
    /// Extrusion direction (normal).
    pub normal: Vector3,
    /// Scale factor.
    pub scale_factor: f64,
    /// Start point in WCS.
    pub start_point: Vector3,
    /// Handle to the MLineStyle.
    pub style_handle: Option<Handle>,
    /// Style name.
    pub style_name: String,
    /// Number of style elements (for reading).
    pub style_element_count: usize,
    /// Vertices.
    pub vertices: Vec<MLineVertex>,
}

impl MLine {
    const GEOMETRY_EPSILON_SQUARED: f64 = 1.0e-20;

    /// Creates a new MLine with default settings.
    pub fn new() -> Self {
        Self {
            common: EntityCommon::default(),
            flags: MLineFlags::HAS_VERTICES,
            justification: MLineJustification::Zero,
            normal: Vector3::new(0.0, 0.0, 1.0),
            scale_factor: 1.0,
            start_point: Vector3::ZERO,
            style_handle: None,
            style_name: "Standard".to_string(),
            style_element_count: 2,
            vertices: Vec::new(),
        }
    }

    /// Creates an MLine from a list of points.
    pub fn from_points(points: &[Vector3]) -> Self {
        let mut mline = Self::new();
        for point in points {
            mline.add_vertex(*point);
        }
        mline
    }

    /// Creates a closed MLine from a list of points.
    pub fn closed_from_points(points: &[Vector3]) -> Self {
        let mut mline = Self::from_points(points);
        mline.close();
        mline
    }

    /// Adds a vertex at the given position.
    pub fn add_vertex(&mut self, position: Vector3) -> &mut MLineVertex {
        if self.vertices.last().is_some_and(|last| {
            (position - last.position).length_squared() <= Self::GEOMETRY_EPSILON_SQUARED
        }) {
            return self.vertices.last_mut().unwrap();
        }

        let mut vertex = MLineVertex::new(position);
        vertex.init_segments(self.style_element_count);
        self.vertices.push(vertex);
        self.rebuild_geometry();

        self.vertices.last_mut().unwrap()
    }

    /// Recalculates vertex directions and miter vectors.
    pub fn rebuild_geometry(&mut self) {
        let count = self.vertices.len();
        self.flags.set(MLineFlags::HAS_VERTICES, count > 0);
        if count == 0 {
            self.start_point = Vector3::ZERO;
            return;
        }

        self.start_point = self.vertices[0].position;
        let positions: Vec<Vector3> = self.vertices.iter().map(|vertex| vertex.position).collect();
        let closed = self.is_closed() && count >= 3;
        let normal = if self.normal.length_squared() > Self::GEOMETRY_EPSILON_SQUARED {
            self.normal.normalize()
        } else {
            Vector3::UNIT_Z
        };

        let direction_after = |index: usize| -> Option<Vector3> {
            let attempts = if closed {
                count - 1
            } else {
                count.saturating_sub(index + 1)
            };
            for step in 1..=attempts {
                let next = if closed {
                    (index + step) % count
                } else {
                    index + step
                };
                let delta = positions[next] - positions[index];
                if delta.length_squared() > Self::GEOMETRY_EPSILON_SQUARED {
                    return Some(delta.normalize());
                }
            }
            None
        };
        let direction_before = |index: usize| -> Option<Vector3> {
            let attempts = if closed { count - 1 } else { index };
            for step in 1..=attempts {
                let previous = if closed {
                    (index + count - (step % count)) % count
                } else {
                    index - step
                };
                let delta = positions[index] - positions[previous];
                if delta.length_squared() > Self::GEOMETRY_EPSILON_SQUARED {
                    return Some(delta.normalize());
                }
            }
            None
        };

        for index in 0..count {
            let before = direction_before(index);
            let after = direction_after(index);
            let direction = after.or(before).unwrap_or(Vector3::UNIT_X);
            let before_normal = before.map(|value| normal.cross(&value).normalize());
            let after_normal = after.map(|value| normal.cross(&value).normalize());
            let miter = match (before_normal, after_normal) {
                (Some(a), Some(b)) => {
                    let sum = a + b;
                    if sum.length_squared() > Self::GEOMETRY_EPSILON_SQUARED {
                        sum.normalize()
                    } else {
                        b
                    }
                }
                (Some(value), None) | (None, Some(value)) => value,
                (None, None) => Vector3::UNIT_Y,
            };
            self.vertices[index].direction = direction;
            self.vertices[index].miter = miter;
        }
    }

    /// Moves one vertex and rebuilds the dependent direction/miter data.
    pub fn set_vertex_position(&mut self, index: usize, position: Vector3) -> bool {
        let Some(vertex) = self.vertices.get_mut(index) else {
            return false;
        };
        vertex.position = position;
        self.rebuild_geometry();
        true
    }

    /// Inserts a vertex while keeping the element segment layout consistent.
    pub fn insert_vertex(&mut self, index: usize, position: Vector3) -> bool {
        if index > self.vertices.len() {
            return false;
        }
        let mut vertex = MLineVertex::new(position);
        vertex.init_segments(self.style_element_count);
        self.vertices.insert(index, vertex);
        self.rebuild_geometry();
        true
    }

    /// Removes a vertex. A drawable multiline always retains at least two.
    pub fn remove_vertex(&mut self, index: usize) -> bool {
        if self.vertices.len() <= 2 || index >= self.vertices.len() {
            return false;
        }
        self.vertices.remove(index);
        self.rebuild_geometry();
        true
    }

    /// Closes the MLine.
    ///
    /// Sets the CLOSED flag regardless of vertex count so the state is never
    /// silently dropped; `rebuild_geometry` guards its closed-segment math
    /// with its own vertex-count check.
    pub fn close(&mut self) {
        self.flags |= MLineFlags::CLOSED;
        self.rebuild_geometry();
    }

    /// Opens the MLine.
    pub fn open(&mut self) {
        self.flags &= !MLineFlags::CLOSED;
        self.rebuild_geometry();
    }

    /// Returns true if the MLine is closed.
    pub fn is_closed(&self) -> bool {
        self.flags.contains(MLineFlags::CLOSED)
    }

    /// Suppresses start caps.
    pub fn suppress_start_caps(&mut self) {
        self.flags |= MLineFlags::NO_START_CAPS;
    }

    /// Suppresses end caps.
    pub fn suppress_end_caps(&mut self) {
        self.flags |= MLineFlags::NO_END_CAPS;
    }

    /// Returns the number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }

    /// Returns an iterator over vertex positions.
    pub fn positions(&self) -> impl Iterator<Item = Vector3> + '_ {
        self.vertices.iter().map(|v| v.position)
    }

    /// Calculates the total length along the centerline.
    pub fn length(&self) -> f64 {
        if self.vertices.len() < 2 {
            return 0.0;
        }

        let mut total: f64 = self
            .vertices
            .windows(2)
            .map(|w| (w[1].position - w[0].position).length())
            .sum();

        // Add closing segment if closed
        if self.is_closed() && self.vertices.len() >= 2 {
            total += (self.vertices.first().unwrap().position
                - self.vertices.last().unwrap().position)
                .length();
        }

        total
    }

    /// Translates the MLine by the given offset.
    pub fn translate(&mut self, offset: Vector3) {
        self.start_point = self.start_point + offset;
        for vertex in &mut self.vertices {
            vertex.position = vertex.position + offset;
        }
    }

    /// Returns the bounding box of the MLine vertices.
    pub fn bounding_box(&self) -> Option<(Vector3, Vector3)> {
        if self.vertices.is_empty() {
            return None;
        }

        let first = self.vertices[0].position;
        let mut min = first;
        let mut max = first;

        for v in &self.vertices[1..] {
            min.x = min.x.min(v.position.x);
            min.y = min.y.min(v.position.y);
            min.z = min.z.min(v.position.z);
            max.x = max.x.max(v.position.x);
            max.y = max.y.max(v.position.y);
            max.z = max.z.max(v.position.z);
        }

        Some((min, max))
    }

    /// Sets the style element count (used when reading).
    pub fn set_style_element_count(&mut self, count: usize) {
        self.style_element_count = count;
    }

    /// Reverses the vertex order.
    pub fn reverse(&mut self) {
        self.vertices.reverse();
        self.rebuild_geometry();
    }
}

impl Default for MLine {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for MLine {
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
        if self.vertices.is_empty() {
            return BoundingBox3D::default();
        }

        let first = self.vertices[0].position;
        let mut min = first;
        let mut max = first;

        for v in &self.vertices[1..] {
            min.x = min.x.min(v.position.x);
            min.y = min.y.min(v.position.y);
            min.z = min.z.min(v.position.z);
            max.x = max.x.max(v.position.x);
            max.y = max.y.max(v.position.y);
            max.z = max.z.max(v.position.z);
        }

        BoundingBox3D::new(min, max)
    }

    fn translate(&mut self, offset: Vector3) {
        super::translate::translate_mline(self, offset);
    }

    fn entity_type(&self) -> &'static str {
        "MLINE"
    }

    fn apply_transform(&mut self, transform: &crate::types::Transform) {
        super::transform::transform_mline(self, transform);
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for MLine entities.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MLineBuilder {
    mline: MLine,
}

impl MLineBuilder {
    /// Creates a new builder.
    pub fn new() -> Self {
        Self {
            mline: MLine::new(),
        }
    }

    /// Adds a vertex.
    pub fn vertex(mut self, position: Vector3) -> Self {
        self.mline.add_vertex(position);
        self
    }

    /// Adds multiple vertices.
    pub fn vertices(mut self, positions: &[Vector3]) -> Self {
        for pos in positions {
            self.mline.add_vertex(*pos);
        }
        self
    }

    /// Sets the scale factor.
    pub fn scale(mut self, scale: f64) -> Self {
        self.mline.scale_factor = scale;
        self
    }

    /// Sets the justification.
    pub fn justification(mut self, justification: MLineJustification) -> Self {
        self.mline.justification = justification;
        self
    }

    /// Sets the style name.
    pub fn style_name(mut self, name: &str) -> Self {
        self.mline.style_name = name.to_string();
        self
    }

    /// Closes the MLine.
    pub fn closed(mut self) -> Self {
        self.mline.close();
        self
    }

    /// Suppresses start caps.
    pub fn no_start_caps(mut self) -> Self {
        self.mline.suppress_start_caps();
        self
    }

    /// Suppresses end caps.
    pub fn no_end_caps(mut self) -> Self {
        self.mline.suppress_end_caps();
        self
    }

    /// Builds the MLine.
    pub fn build(self) -> MLine {
        self.mline
    }
}

impl Default for MLineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mline_creation() {
        let mline = MLine::new();
        assert_eq!(mline.justification, MLineJustification::Zero);
        assert_eq!(mline.scale_factor, 1.0);
        assert_eq!(mline.vertex_count(), 0);
        assert!(!mline.is_closed());
    }

    #[test]
    fn test_mline_from_points() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
        ];
        let mline = MLine::from_points(&points);

        assert_eq!(mline.vertex_count(), 3);
        assert_eq!(mline.start_point, Vector3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn test_mline_closed() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
            Vector3::new(0.0, 10.0, 0.0),
        ];
        let mut mline = MLine::from_points(&points);
        assert!(!mline.is_closed());

        mline.close();
        assert!(mline.is_closed());

        mline.open();
        assert!(!mline.is_closed());
    }

    #[test]
    fn test_mline_length() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
        ];
        let mline = MLine::from_points(&points);
        assert!((mline.length() - 20.0).abs() < 1e-10);
    }

    #[test]
    fn test_mline_length_closed() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
            Vector3::new(0.0, 10.0, 0.0),
        ];
        let mline = MLine::closed_from_points(&points);
        // Perimeter of 10x10 square
        assert!((mline.length() - 40.0).abs() < 1e-10);
    }

    #[test]
    fn test_mline_translate() {
        let points = vec![Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0)];
        let mut mline = MLine::from_points(&points);

        mline.translate(Vector3::new(5.0, 5.0, 0.0));

        assert_eq!(mline.start_point, Vector3::new(5.0, 5.0, 0.0));
        assert_eq!(mline.vertices[0].position, Vector3::new(5.0, 5.0, 0.0));
        assert_eq!(mline.vertices[1].position, Vector3::new(15.0, 5.0, 0.0));
    }

    #[test]
    fn test_mline_bounding_box() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 5.0, 0.0),
            Vector3::new(5.0, 10.0, 0.0),
        ];
        let mline = MLine::from_points(&points);

        let bbox = mline.bounding_box().unwrap();
        assert_eq!(bbox.0, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(bbox.1, Vector3::new(10.0, 10.0, 0.0));
    }

    #[test]
    fn test_mline_justification() {
        assert_eq!(MLineJustification::from(0), MLineJustification::Top);
        assert_eq!(MLineJustification::from(1), MLineJustification::Zero);
        assert_eq!(MLineJustification::from(2), MLineJustification::Bottom);
    }

    #[test]
    fn test_mline_flags() {
        let mut mline = MLine::new();
        assert!(!mline.flags.contains(MLineFlags::CLOSED));

        mline.close();
        assert!(mline.flags.contains(MLineFlags::CLOSED));

        mline.suppress_start_caps();
        assert!(mline.flags.contains(MLineFlags::NO_START_CAPS));

        mline.suppress_end_caps();
        assert!(mline.flags.contains(MLineFlags::NO_END_CAPS));
    }

    #[test]
    fn test_mlinestyle_creation() {
        let style = MLineStyle::new("Custom");
        assert_eq!(style.name, "Custom");
        assert_eq!(style.element_count(), 0);
        assert!((style.start_angle - PI / 2.0).abs() < 1e-10);
        assert!((style.end_angle - PI / 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_mlinestyle_standard() {
        let style = MLineStyle::standard();
        assert_eq!(style.name, "Standard");
        assert_eq!(style.element_count(), 2);
        assert_eq!(style.elements[0].offset, 0.5);
        assert_eq!(style.elements[1].offset, -0.5);
    }

    #[test]
    fn test_mlinestyle_total_width() {
        let mut style = MLineStyle::new("Wide");
        style.add_element(MLineStyleElement::new(2.0));
        style.add_element(MLineStyleElement::new(0.0));
        style.add_element(MLineStyleElement::new(-2.0));

        assert!((style.total_width() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn test_mlinestyle_fill() {
        let mut style = MLineStyle::new("Filled");
        assert!(!style.is_fill_on());

        style.set_fill_on(true);
        assert!(style.is_fill_on());

        style.set_fill_on(false);
        assert!(!style.is_fill_on());
    }

    #[test]
    fn test_mlinestyle_joints() {
        let mut style = MLineStyle::new("Joints");
        assert!(!style.displays_joints());

        style.set_display_joints(true);
        assert!(style.displays_joints());
    }

    #[test]
    fn test_mlinestyle_sort_elements() {
        let mut style = MLineStyle::new("Unsorted");
        style.add_element(MLineStyleElement::new(1.0));
        style.add_element(MLineStyleElement::new(-1.0));
        style.add_element(MLineStyleElement::new(0.0));

        style.sort_elements();

        assert_eq!(style.elements[0].offset, -1.0);
        assert_eq!(style.elements[1].offset, 0.0);
        assert_eq!(style.elements[2].offset, 1.0);
    }

    #[test]
    fn test_mline_vertex() {
        let mut vertex = MLineVertex::new(Vector3::new(5.0, 5.0, 0.0));
        assert_eq!(vertex.position, Vector3::new(5.0, 5.0, 0.0));
        assert_eq!(vertex.segments.len(), 0);

        vertex.init_segments(3);
        assert_eq!(vertex.segments.len(), 3);
    }

    #[test]
    fn test_mline_segment() {
        let mut segment = MLineSegment::new();
        assert!(segment.parameters.is_empty());
        assert!(segment.area_fill_parameters.is_empty());

        segment.add_parameter(1.0);
        segment.add_parameter(2.0);
        segment.add_area_fill_parameter(0.5);

        assert_eq!(segment.parameters.len(), 2);
        assert_eq!(segment.area_fill_parameters.len(), 1);
    }

    #[test]
    fn test_mline_builder() {
        let mline = MLineBuilder::new()
            .vertex(Vector3::new(0.0, 0.0, 0.0))
            .vertex(Vector3::new(10.0, 0.0, 0.0))
            .vertex(Vector3::new(10.0, 10.0, 0.0))
            .scale(2.0)
            .justification(MLineJustification::Top)
            .style_name("Custom")
            .closed()
            .build();

        assert_eq!(mline.vertex_count(), 3);
        assert_eq!(mline.scale_factor, 2.0);
        assert_eq!(mline.justification, MLineJustification::Top);
        assert_eq!(mline.style_name, "Custom");
        assert!(mline.is_closed());
    }

    #[test]
    fn test_mline_reverse() {
        let points = vec![
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::new(10.0, 10.0, 0.0),
        ];
        let mut mline = MLine::from_points(&points);

        mline.reverse();

        assert_eq!(mline.start_point, Vector3::new(10.0, 10.0, 0.0));
        assert_eq!(mline.vertices[0].position, Vector3::new(10.0, 10.0, 0.0));
        assert_eq!(mline.vertices[1].position, Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(mline.vertices[2].position, Vector3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn test_mlinestyle_element() {
        let elem = MLineStyleElement::with_color(0.5, Color::Index(1));
        assert_eq!(elem.offset, 0.5);
        assert_eq!(elem.color, Color::Index(1));
    }
}
